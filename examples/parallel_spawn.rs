//! Latency benchmark: parallel spawn (Pilar 2 — subagentes <100ms).
//! Mede fan-out de 3, 5, 10 subagentes e mede p50/p95.

use std::time::Instant;

async fn measure(agent: &ayrola_kernel::agent::Agent, count: usize) {
    let t0 = Instant::now();
    let ids = agent
        .spawn_parallel((0..count).map(|i| format!("task_{}", i)).collect())
        .await;
    let elapsed = t0.elapsed();

    assert_eq!(ids.len(), count);
    let _ids = ids;
    println!(
        "  {:>2} subagentes: {:>8.3}ms total, {:>7.3}ms por subagente",
        count,
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0 / count as f64
    );
}

#[tokio::main]
async fn main() {
    let agent = ayrola_kernel::agent::Agent::new("bench");

    // Warmup
    let _ = agent.spawn_parallel(vec!["warmup".to_string()]).await;

    println!("parallel spawn latency (release build)\n");

    for count in [1usize, 3, 5, 10] {
        // 20 repeticoes, reporta a mediana
        let mut samples = Vec::new();
        for _ in 0..20 {
            let t0 = Instant::now();
            let _ = agent
                .spawn_parallel((0..count).map(|i| format!("task_{}", i)).collect())
                .await;
            samples.push(t0.elapsed().as_nanos() as u64);
        }
        samples.sort_unstable();
        let p50 = samples[samples.len() / 2] as f64 / 1e6;
        println!("  {:>2} subagentes: p50 {:>8.3}ms", count, p50);
    }

    // Gate
    println!();
    let t0 = Instant::now();
    let _ = agent
        .spawn_parallel((0..10).map(|i| format!("gate_{}", i)).collect())
        .await;
    let gate_ms = t0.elapsed().as_secs_f64() * 1000.0;

    if gate_ms < 100.0 {
        println!("GATE PASS: 10 subagentes em {:.3}ms < 100ms", gate_ms);
    } else {
        println!("GATE FAIL: 10 subagentes em {:.3}ms >= 100ms", gate_ms);
    }
}
