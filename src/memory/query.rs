//! Memory query interface.
//!
//! Busca semantica de eventos passados. Em Phase 0, usa FTS basico.
//! Futuro: integracao com pgvector/Qdrant.

use crate::event_store::{Event, EventStore};

/// Resultado de uma busca de eventos.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub event: Event,
    pub score: f64,
}

/// Busca eventos por tipo (kind).
pub fn search_by_kind(store: &EventStore, kind: &str) -> std::io::Result<Vec<Event>> {
    let events = store.read_all()?;
    let filtered: Vec<Event> = events.into_iter().filter(|e| e.kind == kind).collect();
    Ok(filtered)
}

/// Busca eventos por campo no payload (busca textual simples).
pub fn search_by_payload(store: &EventStore, field: &str, value: &str) -> std::io::Result<Vec<Event>> {
    let events = store.read_all()?;
    let filtered: Vec<Event> = events
        .into_iter()
        .filter(|e| {
            e.payload
                .get(field)
                .and_then(|v| v.as_str())
                .map(|s| s.contains(value))
                .unwrap_or(false)
        })
        .collect();
    Ok(filtered)
}

/// Busca eventos por agent_id no payload.
pub fn search_by_agent(store: &EventStore, agent_id: &str) -> std::io::Result<Vec<Event>> {
    let events = store.read_all()?;
    let filtered: Vec<Event> = events
        .into_iter()
        .filter(|e| {
            e.payload
                .get("agent_id")
                .and_then(|v| v.as_str())
                .map(|s| s == agent_id)
                .unwrap_or(false)
        })
        .collect();
    Ok(filtered)
}

/// Conta eventos por tipo.
pub fn count_by_kind(store: &EventStore) -> std::io::Result<std::collections::BTreeMap<String, u64>> {
    let events = store.read_all()?;
    let mut counts = std::collections::BTreeMap::new();
    for ev in events {
        *counts.entry(ev.kind).or_insert(0) += 1;
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::EventStore;
    use std::env;

    fn tmp_store(tag: &str) -> EventStore {
        let mut p = env::temp_dir();
        p.push(format!("ayrola_q_{}_{}.ndjson", tag, uuid::Uuid::new_v4()));
        // Copy the file path but return a new store
        EventStore::open(&p).unwrap()
    }

    #[test]
    fn search_by_kind_finds_matching() {
        let p = {
            let mut path = std::env::temp_dir();
            path.push(format!("ayrola_q_test_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut store = EventStore::open(&p).unwrap();
        store.append("agent.spawned", serde_json::json!({"agent_id": "a1"})).unwrap();
        store.append("decision.made", serde_json::json!({"answer": true})).unwrap();
        store.append("agent.spawned", serde_json::json!({"agent_id": "a2"})).unwrap();

        let results = search_by_kind(&store, "agent.spawned").unwrap();
        assert_eq!(results.len(), 2);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn count_by_kind_returns_correct_counts() {
        let p = {
            let mut path = std::env::temp_dir();
            path.push(format!("ayrola_q_count_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut store = EventStore::open(&p).unwrap();
        store.append("a", serde_json::json!({})).unwrap();
        store.append("a", serde_json::json!({})).unwrap();
        store.append("b", serde_json::json!({})).unwrap();

        let counts = count_by_kind(&store).unwrap();
        assert_eq!(counts.get("a"), Some(&2));
        assert_eq!(counts.get("b"), Some(&1));

        std::fs::remove_file(&p).ok();
    }
}
