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

use std::net::TcpListener;
use std::io::{BufRead, BufReader, Write};
use std::process::exit;

use ayrola_kernel::{
    bench::default_suite,
    decision::{DecisionEngine, QuestionType},
    obs::init_tracing,
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
fn handle_tool(name: &str, arguments: &serde_json::Value) -> anyhow::Result<ToolResult> {
    match name {
        "decide" => handle_decide(arguments),
        "shadow" => handle_shadow(arguments),
        "bench" => handle_bench(arguments),
        "health" => handle_health(arguments),
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

    let mut engine = if use_llm {
        DecisionEngine::with_llm()
    } else {
        DecisionEngine::new()
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
                    "description": "Decision layer: ask a yesno/choice/score question",
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

        let resp = process_request(req);
        let json = serde_json::to_string(&resp).unwrap_or_default();
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

/// HTTP mode: serve MCP over TCP (JSON-RPC line-delimited).
fn run_http(port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    println!("Ayrola MCP HTTP on {}", addr);
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let reader = BufReader::new(&s);
                let mut lines = reader.lines();
                if let Some(Ok(line)) = lines.next() {
                    if !line.trim().is_empty() {
                        let req: JsonRpcRequest = match serde_json::from_str(&line) {
                            Ok(r) => r,
                            Err(_) => continue,
                        };
                        let resp = process_request(req);
                        let json = serde_json::to_string(&resp).unwrap_or_default();
                        let mut s = s;
                        let _ = writeln!(s, "{}", json);
                        let _ = s.flush();
                    }
                }
            }
            Err(_) => {}
        }
    }
}
