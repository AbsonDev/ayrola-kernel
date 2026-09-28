//! Certificação de decisão. Eixo E.
//!
//! Cada decisão carrega {decision_id, inputs, decision, evidence, cost, tier, replayable}.
//! Replay verificável por hash.
//!
//! Phase 1: implementacao real.
//! Phase 1: integra com event store para replay real.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// ID único de decisão.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DecisionId(pub String);

impl DecisionId {
    pub fn new(prefix: &str) -> Self {
        DecisionId(format!("{}-{}", prefix, uuid::Uuid::new_v4()))
    }
}

impl std::fmt::Display for DecisionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Tier de decisão (0=cache, 1=pre-filter, 2=LLM).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionTier {
    Tier0,
    Tier1,
    Tier2,
}

impl std::fmt::Display for DecisionTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecisionTier::Tier0 => write!(f, "tier0"),
            DecisionTier::Tier1 => write!(f, "tier1"),
            DecisionTier::Tier2 => write!(f, "tier2"),
        }
    }
}

/// Evidência que suporta a decisão.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source: String,
    pub detail: String,
    pub confidence: f64,
}

impl Evidence {
    pub fn new(source: impl Into<String>, detail: impl Into<String>, confidence: f64) -> Self {
        Evidence {
            source: source.into(),
            detail: detail.into(),
            confidence: confidence.clamp(0.0, 1.0),
        }
    }
}

/// Decisão certificada: replayável e auditável.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertifiedDecision {
    pub decision_id: DecisionId,
    pub timestamp: String,
    pub inputs: serde_json::Value,
    pub decision: serde_json::Value,
    pub evidence: Evidence,
    pub cost_usd: f64,
    pub tier: DecisionTier,
    pub replayable: bool,
    pub hash: String,
}

impl CertifiedDecision {
    /// Cria uma decisão certificada com hash SHA-256.
    pub fn new(
        inputs: serde_json::Value,
        decision: serde_json::Value,
        evidence: Evidence,
        cost_usd: f64,
        tier: DecisionTier,
    ) -> Self {
        let decision_id = DecisionId::new("dec");
        let timestamp = chrono::Utc::now().to_rfc3339();
        let replayable = true;
        let temp = CertifiedDecision {
            decision_id: decision_id.clone(),
            timestamp: timestamp.clone(),
            inputs: inputs.clone(),
            decision: decision.clone(),
            evidence: evidence.clone(),
            cost_usd,
            tier,
            replayable,
            hash: String::new(),
        };
        let hash = Self::compute_hash(&temp);
        CertifiedDecision {
            decision_id,
            timestamp,
            inputs,
            decision,
            evidence,
            cost_usd,
            tier,
            replayable,
            hash,
        }
    }

    /// Recalcula o hash e compara com o hash armazenado.
    pub fn verify(&self) -> bool {
        Self::compute_hash(self) == self.hash
    }

    /// SHA-256 da representação canônica da decisão.
    fn compute_hash(cert: &CertifiedDecision) -> String {
        let canonical = serde_json::json!({
            "id": cert.decision_id.0,
            "ts": cert.timestamp,
            "inputs": cert.inputs,
            "decision": cert.decision,
            "evidence": cert.evidence,
            "cost": cert.cost_usd,
            "tier": cert.tier.to_string(),
            "replayable": cert.replayable,
        });
        let s = serde_json::to_string(&canonical).unwrap_or_default();
        let mut h = Sha256::new();
        h.update(s.as_bytes());
        h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// Registro de decisões certificadas.
#[derive(Debug, Clone, Default)]
pub struct DecisionLog {
    pub decisions: std::collections::BTreeMap<String, CertifiedDecision>,
}

impl DecisionLog {
    pub fn new() -> Self {
        DecisionLog::default()
    }

    pub fn record(&mut self, decision: CertifiedDecision) {
        self.decisions.insert(decision.decision_id.0.clone(), decision);
    }

    pub fn get(&self, id: &str) -> Option<&CertifiedDecision> {
        self.decisions.get(id)
    }

    pub fn len(&self) -> usize {
        self.decisions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.decisions.is_empty()
    }

    /// Verifica TODAS as decisões registradas.
    pub fn verify_all(&self) -> bool {
        self.decisions.values().all(|d| d.verify())
    }

    /// Lista decisões por tier.
    pub fn by_tier(&self, tier: DecisionTier) -> Vec<&CertifiedDecision> {
        self.decisions.values().filter(|d| d.tier == tier).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certified_decision_verifies() {
        let d = CertifiedDecision::new(
            serde_json::json!({"q": "spawn?"}),
            serde_json::json!({"yes": true}),
            Evidence::new("tier0", "cache hit", 0.99),
            0.0,
            DecisionTier::Tier0,
        );
        assert!(d.verify());
    }

    #[test]
    fn certified_decision_detects_tampering() {
        let mut d = CertifiedDecision::new(
            serde_json::json!({"q": "spawn?"}),
            serde_json::json!({"yes": true}),
            Evidence::new("tier0", "cache hit", 0.99),
            0.0,
            DecisionTier::Tier0,
        );
        assert!(d.verify());
        d.decision = serde_json::json!({"yes": false});
        assert!(!d.verify(), "tampered decision must fail verify");
    }

    #[test]
    fn decision_log_records_and_retrieves() {
        let mut log = DecisionLog::new();
        let d = CertifiedDecision::new(
            serde_json::json!({"q": "test"}),
            serde_json::json!({"yes": true}),
            Evidence::new("tier1", "heuristic", 0.85),
            0.001,
            DecisionTier::Tier1,
        );
        log.record(d.clone());
        assert_eq!(log.len(), 1);
        assert!(log.get(&d.decision_id.0).is_some());
    }

    #[test]
    fn decision_log_verify_all() {
        let mut log = DecisionLog::new();
        for i in 0..3 {
            let d = CertifiedDecision::new(
                serde_json::json!({"i": i}),
                serde_json::json!({"yes": i % 2 == 0}),
                Evidence::new("test", "unit", 1.0),
                0.0,
                DecisionTier::Tier2,
            );
            log.record(d);
        }
        assert!(log.verify_all());
    }

    #[test]
    fn decision_log_by_tier() {
        let mut log = DecisionLog::new();
        let d0 = CertifiedDecision::new(
            serde_json::json!({}),
            serde_json::json!({}),
            Evidence::new("t0", "cache", 1.0),
            0.0,
            DecisionTier::Tier0,
        );
        let d2 = CertifiedDecision::new(
            serde_json::json!({}),
            serde_json::json!({}),
            Evidence::new("t2", "llm", 0.75),
            0.01,
            DecisionTier::Tier2,
        );
        log.record(d0);
        log.record(d2);
        assert_eq!(log.by_tier(DecisionTier::Tier0).len(), 1);
        assert_eq!(log.by_tier(DecisionTier::Tier2).len(), 1);
        assert_eq!(log.by_tier(DecisionTier::Tier1).len(), 0);
    }

    #[test]
    fn decision_id_is_unique() {
        let a = DecisionId::new("dec");
        let b = DecisionId::new("dec");
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn evidence_clamps_confidence() {
        let e = Evidence::new("test", "detail", 1.5);
        assert_eq!(e.confidence, 1.0);
        let e2 = Evidence::new("test", "detail", -0.5);
        assert_eq!(e2.confidence, 0.0);
    }

    #[test]
    fn certified_decision_serializes() {
        let d = CertifiedDecision::new(
            serde_json::json!({"q": "test"}),
            serde_json::json!({"yes": true}),
            Evidence::new("tier0", "cache", 0.99),
            0.0,
            DecisionTier::Tier0,
        );
        let json = serde_json::to_string(&d).unwrap();
        let back: CertifiedDecision = serde_json::from_str(&json).unwrap();
        assert!(back.verify());
    }

    #[test]
    fn decision_tier_display() {
        assert_eq!(DecisionTier::Tier0.to_string(), "tier0");
        assert_eq!(DecisionTier::Tier1.to_string(), "tier1");
        assert_eq!(DecisionTier::Tier2.to_string(), "tier2");
    }
}
