#![no_main]

use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use bitcoin::transaction::Version;
use bitcoin::{
    Address, Amount, CompressedPublicKey, Network, OutPoint, ScriptBuf, Sequence, Transaction,
    TxIn, TxOut, Witness,
};
use libfuzzer_sys::fuzz_target;
use openbmt_lightning::testing::Make;
use openbmt_swap::follow::{Ctx, ReverseSwap};
use openbmt_swap::tree::{KeyOrder, SwapTree};
use openbmt_swap::{Quote, Reply, Secrets, Seen, SwapState};

/// Valid files of the machine at several stages, so that mutated bytes get
/// past the header.
fn valid() -> &'static Vec<Vec<u8>> {
    static VALID: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    VALID.get_or_init(|| {
        let secrets = Secrets::from_random([7; 32], [9; 32]).unwrap();
        let secp = Secp256k1::new();
        let service = PublicKey::from_secret_key(&secp, &SecretKey::from_slice(&[5; 32]).unwrap());
        let tree = SwapTree::build(
            &secrets.claim_public(),
            &service,
            &secrets.preimage_hash(),
            250_144,
            KeyOrder::RefundFirst,
        )
        .unwrap();
        let mut invoice = Make::good(secrets.preimage_hash());
        invoice.network = "tbs";
        invoice.msat = Some(100_000_000);
        invoice.timestamp = 1_800_000_000;
        invoice.expiry = Some(3600);
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let json = format!(
            "{{\"id\":\"abc\",\"invoice\":\"{}\",\"swapTree\":{{\"claimLeaf\":{{\"version\":192,\
             \"output\":\"{}\"}},\"refundLeaf\":{{\"version\":192,\"output\":\"{}\"}}}},\
             \"lockupAddress\":\"{}\",\"refundPublicKey\":\"{}\",\"timeoutBlockHeight\":250144,\
             \"onchainAmount\":99350}}",
            invoice.write(),
            hex(tree.claim_leaf().as_bytes()),
            hex(tree.refund_leaf().as_bytes()),
            tree.address(Network::Signet),
            hex(&service.serialize()),
        );
        let reply = Reply::parse(json.as_bytes()).unwrap();
        let swap = openbmt_swap::check(
            &reply,
            &openbmt_swap::Expect {
                secrets: &secrets,
                invoice_sats: 100_000,
                network: Network::Signet,
                quote: Quote {
                    fee_ppm: 2_500,
                    lockup_fee_sats: 400,
                },
                tip_height: 250_000,
                now_secs: 1_800_000_000,
            },
        )
        .unwrap();
        let dest = Address::p2wpkh(&CompressedPublicKey(service), Network::Signet);
        let state = SwapState::new(&swap, &secrets, Network::Signet, &dest).unwrap();
        let ctx = Ctx {
            now_secs: 1_800_000_000,
            tip_height: 250_000,
            accept_unconfirmed: true,
        };
        let lockup = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(99_350),
                script_pubkey: tree.script_pubkey(),
            }],
        };
        let created = ReverseSwap::new(state);
        let (locked, _) = created.on_lockup(&lockup, Seen::Block, &ctx).unwrap();
        let (sent, claim) = locked.claim(2_000, &ctx).unwrap();
        let paid = sent.claim_confirmed(claim.compute_txid(), 250_001).unwrap();
        [created, locked, sent, paid]
            .iter()
            .map(ReverseSwap::encode)
            .collect()
    })
}

fuzz_target!(|data: &[u8]| {
    // The bytes as they are.
    if let Ok(machine) = ReverseSwap::decode(data) {
        assert_eq!(machine.encode(), data);
    }
    // A valid file with some bytes changed: the first byte picks the file,
    // then pairs of (place, byte).
    let Some((&pick, rest)) = data.split_first() else {
        return;
    };
    let files = valid();
    let mut file = files[usize::from(pick) % files.len()].clone();
    for pair in rest.chunks_exact(3) {
        let at = usize::from(u16::from_be_bytes([pair[0], pair[1]])) % file.len();
        file[at] = pair[2];
    }
    if let Ok(machine) = ReverseSwap::decode(&file) {
        assert_eq!(machine.encode(), file);
    }
});
