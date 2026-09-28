//! Latency benchmark: spawn_subagent p50/p95/p99.
//! Gate: WORKFLOW Semana 2 exige p50 < 150ms.

use std::time::Instant;

#[tokio::main]
async fn main() {
    let agent = ayrola_kernel::agent::Agent::new("bench");

    // Warmup
    for _ in 0..10 {
        let _ = agent.spawn_subagent("warmup".to_string()).await;
    }

    let n = 200;
    let mut timings = Vec::with_capacity(n);
    for i in 0..n {
        let t0 = Instant::now();
        let _ = agent.spawn_subagent(format!("task_{}", i)).await;
        timings.push(t0.elapsed().as_nanos() as u64);
    }

    timings.sort_unstable();
    let p50 = timings[n / 2];
    let p95 = timings[n * 95 / 100];
    let p99 = timings[n * 99 / 100];
    let max = timings[n - 1];
    let mean: u64 = timings.iter().sum::<u64>() / n as u64;

    println!("spawn_subagent latency over {} runs", n);
    println!("  mean: {:.3}ms", mean as f64 / 1e6);
    println!("  p50:  {:.3}ms", p50 as f64 / 1e6);
    println!("  p95:  {:.3}ms", p95 as f64 / 1e6);
    println!("  p99:  {:.3}ms", p99 as f64 / 1e6);
    println!("  max:  {:.3}ms", max as f64 / 1e6);

    if p50 < 150_000_000 {
        println!("\nGATE PASS: p50 {:.3}ms < 150ms", p50 as f64 / 1e6);
    } else {
        println!("\nGATE FAIL: p50 {:.3}ms >= 150ms", p50 as f64 / 1e6);
    }
}
