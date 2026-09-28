//! Carregamento e validacao de configuracao YAML.

use std::path::PathBuf;
use crate::config::{ConfigError, KernelConfig};

/// Carrega configuracao de um arquivo YAML.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Carrega e parseia o arquivo de configuracao.
    pub fn load(path: impl Into<PathBuf>) -> Result<KernelConfig, ConfigError> {
        let path = path.into();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ConfigError::Io { source: e })?;
        let config: KernelConfig = serde_yaml::from_str(&content)
            .map_err(|e| ConfigError::YamlParse { source: e })?;
        config.validate()?;
        Ok(config)
    }

    /// Carrega configuracao padrao (sem arquivo).
    pub fn load_default() -> KernelConfig {
        KernelConfig::default()
    }

    /// Tenta carregar de um path; retorna default se nao existir.
    pub fn load_or_default(path: impl Into<PathBuf>) -> KernelConfig {
        Self::load(path).unwrap_or_default()
    }

    /// Salva configuracao em arquivo YAML.
    pub fn save(config: &KernelConfig, path: impl Into<PathBuf>) -> Result<(), ConfigError> {
        let yaml = serde_yaml::to_string(config)
            .map_err(|e| ConfigError::YamlParse { source: e })?;
        std::fs::write(path.into(), yaml)
            .map_err(|e| ConfigError::Io { source: e })?;
        Ok(())
    }
}
