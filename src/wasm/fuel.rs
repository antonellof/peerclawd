//! Fuel metering for WASM execution limits.
//!
//! Fuel is a measure of execution "cost" - each WASM instruction consumes
//! some amount of fuel. When fuel runs out, execution stops.

use std::time::Duration;

/// Fuel configuration.
#[derive(Debug, Clone)]
pub struct FuelConfig {
    /// Base fuel limit
    pub base_limit: u64,
    /// Fuel per second (for timeout-based limits)
    pub fuel_per_second: u64,
    /// Maximum fuel regardless of timeout
    pub max_fuel: u64,
}

impl Default for FuelConfig {
    fn default() -> Self {
        Self {
            base_limit: 10_000_000,      // 10M base
            fuel_per_second: 50_000_000, // 50M per second
            max_fuel: 1_000_000_000,     // 1B max
        }
    }
}

impl FuelConfig {
    /// Calculate fuel limit for a given timeout.
    pub fn fuel_for_timeout(&self, timeout: Duration) -> u64 {
        let timeout_fuel = timeout.as_secs() * self.fuel_per_second;
        (self.base_limit + timeout_fuel).min(self.max_fuel)
    }
}

/// Fuel meter for tracking execution costs.
pub struct FuelMeter {
    /// Total fuel allocated
    allocated: u64,
    /// Fuel consumed so far
    consumed: u64,
    /// Configuration
    config: FuelConfig,
}

impl FuelMeter {
    /// Create a new fuel meter.
    pub fn new(config: FuelConfig, timeout: Duration) -> Self {
        let allocated = config.fuel_for_timeout(timeout);
        Self {
            allocated,
            consumed: 0,
            config,
        }
    }

    /// Get remaining fuel.
    pub fn remaining(&self) -> u64 {
        self.allocated.saturating_sub(self.consumed)
    }

    /// Get consumed fuel.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Get allocated fuel.
    pub fn allocated(&self) -> u64 {
        self.allocated
    }

    /// Record fuel consumption.
    pub fn consume(&mut self, amount: u64) -> bool {
        if self.remaining() >= amount {
            self.consumed += amount;
            true
        } else {
            false
        }
    }

    /// Check if fuel is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining() == 0
    }

    /// Get usage ratio (0.0 - 1.0).
    pub fn usage_ratio(&self) -> f64 {
        self.consumed as f64 / self.allocated.max(1) as f64
    }

    /// Estimate cost based on fuel consumed and a rate.
    /// Returns cost in μPCLAW.
    pub fn estimate_cost(&self, fuel_per_micro_pclaw: u64) -> u64 {
        self.consumed / fuel_per_micro_pclaw.max(1)
    }
}

/// Predefined fuel costs for common operations.
pub mod costs {
    /// Cost for a simple instruction (add, sub, etc.)
    pub const SIMPLE_INSTRUCTION: u64 = 1;

    /// Cost for memory access
    pub const MEMORY_ACCESS: u64 = 2;

    /// Cost for function call
    pub const FUNCTION_CALL: u64 = 10;

    /// Cost for memory allocation (per KB)
    pub const MEMORY_ALLOC_PER_KB: u64 = 100;

    /// Cost for host function call (logging, etc.)
    pub const HOST_CALL_BASE: u64 = 1000;

    /// Cost for network request (if allowed)
    pub const NETWORK_REQUEST: u64 = 10_000;

    /// Cost for filesystem operation (if allowed)
    pub const FS_OPERATION: u64 = 5_000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuel_config_timeout() {
        let config = FuelConfig::default();
        let fuel = config.fuel_for_timeout(Duration::from_secs(10));

        // base + 10 * per_second
        let expected = 10_000_000 + 10 * 50_000_000;
        assert_eq!(fuel, expected);
    }

    #[test]
    fn test_fuel_config_max_cap() {
        let config = FuelConfig::default();
        let fuel = config.fuel_for_timeout(Duration::from_secs(1000));

        // Should be capped at max_fuel
        assert_eq!(fuel, 1_000_000_000);
    }

    #[test]
    fn test_fuel_meter_consumption() {
        let config = FuelConfig::default();
        let mut meter = FuelMeter::new(config, Duration::from_secs(1));

        assert!(meter.consume(1_000_000));
        assert_eq!(meter.consumed(), 1_000_000);
        assert!(meter.remaining() > 0);
    }

    #[test]
    fn test_fuel_meter_exhaustion() {
        let config = FuelConfig {
            base_limit: 100,
            fuel_per_second: 0,
            max_fuel: 100,
        };
        let mut meter = FuelMeter::new(config, Duration::from_secs(1));

        assert!(meter.consume(100));
        assert!(meter.is_exhausted());
        assert!(!meter.consume(1)); // Should fail
    }
}
