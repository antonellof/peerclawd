//! CLI argument parsing and command dispatch.

pub mod agent;
pub mod chat;
pub mod job;
pub mod models;
pub mod network;
pub mod peers;
pub mod run;
pub mod serve;
pub mod start;
pub mod test;
pub mod tool;
pub mod wallet;

use clap::{Parser, Subcommand};

/// PeerClaw'd - Decentralized P2P AI Agent Network
///
/// Run without arguments to start in interactive mode.
#[derive(Parser)]
#[command(name = "peerclawd")]
#[command(author, version)]
#[command(about = "Decentralized P2P AI Agent Network", long_about = None)]
#[command(after_help = "Run without arguments to start in interactive mode with menu.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start in interactive mode (default)
    #[command(visible_alias = "i")]
    Start,

    /// Interactive AI chat
    #[command(visible_alias = "c")]
    Chat(chat::ChatArgs),

    /// Run a model with a prompt (Ollama-style)
    Run(run::RunArgs),

    /// Pull/download a model (alias for models download)
    Pull(run::PullArgs),

    /// List downloaded models (alias for models list)
    #[command(visible_alias = "ls")]
    List,

    /// Show running jobs/processes
    Ps,

    /// Manage AI models (list, download, remove)
    #[command(visible_alias = "m")]
    Models(models::ModelsArgs),

    /// Manage P2P peer connections
    #[command(visible_alias = "p")]
    Peers(peers::PeersArgs),

    /// Start a peer node (server mode)
    Serve(serve::ServeArgs),

    /// Agent management
    Agent {
        #[command(subcommand)]
        cmd: agent::AgentCommand,
    },

    /// Network operations (advanced)
    Network {
        #[command(subcommand)]
        cmd: network::NetworkCommand,
    },

    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        cmd: wallet::WalletCommand,
    },

    /// Tool management (WASM)
    Tool {
        #[command(subcommand)]
        cmd: tool::ToolCommand,
    },

    /// Distributed job submission
    Job(job::JobArgs),

    /// Test distributed execution
    Test(test::TestArgs),

    /// Print version information
    #[command(visible_alias = "v")]
    Version,
}
