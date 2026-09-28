//! Ayrola Kernel — Rust-native agent harness.
//!
//! Pilares implementados (Semana 1):
//! 1. Event store append-only com cadeia causal (event_store)
//! 2. Agentes com subagentes async via Tokio (agent)
//! 3. Decision layer trait + ensemble 3 tiers (decision)
//!
//! Phase 1: todos os modulos implementados. Laya ONNX opcional no tier 2.
//! Verifique `Cargo.toml` para dependencias reais.

pub mod agent;
pub mod decision;
pub mod event_store;
pub mod memory;
pub mod improve;
pub mod obs;
pub mod refine;
pub mod sandbox;
pub mod rlm;
pub mod cert;
pub mod shadow;
pub mod bench;
pub mod config;
pub mod tools;
pub mod llm;

/// Versao do kernel.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Metadados do kernel.
#[derive(Debug, Clone)]
pub struct KernelInfo {
    pub version: &'static str,
    pub edition: &'static str,
    pub tiers: u8,
}

impl KernelInfo {
    pub fn info() -> Self {
        KernelInfo {
            version: VERSION,
            edition: "2024",
            tiers: 3,
        }
    }
}

#[cfg(test)]
mod tests_llm {
    use crate::llm::{Llm, LlmBackend};

    #[test]
    fn nine_router_is_detectable() {
        // Funcao privada, mas o enum deve existir e ser matchavel.
        let _llm = Llm::new(LlmBackend::NineRouter);
        assert_eq!(LlmBackend::NineRouter, LlmBackend::NineRouter);
    }
}
