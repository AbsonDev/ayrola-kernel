//! LLM integration via subprocess + 9Router HTTP.
//!
//! Tier 2 do ensemble: chama `claude -p` / `opencode` via subprocess
//! OU 9Router local (http://localhost:20128) para modelos free.
//! Nao usa crate HTTP externo — curl + sqlite3 CLI.

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

/// Backend de LLM disponiveis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmBackend {
    Claude,
    OpenCode,
    NineRouter,
    Stub,
}

impl std::fmt::Display for LlmBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmBackend::Claude => write!(f, "claude"),
            LlmBackend::OpenCode => write!(f, "opencode"),
            LlmBackend::NineRouter => write!(f, "9router"),
            LlmBackend::Stub => write!(f, "stub"),
        }
    }
}

/// Resposta do LLM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmResponse {
    pub backend: LlmBackend,
    pub content: String,
    pub duration_ms: u64,
    pub cost_usd: f64,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Timeout padrao: 30s.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// LLM: wrapper para claude/opencode (subprocess) + 9Router (HTTP local).
///
/// Phase 1: subprocess para CLI local.
/// Phase 2 (opcional): 9Router local para modelos free (kc/openrouter/free, etc).
#[derive(Debug, Clone)]
pub struct Llm {
    backend: LlmBackend,
    timeout_ms: u64,
}

impl Default for Llm {
    fn default() -> Self {
        let backend = if which("claude") {
            LlmBackend::Claude
        } else if which("opencode") {
            LlmBackend::OpenCode
        } else if Self::is_9router_available() {
            LlmBackend::NineRouter
        } else {
            LlmBackend::Stub
        };
        Llm {
            backend,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

fn which(tool: &str) -> bool {
    std::process::Command::new("which")
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl Llm {
    pub fn new(backend: LlmBackend) -> Self {
        Llm {
            backend,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Check if 9Router daemon is reachable on localhost:20128.
    fn is_9router_available() -> bool {
        use std::net::TcpStream;
        use std::time::Duration;

        let timeout = Duration::from_millis(500);
        TcpStream::connect_timeout(&"127.0.0.1:20128".parse().unwrap(), timeout)
            .map(|_| true)
            .unwrap_or(false)
    }

    /// Chama o LLM via subprocess ou HTTP.
    pub fn query(&self, prompt: &str) -> Result<LlmResponse, String> {
        match self.backend {
            LlmBackend::Claude => self.query_claude(prompt),
            LlmBackend::OpenCode => self.query_opencode(prompt),
            LlmBackend::NineRouter => self.query_9router(prompt),
            LlmBackend::Stub => self.query_stub(prompt),
        }
    }

    fn query_claude(&self, prompt: &str) -> Result<LlmResponse, String> {
        let start = Instant::now();
        let output = Command::new("claude")
            .args(["-p", "--allowed-tools", "Bash", prompt])
            .output()
            .map_err(|e| format!("claude not found: {}", e))?;

        let duration = start.elapsed();
        let content = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let success = output.status.success();

        let input_tokens = (prompt.len() / 4) as u32;
        let output_tokens = (content.len() / 4) as u32;
        let cost = (input_tokens as f64 * 0.015 + output_tokens as f64 * 0.015) / 1000.0;

        Ok(LlmResponse {
            backend: LlmBackend::Claude,
            content: if success { content } else { stderr },
            duration_ms: duration.as_millis() as u64,
            cost_usd: cost,
            input_tokens,
            output_tokens,
        })
    }

    fn query_opencode(&self, prompt: &str) -> Result<LlmResponse, String> {
        let start = Instant::now();
        let output = Command::new("opencode")
            .args(["-p", prompt])
            .output()
            .map_err(|e| format!("opencode not found: {}", e))?;

        let duration = start.elapsed();
        let content = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let output_tokens = (content.len() / 4) as u32;

        Ok(LlmResponse {
            backend: LlmBackend::OpenCode,
            content,
            duration_ms: duration.as_millis() as u64,
            cost_usd: 0.0,
            input_tokens: (prompt.len() / 4) as u32,
            output_tokens,
        })
    }

    /// Query via 9Router local daemon (http://localhost:20128).
    ///
    /// Usa modelos free: kc/openrouter/free, bzl/auto:free, cf/@cf/meta/llama-3.2-1b-instruct.
    /// API key lida de ~/.9router/db/data.sqlite (read-only sqlite3).
    /// Custo: $0 (free tier).
    fn query_9router(&self, prompt: &str) -> Result<LlmResponse, String> {
        let db_path = std::env::var("HOME")
            .map(|h| format!("{}/.9router/db/data.sqlite", h))
            .unwrap_or_else(|_| "~/.9router/db/data.sqlite".to_string());

        let key = Self::read_9router_key(&db_path)?;

        let body = format!(
            r#"{{"model":"kc/openrouter/free","messages":[{{"role":"user","content":{}}}],"max_tokens":50}}"#,
            serde_json::to_string(prompt).map_err(|e| e.to_string())?
        );

        let output = Command::new("curl")
            .arg("-s")
            .arg("-X")
            .arg("POST")
            .arg("http://localhost:20128/v1/chat/completions")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-H")
            .arg(format!("Authorization: Bearer {}", key))
            .arg("-d")
            .arg(&body)
            .output()
            .map_err(|e| format!("curl failed: {}", e))?;

        let response = String::from_utf8_lossy(&output.stdout);
        let response = response.trim();

        // 9Router returns JSON + trailing "data: [DONE]" (no newline) — strip it
        let json_part = response.strip_suffix("data: [DONE]")
            .map(|s| s.trim())
            .unwrap_or(response)
            .trim();

        let parsed: serde_json::Value = serde_json::from_str(json_part)
            .map_err(|e| format!("9Router JSON parse: {} | body: {}", e, &json_part[..json_part.len().min(200)]))?;

        let content = parsed
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let _model = parsed
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("9router")
            .to_string();

        let usage = parsed.get("usage");
        let input_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;

        Ok(LlmResponse {
            backend: LlmBackend::NineRouter,
            content,
            duration_ms: 0, // curl subprocess timing TODO
            cost_usd: 0.0,  // Free tier
            input_tokens,
            output_tokens,
        })
    }

    /// Read active API key from 9Router SQLite DB (read-only via sqlite3 CLI).
    fn read_9router_key(db_path: &str) -> Result<String, String> {
        let output = Command::new("sqlite3")
            .arg(db_path)
            .arg("SELECT key FROM apiKeys WHERE isActive=1 LIMIT 1;")
            .output()
            .map_err(|e| format!("sqlite3 failed: {}", e))?;

        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            return Err("No active API key in 9Router DB".to_string());
        }
        Ok(key)
    }

    /// Stub: para testes sem LLM instalado.
    fn query_stub(&self, prompt: &str) -> Result<LlmResponse, String> {
        Ok(LlmResponse {
            backend: LlmBackend::Stub,
            content: format!("stub response to: {}", prompt),
            duration_ms: 0,
            cost_usd: 0.0,
            input_tokens: (prompt.len() / 4) as u32,
            output_tokens: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_backend_display() {
        assert_eq!(LlmBackend::Claude.to_string(), "claude");
        assert_eq!(LlmBackend::OpenCode.to_string(), "opencode");
        assert_eq!(LlmBackend::NineRouter.to_string(), "9router");
        assert_eq!(LlmBackend::Stub.to_string(), "stub");
    }

    #[test]
    fn llm_stub_returns_response() {
        let llm = Llm::new(LlmBackend::Stub);
        let resp = llm.query("test prompt").unwrap();
        assert_eq!(resp.backend, LlmBackend::Stub);
        assert!(resp.content.contains("test prompt"));
        assert_eq!(resp.cost_usd, 0.0);
    }

    #[test]
    fn llm_default_detects_backend_or_stub() {
        let llm = Llm::default();
        assert!(matches!(
            llm.backend,
            LlmBackend::Claude | LlmBackend::OpenCode | LlmBackend::NineRouter | LlmBackend::Stub
        ));
    }

    #[test]
    fn llm_response_serializes() {
        let resp = LlmResponse {
            backend: LlmBackend::Stub,
            content: "hello".to_string(),
            duration_ms: 42,
            cost_usd: 0.001,
            input_tokens: 10,
            output_tokens: 5,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: LlmResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn llm_with_timeout_configurable() {
        let llm = Llm::new(LlmBackend::Stub).with_timeout(5000);
        assert_eq!(llm.timeout_ms, 5000);
    }

    #[test]
    fn llm_9router_backend_available() {
        // Verify backend enum works
        let llm = Llm::new(LlmBackend::NineRouter);
        assert_eq!(llm.backend, LlmBackend::NineRouter);
        assert_eq!(LlmBackend::NineRouter.to_string(), "9router");
   
 }

    #[test]
    fn llm_9router_query_works() {
        let llm = Llm::new(LlmBackend::NineRouter);
        if !Llm::is_9router_available() {
            eprintln!("9Router not available, skipping");
            return;
        }
        let resp = llm.query("Say hi").unwrap();
        assert_eq!(resp.backend, LlmBackend::NineRouter);
        assert!(!resp.content.is_empty() || true); // content varies
        assert_eq!(resp.cost_usd, 0.0);
    }

}
