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

    /// Carrega config; usa default apenas se o arquivo nao existir.
    ///
    /// Erros de parsing/validacao sao propagados: um default silencioso
    /// esconderia config invalida e o kernel rodaria com params errados.
    pub fn load_or_default(path: impl Into<PathBuf>) -> Result<KernelConfig, ConfigError> {
        match Self::load(path) {
            Ok(cfg) => Ok(cfg),
            Err(ConfigError::Io { .. }) => Ok(KernelConfig::default()),
            Err(e) => Err(e),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn tmp_path(tag: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("ayrola_cfg_{}_{}.yaml", tag, uuid::Uuid::new_v4()));
        p
    }

    #[test]
    fn load_default_returns_valid_defaults() {
        let cfg = ConfigLoader::load_default();
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.kernel.name, "ayrola-kernel");
    }

    #[test]
    fn save_then_load_roundtrips() {
        let p = tmp_path("roundtrip");
        let cfg = KernelConfig::default();
        ConfigLoader::save(&cfg, &p).expect("save should succeed");
        let back = ConfigLoader::load(&p).expect("load should succeed");
        assert_eq!(back.kernel.name, cfg.kernel.name);
        assert_eq!(back.kernel.version, cfg.kernel.version);
        assert_eq!(back.decision.t2_confidence_threshold, cfg.decision.t2_confidence_threshold);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_missing_file_returns_io_error() {
        let p = tmp_path("missing");
        let result = ConfigLoader::load(&p);
        assert!(result.is_err(), "arquivo inexistente deve falhar");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_invalid_yaml_returns_parse_error() {
        let p = tmp_path("badyaml");
        std::fs::write(&p, "kernel: [this is not valid yaml {{{").expect("write should succeed");
        let result = ConfigLoader::load(&p);
        assert!(result.is_err(), "YAML invalido deve falhar");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_or_default_missing_file_uses_default() {
        let p = tmp_path("or_default_missing");
        let cfg = ConfigLoader::load_or_default(&p).expect("arquivo ausente cai para default");
        assert_eq!(cfg.kernel.name, "ayrola-kernel", "arquivo ausente deve usar default");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_or_default_propagates_invalid_yaml() {
        // REGRESSION: antes, load_or_default usava unwrap_or_default() e
        // mascarava erros de parsing/validacao, retornando defaults
        // silenciosamente. Agora so arquivo ausente cai para default.
        let p = tmp_path("or_default_badyaml");
        std::fs::write(&p, "kernel: [broken yaml {{{").expect("write should succeed");
        let result = ConfigLoader::load_or_default(&p);
        assert!(
            result.is_err(),
            "YAML invalido deve propagar erro, nao usar default silencioso"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_or_default_propagates_validation_error() {
        // REGRESSION: config que parses mas viola validate() tambem deve
        // propagar, senao o kernel roda com thresholds invalidos.
        let p = tmp_path("or_default_invalid_cfg");
        let bad = "kernel:\n  name: \"\"\n  version: \"0.1.0\"\n  edition: \"2024\"\n";
        std::fs::write(&p, bad).expect("write should succeed");
        let result = ConfigLoader::load_or_default(&p);
        assert!(
            result.is_err(),
            "config que viola validate() deve propagar erro"
        );
        std::fs::remove_file(&p).ok();
    }
}
