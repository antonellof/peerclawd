//! `peerclawd wallet` commands - Wallet operations.

use clap::Subcommand;
use std::path::PathBuf;
use std::sync::Arc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::identity::NodeIdentity;
use crate::wallet::{from_micro, to_micro, Wallet, WalletConfig};

#[derive(Subcommand)]
pub enum WalletCommand {
    /// Generate new keypair and wallet
    Create {
        /// Output path for the wallet file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Check token balance
    Balance,

    /// Transfer tokens
    Send {
        /// Recipient peer ID or address
        #[arg(value_name = "TO")]
        to: String,

        /// Amount to send (in PCLAW)
        #[arg(value_name = "AMOUNT")]
        amount: f64,
    },

    /// Transaction history
    History {
        /// Number of transactions to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Show wallet info (peer ID, public key)
    Info,

    /// Stake tokens as resource provider bond
    Stake {
        /// Amount to stake (in PCLAW)
        #[arg(value_name = "AMOUNT")]
        amount: f64,
    },

    /// Unstake tokens (withdraw from provider bond)
    Unstake {
        /// Amount to unstake (in PCLAW)
        #[arg(value_name = "AMOUNT")]
        amount: f64,
    },

    /// Show active escrows
    Escrows,
}

/// Load or create the wallet.
fn load_wallet() -> anyhow::Result<(Wallet, NodeIdentity)> {
    let identity_path = bootstrap::base_dir().join("identity.key");

    if !identity_path.exists() {
        anyhow::bail!("No wallet found. Run 'peerclawd wallet create' first.");
    }

    let identity = NodeIdentity::load(&identity_path)?;
    let config = Config::load()?;
    let database = Database::open(&config.database.path)?;

    let wallet = Wallet::new(
        Arc::new(identity.clone()),
        WalletConfig::default(),
        database,
    ).map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok((wallet, identity))
}

pub async fn run(cmd: WalletCommand) -> anyhow::Result<()> {
    match cmd {
        WalletCommand::Create { output } => {
            let path = output.unwrap_or_else(|| bootstrap::base_dir().join("identity.key"));

            if path.exists() {
                anyhow::bail!("Wallet already exists at {:?}. Use --output to specify a different path.", path);
            }

            // Generate new identity
            let identity = NodeIdentity::generate();

            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Save to file
            identity.save(&path)?;

            // Initialize empty wallet state in database
            let config = Config::load()?;
            let database = Database::open(&config.database.path)?;
            let _wallet = Wallet::new(
                Arc::new(identity.clone()),
                WalletConfig::default(),
                database,
            ).map_err(|e| anyhow::anyhow!("{}", e))?;

            println!("✓ Wallet created successfully!");
            println!("  Address: {}", identity.peer_id());
            println!("  Keyfile: {:?}", path);
            println!("  Balance: 0.000000 PCLAW");
        }

        WalletCommand::Balance => {
            let (wallet, _identity) = load_wallet()?;
            let balance = wallet.balance().await;

            println!("Wallet Balance");
            println!("--------------");
            println!("  Available:  {:>12.6} PCLAW", from_micro(balance.available));
            println!("  In escrow:  {:>12.6} PCLAW", from_micro(balance.in_escrow));
            println!("  Staked:     {:>12.6} PCLAW", from_micro(balance.staked));
            println!("  ─────────────────────────");
            println!("  Total:      {:>12.6} PCLAW", from_micro(balance.total));
        }

        WalletCommand::Send { to, amount } => {
            let (wallet, _identity) = load_wallet()?;
            let amount_micro = to_micro(amount);

            // Create escrow for the transfer (transfers are implemented as escrows)
            let escrow = wallet.create_escrow(
                amount_micro,
                to.clone(),
                format!("transfer_{}", chrono::Utc::now().timestamp()),
                86400, // 24 hour timeout
            ).await.map_err(|e| anyhow::anyhow!("{}", e))?;

            println!("✓ Transfer initiated");
            println!("  To:     {}", to);
            println!("  Amount: {:.6} PCLAW", amount);
            println!("  Escrow: {}", escrow.id);
            println!();
            println!("Note: Transfer held in escrow until recipient confirms.");
        }

        WalletCommand::History { limit } => {
            let (wallet, _identity) = load_wallet()?;
            let transactions = wallet.transactions(limit).await;

            println!("Transaction History");
            println!("─────────────────────────────────────────────────────────────────");

            if transactions.is_empty() {
                println!("No transactions yet");
            } else {
                for tx in transactions.iter().rev() {
                    println!("{}", tx.display_line());
                }
            }
        }

        WalletCommand::Info => {
            let identity_path = bootstrap::base_dir().join("identity.key");

            if !identity_path.exists() {
                println!("No wallet found. Run 'peerclawd wallet create' to create one.");
                return Ok(());
            }

            let identity = NodeIdentity::load(&identity_path)?;
            let (wallet, _) = load_wallet()?;
            let balance = wallet.balance().await;

            println!("Wallet Info");
            println!("-----------");
            println!("Address:    {}", identity.peer_id());
            println!("Public Key: {}", hex::encode(identity.public_key_bytes()));
            println!("Keyfile:    {:?}", identity_path);
            println!();
            println!("Balance:    {:.6} PCLAW", from_micro(balance.total));
        }

        WalletCommand::Stake { amount } => {
            let (wallet, _identity) = load_wallet()?;
            let amount_micro = to_micro(amount);

            wallet.stake(amount_micro).await.map_err(|e| anyhow::anyhow!("{}", e))?;

            let balance = wallet.balance().await;
            println!("✓ Staked {:.6} PCLAW", amount);
            println!("  New staked balance: {:.6} PCLAW", from_micro(balance.staked));
        }

        WalletCommand::Unstake { amount } => {
            let (wallet, _identity) = load_wallet()?;
            let amount_micro = to_micro(amount);

            wallet.unstake(amount_micro).await.map_err(|e| anyhow::anyhow!("{}", e))?;

            let balance = wallet.balance().await;
            println!("✓ Unstaked {:.6} PCLAW", amount);
            println!("  New staked balance: {:.6} PCLAW", from_micro(balance.staked));
        }

        WalletCommand::Escrows => {
            let (wallet, _identity) = load_wallet()?;
            let escrows = wallet.active_escrows().await;

            println!("Active Escrows");
            println!("──────────────────────────────────────────────────────────────────");

            if escrows.is_empty() {
                println!("No active escrows");
            } else {
                for escrow in &escrows {
                    let remaining = escrow.time_remaining();
                    let remaining_str = if remaining.num_hours() > 0 {
                        format!("{}h {}m", remaining.num_hours(), remaining.num_minutes() % 60)
                    } else {
                        format!("{}m", remaining.num_minutes())
                    };

                    println!(
                        "{}: {:.6} PCLAW → {} (expires in {})",
                        escrow.id,
                        escrow.amount_pclaw(),
                        escrow.recipient,
                        remaining_str
                    );
                }
                println!();
                println!("Total in escrow: {:.6} PCLAW",
                    escrows.iter().map(|e| e.amount_pclaw()).sum::<f64>());
            }
        }
    }

    Ok(())
}
