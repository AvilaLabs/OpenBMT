#![no_main]

use std::sync::OnceLock;

use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use bitcoin::{Address, CompressedPublicKey, Network};
use libfuzzer_sys::fuzz_target;
use openbmt_lightning::testing::Make;
use openbmt_swap::tree::{KeyOrder, SwapTree};
use openbmt_swap::{Quote, Reply, Secrets, SwapState};

/// A valid state file, so that mutated bytes get past the header.
fn valid() -> &'static Vec<u8> {
    static VALID: OnceLock<Vec<u8>> = OnceLock::new();
    VALID.get_or_init(|| {
        let secrets = Secrets::from_random([7; 32], [9; 32]).unwrap();
        let secp = Secp256k1::new();
        let service =
            PublicKey::from_secret_key(&secp, &SecretKey::from_slice(&[5; 32]).unwrap());
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
        SwapState::new(&swap, &secrets, Network::Signet, &dest)
            .unwrap()
            .encode()
    })
}

fuzz_target!(|data: &[u8]| {
    // The bytes as they are.
    if let Ok(state) = SwapState::decode(data) {
        assert_eq!(state.encode(), data);
    }
    // A valid file with some bytes changed: pairs of (place, byte).
    let mut file = valid().clone();
    for pair in data.chunks_exact(3) {
        let at = usize::from(u16::from_be_bytes([pair[0], pair[1]])) % file.len();
        file[at] = pair[2];
    }
    if let Ok(state) = SwapState::decode(&file) {
        assert_eq!(state.encode(), file);
    }
});
