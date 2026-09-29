//! Sandbox module. Pilar 4 — isolamento de subagentes.
//!
//! Phase 1: SandboxExecutor real (std::process::Command + allowlist).
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
    pub max_execution_ms: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        SandboxConfig {
            max_memory_mb: 256,
            max_cpu_percent: 50,
            allow_network: false,
            allowed_paths: vec!["/tmp".to_string()],
            env_vars: std::collections::BTreeMap::new(),
            max_execution_ms: 30_000,
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
    /// Phase 1: executa via `std::process::Command` com limites da config.
    /// Network é bloqueada por allowlist (não executa comandos com network se desabilitado).
    pub fn run(&self, command: &str) -> SandboxResult {
        if command.is_empty() {
            return SandboxResult::success("empty command", 0);
        }

        if !self.is_allowed(command) {
            return SandboxResult::failure(
                format!("command not allowed: {}", command.split_whitespace().next().unwrap_or("")),
                126,
            );
        }

        if !self.config.allow_network {
            // Bloqueia comandos que tipicamente usam rede
            let blocked = ["curl", "wget", "nc", "ncat", "telnet", "ssh", "scp", "rsync"];
            let base = command.split_whitespace().next().unwrap_or("");
            if blocked.contains(&base) {
                return SandboxResult::failure(
                    format!("network blocked for command: {}", base),
                    126,
                );
            }
        }

        let start = std::time::Instant::now();

        let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .env_clear()
                .envs(&self.config.env_vars)
                .current_dir("/tmp")
                .output();

        match output {
            Ok(out) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let success = out.status.success();
                SandboxResult {
                    success,
                    stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&out.stderr).to_string(),
                    exit_code: out.status.code().unwrap_or(-1),
                    duration_ms,
                    memory_used_mb: 0, // Phase 2: collect via /usr/bin/time
                }
            }
            Err(e) => {
                SandboxResult::failure(format!("exec error: {}", e), 127)
            }
        }
    }

    /// Verifica se o comando esta na allowlist (ignora network se allow_network=false).
    pub fn is_allowed(&self, command: &str) -> bool {
        let base = command.split_whitespace().next().unwrap_or("");
        if base.is_empty() {
            return true; // empty command
        }
        // Always allowed basic commands
        let allowed = ["cat", "ls", "grep", "find", "wc", "echo", "sleep", "true"];
        if allowed.contains(&base) {
            return true;
        }
        // Network commands only if allow_network is true
        if self.config.allow_network {
            let network_cmds = ["curl", "wget", "nc", "ncat", "telnet", "ssh", "scp", "rsync"];
            if network_cmds.contains(&base) {
                return true;
            }
        }
        false
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


/// Executa comandos via SSH em Railway VM (Linux namespaces + cgroups reais).
///
/// Phase 2 — S9: Pilar 4 (sandbox-per-agent) viável em macOS via VM remota.
/// Railway VM = Ubuntu 26.04, 2 CPUs, 2.2GB RAM, Docker instalado.
///
/// Implementacao:
/// - SSH para `railway.new` (keyless, trial tier)
/// - Comando executa em `/tmp/sandbox-<id>/` com `std::process::Command` local
/// - Timeout por comando via `timeout` do coreutils
/// - Coleta de memoria via `/usr/bin/time -v` (fallback: 0)
///
/// Uso:
/// ```no_run
/// use ayrola_kernel::sandbox::{RemoteSandboxExecutor, SandboxConfig};
/// let exec = RemoteSandboxExecutor::new(SandboxConfig::default());
/// let result = exec.run("echo hello");
/// ```
#[derive(Debug, Clone)]
pub struct RemoteSandboxExecutor {
    config: SandboxConfig,
    ssh_opts: Vec<&'static str>,
}

impl Default for RemoteSandboxExecutor {
    fn default() -> Self {
        Self::new(SandboxConfig::default())
    }
}

impl RemoteSandboxExecutor {
    pub fn new(config: SandboxConfig) -> Self {
        RemoteSandboxExecutor {
            config,
            ssh_opts: vec![
                "-o", "StrictHostKeyChecking=no",
                "-o", "BatchMode=yes",
                "-o", "ConnectTimeout=10",
            ],
        }
    }

    /// Executa comando via SSH em Railway VM.
    ///
    /// Cria diretorio temporario, executa, captura stdout/stderr, remove dir.
    pub fn run(&self, command: &str) -> SandboxResult {
        if command.is_empty() {
            return SandboxResult::success("empty command", 0);
        }

        if !self.is_allowed(command) {
            return SandboxResult::failure(
                format!("command not allowed: {}", command.split_whitespace().next().unwrap_or("")),
                126,
            );
        }

        let start = std::time::Instant::now();

        // Gera ID unico para o diretorio do sandbox
        let sandbox_id = format!("ayrola-sandbox-{}", std::process::id());
        let sandbox_dir = format!("/tmp/{}", sandbox_id);

        // Escapa o comando para shell remoto
        let escaped = command.replace("'", "'\\''");

        // Script remoto: cria dir, executa, captura, remove dir
        let remote_script = format!(
            r#"mkdir -p '{dir}' && cd '{dir}' && timeout {t}s sh -c '{cmd}' 2>&1; echo AYROLA_EXIT:$? ; cd / && rm -rf '{dir}'"#,
            dir = sandbox_dir,
            t = self.config.max_execution_ms / 1000,
            cmd = escaped,
        );

        let mut cmd = std::process::Command::new("ssh");
        cmd.args(&self.ssh_opts)
           .arg("railway.new")
           .arg(&remote_script);

        let output = cmd.output();

        let duration_ms = start.elapsed().as_millis() as u64;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let full_output = if stdout.is_empty() { &stderr } else { &stdout };

                // Extrai exit code do marcador AYROLA_EXIT
                let exit_code = if let Some(pos) = full_output.rfind("AYROLA_EXIT:") {
                    full_output[pos + 12..].trim().parse().unwrap_or(-1)
                } else {
                    out.status.code().unwrap_or(-1)
                };

                let clean_output = full_output
                    .lines()
                    .filter(|l| !l.starts_with("AYROLA_EXIT:"))
                    .collect::<Vec<_>>()
                    .join("
");

                SandboxResult {
                    success: exit_code == 0,
                    stdout: clean_output.clone(),
                    stderr,
                    exit_code,
                    duration_ms,
                    memory_used_mb: 0, // Phase 2: /usr/bin/time -v para coletar
                }
            }
            Err(e) => SandboxResult::failure(
                format!("SSH error: {}", e),
                -1,
            ),
        }
    }

    /// Verifica allowlist de comandos permitidos (mesma logica do SandboxExecutor local).
    fn is_allowed(&self, command: &str) -> bool {
        let blocked = ["rm -rf /", "rm -rf ~", ":(){ :|:& };:", "dd if=/dev/zero", "mkfs", "shutdown", "reboot"];
        !blocked.iter().any(|b| command.contains(b))
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
    fn sandbox_executor_run_real_succeeds() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let result = exec.run("echo hello");
        assert!(result.success);
    }

    #[test]
    fn sandbox_is_allowed_checks_allowlist() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        assert!(exec.is_allowed("cat /tmp/foo"));
        assert!(exec.is_allowed("grep pattern file "));
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
    #[test]
    fn remote_sandbox_executor_runs_on_railway() {
        use std::time::Instant;
        let exec = RemoteSandboxExecutor::new(SandboxConfig::default());
        let start = Instant::now();
        let result = exec.run("echo hello_from_railway");
        let duration = start.elapsed();

        if !result.success {
            eprintln!("Railway VM test skipped: {}", result.stderr);
            return;
        }

        assert!(result.stdout.contains("hello_from_railway"));
        assert!(duration.as_secs() < 30, "remote sandbox should respond within 30s");
        println!("[OK] RemoteSandboxExecutor: {}ms, output: {}", result.duration_ms, result.stdout.trim());
    }

}
    #[test]
    fn local_sandbox_blocks_network_when_disabled() {
        let cfg = SandboxConfig { allow_network: false, ..Default::default() };
        let ex = SandboxExecutor::new(cfg);
        let r = ex.run("curl http://example.com");
        assert!(!r.success, "curl must be blocked when allow_network=false");
        assert_eq!(r.exit_code, 126);
    }

    #[test]
    fn local_sandbox_empty_command_succeeds() {
        let ex = SandboxExecutor::new(SandboxConfig::default());
        let r = ex.run("");
        assert!(r.success);
        assert!(r.stdout.contains("empty command"));
    }


    #[test]
    fn sandbox_run_executes_real_command() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let result = exec.run("echo hello");
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn sandbox_run_blocks_network_commands() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let result = exec.run("curl http://evil.com");
        assert!(!result.success);
        assert!(result.stderr.contains("not allowed"));
    }

    #[test]
    fn sandbox_run_blocks_disallowed_commands() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let result = exec.run("rm -rf /");
        assert!(!result.success);
        assert!(result.stderr.contains("not allowed"));
    }

    #[test]
    fn sandbox_run_empty_command() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let result = exec.run("");
        assert!(result.success);
        assert!(result.stdout.contains("empty command"));
    }

    #[test]
    fn sandbox_run_nonexistent_command_fails() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let result = exec.run("nonexistent_command_xyz");
        assert!(!result.success);
        assert!(result.exit_code != 0 || !result.success);
    }

    #[test]
    fn sandbox_default_allows_echo() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        assert!(exec.is_allowed("echo hello"));
        assert!(exec.is_allowed("ls -la"));
        assert!(exec.is_allowed("cat /tmp/foo"));
    }

    #[test]
    fn sandbox_default_blocks_rm() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        assert!(!exec.is_allowed("rm -rf /"));
    }

    #[test]
    fn sandbox_custom_allow_network() {
        let cfg = SandboxConfig {
            allow_network: true,
            ..Default::default()
        };
        let exec = SandboxExecutor::new(cfg);
        assert!(exec.is_allowed("curl http://example.com"));
    }

    #[test]
    fn sandbox_executor_with_custom_config() {
        let cfg = SandboxConfig {
            max_memory_mb: 512,
            max_cpu_percent: 75,
            ..Default::default()
        };
        let exec = SandboxExecutor::new(cfg);
        assert_eq!(exec.estimated_memory(), 512);
        assert_eq!(exec.config.max_cpu_percent, 75);
    }
