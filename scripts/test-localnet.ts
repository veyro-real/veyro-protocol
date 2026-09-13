import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, openSync, closeSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { Rpc, PROGRAM_ID, generateKey, exportKey } from '../packages/sdk/src/index.js';

// Each verification gets isolated fixture keys and a ledger. No existing wallet,
// policy, ledger or cumulative spend is reused or reset.
const runDir = resolve('target', 'verification-' + Date.now());
mkdirSync(runDir, { recursive: true, mode: 0o700 });
const keysDir = resolve(runDir, 'keys');
mkdirSync(keysDir, { mode: 0o700 });
const validatorKey = generateKey();
writeFileSync(resolve(keysDir, 'validator.json'), JSON.stringify(exportKey(validatorKey)), { mode: 0o600 });
const rpcUrl = 'http://127.0.0.1:18999';
const env = { ...process.env, SOLANA_RPC_URL: rpcUrl, VEYRO_KEYS_DIR: keysDir, VEYRO_PROGRAM_ID: PROGRAM_ID,
  VEYRO_DEPLOYMENT_FILE: resolve(runDir, 'deployment.json'), VEYRO_REPORT_FILE: resolve(runDir, 'report.json') };
function run(command: string, args: string[], childEnv = process.env): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: 'inherit', env: childEnv });
    child.once('error', reject);
    child.once('exit', code => code === 0 ? resolve() : reject(Error(command + ' failed: ' + code)));
  });
}
let validator: ChildProcess | undefined;
let log: number | undefined;
async function stop() {
  if (validator && validator.exitCode === null) {
    const child = validator;
    await new Promise<void>(resolve => {
      const timer = setTimeout(() => child.kill('SIGKILL'), 5000);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
      child.kill('SIGTERM');
    });
  }
  if (log !== undefined) { closeSync(log); log = undefined; }
}
try {
  await run('cargo', ['build-sbf', '--manifest-path', 'programs/veyro/Cargo.toml']);
  log = openSync(resolve(runDir, 'validator.log'), 'w');
  validator = spawn('solana-test-validator', ['--ledger', resolve(runDir, 'ledger'), '--bind-address', '127.0.0.1',
    '--rpc-port', '18999', '--faucet-port', '19900', '--gossip-port', '19001', '--dynamic-port-range', '19002-19102',
    '--mint', validatorKey.publicKey, '--bpf-program', PROGRAM_ID, resolve('target/deploy/veyro.so'), '--quiet'],
    { stdio: ['ignore', log, log] });
  let startError: Error | undefined;
  validator.once('error', error => { startError = error; });
  let ready = false;
  for (let i = 0; i < 60; i++) {
    if (startError) throw startError;
    if (validator.exitCode !== null) throw Error('Validator stopped; inspect ' + runDir + '/validator.log');
    try {
      const rpc = new Rpc(rpcUrl);
      if (await rpc.call('getHealth') === 'ok' && (await rpc.call('getBalance', [validatorKey.publicKey])).value > 0) {
        ready = true; break; // Unique mint proves this is our newly-created ledger.
      }
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  if (!ready) throw Error('Validator did not become ready; inspect ' + runDir + '/validator.log');
  await run(process.execPath, ['--import', 'tsx', 'scripts/bootstrap.ts'], env);
  await run(process.execPath, ['--import', 'tsx', 'scripts/verify-chain.ts'], env);
  console.log('Verification evidence: ' + env.VEYRO_REPORT_FILE);
} finally { await stop(); }
