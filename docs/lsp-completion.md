# Semantic completion and auto-imports

The native `lsp` tool's `completion` action asks the selected language server
for source-code completions. It can resolve a selected suggestion's details
and auto-import edits, preview the exact edits, and apply them together.
The language server provides semantic candidates; this is not text generation
or a local language parser.

## List, inspect, apply

List at an exact cursor. Both fields in `position` are zero-based; `character`
counts UTF-16 code units, not UTF-8 bytes. The cursor may be at end-of-line.
`position` is deliberately separate from the tool's older one-based `line`
and symbol-substring selectors, which completion does not accept.

```json
{"action":"completion","file":"src/main.rs","position":{"line":8,"character":17},"query":"Hash","limit":50,"timeout":15}
```

`query` is an optional case-sensitive prefix against each item's `filterText`,
or its label when no filter text exists. Items are ordered by server
`sortText`, falling back to label, with ties retaining server order. This is
not editor-specific fuzzy matching. Omit `query` to inspect other candidates.

The response contains `items` with opaque `completionId` values. `isIncomplete`
is the server's own flag; `truncated` separately indicates local limits. A
truncated response is not a complete candidate set. Narrow the prefix or make
a fresh request after changing the source; there is no completion-page cursor.
A new listing retires the old listing's handles.

Select one of the returned IDs without `apply` to resolve and inspect it:

```json
{"action":"completion","completionId":"<returned ID>"}
```

A supported item returns `applied:false`, `canApply:true`, bounded details and
documentation, and its full `edits` array. Preview does not write source files.
If the server supports `completionItem/resolve`, only the selected item is
resolved. Opaque data and negotiated list defaults survive that round trip.
A preview's resolved item is reused for application instead of resolving twice.

Explicitly accept the selection:

```json
{"action":"completion","completionId":"<returned ID>","apply":true}
```

Do not combine `completionId` with `file`, `position`, `range`, `query`, or
`limit`. The handle already identifies those choices. A label or numeric index
is not an application authority. New-listing `apply:true` is rejected.

## Exact insertion semantics

Server `textEdit` ranges take precedence. For an `InsertReplaceEdit`, Pi uses
the **replace** span; preview exposes that choice as `editMode:"replace"`.
Both spans must contain the requested cursor, be on one line, and the insert
span must be a prefix of the replace span. Inserted text may span several lines.

When a server supplies only `insertText` or a label, Pi does not guess the
language's word boundaries. Supply a replacement `range` on the listing:

```json
{"action":"completion","file":"src/main.rs","position":{"line":8,"character":17},"range":{"start":{"line":8,"character":13},"end":{"line":8,"character":17}}}
```

That single-line range must contain the cursor. A zero-width range explicitly
requests insertion. It is used only when no server edit exists. Without an
explicit fallback, such candidates remain inspectable but cannot be applied.
CompletionList `itemDefaults.editRange` and `textEditText` are supported, along
with default `data`, `insertTextFormat`, and `insertTextMode`. Unadvertised
`applyKind` merging is not supported.

`additionalTextEdits`, including imports computed lazily during resolution,
use the original source's coordinates. They are applied with the primary edit
in one existing staged, rollback-safe file transaction. Overlaps, shared
insertion points, malformed ranges, unsupported annotations and resource-shaped
edits are rejected before writing. No edit can select a different destination
URI: this surface edits only its caller-selected source document.

## Freshness and permission

Handles retain the original source text, synchronized document incarnation and
exact server connection. Selection checks them before and after resolution;
application checks again after staging, immediately before commit. Changed
source text, closed/reopened documents, dead/replaced servers, reload, expired
handles, and a subsequent completion listing require a fresh request.
Unchanged bytes alone cannot resurrect a closed document incarnation.

The current caller must retain filesystem I/O authority and must not be
cancelled. Request deadlines cover server initialization, completion and
selected-item resolution; staging checks the remaining budget before commit.
Synchronous filesystem operations cannot be preempted while blocked.

No completion operation grants `workspace/applyEdit` callback permission or
executes `workspace/executeCommand`. Indentation-adjusting insertion and
command-backed items are visible but **cannot be applied**. They are not
silently flattened, partially inserted, or executed. The existing explicit
code-action command workflow remains separate.

A successful application consumes all completion handles and invalidates the
server's document caches for subsequent synchronization. An application failure
also retires handles, including when the shared transaction reports incomplete
rollback. The transaction's recovery error remains visible. This is not a
crash-recovery mechanism or a filesystem sandbox against hostile concurrent
path replacement.

## Parameterized snippet completions

Pi advertises snippet completion support and implements a bounded numeric
subset of the [LSP snippet syntax](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#snippet_syntax).
Tabstops (`$1`, `${1}`), defaults (`${1:argument}`), choices
(`${1|red,green|}`), nested defaults and repeated placeholders are supported.
Ordinary text, Unicode, multiline insertions and context-appropriate escapes
are preserved. This is a one-shot expansion, not an interactive tabstop editor
or a complete TextMate interpreter.

The listing identifies snippet candidates with `snippet:true`. Select a
`completionId` to inspect the expanded edits and `snippetPlaceholders`. Omitted
values use the supplied default or first choice. A positive bare tabstop with
no default appears in `missingPlaceholders` and makes `canApply:false`; it is
not silently erased during application. Bare `$0` denotes the final cursor
marker and contributes no text. A default or explicit value for index zero
contributes that text, but Pi does not move a UI cursor after applying.

Supply `snippetValues` to bind numeric placeholders to literal source text:

```json
{"action":"completion","completionId":"<returned ID>","snippetValues":{"1":"request","2":"options"}}
```

For example, `call(${1:argument}, $1)$0` with `{"1":"request"}` previews
`call(request, request)`. Nested placeholders resolve before their parent's
default is inserted. Replacing an outer placeholder suppresses its nested
fields unless those fields are also referenced elsewhere. Supplying a value
for a suppressed or unknown index is an error rather than ignored input.
Forward references use their later definition; conflicting definitions and
cycles encountered during expansion are rejected.

Repeat the same substitutions with explicit application:

```json
{"action":"completion","completionId":"<returned ID>","snippetValues":{"1":"request","2":"options"},"apply":true}
```

Preview substitutions are **not cached**. Omitting them on another request
returns to the server defaults and required-field rules; the cached server
item remains unchanged. An explicit empty string intentionally fills a bare
tabstop with no text. Choice values may be edited freely, not only selected
from the offered list. Substitution keys are canonical decimal indices from
0 through 65535 and are only accepted on a selected snippet completion.

Only the primary insertion is interpreted as snippet text. Auto-import
`additionalTextEdits` remain plain text even when they contain dollar signs or
placeholder-looking expressions, and still commit with the primary edit in
one checked transaction. Caller substitutions are never parsed again: shell
syntax, dollar signs and backslashes in a value are inserted literally.

### Numeric-placeholder transforms

A transformed occurrence can derive text from another numeric placeholder:

```text
${1:my_name} ${1/(.*)/${1:/pascalcase}/} $1
```

This expands to `my_name MyName my_name`. Supplying `{"1":"other_name"}`
changes it to `other_name OtherName other_name`. The transform belongs only
to its occurrence: it does not alter the source field, another mirror, the
cached server item or the original source file during preview. Forward
references and nested defaults use the same dependency and cycle checks.
A positive transformed field without a default still requires an explicit
`snippetValues` entry before application, even if the replacement is constant.

The supported form is `${index/pattern/replacement/options}`. A replacement
can contain `$0` for the full match, `$1` or `${1}` for a capture, and the five
case modifiers `upcase`, `downcase`, `capitalize`, `camelcase` and `pascalcase`.
Conditional formats are `${1:+yes}`, `${1:-no}`, `${1:default}` and
`${1:?yes:no}`. Empty captures are false. Conditional arms are literal text,
not recursively evaluated snippets; nested `${...}` formats in an arm are
rejected. Escape a literal dollar with `\$`, closing brace with `\}`, slash
with `\/` or backslash with `\\`; `\:` escapes a conditional separator.

Replacement affects the first match unless `g` is supplied. `i` enables
ASCII case-insensitive matching; only `g` and `i` flags are supported. Global
empty matches advance by one ASCII character, including a possible final
empty match. An unmatched pattern leaves the original value unchanged unless
a format contains a nonempty else arm, in which case that replacement is
rendered with empty captures. Captured or generated text is never reparsed as
snippet syntax and never executed.

This is deliberately **not a full JavaScript regular-expression engine**.
Both the regex pattern and transformed source value must be ASCII and contain
no CR or LF. Ordinary untransformed snippet fields still support Unicode and
multiline text. Supported patterns use the shared subset of literals,
character classes, anchors, alternatives, captures and quantifiers. Lookaround,
backreferences, Unicode escapes/properties, inline flags, named groups,
repeated groups using `*`, `+` or `{...}`, and Rust-only character-set
operators are rejected. These restrictions avoid silently changing the
server's UTF-16, Unicode, newline or repeated-capture semantics. Unsupported
syntax fails during parsing even inside an overridden outer default; an
unsupported supplied value fails before any insertion or auto-import.

Variables (including environment-, filename- and clipboard-derived values)
remain unsupported. Pi does not read ambient data to expand a completion.
Commands and indentation-adjusting items remain separate rejected cases.

## Limits and validation

One tool instance retains only its newest listing. Source text and incoming
responses are bounded at 2 MiB each; normalized cached item bytes are also
bounded at 2 MiB. Individual items are at most 64 KiB. At most 4096 wire items
are admitted, at most 128 handles returned (default 50), and at most 128
additional edits allowed per item. Structured output is bounded at 200 KiB;
handles expire after five minutes. Large documentation strings are visibly
clipped in the inspection output.

Snippet input and expanded insertion text are bounded at 64 KiB, with at most
64 distinct placeholder indices, 32 options per choice, 2048 syntax nodes and
32 nesting/expansion levels. Substitutions are at most 16 KiB each and 64 KiB
combined. Expansion has a separate work budget and a 2 MiB memoized-text cap.
Transforms additionally admit at most 32 occurrences, 1024 pattern bytes,
64 capture groups, 128 replacement parts, and 64 KiB compiled-regex and DFA
limits per pattern. Matching shares a conservative 16 MiB pattern-times-suffix
work allowance across the entire expansion; this can reject large global
replacements before the output limit. These are computational bounds, not a
hard real-time execution guarantee.
All limits fail before applying an edit; response-size limits still apply to
preview edits and placeholder metadata.

Regression tests are in `src/lsp/completion/tests.rs`, `completion/item/tests.rs`,
`completion/snippet/tests.rs`, `completion/snippet/transform/tests.rs` and
`completion/tests/protocol.rs`. Protocol tests drive the real tool over framed
child stdio and real temporary source files. `PI_LSP_REQUIRE_PROTOCOL` makes a
missing Python peer an error rather than a skip. Run the repository's DSR
quality entry point; standalone Python-peer checks are not Rust test evidence.
