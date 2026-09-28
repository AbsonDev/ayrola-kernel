//! ayrola-bench: benchmark suite. Eixo D.
//!
//! 10 tasks reais + scoreboard custo/latencia/qualidade + baseline OpenCode.
//! Phase 0: stubs + estrutura de dados.

use serde::{Deserialize, Serialize};

/// Tipo de task de benchmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BenchTaskType {
    CodeSearch,
    CodeWrite,
    BugFix,
    PrReview,
    Debug,
    DocWrite,
    Architecture,
    Refactoring,
    TestWrite,
    MultiAgent,
}

impl std::fmt::Display for BenchTaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchTaskType::CodeSearch => write!(f, "code_search"),
            BenchTaskType::CodeWrite => write!(f, "code_write"),
            BenchTaskType::BugFix => write!(f, "bug_fix"),
            BenchTaskType::PrReview => write!(f, "pr_review"),
            BenchTaskType::Debug => write!(f, "debug"),
            BenchTaskType::DocWrite => write!(f, "doc_write"),
            BenchTaskType::Architecture => write!(f, "architecture"),
            BenchTaskType::Refactoring => write!(f, "refactoring"),
            BenchTaskType::TestWrite => write!(f, "test_write"),
            BenchTaskType::MultiAgent => write!(f, "multi_agent"),
        }
    }
}

/// Uma task de benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchTask {
    pub id: String,
    pub name: String,
    pub task_type: BenchTaskType,
    pub prompt: String,
    pub golden_answer: Option<String>,
    pub timeout_secs: u64,
}

impl BenchTask {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        task_type: BenchTaskType,
        prompt: impl Into<String>,
    ) -> Self {
        BenchTask {
            id: id.into(),
            name: name.into(),
            task_type,
            prompt: prompt.into(),
            golden_answer: None,
            timeout_secs: 30,
        }
    }
}

/// Resultado de execucao de uma task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub task_id: String,
    pub task_name: String,
    pub task_type: BenchTaskType,
    pub success: bool,
    pub duration_ms: u64,
    pub cost_usd: f64,
    pub output: String,
    pub quality_score: f64,
    pub executed_ms: u64,
}

impl BenchResult {
    pub fn stub(task: &BenchTask, success: bool, duration_ms: u64) -> Self {
        BenchResult {
            task_id: task.id.clone(),
            task_name: task.name.clone(),
            task_type: task.task_type,
            success,
            duration_ms,
            cost_usd: 0.0,
            output: String::new(),
            quality_score: if success { 1.0 } else { 0.0 },
            executed_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}

/// Scoreboard de resultados.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scoreboard {
    pub results: Vec<BenchResult>,
}

impl Scoreboard {
    pub fn new() -> Self {
        Scoreboard::default()
    }

    pub fn add(&mut self, result: BenchResult) {
        self.results.push(result);
    }

    pub fn resolve_rate(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let ok = self.results.iter().filter(|r| r.success).count();
        ok as f64 / self.results.len() as f64
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        self.results.iter().map(|r| r.duration_ms).sum::<u64>() as f64 / self.results.len() as f64
    }

    pub fn total_cost(&self) -> f64 {
        self.results.iter().map(|r| r.cost_usd).sum()
    }

    pub fn avg_quality(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        self.results.iter().map(|r| r.quality_score).sum::<f64>() / self.results.len() as f64
    }

    pub fn save_json(&self, path: impl Into<String>) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path.into(), json).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Suite padrao de 10 tasks. Eixo D.
pub fn default_suite() -> Vec<BenchTask> {
    vec![
        BenchTask::new("t1", "Code search in Rust", BenchTaskType::CodeSearch, "Find the spawn_subagent function in the agent module"),
        BenchTask::new("t2", "Write Rust function", BenchTaskType::CodeWrite, "Write a function parse_json(s: &str) -> Option<Value>"),
        BenchTask::new("t3", "Fix borrow checker error", BenchTaskType::BugFix, "Fix: borrow of moved value: task"),
        BenchTask::new("t4", "Review PR diff", BenchTaskType::PrReview, "Review this diff for clippy issues"),
        BenchTask::new("t5", "Debug compilation error", BenchTaskType::Debug, "Why does cargo build fail with E0382?"),
        BenchTask::new("t6", "Write API docs", BenchTaskType::DocWrite, "Document the Snapshot struct"),
        BenchTask::new("t7", "Architecture decision", BenchTaskType::Architecture, "Should we use Tokio or asupersync?"),
        BenchTask::new("t8", "Refactor function", BenchTaskType::Refactoring, "Extract the spawn logic into a separate method"),
        BenchTask::new("t9", "Write test", BenchTaskType::TestWrite, "Add a test for error handling"),
        BenchTask::new("t10", "Multi-agent coordination", BenchTaskType::MultiAgent, "Spawn 3 agents for parallel tasks"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_suite_has_10_tasks() {
        let suite = default_suite();
        assert_eq!(suite.len(), 10);
    }

    #[test]
    fn bench_task_type_display() {
        assert_eq!(BenchTaskType::CodeSearch.to_string(), "code_search");
        assert_eq!(BenchTaskType::MultiAgent.to_string(), "multi_agent");
    }

    #[test]
    fn bench_result_stub_success() {
        let task = default_suite()[0].clone();
        let res = BenchResult::stub(&task, true, 150);
        assert!(res.success);
        assert_eq!(res.quality_score, 1.0);
        assert_eq!(res.duration_ms, 150);
    }

    #[test]
    fn bench_result_stub_failure() {
        let task = default_suite()[0].clone();
        let res = BenchResult::stub(&task, false, 500);
        assert!(!res.success);
        assert_eq!(res.quality_score, 0.0);
    }

    #[test]
    fn scoreboard_resolve_rate_all_pass() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        for _ in 0..5 {
            sb.add(BenchResult::stub(&task, true, 100));
        }
        assert!((sb.resolve_rate() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn scoreboard_resolve_rate_partial() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        sb.add(BenchResult::stub(&task, true, 100));
        sb.add(BenchResult::stub(&task, false, 200));
        assert!((sb.resolve_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn scoreboard_empty_returns_zero() {
        let sb = Scoreboard::new();
        assert_eq!(sb.resolve_rate(), 0.0);
        assert_eq!(sb.avg_latency_ms(), 0.0);
    }

    #[test]
    fn scoreboard_avg_latency() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        sb.add(BenchResult::stub(&task, true, 100));
        sb.add(BenchResult::stub(&task, true, 200));
        assert!((sb.avg_latency_ms() - 150.0).abs() < 1e-9);
    }

    #[test]
    fn scoreboard_total_cost_accumulates() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        let mut r = BenchResult::stub(&task, true, 100);
        r.cost_usd = 0.01;
        sb.add(r);
        let mut r2 = BenchResult::stub(&task, true, 100);
        r2.cost_usd = 0.02;
        sb.add(r2);
        assert!((sb.total_cost() - 0.03).abs() < 1e-9);
    }

    #[test]
    fn scoreboard_avg_quality() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        let mut r = BenchResult::stub(&task, true, 100);
        r.quality_score = 0.8;
        sb.add(r);
        let mut r2 = BenchResult::stub(&task, true, 100);
        r2.quality_score = 0.6;
        sb.add(r2);
        assert!((sb.avg_quality() - 0.7).abs() < 1e-9);
    }

    #[test]
    fn bench_task_creation() {
        let t = BenchTask::new("x1", "test", BenchTaskType::Debug, "fix it");
        assert_eq!(t.id, "x1");
        assert_eq!(t.task_type, BenchTaskType::Debug);
        assert_eq!(t.timeout_secs, 30);
    }

    #[test]
    fn scoreboard_serializes_to_json() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        sb.add(BenchResult::stub(&task, true, 100));
        let json = serde_json::to_string_pretty(&sb).unwrap();
        assert!(json.contains("task_id"));
        assert!(json.contains("resolve_rate") == false); // method, not field
    }
}
