//! Payment verification for HTTP 402 proxy.

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

/// Payment method for proxy requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentMethod {
    /// Direct per-request payment with signed proof
    Direct {
        /// Transaction or proof ID
        proof_id: String,
        /// Amount paid in μPCLAW
        amount: u64,
        /// Signature from payer
        signature: String,
    },
    /// Payment channel micropayment
    Channel {
        /// Channel ID
        channel_id: String,
        /// State nonce
        nonce: u64,
        /// Amount for this request
        amount: u64,
        /// Signed state update
        signature: String,
    },
    /// Prepaid balance on account
    Prepaid {
        /// Account identifier
        account_id: String,
        /// API key or auth token
        api_key: String,
    },
}

/// Payment proof extracted from request headers.
#[derive(Debug, Clone)]
pub struct PaymentProof {
    /// Payment method
    pub method: PaymentMethod,
    /// Timestamp of payment
    pub timestamp: u64,
}

impl PaymentProof {
    /// Extract payment proof from HTTP headers.
    pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
        // Check for different payment header formats

        // X-Payment-Proof: direct:<proof_id>:<amount>:<signature>
        if let Some(proof) = headers.get("X-Payment-Proof") {
            if let Ok(proof_str) = proof.to_str() {
                return Self::parse_proof(proof_str);
            }
        }

        // X-Channel-Payment: <channel_id>:<nonce>:<amount>:<signature>
        if let Some(channel_payment) = headers.get("X-Channel-Payment") {
            if let Ok(payment_str) = channel_payment.to_str() {
                return Self::parse_channel_payment(payment_str);
            }
        }

        // Authorization: Bearer <api_key> for prepaid accounts
        if let Some(auth) = headers.get("Authorization") {
            if let Ok(auth_str) = auth.to_str() {
                if auth_str.starts_with("Bearer ") {
                    let api_key = auth_str.trim_start_matches("Bearer ").to_string();
                    return Some(Self {
                        method: PaymentMethod::Prepaid {
                            account_id: String::new(), // Will be looked up
                            api_key,
                        },
                        timestamp: chrono::Utc::now().timestamp() as u64,
                    });
                }
            }
        }

        None
    }

    /// Parse a direct payment proof.
    fn parse_proof(proof: &str) -> Option<Self> {
        let parts: Vec<&str> = proof.split(':').collect();

        if parts.len() >= 4 && parts[0] == "direct" {
            let proof_id = parts[1].to_string();
            let amount = parts[2].parse().ok()?;
            let signature = parts[3].to_string();

            return Some(Self {
                method: PaymentMethod::Direct {
                    proof_id,
                    amount,
                    signature,
                },
                timestamp: chrono::Utc::now().timestamp() as u64,
            });
        }

        None
    }

    /// Parse a channel payment proof.
    fn parse_channel_payment(payment: &str) -> Option<Self> {
        let parts: Vec<&str> = payment.split(':').collect();

        if parts.len() >= 4 {
            let channel_id = parts[0].to_string();
            let nonce = parts[1].parse().ok()?;
            let amount = parts[2].parse().ok()?;
            let signature = parts[3].to_string();

            return Some(Self {
                method: PaymentMethod::Channel {
                    channel_id,
                    nonce,
                    amount,
                    signature,
                },
                timestamp: chrono::Utc::now().timestamp() as u64,
            });
        }

        None
    }

    /// Verify the payment proof against the required amount.
    pub fn verify(&self, required_amount: u64) -> Result<bool, String> {
        match &self.method {
            PaymentMethod::Direct { amount, signature, proof_id } => {
                // Check amount
                if *amount < required_amount {
                    return Ok(false);
                }

                // TODO: Verify signature against known public key
                // For now, just check that signature is present
                if signature.is_empty() {
                    return Err("Empty signature".into());
                }

                // TODO: Check proof_id hasn't been used before (replay protection)
                if proof_id.is_empty() {
                    return Err("Empty proof ID".into());
                }

                Ok(true)
            }

            PaymentMethod::Channel { amount, signature, channel_id, nonce: _ } => {
                // Check amount
                if *amount < required_amount {
                    return Ok(false);
                }

                // TODO: Verify channel exists and has balance
                // TODO: Verify signature is valid state update
                // TODO: Verify nonce is incremented

                if signature.is_empty() || channel_id.is_empty() {
                    return Err("Invalid channel payment".into());
                }

                Ok(true)
            }

            PaymentMethod::Prepaid { api_key, .. } => {
                // TODO: Look up account by API key
                // TODO: Check account has sufficient balance
                // TODO: Deduct from prepaid balance

                if api_key.is_empty() {
                    return Err("Empty API key".into());
                }

                // For now, accept any non-empty API key
                Ok(true)
            }
        }
    }

    /// Get the amount claimed in the payment.
    pub fn claimed_amount(&self) -> u64 {
        match &self.method {
            PaymentMethod::Direct { amount, .. } => *amount,
            PaymentMethod::Channel { amount, .. } => *amount,
            PaymentMethod::Prepaid { .. } => u64::MAX, // Prepaid has unlimited per-request
        }
    }
}

/// Response with payment information for 402 responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct PaymentInfo {
    /// Required amount in μPCLAW
    pub required_amount: u64,
    /// Required amount in PCLAW (for display)
    pub required_amount_pclaw: f64,
    /// Wallet address to send payment
    pub payment_address: String,
    /// Supported payment methods
    pub supported_methods: Vec<String>,
    /// Instructions for payment
    pub instructions: String,
}

#[allow(dead_code)]
impl PaymentInfo {
    /// Create payment info for a 402 response.
    pub fn new(required_amount: u64, payment_address: String) -> Self {
        Self {
            required_amount,
            required_amount_pclaw: crate::wallet::from_micro(required_amount),
            payment_address,
            supported_methods: vec![
                "direct".to_string(),
                "channel".to_string(),
                "prepaid".to_string(),
            ],
            instructions: "Include payment proof in request headers:\n\
                 - X-Payment-Proof: direct:<proof_id>:<amount>:<signature>\n\
                 - X-Channel-Payment: <channel_id>:<nonce>:<amount>:<signature>\n\
                 - Authorization: Bearer <api_key>".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_parse_direct_payment() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Payment-Proof",
            HeaderValue::from_static("direct:proof123:1000000:sig456"),
        );

        let proof = PaymentProof::from_headers(&headers).unwrap();

        match proof.method {
            PaymentMethod::Direct { proof_id, amount, signature } => {
                assert_eq!(proof_id, "proof123");
                assert_eq!(amount, 1000000);
                assert_eq!(signature, "sig456");
            }
            _ => panic!("Expected Direct payment method"),
        }
    }

    #[test]
    fn test_parse_channel_payment() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Channel-Payment",
            HeaderValue::from_static("chan_123:5:500000:sig789"),
        );

        let proof = PaymentProof::from_headers(&headers).unwrap();

        match proof.method {
            PaymentMethod::Channel { channel_id, nonce, amount, signature } => {
                assert_eq!(channel_id, "chan_123");
                assert_eq!(nonce, 5);
                assert_eq!(amount, 500000);
                assert_eq!(signature, "sig789");
            }
            _ => panic!("Expected Channel payment method"),
        }
    }

    #[test]
    fn test_parse_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("Bearer sk_test_12345"),
        );

        let proof = PaymentProof::from_headers(&headers).unwrap();

        match proof.method {
            PaymentMethod::Prepaid { api_key, .. } => {
                assert_eq!(api_key, "sk_test_12345");
            }
            _ => panic!("Expected Prepaid payment method"),
        }
    }

    #[test]
    fn test_verify_sufficient_payment() {
        let proof = PaymentProof {
            method: PaymentMethod::Direct {
                proof_id: "test".to_string(),
                amount: 1000000,
                signature: "valid".to_string(),
            },
            timestamp: 0,
        };

        // Amount is sufficient
        assert!(proof.verify(500000).unwrap());

        // Amount is exactly right
        assert!(proof.verify(1000000).unwrap());

        // Amount is insufficient
        assert!(!proof.verify(2000000).unwrap());
    }
}
