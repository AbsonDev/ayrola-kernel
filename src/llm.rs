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

/// Modelo padrao do 9Router (128k ctx, reasoning, tools, vision).
const MODEL: &str = "fusion-5tier";

/// Tokens de saida pedidos por chamada. Precisa ser alto porque
/// modelos de reasoning consomem tokens antes de emitir `content`.
const TOKENS: &str = r#""max_tokens":800"#;

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
    pub fn is_9router_available() -> bool {
        use std::net::TcpStream;
        use std::time::Duration;

        let timeout = Duration::from_millis(500);
        TcpStream::connect_timeout(&"127.0.0.1:20128".parse().expect("const addr must parse"), timeout)
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
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("opencode failed: {}", stderr));
        }

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
    /// Usa modelo fusion-5tier (128k context, reasoning, tools, vision).
    /// API key lida de ~/.9router/db/data.sqlite (read-only sqlite3).
    /// Custo: $0 (free tier).
    fn query_9router(&self, prompt: &str) -> Result<LlmResponse, String> {
        let start = Instant::now();
        let db_path = std::env::var("HOME")
            .map(|h| format!("{}/.9router/db/data.sqlite", h))
            .unwrap_or_else(|_| "~/.9router/db/data.sqlite".to_string());

        let key = Self::read_9router_key(&db_path)?;

        let body = format!(
            r#"{{"model":"{MODEL}","messages":[{{"role":"user","content":{}}}],{TOKENS}}}"#,
            serde_json::to_string(prompt).map_err(|e| e.to_string())?
        );

        let output = Command::new("curl")
            .arg("-s")
            .arg("-f")  // fail on HTTP >= 400 so error responses surface as Err
            .arg("--max-time")
            .arg((self.timeout_ms / 1000).max(1).to_string())
            .arg("-X")
            .arg("POST")
            .arg("http://localhost:20128/v1/chat/completions")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-H")
            .arg(format!("Authorization: Bearer {key}"))
            .arg("-d")
            .arg(&body)
            .output()
            .map_err(|e| format!("curl failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("curl to 9Router failed: {}", stderr));
        }
        let response = String::from_utf8_lossy(&output.stdout);
        let duration = start.elapsed();

        // 9Router devolve 2 formatos: JSON unico + "data: [DONE]", ou SSE com chunks "data: {...}".
        // O campo de texto varia: content, reasoning, reasoning_content.
        let (content, input_tokens, output_tokens) = Self::parse_9router(&response)?;

        Ok(LlmResponse {
            backend: LlmBackend::NineRouter,
            content,
            duration_ms: duration.as_millis() as u64,
            cost_usd: 0.0, // free tier
            input_tokens,
            output_tokens,
        })
    }

    #[allow(clippy::collapsible_if)]
    /// Extrai texto e tokens de qualquer formato de resposta do 9Router.
    ///
    /// Formatos aceitos:
    /// 1. `{...json...}data: [DONE]` — resposta unica
    /// 2. `data: {...json...}\ndata: {...}\ndata: [DONE]` — SSE com chunks
    ///
    /// Campos de texto attemptados, em ordem: `content`, `reasoning_content`, `reasoning`.
    fn parse_9router(response: &str) -> Result<(String, u32, u32), String> {
        /// Extrai o texto de um objeto JSON de completion.
        fn extract_text(obj: &serde_json::Value, from_delta: bool) -> Result<String, String> {
            let choices = match obj.get("choices").and_then(|c| c.as_array()) {
                Some(c) if !c.is_empty() => c,
                _ => return Err("9Router response missing choices".to_string()),
            };
            let container = if from_delta {
                choices[0].get("delta")
            } else {
                choices[0].get("message")
            };
            let container = match container {
                Some(c) => c,
                None => return Err("9Router response missing message/delta".to_string()),
            };
            for field in ["content", "reasoning_content", "reasoning"] {
                if let Some(text) = container.get(field).and_then(|v| v.as_str()) {
                    if !text.trim().is_empty() {
                        return Ok(text.trim().to_string());
                    }
                }
            }
            Err("9Router response has empty content".to_string())
        }

        fn tokens(obj: &serde_json::Value) -> (u32, u32) {
            let usage = obj.get("usage");
            let in_t = usage
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32;
            let out_t = usage
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32;
            (in_t, out_t)
        }

        let trimmed = response.trim();
        if trimmed.is_empty() {
            return Err("9Router returned empty body".to_string());
        }

        let mut text = String::new();
        let (mut in_t, mut out_t) = (0u32, 0u32);
        let mut matched = false;

        // Caminho 1: SSE — cada linha comeca com "data: ".
        if trimmed.starts_with("data:") {
            // Reasoning models mandam o raciocínio em deltas "reasoning" e a
            // resposta final em deltas "content". Acumula os dois separados e
            // prefere "content" — o reasoning contem a pergunta de volta e
            // polui a deteccao de yes/no.
            let mut content_buf = String::new();
            let mut reasoning_buf = String::new();
            for line in trimmed.lines() {
                let line = line.trim();
                let Some(payload) = line.strip_prefix("data:") else {
                    continue;
                };
                let payload = payload.trim();
                if payload.is_empty() || payload == "[DONE]" {
                    continue;
                }
                let Ok(obj) = serde_json::from_str::<serde_json::Value>(payload) else {
                    continue;
                };
                matched = true;
                if let Some(err) = obj.get("error").and_then(|e| e.as_str()) {
                    return Err(format!("9Router error: {}", err));
                }
                if let Some(choices) = obj.get("choices").and_then(|c| c.as_array()) {
                    if let Some(delta) = choices.first().and_then(|ch| ch.get("delta")) {
                        if let Some(c) = delta.get("content").and_then(|v| v.as_str()) {
                            content_buf.push_str(c);
                        }
                        for field in ["reasoning_content", "reasoning"] {
                            if let Some(r) = delta.get(field).and_then(|v| v.as_str()) {
                                reasoning_buf.push_str(r);
                                break;
                            }
                        }
                    }
                }
                let (ci, co) = tokens(&obj);
                in_t = in_t.max(ci);
                out_t = out_t.max(co);
            }
            text = if content_buf.trim().is_empty() {
                reasoning_buf
            } else {
                content_buf
            };
        }

        // Caminho 2: JSON unico + "data: [DONE]" no final.
        if !matched {
            let json_part = trimmed.trim_end_matches("data: [DONE]").trim();
            let obj: serde_json::Value = serde_json::from_str(json_part)
                .map_err(|_| "9Router parse error".to_string())?;
            matched = true;
            if let Some(err) = obj.get("error").and_then(|e| e.as_str()) {
                return Err(format!("9Router error: {}", err));
            }
            text = extract_text(&obj, false)?;
            let (ci, co) = tokens(&obj);
            in_t = ci;
            out_t = co;
        }

        let _ = matched;
        let final_text = text.trim().to_string();
        if final_text.is_empty() {
            return Err("9Router returned no content".to_string());
        }
        Ok((final_text, in_t, out_t))
    }

    fn read_9router_key(db_path: &str) -> Result<String, String> {
        let output = Command::new("sqlite3")
            .arg(db_path)
            .arg("SELECT key FROM apiKeys WHERE isActive=1 LIMIT 1;")
            .output()
            .map_err(|e| format!("sqlite3 failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("sqlite3 failed: {}", stderr));
        }

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
            // 9Router not available, skipping (expected in CI)
            return;
        }
        let resp = llm.query("Say hi").unwrap();
        assert_eq!(resp.backend, LlmBackend::NineRouter);
        // Content may or may not be returned depending on 9Router response;
        // asserting the response was successfully received (not an error).
        let _ = &resp.content;
        assert_eq!(resp.cost_usd, 0.0);
    }



    #[test]
    fn parse_9router_error_payload_returns_error() {
        // Regression: HTTP error payloads (no "choices" field) must NOT
        // silently return Ok(("", ...)). A 401/500 response body should
        // propagate as an error so the caller knows the LLM call failed.
        let payload = r#"{"error":{"message":"Unauthorized","type":"invalid_request_error"}}"#;
        let result = Llm::parse_9router(payload);
        assert!(result.is_err(),
            "error payload without choices must return Err, got {:?}", result);
    }

    #[test]
    fn parse_9router_empty_choices_returns_error() {
        // Choices array present but empty = no completion generated.
        let payload = r#"{"choices":[],"usage":{"prompt_tokens":0,"completion_tokens":0}}"#;
        let result = Llm::parse_9router(payload);
        assert!(result.is_err(),
            "empty choices must return Err, got {:?}", result);
    }
}
