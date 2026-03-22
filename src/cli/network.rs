//! `peerclaw network` commands - Network operations.

use clap::Subcommand;

use crate::bootstrap;
use crate::db::Database;
use crate::identity::NodeIdentity;

#[derive(Subcommand)]
pub enum NetworkCommand {
    /// Show network topology and connected peers
    Status,

    /// List known peers and their resources
    Peers,

    /// Force peer discovery round
    Discover,

    /// Show local node identity
    Identity,
}

pub async fn run(cmd: NetworkCommand) -> anyhow::Result<()> {
    match cmd {
        NetworkCommand::Status => {
            show_status().await?;
        }
        NetworkCommand::Peers => {
            list_peers().await?;
        }
        NetworkCommand::Discover => {
            trigger_discover().await?;
        }
        NetworkCommand::Identity => {
            show_identity().await?;
        }
    }

    Ok(())
}

async fn show_status() -> anyhow::Result<()> {
    println!("Network Status");
    println!("{}", "=".repeat(40));

    // Load identity
    let identity_path = bootstrap::identity_path();
    if identity_path.exists() {
        let identity = NodeIdentity::load(&identity_path)?;
        println!("Peer ID:    {}", identity.peer_id());
        println!("Public Key: {}", hex::encode(identity.public_key_bytes()));
    } else {
        println!("Peer ID:    (not initialized)");
    }

    // Check database for peer count
    let db_path = bootstrap::database_path();
    if db_path.exists() {
        let db = Database::open(&db_path)?;
        let peer_count = db.list_peer_ids()?.len();
        println!("Known Peers: {}", peer_count);
    } else {
        println!("Known Peers: 0 (database not initialized)");
    }

    // Check if node is running by trying to connect to web API
    let config_path = bootstrap::config_path();
    let web_addr = if config_path.exists() {
        let config_str = std::fs::read_to_string(&config_path).unwrap_or_default();
        let config: toml::Value = toml::from_str(&config_str).unwrap_or(toml::Value::Table(Default::default()));
        config.get("web")
            .and_then(|w| w.get("listen_addr"))
            .and_then(|a| a.as_str())
            .unwrap_or("127.0.0.1:8080")
            .to_string()
    } else {
        "127.0.0.1:8080".to_string()
    };

    match reqwest::Client::new()
        .get(format!("http://{}/api/status", web_addr))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(status) = resp.json::<serde_json::Value>().await {
                println!("\nLive Node Status:");
                if let Some(peers) = status.get("connected_peers").and_then(|v| v.as_u64()) {
                    println!("  Connected:     {} peers", peers);
                }
                if let Some(active) = status.get("active_jobs").and_then(|v| v.as_u64()) {
                    println!("  Active Jobs:   {}", active);
                }
                if let Some(completed) = status.get("completed_jobs").and_then(|v| v.as_u64()) {
                    println!("  Completed:     {}", completed);
                }
                if let Some(cpu) = status.get("cpu_usage").and_then(|v| v.as_f64()) {
                    println!("  CPU Usage:     {:.1}%", cpu * 100.0);
                }
                if let Some(balance) = status.get("balance").and_then(|v| v.as_f64()) {
                    println!("  Balance:       {:.6} PCLAW", balance);
                }
            }
        }
        _ => {
            println!("\nNode Status: Offline");
            println!("  Start with: peerclaw serve");
        }
    }

    Ok(())
}

async fn list_peers() -> anyhow::Result<()> {
    let db_path = bootstrap::database_path();
    if !db_path.exists() {
        println!("No peers known (database not initialized)");
        println!("Run 'peerclaw serve' to start discovering peers.");
        return Ok(());
    }

    let db = Database::open(&db_path)?;
    let peer_ids = db.list_peer_ids()?;

    if peer_ids.is_empty() {
        println!("No peers discovered yet.");
        println!("\nPeers are discovered automatically when the node is running.");
        println!("mDNS discovers LAN peers, Kademlia DHT finds internet peers.");
        return Ok(());
    }

    println!("{:<52} {:<15}", "PEER ID", "STATUS");
    println!("{}", "-".repeat(70));

    for id in &peer_ids {
        // Try to get stored peer info
        let status = if let Ok(Some(info)) = db.get_peer::<serde_json::Value>(id) {
            info.get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("known")
                .to_string()
        } else {
            "known".to_string()
        };

        // Truncate peer ID for display
        let display_id = if id.len() > 50 {
            format!("{}...{}", &id[..8], &id[id.len()-8..])
        } else {
            id.clone()
        };

        println!("{:<52} {:<15}", display_id, status);
    }

    println!("\nTotal: {} peer(s)", peer_ids.len());

    Ok(())
}

async fn trigger_discover() -> anyhow::Result<()> {
    println!("Triggering peer discovery...");

    // Try to reach the running node's API
    match reqwest::Client::new()
        .get("http://127.0.0.1:8080/api/status")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            println!("Node is running. Discovery is automatic via:");
            println!("  - mDNS (LAN peers)");
            println!("  - Kademlia DHT (internet peers)");
            println!("  - GossipSub (peer exchange)");
            println!("\nCheck discovered peers: peerclaw network peers");
        }
        _ => {
            println!("Node is not running.");
            println!("Start the node first: peerclaw serve");
            println!("Peer discovery runs automatically when the node is active.");
        }
    }

    Ok(())
}

async fn show_identity() -> anyhow::Result<()> {
    let identity_path = bootstrap::identity_path();

    if !identity_path.exists() {
        println!("No identity found. Run 'peerclaw serve' to generate one.");
        return Ok(());
    }

    let identity = NodeIdentity::load(&identity_path)?;

    println!("Node Identity");
    println!("{}", "=".repeat(40));
    println!("Peer ID:       {}", identity.peer_id());
    println!("Public Key:    {}", hex::encode(identity.public_key_bytes()));
    println!("Key File:      {}", identity_path.display());
    println!("Algorithm:     Ed25519");

    Ok(())
}
