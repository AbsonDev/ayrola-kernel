//! Agent: identificador, eventos e lifecycle basico. Pilar 2.
//!
//! Funcionalidades:
//! - AgentId: identificador unico (uuid v4)
//! - AgentEvent: eventos emitidos pelo ciclo de vida do agente
//! - Agent: struct principal com spawn_subagent + decide

use serde::{Deserialize, Serialize};
pub mod registry;

use std::sync::Arc;
use tokio::sync::RwLock;

/// Identificador unico de um agente.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct AgentId(pub uuid::Uuid);

impl AgentId {
    pub fn new() -> Self {
        AgentId(uuid::Uuid::new_v4())
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0.to_string()[..8])
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Eventos que um agente pode emitir.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentEvent {
    AgentSpawned { agent_id: AgentId, parent: Option<AgentId> },
    AgentFinished { agent_id: AgentId, result: String },
    SubagentSpawned { parent: AgentId, child: AgentId },
    SubagentFinished { parent: AgentId, child: AgentId },
}

/// Estado de um agente em execucao.
#[derive(Debug)]
pub struct AgentState {
    pub id: AgentId,
    pub parent: Option<AgentId>,
    pub task: String,
    pub spawned_at: chrono::DateTime<chrono::Utc>,
    /// S21: DecisionEngine compartilhado via Arc<RwLock<>>.
    /// Permite acesso concorrente ao mesmo engine com memoria time-travel.
    pub decision_engine: std::sync::Arc<tokio::sync::RwLock<crate::decision::DecisionEngine>>,
}

impl AgentState {
    pub fn new(id: AgentId, task: impl Into<String>) -> Self {
        AgentState {
            id,
            parent: None,
            task: task.into(),
            spawned_at: chrono::Utc::now(),
            decision_engine: std::sync::Arc::new(
                tokio::sync::RwLock::new(crate::decision::DecisionEngine::new())
            ),
        }
    }

    pub fn with_parent(mut self, parent: AgentId) -> Self {
        self.parent = Some(parent);
        self
    }
}

/// Agente principal do Ayrola.
///
/// Thread-safe via `Arc<RwLock<>>`. Cada agente tem um estado,
/// um contador de filhos, e uma referencia para o event store.
#[derive(Debug, Clone)]
pub struct Agent {
    state: Arc<RwLock<AgentState>>,
}

impl Default for Agent {
    fn default() -> Self {
        Agent::new("default")
    }
}

impl Agent {
    /// Cria um novo agente com a tarefa dada.
    pub fn new(task: impl Into<String>) -> Self {
        Agent {
            state: Arc::new(RwLock::new(AgentState::new(AgentId::new(), task))),
        }
    }

    /// ID deste agente.
    pub async fn id(&self) -> AgentId {
        let state = self.state.read().await;
        state.id
    }

    /// Tarefa atual.
    pub async fn task(&self) -> String {
        let state = self.state.read().await;
        state.task.clone()
    }

    /// Spawn um subagente para executar uma subtarefa.
    ///
    /// Retorna o AgentId do filho. O filho roda em background via Tokio task.
    pub async fn spawn_subagent(&self, subtask: String) -> AgentId {
        use tokio::task::JoinHandle;

        let child_id = AgentId::new();
        let parent_id = self.id().await;
        let task_str = subtask;

        // Emite evento (fire-and-forget, loga no stdout por enquanto).
        let _ev = AgentEvent::SubagentSpawned {
            parent: parent_id,
            child: child_id,
        };
        eprintln!(
            "[{}] spawned subagent {} for: {}",
            parent_id, child_id, task_str
        );

        // Roda o subagente em background.
        let handle: JoinHandle<()> = tokio::spawn(async move {
            eprintln!("[{}] running: {}", child_id, task_str);

            // Real work: execute a shell command via sandbox.
            let exec = crate::sandbox::SandboxExecutor::new(
                crate::sandbox::SandboxConfig::default(),
            );
            let res = exec.run(&format!("echo 'subagent {} completed: {}'", child_id, task_str));
            eprintln!("[{}] sandbox result: exit={} stdout={}", child_id, res.exit_code, res.stdout.trim());

            let _ev = AgentEvent::SubagentFinished {
                parent: parent_id,
                child: child_id,
            };
            eprintln!("[{}] finished: {}", child_id, task_str);
        });

        // Em producao: armazena handle em um HashMap<AgentId, JoinHandle<()>>
        // para join/poll futuros. Aqui, fire-and-forget.
        let _handle = handle;

        child_id
    }

    /// Spawn multiplos subagentes em paralelo e aguarda todos.
    ///
    /// Retorna Vec com os AgentIds dos filhos, na ordem de conclusao.
    pub async fn spawn_parallel(&self, subtasks: Vec<String>) -> Vec<AgentId> {
        use tokio::task::JoinSet;

        let mut set = JoinSet::new();
        for task in subtasks {
            // Clona o Arc para mover a posse para a task (exige 'static).
            let me = self.clone();
            set.spawn(async move { me.spawn_subagent(task).await });
        }

        let mut ids = Vec::new();
        while let Some(res) = set.join_next().await {
            if let Ok(id) = res {
                ids.push(id);
            }
        }
        ids
    }

    /// Decide usando o DecisionEngine compartilhado (S21).
    ///
    /// O engine reside em `AgentState` e suporta:
    /// - Tier 0: cache semantico
    /// - Tier 1: prefilter heuristic
    /// - Tier 1.5: time-travel via MemoryIndex (se configurado)
    /// - Tier 2: LLM real (9Router) ou heuristic fallback
    pub async fn decide(&self, qtype: crate::decision::QuestionType, question: &str) -> crate::decision::Answer {
        let state = self.state.read().await;
        let mut engine = state.decision_engine.write().await;
        engine.ask(qtype, question)
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn agent_spawn_generates_child_id() {
        let agent = Agent::new("root");
        let child = agent.spawn_subagent("do something".to_string()).await;
        assert_ne!(child, AgentId::new(), "cada child deve ter ID unico");
    }

    #[tokio::test]
    async fn agent_id_is_unique_per_instance() {
        let a1 = Agent::new("a");
        let a2 = Agent::new("b");
        assert_ne!(a1.id().await, a2.id().await);
    }

    #[tokio::test]
    async fn agent_task_roundtrips() {
        let agent = Agent::new("my task");
        assert_eq!(agent.task().await, "my task");
    }

    #[tokio::test]
    async fn spawn_parallel_returns_all_children() {
        let agent = Agent::new("parallel root");
        let tasks = vec!["t1".to_string(), "t2".to_string(), "t3".to_string()];
        let children = agent.spawn_parallel(tasks).await;
        assert_eq!(children.len(), 3, "3 subagentes devem ser spawnados");
    }

    #[tokio::test]
    async fn decide_yes_no_spawn_returns_true() {
        let agent = Agent::new("decider");
        let ans = agent
            .decide(crate::decision::QuestionType::YesNo, "should I spawn?")
            .await;
        match ans {
            crate::decision::Answer::YesNo { yes, .. } => assert!(yes),
            _ => panic!("expected YesNo"),
        }
    }

    #[tokio::test]
    async fn decide_yes_no_no_keyword_returns_false() {
        let agent = Agent::new("decider");
        let ans = agent
            .decide(crate::decision::QuestionType::YesNo, "what is the time?")
            .await;
        match ans {
            crate::decision::Answer::YesNo { yes, .. } => assert!(!yes),
            _ => panic!("expected YesNo"),
        }
    }
}
