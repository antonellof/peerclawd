//! Balance types for the wallet.

use serde::{Deserialize, Serialize};
use std::fmt;

use super::{from_micro, MICRO_PCLAW};

/// A balance amount in μPCLAW (micro-PCLAW).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Balance(pub u64);

impl Balance {
    /// Create a new balance from μPCLAW.
    pub fn from_micro(amount: u64) -> Self {
        Self(amount)
    }

    /// Create a new balance from PCLAW (float).
    pub fn from_pclaw(amount: f64) -> Self {
        Self((amount * MICRO_PCLAW as f64) as u64)
    }

    /// Get the balance in μPCLAW.
    pub fn as_micro(&self) -> u64 {
        self.0
    }

    /// Get the balance in PCLAW (float).
    pub fn as_pclaw(&self) -> f64 {
        from_micro(self.0)
    }

    /// Check if the balance is zero.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Saturating addition.
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating subtraction.
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for Balance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.6} PCLAW", self.as_pclaw())
    }
}

impl From<u64> for Balance {
    fn from(amount: u64) -> Self {
        Self(amount)
    }
}

impl From<Balance> for u64 {
    fn from(balance: Balance) -> Self {
        balance.0
    }
}

impl std::ops::Add for Balance {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl std::ops::Sub for Balance {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl std::ops::AddAssign for Balance {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl std::ops::SubAssign for Balance {
    fn sub_assign(&mut self, other: Self) {
        self.0 -= other.0;
    }
}

/// A snapshot of the wallet balance at a point in time.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BalanceSnapshot {
    /// Available balance that can be spent immediately (in μPCLAW).
    pub available: u64,
    /// Amount currently held in escrow for pending jobs (in μPCLAW).
    pub in_escrow: u64,
    /// Amount staked as resource provider bond (in μPCLAW).
    pub staked: u64,
    /// Total balance (available + in_escrow + staked) (in μPCLAW).
    pub total: u64,
}

impl BalanceSnapshot {
    /// Format the balance for display.
    pub fn display(&self) -> String {
        format!(
            "Available: {:.6} PCLAW\nIn escrow: {:.6} PCLAW\nStaked: {:.6} PCLAW\nTotal: {:.6} PCLAW",
            from_micro(self.available),
            from_micro(self.in_escrow),
            from_micro(self.staked),
            from_micro(self.total),
        )
    }
}

impl fmt::Display for BalanceSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_conversions() {
        let balance = Balance::from_pclaw(1.5);
        assert_eq!(balance.as_micro(), 1_500_000);
        assert_eq!(balance.as_pclaw(), 1.5);
    }

    #[test]
    fn test_balance_arithmetic() {
        let a = Balance::from_pclaw(10.0);
        let b = Balance::from_pclaw(3.0);

        assert_eq!((a + b).as_pclaw(), 13.0);
        assert_eq!((a - b).as_pclaw(), 7.0);
    }

    #[test]
    fn test_balance_display() {
        let balance = Balance::from_pclaw(1234.567890);
        assert!(balance.to_string().contains("PCLAW"));
    }
}
