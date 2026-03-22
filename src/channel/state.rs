//! Channel state and signed updates.

use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};

use super::ChannelId;
use crate::identity::NodeIdentity;

/// A channel state update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelUpdate {
    /// Channel this update applies to
    pub channel_id: ChannelId,
    /// Monotonically increasing nonce
    pub nonce: u64,
    /// Local peer's new balance
    pub local_balance: u64,
    /// Remote peer's new balance
    pub remote_balance: u64,
    /// When this update was created
    pub timestamp: DateTime<Utc>,
}

impl ChannelUpdate {
    /// Create a new channel update.
    pub fn new(
        channel_id: ChannelId,
        nonce: u64,
        local_balance: u64,
        remote_balance: u64,
    ) -> Self {
        Self {
            channel_id,
            nonce,
            local_balance,
            remote_balance,
            timestamp: Utc::now(),
        }
    }

    /// Serialize the update for signing.
    pub fn to_signing_bytes(&self) -> Vec<u8> {
        // Create a canonical representation for signing
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.channel_id.0.as_bytes());
        bytes.extend_from_slice(&self.nonce.to_le_bytes());
        bytes.extend_from_slice(&self.local_balance.to_le_bytes());
        bytes.extend_from_slice(&self.remote_balance.to_le_bytes());
        bytes.extend_from_slice(&self.timestamp.timestamp().to_le_bytes());
        bytes
    }

    /// Sign this update with the given identity.
    pub fn sign(&self, identity: &NodeIdentity) -> SignedUpdate {
        let bytes = self.to_signing_bytes();
        let signature = identity.sign(&bytes);

        SignedUpdate {
            update: self.clone(),
            signature: hex::encode(signature.to_bytes()),
            signer: identity.peer_id().to_string(),
        }
    }
}

/// A signed channel update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedUpdate {
    /// The update being signed
    pub update: ChannelUpdate,
    /// Hex-encoded Ed25519 signature
    pub signature: String,
    /// Peer ID of the signer
    pub signer: String,
}

impl SignedUpdate {
    /// Verify the signature against a peer's public key.
    pub fn verify(&self, verifier: &NodeIdentity) -> bool {
        let bytes = self.update.to_signing_bytes();

        // Decode signature
        let sig_bytes = match hex::decode(&self.signature) {
            Ok(b) if b.len() == 64 => b,
            _ => return false,
        };

        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&sig_bytes);

        let sig = Signature::from_bytes(&sig_array);
        verifier.verify(&bytes, &sig)
    }
}

/// Complete state of a channel for settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelState {
    /// Channel ID
    pub channel_id: ChannelId,
    /// Opening transaction reference (for on-chain anchoring)
    pub opening_tx: Option<String>,
    /// Latest update signed by local peer
    pub local_signed: Option<SignedUpdate>,
    /// Latest update signed by remote peer
    pub remote_signed: Option<SignedUpdate>,
    /// Whether the channel has been disputed
    pub disputed: bool,
    /// Closing transaction reference
    pub closing_tx: Option<String>,
}

impl ChannelState {
    /// Create a new channel state.
    pub fn new(channel_id: ChannelId) -> Self {
        Self {
            channel_id,
            opening_tx: None,
            local_signed: None,
            remote_signed: None,
            disputed: false,
            closing_tx: None,
        }
    }

    /// Get the latest agreed-upon state (both parties signed).
    pub fn latest_agreed(&self) -> Option<&ChannelUpdate> {
        match (&self.local_signed, &self.remote_signed) {
            (Some(local), Some(remote)) => {
                // Return the one with higher nonce
                if local.update.nonce >= remote.update.nonce {
                    Some(&local.update)
                } else {
                    Some(&remote.update)
                }
            }
            (Some(local), None) => Some(&local.update),
            (None, Some(remote)) => Some(&remote.update),
            (None, None) => None,
        }
    }

    /// Check if channel can be settled.
    pub fn can_settle(&self) -> bool {
        // Need at least one signed state
        self.local_signed.is_some() || self.remote_signed.is_some()
    }

    /// Update with a new local signature.
    pub fn update_local(&mut self, signed: SignedUpdate) {
        self.local_signed = Some(signed);
    }

    /// Update with a new remote signature.
    pub fn update_remote(&mut self, signed: SignedUpdate) {
        self.remote_signed = Some(signed);
    }

    /// Mark channel as disputed.
    pub fn dispute(&mut self) {
        self.disputed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_update_signing() {
        let identity = NodeIdentity::generate();

        let update = ChannelUpdate::new(
            ChannelId::new(),
            1,
            90_000_000,
            10_000_000,
        );

        let signed = update.sign(&identity);

        // Verify with same identity
        assert!(signed.verify(&identity));
    }

    #[test]
    fn test_channel_update_tamper_detection() {
        let identity = NodeIdentity::generate();

        let update = ChannelUpdate::new(
            ChannelId::new(),
            1,
            90_000_000,
            10_000_000,
        );

        let mut signed = update.sign(&identity);

        // Tamper with the update
        signed.update.local_balance = 100_000_000;

        // Verification should fail
        assert!(!signed.verify(&identity));
    }

    #[test]
    fn test_channel_state() {
        let identity = NodeIdentity::generate();

        let channel_id = ChannelId::new();
        let mut state = ChannelState::new(channel_id.clone());

        assert!(state.latest_agreed().is_none());
        assert!(!state.can_settle());

        // Add a local signed update
        let update = ChannelUpdate::new(channel_id, 1, 90_000_000, 10_000_000);
        let signed = update.sign(&identity);
        state.update_local(signed);

        assert!(state.latest_agreed().is_some());
        assert!(state.can_settle());
    }
}
