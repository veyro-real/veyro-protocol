//! Client-side helpers for building and checking Veyro spending policies.
//!
//! [`veyro`] holds the rules a spend is judged against. This crate is the step
//! before that: assembling a policy that the program will actually accept, and
//! sizing a spend that a policy will actually authorize.
//!
//! Every invariant here is one the on-chain program enforces. Checking them
//! locally turns a failed transaction into a typed error.
//!
//! ```
//! use veyro_sdk::{PolicyDraft, Invalid};
//!
//! let owner = [1u8; 32];
//! let agent = [2u8; 32];
//! let executor = [3u8; 32];
//!
//! let draft = PolicyDraft::new(owner, agent, executor)
//!     .max_amount(100)
//!     .total_limit(1_000)
//!     .expires_at(1_800_000_000);
//!
//! assert!(draft.validate(1_700_000_000).is_ok());
//!
//! // The agent may not be the owner: that would defeat the policy.
//! let self_dealing = PolicyDraft::new(owner, owner, executor)
//!     .max_amount(100)
//!     .total_limit(1_000)
//!     .expires_at(1_800_000_000);
//! assert_eq!(self_dealing.validate(1_700_000_000), Err(Invalid::AgentIsOwner));
//! ```

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;

pub use veyro::{
    authorize, validate_allowlists, Denial, Limits, Pubkey, Spend, MAX_PROGRAMS, MAX_RECIPIENTS,
    PROGRAM_ID,
};

/// Why a policy would be rejected before it is ever submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Invalid {
    /// A per-transaction ceiling of zero authorizes nothing.
    MaxAmountIsZero,
    /// The budget is smaller than a single permitted transaction.
    TotalBelowMax,
    /// The policy expires at or before the moment it is created.
    AlreadyExpired,
    /// The agent is the owner, so the policy bounds nobody.
    AgentIsOwner,
    /// The executor is the agent, collapsing proposal and execution.
    ExecutorIsAgent,
    /// The executor is the owner, collapsing custody and execution.
    ExecutorIsOwner,
    /// An allowlist is too long or repeats a key.
    Allowlist(Denial),
}

impl Invalid {
    /// The program's error string for this rejection.
    pub const fn code(self) -> &'static str {
        match self {
            Invalid::AlreadyExpired => "EXPIRED",
            Invalid::Allowlist(denial) => denial.code(),
            _ => "INVALID_POLICY",
        }
    }
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Invalid {}

/// A policy under construction.
///
/// Built by value so it works without an allocator. Allowlists are borrowed
/// slices; empty means unrestricted, matching the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDraft<'a> {
    /// The account granting the authority.
    pub owner: Pubkey,
    /// The account permitted to propose spends.
    pub agent: Pubkey,
    /// The account permitted to submit them.
    pub executor: Pubkey,
    /// Ceiling on any single spend.
    pub max_amount: u64,
    /// Ceiling on the sum of all spends.
    pub total_limit: u64,
    /// Unix seconds after which the policy is dead.
    pub expires_at: i64,
    /// Recipients the agent may pay; empty is unrestricted.
    pub allowed_recipients: &'a [Pubkey],
    /// Programs the agent may invoke; empty is unrestricted.
    pub allowed_programs: &'a [Pubkey],
}

impl<'a> PolicyDraft<'a> {
    /// Start a draft. The three accounts must be distinct; [`validate`] says so.
    ///
    /// [`validate`]: PolicyDraft::validate
    pub const fn new(owner: Pubkey, agent: Pubkey, executor: Pubkey) -> Self {
        Self {
            owner,
            agent,
            executor,
            max_amount: 0,
            total_limit: 0,
            expires_at: 0,
            allowed_recipients: &[],
            allowed_programs: &[],
        }
    }

    /// Set the per-transaction ceiling.
    pub const fn max_amount(mut self, amount: u64) -> Self {
        self.max_amount = amount;
        self
    }

    /// Set the cumulative budget.
    pub const fn total_limit(mut self, limit: u64) -> Self {
        self.total_limit = limit;
        self
    }

    /// Set the expiry, in unix seconds.
    pub const fn expires_at(mut self, at: i64) -> Self {
        self.expires_at = at;
        self
    }

    /// Restrict which recipients the agent may pay.
    pub const fn allowed_recipients(mut self, recipients: &'a [Pubkey]) -> Self {
        self.allowed_recipients = recipients;
        self
    }

    /// Restrict which programs the agent may invoke.
    pub const fn allowed_programs(mut self, programs: &'a [Pubkey]) -> Self {
        self.allowed_programs = programs;
        self
    }

    /// Check every invariant the program checks at creation.
    ///
    /// `now` is unix seconds; the program compares the expiry against the
    /// cluster clock, so pass the clock you expect the transaction to land
    /// under rather than a local timestamp you have held for a while.
    pub fn validate(&self, now: i64) -> Result<(), Invalid> {
        if self.max_amount == 0 {
            return Err(Invalid::MaxAmountIsZero);
        }
        if self.total_limit < self.max_amount {
            return Err(Invalid::TotalBelowMax);
        }
        if self.expires_at <= now {
            return Err(Invalid::AlreadyExpired);
        }
        if self.agent == self.owner {
            return Err(Invalid::AgentIsOwner);
        }
        if self.executor == self.agent {
            return Err(Invalid::ExecutorIsAgent);
        }
        if self.executor == self.owner {
            return Err(Invalid::ExecutorIsOwner);
        }
        validate_allowlists(self.allowed_recipients, self.allowed_programs)
            .map_err(Invalid::Allowlist)?;
        Ok(())
    }

    /// The limits this draft becomes once created, before anything is spent.
    pub const fn limits(&self) -> Limits {
        Limits {
            active: true,
            expires_at: self.expires_at,
            expected_nonce: 0,
            max_amount: self.max_amount,
            spent: 0,
            total_limit: self.total_limit,
        }
    }
}

/// Whether a recipient is permitted. An empty allowlist permits everyone.
pub fn recipient_allowed(allowlist: &[Pubkey], recipient: &Pubkey) -> bool {
    allowlist.is_empty() || allowlist.contains(recipient)
}

/// Whether a program is permitted. An empty allowlist permits everyone.
pub fn program_allowed(allowlist: &[Pubkey], program: &Pubkey) -> bool {
    allowlist.is_empty() || allowlist.contains(program)
}

/// The largest spend this policy would authorize right now, if any.
///
/// `None` when the policy is revoked, expired, or has no budget left. Use it to
/// offer a user the amount that will actually go through instead of letting
/// them pick one that cannot.
pub fn largest_authorized(limits: &Limits, now: i64) -> Option<u64> {
    if !limits.active || now >= limits.expires_at {
        return None;
    }
    match limits.headroom() {
        0 => None,
        amount => Some(amount),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: Pubkey = [1u8; 32];
    const AGENT: Pubkey = [2u8; 32];
    const EXECUTOR: Pubkey = [3u8; 32];
    const NOW: i64 = 1_000;

    fn draft<'a>() -> PolicyDraft<'a> {
        PolicyDraft::new(OWNER, AGENT, EXECUTOR)
            .max_amount(100)
            .total_limit(1_000)
            .expires_at(2_000)
    }

    #[test]
    fn a_well_formed_draft_validates() {
        assert_eq!(draft().validate(NOW), Ok(()));
    }

    #[test]
    fn a_ceiling_of_zero_is_refused() {
        assert_eq!(
            draft().max_amount(0).validate(NOW),
            Err(Invalid::MaxAmountIsZero)
        );
    }

    #[test]
    fn a_budget_below_one_transaction_is_refused() {
        assert_eq!(
            draft().total_limit(99).validate(NOW),
            Err(Invalid::TotalBelowMax)
        );
        assert_eq!(draft().total_limit(100).validate(NOW), Ok(()));
    }

    #[test]
    fn an_expiry_at_or_before_now_is_refused() {
        assert_eq!(
            draft().expires_at(NOW).validate(NOW),
            Err(Invalid::AlreadyExpired)
        );
        assert_eq!(draft().expires_at(NOW + 1).validate(NOW), Ok(()));
    }

    #[test]
    fn the_three_roles_must_be_distinct() {
        assert_eq!(
            PolicyDraft::new(OWNER, OWNER, EXECUTOR).max_amount(1).total_limit(1)
                .expires_at(2_000).validate(NOW),
            Err(Invalid::AgentIsOwner)
        );
        assert_eq!(
            PolicyDraft::new(OWNER, AGENT, AGENT).max_amount(1).total_limit(1)
                .expires_at(2_000).validate(NOW),
            Err(Invalid::ExecutorIsAgent)
        );
        assert_eq!(
            PolicyDraft::new(OWNER, AGENT, OWNER).max_amount(1).total_limit(1)
                .expires_at(2_000).validate(NOW),
            Err(Invalid::ExecutorIsOwner)
        );
    }

    #[test]
    fn a_duplicate_recipient_is_refused_with_the_programs_code() {
        let dupes = [EXECUTOR, EXECUTOR];
        let bad = draft().allowed_recipients(&dupes);
        assert_eq!(bad.validate(NOW), Err(Invalid::Allowlist(Denial::DuplicateEntry)));
        assert_eq!(bad.validate(NOW).unwrap_err().code(), "INVALID_POLICY");
    }

    #[test]
    fn an_empty_allowlist_permits_everyone() {
        assert!(recipient_allowed(&[], &AGENT));
        assert!(program_allowed(&[], &AGENT));
        assert!(!recipient_allowed(&[OWNER], &AGENT));
        assert!(recipient_allowed(&[OWNER, AGENT], &AGENT));
    }

    #[test]
    fn a_fresh_draft_authorizes_up_to_its_ceiling() {
        let limits = draft().limits();
        assert_eq!(largest_authorized(&limits, NOW), Some(100));
        assert_eq!(authorize(&limits, &Spend { amount: 100, nonce: 0, now: NOW }), Ok(100));
    }

    #[test]
    fn a_nearly_exhausted_policy_offers_only_what_is_left() {
        let limits = Limits { spent: 970, ..draft().limits() };
        assert_eq!(largest_authorized(&limits, NOW), Some(30));
        assert_eq!(authorize(&limits, &Spend { amount: 30, nonce: 0, now: NOW }), Ok(1_000));
    }

    #[test]
    fn an_expired_or_revoked_policy_offers_nothing() {
        let limits = draft().limits();
        assert_eq!(largest_authorized(&limits, 2_000), None);
        assert_eq!(largest_authorized(&Limits { active: false, ..limits }, NOW), None);
        assert_eq!(largest_authorized(&Limits { spent: 1_000, ..limits }, NOW), None);
    }
}
