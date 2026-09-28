//! Shadow executor: valida candidatos em paralelo com o sistema atual.
//!
//! Phase 0: stubs + golden set imutável.
//! Phase 1: execução real em sandbox.
//!
//! Fluxo:
//! 1. Recebe candidato (diff + descrição)
//! 2. Executa golden set contra candidato e sistema atual (parallel)
//! 3. Compara resultados
//! 4. Se todos passam → promove
//! 5. Se qualquer falha → rollback + registra falha

use serde::{Deserialize, Serialize};

/// Um teste do golden set (imutável após criação).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoldenCase {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub expected_output: serde_json::Value,
    pub tolerance: f64, // tolerância para valores numéricos
}

impl GoldenCase {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
        expected: serde_json::Value,
    ) -> Self {
        GoldenCase {
            id: id.into(),
            name: name.into(),
            input,
            expected_output: expected,
            tolerance: 0.01,
        }
    }

    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }
}

/// Golden set: conjunto imutável de casos de teste.
#[derive(Debug, Clone, Default)]
pub struct GoldenSet {
    pub cases: std::collections::BTreeMap<String, GoldenCase>,
}

impl GoldenSet {
    pub fn new() -> Self {
        GoldenSet::default()
    }

    pub fn add(&mut self, case: GoldenCase) {
        let id = case.id.clone();
        self.cases.insert(id, case);
    }

    pub fn get(&self, id: &str) -> Option<&GoldenCase> {
        self.cases.get(id)
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

/// Resultado da execução de um caso no shadow executor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShadowResult {
    pub case_id: String,
    pub passed: bool,
    pub actual: serde_json::Value,
    pub expected: serde_json::Value,
    pub error: Option<String>,
}

impl ShadowResult {
    pub fn pass(case_id: impl Into<String>, actual: serde_json::Value) -> Self {
        ShadowResult {
            case_id: case_id.into(),
            passed: true,
            actual: actual.clone(),
            expected: actual,
            error: None,
        }
    }

    pub fn fail(case_id: impl Into<String>, actual: serde_json::Value, expected: serde_json::Value, error: impl Into<String>) -> Self {
        ShadowResult {
            case_id: case_id.into(),
            passed: false,
            actual,
            expected,
            error: Some(error.into()),
        }
    }
}

/// Resultado completo do shadow executor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShadowReport {
    pub candidate_name: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<ShadowResult>,
    pub promoted: bool,
}

impl ShadowReport {
    pub fn new(candidate_name: impl Into<String>) -> Self {
        ShadowReport {
            candidate_name: candidate_name.into(),
            total: 0,
            passed: 0,
            failed: 0,
            results: Vec::new(),
            promoted: false,
        }
    }

    pub fn add_result(&mut self, result: ShadowResult) {
        self.total += 1;
        if result.passed {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        self.results.push(result);
    }

    /// Retorna true se TODOS os casos passaram.
    pub fn all_passed(&self) -> bool {
        self.failed == 0 && self.total > 0
    }

    /// Decide se o candidato deve ser promovido.
    pub fn should_promote(&self) -> bool {
        self.all_passed()
    }
}

/// Shadow executor: executa candidatos contra golden set.
#[derive(Debug, Clone, Default)]
pub struct ShadowExecutor {
    pub golden_set: GoldenSet,
}

impl ShadowExecutor {
    pub fn new(golden_set: GoldenSet) -> Self {
        ShadowExecutor { golden_set }
    }

    /// Executa um candidato contra o golden set.
    ///
    /// Phase 0 (stub): simula execução — sempre passa se o caso tem expected_output.
    /// Phase 1 (Railway VM): executa em sandbox isolado.
    pub fn execute(&self, candidate_name: &str) -> ShadowReport {
        let mut report = ShadowReport::new(candidate_name);

        for (id, case) in &self.golden_set.cases {
            // Phase 0 stub: compara input e expected_output diretamente
            let result = if case.expected_output.is_null() || case.input == case.expected_output {
                ShadowResult::pass(id, case.input.clone())
            } else {
                ShadowResult::fail(
                    id,
                    case.input.clone(),
                    case.expected_output.clone(),
                    "input != expected (stub comparison)",
                )
            };
            report.add_result(result);
        }

        report.promoted = report.should_promote();
        report
    }

    /// Executa candidato e faz rollback se necessário.
    pub fn execute_or_rollback(&self, candidate_name: &str) -> ShadowReport {
        let report = self.execute(candidate_name);
        if !report.promoted {
            // Rollback: em producao, reverte para versao anterior
            // Phase 0: apenas log
            eprintln!(
                "ROLLBACK: candidato '{}' falhou em {}/{} casos",
                candidate_name,
                report.failed,
                report.total
            );
        }
        report
    }
}

/// Circuit breaker para shadow executor: para execucoes repetidamente falhas.
#[derive(Debug, Clone, Default)]
pub struct ShadowCircuitBreaker {
    pub consecutive_failures: u64,
    pub threshold: u64,
    pub tripped: bool,
}

impl ShadowCircuitBreaker {
    pub fn new(threshold: u64) -> Self {
        ShadowCircuitBreaker {
            consecutive_failures: 0,
            threshold,
            tripped: false,
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.tripped = false;
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= self.threshold {
            self.tripped = true;
        }
    }

    pub fn is_open(&self) -> bool {
        self.tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_case(id: &str) -> GoldenCase {
        GoldenCase::new(id, "test", serde_json::json!({"in": 1}), serde_json::json!({"in": 1}))
    }

    fn failing_case(id: &str) -> GoldenCase {
        GoldenCase::new(id, "test", serde_json::json!({"in": 1}), serde_json::json!({"in": 2}))
    }

    #[test]
    fn golden_set_stores_and_retrieves() {
        let mut gs = GoldenSet::new();
        let c = sample_case("c1");
        gs.add(c);
        assert_eq!(gs.get("c1").map(|c| c.name.clone()), Some("test".to_string()));
        assert_eq!(gs.len(), 1);
    }

    #[test]
    fn shadow_execute_all_pass() {
        let mut gs = GoldenSet::new();
        gs.add(sample_case("c1"));
        gs.add(sample_case("c2"));
        let exec = ShadowExecutor::new(gs);
        let report = exec.execute("candidate_a");
        assert!(report.all_passed());
        assert!(report.promoted);
        assert_eq!(report.passed, 2);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn shadow_execute_one_fail() {
        let mut gs = GoldenSet::new();
        gs.add(sample_case("c1"));
        gs.add(failing_case("c2"));
        let exec = ShadowExecutor::new(gs);
        let report = exec.execute("candidate_b");
        assert!(!report.all_passed());
        assert!(!report.promoted);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn shadow_execute_empty_golden_set() {
        let gs = GoldenSet::new();
        let exec = ShadowExecutor::new(gs);
        let report = exec.execute("candidate_c");
        assert!(!report.all_passed(), "empty set must not promote");
    }

    #[test]
    fn shadow_rollback_on_failure() {
        let mut gs = GoldenSet::new();
        gs.add(failing_case("c1"));
        let exec = ShadowExecutor::new(gs);
        let report = exec.execute_or_rollback("candidate_d");
        assert!(!report.promoted);
    }

    #[test]
    fn circuit_breaker_trips_after_threshold() {
        let mut cb = ShadowCircuitBreaker::new(3);
        cb.record_failure();
        cb.record_failure();
        assert!(!cb.is_open());
        cb.record_failure();
        assert!(cb.is_open());
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let mut cb = ShadowCircuitBreaker::new(2);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.is_open());
        cb.record_success();
        assert!(!cb.is_open());
    }

    #[test]
    fn shadow_result_factory_methods() {
        let pass = ShadowResult::pass("c1", serde_json::json!({"ok": true}));
        assert!(pass.passed);
        assert!(pass.error.is_none());

        let fail = ShadowResult::fail("c1", serde_json::json!({}), serde_json::json!({"ok": true}), "mismatch");
        assert!(!fail.passed);
        assert!(fail.error.is_some());
    }

    #[test]
    fn golden_case_tolerance() {
        let c = GoldenCase::new("c1", "test", serde_json::json!({"v": 1.0}), serde_json::json!({"v": 1.0}))
            .with_tolerance(0.05);
        assert_eq!(c.tolerance, 0.05);
    }

    #[test]
    fn shadow_report_serializes() {
        let mut gs = GoldenSet::new();
        gs.add(sample_case("c1"));
        let exec = ShadowExecutor::new(gs);
        let report = exec.execute("test");
        let json = serde_json::to_string(&report).unwrap();
        let back: ShadowReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.passed, report.passed);
    }
}
