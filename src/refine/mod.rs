//! Refine loop: critic + pruner + proposer. Auto-melhoria Nivel 3.
//!
//! Ciclo:
//! 1. Critic: avalia mudanca candidata contra o golden set (shadow executor)
//! 2. Pruner: remove codigo morto (funcoes/structs nao usadas)
//! 3. Proposer: gera patch candidato via diff
//! 4. Environment: valida patch em ambiente isolado (cargo check/test)
//!
//! Phase 1: integracao real com shadow executor + cargo.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Command;

use crate::shadow::{GoldenSet, ShadowExecutor, ShadowReport};

/// Resultado da avaliacao de uma mudanca candidata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evaluation {
    pub quality_delta: f64,
    pub latency_delta: f64,
    pub cost_delta: f64,
    pub security_delta: f64,
    pub overall: f64,
    pub passed: bool,
}

/// Heuristica de seguranca: retorna 1.0 se seguro, 0.0 se sinais de risco.
/// Checa: comandos shell inline, hardcoded secrets, network sem allowlist.
fn security_score(content: &str) -> f64 {
    let content = content.to_lowercase();
    let mut score: f64 = 1.0;

    let risky = [
        "password =", "api_key =", "secret =", "token =",
        "execute(", "system(", "shell_exec", "eval(",
        "rm -rf", "curl http", "wget http",
    ];
    for pat in &risky {
        if content.contains(pat) {
            score -= 0.2;
        }
    }

    score.clamp(0.0, 1.0)
}

impl Evaluation {
    /// Retorna true se TODOS os eixos sao positivos (promocao).
    pub fn is_promotable(&self) -> bool {
        self.quality_delta > 0.0
            && self.latency_delta >= 0.0
            && self.cost_delta <= 0.0
            && self.security_delta > 0.5
    }

    /// Score composto: soma ponderada dos eixos.
    pub fn score(&self) -> f64 {
        self.quality_delta * 0.4
            + self.latency_delta * 0.2
            + self.cost_delta * 0.2
            + self.security_delta * 0.2
    }

    /// Cria Evaluation a partir de um ShadowReport.
    pub fn from_shadow_report(report: &ShadowReport) -> Self {
        let total = report.total.max(1) as f64;
        let pass_rate = report.passed as f64 / total;
        let _fail_rate = report.failed as f64 / total;

        // quality: proporcao de casos que passam
        let quality_delta = pass_rate;
        // latency: proxy via taxa de sucesso (tests passing = system fast)
        let latency_delta = pass_rate * 0.5; // max 0.5 (50% do peso do quality)
        // cost: custo por caso executado
        let cost_delta = -(report.total as f64 * 0.001);
        // security: heuristica sobre conteudo dos resultados
        let report_content = report.results.iter()
            .map(|r| r.actual.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let security_delta = security_score(&report_content);

        let overall = quality_delta * 0.4 + latency_delta * 0.2 + cost_delta * 0.2 + security_delta * 0.2;

        Evaluation {
            quality_delta,
            latency_delta,
            cost_delta,
            security_delta,
            overall,
            passed: report.all_passed(),
        }
    }
}

/// Candidata a refinamento: codigo fonte + diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub description: String,
    pub diff: String,
    pub author: String,
}

impl Candidate {
    pub fn new(id: impl Into<String>, description: impl Into<String>, diff: impl Into<String>) -> Self {
        Candidate {
            id: id.into(),
            description: description.into(),
            diff: diff.into(),
            author: "phase1-real".to_string(),
        }
    }
}

/// Critic: avalia candidatos contra o golden set.
#[derive(Debug, Clone)]
pub struct Critic {
    pub evaluations: BTreeMap<String, Evaluation>,
    pub golden_set: GoldenSet,
}

impl Critic {
    pub fn new(golden_set: GoldenSet) -> Self {
        Critic {
            evaluations: BTreeMap::new(),
            golden_set,
        }
    }

    /// Avalia um candidato executando o golden set.
    pub fn evaluate(&mut self, candidate: &Candidate) -> Evaluation {
        let executor = ShadowExecutor::new(self.golden_set.clone());
        let report = executor.execute(&candidate.id);
        let eval = Evaluation::from_shadow_report(&report);
        self.evaluations.insert(candidate.id.clone(), eval.clone());
        eval
    }

    /// Retorna true se o candidato deve ser promovido.
    pub fn should_promote(&self, candidate: &Candidate) -> bool {
        self.evaluations
            .get(&candidate.id)
            .map(|e| e.is_promotable())
            .unwrap_or(false)
    }

    /// Lista todas as avaliacoes.
    pub fn list_evaluations(&self) -> Vec<(&String, &Evaluation)> {
        self.evaluations.iter().collect()
    }
}

/// Pruner: remove codigo morto (funcoes/structs nao usadas).
#[derive(Debug, Clone)]
pub struct Pruner;

impl Default for Pruner {
    fn default() -> Self {
        Self::new()
    }
}

impl Pruner {
    pub fn new() -> Self {
        Self
    }

    /// Analisa codigo fonte Rust e retorna simbolos nao usados.
    ///
    /// Heuristica: um simbolo e considerado morto se aparece apenas uma vez
    /// (sua definicao) e nao e chamado em nenhum outro lugar.
    pub fn find_dead_symbols(&self, source: &str) -> Vec<String> {
        let mut dead = Vec::new();
        let lines: Vec<&str> = source.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            // Procura definicoes de funcoes
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                let name = Self::extract_fn_name(trimmed);
                if let Some(name) = name {
                    // main e entry point — nunca e dead code mesmo se aparece so uma vez
                    if name == "main" {
                        continue;
                    }
                    // Conta ocorrencias no arquivo inteiro
                    let count = source.matches(&name).count();
                    if count <= 1 {
                        dead.push(format!("{} (line {})", name, i + 1));
                    }
                }
            }
            // Procura definicoes de structs
            if trimmed.starts_with("pub struct ") || trimmed.starts_with("struct ") {
                let name = Self::extract_struct_name(trimmed);
                if let Some(name) = name {
                    let count = source.matches(&name).count();
                    if count <= 1 {
                        dead.push(format!("{} (line {})", name, i + 1));
                    }
                }
            }
        }
        dead
    }

    /// Retorna linhas que podem ser removidas (numeros de linha).
    pub fn prune(&self, source: &str) -> Vec<u32> {
        let mut lines_to_remove = Vec::new();
        let lines: Vec<&str> = source.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                let name = Self::extract_fn_name(trimmed);
                if let Some(name) = name {
                    // main e entry point — nunca e codigo morto, mesmo
                    // aparecendo uma unica vez. find_dead_symbols ja faz
                    // esta checagem; prune precisa do mesmo filtro.
                    if name == "main" {
                        continue;
                    }
                    let count = source.matches(&name).count();
                    if count <= 1 {
                        lines_to_remove.push((i + 1) as u32);
                    }
                }
            }
        }
        lines_to_remove
    }

    /// Conta linhas removidas.
    pub fn lines_removed(&self, source: &str) -> usize {
        self.prune(source).len()
    }

    fn extract_fn_name(line: &str) -> Option<String> {
        let start = line.find("fn ")? + 3;
        let rest = &line[start..];
        let end = rest.find('(').unwrap_or(rest.len());
        let name = rest[..end].trim().to_string();
        if name.is_empty() || name.contains(' ') {
            None
        } else {
            Some(name)
        }
    }

    fn extract_struct_name(line: &str) -> Option<String> {
        let start = line.find("struct ")? + 7;
        let rest = &line[start..];
        let end = rest.find([' ', '{', '(']).unwrap_or(rest.len());
        let name = rest[..end].trim().to_string();
        if name.is_empty() || name.contains(' ') {
            None
        } else {
            Some(name)
        }
    }
}

/// Proposer: gera candidatos de mudanca.
#[derive(Debug, Clone, Default)]
pub struct Proposer {
    pub generated: BTreeMap<String, Candidate>,
}

impl Proposer {
    pub fn new() -> Self {
        Proposer::default()
    }

    /// Gera um candidato com diff real (unified diff).
    pub fn propose(&mut self, description: impl Into<String>, diff: impl Into<String>) -> Candidate {
        let id = format!("cand_{}", uuid::Uuid::new_v4());
        let candidate = Candidate::new(&id, description, diff);
        self.generated.insert(id.clone(), candidate.clone());
        candidate
    }

    /// Lista candidatos gerados.
    pub fn list(&self) -> Vec<&Candidate> {
        self.generated.values().collect()
    }
}

/// Ambiente de validacao: executa cargo check/test em um patch.
#[derive(Debug, Clone)]
pub struct Environment {
    pub work_dir: String,
}

impl Environment {
    pub fn new(work_dir: impl Into<String>) -> Self {
        Environment {
            work_dir: work_dir.into(),
        }
    }

    /// Aplica um diff e executa `cargo check`.
    pub fn run(&self, candidate: &Candidate) -> Result<Evaluation, String> {
        // Escreve o diff em um arquivo temporario
        let patch_path = format!("{}/candidate_{}.patch", self.work_dir, candidate.id);
        std::fs::write(&patch_path, &candidate.diff)
            .map_err(|e| format!("failed to write patch: {}", e))?;

        // Aplica o patch
        let output = Command::new("git")
            .args(["apply", &patch_path])
            .current_dir(&self.work_dir)
            .output()
            .map_err(|e| format!("failed to apply patch: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "patch apply failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Executa cargo check
        let check = Command::new("cargo")
            .args(["check"])
            .current_dir(&self.work_dir)
            .output()
            .map_err(|e| format!("cargo check failed: {}", e))?;

        let passed = check.status.success();

        // Reverte o patch
        let _ = Command::new("git")
            .args(["apply", "-R", &patch_path])
            .current_dir(&self.work_dir)
            .output();

        if passed {
            Ok(Evaluation {
                quality_delta: 1.0,
                latency_delta: 0.0,
                cost_delta: -0.001,
                security_delta: 0.0,
                overall: 0.4,
                passed: true,
            })
        } else {
            Err("cargo check failed".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_creation() {
        let c = Candidate::new("c1", "add feature X", "diff here");
        assert_eq!(c.id, "c1");
        assert_eq!(c.description, "add feature X");
    }

    #[test]
    fn critic_evaluate_with_golden_set() {
        let mut gs = GoldenSet::new();
        gs.add(crate::shadow::GoldenCase::new(
            "g1", "test",
            serde_json::json!({"in": 1}),
            serde_json::json!({"in": 1}),
        ));
        let mut critic = Critic::new(gs);
        let c = Candidate::new("c1", "test", "diff");
        let eval = critic.evaluate(&c);
        assert!(eval.passed, "golden set deve passar");
        assert!(eval.quality_delta > 0.0);
    }

    #[test]
    fn critic_should_promote_when_all_pass() {
        let mut gs = GoldenSet::new();
        gs.add(crate::shadow::GoldenCase::new(
            "g1", "test",
            serde_json::json!({"in": 1}),
            serde_json::json!({"in": 1}),
        ));
        let mut critic = Critic::new(gs);
        let c = Candidate::new("c1", "test", "diff");
        critic.evaluate(&c);
        assert!(critic.should_promote(&c));
    }

    #[test]
    fn pruner_finds_dead_function() {
        let pruner = Pruner::new();
        let source = r#"
fn used_function() {}
fn dead_function() {}

fn main() {
    used_function();
}
"#;
        let dead = pruner.find_dead_symbols(source);
        assert!(dead.iter().any(|s| s.contains("dead_function")));
        assert!(!dead.iter().any(|s| s.contains("used_function")));
    }

    #[test]
    fn pruner_finds_dead_struct() {
        let pruner = Pruner::new();
        let source = r#"
struct UsedStruct { x: i32 }
struct DeadStruct { y: i32 }

fn main() {
    let s = UsedStruct { x: 1 };
}
"#;
        let dead = pruner.find_dead_symbols(source);
        assert!(dead.iter().any(|s| s.contains("DeadStruct")));
    }

    #[test]
    fn pruner_returns_line_numbers() {
        let pruner = Pruner::new();
        let source = "fn dead_fn() {}
fn main() {}
";
        let lines = pruner.prune(source);
        assert!(!lines.is_empty());
    }

    #[test]
    fn proposer_generates_unique_ids() {
        let mut proposer = Proposer::new();
        let a = proposer.propose("first", "diff a");
        let b = proposer.propose("second", "diff b");
        assert_ne!(a.id, b.id);
        assert_eq!(proposer.list().len(), 2);
    }

    #[test]
    fn evaluation_from_shadow_report() {
        let mut report = ShadowReport::new("test");
        report.add_result(crate::shadow::ShadowResult::pass("g1", serde_json::json!({"ok": true})));
        report.add_result(crate::shadow::ShadowResult::pass("g2", serde_json::json!({"ok": true})));
        report.promoted = true;

        let eval = Evaluation::from_shadow_report(&report);
        assert!(eval.passed);
        assert!((eval.quality_delta - 1.0).abs() < 1e-9);
    }

    #[test]
    fn evaluation_is_promotable_requires_all_positive() {
        let e = Evaluation {
            quality_delta: 1.0,
            latency_delta: 1.0,
            cost_delta: 1.0,
            security_delta: 1.0,
            overall: 0.0,
            passed: false,
        };
        assert!(!e.is_promotable());
    }

    #[test]
    fn evaluation_security_gate_blocks_risky_candidates() {
        // REGRESSION: security_delta must be > 0.5 to be promotable.
        // Before the fix, `>= 0.0` made the gate a no-op (security_score
        // clamps to [0.0, 1.0], so a maximally risky score of 0.0 still passed).
        let unsafe_eval = Evaluation {
            quality_delta: 1.0,
            latency_delta: 1.0,
            cost_delta: -0.001,
            security_delta: 0.0, // maximally risky
            overall: 0.0,
            passed: true,
        };
        assert!(!unsafe_eval.is_promotable(), "security gate must block unsafe candidates");

        let safe_eval = Evaluation {
            quality_delta: 1.0,
            latency_delta: 1.0,
            cost_delta: -0.001,
            security_delta: 0.8, // safe
            overall: 0.0,
            passed: true,
        };
        assert!(safe_eval.is_promotable(), "safe candidates must still be promotable");
    }

    #[test]
    fn evaluation_score_is_weighted_sum() {
        let e = Evaluation {
            quality_delta: 0.4,
            latency_delta: 0.2,
            cost_delta: -0.2,
            security_delta: 0.2,
            overall: 0.0,
            passed: false,
        };
        assert!((e.score() - 0.20).abs() < 1e-9);
    }

    #[test]
    fn pruner_handles_empty_source() {
        let pruner = Pruner::new();
        assert!(pruner.find_dead_symbols("").is_empty());
        assert!(pruner.prune("").is_empty());
    }

    #[test]
    fn pruner_ignores_main() {
        let pruner = Pruner::new();
        let source = "fn main() {}
";
        let dead = pruner.find_dead_symbols(source);
        // main nao deve ser reportado como dead (entry point)
        assert!(!dead.iter().any(|s| s.contains("main")), "main should not be dead: {:?}", dead);
    }
}    #[test]
    fn prune_ignores_main() {
        // REGRESSION: prune() must skip main, same as find_dead_symbols().
        let pruner = Pruner::new();
        let source = "fn main() { println!(\"hello\"); }
fn unused_helper() {}
";
        let lines = pruner.prune(source);
        assert!(!lines.contains(&1), "main at line 1 must not be pruned, got: {:?}", lines);
        assert!(lines.contains(&2), "unused_helper at line 2 must be pruned, got: {:?}", lines);
    }


