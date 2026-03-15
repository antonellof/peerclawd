//! `peerclaw network` commands - Network operations.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum NetworkCommand {
    /// Show network topology and connected peers
    Status,

    /// List known peers and their resources
    Peers,

    /// Force peer discovery round
    Discover,
}

pub async fn run(cmd: NetworkCommand) -> anyhow::Result<()> {
    match cmd {
        NetworkCommand::Status => {
            // TODO: Show network status
            println!("Network Status");
            println!("--------------");
            println!("Status: Not connected (node not running)");
            println!("Connected peers: 0");
        }
        NetworkCommand::Peers => {
            // TODO: List peers from database
            println!("Known Peers");
            println!("-----------");
            println!("No peers discovered yet");
        }
        NetworkCommand::Discover => {
            // TODO: Trigger discovery
            println!("Forcing peer discovery...");
            println!("Note: Node must be running for discovery");
        }
    }

    Ok(())
}
