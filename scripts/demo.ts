import {readFileSync} from 'node:fs';
import {Rpc,keyFromSecret,swapIx,revokeIx,decodePolicy,errorReason} from '../packages/sdk/src/index.js';
const cfg=JSON.parse(readFileSync(process.env.VEYRO_DEPLOYMENT_FILE||'deployment.local.json','utf8'));
const rpc=new Rpc(process.env.SOLANA_RPC_URL||cfg.rpcUrl);await rpc.assertTestCluster();
const key=(name:string)=>keyFromSecret(Uint8Array.from(JSON.parse(readFileSync(`${process.env.VEYRO_KEYS_DIR||'keys'}/${name}.json`,'utf8'))));
const agent=key('agent'),executor=key('executor'),owner=key('owner');
async function attempt(label:string,amount:bigint,recipient=cfg.recipient){const account=await rpc.account(cfg.policy);if(!account)throw Error('POLICY_MISSING');const p=decodePolicy(account.data);const tx=await rpc.transaction(executor,[agent],[swapIx({...cfg,recipient},amount,amount*990n,p.nonce,cfg.programId)]);const sim=await rpc.simulate(tx.bytes);const reason=errorReason(sim.value.logs,sim.value.err);if(sim.value.err){console.log(label,'DENY',reason);return;}await rpc.send(tx.bytes);await rpc.confirm(tx.signature);console.log(label,'ALLOW','FINALIZED',tx.signature);}
await attempt('Approved',60_000_000n);
await attempt('Over transaction cap',100_000_001n);
await attempt('Over cumulative cap',90_000_001n);
await attempt('Unauthorized recipient',1_000_000n,cfg.attacker);
await attempt('Compromised agent',60_000_000n,cfg.attacker);
const revoke=await rpc.transaction(owner,[],[revokeIx(owner.publicKey,cfg.policy,cfg.programId)]);await rpc.send(revoke.bytes);await rpc.confirm(revoke.signature);
await attempt('Revoked',1_000_000n);
