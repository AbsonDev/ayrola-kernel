# ADR-010: 9Router como LLM Backend Free Tier

**Data:** 2026-10-03  
**Status:** ✅ Aceito  
**Autores:** Ayrola agent (autônomo)

---

## Contexto

Ayrola Kernel precisa de um Tier 2 LLM para o decision ensemble (ADR-003).

Opções avaliadas:
1. **ONNX local via crate `ort`** — sem crate Rust oficial, distribuição Python-only
2. **API paga via subprocess `claude -p`** — funciona, mas custo
3. **9Router local (http://localhost:20128)** — modelos free via OpenRouter/Kilocode/Cloudflare Workers AI

## Decisão

**Usar 9Router local como Tier 2 LLM backend (free tier).**

```rust
pub enum LlmBackend {
    Claude,        // subprocess: claude -p
    OpenCode,      // subprocess: opencode -p
    NineRouter,    // HTTP: curl → localhost:20128/v1/chat/completions
    Stub,          // para testes
}
```

## Consequências

✅ **Positivo**
- $0 custo (free tier)
- Sem crate HTTP externo — usa `curl` + `sqlite3` CLI (já no PATH)
- API key lida do DB SQLite do 9Router (`~/.9router/db/data.sqlite`)
- `Llm::default()` auto-detecta 9Router se `claude` e `opencode` não existirem

⚠️ **Trade-off**
- Dependência do daemon local (`localhost:20128`)
- Modelos free podem ter rate-limit (429) ou upstream 422
- `curl` + SQLite CLI são dependencies de sistema (macOS/Linux)

## Implementação

- `src/llm.rs`: `LlmBackend::NineRouter`, `query_9router()`, `read_9router_key()`
- Key retrieval: `sqlite3 ~/.9router/db/data.sqlite "SELECT key FROM apiKeys WHERE isActive=1 LIMIT 1;"`
- Request: `POST /v1/chat/completions` com `Authorization: Bearer <key>`
- Response parsing: JSON + trailing `data: [DONE]`

## Alternativas rejeitadas

| Alternativa | Razão |
|---|---|
| `ort` crate (ONNX) | Sem crate Rust oficial; Python-only |
| API OpenAI/Anthropic direta | Custo; precisa API key externa |
| Servidor LLM próprio (llama.cpp) | Overhead; 9Router já resolve |

## Modelos testados

| Modelo | Provider | Status |
|---|---|---|
| `kc/openrouter/free` | Kilocode/OpenRouter | ✅ Funciona |
| `bzl/auto:free` | Bzl | ✅ Funciona |
| `cf/@cf/meta/llama-3.2-1b-instruct` | Cloudflare Workers AI | ✅ Funciona |
| `ocz/jev-1.13-free` | OpenCode Zen | ✅ Via /v1/systemone (Jev) |

## Reversão

Se 9Router não estiver disponível, `Llm::default()` cai para `Stub`.  
Nenhuma breaking change em código existente (novo enum variant, opt-in via `DecisionEngine::with_llm()`).

---

*Documentação da decisão de arquitetura 010.*
