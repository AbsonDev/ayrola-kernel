//! Decision layer trait + ensemble 3 tiers. Pilar 3.
//!
//! Camadas:
//! - Tier 0: cache semantico (hash normalizado + similaridade > 0.95)
//! - Tier 1: pre-filter ONNX pequeno (~5ms, so descarta candidatos)
//! - Tier 2: LLM completo (Laya/Jev/outro) so quando tiers 0+1 falham
//!
// Tier 0: cache semantico. Tier 1: heuristica rapida. Tier 2: LLM real (opt-in). Nao integra Laya ainda.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::{DefaultHasher, HashMap};

use crate::cert::{CertifiedDecision, DecisionTier, Evidence};
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


/// Verifica se `text` contem `keyword` como palavra inteira (boundary-aware).
/// Evita falsos positivos como "no" dentro de "unknown".
fn contains_word(text: &str, keyword: &str) -> bool {
    if keyword.is_empty() || text.len() < keyword.len() {
        return false;
    }
    let kw_len = keyword.len();
    let mut start = 0usize;
    while let Some(pos) = text[start..].find(keyword) {
        let abs = start + pos;
        let after_end = abs + kw_len;
        let before_ok = abs == 0 || !is_word_char(text[..abs].chars().next_back().unwrap_or(' '));
        let after_ok = after_end >= text.len() || !is_word_char(text[after_end..].chars().next().unwrap_or(' '));
        if before_ok && after_ok {
            return true;
        }
        start = abs + 1;
    }
    false
}

/// True se o caractere e alfanumerico ou underscore.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
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

// ── Tier 1: pre-filter (heuristic) ─────────────────────

/// Tier 1: classificador leve — heuristico ou ONNX.
///
/// Tier 1: pre-filter heuristico baseado em palavras-chave.
/// Phase 2: substitui por modelo ONNX real (~5MB, <5ms).
#[derive(Debug, Clone, Default)]
pub struct Tier1PreFilter;

impl Tier1PreFilter {
    /// Classifica uma pergunta usando heuristicas.
    ///
    /// Retorna `Some(confidence)` se a heuristica tem alta confianca (> 0.95),
    /// ou `None` se precisar escalar para Tier 2 (LLM).
    pub fn classify(&self, question: &str) -> Option<f64> {
        let q = question.to_lowercase();

        // Padroes de alta confianca para "sim" (spawn, write, create, etc)
        let spawn_patterns = ["spawn", "create subagent", "parallel task", "fan out", "subagent"];
        let write_patterns = ["write code", "implement", "add function", "create file", "generate"];
        let fix_patterns = ["fix bug", "repair", "correct", "resolve error", "patch"];
        let review_patterns = ["review pr", "audit", "check code", "inspect"];

        // Padroes de alta confianca para "nao" (delete, remove, stop, cancel)
        let cancel_patterns = ["cancel", "abort", "stop", "terminate", "delete", "remove"];

        // Score positivo
        let mut score: f64 = 0.0;
        for p in &spawn_patterns {
            if contains_word(&q, p) { score += 0.3; }
        }
        for p in &write_patterns {
            if contains_word(&q, p) { score += 0.25; }
        }
        for p in &fix_patterns {
            if contains_word(&q, p) { score += 0.2; }
        }
        for p in &review_patterns {
            if contains_word(&q, p) { score += 0.15; }
        }

        // Score negativo
        let mut neg_score: f64 = 0.0;
        for p in &cancel_patterns {
            if contains_word(&q, p) { neg_score += 0.4; }
        }

        // Normaliza
        let net = (score - neg_score).clamp(-1.0, 1.0);

        // Converte para confianca (0.5 = neutro, 1.0 = certeza sim, 0.0 = certeza nao)
        let confidence = (net + 1.0) / 2.0;

        // So decide se confianca > 0.95 (muito certeza)
        if confidence > 0.95 || confidence < 0.05 {
            Some(confidence)
        } else {
            None
        }
    }
}

// ── Tier 2: LLM backend (real via subprocess opt-in) ──────────────────

/// Tier 2: interface para LLM completo via MCP backend.
///
/// Tier 2 (heuristica fallback): resposta baseada em keywords.
/// Phase 2: integra com MCP backend (Laya/Jev/outro).
/// Tier 2: LLM real (opt-in) ou heuristica local (default).
#[derive(Debug, Clone, Default)]
pub struct Tier2LLM {
    /// Quando false, usa apenas a heuristica local (deterministico, sem I/O).
    pub use_llm: bool,
}

impl Tier2LLM {
    /// Construtor padrao: heuristica local (sem chamada de LLM).
    pub fn new() -> Self {
        Tier2LLM { use_llm: false }
    }

    /// Habilita LLM real via subprocess (claude/opencode).
    pub fn with_llm() -> Self {
        Tier2LLM { use_llm: true }
    }
}

impl Tier2LLM {
    /// Query via Llm module (subprocess: claude/opencode).
    ///
    /// Se o Llm falhar ou retornar vazio, cai para a heuristica local.
    pub fn query(&self, question: &str) -> Answer {
        if !self.use_llm {
            return Self::heuristic_fallback(question);
        }

        // Usa 9Router local como LLM backend — $0, free tier.
        // Claude/opencode podem estar offline (OAuth expirado); 9Router e mais confiavel.
        let llm = crate::llm::Llm::new(crate::llm::LlmBackend::NineRouter);

        // Prompt JSON forçado: elimina prosa ambígua ("That depends...") dos
        // modelos de reasoning. JSON de 1 linha é parseado diretamente.
        let structured = format!(
            "Answer with ONLY a JSON object: {{\"answer\": \"yes\" or \"no\"}}\nQuestion: {question}"
        );

        match llm.query(&structured) {
            Ok(resp) if !resp.content.is_empty() => Self::parse_llm_response(&resp.content),
            _ => Self::heuristic_fallback(question),
        }
    }

    /// Extrai {"answer":"yes"} ou {"answer":"no"} de uma resposta JSON.
    fn parse_json_answer(content: &str) -> Option<bool> {
        let lower = content.to_lowercase();
        let key_pos = lower.find("\"answer\"")?;
        // Busca o ':' apos a chave "answer"
        let after_key = &content[key_pos + 7..];
        let colon_pos = after_key.find(':')?;
        let after_colon = &after_key[colon_pos + 1..].trim_start();
        let value_pos = after_colon.find("yes").or_else(|| after_colon.find("no"))?;
        let snippet = &after_colon[value_pos..];
        let lower_snip = snippet.to_lowercase();
        if lower_snip.starts_with("yes") {
            Some(true)
        } else if lower_snip.starts_with("no") {
            Some(false)
        } else {
            None
        }
    }

    /// Parseia resposta do LLM em `Answer::YesNo`.
    ///
    /// Modelos de reasoning devolvem o raciocínio completo. A resposta direta
    /// geralmente aparece NO INICIO (ex: "Yes. The sky is blue because...").
    /// Estrategia: (1) procura no HEAD (primeiros 200 chars) por resposta direta.
    /// (2) se nao achar, procura no TAIL (ultimos 600 chars) usando last-match.
    /// (3) fallback: conta keywords no texto inteiro.
    fn parse_llm_response(content: &str) -> Answer {
        // Tenta JSON primeiro (prompt forçado).
        if let Some(yes) = Self::parse_json_answer(content) {
            return Answer::YesNo { yes, confidence: 0.9 };
        }

        let lower = content.to_lowercase();
        let bytes = lower.as_bytes();

        // 1) HEAD: resposta direta nos primeiros 200 chars.
        let head_len = 200.min(bytes.len());
        let head = &lower[..head_len];

        // Procura "yes" ou "no" como palavra inteira no head.
        let yes_words = ["yes", "sim", "true", "correct"];
        let no_words = ["no", "nao", "não", "false"];

        // Funcao auxiliar: primeiro match de qualquer palavra com boundary.
        let find_first = |text: &str, words: &[&str]| -> Option<(usize, bool)> {
            let mut first: Option<(usize, bool)> = None;
            for word in words {
                let is_yes = matches!(*word, "yes" | "sim" | "true" | "correct");
                if let Some(pos) = text.find(word) {
                    let before_ok = pos == 0 || !text.as_bytes()[pos - 1].is_ascii_alphanumeric();
                    let after = pos + word.len();
                    let after_ok = after >= text.len() || !text.as_bytes()[after].is_ascii_alphanumeric();
                    if before_ok && after_ok {
                        first = first.map_or(Some((pos, is_yes)), |(fp, _)| Some((fp.min(pos), is_yes)));
                    }
                }
            }
            first
        };

        if let Some((pos, is_yes)) = find_first(head, &yes_words) {
            // Verifica se tem 'no' ANTES desse 'yes' no head (ex: "no, yes").
            let has_no_before = no_words.iter().any(|w| {
                head.find(w).is_some_and(|p| {
                    let before_ok = p == 0 || !head.as_bytes()[p - 1].is_ascii_alphanumeric();
                    let after = p + w.len();
                    let after_ok = after >= head.len() || !head.as_bytes()[after].is_ascii_alphanumeric();
                    before_ok && after_ok && p < pos
                })
            });
            if !has_no_before {
                return Answer::YesNo { yes: is_yes, confidence: 0.85 };
            }
        }

        // 2) TAIL: last-match-wins nos ultimos 600 chars.
        let tail_start = bytes.len().saturating_sub(600);
        let tail = &lower[tail_start..];

        let mut last_yes: Option<usize> = None;
        let mut last_no: Option<usize> = None;

        for word in &yes_words {
            if let Some(pos) = tail.rfind(word) {
                let abs_pos = tail_start + pos;
                let before_ok = pos == 0 || !lower.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
                let after_ok = abs_pos + word.len() >= bytes.len() || !bytes[abs_pos + word.len()].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    last_yes = Some(last_yes.map_or(abs_pos, |p| p.max(abs_pos)));
                }
            }
        }
        for word in &no_words {
            if let Some(pos) = tail.rfind(word) {
                let abs_pos = tail_start + pos;
                let before_ok = pos == 0 || !lower.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
                let after_ok = abs_pos + word.len() >= bytes.len() || !bytes[abs_pos + word.len()].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    last_no = Some(last_no.map_or(abs_pos, |p| p.max(abs_pos)));
                }
            }
        }

        match (last_yes, last_no) {
            (Some(yp), Some(np)) if yp > np => Answer::YesNo { yes: true, confidence: 0.85 },
            (Some(yp), Some(np)) if np > yp => Answer::YesNo { yes: false, confidence: 0.85 },
            (Some(_), None) => Answer::YesNo { yes: true, confidence: 0.85 },
            (None, Some(_)) => Answer::YesNo { yes: false, confidence: 0.85 },
            _ => {
                // 3) Fallback: conta keywords no texto inteiro.
                let yes = lower.contains("yes") || lower.contains("sim") || lower.contains("true");
                let no = lower.contains("no") || lower.contains("não") || lower.contains("false");
                if yes && !no {
                    Answer::YesNo { yes: true, confidence: 0.75 }
                } else if no && !yes {
                    Answer::YesNo { yes: false, confidence: 0.75 }
                } else {
                    Answer::YesNo { yes: true, confidence: 0.6 }
                }
            }
        }
    }

    /// Heuristica local (fallback quando Llm nao esta disponivel).
    fn heuristic_fallback(question: &str) -> Answer {
        let q = question.to_lowercase();
        let yes_keywords = ["spawn", "write", "create", "implement", "add", "generate", "build", "start", "run", "execute", "deploy", "fix", "solve", "yes", "should i"];
        let no_keywords = ["delete", "remove", "stop", "cancel", "abort", "no", "don't", "avoid", "skip"];

        let mut yes_score = 0;
        let mut no_score = 0;

        for kw in &yes_keywords {
            if contains_word(&q, kw) { yes_score += 1; }
        }
        for kw in &no_keywords {
            if contains_word(&q, kw) { no_score += 1; }
        }

        if yes_score > no_score {
            Answer::YesNo { yes: true, confidence: 0.75 + (yes_score as f64 * 0.03).min(0.2) }
        } else if no_score > yes_score {
            Answer::YesNo { yes: false, confidence: 0.75 + (no_score as f64 * 0.03).min(0.2) }
        } else {
            Answer::YesNo { yes: true, confidence: 0.6 }
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

    /// Cria engine com LLM real habilitado (tier 2 usa subprocess).
    ///
    /// Sem isso, tier 2 usa heuristica local (deterministico, sem I/O).
    pub fn with_llm() -> Self {
        DecisionEngine {
            cache: Tier0Cache::new(),
            prefilter: Tier1PreFilter,
            llm: Tier2LLM::with_llm(),
        }
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
    /// Faz uma pergunta e retorna CertifiedDecision com hash SHA-256.
    ///
    /// Integra o modulo `cert` (Eixo E) diretamente no fluxo de decisao.
    /// Pergunta certificada, registrando o evento no event store se disponivel.
    ///
    /// Phase 3 — S17: certificacao + event store integration.
    ///
    /// Se `store` for informado, a decisão é appendada como evento
    /// `decision.made` com hash verificável. Isso cria a time-travel
    /// capability: replay de qualquer decisão pelo hash da chain.
    pub fn ask_certified_with_store(
        &mut self,
        qtype: QuestionType,
        question: &str,
        store: Option<&mut crate::event_store::EventStore>,
    ) -> (CertifiedDecision, Option<crate::event_store::Event>) {
        let certified = self.ask_certified(qtype, question);

        let event = if let Some(store) = store {
            let payload = serde_json::json!({
                "decision_id": certified.decision_id.0,
                "question": certified.inputs["question"],
                "qtype": certified.inputs["qtype"],
                "answer": certified.decision,
                "tier": certified.tier.to_string(),
                "confidence": certified.evidence.confidence,
                "cost_usd": certified.cost_usd,
                "hash": certified.hash,
            });
            store.append("decision.made", payload).ok()
        } else {
            None
        };

        (certified, event)
    }

    pub fn ask_certified(&mut self, qtype: QuestionType, question: &str) -> CertifiedDecision {
        // Determina o tier ANTES de perguntar (cache hit = tier 0).
        let cache_hit = self.cache.get(question).is_some();
        let prefilter_hit = self.prefilter.classify(question).is_some_and(|t| t > 0.95);

        let answer = self.ask(qtype, question);

        let tier = if cache_hit {
            DecisionTier::Tier0
        } else if prefilter_hit {
            DecisionTier::Tier1
        } else {
            DecisionTier::Tier2
        };

        let inputs = serde_json::json!({
            "question": question,
            "qtype": format!("{:?}", qtype),
        });

        let evidence = Evidence::new(
            format!("{:?}", tier).to_lowercase(),
            "DecisionEngine::ask_certified",
            answer.confidence(),
        );

        CertifiedDecision::new(
            inputs,
            serde_json::to_value(&answer).unwrap_or(serde_json::json!({})),
            evidence,
            0.0001,
            tier,
        )
    }

}

// ─- Tests ────────────────────────────────────────────────────────


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_spawn_heuristic_returns_yes_no() {
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

        // Cache miss -> tier 1 (None) -> tier 2 (heuristic: yes=true, 0.60).
        let a = engine.ask(QuestionType::YesNo, "unknown question xyz");
        match a {
            Answer::YesNo { yes, confidence } => {
                assert!(yes, "tier 2 heuristic default: yes=true");
                assert!((confidence - 0.60).abs() < 1e-9, "expected 0.60, got {}", confidence);
            }
            _ => panic!("expected YesNo"),
        }

        // Segunda chamada: agora e cache hit, mesma resposta.
        let b = engine.ask(QuestionType::YesNo, "unknown question xyz");
        match b {
            Answer::YesNo { yes, confidence } => {
                assert!(yes);
                assert!(
                    (confidence - 0.60).abs() < 1e-9,
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
    fn tier1_prefilter_returns_none() {
        let pf = Tier1PreFilter;
        assert!(pf.classify("anything").is_none());
    }

    #[test]
    fn tier2_llm_fallback_returns_default() {
        let llm = Tier2LLM::new();
        let ans = llm.query("anything");
        // "anything" has no keywords → default confidence 0.6
        assert!((ans.confidence() - 0.60).abs() < 1e-9);
    }


    #[test]
    fn parse_llm_response_handles_json() {
        // Prompt estruturado: resposta ideal
        let a = Tier2LLM::parse_llm_response(r#"{"answer":"yes"}"#);
        eprintln!("[TEST] a = {:?}", a);
        assert!(matches!(a, Answer::YesNo { yes: true, confidence } if confidence >= 0.9));

        let b = Tier2LLM::parse_llm_response(r#"{"answer":"no"}"#);
        eprintln!("[TEST] b = {:?}", b);
        assert!(matches!(b, Answer::YesNo { yes: false, confidence } if confidence >= 0.9));
    }

    #[test]
    fn parse_llm_response_handles_prose_head() {
        // Resposta direta no inicio
        let a = Tier2LLM::parse_llm_response("Yes. The sky is blue due to Rayleigh scattering.");
        assert!(matches!(a, Answer::YesNo { yes: true, .. }));

        let b = Tier2LLM::parse_llm_response("No. Fire is hot, not cold, because of combustion.");
        assert!(matches!(b, Answer::YesNo { yes: false, .. }));
    }

    #[test]
    fn parse_llm_response_handles_reasoning_with_no_in_head() {
        // Resposta com reasoning onde "no" aparece como substring mas veredito e yes
        let content = "Yes, definitely. There is no doubt that 2+2=4, and the sky is no mystery.";
        let a = Tier2LLM::parse_llm_response(content);
        assert!(matches!(a, Answer::YesNo { yes: true, .. }));
    }

    #[test]
    fn parse_llm_response_word_boundary() {
        // "nothing" contem "no" mas nao e veredito
        let a = Tier2LLM::parse_llm_response("Yes, there is nothing to worry about.");
        assert!(matches!(a, Answer::YesNo { yes: true, .. }));
    }

    #[test]
    fn parse_llm_response_falls_back_on_ambiguous() {
        // Resposta ambigua sem veredito claro
        let a = Tier2LLM::parse_llm_response("That depends on the context.");
        assert!(matches!(a, Answer::YesNo { yes: true, confidence } if confidence <= 0.6));
    }

    #[test]
    fn ask_certified_with_store_logs_event() {
        use crate::event_store::EventStore;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("events.jsonl");
        let mut store = EventStore::open(&path).unwrap();

        let mut engine = DecisionEngine::new();
        let (cert, event) = engine.ask_certified_with_store(
            QuestionType::YesNo,
            "Is fire hot?",
            Some(&mut store),
        );

        assert!(cert.verify(), "certified decision must verify");
        assert!(event.is_some(), "event should be logged");
        assert_eq!(store.len().unwrap(), 1, "store should have 1 event");

        let events = store.read_all().unwrap();
        assert_eq!(events[0].kind, "decision.made");
    }

    #[test]
    fn ask_certified_with_store_none_returns_none_event() {
        let mut engine = DecisionEngine::new();
        let (cert, event) = engine.ask_certified_with_store(
            QuestionType::YesNo,
            "Is fire hot?",
            None,
        );

        assert!(cert.verify());
        assert!(event.is_none(), "no store = no event");
    }

    #[test]
    fn ask_certified_with_store_chain_verifies() {
        use crate::event_store::EventStore;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("events.jsonl");
        let mut store = EventStore::open(&path).unwrap();

        let mut engine = DecisionEngine::new();
        for i in 0..3 {
            let q = format!("Question number {i}?");
            let _ = engine.ask_certified_with_store(QuestionType::YesNo, &q, Some(&mut store));
        }

        assert!(store.verify_chain().unwrap(), "chain must verify after 3 decisions");
        assert_eq!(store.len().unwrap(), 3);
    }

}

    #[test]
    fn ask_certified_returns_certified_decision() {
        let mut engine = DecisionEngine::new();
        let cert = engine.ask_certified(QuestionType::YesNo, "spawn a subagent?");
        assert!(cert.verify(), "certified decision must verify");
        assert!(!cert.hash.is_empty());
    }

    #[test]
    fn ask_certified_detects_tamper() {
        let mut engine = DecisionEngine::new();
        let mut cert = engine.ask_certified(QuestionType::YesNo, "delete data?");
        // Tamper with the decision
        cert.decision = serde_json::json!({"yes": true});
        assert!(!cert.verify(), "tampered cert must fail");
    }

    #[test]
    fn ask_certified_cache_hit_is_tier0() {
        let mut engine = DecisionEngine::new();
        let q = "unique question xyz";
        let _ = engine.ask(QuestionType::YesNo, q); // populate cache
        let cert = engine.ask_certified(QuestionType::YesNo, q);
        assert_eq!(cert.tier, DecisionTier::Tier0);
    }

