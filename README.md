# Veyro Protocol

**Tell it what you want. Set the limits. Let it work.**

Solana-native spending authorization for autonomous agents. This repository contains a Rust/Anchor program, `@veyro/sdk`, `@veyro/core`, test-token setup and an executable on-chain acceptance suite.

## Scope

The current implementation is an early testnet prototype. An owner allocates TEST-USD to a policy-controlled SPL Token account. An agent can buy TEST-MEME from a fixed-price test pool only within the owner's budget, recipient, program, expiry and minimum-output constraints. Revocation is enforced at execution in chain order.

**TEST-USD is not official USDC. TEST-MEME is not the coin discovered on X. Both are valueless fixtures.** Research of real mainnet tokens is separate from testnet execution. No mainnet purchase or profit guarantee is implemented.

## Packages

- `programs/veyro`: Anchor enforcement and an immutable fixed-price test pool.
- `packages/sdk`: narrow transaction encoder, signatures, PDA derivation, RPC and typed instruction builders. It supports only this demo's legacy transaction flow, not a general wallet SDK.
- `packages/core`: intent parsing, deterministic research ranking and explicitly provisional UI policy previews.
- `scripts`: faucet funding, isolated test fixtures and finalized ALLOW/DENY verification.

## Build and verify

Use Node 24+, Rust and Solana CLI/build tools. Anchor is used as a Rust dependency; the Anchor CLI is not required.

```sh
npm ci
npm run build
npm test
cargo test --workspace --locked
npm run build:onchain
npm run test:localnet
```

`test:localnet` rebuilds the program, starts an isolated validator on port 18999, creates disposable fixture keys and test tokens, executes the acceptance suite, and stops its validator. Evidence is saved under `target/verification-*/report.json`. It uses no existing wallet or ledger. `CARGO_HOME` can point to a writable cache if your environment restricts the default Rust cache.

Each accepted swap is checked against finalized policy counters and exact SPL token balance deltas. Denied transactions are deliberately broadcast with preflight disabled; the suite requires the expected finalized program error and unchanged token balances and policy bytes. Transaction fees are separate from the token spending policy.

To work interactively, run your own local validator with `target/deploy/veyro.so` loaded at the program address, then:

```sh
SOLANA_RPC_URL=http://127.0.0.1:8899 npm run bootstrap
npm run verify:chain
```

Bootstrap generates **disposable test-only keys** in ignored `keys/` with restricted permissions. The owner, agent and executor have separate keys. Never supply a mainnet wallet key. Verification revokes the initial agent at the end. For another full run, use `test:localnet` or a fresh `VEYRO_KEYS_DIR` and `VEYRO_DEPLOYMENT_FILE`. Bootstrap refuses a used, expired or revoked policy instead of resetting spending or falsely reporting it ready.

## Testnet deployment

The declared program address must match the deployment keypair. For a new checkout, generate a new program key locally, replace the `declare_id!` value in Rust and `PROGRAM_ID` in the SDK with its public key, then rebuild. Do not publish the keypair.

```sh
mkdir -p keys
solana-keygen new --silent --no-bip39-passphrase --outfile keys/program.json
solana-keygen new --silent --no-bip39-passphrase --outfile keys/deployer.json
solana-keygen pubkey keys/program.json
# Update program ID in Rust and SDK, then rebuild both.
solana airdrop 2 --url testnet --keypair keys/deployer.json
solana program deploy --url testnet --keypair keys/deployer.json --program-id keys/program.json target/deploy/veyro.so
SOLANA_RPC_URL=https://api.testnet.solana.com VEYRO_KEYS_DIR=keys/testnet VEYRO_DEPLOYMENT_FILE=target/testnet-deployment.json npm run bootstrap
VEYRO_KEYS_DIR=keys/testnet VEYRO_DEPLOYMENT_FILE=target/testnet-deployment.json npm run verify:chain
```

Faucets are rate limited and may fail. Do not retry indefinitely or substitute mainnet funds. Preserve the deployment/upgrade key securely. `deployment.local.json` records addresses and is ignored because RPC URLs can contain provider credentials. Configure `veyro-live` with that file and disposable demo keys on a private server volume.

## Validation status

Rust host tests, the deployable SBF build and SDK tests pass. The acceptance suite verifies actual execution using a local Solana validator; see [validation evidence](docs/VALIDATION.md) for the recorded results and remaining testnet deployment requirements. Public testnet deployment is a separate step and must not be inferred from local signatures. This is an unaudited MVP; see [the threat model](docs/THREAT_MODEL.md).

Related repositories: [Veyro Live](https://github.com/veyro-real/veyro-live) · [Landing page](https://github.com/veyro-real/veyro-landing-page).
