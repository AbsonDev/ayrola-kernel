//! Memory module. Pilar 1 (cont.) — snapshot + query + compaction.

pub mod compaction;
pub mod query;
pub mod snapshot;

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::ReplayState;

    fn state_with(n: u64) -> ReplayState {
        let mut s = ReplayState::default();
        s.event_count = n;
        s.head_hash = format!("head{}", n);
        s.agent_order.push("agent_1".to_string());
        s.by_kind.insert("test".to_string(), n);
        s
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
