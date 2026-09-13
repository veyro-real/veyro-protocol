# Protocol validation

Verified on 2026-09-13 using Solana test-validator 4.2.1, cargo-build-sbf 4.1.0 / platform-tools 1.54, and the compiled Rust/Anchor program.

- Rust host tests: 3 passed.
- SDK tests: 11 passed, including allowlist bounds, preflight verdicts, independent CLI PDA vectors, signature binding, integer bounds and mainnet rejection through a local proxy.
- Solana SBF release build: passed; binary is 225,616 bytes.
- Local validator: 18 actions finalized and all assertions passed.
- Program address: `2Z7xH99Z4YvG4U2Ew5PUZtVh8FE1VRhQ1Mo9dFvRvS3Q`.
- Binary SHA-256: `789a7b2883fdefc3dbe5bcdc7efa80b439cc8e0cdefb9bb25ef45db2345353f7`.

The [machine-readable report](verification/localnet.json) contains exact transactions, evaluated policy snapshots, results and before/after balances. These signatures belong to the local test ledger; they are not public testnet explorer receipts.

| Scenario | Verified result |
| --- | --- |
| Approved 60 TEST-USD swap | ALLOW; exact input/output balance changes and nonce increment |
| Above per-transaction cap by one base unit | DENY; no token or policy changes |
| Above cumulative cap by one base unit | DENY; no token or policy changes |
| Unauthorized recipient and compromised agent | DENY; attacker balance unchanged |
| Stale nonce, unapproved agent or executor | DENY |
| Agent weakens minimum output / impossible output | DENY |
| Spending exactly the remaining 90 TEST-USD | ALLOW; lifetime spend reaches exactly 150 |
| Owner creates two supplemental test policies | ALLOW; existing policy unchanged |
| No allowed program / expired policy | DENY |
| Owner revocation | ALLOW; only active state changes |
| Revoked agent spends or diverts funds | DENY |

Expected denials were broadcast with preflight disabled and still rejected by the deployed program in the validator. Success requires finalized metadata and exact accounting; a passing UI preview alone cannot pass this suite.

## Public testnet

The public testnet genesis was checked successfully. Public deployment is not yet verified: CLI faucet funding requests failed, and the official website requires verification before dispensing funds. No mainnet funds are used. The dedicated deployment address is `7QF84Q6g9BhNve3NDp9mhN8bEAKpLmmigdAVkBQQqkqS`.

This evidence covers this narrow fixed-price test-token swap. It is not an external audit, real USDC integration, live market routing or profit validation.
