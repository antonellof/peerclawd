//! Swarm Agent definition and state machine.

use chrono::{DateTime, Utc};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::profile::AgentProfile;

/// A swarm agent represents an AI agent in the network.
/// Can be local (running on this node) or remote (discovered via P2P).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmAgent {
    /// Unique agent identifier
    pub id: Uuid,

    /// Associated P2P peer ID (if networked)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,

    /// Human-readable name
    pub name: String,

    /// Agent profile with personality and capabilities
    pub profile: AgentProfile,

    /// Current agent state
    pub state: SwarmAgentState,

    /// Current task description (if working)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,

    /// Total actions performed
    pub action_count: u64,

    /// Jobs completed successfully
    pub jobs_completed: u64,

    /// Jobs failed
    pub jobs_failed: u64,

    /// When the agent joined the swarm
    pub created_at: DateTime<Utc>,

    /// Last activity timestamp
    pub last_active_at: DateTime<Utc>,

    /// Whether this is a local agent (vs remote peer)
    pub is_local: bool,
}

impl SwarmAgent {
    /// Create a new local agent
    pub fn new_local(name: String, profile: AgentProfile) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            peer_id: None,
            name,
            profile,
            state: SwarmAgentState::Idle,
            current_task: None,
            action_count: 0,
            jobs_completed: 0,
            jobs_failed: 0,
            created_at: now,
            last_active_at: now,
            is_local: true,
        }
    }

    /// Create an agent from a discovered peer
    pub fn from_peer(peer_id: PeerId, name: Option<String>, profile: AgentProfile) -> Self {
        let now = Utc::now();
        let peer_id_str = peer_id.to_string();
        let display_name = name.unwrap_or_else(|| {
            // Generate name from peer ID (first 8 chars)
            format!("Peer-{}", &peer_id_str[..8.min(peer_id_str.len())])
        });

        Self {
            id: Uuid::new_v4(),
            peer_id: Some(peer_id_str),
            name: display_name,
            profile,
            state: SwarmAgentState::Idle,
            current_task: None,
            action_count: 0,
            jobs_completed: 0,
            jobs_failed: 0,
            created_at: now,
            last_active_at: now,
            is_local: false,
        }
    }

    /// Update agent state
    pub fn set_state(&mut self, state: SwarmAgentState) {
        self.state = state;
        self.last_active_at = Utc::now();
    }

    /// Record an action
    pub fn record_action(&mut self) {
        self.action_count += 1;
        self.last_active_at = Utc::now();
    }

    /// Record job completion
    pub fn record_job_result(&mut self, success: bool) {
        if success {
            self.jobs_completed += 1;
        } else {
            self.jobs_failed += 1;
        }
        self.last_active_at = Utc::now();
    }

    /// Calculate success rate (0.0 - 1.0)
    pub fn success_rate(&self) -> f64 {
        let total = self.jobs_completed + self.jobs_failed;
        if total == 0 {
            1.0 // No jobs yet, assume perfect
        } else {
            self.jobs_completed as f64 / total as f64
        }
    }

    /// Check if agent is currently busy
    pub fn is_busy(&self) -> bool {
        matches!(
            self.state,
            SwarmAgentState::Thinking | SwarmAgentState::Working { .. }
        )
    }

    /// Get state as display string
    pub fn state_display(&self) -> &'static str {
        match &self.state {
            SwarmAgentState::Idle => "idle",
            SwarmAgentState::Thinking => "thinking",
            SwarmAgentState::Working { .. } => "working",
            SwarmAgentState::Waiting { .. } => "waiting",
            SwarmAgentState::Error { .. } => "error",
            SwarmAgentState::Offline => "offline",
        }
    }
}

/// Agent state machine
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum SwarmAgentState {
    /// Idle, waiting for work
    #[default]
    Idle,

    /// Processing/reasoning
    Thinking,

    /// Actively executing a task
    Working {
        task: String,
    },

    /// Blocked on external resource
    Waiting {
        reason: String,
    },

    /// In error state
    Error {
        message: String,
    },

    /// Agent went offline (for remote peers)
    Offline,
}

impl std::fmt::Display for SwarmAgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Thinking => write!(f, "Thinking"),
            Self::Working { task } => write!(f, "Working: {}", task),
            Self::Waiting { reason } => write!(f, "Waiting: {}", reason),
            Self::Error { message } => write!(f, "Error: {}", message),
            Self::Offline => write!(f, "Offline"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::profile::AgentProfile;

    #[test]
    fn test_new_local_agent() {
        let profile = AgentProfile::default();
        let agent = SwarmAgent::new_local("TestAgent".to_string(), profile);

        assert!(agent.is_local);
        assert!(agent.peer_id.is_none());
        assert_eq!(agent.name, "TestAgent");
        assert_eq!(agent.state, SwarmAgentState::Idle);
        assert_eq!(agent.action_count, 0);
    }

    #[test]
    fn test_success_rate() {
        let profile = AgentProfile::default();
        let mut agent = SwarmAgent::new_local("TestAgent".to_string(), profile);

        // No jobs yet
        assert_eq!(agent.success_rate(), 1.0);

        // 3 successes, 1 failure
        agent.jobs_completed = 3;
        agent.jobs_failed = 1;
        assert_eq!(agent.success_rate(), 0.75);
    }
}
