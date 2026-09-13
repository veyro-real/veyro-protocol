# Architecture

The current scope replaces the original transfer-only proposal with a narrow test-token swap demonstration. Veyro is independent and uses only its own project source and ordinary platform dependencies.

## Flow

Phone/chat intent → bounded crypto-X research → verified mainnet SPL mint candidate → explicit testnet proxy-token mapping → current policy read → actual transaction simulation → durable decision → atomic on-chain swap → finalization and receipt.

On testnet the executor buys TEST-MEME with TEST-USD from the fixed-price pool. This is a real SPL Token transaction when configured and deployed, but is not a purchase of the research candidate. Fixtures never get labeled as live X data. The hosted app has a separate, explicitly labeled rehearsal mode that performs no blockchain transaction.

## On-chain

One Anchor program implements immutable pool creation, owner policy creation, agent/executor-authorized swaps, owner revocation and owner recovery after revocation. A policy is derived from owner and agent keys and cannot be closed, reset or reactivated. A pool is derived from admin and output-mint keys; its quote/output mints and conversion rate are immutable.

Policy fields bind owner, agent, executor, one approved recipient wallet, one approved pool, an allowed program (canonical SPL Token or the default key meaning deny all), maximum input per transaction, lifetime cumulative input, expiry, minimum output/input rate, active state and replay nonce. The recipient and program sets are intentionally bounded to one entry in this MVP. Amounts use six-decimal integer test-token units. Successful input spending is monotonic; deposit and recovery do not reset it.

The vault is a classic SPL Token account with the policy PDA as token authority. The pool controls its reserves. Execution verifies token-program identity, account ownership, mints, authority, initialization state and absence of delegate/native/close-authority options. Token-2022 is unsupported. Fixed CPI instructions move input from vault to pool and output from pool to the approved recipient. The transaction is restricted to a single top-level Veyro instruction; foreign wrappers fail introspection. Spend and nonce update atomically with both transfers.

The owner-defined `min_rate` protects output even if the compromised agent submits a weak quote. An agent-specified minimum must meet that floor. A fixed-price pool is intentionally not a production pricing or slippage model.

## Off-chain

`@veyro/sdk` builds exactly this limited set of transactions and handles native RPC, Ed25519 signatures and Anchor field encoding. `@veyro/core` parses a separate numeric budget and ranks candidates deterministically. UI previews are not authoritative authorization.

`veyro-live` is one Next.js service, one SQLite audit store and a phone-friendly interface. Testnet mode uses disposable operator-owned demo keys; it does not claim that the phone user has a self-custodied production wallet. Operator and agent tool credentials have different privileges. A future wallet-based owner flow must be implemented before real funds.

## Acceptance

- An approved swap finalizes with vault −input, pool +input/−output, approved recipient +output, spent +input and nonce +1.
- Over per-transaction or cumulative limits, wrong recipient, wrong program, expired or revoked policy, stale nonce, wrong signer, wrong accounts and extra instructions fail without token/counter changes.
- A policy-approved simulation can still fail after revocation or a competing spend; receipts preserve the distinction.
- Every admitted service request gets durable intake and a decision or explicit pending/error status. Signed bytes are saved before sending. Unknown settlement is never retried with a new nonce automatically.
- X uses official read-only API calls with bounded results, persistent daily request counts, caching and no automatic fallback that pretends to be live data.
- Public UI exposes no keys, RPC credentials, bearer tokens or signed transaction bytes.
- Testnet genesis is verified before signing or sending. Mainnet remains disabled.

## Later mainnet gate

Add a narrowly reviewed production DEX adapter; validate exact quote/output mints and balance deltas; implement self-custodied owner authorization, minimum-output and expiry semantics for changing prices; replace test-token issuance and demo custody; review the protocol and key management; rehearse failure cases. A new owner authorization is needed for the actual real-money pilot. The testnet build is not moved to mainnet by changing an endpoint.
