# veyro-sdk

Client-side helpers for [Veyro](https://github.com/veyro-real/veyro-protocol)
spending policies on Solana.

[`veyro`](https://crates.io/crates/veyro) holds the rules a spend is judged
against. This crate is the step before that: assembling a policy the program
will accept, and sizing a spend a policy will authorize. Every invariant it
checks is one the on-chain program enforces, so checking locally turns a failed
transaction into a typed error.

```rust
use veyro_sdk::{largest_authorized, Invalid, PolicyDraft};

let (owner, agent, executor) = ([1u8; 32], [2u8; 32], [3u8; 32]);

let draft = PolicyDraft::new(owner, agent, executor)
    .max_amount(100)
    .total_limit(1_000)
    .expires_at(1_800_000_000);

assert!(draft.validate(1_700_000_000).is_ok());
assert_eq!(largest_authorized(&draft.limits(), 1_700_000_000), Some(100));

// The executor may not also be the owner: that collapses custody into execution.
let collapsed = PolicyDraft::new(owner, agent, owner)
    .max_amount(100)
    .total_limit(1_000)
    .expires_at(1_800_000_000);
assert_eq!(collapsed.validate(1_700_000_000), Err(Invalid::ExecutorIsOwner));
```

No dependencies beyond `veyro`, no keys, no network, and `no_std` with
`default-features = false`.

The program it mirrors runs on testnet. Nothing here asserts that any
deployment, balance or transfer exists.

## License

Apache-2.0
