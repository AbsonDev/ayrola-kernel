//! RLM Engine (Recursive Language Model). Pilar 2.
//!
//! Decompoe tarefas complexas em subtarefas menores e spawn de subagentes.
//!
//! Phase 0: stubs + heuristica.
//! Phase 1: integracao LLM real via MCP backend.

use crate::agent::{Agent, AgentId};
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
        Decomposer::default()
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

/// RLM planner: spawn de subagentes para cada subtarefa.
#[derive(Debug, Clone)]
pub struct Planner {
    pub agent: Agent,
}

impl Planner {
    pub fn new(agent: Agent) -> Self {
        Planner { agent }
    }

    /// Executa uma decomposicao: spawn de subagentes para cada subtarefa.
    pub async fn execute(&self, decomp: &Decomposition) -> Vec<(String, AgentId)> {
        use tokio::task::JoinSet;

        let mut set = JoinSet::new();
        let agent = self.agent.clone();

        for st in &decomp.subtasks {
            let st = st.clone();
            let agent = agent.clone();
            set.spawn(async move {
                let child = agent.spawn_subagent(st.description).await;
                (st.id, child)
            });
        }

        let mut results = Vec::new();
        while let Some(res) = set.join_next().await {
            if let Ok(r) = res {
                results.push(r);
            }
        }
        results
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

    #[test]
    fn planner_execute_spawns_subagents() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            let agent = Agent::new("planner");
            let planner = Planner::new(agent);
            let dec = Decomposer::new();
            let decomp = dec.decompose("Write tests");
            planner.execute(&decomp).await
        });
        assert_eq!(result.len(), 5, "5 subagentes devem ser spawnados");
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
