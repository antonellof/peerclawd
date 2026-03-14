//! `peerclawd chat` command - Interactive AI chat.

use clap::Args;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::{ExecutionTask, InferenceTask, TaskData};
use crate::identity::NodeIdentity;
use crate::runtime::Runtime;

#[derive(Args)]
pub struct ChatArgs {
    /// Model to use for chat
    #[arg(long, default_value = "llama-3.2-3b")]
    pub model: String,

    /// System prompt
    #[arg(long, default_value = "You are a helpful AI assistant.")]
    pub system: String,

    /// Maximum tokens per response
    #[arg(long, default_value = "500")]
    pub max_tokens: u32,

    /// Temperature for sampling (0.0 - 2.0)
    #[arg(long, default_value = "0.7")]
    pub temperature: f32,

    /// Use distributed inference (offload to network if needed)
    #[arg(long)]
    pub distributed: bool,
}

pub async fn run(args: ChatArgs) -> anyhow::Result<()> {
    println!("=== PeerClaw'd AI Chat ===");
    println!("Model: {}", args.model);
    println!("Max tokens: {}", args.max_tokens);
    println!("Temperature: {}", args.temperature);
    if args.distributed {
        println!("Mode: Distributed (will use network peers if needed)");
    } else {
        println!("Mode: Local");
    }
    println!();
    println!("Type your message and press Enter. Type 'quit' or 'exit' to end.");
    println!("Type '/clear' to clear conversation history.");
    println!("Type '/status' to show runtime status.");
    println!();

    // Create runtime
    let runtime = create_runtime().await?;

    // Subscribe to job topics if distributed mode
    if args.distributed {
        runtime.subscribe_to_job_topics().await?;
        let mut network = runtime.network.write().await;
        network.start().await?;

        // Wait for connections
        println!("Connecting to network...");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let peers = runtime.connected_peers_count().await;
        println!("Connected to {} peers\n", peers);
    }

    // Conversation history
    let mut history: Vec<(String, String)> = Vec::new();
    let system_prompt = args.system.clone();

    // Chat loop
    let stdin = io::stdin();
    loop {
        print!("You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Handle commands
        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            println!("Goodbye!");
            break;
        }

        if input.eq_ignore_ascii_case("/clear") {
            history.clear();
            println!("Conversation cleared.\n");
            continue;
        }

        if input.eq_ignore_ascii_case("/status") {
            let stats = runtime.stats().await;
            println!("\n=== Status ===");
            println!("Peer ID: {}", stats.peer_id);
            println!("Connected peers: {}", stats.connected_peers);
            println!("Balance: {:.6} PCLAW", stats.balance);
            println!("CPU usage: {:.1}%", stats.resource_state.cpu_usage * 100.0);
            println!("RAM: {}/{} MB", stats.resource_state.ram_available_mb, stats.resource_state.ram_total_mb);
            println!("Active jobs: {}", stats.active_jobs);
            println!();
            continue;
        }

        // Build prompt with history
        let full_prompt = build_prompt(&system_prompt, &history, input);

        // Execute inference
        print!("\nAssistant: ");
        io::stdout().flush()?;

        let task = InferenceTask::new(&args.model, &full_prompt)
            .with_max_tokens(args.max_tokens)
            .with_temperature(args.temperature);

        match runtime.execute_task(ExecutionTask::Inference(task)).await {
            Ok(result) => {
                match &result.data {
                    TaskData::Inference(r) => {
                        println!("{}", r.text);

                        // Add to history
                        history.push((input.to_string(), r.text.clone()));

                        // Show metrics
                        if r.tokens_generated > 0 {
                            println!(
                                "\n[{} tokens, {:.1} tok/s, {:?}]",
                                r.tokens_generated,
                                r.tokens_per_second,
                                result.location
                            );
                        }
                    }
                    TaskData::Error(e) => {
                        println!("[Error: {}]", e);
                    }
                    _ => {
                        println!("[Unexpected response type]");
                    }
                }
            }
            Err(e) => {
                println!("[Error: {}]", e);
            }
        }

        println!();
    }

    Ok(())
}

async fn create_runtime() -> anyhow::Result<Runtime> {
    bootstrap::ensure_dirs()?;

    let identity_path = bootstrap::identity_path();
    let identity = if identity_path.exists() {
        Arc::new(NodeIdentity::load(&identity_path)?)
    } else {
        let id = NodeIdentity::generate();
        id.save(&identity_path)?;
        Arc::new(id)
    };

    let config = Config::load()?;
    let db = Database::open(&config.database.path)?;

    Runtime::new(identity, db, config).await
}

fn build_prompt(system: &str, history: &[(String, String)], user_input: &str) -> String {
    let mut prompt = String::new();

    // System prompt
    prompt.push_str(&format!("System: {}\n\n", system));

    // Conversation history (last 5 turns)
    let start = history.len().saturating_sub(5);
    for (user, assistant) in &history[start..] {
        prompt.push_str(&format!("User: {}\n", user));
        prompt.push_str(&format!("Assistant: {}\n\n", assistant));
    }

    // Current input
    prompt.push_str(&format!("User: {}\nAssistant:", user_input));

    prompt
}
