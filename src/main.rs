//! CLI do Ayrola Kernel.
//!
//! Comandos:
//! - ayrola status: mostra versao e metadados
//! - ayrola decide --question <q> [--type yesno|choice|score]: decision layer
//! - ayrola test: executa testes internos rapidos

use clap::{Parser, Subcommand};
use ayrola_kernel::decision::{DecisionEngine, QuestionType};

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
    },
    /// Cria agente e spawn de subagente (demonstracao)
    Spawn {
        /// Tarefa do subagente
        #[arg(short, long)]
        task: String,
    },
    /// Executa a suite de benchmark (10 tasks)
    Bench {
        /// Salva resultado em JSON
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Roda todos os gates: test, clippy, build, doc
    Doctor,
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
            println!("Phase: 1 (real — opt-in LLM via --llm, 15 modulos)");
        }
        Commands::Decide { question, qtype, llm } => {
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
            let mut engine = if llm {
                DecisionEngine::with_llm()
            } else {
                DecisionEngine::new()
            };
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
        Commands::Doctor => {
            run_doctor().await;
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
