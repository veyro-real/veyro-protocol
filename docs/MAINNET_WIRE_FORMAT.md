# Mainnet program wire format

The deployed mainnet program `2Z7xH99Z4YvG4U2Ew5PUZtVh8FE1VRhQ1Mo9dFvRvS3Q` is not
an Anchor program. It is a hand-rolled BPF entrypoint that dispatches on a single
leading byte and reads fixed offsets out of a 154-byte account. It carries no IDL
and no discriminators, so the Anchor clients in `packages/` cannot talk to it.

This document is the contract. `programs/veyro-mainnet/src/lib.rs` is the source of
truth; if the two disagree, the Rust wins and this file is stale.

## Why this needs writing down

A client exists outside this repository. `lib/chain/policy.ts` in
[veyro-live](https://github.com/veyro-real/veyro-live) encodes every offset below by
hand. Nothing mechanically couples the two: change an offset here and that client
keeps compiling, keeps passing its tests, and starts producing transactions the
program rejects on chain. A signer swap is worse still, because it fails only for
the account that was supposed to be authorised.

Treat any change to the layout or the signer checks as a breaking change with an
out-of-tree consumer.

## Instructions

Dispatch is `data[0]`. Any other value returns `INVALID_INSTRUCTION_DATA`.

| Byte | Instruction | Data length | Signer checked |
| --- | --- | --- | --- |
| 1 | `CREATE_POLICY` | exactly 113 | `accounts[0]` must be the owner |
| 2 | `REVOKE_POLICY` | 1 | `accounts[0]` must be the owner already stored at offset 2 |
| 3 | `CHECK_SPEND` | exactly 65 | `accounts[0]` must be the **agent** stored at offset 34 |

Account order is fixed for all three: `accounts[0]` is the signer being checked and
`accounts[1]` is the policy account, which must be writable. A length other than the
one stated is rejected outright rather than padded or truncated.

The signer difference is the part most easily got wrong. Creating and revoking a
mandate are the owner's acts. Spending against one is the agent's, and the program
compares the signer to the policy's agent field, not its owner field.

### `CREATE_POLICY` (113 bytes)

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 1 | tag, `1` |
| 1 | 32 | agent pubkey |
| 33 | 8 | max lamports per trade, u64 LE |
| 41 | 8 | budget in lamports, u64 LE |
| 49 | 8 | expiry, u64 LE |
| 57 | 8 | starting nonce, u64 LE |
| 65 | 32 | recipient pubkey |
| 97 | 16 | tag bytes |

### `CHECK_SPEND` (65 bytes)

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 1 | tag, `3` |
| 1 | 8 | lamports, u64 LE |
| 9 | 8 | timestamp, u64 LE |
| 17 | 32 | recipient pubkey |
| 49 | 16 | tag bytes |

The timestamp at offset 9 is compared against the policy's expiry and must not
exceed it. The recipient and the 16 tag bytes must equal the values stored in the
policy, or the call is rejected. On success the program adds the amount to `spent`
and increments `nonce`; both are checked for overflow first.

### `REVOKE_POLICY` (1 byte)

Sets the active flag at offset 1 to `0`. The policy is never closed, reset or
reactivated.

## Policy account (154 bytes)

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 1 | version, currently `1` |
| 1 | 1 | active, `1` or `0` |
| 2 | 32 | owner pubkey |
| 34 | 32 | agent pubkey |
| 66 | 8 | max lamports per trade |
| 74 | 8 | budget in lamports |
| 82 | 8 | spent lamports |
| 90 | 8 | expiry |
| 98 | 8 | nonce |
| 106 | 32 | recipient pubkey |
| 138 | 16 | tag bytes |

An account shorter than 154 bytes, or whose version byte is not `1`, is rejected.
A client reading one should return nothing rather than a plausible-looking guess.

## What this program does not do

It validates limits and accounting and holds no tokens. Including `CHECK_SPEND` in a
transaction gates that transaction atomically without taking custody, which makes it
additive to an off-chain limit check rather than a replacement for one.

The custodial program with `execute_route` and a program-owned vault is **not**
deployed to mainnet. Nothing here should be described as if it were.
