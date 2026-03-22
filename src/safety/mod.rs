//! Safety module for credential leak detection, prompt injection defense,
//! and content sanitization.
//!
//! Defense-in-depth security for AI agent operations:
//! - Inbound: Scan user input for accidental credential exposure
//! - Tool output: Detect leaks and injection attempts before LLM sees them
//! - Outbound: Validate responses before returning to user

pub mod leak_detector;
pub mod sanitizer;
pub mod policy;
pub mod validator;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use leak_detector::{LeakDetector, LeakMatch, SecretPattern};
pub use sanitizer::{Sanitizer, SanitizedOutput, SanitizeAction};
pub use policy::{Policy, PolicyRule, PolicyAction, PolicyViolation};
pub use validator::{InputValidator, ValidationResult};

/// Safety layer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    /// Enable leak detection
    pub leak_detection_enabled: bool,
    /// Enable prompt injection defense
    pub injection_defense_enabled: bool,
    /// Enable content policy enforcement
    pub policy_enabled: bool,
    /// Maximum output length (bytes)
    pub max_output_length: usize,
    /// Redaction string for detected secrets
    pub redaction_string: String,
    /// Tool output timeout
    pub tool_timeout: Duration,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            leak_detection_enabled: true,
            injection_defense_enabled: true,
            policy_enabled: true,
            max_output_length: 100_000,
            redaction_string: "[REDACTED]".to_string(),
            tool_timeout: Duration::from_secs(30),
        }
    }
}

/// Safety layer error
#[derive(Debug, Error)]
pub enum SafetyError {
    #[error("Credential leak detected: {pattern}")]
    LeakDetected { pattern: String },

    #[error("Prompt injection detected: {reason}")]
    InjectionDetected { reason: String },

    #[error("Policy violation: {rule}")]
    PolicyViolation { rule: String },

    #[error("Content too large: {size} bytes (max {max})")]
    ContentTooLarge { size: usize, max: usize },

    #[error("Validation failed: {reason}")]
    ValidationFailed { reason: String },
}

pub type Result<T> = std::result::Result<T, SafetyError>;

/// Main safety layer combining all defense mechanisms
#[allow(dead_code)]
pub struct SafetyLayer {
    config: SafetyConfig,
    leak_detector: LeakDetector,
    sanitizer: Sanitizer,
    policy: Policy,
    validator: InputValidator,
}

impl SafetyLayer {
    /// Create a new safety layer with default configuration
    pub fn new() -> Self {
        Self::with_config(SafetyConfig::default())
    }

    /// Create a safety layer with custom configuration
    pub fn with_config(config: SafetyConfig) -> Self {
        Self {
            leak_detector: LeakDetector::new(),
            sanitizer: Sanitizer::new(config.injection_defense_enabled),
            policy: Policy::default(),
            validator: InputValidator::new(),
            config,
        }
    }

    /// Scan inbound message for accidental credential exposure
    pub fn scan_inbound(&self, content: &str) -> Result<()> {
        if !self.config.leak_detection_enabled {
            return Ok(());
        }

        let matches = self.leak_detector.scan(content);
        if !matches.is_empty() {
            return Err(SafetyError::LeakDetected {
                pattern: matches[0].pattern_name.clone(),
            });
        }

        Ok(())
    }

    /// Sanitize tool output before it reaches the LLM
    pub fn sanitize_tool_output(&self, tool_name: &str, output: &str) -> SanitizedOutput {
        let mut content = output.to_string();

        // 1. Check length
        if content.len() > self.config.max_output_length {
            let truncated = floor_char_boundary(&content, self.config.max_output_length);
            return SanitizedOutput {
                content: format!("{}...[truncated]", truncated),
                action: SanitizeAction::Truncated,
                warnings: vec![format!(
                    "Output truncated from {} to {} bytes",
                    content.len(),
                    self.config.max_output_length
                )],
            };
        }

        // 2. Leak detection and redaction
        if self.config.leak_detection_enabled {
            match self.leak_detector.scan_and_clean(&content, &self.config.redaction_string) {
                Ok((cleaned, redacted_count)) => {
                    if redacted_count > 0 {
                        content = cleaned;
                        tracing::warn!(
                            tool = tool_name,
                            redacted = redacted_count,
                            "Redacted potential secrets from tool output"
                        );
                    }
                }
                Err(e) => {
                    return SanitizedOutput {
                        content: format!("[Output blocked: {}]", e),
                        action: SanitizeAction::Blocked,
                        warnings: vec![e.to_string()],
                    };
                }
            }
        }

        // 3. Policy enforcement
        if self.config.policy_enabled {
            let violations = self.policy.check(&content);
            let blocked = violations.iter().any(|v| v.action == PolicyAction::Block);

            if blocked {
                return SanitizedOutput {
                    content: "[Output blocked by policy]".to_string(),
                    action: SanitizeAction::Blocked,
                    warnings: violations.iter().map(|v| v.rule.clone()).collect(),
                };
            }
        }

        // 4. Injection defense
        if self.config.injection_defense_enabled {
            let sanitized = self.sanitizer.sanitize(&content);
            if !sanitized.warnings.is_empty() {
                tracing::warn!(
                    tool = tool_name,
                    warnings = ?sanitized.warnings,
                    "Potential injection patterns detected"
                );
            }
            return sanitized;
        }

        SanitizedOutput {
            content,
            action: SanitizeAction::Allowed,
            warnings: vec![],
        }
    }

    /// Validate outbound response before returning to user
    pub fn validate_outbound(&self, content: &str) -> Result<()> {
        // Final leak check
        if self.config.leak_detection_enabled {
            let matches = self.leak_detector.scan(content);
            if !matches.is_empty() {
                return Err(SafetyError::LeakDetected {
                    pattern: matches[0].pattern_name.clone(),
                });
            }
        }

        Ok(())
    }

    /// Get the leak detector for direct access
    pub fn leak_detector(&self) -> &LeakDetector {
        &self.leak_detector
    }

    /// Get the sanitizer for direct access
    pub fn sanitizer(&self) -> &Sanitizer {
        &self.sanitizer
    }

    /// Get the policy for direct access
    pub fn policy(&self) -> &Policy {
        &self.policy
    }
}

impl Default for SafetyLayer {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the largest valid char boundary at or before the given index
fn floor_char_boundary(s: &str, index: usize) -> &str {
    if index >= s.len() {
        return s;
    }

    let mut idx = index;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_layer_creation() {
        let layer = SafetyLayer::new();
        assert!(layer.config.leak_detection_enabled);
        assert!(layer.config.injection_defense_enabled);
    }

    #[test]
    fn test_inbound_leak_detection() {
        let layer = SafetyLayer::new();

        // Should detect AWS key
        let result = layer.scan_inbound("My key is AKIAIOSFODNN7EXAMPLE");
        assert!(result.is_err());

        // Should allow normal content
        let result = layer.scan_inbound("Hello, how are you?");
        assert!(result.is_ok());
    }

    #[test]
    fn test_tool_output_sanitization() {
        let layer = SafetyLayer::new();

        // Normal output
        let result = layer.sanitize_tool_output("test", "Hello world");
        assert_eq!(result.action, SanitizeAction::Allowed);

        // Output with secret
        let result = layer.sanitize_tool_output(
            "test",
            "API key: sk-proj-abcdefghijklmnopqrstuvwxyz123456"
        );
        assert!(result.content.contains("[REDACTED]"));
    }

    #[test]
    fn test_truncation() {
        let config = SafetyConfig {
            max_output_length: 100,
            ..Default::default()
        };
        let layer = SafetyLayer::with_config(config);

        let long_output = "a".repeat(200);
        let result = layer.sanitize_tool_output("test", &long_output);
        assert_eq!(result.action, SanitizeAction::Truncated);
        assert!(result.content.len() < 200);
    }
}
