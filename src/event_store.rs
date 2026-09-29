//! Event store append-only NDJSON. Pilar 1 do Ayrola.
//!
//! Garantias:
//! - Append-only: eventos nunca sao reescritos nem removidos
//! - Causal chain: cada evento tem `prev_hash` -> forma uma cadeia
//! - Determinismo: replay a partir de um offset reproduz o mesmo estado
//! - Imutavel: snapshot e hash-lock, sem edicao posterior

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Um evento no log. Serializado como uma linha JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// Sequencia monotonica (0, 1, 2, ...)
    pub seq: u64,
    /// Timestamp UTC ISO-8601
    pub ts: String,
    /// Tipo do evento (ex: "agent.spawned", "decision.made")
    pub kind: String,
    /// Payload livre
    pub payload: serde_json::Value,
    /// Hash do evento anterior (cadeia causal)
    pub prev_hash: String,
    /// Hash deste evento (prev_hash + conteudo)
    pub hash: String,
}

impl Event {
    /// Calcula o hash deterministico deste evento.
    pub fn compute_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        // Deterministic byte representation via a struct with fixed key order
        // (seq, ts, kind, payload, prev_hash). JSON Value objects do not
        // guarantee key ordering, so a struct is used instead.
        #[derive(Serialize)]
        struct HashCanonical<'a> {
            seq: u64,
            ts: &'a str,
            kind: &'a str,
            payload: &'a serde_json::Value,
            prev_hash: &'a str,
        }
        let canonical = HashCanonical {
            seq: self.seq,
            ts: &self.ts,
            kind: &self.kind,
            payload: &self.payload,
            prev_hash: &self.prev_hash,
        };
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&canonical).unwrap_or_default());
        let result = hasher.finalize();
        result.iter().map(|b| format!("{:02x}", b)).collect::<String>()
    }

    /// Verifica se o hash gravado bate com o hash recalculado.
    pub fn verify(&self) -> bool {
        self.hash == self.compute_hash()
    }
}

/// Estado reproduzido por replay de uma sequencia de eventos.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayState {
    /// Numero de eventos aplicados
    pub event_count: u64,
    /// Hash final da cadeia (prova de integridade)
    pub head_hash: String,
    /// Contagem por tipo de evento
    pub by_kind: BTreeMap<String, u64>,
    /// Ids de agentes vistos, em ordem de nascimento
    pub agent_order: Vec<String>,
}

impl ReplayState {
    /// Aplica um evento ao estado.
    pub fn apply(&mut self, ev: &Event) {
        self.event_count += 1;
        self.head_hash = ev.hash.clone();
        *self.by_kind.entry(ev.kind.clone()).or_insert(0) += 1;
        if ev.kind == "agent.spawned"
            && let Some(id) = ev.payload.get("agent_id").and_then(|v| v.as_str())
        {
            self.agent_order.push(id.to_string());
        }
    }
}

/// Event store append-only em NDJSON.
pub struct EventStore {
    path: PathBuf,
    last_hash: String,
    next_seq: u64,
}

impl EventStore {
    /// Abre (ou cria) um event store no path dado.
    ///
    /// Se o arquivo ja existir, reidrata `next_seq` e `last_hash`
    /// a partir do ultimo evento valido da cadeia.
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut store = EventStore {
            path,
            last_hash: "genesis".to_string(),
            next_seq: 0,
        };
        store.rehydrate()?;
        Ok(store)
    }

    /// Le o arquivo e restaura next_seq / last_hash.
    fn rehydrate(&mut self) -> std::io::Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if !line.trim().is_empty()
                && let Ok(ev) = serde_json::from_str::<Event>(&line)
                && ev.verify()
            {
                self.next_seq = ev.seq.saturating_add(1);
                self.last_hash = ev.hash.clone();
            }
        }
        Ok(())
    }

    /// Acrescenta um evento. Retorna o evento gravado.
    ///
    /// Falha se o arquivo nao puder ser aberto para append.
    pub fn append(&mut self, kind: &str, payload: serde_json::Value) -> std::io::Result<Event> {
        let ts = chrono::Utc::now().to_rfc3339();
        let mut ev = Event {
            seq: self.next_seq,
            ts,
            kind: kind.to_string(),
            payload,
            prev_hash: self.last_hash.clone(),
            hash: String::new(),
        };
        ev.hash = ev.compute_hash();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(&ev)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
        file.flush()?;

        self.next_seq = self.next_seq.saturating_add(1);
        self.last_hash = ev.hash.clone();
        Ok(ev)
    }

    /// Le todos os eventos validos, em ordem.
    pub fn read_all(&self) -> std::io::Result<Vec<Event>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if !line.trim().is_empty()
                && let Ok(ev) = serde_json::from_str::<Event>(&line)
                && ev.verify()
            {
                out.push(ev);
            }
        }
        Ok(out)
    }

    /// Reproduz todos os eventos a partir de `from_seq`, aplicando ao estado.
    pub fn replay_from(&self, from_seq: u64) -> std::io::Result<ReplayState> {
        let mut state = ReplayState::default();
        for ev in self.read_all()? {
            if ev.seq >= from_seq {
                state.apply(&ev);
            }
        }
        Ok(state)
    }

    /// Reproduz todo o log do inicio.
    pub fn replay(&self) -> std::io::Result<ReplayState> {
        self.replay_from(0)
    }

    /// Verifica a cadeia causal inteira: cada `prev_hash` bate com o
    /// hash do evento anterior, e cada `seq` incrementa em 1.
    pub fn verify_chain(&self) -> std::io::Result<bool> {
        let events = self.read_all()?;
        let mut prev = "genesis".to_string();
        for (i, ev) in events.iter().enumerate() {
            if ev.prev_hash != prev {
                return Ok(false);
            }
            if ev.seq != i as u64 {
                return Ok(false);
            }
            if !ev.verify() {
                return Ok(false);
            }
            prev = ev.hash.clone();
        }
        Ok(true)
    }

    /// Numero de eventos no log.
    pub fn len(&self) -> std::io::Result<usize> {
        Ok(self.read_all()?.len())
    }

    /// True se o log estiver vazio.
    pub fn is_empty(&self) -> std::io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Hash da cabeca da cadeia.
    pub fn head_hash(&self) -> &str {
        &self.last_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn tmp_path(tag: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("ayrola_es_{}_{}.ndjson", tag, uuid::Uuid::new_v4()));
        p
    }

    #[test]
    fn append_then_read_all_roundtrips() {
        let p = tmp_path("roundtrip");
        let mut es = EventStore::open(&p).unwrap();

        let e0 = es.append("agent.spawned", serde_json::json!({"agent_id": "a1"})).unwrap();
        let e1 = es.append("decision.made", serde_json::json!({"answer": true})).unwrap();

        assert_eq!(e0.seq, 0);
        assert_eq!(e1.seq, 1);
        assert_eq!(e1.prev_hash, e0.hash);

        let all = es.read_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0], e0);
        assert_eq!(all[1], e1);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn empty_store_replays_to_default() {
        let p = tmp_path("empty");
        let es = EventStore::open(&p).unwrap();
        assert!(es.is_empty().unwrap());

        let st = es.replay().unwrap();
        assert_eq!(st.event_count, 0);
        assert_eq!(st.agent_order.len(), 0);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn replay_from_offset_is_deterministic() {
        let p = tmp_path("determinism");
        let mut es = EventStore::open(&p).unwrap();

        for i in 0..5 {
            es.append("tick", serde_json::json!({"i": i})).unwrap();
        }
        es.append("agent.spawned", serde_json::json!({"agent_id": "a9"})).unwrap();

        // Duas execucoes de replay produzem o mesmo estado.
        let r1 = es.replay().unwrap();
        let r2 = es.replay().unwrap();
        assert_eq!(r1, r2);
        assert_eq!(r1.event_count, 6);
        assert_eq!(r1.agent_order, vec!["a9".to_string()]);
        assert_eq!(r1.by_kind.get("tick"), Some(&5));
        assert_eq!(r1.by_kind.get("agent.spawned"), Some(&1));

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn chain_verifies_and_detects_tampering() {
        let p = tmp_path("chain");
        let mut es = EventStore::open(&p).unwrap();
        es.append("a", serde_json::json!(1)).unwrap();
        es.append("b", serde_json::json!(2)).unwrap();
        es.append("c", serde_json::json!(3)).unwrap();

        assert!(es.verify_chain().unwrap(), "cadeia intacta deve verificar");

        // Adulteracao: muda o payload de um evento sem recalcular o hash.
        let content = std::fs::read_to_string(&p).unwrap();
        let tampered = content.replacen(r#""payload":2"#, r#""payload":999"#, 1);
        std::fs::write(&p, tampered).unwrap();

        let es2 = EventStore::open(&p).unwrap();
        assert!(!es2.verify_chain().unwrap(), "cadeia adulterada nao deve verificar");

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn rehydrate_continues_sequence_after_reopen() {
        let p = tmp_path("rehydrate");
        {
            let mut es = EventStore::open(&p).unwrap();
            es.append("x", serde_json::json!(1)).unwrap();
            es.append("x", serde_json::json!(2)).unwrap();
        }
        // Reabre: next_seq deve continuar em 2, nao reiniciar em 0.
        let mut es = EventStore::open(&p).unwrap();
        let e = es.append("x", serde_json::json!(3)).unwrap();
        assert_eq!(e.seq, 2, "seq deve continuar apos reabrir");
        assert_eq!(es.len().unwrap(), 3);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn compute_hash_distinguishes_pipe_in_kind_vs_payload() {
        // REGRESSION: a hash chain must be collision-free even when `kind`
        // or `payload` contains the field separator character `|`.
        let ts = "2025-01-01T00:00:00Z";
        let prev = "genesis";

        // Event A: kind="decision", payload="leaked|secret"
        let mut a = Event {
            seq: 0,
            ts: ts.into(),
            kind: "decision".into(),
            payload: serde_json::json!("leaked|secret"),
            prev_hash: prev.into(),
            hash: String::new(),
        };
        a.hash = a.compute_hash();

        // Event B: kind="decision|leaked", payload="secret"
        let mut b = Event {
            seq: 0,
            ts: ts.into(),
            kind: "decision|leaked".into(),
            payload: serde_json::json!("secret"),
            prev_hash: prev.into(),
            hash: String::new(),
        };
        b.hash = b.compute_hash();

        // These are different events and must produce different hashes.
        assert_ne!(a.hash, b.hash,
            "events with kind/payload swapped across pipe boundary must hash differently, got {}",
            a.hash);
    }
}
