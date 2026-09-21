# veyro

Spend-authorization rules for the [Veyro](https://github.com/veyro-real/veyro-protocol)
Solana program.

An owner grants an agent a policy: a per-transaction ceiling, a cumulative
budget, an expiry, a nonce, and optional recipient and program allowlists.
Before an executor submits a transfer, the same checks run here that the
on-chain program runs, in the same order, so a client can refuse a spend
without paying for a failed transaction.

```rust
use veyro::{authorize, Denial, Limits, Spend};

let limits = Limits {
    active: true,
    expires_at: 1_800_000_000,
    expected_nonce: 0,
    max_amount: 100,
    spent: 120,
    total_limit: 150,
};

// Inside the per-transaction ceiling, past what is left of the budget.
let spend = Spend { amount: 50, nonce: 0, now: 1_700_000_000 };
assert_eq!(authorize(&limits, &spend), Err(Denial::CumulativeLimitExceeded));
assert_eq!(limits.headroom(), 30);
```

This crate is the rules only. It holds no keys, builds no transactions and
talks to no network, so it has no dependencies and builds on `no_std`
(`default-features = false`).

The program it mirrors runs on testnet. Nothing here asserts that any
deployment, balance or transfer exists.

## License

Apache-2.0
