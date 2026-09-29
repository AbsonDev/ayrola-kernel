# ADR-007: Configuração via YAML com validação

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

Kernel precisa de configuração:
- Limites (max_subagents, timeout, memória)
- Flags (tiers habilitados, logging)
- Env vars (API keys, endpoints)

Opções: TOML, YAML, JSON, env vars.

## Decisão

**YAML** para config humana + env vars para secrets.

Motivos:
- `KernelConfig` serializa/deserializa com `serde_yaml`
- Validação estrutural no load (`config.validate()`)
- Defaults seguros em código
- `config/default.yaml`: exemplo versionado
- Secrets via env vars (nunca em arquivo)

Estrutura:
```yaml
kernel:
  name: ayrola-kernel
  edition: "2024"

decision:
  t0_cache_enabled: true
  t1_prefilter_enabled: true
  t2_confidence_threshold: 0.75
  ensemble_mode: three_tier

agent:
  max_subagents: 10
  default_timeout_ms: 30000
  retry_count: 2

logging:
  level: info
  format: json
  output: stderr
```

## Consequências

- `config/mod.rs`: 175 linhas, 6 testes
- `ConfigLoader::load()`: parse + validate em um passo
- `ConfigError`: erros tipados (NotFound, YamlParse, MissingField, InvalidValue)
- `thiserror` para derivar `Error`
