//! P2P network integration for job broadcasting.

use serde::{Deserialize, Serialize};
use libp2p::PeerId;

use super::{JobBid, JobId, JobRequest, JobResult};

/// Topics for job-related GossipSub messages.
pub mod topics {
    /// Topic for broadcasting job requests
    pub const JOB_REQUESTS: &str = "peerclaw/jobs/requests/v1";
    /// Topic for broadcasting job bids
    pub const JOB_BIDS: &str = "peerclaw/jobs/bids/v1";
    /// Topic for job status updates
    pub const JOB_STATUS: &str = "peerclaw/jobs/status/v1";
}

/// Message types sent over the P2P network for job coordination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobMessage {
    /// A new job request broadcast to the network
    Request(JobRequestMessage),
    /// A bid in response to a job request
    Bid(JobBidMessage),
    /// Notification that a bid was accepted
    BidAccepted(BidAcceptedMessage),
    /// Job result submitted by provider
    Result(JobResultMessage),
    /// Job status update
    StatusUpdate(JobStatusMessage),
}

/// Job request broadcast message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequestMessage {
    /// The job request
    pub request: JobRequest,
    /// Requester's peer ID
    pub requester_peer_id: String,
    /// Signature from requester (for verification)
    pub signature: Vec<u8>,
}

impl JobRequestMessage {
    pub fn new(request: JobRequest, peer_id: &PeerId) -> Self {
        Self {
            request,
            requester_peer_id: peer_id.to_string(),
            signature: vec![], // TODO: Sign with Ed25519
        }
    }
}

/// Bid message sent to the requester.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobBidMessage {
    /// The bid
    pub bid: JobBid,
    /// Bidder's peer ID
    pub bidder_peer_id: String,
    /// Signature from bidder
    pub signature: Vec<u8>,
}

impl JobBidMessage {
    pub fn new(bid: JobBid, peer_id: &PeerId) -> Self {
        Self {
            bid,
            bidder_peer_id: peer_id.to_string(),
            signature: vec![], // TODO: Sign with Ed25519
        }
    }
}

/// Notification that a bid was accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidAcceptedMessage {
    /// Job ID
    pub job_id: JobId,
    /// Accepted bid ID
    pub bid_id: String,
    /// Winner's peer ID
    pub winner_peer_id: String,
    /// Escrow ID for payment
    pub escrow_id: String,
    /// Requester's signature
    pub signature: Vec<u8>,
}

/// Job result message from provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResultMessage {
    /// Job ID
    pub job_id: JobId,
    /// The result
    pub result: JobResult,
    /// Provider's peer ID
    pub provider_peer_id: String,
    /// Provider's signature
    pub signature: Vec<u8>,
}

/// Job status update message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusMessage {
    /// Job ID
    pub job_id: JobId,
    /// New status
    pub status: JobStatusUpdate,
    /// Peer ID of sender
    pub peer_id: String,
    /// Timestamp
    pub timestamp: u64,
}

/// Status updates that can be broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatusUpdate {
    /// Job execution started
    Started,
    /// Progress update (0-100%)
    Progress { percent: u8, message: Option<String> },
    /// Job completed successfully
    Completed,
    /// Job failed
    Failed { reason: String },
    /// Job cancelled
    Cancelled,
}

/// Serialize a job message to bytes for network transmission.
pub fn serialize_message(msg: &JobMessage) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(msg)
}

/// Deserialize a job message from network bytes.
pub fn deserialize_message(data: &[u8]) -> Result<JobMessage, rmp_serde::decode::Error> {
    rmp_serde::from_slice(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{ResourceType, JobRequirements};
    use crate::wallet::to_micro;

    #[test]
    fn test_message_serialization() {
        let request = JobRequest::new(
            ResourceType::Inference {
                model: "llama-7b".into(),
                tokens: 1000,
            },
            to_micro(10.0),
            300,
        );

        let msg = JobMessage::Request(JobRequestMessage {
            request,
            requester_peer_id: "peer123".to_string(),
            signature: vec![1, 2, 3],
        });

        let bytes = serialize_message(&msg).unwrap();
        let decoded: JobMessage = deserialize_message(&bytes).unwrap();

        match decoded {
            JobMessage::Request(req_msg) => {
                assert_eq!(req_msg.requester_peer_id, "peer123");
            }
            _ => panic!("Wrong message type"),
        }
    }
}
