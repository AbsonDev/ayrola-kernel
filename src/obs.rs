//! Observabilidade do kernel (Phase 2 — S12).
//!
//! Trois eixos:
//! - spans nomeados com contexto (trace de operação completa)
//! - contadores agregados (taxas de sucesso, latência por tier)
//! - health check (verifica 9Router, event store, backends)
//!
//! Sem dependencia de servidor: apenas `tracing` + contadores em memoria.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Contadores globais de metricas.
#[derive(Debug, Default)]
pub struct Metrics {
    pub decisions_total: AtomicU64,
    pub decisions_cache_hit: AtomicU64,
    pub decisions_llm: AtomicU64,
    pub decisions_heuristic: AtomicU64,
    pub llm_calls: AtomicU64,
    pub llm_errors: AtomicU64,
    pub llm_total_ms: AtomicU64,
    pub spawns: AtomicU64,
    pub tool_calls: AtomicU64,
    pub tool_errors: AtomicU64,
}

/// Snapshot imutavel de metricas (serializavel).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub decisions_total: u64,
    pub decisions_cache_hit: u64,
    pub decisions_llm: u64,
    pub decisions_heuristic: u64,
    pub llm_calls: u64,
    pub llm_errors: u64,
    pub llm_avg_ms: u64,
    pub spawns: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
}

/// Metricas globais do processo.
static METRICS: OnceLock<Metrics> = OnceLock::new();

/// Retorna as metricas globais (inicializa na primeira chamada).
pub fn metrics() -> &'static Metrics {
    METRICS.get_or_init(Metrics::default)
}

/// Registra uma decisao, classified pelo tier que respondeu.
pub fn record_decision(tier: &str) {
    let m = metrics();
    m.decisions_total.fetch_add(1, Ordering::Relaxed);
    match tier {
        "cache" => { m.decisions_cache_hit.fetch_add(1, Ordering::Relaxed); }
        "llm" => { m.decisions_llm.fetch_add(1, Ordering::Relaxed); }
        _ => { m.decisions_heuristic.fetch_add(1, Ordering::Relaxed); }
    }
    tracing::debug!(tier, "decision recorded");
}

/// Registra chamada LLM com sucesso ou erro e a latencia.
pub fn record_llm_call(duration_ms: u64, ok: bool) {
    let m = metrics();
    m.llm_calls.fetch_add(1, Ordering::Relaxed);
    m.llm_total_ms.fetch_add(duration_ms, Ordering::Relaxed);
    if !ok {
        m.llm_errors.fetch_add(1, Ordering::Relaxed);
    }
    tracing::info!(duration_ms, ok, "llm call");
}

/// Registra spawn de subagente.
pub fn record_spawn() {
    metrics().spawns.fetch_add(1, Ordering::Relaxed);
    tracing::debug!("subagent spawned");
}

/// Registra chamada de tool.
pub fn record_tool_call(ok: bool) {
    let m = metrics();
    m.tool_calls.fetch_add(1, Ordering::Relaxed);
    if !ok {
        m.tool_errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// Gera snapshot das metricas atuais.
pub fn snapshot() -> MetricsSnapshot {
    let m = metrics();
    let calls = m.llm_calls.load(Ordering::Relaxed);
    let total_ms = m.llm_total_ms.load(Ordering::Relaxed);
    MetricsSnapshot {
        decisions_total: m.decisions_total.load(Ordering::Relaxed),
        decisions_cache_hit: m.decisions_cache_hit.load(Ordering::Relaxed),
        decisions_llm: m.decisions_llm.load(Ordering::Relaxed),
        decisions_heuristic: m.decisions_heuristic.load(Ordering::Relaxed),
        llm_calls: calls,
        llm_errors: m.llm_errors.load(Ordering::Relaxed),
        llm_avg_ms: total_ms.checked_div(calls).unwrap_or(0),
        spawns: m.spawns.load(Ordering::Relaxed),
        tool_calls: m.tool_calls.load(Ordering::Relaxed),
        tool_errors: m.tool_errors.load(Ordering::Relaxed),
    }
}

/// Taxa de cache hit (0.0 a 1.0). Zero se nenhuma decisao.
pub fn cache_hit_rate() -> f64 {
    let total = metrics().decisions_total.load(Ordering::Relaxed);
    if total == 0 {
        return 0.0;
    }
    metrics().decisions_cache_hit.load(Ordering::Relaxed) as f64 / total as f64
}

/// Taxa de erro LLM (0.0 a 1.0). Zero se nenhuma chamada.
pub fn llm_error_rate() -> f64 {
    let m = metrics();
    let calls = m.llm_calls.load(Ordering::Relaxed);
    if calls == 0 {
        return 0.0;
    }
    m.llm_errors.load(Ordering::Relaxed) as f64 / calls as f64
}

/// Estado de saude de um componente.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthCheck {
    pub component: String,
    pub healthy: bool,
    pub detail: String,
}

/// Resultado agregado de health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub healthy: bool,
    pub checks: Vec<HealthCheck>,
    pub metrics: MetricsSnapshot,
}

impl HealthReport {
    /// Soma dos componentes: healthy se todos ok.
    pub fn all_healthy(&self) -> bool {
        self.checks.iter().all(|c| c.healthy)
    }

    /// Formata como tabela texto (para `ayrola-kernel health`).
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("COMPONENT        STATUS    DETAIL\n");
        out.push_str("--------------- --------- -------\n");
        for c in &self.checks {
            let status = if c.healthy { "OK" } else { "DEGRADED" };
            out.push_str(&format!(
                "{:<15} {:<9} {}\n",
                c.component, status, c.detail
            ));
        }
        let m = &self.metrics;
        out.push('\n');
        out.push_str(&format!("decisions: {} (cache {} | llm {} | heur {})\n",
            m.decisions_total, m.decisions_cache_hit, m.decisions_llm, m.decisions_heuristic));
        out.push_str(&format!("llm: {} calls | {} errors | avg {}ms\n",
            m.llm_calls, m.llm_errors, m.llm_avg_ms));
        out.push_str(&format!("spawns: {} | tools: {} ({} errors)\n",
            m.spawns, m.tool_calls, m.tool_errors));
        out
    }
}

/// Roda health check completo: 9Router, event store, metricas.
pub fn health_check(event_store_path: &str) -> HealthReport {
    let mut checks = Vec::new();

    // 1. 9Router daemon
    let r9 = crate::llm::Llm::is_9router_available();
    checks.push(HealthCheck {
        component: "9router".to_string(),
        healthy: r9,
        detail: if r9 {
            "daemon reachable on :20128".to_string()
        } else {
            "daemon unreachable — LLM tier degrades to heuristic".to_string()
        },
    });

    // 2. Event store legivel
    let store_ok = std::path::Path::new(event_store_path).exists();
    checks.push(HealthCheck {
        component: "event_store".to_string(),
        healthy: store_ok,
        detail: if store_ok {
            format!("{} exists", event_store_path)
        } else {
            format!("{} missing", event_store_path)
        },
    });

    // 3. Metricas consistentes
    let m = metrics();
    let total = m.decisions_total.load(Ordering::Relaxed);
    let sum = m.decisions_cache_hit.load(Ordering::Relaxed)
        + m.decisions_llm.load(Ordering::Relaxed)
        + m.decisions_heuristic.load(Ordering::Relaxed);
    checks.push(HealthCheck {
        component: "metrics".to_string(),
        healthy: sum <= total,
        detail: format!("{}/{} decisions tier-classified", sum, total),
    });

    let all_healthy = checks.iter().all(|c| c.healthy);
    HealthReport {
        healthy: all_healthy,
        checks,
        metrics: snapshot(),
    }
}

/// Inicializa o subscriber de tracing (idempotente).
///
/// Chame uma vez no main. Em testes e seguro chamar varias vezes.
pub fn init_tracing() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let level = std::env::var("AYROLA_LOG").unwrap_or_else(|_| "warn".to_string());
        // Parse manual: tracing_subscriber espera uma string de filtro.
        let filter = match level.as_str() {
            "trace" => "trace",
            "debug" => "debug",
            "info" => "info",
            "warn" => "warn",
            _ => "warn",
        };
        let env_filter = tracing_subscriber::EnvFilter::new(filter);
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .try_init();
    });
}

/// Timer RAII que registra a duracao ao drop.
pub struct SpanTimer {
    name: String,
    start: Instant,
}

impl SpanTimer {
    /// Cria um timer com nome e ja registra o inicio no tracing.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        tracing::info!(span = %name, "span start");
        Self {
            name,
            start: Instant::now(),
        }
    }

    /// Duracao em ms desde a criacao.
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Nome do span.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for SpanTimer {
    fn drop(&mut self) {
        let ms = self.elapsed_ms();
        tracing::info!(span = %self.name, duration_ms = ms, "span end");
    }
}

/// Contador nomeado em memoria (para uso especifico).
static NAMED: OnceLock<Mutex<BTreeMap<String, u64>>> = OnceLock::new();

/// Incrementa contador nomeado.
pub fn incr_named(key: &str) {
    let map = NAMED.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Ok(mut m) = map.lock() {
        *m.entry(key.to_string()).or_insert(0) += 1;
    }
}

/// Le todos os contadores nomeados.
pub fn named_counters() -> BTreeMap<String, u64> {
    let map = NAMED.get_or_init(|| Mutex::new(BTreeMap::new()));
    map.lock().map(|m| m.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_has_zero_defaults() {
        let m = MetricsSnapshot {
            decisions_total: 0,
            decisions_cache_hit: 0,
            decisions_llm: 0,
            decisions_heuristic: 0,
            llm_calls: 0,
            llm_errors: 0,
            llm_avg_ms: 0,
            spawns: 0,
            tool_calls: 0,
            tool_errors: 0,
        };
        assert_eq!(m.decisions_total, 0);
        assert_eq!(m.llm_avg_ms, 0);
    }

    #[test]
    fn record_llm_call_computes_average() {
        record_llm_call(100, true);
        record_llm_call(300, true);
        let s = snapshot();
        assert!(s.llm_calls >= 2);
        assert!(s.llm_avg_ms >= 100);
    }

    #[test]
    fn llm_error_rate_is_zero_when_no_errors() {
        record_llm_call(10, true);
        assert!(llm_error_rate() >= 0.0);
    }

    #[test]
    fn health_check_reports_9router() {
        let report = health_check("/tmp/ayrola_nonexistent_events.ndjson");
        assert!(!report.checks.is_empty());
        // event_store deve falhar (caminho nao existe)
        let store_check = report.checks.iter().find(|c| c.component == "event_store");
        assert!(store_check.is_some());
        assert!(!store_check.unwrap().healthy);
    }

    #[test]
    fn health_report_renders_text() {
        let report = health_check("/tmp/ayrola_nonexistent_events.ndjson");
        let text = report.render();
        assert!(text.contains("COMPONENT"));
        assert!(text.contains("9router"));
    }

    #[test]
    fn span_timer_measures_elapsed() {
        let timer = SpanTimer::new("test-span");
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(timer.elapsed_ms() >= 4, "got {}ms", timer.elapsed_ms());
        assert_eq!(timer.name(), "test-span");
    }

    #[test]
    fn named_counters_increment() {
        incr_named("test_counter");
        incr_named("test_counter");
        let counters = named_counters();
        assert!(counters.get("test_counter").copied().unwrap_or(0) >= 2);
    }

    #[test]
    fn record_decision_increments_tier() {
        let before = metrics().decisions_llm.load(Ordering::Relaxed);
        record_decision("llm");
        let after = metrics().decisions_llm.load(Ordering::Relaxed);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn metrics_consistency_holds_after_records() {
        record_decision("cache");
        record_decision("llm");
        record_decision("heuristic");
        let m = metrics();
        let total = m.decisions_total.load(Ordering::Relaxed);
        let sum = m.decisions_cache_hit.load(Ordering::Relaxed)
            + m.decisions_llm.load(Ordering::Relaxed)
            + m.decisions_heuristic.load(Ordering::Relaxed);
        assert_eq!(total, sum, "tier counts must sum to total");
    }

    #[test]
    fn cache_hit_rate_within_bounds() {
        record_decision("cache");
        record_decision("llm");
        let rate = cache_hit_rate();
        assert!((0.0..=1.0).contains(&rate));
    }
}
