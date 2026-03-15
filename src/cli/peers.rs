//! `peerclawd peers` command - Manage P2P connections.

use std::sync::Arc;

use clap::{Args, Subcommand};

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::identity::NodeIdentity;
use crate::runtime::Runtime;

#[derive(Args)]
pub struct PeersArgs {
    #[command(subcommand)]
    pub cmd: Option<PeersCommand>,
}

#[derive(Subcommand)]
pub enum PeersCommand {
    /// List connected peers
    List,

    /// Connect to a peer by multiaddr
    Join {
        /// Peer multiaddress (e.g., /ip4/192.168.1.10/tcp/9000/p2p/12D3KooW...)
        addr: String,
    },

    /// Show this node's peer info
    Info,

    /// Discover peers on local network
    Discover,
}

/// Well-known bootstrap peers (community maintained)
const BOOTSTRAP_PEERS: &[(&str, &str)] = &[
    // Add community bootstrap peers here when available
    // ("name", "/ip4/x.x.x.x/tcp/9000/p2p/12D3KooW..."),
];

pub async fn run(args: PeersArgs) -> anyhow::Result<()> {
    match args.cmd {
        None | Some(PeersCommand::List) => list_peers().await,
        Some(PeersCommand::Join { addr }) => join_peer(&addr).await,
        Some(PeersCommand::Info) => show_info().await,
        Some(PeersCommand::Discover) => discover_peers().await,
    }
}

async fn list_peers() -> anyhow::Result<()> {
    let runtime = create_runtime().await?;

    // Start network
    {
        let mut network = runtime.network.write().await;
        network.start().await?;
    }

    // Wait for discovery
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let network = runtime.network.read().await;
    let peers = network.connected_peers();

    println!();
    println!("\x1b[1m═══ Connected Peers ═══\x1b[0m");
    println!("  Local Peer ID: \x1b[36m{}\x1b[0m", runtime.local_peer_id);
    println!();

    if peers.is_empty() {
        println!("  \x1b[33mNo peers connected.\x1b[0m");
        println!();
        println!("  To connect to a peer:");
        println!("  \x1b[36m  peerclawd peers join /ip4/<ip>/tcp/<port>/p2p/<peer_id>\x1b[0m");
        println!();
        println!("  To discover local peers:");
        println!("  \x1b[36m  peerclawd peers discover\x1b[0m");
    } else {
        println!("  \x1b[32m{} peer(s) connected:\x1b[0m", peers.len());
        println!();
        for peer in peers {
            println!("    • \x1b[36m{}\x1b[0m", peer);
        }
    }

    println!();

    if !BOOTSTRAP_PEERS.is_empty() {
        println!("\x1b[1m═══ Bootstrap Peers ═══\x1b[0m");
        for (name, _addr) in BOOTSTRAP_PEERS {
            println!("  • {} ", name);
        }
        println!();
    }

    Ok(())
}

async fn join_peer(addr: &str) -> anyhow::Result<()> {
    println!();
    println!("\x1b[1m═══ Connecting to Peer ═══\x1b[0m");
    println!("  Address: \x1b[36m{}\x1b[0m", addr);
    println!();

    // Parse multiaddr
    let multiaddr: libp2p::Multiaddr = addr.parse()
        .map_err(|e| anyhow::anyhow!("Invalid multiaddr: {}", e))?;

    let runtime = create_runtime().await?;

    // Start network
    {
        let mut network = runtime.network.write().await;
        network.start().await?;

        // Dial the peer
        println!("\x1b[90mConnecting...\x1b[0m");
        network.dial(multiaddr.clone())?;
    }

    // Wait for connection
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let peers: usize = runtime.connected_peers_count().await;
        if peers > 0 {
            println!();
            println!("\x1b[32m✓ Connected!\x1b[0m");
            println!("  Connected peers: {}", peers);
            println!();
            return Ok(());
        }
        print!(".");
        std::io::Write::flush(&mut std::io::stdout())?;
    }

    println!();
    println!("\x1b[33m⚠ Connection timeout.\x1b[0m");
    println!("  The peer may be offline or the address may be incorrect.");
    println!();

    Ok(())
}

async fn show_info() -> anyhow::Result<()> {
    let identity = load_identity()?;

    println!();
    println!("\x1b[1m═══ Node Info ═══\x1b[0m");
    println!("  Peer ID: \x1b[36m{}\x1b[0m", identity.peer_id());
    println!();
    println!("  Share this with others to let them connect to you:");
    println!();

    // Get listen addresses from config
    let config = Config::load()?;
    let listen_addr = config.p2p.listen_addresses.first()
        .cloned()
        .unwrap_or_else(|| "/ip4/0.0.0.0/tcp/9000".to_string());

    println!("  \x1b[90m{}/p2p/{}\x1b[0m", listen_addr, identity.peer_id());
    println!();
    println!("  Note: Replace the IP with your public IP if connecting over internet.");
    println!();

    Ok(())
}

async fn discover_peers() -> anyhow::Result<()> {
    println!();
    println!("\x1b[1m═══ Discovering Peers ═══\x1b[0m");
    println!("  Using mDNS to find peers on local network...");
    println!();

    let runtime = create_runtime().await?;

    // Start network with mDNS
    {
        let mut network = runtime.network.write().await;
        network.start().await?;
    }

    // Wait and check for discoveries
    print!("  Scanning");
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        print!(".");
        std::io::Write::flush(&mut std::io::stdout())?;
    }
    println!();
    println!();

    let peers: usize = runtime.connected_peers_count().await;

    if peers > 0 {
        println!("  \x1b[32m✓ Found {} peer(s)!\x1b[0m", peers);
        println!();

        let network = runtime.network.read().await;
        for peer in network.connected_peers() {
            println!("    • \x1b[36m{}\x1b[0m", peer);
        }
    } else {
        println!("  \x1b[33mNo peers found on local network.\x1b[0m");
        println!();
        println!("  To connect to a remote peer:");
        println!("  \x1b[36m  peerclawd peers join /ip4/<ip>/tcp/<port>/p2p/<peer_id>\x1b[0m");
    }

    println!();

    Ok(())
}

fn load_identity() -> anyhow::Result<NodeIdentity> {
    let path = bootstrap::identity_path();
    if path.exists() {
        Ok(NodeIdentity::load(&path)?)
    } else {
        anyhow::bail!("No identity found. Run 'peerclawd' first to initialize.")
    }
}

async fn create_runtime() -> anyhow::Result<Runtime> {
    bootstrap::ensure_dirs()?;

    let identity = Arc::new(load_identity()?);
    let config = Config::load()?;
    let db = Database::open(&config.database.path)?;

    Runtime::new(identity, db, config).await
}
