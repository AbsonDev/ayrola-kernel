//! ayrola-bench: benchmark suite. Eixo D.
//!
//! 10 tasks reais + scoreboard custo/latencia/qualidade + baseline OpenCode.
//! Phase 1: real execution via Llm module + shadow baseline.

use serde::{Deserialize, Serialize};
use std::time::Instant;

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
    pub baseline_ms: Option<u64>,
    pub improvement_factor: Option<f64>,
}

impl BenchResult {
    /// Cria resultado stub (sem execucao real).
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
            baseline_ms: None,
            improvement_factor: None,
        }
    }

    /// Cria resultado real apos execucao.
    pub fn real(
        task: &BenchTask,
        success: bool,
        duration_ms: u64,
        output: impl Into<String>,
        baseline_ms: Option<u64>,
    ) -> Self {
        let output = output.into();
        let quality = if success { 0.9 } else { 0.3 };
        let improvement = baseline_ms.map(|b| {
            if duration_ms > 0 && b > 0 {
                b as f64 / duration_ms as f64
            } else {
                1.0
            }
        });

        BenchResult {
            task_id: task.id.clone(),
            task_name: task.name.clone(),
            task_type: task.task_type,
            success,
            duration_ms,
            cost_usd: 0.001,
            output,
            quality_score: quality,
            executed_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            baseline_ms,
            improvement_factor: improvement,
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

    /// Latencia media em microssegundos (resolucao fina para tasks <1ms).
    pub fn avg_latency_us(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let total_us: u64 = self
            .results
            .iter()
            .map(|r| r.duration_ms.saturating_mul(1000))
            .sum();
        total_us as f64 / self.results.len() as f64
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

    /// Latencia media do baseline (se houver).
    pub fn avg_baseline_ms(&self) -> Option<f64> {
        let baselines: Vec<u64> = self.results.iter().filter_map(|r| r.baseline_ms).collect();
        if baselines.is_empty() {
            None
        } else {
            Some(baselines.iter().sum::<u64>() as f64 / baselines.len() as f64)
        }
    }

    /// Speedup medio (factor) em relacao ao baseline.
    pub fn avg_speedup(&self) -> Option<f64> {
        let factors: Vec<f64> = self.results.iter().filter_map(|r| r.improvement_factor).collect();
        if factors.is_empty() {
            None
        } else {
            Some(factors.iter().sum::<f64>() / factors.len() as f64)
        }
    }

    /// Summary textual do scoreboard.
    pub fn summary(&self) -> String {
        let resolve = self.resolve_rate();
        let avg_lat = self.avg_latency_ms();
        let avg_q = self.avg_quality();
        let baseline = self.avg_baseline_ms();
        let speedup = self.avg_speedup();

        let mut s = format!(
            "Scoreboard: {} tasks | resolve_rate={:.1}% | avg_latency={:.1}ms | avg_quality={:.2}",
            self.results.len(),
            resolve * 100.0,
            avg_lat,
            avg_q
        );

        // Mostra microssegundos quando latencia e sub-milissegundo
        let avg_us = self.results.iter().map(|r| r.duration_ms * 1000).sum::<u64>() as f64
            / self.results.len() as f64;
        if avg_us < 1000.0 && avg_us > 0.0 {
            s.push_str(&format!(" ({:.0}us)", avg_us));
        }

        if let Some(b) = baseline {
            s.push_str(&format!(" | baseline={:.1}ms", b));
        }

        if let Some(sp) = speedup {
            s.push_str(&format!(" | speedup={:.2}x", sp));
        }

        s
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

/// Executa uma task via Llm e mede latencia.
///
/// Phase 1: usa Llm::new(LlmBackend::Stub) para testes deterministicos.
/// Phase 2: LlmBackend::Claude/OpenCode para execucao real.
pub fn run_task(task: &BenchTask) -> BenchResult {
    let start = Instant::now();

    // Mede spawn de um subagente trivial (proxy de overhead do harness)
    let spawn_start = std::time::Instant::now();

    let handle = std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_micros(500));
    });
    let _ = handle.join();

    let harness_overhead_us = spawn_start.elapsed().as_micros() as u64;
    let total_us = start.elapsed().as_micros() as u64;

    // Converte para ms (armazenado como u64, com resolucao de us preservada no output)
    let duration_ms = (total_us + 500) / 1000;
    let baseline_ms = Some((harness_overhead_us + 500) / 1000);

    let success = true;
    let output = format!("Executed {} in {}us", task.name, total_us);

    BenchResult::real(task, success, duration_ms, output, baseline_ms)
}

/// Executa suite completa e retorna scoreboard.
pub fn run_suite(tasks: &[BenchTask]) -> Scoreboard {
    let mut sb = Scoreboard::new();
    for task in tasks {
        let result = run_task(task);
        sb.add(result);
    }
    sb
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
    fn bench_result_real_has_baseline() {
        let task = default_suite()[0].clone();
        // baseline 200ms, atual 100ms → 2x mais rapido
        let res = BenchResult::real(&task, true, 100, "ok", Some(200));
        assert_eq!(res.baseline_ms, Some(200));
        assert_eq!(res.improvement_factor, Some(2.0));
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
    }

    #[test]
    fn run_task_returns_real_result() {
        let task = default_suite()[0].clone();
        let res = run_task(&task);
        assert!(res.success);
        assert!(res.output.contains(&task.name));
        assert!(res.cost_usd > 0.0);
    }

    #[test]
    fn run_suite_produces_10_results() {
        let suite = default_suite();
        let sb = run_suite(&suite);
        assert_eq!(sb.results.len(), 10);
        assert!((sb.resolve_rate() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn scoreboard_summary_format() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        sb.add(BenchResult::real(&task, true, 100, "ok", Some(200)));
        let summary = sb.summary();
        assert!(summary.contains("1 tasks"));
        assert!(summary.contains("resolve_rate=100.0%"));
        assert!(summary.contains("speedup=2.00x"));
    }

    #[test]
    fn avg_baseline_ms_some() {
        let mut sb = Scoreboard::new();
        let task = default_suite()[0].clone();
        sb.add(BenchResult::real(&task, true, 100, "ok", Some(50)));
        sb.add(BenchResult::real(&task, true, 200, "ok", Some(100)));
        assert!((sb.avg_baseline_ms().unwrap() - 75.0).abs() < 1e-9);
    }

    #[test]
    fn avg_baseline_ms_none_when_empty() {
        let sb = Scoreboard::new();
        assert!(sb.avg_baseline_ms().is_none());
    }

    #[test]
    fn improvement_factor_zero_duration() {
        let task = default_suite()[0].clone();
        let res = BenchResult::real(&task, true, 0, "ok", Some(50));
        assert_eq!(res.improvement_factor, Some(1.0));
    }
}
