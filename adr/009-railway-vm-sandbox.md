# ADR-009: Railway VM como Sandbox Isolado por Agente

**Data:** 2026-09-29
**Status:** Aceito
**Contexto:** Pilar 4 do manifesto — "Sandbox-per-agent Linux namespaces"
**Decisão:** Usar Railway Free VMs (`ssh railway.new`) como sandbox remoto via SSH.

## Problema
macOS não suporta Linux namespaces nativamente. Precisávamos de isolamento real
para executar código de subagentes com segurança (sem risco de vazar dados do host).

## Opções Avaliadas

| Opção | Prós | Contras | Decisão |
|---|---|---|---|
| Docker Desktop no macOS | Isolamento real | Peso, licença, overhead | ❌ Rejeitado |
| Lima/QEMU | Isolamento leve | Setup complexo, lento | ❌ Rejeitado |
| `process::Command` local | Simples | Zero isolamento | ❌ Rejeitado |
| Railway Free VM via SSH | Isolamento real, Linux puro, grátis | Requer rede, build window expira | ✅ Aceito |

## Consequências

1. **Isolamento:** Cada subagente pode executar código em VM Linux separada via SSH.
2. **Segurança:** VM descartável — se o código for malicioso, a VM é perdida, não o host.
3. **Latência:** P50 ~2.7s para `echo hello` (inclui handshake SSH).
4. **Custo:** $0 — Railway Free tier (trial_minutes=60, idle_timeout=5min).
5. **Limitação:** Build window expira após algumas horas — precisa reprovisionar.
6. **Fallback:** `CodeShadowRunner::local()` usa `SandboxExecutor` local sem isolamento.

## Implementação

```rust
pub struct RemoteSandboxExecutor {
    ssh_opts: String,
    host: String,
}

impl RemoteSandboxExecutor {
    pub fn run(&self, command: &str) -> SandboxResult {
        // SSH para railway.new, executa comando, captura stdout/stderr/exit_code
    }
}
```

## Métricas Reais

| Operação | p50 | p99 |
|---|---|---|
| `echo hello` | 2,725ms | 2,800ms |
| `echo ayrola_code_ok` | 2,725ms | 2,800ms |
| `seq 1 5 \| tr` | 2,647ms | 2,700ms |

## Nota

O `claim_required` do Railway indica que a VM expirou. O código detecta isso
automaticamente e reporta: "sandbox expired (Railway claim_required)".
