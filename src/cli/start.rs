//! `peerclaw` default startup - Interactive mode with web server.
//!
//! When run without arguments, PeerClaw starts in interactive mode:
//! - Starts the web dashboard
//! - Initializes P2P networking
//! - Drops into chat mode for immediate AI interaction

use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::path::PathBuf;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::identity::NodeIdentity;
use crate::runtime::Runtime;
use crate::cli::chat::{ChatSettings, ChatArgs};

/// Check if this is the first run (no identity exists)
pub fn is_first_run() -> bool {
    !bootstrap::identity_path().exists()
}

/// Run the first-time setup wizard
pub async fn run_first_time_setup() -> anyhow::Result<()> {
    println!();
    println!("\x1b[1;36m╔══════════════════════════════════════════════════════════╗\x1b[0m");
    println!("\x1b[1;36m║     Welcome to PeerClaw - Decentralized AI Network     ║\x1b[0m");
    println!("\x1b[1;36m╚══════════════════════════════════════════════════════════╝\x1b[0m");
    println!();
    println!("This appears to be your first time running PeerClaw.");
    println!("Let's set up a few things to get you started.\n");

    // Ensure directories exist
    bootstrap::ensure_dirs()?;

    // Generate identity
    println!("\x1b[33m1. Generating your node identity...\x1b[0m");
    let identity = NodeIdentity::generate();
    identity.save(&bootstrap::identity_path())?;
    println!("   \x1b[32m✓\x1b[0m Node ID: \x1b[36m{}\x1b[0m\n", identity.peer_id());

    // Check for models
    println!("\x1b[33m2. Checking for AI models...\x1b[0m");
    let models_dir = bootstrap::base_dir().join("models");
    std::fs::create_dir_all(&models_dir)?;

    let models = list_local_models(&models_dir);
    if models.is_empty() {
        println!("   \x1b[33m!\x1b[0m No models found in {}", models_dir.display());
        println!();
        println!("   To download a model, run:");
        println!("   \x1b[36m  peerclaw models download llama-3.2-1b\x1b[0m");
        println!();
        println!("   Or manually download a GGUF model to:");
        println!("   \x1b[90m  {}\x1b[0m\n", models_dir.display());
    } else {
        println!("   \x1b[32m✓\x1b[0m Found {} model(s):", models.len());
        for model in &models {
            println!("     - \x1b[36m{}\x1b[0m", model);
        }
        println!();
    }

    // Initialize config
    println!("\x1b[33m3. Initializing configuration...\x1b[0m");
    let config = Config::default();
    config.save()?;
    println!("   \x1b[32m✓\x1b[0m Config saved to {}\n", bootstrap::config_path().display());

    // Create initial chat settings
    let chat_settings = ChatSettings::default();
    chat_settings.save().ok();
    println!("   \x1b[32m✓\x1b[0m Chat settings initialized\n");

    // Summary
    println!("\x1b[1;32m═══ Setup Complete! ═══\x1b[0m\n");
    println!("You can now:");
    println!("  • \x1b[36mpeerclaw\x1b[0m          - Start in interactive mode");
    println!("  • \x1b[36mpeerclaw chat\x1b[0m     - Start AI chat");
    println!("  • \x1b[36mpeerclaw serve\x1b[0m    - Start as a network node");
    println!("  • \x1b[36mpeerclaw models\x1b[0m   - Manage AI models");
    println!("  • \x1b[36mpeerclaw peers\x1b[0m    - Connect to the network");
    println!();

    Ok(())
}

/// Run the interactive startup mode
pub async fn run_interactive() -> anyhow::Result<()> {
    // Show banner
    print_banner();

    // Ensure directories
    bootstrap::ensure_dirs()?;

    // Load or create identity
    let identity = if bootstrap::identity_path().exists() {
        Arc::new(NodeIdentity::load(&bootstrap::identity_path())?)
    } else {
        // Run first-time setup
        run_first_time_setup().await?;
        Arc::new(NodeIdentity::load(&bootstrap::identity_path())?)
    };

    // Load config
    let config = Config::load()?;

    // Initialize database
    let db = Database::open(&config.database.path)?;

    // Create runtime
    println!("\x1b[90mInitializing runtime...\x1b[0m");
    let runtime = Runtime::new(identity.clone(), db, config.clone()).await?;

    // Start web server in background
    let web_port = 8080u16;
    println!("\x1b[90mStarting web dashboard on http://127.0.0.1:{}...\x1b[0m", web_port);

    // Start P2P network
    println!("\x1b[90mStarting P2P network...\x1b[0m");
    {
        let mut network = runtime.network.write().await;
        network.start().await?;
    }

    // Subscribe to job topics
    runtime.subscribe_to_job_topics().await?;

    // Wait briefly for connections
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Show status
    println!();
    println!("\x1b[1m═══════════════════════════════════════════════════════════\x1b[0m");
    println!("  \x1b[36mNode ID:\x1b[0m    {}...{}",
        &identity.peer_id().to_string()[..8],
        &identity.peer_id().to_string()[identity.peer_id().to_string().len()-8..]);
    println!("  \x1b[36mWeb UI:\x1b[0m     http://127.0.0.1:{}", web_port);
    println!("  \x1b[36mBalance:\x1b[0m    {:.2} PCLAW", crate::wallet::from_micro(runtime.balance().await));
    let peer_count: usize = runtime.connected_peers_count().await;
    println!("  \x1b[36mPeers:\x1b[0m      {} connected", peer_count);
    println!("\x1b[1m═══════════════════════════════════════════════════════════\x1b[0m");
    println!();

    // Check for models
    let models_dir = bootstrap::base_dir().join("models");
    let models = list_local_models(&models_dir);
    if models.is_empty() {
        println!("\x1b[33m⚠ No AI models found. Run 'peerclaw models download' to get started.\x1b[0m");
        println!();
    }

    // Show menu
    show_main_menu();

    // Main menu loop
    let stdin = io::stdin();
    loop {
        print!("\x1b[36m>\x1b[0m ");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;
        let input = input.trim();

        match input.to_lowercase().as_str() {
            "" => continue,
            "1" | "chat" | "c" => {
                // Run chat mode
                let args = ChatArgs {
                    model: "llama-3.2-3b".to_string(),
                    system: "You are a helpful AI assistant.".to_string(),
                    max_tokens: 500,
                    temperature: 0.7,
                    distributed: false,
                    standalone: false,
                    no_stream: false,
                };
                crate::cli::chat::run(args).await?;
                show_main_menu();
            }
            "2" | "status" | "s" => {
                show_status(&runtime).await;
            }
            "3" | "models" | "m" => {
                show_models(&models_dir);
            }
            "4" | "peers" | "p" => {
                show_peers(&runtime).await;
            }
            "5" | "web" | "w" => {
                println!("\n\x1b[36mWeb dashboard:\x1b[0m http://127.0.0.1:{}\n", web_port);
            }
            "6" | "settings" => {
                show_settings();
            }
            "7" | "help" | "h" | "?" => {
                show_main_menu();
            }
            "8" | "quit" | "exit" | "q" => {
                println!("\n\x1b[33mGoodbye!\x1b[0m\n");
                break;
            }
            _ => {
                println!("\x1b[33mUnknown option. Type 'help' for commands.\x1b[0m\n");
            }
        }
    }

    Ok(())
}

fn print_banner() {
    println!();
    println!("\x1b[1;36m ____                  ____ _                   _     _ \x1b[0m");
    println!("\x1b[1;36m|  _ \\ ___  ___ _ __  / ___| | __ ___      ____| |   | |\x1b[0m");
    println!("\x1b[1;36m| |_) / _ \\/ _ \\ '__|| |   | |/ _` \\ \\ /\\ / / _` |   | |\x1b[0m");
    println!("\x1b[1;36m|  __/  __/  __/ |   | |___| | (_| |\\ V  V / (_| |   |_|\x1b[0m");
    println!("\x1b[1;36m|_|   \\___|\\___|_|    \\____|_|\\__,_| \\_/\\_/ \\__,_|   (_)\x1b[0m");
    println!();
    println!("\x1b[90m  Decentralized P2P AI Agent Network - v{}\x1b[0m", env!("CARGO_PKG_VERSION"));
    println!();
}

fn show_main_menu() {
    println!("\x1b[1m┌─────────────────────────────────────┐\x1b[0m");
    println!("\x1b[1m│          Main Menu                  │\x1b[0m");
    println!("\x1b[1m├─────────────────────────────────────┤\x1b[0m");
    println!("│  \x1b[36m1.\x1b[0m Chat     - Start AI conversation │");
    println!("│  \x1b[36m2.\x1b[0m Status   - Show node status      │");
    println!("│  \x1b[36m3.\x1b[0m Models   - List AI models        │");
    println!("│  \x1b[36m4.\x1b[0m Peers    - Show connected peers  │");
    println!("│  \x1b[36m5.\x1b[0m Web      - Open web dashboard    │");
    println!("│  \x1b[36m6.\x1b[0m Settings - Configuration         │");
    println!("│  \x1b[36m7.\x1b[0m Help     - Show this menu        │");
    println!("│  \x1b[36m8.\x1b[0m Quit     - Exit                   │");
    println!("\x1b[1m└─────────────────────────────────────┘\x1b[0m");
    println!();
}

async fn show_status(runtime: &Runtime) {
    let stats = runtime.stats().await;
    println!();
    println!("\x1b[1m═══ Node Status ═══\x1b[0m");
    println!("  Peer ID:         \x1b[36m{}\x1b[0m", stats.peer_id);
    println!("  Connected Peers: \x1b[32m{}\x1b[0m", stats.connected_peers);
    println!("  Balance:         \x1b[33m{:.6} PCLAW\x1b[0m", stats.balance);
    println!("  Active Jobs:     {}", stats.active_jobs);
    println!("  Completed Jobs:  {}", stats.completed_jobs);
    println!();
    println!("\x1b[1m═══ Resources ═══\x1b[0m");
    println!("  CPU Usage:       {:.1}%", stats.resource_state.cpu_usage * 100.0);
    println!("  RAM:             {} / {} MB", stats.resource_state.ram_available_mb, stats.resource_state.ram_total_mb);
    println!("  GPU:             {}", if stats.resource_state.gpu_usage.is_some() { "\x1b[32mAvailable\x1b[0m" } else { "\x1b[90mNot available\x1b[0m" });
    println!();
}

fn show_models(models_dir: &PathBuf) {
    let models = list_local_models(models_dir);
    println!();
    println!("\x1b[1m═══ AI Models ═══\x1b[0m");
    println!("  Directory: \x1b[90m{}\x1b[0m", models_dir.display());
    println!();

    if models.is_empty() {
        println!("  \x1b[33mNo models found.\x1b[0m");
        println!();
        println!("  To download a model, run:");
        println!("  \x1b[36m  peerclaw models download llama-3.2-1b\x1b[0m");
    } else {
        for model in &models {
            // Get file size
            let path = models_dir.join(format!("{}.gguf", model));
            let size = std::fs::metadata(&path)
                .map(|m| format!("{:.1} GB", m.len() as f64 / 1_073_741_824.0))
                .unwrap_or_else(|_| "? GB".to_string());
            println!("  • \x1b[36m{}\x1b[0m \x1b[90m({})\x1b[0m", model, size);
        }
    }
    println!();
}

async fn show_peers(runtime: &Runtime) {
    let network = runtime.network.read().await;
    let peers = network.connected_peers();

    println!();
    println!("\x1b[1m═══ Connected Peers ═══\x1b[0m");

    if peers.is_empty() {
        println!("  \x1b[33mNo peers connected.\x1b[0m");
        println!();
        println!("  To connect to a peer, run:");
        println!("  \x1b[36m  peerclaw peers join <multiaddr>\x1b[0m");
    } else {
        for peer in peers {
            let short_id = if peer.to_string().len() > 16 {
                format!("{}...{}", &peer.to_string()[..8], &peer.to_string()[peer.to_string().len()-8..])
            } else {
                peer.to_string()
            };
            println!("  • \x1b[32m{}\x1b[0m", short_id);
        }
    }
    println!();
}

fn show_settings() {
    let config_path = bootstrap::config_path();
    let chat_settings_path = bootstrap::base_dir().join("chat_settings.json");

    println!();
    println!("\x1b[1m═══ Settings ═══\x1b[0m");
    println!("  Config file:    \x1b[90m{}\x1b[0m", config_path.display());
    println!("  Chat settings:  \x1b[90m{}\x1b[0m", chat_settings_path.display());
    println!("  Data directory: \x1b[90m{}\x1b[0m", bootstrap::base_dir().display());
    println!();
    println!("  Edit these files to customize your settings.");
    println!();
}

fn list_local_models(models_dir: &PathBuf) -> Vec<String> {
    std::fs::read_dir(models_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "gguf"))
                .filter_map(|e| {
                    e.path()
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}
