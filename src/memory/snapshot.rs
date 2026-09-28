//! Snapshot operations: create, restore, list.
//! 
//! Em Ayrola, um snapshot e uma captura imutavel do estado
//! em um ponto no tempo. Usado para time-travel debugging e
//! para evitar recompute caro.

use crate::memory::Snapshot;
use crate::event_store::EventStore;
use std::path::Path;

/// Cria um snapshot a partir do replay completo do event store.
pub fn create_snapshot(store: &EventStore) -> Result<Snapshot, std::io::Error> {
    let state = store.replay()?;
    let seq = state.event_count.saturating_sub(1);
    Ok(Snapshot::from_replay(seq, &state))
}

/// Cria um snapshot a partir do replay a partir de um offset.
pub fn create_snapshot_from(
    store: &EventStore,
    from_seq: u64,
) -> Result<Snapshot, std::io::Error> {
    let state = store.replay_from(from_seq)?;
    let seq = state.event_count + from_seq;
    Ok(Snapshot::from_replay(seq, &state))
}

/// Lista todos os snapshots disponiveis em um diretorio.
/// Cada snapshot e um arquivo NDJSON: {hash, seq, ts, state}
pub fn list_snapshots(dir: &Path) -> std::io::Result<Vec<Snapshot>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json")
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(snap) = serde_json::from_str::<Snapshot>(&content)
            && snap.verify()
        {
            out.push(snap);
        }
    }
    out.sort_by_key(|s| s.seq);
    Ok(out)
}

/// Salva um snapshot em um arquivo JSON.
pub fn save_snapshot(snap: &Snapshot, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("snapshot-{}.json", snap.seq));
    let content = serde_json::to_string_pretty(snap)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::EventStore;
    use std::env;

    fn tmp_path(tag: &str) -> std::path::PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("ayrola_snap_{}_{}", tag, uuid::Uuid::new_v4()));
        p
    }

    #[test]
    fn create_snapshot_after_events() {
        let p = tmp_path("create");
        let mut store = EventStore::open(&p).unwrap();
        store.append("test", serde_json::json!({"v": 1})).unwrap();
        store.append("test", serde_json::json!({"v": 2})).unwrap();

        let snap = create_snapshot(&store).unwrap();
        assert!(snap.verify());
        assert!(snap.state.get("event_count").is_some());

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn save_and_list_snapshot() {
        let p = tmp_path("save");
        let dir = p.parent().unwrap().join(format!("ayrola_snap_dir_{}", uuid::Uuid::new_v4()));

        let mut store = EventStore::open(&p).unwrap();
        store.append("test", serde_json::json!({"v": 1})).unwrap();

        let snap = create_snapshot(&store).unwrap();
        save_snapshot(&snap, &dir).unwrap();

        let snaps = list_snapshots(&dir).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].seq, snap.seq);
        assert!(snaps[0].verify());

        std::fs::remove_file(&p).ok();
        std::fs::remove_dir_all(&dir).ok();
    }
}
