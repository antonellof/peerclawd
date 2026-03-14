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

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "peerclawd=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Dispatch to command handlers
    match cli.command {
        Command::Serve(args) => {
            peerclawd::cli::serve::run(args).await?;
        }
        Command::Agent { cmd } => {
            peerclawd::cli::agent::run(cmd).await?;
        }
        Command::Network { cmd } => {
            peerclawd::cli::network::run(cmd).await?;
        }
        Command::Wallet { cmd } => {
            peerclawd::cli::wallet::run(cmd).await?;
        }
        Command::Tool { cmd } => {
            peerclawd::cli::tool::run(cmd).await?;
        }
        Command::Job(args) => {
            peerclawd::cli::job::run(args).await?;
        }
        Command::Chat(args) => {
            peerclawd::cli::chat::run(args).await?;
        }
        Command::Test(args) => {
            peerclawd::cli::test::run(args).await?;
        }
        Command::Version => {
            println!("peerclawd {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
