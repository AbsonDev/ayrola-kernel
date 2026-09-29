//! Tools module. Interface para tools externas (speculative reading, etc).
//!
//! Phase 1: GrepTool + ToolExecutor reais.
//! Phase 1: integracao real via MCP (read, grep, web, etc).

use serde::{Deserialize, Serialize};

/// Tipo de tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolType {
    Read,
    Grep,
    WebFetch,
    Run,
    List,
}

impl std::fmt::Display for ToolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolType::Read => write!(f, "read"),
            ToolType::Grep => write!(f, "grep"),
            ToolType::WebFetch => write!(f, "web_fetch"),
            ToolType::Run => write!(f, "run"),
            ToolType::List => write!(f, "list"),
        }
    }
}

/// Resultado de uma chamada de tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub tool: ToolType,
    pub success: bool,
    pub content: String,
    pub duration_ms: u64,
    pub cost_usd: f64,
}

impl ToolResult {
    pub fn ok(tool: ToolType, content: impl Into<String>, duration_ms: u64, cost_usd: f64) -> Self {
        ToolResult {
            tool,
            success: true,
            content: content.into(),
            duration_ms,
            cost_usd,
        }
    }

    pub fn err(tool: ToolType, error: impl Into<String>) -> Self {
        ToolResult {
            tool,
            success: false,
            content: error.into(),
            duration_ms: 0,
            cost_usd: 0.0,
        }
    }
}

/// ToolReader: leitura especulativa de arquivos.
#[derive(Debug, Clone, Default)]
pub struct ToolReader;

impl ToolReader {
    pub fn new() -> Self {
        ToolReader
    }

    /// Le um arquivo. Retorna erro se nao existir.
    pub fn read_file(&self, path: &str) -> ToolResult {
        match std::fs::read_to_string(path) {
            Ok(content) => ToolResult::ok(ToolType::Read, content, 0, 0.0),
            Err(e) => ToolResult::err(ToolType::Read, format!("error: {}", e)),
        }
    }

    /// Lista arquivos em um diretorio.
    pub fn list_dir(&self, path: &str) -> ToolResult {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect();
                ToolResult::ok(ToolType::List, names.join("\n"), 0, 0.0)
            }
            Err(e) => ToolResult::err(ToolType::List, format!("error: {}", e)),
        }
    }
}


/// GrepTool: busca por regex em arquivos.
#[derive(Debug, Clone, Default)]
pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        GrepTool
    }

    /// Busca por padrao em um arquivo.
    pub fn grep_file(&self, path: &str, pattern: &str) -> ToolResult {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => return ToolResult::err(ToolType::Grep, format!("error: {}", e)),
        };

        let matches: Vec<String> = content
            .lines()
            .filter(|line| line.contains(pattern))
            .map(|s| s.to_string())
            .collect();

        if matches.is_empty() {
            ToolResult::err(ToolType::Grep, "no matches found")
        } else {
            ToolResult::ok(ToolType::Grep, matches.join("\n"), 0, 0.0)
        }
    }

    /// Busca recursiva em um diretorio (nao-recursivo por enquanto).
    pub fn grep_dir(&self, dir: &str, pattern: &str) -> ToolResult {
        let mut all_matches = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    let path_str = path.to_string_lossy();
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            if line.contains(pattern) {
                                all_matches.push(format!("{}:{}", path_str, line));
                            }
                        }
                    }
                }
            }
        }

        if all_matches.is_empty() {
            ToolResult::err(ToolType::Grep, "no matches found")
        } else {
            ToolResult::ok(ToolType::Grep, all_matches.join("\n"), 0, 0.0)
        }
    }
}

/// SpeculativeTool: executa tools de forma especulativa e pode descartar.
#[derive(Debug, Clone, Default)]
pub struct SpeculativeTool {
    reader: ToolReader,
}

impl SpeculativeTool {
    pub fn new() -> Self {
        SpeculativeTool {
            reader: ToolReader::new(),
        }
    }

    /// Tenta uma tool. Se falhar, retorna None (descartado).
    pub fn try_read(&self, path: &str) -> Option<String> {
        let res = self.reader.read_file(path);
        if res.success {
            Some(res.content)
        } else {
            None
        }
    }

    /// Executa multiplas leituras speculativas, descartando falhas.
    pub fn try_read_batch(&self, paths: &[String]) -> Vec<(String, Option<String>)> {
        paths
            .iter()
            .map(|p| {
                let content = self.try_read(p);
                (p.clone(), content)
            })
            .collect()
    }
}

/// Registry de tools disponiveis.
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    pub available: Vec<(ToolType, String)>, // (type, description)
}

impl ToolRegistry {
    pub fn new() -> Self {
        let available = vec![
            (ToolType::Read, "Le arquivo local ou remote".to_string()),
            (ToolType::Grep, "Busca por regex em arquivos".to_string()),
            (ToolType::WebFetch, "Busca conteudo de URL via web".to_string()),
            (ToolType::Run, "Executa comando shell".to_string()),
            (ToolType::List, "Lista arquivos em diretorio".to_string()),
        ];
        ToolRegistry { available }
    }

    /// Lista todas as tools disponiveis.
    pub fn list(&self) -> &[(ToolType, String)] {
        &self.available
    }

    /// Busca tool por tipo.
    pub fn find(&self, tool_type: ToolType) -> Option<&(ToolType, String)> {
        self.available.iter().find(|(t, _)| *t == tool_type)
    }
}


/// ToolExecutor: despacha chamadas de tool pelo tipo.
#[derive(Debug, Clone, Default)]
pub struct ToolExecutor {
    reader: ToolReader,
    grep: GrepTool,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            reader: ToolReader::new(),
            grep: GrepTool::new(),
        }
    }

    /// Despacha uma chamada de tool.
    ///
    /// `args[0]` = path (read/list/grep), `args[1]` = pattern (grep).
    pub fn dispatch(&self, tool: ToolType, args: &[&str]) -> ToolResult {
        match tool {
            ToolType::Read => match args.first() {
                Some(path) => self.reader.read_file(path),
                None => ToolResult::err(ToolType::Read, "missing path argument"),
            },
            ToolType::List => match args.first() {
                Some(path) => self.reader.list_dir(path),
                None => ToolResult::err(ToolType::List, "missing path argument"),
            },
            ToolType::Grep => match (args.first(), args.get(1)) {
                (Some(_path), Some(pattern)) => self.grep.grep_file(_path, pattern),
                (Some(_), None) => ToolResult::err(ToolType::Grep, "missing pattern argument"),
                _ => ToolResult::err(ToolType::Grep, "missing path or pattern"),
            },
            ToolType::WebFetch => {
                let start = std::time::Instant::now();
                let url = match args.first() {
                    Some(u) if !u.is_empty() => u,
                    _ => return ToolResult::err(ToolType::WebFetch, "missing url"),
                };
                // Proper SSRF guard: parse URL, extract host, resolve to IP, block RFC1918/loopback/link-local/ipv6.
                use std::net::ToSocketAddrs;
                let Ok(parsed) = url::Url::parse(url) else {
                    return ToolResult::err(ToolType::WebFetch, "invalid url");
                };
                if !matches!(parsed.scheme(), "http" | "https") {
                    return ToolResult::err(ToolType::WebFetch, "scheme must be http or https");
                }
                let host = match parsed.host_str() {
                    Some(h) => h,
                    None => return ToolResult::err(ToolType::WebFetch, "missing host"),
                };
                // Strip userinfo (defense in depth).
                let host = host.split('@').next_back().unwrap_or(host);
                let is_private = match host.parse::<std::net::Ipv4Addr>() {
                    Ok(ip) => {
                        ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
                    }
                    Err(_) => match host.parse::<std::net::Ipv6Addr>() {
                        Ok(ip) => ip.is_loopback() || ip.is_unspecified(),
                        Err(_) => {
                            // Hostname: resolve via DNS and check each resulting IP.
                            let addrs = (host, parsed.port_or_known_default().unwrap_or(80))
                                .to_socket_addrs();
                            match addrs {
                                Ok(mut iter) => iter.any(|a| {
                                    match a.ip() {
                                        std::net::IpAddr::V4(ip) => ip.is_loopback() || ip.is_unspecified() || ip.is_private() || ip.is_link_local(),
                                        std::net::IpAddr::V6(ip) => ip.is_loopback() || ip.is_unspecified(),
                                    }
                                }),
                                Err(_) => true, // cannot resolve -> treat as private to avoid blind SSRF
                            }
                        }
                    },
                };
                if is_private {
                    return ToolResult::err(ToolType::WebFetch, "url blocked: internal/private address");
                }
                // Curl must fail on HTTP >= 400 and must not follow unexpected protocol redirects.
                let output = std::process::Command::new("curl")
                    .args(["-sLf", "--proto", "=http,https", "--proto-redir", "=http,https", "--max-time", "10", url])
                    .output();
                let duration = start.elapsed().as_millis() as u64;

                match output {
                    Ok(o) if o.status.success() => {
                        let content = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        ToolResult::ok(ToolType::WebFetch, content, duration, 0.0)
                    }
                    Ok(o) => {
                        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        ToolResult::err(ToolType::WebFetch, format!("curl failed: {}", err))
                    }
                    Err(e) => ToolResult::err(ToolType::WebFetch, format!("curl not found: {}", e)),
                }
            }
            ToolType::Run => {
                let start = std::time::Instant::now();
                let command = args.first().copied().unwrap_or("");

                // Usa o sandbox local com allowlist de segurança.
                let sandbox = crate::sandbox::SandboxExecutor::new(
                    crate::sandbox::SandboxConfig::default(),
                );
                let res = sandbox.run(command);
                let duration = start.elapsed().as_millis() as u64;

                if res.exit_code == 0 {
                    ToolResult::ok(ToolType::Run, res.stdout, duration, 0.0)
                } else {
                    ToolResult::err(ToolType::Run, format!("exit {}: {}", res.exit_code, res.stderr))
                }
            },
        }
    }

    /// Dispatcha multiplas tools em paralelo usando tokio::task::JoinSet.
    pub async fn dispatch_parallel(
        self,
        calls: Vec<(ToolType, Vec<String>)>,
    ) -> Vec<ToolResult> {
        use tokio::task::JoinSet;

        let mut results = Vec::with_capacity(calls.len());
        let mut set = JoinSet::new();

        for (tool, args) in calls {
            let exec = self.clone();
            set.spawn(async move {
                let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                exec.dispatch(tool, &arg_refs)
            });
        }

        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(result) => results.push(result),
                Err(e) => results.push(ToolResult::err(
                    ToolType::Read,
                    format!("join error: {}", e),
                )),
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_type_display() {
        assert_eq!(ToolType::Read.to_string(), "read");
        assert_eq!(ToolType::Grep.to_string(), "grep");
        assert_eq!(ToolType::WebFetch.to_string(), "web_fetch");
        assert_eq!(ToolType::Run.to_string(), "run");
        assert_eq!(ToolType::List.to_string(), "list");
    }

    #[test]
    fn tool_result_ok_factory() {
        let r = ToolResult::ok(ToolType::Read, "content", 42, 0.01);
        assert!(r.success);
        assert_eq!(r.duration_ms, 42);
        assert!((r.cost_usd - 0.01).abs() < 1e-9);
    }

    #[test]
    fn tool_result_err_factory() {
        let r = ToolResult::err(ToolType::Grep, "not found");
        assert!(!r.success);
        assert_eq!(r.content, "not found");
    }

    #[test]
    fn tool_reader_read_existing_file() {
        let r = ToolReader::new();
        let res = r.read_file("Cargo.toml");
        assert!(res.success, "Cargo.toml deve existir");
        assert!(res.content.contains("ayrola"));
    }

    #[test]
    fn tool_reader_read_nonexistent_file() {
        let r = ToolReader::new();
        let res = r.read_file("/nonexistent/path/file.txt");
        assert!(!res.success);
    }

    #[test]
    fn speculative_tool_discards_failures() {
        let tool = SpeculativeTool::new();
        let res = tool.try_read("/nonexistent/file.txt");
        assert!(res.is_none());
    }

    #[test]
    fn speculative_tool_success_returns_content() {
        let tool = SpeculativeTool::new();
        let res = tool.try_read("Cargo.toml");
        assert!(res.is_some());
        assert!(res.unwrap().contains("ayrola"));
    }

    #[test]
    fn tool_registry_has_five_tools() {
        let reg = ToolRegistry::new();
        assert_eq!(reg.list().len(), 5);
    }

    #[test]
    fn tool_registry_find_by_type() {
        let reg = ToolRegistry::new();
        let read = reg.find(ToolType::Read);
        assert!(read.is_some());
        assert!(!read.unwrap().1.is_empty());
        assert!(reg.find(ToolType::List).is_some());
    }

    #[test]
    fn try_read_batch_returns_all_paths() {
        let tool = SpeculativeTool::new();
        let paths = vec!["Cargo.toml".to_string(), "/nonexistent".to_string()];
        let batch = tool.try_read_batch(&paths);
        assert_eq!(batch.len(), 2);
        assert!(batch[0].1.is_some(), "Cargo.toml deve existir");
        assert!(batch[1].1.is_none(), "nonexistent deve ser None");
    }

    #[test]
    fn tool_result_serializes() {
        let r = ToolResult::ok(ToolType::Run, "output", 10, 0.005);
        let json = serde_json::to_string(&r).unwrap();
        let back: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
    #[test]
    fn grep_tool_finds_pattern() {
        let tool = GrepTool::new();
        let res = tool.grep_file("Cargo.toml", "ayrola");
        assert!(res.success);
        assert!(res.content.contains("ayrola"));
    }

    #[test]
    fn grep_tool_no_match() {
        let tool = GrepTool::new();
        let res = tool.grep_file("Cargo.toml", "nonexistent_pattern_xyz");
        assert!(!res.success);
    }

    #[test]
    fn grep_tool_missing_file() {
        let tool = GrepTool::new();
        let res = tool.grep_file("/nonexistent/file.txt", "pattern");
        assert!(!res.success);
    }
    #[test]
    fn grep_dir_finds_pattern_in_cargo_toml_dir() {
        let tool = GrepTool::new();
        let res = tool.grep_dir(".", "ayrola");
        assert!(res.success, "should find 'ayrola' in project root");
        assert!(res.content.contains("ayrola"), "content should contain match");
    }

    #[test]
    fn grep_without_pattern_returns_error() {
        let exec = ToolExecutor::new();
        let res = exec.dispatch(ToolType::Grep, &["Cargo.toml"]);
        assert!(!res.success, "missing pattern must return error");
        assert!(res.content.contains("pattern"), "error must mention pattern");
    }



    #[tokio::test]
    async fn tool_executor_dispatches_read() {
        let exec = ToolExecutor::new();
        let res = exec.dispatch(ToolType::Read, &["Cargo.toml"]);
        assert!(res.success);
        assert!(res.content.contains("ayrola"));
    }

    #[tokio::test]
    async fn tool_executor_dispatches_grep() {
        let exec = ToolExecutor::new();
        let res = exec.dispatch(ToolType::Grep, &["Cargo.toml", "ayrola"]);
        assert!(res.success);
    }

    #[tokio::test]
    async fn tool_executor_missing_args() {
        let exec = ToolExecutor::new();
        let res = exec.dispatch(ToolType::Read, &[]);
        assert!(!res.success);
        assert!(res.content.contains("missing"));
    }

    #[tokio::test]
    #[ignore = "requires Railway VM; run with --ignored"]
    async fn tool_executor_run_via_sandbox() {
        let exec = ToolExecutor::new();
        let res = exec.dispatch(ToolType::Run, &["echo hello_from_sandbox"]);
        if res.success {
            assert!(res.content.contains("hello_from_sandbox"));
        } else {
            // Railway build window may expire; must be a clear, actionable message.
            assert!(
                res.content.contains("claim_required") || res.content.contains("expired"),
                "failure must explain the expiry: {}",
                res.content
            );
        }
    }

    #[tokio::test]
    async fn tool_executor_parallel_dispatch() {
        let exec = ToolExecutor::new();
        let calls = vec![
            (ToolType::Read, vec!["Cargo.toml".to_string()]),
            (ToolType::Read, vec!["/nonexistent".to_string()]),
        ];
        let results = exec.dispatch_parallel(calls).await;
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| r.success));
        assert!(results.iter().any(|r| !r.success));
    }





    #[test]
    fn web_fetch_rejects_empty_url() {
        let exec = ToolExecutor::new();
        let res = exec.dispatch(ToolType::WebFetch, &[]);
        assert!(!res.success);
        assert!(res.content.contains("missing url"));
    }

    #[test]
    fn web_fetch_blocks_private_ips() {
        let exec = ToolExecutor::new();
        for url in [
            "http://127.0.0.1/",
            "http://localhost/",
            "http://192.168.1.1/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://169.254.169.254/",  // AWS metadata
            "http://[::1]/",
        ] {
            let res = exec.dispatch(ToolType::WebFetch, &[url]);
            assert!(!res.success, "should block {}", url);
            assert!(res.content.contains("blocked"), "{}", res.content);
        }
    }

    #[test]
    fn web_fetch_allows_public_urls() {
        let exec = ToolExecutor::new();
        // These won't actually connect in tests, but should not be blocked.
        // We test the filter logic, not the actual network call.
        let res = exec.dispatch(ToolType::WebFetch, &["http://example.com/"]);
        // May succeed or fail on network, but should not be "blocked"
        assert!(!res.content.contains("blocked"), "false positive: {}", res.content);
    }

    // ── REGRESSION (Bug 40): o blocklist era prefix-match de string crua.
    // Bypassava com https://, userinfo (@), IP decimal/octal/hex e
    // IPv6-mapped. O teste de bypass abaixo falhava com a versao antiga.

    #[test]
    fn web_fetch_blocks_https_metadata_endpoint() {
        // A versao anterior so checava prefixos "http://", entao https://
        // passava direto para o curl.
        let exec = ToolExecutor::new();
        let res = exec.dispatch(ToolType::WebFetch, &["https://169.254.169.254/latest/meta-data/"]);
        assert!(!res.success, "https metadata endpoint must be blocked");
        assert!(res.content.contains("blocked"), "{}", res.content);
    }

    #[test]
    fn web_fetch_blocks_userinfo_trick() {
        // "http://anything.com@127.0.0.1/" — curl conecta em 127.0.0.1,
        // o userinfo antes do @ engana o prefix-match.
        let exec = ToolExecutor::new();
        for url in [
            "http://anything.com@127.0.0.1/",
            "http://x@169.254.169.254/",
            "http://foo@10.0.0.1/",
        ] {
            let res = exec.dispatch(ToolType::WebFetch, &[url]);
            assert!(!res.success, "userinfo trick must be blocked: {}", url);
        }
    }

    #[test]
    fn web_fetch_blocks_non_ip_literal_encodings() {
        // IP decimal/octal/hex nao casam com os prefixos literais.
        // Url::parse normaliza decimal->dotted, octal->dotted, hex->dotted.
        let exec = ToolExecutor::new();
        for url in [
            "http://2130706433/",   // decimal 127.0.0.1
            "http://017700000001/", // octal 127.0.0.1
            "http://0x7f000001/",   // hex 127.0.0.1
        ] {
            let res = exec.dispatch(ToolType::WebFetch, &[url]);
            assert!(!res.success, "non-standard IP encoding must be blocked: {}", url);
            assert!(res.content.contains("blocked"), "{}: {}", url, res.content);
        }
    }

    #[test]
    fn web_fetch_blocks_ipv6_mapped_loopback() {
        // [::ffff:127.0.0.1] mapeia para 127.0.0.1 mas nao casa com "http://[::1]".
        let exec = ToolExecutor::new();
        for url in ["http://[::ffff:127.0.0.1]/", "http://[0:0:0:0:0:ffff:127.0.0.1]/"] {
            let res = exec.dispatch(ToolType::WebFetch, &[url]);
            assert!(!res.success, "IPv6-mapped loopback must be blocked: {}", url);
        }
    }

    #[test]
    fn web_fetch_rejects_non_http_schemes() {
        // Sem restricao de esquema, curl trataria file:// e gopher://.
        let exec = ToolExecutor::new();
        for url in [
            "file:///etc/passwd",
            "gopher://127.0.0.1:6379/_INFO",
            "ftp://example.com/x",
            "dict://127.0.0.1:11211/stat",
        ] {
            let res = exec.dispatch(ToolType::WebFetch, &[url]);
            assert!(!res.success, "non-http scheme must be rejected: {}", url);
            assert!(
                res.content.contains("scheme must be") || res.content.contains("invalid url"),
                "{}: {}",
                url,
                res.content
            );
        }
    }

    #[test]
    fn web_fetch_rejects_malformed_url() {
        let exec = ToolExecutor::new();
        for url in ["not-a-url", "http://", "://missing-scheme", "http://[invalid"] {
            let res = exec.dispatch(ToolType::WebFetch, &[url]);
            assert!(!res.success, "malformed url must be rejected: {}", url);
        }
    }

    #[test]
    fn web_fetch_allows_public_https_url() {
        // Contra-regressao: a guarda nova nao pode bloquear trafego publico.
        let exec = ToolExecutor::new();
        for url in ["https://example.com/", "http://example.com/"] {
            let res = exec.dispatch(ToolType::WebFetch, &[url]);
            assert!(
                !res.content.contains("blocked") && !res.content.contains("scheme must be"),
                "public url false positive: {} -> {}",
                url,
                res.content
            );
        }
    }
}
