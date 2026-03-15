//! libp2p behaviour composition.

use libp2p::{
    gossipsub, identify, kad,
    mdns,
    swarm::{NetworkBehaviour, Swarm},
    identity::Keypair,
    noise, tcp, yamux, PeerId,
};
use std::time::Duration;

use crate::config::P2pConfig;

/// Combined network behaviour for PeerClaw.
#[derive(NetworkBehaviour)]
pub struct PeerclawdBehaviour {
    /// Kademlia DHT for peer discovery and content routing
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,

    /// mDNS for local network discovery
    pub mdns: mdns::tokio::Behaviour,

    /// GossipSub for pub/sub messaging
    pub gossipsub: gossipsub::Behaviour,

    /// Identify protocol for peer information exchange
    pub identify: identify::Behaviour,
}

/// Build a complete swarm with all behaviours configured.
pub fn build_swarm(keypair: Keypair, config: &P2pConfig) -> anyhow::Result<Swarm<PeerclawdBehaviour>> {
    let local_peer_id = PeerId::from(keypair.public());

    // Build the swarm with TCP transport
    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|keypair| {
            // Kademlia DHT
            let kademlia = {
                let store = kad::store::MemoryStore::new(local_peer_id);
                let config = kad::Config::new(libp2p::StreamProtocol::new("/peerclaw/kad/1.0.0"));
                kad::Behaviour::with_config(local_peer_id, store, config)
            };

            // mDNS
            let mdns = mdns::tokio::Behaviour::new(
                mdns::Config::default(),
                local_peer_id,
            )?;

            // GossipSub
            let gossipsub = {
                let config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .build()
                    .map_err(|e| anyhow::anyhow!("GossipSub config error: {}", e))?;

                let message_id_fn = |message: &gossipsub::Message| {
                    // Use blake3 hash of message content as ID
                    let hash = blake3::hash(&message.data);
                    gossipsub::MessageId::from(hash.as_bytes().to_vec())
                };

                gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(keypair.clone()),
                    config,
                )
                .map_err(|e| anyhow::anyhow!("GossipSub creation error: {}", e))?
            };

            // Identify
            let identify = identify::Behaviour::new(
                identify::Config::new(
                    "/peerclaw/1.0.0".to_string(),
                    keypair.public(),
                )
                .with_agent_version(format!("peerclaw/{}", env!("CARGO_PKG_VERSION"))),
            );

            Ok(PeerclawdBehaviour {
                kademlia,
                mdns,
                gossipsub,
                identify,
            })
        })?
        .with_swarm_config(|cfg| {
            cfg.with_idle_connection_timeout(Duration::from_secs(60))
        })
        .build();

    Ok(swarm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::NodeIdentity;

    #[test]
    fn test_build_swarm() {
        let identity = NodeIdentity::generate();
        let config = P2pConfig::default();
        let result = build_swarm(identity.to_libp2p_keypair(), &config);
        assert!(result.is_ok());
    }
}
