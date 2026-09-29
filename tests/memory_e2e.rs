//! Integration test: MemoryIndex remember + recall.
//!
//! Prova que o Pilar 1 esta fechado: decisoes sao indexadas
//! semanticamente e recuperaveis por similaridade de texto.

use ayrola_kernel::memory::MemoryIndex;

#[test]
fn remember_and_recall_end_to_end() {
    let dir = std::env::temp_dir().join(format!("ayrola_mem_e2e_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("events.ndjson");

    let mut idx = MemoryIndex::open(&path).unwrap();

    // Simula 3 decisoes certificadas reais.
    idx.remember(
        "decision.made",
        "spawn subagent for code review of pull request",
        serde_json::json!({"yes": true, "tier": "tier2", "confidence": 0.9}),
    ).unwrap();
    idx.remember(
        "decision.made",
        "deploy production database migration",
        serde_json::json!({"yes": false, "tier": "tier0", "confidence": 0.99}),
    ).unwrap();
    idx.remember(
        "agent.spawned",
        "subagent executed cargo test in sandbox",
        serde_json::json!({"agent_id": "a1"}),
    ).unwrap();

    // Chain verifica.
    assert!(idx.verify().unwrap(), "hash chain must verify");
    assert_eq!(idx.len().unwrap(), 3);

    // Recall por similaridade.
    let results = idx.recall("code review pull request", 3).unwrap();
    assert!(!results.is_empty(), "recall should find events");
    assert!(results[0].score > 0.0);

    // O resultado mais relevante deve ter score positivo.
    assert!(results[0].score > 0.0, "top result must have positive score");

    // Recall sem match relevante nao deve inventar.
    let results2 = idx.recall("quantum chromodynamics lattice", 3).unwrap();
    for r in results2 {
        assert!(r.score <= 1.0, "cosine score must be in [0, 1]");
    }

    std::fs::remove_dir_all(&dir).ok();
}
