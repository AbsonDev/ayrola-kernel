# ADR-002: Event Store append-only com causal chain

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

Memória do kernel precisa ser:
1. Auditável — cada mudança de estado tem rastro
2. Replayable — reconstruir estado a partir de eventos
3. Imutável — nenhum evento pode ser alterado ou deletado

## Decisão

**Event store append-only** com SHA-256 causal chain.

Cada evento:
- `seq` (u64, auto-incremento)
- `ts` (ISO-8601 timestamp)
- `kind` (string: "decision", "spawn", "snapshot", etc)
- `payload` (JSON arbitrário)
- `prev_hash` (SHA-256 do evento anterior)
- `hash` (SHA-256 do próprio evento)

Verificação: `verify_chain()` percorre todos os eventos e confirma que `hash == SHA256(prev_hash + content)`.

## Consequências

- `event_store.rs`: 328 linhas, 5 testes
- Formato: NDJSON (um JSON por linha) em arquivo
- `replay()`: reconstrói `ReplayState` a partir do log
- `snapshot`: ponto de verificação do estado (Pilar 1)
- Nenhum `DELETE` ou `UPDATE` — apenas `APPEND`
