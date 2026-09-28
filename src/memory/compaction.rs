//! Memory compaction: remove eventos antigos mantendo snapshots.
//!
//! Em Ayrola, o event log e append-only (nunca deletado).
//! A "compaction" aqui significa: manter snapshots como pontos de
//! restauracao e poder pular eventos ja incorporados ao snapshot.

use crate::event_store::{Event, EventStore};

/// Resultado de uma operacao de compaction.
#[derive(Debug)]
pub struct CompactionReport {
    pub total_events: u64,
    pub events_to_compact: u64,
    pub events_kept: u64,
    pub snapshot_seq: u64,
}

/// Gera um relatorio de compaction sem modificar o event store.
/// (Event store e append-only — compaction e logica de leitura.)
pub fn compaction_report(
    store: &EventStore,
    snapshot_seq: u64,
) -> std::io::Result<CompactionReport> {
    let events = store.read_all()?;
    let total = events.len() as u64;
    let to_compact = events
        .iter()
        .filter(|e| e.seq <= snapshot_seq)
        .count() as u64;
    let kept = total - to_compact;

    Ok(CompactionReport {
        total_events: total,
        events_to_compact: to_compact,
        events_kept: kept,
        snapshot_seq,
    })
}

/// Retorna eventos que precisam ser lidos a partir de um snapshot.
pub fn events_since_snapshot(
    store: &EventStore,
    from_seq: u64,
) -> std::io::Result<Vec<Event>> {
    Ok(store
        .read_all()?
        .into_iter()
        .filter(|e| e.seq > from_seq)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::EventStore;
    use std::env;

    fn tmp_store(tag: &str) -> (EventStore, std::path::PathBuf) {
        let mut p = env::temp_dir();
        p.push(format!("ayrola_compact_{}.ndjson", tag, )); // uuid appended separately
        p.set_extension(format!("ndjson"));
        let p = std::env::temp_dir().join(format!("ayrola_compact_{}.ndjson", uuid::Uuid::new_v4()));
        let store = EventStore::open(&p).unwrap();
        (store, p)
    }

    #[test]
    fn report_shows_correct_counts() {
        let (mut store, p) = tmp_store("report");
        for _ in 0..5 {
            store.append("tick", serde_json::json!({})).unwrap();
        }
        store.append("agent.spawned", serde_json::json!({"agent_id": "x"})).unwrap();

        let report = compaction_report(&store, 2).unwrap();
        assert_eq!(report.total_events, 6);
        assert_eq!(report.events_to_compact, 3); // seq 0,1,2
        assert_eq!(report.events_kept, 3);       // seq 3,4,5

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn events_since_snapshot_filters_correctly() {
        let (mut store, p) = tmp_store("since");
        for i in 0..5 {
            store.append("tick", serde_json::json!({"i": i})).unwrap();
        }

        let events = events_since_snapshot(&store, 2).unwrap();
        assert_eq!(events.len(), 2); // seq 3,4 only (seq > 2)
        assert_eq!(events[0].seq, 3);
        assert_eq!(events[1].seq, 4);

        std::fs::remove_file(&p).ok();
    }
}
