//! Refine loop: critic + pruner + proposer. Auto-melhoria Nivel 3.
//!
//! Ciclo:
//! 1. Critic: avalia mudanca candidata contra o golden set
//! 2. Pruner: remove codigo morto (funcoes/structs nao usadas)
//! 3. Proposer: gera patch candidato via diff
//! 4. Environment: valida patch em ambiente isolado (stub Phase 0)
//!
//! Phase 0: stubs sem integracao LLM real.
//! Phase 1: integra com LLM backend via MCP.

use serde::{Deserialize, Serialize};

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

impl Evaluation {
    /// Retorna true se TODOS os eixos sao positivos (promocao).
    pub fn is_promotable(&self) -> bool {
        self.quality_delta > 0.0
            && self.latency_delta >= 0.0
            && self.cost_delta <= 0.0
            && self.security_delta >= 0.0
    }

    /// Score composto: soma ponderada dos eixos.
    pub fn score(&self) -> f64 {
        self.quality_delta * 0.4
            + self.latency_delta * 0.2
            + self.cost_delta * 0.2
            + self.security_delta * 0.2
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
            author: "phase0-stub".to_string(),
        }
    }
}

/// Critic: avalia candidatos e decide se devem ser promovidos.
#[derive(Debug, Clone, Default)]
pub struct Critic {
    pub evaluations: std::collections::BTreeMap<String, Evaluation>,
}

impl Critic {
    pub fn new() -> Self {
        Critic::default()
    }

    /// Avalia um candidato. Stub: retorna Evaluation neutra.
    pub fn evaluate(&mut self, candidate: &Candidate) -> Evaluation {
        let eval = Evaluation {
            quality_delta: 0.0,
            latency_delta: 0.0,
            cost_delta: 0.0,
            security_delta: 0.0,
            overall: 0.0,
            passed: false,
        };
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
}

/// Pruner: remove codigo morto.
#[derive(Debug, Clone, Default)]
pub struct Pruner;

impl Pruner {
    pub fn new() -> Self {
        Pruner
    }

    /// Analisa codigo fonte e retorna linhas que podem ser removidas.
    /// Stub: retorna vazio (nenhuma remocao em Phase 0).
    pub fn prune(&self, _source: &str) -> Vec<u32> {
        Vec::new()
    }

    /// Conta linhas removidas.
    pub fn lines_removed(&self, _source: &str) -> usize {
        0
    }
}

/// Proposer: gera candidatos de mudanca.
#[derive(Debug, Clone, Default)]
pub struct Proposer {
    pub generated: std::collections::BTreeMap<String, Candidate>,
}

impl Proposer {
    pub fn new() -> Self {
        Proposer::default()
    }

    /// Gera um candidato stub. Em producao: chama LLM backend.
    pub fn propose(&mut self, description: impl Into<String>) -> Candidate {
        let id = format!("cand_{}", uuid::Uuid::new_v4());
        let candidate = Candidate::new(&id, description, "diff stub");
        self.generated.insert(id.clone(), candidate.clone());
        candidate
    }

    /// Lista candidatos gerados.
    pub fn list(&self) -> Vec<&Candidate> {
        self.generated.values().collect()
    }
}

/// Ambiente de validacao (stub).
#[derive(Debug, Clone, Default)]
pub struct Environment;

impl Environment {
    pub fn new() -> Self {
        Environment
    }

    /// Executa candidato no ambiente isolado.
    /// Stub: sempre retorna sucesso.
    pub fn run(&self, _candidate: &Candidate) -> Result<Evaluation, String> {
        Ok(Evaluation {
            quality_delta: 0.0,
            latency_delta: 0.0,
            cost_delta: 0.0,
            security_delta: 0.0,
            overall: 0.0,
            passed: false,
        })
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
    fn critic_evaluate_returns_neutral() {
        let mut critic = Critic::new();
        let c = Candidate::new("c1", "test", "diff");
        let eval = critic.evaluate(&c);
        assert!(!eval.passed, "stub sem LLM nao promove");
    }

    #[test]
    fn proposer_generates_unique_ids() {
        let mut proposer = Proposer::new();
        let a = proposer.propose("first");
        let b = proposer.propose("second");
        assert_ne!(a.id, b.id);
        assert_eq!(proposer.list().len(), 2);
    }

    #[test]
    fn pruner_returns_no_lines_in_stub() {
        let pruner = Pruner::new();
        assert_eq!(pruner.prune("fn main() {}"), Vec::<u32>::new());
        assert_eq!(pruner.lines_removed("fn main() {}"), 0);
    }

    #[test]
    fn environment_run_returns_result() {
        let env = Environment::new();
        let c = Candidate::new("c1", "test", "diff");
        assert!(env.run(&c).is_ok());
    }

    #[test]
    fn evaluation_is_promotable_requires_all_positive() {
        let e = Evaluation {
            quality_delta: 1.0,
            latency_delta: 1.0,
            cost_delta: 1.0,  // custo AUMENTOU (ruim)
            security_delta: 1.0,
            overall: 0.0,
            passed: false,
        };
        assert!(!e.is_promotable(), "cost_delta positivo (aumento) deve bloquear");
    }

    #[test]
    fn evaluation_score_is_weighted_sum() {
        let e = Evaluation {
            quality_delta: 0.4,
            latency_delta: 0.2,
            cost_delta: -0.2,  // custo diminuiu (bom)
            security_delta: 0.2,
            overall: 0.0,
            passed: false,
        };
        // 0.4*0.4 + 0.2*0.2 + (-0.2)*0.2 + 0.2*0.2 = 0.16 + 0.04 - 0.04 + 0.04 = 0.20
        assert!((e.score() - 0.20).abs() < 1e-9);
    }
}
