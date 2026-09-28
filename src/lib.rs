//! Ayrola Kernel — Rust-native agent harness.
//!
//! Pilares implementados (Semana 1):
//! 1. Event store append-only com cadeia causal (event_store)
//! 2. Agentes com subagentes async via Tokio (agent)
//! 3. Decision layer trait + ensemble 3 tiers (decision)
//!
//! Phase 0: stubs e heuristicas. Nao usa Laya ONNX.
//! Verifique `Cargo.toml` para dependencias reais.

pub mod agent;
pub mod decision;
pub mod event_store;
pub mod memory;
pub mod refine;
pub mod sandbox;
pub mod rlm;
pub mod tools;

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
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn info_has_3_tiers() {
        let info = KernelInfo::info();
        assert_eq!(info.tiers, 3);
    }

    #[test]
    fn edition_is_2024() {
        let info = KernelInfo::info();
        assert_eq!(info.edition, "2024");
    }
}
