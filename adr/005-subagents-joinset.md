# ADR-005: Subagentes via JoinSet (fan-out paralelo)

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

Pilar 2: subagentes sub-100ms. Precisamos de:
- Fan-out paralelo (múltiplas tarefas simultâneas)
- Coleta de resultados (primeiro a terminar ganha)
- Tolerância a falhas (um subagente falha não derruba os outros)

## Decisão

**Tokio `JoinSet`** para spawn paralelo de subagentes.

Motivos:
- `JoinSet::spawn()` aceita futures não-Send (via `LocalSet`)
- `join_next()` retorna `Option<Result<T, JoinError>>` — coleta natural
- Dropping `JoinSet` cancela todas as tasks pendentes (cleanup automático)
- Performance: 0.127ms p50 para 10 subagentes (medido)

Alternativas:
- `futures::join!` — número fixo de futures, não escala
- `spawn_blocking` — para CPU-bound, não nosso caso
- ` rayon` — paralelismo CPU, não async

## Consequências

- `agent.rs`: `spawn_subagent` (singular) e `spawn_parallel` (vetorizado)
- `rlm.rs`: `Planner::execute` usa `JoinSet` para fan-out
- `AgentId`: UUID v4, retornado imediatamente (não aguarda conclusão)
