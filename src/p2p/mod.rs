//! P2P networking layer using libp2p.
//!
//! Provides peer discovery, messaging, and resource advertisement.

mod behaviour;
mod events;
mod resource;

pub use behaviour::PeerclawdBehaviour;
pub use events::NetworkEvent;
pub use resource::{Capability, ResourceManifest, Resources};

use futures::StreamExt;
use libp2p::{
    identity::Keypair,
    kad,
    mdns,
    swarm::{SwarmEvent, Swarm},
    Multiaddr, PeerId,
};
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::interval;

use crate::config::P2pConfig;
use crate::identity::NodeIdentity;

/// Network controller for managing P2P connections.
pub struct Network {
    /// The libp2p swarm - public for direct polling in serve loop
    pub swarm: Swarm<PeerclawdBehaviour>,
    event_tx: broadcast::Sender<NetworkEvent>,
    local_peer_id: PeerId,
    connected_peers: HashSet<PeerId>,
    config: P2pConfig,
}

impl Network {
    /// Create a new network controller.
    pub fn new(identity: &NodeIdentity, config: P2pConfig) -> anyhow::Result<Self> {
        let keypair = identity.to_libp2p_keypair();
        let local_peer_id = *identity.peer_id();

        let swarm = behaviour::build_swarm(keypair, &config)?;

        let (event_tx, _) = broadcast::channel(256);

        Ok(Self {
            swarm,
            event_tx,
            local_peer_id,
            connected_peers: HashSet::new(),
            config,
        })
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Get a receiver for network events.
    pub fn event_receiver(&self) -> broadcast::Receiver<NetworkEvent> {
        self.event_tx.subscribe()
    }

    /// Get the list of connected peers.
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.connected_peers.iter().cloned().collect()
    }

    /// Dial a remote peer by multiaddress.
    pub fn dial(&mut self, addr: Multiaddr) -> anyhow::Result<()> {
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Start the network and listen on configured addresses.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        // Listen on configured addresses
        for addr_str in &self.config.listen_addresses {
            let addr: Multiaddr = addr_str.parse()?;
            self.swarm.listen_on(addr)?;
            tracing::info!("Listening on {}", addr_str);
        }

        // Dial bootstrap peers
        for peer_str in &self.config.bootstrap_peers {
            let addr: Multiaddr = peer_str.parse()?;
            self.swarm.dial(addr)?;
            tracing::info!("Dialing bootstrap peer: {}", peer_str);
        }

        Ok(())
    }

    /// Run the network event loop.
    pub async fn run(&mut self, mut shutdown_rx: mpsc::Receiver<()>) -> anyhow::Result<()> {
        let mut advertise_interval = interval(Duration::from_secs(self.config.advertise_interval_secs));

        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    let _ = self.handle_swarm_event_internal(event).await;
                }

                // Periodic resource advertisement
                _ = advertise_interval.tick() => {
                    self.advertise_resources().await;
                }

                // Shutdown signal
                _ = shutdown_rx.recv() => {
                    tracing::info!("Network shutdown signal received");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Process a swarm event and return any resulting NetworkEvent.
    /// This is called from the serve loop to drive the network.
    pub async fn process_swarm_event(&mut self, event: SwarmEvent<behaviour::PeerclawdBehaviourEvent>) -> Option<NetworkEvent> {
        self.handle_swarm_event_internal(event).await
    }

    /// Handle a swarm event internally.
    async fn handle_swarm_event_internal(&mut self, event: SwarmEvent<behaviour::PeerclawdBehaviourEvent>) -> Option<NetworkEvent> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!("Listening on {}", address);
                None
            }

            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                self.connected_peers.insert(peer_id);
                tracing::info!("Connected to peer: {}", peer_id);
                Some(NetworkEvent::PeerConnected(peer_id))
            }

            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                self.connected_peers.remove(&peer_id);
                tracing::info!("Disconnected from peer: {}", peer_id);
                Some(NetworkEvent::PeerDisconnected(peer_id))
            }

            SwarmEvent::Behaviour(behaviour::PeerclawdBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer_id, addr) in &peers {
                    tracing::debug!("mDNS discovered peer: {} at {}", peer_id, addr);

                    // Add to Kademlia routing table
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(peer_id, addr.clone());
                }
                // Return first discovered peer
                peers.first().map(|(peer_id, addr)| NetworkEvent::PeerDiscovered {
                    peer_id: *peer_id,
                    addresses: vec![addr.clone()],
                })
            }

            SwarmEvent::Behaviour(behaviour::PeerclawdBehaviourEvent::Mdns(mdns::Event::Expired(peers))) => {
                for (peer_id, _) in peers {
                    tracing::debug!("mDNS peer expired: {}", peer_id);
                }
                None
            }

            SwarmEvent::Behaviour(behaviour::PeerclawdBehaviourEvent::Kademlia(kad::Event::RoutingUpdated { peer, .. })) => {
                tracing::debug!("Kademlia routing updated for peer: {}", peer);
                None
            }

            SwarmEvent::Behaviour(behaviour::PeerclawdBehaviourEvent::Gossipsub(event)) => {
                if let libp2p::gossipsub::Event::Message { message, .. } = event {
                    Some(NetworkEvent::GossipMessage {
                        topic: message.topic.to_string(),
                        data: message.data,
                        source: message.source,
                    })
                } else {
                    None
                }
            }

            SwarmEvent::Behaviour(behaviour::PeerclawdBehaviourEvent::Identify(event)) => {
                if let libp2p::identify::Event::Received { peer_id, info, .. } = event {
                    tracing::debug!(
                        "Identify received from {}: {} with {} addresses",
                        peer_id,
                        info.agent_version,
                        info.listen_addrs.len()
                    );

                    // Add addresses to Kademlia
                    for addr in info.listen_addrs {
                        self.swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, addr);
                    }
                }
                None
            }

            _ => None,
        }
    }

    /// Advertise resources to the DHT.
    async fn advertise_resources(&mut self) {
        // TODO: Build and publish ResourceManifest to DHT
        tracing::debug!("Resource advertisement tick");
    }

    /// Subscribe to a GossipSub topic.
    pub fn subscribe(&mut self, topic: &str) -> anyhow::Result<()> {
        let topic = libp2p::gossipsub::IdentTopic::new(topic);
        self.swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
        tracing::info!("Subscribed to topic: {}", topic);
        Ok(())
    }

    /// Publish a message to a GossipSub topic.
    pub fn publish(&mut self, topic: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let topic = libp2p::gossipsub::IdentTopic::new(topic);
        self.swarm.behaviour_mut().gossipsub.publish(topic, data)?;
        Ok(())
    }
}
