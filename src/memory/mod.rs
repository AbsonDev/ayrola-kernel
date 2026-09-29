//! Memory module. Pilar 1 (cont.) — snapshot + query + compaction.

pub mod compaction;
pub mod query;
pub mod snapshot;

use serde::{Deserialize, Serialize};
use crate::memory::query::SearchResult;

/// SHA-256 em hex de uma string.
fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Representacao canonica de um `ReplayState`.
///
/// `serde_json::Map` sem o feature `preserve_order` e um `BTreeMap`,
/// entao a serializacao de chaves e sempre ordenada. Isso torna o hash
/// reproduzivel entre processos e versoes.
fn canonical_state(state: &crate::event_store::ReplayState) -> serde_json::Value {
    serde_json::json!({
        "agent_order": state.agent_order,
        "by_kind": state.by_kind,
        "event_count": state.event_count,
        "head_hash": state.head_hash,
    })
}

/// Snapshot imutavel do estado em um ponto no tempo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    /// SHA-256 da representacao canonica de `state`.
    pub hash: String,
    /// Ultimo `seq` de evento coberto pelo snapshot.
    pub seq: u64,
    /// Timestamp de geracao (ISO-8601).
    pub ts: String,
    /// Estado capturado.
    pub state: serde_json::Value,
}

impl Snapshot {
    /// Cria um snapshot a partir de um `ReplayState`.
    pub fn from_replay(seq: u64, state: &crate::event_store::ReplayState) -> Self {
        let state = canonical_state(state);
        let hash = sha256_hex(&serde_json::to_string(&state).unwrap_or_default());
        Snapshot {
            hash,
            seq,
            ts: chrono::Utc::now().to_rfc3339(),
            state,
        }
    }

    /// Recalcula o hash do estado gravado e compara com o hash declarado.
    pub fn verify(&self) -> bool {
        sha256_hex(&serde_json::to_string(&self.state).unwrap_or_default()) == self.hash
    }
}

/// Gerenciador de snapshots em memoria.
#[derive(Debug, Clone, Default)]
pub struct SnapshotManager {
    pub snapshots: std::collections::BTreeMap<u64, Snapshot>,
}

impl SnapshotManager {
    pub fn new() -> Self {
        SnapshotManager::default()
    }

    /// Cria e armazena um snapshot.
    pub fn take(&mut self, seq: u64, state: &crate::event_store::ReplayState) -> &Snapshot {
        self.snapshots.insert(seq, Snapshot::from_replay(seq, state));
        self.snapshots.get(&seq).expect("acabou de inserir")
    }

    pub fn get(&self, seq: u64) -> Option<&Snapshot> {
        self.snapshots.get(&seq)
    }

    pub fn list(&self) -> Vec<&Snapshot> {
        self.snapshots.values().collect()
    }
}


/// Indice de memoria: combina event store + snapshots + busca semantica.
///
/// Phase 3 — S20: fecha o loop Pilar 1.
/// Cada decisao certificada vira um evento indexado semanticamente.
/// O agente pode "lembrar" (append) e "recordar" (search) qualquer
/// decisao passada por similaridade de texto.
pub struct MemoryIndex {
    store: crate::event_store::EventStore,
    manager: SnapshotManager,
}

impl MemoryIndex {
    /// Abre ou cria o indice no caminho.
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let store = crate::event_store::EventStore::open(path)?;
        Ok(Self {
            store,
            manager: SnapshotManager::new(),
        })
    }

    /// Lembra (indexa) uma entrada de memoria.
    ///
    /// `kind` categoriza a memoria (ex: "decision.made", "agent.spawned").
    /// `text` e o texto livre para busca semantica (TF-IDF).
    /// `payload` sao dados estruturados adicionais.
    pub fn remember(
        &mut self,
        kind: impl Into<String>,
        text: impl Into<String>,
        payload: serde_json::Value,
    ) -> std::io::Result<crate::event_store::Event> {
        let kind = kind.into();
        let text = text.into();

        // Indexa o texto no payload para busca semantica.
        let mut p = payload;
        if let Some(obj) = p.as_object_mut() {
            obj.insert("_text".to_string(), serde_json::json!(text));
        } else {
            p = serde_json::json!({"_text": text, "value": p});
        }

        let event = self.store.append(&kind, p)?;

        // Tira um snapshot a cada 10 eventos.
        let len = self.store.len().unwrap_or(0);
        #[allow(clippy::collapsible_if)]
        if len.is_multiple_of(10) {
            if let Ok(state) = self.store.replay() {
                let _ = self.manager.take(state.event_count, &state);
            }
        }

        Ok(event)
    }

    /// Recorda (busca) memorias por similaridade semantica.
    ///
    /// Retorna os top-K eventos mais similares a `query`.
    pub fn recall(&self, query: &str, top_k: usize) -> std::io::Result<Vec<SearchResult>> {
        query::search_semantic(&self.store, query, top_k)
    }

    /// Replay completo do indice + verificacao da hash chain.
    pub fn verify(&self) -> std::io::Result<bool> {
        self.store.verify_chain()
    }

    /// Numero de eventos indexados.
    pub fn len(&self) -> std::io::Result<usize> {
        self.store.len()
    }

    /// Vazio?
    pub fn is_empty(&self) -> std::io::Result<bool> {
        self.store.is_empty()
    }

    /// Snapshots disponiveis.
    pub fn snapshots(&self) -> Vec<&Snapshot> {
        self.manager.list()
    }
}


impl std::fmt::Debug for MemoryIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryIndex")
            .field("events", &self.store.len().unwrap_or(0))
            .field("snapshots", &self.manager.list().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::ReplayState;

    fn state_with(n: u64) -> ReplayState {
        ReplayState {
            event_count: n,
            head_hash: format!("head{}", n),
            agent_order: vec!["agent_1".to_string()],
            by_kind: [("test".to_string(), n)].into_iter().collect(),
        }
    }

    #[test]
    fn snapshot_from_replay_has_correct_seq() {
        let snap = Snapshot::from_replay(42, &state_with(42));
        assert_eq!(snap.seq, 42);
    }

    #[test]
    fn snapshot_verifies_against_own_hash() {
        let snap = Snapshot::from_replay(7, &state_with(7));
        assert!(snap.verify(), "hash deve bater com o estado gravado");
    }

    #[test]
    fn snapshot_verify_detects_tampered_state() {
        let mut snap = Snapshot::from_replay(1, &state_with(1));
        assert!(snap.verify());

        // Adultera o estado sem atualizar o hash.
        snap.state = serde_json::json!({"event_count": 999});
        assert!(!snap.verify(), "estado adulterado nao deve verificar");
    }

    #[test]
    fn snapshot_verify_detects_tampered_hash() {
        let mut snap = Snapshot::from_replay(1, &state_with(1));
        snap.hash = "deadbeef".to_string();
        assert!(!snap.verify(), "hash adulterado nao deve verificar");
    }

    #[test]
    fn snapshot_hash_is_deterministic_across_calls() {
        let a = Snapshot::from_replay(3, &state_with(3));
        let b = Snapshot::from_replay(3, &state_with(3));
        assert_eq!(a.hash, b.hash, "mesmo estado, mesmo hash");
    }

    #[test]
    fn snapshot_hash_changes_with_state() {
        let a = Snapshot::from_replay(0, &state_with(1));
        let b = Snapshot::from_replay(0, &state_with(2));
        assert_ne!(a.hash, b.hash, "estados diferentes, hashes diferentes");
    }

    #[test]
    fn snapshot_survives_json_roundtrip() {
        let snap = Snapshot::from_replay(5, &state_with(5));
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert!(back.verify(), "hash deve sobreviver ao round-trip JSON");
        assert_eq!(back.seq, snap.seq);
    }

    #[test]
    fn manager_stores_and_retrieves() {
        let mut mgr = SnapshotManager::new();
        let snap = mgr.take(0, &state_with(1)).clone();
        assert_eq!(mgr.get(0).map(|s| s.hash.clone()), Some(snap.hash));
        assert_eq!(mgr.list().len(), 1);
        assert!(mgr.get(99).is_none());
    }

    #[test]
    fn manager_take_overwrites_same_seq() {
        let mut mgr = SnapshotManager::new();
        mgr.take(0, &state_with(1));
        mgr.take(0, &state_with(2));
        assert_eq!(mgr.list().len(), 1, "mesmo seq sobrescreve");
    }
}

#[cfg(test)]
mod memory_index_tests {
    use crate::memory::MemoryIndex;
    use std::env;

    #[test]
    fn memory_index_remembers_and_recalls() {
        let p = {
            let mut path = env::temp_dir();
            path.push(format!("ayrola_mem_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut idx = MemoryIndex::open(&p).unwrap();
        idx.remember("decision.made", "spawn subagent for code review", serde_json::json!({"yes": true})).unwrap();
        idx.remember("agent.spawned", "agent a1 executed", serde_json::json!({"agent_id": "a1"})).unwrap();

        let results = idx.recall("spawn agent", 5).unwrap();
        assert!(!results.is_empty(), "recall should find events");
        assert!(results[0].score > 0.0, "top result should have positive score");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn memory_index_verifies_chain() {
        let p = {
            let mut path = env::temp_dir();
            path.push(format!("ayrola_mem_v_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let idx = MemoryIndex::open(&p).unwrap();
        assert!(idx.verify().unwrap());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn memory_index_len_counts_events() {
        let p = {
            let mut path = env::temp_dir();
            path.push(format!("ayrola_mem_len_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut idx = MemoryIndex::open(&p).unwrap();
        assert_eq!(idx.len().unwrap(), 0);
        idx.remember("a", "text", serde_json::json!({})).unwrap();
        assert_eq!(idx.len().unwrap(), 1);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn memory_index_is_empty() {
        let p = {
            let mut path = env::temp_dir();
            path.push(format!("ayrola_mem_empty_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let idx = MemoryIndex::open(&p).unwrap();
        assert!(idx.is_empty().unwrap());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn memory_index_takes_snapshot_every_10_events() {
        let p = {
            let mut path = env::temp_dir();
            path.push(format!("ayrola_mem_snap_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut idx = MemoryIndex::open(&p).unwrap();
        for i in 0..10 {
            idx.remember("event", format!("event {}", i), serde_json::json!({"i": i})).unwrap();
        }
        let snaps = idx.snapshots();
        assert_eq!(snaps.len(), 1, "one snapshot at 10 events");
        std::fs::remove_file(&p).ok();
    }
}

