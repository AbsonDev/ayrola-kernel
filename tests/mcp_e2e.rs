
//! Integration tests for the MCP server (ayrok-agent).
//!
//! Tests JSON-RPC stdio mode — the primary protocol.
//! HTTP mode tested via manual smoke test in CI.

use std::io::Write;
use std::process::Command;

/// Sends a JSON-RPC request to the MCP server via stdin and captures stdout.
fn mcp_call(input: &str) -> String {
    let mut child = Command::new("./target/release/ayrola-agent")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn ayrola-agent");

    {
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        stdin.write_all((input.to_string() + "\n").as_bytes()).ok();
    }

    let output = child.wait_with_output().expect("failed to read stdout");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
#[ignore = "requires release build"]
fn mcp_initialize_returns_protocol() {
    let resp = mcp_call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}}}"#);
    assert!(resp.contains("2.0"), "resp: {}", resp);
    assert!(resp.contains("ayrola-agent"));
}

#[test]
#[ignore = "requires release build"]
fn mcp_tools_list_returns_eight() {
    let resp = mcp_call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#);
    assert!(resp.contains("decide"), "resp: {}", resp);
    assert!(resp.contains("run"), "resp: {}", resp);
}

#[test]
#[ignore = "requires release build"]
fn mcp_decide_returns_confidence() {
    let resp = mcp_call(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"decide","arguments":{"question":"Is water wet?","qtype":"yesno","llm":false}}}"#);
    assert!(resp.contains("Yes:"), "resp: {}", resp);
}
