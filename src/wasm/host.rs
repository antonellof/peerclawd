//! Host functions exposed to WASM modules.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Capabilities that can be granted to WASM modules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostCapabilities {
    /// Allow logging to host
    pub logging: bool,
    /// Allow network access
    pub network_access: bool,
    /// Allow filesystem read
    pub filesystem_read: bool,
    /// Allow filesystem write
    pub filesystem_write: bool,
    /// Allow environment variable access
    pub env_access: bool,
    /// Allow clock/time access
    pub clock_access: bool,
    /// Allow random number generation
    pub random_access: bool,
    /// Allowed network hosts (only if network_access is true)
    pub allowed_hosts: HashSet<String>,
    /// Allowed filesystem paths (only if filesystem access is granted)
    pub allowed_paths: HashSet<String>,
}

impl HostCapabilities {
    /// Create capabilities with all features enabled (unsafe for untrusted code).
    pub fn all() -> Self {
        Self {
            logging: true,
            network_access: true,
            filesystem_read: true,
            filesystem_write: true,
            env_access: true,
            clock_access: true,
            random_access: true,
            allowed_hosts: HashSet::new(),
            allowed_paths: HashSet::new(),
        }
    }

    /// Create minimal safe capabilities.
    pub fn minimal() -> Self {
        Self {
            logging: true,
            clock_access: true,
            random_access: true,
            ..Default::default()
        }
    }

    /// Check if these capabilities satisfy the required capabilities.
    pub fn satisfies(&self, required: &HostCapabilities) -> bool {
        // Check each capability
        if required.network_access && !self.network_access {
            return false;
        }
        if required.filesystem_read && !self.filesystem_read {
            return false;
        }
        if required.filesystem_write && !self.filesystem_write {
            return false;
        }
        if required.env_access && !self.env_access {
            return false;
        }

        // Check allowed hosts
        for host in &required.allowed_hosts {
            if !self.allowed_hosts.contains(host) && !self.allowed_hosts.is_empty() {
                // If we have restrictions and the required host isn't in our list
                return false;
            }
        }

        true
    }

    /// Add an allowed host.
    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        self.allowed_hosts.insert(host.into());
        self
    }

    /// Add an allowed path.
    pub fn allow_path(mut self, path: impl Into<String>) -> Self {
        self.allowed_paths.insert(path.into());
        self
    }

    /// Enable network access.
    pub fn with_network(mut self) -> Self {
        self.network_access = true;
        self
    }

    /// Enable filesystem read.
    pub fn with_fs_read(mut self) -> Self {
        self.filesystem_read = true;
        self
    }

    /// Enable filesystem write.
    pub fn with_fs_write(mut self) -> Self {
        self.filesystem_write = true;
        self
    }
}

/// State passed to host functions.
pub struct HostState {
    /// Granted capabilities
    pub capabilities: HostCapabilities,
    /// Log buffer
    pub logs: Vec<LogEntry>,
    /// Current memory usage
    pub memory_used: u64,
    /// Network requests made
    pub network_requests: u32,
    /// Filesystem operations
    pub fs_operations: u32,
}

impl HostState {
    /// Create new host state with given capabilities.
    pub fn new(capabilities: HostCapabilities) -> Self {
        Self {
            capabilities,
            logs: Vec::new(),
            memory_used: 0,
            network_requests: 0,
            fs_operations: 0,
        }
    }

    /// Log a message.
    pub fn log(&mut self, level: LogLevel, message: String) {
        if self.capabilities.logging {
            self.logs.push(LogEntry {
                level,
                message,
                timestamp: std::time::SystemTime::now(),
            });
        }
    }

    /// Check if network access is allowed for a host.
    pub fn can_access_host(&self, host: &str) -> bool {
        if !self.capabilities.network_access {
            return false;
        }

        if self.capabilities.allowed_hosts.is_empty() {
            // No restrictions
            return true;
        }

        // Check exact match or wildcard
        self.capabilities.allowed_hosts.contains(host)
            || self.capabilities.allowed_hosts.iter().any(|pattern| {
                if pattern.starts_with("*.") {
                    host.ends_with(&pattern[1..])
                } else {
                    pattern == host
                }
            })
    }

    /// Check if filesystem access is allowed for a path.
    pub fn can_access_path(&self, path: &str, write: bool) -> bool {
        if write && !self.capabilities.filesystem_write {
            return false;
        }
        if !write && !self.capabilities.filesystem_read {
            return false;
        }

        if self.capabilities.allowed_paths.is_empty() {
            return true;
        }

        self.capabilities.allowed_paths.iter().any(|allowed| {
            path.starts_with(allowed)
        })
    }
}

/// Log entry from WASM execution.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp: std::time::SystemTime,
}

/// Log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_satisfies() {
        let granted = HostCapabilities::minimal().with_network();
        let required = HostCapabilities {
            network_access: true,
            ..Default::default()
        };

        assert!(granted.satisfies(&required));
    }

    #[test]
    fn test_capabilities_not_satisfies() {
        let granted = HostCapabilities::minimal();
        let required = HostCapabilities {
            network_access: true,
            ..Default::default()
        };

        assert!(!granted.satisfies(&required));
    }

    #[test]
    fn test_host_access_check() {
        let caps = HostCapabilities::minimal()
            .with_network()
            .allow_host("*.example.com")
            .allow_host("api.test.com");

        let state = HostState::new(caps);

        assert!(state.can_access_host("api.test.com"));
        assert!(state.can_access_host("sub.example.com"));
        assert!(!state.can_access_host("other.com"));
    }
}
