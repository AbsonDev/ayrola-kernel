//! CLI do Ayrola Kernel.
//!
//! Comandos:
//! - ayrola status: mostra versao e metadados
//! - ayrola decide --question <q> [--type yesno|choice|score]: decision layer
//! - ayrola test: executa testes internos rapidos

use clap::{Parser, Subcommand};
use ayrola_kernel::decision::{DecisionEngine, QuestionType};

/// Ayrola Kernel — Rust-native agent harness (Phase 0 stub)
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
            println!("Phase: 0 (stub — nao usa Laya ONNX)");
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
