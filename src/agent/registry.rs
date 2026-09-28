//! AgentRegistry: armazena handles de subagentes para join/poll.
//!
//! Phase 1: track de handles vivos + conclusao.

use std::collections::BTreeMap;
use tokio::task::JoinHandle;

use super::{AgentId};

/// Registry de handles de subagentes.
#[derive(Debug, Default)]
pub struct AgentRegistry {
    handles: BTreeMap<AgentId, JoinHandle<()>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        AgentRegistry::default()
    }

    pub fn register(&mut self, id: AgentId, handle: JoinHandle<()>) {
        self.handles.insert(id, handle);
    }

    pub fn is_running(&self, id: AgentId) -> bool {
        self.handles.get(&id).map(|h| !h.is_finished()).unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.handles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    pub fn reap_finished(&mut self) {
        self.handles.retain(|_, h| !h.is_finished());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn registry_stores_handle() {
        let mut reg = AgentRegistry::new();
        let handle = tokio::spawn(async {});
        let id = AgentId::new();
        reg.register(id, handle);
        assert_eq!(reg.len(), 1);
    }

    #[tokio::test]
    async fn registry_reaps_finished() {
        let mut reg = AgentRegistry::new();
        let id = AgentId::new();
        reg.register(id, tokio::spawn(async {}));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        reg.reap_finished();
        assert!(reg.is_empty());
    }

    #[tokio::test]
    async fn registry_is_running_true() {
        let mut reg = AgentRegistry::new();
        let id = AgentId::new();
        reg.register(id, tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await
        }));
        assert!(reg.is_running(id));
    }

    #[tokio::test]
    async fn registry_multiple_handles() {
        let mut reg = AgentRegistry::new();
        reg.register(AgentId::new(), tokio::spawn(async {}));
        reg.register(AgentId::new(), tokio::spawn(async {}));
        assert_eq!(reg.len(), 2);
    }
}
