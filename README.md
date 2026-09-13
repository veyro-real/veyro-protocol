# Veyro Protocol

**Tell it what you want. Set the limits. Let it work.**

Solana-native spending authorization for autonomous agents. This repository contains a Rust/Anchor program, `@veyro/sdk`, `@veyro/core`, test-token setup and a deterministic demo.

## Scope

The current implementation is an early testnet prototype. An owner allocates TEST-USD to a policy-controlled SPL Token account. An agent can buy TEST-MEME from a fixed-price test pool only within the owner's budget, recipient, program, expiry and minimum-output constraints. Revocation is enforced at execution in chain order.

**TEST-USD is not official USDC. TEST-MEME is not the coin discovered on X. Both are valueless fixtures.** Research of real mainnet tokens is separate from testnet execution. No mainnet purchase or profit guarantee is implemented.

## Packages

- `programs/veyro`: Anchor enforcement and an immutable fixed-price test pool.
- `packages/sdk`: narrow transaction encoder, signatures, PDA derivation, RPC and typed instruction builders. It supports only this demo's legacy transaction flow, not a general wallet SDK.
- `packages/core`: intent parsing, deterministic research ranking and explicitly provisional UI policy previews.
- `scripts`: faucet funding, test mint/pool setup and six visible ALLOW/DENY scenarios.

## Run locally

Use Node 24+, Rust and Solana CLI/build tools. Install from the checked-in lockfile:

```sh
npm ci
npm run build
npm test
cargo test --workspace
cargo build-sbf --manifest-path programs/veyro/Cargo.toml
solana-test-validator --ledger /tmp/veyro-validator --bpf-program 2Z7xH99Z4YvG4U2Ew5PUZtVh8FE1VRhQ1Mo9dFvRvS3Q target/deploy/veyro.so
```

In another terminal:

```sh
SOLANA_RPC_URL=http://127.0.0.1:8899 npm run bootstrap
npm run demo
```

Bootstrap generates **disposable test-only keys** in ignored `keys/` with restricted permissions. The owner, agent and executor have separate keys. Never supply a mainnet wallet key. The demo revokes the initial agent at the end; create a fresh local fixture for another full run, rather than resetting on-chain spending.

## Testnet deployment

The declared program address must match the deployment keypair. For a new checkout, generate a new program key locally, replace the `declare_id!` value in Rust and `PROGRAM_ID` in the SDK with its public key, then rebuild. Do not publish the keypair.

```sh
mkdir -p keys
solana-keygen new --no-bip39-passphrase --outfile keys/program.json
solana-keygen new --no-bip39-passphrase --outfile keys/deployer.json
solana-keygen pubkey keys/program.json
# Update program ID in Rust and SDK, then rebuild both.
solana airdrop 2 --url testnet --keypair keys/deployer.json
solana program deploy --url testnet --keypair keys/deployer.json --program-id keys/program.json target/deploy/veyro.so
SOLANA_RPC_URL=https://api.testnet.solana.com npm run bootstrap
```

Faucets are rate limited and may fail. Do not retry indefinitely or substitute mainnet funds. Preserve the deployment/upgrade key securely. `deployment.local.json` records addresses and is ignored because RPC URLs can contain provider credentials. Configure `veyro-live` with that file and disposable demo keys on a private server volume.

## Validation status

TypeScript package compilation and unit tests can run independently of a validator. Rust compilation, validator integration and public testnet execution must pass before calling the protocol deployed or verified. See CI results and [the threat model](docs/THREAT_MODEL.md); a successful TypeScript test is not an on-chain security audit.

Related repositories: [Veyro Live](https://github.com/veyro-real/veyro-live) · [Landing page](https://github.com/veyro-real/veyro-landing-page).
