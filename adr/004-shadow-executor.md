# ADR-004: Shadow Executor + Golden Set

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

Auto-melhoria Nível 3 exige que o harness possa modificar seu próprio código.
Risco: candidato malicioso ou buggy que degrada o sistema.

Opções:
1. Testes unitários — não pegam regressões semânticas
2. Canary deploy — arriscado em produção
3. **Shadow executor** — valida em paralelo antes de promover

## Decisão

**Shadow executor com golden set imutável.**

Fluxo:
1. Candidato entra como diff + descrição
2. Shadow executa golden set contra candidato e sistema atual (parallel)
3. Compara resultados
4. Todos passam → promove
5. Qualquer falha → rollback + registra no `ShadowCircuitBreaker`

Golden set: casos de teste imutáveis criados pelo usuário ou gerados por ADR.
Circuit breaker: 3 falhas consecutivas → bloqueia promoções.

## Consequências

- `shadow/mod.rs`: 737 linhas, 16 testes
- `GoldenCase`: input + expected_output + tolerance
- `ShadowReport`: total/passed/failed/promoted
- `execute_or_rollback()`: rollback automático em falha
- Phase 0: comparação stub (input == expected)
- Phase 1: Railway VM execução real
