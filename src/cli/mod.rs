//! CLI argument parsing and command dispatch.

pub mod agent;
pub mod chat;
pub mod job;
pub mod network;
pub mod serve;
pub mod test;
pub mod tool;
pub mod wallet;

use clap::{Parser, Subcommand};

/// PeerClaw'd - Decentralized P2P AI Agent Network
#[derive(Parser)]
#[command(name = "peerclawd")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start a peer node
    Serve(serve::ServeArgs),

    /// Agent management
    Agent {
        #[command(subcommand)]
        cmd: agent::AgentCommand,
    },

    /// Network operations
    Network {
        #[command(subcommand)]
        cmd: network::NetworkCommand,
    },

    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        cmd: wallet::WalletCommand,
    },

    /// Tool management
    Tool {
        #[command(subcommand)]
        cmd: tool::ToolCommand,
    },

    /// Distributed job submission
    Job(job::JobArgs),

    /// Interactive AI chat
    Chat(chat::ChatArgs),

    /// Test distributed execution
    Test(test::TestArgs),

    /// Print version information
    Version,
}
