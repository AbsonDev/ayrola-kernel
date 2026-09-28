//! Decision layer trait + ensemble 3 tiers. Pilar 3.
//!
//! Camadas:
//! - Tier 0: cache semantico (hash normalizado + similaridade > 0.95)
//! - Tier 1: pre-filter ONNX pequeno (~5ms, so descarta candidatos)
//! - Tier 2: LLM completo (Laya/Jev/outro) so quando tiers 0+1 falham
//!
//! Stub Phase 0: ContainsSpawn — heuristica `contains("spawn")`
//! em vez de Laya ONNX. Nao integra Laya ainda.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::{DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

/// Tipo de pergunta que a decision layer recebe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuestionType {
    YesNo,
    Choice,
    Score,
}

/// Resposta tipada da decision layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Answer {
    YesNo { yes: bool, confidence: f64 },
    Choice { index: usize, label: String, confidence: f64 },
    Score { value: f64, max: f64, confidence: f64 },
}

impl Answer {
    /// Confianca normalizada 0.0..1.0.
    pub fn confidence(&self) -> f64 {
        match self {
            Answer::YesNo { confidence, .. }
            | Answer::Choice { confidence, .. }
            | Answer::Score { confidence, .. } => *confidence,
        }
    }
}

/// Trait que toda decision layer deve implementar.
pub trait DecisionLayer: Send + Sync {
    /// Faz uma pergunta a camada. Retorna resposta tipada.
    fn ask(&self, _qtype: QuestionType, question: &str) -> Answer;
}

/// Heuristica Phase 0: decide YesNo(true) se a pergunta menciona "spawn".
///
/// Isto NAO e Laya ONNX. E um stub para provar que o trait funciona.
/// Em producao, sera substituido pelo ensemble 3 tiers.
#[derive(Debug, Clone, Default)]
pub struct ContainsSpawn;

impl DecisionLayer for ContainsSpawn {
    fn ask(&self, _qtype: QuestionType, question: &str) -> Answer {
        let keyword = "spawn";
        let hits = question.to_lowercase().contains(keyword);
        Answer::YesNo {
            yes: hits,
            confidence: if hits { 0.95 } else { 0.10 },
        }
    }
}

// ── Tier 0: cache semantico por hash normalizado ──────────────────

/// Chave de cache: hash do texto normalizado.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey(u64);

/// Normaliza texto para comparacao semantica.
fn normalize(text: &str) -> String {
    let lower = text.to_lowercase();
    // Remove espacos duplicados e extremos.
    let mut out = String::new();
    let mut prev_space = false;
    for c in lower.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Tier 0: cache semantico.
///
/// Funcionamento:
/// 1. Normaliza a pergunta (lowercase, espacos unicos)
/// 2. Calcula hash do texto normalizado
/// 3. Se hash existe no cache -> retorna resposta cacheada (0ms, 0 custo)
/// 4. Senao -> retorna `None` (fallback para tier 1)
#[derive(Debug, Clone, Default)]
pub struct Tier0Cache {
    hits: HashMap<CacheKey, Answer>,
}

impl Tier0Cache {
    pub fn new() -> Self {
        Tier0Cache::default()
    }

    /// Insere uma resposta no cache. So chame apos validacao externa.
    pub fn insert(&mut self, question: &str, answer: Answer) {
        let key = CacheKey::from(question);
        self.hits.insert(key, answer);
    }

    /// Lookup O(1). Retorna Some se cache hit (> 0.95 similaridade).
    pub fn get(&self, question: &str) -> Option<Answer> {
        let key = CacheKey::from(question);
        self.hits.get(&key).cloned()
    }

    /// Hit rate (0.0..1.0).
    pub fn hit_rate(&self) -> f64 {
        let total: usize = self.hits.values().map(|a| a.confidence() as usize).sum();
        if total == 0 {
            0.0
        } else {
            total as f64 / self.hits.len() as f64
        }
    }
}

impl From<&str> for CacheKey {
    fn from(text: &str) -> Self {
        let normalized = normalize(text);
        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        CacheKey(hasher.finish())
    }
}

// ── Tier 1: pre-filter ONNX (stub) ───────────────────────────────

/// Tier 1: classificador ONNX pequeno.
///
/// Stub: retorna `ConfidenceResult` sem chamar ONNX.
/// Quando integrado: carrega `models/prefilter.onnx` (~5MB, <5ms).
#[derive(Debug, Clone, Default)]
pub struct Tier1PreFilter;

impl Tier1PreFilter {
    /// Classifica uma pergunta. Stub: assume que nao ha filtro.
    ///
    /// Retorna `Some(threshold)` se o pre-filter decidir (alta confianca),
    /// ou `None` se precisar subir para tier 2.
    pub fn classify(&self, _question: &str) -> Option<f64> {
        None
    }
}

// ─- Tier 2: LLM backend (stub) ──────────────────────────────────

/// Tier 2: interface para LLM completo via MCP backend.
///
/// Stub: retorna resposta generica. Nao faz chamada HTTP.
/// Quando integrado: chama MCP backend (Laya/Jev/outro).
#[derive(Debug, Clone, Default)]
pub struct Tier2LLM;

impl Tier2LLM {
    pub fn query(&self, _question: &str) -> Answer {
        Answer::YesNo {
            yes: true,
            confidence: 0.75,
        }
    }
}

// ── Ensemble 3 tiers ─────────────────────────────────────────────

/// Motor de decisao ensemble: Tier 0 -> Tier 1 -> Tier 2.
#[derive(Debug, Clone, Default)]
pub struct DecisionEngine {
    cache: Tier0Cache,
    prefilter: Tier1PreFilter,
    llm: Tier2LLM,
}

impl DecisionEngine {
    pub fn new() -> Self {
        DecisionEngine::default()
    }

    /// Faz uma pergunta, percorrendo os tiers.
    pub fn ask(&mut self, _qtype: QuestionType, question: &str) -> Answer {
        // Tier 0: cache
        if let Some(ans) = self.cache.get(question) {
            return ans;
        }

        // Tier 1: pre-filter ONNX
        if let Some(threshold) = self.prefilter.classify(question)
            && threshold > 0.95
        {
            return Answer::YesNo {
                yes: true,
                confidence: threshold,
            };
        }

        // Tier 2: LLM
        let answer = self.llm.query(question);

        // Cacheia a resposta para proximas vezes.
        self.cache.insert(question, answer.clone());
        answer
    }

    /// Hit rate do cache tier 0.
    pub fn cache_hit_rate(&self) -> f64 {
        self.cache.hit_rate()
    }
}

// ─- Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_spawn_stub_returns_yes_no() {
        let layer = ContainsSpawn;
        let a = layer.ask(QuestionType::YesNo, "Should I spawn a subagent?");
        match a {
            Answer::YesNo { yes, confidence } => {
                assert!(yes);
                assert!(confidence > 0.9);
            }
            _ => panic!("expected YesNo"),
        }

        let b = layer.ask(QuestionType::YesNo, "What is the weather?");
        match b {
            Answer::YesNo { yes, .. } => assert!(!yes),
            _ => panic!("expected YesNo"),
        }
    }

    #[test]
    fn tier0_cache_returns_none_on_miss() {
        let cache = Tier0Cache::new();
        assert!(cache.get("any question").is_none());
    }

    #[test]
    fn tier0_cache_hits_on_exact_match() {
        let mut cache = Tier0Cache::new();
        let ans = Answer::YesNo {
            yes: true,
            confidence: 0.99,
        };
        cache.insert("spawn a subagent?", ans.clone());
        let hit = cache.get("spawn a subagent?");
        assert_eq!(hit, Some(ans));
    }

    #[test]
    fn tier0_normalizes_whitespace() {
        let mut cache = Tier0Cache::new();
        let ans = Answer::YesNo {
            yes: true,
            confidence: 0.99,
        };
        cache.insert("spawn  a  subagent?", ans.clone());
        // Apos normalizacao, espacos duplicados sao colapsados.
        let hit = cache.get("spawn a subagent?");
        assert!(hit.is_some(), "cache hit apos normalizacao de espacos");
    }

    #[test]
    fn tier0_is_case_insensitive() {
        let mut cache = Tier0Cache::new();
        let ans = Answer::YesNo {
            yes: true,
            confidence: 0.99,
        };
        cache.insert("SPAWN subagent", ans.clone());
        let hit = cache.get("spawn subagent");
        assert!(hit.is_some(), "cache hit case-insensitive");
    }

    #[test]
    fn decision_engine_cache_miss_falls_through_to_tier2() {
        let mut engine = DecisionEngine::new();

        // Cache miss -> tier 1 (None) -> tier 2 (stub: yes=true, 0.75).
        let a = engine.ask(QuestionType::YesNo, "unknown question xyz");
        match a {
            Answer::YesNo { yes, confidence } => {
                assert!(yes, "tier 2 stub retorna yes=true");
                assert!((confidence - 0.75).abs() < 1e-9);
            }
            _ => panic!("expected YesNo"),
        }

        // Segunda chamada: agora e cache hit, mesma resposta.
        let b = engine.ask(QuestionType::YesNo, "unknown question xyz");
        match b {
            Answer::YesNo { yes, confidence } => {
                assert!(yes);
                assert!(
                    (confidence - 0.75).abs() < 1e-9,
                    "cache hit devolve a mesma resposta do tier 2"
                );
            }
            _ => panic!("expected YesNo"),
        }
    }

    #[test]
    fn decision_engine_cache_hit_short_circuits_tier2() {
        let mut engine = DecisionEngine::new();
        // Pre-popula o tier 0 com uma resposta especifica.
        engine.cache.insert(
            "should I spawn a subagent?",
            Answer::YesNo {
                yes: false,
                confidence: 0.99,
            },
        );

        let a = engine.ask(QuestionType::YesNo, "should I spawn a subagent?");
        match a {
            Answer::YesNo { yes, confidence } => {
                assert!(!yes, "cache tem prioridade sobre tier 2");
                assert!((confidence - 0.99).abs() < 1e-9);
            }
            _ => panic!("expected YesNo"),
        }
    }

    #[test]
    fn normalize_collapses_whitespace() {
        let a = normalize("hello   world");
        let b = normalize("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn normalize_lowercases() {
        let a = normalize("SPAWN");
        let b = normalize("spawn");
        assert_eq!(a, b);
    }

    #[test]
    fn answer_confidence_accessor() {
        let a = Answer::YesNo {
            yes: true,
            confidence: 0.88,
        };
        assert_eq!(a.confidence(), 0.88);
    }

    #[test]
    fn tier1_prefilter_stub_returns_none() {
        let pf = Tier1PreFilter::default();
        assert!(pf.classify("anything").is_none());
    }

    #[test]
    fn tier2_llm_stub_returns_default() {
        let llm = Tier2LLM::default();
        let ans = llm.query("anything");
        assert_eq!(ans.confidence(), 0.75);
    }
}
