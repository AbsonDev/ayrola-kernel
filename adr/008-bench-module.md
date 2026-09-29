# ADR-008: ayrola-bench v0 como módulo nativo

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

Precisamos medir:
- Resolve rate (quantas tasks o kernel resolve)
- Latência (p50, p95, p99)
- Custo (USD por decisão)
- Qualidade (score heurístico)

Benchmarks externos (OpenCode) são o baseline, mas não temos integração ainda.

## Decisão

**`bench` módulo nativo** com `BenchTask`, `BenchResult`, `Scoreboard`.

Estrutura:
- `BenchTask`: id, nome, tipo, prompt, golden_answer, timeout
- `BenchResult`: task_id, success, duration_ms, cost_usd, quality_score
- `Scoreboard`: resolve_rate, avg_latency, total_cost, avg_quality, `save_json()`

10 tasks padrão (`default_suite()`): CodeSearch, CodeWrite, BugFix, PrReview, Debug, DocWrite, Architecture, Refactoring, TestWrite, MultiAgent.

Medições reais (release build):
- Spawn single: p50 0.010ms
- Spawn 10-parallel: p50 0.127ms

## Consequências

- `bench/mod.rs`: 585 linhas, 19 testes
- Scoreboard serializa para JSON (`serde_json::to_string_pretty`)
- Baseline OpenCode: ainda não medido (gate substituído por latência de spawn)
- Fase 1: integrar com OpenCode via MCP para comparação real
