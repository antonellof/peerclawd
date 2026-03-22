//! Swarm Manager for agent lifecycle and event broadcasting.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use libp2p::PeerId;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, info};
use uuid::Uuid;

use super::agent::{SwarmAgent, SwarmAgentState};
use super::event::{
    ActionType, AgentAction, AgentConnectionInfo, AgentSummary, ConnectionType, SwarmEvent,
};
use super::profile::{AgentCapability, AgentProfile};
use crate::p2p::{NetworkEvent, ResourceManifest};

/// Configuration for the SwarmManager
#[derive(Debug, Clone)]
pub struct SwarmManagerConfig {
    /// Maximum number of events to buffer
    pub event_buffer_size: usize,
    /// Maximum number of actions to keep in history
    pub max_action_history: usize,
    /// Whether to track remote peers as agents
    pub track_remote_peers: bool,
}

impl Default for SwarmManagerConfig {
    fn default() -> Self {
        Self {
            event_buffer_size: 256,
            max_action_history: 1000,
            track_remote_peers: true,
        }
    }
}

/// Manages swarm agents and broadcasts events for visualization.
pub struct SwarmManager {
    config: SwarmManagerConfig,

    /// All tracked agents (local + remote)
    agents: RwLock<HashMap<Uuid, SwarmAgent>>,

    /// Peer ID to agent ID mapping
    peer_to_agent: RwLock<HashMap<String, Uuid>>,

    /// Event broadcast channel
    event_tx: broadcast::Sender<SwarmEvent>,

    /// Action history for timeline
    action_history: RwLock<Vec<AgentAction>>,

    /// Connection tracking
    connections: RwLock<Vec<AgentConnectionInfo>>,

    /// Statistics
    stats: RwLock<SwarmStats>,
}

#[derive(Debug, Default)]
struct SwarmStats {
    total_actions: u64,
    total_jobs: u64,
}

impl SwarmManager {
    /// Create a new SwarmManager
    pub fn new(config: SwarmManagerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(config.event_buffer_size);

        Self {
            config,
            agents: RwLock::new(HashMap::new()),
            peer_to_agent: RwLock::new(HashMap::new()),
            event_tx,
            action_history: RwLock::new(Vec::new()),
            connections: RwLock::new(Vec::new()),
            stats: RwLock::new(SwarmStats::default()),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SwarmManagerConfig::default())
    }

    /// Subscribe to swarm events
    pub fn subscribe(&self) -> broadcast::Receiver<SwarmEvent> {
        self.event_tx.subscribe()
    }

    /// Register a local agent
    pub fn register_local_agent(&self, name: String, profile: AgentProfile) -> Uuid {
        let agent = SwarmAgent::new_local(name.clone(), profile.clone());
        let agent_id = agent.id;

        self.agents.write().insert(agent_id, agent.clone());

        // Broadcast join event
        let event = SwarmEvent::AgentJoined {
            agent_id,
            name,
            peer_id: None,
            profile,
            is_local: true,
            timestamp: Utc::now(),
        };
        let _ = self.event_tx.send(event);

        info!("Registered local agent: {}", agent_id);
        agent_id
    }

    /// Register an agent from a discovered peer
    pub fn register_peer_agent(&self, peer_id: PeerId, manifest: Option<&ResourceManifest>) -> Uuid {
        let peer_id_str = peer_id.to_string();

        // Check if already registered
        if let Some(existing_id) = self.peer_to_agent.read().get(&peer_id_str) {
            return *existing_id;
        }

        // Build profile from manifest
        let profile = if let Some(manifest) = manifest {
            let capabilities: Vec<AgentCapability> = manifest
                .capabilities
                .iter()
                .map(|c| AgentCapability::from(*c))
                .collect();
            AgentProfile::from_capabilities(capabilities)
        } else {
            AgentProfile::default()
        };

        let agent = SwarmAgent::from_peer(peer_id, None, profile.clone());
        let agent_id = agent.id;
        let name = agent.name.clone();

        self.agents.write().insert(agent_id, agent);
        self.peer_to_agent.write().insert(peer_id_str.clone(), agent_id);

        // Broadcast join event
        let event = SwarmEvent::AgentJoined {
            agent_id,
            name,
            peer_id: Some(peer_id_str),
            profile,
            is_local: false,
            timestamp: Utc::now(),
        };
        let _ = self.event_tx.send(event);

        info!("Registered peer agent: {} (peer: {})", agent_id, peer_id);
        agent_id
    }

    /// Remove an agent
    pub fn remove_agent(&self, agent_id: Uuid, reason: &str) {
        let agent = self.agents.write().remove(&agent_id);

        if let Some(agent) = agent {
            // Remove from peer mapping
            if let Some(peer_id) = &agent.peer_id {
                self.peer_to_agent.write().remove(peer_id);
            }

            // Remove connections involving this agent
            self.connections.write().retain(|c| c.from != agent_id && c.to != agent_id);

            // Broadcast leave event
            let event = SwarmEvent::AgentLeft {
                agent_id,
                name: agent.name,
                reason: reason.to_string(),
                timestamp: Utc::now(),
            };
            let _ = self.event_tx.send(event);

            info!("Removed agent: {} ({})", agent_id, reason);
        }
    }

    /// Update agent state
    pub fn update_agent_state(&self, agent_id: Uuid, new_state: SwarmAgentState) {
        let mut agents = self.agents.write();
        if let Some(agent) = agents.get_mut(&agent_id) {
            let old_state = agent.state.clone();
            agent.set_state(new_state.clone());

            // Broadcast state change
            let event = SwarmEvent::AgentStateChanged {
                agent_id,
                name: agent.name.clone(),
                old_state,
                new_state,
                timestamp: Utc::now(),
            };
            drop(agents);
            let _ = self.event_tx.send(event);
        }
    }

    /// Record an action
    pub fn record_action(&self, action: AgentAction) {
        // Update agent stats
        {
            let mut agents = self.agents.write();
            if let Some(agent) = agents.get_mut(&action.agent_id) {
                agent.record_action();
            }
        }

        // Update global stats
        self.stats.write().total_actions += 1;

        // Add to history (with limit)
        {
            let mut history = self.action_history.write();
            history.push(action.clone());
            if history.len() > self.config.max_action_history {
                history.remove(0);
            }
        }

        // Broadcast action event
        let event = SwarmEvent::AgentAction(action);
        let _ = self.event_tx.send(event);
    }

    /// Record a job-related action
    pub fn record_job_action(
        &self,
        agent_id: Uuid,
        action_type: ActionType,
        description: &str,
        success: bool,
    ) {
        let agent_name = self
            .agents
            .read()
            .get(&agent_id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        let mut action = AgentAction::new(
            agent_id,
            agent_name,
            action_type.clone(),
            description.to_string(),
        );

        if !success {
            action = action.failed();
        }

        // Update job stats
        if matches!(action_type, ActionType::JobCompleted | ActionType::JobFailed) {
            self.stats.write().total_jobs += 1;
            if let Some(agent) = self.agents.write().get_mut(&agent_id) {
                agent.record_job_result(success);
            }
        }

        self.record_action(action);
    }

    /// Add a connection between agents
    pub fn add_connection(&self, from: Uuid, to: Uuid, connection_type: ConnectionType) {
        let connection = AgentConnectionInfo {
            from,
            to,
            connection_type: connection_type.clone(),
            strength: 1.0,
        };

        self.connections.write().push(connection);

        let event = SwarmEvent::AgentConnection {
            from_agent: from,
            to_agent: to,
            connection_type,
            timestamp: Utc::now(),
        };
        let _ = self.event_tx.send(event);
    }

    /// Handle a P2P network event
    pub fn handle_network_event(&self, event: NetworkEvent) {
        if !self.config.track_remote_peers {
            return;
        }

        match event {
            NetworkEvent::PeerConnected(peer_id) => {
                self.register_peer_agent(peer_id, None);
            }
            NetworkEvent::PeerDisconnected(peer_id) => {
                let peer_id_str = peer_id.to_string();
                if let Some(agent_id) = self.peer_to_agent.read().get(&peer_id_str).copied() {
                    self.update_agent_state(agent_id, SwarmAgentState::Offline);
                }
            }
            NetworkEvent::ResourceAdvertised { peer_id, manifest } => {
                let peer_id_str = peer_id.to_string();
                let agent_id = if let Some(id) = self.peer_to_agent.read().get(&peer_id_str).copied() {
                    id
                } else {
                    self.register_peer_agent(peer_id, Some(&manifest))
                };

                // Update profile with new capabilities
                let capabilities: Vec<AgentCapability> = manifest
                    .capabilities
                    .iter()
                    .map(|c| AgentCapability::from(*c))
                    .collect();

                if let Some(agent) = self.agents.write().get_mut(&agent_id) {
                    agent.profile.capabilities = capabilities;
                    agent.last_active_at = Utc::now();
                }
            }
            _ => {
                debug!("Unhandled network event for swarm: {:?}", event);
            }
        }
    }

    /// Get all agents
    pub fn get_agents(&self) -> Vec<SwarmAgent> {
        self.agents.read().values().cloned().collect()
    }

    /// Get agent by ID
    pub fn get_agent(&self, id: Uuid) -> Option<SwarmAgent> {
        self.agents.read().get(&id).cloned()
    }

    /// Get agent by peer ID
    pub fn get_agent_by_peer(&self, peer_id: &str) -> Option<SwarmAgent> {
        let agent_id = self.peer_to_agent.read().get(peer_id).copied()?;
        self.get_agent(agent_id)
    }

    /// Get recent actions
    pub fn get_actions(&self, limit: usize, offset: usize) -> Vec<AgentAction> {
        let history = self.action_history.read();
        let total = history.len();
        if offset >= total {
            return Vec::new();
        }
        let start = total.saturating_sub(offset + limit);
        let end = total.saturating_sub(offset);
        history[start..end].to_vec()
    }

    /// Get topology data for visualization
    pub fn get_topology(&self) -> SwarmEvent {
        let agents: Vec<AgentSummary> = self
            .agents
            .read()
            .values()
            .map(|a| AgentSummary {
                id: a.id,
                name: a.name.clone(),
                peer_id: a.peer_id.clone(),
                state: a.state_display().to_string(),
                is_local: a.is_local,
                action_count: a.action_count,
                success_rate: a.success_rate(),
            })
            .collect();

        let connections = self.connections.read().clone();

        SwarmEvent::TopologyUpdate {
            agents,
            connections,
            timestamp: Utc::now(),
        }
    }

    /// Get current stats
    pub fn get_stats(&self) -> SwarmEvent {
        let agents = self.agents.read();
        let stats = self.stats.read();

        let active_count = agents
            .values()
            .filter(|a| a.is_busy())
            .count();

        SwarmEvent::StatsUpdate {
            total_agents: agents.len(),
            active_agents: active_count,
            total_actions: stats.total_actions,
            total_jobs: stats.total_jobs,
            timestamp: Utc::now(),
        }
    }

    /// Broadcast current topology (for new subscribers)
    pub fn broadcast_topology(&self) {
        let event = self.get_topology();
        let _ = self.event_tx.send(event);
    }

    /// Broadcast current stats
    pub fn broadcast_stats(&self) {
        let event = self.get_stats();
        let _ = self.event_tx.send(event);
    }
}

impl Default for SwarmManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Create a shared SwarmManager
#[allow(dead_code)]
pub fn create_swarm_manager(config: SwarmManagerConfig) -> Arc<SwarmManager> {
    Arc::new(SwarmManager::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_local_agent() {
        let manager = SwarmManager::with_defaults();
        let mut rx = manager.subscribe();

        let agent_id = manager.register_local_agent(
            "TestAgent".to_string(),
            AgentProfile::default(),
        );

        // Should have the agent
        assert!(manager.get_agent(agent_id).is_some());

        // Should have received join event
        let event = rx.try_recv().unwrap();
        matches!(event, SwarmEvent::AgentJoined { .. });
    }

    #[test]
    fn test_record_action() {
        let manager = SwarmManager::with_defaults();
        let agent_id = manager.register_local_agent(
            "TestAgent".to_string(),
            AgentProfile::default(),
        );

        let action = AgentAction::new(
            agent_id,
            "TestAgent".to_string(),
            ActionType::Inference,
            "Test inference".to_string(),
        );

        manager.record_action(action);

        let actions = manager.get_actions(10, 0);
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn test_topology() {
        let manager = SwarmManager::with_defaults();
        manager.register_local_agent("Agent1".to_string(), AgentProfile::default());
        manager.register_local_agent("Agent2".to_string(), AgentProfile::default());

        let topology = manager.get_topology();
        if let SwarmEvent::TopologyUpdate { agents, .. } = topology {
            assert_eq!(agents.len(), 2);
        } else {
            panic!("Expected TopologyUpdate");
        }
    }
}
