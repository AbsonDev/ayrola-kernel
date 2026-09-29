//! ayrola-agent — MCP Server exposing Ayrola Kernel as JSON-RPC tools.
//!
//! Phase 3 — S14: torna o kernel consumível por qualquer cliente MCP
//! (Claude Code, OpenCode, Cursor, VS Code, etc.).
//!
//! Protocolo: JSON-RPC 2.0 sobre stdio (linha-delimited JSON).
//! Ferramentas expostas:
//!   - decide: pergunta yesno/choice/score (com ou sem LLM)
//!   - shadow: executa golden set contra 9Router
//!   - bench: roda suite de benchmarks
//!   - health: health check do kernel
//!
//! Uso:
//!   ayrola-agent                  # stdio MCP server (default)
//!   ayrola-agent --http 20130     # HTTP MCP server na porta 20130

use std::io::{BufRead, BufReader, Write};
use std::process::exit;

use ayrola_kernel::{
    bench::default_suite,
    config::ConfigLoader,
    decision::{DecisionEngine, QuestionType},
    obs::init_tracing,
    tools::{GrepTool, ToolReader},
};

/// JSON-RPC 2.0 request.
#[derive(serde::Deserialize, Debug)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

/// JSON-RPC 2.0 response (unified: result or error).
#[derive(serde::Serialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorObject>,
}

#[derive(serde::Serialize)]
struct ErrorObject {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    fn ok(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    fn err(id: Option<serde_json::Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(ErrorObject { code, message: message.into(), data: None }),
        }
    }
}

/// Tool call arguments (params.params).
#[derive(serde::Deserialize, Debug)]
struct ToolCall {
    name: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

/// Tool result format (MCP).
#[derive(serde::Serialize)]
struct ToolResult {
    content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
}

#[derive(serde::Serialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

/// Routes a tool call to the kernel and returns the MCP-formatted result.
fn handle_read(args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "missing path".into() }], is_error: Some(true) });
    }
    let res = ToolReader::new().read_file(path);
    Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text: format!("{}: {}", if res.success { "OK" } else { "FAIL" }, res.content) }],
        is_error: Some(!res.success),
    })
}

fn handle_grep(args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() || pattern.is_empty() {
        return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "missing path or pattern".into() }], is_error: Some(true) });
    }
    let res = GrepTool::new().grep_file(path, pattern);
    Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text: format!("{}: {}", if res.success { "MATCH" } else { "NO MATCH" }, res.content) }],
        is_error: Some(!res.success),
    })
}

fn handle_webfetch(args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "missing url".into() }], is_error: Some(true) });
    }
    // Proper SSRF guard: parse URL, extract host, resolve to IP, block private/loopback/link-local/unspecified.
    use std::net::ToSocketAddrs;
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "invalid url".into() }], is_error: Some(true) }),
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "scheme must be http or https".into() }], is_error: Some(true) });
    }
    let host = match parsed.host_str() {
        Some(h) => h,
        None => return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "missing host".into() }], is_error: Some(true) }),
    };
    let host = host.split('@').next_back().unwrap_or(host);
    let is_private = match host.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified(),
        Err(_) => match host.parse::<std::net::Ipv6Addr>() {
            Ok(ip) => ip.is_loopback() || ip.is_unspecified()
                || ip.is_unique_local() || ip.is_unicast_link_local(),
            Err(_) => {
                let addrs = (host, parsed.port_or_known_default().unwrap_or(80)).to_socket_addrs();
                match addrs {
                    Ok(mut iter) => iter.any(|a| {
                        match a.ip() {
                            std::net::IpAddr::V4(ip) => ip.is_loopback() || ip.is_unspecified() || ip.is_private() || ip.is_link_local(),
                            std::net::IpAddr::V6(ip) => ip.is_loopback() || ip.is_unspecified() || ip.is_unique_local() || ip.is_unicast_link_local(),
                        }
                    }),
                    Err(_) => true, // cannot resolve -> block to avoid blind SSRF
                }
            }
        },
    };
    if is_private {
        return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "url blocked: internal/private address".into() }], is_error: Some(true) });
    }
    let start = std::time::Instant::now();
    // -f fails on HTTP >= 400, --proto restricts to http/https + redirects.
    let output = std::process::Command::new("curl")
        .args(["-sLf", "--proto", "=http,https", "--proto-redir", "=http,https", "--max-time", "10", url])
        .output();
    let duration = start.elapsed().as_millis() as u64;

    match output {
        Ok(o) if o.status.success() => {
            let content = String::from_utf8_lossy(&o.stdout).trim().to_string();
            Ok(ToolResult {
                content: vec![ContentBlock { kind: "text".into(), text: format!("fetched {} bytes in {}ms", content.len(), duration) }],
                is_error: None,
            })
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            Ok(ToolResult {
                content: vec![ContentBlock { kind: "text".into(), text: format!("curl failed: {}", err) }],
                is_error: Some(true),
            })
        }
        Err(e) => Ok(ToolResult {
            content: vec![ContentBlock { kind: "text".into(), text: format!("curl not found: {}", e) }],
            is_error: Some(true),
        }),
    }
}

fn handle_run(args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() {
        return anyhow::Ok(ToolResult { content: vec![ContentBlock { kind: "text".into(), text: "missing command".into() }], is_error: Some(true) });
    }
    // Usa o sandbox LOCAL com allowlist de seguranca.
    // RemoteSandboxExecutor exigiria um claim humano na Railway e nao e
    // acessivel a partir de um cliente MCP sem intervencao manual.
    let exec = ayrola_kernel::sandbox::SandboxExecutor::new(
        ayrola_kernel::sandbox::SandboxConfig::default(),
    );
    let res = exec.run(command);

    Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text: format!("exit {} | stdout: {} | stderr: {}", res.exit_code, res.stdout.trim(), res.stderr.trim()) }],
        is_error: Some(res.exit_code != 0),
    })
}


fn handle_remember(args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("memory.note");
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() {
        return anyhow::Ok(ToolResult {
            content: vec![ContentBlock { kind: "text".into(), text: "missing 'text' argument".into() }],
            is_error: Some(true),
        });
    }
    let payload = args.get("payload").cloned().unwrap_or(serde_json::json!({}));
    let payload = match payload {
        serde_json::Value::String(s) => {
            // Aceita JSON string, parseia.
            serde_json::from_str(&s).unwrap_or(serde_json::json!({"value": s}))
        }
        p => p,
    };

    let path = std::env::var("AYROLA_EVENT_STORE")
        .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
    let mut idx = ayrola_kernel::memory::MemoryIndex::open(&path)
        .map_err(|e| anyhow::anyhow!("failed to open memory index: {}", e))?;
    let event = idx.remember(kind, text, payload)
        .map_err(|e| anyhow::anyhow!("remember failed: {}", e))?;

    Ok(ToolResult {
        content: vec![ContentBlock {
            kind: "text".into(),
            text: format!("remembered: seq={} kind={}", event.seq, event.kind),
        }],
        is_error: None,
    })
}

fn handle_recall(args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    if query.is_empty() {
        return anyhow::Ok(ToolResult {
            content: vec![ContentBlock { kind: "text".into(), text: "missing 'query' argument".into() }],
            is_error: Some(true),
        });
    }
    let top_k = args.get("top_k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    let path = std::env::var("AYROLA_EVENT_STORE")
        .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
    let idx = ayrola_kernel::memory::MemoryIndex::open(&path)
        .map_err(|e| anyhow::anyhow!("failed to open memory index: {}", e))?;
    let results = idx.recall(query, top_k)
        .map_err(|e| anyhow::anyhow!("recall failed: {}", e))?;

    if results.is_empty() {
        return Ok(ToolResult {
            content: vec![ContentBlock { kind: "text".into(), text: "(no results)".into() }],
            is_error: None,
        });
    }

    let mut out = String::new();
    for r in results {
        out.push_str(&format!("[score={:.3}] {}: {}\n", r.score, r.event.kind, r.event.payload));
    }

    Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text: out.trim_end().to_string() }],
        is_error: None,
    })
}

fn handle_tool(name: &str, arguments: &serde_json::Value) -> anyhow::Result<ToolResult> {
    match name {
        "decide" => handle_decide(arguments),
        "shadow" => handle_shadow(arguments),
        "bench" => handle_bench(arguments),
        "health" => handle_health(arguments),
        "read" => handle_read(arguments),
        "grep" => handle_grep(arguments),
        "webfetch" => handle_webfetch(arguments),
        "run" => handle_run(arguments),
        "remember" => handle_remember(arguments),
        "recall" => handle_recall(arguments),
        _ => anyhow::Ok(ToolResult {
            content: vec![ContentBlock { kind: "text".into(), text: format!("unknown tool: {}", name) }],
            is_error: Some(true),
        }),
    }
}

fn handle_decide(args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("");
    let qtype = args.get("qtype").and_then(|v| v.as_str()).unwrap_or("yesno");
    let use_llm = args.get("llm").and_then(|v| v.as_bool()).unwrap_or(false);

    if question.is_empty() {
        return anyhow::Ok(ToolResult {
            content: vec![ContentBlock { kind: "text".into(), text: "missing `question`".into() }],
            is_error: Some(true),
        });
    }

    let qt = match qtype {
        "choice" => QuestionType::Choice,
        "score" => QuestionType::Score,
        _ => QuestionType::YesNo,
    };

    let cfg = ConfigLoader::load_or_default("config/default.yaml")
        .unwrap_or_else(|_| ayrola_kernel::config::KernelConfig::default());
    let mut engine = if use_llm {
        DecisionEngine::with_llm()
    } else {
        DecisionEngine::from_config(&cfg)
    };

    let answer = engine.ask(qt, question);
    let text = match answer {
        ayrola_kernel::decision::Answer::YesNo { yes, confidence } => {
            format!("Yes: {} | Confidence: {:.2}", yes, confidence)
        }
        ayrola_kernel::decision::Answer::Choice { index, label, confidence } => {
            format!("Choice: {} | Label: {} | Confidence: {:.2}", index, label, confidence)
        }
        ayrola_kernel::decision::Answer::Score { value, max, confidence } => {
            format!("Score: {}/{} | Confidence: {:.2}", value, max, confidence)
        }
    };

    anyhow::Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text }],
        is_error: None,
    })
}

fn handle_shadow(_args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    use ayrola_kernel::shadow::{default_golden_set, LlmShadowRunner};
    let gs = default_golden_set();
    let runner = LlmShadowRunner::new(gs);
    let report = runner.execute("mcp-shadow");
    let text = format!(
        "Shadow: {}/{} passed (promoted: {})",
        report.passed, report.total, report.promoted
    );
    anyhow::Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text }],
        is_error: if report.promoted { None } else { Some(true) },
    })
}

fn handle_bench(_args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    let suite = default_suite();
    let sb = ayrola_kernel::bench::run_suite(&suite);
    anyhow::Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text: sb.summary() }],
        is_error: None,
    })
}

fn handle_health(_args: &serde_json::Value) -> anyhow::Result<ToolResult> {
    init_tracing();
    let event_path = std::env::var("AYROLA_EVENT_STORE")
        .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
    let report = ayrola_kernel::obs::health_check(&event_path);
    anyhow::Ok(ToolResult {
        content: vec![ContentBlock { kind: "text".into(), text: report.render() }],
        is_error: if report.all_healthy() { None } else { Some(true) },
    })
}

/// Processa uma requisicao JSON-RPC e devolve a resposta.
fn process_request(req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => {
            JsonRpcResponse::ok(req.id, serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "ayrola-agent", "version": "0.1.0" }
            }))
        }
        "tools/list" => {
            let tools = vec![
                serde_json::json!({
                    "name": "decide",
                    "description": "Decision layer: ask a yesno/choice/score question (tier0/tier1/llm)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "question": { "type": "string" },
                            "qtype": { "type": "string", "enum": ["yesno","choice","score"] },
                            "llm": { "type": "boolean" }
                        },
                        "required": ["question"]
                    }
                }),
                serde_json::json!({
                    "name": "shadow",
                    "description": "Run golden set against real 9Router LLM",
                    "inputSchema": { "type": "object", "properties": {} }
                }),
                serde_json::json!({
                    "name": "bench",
                    "description": "Run benchmark suite (Ayrola vs baseline)",
                    "inputSchema": { "type": "object", "properties": {} }
                }),
                serde_json::json!({
                    "name": "health",
                    "description": "Health check: 9Router, event store, metrics",
                    "inputSchema": { "type": "object", "properties": {} }
                }),
                serde_json::json!({
                    "name": "read",
                    "description": "Read a local file path",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                }),
                serde_json::json!({
                    "name": "grep",
                    "description": "Search regex pattern in a file",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "pattern": { "type": "string" }
                        },
                        "required": ["path", "pattern"]
                    }
                }),
                serde_json::json!({
                    "name": "webfetch",
                    "description": "Fetch URL content via curl",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "url": { "type": "string" } },
                        "required": ["url"]
                    }
                }),
                serde_json::json!({
                    "name": "run",
                    "description": "Run allowlisted shell command in sandbox (cat/ls/grep/find/wc/echo/sleep/true)",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "command": { "type": "string" } },
                        "required": ["command"]
                    }
                }),
                serde_json::json!({
                    "name": "remember",
                    "description": "Store a memory with free text for semantic search (TF-IDF)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "description": "Event kind, e.g. decision.made" },
                            "text": { "type": "string", "description": "Free text used for semantic recall" },
                            "payload": { "type": "object", "description": "Structured data" }
                        },
                        "required": ["text"]
                    }
                }),
                serde_json::json!({
                    "name": "recall",
                    "description": "Search past memories by semantic similarity (TF-IDF cosine)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "top_k": { "type": "integer", "description": "Number of results (default 5)" }
                        },
                        "required": ["query"]
                    }
                }),
            ];
            JsonRpcResponse::ok(req.id, serde_json::json!({ "tools": tools }))
        }
        "tools/call" => {
            let call: ToolCall = match serde_json::from_value(req.params) {
                Ok(c) => c,
                Err(e) => {
                    return JsonRpcResponse::err(req.id, -32600, format!("invalid params: {e}"))
                }
            };
            match handle_tool(&call.name, &call.arguments) {
                Ok(res) => {
                    let result = serde_json::json!({
                        "content": res.content,
                        "isError": res.is_error.unwrap_or(false)
                    });
                    JsonRpcResponse::ok(req.id, result)
                }
                Err(e) => JsonRpcResponse::err(req.id, -32603, format!("tool error: {e}")),
            }
        }
        "shutdown" => {
            exit(0);
        }
        _ => JsonRpcResponse::err(req.id, -32601, format!("method not found: {}", req.method)),
    }
}

fn run_stdio() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let req_id = req.id.clone();
        let resp = process_request(req);
        let json = match serde_json::to_string(&resp) {
            Ok(j) => j,
            Err(e) => {
                let err = JsonRpcResponse::err(
                    req_id,
                    -32603,
                    format!("response serialization failed: {}", e),
                );
                serde_json::to_string(&err).unwrap_or_else(|_| {
                    "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"serialize failed\"}}"
                        .to_string()
                })
            }
        };
        if !json.is_empty() {
            writeln!(stdout, "{}", json).ok();
            let _ = stdout.flush();
        }
    }
}

fn main() {
    init_tracing();

    // Check for --http flag
    let args: Vec<String> = std::env::args().collect();
    let http_port = args.iter().position(|a| a == "--http")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse::<u16>().ok());

    match http_port {
        Some(port) => run_http(port),
        None => run_stdio(),
    }
}

/// TCP mode: serve MCP over raw TCP (JSON-RPC line-delimited, not HTTP).
fn run_http(port: u16) {
    // Bind to localhost only — remote access would expose run/read/grep/webfetch
    // without any authentication, allowing arbitrary command execution.
    let addr = format!("127.0.0.1:{}", port);
    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    println!("Ayrola MCP TCP on {}", addr);
    for stream in listener.incoming() {
        let Ok(s) = stream else { continue };
        let reader = BufReader::new(&s);
        let mut lines = reader.lines();
        let Some(Ok(line)) = lines.next() else { continue };
        if line.trim().is_empty() {
            continue;
        }
        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let req_id = req.id.clone();
        let resp = process_request(req);
        let json = match serde_json::to_string(&resp) {
            Ok(j) => j,
            Err(e) => {
                // Serialization failure must surface as a JSON-RPC error, not silence.
                let err = JsonRpcResponse::err(
                    req_id,
                    -32603,
                    format!("response serialization failed: {}", e),
                );
                serde_json::to_string(&err).unwrap_or_else(|_| {
                    "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"serialize failed\"}}"
                        .to_string()
                })
            }
        };
        let mut s = s;
        let _ = writeln!(s, "{}", json);
        let _ = s.flush();
    }
}
