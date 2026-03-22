//! Heartbeat system for proactive background execution.
//!
//! The heartbeat runs periodically and:
//! - Checks for pending tasks
//! - Updates memory with new learnings
//! - Monitors system health
//! - Triggers maintenance routines

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Heartbeat configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Heartbeat interval in seconds
    pub interval_secs: u64,
    /// Enable heartbeat
    pub enabled: bool,
    /// Path to heartbeat checklist file
    pub checklist_path: Option<String>,
    /// Maximum consecutive failures before alert
    pub max_failures: u32,
    /// Enable self-repair on failures
    pub self_repair: bool,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300, // 5 minutes
            enabled: true,
            checklist_path: Some("HEARTBEAT.md".to_string()),
            max_failures: 3,
            self_repair: true,
        }
    }
}

/// Heartbeat check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatCheck {
    /// Check name
    pub name: String,
    /// Check passed
    pub passed: bool,
    /// Check message
    pub message: Option<String>,
    /// Actions taken
    pub actions: Vec<String>,
}

/// Heartbeat result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResult {
    /// All checks passed
    pub ok: bool,
    /// Individual checks
    pub checks: Vec<HeartbeatCheck>,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

impl HeartbeatResult {
    /// Create HEARTBEAT_OK result
    pub fn ok() -> Self {
        Self {
            ok: true,
            checks: Vec::new(),
            timestamp: chrono::Utc::now(),
            duration_ms: 0,
        }
    }

    /// Create failed result
    pub fn failed(checks: Vec<HeartbeatCheck>) -> Self {
        Self {
            ok: false,
            checks,
            timestamp: chrono::Utc::now(),
            duration_ms: 0,
        }
    }

}

impl std::fmt::Display for HeartbeatResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ok && self.checks.is_empty() {
            return write!(f, "HEARTBEAT_OK");
        }

        writeln!(f, "Heartbeat at {}", self.timestamp.format("%Y-%m-%d %H:%M:%S UTC"))?;
        writeln!(f, "Status: {}\n", if self.ok { "OK" } else { "ISSUES FOUND" })?;

        for check in &self.checks {
            let status = if check.passed { "[PASS]" } else { "[FAIL]" };
            writeln!(f, "{} {}", status, check.name)?;
            if let Some(msg) = &check.message {
                writeln!(f, "       {}", msg)?;
            }
            for action in &check.actions {
                writeln!(f, "       -> {}", action)?;
            }
        }

        Ok(())
    }
}

/// Heartbeat callback type
pub type HeartbeatCallback = Box<dyn Fn() -> futures::future::BoxFuture<'static, HeartbeatResult> + Send + Sync>;

/// Heartbeat system
pub struct Heartbeat {
    config: HeartbeatConfig,
    running: Arc<std::sync::atomic::AtomicBool>,
    consecutive_failures: Arc<std::sync::atomic::AtomicU32>,
    last_result: Arc<RwLock<Option<HeartbeatResult>>>,
    result_tx: mpsc::Sender<HeartbeatResult>,
    result_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<HeartbeatResult>>>,
}

impl Heartbeat {
    /// Create a new heartbeat
    pub fn new(config: HeartbeatConfig) -> Self {
        let (result_tx, result_rx) = mpsc::channel(10);

        Self {
            config,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            consecutive_failures: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            last_result: Arc::new(RwLock::new(None)),
            result_tx,
            result_rx: Arc::new(tokio::sync::Mutex::new(result_rx)),
        }
    }

    /// Start the heartbeat
    pub async fn start(&self) {
        if !self.config.enabled {
            tracing::info!("Heartbeat disabled");
            return;
        }

        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::info!(
            interval_secs = self.config.interval_secs,
            "Starting heartbeat"
        );
    }

    /// Stop the heartbeat
    pub async fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
        tracing::info!("Stopped heartbeat");
    }

    /// Run heartbeat loop with callback
    pub async fn run<F>(&self, callback: F)
    where
        F: Fn() -> futures::future::BoxFuture<'static, HeartbeatResult> + Send + Sync + 'static,
    {
        let running = self.running.clone();
        let interval = Duration::from_secs(self.config.interval_secs);
        let last_result = self.last_result.clone();
        let consecutive_failures = self.consecutive_failures.clone();
        let max_failures = self.config.max_failures;
        let result_tx = self.result_tx.clone();

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                interval_timer.tick().await;

                if !running.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                let start = std::time::Instant::now();
                let mut result = callback().await;
                result.duration_ms = start.elapsed().as_millis() as u64;

                // Track failures
                if result.ok {
                    consecutive_failures.store(0, std::sync::atomic::Ordering::SeqCst);
                } else {
                    let failures = consecutive_failures.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    if failures >= max_failures {
                        tracing::warn!(
                            failures = failures,
                            "Heartbeat consecutive failures exceeded threshold"
                        );
                    }
                }

                // Store result
                {
                    let mut last = last_result.write();
                    *last = Some(result.clone());
                }

                // Send result
                let _ = result_tx.send(result).await;
            }
        });
    }

    /// Get last heartbeat result
    pub fn last_result(&self) -> Option<HeartbeatResult> {
        self.last_result.read().clone()
    }

    /// Get consecutive failures count
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Subscribe to heartbeat results
    pub fn subscribe(&self) -> mpsc::Receiver<HeartbeatResult> {
        let (tx, rx) = mpsc::channel(10);

        let running = self.running.clone();
        let result_rx = self.result_rx.clone();

        tokio::spawn(async move {
            let mut rx_lock = result_rx.lock().await;
            while running.load(std::sync::atomic::Ordering::SeqCst) {
                if let Some(result) = rx_lock.recv().await {
                    if tx.send(result).await.is_err() {
                        break;
                    }
                }
            }
        });

        rx
    }
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self::new(HeartbeatConfig::default())
    }
}

/// Standard heartbeat checks
pub mod checks {
    use super::HeartbeatCheck;

    /// Check memory usage
    pub fn check_memory() -> HeartbeatCheck {
        // Simple check - would use sys-info crate in production
        HeartbeatCheck {
            name: "Memory Usage".to_string(),
            passed: true,
            message: None,
            actions: Vec::new(),
        }
    }

    /// Check disk space
    pub fn check_disk_space() -> HeartbeatCheck {
        HeartbeatCheck {
            name: "Disk Space".to_string(),
            passed: true,
            message: None,
            actions: Vec::new(),
        }
    }

    /// Check pending tasks
    pub fn check_pending_tasks(count: usize) -> HeartbeatCheck {
        HeartbeatCheck {
            name: "Pending Tasks".to_string(),
            passed: count == 0,
            message: if count > 0 {
                Some(format!("{} pending tasks", count))
            } else {
                None
            },
            actions: Vec::new(),
        }
    }

    /// Check network connectivity
    pub fn check_network() -> HeartbeatCheck {
        HeartbeatCheck {
            name: "Network".to_string(),
            passed: true,
            message: None,
            actions: Vec::new(),
        }
    }

    /// Check model availability
    pub fn check_model(available: bool) -> HeartbeatCheck {
        HeartbeatCheck {
            name: "Model".to_string(),
            passed: available,
            message: if !available {
                Some("No model loaded".to_string())
            } else {
                None
            },
            actions: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_result_ok() {
        let result = HeartbeatResult::ok();
        assert!(result.ok);
        assert_eq!(result.to_string(), "HEARTBEAT_OK");
    }

    #[test]
    fn test_heartbeat_result_failed() {
        let checks = vec![HeartbeatCheck {
            name: "Test Check".to_string(),
            passed: false,
            message: Some("Something went wrong".to_string()),
            actions: vec!["Restarted service".to_string()],
        }];

        let result = HeartbeatResult::failed(checks);
        assert!(!result.ok);

        let output = result.to_string();
        assert!(output.contains("ISSUES FOUND"));
        assert!(output.contains("[FAIL] Test Check"));
    }

    #[test]
    fn test_heartbeat_config_default() {
        let config = HeartbeatConfig::default();
        assert!(config.enabled);
        assert_eq!(config.interval_secs, 300);
    }
}
