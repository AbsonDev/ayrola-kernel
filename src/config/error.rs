//! Custom error types para o modulo de configuracao.

use thiserror::Error;
use std::path::PathBuf;

/// Erros do modulo config.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("arquivo nao encontrado: {path}")]
    NotFound { path: PathBuf },

    #[error("erro de parsing YAML: {source}")]
    YamlParse { source: serde_yaml::Error },

    #[error("campo obrigatorio ausente: {field}")]
    MissingField { field: &'static str },

    #[error("valor invalido para {field}: {message}")]
    InvalidValue { field: &'static str, message: String },

    #[error("IO error: {source}")]
    Io {
        #[source]
        #[from]
        source: std::io::Error,
    },
}

impl ConfigError {
    pub fn missing(field: &'static str) -> Self {
        ConfigError::MissingField { field }
    }

    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        ConfigError::InvalidValue {
            field,
            message: message.into(),
        }
    }
}
