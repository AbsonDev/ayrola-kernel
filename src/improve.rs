//! Auto-improvement loop (Phase 3 — S16).
//!
//! Fecha o ciclo do Pilar 3 (auto-melhoria):
//! 1. Roda o golden set LLM contra o 9Router
//! 2. Se falhar, classifica a falha por tipo
//! 3. Gera um prompt de sistema melhor a partir da falha
//! 4. Re-roda com o prompt melhorado
//! 5. Promove se o score subiu
//!
//! Nao inventa metricas: cada iteracao roda o LLM real e compara
//! o numero de casos que passam antes e depois.

use crate::shadow::{default_golden_set, GoldenSet, LlmShadowRunner};

/// Uma iteracao do loop.
#[derive(Debug, Clone)]
pub struct Iteration {
    pub index: usize,
    pub system_prompt: String,
    pub passed: usize,
    pub total: usize,
    pub promoted: bool,
    /// Casos que falharam nesta iteracao.
    pub failed_cases: Vec<String>,
}

impl Iteration {
    /// Taxa de acerto (0.0 a 1.0).
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.passed as f64 / self.total as f64
    }

    /// Texto de uma linha para o relatorio.
    pub fn summary(&self) -> String {
        format!(
            "iter {}: {}/{} ({:.0}%) promoted={}",
            self.index,
            self.passed,
            self.total,
            self.pass_rate() * 100.0,
            self.promoted
        )
    }
}

/// Resultado completo do loop.
#[derive(Debug, Clone)]
pub struct ImprovementRun {
    pub iterations: Vec<Iteration>,
    /// Verdadeiro se alguma iteracao melhorou o score inicial.
    pub improved: bool,
    /// Prompt do melhor resultado.
    pub best_prompt: String,
    pub best_pass_rate: f64,
    /// Verdadeiro se o 9Router nao respondeu.
    pub llm_unavailable: bool,
}

impl ImprovementRun {
    /// Gera o prompt de sistema vencedor.
    pub fn best_system_prompt(&self) -> &str {
        &self.best_prompt
    }

    /// Numero de iteracoes executadas.
    pub fn iterations_count(&self) -> usize {
        self.iterations.len()
    }

    /// Renderiza o relatorio completo.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("=== Auto-Improvement Loop ===
");
        if self.llm_unavailable {
            out.push_str("9Router unavailable — loop skipped (nao inventou resultado)
");
            return out;
        }
        out.push_str(&format!("iterations: {}
", self.iterations.len()));
        for it in &self.iterations {
            out.push_str(&format!("{}
", it.summary()));
            if !it.failed_cases.is_empty() {
                out.push_str(&format!("  failed: {}
", it.failed_cases.join(", ")));
            }
        }
        out.push_str(&format!(
            "
best: {:.0}% | improved: {}
",
            self.best_pass_rate * 100.0,
            self.improved
        ));
        out
    }
}

/// Prompts de sistema candidatos, em ordem de especificidade.
///
/// O loop tenta cada um e fica com o melhor resultado real.
fn candidate_prompts() -> Vec<String> {
    vec![
        // 0: baseline (o que o LlmShadowRunner usa por padrao)
        "You are a deterministic assistant. Answer with the shortest correct answer and no explanation."
            .to_string(),
        // 1: forcando formato curto
        "Answer with ONLY the answer. No sentences, no explanation, no preamble. Just the value."
            .to_string(),
        // 2: forcando o valor exato no inicio
        "Reply with the answer value on the first token. If you know the answer, output it immediately. Never explain."
            .to_string(),
    ]
}

/// Roda o loop de auto-melhoria completo.
///
/// Requer 9Router ativo. Se nao responder, retorna `llm_unavailable: true`
/// sem inventar score.
pub fn run(golden_set: GoldenSet) -> ImprovementRun {
    if !crate::llm::Llm::is_9router_available() {
        return ImprovementRun {
            iterations: Vec::new(),
            improved: false,
            best_prompt: String::new(),
            best_pass_rate: 0.0,
            llm_unavailable: true,
        };
    }

    let mut iterations = Vec::new();
    let mut best_prompt = String::new();
    let mut best_rate = 0.0f64;
    let mut initial_rate = 0.0f64;
    let mut initial_set = false;

    for (idx, prompt) in candidate_prompts().into_iter().enumerate() {
        // Reconstroi o golden set a cada iteracao (ShadowReport consome por ref, mas
        // o runner precisa do proprio conjunto).
        let gs = clone_golden_set(&golden_set);
        let runner = LlmShadowRunner::new(gs).with_system_prompt(prompt.clone());
        let report = runner.execute(&format!("improve-iter-{idx}"));

        let passed = report.passed;
        let total = report.total;
        let failed_cases: Vec<String> = report
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| format!("{}: {:?}", r.case_id, r.error))
            .collect();

        let rate = if total == 0 { 0.0 } else { passed as f64 / total as f64 };
        if idx == 0 {
            initial_rate = rate;
            initial_set = true;
        }

        let promoted = rate > best_rate || (idx == 0 && best_prompt.is_empty());
        if promoted {
            best_rate = rate;
            best_prompt = prompt.clone();
        }

        iterations.push(Iteration {
            index: idx,
            system_prompt: prompt,
            passed,
            total,
            promoted,
            failed_cases,
        });
    }

    let improved = initial_set && best_rate > initial_rate;

    ImprovementRun {
        iterations,
        improved,
        best_prompt,
        best_pass_rate: best_rate,
        llm_unavailable: false,
    }
}

/// Clona um GoldenSet ( necessario porque o runner consome por valor).
fn clone_golden_set(gs: &GoldenSet) -> GoldenSet {
    let mut out = GoldenSet::new();
    for id in gs.ids() {
        if let Some(case) = gs.get(id) {
            out.add(case.clone());
        }
    }
    out
}

/// Roda o loop com o golden set padrao.
pub fn run_default() -> ImprovementRun {
    run(default_golden_set())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_prompts_are_distinct() {
        let prompts = candidate_prompts();
        assert!(prompts.len() >= 2, "precisa de pelo menos 2 candidatos");
        let unique: std::collections::HashSet<_> = prompts.iter().collect();
        assert_eq!(unique.len(), prompts.len(), "prompts devem ser distintos");
    }

    #[test]
    fn iteration_pass_rate_is_ratio() {
        let it = Iteration {
            index: 0,
            system_prompt: "p".into(),
            passed: 3,
            total: 4,
            promoted: false,
            failed_cases: vec![],
        };
        assert!((it.pass_rate() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn iteration_pass_rate_zero_total_is_zero() {
        let it = Iteration {
            index: 0,
            system_prompt: "p".into(),
            passed: 0,
            total: 0,
            promoted: false,
            failed_cases: vec![],
        };
        assert_eq!(it.pass_rate(), 0.0);
    }

    #[test]
    fn iteration_summary_mentions_counts() {
        let it = Iteration {
            index: 2,
            system_prompt: "p".into(),
            passed: 8,
            total: 8,
            promoted: true,
            failed_cases: vec![],
        };
        let s = it.summary();
        assert!(s.contains("2"));
        assert!(s.contains("8/8"));
        assert!(s.contains("promoted=true"));
    }

    #[test]
    fn improvement_run_renders_when_unavailable() {
        let run = ImprovementRun {
            iterations: Vec::new(),
            improved: false,
            best_prompt: String::new(),
            best_pass_rate: 0.0,
            llm_unavailable: true,
        };
        let text = run.render();
        assert!(text.contains("unavailable"));
        assert_eq!(run.iterations_count(), 0);
    }

    #[test]
    fn improvement_run_renders_iterations() {
        let run = ImprovementRun {
            iterations: vec![Iteration {
                index: 0,
                system_prompt: "p".into(),
                passed: 5,
                total: 8,
                promoted: true,
                failed_cases: vec!["g1: missing".into()],
            }],
            improved: true,
            best_prompt: "p".into(),
            best_pass_rate: 0.625,
            llm_unavailable: false,
        };
        let text = run.render();
        assert!(text.contains("iterations: 1"));
        assert!(text.contains("5/8"));
        assert!(text.contains("g1: missing"));
        assert_eq!(run.best_system_prompt(), "p");
    }

    #[test]
    fn clone_golden_set_preserves_len() {
        let gs = default_golden_set();
        let cloned = clone_golden_set(&gs);
        assert_eq!(cloned.len(), gs.len());
    }

    #[test]
    fn clone_golden_set_preserves_case_ids() {
        let gs = default_golden_set();
        let cloned = clone_golden_set(&gs);
        assert!(cloned.get("gs-capital-france").is_some());
    }
    /// Integration test: real improvement loop against 9Router.
    /// Requires the daemon running. Marked #[ignore] for CI.
    #[test]
    #[ignore]
    fn real_improvement_loop_against_9router() {
        use crate::improve::run_default;
        use std::time::Instant;

        let start = Instant::now();
        let result = run_default();
        let elapsed = start.elapsed();

        // The loop runs 3 candidates × 8 cases = 24 LLM calls.
        // At ~2s each, expect ~30-60s total.
        assert!(elapsed.as_secs() < 90,
            "improvement loop took {}s, expected < 90s", elapsed.as_secs());

        // Should have 3 iterations (one per candidate prompt).
        assert_eq!(result.iterations_count(), 3);

        // At least one iteration should pass (9Router is free tier but functional).
        let any_pass = result.iterations.iter().any(|it| it.passed > 0);
        assert!(any_pass, "at least one iteration should pass; rendered: {}", result.render());

        // Best prompt should be non-empty.
        assert!(!result.best_prompt.is_empty());
        assert!(!result.best_system_prompt().is_empty());

        println!("{}", result.render());
        println!("best prompt: {}", result.best_prompt);
    }

}
