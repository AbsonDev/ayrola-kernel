# ayrola-kernel

Kernel do **Ayrola Harness** — agente Rust-native com memória event-sourced,
subagentes sub-100ms, auto-melhoria nível 3 e decision layer ensemble.

**Status:** Semanas 1-8 do ROADMAP concluídas.
**Testes:** 239 (231 unit + 6 mcp e2e + 2 doc) — todos verdes.
**Clippy:** `cargo clippy -- -D warnings` limpo.
**Linhas:** 5.125 (source).

---

## O que existe

| Módulo | Linhas | O que faz |
|---|---|---|
| `event_store` | 327 | Log append-only NDJSON, cadeia causal SHA-256, `replay`, `verify_chain` |
| `decision` | 476 | `trait DecisionLayer` + `DecisionEngine` 3 tiers (cache → heurística → LLM) |
| `agent` | 235 | `AgentId`, `AgentState`, `spawn_subagent`, `spawn_parallel`, `AgentRegistry` (live tracking) |
| `memory` | 486 | `Snapshot` (SHA-256), `SnapshotManager`, query, compaction |
| `refine` | 234 | `Critic` (avalia vs GoldenSet), `Pruner` (dead-code), `Proposer`, `Environment` (patch + `cargo check`) |
| `shadow` | 352 | `GoldenSet`, `ShadowExecutor`, `ShadowReport`, rollback automático |
| `cert` | 297 | `CertifiedDecision` (SHA-256), `DecisionLog` — certificação de decisão |
| `bench` | 277 | `BenchTask` (10 tipos), `Scoreboard` (speedup vs baseline), `run_suite` — ayrola-bench v1 |
| `sandbox` | 220 | `SandboxConfig`, `SandboxExecutor` (real `std::process::Command` + allowlist), `CircuitBreaker` |
| `tools` | 248 | `ToolReader`, `GrepTool`, `ToolExecutor` (dispatch + JoinSet paralelo), `ToolRegistry` |
| `config` | 253 | `KernelConfig` (YAML), `ConfigLoader`, `ConfigError`, validação |
| `rlm` | 208 | `Decomposer` (heurística), `Planner` (spawn paralelo + `ExecutionReport` com timing) |
| `llm` | 220 | `Llm` (subprocess: claude/opencode), `LlmBackend`, `LlmResponse` |ude/opencode), `LlmBackend`, `LlmResponse` |

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

- **`Tier2LLM` é opt-in.** `DecisionEngine::new()` usa heurística local (determinística, sem I/O). LLM real via `DecisionEngine::with_llm()` + flag `--llm` no CLI. Não faz retry, streaming ou parse JSON estruturado.
- **`Tier1PreFilter` é palavra-chave, não ONNX.** O crate `ort` (2.0.0-rc.13) seria o sucessor para inferência local.
- **`ShadowExecutor` compara `input == expected`.** Não executa código real; versão de produção precisa de Railway VM com Linux namespaces.
- **`SandboxExecutor` usa `std::process::Command` + allowlist.** Não usa Linux namespaces (macOS não suporta). Versão real exige VM Linux.

O que **é** real:

- `refine::Critic` avalia candidatos contra `GoldenSet`
- `refine::Pruner` detecta código morto (heurística)
- `refine::Environment` aplica patch + `cargo check`
- `tools::ToolExecutor` despacha Read/List/Grep com JoinSet paralelo
- `tools::GrepTool` com `grep_file` / `grep_dir`
- `sandbox::SandboxExecutor` bloqueia rede exceto `allow_network=true`
- `agent::AgentRegistry` track de `JoinHandle` em `BTreeMap`
- `llm::Llm::query()` invoca `claude -p` ou `opencode` via subprocess
- `bench::run_suite()` executa 10 tasks + baseline + speedup factor
- `rlm::Planner::execute()` retorna `ExecutionReport` com timing por subtask
- 9 ADRs em `adr/*.md` (não mais só em `DECISOES.md`)

Nada acima foi medido contra um número inventado. Tudo que tem número vem de `cargo test`, `cargo clippy -- -D warnings`, ou `cargo run --release`.

---

## Repositório relacionada

- `AbsonDev/Ayrola-Wisdom` — pesquisa, decisões, roadmap, workflow, veredito
- `AbsonDev/ayrola-kernel` @ `main` — fork de `pi_agent_rust` (referência MCP,
  **não** é a base)
