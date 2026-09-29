//! CLI do Ayrola Kernel.
//!
//! Comandos:
//! - ayrola status: mostra versao e metadados
//! - ayrola decide --question <q> [--type yesno|choice|score]: decision layer
//! - ayrola test: executa testes internos rapidos

use clap::{Parser, Subcommand};
use ayrola_kernel::decision::{DecisionEngine, QuestionType};
use ayrola_kernel::obs::{health_check, init_tracing};

/// Ayrola Kernel — Rust-native agent harness (Phase 1)
#[derive(Parser)]
#[command(name = "ayrola")]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Status do kernel
    Status,
    /// Usa a decision layer para responder uma pergunta
    Decide {
        /// A pergunta
        #[arg(short, long)]
        question: String,
        /// Tipo de resposta
        #[arg(short = 't', long, default_value = "yesno")]
        qtype: String,
        /// Habilita LLM real (claude/opencode) no tier 2
        #[arg(long, default_value = "false")]
        llm: bool,
        /// Desabilita time-travel via MemoryIndex
        #[arg(long, default_value = "false")]
        no_memory: bool,
    },
    /// Indexa uma memoria (texto livre + payload)
    Remember {
        /// Tipo do evento (ex: decision.made)
        kind: String,
        /// Texto livre para busca semantica
        text: String,
        /// Payload JSON opcional
        #[arg(long, default_value = "{}")]
        payload: String,
    },
    /// Mostra estatisticas do indice de memoria
    Memory,
    /// Busca memorias por similaridade semantica (TF-IDF)
    Recall {
        /// Query de busca
        query: String,
        /// Numero de resultados
        #[arg(long, default_value_t = 5)]
        top_k: usize,
    },
    /// Cria agente e spawn de subagente (demonstracao)
    Spawn {
        /// Tarefa do subagente
        #[arg(short, long)]
        task: String,
    },
    /// Executa o benchmark de throughput (req/s)
    Throughput {
        /// Numero de iteracoes
        #[arg(long, default_value_t = 1000)]
        iterations: usize,
    },
    /// Executa a suite de benchmark (10 tasks)
    Bench {
        /// Salva resultado em JSON
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Roda todos os gates: test, clippy, build, doc
    Doctor,
    /// Health check: 9Router, event store, metricas
    Health,
    /// Roda golden set contra o LLM real (9Router)
    Shadow,
    /// Roda golden set de CODIGO no sandbox (Railway VM)
    CodeShadow,
}


/// Roda todos os gates do projeto e reporta status.
async fn run_doctor() {
    use std::process::Command;

    println!("Ayrola Kernel — Doctor\n");

    // 1. Tests
    println!("[1/4] cargo test ...");
    let test = Command::new("cargo").args(["test", "--", "--test-threads=4"]).output();
    match test {
        Ok(o) if o.status.success() => println!("  {} tests: PASS", count_tests(&String::from_utf8_lossy(&o.stdout))),
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let count = count_tests(&out);
            println!("  {} tests: FAIL (see output below)", count);
            print_test_failures(&out);
        }
        Err(e) => println!("  FAIL: {}", e),
    }

    // 2. Clippy
    println!("\n[2/4] cargo clippy -- -D warnings ...");
    let clippy = Command::new("cargo").args(["clippy", "--", "-D", "warnings"]).output();
    match clippy {
        Ok(o) if o.status.success() => println!("  CLEAN"),
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stderr);
            println!("  FAIL");
            for line in out.lines().take(20) {
                if !line.trim().is_empty() {
                    println!("    {}", line);
                }
            }
        }
        Err(e) => println!("  FAIL: {}", e),
    }

    // 3. Build
    println!("\n[3/4] cargo build --release ...");
    let build = Command::new("cargo").args(["build", "--release"]).output();
    match build {
        Ok(o) if o.status.success() => println!("  OK"),
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stderr);
            println!("  FAIL");
            for line in out.lines().take(20) {
                if !line.trim().is_empty() {
                    println!("    {}", line);
                }
            }
        }
        Err(e) => println!("  FAIL: {}", e),
    }

    // 4. Doc
    println!("\n[4/4] cargo doc --no-deps --document-private-items ...");
    let doc = Command::new("cargo").args(["doc", "--no-deps", "--document-private-items"]).output();
    match doc {
        Ok(o) if o.status.success() => println!("  OK"),
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stderr);
            println!("  FAIL");
            for line in out.lines().take(20) {
                if !line.trim().is_empty() {
                    println!("    {}", line);
                }
            }
        }
        Err(e) => println!("  FAIL: {}", e),
    }

    println!("\nDoctor complete.");
}

/// Counts .rs files under `src/`, recursing into subdirectories.
/// Non-recursive counting reported 7 of 24 files, understating the codebase.
fn count_src_files() -> usize {
    fn walk(dir: &std::path::Path) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        entries.filter_map(|e| e.ok()).map(|e| {
            let path = e.path();
            if path.is_dir() {
                walk(&path)
            } else if path.extension().map(|ext| ext == "rs").unwrap_or(false) {
                1
            } else {
                0
            }
        }).sum()
    }
    walk(std::path::Path::new("src"))
}

fn count_tests(output: &str) -> usize {
    output.lines()
        .filter(|l| l.contains("test result:"))
        .map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0)
        })
        .sum()
}

fn print_test_failures(output: &str) {
    let mut in_failures = false;
    for line in output.lines() {
        if line.contains("failures:") {
            in_failures = true;
        }
        if in_failures && !line.trim().is_empty() && !line.starts_with("test result") {
            println!("    {}", line);
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status => {
            let info = ayrola_kernel::KernelInfo::info();
            println!("Ayrola Kernel v{}", info.version);
            println!("Edition: {}", info.edition);
            println!("Decision tiers: {}", info.tiers);
            let src_files = count_src_files();
            println!("Phase: 1 (real — opt-in LLM via --llm, {} src files, {} commits)",
                src_files, info.commits);
        }
        Commands::Decide { question, qtype, llm, no_memory } => {
            let qtype = match qtype.as_str() {
                "yesno" => QuestionType::YesNo,
                "choice" => QuestionType::Choice,
                "score" => QuestionType::Score,
                _ => {
                    eprintln!("Unknown qtype: {}. Use yesno|choice|score", qtype);
                    std::process::exit(1);
                }
            };

            // Usa DecisionEngine: with_llm() habilita LLM real (subprocess).
            // Time-travel via MemoryIndex quando disponível (AYROLA_EVENT_STORE).
            let mut engine = if llm {
                DecisionEngine::with_llm()
            } else {
                DecisionEngine::new()
            };
            if !no_memory {
                let p = std::env::var("AYROLA_EVENT_STORE")
                    .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
                if let Ok(mem) = ayrola_kernel::memory::MemoryIndex::open(&p) {
                    engine = engine.with_memory(mem);
                }
            }
            let answer = engine.ask(qtype, &question);
            match answer {
                ayrola_kernel::decision::Answer::YesNo { yes, confidence } => {
                    println!("Yes: {} | Confidence: {:.2}", yes, confidence);
                }
                ayrola_kernel::decision::Answer::Choice { index, label, confidence } => {
                    println!("Choice: {} | Label: {} | Confidence: {:.2}", index, label, confidence);
                }
                ayrola_kernel::decision::Answer::Score { value, max, confidence } => {
                    println!("Score: {}/{} | Confidence: {:.2}", value, max, confidence);
                }
            }
        }
        Commands::Throughput { iterations } => {
            let results = ayrola_kernel::bench::throughput_bench(iterations);
            println!("{}", ayrola_kernel::bench::render_throughput(&results));
        }
        Commands::Bench { output } => {
            let suite = ayrola_kernel::bench::default_suite();
            let sb = ayrola_kernel::bench::run_suite(&suite);
            println!("{}", sb.summary());
            if let Some(path) = output {
                match sb.save_json(&path) {
                    Ok(_) => println!("Saved to {}", path),
                    Err(e) => eprintln!("Failed to save: {}", e),
                }
            }
        }
        Commands::CodeShadow => {
            use ayrola_kernel::shadow::{default_code_golden_set, CodeShadowRunner};
            let cases = default_code_golden_set();
            println!("Running {} code golden cases on Railway VM...", cases.len());
            let runner = CodeShadowRunner::remote(cases);
            let report = runner.execute();
            println!("{}", report.render());
            if !report.promoted {
                std::process::exit(1);
            }
        }
        Commands::Shadow => {
            use ayrola_kernel::shadow::{default_golden_set, LlmShadowRunner};
            let gs = default_golden_set();
            println!("Running {} golden cases against 9Router...", gs.len());
            let runner = LlmShadowRunner::new(gs);
            let report = runner.execute("ayrola-shadow");
            println!("Shadow: {}/{} passed (promoted: {})", report.passed, report.total, report.promoted);
            for r in &report.results {
                let status = if r.passed { "PASS" } else { "FAIL" };
                println!("  [{}] {}: {:?}", status, r.case_id, r.error);
            }
            if !report.promoted {
                std::process::exit(1);
            }
        }
        Commands::Health => {
            init_tracing();
            let event_path = std::env::var("AYROLA_EVENT_STORE")
                .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
            let report = health_check(&event_path);
            println!("{}", report.render());
            if !report.all_healthy() {
                std::process::exit(1);
            }
        }
        Commands::Doctor => {
            run_doctor().await;
        }
        Commands::Remember { kind, text, payload } => {
            let p = std::env::var("AYROLA_EVENT_STORE")
                .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
            let mut idx = ayrola_kernel::memory::MemoryIndex::open(&p)
                .expect("failed to open memory index");
            let payload_json: serde_json::Value = serde_json::from_str(&payload)
                .unwrap_or(serde_json::json!({}));
            let event = idx.remember(&kind, &text, payload_json)
                .expect("remember failed");
            println!("remembered: seq={} kind={}", event.seq, event.kind);
        }
        Commands::Memory => {
            let p = std::env::var("AYROLA_EVENT_STORE")
                .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
            let idx = ayrola_kernel::memory::MemoryIndex::open(&p)
                .expect("failed to open memory index");
            let len = idx.len().expect("len failed");
            let empty = idx.is_empty().expect("is_empty failed");
            let snaps = idx.snapshots();
            let verified = idx.verify().expect("verify failed");

            println!("MemoryIndex {{");
            println!("  events: {}", len);
            println!("  empty: {}", empty);
            println!("  snapshots: {}", snaps.len());
            println!("  chain_verified: {}", verified);
            println!("}}");
        }
        Commands::Recall { query, top_k } => {
            let p = std::env::var("AYROLA_EVENT_STORE")
                .unwrap_or_else(|_| "/tmp/ayrola-events.ndjson".to_string());
            let idx = ayrola_kernel::memory::MemoryIndex::open(&p)
                .expect("failed to open memory index");
            let results = idx.recall(&query, top_k)
                .expect("recall failed");
            for r in results {
                println!("[score={:.3}] {}: {}", r.score, r.event.kind, r.event.payload);
            }
        }
        Commands::Spawn { task } => {
            println!("Spawning subagent for: {}", task);
            let mut engine = ayrola_kernel::decision::DecisionEngine::new();
            let agent = ayrola_kernel::agent::Agent::new("demo");
            let child_id = agent.spawn_subagent(task.clone()).await;
            println!("Spawned subagent: {}", child_id);

            // Usa decision layer para validar.
            let q = format!("should I spawn a subagent for {}?", task);
            let ans = engine.ask(QuestionType::YesNo, &q);
            if let ayrola_kernel::decision::Answer::YesNo { yes, .. } = ans {
                println!("Decision layer approved: {}", yes);
            }
        }
    }
}
