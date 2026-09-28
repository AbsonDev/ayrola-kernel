# ADR-001: Tokio como runtime assíncrono

**Status:** Aceito
**Data:** 2026-10-01

## Contexto

O Ayrola Kernel precisa de um runtime assíncrono para:
- Spawn de subagentes concorrentes (`JoinSet`)
- I/O não-bloqueante (event store, rede, MCP)
- Integração com crates async do ecossistema Rust

Candidatos: Tokio, asupersync, smol.

## Decisão

**Tokio 1.x** como runtime padrão.

Motivos:
- Ecossistema maduro: maioria dos crates async usam Tokio por padrão
- `JoinSet` nativo para fan-out de subagentes
- `tokio::task::spawn` com suporte a `LocalSet` (para spawn não-Send)
- Documentação extensa, comunidade ativa
- Zero Python/FFI — puro Rust

Rejeitado: `asupersync` (runtime customizado do pi_agent_rust). Complexo, pouco documentado, sem vantagem mensurável para nosso caso de uso.

## Consequências

- Todos os módulos usam `#[tokio::test]` ou `#[tokio::main]`
- `Cargo.toml`: `tokio = { version = "1.53.1", features = ["full"] }`
- Spawn de subagentes via `JoinSet` (tolerância a falhas)
- `agent.rs`: `spawn_subagent` e `spawn_parallel` assíncronos
