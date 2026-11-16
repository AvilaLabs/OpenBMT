#!/usr/bin/env python3
"""Synthetic Bitcoin Core regtest test of ordinary-payment UTXO batching."""
import base64, hashlib, hmac, json, os, pathlib, secrets, socket, subprocess, time, urllib.error, urllib.request
from decimal import Decimal

CORE = pathlib.Path(os.environ.get('BITCOIND', str(pathlib.Path.home()/'.local/opt/bitcoin-core-32.1/bin/bitcoind')))
RPC, P2P = int(os.environ.get('PROBE_RPC_PORT', '19730')), int(os.environ.get('PROBE_P2P_PORT', '19731'))
if RPC == P2P or any(not 1024 <= p <= 65535 for p in (RPC, P2P)):
    raise ValueError('ports must be distinct unprivileged TCP ports')
for port in (RPC, P2P):
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', port))
run = pathlib.Path('/tmp') / ('openbmt-onchain-batch-' + secrets.token_hex(6))
run.mkdir(mode=0o700)
password, salt = secrets.token_hex(24), secrets.token_hex(16)
rpcauth = hmac.new(salt.encode(), password.encode(), hashlib.sha256).hexdigest()
(run/'bitcoin.conf').write_text(f'regtest=1\nserver=1\nfallbackfee=0.00002\nlisten=0\ndnsseed=0\n[regtest]\nconnect=0\nrpcport={RPC}\nport={P2P}\nrpcauth=probe:{salt}${rpcauth}\nrpcbind=127.0.0.1\nrpcallowip=127.0.0.1\n')
(run/'bitcoin.conf').chmod(0o600)
AUTH = 'Basic ' + base64.b64encode(('probe:'+password).encode()).decode()

class RpcError(RuntimeError):
    def __init__(self, method, error):
        self.code = error.get('code')
        super().__init__(f'{method}: {error}')

def rpc(method, *params, wallet=None):
    path = '/wallet/' + wallet if wallet else '/'
    body = json.dumps({'jsonrpc':'2.0','id':'probe','method':method,'params':params}, default=str).encode()
    req = urllib.request.Request(f'http://127.0.0.1:{RPC}{path}', data=body, headers={'Authorization':AUTH,'Content-Type':'application/json'})
    with urllib.request.urlopen(req, timeout=30) as response:
        result = json.load(response, parse_float=Decimal)
    if result.get('error'):
        raise RpcError(method, result['error'])
    return result['result']
def btc(sats):
    return f'{sats // 100_000_000}.{sats % 100_000_000:08d}'
def exact_sats(value):
    sats = Decimal(str(value)) * 100_000_000
    assert sats == sats.to_integral_value(), 'non-integral sat amount'
    return int(sats)
log = (run/'core.log').open('w')
proc = subprocess.Popen([str(CORE), '-datadir='+str(run), '-nosettings'], stdout=log, stderr=subprocess.STDOUT)
try:
    for _ in range(120):
        if proc.poll() is not None:
            raise RuntimeError((run/'core.log').read_text())
        try:
            info = rpc('getblockchaininfo')
            assert info['chain'] == 'regtest', info['chain']
            break
        except (OSError, urllib.error.URLError):
            time.sleep(.25)
    else:
        raise RuntimeError('Core RPC did not become ready')
    core_version = rpc('getnetworkinfo')['subversion']
    rpc('createwallet', 'payer')
    rpc('createwallet', 'merchant_hot', False, False, '', False, True)
    rpc('createwallet', 'merchant_savings', True, True, '', False, True)
    payer, hot = 'payer', 'merchant_hot'
    miner = rpc('getnewaddress', '', 'bech32', wallet=payer)
    rpc('generatetoaddress', 101, miner)
    savings_address = 'bcrt1pngylwuvf9ud79em6cvp0lzx48t7ujn36670kdfsxt08ngvmc59xs3w9zvk'
    desc = rpc('getdescriptorinfo', f'addr({savings_address})')['descriptor']
    imported = rpc('importdescriptors', [{'desc':desc,'timestamp':'now','label':'synthetic-public-fixture'}], wallet='merchant_savings')
    assert imported[0]['success'], imported
    assert not rpc('listunspent', 0, 9999999, [], True, wallet=hot)
    assert not rpc('listunspent', 0, 9999999, [], True, wallet='merchant_savings')
    hot_addresses = [rpc('getnewaddress', '', 'bech32', wallet=hot) for _ in range(100)]
    payment_txids = []
    for index, address in enumerate(hot_addresses, 1):
        payment_txids.append(rpc('sendtoaddress', address, btc(10_000), wallet=payer))
        if index % 10 == 0:
            rpc('generatetoaddress', 1, miner)
    assert len(set(payment_txids)) == 100
    unspent = rpc('listunspent', 1, 9999999, [], True, wallet=hot)
    assert len(unspent) == 100 and sum(exact_sats(u['amount']) for u in unspent) == 1_000_000
    plans = []
    signed_raw = {}
    for rate in (1, 10, 100):
        options = {'add_inputs':False,'subtractFeeFromOutputs':[0],'replaceable':True,'fee_rate':rate}
        funded = rpc('walletcreatefundedpsbt', [{'txid':u['txid'],'vout':u['vout']} for u in unspent], [{savings_address:btc(1_000_000)}], 0, options, True, wallet=hot)
        psbt = rpc('walletprocesspsbt', funded['psbt'], True, 'ALL', True, wallet=hot)
        final = rpc('finalizepsbt', psbt['psbt'], True)
        assert final['complete'], f'PSBT signing incomplete at {rate} sat/vB'
        decoded = rpc('decoderawtransaction', final['hex'])
        accepted = rpc('testmempoolaccept', [final['hex']])[0]
        fee = exact_sats(funded['fee'])
        plans.append({'fee_rate_sat_vb':rate,'input_count':len(decoded['vin']),'output_count':len(decoded['vout']),'vsize':decoded['vsize'],'fee_sats':fee,'cold_output_sats':exact_sats(decoded['vout'][0]['value']),'mempool_allowed':accepted['allowed'],'txid':decoded['txid']})
        assert len(decoded['vin']) == 100 and len(decoded['vout']) == 1 and accepted['allowed'], accepted
        signed_raw[rate] = final['hex']
    assert all(p['cold_output_sats'] + p['fee_sats'] == 1_000_000 for p in plans)
    assert [p['fee_sats'] for p in plans] == sorted(p['fee_sats'] for p in plans)
    negative = None
    try:
        rpc('walletcreatefundedpsbt', [{'txid':u['txid'],'vout':u['vout']} for u in unspent], [{savings_address:btc(1_000_000)}], 0, {'add_inputs':False,'subtractFeeFromOutputs':[0],'replaceable':True,'fee_rate':1000}, True, wallet=hot)
        negative = {'refused':False,'detail':'funding RPC unexpectedly created a PSBT'}
    except RpcError as error:
        assert error.code in (-4, -6), error
        negative = {'refused':True,'rpc_error_code':error.code,'detail':str(error)}
    assert negative['refused'], negative
    chosen = plans[0]
    txid = rpc('sendrawtransaction', signed_raw[1])
    assert txid == chosen['txid']
    rpc('generatetoaddress', 1, miner)
    hot_after = rpc('listunspent', 1, 9999999, [], True, wallet=hot)
    cold_after = rpc('listunspent', 1, 9999999, [], True, wallet='merchant_savings')
    assert not hot_after and len(cold_after) == 1
    assert exact_sats(cold_after[0]['amount']) == chosen['cold_output_sats']
    assert rpc('getwalletinfo', wallet='merchant_savings')['private_keys_enabled'] is False
    evidence = {'purpose':'synthetic Core regtest transaction-policy probe; not an OpenBMT controller','core_version':core_version,'chain':'regtest','ordinary_customer_payments':{'count':100,'distinct_payment_txids':len(set(payment_txids)),'confirmed_merchant_hot_utxos':100,'initial_merchant_hot_sats':0,'initial_merchant_savings_sats':0,'gross_receipts_sats':1_000_000,'gross_receipts_msat':1_000_000_000,'individual_payment_sats':10_000,'payjoin_used':False},'merchant_savings':{'address':savings_address,'descriptor_imported':True,'label':'synthetic-public-fixture','private_keys_enabled':False},'signed_alternative_spend_plans':plans,'broadcast':{'selected_fee_rate_sat_vb':1,'txid':txid,'confirmed':True,'merchant_hot_unspent_after':len(hot_after),'merchant_savings_unspent_after':len(cold_after),'fee_once_sats':chosen['fee_sats'],'cold_output_sats':chosen['cold_output_sats']},'oversize_fee_refusal_1000_sat_vb':negative,'limitations':'Local Core wallet and public fixture only; label is not hardware proof. Assumes online hot key is available to batch. Cold receiving requires no signature. Fee ceilings can leave hot-key funds exposed at high fees; no lifetime fee guarantee, zero-conf retail proof, future fee forecast, or all-wallet compatibility proof. No Lightning, provider, or Payjoin. Fee plans are alternatives; only 1 sat/vB was broadcast.','run_directory':str(run)}
    (run/'result.json').write_text(json.dumps(evidence, indent=2)+'\n')
    print(json.dumps({'complete':True,'run_directory':str(run),'confirmed_payments':100,'batched_inputs':100,'chosen_fee_sats':chosen['fee_sats'],'cold_output_sats':chosen['cold_output_sats']}), flush=True)
finally:
    if proc.poll() is None:
        try:
            rpc('stop')
        except Exception:
            proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)
    log.close()
