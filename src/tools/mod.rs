//! Tools module. Interface para tools externas (speculative reading, etc).
//!
//! Phase 0: stubs.
//! Phase 1: integracao real via MCP (read, grep, web, etc).

use serde::{Deserialize, Serialize};

/// Tipo de tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolType {
    Read,
    Grep,
    WebFetch,
    Run,
    List,
}

impl std::fmt::Display for ToolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolType::Read => write!(f, "read"),
            ToolType::Grep => write!(f, "grep"),
            ToolType::WebFetch => write!(f, "web_fetch"),
            ToolType::Run => write!(f, "run"),
            ToolType::List => write!(f, "list"),
        }
    }
}

/// Resultado de uma chamada de tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub tool: ToolType,
    pub success: bool,
    pub content: String,
    pub duration_ms: u64,
    pub cost_usd: f64,
}

impl ToolResult {
    pub fn ok(tool: ToolType, content: impl Into<String>, duration_ms: u64, cost_usd: f64) -> Self {
        ToolResult {
            tool,
            success: true,
            content: content.into(),
            duration_ms,
            cost_usd,
        }
    }

    pub fn err(tool: ToolType, error: impl Into<String>) -> Self {
        ToolResult {
            tool,
            success: false,
            content: error.into(),
            duration_ms: 0,
            cost_usd: 0.0,
        }
    }
}

/// ToolReader: leitura especulativa de arquivos.
#[derive(Debug, Clone, Default)]
pub struct ToolReader;

impl ToolReader {
    pub fn new() -> Self {
        ToolReader
    }

    /// Le um arquivo. Stub: retorna erro se nao existir.
    pub fn read_file(&self, path: &str) -> ToolResult {
        match std::fs::read_to_string(path) {
            Ok(content) => ToolResult::ok(ToolType::Read, content, 0, 0.0),
            Err(e) => ToolResult::err(ToolType::Read, format!("error: {}", e)),
        }
    }

    /// Lista arquivos em um diretorio. Stub.
    pub fn list_dir(&self, path: &str) -> ToolResult {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect();
                ToolResult::ok(ToolType::List, names.join("\n"), 0, 0.0)
            }
            Err(e) => ToolResult::err(ToolType::List, format!("error: {}", e)),
        }
    }
}

/// SpeculativeTool: executa tools de forma especulativa e pode descartar.
#[derive(Debug, Clone, Default)]
pub struct SpeculativeTool {
    reader: ToolReader,
}

impl SpeculativeTool {
    pub fn new() -> Self {
        SpeculativeTool {
            reader: ToolReader::new(),
        }
    }

    /// Tenta uma tool. Se falhar, retorna None (descartado).
    pub fn try_read(&self, path: &str) -> Option<String> {
        let res = self.reader.read_file(path);
        if res.success {
            Some(res.content)
        } else {
            None
        }
    }

    /// Executa multiplas leituras speculativas, descartando falhas.
    pub fn try_read_batch(&self, paths: &[String]) -> Vec<(String, Option<String>)> {
        paths
            .iter()
            .map(|p| {
                let content = self.try_read(p);
                (p.clone(), content)
            })
            .collect()
    }
}

/// Registry de tools disponiveis.
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    pub available: Vec<(ToolType, String)>, // (type, description)
}

impl ToolRegistry {
    pub fn new() -> Self {
        let available = vec![
            (ToolType::Read, "Le arquivo local ou remote".to_string()),
            (ToolType::Grep, "Busca por regex em arquivos".to_string()),
            (ToolType::WebFetch, "Busca conteudo de URL via web".to_string()),
            (ToolType::Run, "Executa comando shell".to_string()),
            (ToolType::List, "Lista arquivos em diretorio".to_string()),
        ];
        ToolRegistry { available }
    }

    /// Lista todas as tools disponiveis.
    pub fn list(&self) -> &[(ToolType, String)] {
        &self.available
    }

    /// Busca tool por tipo.
    pub fn find(&self, tool_type: ToolType) -> Option<&(ToolType, String)> {
        self.available.iter().find(|(t, _)| *t == tool_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_type_display() {
        assert_eq!(ToolType::Read.to_string(), "read");
        assert_eq!(ToolType::Grep.to_string(), "grep");
        assert_eq!(ToolType::WebFetch.to_string(), "web_fetch");
        assert_eq!(ToolType::Run.to_string(), "run");
        assert_eq!(ToolType::List.to_string(), "list");
    }

    #[test]
    fn tool_result_ok_factory() {
        let r = ToolResult::ok(ToolType::Read, "content", 42, 0.01);
        assert!(r.success);
        assert_eq!(r.duration_ms, 42);
        assert!((r.cost_usd - 0.01).abs() < 1e-9);
    }

    #[test]
    fn tool_result_err_factory() {
        let r = ToolResult::err(ToolType::Grep, "not found");
        assert!(!r.success);
        assert_eq!(r.content, "not found");
    }

    #[test]
    fn tool_reader_read_existing_file() {
        let r = ToolReader::new();
        let res = r.read_file("Cargo.toml");
        assert!(res.success, "Cargo.toml deve existir");
        assert!(res.content.contains("ayrola"));
    }

    #[test]
    fn tool_reader_read_nonexistent_file() {
        let r = ToolReader::new();
        let res = r.read_file("/nonexistent/path/file.txt");
        assert!(!res.success);
    }

    #[test]
    fn speculative_tool_discards_failures() {
        let tool = SpeculativeTool::new();
        let res = tool.try_read("/nonexistent/file.txt");
        assert!(res.is_none());
    }

    #[test]
    fn speculative_tool_success_returns_content() {
        let tool = SpeculativeTool::new();
        let res = tool.try_read("Cargo.toml");
        assert!(res.is_some());
        assert!(res.unwrap().contains("ayrola"));
    }

    #[test]
    fn tool_registry_has_five_tools() {
        let reg = ToolRegistry::new();
        assert_eq!(reg.list().len(), 5);
    }

    #[test]
    fn tool_registry_find_by_type() {
        let reg = ToolRegistry::new();
        let read = reg.find(ToolType::Read);
        assert!(read.is_some());
        assert!(!read.unwrap().1.is_empty());
        assert!(reg.find(ToolType::List).is_some());
    }

    #[test]
    fn try_read_batch_returns_all_paths() {
        let tool = SpeculativeTool::new();
        let paths = vec!["Cargo.toml".to_string(), "/nonexistent".to_string()];
        let batch = tool.try_read_batch(&paths);
        assert_eq!(batch.len(), 2);
        assert!(batch[0].1.is_some(), "Cargo.toml deve existir");
        assert!(batch[1].1.is_none(), "nonexistent deve ser None");
    }

    #[test]
    fn tool_result_serializes() {
        let r = ToolResult::ok(ToolType::Run, "output", 10, 0.005);
        let json = serde_json::to_string(&r).unwrap();
        let back: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
