//! Synthetic, regtest-only demonstration of two independent SW-25 trees in
//! one unilateral script-path claim. Fixed keys are public test fixtures.
use std::{env, error::Error};

use bitcoin::{
    absolute::LockTime,
    consensus::encode::{deserialize, serialize},
    hashes::Hash,
    key::{Keypair, TapTweak},
    secp256k1::{Message, Secp256k1, SecretKey},
    sighash::{Prevouts, SighashCache, TapSighashType},
    taproot::{LeafVersion, TapLeafHash},
    transaction::Version,
    Address, Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
};
use miniscript::interpreter::Interpreter;
use openbmt_swap::{KeyOrder, SwapTree};

const CLAIM_SK: [[u8; 32]; 2] = [[1; 32], [2; 32]];
const PREIMAGES: [[u8; 32]; 2] = [[0xa1; 32], [0xa2; 32]];
const REFUND_PUBKEYS: [[u8; 33]; 2] = [
    [
        0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87,
        0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16,
        0xf8, 0x17, 0x98,
    ],
    [
        0x02, 0xc6, 0x04, 0x7f, 0x94, 0x41, 0xed, 0x7d, 0x6d, 0x30, 0x45, 0x40, 0x6e, 0x95, 0xc0,
        0x7c, 0xd8, 0x5c, 0x77, 0x8e, 0x4b, 0x8c, 0xef, 0x3c, 0xa7, 0xab, 0xac, 0x09, 0xb9, 0x5c,
        0x70, 0x9e, 0xe5,
    ],
];
const MERCHANT_COLD_SK: [u8; 32] = [5; 32];
const COLD_INPUT_SK: [u8; 32] = [6; 32];
const COLD_INPUT_SATS: u64 = 300_000;
const COLD_OUTPUT_SATS: u64 = 490_000;
const CLAIM_FIXTURE_SATS: u64 = 100_000;
const TIMEOUTS: [u32; 2] = [500_000, 500_000];

fn secret(b: &[u8; 32]) -> Result<SecretKey, Box<dyn Error>> {
    Ok(SecretKey::from_slice(b)?)
}
fn fixture(i: usize) -> Result<(SwapTree, [u8; 32]), Box<dyn Error>> {
    let claim = secret(&CLAIM_SK[i])?;
    let refund = bitcoin::secp256k1::PublicKey::from_slice(&REFUND_PUBKEYS[i])?;
    let preimage = PREIMAGES[i];
    let hash = bitcoin::hashes::sha256::Hash::hash(&preimage).to_byte_array();
    let tree = SwapTree::build(
        &bitcoin::secp256k1::PublicKey::from_secret_key(&Secp256k1::signing_only(), &claim),
        &refund,
        &hash,
        TIMEOUTS[i],
        KeyOrder::RefundFirst,
    )?;
    Ok((tree, preimage))
}
fn from_hex(s: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if !s.len().is_multiple_of(2) {
        return Err("hex must have even length".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().as_chunks::<2>().0 {
        let h = std::str::from_utf8(pair)?;
        out.push(u8::from_str_radix(h, 16)?);
    }
    Ok(out)
}
fn prepare() -> Result<(), Box<dyn Error>> {
    let mut swaps = Vec::new();
    for (i, timeout) in TIMEOUTS.iter().enumerate() {
        let (tree, _) = fixture(i)?;
        let control = tree
            .claim_control_block()
            .ok_or("claim control block missing")?;
        swaps.push(serde_json::json!({
            "index": i,
            "network": "regtest",
            "address": tree.address(Network::Regtest).to_string(),
            "scriptPubKey": hex_bytes(tree.script_pubkey().as_bytes()),
            "claimLeaf": hex_bytes(tree.claim_leaf().as_bytes()),
            "controlBlock": hex_bytes(&control.serialize()),
            "timeoutHeight": timeout,
            "fixture": "fixed synthetic probe key; not a production secret"
        }));
    }
    let cold_key = Keypair::from_secret_key(&Secp256k1::new(), &secret(&MERCHANT_COLD_SK)?);
    let cold_address = Address::p2tr(
        &Secp256k1::verification_only(),
        cold_key.x_only_public_key().0,
        None,
        Network::Regtest,
    );
    let input_address = Address::from_script(&cold_input_script()?, Network::Regtest)?;
    println!(
        "{}",
        serde_json::json!({"purpose":"synthetic batch claim probe", "merchantColdDestination": cold_address.to_string(), "coldInputAddress": input_address.to_string(), "swaps": swaps})
    );
    Ok(())
}
fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn cold_destination() -> Result<Address, Box<dyn Error>> {
    let key = Keypair::from_secret_key(&Secp256k1::new(), &secret(&MERCHANT_COLD_SK)?);
    Ok(Address::p2tr(
        &Secp256k1::verification_only(),
        key.x_only_public_key().0,
        None,
        Network::Regtest,
    ))
}
fn cold_input_script() -> Result<ScriptBuf, Box<dyn Error>> {
    let key = Keypair::from_secret_key(&Secp256k1::new(), &secret(&COLD_INPUT_SK)?);
    Ok(Address::p2tr(
        &Secp256k1::verification_only(),
        key.x_only_public_key().0,
        None,
        Network::Regtest,
    )
    .script_pubkey())
}
fn cold_coupon(raw: &str, vout: &str) -> Result<(), Box<dyn Error>> {
    let (outpoint, prevout) = funding(raw, vout, &cold_input_script()?)?;
    if prevout.value.to_sat() != COLD_INPUT_SATS {
        return Err("coupon requires the fixed 300000-sat cold input".into());
    }
    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(COLD_OUTPUT_SATS),
            script_pubkey: cold_destination()?.script_pubkey(),
        }],
    };
    let prevouts = [prevout.clone()];
    let secp = Secp256k1::new();
    let key = Keypair::from_secret_key(&secp, &secret(&COLD_INPUT_SK)?)
        .tap_tweak(&secp, None)
        .to_keypair();
    let sighash = SighashCache::new(&tx).taproot_key_spend_signature_hash(
        0,
        &Prevouts::All(&prevouts),
        TapSighashType::SinglePlusAnyoneCanPay,
    )?;
    let sig = secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &key);
    let mut encoded = sig.as_ref().to_vec();
    encoded.push(TapSighashType::SinglePlusAnyoneCanPay as u8);
    tx.input[0].witness = Witness::from_slice(&[encoded]);
    if !verify_cold(&tx, &prevout)? {
        return Err("cold coupon signature did not verify".into());
    }
    let mut decreased = tx.clone();
    decreased.output[0].value = Amount::from_sat(COLD_OUTPUT_SATS - 1);
    let mut redirected = tx.clone();
    redirected.output[0].script_pubkey = ScriptBuf::new_op_return([1u8]);
    println!(
        "{}",
        serde_json::json!({
            "signedCouponHex": hex_bytes(&serialize(&tx)),
            "coldInputOutpoint": tx.input[0].previous_output.to_string(),
            "coldInputSats": COLD_INPUT_SATS,
            "coldOutputSats": COLD_OUTPUT_SATS,
            "negativeSameSignatureChecks": {"decreasedColdOutputRejected": !verify_cold(&decreased, &prevout)?, "redirectedColdOutputRejected": !verify_cold(&redirected, &prevout)?}
        })
    );
    Ok(())
}
fn verify_cold(tx: &Transaction, prevout: &TxOut) -> Result<bool, Box<dyn Error>> {
    let Some(input) = tx.input.first() else {
        return Ok(false);
    };
    let Some(raw) = input.witness.iter().next() else {
        return Ok(false);
    };
    if raw.len() != 65 || raw[64] != TapSighashType::SinglePlusAnyoneCanPay as u8 {
        return Ok(false);
    }
    let sighash = SighashCache::new(tx).taproot_key_spend_signature_hash(
        0,
        &Prevouts::One(0, prevout),
        TapSighashType::SinglePlusAnyoneCanPay,
    )?;
    let signature = bitcoin::secp256k1::schnorr::Signature::from_slice(&raw[..64])?;
    let script = prevout.script_pubkey.as_bytes();
    if script.len() != 34 || script[0] != 0x51 || script[1] != 0x20 {
        return Ok(false);
    }
    let output_key = match bitcoin::XOnlyPublicKey::from_slice(&script[2..]) {
        Ok(k) => k,
        Err(_) => return Ok(false),
    };
    Ok(Secp256k1::verification_only()
        .verify_schnorr(
            &signature,
            &Message::from_digest(sighash.to_byte_array()),
            &output_key,
        )
        .is_ok())
}
/// Assemble a single pre-authorized coupon with two unilateral claim inputs.
/// Deliberately has no reference to, or use of, `COLD_INPUT_SK`.
fn assemble_coupon(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() != 9 {
        return Err("usage: batch-claim-probe assemble <coupon-tx-hex> <cold-funding-tx-hex> <cold-vout> <swap-tx-hex-1> <swap-vout-1> <swap-tx-hex-2> <swap-vout-2>".into());
    }
    let coupon_bytes = from_hex(&args[2])?;
    let mut tx: Transaction = deserialize(&coupon_bytes)?;
    if tx.input.len() != 1 || tx.output.len() != 1 {
        return Err("coupon must be exactly one input and one paired output".into());
    }
    let (cold_outpoint, cold_prevout) = raw_prevout(&args[3], &args[4])?;
    if tx.input[0].previous_output != cold_outpoint
        || cold_prevout.value.to_sat() != COLD_INPUT_SATS
    {
        return Err("coupon does not match the fixed cold funding outpoint and amount".into());
    }
    if tx.output[0].value.to_sat() != COLD_OUTPUT_SATS
        || tx.output[0].script_pubkey != cold_destination()?.script_pubkey()
    {
        return Err("coupon cold output differs from the fixed authorized pair".into());
    }
    if !verify_cold(&tx, &cold_prevout)? {
        return Err("coupon signature is invalid".into());
    }
    let (tree0, pre0) = fixture(0)?;
    let (tree1, pre1) = fixture(1)?;
    let (op0, prev0) = funding(&args[5], &args[6], &tree0.script_pubkey())?;
    let (op1, prev1) = funding(&args[7], &args[8], &tree1.script_pubkey())?;
    if prev0.value.to_sat() != CLAIM_FIXTURE_SATS || prev1.value.to_sat() != CLAIM_FIXTURE_SATS {
        return Err("coupon probe requires two fixed 100000-sat swap inputs".into());
    }
    if op0 == op1 || op0 == cold_outpoint || op1 == cold_outpoint {
        return Err("coupon and claim inputs must be distinct outpoints".into());
    }
    let prevouts = [cold_prevout, prev0, prev1];
    tx.input
        .extend([op0, op1].into_iter().map(|previous_output| TxIn {
            previous_output,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }));
    let secp = Secp256k1::new();
    for (n, tree, preimage) in [(1usize, &tree0, pre0), (2usize, &tree1, pre1)] {
        let key = Keypair::from_secret_key(&secp, &secret(&CLAIM_SK[n - 1])?);
        let leaf = TapLeafHash::from_script(tree.claim_leaf(), LeafVersion::TapScript);
        let sighash = SighashCache::new(&tx).taproot_script_spend_signature_hash(
            n,
            &Prevouts::All(&prevouts),
            leaf,
            TapSighashType::Default,
        )?;
        let sig =
            secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &key);
        let control = tree
            .claim_control_block()
            .ok_or("claim control block missing")?;
        tx.input[n].witness = Witness::from_slice(&[
            sig.as_ref().as_slice(),
            preimage.as_slice(),
            tree.claim_leaf().as_bytes(),
            &control.serialize(),
        ]);
    }
    if !verify_cold(&tx, &prevouts[0])? {
        return Err("cold coupon signature failed after claim inputs were appended".into());
    }
    for (n, tree) in [(1usize, &tree0), (2usize, &tree1)] {
        let input = &tx.input[n];
        let interp = Interpreter::from_txdata(
            &tree.script_pubkey(),
            &input.script_sig,
            &input.witness,
            input.sequence,
            tx.lock_time,
        )?;
        let mut steps = 0usize;
        for step in interp.iter(&secp, &tx, n, &Prevouts::All(&prevouts)) {
            step?;
            steps += 1;
        }
        if steps < 2 {
            return Err(format!("input {n} did not satisfy claim script conditions").into());
        }
    }
    let total = COLD_INPUT_SATS + CLAIM_FIXTURE_SATS + CLAIM_FIXTURE_SATS;
    if total - COLD_OUTPUT_SATS != 10_000 {
        return Err("coupon fee fixture is not the fixed 10000 sats".into());
    }
    println!("{}", hex_bytes(&serialize(&tx)));
    Ok(())
}
fn funding(
    hex: &str,
    vout: &str,
    expected: &ScriptBuf,
) -> Result<(OutPoint, TxOut), Box<dyn Error>> {
    let (outpoint, out) = raw_prevout(hex, vout)?;
    if out.script_pubkey != *expected {
        return Err("funding output script does not match fixture".into());
    }
    Ok((outpoint, out))
}
fn raw_prevout(hex: &str, vout: &str) -> Result<(OutPoint, TxOut), Box<dyn Error>> {
    let bytes = from_hex(hex)?;
    let tx: Transaction = deserialize(&bytes)?;
    let n: usize = vout.parse()?;
    let out = tx
        .output
        .get(n)
        .ok_or("vout outside funding transaction")?
        .clone();
    if out.value.to_sat() == 0 {
        return Err("funding output value is zero".into());
    }
    Ok((
        OutPoint {
            txid: tx.compute_txid(),
            vout: u32::try_from(n)?,
        },
        out,
    ))
}
fn spend(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() != 7 {
        return Err("usage: batch-claim-probe spend <funding-tx-hex-1> <vout-1> <funding-tx-hex-2> <vout-2> <fee-sats>".into());
    }
    let (tree0, pre0) = fixture(0)?;
    let (tree1, pre1) = fixture(1)?;
    let (op0, prev0) = funding(&args[2], &args[3], &tree0.script_pubkey())?;
    let (op1, prev1) = funding(&args[4], &args[5], &tree1.script_pubkey())?;
    if op0 == op1 {
        return Err("two independent inputs must be distinct outpoints".into());
    }
    let cold_key = Keypair::from_secret_key(&Secp256k1::new(), &secret(&MERCHANT_COLD_SK)?);
    let dest = Address::p2tr(
        &Secp256k1::verification_only(),
        cold_key.x_only_public_key().0,
        None,
        Network::Regtest,
    );
    let fee: u64 = args[6].parse()?;
    let total = prev0
        .value
        .to_sat()
        .checked_add(prev1.value.to_sat())
        .ok_or("input total overflow")?;
    let out_value = total
        .checked_sub(fee)
        .filter(|v| *v >= dest.script_pubkey().minimal_non_dust().to_sat())
        .ok_or("fee leaves dust or exceeds value")?;
    let prevouts = [prev0, prev1];
    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![op0, op1]
            .into_iter()
            .map(|previous_output| TxIn {
                previous_output,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            })
            .collect(),
        output: vec![TxOut {
            value: Amount::from_sat(out_value),
            script_pubkey: dest.script_pubkey(),
        }],
    };
    let secp = Secp256k1::new();
    for (i, claim_bytes) in CLAIM_SK.iter().enumerate() {
        let (tree, preimage) = if i == 0 {
            (&tree0, pre0)
        } else {
            (&tree1, pre1)
        };
        let claim_sk = secret(claim_bytes)?;
        let kp = Keypair::from_secret_key(&secp, &claim_sk);
        let leaf_hash = TapLeafHash::from_script(tree.claim_leaf(), LeafVersion::TapScript);
        let sighash = SighashCache::new(&tx).taproot_script_spend_signature_hash(
            i,
            &Prevouts::All(&prevouts),
            leaf_hash,
            TapSighashType::Default,
        )?;
        let sig =
            secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &kp);
        let control = tree
            .claim_control_block()
            .ok_or("claim control block missing")?;
        tx.input[i].witness = Witness::from_slice(&[
            sig.as_ref().as_slice(),
            preimage.as_slice(),
            tree.claim_leaf().as_bytes(),
            &control.serialize(),
        ]);
    }
    // Verify each script path with the complete all-input prevout set before
    // exposing the serialized transaction (and therefore its preimages).
    for i in 0..2 {
        let input = &tx.input[i];
        let spk = if i == 0 {
            tree0.script_pubkey()
        } else {
            tree1.script_pubkey()
        };
        let interp = Interpreter::from_txdata(
            &spk,
            &input.script_sig,
            &input.witness,
            input.sequence,
            tx.lock_time,
        )?;
        let mut steps = 0usize;
        for step in interp.iter(&secp, &tx, i, &Prevouts::All(&prevouts)) {
            step?;
            steps += 1;
        }
        if steps < 2 {
            return Err(format!("input {i} did not satisfy claim script conditions").into());
        }
    }
    println!("{}", hex_bytes(&serialize(&tx)));
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("prepare") => prepare(),
        Some("coupon") if args.len() == 4 => cold_coupon(&args[2], &args[3]),
        Some("spend") => spend(&args),
        Some("assemble") => assemble_coupon(&args),
        _ => Err("usage: batch-claim-probe prepare | spend ...".into()),
    }
}
