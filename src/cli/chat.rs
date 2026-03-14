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

/// Check if a node is running by trying the API at the configured address
async fn check_running_node() -> Option<String> {
    // Load config to get the web address
    let config = Config::load().ok()?;
    if !config.web.enabled {
        return None;
    }

    let addr = config.web.listen_addr;
    let url = format!("http://{}/api/status", addr);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .ok()?;

    if let Ok(resp) = client.get(&url).send().await {
        if resp.status().is_success() {
            return Some(format!("http://{}", addr));
        }
    }
    None
}

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

    /// Force standalone mode (use separate database)
    #[arg(long)]
    pub standalone: bool,
}

/// Mode of operation for the chat
enum ChatMode {
    /// Using a running node via API
    Api { base_url: String },
    /// Standalone local runtime
    Local { runtime: Runtime },
}

pub async fn run(args: ChatArgs) -> anyhow::Result<()> {
    println!("=== PeerClaw'd AI Chat ===");
    println!("Model: {}", args.model);
    println!("Max tokens: {}", args.max_tokens);
    println!("Temperature: {}", args.temperature);

    // Determine mode
    let mode = if args.standalone {
        println!("Mode: Standalone");
        ChatMode::Local { runtime: create_standalone_runtime().await? }
    } else if let Some(base_url) = check_running_node().await {
        println!("Mode: Connected to running node at {}", base_url);
        ChatMode::Api { base_url }
    } else if args.distributed {
        println!("Mode: Distributed (will use network peers if needed)");
        ChatMode::Local { runtime: create_standalone_runtime().await? }
    } else {
        println!("Mode: Local");
        ChatMode::Local { runtime: create_standalone_runtime().await? }
    };

    println!();
    println!("Type your message and press Enter. Type 'quit' or 'exit' to end.");
    println!("Type '/clear' to clear conversation history.");
    println!("Type '/status' to show runtime status.");
    println!();

    // Get runtime reference if in local mode
    let runtime = match &mode {
        ChatMode::Local { runtime } => Some(runtime),
        ChatMode::Api { .. } => None,
    };

    // Subscribe to job topics if distributed mode
    if args.distributed {
        if let Some(rt) = &runtime {
            rt.subscribe_to_job_topics().await?;
            let mut network = rt.network.write().await;
            network.start().await?;

            // Wait for connections
            println!("Connecting to network...");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let peers = rt.connected_peers_count().await;
            println!("Connected to {} peers\n", peers);
        }
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
            match &mode {
                ChatMode::Local { runtime: rt } => {
                    let stats = rt.stats().await;
                    println!("\n=== Status ===");
                    println!("Peer ID: {}", stats.peer_id);
                    println!("Connected peers: {}", stats.connected_peers);
                    println!("Balance: {:.6} PCLAW", stats.balance);
                    println!("CPU usage: {:.1}%", stats.resource_state.cpu_usage * 100.0);
                    println!("RAM: {}/{} MB", stats.resource_state.ram_available_mb, stats.resource_state.ram_total_mb);
                    println!("Active jobs: {}", stats.active_jobs);
                }
                ChatMode::Api { base_url } => {
                    if let Ok(status) = fetch_api_status(base_url).await {
                        println!("\n=== Status (via API) ===");
                        println!("{}", status);
                    } else {
                        println!("\n[Could not fetch status from node]");
                    }
                }
            }
            println!();
            continue;
        }

        // Build prompt with history
        let full_prompt = build_prompt(&system_prompt, &history, input);

        // Execute inference
        print!("\nAssistant: ");
        io::stdout().flush()?;

        let response = match &mode {
            ChatMode::Local { runtime: rt } => {
                let task = InferenceTask::new(&args.model, &full_prompt)
                    .with_max_tokens(args.max_tokens)
                    .with_temperature(args.temperature);

                match rt.execute_task(ExecutionTask::Inference(task)).await {
                    Ok(result) => {
                        match &result.data {
                            TaskData::Inference(r) => {
                                let metrics = if r.tokens_generated > 0 {
                                    Some(format!(
                                        "[{} tokens, {:.1} tok/s, {:?}]",
                                        r.tokens_generated,
                                        r.tokens_per_second,
                                        result.location
                                    ))
                                } else {
                                    None
                                };
                                Ok((r.text.clone(), metrics))
                            }
                            TaskData::Error(e) => Err(e.clone()),
                            _ => Err("Unexpected response type".to_string()),
                        }
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            ChatMode::Api { base_url } => {
                // Use the API endpoint
                execute_via_api(base_url, &args.model, &full_prompt, args.max_tokens, args.temperature).await
            }
        };

        match response {
            Ok((text, metrics)) => {
                println!("{}", text);
                history.push((input.to_string(), text));
                if let Some(m) = metrics {
                    println!("\n{}", m);
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

/// Create a standalone runtime with a separate database for chat
async fn create_standalone_runtime() -> anyhow::Result<Runtime> {
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

    // Use a separate database file for chat to avoid lock conflicts
    let chat_db_path = config.database.path.with_file_name("chat.redb");
    config.database.path = chat_db_path;

    let db = Database::open(&config.database.path)?;

    Runtime::new(identity, db, config).await
}

/// Fetch status from the running node via API
async fn fetch_api_status(base_url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/status", base_url);
    let resp: serde_json::Value = client.get(&url).send().await?.json().await?;

    Ok(format!(
        "Peer ID: {}\nConnected peers: {}\nBalance: {} PCLAW\nActive jobs: {}",
        resp.get("peer_id").and_then(|v| v.as_str()).unwrap_or("unknown"),
        resp.get("connected_peers").and_then(|v| v.as_u64()).unwrap_or(0),
        resp.get("balance").and_then(|v| v.as_f64()).unwrap_or(0.0),
        resp.get("active_jobs").and_then(|v| v.as_u64()).unwrap_or(0),
    ))
}

/// Execute inference via the running node's API
async fn execute_via_api(
    base_url: &str,
    model: &str,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
) -> Result<(String, Option<String>), String> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/chat", base_url);

    let payload = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "max_tokens": max_tokens,
        "temperature": temperature,
    });

    match client.post(&url).json(&payload).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let text = json.get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tokens = json.get("tokens_generated").and_then(|v| v.as_u64());
                        let tps = json.get("tokens_per_second").and_then(|v| v.as_f64());

                        let metrics = match (tokens, tps) {
                            (Some(t), Some(s)) if t > 0 => Some(format!("[{} tokens, {:.1} tok/s, via API]", t, s)),
                            _ => None,
                        };

                        Ok((text, metrics))
                    }
                    Err(e) => Err(format!("Failed to parse response: {}", e)),
                }
            } else {
                Err(format!("API error: {}", resp.status()))
            }
        }
        Err(e) => Err(format!("Request failed: {}", e)),
    }
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
