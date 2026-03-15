//! PeerClaw - Decentralized P2P AI Agent Network
//!
//! One binary. Distributed intelligence. Token-powered autonomy.

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use peerclaw::cli::{Cli, Command};
use peerclaw::bootstrap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables from .peerclaw/.env if present
    bootstrap::load_env();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Silence llama.cpp/ggml logs unless --debug is passed
    if !cli.debug {
        peerclaw::inference::silence_llama_logs();
    }

    // For interactive mode, use minimal logging
    let log_level = match &cli.command {
        None | Some(Command::Start) | Some(Command::Chat(_)) | Some(Command::Run(_)) => "peerclaw=warn",
        _ => "peerclaw=info",
    };

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Dispatch to command handlers
    match cli.command {
        // No command = interactive mode
        None | Some(Command::Start) => {
            peerclaw::cli::start::run_interactive().await?;
        }
        Some(Command::Chat(args)) => {
            peerclaw::cli::chat::run(args).await?;
        }
        // Ollama/vLLM-style commands
        Some(Command::Run(args)) => {
            peerclaw::cli::run::run(args).await?;
        }
        Some(Command::Pull(args)) => {
            peerclaw::cli::run::pull(args).await?;
        }
        Some(Command::List) => {
            peerclaw::cli::run::list().await?;
        }
        Some(Command::Ps) => {
            peerclaw::cli::run::ps().await?;
        }
        Some(Command::Models(args)) => {
            peerclaw::cli::models::run(args).await?;
        }
        Some(Command::Peers(args)) => {
            peerclaw::cli::peers::run(args).await?;
        }
        Some(Command::Serve(args)) => {
            peerclaw::cli::serve::run(args).await?;
        }
        Some(Command::Agent { cmd }) => {
            peerclaw::cli::agent::run(cmd).await?;
        }
        Some(Command::Network { cmd }) => {
            peerclaw::cli::network::run(cmd).await?;
        }
        Some(Command::Wallet { cmd }) => {
            peerclaw::cli::wallet::run(cmd).await?;
        }
        Some(Command::Tool { cmd }) => {
            peerclaw::cli::tool::run(cmd).await?;
        }
        Some(Command::Skill { cmd }) => {
            peerclaw::cli::skill::run(cmd).await?;
        }
        Some(Command::Job(args)) => {
            peerclaw::cli::job::run(args).await?;
        }
        Some(Command::Test(args)) => {
            peerclaw::cli::test::run(args).await?;
        }
        Some(Command::Version) => {
            println!("peerclaw {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
