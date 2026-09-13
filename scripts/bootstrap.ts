import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { Rpc, generateKey, keyFromSecret, exportKey, poolAddress, policyAddress, createPoolIx, createPolicyIx, systemCreate, initializeMint, initializeToken, mintTo, TOKEN, PROGRAM_ID, decodePolicy, type Key, type Instruction } from '../packages/sdk/src/index.js';
const rpc=new Rpc(process.env.SOLANA_RPC_URL||'http://127.0.0.1:8899');const genesis=await rpc.assertTestCluster();
const program=process.env.VEYRO_PROGRAM_ID||PROGRAM_ID;
const programInfo=(await rpc.call('getAccountInfo',[program,{encoding:'base64',commitment:'confirmed'}])).value;
if(!programInfo?.executable)throw Error('Deploy the executable Veyro program before bootstrapping.');
const dir=resolve(process.env.VEYRO_KEYS_DIR||'keys');mkdirSync(dir,{recursive:true,mode:0o700});
function key(name:string):Key {const file=resolve(dir,name+'.json');if(existsSync(file))return keyFromSecret(Uint8Array.from(JSON.parse(readFileSync(file,'utf8'))));const k=generateKey();writeFileSync(file,JSON.stringify(exportKey(k)),{mode:0o600});return k;}
const admin=key('admin'),owner=key('owner'),agent=key('agent'),executor=key('executor');
for(const k of [admin,owner,executor]){const balance=await rpc.call<number>('getBalance',[k.publicKey]).then((v:any)=>v.value);if(balance<100_000_000){const s=await rpc.call<string>('requestAirdrop',[k.publicKey,1_000_000_000]);await rpc.confirm(s);}}
async function send(ix:Instruction[],keys:Key[]=[]){const tx=await rpc.transaction(admin,keys,ix);await rpc.send(tx.bytes);await rpc.confirm(tx.signature);return tx.signature;}
const quote=key('quote-mint'),output=key('output-mint');
for(const mint of [quote,output])if(!(await rpc.account(mint.publicKey))){const rent=await rpc.call<number>('getMinimumBalanceForRentExemption',[82]);await send([systemCreate(admin.publicKey,mint.publicKey,BigInt(rent),82n,TOKEN),initializeMint(mint.publicKey,admin.publicKey)],[mint]);}
const pool=poolAddress(admin.publicKey,output.publicKey,program),policy=policyAddress(owner.publicKey,agent.publicKey,program);
const existingPolicy=await rpc.account(policy);
if(existingPolicy){
 if(existingPolicy.owner!==program)throw Error('Existing policy has an unexpected program owner.');
 const p=decodePolicy(existingPolicy.data);
 if(!p.active || p.expiresAt<=BigInt(Math.floor(Date.now()/1000)) || p.spent!==0n || p.nonce!==0n)throw Error('Fixture policy was already used, expired or revoked. Choose a fresh VEYRO_KEYS_DIR and VEYRO_DEPLOYMENT_FILE; existing spending limits are never reset.');
}
if(!(await rpc.account(pool)))await send([createPoolIx(admin.publicKey,quote.publicKey,output.publicKey,1000n,program)]);
const accounts:Record<string,string>={};
for(const [name,mint,authority,funding] of [['vault',quote.publicKey,policy,150_000_000n],['pool-quote',quote.publicKey,pool,0n],['pool-output',output.publicKey,pool,1_000_000_000_000n],['recipient',output.publicKey,owner.publicKey,0n],['attacker',output.publicKey,key('attacker-owner').publicKey,0n],['owner-quote',quote.publicKey,owner.publicKey,0n]] as const){const k=key(name);accounts[name]=k.publicKey;if(!(await rpc.account(k.publicKey))){const rent=await rpc.call<number>('getMinimumBalanceForRentExemption',[165]);const ix=[systemCreate(admin.publicKey,k.publicKey,BigInt(rent),165n,TOKEN),initializeToken(k.publicKey,mint,authority)];if(funding)ix.push(mintTo(mint,k.publicKey,admin.publicKey,funding));await send(ix,[k]);}}
if(!(await rpc.account(policy))){const tx=await rpc.transaction(owner,[],[createPolicyIx({owner:owner.publicKey,agent:agent.publicKey,executor:executor.publicKey,recipient:owner.publicKey,pool,maxAmount:100_000_000n,totalLimit:150_000_000n,expiresAt:BigInt(Math.floor(Date.now()/1000)+86400),minRate:990n},program)]);await rpc.send(tx.bytes);await rpc.confirm(tx.signature);}
const deployment={cluster:genesis==='4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY'?'testnet':'localnet',rpcUrl:rpc.endpoint,programId:program,pool,policy,owner:owner.publicKey,agent:agent.publicKey,executor:executor.publicKey,quoteMint:quote.publicKey,outputMint:output.publicKey,vault:accounts.vault,poolQuote:accounts['pool-quote'],poolOutput:accounts['pool-output'],recipient:accounts.recipient,attacker:accounts.attacker,ownerQuote:accounts['owner-quote'],rate:'1000',quoteLabel:'TEST-USD',outputLabel:'TEST-MEME',officialUsdc:false};
const deploymentFile=resolve(process.env.VEYRO_DEPLOYMENT_FILE||'deployment.local.json');
writeFileSync(deploymentFile,JSON.stringify(deployment,null,2)+'\n',{mode:0o600});
console.log('Test-token pool and unused policy ready. Configuration saved to '+deploymentFile+'. Private keys remain in the configured keys directory.');
