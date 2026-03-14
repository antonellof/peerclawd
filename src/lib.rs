//! PeerClaw'd - Decentralized P2P AI Agent Network
//!
//! A fully decentralized, peer-to-peer network where autonomous AI agents
//! collaborate, share resources, and transact using a native token economy.

pub mod bootstrap;
pub mod cli;
pub mod config;
pub mod db;
pub mod identity;
pub mod job;
pub mod node;
pub mod p2p;
pub mod wallet;

// Re-export commonly used types
pub use config::Config;
pub use identity::NodeIdentity;
pub use node::Node;
pub use wallet::{Wallet, WalletConfig};
