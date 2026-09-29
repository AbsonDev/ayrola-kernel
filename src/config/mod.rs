//! Config module — YAML-based kernel configuration with validation.
//!
//! Carrega configuracao de `config/default.yaml` (ou path customizado).
//! Campos ausentes usam defaults seguros.

pub mod error;
pub mod loader;

pub use error::ConfigError;
pub use loader::ConfigLoader;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Configuracao principal do kernel Ayrola.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KernelConfig {
    pub kernel: KernelSection,
    pub decision: DecisionSection,
    pub agent: AgentSection,
    pub logging: LoggingSection,
}

impl KernelConfig {
    /// Valida a configuracao.
    pub fn validate(&self) -> Result<(), crate::config::ConfigError> {
        if self.kernel.name.trim().is_empty() {
            return Err(ConfigError::missing("kernel.name"));
        }
        if self.kernel.version.trim().is_empty() {
            return Err(ConfigError::missing("kernel.version"));
        }
        let t = self.decision.t2_confidence_threshold;
        if !t.is_finite() || t < 0.0 || t > 1.0 {
            return Err(ConfigError::invalid(
                "decision.t2_confidence_threshold",
                "must be a finite number between 0.0 and 1.0",
            ));
        }
        if self.agent.max_subagents == 0 {
            return Err(ConfigError::invalid("agent.max_subagents", "must be > 0"));
        }

        // ensemble_mode must be one of the recognized modes.
        match self.decision.ensemble_mode.as_str() {
            "three_tier" | "two_tier" | "tier1_only" => {}
            _ => {
                return Err(ConfigError::invalid(
                    "decision.ensemble_mode",
                    format!(
                        "must be one of: three_tier, two_tier, tier1_only (got: {})",
                        self.decision.ensemble_mode
                    ),
                ));
            }
        }

        Ok(())
    }
}

/// Secao geral do kernel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelSection {
    pub name: String,
    pub version: String,
    pub edition: String,
}

impl Default for KernelSection {
    fn default() -> Self {
        KernelSection {
            name: "ayrola-kernel".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            edition: "2024".to_string(),
        }
    }
}

/// Secao de configuracao do decision layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionSection {
    pub t0_cache_enabled: bool,
    pub t1_prefilter_enabled: bool,
    pub t2_confidence_threshold: f64,
    pub ensemble_mode: String,
}

impl Default for DecisionSection {
    fn default() -> Self {
        DecisionSection {
            t0_cache_enabled: true,
            t1_prefilter_enabled: true,
            t2_confidence_threshold: 0.75,
            ensemble_mode: "three_tier".to_string(),
        }
    }
}

/// Secao de configuracao de agentes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSection {
    pub max_subagents: u32,
    pub default_timeout_ms: u64,
    pub retry_count: u32,
    pub env_vars: BTreeMap<String, String>,
}

impl Default for AgentSection {
    fn default() -> Self {
        AgentSection {
            max_subagents: 10,
            default_timeout_ms: 30_000,
            retry_count: 2,
            env_vars: BTreeMap::new(),
        }
    }
}

/// Secao de logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSection {
    pub level: String,
    pub format: String,
    pub output: String,
}

impl Default for LoggingSection {
    fn default() -> Self {
        LoggingSection {
            level: "info".to_string(),
            format: "json".to_string(),
            output: "stderr".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates() {
        let cfg = KernelConfig::default();
        assert!(cfg.validate().is_ok(), "default deve ser valido");
    }

    #[test]
    fn empty_name_rejected() {
        let mut cfg = KernelConfig::default();
        cfg.kernel.name = "".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn empty_version_rejected() {
        let mut cfg = KernelConfig::default();
        cfg.kernel.version = "".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn confidence_threshold_out_of_range() {
        let mut cfg = KernelConfig::default();
        cfg.decision.t2_confidence_threshold = 1.5;
        assert!(cfg.validate().is_err());
        cfg.decision.t2_confidence_threshold = -0.1;
        assert!(cfg.validate().is_err());
        cfg.decision.t2_confidence_threshold = 0.5;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn zero_max_subagents_rejected() {
        let mut cfg = KernelConfig::default();
        cfg.agent.max_subagents = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_serializes_to_yaml() {
        let cfg = KernelConfig::default();
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let back: KernelConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(cfg.kernel.name, back.kernel.name);
        assert_eq!(cfg.decision.t2_confidence_threshold, back.decision.t2_confidence_threshold);
    }

    #[test]
    fn nan_confidence_threshold_rejected() {
        let mut cfg = KernelConfig::default();
        cfg.decision.t2_confidence_threshold = f64::NAN;
        // NaN must be rejected — comparisons with NaN are always false,
        // so the current < 0.0 || > 1.0 check would pass NaN.
        assert!(cfg.validate().is_err(), "NaN confidence threshold must be rejected");
    }

    #[test]
    fn infinity_confidence_threshold_rejected() {
        let mut cfg = KernelConfig::default();
        cfg.decision.t2_confidence_threshold = f64::INFINITY;
        assert!(cfg.validate().is_err(), "INFINITY must be rejected");
    }

    #[test]
    fn neg_infinity_confidence_threshold_rejected() {
        let mut cfg = KernelConfig::default();
        cfg.decision.t2_confidence_threshold = f64::NEG_INFINITY;
        assert!(cfg.validate().is_err(), "NEG_INFINITY must be rejected");
    }

    #[test]
    fn invalid_ensemble_mode_rejected() {
        let mut cfg = KernelConfig::default();
        cfg.decision.ensemble_mode = "two_tire".to_string();
        assert!(cfg.validate().is_err(), "typo in ensemble_mode must be rejected");
        cfg.decision.ensemble_mode = "three_tier".to_string();
        assert!(cfg.validate().is_ok(), "three_tier must be accepted");
    }
}
