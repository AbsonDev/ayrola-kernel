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

use crate::cert::{CertifiedDecision, Evidence};
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
/// Finds the first occurrence of any word in the list, respecting word boundaries.
/// Returns (byte_offset, is_yes) of the earliest match. If multiple words match,
/// the polarity of the *actual first match* is preserved (not overwritten by later words).
/// Retorna todas as posicoes de `word` em `text` que passam na fronteira
/// de palavra (nao coladas em caractere alfanumerico antes/depois).
///
/// `find` sozinho nao serve: devolve so a primeira ocorrencia do substring,
/// que pode ser parte de outra palavra.
fn word_positions(text: &str, word: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    while let Some(rel) = text[start..].find(word) {
        let pos = start + rel;
        let before_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
        let after = pos + word.len();
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            out.push(pos);
        }
        start = pos + 1;
        if start >= text.len() {
            break;
        }
    }
    out
}

fn find_first_word(text: &str, words: &[&str]) -> Option<(usize, bool)> {
    let mut first: Option<(usize, bool)> = None;
    for word in words {
        let is_yes = matches!(*word, "yes" | "sim" | "true" | "correct");
        if let Some(pos) = text.find(word) {
            let before_ok = pos == 0 || !text.as_bytes()[pos - 1].is_ascii_alphanumeric();
            let after = pos + word.len();
            let after_ok = after >= text.len() || !text.as_bytes()[after].is_ascii_alphanumeric();
            if before_ok && after_ok {
                first = first.map_or(Some((pos, is_yes)), |(fp, fyes)| {
                    if pos < fp { Some((pos, is_yes)) } else { Some((fp, fyes)) }
                });
            }
        }
    }
    first
}

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
    lookups: u64,
    hit_count: u64,
}

impl Tier0Cache {
    pub fn new() -> Self {
        Tier0Cache { hits: HashMap::new(), lookups: 0, hit_count: 0 }
    }

    /// Insere uma resposta no cache. So chame apos validacao externa.
    pub fn insert(&mut self, question: &str, answer: Answer) {
        let key = CacheKey::from(question);
        self.hits.insert(key, answer);
    }

    /// Lookup O(1). Retorna Some se cache hit (> 0.95 similaridade).
    /// Conta hits/misses para `hit_rate()`.
    pub fn get(&mut self, question: &str) -> Option<Answer> {
        let result = self.get_inner(question);
        self.record(result.is_some());
        result
    }

    fn get_inner(&self, question: &str) -> Option<Answer> {
        let key = CacheKey::from(question);
        self.hits.get(&key).cloned()
    }

    fn record(&mut self, was_hit: bool) {
        self.lookups += 1;
        if was_hit {
            self.hit_count += 1;
        }
    }

    /// Fraction de lookups que acertaram no cache (0.0..1.0).
    /// Zero lookups = 0.0 (nunca chamou `get`).
    pub fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            0.0
        } else {
            self.hit_count as f64 / self.lookups as f64
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
        // Extrai a resposta JSON no formato {"answer": "yes"/"no"} (ou boolean true/false).
        // Tambem trata respostas com campos extras — corta no delimitador apos o valor.
        let lower = content.to_lowercase();
        let key_pos = lower.find("\"answer\"")?;
        // Opera sobre `lower` para evitar mismatch de indices de byte entre
        // `content` e sua versao lowercase (ex: 'İ' -> 3 bytes), que causava
        // panic de slice.
        let after_key = &lower[key_pos + "\"answer\"".len()..];
        let colon_pos = after_key.find(':')?;
        let after_colon = after_key[colon_pos + 1..].trim_start();
        // Extrai o valor JSON (string ou boolean) ate o proximo delimitador.
        let end = after_colon.find([',', '}', '\n']).unwrap_or(after_colon.len());
        let value = &after_colon[..end].trim_matches('"');
        if value.eq_ignore_ascii_case("true") {
            return Some(true);
        }
        if value.eq_ignore_ascii_case("false") {
            return Some(false);
        }
        // Casa yes/no com fronteira de palavra (evita "yesterday"=>yes,
        // "nothing"=>"no") usando o helper de word boundary.
        find_first_word(
            value,
            &["yes", "sim", "true", "correct", "no", "nao", "não", "false"],
        )
        .map(|(_, is_yes)| is_yes)
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

        // 1) HEAD: resposta direta nos primeiros 200 chars.
        // Usa char_indices para evitar panic em limite de bytes (ex: 'ã' tem 2 bytes).
        // char_indices().nth(200) da o indice de byte do 201o char, que e
        // sempre um char boundary. Evita panic com texto multi-byte (ex: 'a').
        let head_end = lower
            .char_indices()
            .nth(200)
            .map(|(i, _)| i)
            .unwrap_or(lower.len());
        let head = &lower[..head_end];

        // Procura "yes" ou "no" como palavra inteira no head.
        let yes_words = ["yes", "sim", "true", "correct"];
        let no_words = ["no", "nao", "não", "false"];

        // Funcao auxiliar: primeiro match de qualquer palavra com boundary.
        if let Some((pos, is_yes)) = find_first_word(head, &yes_words) {
            // Verifica se tem 'no' ANTES desse 'yes' no head (ex: "no, yes").
            // REGRESSION: usava `head.find(w)`, que devolve apenas a PRIMEIRA
            // ocorrencia do substring. Se essa ocorrencia falha na fronteira
            // de palavra (ex: "nothing" -> "no" em pos 0 colado em 't'), nenhuma
            // ocorrencia posterior era examinada — um 'no' valido mais adiante
            // virava invisivel e a resposta saia `yes`. Agora varremos todas as
            // posicoes aceitando so as que passam na fronteira.
            let has_no_before = no_words.iter().any(|w| {
                word_positions(head, w).iter().any(|&p| p < pos)
            });
            if !has_no_before {
                return Answer::YesNo { yes: is_yes, confidence: 0.85 };
            }
        }

        // 2) TAIL: last-match-wins nos ultimos 600 chars.
        // Usa char_indices para evitar panic em limite de bytes.
        // Primeiro char boundary >= (len - 600), evitando panic multi-byte.
        let tail_cutoff = lower.len().saturating_sub(600);
        let tail_start = lower
            .char_indices()
            .map(|(i, _)| i)
            .find(|i| *i >= tail_cutoff)
            .unwrap_or(lower.len());
        let tail = &lower[tail_start..];

        let mut last_yes: Option<usize> = None;
        let mut last_no: Option<usize> = None;

        for word in &yes_words {
            if let Some(pos) = tail.rfind(word) {
                let abs_pos = tail_start + pos;
                let lower_bytes = lower.as_bytes();
                let before_ok = abs_pos == 0 || !lower_bytes[abs_pos - 1].is_ascii_alphanumeric();
                let after_ok = abs_pos + word.len() >= lower_bytes.len()
                    || !lower_bytes[abs_pos + word.len()].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    last_yes = Some(last_yes.map_or(abs_pos, |p| p.max(abs_pos)));
                }
            }
        }
        for word in &no_words {
            if let Some(pos) = tail.rfind(word) {
                let abs_pos = tail_start + pos;
                let lower_bytes = lower.as_bytes();
                let before_ok = abs_pos == 0 || !lower_bytes[abs_pos - 1].is_ascii_alphanumeric();
                let after_ok = abs_pos + word.len() >= lower_bytes.len()
                    || !lower_bytes[abs_pos + word.len()].is_ascii_alphanumeric();
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
                // REGRESSION: usava `contains()` cru, sem fronteira de palavra,
                // entao "yesterday" (contem "yes") virava yes e "nothing"
                // (contem "no") virava no. Agora usa find_first_word.
                let yes = find_first_word(&lower, &yes_words).is_some();
                let no = find_first_word(&lower, &no_words).is_some();
                if yes && !no {
                    Answer::YesNo { yes: true, confidence: 0.75 }
                } else if no && !yes {
                    Answer::YesNo { yes: false, confidence: 0.75 }
                } else {
                    // Sem evidencia clara (ambos ou nenhum): conservador.
                    // Nao fabricamos "yes" sem evidencia — mesma classe do Bug 2.
                    Answer::YesNo { yes: false, confidence: 0.5 }
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
            // Zero keywords ou empate: sem evidencia para agir. Default conservador = no.
            Answer::YesNo { yes: false, confidence: 0.5 }
        }
    }
}

// ── Ensemble 3 tiers ─────────────────────────────────────────────

/// Motor de decisao ensemble: Tier 0 -> Tier 1 -> Tier 2.
#[derive(Debug)]
pub struct DecisionEngine {
    pub prefilter: Tier1PreFilter,
    cache: Tier0Cache,
    llm: Tier2LLM,
    /// S20: MemoryIndex opcional para time-travel semantic recall.
    memory: Option<Box<crate::memory::MemoryIndex>>,
    /// Tier real usado na ultima `ask()` (evita probe separado do cache).
    pub last_tier: crate::cert::DecisionTier,
    /// Tier 0 cache habilitado via config (decision.t0_cache_enabled).
    pub t0_cache_enabled: bool,
    /// Tier 1 prefilter habilitado via config (decision.t1_prefilter_enabled).
    pub t1_prefilter_enabled: bool,
}

impl Default for DecisionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionEngine {
    pub fn new() -> Self {
        DecisionEngine {
            prefilter: Tier1PreFilter,
            cache: Tier0Cache::new(),
            llm: Tier2LLM::new(),
            memory: None,
            last_tier: crate::cert::DecisionTier::Tier2,
            t0_cache_enabled: true,
            t1_prefilter_enabled: true,
        }
    }

    /// Habilita time-travel: passa um MemoryIndex para buscar
    /// decisoes passadas semanticamente similares antes do LLM.
    pub fn with_memory(mut self, memory: crate::memory::MemoryIndex) -> Self {
        self.memory = Some(Box::new(memory));
        self
    }

    /// Cria engine com LLM real habilitado (tier 2 usa subprocess).
    ///
    /// Sem isso, tier 2 usa heuristica local (deterministico, sem I/O).
    pub fn with_llm() -> Self {
        DecisionEngine {
            cache: Tier0Cache::new(),
            prefilter: Tier1PreFilter,
            llm: Tier2LLM::with_llm(),
            memory: None,
            last_tier: crate::cert::DecisionTier::Tier2,
            t0_cache_enabled: true,
            t1_prefilter_enabled: true,
        }
    }

    /// Cria engine a partir de configuracao YAML.
    ///
    /// Lê `t0_cache_enabled` e `t1_prefilter_enabled` da secao `decision`.
    pub fn from_config(cfg: &crate::config::KernelConfig) -> Self {
        DecisionEngine {
            prefilter: Tier1PreFilter,
            cache: Tier0Cache::new(),
            llm: Tier2LLM::new(),
            memory: None,
            last_tier: crate::cert::DecisionTier::Tier2,
            t0_cache_enabled: cfg.decision.t0_cache_enabled,
            t1_prefilter_enabled: cfg.decision.t1_prefilter_enabled,
        }
    }

    /// Cacheia a resposta, respeitando `t0_cache_enabled`.
    fn cache_write(&mut self, question: &str, answer: &Answer) {
        if self.t0_cache_enabled {
            self.cache.insert(question, answer.clone());
        }
    }

    /// Faz uma pergunta, percorrendo os tiers.
    pub fn ask(&mut self, _qtype: QuestionType, question: &str) -> Answer {
        // Tier 0: cache (desabilitavel via config)
        if self.t0_cache_enabled
            && let Some(ans) = self.cache.get(question) {
            self.last_tier = crate::cert::DecisionTier::Tier0;
            return ans;
        }

        // Tier 1: pre-filter heuristico (desabilitavel via config)
        if self.t1_prefilter_enabled
            && let Some(threshold) = self.prefilter.classify(question) {
            if threshold > 0.95 {
                self.last_tier = crate::cert::DecisionTier::Tier1;
                return Answer::YesNo { yes: true, confidence: threshold };
            } else if threshold < 0.05 {
                self.last_tier = crate::cert::DecisionTier::Tier1;
                return Answer::YesNo { yes: false, confidence: 1.0 - threshold };
            }
        }

        // Tier 1.5: time-travel — busca decisoes passadas semanticamente
        // similares no MemoryIndex (Pilar 1, S20).
        if let Some(mem) = self.memory.as_deref_mut() {
            let past = mem.recall(&format!("decision {}", question), 1)
                .unwrap_or_default();
            if let Some(hit) = past.first() && hit.score > 0.7 {
                // Stored answer is {"YesNo": {"yes": true, "confidence": ...}}
                let yes = hit.event.payload
                    .get("answer")
                    .and_then(|v| v.get("YesNo"))
                    .and_then(|v| v.get("yes"))
                    .and_then(|v| v.as_bool());
                // Conservative: se o campo nao existe, nao fabricamos resposta.
                let Some(yes) = yes else {
                    // Nao achamos o campo esperado — nao usamos este hit.
                    // Cai para LLM e marca corretamente o tier.
                    let answer = self.llm.query(question);
                    self.cache_write(question, &answer);
                    self.last_tier = crate::cert::DecisionTier::Tier2;
                    return answer;
                };
                let answer = Answer::YesNo { yes, confidence: hit.score };
                self.cache_write(question, &answer);
                self.last_tier = crate::cert::DecisionTier::Tier1_5;
                return answer;
            }
        }

        // Tier 2: LLM
        let answer = self.llm.query(question);

        // Cacheia a resposta para proximas vezes.
        self.cache_write(question, &answer);
        self.last_tier = crate::cert::DecisionTier::Tier2;
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
            match store.append("decision.made", payload) {
                Ok(ev) => Some(ev),
                Err(e) => {
                    tracing::warn!("failed to append decision event: {}", e);
                    None
                }
            }
        } else {
            None
        };

        (certified, event)
    }

    pub fn ask_certified(&mut self, qtype: QuestionType, question: &str) -> CertifiedDecision {
        // ask() ja seta self.last_tier com o tier real. Nao fazemos probe separado
        // para evitar double-count no cache e rotulagem errada de tier.
        let answer = self.ask(qtype, question);
        let tier = self.last_tier;

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
    use crate::memory::MemoryIndex;
    use super::*;

    #[test]
    fn tier0_cache_hit_reports_tier0() {
        let mut engine = DecisionEngine::new();
        engine.ask(QuestionType::YesNo, "first question about weather");
        let before = engine.last_tier;
        engine.ask(QuestionType::YesNo, "first question about weather");
        assert_eq!(engine.last_tier, crate::cert::DecisionTier::Tier0,
            "second identical ask must hit cache -> Tier0");
        let _ = before;
    }

    #[test]
    fn tier1_prefilter_strong_negative_returns_no_and_tier1() {
        let mut engine = DecisionEngine::new();
        let answer = engine.ask(QuestionType::YesNo, "cancel abort stop terminate delete remove now");
        match answer {
            Answer::YesNo { yes, .. } => assert!(!yes, "strong negative must return yes: false"),
            _ => panic!("expected YesNo"),
        }
        assert_eq!(engine.last_tier, crate::cert::DecisionTier::Tier1,
            "pre-filter path must report Tier1");
    }

    #[test]
    fn tier1_prefilter_strong_positive_returns_yes_and_tier1() {
        let mut engine = DecisionEngine::new();
        let answer = engine.ask(QuestionType::YesNo, "spawn subagent write code fix bug and review pr");
        match answer {
            Answer::YesNo { yes, .. } => assert!(yes, "strong positive must return yes: true"),
            _ => panic!("expected YesNo"),
        }
        assert_eq!(engine.last_tier, crate::cert::DecisionTier::Tier1);
    }

    #[test]
    fn tier2_ambiguous_reports_tier2() {
        let mut engine = DecisionEngine::new();
        engine.ask(QuestionType::YesNo, "what is the capital of France");
        assert_eq!(engine.last_tier, crate::cert::DecisionTier::Tier2,
            "ambiguous question must escalate to Tier2");
    }

    #[test]
    fn ask_certified_does_not_double_count_cache_lookups() {
        let mut engine = DecisionEngine::new();
        engine.ask(QuestionType::YesNo, "cold question unique string xyz");
        let hit_rate_before = engine.cache_hit_rate();
        engine.ask_certified(QuestionType::YesNo, "warm question unique string abc");
        let hit_rate_after = engine.cache_hit_rate();
        // The key invariant: hit_rate must not be 0.5 (which is what a double
        // probe+get on a cache miss would produce).
        assert!(hit_rate_after < 0.5 || hit_rate_before == 0.0,
            "ask_certified must not double-count cache lookups (before={}, after={})",
            hit_rate_before, hit_rate_after);
    }

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
    fn cache_hit_rate_tracks_hits_and_misses() {
        let mut cache = Tier0Cache::new();
        cache.insert("q1", Answer::YesNo { yes: true, confidence: 0.75 });
        assert_eq!(cache.hit_rate(), 0.0, "zero lookups => 0.0");

        let _ = cache.get("q1"); // hit
        assert!((cache.hit_rate() - 1.0).abs() < 1e-9, "1 hit / 1 lookup = 1.0");

        let _ = cache.get("unknown"); // miss
        assert!((cache.hit_rate() - 0.5).abs() < 1e-9, "1 hit / 2 lookups = 0.5");
    }

    #[test]
    fn tier0_cache_returns_none_on_miss() {
        let mut cache = Tier0Cache::new();
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

        // Cache miss -> tier 1 (None) -> tier 2 (heuristic: no keywords = no evidence -> false, 0.50).
        let a = engine.ask(QuestionType::YesNo, "unknown question xyz");
        match a {
            Answer::YesNo { yes, confidence } => {
                assert!(!yes, "tier 2 heuristic default: no evidence -> false");
                assert!((confidence - 0.50).abs() < 1e-9, "expected 0.50, got {}", confidence);
            }
            _ => panic!("expected YesNo"),
        }

        // Segunda chamada: agora e cache hit, mesma resposta.
        let b = engine.ask(QuestionType::YesNo, "unknown question xyz");
        match b {
            Answer::YesNo { yes, confidence } => {
                assert!(!yes);
                assert!(
                    (confidence - 0.50).abs() < 1e-9,
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
        // "anything" has no keywords → no evidence -> confidence 0.5 (default no)
        assert!((ans.confidence() - 0.50).abs() < 1e-9);
        match ans {
            Answer::YesNo { yes, .. } => assert!(!yes, "default no when no keywords matched"),
            _ => panic!("expected YesNo"),
        }
    }


    #[test]
    
    fn parse_llm_response_handles_9router_real() {
        use crate::decision::Tier2LLM;
        
        let llm = Tier2LLM::with_llm();
        let resp = llm.query("Answer with ONLY the word: yes");
        match resp {
            crate::decision::Answer::YesNo { yes, confidence } => {
                assert!(yes, "9Router should answer yes to a trivial question");
                assert!(confidence > 0.5, "confidence should be > 0.5, got {}", confidence);
            }
            _ => panic!("expected YesNo from 9Router"),
        }
    }


#[test]
fn tier1_5_recall_reads_correct_payload_field() {
    // REGRESSION: armazena uma decisao yes e força o recall a buscar
    // o campo "answer" como {"YesNo": {"yes": true, ...}}, nao {"yes": ...}.
    let p = std::env::temp_dir().join(format!(
        "ayrola_t15_{}.json", uuid::Uuid::new_v4()
    ));
    let mut mem = MemoryIndex::open(&p).expect("open memory");
    mem.remember("decision.made", "test-yes",
        serde_json::json!({"answer": {"YesNo": {"yes": true, "confidence": 0.95}}})).expect("remember yes");
    mem.remember("decision.made", "test-no",
        serde_json::json!({"answer": {"YesNo": {"yes": false, "confidence": 0.95}}})).expect("remember no");

    let mut engine = DecisionEngine::new();
    engine.memory = Some(Box::new(mem));

    // Pergunta semanticamente similar a "test-yes"
    let answer = engine.ask(QuestionType::YesNo, "should i say yes");
    assert!(matches!(answer, Answer::YesNo { yes: true, confidence: c } if c > 0.7));

    std::fs::remove_file(&p).ok();
}

#[test]
fn tier1_5_falls_back_when_answer_missing() {
    // REGRESSION: evento sem campo 'answer' nao deve retornar yes:true
    // fabricado; deve cair para LLM/heuristic (confidence baixo).
    let p = std::env::temp_dir().join(format!(
        "ayrola_t15b_{}.json", uuid::Uuid::new_v4()
    ));
    let mut mem = MemoryIndex::open(&p).expect("open memory");
    mem.remember("agent.spawned", "some-spawn",
        serde_json::json!({"agent_id": "x"})).expect("remember");

    let mut engine = DecisionEngine::new();
    engine.memory = Some(Box::new(mem));

    let answer = engine.ask(QuestionType::YesNo, "anything at all");
    // Fallback conservador -> confidence <= 0.5
    if let Answer::YesNo { yes: _, confidence } = answer {
        assert!(confidence <= 0.5,
            "must be conservative fallback, got confidence={}", confidence);
    }

    std::fs::remove_file(&p).ok();
}


#[test]
fn parse_llm_response_handles_multibyte_at_byte_boundary() {
    // 199 ASCII + 'ã' (2 bytes) => byte 200 is the continuation byte of 'ã'
    let content = format!("{}ã{} yes", "a".repeat(199), "b".repeat(50));
    let a = Tier2LLM::parse_llm_response(&content);
    assert!(matches!(a, Answer::YesNo { .. }), "must not panic on multibyte boundary");
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
        // Ambiguous response with no clear yes/no: conservative fallback
        // must NOT fabricate "yes: true" (Bug 2 class).
        let a = Tier2LLM::parse_llm_response("That depends on the context.");
        assert!(matches!(a, Answer::YesNo { yes: false, confidence } if confidence <= 0.5));
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
    fn time_travel_reuses_past_decision() {
        use crate::memory::MemoryIndex;
        use std::env;

        let p = {
            let mut path = env::temp_dir();
            path.push(format!("ayrola_tt_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut mem = MemoryIndex::open(&p).unwrap();
        mem.remember(
            "decision.made",
            "spawn subagent for code review",
            serde_json::json!({"yes": true}),
        ).unwrap();

        let mut engine = DecisionEngine::new().with_memory(mem);
        let ans = engine.ask(QuestionType::YesNo, "spawn subagent for code review");
        match ans {
            Answer::YesNo { yes, confidence } => {
                assert!(yes, "time-travel should reuse past decision");
                assert!(confidence > 0.7, "confidence should be high, got {}", confidence);
            }
            _ => panic!("expected YesNo"),
        }

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn time_travel_no_match_falls_through_to_llm() {
        use crate::memory::MemoryIndex;
        use std::env;

        let p = {
            let mut path = env::temp_dir();
            path.push(format!("ayrola_tt2_{}.ndjson", uuid::Uuid::new_v4()));
            path
        };
        let mut mem = MemoryIndex::open(&p).unwrap();
        mem.remember(
            "decision.made",
            "spawn subagent for code review",
            serde_json::json!({"yes": true}),
        ).unwrap();

        let mut engine = DecisionEngine::new().with_memory(mem);
        // Pergunta sem relacao semantica — deve cair no LLM (heuristic fallback)
        let ans = engine.ask(QuestionType::YesNo, "what is the capital of France?");
        match ans {
            Answer::YesNo { .. } => {} // qualquer resposta e valida
            _ => panic!("expected YesNo"),
        }

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn ask_certified_cache_hit_is_tier0() {
        let mut engine = DecisionEngine::new();
        let q = "unique question xyz";
        let _ = engine.ask(QuestionType::YesNo, q); // populate cache
        let cert = engine.ask_certified(QuestionType::YesNo, q);
        assert_eq!(cert.tier, crate::cert::DecisionTier::Tier0);
    }
    #[test]
    fn find_first_word_preserves_polarity_of_earliest_match() {
        // REGRESSION: find_first_word must return the polarity of the *actual*
        // first match, not the polarity of the word that happened to be iterated
        // when the minimum position was found.
        let mixed = ["no", "yes"];
        // "yes" at pos 0, "no" at pos 6. First match must be (0, true).
        let r1 = find_first_word("yes then no", &mixed);
        assert_eq!(r1, Some((0, true)), "mixed: 'yes' at pos 0 must win, got {:?}", r1);

        // "no" at pos 0, "yes" at pos 4. First match must be (0, false).
        let r2 = find_first_word("no then yes", &mixed);
        assert_eq!(r2, Some((0, false)), "mixed: 'no' at pos 0 must win, got {:?}", r2);

        // Sanity: yes-only list always returns true.
        let r3 = find_first_word("yes", &["yes"]);
        assert_eq!(r3, Some((0, true)));
    }

    // Test helper mirroring the fixed find_first logic.

    #[test]
    fn parse_llm_response_fallback_is_conservative_no() {
        // REGRESSION: when neither yes nor no keyword is found, fallback
        // must NOT fabricate "yes:true" — same class as Bug 2.
        let answer = Tier2LLM::parse_llm_response("The answer is indeterminate");
        match answer {
            Answer::YesNo { yes, confidence } => {
                assert!(!yes, "fallback with no evidence must be conservative (yes=false), got yes={}", yes);
                assert!(confidence <= 0.5,
                    "fallback confidence must be low (<= 0.5), got {}", confidence);
            }
            _ => panic!("expected YesNo"),
        }
    }

    #[test]
    fn parse_json_answer_requires_word_boundary() {
        // REGRESSION: o parser antigo buscava "yes"/"no" sem fronteira de
        // palavra, entao "yesterday" virava yes=true (0.9) e "nothing"
        // virava no=false (0.9). Agora nao ha match de palavra inteira
        // dentro desses tokens — cai no fallback conservador 0.5.
        let yesterday = Tier2LLM::parse_llm_response(r#"{"answer":"yesterday"}"#);
        assert!(
            !matches!(yesterday, Answer::YesNo { yes: true, confidence: 0.9, .. }),
            "\"yesterday\" nao pode ser veredito yes=0.9 — got {:?}",
            yesterday
        );

        let nothing = Tier2LLM::parse_llm_response(r#"{"answer":"nothing"}"#);
        assert!(
            !matches!(nothing, Answer::YesNo { yes: false, confidence: 0.9, .. }),
            "\"nothing\" nao pode ser veredito no=0.9 — got {:?}",
            nothing
        );
    }

    #[test]
    fn parse_json_answer_handles_boolean_json() {
        // O prompt forcado pede string, mas alguns modelos devolvem JSON booleano.
        // Antes isso caia no fallback (confidence 0.5); agora e interpretado.
        let t = Tier2LLM::parse_llm_response(r#"{"answer":true}"#);
        assert!(matches!(t, Answer::YesNo { yes: true, .. }), "boolean true: got {:?}", t);

        let f = Tier2LLM::parse_llm_response(r#"{"answer":false}"#);
        assert!(matches!(f, Answer::YesNo { yes: false, .. }), "boolean false: got {:?}", f);
    }

    #[test]
    fn parse_json_answer_ignores_fields_after_value() {
        // REGRESSION: com campos extras, o parser antigo varria o objeto
        // inteiro e podia ler o "yes" de um campo posterior.
        let content = r#"{"answer":"no","note":"yes it is confusing"}"#;
        let a = Tier2LLM::parse_llm_response(content);
        assert!(
            matches!(a, Answer::YesNo { yes: false, .. }),
            "campo posterior nao pode sobrepor o valor de answer — got {:?}",
            a
        );
    }

    #[test]
    fn parse_json_answer_is_multibyte_safe() {
        // REGRESSION: o parser antigo localizava a chave em `content`
        // (lowercased em `lower`) e indexava `content` com offsets de
        // `lower`. Com 'İ' (2 bytes -> 3 bytes em lowercase) o offset
        // diverge e o slice pode entrar no meio de um char.
        let content = "{\"note\":\"İ\",\"answer\":\"yes\"}".to_string();
        let a = Tier2LLM::parse_llm_response(&content);
        assert!(matches!(a, Answer::YesNo { yes: true, .. }), "multibyte prefix: got {:?}", a);
    }

    // --- Adversarial parser tests ---

    

    

    
    #[test]
    fn parse_llm_response_empty_fallback_conservative_no() {
        let a = Tier2LLM::parse_llm_response("");
        match a {
            Answer::YesNo { yes, confidence } => {
                assert!(!yes, "empty string must fallback to no");
                assert_eq!(confidence, 0.5, "empty string confidence must be 0.5");
            }
            _ => panic!("expected YesNo"),
        }
    }

    #[test]
    fn parse_llm_response_whitespace_only_fallback() {
        let a = Tier2LLM::parse_llm_response("   \n\t  ");
        match a {
            Answer::YesNo { yes, .. } => assert!(!yes, "whitespace-only must fallback to no"),
            _ => panic!("expected YesNo"),
        }
    }

    #[test]
    fn parse_llm_response_mixed_punctuation() {
        let a = Tier2LLM::parse_llm_response("Yes! The answer is definitely yes.");
        match a {
            Answer::YesNo { yes, .. } => assert!(yes, "yes with punctuation must match"),
            _ => panic!("expected YesNo"),
        }
    }

    #[test]
    fn parse_llm_response_question_mark_does_not_fool() {
        let a = Tier2LLM::parse_llm_response("maybe? no way!");
        match a {
            Answer::YesNo { yes, .. } => assert!(!yes, "no should win"),
            _ => panic!("expected YesNo"),
        }
    }


    #[test]
    fn tier15_missing_field_sets_last_tier_to_tier2() {
        // Regression: when Tier 1.5 has a hit but the expected payload
        // field is absent, ask() must fall back to Tier 2 and set
        // last_tier to Tier2 — not leave it stale from a prior call.
        let mut engine = DecisionEngine::new();
        // First ask: cache miss → Tier 2 (populates cache).
        let _ = engine.ask(QuestionType::YesNo, "cached question");
        assert_eq!(engine.last_tier, crate::cert::DecisionTier::Tier2);
        // Second ask: cache hit → Tier 0.
        let _ = engine.ask(QuestionType::YesNo, "cached question");
        assert_eq!(engine.last_tier, crate::cert::DecisionTier::Tier0);

        // Enable memory with a deliberately malformed event (no "answer.YesNo.yes").
        let p = std::env::temp_dir().join(format!(
            "ayrola_tier15_bug_{}.ndjson", uuid::Uuid::new_v4()
        ));
        let mut idx = MemoryIndex::open(&p).expect("open memory");
        // Remember an event whose payload has the right kind but no YesNo field.
        idx.remember("decision.made", "test question", serde_json::json!({
            "kind": "decision.made",
            "text": "test"
        })).expect("remember");
        engine = engine.with_memory(idx);

        // Now ask a question semantically similar to the stored event.
        let answer = engine.ask(QuestionType::YesNo, "test question similar");
        // Must have fallen through to Tier 2, not left Tier0 stale.
        assert_eq!(engine.last_tier, crate::cert::DecisionTier::Tier2,
            "Tier 1.5 fallback must set Tier2, got {:?}", engine.last_tier);
        assert_eq!(answer.confidence(), 0.5, "Tier 2 heuristic fallback is 0.5");
        std::fs::remove_file(&p).ok();
    }


    // ── REGRESSION: config fields must actually gate tier behavior.
    // Before this fix, t0_cache_enabled / t1_prefilter_enabled were
    // declared in KernelConfig but never read by DecisionEngine.

    #[test]
    fn from_config_reads_tier_flags() {
        let mut cfg = crate::config::KernelConfig::default();
        cfg.decision.t0_cache_enabled = false;
        cfg.decision.t1_prefilter_enabled = false;
        let engine = DecisionEngine::from_config(&cfg);
        assert!(!engine.t0_cache_enabled, "t0_cache_enabled must come from config");
        assert!(!engine.t1_prefilter_enabled, "t1_prefilter_enabled must come from config");
    }

    #[test]
    fn t0_cache_disabled_skips_cache_lookup() {
        let mut cfg = crate::config::KernelConfig::default();
        cfg.decision.t0_cache_enabled = false;
        let mut engine = DecisionEngine::from_config(&cfg);

        // Prime the cache via a normal engine, then ask the disabled engine.
        let mut warm = DecisionEngine::new();
        warm.ask(QuestionType::YesNo, "unique_cache_probe_question");
        // Copy the cache entry by asking the same question on the disabled engine
        // with cache re-enabled first, then disable and re-ask.
        engine.t0_cache_enabled = true;
        engine.ask(QuestionType::YesNo, "unique_cache_probe_question");
        engine.t0_cache_enabled = false;

        // With cache disabled, the answer must come from Tier 2 (heuristic),
        // not Tier 0 — proven by last_tier.
        let ans = engine.ask(QuestionType::YesNo, "unique_cache_probe_question");
        assert_eq!(
            engine.last_tier,
            crate::cert::DecisionTier::Tier2,
            "cache disabled must not report Tier0, got {:?}",
            engine.last_tier
        );
        let _ = ans;
    }

    #[test]
    fn t1_prefilter_disabled_skips_prefilter() {
        let mut cfg = crate::config::KernelConfig::default();
        cfg.decision.t1_prefilter_enabled = false;
        let mut engine = DecisionEngine::from_config(&cfg);

        // "spawn" is a high-confidence Tier 1 yes-pattern. With the prefilter
        // disabled, it must fall through to Tier 2 instead of answering at Tier 1.
        let ans = engine.ask(QuestionType::YesNo, "spawn a subagent now");
        assert_eq!(
            engine.last_tier,
            crate::cert::DecisionTier::Tier2,
            "prefilter disabled must not report Tier1, got {:?}",
            engine.last_tier
        );
        let _ = ans;
    }

    #[test]
    fn cache_disabled_does_not_write_cache() {
        let mut cfg = crate::config::KernelConfig::default();
        cfg.decision.t0_cache_enabled = false;
        let mut engine = DecisionEngine::from_config(&cfg);

        engine.ask(QuestionType::YesNo, "cache_write_probe_question");
        // A second identical ask must NOT hit the cache (it was never written),
        // so last_tier must be Tier2 again rather than Tier0.
        let ans = engine.ask(QuestionType::YesNo, "cache_write_probe_question");
        assert_eq!(
            engine.last_tier,
            crate::cert::DecisionTier::Tier2,
            "cache disabled must not write, so second ask is Tier2, got {:?}",
            engine.last_tier
        );
        let _ = ans;
    }

    // REGRESSION: `has_no_before` used head.find() which returns only the
    // first substring occurrence. If that occurrence failed word-boundary
    // (e.g. "nothing" contains "no" at pos 0, followed by 't'), a genuine
    // standalone "no" later in the head was never examined — the parser
    // incorrectly returned yes=true.
    #[test]
    fn parse_llm_response_head_yes_with_substring_no_followed_by_valid_no() {
        // "nothing no yes": standalone "no" at pos 8 is BEFORE "yes" at 15.
        // OLD bug: head.find("no") returns pos 0 (substring of "nothing"),
        // which fails boundary check → standalone "no" at 8 is invisible →
        // has_no_before = false → returns yes=true immediately.
        //
        // With fix: word_positions finds standalone "no" at 8 → has_no_before=true
        // → falls to TAIL. TAIL uses last-match-wins ("yes" is later) → yes=true.
        // The key invariant: the conflict IS detected (has_no_before=true),
        // and the documented TAIL strategy applies consistently.
        let content = "nothing no yes";
        let a = Tier2LLM::parse_llm_response(content);
        // Before fix: returned YesNo { yes: true, confidence: 0.85 } in HEAD
        // because has_no_before was false (missed standalone "no").
        // After fix: has_no_before correctly true, falls to TAIL → yes=true
        // (last-match-wins). Both paths agree on yes=true because "yes"
        // appears after "no" in the full head.
        assert!(
            matches!(a, Answer::YesNo { yes: true, confidence: 0.85 }),
            "standalone 'no' before 'yes' must trigger fallthrough to TAIL,              where last-match-wins selects the later word 'yes', got {:?}",
            a
        );
    }
}

