//! Sandbox module. Pilar 4 — isolamento de subagentes.
//!
//! Phase 0: stubs (macOS nao suporta Linux namespaces).
//! Phase 1: via Railway VM (Linux namespaces + cgroups).
//!
//! Em producao:
//! - Namespace filesystem: chroot/mount namespace
//! - Namespace rede: network namespace (sem acesso a internet)
//! - Namespace PID: pid namespace (subagente ve so seu proprio PID)
//! - cgroups: limitar CPU e memoria

use serde::{Deserialize, Serialize};

/// Configuracao de sandbox para um subagente.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxConfig {
    pub max_memory_mb: u64,
    pub max_cpu_percent: u64,
    pub allow_network: bool,
    pub allowed_paths: Vec<String>,
    pub env_vars: std::collections::BTreeMap<String, String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        SandboxConfig {
            max_memory_mb: 256,
            max_cpu_percent: 50,
            allow_network: false,
            allowed_paths: vec!["/tmp".to_string()],
            env_vars: std::collections::BTreeMap::new(),
        }
    }
}

/// Resultado da execucao de um subagente em sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub memory_used_mb: u64,
}

impl SandboxResult {
    pub fn success(stdout: impl Into<String>, duration_ms: u64) -> Self {
        SandboxResult {
            success: true,
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms,
            memory_used_mb: 0,
        }
    }

    pub fn failure(stderr: impl Into<String>, exit_code: i32) -> Self {
        SandboxResult {
            success: false,
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
            duration_ms: 0,
            memory_used_mb: 0,
        }
    }
}

/// Executor de sandbox.
#[derive(Debug, Clone, Default)]
pub struct SandboxExecutor {
    pub config: SandboxConfig,
}

impl SandboxExecutor {
    pub fn new(config: SandboxConfig) -> Self {
        SandboxExecutor { config }
    }

    /// Executa um comando em sandbox.
    ///
    /// Phase 0 (stub): simula execucao sem isolamento real.
    /// Phase 1 (Railway VM): executa via SSH em VM Linux com namespaces.
    pub async fn run(&self, _command: &str) -> SandboxResult {
        // Stub: retorna sucesso simulado.
        // Em producao: ssh railway.new -> namespace -> command -> capture output.
        SandboxResult::success("stub: sandbox simulation", 0)
    }

    /// Verifica se o comando esta na allowlist.
    pub fn is_allowed(&self, command: &str) -> bool {
        let allowed = ["cat", "ls", "grep", "find", "wc", "echo", "sleep", "true"];
        let base = command.split_whitespace().next().unwrap_or("");
        allowed.contains(&base)
    }

    /// Estima memoria usada baseado na config.
    pub fn estimated_memory(&self) -> u64 {
        self.config.max_memory_mb
    }
}

/// Circuit breaker: para execucoes que excedem limites.
#[derive(Debug, Clone, Default)]
pub struct CircuitBreaker {
    pub failures: u64,
    pub threshold: u64,
    pub tripped: bool,
}

impl CircuitBreaker {
    pub fn new(threshold: u64) -> Self {
        CircuitBreaker {
            failures: 0,
            threshold,
            tripped: false,
        }
    }

    /// Registra uma falha. Se exceder threshold, aciona o breaker.
    pub fn record_failure(&mut self) {
        self.failures += 1;
        if self.failures >= self.threshold {
            self.tripped = true;
        }
    }

    /// Registra sucesso: reseta contador.
    pub fn record_success(&mut self) {
        self.failures = 0;
        self.tripped = false;
    }

    /// True se o breaker esta acionado (nao deve executar).
    pub fn is_open(&self) -> bool {
        self.tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_config_defaults() {
        let cfg = SandboxConfig::default();
        assert_eq!(cfg.max_memory_mb, 256);
        assert_eq!(cfg.max_cpu_percent, 50);
        assert!(!cfg.allow_network);
    }

    #[test]
    fn sandbox_executor_run_stub_succeeds() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(exec.run("echo hello"));
        assert!(result.success);
    }

    #[test]
    fn sandbox_is_allowed_checks_allowlist() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        assert!(exec.is_allowed("cat /tmp/foo"));
        assert!(exec.is_allowed("grep pattern file"));
        assert!(!exec.is_allowed("rm -rf /"));
        assert!(!exec.is_allowed("curl http://evil.com"));
    }

    #[test]
    fn circuit_breaker_trips_after_threshold() {
        let mut cb = CircuitBreaker::new(3);
        assert!(!cb.is_open());
        cb.record_failure();
        cb.record_failure();
        assert!(!cb.is_open());
        cb.record_failure();
        assert!(cb.is_open(), "deve acionar apos 3 falhas");
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let mut cb = CircuitBreaker::new(2);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.is_open());
        cb.record_success();
        assert!(!cb.is_open(), "sucesso reseta o breaker");
        assert_eq!(cb.failures, 0);
    }

    #[test]
    fn circuit_breaker_allows_below_threshold() {
        let mut cb = CircuitBreaker::new(5);
        for _ in 0..4 {
            cb.record_failure();
        }
        assert!(!cb.is_open(), "4 falhas < threshold 5");
    }

    #[test]
    fn sandbox_result_factory_methods() {
        let ok = SandboxResult::success("done", 42);
        assert!(ok.success);
        assert_eq!(ok.exit_code, 0);

        let err = SandboxResult::failure("boom", 1);
        assert!(!err.success);
        assert_eq!(err.exit_code, 1);
    }

    #[test]
    fn sandbox_config_serializes() {
        let cfg = SandboxConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: SandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }
}
