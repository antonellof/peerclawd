//! PeerClaw'd - Decentralized P2P AI Agent Network
//!
//! One binary. Distributed intelligence. Token-powered autonomy.

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use peerclawd::cli::{Cli, Command};
use peerclawd::bootstrap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables from .peerclawd/.env if present
    bootstrap::load_env();

    // Parse CLI arguments
    let cli = Cli::parse();

    // For interactive mode, use minimal logging
    let log_level = match &cli.command {
        None | Some(Command::Start) | Some(Command::Chat(_)) => "peerclawd=warn",
        _ => "peerclawd=info",
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
            peerclawd::cli::start::run_interactive().await?;
        }
        Some(Command::Chat(args)) => {
            peerclawd::cli::chat::run(args).await?;
        }
        Some(Command::Models(args)) => {
            peerclawd::cli::models::run(args).await?;
        }
        Some(Command::Peers(args)) => {
            peerclawd::cli::peers::run(args).await?;
        }
        Some(Command::Serve(args)) => {
            peerclawd::cli::serve::run(args).await?;
        }
        Some(Command::Agent { cmd }) => {
            peerclawd::cli::agent::run(cmd).await?;
        }
        Some(Command::Network { cmd }) => {
            peerclawd::cli::network::run(cmd).await?;
        }
        Some(Command::Wallet { cmd }) => {
            peerclawd::cli::wallet::run(cmd).await?;
        }
        Some(Command::Tool { cmd }) => {
            peerclawd::cli::tool::run(cmd).await?;
        }
        Some(Command::Job(args)) => {
            peerclawd::cli::job::run(args).await?;
        }
        Some(Command::Test(args)) => {
            peerclawd::cli::test::run(args).await?;
        }
        Some(Command::Version) => {
            println!("peerclawd {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
