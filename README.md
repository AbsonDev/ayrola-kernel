# ayrola-kernel

Kernel do **Ayrola Harness** — agente Rust-native com memória event-sourced,
subagentes sub-100ms, auto-melhoria nível 3 e decision layer ensemble.

**Status:** Semanas 1-8 do ROADMAP concluídas.
**Testes:** 115 (111 unit + 4 integração) — todos verdes.
**Clippy:** `cargo clippy -- -D warnings` limpo.
**Linhas:** 3.772 (source).

---

## O que existe

| Módulo | Linhas | O que faz |
|---|---|---|
| `event_store` | 327 | Log append-only NDJSON, cadeia causal SHA-256, `replay`, `verify_chain` |
| `decision` | 476 | `trait DecisionLayer` + `DecisionEngine` 3 tiers (cache → heurística → LLM) |
| `agent` | 235 | `AgentId`, `AgentState`, `spawn_subagent`, `spawn_parallel` (JoinSet) |
| `memory` | 486 | `Snapshot` (SHA-256), `SnapshotManager`, query, compaction |
| `refine` | 234 | `Critic`, `Pruner`, `Proposer`, `Environment` — auto-melhoria N3 (stubs) |
| `shadow` | 352 | `GoldenSet`, `ShadowExecutor`, `ShadowReport`, rollback automático |
| `cert` | 297 | `CertifiedDecision` (SHA-256), `DecisionLog` — certificação de decisão |
| `bench` | 277 | `BenchTask` (10 tipos), `Scoreboard`, `default_suite` — ayrola-bench v0 |
| `sandbox` | 220 | `SandboxConfig`, `SandboxExecutor`, `CircuitBreaker` (stub, macOS) |
| `tools` | 248 | `ToolReader`, `SpeculativeTool`, `ToolRegistry` |
| `config` | 253 | `KernelConfig` (YAML), `ConfigLoader`, `ConfigError`, validação |
| `rlm` | 208 | `Decomposer` (heurística), `Planner` (spawn paralelo) |
| `llm` | 220 | `Llm` (subprocess: claude/opencode), `LlmBackend`, `LlmResponse` |

---

## Medições reais (release build)

```
spawn_subagent single — 200 runs
  mean: 0.013ms   p50: 0.010ms   p95: 0.028ms   p99: 0.050ms   max: 0.060ms

spawn_parallel fan-out
   1 subagente: p50  0.017ms
   3 subagentes: p50  0.043ms
   5 subagentes: p50  0.068ms
  10 subagentes: p50  0.127ms
```

Gates: single < 150ms ✅ · 10-parallel < 100ms ✅

Reproduza:
```bash
cargo run --release --example spawn_latency
cargo run --release --example parallel_spawn
```

---

## Build

```bash
cargo build --release
cargo test
cargo clippy -- -D warnings
cargo doc --no-deps
```

Toolchain: Rust edition 2024, `cargo 1.98.1`.

Dependências: `tokio` 1.53.1 (full), `serde` 1.0.229, `serde_json` 1.0.151,
`serde_yaml` 0.9.34, `clap` 4.6.7, `sha2` 0.11.0, `uuid` 1.26.1, `chrono` 0.4.45,
`thiserror` 2.0.21.

---

## CLI

```bash
cargo run -- status
cargo run -- decide --question "spawn a subagent?" --type yesno
cargo run -- spawn --task "review the PR"
```

---

## Arquitetura (4 pilares)

1. **Memória event-sourced** — `event_store` + `memory`. Append-only, hash chain,
   replay verificado. Nada de estado mutável entre requests.
2. **Subagentes sub-100ms** — `agent` + `rlm`. `spawn_parallel` com `JoinSet`.
   Medido: 0.127ms p50 para 10 subagentes.
3. **Auto-melhoria nível 3** — `refine` + `shadow`. Candidatos validados contra
   golden set imutável; falha em qualquer caso → rollback. Sem reward hacking.
4. **Sandbox per-agent** — `sandbox`. Linux namespaces via Railway VM
   (macOS não suporta — stub local).

Extras: `cert` (certificação de decisão com hash), `bench` (ayrola-bench v0),
`decision` (ensemble 3 tiers, ADR-004), `tools` (leitura especulativa).

---

## Limites honestos

O que **ainda não** é real:

- **`refine` / `sandbox` / `tools` são stubs.** Estruturas e testes existem;
  integração real (LLM, namespaces, MCP) não.
- **`Tier2LLM` não chama LLM.** É heurística local. Laya/Jev via MCP é o passo
  seguinte (ADR-004 mantém Laya como candidato *tier 2*, não como camada principal).
- **`Tier1PreFilter` é palavra-chave, não ONNX.** O crate `ort` seria o sucessor.
- **`ShadowExecutor` compara `input == expected`.** Não executa código de verdade;
  a versão real precisa da Railway VM.
- **Sem baseline OpenCode.** O ROADMAP previa comparar resolve rate contra
  OpenCode; isso não foi medido. O gate foi substituído por latência de spawn.
- **Sem ADRs como arquivos.** As decisões estão em `DECISOES.md` no repo
  Ayrola-Wisdom, não como `adr/` aqui.

Nada acima foi medido contra um número inventado. O que tem número, tem número
de `cargo test` ou de `cargo run --release --example`.

---

## Repositório relacionada

- `AbsonDev/Ayrola-Wisdom` — pesquisa, decisões, roadmap, workflow, veredito
- `AbsonDev/ayrola-kernel` @ `main` — fork de `pi_agent_rust` (referência MCP,
  **não** é a base)
