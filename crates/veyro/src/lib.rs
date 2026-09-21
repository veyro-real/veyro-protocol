//! Spend-authorization rules for the Veyro Solana program.
//!
//! An owner grants an agent a policy: a per-transaction ceiling, a cumulative
//! budget, an expiry, a nonce, and optional recipient and program allowlists.
//! Before an executor submits a transfer, the same six checks run here that the
//! on-chain program runs, in the same order, so a client can refuse a spend
//! without paying for a failed transaction.
//!
//! This crate is the rules only. It holds no keys, builds no transactions and
//! talks to no network, which is why it has no dependencies and builds on
//! `no_std`.
//!
//! ```
//! use veyro::{authorize, Limits, Spend};
//!
//! let limits = Limits {
//!     active: true,
//!     expires_at: 1_800_000_000,
//!     expected_nonce: 0,
//!     max_amount: 100,
//!     spent: 0,
//!     total_limit: 150,
//! };
//! let spend = Spend { amount: 60, nonce: 0, now: 1_700_000_000 };
//! assert_eq!(authorize(&limits, &spend), Ok(60));
//!
//! // The same policy will not cover a second spend of 100: 60 + 100 > 150.
//! let after = Limits { spent: 60, expected_nonce: 1, ..limits };
//! let again = Spend { amount: 100, nonce: 1, now: 1_700_000_000 };
//! assert_eq!(again.amount > after.max_amount, false);
//! assert!(authorize(&after, &again).is_err());
//! ```
//!
//! The program this mirrors runs on testnet. Nothing here asserts that any
//! deployment, balance or transfer exists.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;

/// Program ID of the Veyro authorization program, base58.
pub const PROGRAM_ID: &str = "2Z7xH99Z4YvG4U2Ew5PUZtVh8FE1VRhQ1Mo9dFvRvS3Q";

/// Most recipients a single policy may allowlist.
pub const MAX_RECIPIENTS: usize = 8;

/// Most programs a single policy may allowlist.
pub const MAX_PROGRAMS: usize = 4;

/// Account version of a pool-routed policy.
pub const POLICY_VERSION: u8 = 2;

/// Account version of a live policy.
pub const LIVE_POLICY_VERSION: u8 = 1;

/// A 32-byte Solana public key, unencoded.
///
/// Deliberately a plain array: this crate does not depend on `solana-program`,
/// and every Solana `Pubkey` type converts to and from one for free.
pub type Pubkey = [u8; 32];

/// Why a spend was refused.
///
/// The variants carry the same names the on-chain program reports, so a client
/// refusal and a program refusal are the same string to a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Denial {
    /// The owner revoked the policy.
    Revoked,
    /// The policy's expiry has passed.
    Expired,
    /// The nonce does not match the one the policy expects; a replay.
    StaleNonce,
    /// A spend of zero.
    InvalidAmount,
    /// Over the per-transaction ceiling.
    MaxTransactionExceeded,
    /// Within the per-transaction ceiling, but over the remaining budget.
    CumulativeLimitExceeded,
    /// Spent plus amount does not fit in a `u64`.
    Overflow,
    /// More than [`MAX_RECIPIENTS`] recipients.
    TooManyRecipients,
    /// More than [`MAX_PROGRAMS`] programs.
    TooManyPrograms,
    /// The same key listed twice in one allowlist.
    DuplicateEntry,
}

impl Denial {
    /// The program's error string for this denial.
    pub const fn code(self) -> &'static str {
        match self {
            Denial::Revoked => "REVOKED",
            Denial::Expired => "EXPIRED",
            Denial::StaleNonce => "STALE_NONCE",
            Denial::InvalidAmount => "INVALID_AMOUNT",
            Denial::MaxTransactionExceeded => "MAX_TRANSACTION_EXCEEDED",
            Denial::CumulativeLimitExceeded => "CUMULATIVE_LIMIT_EXCEEDED",
            Denial::Overflow => "OVERFLOW",
            Denial::TooManyRecipients | Denial::TooManyPrograms | Denial::DuplicateEntry => {
                "INVALID_POLICY"
            }
        }
    }
}

impl fmt::Display for Denial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Denial {}

/// The parts of a policy that bound a spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// False once the owner revokes.
    pub active: bool,
    /// Unix seconds after which the policy is dead.
    pub expires_at: i64,
    /// The nonce the next spend must carry.
    pub expected_nonce: u64,
    /// Ceiling on any single spend.
    pub max_amount: u64,
    /// Total already spent under this policy.
    pub spent: u64,
    /// Ceiling on the sum of all spends.
    pub total_limit: u64,
}

impl Limits {
    /// What is left of the budget, saturating at zero.
    pub const fn remaining(&self) -> u64 {
        self.total_limit.saturating_sub(self.spent)
    }

    /// The largest spend this policy would currently authorize.
    ///
    /// The per-transaction ceiling and the remaining budget, whichever binds
    /// first. Zero for a revoked policy.
    pub const fn headroom(&self) -> u64 {
        if !self.active {
            return 0;
        }
        let remaining = self.remaining();
        if self.max_amount < remaining {
            self.max_amount
        } else {
            remaining
        }
    }
}

/// A spend an agent proposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spend {
    /// Amount in the policy's own units.
    pub amount: u64,
    /// The nonce this spend carries.
    pub nonce: u64,
    /// Unix seconds to evaluate the expiry against.
    pub now: i64,
}

/// Decide whether a policy authorizes a spend.
///
/// Returns the new cumulative total on success. The checks run in the program's
/// order, so the first failure reported here is the one the chain would report.
pub fn authorize(limits: &Limits, spend: &Spend) -> Result<u64, Denial> {
    if !limits.active {
        return Err(Denial::Revoked);
    }
    if spend.now >= limits.expires_at {
        return Err(Denial::Expired);
    }
    if spend.nonce != limits.expected_nonce {
        return Err(Denial::StaleNonce);
    }
    if spend.amount == 0 {
        return Err(Denial::InvalidAmount);
    }
    if spend.amount > limits.max_amount {
        return Err(Denial::MaxTransactionExceeded);
    }
    let total = limits
        .spent
        .checked_add(spend.amount)
        .ok_or(Denial::Overflow)?;
    if total > limits.total_limit {
        return Err(Denial::CumulativeLimitExceeded);
    }
    Ok(total)
}

/// Check that a policy's allowlists are within bounds and free of duplicates.
///
/// Empty is allowed and means unrestricted, matching the program.
pub fn validate_allowlists(recipients: &[Pubkey], programs: &[Pubkey]) -> Result<(), Denial> {
    if recipients.len() > MAX_RECIPIENTS {
        return Err(Denial::TooManyRecipients);
    }
    if programs.len() > MAX_PROGRAMS {
        return Err(Denial::TooManyPrograms);
    }
    for (index, recipient) in recipients.iter().enumerate() {
        if recipients[..index].contains(recipient) {
            return Err(Denial::DuplicateEntry);
        }
    }
    for (index, program) in programs.iter().enumerate() {
        if programs[..index].contains(program) {
            return Err(Denial::DuplicateEntry);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn limits() -> Limits {
        Limits {
            active: true,
            expires_at: 100,
            expected_nonce: 0,
            max_amount: 100,
            spent: 0,
            total_limit: 150,
        }
    }

    const fn at(amount: u64, now: i64) -> Spend {
        Spend { amount, nonce: 0, now }
    }

    #[test]
    fn a_spend_inside_every_bound_is_authorized() {
        assert_eq!(authorize(&limits(), &at(100, 99)), Ok(100));
    }

    #[test]
    fn expiry_is_exclusive_at_the_boundary_second() {
        assert_eq!(authorize(&limits(), &at(1, 99)), Ok(1));
        assert_eq!(authorize(&limits(), &at(1, 100)), Err(Denial::Expired));
    }

    #[test]
    fn a_revoked_policy_authorizes_nothing() {
        let revoked = Limits { active: false, ..limits() };
        assert_eq!(authorize(&revoked, &at(1, 0)), Err(Denial::Revoked));
        assert_eq!(revoked.headroom(), 0);
    }

    #[test]
    fn a_replayed_nonce_is_refused() {
        let policy = Limits { expected_nonce: 1, ..limits() };
        assert_eq!(authorize(&policy, &at(1, 0)), Err(Denial::StaleNonce));
    }

    #[test]
    fn zero_is_not_a_spend() {
        assert_eq!(authorize(&limits(), &at(0, 0)), Err(Denial::InvalidAmount));
    }

    #[test]
    fn the_per_transaction_ceiling_binds_before_the_budget() {
        assert_eq!(
            authorize(&limits(), &at(101, 0)),
            Err(Denial::MaxTransactionExceeded)
        );
    }

    #[test]
    fn the_budget_binds_even_when_one_spend_would_fit() {
        let spent = Limits { spent: 100, ..limits() };
        assert_eq!(authorize(&spent, &at(51, 0)), Err(Denial::CumulativeLimitExceeded));
        assert_eq!(authorize(&spent, &at(50, 0)), Ok(150));
    }

    #[test]
    fn an_overflowing_total_is_refused_rather_than_wrapped() {
        let huge = Limits {
            max_amount: u64::MAX,
            spent: 1,
            total_limit: u64::MAX,
            ..limits()
        };
        assert_eq!(authorize(&huge, &at(u64::MAX, 0)), Err(Denial::Overflow));
    }

    #[test]
    fn headroom_is_the_tighter_of_ceiling_and_remaining_budget() {
        assert_eq!(limits().headroom(), 100);
        assert_eq!(Limits { spent: 100, ..limits() }.headroom(), 50);
        assert_eq!(Limits { spent: 150, ..limits() }.headroom(), 0);
        assert_eq!(Limits { spent: 200, ..limits() }.remaining(), 0);
    }

    #[test]
    fn allowlists_are_bounded_unique_and_may_be_empty() {
        let keys: [Pubkey; 9] = core::array::from_fn(|i| [i as u8; 32]);
        assert_eq!(validate_allowlists(&[], &[]), Ok(()));
        assert_eq!(validate_allowlists(&keys[..8], &keys[..4]), Ok(()));
        assert_eq!(validate_allowlists(&keys, &[]), Err(Denial::TooManyRecipients));
        assert_eq!(validate_allowlists(&[], &keys[..5]), Err(Denial::TooManyPrograms));
        assert_eq!(
            validate_allowlists(&[keys[0], keys[0]], &[]),
            Err(Denial::DuplicateEntry)
        );
        assert_eq!(
            validate_allowlists(&[], &[keys[1], keys[1]]),
            Err(Denial::DuplicateEntry)
        );
    }

    #[test]
    fn a_denial_reports_the_programs_own_error_string() {
        assert_eq!(Denial::CumulativeLimitExceeded.code(), "CUMULATIVE_LIMIT_EXCEEDED");
        assert_eq!(Denial::TooManyRecipients.code(), "INVALID_POLICY");
    }
}
