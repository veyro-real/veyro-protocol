# Threat model

Assume the agent, prompts and research posts are hostile. Trust the owner to choose policy and the deployed program to enforce it. The runtime and signature primitives must be correct. The program upgrade authority can replace enforcement and remains trusted.

| Threat | Control | Residual risk |
| --- | --- | --- |
| Stolen agent key / prompt injection | On-chain budget, fixed pool/program, recipient, expiry, minimum rate and nonce | An attacker can spend the full authorized budget on permitted actions |
| Direct vault drain | PDA token authority; no exposed arbitrary signing or CPI | Program/upgrade compromise defeats this |
| Race / replay | Shared writable policy, checked sums and atomic nonce increment | Simulated ALLOW is provisional |
| Revocation race | Active checked at execution; owner-only revoke | Earlier chain-ordered transfers may succeed |
| Malicious accounts | Canonical policy/pool seeds, token program, mint/authority/state checks | Rust and integration tests are required to validate implementation |
| Poor output | Owner minimum conversion rate, agent minimum output | Fixed-rate test pool is not real-market protection |
| Fake social token | Parse candidate mint and verify classic SPL mint account on a separate mainnet read endpoint | Verification does not prove legitimacy or liquidity |
| Research instructions | Social text is data; it never changes policy, destinations or tool privileges | Popularity can be manipulated; ranking is not financial advice |
| Audit crash | Persist intake and signed bytes before broadcast; reconcile candidates | Local admin can alter SQLite; no network-wide failed-attempt coverage |
| Demo server compromise | Disposable test-only owner/agent/executor keys; protected operator controls | Server custody is trusted in hosted demo; unsuitable for production owner funds |
| X credit exhaustion | Authentication, fixed query, max 20 posts, cache and daily request cap | Requests can cost money; monetary rates not estimated |
| Public exposure | No committed keys/config; runtime secrets; public pages show only safe state | Deployment permissions and backups require operator care |

Never claim profitable trading, a legitimate meme coin, mainnet readiness, investor endorsement, global instantaneous cancellation or complete logging outside the service boundary. Keys under `keys/`, local deployments, runtime state and environment files are excluded from Git and Docker build contexts.
