//! Node identity - Ed25519 keypair management.

use ed25519_dalek::{SecretKey, SigningKey, VerifyingKey, Signature, Signer, Verifier};
use libp2p::identity::{Keypair as Libp2pKeypair, ed25519};
use libp2p::PeerId;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IdentityError {
    #[error("Invalid key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),

    #[error("Failed to read key file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Invalid key format: {0}")]
    InvalidFormat(String),

    #[error("Signature verification failed")]
    VerificationFailed,
}

/// Node identity backed by Ed25519 keypair.
///
/// Maps 1:1 with a libp2p PeerId for network identification.
pub struct NodeIdentity {
    signing_key: SigningKey,
    peer_id: PeerId,
}

impl NodeIdentity {
    /// Generate a new random identity.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let peer_id = Self::derive_peer_id(&signing_key);
        Self { signing_key, peer_id }
    }

    /// Create identity from raw secret key bytes.
    pub fn from_bytes(secret: &[u8; 32]) -> Result<Self, IdentityError> {
        let signing_key = SigningKey::from_bytes(secret);
        let peer_id = Self::derive_peer_id(&signing_key);
        Ok(Self { signing_key, peer_id })
    }

    /// Load identity from a file.
    pub fn load(path: &Path) -> Result<Self, IdentityError> {
        let bytes = std::fs::read(path)?;
        if bytes.len() != 32 {
            return Err(IdentityError::InvalidKeyLength(bytes.len()));
        }
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&bytes);
        Self::from_bytes(&secret)
    }

    /// Save identity to a file.
    pub fn save(&self, path: &Path) -> Result<(), IdentityError> {
        std::fs::write(path, self.signing_key.to_bytes())?;
        Ok(())
    }

    /// Get the libp2p PeerId.
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Get the public key.
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Get the public key bytes.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    /// Verify a signature.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.signing_key.verifying_key().verify(message, signature).is_ok()
    }

    /// Convert to a libp2p Keypair.
    pub fn to_libp2p_keypair(&self) -> Libp2pKeypair {
        // libp2p expects a SecretKey which is just the 32-byte seed
        let secret = ed25519::SecretKey::try_from_bytes(self.signing_key.to_bytes())
            .expect("valid ed25519 secret key");
        let ed25519_keypair = ed25519::Keypair::from(secret);
        Libp2pKeypair::from(ed25519_keypair)
    }

    /// Derive PeerId from signing key.
    fn derive_peer_id(signing_key: &SigningKey) -> PeerId {
        let secret = ed25519::SecretKey::try_from_bytes(signing_key.to_bytes())
            .expect("valid ed25519 secret key");
        let ed25519_keypair = ed25519::Keypair::from(secret);
        let libp2p_keypair = Libp2pKeypair::from(ed25519_keypair);
        PeerId::from(libp2p_keypair.public())
    }
}

impl std::fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("peer_id", &self.peer_id.to_string())
            .finish()
    }
}

impl Clone for NodeIdentity {
    fn clone(&self) -> Self {
        // Clone by reconstructing from the signing key bytes
        let signing_key = SigningKey::from_bytes(&self.signing_key.to_bytes());
        Self {
            signing_key,
            peer_id: self.peer_id,
        }
    }
}

/// Serializable identity info for storage and display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityInfo {
    pub peer_id: String,
    pub public_key: String,
}

impl From<&NodeIdentity> for IdentityInfo {
    fn from(identity: &NodeIdentity) -> Self {
        Self {
            peer_id: identity.peer_id().to_string(),
            public_key: hex::encode(identity.public_key_bytes()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_and_sign() {
        let identity = NodeIdentity::generate();
        let message = b"hello world";
        let signature = identity.sign(message);
        assert!(identity.verify(message, &signature));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("identity.key");

        let identity = NodeIdentity::generate();
        identity.save(&path).unwrap();

        let loaded = NodeIdentity::load(&path).unwrap();
        assert_eq!(identity.peer_id(), loaded.peer_id());
    }

    #[test]
    fn test_peer_id_derivation() {
        let identity = NodeIdentity::generate();
        let keypair = identity.to_libp2p_keypair();
        let derived_peer_id = PeerId::from(keypair.public());
        assert_eq!(identity.peer_id(), &derived_peer_id);
    }
}
