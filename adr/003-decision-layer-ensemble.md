# ADR-003: Decision Layer — Ensemble 3 tiers

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

O kernel precisa tomar decisões (spawn? qual modelo? qual ferramenta?).
Abordagens:

1. **Laya solo** — modelo único, ~33ms, Python-only
2. **Jev puro** — ~$0/call, local, closed weights
3. **Ensemble 3 tiers** — cache + pre-filter + LLM

## Decisão

**Ensemble 3 tiers** com fallback.

- **Tier 0 (cache):** hash normalizado da pergunta → resposta cacheada. 0ms, 0 custo.
- **Tier 1 (pre-filter):** heurística word-boundary matching. ~5ms, só descarta.
- **Tier 2 (LLM):** modelo completo via MCP. ~33ms, usado como último recurso.

Laya é candidata para Tier 2 (quando integrado via MCP), não camada principal.

## Consequências

- `decision.rs`: 1285 linhas, 45 testes
- `DecisionEngine::ask()`: cache-first, fallback progressivo
- `Tier1PreFilter`: word-boundary matching (evita falsos como "no" em "unknown")
- `Tier2LLM`: stub simulado; integração MCP é próxima etapa
- Cache hit rate: medido via `engine.cache_hit_rate()`
