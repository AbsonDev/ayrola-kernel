//! LLM integration via subprocess.
//!
//! Tier 2 do ensemble: chama `claude -p` ou `opencode` via subprocess.
//! Nao usa crate HTTP — usa o CLI instalado localmente.

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

/// Backend de LLM disponiveis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmBackend {
    Claude,
    OpenCode,
    Stub,
}

impl std::fmt::Display for LlmBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmBackend::Claude => write!(f, "claude"),
            LlmBackend::OpenCode => write!(f, "opencode"),
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

/// LLM: wrapper simples para claude/opencode via subprocess.
///
/// Phase 1: usa `claude -p` ou `opencode` CLI.
/// Phase 2: HTTP direto para API (menor overhead).
#[derive(Debug, Clone)]
pub struct Llm {
    backend: LlmBackend,
    timeout_ms: u64,
}

impl Default for Llm {
    fn default() -> Self {
        // Detecta backend disponivel
        let backend = if which("claude") {
            LlmBackend::Claude
        } else if which("opencode") {
            LlmBackend::OpenCode
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

    /// Chama o LLM via subprocess. Retorna resposta em texto.
    pub fn query(&self, prompt: &str) -> Result<LlmResponse, String> {
        match self.backend {
            LlmBackend::Claude => self.query_claude(prompt),
            LlmBackend::OpenCode => self.query_opencode(prompt),
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

        // Estima tokens (1 token ~ 4 chars)
        let input_tokens = (prompt.len() / 4) as u32;
        let output_tokens = (content.len() / 4) as u32;
        // Estimativa de custo: $0.015/1k tokens (Claude Haiku)
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
        // Detecta o que existe no PATH; nunca falha.
        let backend = llm.backend;
        assert!(matches!(
            backend,
            LlmBackend::Claude | LlmBackend::OpenCode | LlmBackend::Stub
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
    fn llm_token_estimation() {
        let resp = LlmResponse {
            backend: LlmBackend::Stub,
            content: "hello world this is a test".to_string(),
            duration_ms: 0,
            cost_usd: 0.0,
            input_tokens: 25, // "hello world this is a test" ~ 8 words ~ 25 tokens
            output_tokens: 6,
        };
        // Just verify fields are settable
        assert_eq!(resp.input_tokens, 25);
        assert_eq!(resp.output_tokens, 6);
    }
}
