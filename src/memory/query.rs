//! Memory query interface.
//!
//! Busca semantica de eventos passados. Usa FTS basico (BTreeMap de keywords).
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


/// Busca semantica por similaridade de cosseno (TF-IDF).
///
/// Phase 2 — S11: substitui busca exata por similaridade semantica.
/// Implementacao: TF-IDF + cosine similarity, sem dependencias externas.
/// Para cada evento, extrai texto do payload, tokeniza, pondera por TF-IDF,
/// e retorna os top-N mais similares a query.
///
/// Uso:
/// ```
/// let results = search_semantic(&store, "spawn subagent for code review", 5);
/// ```
pub fn search_semantic(
    store: &EventStore,
    query: &str,
    top_k: usize,
) -> std::io::Result<Vec<SearchResult>> {
    let events = store.read_all()?;
    if events.is_empty() {
        return Ok(Vec::new());
    }

    // Extrai texto de cada evento (kind + payload values)
    let event_texts: Vec<String> = events
        .iter()
        .map(|e| {
            let mut text = e.kind.clone();
            if let Some(obj) = e.payload.as_object() {
                for (k, v) in obj {
                    text.push(' ');
                    text.push_str(k);
                    text.push(' ');
                    if let Some(s) = v.as_str() {
                        text.push_str(s);
                    } else {
                        text.push_str(&v.to_string());
                    }
                }
            } else {
                text.push(' ');
                text.push_str(&e.payload.to_string());
            }
            text
        })
        .collect();

    // Tokeniza e calcula TF-IDF
    let query_tokens = tokenize(query);
    let mut idf: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let mut doc_freq: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for text in &event_texts {
        let tokens = tokenize(text);
        let unique: std::collections::HashSet<_> = tokens.iter().cloned().collect();
        for tok in unique {
            *doc_freq.entry(tok).or_insert(0) += 1;
        }
    }

    let n_docs = event_texts.len() as f64;
    for (tok, df) in &doc_freq {
        *idf.entry(tok.clone()).or_insert(0.0) = (n_docs / *df as f64).ln();
    }

    // Vetor da query
    let query_vec = tfidf_vector(&query_tokens, &idf);

    // Similaridade de cosseno para cada evento
    let mut results: Vec<SearchResult> = events
        .into_iter()
        .zip(event_texts.iter())
        .map(|(event, text)| {
            let tokens = tokenize(text);
            let vec = tfidf_vector(&tokens, &idf);
            let score = cosine_similarity(&query_vec, &vec);
            SearchResult { event, score }
        })
        .filter(|r| r.score > 0.0)
        .collect();

    // Ordena por score decrescente e retorna top_k
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(top_k);
    Ok(results)
}

/// Tokeniza texto em palavras lowercase (sem stopwords basicas).
fn tokenize(text: &str) -> Vec<String> {
    let stopwords = ["the", "a", "an", "is", "are", "was", "were", "be", "been",
                     "being", "have", "has", "had", "do", "does", "did", "will",
                     "would", "could", "should", "may", "might", "can", "to", "of",
                     "in", "for", "on", "with", "at", "by", "from", "as", "into",
                     "and", "or", "but", "not", "no", "if", "then", "than", "that",
                     "this", "it", "its"];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 2 && !stopwords.contains(s))
        .map(|s| s.to_string())
        .collect()
}

/// Calcula vetor TF-IDF para uma lista de tokens.
fn tfidf_vector(tokens: &[String], idf: &std::collections::HashMap<String, f64>) -> Vec<f64> {
    let mut tf: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for tok in tokens {
        *tf.entry(tok.clone()).or_insert(0.0) += 1.0;
    }
    let max_tf = tf.values().cloned().fold(1.0, f64::max);

    let mut vec: Vec<(String, f64)> = tf
        .iter()
        .map(|(tok, count)| {
            let tf_weight = 0.5 + 0.5 * *count / max_tf;
            let idf_weight = idf.get(tok).copied().unwrap_or(0.0);
            (tok.clone(), tf_weight * idf_weight)
        })
        .filter(|(_, w)| *w > 0.0)
        .collect();

    vec.sort_by(|a, b| a.0.cmp(&b.0));
    vec.into_iter().map(|(_, w)| w).collect()
}

/// Similaridade de cosseno entre dois vetores.
fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::EventStore;
    use std::env;

    #[allow(dead_code)]
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
    #[test]
    fn search_semantic_finds_similar_events() {
        let p = {
            let mut path = std::env::temp_dir();
            path.push(format!("ayrola_sem_test_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut store = EventStore::open(&p).unwrap();
        store.append("agent.spawned", serde_json::json!({"agent_id": "a1", "task": "code review"})).unwrap();
        store.append("decision.made", serde_json::json!({"answer": true})).unwrap();
        store.append("agent.spawned", serde_json::json!({"agent_id": "a2", "task": "run tests"})).unwrap();

        let results = search_semantic(&store, "spawn agent for code", 2).unwrap();
        assert!(!results.is_empty(), "should find at least one result");
        assert!(results[0].score > 0.0, "first result should have positive score");
        println!("Semantic search results: {:?}", results);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn search_semantic_returns_empty_for_empty_store() {
        let p = {
            let mut path = std::env::temp_dir();
            path.push(format!("ayrola_sem_empty_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let store = EventStore::open(&p).unwrap();

        let results = search_semantic(&store, "query", 5).unwrap();
        assert!(results.is_empty(), "empty store should return empty results");

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn search_semantic_ranks_by_score() {
        let p = {
            let mut path = std::env::temp_dir();
            path.push(format!("ayrola_sem_rank_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut store = EventStore::open(&p).unwrap();
        store.append("event", serde_json::json!({"text": "spawn subagent for code review"})).unwrap();
        store.append("event", serde_json::json!({"text": "unrelated weather forecast"})).unwrap();
        store.append("event", serde_json::json!({"text": "spawn another agent for testing"})).unwrap();

        let results = search_semantic(&store, "spawn agent", 3).unwrap();
        assert!(results.len() >= 1, "should find matches");
        // Scores devem ser ordenados decrescentemente
        for i in 0..results.len().saturating_sub(1) {
            assert!(results[i].score >= results[i+1].score, "scores should be descending");
        }
        println!("Ranked results: {:?}", results);

        std::fs::remove_file(&p).ok();
    }

}
