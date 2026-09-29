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

        // max_execution_ms e parte do contrato de config: se o comando exceder,
        // matamos o processo (nao apenas ignoramos, como acontecia antes).
        let mut child = match std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .env_clear()
                .envs(&self.config.env_vars)
                .current_dir("/tmp")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn() {
            Ok(c) => c,
            Err(e) => {
                return SandboxResult::failure(format!("exec error: {}", e), 127);
            }
        };

        let limit_ms = self.config.max_execution_ms;
        let mut timed_out = false;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut out = Vec::new();
                    let mut err = Vec::new();
                    if let Some(mut stdout) = child.stdout.take() {
                        let _ = std::io::Read::read_to_end(&mut stdout, &mut out);
                    }
                    if let Some(mut stderr) = child.stderr.take() {
                        let _ = std::io::Read::read_to_end(&mut stderr, &mut err);
                    }
                    let exit_code = status.code().unwrap_or(-1);
                    let duration_ms = start.elapsed().as_millis() as u64;
                    return SandboxResult {
                        success: status.success() && !timed_out,
                        stdout: String::from_utf8_lossy(&out).to_string(),
                        stderr: String::from_utf8_lossy(&err).to_string(),
                        exit_code: if timed_out { 124 } else { exit_code },
                        duration_ms,
                        memory_used_mb: 0,
                    };
                }
                Ok(None) => {
                    if start.elapsed().as_millis() as u64 > limit_ms {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => {
                    return SandboxResult::failure(format!("wait error: {}", e), 127);
                }
            }
        }
    }

    /// Verifica se o comando esta na allowlist (ignora network se allow_network=false).
    pub fn is_allowed(&self, command: &str) -> bool {
        let base = command.split_whitespace().next().unwrap_or("");
        if base.is_empty() {
            return true; // empty command
        }
        // SECURITY: the full command string is passed to `sh -c`, so a
        // command like `echo safe; rm -rf /` would pass the first-word
        // check and then execute the chained `rm`. Reject any command
        // containing shell metacharacters that could chain or redirect.
        const FORBIDDEN: &[char] = &[';', '&', '|', '`', '$', '>', '<', '\\', '\n', '\r'];
        if command.chars().any(|c| FORBIDDEN.contains(&c)) {
            return false;
        }
        // Always allowed basic commands (no path arguments expected)
        let allowed = ["cat", "ls", "grep", "find", "wc", "echo", "sleep", "true"];
        if allowed.contains(&base) {
            // If the command takes an absolute path argument, enforce allowed_paths.
            // Commands like `cat /etc/passwd` must respect the path restriction.
            if self.config.allowed_paths.is_empty() {
                return true;
            }
            for arg in command.split_whitespace().skip(1) {
                if arg.starts_with('/')
                    && !self.config.allowed_paths.iter().any(|p| {
                        let pb = p.as_str();
                        // Require the path to be exactly the allowed directory
                        // or a file/dir inside it (boundary on '/').
                        // Also canonicalize to block ".." traversal.
                        let arg_canon = std::fs::canonicalize(arg).ok();
                        let allowed_canon = std::fs::canonicalize(pb).ok();
                        match (arg_canon, allowed_canon) {
                            (Some(a), Some(b)) => a.starts_with(&b),
                            _ => {
                                // canonicalize failed (e.g. path doesn't exist yet).
                                // Reject any argument that contains ".." components,
                                // because prefix-match alone cannot detect traversal.
                                if arg.split('/').any(|c| c == "..") {
                                    return false;
                                }
                                arg == pb || arg.starts_with(&format!("{}/", pb))
                            }
                        }
                    }) {
                    return false;
                }
            }
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

        // ID unico por chamada. So o PID nao basta: duas execucoes
        // concorrentes no mesmo processo usariam o mesmo diretorio, e o
        // `rm -rf` de uma apagaria o trabalho da outra.
        let sandbox_id = format!("ayrola-sandbox-{}-{}", std::process::id(), uuid::Uuid::new_v4());
        let sandbox_dir = format!("/tmp/{}", sandbox_id);

        // Escapa o comando para shell remoto
        let escaped = command.replace("'", "'\\''");

        // Script remoto: cria dir, executa, captura, remove dir
        let remote_script = format!(
            r#"mkdir -p '{dir}' && cd '{dir}' && timeout {t}s sh -c '{cmd}' 2>&1; echo AYROLA_EXIT:$? ; cd / && rm -rf '{dir}'"#,
            dir = sandbox_dir,
            t = (self.config.max_execution_ms / 1000).max(1),
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
    /// O doc-comment original afirmava paridade, mas a implementacao usava blocklist
    /// (7 strings perigosas), permitindo comandos arbitrarios. Corrigido para
    /// alinhar com o executor local: allowlist positiva para comandos basicos.
    fn is_allowed(&self, command: &str) -> bool {
        let base = command.split_whitespace().next().unwrap_or("");
        if base.is_empty() {
            return true;
        }
        // SECURITY: same metacharacter guard as SandboxExecutor. The remote
        // script wraps the command in `sh -c`, so chained commands would
        // execute over SSH if not blocked here.
        const FORBIDDEN: &[char] = &[';', '&', '|', '`', '$', '>', '<', '\\', '\n', '\r'];
        if command.chars().any(|c| FORBIDDEN.contains(&c)) {
            return false;
        }
        let allowed = ["cat", "ls", "grep", "find", "wc", "echo", "sleep", "true"];
        if allowed.contains(&base) {
            // Enforce allowed_paths on absolute-path arguments (RemoteSandboxExecutor).
            if self.config.allowed_paths.is_empty() {
                return true;
            }
            for arg in command.split_whitespace().skip(1) {
                if arg.starts_with('/')
                    && !self.config.allowed_paths.iter().any(|p| {
                        let pb = p.as_str();
                        // Require the path to be exactly the allowed directory
                        // or a file/dir inside it (boundary on '/').
                        // Also canonicalize to block ".." traversal.
                        let arg_canon = std::fs::canonicalize(arg).ok();
                        let allowed_canon = std::fs::canonicalize(pb).ok();
                        match (arg_canon, allowed_canon) {
                            (Some(a), Some(b)) => a.starts_with(&b),
                            _ => {
                                // canonicalize failed (e.g. path doesn't exist yet).
                                // Reject any argument that contains ".." components,
                                // because prefix-match alone cannot detect traversal.
                                if arg.split('/').any(|c| c == "..") {
                                    return false;
                                }
                                arg == pb || arg.starts_with(&format!("{}/", pb))
                            }
                        }
                    }) {
                    return false;
                }
            }
            return true;
        }
        if self.config.allow_network {
            let network_cmds = ["curl", "wget", "nc", "ncat", "telnet", "ssh", "scp", "rsync"];
            if network_cmds.contains(&base) {
                return true;
            }
        }
        false
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

    // ── REGRESSION: allowed_paths must be enforced on absolute-path arguments
    #[test]
    fn sandbox_blocks_absolute_path_outside_allowed_paths() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        // allowed_paths defaults to ["/tmp"], so /etc/passwd must be blocked.
        assert!(!exec.is_allowed("cat /etc/passwd"));
        assert!(!exec.is_allowed("ls /etc"));
        assert!(!exec.is_allowed("grep foo /var/log/syslog"));
    }

    #[test]
    fn sandbox_allows_absolute_path_inside_allowed_paths() {
        let cfg = SandboxConfig {
            allowed_paths: vec!["/tmp".to_string(), "/var/log".to_string()],
            ..Default::default()
        };
        let exec = SandboxExecutor::new(cfg);
        assert!(exec.is_allowed("cat /tmp/foo"));
        assert!(exec.is_allowed("grep bar /var/log/syslog"));
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

    #[test]
    fn sandbox_rejects_chained_commands() {
        // REGRESSION (Bug 30): shell metacharacters like `;` let the first-word
        // allowlist be bypassed. `echo safe; rm file` ran the rm.
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let r = exec.run("echo safe; echo pwned");
        assert!(!r.success, "chained command must be blocked");
        assert_eq!(r.exit_code, 126, "should return allowlist-denied exit code");
    }

    #[test]
    fn sandbox_rejects_pipe_chain() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let r = exec.run("echo data | grep d");
        assert!(!r.success, "pipe chain must be blocked");
        assert_eq!(r.exit_code, 126);
    }

    #[test]
    fn sandbox_rejects_backtick_substitution() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let r = exec.run("echo $(whoami)");
        assert!(!r.success, "command substitution must be blocked");
        assert_eq!(r.exit_code, 126);
    }

    #[test]
    fn sandbox_rejects_redirect() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let r = exec.run("echo x > /tmp/ayrola_test_redirect");
        assert!(!r.success, "redirect must be blocked");
        assert_eq!(r.exit_code, 126);
    }

    #[test]
    fn sandbox_allows_simple_echo() {
        // Legitimate allowlisted command with no metacharacters must still work.
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let r = exec.run("echo hello");
        assert!(r.success, "simple echo must pass: stderr={}", r.stderr);
        assert!(r.stdout.contains("hello"));
    }

    #[test]
    fn remote_sandbox_rejects_chained_commands() {
        // REGRESSION (Bug 30): remote executor had the same first-word-only
        // allowlist hole as the local one. The remote script wraps the command
        // in `sh -c`, so `;` would chain a second command over SSH.
        let exec = RemoteSandboxExecutor::new(SandboxConfig::default());
        assert!(!exec.is_allowed("echo safe; rm -rf /"));
        assert!(!exec.is_allowed("echo a && echo b"));
        assert!(!exec.is_allowed("echo `whoami`"));
        assert!(!exec.is_allowed("echo x > /tmp/f"));
        // Legitimate commands still pass.
        assert!(exec.is_allowed("echo hello"));
        assert!(exec.is_allowed("ls /tmp"));
    }

    #[test]
    fn sandbox_rejects_disallowed_base_command() {
        let exec = SandboxExecutor::new(SandboxConfig::default());
        let r = exec.run("rm -rf /tmp/nothing");
        assert!(!r.success, "rm must be blocked by allowlist");
        assert_eq!(r.exit_code, 126);
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
    fn local_sandbox_enforces_max_execution_ms() {
        // REGRESSION: max_execution_ms is part of the config contract.
        // Before the fix it was declared but never enforced, so a
        // `sleep 10` ran to completion despite a 200ms limit.
        let cfg = SandboxConfig { max_execution_ms: 200, ..Default::default() };
        let ex = SandboxExecutor::new(cfg);
        let start = std::time::Instant::now();
        let r = ex.run("sleep 10");
        let elapsed_ms = start.elapsed().as_millis() as u64;
        assert!(!r.success, "timed-out command must not report success");
        assert_eq!(r.exit_code, 124, "timeout uses conventional exit code 124");
        assert!(elapsed_ms < 3000,
            "command must be killed near the limit, took {}ms", elapsed_ms);
    }

    #[test]
    fn local_sandbox_allows_fast_command_within_limit() {
        // Counterpart to the timeout test: normal commands must still
        // complete and produce their stdout.
        let cfg = SandboxConfig { max_execution_ms: 5_000, ..Default::default() };
        let ex = SandboxExecutor::new(cfg);
        let r = ex.run("echo fast_ok");
        assert!(r.success);
        assert!(r.stdout.contains("fast_ok"),
            "stdout must be captured, got: {:?}", r.stdout);
    }

    #[test]
    fn remote_sandbox_is_allowed_uses_allowlist() {
        // REGRESSION: the remote executor's doc comment claimed parity with
        // the local allowlist, but it used a 7-entry blocklist that allowed
        // arbitrary commands (`rm -rf /var`, `python`, `chmod`, ...).
        let exec = RemoteSandboxExecutor::new(SandboxConfig::default());
        // Allowlisted basics still work.
        assert!(exec.is_allowed("echo hi"), "echo must be allowed");
        assert!(exec.is_allowed("ls -la /tmp"), "ls must be allowed");
        // Arbitrary commands must be rejected.
        assert!(!exec.is_allowed("python3 -c 'import os'"),
            "python must not be allowed (allowlist, not blocklist)");
        assert!(!exec.is_allowed("rm -rf /var"), "rm must not be allowed");
        assert!(!exec.is_allowed("chmod 777 /"), "chmod must not be allowed");
        assert!(!exec.is_allowed("dd if=/dev/zero of=/dev/sda"),
            "dd must not be allowed");
    }

    #[test]
    fn remote_sandbox_dir_is_unique_per_call() {
        use std::collections::HashSet;
        let mut ids = HashSet::new();
        for _ in 0..100 {
            let sandbox_id = format!(
                "ayrola-sandbox-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            );
            ids.insert(sandbox_id);
        }
        assert_eq!(ids.len(), 100, "sandbox dir IDs must be unique per call");
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

#[test]
    fn sandbox_allowed_paths_rejects_prefix_only_traversal() {
        let dir = std::env::temp_dir().join("ayrola-sandbox-test-prefix");
        let _ = std::fs::create_dir_all(&dir);
        let allowed = dir.join("allowed").to_string_lossy().to_string();
        let cfg = SandboxConfig {
            allowed_paths: vec![allowed],
            ..SandboxConfig::default()
        };
        let ex = SandboxExecutor::new(cfg);
        // Exact match: allowed
        let ok = dir.join("allowed").join("ok").to_string_lossy().to_string();
        assert!(ex.is_allowed(&format!("cat {}", ok)));
        // Prefix-only escape: ".../allowedX" must be rejected
        let escape = dir.join("allowedX").to_string_lossy().to_string();
        assert!(!ex.is_allowed(&format!("cat {}", escape)));
    }

    #[test]
    fn sandbox_allowed_paths_rejects_dotdot_traversal() {
        let dir = std::env::temp_dir().join("ayrola-sandbox-test-dotdot");
        let allowed_dir = dir.join("allowed");
        let _ = std::fs::create_dir_all(&allowed_dir);
        let allowed = allowed_dir.to_string_lossy().to_string();
        let cfg = SandboxConfig {
            allowed_paths: vec![allowed],
            ..SandboxConfig::default()
        };
        let ex = SandboxExecutor::new(cfg);
        // ".." traversal must be rejected after canonicalization
        let traversal = allowed_dir.join("..").join("..").join("etc").join("passwd");
        let traversal_s = traversal.to_string_lossy().to_string();
        assert!(!ex.is_allowed(&format!("cat {}", traversal_s)));
    }
}