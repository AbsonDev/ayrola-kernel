# ADR-006: Certificação de Decisão com SHA-256

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

Cada decisão do kernel deve ser:
1. **Auditável** — quem decidiu, quando, por quê
2. **Verificável** — detectar adulteração posterior
3. **Replayable** — reconstruir o raciocínio

## Decisão

**CertifiedDecision** com hash SHA-256.

Estrutura:
```rust
pub struct CertifiedDecision {
    pub decision_id: DecisionId,      // UUID
    pub timestamp: String,            // ISO-8601
    pub inputs: Value,                // pergunta/contexto
    pub decision: Value,              // resposta
    pub evidence: Evidence,           // tier, fonte, confiança
    pub cost_usd: f64,
    pub tier: DecisionTier,           // Tier0/Tier1/Tier2
    pub replayable: bool,
    pub hash: String,                 // SHA-256
}
```

Verificação: `cert.verify()` recalcula o hash e compara com o hash armazenado.
Se alguém adulterar o `decision` posteriormente, `verify()` retorna `false`.

## Consequências

- `cert/mod.rs`: 179 linhas, 16 testes
- `DecisionLog`: registry de decisões, `verify_all()`, `by_tier()`
- Integração futura: `DecisionEngine::ask()` retorna `CertifiedDecision` ao invés de `Answer`
- Hash usa representação canônica JSON (chaves ordenadas)
