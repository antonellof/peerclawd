//! `peerclaw chat` command - Interactive AI chat with Claude-Code-style commands.

use clap::Args;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::{ExecutionTask, ExecutionLocation, InferenceTask, TaskData};
use crate::identity::NodeIdentity;
use crate::runtime::Runtime;

// ============================================================================
// Chat Settings (Persistent)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSettings {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub system_prompt: String,
    pub distributed: bool,
    #[serde(default = "default_stream")]
    pub stream: bool,
}

fn default_stream() -> bool { true }

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            model: "llama-3.2-3b".to_string(),
            max_tokens: 500,
            temperature: 0.7,
            system_prompt: "You are a helpful AI assistant.".to_string(),
            distributed: false,
            stream: true,
        }
    }
}

impl ChatSettings {
    fn settings_path() -> PathBuf {
        bootstrap::base_dir().join("chat_settings.json")
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(settings) = serde_json::from_str(&content) {
                    return settings;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::settings_path();
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

// ============================================================================
// Slash Commands
// ============================================================================

#[derive(Debug)]
enum SlashCommand {
    Help,
    Clear,
    Status,
    Model(String),
    Temperature(f32),
    MaxTokens(u32),
    System(String),
    Settings,
    History,
    Export(PathBuf),
    Distributed(bool),
    Stream(bool),
    // New Claude-Code-style commands
    Cost,
    Balance,
    Peers,
    Jobs,
    Compact,
    Doctor,
    Config,
    // Tool commands
    Tools,
    ToolInfo(String),
    ToolExec(String, String),
    // Skill commands
    Skills,
    SkillInfo(String),
    SkillCreate(String),
    SkillScan,
    Quit,
}

/// Session statistics for tracking usage
#[derive(Debug, Default)]
struct SessionStats {
    total_tokens: u64,
    total_requests: u32,
    start_time: Option<std::time::Instant>,
}

fn parse_slash_command(input: &str) -> Option<SlashCommand> {
    if !input.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
    let cmd = parts.first()?.to_lowercase();
    let arg = parts.get(1).map(|s| s.trim());

    match cmd.as_str() {
        "help" | "h" | "?" => Some(SlashCommand::Help),
        "clear" | "c" => Some(SlashCommand::Clear),
        "status" | "s" => Some(SlashCommand::Status),
        "model" | "m" => arg.map(|s| SlashCommand::Model(s.to_string())),
        "temperature" | "temp" | "t" => {
            arg.and_then(|s| s.parse().ok()).map(SlashCommand::Temperature)
        }
        "max-tokens" | "tokens" | "max" => {
            arg.and_then(|s| s.parse().ok()).map(SlashCommand::MaxTokens)
        }
        "system" | "sys" => arg.map(|s| SlashCommand::System(s.to_string())),
        "settings" => Some(SlashCommand::Settings),
        "history" | "hist" => Some(SlashCommand::History),
        "export" | "save" => arg.map(|s| SlashCommand::Export(PathBuf::from(s))),
        "distributed" | "dist" | "d" => {
            let enabled = arg.map(|s| matches!(s.to_lowercase().as_str(), "on" | "true" | "1" | "yes"))
                .unwrap_or(true);
            Some(SlashCommand::Distributed(enabled))
        }
        "stream" => {
            let enabled = arg.map(|s| matches!(s.to_lowercase().as_str(), "on" | "true" | "1" | "yes"))
                .unwrap_or(true);
            Some(SlashCommand::Stream(enabled))
        }
        // New Claude-Code-style commands
        "cost" => Some(SlashCommand::Cost),
        "balance" | "bal" | "wallet" => Some(SlashCommand::Balance),
        "peers" | "p" => Some(SlashCommand::Peers),
        "jobs" | "j" => Some(SlashCommand::Jobs),
        "compact" => Some(SlashCommand::Compact),
        "doctor" | "doc" => Some(SlashCommand::Doctor),
        "config" | "conf" => Some(SlashCommand::Config),
        // Tool commands
        "tools" => Some(SlashCommand::Tools),
        "tool" => {
            let parts: Vec<&str> = arg.unwrap_or("").splitn(2, ' ').collect();
            match parts.first().map(|s| s.to_lowercase()).as_deref() {
                Some("list") | None => Some(SlashCommand::Tools),
                Some("info") => parts.get(1).map(|s| SlashCommand::ToolInfo(s.to_string())),
                Some("exec") => {
                    let rest = parts.get(1).unwrap_or(&"");
                    let exec_parts: Vec<&str> = rest.splitn(2, ' ').collect();
                    exec_parts.first().map(|name| {
                        SlashCommand::ToolExec(name.to_string(), exec_parts.get(1).unwrap_or(&"").to_string())
                    })
                }
                _ => Some(SlashCommand::Tools),
            }
        }
        // Skill commands
        "skills" => Some(SlashCommand::Skills),
        "skill" => {
            let parts: Vec<&str> = arg.unwrap_or("").splitn(2, ' ').collect();
            match parts.first().map(|s| s.to_lowercase()).as_deref() {
                Some("list") | None => Some(SlashCommand::Skills),
                Some("info") => parts.get(1).map(|s| SlashCommand::SkillInfo(s.to_string())),
                Some("create") => parts.get(1).map(|s| SlashCommand::SkillCreate(s.to_string())),
                Some("scan") => Some(SlashCommand::SkillScan),
                _ => Some(SlashCommand::Skills),
            }
        }
        "quit" | "exit" | "q" => Some(SlashCommand::Quit),
        _ => None,
    }
}

fn show_help() {
    println!("\n\x1b[1m=== PeerClaw Chat Commands ===\x1b[0m");
    println!();
    println!("  \x1b[1mGeneral\x1b[0m");
    println!("  \x1b[36m/help, /h, /?\x1b[0m         Show this help");
    println!("  \x1b[36m/status, /s\x1b[0m           Show runtime status");
    println!("  \x1b[36m/settings\x1b[0m             Open interactive settings menu");
    println!("  \x1b[36m/config\x1b[0m               Open config file location");
    println!("  \x1b[36m/doctor\x1b[0m               Health check (models, network, wallet)");
    println!();
    println!("  \x1b[1mConversation\x1b[0m");
    println!("  \x1b[36m/clear, /c\x1b[0m            Clear conversation history");
    println!("  \x1b[36m/history\x1b[0m              Show conversation summary");
    println!("  \x1b[36m/compact\x1b[0m              Compress history to save context");
    println!("  \x1b[36m/export <path>\x1b[0m        Export conversation to file");
    println!();
    println!("  \x1b[1mModel Settings\x1b[0m");
    println!("  \x1b[36m/model <name>\x1b[0m         Switch model (e.g., /model llama-3.2-1b)");
    println!("  \x1b[36m/temperature <n>\x1b[0m      Set temperature (0.0-2.0)");
    println!("  \x1b[36m/max-tokens <n>\x1b[0m       Set max tokens per response");
    println!("  \x1b[36m/system <prompt>\x1b[0m      Set system prompt");
    println!("  \x1b[36m/stream on|off\x1b[0m        Toggle streaming output");
    println!();
    println!("  \x1b[1mTools (WASM)\x1b[0m");
    println!("  \x1b[36m/tools\x1b[0m                List available tools");
    println!("  \x1b[36m/tool info <name>\x1b[0m     Show tool details");
    println!("  \x1b[36m/tool exec <name>\x1b[0m     Execute a tool");
    println!();
    println!("  \x1b[1mSkills (SKILL.md)\x1b[0m");
    println!("  \x1b[36m/skills\x1b[0m               List installed skills");
    println!("  \x1b[36m/skill info <name>\x1b[0m    Show skill details");
    println!("  \x1b[36m/skill create <name>\x1b[0m  Create new skill template");
    println!("  \x1b[36m/skill scan\x1b[0m           Reload skills from disk");
    println!();
    println!("  \x1b[1mNetwork & Economy\x1b[0m");
    println!("  \x1b[36m/cost\x1b[0m                 Show session token usage");
    println!("  \x1b[36m/balance\x1b[0m              Show wallet balance");
    println!("  \x1b[36m/peers\x1b[0m                Show connected peers");
    println!("  \x1b[36m/jobs\x1b[0m                 Show active/recent jobs");
    println!("  \x1b[36m/distributed on|off\x1b[0m   Toggle distributed mode");
    println!();
    println!("  \x1b[36m/quit, /exit, /q\x1b[0m      Exit chat");
    println!();
}

fn show_settings_menu(settings: &mut ChatSettings) -> bool {
    println!("\n\x1b[1m=== Settings ===\x1b[0m");
    println!();
    println!("  \x1b[33m1.\x1b[0m Model:        {}", settings.model);
    println!("  \x1b[33m2.\x1b[0m Max Tokens:   {}", settings.max_tokens);
    println!("  \x1b[33m3.\x1b[0m Temperature:  {:.2}", settings.temperature);
    println!("  \x1b[33m4.\x1b[0m System:       {}...", &settings.system_prompt[..settings.system_prompt.len().min(40)]);
    println!("  \x1b[33m5.\x1b[0m Distributed:  {}", if settings.distributed { "On" } else { "Off" });
    println!();
    println!("Enter number to edit, or press Enter to return:");

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    let input = input.trim();

    if input.is_empty() {
        return false;
    }

    match input {
        "1" => {
            print!("New model name: ");
            io::stdout().flush().ok();
            let mut val = String::new();
            io::stdin().lock().read_line(&mut val).ok();
            let val = val.trim();
            if !val.is_empty() {
                settings.model = val.to_string();
                println!("\x1b[32mModel set to: {}\x1b[0m", settings.model);
            }
        }
        "2" => {
            print!("Max tokens: ");
            io::stdout().flush().ok();
            let mut val = String::new();
            io::stdin().lock().read_line(&mut val).ok();
            if let Ok(n) = val.trim().parse() {
                settings.max_tokens = n;
                println!("\x1b[32mMax tokens set to: {}\x1b[0m", settings.max_tokens);
            }
        }
        "3" => {
            print!("Temperature (0.0-2.0): ");
            io::stdout().flush().ok();
            let mut val = String::new();
            io::stdin().lock().read_line(&mut val).ok();
            if let Ok(t) = val.trim().parse::<f32>() {
                settings.temperature = t.clamp(0.0, 2.0);
                println!("\x1b[32mTemperature set to: {:.2}\x1b[0m", settings.temperature);
            }
        }
        "4" => {
            print!("System prompt: ");
            io::stdout().flush().ok();
            let mut val = String::new();
            io::stdin().lock().read_line(&mut val).ok();
            let val = val.trim();
            if !val.is_empty() {
                settings.system_prompt = val.to_string();
                println!("\x1b[32mSystem prompt updated\x1b[0m");
            }
        }
        "5" => {
            settings.distributed = !settings.distributed;
            println!("\x1b[32mDistributed mode: {}\x1b[0m", if settings.distributed { "On" } else { "Off" });
        }
        _ => {
            println!("\x1b[33mInvalid option\x1b[0m");
        }
    }

    // Save settings
    if let Err(e) = settings.save() {
        println!("\x1b[33mWarning: Could not save settings: {}\x1b[0m", e);
    }

    true // Continue showing menu
}

fn show_history(history: &[(String, String)]) {
    if history.is_empty() {
        println!("\n\x1b[33mNo conversation history.\x1b[0m\n");
        return;
    }

    println!("\n\x1b[1m=== Conversation History ({} exchanges) ===\x1b[0m\n", history.len());
    for (i, (user, assistant)) in history.iter().enumerate() {
        let user_preview = if user.len() > 50 { format!("{}...", &user[..50]) } else { user.clone() };
        let assistant_preview = if assistant.len() > 50 { format!("{}...", &assistant[..50]) } else { assistant.clone() };
        println!("  \x1b[36m{}.\x1b[0m You: {}", i + 1, user_preview);
        println!("     AI: {}", assistant_preview);
    }
    println!();
}

fn export_conversation(path: &PathBuf, history: &[(String, String)], settings: &ChatSettings) -> anyhow::Result<()> {
    let mut content = String::new();
    content.push_str("# PeerClaw Chat Export\n\n");
    content.push_str(&format!("Model: {}\n", settings.model));
    content.push_str(&format!("Temperature: {}\n", settings.temperature));
    content.push_str(&format!("System: {}\n\n", settings.system_prompt));
    content.push_str("---\n\n");

    for (user, assistant) in history {
        content.push_str(&format!("**You:** {}\n\n", user));
        content.push_str(&format!("**Assistant:** {}\n\n", assistant));
    }

    std::fs::write(path, content)?;
    Ok(())
}

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

    /// Disable streaming (wait for complete response)
    #[arg(long)]
    pub no_stream: bool,
}

/// Mode of operation for the chat
enum ChatMode {
    /// Using a running node via API
    Api { base_url: String },
    /// Standalone local runtime
    Local { runtime: Runtime },
}

pub async fn run(args: ChatArgs) -> anyhow::Result<()> {
    // Load persistent settings and merge with CLI args
    let mut settings = ChatSettings::load();

    // CLI args override saved settings
    if args.model != "llama-3.2-3b" {
        settings.model = args.model.clone();
    }
    if args.max_tokens != 500 {
        settings.max_tokens = args.max_tokens;
    }
    if args.temperature != 0.7 {
        settings.temperature = args.temperature;
    }
    if args.system != "You are a helpful AI assistant." {
        settings.system_prompt = args.system.clone();
    }
    if args.distributed {
        settings.distributed = true;
    }
    if args.no_stream {
        settings.stream = false;
    }

    println!("\x1b[1m=== PeerClaw AI Chat ===\x1b[0m");
    println!("Model: \x1b[36m{}\x1b[0m", settings.model);
    println!("Max tokens: {}", settings.max_tokens);
    println!("Temperature: {:.2}", settings.temperature);
    println!("Streaming: {}", if settings.stream { "\x1b[32mOn\x1b[0m" } else { "\x1b[33mOff\x1b[0m" });

    // Determine mode
    let mode = if args.standalone {
        println!("Mode: \x1b[33mStandalone\x1b[0m");
        ChatMode::Local { runtime: create_standalone_runtime().await? }
    } else if let Some(base_url) = check_running_node().await {
        println!("Mode: \x1b[32mConnected to running node\x1b[0m at {}", base_url);
        ChatMode::Api { base_url }
    } else if settings.distributed {
        println!("Mode: \x1b[35mDistributed\x1b[0m (will use network peers if needed)");
        ChatMode::Local { runtime: create_standalone_runtime().await? }
    } else {
        println!("Mode: \x1b[36mLocal\x1b[0m");
        ChatMode::Local { runtime: create_standalone_runtime().await? }
    };

    println!();
    println!("Type \x1b[36m/help\x1b[0m for commands. Type \x1b[33mquiet\x1b[0m or \x1b[33mexit\x1b[0m to end.");
    println!();

    // Get runtime reference if in local mode
    let runtime = match &mode {
        ChatMode::Local { runtime } => Some(runtime),
        ChatMode::Api { .. } => None,
    };

    // Subscribe to job topics if distributed mode
    if settings.distributed {
        if let Some(rt) = &runtime {
            rt.subscribe_to_job_topics().await?;
            let mut network = rt.network.write().await;
            network.start().await?;

            // Wait for connections
            println!("Connecting to network...");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let peers = rt.connected_peers_count().await;
            println!("Connected to \x1b[32m{}\x1b[0m peers\n", peers);
        }
    }

    // Conversation history
    let mut history: Vec<(String, String)> = Vec::new();

    // Session statistics for /cost command
    let mut session_stats = SessionStats {
        total_tokens: 0,
        total_requests: 0,
        start_time: Some(std::time::Instant::now()),
    };

    // Chat loop
    let stdin = io::stdin();
    loop {
        print!("\x1b[36mYou:\x1b[0m ");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Handle quit/exit without slash
        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            println!("\x1b[33mGoodbye!\x1b[0m");
            break;
        }

        // Handle slash commands
        if let Some(cmd) = parse_slash_command(input) {
            match cmd {
                SlashCommand::Help => {
                    show_help();
                    continue;
                }
                SlashCommand::Clear => {
                    history.clear();
                    println!("\x1b[32mConversation cleared.\x1b[0m\n");
                    continue;
                }
                SlashCommand::Status => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            let stats = rt.stats().await;
                            println!("\n\x1b[1m=== Status ===\x1b[0m");
                            println!("Peer ID: \x1b[36m{}\x1b[0m", stats.peer_id);
                            println!("Connected peers: \x1b[32m{}\x1b[0m", stats.connected_peers);
                            println!("Balance: \x1b[33m{:.6} PCLAW\x1b[0m", stats.balance);
                            println!("CPU usage: {:.1}%", stats.resource_state.cpu_usage * 100.0);
                            println!("RAM: {}/{} MB", stats.resource_state.ram_available_mb, stats.resource_state.ram_total_mb);
                            println!("Active jobs: {}", stats.active_jobs);
                            println!();
                            println!("Current settings:");
                            println!("  Model: \x1b[36m{}\x1b[0m", settings.model);
                            println!("  Max tokens: {}", settings.max_tokens);
                            println!("  Temperature: {:.2}", settings.temperature);
                            println!("  Distributed: {}", if settings.distributed { "On" } else { "Off" });
                        }
                        ChatMode::Api { base_url } => {
                            if let Ok(status) = fetch_api_status(base_url).await {
                                println!("\n\x1b[1m=== Status (via API) ===\x1b[0m");
                                println!("{}", status);
                            } else {
                                println!("\n\x1b[31m[Could not fetch status from node]\x1b[0m");
                            }
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::Model(name) => {
                    settings.model = name.clone();
                    settings.save().ok();
                    println!("\x1b[32mModel set to: {}\x1b[0m\n", name);
                    continue;
                }
                SlashCommand::Temperature(t) => {
                    settings.temperature = t.clamp(0.0, 2.0);
                    settings.save().ok();
                    println!("\x1b[32mTemperature set to: {:.2}\x1b[0m\n", settings.temperature);
                    continue;
                }
                SlashCommand::MaxTokens(n) => {
                    settings.max_tokens = n;
                    settings.save().ok();
                    println!("\x1b[32mMax tokens set to: {}\x1b[0m\n", n);
                    continue;
                }
                SlashCommand::System(prompt) => {
                    settings.system_prompt = prompt.clone();
                    settings.save().ok();
                    println!("\x1b[32mSystem prompt updated\x1b[0m\n");
                    continue;
                }
                SlashCommand::Settings => {
                    while show_settings_menu(&mut settings) {}
                    println!();
                    continue;
                }
                SlashCommand::History => {
                    show_history(&history);
                    continue;
                }
                SlashCommand::Export(path) => {
                    match export_conversation(&path, &history, &settings) {
                        Ok(_) => println!("\x1b[32mConversation exported to: {}\x1b[0m\n", path.display()),
                        Err(e) => println!("\x1b[31mExport failed: {}\x1b[0m\n", e),
                    }
                    continue;
                }
                SlashCommand::Distributed(enabled) => {
                    settings.distributed = enabled;
                    settings.save().ok();
                    println!("\x1b[32mDistributed mode: {}\x1b[0m\n", if enabled { "On" } else { "Off" });
                    continue;
                }
                SlashCommand::Stream(enabled) => {
                    settings.stream = enabled;
                    settings.save().ok();
                    println!("\x1b[32mStreaming: {}\x1b[0m\n", if enabled { "On" } else { "Off" });
                    continue;
                }
                // New Claude-Code-style commands
                SlashCommand::Cost => {
                    println!("\n\x1b[1m=== Session Cost ===\x1b[0m");
                    println!("  Tokens used:    \x1b[36m{}\x1b[0m", session_stats.total_tokens);
                    println!("  Requests:       {}", session_stats.total_requests);
                    if let Some(start) = session_stats.start_time {
                        let elapsed = start.elapsed().as_secs();
                        let mins = elapsed / 60;
                        let secs = elapsed % 60;
                        println!("  Session time:   {}m {}s", mins, secs);
                    }
                    // Estimate cost in PCLAW (rough: 1 PCLAW per 1000 tokens)
                    let estimated_cost = session_stats.total_tokens as f64 / 1000.0;
                    println!("  Est. cost:      \x1b[33m{:.4} PCLAW\x1b[0m", estimated_cost);
                    println!();
                    continue;
                }
                SlashCommand::Balance => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            let balance = crate::wallet::from_micro(rt.balance().await);
                            println!("\n  Wallet balance: \x1b[33m{:.6} PCLAW\x1b[0m\n", balance);
                        }
                        ChatMode::Api { base_url } => {
                            if let Ok(status) = fetch_api_status(base_url).await {
                                println!("\n{}\n", status);
                            } else {
                                println!("\n  \x1b[31mCould not fetch balance\x1b[0m\n");
                            }
                        }
                    }
                    continue;
                }
                SlashCommand::Peers => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            let count: usize = rt.connected_peers_count().await;
                            let network = rt.network.read().await;
                            let peers = network.connected_peers();
                            println!("\n\x1b[1m=== Peers ({}) ===\x1b[0m", count);
                            if peers.is_empty() {
                                println!("  \x1b[33mNo peers connected\x1b[0m");
                            } else {
                                for peer in peers.iter().take(10) {
                                    let short = format!("{}...{}",
                                        &peer.to_string()[..8],
                                        &peer.to_string()[peer.to_string().len()-8..]);
                                    println!("  • \x1b[36m{}\x1b[0m", short);
                                }
                                if peers.len() > 10 {
                                    println!("  ... and {} more", peers.len() - 10);
                                }
                            }
                            println!();
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mPeer info not available via API\x1b[0m\n");
                        }
                    }
                    continue;
                }
                SlashCommand::Jobs => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            let jm = rt.job_manager.read().await;
                            let active = jm.active_jobs().await;
                            let completed = jm.completed_jobs(5).await;
                            println!("\n\x1b[1m=== Jobs ===\x1b[0m");
                            println!("  Active: \x1b[32m{}\x1b[0m", active.len());
                            if !active.is_empty() {
                                for job in active.iter().take(5) {
                                    let id_str = &job.id.0;
                                    let short_id = if id_str.len() > 12 { &id_str[..12] } else { id_str };
                                    println!("    • {} - {}", short_id, job.status);
                                }
                            }
                            println!("  Recent completed: {}", completed.len());
                            for job in completed.iter().take(3) {
                                let id_str = &job.id.0;
                                let short_id = if id_str.len() > 12 { &id_str[..12] } else { id_str };
                                println!("    • {} - {}", short_id, job.status);
                            }
                            println!();
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mJob info not available via API\x1b[0m\n");
                        }
                    }
                    continue;
                }
                SlashCommand::Compact => {
                    if history.len() <= 2 {
                        println!("\n  \x1b[33mHistory too short to compact\x1b[0m\n");
                    } else {
                        // Keep only last 2 exchanges but summarize earlier ones
                        let old_len = history.len();
                        let summary = format!(
                            "[Previous {} exchanges summarized: discussed {}]",
                            old_len - 2,
                            history.iter().take(old_len - 2)
                                .map(|(q, _)| q.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
                                .collect::<Vec<_>>().join(", ")
                        );
                        history = history.split_off(old_len - 2);
                        history.insert(0, (summary, "Understood, continuing from context.".to_string()));
                        println!("\n  \x1b[32mCompacted {} exchanges into summary\x1b[0m\n", old_len - 2);
                    }
                    continue;
                }
                SlashCommand::Doctor => {
                    println!("\n\x1b[1m=== Health Check ===\x1b[0m\n");

                    // Check models
                    let models_dir = bootstrap::base_dir().join("models");
                    let model_count = std::fs::read_dir(&models_dir)
                        .map(|e| e.filter_map(|f| f.ok())
                            .filter(|f| f.path().extension().map_or(false, |e| e == "gguf"))
                            .count())
                        .unwrap_or(0);
                    if model_count > 0 {
                        println!("  \x1b[32m✓\x1b[0m Models: {} GGUF files found", model_count);
                    } else {
                        println!("  \x1b[31m✗\x1b[0m Models: No models found");
                        println!("    Run: peerclaw models download llama-3.2-1b");
                    }

                    // Check identity
                    if bootstrap::identity_path().exists() {
                        println!("  \x1b[32m✓\x1b[0m Identity: Configured");
                    } else {
                        println!("  \x1b[31m✗\x1b[0m Identity: Not configured");
                    }

                    // Check config
                    if bootstrap::config_path().exists() {
                        println!("  \x1b[32m✓\x1b[0m Config: Found");
                    } else {
                        println!("  \x1b[33m!\x1b[0m Config: Using defaults");
                    }

                    // Check network/wallet if runtime available
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            let peers: usize = rt.connected_peers_count().await;
                            if peers > 0 {
                                println!("  \x1b[32m✓\x1b[0m Network: {} peers connected", peers);
                            } else {
                                println!("  \x1b[33m!\x1b[0m Network: No peers (standalone mode)");
                            }

                            let balance = crate::wallet::from_micro(rt.balance().await);
                            if balance > 0.0 {
                                println!("  \x1b[32m✓\x1b[0m Wallet: {:.2} PCLAW", balance);
                            } else {
                                println!("  \x1b[33m!\x1b[0m Wallet: Empty");
                            }
                        }
                        ChatMode::Api { base_url } => {
                            println!("  \x1b[32m✓\x1b[0m API: Connected to {}", base_url);
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::Config => {
                    let config_path = bootstrap::config_path();
                    let settings_path = bootstrap::base_dir().join("chat_settings.json");
                    println!("\n\x1b[1m=== Config Files ===\x1b[0m");
                    println!("  Main config:    \x1b[36m{}\x1b[0m", config_path.display());
                    println!("  Chat settings:  \x1b[36m{}\x1b[0m", settings_path.display());
                    println!("  Data directory: \x1b[36m{}\x1b[0m", bootstrap::base_dir().display());
                    println!("  Models:         \x1b[36m{}\x1b[0m", bootstrap::base_dir().join("models").display());
                    println!();
                    continue;
                }
                // Tool commands
                SlashCommand::Tools => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            let tools = rt.tools.list_tools().await;
                            println!("\n\x1b[1m=== Available Tools ({}) ===\x1b[0m\n", tools.len());
                            if tools.is_empty() {
                                println!("  No tools found.");
                                println!("  Tools directory: {}", bootstrap::base_dir().join("tools").display());
                            } else {
                                for tool in &tools {
                                    let loc = match tool.location {
                                        crate::tools::ToolLocation::Local => "\x1b[32mlocal\x1b[0m",
                                        crate::tools::ToolLocation::Remote => "\x1b[36mremote\x1b[0m",
                                        crate::tools::ToolLocation::Auto => "\x1b[33mauto\x1b[0m",
                                    };
                                    println!("  {:20} {} - {}", tool.name, loc, truncate(&tool.description, 40));
                                }
                            }
                            println!();
                            println!("  Use \x1b[36m/tool info <name>\x1b[0m for details");
                            println!("  Use \x1b[36m/tool exec <name> <params>\x1b[0m to execute");
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mTool listing not available via API\x1b[0m\n");
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::ToolInfo(name) => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            if let Some(tool) = rt.tools.get(&name) {
                                println!("\n\x1b[1m=== Tool: {} ===\x1b[0m\n", tool.name());
                                println!("  Description: {}", tool.description());
                                println!("  Approval:    {:?}", tool.approval_requirement());
                                println!("  Domain:      {:?}", tool.domain());
                                let schema = tool.parameters_schema();
                                println!("\n  Parameters:");
                                if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                                    for (key, value) in props {
                                        let desc = value.get("description").and_then(|d| d.as_str()).unwrap_or("");
                                        let typ = value.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                                        println!("    {} ({}) - {}", key, typ, desc);
                                    }
                                } else {
                                    println!("    (no parameters)");
                                }
                            } else {
                                println!("\n  \x1b[33mTool '{}' not found\x1b[0m", name);
                                println!("  Use \x1b[36m/tools\x1b[0m to see available tools");
                            }
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mTool info not available via API\x1b[0m\n");
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::ToolExec(name, params) => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            if let Some(tool) = rt.tools.get(&name) {
                                println!("\n  Executing tool: {}", name);
                                let params_json: serde_json::Value = if params.is_empty() {
                                    serde_json::json!({})
                                } else {
                                    serde_json::from_str(&params).unwrap_or_else(|_| serde_json::json!({"input": params}))
                                };
                                let peer_id = rt.local_peer_id.to_string();
                                let ctx = crate::tools::ToolContext::local(peer_id);
                                match tool.execute(params_json, &ctx).await {
                                    Ok(output) => {
                                        println!("  \x1b[32mSuccess\x1b[0m ({}ms)", output.duration_ms);
                                        println!("  Result: {}", output.data);
                                    }
                                    Err(e) => {
                                        println!("  \x1b[31mError:\x1b[0m {}", e);
                                    }
                                }
                            } else {
                                println!("\n  \x1b[33mTool '{}' not found\x1b[0m", name);
                            }
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mTool execution not available via API\x1b[0m\n");
                        }
                    }
                    println!();
                    continue;
                }
                // Skill commands
                SlashCommand::Skills => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            let skills = rt.skills.list_local().await;
                            println!("\n\x1b[1m=== Available Skills ({}) ===\x1b[0m\n", skills.len());
                            if skills.is_empty() {
                                println!("  No skills found.");
                                println!("  Skills directory: {}", bootstrap::base_dir().join("skills").display());
                                println!("  Create a SKILL.md file to add a skill.");
                            } else {
                                for skill in &skills {
                                    let status = if skill.is_available() { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };
                                    println!("  {} {:20} {} (v{})",
                                        status,
                                        skill.name(),
                                        truncate(skill.description(), 30),
                                        skill.manifest.version
                                    );
                                }
                            }
                            println!();
                            println!("  Use \x1b[36m/skill info <name>\x1b[0m for details");
                            println!("  Use \x1b[36m/skill create <name>\x1b[0m to create new");
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mSkill listing not available via API\x1b[0m\n");
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::SkillInfo(name) => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            if let Some(skill) = rt.skills.get(&name).await {
                                println!("\n\x1b[1m=== Skill: {} ===\x1b[0m\n", skill.name());
                                println!("  Version:     {}", skill.manifest.version);
                                println!("  Description: {}", skill.description());
                                println!("  Trust:       {:?}", skill.trust);
                                println!("  Available:   {}", skill.is_available());
                                println!("  Hash:        {}", skill.hash);
                                if let Some(author) = &skill.manifest.author {
                                    println!("  Author:      {}", author);
                                }
                                if !skill.manifest.activation.keywords.is_empty() {
                                    println!("\n  Keywords: {}", skill.manifest.activation.keywords.join(", "));
                                }
                                if !skill.manifest.activation.tags.is_empty() {
                                    println!("  Tags:     {}", skill.manifest.activation.tags.join(", "));
                                }
                                println!("\n  Prompt Preview:");
                                println!("  {:-<56}", "");
                                let preview = truncate(skill.prompt(), 500);
                                for line in preview.lines().take(10) {
                                    println!("  {}", line);
                                }
                                if skill.prompt().len() > 500 {
                                    println!("  ... (truncated)");
                                }
                            } else {
                                println!("\n  \x1b[33mSkill '{}' not found\x1b[0m", name);
                                println!("  Use \x1b[36m/skills\x1b[0m to see available skills");
                            }
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mSkill info not available via API\x1b[0m\n");
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::SkillCreate(name) => {
                    let skills_dir = bootstrap::base_dir().join("skills");
                    let skill_path = skills_dir.join(format!("{}.md", &name));
                    if skill_path.exists() {
                        println!("\n  \x1b[33mSkill '{}' already exists at {}\x1b[0m", name, skill_path.display());
                    } else {
                        let template = format!(r#"---
name: {}
version: 1.0.0
description: A custom skill
author: Your Name
activation:
  keywords:
    - {}
  tags:
    - custom
requires:
  bins: []
  env: []
sharing:
  enabled: false
  price: 0
---

# {}

You are a helpful assistant with expertise in [your domain].

When helping users:
1. Be clear and concise
2. Provide examples when helpful
3. Ask clarifying questions if needed

## Guidelines

- Follow best practices
- Be security-conscious
- Cite sources when applicable
"#, name, name, name.replace("-", " ").to_uppercase());
                        if let Err(e) = std::fs::create_dir_all(&skills_dir) {
                            println!("\n  \x1b[31mError creating skills dir: {}\x1b[0m", e);
                        } else if let Err(e) = std::fs::write(&skill_path, template) {
                            println!("\n  \x1b[31mError creating skill: {}\x1b[0m", e);
                        } else {
                            println!("\n  \x1b[32mCreated skill template:\x1b[0m {}", skill_path.display());
                            println!("\n  Edit the file to customize, then run \x1b[36m/skill scan\x1b[0m");
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::SkillScan => {
                    match &mode {
                        ChatMode::Local { runtime: rt } => {
                            println!("\n  Scanning skills directory...");
                            match rt.skills.scan().await {
                                Ok(count) => {
                                    println!("  \x1b[32mLoaded {} skills\x1b[0m", count);
                                }
                                Err(e) => {
                                    println!("  \x1b[31mError scanning skills: {}\x1b[0m", e);
                                }
                            }
                        }
                        ChatMode::Api { .. } => {
                            println!("\n  \x1b[33mSkill scanning not available via API\x1b[0m\n");
                        }
                    }
                    println!();
                    continue;
                }
                SlashCommand::Quit => {
                    println!("\x1b[33mGoodbye!\x1b[0m");
                    break;
                }
            }
        }

        // Unknown slash command
        if input.starts_with('/') {
            println!("\x1b[33mUnknown command. Type /help for available commands.\x1b[0m\n");
            continue;
        }

        // Build prompt with history
        let full_prompt = build_prompt(&settings.system_prompt, &history, input);

        // Execute inference
        print!("\n\x1b[35mAssistant:\x1b[0m ");
        io::stdout().flush()?;

        let response = match &mode {
            ChatMode::Local { runtime: rt } => {
                if settings.stream {
                    // Use streaming inference - tokens print directly as they're generated
                    let result = rt.inference_streaming_print(
                        &settings.model,
                        &full_prompt,
                        settings.max_tokens,
                        settings.temperature,
                    ).await;

                    match result {
                        Ok(result) => {
                            // Text was already printed via streaming callback
                            let metrics = if result.tokens_generated > 0 {
                                Some(format!(
                                    "\x1b[90m[{} tokens, {:.1} tok/s, Local, streamed]\x1b[0m",
                                    result.tokens_generated,
                                    result.tokens_per_second,
                                ))
                            } else {
                                None
                            };
                            Ok((result.text, metrics))
                        }
                        Err(e) => Err(e.to_string()),
                    }
                } else {
                    // Non-streaming inference
                    let task = InferenceTask::new(&settings.model, &full_prompt)
                        .with_max_tokens(settings.max_tokens)
                        .with_temperature(settings.temperature);

                    match rt.execute_task(ExecutionTask::Inference(task)).await {
                        Ok(result) => {
                            let provider_info = match &result.location {
                                ExecutionLocation::Local => "Local".to_string(),
                                ExecutionLocation::Remote { peer_id, .. } => {
                                    let short = if peer_id.len() > 16 {
                                        format!("{}...{}", &peer_id[..8], &peer_id[peer_id.len()-8..])
                                    } else {
                                        peer_id.clone()
                                    };
                                    format!("Remote ({})", short)
                                }
                            };
                            match &result.data {
                                TaskData::Inference(r) => {
                                    let metrics = if r.tokens_generated > 0 {
                                        Some(format!(
                                            "\x1b[90m[{} tokens, {:.1} tok/s, {}]\x1b[0m",
                                            r.tokens_generated,
                                            r.tokens_per_second,
                                            provider_info
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
            }
            ChatMode::Api { base_url } => {
                // Use the API endpoint (non-streaming for now)
                execute_via_api(base_url, &settings.model, &full_prompt, settings.max_tokens, settings.temperature).await
            }
        };

        match response {
            Ok((text, metrics)) => {
                // For streaming, text was already printed
                if !settings.stream {
                    println!("{}", text);
                }
                history.push((input.to_string(), text.clone()));

                // Update session stats (estimate tokens from text length / 4)
                let estimated_tokens = (text.len() / 4) as u64;
                session_stats.total_tokens += estimated_tokens;
                session_stats.total_requests += 1;

                if let Some(m) = metrics {
                    println!("\n{}", m);
                }
            }
            Err(e) => {
                println!("\x1b[31m[Error: {}]\x1b[0m", e);
            }
        }

        println!();
    }

    // Save settings on exit
    settings.save().ok();

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

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a safe character boundary
        let mut end = max_len.saturating_sub(3);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
