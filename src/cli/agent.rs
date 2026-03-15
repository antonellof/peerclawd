//! `peerclaw agent` commands - Agent management.

use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Deploy and run an agent from spec
    Run {
        /// Path to agent spec file (TOML)
        #[arg(value_name = "SPEC")]
        spec: PathBuf,
    },

    /// List running agents
    List,

    /// Stream agent logs
    Logs {
        /// Agent ID
        #[arg(value_name = "ID")]
        id: String,
    },

    /// Stop an agent
    Stop {
        /// Agent ID
        #[arg(value_name = "ID")]
        id: String,
    },
}

pub async fn run(cmd: AgentCommand) -> anyhow::Result<()> {
    match cmd {
        AgentCommand::Run { spec } => {
            tracing::info!("Loading agent spec from {:?}", spec);
            // TODO: Implement agent loading and execution
            println!("Agent deployment not yet implemented");
        }
        AgentCommand::List => {
            // TODO: List running agents
            println!("No agents running");
        }
        AgentCommand::Logs { id } => {
            // TODO: Stream agent logs
            println!("Streaming logs for agent: {}", id);
        }
        AgentCommand::Stop { id } => {
            // TODO: Stop agent
            println!("Stopping agent: {}", id);
        }
    }

    Ok(())
}
