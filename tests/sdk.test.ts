import test from 'node:test';import assert from 'node:assert/strict';
import {ed25519} from '@noble/curves/ed25519';
import {generateKey,address,compile,signTransaction,systemCreate,SYSTEM,TOKEN,policyAddress,livePolicyAddress,pda,utf8,u64,i64,u32,decodePolicy,createPolicyIx,createLivePolicyIx,executeRouteIx,evaluateTransaction,PROGRAM_ID,JUPITER_V6,USDC,meta,Rpc} from '../packages/sdk/src/index.js';
import {parseIntent,pickCandidate,FIXTURES,previewPolicy} from '../packages/core/src/index.js';
test('signatures bind the full serialized message',()=>{const a=generateKey(),b=generateKey();const c=compile(a.publicKey,SYSTEM,[systemCreate(a.publicKey,b.publicKey,1n,0n,SYSTEM)]);const tx=signTransaction(c,[a,b]);assert.equal(tx.bytes[0],2);assert.ok(ed25519.verify(tx.bytes.slice(1,65),c.message,address(a.publicKey)));const altered=c.message.slice();altered[10]^=1;assert.equal(ed25519.verify(tx.bytes.slice(1,65),altered,address(a.publicKey)),false);assert.throws(()=>signTransaction(c,[a]),/MISSING_SIGNER/);});
test('PDA is deterministic and off curve',()=>{const a=generateKey(),b=generateKey();const p=policyAddress(a.publicKey,b.publicKey);assert.equal(p,policyAddress(a.publicKey,b.publicKey));assert.throws(()=>ed25519.ExtendedPoint.fromHex(address(p)));assert.notEqual(p,policyAddress(b.publicKey,a.publicKey));assert.throws(()=>pda([new Uint8Array(33)]));});
test('PDA matches independent Solana CLI vectors including skipped curve points',()=>{
 // Generated with solana find-program-derived-address 4.2.0, using public constant seeds.
 assert.deepEqual(pda([utf8('policy'),address(SYSTEM),address(TOKEN)]),['bN3EJHcAD1tUk3w15FeY5NqJ7RSefZuuHRD4cucJnkS',255]);
 assert.deepEqual(pda([utf8('policy'),address(TOKEN),address(SYSTEM)]),['GoN5DHch9dmshaMNnRdiSdV9QCd9fbZ8PzBzEZGXmwe1',252]);
});
test('integer amounts cannot silently wrap',()=>{assert.throws(()=>u64(-1n));assert.throws(()=>u64(2n**64n));assert.equal(u64(100n)[0],100);assert.throws(()=>decodePolicy(new Uint8Array(250)));});
test('signed timestamps and instruction integers reject wrapping and fractional input',()=>{
 for(const value of [-(2n**63n)-1n,2n**63n])assert.throws(()=>i64(value),/INVALID_I64/);
 for(const value of [-(2n**63n),-1n,0n,2n**63n-1n])assert.equal(new DataView(i64(value).buffer).getBigInt64(0,true),value);
 for(const value of [-1,2**32,0.5,NaN,Infinity])assert.throws(()=>u32(value),/INVALID_U32/);
 assert.equal(new DataView(u32(0xffffffff).buffer).getUint32(0,true),0xffffffff);
});
test('cluster checks reject mainnet through local proxies and accept IPv6 local validators',async()=>{
 const mainnet='5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d';
 for(const endpoint of ['https://api.mainnet-beta.solana.com','http://localhost:8899','http://127.0.0.1:8899','http://[::1]:8899']){
  const rpc=new Rpc(endpoint);rpc.call=async<T>()=>mainnet as T;
  await assert.rejects(rpc.assertTestCluster(),/MAINNET_DISABLED/);
  assert.equal(await rpc.assertMainnet(),mainnet);
 }
 const ipv6=new Rpc('http://[::1]:8899');ipv6.call=async<T>()=>'local-validator-genesis' as T;
 assert.equal(await ipv6.assertTestCluster(),'local-validator-genesis');
 const remote=new Rpc('https://rpc.example.com');remote.call=async<T>()=>'unknown-genesis' as T;
 await assert.rejects(remote.assertTestCluster(),/EXPECTED_SOLANA_TESTNET/);
 remote.call=async<T>()=>'4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY' as T;
 assert.equal(await remote.assertTestCluster(),'4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY');
 await assert.rejects(remote.assertMainnet(),/EXPECTED_SOLANA_MAINNET/);
});
test('broadcast checks the cluster before sending even when supplied pre-signed bytes',async()=>{
 const calls:string[]=[];let hash='5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d';
 const rpc=new Rpc('http://127.0.0.1:8899');
 rpc.call=async<T>(method:string)=>{calls.push(method);return (method==='getGenesisHash'?hash:'signature') as T;};
 await assert.rejects(rpc.send(new Uint8Array()),/MAINNET_DISABLED/);
 assert.deepEqual(calls,['getGenesisHash']);
 calls.length=0;hash='local-validator-genesis';
 assert.equal(await rpc.send(new Uint8Array()),'signature');
 assert.deepEqual(calls,['getGenesisHash','sendTransaction']);
});
test('intent budget is exact and not a promised return',()=>{assert.equal(parseIntent('make me a million','100').budgetMicros,'100000000');assert.equal(parseIntent('buy','0.000001').budgetMicros,'1');assert.throws(()=>parseIntent('buy','-1'));assert.throws(()=>parseIntent('buy','1e3'));assert.equal(pickCandidate(FIXTURES).symbol,'BREAD');assert.throws(()=>pickCandidate([{...FIXTURES[0],source:'x'}]),/NO_VERIFIED/);});
test('preview boundaries and allowlists fail closed',()=>{const p={active:true,expiresAt:100,maxAmount:'100',totalLimit:'150',spent:'60',allowedRecipients:['owner','treasury'],allowedPrograms:[TOKEN]};assert.equal(previewPolicy(p,{amount:'90',recipient:'owner',program:TOKEN},99),'POLICY_SATISFIED');assert.equal(previewPolicy(p,{amount:'91',recipient:'owner',program:TOKEN},99),'CUMULATIVE_LIMIT_EXCEEDED');assert.equal(previewPolicy({...p,active:false},{amount:'1',recipient:'owner',program:TOKEN},99),'REVOKED');assert.equal(previewPolicy(p,{amount:'1',recipient:'attacker',program:TOKEN},99),'RECIPIENT_NOT_ALLOWED');assert.equal(previewPolicy(p,{amount:'1',recipient:'owner',program:SYSTEM},99),'PROGRAM_NOT_ALLOWED');assert.equal(previewPolicy(p,{amount:'1',recipient:'owner',program:TOKEN},100),'EXPIRED');});
test('policy instruction accepts bounded unique allowlists',()=>{const owner=generateKey(),agent=generateKey(),executor=generateKey(),pool=generateKey();const args={owner:owner.publicKey,agent:agent.publicKey,executor:executor.publicKey,pool:pool.publicKey,maxAmount:1n,totalLimit:2n,expiresAt:3n,minRate:1n};assert.ok(createPolicyIx({...args,allowedRecipients:[owner.publicKey],allowedPrograms:[TOKEN]}).data.length>100);assert.throws(()=>createPolicyIx({...args,allowedRecipients:Array(9).fill(owner.publicKey)}),/INVALID_POLICY_ALLOWLIST/);assert.throws(()=>createPolicyIx({...args,allowedRecipients:[owner.publicKey,owner.publicKey]}),/INVALID_POLICY_ALLOWLIST/);});
test('live policy and routed swap bind owner limits to the CPI instruction',()=>{const owner=generateKey(),agent=generateKey(),executor=generateKey(),vault=generateKey(),destination=generateKey();const policy=livePolicyAddress(owner.publicKey,agent.publicKey);const create=createLivePolicyIx({owner:owner.publicKey,agent:agent.publicKey,executor:executor.publicKey,quoteMint:USDC,allowedRecipients:[owner.publicKey],maxAmount:100_000_000n,totalLimit:150_000_000n,expiresAt:2_000_000_000n,allowedPrograms:[JUPITER_V6]});assert.equal(create.accounts[1].address,policy);const route={program:JUPITER_V6,accounts:[meta(policy,true),meta(vault.publicKey,false,true),meta(destination.publicKey,false,true)],data:Uint8Array.of(1,2,3)};const wrapped=executeRouteIx({agent:agent.publicKey,executor:executor.publicKey,policy,vault:vault.publicKey,destination:destination.publicKey},route,100_000_000n,1n,0n);assert.equal(wrapped.program,PROGRAM_ID);assert.equal(wrapped.accounts[5].address,JUPITER_V6);assert.deepEqual([...wrapped.data.slice(-7)],[3,0,0,0,1,2,3]);});
test('transaction evaluation never broadcasts and returns a verdict',async()=>{const rpc=new Rpc('http://127.0.0.1:8899');const calls:string[]=[];rpc.call=async<T>(method:string)=>{calls.push(method);return (method==='getGenesisHash'?'local-validator-genesis':{context:{slot:7},value:{err:null,logs:[]}}) as T;};assert.deepEqual(await evaluateTransaction(rpc,new Uint8Array([1])),{decision:'ALLOW',reason:'POLICY_SATISFIED',simulationSlot:7});assert.deepEqual(calls,['getGenesisHash','simulateTransaction']);});
