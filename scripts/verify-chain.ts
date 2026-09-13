import { readFileSync, writeFileSync, mkdirSync, renameSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import {
  Rpc, TOKEN, SYSTEM, keyFromSecret, generateKey, decodePolicy, swapIx, revokeIx,
  createPolicyIx, policyAddress, systemCreate, initializeToken, mintTo,
  type Key, type Policy, type Instruction, type SwapAccounts,
} from '../packages/sdk/src/index.js';

// This suite consumes and finally revokes a fresh bootstrap fixture. It never
// creates a replacement policy or falls back to another cluster on failure.
const UNIT = 1_000_000n;
const RATE = 1000n;
const reportFile = resolve(process.env.VEYRO_REPORT_FILE || 'outputs/chain-verification.json');
const json = (value: unknown) => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v, 2);
const now = () => new Date().toISOString();
const pause = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
function check(condition: unknown, code: string): asserts condition { if (!condition) throw Error(code); }

type Deployment = SwapAccounts & {
  programId: string; owner: string; attacker: string; ownerQuote: string;
  quoteMint: string; outputMint: string; rpcUrl: string;
};
type Snapshot = { policyAddress: string; vaultAddress: string; slot: number; policy: Policy; balances: Record<string, bigint>; policyBytes: string };
type ExpectedError = { number: number; code: string; reason: string };
type ChainError = { InstructionError: [number, { Custom: number }] } | null;
type Attempt = {
  id: number; scenario: string; timestamp: string; agent: string; actor: string;
  requestedAction: Record<string, unknown>; policyEvaluated: Record<string, unknown>;
  result: 'PENDING' | 'ALLOW' | 'DENY'; reason: string;
  simulation?: { result: 'ALLOW' | 'DENY'; reason: string; error: unknown };
  expectedError?: ExpectedError; submittedSignature?: string; transactionSignature?: string;
  submittedWithSkipPreflight?: boolean; finalizedAt?: string; slot?: number;
  chainError?: unknown; stateBefore: unknown; stateAfter?: unknown; assertionsPassed?: boolean;
};
const report: {
  schemaVersion: number; suite: string; startedAt: string; finishedAt?: string;
  status: 'RUNNING' | 'PASSED' | 'FAILED'; cluster?: string; genesisHash?: string;
  programId?: string; policy?: string; assets?: unknown; attempts: Attempt[]; failure?: string;
} = {
  schemaVersion: 1, suite: 'Veyro on-chain policy acceptance', startedAt: now(),
  status: 'RUNNING', attempts: [],
};
function save() {
  mkdirSync(dirname(reportFile), { recursive: true });
  writeFileSync(reportFile + '.tmp', json(report) + '\n');
  renameSync(reportFile + '.tmp', reportFile);
}

async function run() {
  const cfg = JSON.parse(readFileSync(process.env.VEYRO_DEPLOYMENT_FILE || 'deployment.local.json', 'utf8')) as Deployment;
  const rpc = new Rpc(process.env.SOLANA_RPC_URL || cfg.rpcUrl);
  const genesis = await rpc.assertTestCluster();
  report.cluster = genesis === '4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY' ? 'testnet' : 'localnet';
  report.genesisHash = genesis;
  report.programId = cfg.programId;
  report.policy = cfg.policy;
  report.assets = { quoteMint: cfg.quoteMint, outputMint: cfg.outputMint, quoteDecimals: 6, outputDecimals: 6, fixedPoolRate: RATE.toString(), officialUsdc: false, realFunds: false };
  save();
  const key = (name: string): Key => keyFromSecret(Uint8Array.from(JSON.parse(readFileSync(resolve(process.env.VEYRO_KEYS_DIR || 'keys', name + '.json'), 'utf8'))));
  const agent = key('agent'), executor = key('executor'), owner = key('owner'), outsider = key('admin');
  check(agent.publicKey === cfg.agent && executor.publicKey === cfg.executor && owner.publicKey === cfg.owner, 'FIXTURE_KEY_ADDRESSES_MISMATCH');
  check(![cfg.owner, cfg.agent, cfg.executor].includes(outsider.publicKey), 'FIXTURE_ADMIN_MUST_BE_DISTINCT');

  const tokenAccounts = {
    vault: cfg.vault, poolQuote: cfg.poolQuote, poolOutput: cfg.poolOutput,
    recipient: cfg.recipient, attacker: cfg.attacker, ownerQuote: cfg.ownerQuote,
  };
  async function snapshot(policyAddress = cfg.policy, vaultAddress = cfg.vault): Promise<Snapshot> {
    const data = await rpc.call<{ context: { slot: number }; value: ({ owner: string; data: [string, string] } | null)[] }>(
      'getMultipleAccounts', [[policyAddress, ...Object.values({ ...tokenAccounts, vault: vaultAddress })], { encoding: 'base64', commitment: 'finalized' }],
    );
    const account = data.value[0];
    check(account && account.owner === cfg.programId, 'POLICY_MISSING_OR_WRONG_OWNER');
    const policyBytes = account.data[0];
    const policy = decodePolicy(Buffer.from(policyBytes, 'base64'));
    const balances: Record<string, bigint> = {};
    Object.keys(tokenAccounts).forEach((name, index) => {
      const value = data.value[index + 1];
      check(value && value.owner === TOKEN, 'FIXTURE_TOKEN_ACCOUNT_INVALID_' + name.toUpperCase());
      const bytes = Buffer.from(value.data[0], 'base64');
      check(bytes.length === 165 && bytes[108] === 1, 'FIXTURE_TOKEN_STATE_INVALID_' + name.toUpperCase());
      balances[name] = bytes.readBigUInt64LE(64);
    });
    return { policyAddress, vaultAddress, slot: data.context.slot, policy, balances, policyBytes };
  }
  const publicState = (s: Snapshot) => ({ slot: s.slot, policyAddress: s.policyAddress, vaultAddress: s.vaultAddress, policy: s.policy, balances: s.balances });
  const policyView = (s: Snapshot) => ({ address: s.policyAddress, ...s.policy });
  function unchanged(before: Snapshot, after: Snapshot) {
    check(before.policyBytes === after.policyBytes, 'DENIED_TRANSACTION_CHANGED_POLICY');
    for (const name of Object.keys(tokenAccounts)) check(before.balances[name] === after.balances[name], 'DENIED_TRANSACTION_MOVED_' + name.toUpperCase());
  }
  function success(before: Snapshot, after: Snapshot, amount: bigint) {
    check(after.policy.spent === before.policy.spent + amount, 'SUCCESS_SPEND_COUNTER_MISMATCH');
    check(after.policy.nonce === before.policy.nonce + 1n, 'SUCCESS_NONCE_MISMATCH');
    check(json({ ...before.policy, spent: after.policy.spent, nonce: after.policy.nonce }) === json(after.policy), 'SUCCESS_CHANGED_POLICY_CONSTRAINTS');
    const deltas: Record<string, bigint> = { vault: -amount, poolQuote: amount, poolOutput: -amount * RATE, recipient: amount * RATE, attacker: 0n, ownerQuote: 0n };
    for (const [name, delta] of Object.entries(deltas)) check(after.balances[name] === before.balances[name] + delta, 'SUCCESS_BALANCE_MISMATCH_' + name.toUpperCase());
  }
  function expectError(error: unknown, logs: string[] | null, expected: ExpectedError, phase: string) {
    const actual = error as ChainError;
    check(actual?.InstructionError?.[0] === 0 && actual?.InstructionError?.[1]?.Custom === expected.number, phase + '_UNEXPECTED_ERROR_' + expected.code);
    check(logs?.some(line => line.includes('Error Code: ' + expected.code + '.')), phase + '_MISSING_ERROR_CODE_' + expected.code);
  }
  async function finalized(signature: string) {
    const until = Date.now() + 120_000;
    while (Date.now() < until) {
      const statuses = await rpc.call<{ value: ({ confirmationStatus: string; err: unknown } | null)[] }>('getSignatureStatuses', [[signature], { searchTransactionHistory: true }]);
      if (statuses.value[0]?.confirmationStatus === 'finalized') {
        const tx = await rpc.call<{ slot: number; meta: { err: unknown; logMessages: string[] | null } } | null>('getTransaction', [signature, { commitment: 'finalized', encoding: 'json', maxSupportedTransactionVersion: 0 }]);
        if (tx?.meta) return tx;
      }
      await pause(1000);
    }
    throw Error('FINALIZATION_UNKNOWN_STOP_AND_INSPECT_SUBMITTED_SIGNATURE');
  }
  async function submit(attempt: Attempt, tx: Awaited<ReturnType<Rpc['transaction']>>, skipPreflight: boolean) {
    // Save the deterministic signed transaction's public signature before any
    // broadcast. A timed-out RPC request must not erase an admitted attempt.
    attempt.submittedSignature = tx.signature;
    attempt.submittedWithSkipPreflight = skipPreflight;
    save();
    const returned = await rpc.call<string>('sendTransaction', [Buffer.from(tx.bytes).toString('base64'), {
      encoding: 'base64', skipPreflight, preflightCommitment: 'confirmed', maxRetries: 3,
    }]);
    check(returned === tx.signature, 'RPC_RETURNED_UNEXPECTED_SIGNATURE');
    const landed = await finalized(tx.signature);
    attempt.finalizedAt = now();
    attempt.slot = landed.slot;
    attempt.chainError = landed.meta.err;
    if (!landed.meta.err) attempt.transactionSignature = tx.signature;
    save();
    return landed;
  }

  const initial = await snapshot();
  check(initial.policy.active, 'FIXTURE_ALREADY_REVOKED_USE_A_FRESH_BOOTSTRAP_FIXTURE');
  check(initial.policy.spent === 0n && initial.policy.nonce === 0n, 'FIXTURE_ALREADY_USED_USE_A_FRESH_BOOTSTRAP_FIXTURE');
  check(initial.policy.expiresAt > BigInt(Math.floor(Date.now() / 1000) + 600), 'FIXTURE_EXPIRY_TOO_CLOSE_USE_A_FRESH_BOOTSTRAP_FIXTURE');
  check(initial.policy.maxAmount === 100n * UNIT && initial.policy.totalLimit === 150n * UNIT, 'FIXTURE_LIMITS_MUST_BE_100_AND_150');
  check(initial.policy.minRate === 990n && initial.policy.allowedProgram === TOKEN, 'FIXTURE_ROUTE_OR_MIN_RATE_MISMATCH');
  check(initial.policy.owner === owner.publicKey && initial.policy.agent === agent.publicKey && initial.policy.executor === executor.publicKey && initial.policy.recipient === owner.publicKey && initial.policy.pool === cfg.pool, 'FIXTURE_POLICY_ADDRESSES_MISMATCH');
  check(initial.balances.vault === 150n * UNIT && initial.balances.poolOutput >= 150n * UNIT * RATE, 'FIXTURE_BALANCES_INSUFFICIENT_OR_NOT_FRESH');

  async function swap(scenario: string, amount: bigint, options: {
    expected?: ExpectedError; recipient?: string; nonce?: bigint; signer?: Key; executionSigner?: Key; extra?: Instruction[];
    policy?: string; vault?: string; minOutput?: bigint;
  } = {}) {
    const before = await snapshot(options.policy, options.vault);
    const signingAgent = options.signer || agent;
    const signingExecutor = options.executionSigner || executor;
    const recipient = options.recipient || cfg.recipient;
    const nonce = options.nonce ?? before.policy.nonce;
    const expected = options.expected;
    const minOutput = options.minOutput ?? amount * 990n;
    const attempt: Attempt = {
      id: report.attempts.length + 1, scenario, timestamp: now(), agent: signingAgent.publicKey, actor: signingAgent.publicKey,
      requestedAction: { type: 'execute_swap', amountBaseUnits: amount, minOutputBaseUnits: minOutput, recipientTokenAccount: recipient, executor: signingExecutor.publicKey, nonce },
      policyEvaluated: policyView(before), result: 'PENDING', reason: 'EVALUATION_PENDING', expectedError: expected,
      stateBefore: publicState(before),
    };
    report.attempts.push(attempt); save();
    const instruction = swapIx({ ...cfg, policy: before.policyAddress, vault: before.vaultAddress, agent: signingAgent.publicKey, executor: signingExecutor.publicKey, recipient }, amount, minOutput, nonce, cfg.programId);
    const tx = await rpc.transaction(executor, [signingAgent, signingExecutor], [instruction, ...(options.extra || [])]);
    const sim = await rpc.simulate(tx.bytes);
    attempt.simulation = { result: sim.value.err ? 'DENY' : 'ALLOW', reason: sim.value.err ? expected?.reason || 'UNEXPECTED_REJECTION' : 'POLICY_SATISFIED', error: sim.value.err };
    attempt.result = sim.value.err ? 'DENY' : 'ALLOW';
    attempt.reason = attempt.simulation.reason; save();
    if (expected) expectError(sim.value.err, sim.value.logs, expected, 'SIMULATION');
    else check(!sim.value.err, 'EXPECTED_ALLOW_BUT_SIMULATION_REJECTED');
    // Every expected denial is deliberately broadcast with preflight disabled
    // to prove that enforcement survives a hostile caller bypassing the SDK.
    const landed = await submit(attempt, tx, !!expected);
    const after = await snapshot(options.policy, options.vault); attempt.stateAfter = publicState(after); save();
    if (expected) {
      expectError(landed.meta.err, landed.meta.logMessages, expected, 'FINALIZED');
      unchanged(before, after);
      check(!attempt.transactionSignature, 'DENIED_TRANSACTION_HAS_EXECUTED_SIGNATURE');
    } else {
      check(!landed.meta.err, 'EXPECTED_ALLOW_BUT_CHAIN_REJECTED');
      success(before, after, amount);
    }
    attempt.assertionsPassed = true; save();
    console.log('PASS', scenario, expected ? 'DENY' : 'ALLOW', 'finalized', tx.signature);
  }
  const error = (number: number, code: string, reason: string): ExpectedError => ({ number, code, reason });
  await swap('Approved transfer within limits', 60n * UNIT);
  await swap('Over maximum transaction amount by one base unit', 100n * UNIT + 1n, { expected: error(6005, 'MaxTransactionExceeded', 'MAX_TRANSACTION_EXCEEDED') });
  await swap('Over cumulative limit by one base unit', 90n * UNIT + 1n, { expected: error(6006, 'CumulativeLimitExceeded', 'CUMULATIVE_LIMIT_EXCEEDED') });
  await swap('Unauthorized recipient', UNIT, { recipient: cfg.attacker, expected: error(6008, 'RecipientNotAllowed', 'RECIPIENT_NOT_ALLOWED') });
  await swap('Compromised agent attempts to divert funds', 60n * UNIT, { recipient: cfg.attacker, expected: error(6008, 'RecipientNotAllowed', 'RECIPIENT_NOT_ALLOWED') });
  await swap('Stale nonce replay', UNIT, { nonce: 0n, expected: error(6003, 'StaleNonce', 'STALE_NONCE') });
  await swap('Unapproved agent with a valid signature', UNIT, { signer: outsider, expected: error(2001, 'ConstraintHasOne', 'AGENT_NOT_AUTHORIZED') });
  await swap('Unapproved executor with a valid signature', UNIT, { executionSigner: outsider, expected: error(2001, 'ConstraintHasOne', 'EXECUTOR_NOT_AUTHORIZED') });
  await swap('Agent attempts to weaken minimum output', UNIT, { minOutput: 990n * UNIT - 1n, expected: error(6009, 'SlippageExceeded', 'SLIPPAGE_EXCEEDED') });
  await swap('Pool cannot satisfy requested minimum output', UNIT, { minOutput: RATE * UNIT + 1n, expected: error(6009, 'SlippageExceeded', 'SLIPPAGE_EXCEEDED') });
  await swap('Exact cumulative limit succeeds', 90n * UNIT);

  async function supplementalPolicy(label: string, allowedProgram: string, expiresAt: bigint) {
    const testAgent = generateKey(), testVault = generateKey();
    const policy = policyAddress(owner.publicKey, testAgent.publicKey, cfg.programId);
    const before = await snapshot();
    const requestedAction = { type: 'create_policy_and_fund_test_vault', policy, agent: testAgent.publicKey, vault: testVault.publicKey, allowedProgram, expiresAt, amountBaseUnits: 2n * UNIT };
    const attempt: Attempt = {
      id: report.attempts.length + 1, scenario: label, timestamp: now(), agent: testAgent.publicKey, actor: owner.publicKey,
      requestedAction, policyEvaluated: { address: policy, owner: owner.publicKey, executor: executor.publicKey, allowedProgram, expiresAt },
      result: 'PENDING', reason: 'EVALUATION_PENDING', stateBefore: { policy: null, mainFixture: publicState(before) },
    };
    report.attempts.push(attempt); save();
    const rent = await rpc.call<number>('getMinimumBalanceForRentExemption', [165]);
    const setup = await rpc.transaction(owner, [testVault, outsider], [
      createPolicyIx({ owner: owner.publicKey, agent: testAgent.publicKey, executor: executor.publicKey, recipient: owner.publicKey, pool: cfg.pool, maxAmount: 100n * UNIT, totalLimit: 150n * UNIT, expiresAt, minRate: 990n, allowedProgram }, cfg.programId),
      systemCreate(owner.publicKey, testVault.publicKey, BigInt(rent), 165n, TOKEN),
      initializeToken(testVault.publicKey, cfg.quoteMint, policy),
      mintTo(cfg.quoteMint, testVault.publicKey, outsider.publicKey, 2n * UNIT),
    ]);
    const sim = await rpc.simulate(setup.bytes);
    attempt.simulation = { result: sim.value.err ? 'DENY' : 'ALLOW', reason: sim.value.err ? 'FIXTURE_SETUP_REJECTED' : 'OWNER_AUTHORIZED', error: sim.value.err };
    attempt.result = attempt.simulation.result; attempt.reason = attempt.simulation.reason; save();
    check(!sim.value.err, 'SUPPLEMENTAL_POLICY_SIMULATION_REJECTED');
    const landed = await submit(attempt, setup, false);
    check(!landed.meta.err, 'SUPPLEMENTAL_POLICY_CHAIN_REJECTED');
    const created = await snapshot(policy, testVault.publicKey); attempt.stateAfter = publicState(created); save();
    check(created.policy.active && created.policy.spent === 0n && created.policy.nonce === 0n && created.policy.allowedProgram === allowedProgram && created.policy.expiresAt === expiresAt, 'SUPPLEMENTAL_POLICY_FIELDS_MISMATCH');
    check(created.balances.vault === 2n * UNIT, 'SUPPLEMENTAL_VAULT_FUNDING_MISMATCH');
    unchanged(before, await snapshot());
    attempt.assertionsPassed = true; save();
    console.log('PASS', label, 'ALLOW finalized', setup.signature);
    return { policy, signer: testAgent, vault: testVault.publicKey };
  }
  const blocked = await supplementalPolicy('Owner creates policy with no allowed programs', SYSTEM, BigInt(Math.floor(Date.now() / 1000) + 600));
  await swap('Policy forbids the SPL Token program', UNIT, { ...blocked, expected: error(6007, 'ProgramNotAllowed', 'PROGRAM_NOT_ALLOWED') });
  const expiry = BigInt(Math.floor(Date.now() / 1000) + 45);
  const expiring = await supplementalPolicy('Owner creates expiring policy', TOKEN, expiry);
  // Compare to finalized chain time, rather than trusting the machine clock.
  const expiryDeadline = Date.now() + 120_000;
  while (true) {
    const slot = await rpc.call<number>('getSlot', [{ commitment: 'finalized' }]);
    const timestamp = await rpc.call<number | null>('getBlockTime', [slot]);
    if (timestamp !== null && BigInt(timestamp) >= expiry) break;
    check(Date.now() < expiryDeadline, 'CHAIN_TIME_DID_NOT_REACH_POLICY_EXPIRY');
    await pause(1000);
  }
  await swap('Expired agent policy cannot spend', UNIT, { ...expiring, expected: error(6002, 'Expired', 'EXPIRED') });

  const beforeRevoke = await snapshot();
  const revokeAttempt: Attempt = {
    id: report.attempts.length + 1, scenario: 'Owner revokes agent', timestamp: now(), agent: agent.publicKey, actor: owner.publicKey,
    requestedAction: { type: 'revoke', policy: cfg.policy }, policyEvaluated: policyView(beforeRevoke),
    result: 'PENDING', reason: 'EVALUATION_PENDING', stateBefore: publicState(beforeRevoke),
  };
  report.attempts.push(revokeAttempt); save();
  const revoke = await rpc.transaction(owner, [], [revokeIx(owner.publicKey, cfg.policy, cfg.programId)]);
  const revokeSim = await rpc.simulate(revoke.bytes);
  revokeAttempt.simulation = { result: revokeSim.value.err ? 'DENY' : 'ALLOW', reason: revokeSim.value.err ? 'OWNER_REVOKE_REJECTED' : 'OWNER_AUTHORIZED', error: revokeSim.value.err };
  revokeAttempt.result = revokeAttempt.simulation.result; revokeAttempt.reason = revokeAttempt.simulation.reason; save();
  check(!revokeSim.value.err, 'OWNER_REVOKE_SIMULATION_REJECTED');
  const revoked = await submit(revokeAttempt, revoke, false);
  check(!revoked.meta.err, 'OWNER_REVOKE_CHAIN_REJECTED');
  const afterRevoke = await snapshot(); revokeAttempt.stateAfter = publicState(afterRevoke); save();
  check(!afterRevoke.policy.active, 'OWNER_REVOKE_DID_NOT_DEACTIVATE_POLICY');
  check(json({ ...beforeRevoke.policy, active: false }) === json(afterRevoke.policy), 'OWNER_REVOKE_CHANGED_OTHER_POLICY_FIELDS');
  check(json(beforeRevoke.balances) === json(afterRevoke.balances), 'OWNER_REVOKE_MOVED_TOKENS');
  revokeAttempt.assertionsPassed = true; save();
  console.log('PASS Owner revokes agent ALLOW finalized', revoke.signature);
  await swap('Revoked agent cannot spend', UNIT, { expected: error(6001, 'Revoked', 'REVOKED') });
  await swap('Revoked agent cannot divert funds', UNIT, { recipient: cfg.attacker, expected: error(6001, 'Revoked', 'REVOKED') });
  report.status = 'PASSED'; report.finishedAt = now(); save();
  console.log(`Verified ${report.attempts.length} finalized attempts. Public-safe report: ${reportFile}`);
}

try { save(); await run(); }
catch (error) {
  const message = error instanceof Error ? error.message : '';
  // Raw provider errors can contain credential-bearing URLs. Store only our
  // controlled assertion codes, never remote error strings or runtime config.
  report.failure = /^[A-Z][A-Z0-9_]{1,160}$/.test(message) ? message : 'RPC_OR_LOCAL_OPERATION_FAILED';
  report.status = 'FAILED'; report.finishedAt = now(); save();
  console.error(report.failure + '. Inspect submitted signatures in ' + reportFile + ' before retrying. A completed suite revokes its fixture.');
  process.exitCode = 1;
}
