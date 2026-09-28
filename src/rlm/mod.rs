//! RLM Engine (Recursive Language Model). Pilar 2.
//!
//! Decompoe tarefas complexas em subtarefas menores e spawn de subagentes.
//!
//! Phase 0: stubs + heuristica.
//! Phase 1: integracao LLM real via MCP backend.

use crate::agent::Agent;
use serde::{Deserialize, Serialize};

/// Subtarefa decomposta de uma tarefa maior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub id: String,
    pub description: String,
    pub parent_task: String,
    pub depth: u32,
    pub status: SubTaskStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubTaskStatus {
    Pending,
    Running,
    Done,
    Failed,
}

/// Resultado da decomposicao de uma tarefa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decomposition {
    pub task_id: String,
    pub original: String,
    pub subtasks: Vec<SubTask>,
    pub depth: u32,
}

impl Decomposition {
    pub fn new(task_id: impl Into<String>, original: impl Into<String>) -> Self {
        Decomposition {
            task_id: task_id.into(),
            original: original.into(),
            subtasks: Vec::new(),
            depth: 0,
        }
    }

    pub fn add_subtask(&mut self, description: impl Into<String>) -> &SubTask {
        let id = format!("{}-{}", self.task_id, self.subtasks.len());
        let st = SubTask {
            id: id.clone(),
            description: description.into(),
            parent_task: self.task_id.clone(),
            depth: self.depth + 1,
            status: SubTaskStatus::Pending,
        };
        self.subtasks.push(st);
        self.subtasks.last().unwrap()
    }
}

/// RLM decomposer: quebra tarefas em subtarefas.
#[derive(Debug, Clone, Default)]
pub struct Decomposer;

impl Decomposer {
    pub fn new() -> Self {
        Decomposer
    }

    /// Decompoe uma tarefa. Stub: heuristicas baseadas em keywords.
    pub fn decompose(&self, task: &str) -> Decomposition {
        let task_id = format!("task-{}", uuid::Uuid::new_v4());
        let mut decomp = Decomposition::new(&task_id, task);

        // Heuristica: se menciona "write" ou "code", gera subtarefas de implementacao.
        let lower = task.to_lowercase();
        if lower.contains("write") || lower.contains("code") || lower.contains("implement") {
            decomp.add_subtask("Read requirements and existing code");
            decomp.add_subtask("Design the solution");
            decomp.add_subtask("Write the implementation");
            decomp.add_subtask("Write tests");
            decomp.add_subtask("Run tests and fix issues");
        } else if lower.contains("fix") || lower.contains("bug") {
            decomp.add_subtask("Reproduce the bug");
            decomp.add_subtask("Identify root cause");
            decomp.add_subtask("Implement fix");
            decomp.add_subtask("Verify fix with tests");
        } else if lower.contains("review") || lower.contains("audit") {
            decomp.add_subtask("Read the code");
            decomp.add_subtask("Identify issues");
            decomp.add_subtask("Write findings");
        } else {
            decomp.add_subtask("Analyze the task");
            decomp.add_subtask("Execute the task");
            decomp.add_subtask("Validate the result");
        }

        decomp
    }

    /// Retorna o numero de subtarefas geradas.
    pub fn subtask_count(&self, task: &str) -> usize {
        self.decompose(task).subtasks.len()
    }
}

/// Resultado da execucao de uma subtarefa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskResult {
    pub subtask_id: String,
    pub description: String,
    pub agent_id: String,
    pub status: SubTaskStatus,
    pub duration_ms: u64,
    pub output: String,
}

impl SubTaskResult {
    pub fn success(subtask_id: impl Into<String>, description: impl Into<String>, agent_id: impl Into<String>, duration_ms: u64) -> Self {
        SubTaskResult {
            subtask_id: subtask_id.into(),
            description: description.into(),
            agent_id: agent_id.into(),
            status: SubTaskStatus::Done,
            duration_ms,
            output: String::new(),
        }
    }
}

/// Relatorio de execucao de uma decomposicao inteira.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub task_id: String,
    pub original: String,
    pub results: Vec<SubTaskResult>,
    pub total_duration_ms: u64,
}

impl ExecutionReport {
    pub fn new(task_id: impl Into<String>, original: impl Into<String>) -> Self {
        ExecutionReport {
            task_id: task_id.into(),
            original: original.into(),
            results: Vec::new(),
            total_duration_ms: 0,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let ok = self.results.iter().filter(|r| r.status == SubTaskStatus::Done).count();
        ok as f64 / self.results.len() as f64
    }

    /// Latencia do subtask mais lento (critical path proxy).
    pub fn slowest_ms(&self) -> u64 {
        self.results.iter().map(|r| r.duration_ms).max().unwrap_or(0)
    }

    /// Latencia media por subtask.
    pub fn avg_ms(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        self.results.iter().map(|r| r.duration_ms).sum::<u64>() as f64
            / self.results.len() as f64
    }

    pub fn summary(&self) -> String {
        format!(
            "ExecutionReport[{}] subtasks={} success={:.0}% wall={}ms slowest={}ms avg={:.1}ms",
            self.task_id,
            self.results.len(),
            self.success_rate() * 100.0,
            self.total_duration_ms,
            self.slowest_ms(),
            self.avg_ms()
        )
    }
}

/// RLM planner: spawn de subagentes para cada subtarefa.
#[derive(Debug, Clone)]
pub struct Planner {
    pub agent: Agent,
}

impl Planner {
    pub fn new(agent: Agent) -> Self {
        Planner { agent }
    }

    /// Executa uma decomposicao com medicao de tempo real.
    /// Retorna ExecutionReport com status, duracao e output de cada subtask.
    pub async fn execute(&self, decomp: &Decomposition) -> ExecutionReport {
        use tokio::task::JoinSet;
        use std::time::Instant;

        let start = Instant::now();
        let mut report = ExecutionReport::new(&decomp.task_id, &decomp.original);

        let mut set = JoinSet::new();
        let agent = self.agent.clone();

        for st in &decomp.subtasks {
            let st = st.clone();
            let agent = agent.clone();
            set.spawn(async move {
                let task_start = Instant::now();
                let child = agent.spawn_subagent(st.description.clone()).await;
                let duration = task_start.elapsed().as_millis() as u64;

                SubTaskResult::success(st.id, st.description, child.to_string(), duration)
            });
        }

        while let Some(res) = set.join_next().await {
            if let Ok(r) = res {
                report.results.push(r);
            }
        }

        report.total_duration_ms = start.elapsed().as_millis() as u64;
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;

    #[test]
    fn decompose_code_task_returns_5_subtasks() {
        let dec = Decomposer::new();
        let decomp = dec.decompose("Write a Rust function to parse JSON");
        assert_eq!(decomp.subtasks.len(), 5);
        assert_eq!(decomp.subtasks[0].status, SubTaskStatus::Pending);
    }

    #[test]
    fn decompose_bug_task_returns_4_subtasks() {
        let dec = Decomposer::new();
        let decomp = dec.decompose("Fix the login bug");
        assert_eq!(decomp.subtasks.len(), 4);
    }

    #[test]
    fn decompose_review_task_returns_3_subtasks() {
        let dec = Decomposer::new();
        let decomp = dec.decompose("Review the PR");
        assert_eq!(decomp.subtasks.len(), 3);
    }

    #[test]
    fn decompose_generic_task_returns_3_subtasks() {
        let dec = Decomposer::new();
        let decomp = dec.decompose("Do something complex");
        assert_eq!(decomp.subtasks.len(), 3);
    }

    #[tokio::test]
    async fn planner_execute_returns_report_with_all_subtasks() {
        let agent = Agent::new("planner");
        let planner = Planner::new(agent);
        let dec = Decomposer::new();
        let decomp = dec.decompose("Write tests");
        let report = planner.execute(&decomp).await;
        assert_eq!(report.results.len(), 5, "5 subagentes devem ser spawnados");
        assert_eq!(report.success_rate(), 1.0);
        // Spawn e sub-ms, entao total_duration_ms pode ser 0
        assert!(report.results.iter().all(|r| r.status == SubTaskStatus::Done));
    }

    #[tokio::test]
    async fn planner_execute_generic_task_returns_3_subtasks() {
        let agent = Agent::new("planner");
        let planner = Planner::new(agent);
        let dec = Decomposer::new();
        let decomp = dec.decompose("Do something complex");
        let report = planner.execute(&decomp).await;
        assert_eq!(report.results.len(), 3);
    }

    #[test]
    fn execution_report_summary_format() {
        let mut report = ExecutionReport::new("t1", "original task");
        report.results.push(SubTaskResult::success("s1", "desc", "agent-1", 100));
        report.results.push(SubTaskResult::success("s2", "desc", "agent-2", 200));
        report.total_duration_ms = 300;
        let s = report.summary();
        assert!(s.contains("subtasks=2"));
        assert!(s.contains("success=100%"));
        assert!(s.contains("wall=300ms"));
        assert!(s.contains("slowest=200ms"));
    }

    #[test]
    fn execution_report_empty() {
        let report = ExecutionReport::new("t1", "task");
        assert_eq!(report.success_rate(), 0.0);
        assert_eq!(report.slowest_ms(), 0);
        assert_eq!(report.avg_ms(), 0.0);
    }

    #[test]
    fn subtask_result_success() {
        let r = SubTaskResult::success("s1", "desc", "a1", 42);
        assert_eq!(r.status, SubTaskStatus::Done);
        assert_eq!(r.duration_ms, 42);
        assert_eq!(r.agent_id, "a1");
    }

    #[test]
    fn decomposition_task_id_is_unique() {
        let dec = Decomposer::new();
        let a = dec.decompose("task A");
        let b = dec.decompose("task B");
        assert_ne!(a.task_id, b.task_id);
    }

    #[test]
    fn subtask_depth_increments() {
        let mut decomp = Decomposition::new("t1", "root");
        let st = decomp.add_subtask("child");
        assert_eq!(st.depth, 1);
        let child = decomp.add_subtask("grandchild");
        assert_eq!(child.depth, 1);
    }
}
