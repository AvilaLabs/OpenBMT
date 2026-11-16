#!/usr/bin/env python3
"""Local regtest-only runner using fresh synthetic funds; see README."""
import json, os, pathlib, secrets, socket, subprocess, time, urllib.request

CORE = pathlib.Path(os.environ.get('BITCOIND', str(pathlib.Path.home()/'.local/opt/bitcoin-core-32.1/bin/bitcoind')))
PROBE = pathlib.Path(os.environ.get('PROBE_BINARY', str(pathlib.Path(__file__).resolve().parent/'target/debug/batch-claim-probe')))
PORT, P2P = int(os.environ.get('PROBE_RPC_PORT','19720')), int(os.environ.get('PROBE_P2P_PORT','19721'))
if PORT == P2P or not all(1024 <= port <= 65535 for port in (PORT,P2P)):
    raise ValueError('probe ports must be distinct unprivileged TCP ports')
for port in (PORT, P2P):
    with socket.socket() as s:
        s.bind(('127.0.0.1', port))
run = pathlib.Path('/tmp') / ('openbmt-batch-claim-' + secrets.token_hex(6))
run.mkdir(mode=0o700)
password = secrets.token_hex(24)
(run/'bitcoin.conf').write_text(f'regtest=1\nserver=1\nfallbackfee=0.00002\ntxindex=1\nlisten=0\ndnsseed=0\n[regtest]\nconnect=0\nrpcport={PORT}\nport={P2P}\nrpcuser=probe\nrpcpassword={password}\nrpcbind=127.0.0.1\nrpcallowip=127.0.0.1\n')
(run/'bitcoin.conf').chmod(0o600)
import base64
AUTH = 'Basic '+base64.b64encode(('probe:'+password).encode()).decode()
def rpc(method, *params, wallet=False):
    path = '/wallet/probe' if wallet else '/'
    req = urllib.request.Request(f'http://127.0.0.1:{PORT}{path}', data=json.dumps({'jsonrpc':'2.0','id':'probe','method':method,'params':params}).encode(), headers={'Authorization':AUTH,'Content-Type':'application/json'})
    with urllib.request.urlopen(req, timeout=15) as response:
        value = json.load(response)
    if value.get('error'): raise RuntimeError(value['error'])
    return value['result']
def probe(*args, is_json=False, should_fail=False):
    p = subprocess.run([str(PROBE), *map(str,args)], text=True, capture_output=True)
    if should_fail:
        assert p.returncode != 0 and not p.stdout.strip(), (p.returncode,p.stdout,p.stderr)
        return {'refused':True,'reason':p.stderr.strip()}
    if p.returncode: raise RuntimeError(p.stderr)
    return json.loads(p.stdout) if is_json else p.stdout.strip()
def fund(address, sats):
    txid = rpc('sendtoaddress',address,f'{sats // 100_000_000}.{sats % 100_000_000:08d}',wallet=True)
    tx = rpc('gettransaction',txid,wallet=True)
    decoded = rpc('decoderawtransaction',tx['hex'])
    output = next(v for v in decoded['vout'] if v['scriptPubKey'].get('address')==address)
    return tx['hex'], output['n'], txid
log = (run/'core.log').open('w')
proc = subprocess.Popen([str(CORE),'-datadir='+str(run),'-nosettings'], stdout=log, stderr=subprocess.STDOUT)
try:
    for _ in range(100):
        if proc.poll() is not None: raise RuntimeError((run/'core.log').read_text())
        try:
            info=rpc('getblockchaininfo')
            assert info['chain']=='regtest'
            break
        except (OSError,urllib.error.URLError): time.sleep(0.2)
    else: raise RuntimeError('Core RPC did not become ready')
    rpc('createwallet','probe')
    miner=rpc('getnewaddress',wallet=True)
    rpc('generatetoaddress',101,miner)
    prepared=probe('prepare',is_json=True)
    inputs=[fund(s['address'],100_000) for s in prepared['swaps']]
    rpc('generatetoaddress',1,miner)
    for _,vout,txid in inputs: assert rpc('gettxout',txid,vout)['confirmations']>=1
    raw=probe('spend',inputs[0][0],inputs[0][1],inputs[1][0],inputs[1][1],1000)
    decoded=rpc('decoderawtransaction',raw)
    allowed=rpc('testmempoolaccept',[raw])[0]
    assert allowed['allowed'],allowed
    sig=decoded['vin'][0]['txinwitness'][0]
    mutated_sig=('01' if sig[:2]=='00' else '00')+sig[2:]
    mutated=raw.replace(sig,mutated_sig,1)
    negative=rpc('testmempoolaccept',[mutated])[0]
    assert not negative['allowed']
    absent=probe('spend','00',0,inputs[1][0],inputs[1][1],1000,should_fail=True)
    bad_script=probe('spend',inputs[1][0],inputs[1][1],inputs[0][0],inputs[0][1],1000,should_fail=True)
    txid=rpc('sendrawtransaction',raw)
    rpc('generatetoaddress',1,miner)
    assert len(decoded['vin'])==2 and len(decoded['vout'])==1
    assert rpc('gettxout',txid,0)['confirmations']>=1
    assert all(rpc('gettxout',p[2],p[1]) is None for p in inputs)
    evidence={'purpose':'synthetic Bitcoin-consensus probes; no service or application integration','core_version':rpc('getnetworkinfo')['subversion'],'chain':'regtest','base':{'input_count':2,'output_count':1,'input_sats':200000,'cold_output_sats':199000,'fee_sats':1000,'vsize':decoded['vsize'],'txid':txid,'confirmed':True,'valid_mempool':allowed,'altered_signature_rejected':negative,'absent_funding_rejected':absent,'wrong_script_rejected':bad_script},'run_directory':str(run)}
    (run/'base-result.json').write_text(json.dumps(evidence,indent=2)+'\n')
    print(json.dumps({'base_complete':True,'run_directory':str(run),'vsize':decoded['vsize']}),flush=True)
    cold=fund(prepared['coldInputAddress'],300_000)
    rpc('generatetoaddress',1,miner)
    coupon=probe('coupon',cold[0],cold[1],is_json=True)
    assert all(coupon['negativeSameSignatureChecks'].values())
    # The pre-authorization exists before these two claim outpoints exist.
    later=[fund(s['address'],100_000) for s in prepared['swaps']]
    rpc('generatetoaddress',1,miner)
    for _,vout,parent in [cold,*later]: assert rpc('gettxout',parent,vout)['confirmations']>=1
    combined=probe('assemble',coupon['signedCouponHex'],cold[0],cold[1],later[0][0],later[0][1],later[1][0],later[1][1])
    combined_decoded=rpc('decoderawtransaction',combined)
    old_sig=rpc('decoderawtransaction',coupon['signedCouponHex'])['vin'][0]['txinwitness'][0]
    assert combined_decoded['vin'][0]['txinwitness'][0]==old_sig
    combined_allowed=rpc('testmempoolaccept',[combined])[0]
    assert combined_allowed['allowed'],combined_allowed
    combined_id=rpc('sendrawtransaction',combined)
    rpc('generatetoaddress',1,miner)
    assert len(combined_decoded['vin'])==3 and len(combined_decoded['vout'])==1
    assert rpc('gettxout',combined_id,0)['confirmations']>=1
    assert all(rpc('gettxout',p[2],p[1]) is None for p in [cold,*later])
    evidence['limited_cold_authorization']={'input_count':3,'output_count':1,'existing_cold_input_sats':300000,'claim_input_sats':[100000,100000],'fixed_cold_output_sats':490000,'fee_sats':10000,'vsize':combined_decoded['vsize'],'txid':combined_id,'confirmed':True,'original_cold_signature_unchanged':True,'coupon_created_before_claim_outpoints':True,'valid_mempool':combined_allowed,'negative_same_signature_checks':coupon['negativeSameSignatureChecks'],'limits':'One known cold outpoint only; fixture signing, no hardware or service integration, no crash/timeout/fee-replacement proof.'}
    print(json.dumps({'coupon_complete':True,'vsize':combined_decoded['vsize']}),flush=True)
    (run/'result.json').write_text(json.dumps(evidence,indent=2)+'\n')
finally:
    if proc.poll() is None:
        try: rpc('stop')
        except Exception: proc.terminate()
    try: proc.wait(timeout=10)
    except subprocess.TimeoutExpired: proc.kill();proc.wait(timeout=5)
    log.close()
