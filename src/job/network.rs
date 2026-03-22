//! P2P network integration for job broadcasting.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
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
    /// Ed25519 signature over the canonical message bytes
    pub signature: Vec<u8>,
}

impl JobRequestMessage {
    /// Create a new unsigned request message.
    pub fn new(request: JobRequest, peer_id: &PeerId) -> Self {
        Self {
            request,
            requester_peer_id: peer_id.to_string(),
            signature: vec![],
        }
    }

    /// Create a signed request message.
    pub fn new_signed(request: JobRequest, peer_id: &PeerId, signing_key: &SigningKey) -> Self {
        let mut msg = Self::new(request, peer_id);
        msg.sign(signing_key);
        msg
    }

    /// Sign this message with the given key.
    pub fn sign(&mut self, signing_key: &SigningKey) {
        let payload = self.signable_bytes();
        let sig: Signature = signing_key.sign(&payload);
        self.signature = sig.to_bytes().to_vec();
    }

    /// Verify the signature against a verifying key.
    pub fn verify(&self, verifying_key: &VerifyingKey) -> bool {
        if self.signature.len() != 64 {
            return false;
        }
        let Ok(sig) = Signature::from_slice(&self.signature) else {
            return false;
        };
        let payload = self.signable_bytes();
        verifying_key.verify(&payload, &sig).is_ok()
    }

    /// Canonical bytes used for signing (excludes the signature field).
    fn signable_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"job_request:");
        buf.extend_from_slice(self.requester_peer_id.as_bytes());
        buf.extend_from_slice(b":");
        buf.extend_from_slice(self.request.id.0.as_bytes());
        buf
    }
}

/// Bid message sent to the requester.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobBidMessage {
    /// The bid
    pub bid: JobBid,
    /// Bidder's peer ID
    pub bidder_peer_id: String,
    /// Ed25519 signature over the canonical message bytes
    pub signature: Vec<u8>,
}

impl JobBidMessage {
    /// Create a new unsigned bid message.
    pub fn new(bid: JobBid, peer_id: &PeerId) -> Self {
        Self {
            bid,
            bidder_peer_id: peer_id.to_string(),
            signature: vec![],
        }
    }

    /// Create a signed bid message.
    pub fn new_signed(bid: JobBid, peer_id: &PeerId, signing_key: &SigningKey) -> Self {
        let mut msg = Self::new(bid, peer_id);
        msg.sign(signing_key);
        msg
    }

    /// Sign this message.
    pub fn sign(&mut self, signing_key: &SigningKey) {
        let payload = self.signable_bytes();
        let sig: Signature = signing_key.sign(&payload);
        self.signature = sig.to_bytes().to_vec();
    }

    /// Verify the signature.
    pub fn verify(&self, verifying_key: &VerifyingKey) -> bool {
        if self.signature.len() != 64 {
            return false;
        }
        let Ok(sig) = Signature::from_slice(&self.signature) else {
            return false;
        };
        let payload = self.signable_bytes();
        verifying_key.verify(&payload, &sig).is_ok()
    }

    fn signable_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"job_bid:");
        buf.extend_from_slice(self.bidder_peer_id.as_bytes());
        buf.extend_from_slice(b":");
        buf.extend_from_slice(self.bid.job_id.0.as_bytes());
        buf
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
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

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

    #[test]
    fn test_request_sign_verify() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let request = JobRequest::new(
            ResourceType::Inference {
                model: "llama-7b".into(),
                tokens: 1000,
            },
            to_micro(5.0),
            300,
        );

        let peer_id = libp2p::PeerId::random();
        let msg = JobRequestMessage::new_signed(request, &peer_id, &signing_key);

        assert!(!msg.signature.is_empty());
        assert_eq!(msg.signature.len(), 64);
        assert!(msg.verify(&verifying_key));

        // Verify fails with wrong key
        let wrong_key = SigningKey::generate(&mut OsRng);
        assert!(!msg.verify(&wrong_key.verifying_key()));
    }

    #[test]
    fn test_bid_sign_verify() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let bid = JobBid::new(
            crate::job::JobId("job-123".to_string()),
            "bidder-peer".to_string(),
            to_micro(3.0),
            120,
            300,
        );

        let peer_id = libp2p::PeerId::random();
        let msg = JobBidMessage::new_signed(bid, &peer_id, &signing_key);

        assert!(msg.verify(&verifying_key));

        // Tampered message fails verification
        let mut tampered = msg.clone();
        tampered.bidder_peer_id = "attacker".to_string();
        assert!(!tampered.verify(&verifying_key));
    }
}
