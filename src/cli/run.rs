//! `peerclawd run` and `peerclawd pull` commands - Ollama/vLLM-style interface.
//!
//! Provides familiar commands for users coming from Ollama or vLLM:
//! - `peerclawd run <model>` - Run a model interactively
//! - `peerclawd pull <model>` - Download a model

use clap::Args;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::{ExecutionTask, InferenceTask};
use crate::identity::NodeIdentity;
use crate::runtime::Runtime;

/// Arguments for the `run` command
#[derive(Args)]
pub struct RunArgs {
    /// Model name to run (e.g., llama-3.2-1b, llama-3.2-3b)
    pub model: String,

    /// Optional prompt (if not provided, enters interactive mode)
    pub prompt: Option<String>,

    /// Use tensor parallelism across N GPUs (vLLM-style)
    #[arg(long, short = 't', default_value = "1")]
    pub tensor_parallel: u32,

    /// Use pipeline parallelism across N nodes (vLLM-style)
    #[arg(long, short = 'p', default_value = "1")]
    pub pipeline_parallel: u32,

    /// Maximum tokens to generate
    #[arg(long, default_value = "500")]
    pub max_tokens: u32,

    /// Temperature for sampling
    #[arg(long, default_value = "0.7")]
    pub temperature: f32,

    /// Use distributed execution across network peers
    #[arg(long, short = 'd')]
    pub distributed: bool,

    /// System prompt
    #[arg(long, short = 's')]
    pub system: Option<String>,
}

/// Arguments for the `pull` command
#[derive(Args)]
pub struct PullArgs {
    /// Model name to download (e.g., llama-3.2-1b, llama-3.2-3b)
    pub model: String,

    /// Quantization level (q4_k_m, q5_k_m, q6_k, q8_0)
    #[arg(long, default_value = "q4_k_m")]
    pub quant: String,
}

/// Run a model with Ollama-style interface
pub async fn run(args: RunArgs) -> anyhow::Result<()> {
    // Resolve model path
    let model_name = resolve_model_name(&args.model);

    println!("\x1b[90mLoading model: {}\x1b[0m", model_name);

    // Check if model exists
    let models_dir = bootstrap::base_dir().join("models");
    let model_exists = std::fs::read_dir(&models_dir)
        .map(|entries| {
            entries.filter_map(|e| e.ok())
                .any(|e| {
                    e.path().file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase().contains(&model_name.to_lowercase()))
                        .unwrap_or(false)
                })
        })
        .unwrap_or(false);

    if !model_exists {
        println!("\x1b[33mModel '{}' not found locally.\x1b[0m", model_name);
        println!("Run: \x1b[36mpeerclawd pull {}\x1b[0m", args.model);
        return Ok(());
    }

    // Show distributed config if set
    if args.tensor_parallel > 1 || args.pipeline_parallel > 1 {
        println!("\x1b[90mDistributed config: TP={}, PP={}\x1b[0m",
            args.tensor_parallel, args.pipeline_parallel);
    }

    // Create runtime
    let runtime = create_runtime().await?;

    // If prompt provided, run single inference
    if let Some(prompt) = args.prompt {
        let system = args.system.as_deref().unwrap_or("You are a helpful assistant.");
        let full_prompt = format!("System: {}\n\nUser: {}\nAssistant:", system, prompt);

        let result = runtime.inference_streaming_print(
            &model_name,
            &full_prompt,
            args.max_tokens,
            args.temperature,
        ).await?;

        println!("\n\x1b[90m[{} tokens, {:.1} tok/s]\x1b[0m\n",
            result.tokens_generated, result.tokens_per_second);
        return Ok(());
    }

    // Interactive mode
    println!("\n\x1b[1m{}\x1b[0m", model_name);
    println!("Type your message and press Enter. Type \x1b[33m/bye\x1b[0m to exit.\n");

    let system = args.system.as_deref().unwrap_or("You are a helpful assistant.");
    let mut history: Vec<(String, String)> = Vec::new();

    let stdin = io::stdin();
    loop {
        print!("\x1b[32m>>> \x1b[0m");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "/bye" || input == "exit" || input == "quit" {
            println!("\x1b[33mGoodbye!\x1b[0m");
            break;
        }

        // Build prompt with history
        let full_prompt = build_prompt(system, &history, input);

        // Run inference with streaming
        let result = runtime.inference_streaming_print(
            &model_name,
            &full_prompt,
            args.max_tokens,
            args.temperature,
        ).await;

        match result {
            Ok(r) => {
                history.push((input.to_string(), r.text.clone()));
                println!("\n\x1b[90m[{} tokens, {:.1} tok/s]\x1b[0m\n",
                    r.tokens_generated, r.tokens_per_second);
            }
            Err(e) => {
                println!("\n\x1b[31mError: {}\x1b[0m\n", e);
            }
        }
    }

    Ok(())
}

/// Pull/download a model (delegates to models download)
pub async fn pull(args: PullArgs) -> anyhow::Result<()> {
    // Delegate to models download
    crate::cli::models::run(crate::cli::models::ModelsArgs {
        cmd: Some(crate::cli::models::ModelsCommand::Download {
            model: args.model,
            quant: args.quant,
        }),
    }).await
}

/// List models (delegates to models list)
pub async fn list() -> anyhow::Result<()> {
    crate::cli::models::run(crate::cli::models::ModelsArgs {
        cmd: Some(crate::cli::models::ModelsCommand::List),
    }).await
}

/// Show running processes/jobs
pub async fn ps() -> anyhow::Result<()> {
    let runtime = create_runtime().await?;

    let jm = runtime.job_manager.read().await;
    let active = jm.active_jobs().await;

    println!();
    println!("\x1b[1mNAME                   SIZE      STATUS\x1b[0m");

    if active.is_empty() {
        println!("\x1b[90m(no jobs running)\x1b[0m");
    } else {
        for job in &active {
            let id_str = &job.id.0;
            let short = if id_str.len() > 20 { &id_str[..20] } else { id_str };
            println!("{:<22} {:>8}  \x1b[32m{}\x1b[0m", short, "-", job.status);
        }
    }

    println!();
    Ok(())
}

/// Resolve model name to actual file path
fn resolve_model_name(name: &str) -> String {
    // Handle common aliases
    match name.to_lowercase().as_str() {
        "llama2" | "llama" => "llama-3.2-3b".to_string(),
        "llama:7b" | "llama2:7b" => "llama-3.2-3b".to_string(),
        "llama:13b" | "llama2:13b" => "llama-3.2-3b".to_string(),
        "phi" | "phi3" => "phi-3-mini".to_string(),
        "qwen" | "qwen2" => "qwen2.5-3b".to_string(),
        "gemma" | "gemma2" => "gemma-2-2b".to_string(),
        "tinyllama" => "tinyllama-1.1b".to_string(),
        _ => name.to_string(),
    }
}

fn build_prompt(system: &str, history: &[(String, String)], user_input: &str) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!("System: {}\n\n", system));

    // Last 5 turns
    let start = history.len().saturating_sub(5);
    for (user, assistant) in &history[start..] {
        prompt.push_str(&format!("User: {}\n", user));
        prompt.push_str(&format!("Assistant: {}\n\n", assistant));
    }

    prompt.push_str(&format!("User: {}\nAssistant:", user_input));

    prompt
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

    let mut config = Config::load()?;

    // Use separate database for run command
    let run_db_path = config.database.path.with_file_name("run.redb");
    config.database.path = run_db_path;

    let db = Database::open(&config.database.path)?;

    Runtime::new(identity, db, config).await
}
