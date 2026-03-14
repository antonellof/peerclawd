//! PeerClaw'd - Decentralized P2P AI Agent Network
//!
//! A fully decentralized, peer-to-peer network where autonomous AI agents
//! collaborate, share resources, and transact using a native token economy.

pub mod bootstrap;
pub mod channel;
pub mod cli;
pub mod config;
pub mod db;
pub mod executor;
pub mod identity;
pub mod inference;
pub mod job;
pub mod node;
pub mod p2p;
pub mod proxy;
pub mod runtime;
pub mod wallet;
pub mod wasm;
pub mod web;

// Re-export commonly used types
pub use config::Config;
pub use executor::{ExecutorConfig, ResourceMonitor, TaskExecutor};
pub use identity::NodeIdentity;
pub use inference::{InferenceConfig, InferenceEngine};
pub use node::Node;
pub use runtime::Runtime;
pub use wallet::{Wallet, WalletConfig};
