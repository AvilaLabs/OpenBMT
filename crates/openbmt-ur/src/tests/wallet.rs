//! Wallet text from crypto-account and output-descriptor URs.

use crate::{
    ur_to_wallet_text, Decoder, Encoder, WalletDecoder, WalletRefusal, MAX_WALLET_CBOR,
    MAX_WALLET_PARTS, MAX_WALLET_SEQ_LEN, MAX_WALLET_TEXT,
};
use openbmt_core::wallet_single::ImportedWallet;

const ACCOUNT_UR: &str = "ur:crypto-account/oeadcyemrewytyaolttaadeetaadmutaaddloxaxhdclaxwmfmdeiamecsdsemgtvsjzcncygrkowtrontzschgezokstswkkscfmklrtauteyaahdcxiehfonurdppfyntapejpproypegrdawkgmaewejlsfdtsrfybdehcaflmtrlbdhpamtaaddyoeadlncsdwykaeykaeykaocyemrewytyaycynlytsnyltaadeetaadmhtaadmwtaaddloxaxhdclaostvelfemdyynwydwyaievosrgmambklovabdgypdglldvespsthysadamhpmjeinaahdcxntdllnaaeykoytdacygegwhgjsiyonpywmcmrpwphsvodsrerozsbyaxluzcoxdpamtaaddyoeadlncsehykaeykaeykaocyemrewytyaycypdbskeuytaadeetaadmwtaaddloxaxhdclaxzcfxeegdrpmogrgwkbzctlttweadkiengrwlhtprremouoluutqdpfbncedkynfhaahdcxjpwevdeogthttkmeswzcolcpsaahcfnshkhtehytclmnteatmoteadtlwynnftloamtaaddyoeadlncsghykaeykaeykaocyemrewytyaycybthlvytstaadeetaadmhtaadnytaaddloxaxhdclaxhhsnhdrpftdwuocntilydibehnecmovdfekpjkclcslasbhkpawsaddmcmmnahnyaahdcxlotedtndfymyltclhlmtpfsadscnhtztaolbnnkistaedegwfmmedreetnwmcycnamtaaddyoeadlfcsdpykaocyemrewytyaycyemrewytytaadeetaadmhtaadmetaadnytaaddloxaxhdclaxdwkswmztpytnswtsecnblfbayajkdldeclqzzolrsnhljedsgminetytbnahatbyaahdcxkkguwsvyimjkvwteytwztyswvendtpmncpasfrrylprnhtkblndrgrmkoyjtbkrpamtaaddyoeadlocsdyykaeykaeykadykaocyemrewytyaycyhkrpnddrtaadeetaadmetaadnytaaddloxaxhdclaohnhffmvsbndslrfgclpfjejyatbdpebacnzokotofxntaoemvskpaowmryfnotfgaahdcxdlnbvecentssfsssgylnhkrstoytecrdlyadrekirfaybglahltalsrfcaeerobwamtaaddyoeadlocsdyykaeykaeykaoykaocyemrewytyaycyhkrpnddrtaadeetaadnltaaddloxaxhdclaorkrhkeytwsoykorletwstbwycagtbsotmeptjkesgwrfcmveskvdmngujzttgtdpaahdcxgrfgmuvyylmwcxjtttechplslgoegagaptdniatidmhdmebdwfryfsnsdkcplyvaamtaaddyoeadlncshfykaeykaeykaocyemrewytyaycytostatbngmdavolk";

const BIP84_XPUB: &str = "xpub6BkU445MSEBXbPjD3g2c2ch6mn8yy1SXXQUM7EwjgYiq6Wt1NDwDZ45npqWcV8uQC5oi2gHuVukoCoZZyT4HKq8EpotPMqGqxdZRuapCQ23";
const ACCOUNT_DESCRIPTOR: &str = "wpkh([37b5eed4/84h/0h/0h]xpub6BkU445MSEBXbPjD3g2c2ch6mn8yy1SXXQUM7EwjgYiq6Wt1NDwDZ45npqWcV8uQC5oi2gHuVukoCoZZyT4HKq8EpotPMqGqxdZRuapCQ23/<0;1>/*)#yhygl07f";

const V3_SORTEDMULTI: &str = "wsh(sortedmulti(2,[dc567276/48'/0'/0'/2']xpub6DiYrfRwNnjeX4vHsWMajJVFKrbEEnu8gAW9vDuQzgTWEsEHE16sGWeXXUV1LBWQE1yCTmeprSNcqZ3W74hqVdgDbtYHUv3eM4W2TEUhpan/<0;1>/*,[f245ae38/48'/0'/0'/2']xpub6DnT4E1fT8VxuAZW29avMjr5i99aYTHBp9d7fiLnpL5t4JEprQqPMbTw7k7rh5tZZ2F5g8PJpssqrZoebzBChaiJrmEvWwUTEMAbHsY39Ge/<0;1>/*,[c5d87297/48'/0'/0'/2']xpub6DjrnfAyuonMaboEb3ZQZzhQ2ZEgaKV2r64BFmqymZqJqviLTe1JzMr2X2RfQF892RH7MyYUbcy77R7pPu1P71xoj8cDUMNhAMGYzKR4noZ/<0;1>/*))";
const V3_SORTEDMULTI_CBOR_HEX: &str = "d99d74a301781c77736828736f727465646d756c746928322c40302c40312c403229290283d99d6fa5035821021c0b479ecf6e67713ddf0c43b634592f51c037b6f951fb1dc6361a98b1e5735e0458206b3a4cfb6a45f6305efe6e0e976b5d26ba27f7c344d7fc7abef7be2d06d52dfd06d99d70a201881830f500f500f502f5021adc56727607d99d70a101838400f401f480f4081a18f8c2e7d99d6fa50358210397fcf2274abd243d42d42d3c248608c6d1935efca46138afef43af08e9712896045820c887c72d9d8ac29cddd5b2b060e8b0239039a149c784abe6079e24445db4aa8a06d99d70a201881830f500f500f502f5021af245ae3807d99d70a101838400f401f480f4081a221eb5a0d99d6fa5035821028342f5f7773f6fab374e1c2d3ccdba26bc0933fc4f63828b662b4357e4cc37910458205afed56d755c088320ec9bc6acd84d33737b580083759e0a0ff8f26e429e0b7706d99d70a201881830f500f500f502f5021ac5d8729707d99d70a101838400f401f480f4081a1c0ae906036f5361746f7368692773205374617368";
const V3_CANONICAL: &str = "wsh(sortedmulti(2,[dc567276/48h/0h/0h/2h]xpub6DiYrfRwNnjeX4vHsWMajJVFKrbEEnu8gAW9vDuQzgTWEsEHE16sGWeXXUV1LBWQE1yCTmeprSNcqZ3W74hqVdgDbtYHUv3eM4W2TEUhpan/<0;1>/*,[f245ae38/48h/0h/0h/2h]xpub6DnT4E1fT8VxuAZW29avMjr5i99aYTHBp9d7fiLnpL5t4JEprQqPMbTw7k7rh5tZZ2F5g8PJpssqrZoebzBChaiJrmEvWwUTEMAbHsY39Ge/<0;1>/*,[c5d87297/48h/0h/0h/2h]xpub6DjrnfAyuonMaboEb3ZQZzhQ2ZEgaKV2r64BFmqymZqJqviLTe1JzMr2X2RfQF892RH7MyYUbcy77R7pPu1P71xoj8cDUMNhAMGYzKR4noZ/<0;1>/*))#ud8uyjz3";

fn head(major: u8, value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    crate::cbor::push_head(&mut bytes, major, value);
    bytes
}

fn uint(value: u64) -> Vec<u8> {
    head(0, value)
}

fn byte_string(bytes: &[u8]) -> Vec<u8> {
    [head(2, bytes.len() as u64), bytes.to_vec()].concat()
}

fn array(items: &[Vec<u8>]) -> Vec<u8> {
    let mut result = head(4, items.len() as u64);
    for item in items {
        result.extend_from_slice(item);
    }
    result
}

fn map(entries: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut result = head(5, entries.len() as u64);
    for (key, value) in entries {
        result.extend(uint(*key));
        result.extend_from_slice(value);
    }
    result
}

fn tagged(tag: u64, item: &[u8]) -> Vec<u8> {
    [head(6, tag), item.to_vec()].concat()
}

fn descriptor_text_cbor(text: &str) -> Vec<u8> {
    let text = text.as_bytes();
    let mut map = head(5, 1);
    map.extend(head(0, 1));
    map.extend(head(3, text.len() as u64));
    map.extend_from_slice(text);
    tagged(40308, &map)
}

fn replace_once(text: &str, old: &str, new: &str) -> String {
    text.replacen(old, new, 1)
}

fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let hi = char::from(*pair.first().expect("hex high digit"))
                .to_digit(16)
                .expect("hex digit");
            let lo = char::from(*pair.get(1).expect("hex low digit"))
                .to_digit(16)
                .expect("hex digit");
            u8::try_from(hi * 16 + lo).expect("byte")
        })
        .collect()
}

fn origin_keypath(path: &[(u32, bool)], fingerprint: u32) -> Vec<u8> {
    let mut parts = Vec::new();
    for (index, hardened) in path {
        parts.push(uint(u64::from(*index)));
        parts.push(if *hardened { vec![0xf5] } else { vec![0xf4] });
    }
    tagged(
        304,
        &map(&[(1, array(&parts)), (2, uint(u64::from(fingerprint)))]),
    )
}

fn public_hdkey(
    public_key: &str,
    chain_code: &str,
    fingerprint: u32,
    parent_fingerprint: u32,
    path: &[(u32, bool)],
) -> Vec<u8> {
    let children_path = array(&[
        array(&[uint(0), vec![0xf4], uint(1), vec![0xf4]]),
        array(&[]),
        vec![0xf4],
    ]);
    let children = tagged(304, &map(&[(1, children_path)]));
    let body = map(&[
        (3, byte_string(&unhex(public_key))),
        (4, byte_string(&unhex(chain_code))),
        (6, origin_keypath(path, fingerprint)),
        (7, children),
        (8, uint(u64::from(parent_fingerprint))),
    ]);
    tagged(303, &body)
}

fn legacy_sortedmulti_cbor() -> Vec<u8> {
    let keys = [
        public_hdkey(
            "021c0b479ecf6e67713ddf0c43b634592f51c037b6f951fb1dc6361a98b1e5735e",
            "6b3a4cfb6a45f6305efe6e0e976b5d26ba27f7c344d7fc7abef7be2d06d52dfd",
            0xdc56_7276,
            0x18f8_c2e7,
            &[(48, true), (0, true), (0, true), (2, true)],
        ),
        public_hdkey(
            "0397fcf2274abd243d42d42d3c248608c6d1935efca46138afef43af08e9712896",
            "c887c72d9d8ac29cddd5b2b060e8b0239039a149c784abe6079e24445db4aa8a",
            0xf245_ae38,
            0x221e_b5a0,
            &[(48, true), (0, true), (0, true), (2, true)],
        ),
        public_hdkey(
            "028342f5f7773f6fab374e1c2d3ccdba26bc0933fc4f63828b662b4357e4cc3791",
            "5afed56d755c088320ec9bc6acd84d33737b580083759e0a0ff8f26e429e0b77",
            0xc5d8_7297,
            0x1c0a_e906,
            &[(48, true), (0, true), (0, true), (2, true)],
        ),
    ];
    let sortedmulti = tagged(407, &map(&[(1, uint(2)), (2, array(&keys))]));
    tagged(401, &sortedmulti)
}

fn legacy_wpkh_cbor() -> Vec<u8> {
    let path = [(84, true), (0, true), (0, true)];
    let body = map(&[
        (
            3,
            byte_string(&unhex(
                "03FD433450B6924B4F7EFDD5D1ED017D364BE95AB2B592DC8BDDB3B00C1C24F63F",
            )),
        ),
        (
            4,
            byte_string(&unhex(
                "72EDE7334D5ACF91C6FDA622C205199C595A31F9218ED30792D301D5EE9E3A88",
            )),
        ),
        (6, origin_keypath(&path, 0x37b5_eed4)),
        (8, uint(224_256_471)),
    ]);
    tagged(404, &tagged(303, &body))
}

fn legacy_account_with_outputs(outputs: &[Vec<u8>]) -> Vec<u8> {
    map(&[(1, uint(0x37b5_eed4)), (2, array(outputs))])
}

fn account_bip84_output() -> Vec<u8> {
    let path = [(84, true), (0, true), (0, true)];
    let body = map(&[
        (
            3,
            byte_string(&unhex(
                "03FD433450B6924B4F7EFDD5D1ED017D364BE95AB2B592DC8BDDB3B00C1C24F63F",
            )),
        ),
        (
            4,
            byte_string(&unhex(
                "72EDE7334D5ACF91C6FDA622C205199C595A31F9218ED30792D301D5EE9E3A88",
            )),
        ),
        (6, origin_keypath(&path, 0x37b5_eed4)),
        (8, uint(224_256_471)),
    ]);
    tagged(308, &tagged(404, &tagged(303, &body)))
}

fn tagged_multisig_output(keys: &[Vec<u8>]) -> Vec<u8> {
    let sortedmulti = tagged(407, &map(&[(1, uint(2)), (2, array(keys))]));
    tagged(308, &tagged(401, &sortedmulti))
}

#[test]
fn ur_34_bcr_2020_015_account_selects_the_bip84_output() {
    let mut decoder = Decoder::new();
    let progress = decoder.receive(ACCOUNT_UR).expect("official account UR");
    assert!(progress.complete);
    let (kind, cbor) = decoder.result().expect("complete account");
    assert_eq!(kind, "crypto-account");

    let wallet = ur_to_wallet_text(&kind, &cbor).expect("supported BIP84 output");
    let text = wallet.as_str();
    assert_eq!(text, ACCOUNT_DESCRIPTOR);
    assert!(ImportedWallet::read(text).is_ok(), "{text}");
}

#[test]
fn ur_34_legacy_308_rejects_unsupported_scripts_and_new_40308_is_distinct() {
    // The published v1 pkh output vector is syntactically valid, but this
    // wallet profile only accepts the BIP84 account output.
    let legacy_pkh = [
        0xd9, 0x01, 0x93, 0xd9, 0x01, 0x32, 0xa1, 0x03, 0x58, 0x21, 0x02, 0xc6, 0x04, 0x7f, 0x94,
        0x41, 0xed, 0x7d, 0x6d, 0x30, 0x45, 0x40, 0x6e, 0x95, 0xc0, 0x7c, 0xd8, 0x5c, 0x77, 0x8e,
        0x4b, 0x8c, 0xef, 0x3c, 0xa7, 0xab, 0xac, 0x09, 0xb9, 0x5c, 0x70, 0x9e, 0xe5,
    ];
    assert!(ur_to_wallet_text("crypto-output", &legacy_pkh).is_err());

    // BCR-2023-010 tag 40308 carries descriptor text and keys directly; it
    // is not the older tag-308 script-expression payload.
    let v3_wrong_shape = [0xd9, 0x9d, 0x74, 0xd9, 0x01, 0x91, 0x00];
    assert!(ur_to_wallet_text("output-descriptor", &v3_wrong_shape).is_err());
}

#[test]
fn ur_34_legacy_crypto_output_reads_bip84_wpkh_and_2_of_3_sortedmulti() {
    let wpkh = ur_to_wallet_text("crypto-output", &legacy_wpkh_cbor()).expect("legacy wpkh output");
    assert_eq!(wpkh.as_str(), ACCOUNT_DESCRIPTOR);
    assert!(ImportedWallet::read(wpkh.as_str()).is_ok());

    let multisig = ur_to_wallet_text("crypto-output", &legacy_sortedmulti_cbor())
        .expect("legacy sortedmulti output");
    assert_eq!(multisig.as_str(), V3_CANONICAL);
    assert!(ImportedWallet::read(multisig.as_str()).is_ok());
}

#[test]
fn ur_34_bcr_2023_010_v3_sortedmulti_imports_as_a_wallet() {
    let cbor = descriptor_text_cbor(V3_SORTEDMULTI);
    let text = ur_to_wallet_text("output-descriptor", &cbor).expect("v3 descriptor");
    assert!(text.as_str().starts_with("wsh(sortedmulti(2,"));
    assert!(text.as_str().contains("xpub6DiYrfRwNnjeX4vHsWMaj"));
    assert!(text.as_str().contains("#"));
    assert!(
        ImportedWallet::read(text.as_str()).is_ok(),
        "{}",
        text.as_str()
    );

    let bytes = crate::cbor::encode_bytes(V3_SORTEDMULTI.as_bytes());
    let from_bytes = ur_to_wallet_text("bytes", &bytes).expect("descriptor in bytes");
    assert!(ImportedWallet::read(from_bytes.as_str()).is_ok());
}

#[test]
fn ur_34_bcr_2023_010_published_40308_hdkey_table_imports_exactly() {
    // Literal 405-byte CBOR known answer from BCR-2023-010 Example 4:
    // https://github.com/BlockchainCommons/Research/blob/master/papers/bcr-2023-010-output-descriptor.md
    let cbor = unhex(V3_SORTEDMULTI_CBOR_HEX);
    assert_eq!(cbor.len(), 405);
    let wallet = ur_to_wallet_text("output-descriptor", &cbor).expect("published v3 vector");
    assert_eq!(wallet.as_str(), V3_CANONICAL);
    assert!(ImportedWallet::read(wallet.as_str()).is_ok());
}

#[test]
fn ur_37_account_refuses_hidden_testnet_cosigner() {
    let selected = account_bip84_output();
    let hidden_testnet = public_hdkey(
        "021c0b479ecf6e67713ddf0c43b634592f51c037b6f951fb1dc6361a98b1e5735e",
        "6b3a4cfb6a45f6305efe6e0e976b5d26ba27f7c344d7fc7abef7be2d06d52dfd",
        0xdc56_7276,
        0x18f8_c2e7,
        &[(48, true), (1, true), (0, true), (2, true)],
    );
    let account =
        legacy_account_with_outputs(&[selected, tagged_multisig_output(&[hidden_testnet])]);
    assert_eq!(
        ur_to_wallet_text("crypto-account", &account),
        Err(WalletRefusal::TestNetwork)
    );
}

#[test]
fn ur_38_account_refuses_unselected_hdkey_with_32_byte_private_data() {
    let selected = account_bip84_output();
    let private_key_without_flag = tagged(303, &map(&[(3, byte_string(&[7; 32]))]));
    let account = legacy_account_with_outputs(&[
        selected,
        tagged_multisig_output(&[private_key_without_flag]),
    ]);
    assert_eq!(
        ur_to_wallet_text("crypto-account", &account),
        Err(WalletRefusal::SecretKey)
    );
}

#[test]
fn ur_35_refuses_unknown_types_noncanonical_cbor_and_trailing_data() {
    assert_eq!(
        ur_to_wallet_text("crypto-psbt", &[0x00]),
        Err(WalletRefusal::Kind)
    );
    for bad in [&[][..], &[0x18, 0x01][..], &[0x01, 0x00][..]] {
        assert_eq!(
            ur_to_wallet_text("crypto-account", bad),
            Err(WalletRefusal::Malformed)
        );
    }
    assert_eq!(
        ur_to_wallet_text("crypto-account", &vec![0; MAX_WALLET_CBOR + 1]),
        Err(WalletRefusal::TooLarge)
    );
    assert_eq!(
        ur_to_wallet_text("crypto-account", &descriptor_text_cbor(V3_SORTEDMULTI)),
        Err(WalletRefusal::Malformed),
        "an account cannot be wrapped in the v3 descriptor tag"
    );
}

#[test]
fn ur_36_refuses_unsupported_wallet_shapes() {
    let wrong_purpose = replace_once(V3_SORTEDMULTI, "/48'/", "/49'/");
    assert_eq!(
        ur_to_wallet_text("output-descriptor", &descriptor_text_cbor(&wrong_purpose)),
        Err(WalletRefusal::Unsupported)
    );
}

#[test]
fn ur_37_refuses_test_network_paths_and_keys() {
    let test_path = replace_once(V3_SORTEDMULTI, "/48'/0'/", "/48'/1'/");
    assert_eq!(
        ur_to_wallet_text("output-descriptor", &descriptor_text_cbor(&test_path)),
        Err(WalletRefusal::TestNetwork)
    );
}

#[test]
fn ur_38_refuses_private_extended_keys() {
    let secret = replace_once(V3_SORTEDMULTI, "xpub6DiYrfRwNnjeX4vHsWMaj", "xprv-secret");
    assert_eq!(
        ur_to_wallet_text("output-descriptor", &descriptor_text_cbor(&secret)),
        Err(WalletRefusal::SecretKey)
    );
}

#[test]
fn ur_34_wallet_decoder_reports_incomplete_and_refuses_oversized_text() {
    let mut decoder = WalletDecoder::new();
    assert!(matches!(decoder.finish(), Err(WalletRefusal::Incomplete)));

    let oversized = format!("ur:output-descriptor/{}", "a".repeat(MAX_WALLET_TEXT));
    assert!(matches!(
        decoder.receive(&oversized),
        Err(WalletRefusal::TooLarge)
    ));
}

#[test]
fn ur_39_requires_both_descriptor_branches_and_enforces_input_limits() {
    let receive_only = V3_SORTEDMULTI.replace("/<0;1>/*", "/0/*");
    assert_eq!(
        ur_to_wallet_text("output-descriptor", &descriptor_text_cbor(&receive_only)),
        Err(WalletRefusal::Incomplete)
    );
    assert_eq!(
        ur_to_wallet_text("crypto-account", &vec![0; MAX_WALLET_CBOR + 1]),
        Err(WalletRefusal::TooLarge)
    );
    let too_long = format!("wpkh({})", "x".repeat(4100));
    assert_eq!(
        ur_to_wallet_text("output-descriptor", &descriptor_text_cbor(&too_long)),
        Err(WalletRefusal::TooLarge)
    );

    let over_cbor_limit = vec![0; MAX_WALLET_CBOR + 1];
    let mut encoded = Encoder::new("crypto-account", &over_cbor_limit, 512, 32)
        .expect("bounded transport sequence");
    assert!(encoded.seq_len() <= MAX_WALLET_SEQ_LEN);
    assert_eq!(
        WalletDecoder::new().receive(&encoded.next_part()),
        Err(WalletRefusal::TooLarge),
        "declared message length is rejected before assembling its payload"
    );

    let below_cbor_limit = vec![0; MAX_WALLET_CBOR - 1];
    let mut encoded = Encoder::new("crypto-account", &below_cbor_limit, 64, 10)
        .expect("long but bounded transport sequence");
    assert!(encoded.seq_len() > MAX_WALLET_SEQ_LEN);
    assert_eq!(
        WalletDecoder::new().receive(&encoded.next_part()),
        Err(WalletRefusal::TooLarge),
        "declared sequence length is rejected before retaining fragments"
    );
}

#[test]
fn ur_40_incomplete_scans_and_too_many_parts_are_refused() {
    let mut decoder = WalletDecoder::new();
    assert_eq!(decoder.finish(), Err(WalletRefusal::Incomplete));
    for _ in 0..MAX_WALLET_PARTS {
        let _ = decoder.receive("not-a-ur");
    }
    assert_eq!(
        decoder.receive("not-a-ur"),
        Err(WalletRefusal::SequenceLimit)
    );
}

#[test]
fn ur_34_wallet_decoder_handles_repeats_order_and_fountain_parity() {
    let mut account_decoder = Decoder::new();
    account_decoder
        .receive(ACCOUNT_UR)
        .expect("official account");
    let (_, payload) = account_decoder.result().expect("account CBOR");
    let mut encoder = Encoder::new("crypto-account", &payload, 80, 10).expect("encoder");
    let sequence_len = encoder.seq_len();
    assert!(sequence_len > 1);

    let mut parts = Vec::new();
    for _ in 0..(sequence_len * 20) {
        parts.push(encoder.next_part());
    }
    // Skip all systematic fragments. The decoder must recover from parity
    // fragments, out-of-order delivery, and repeated parts.
    let mut decoder = WalletDecoder::new();
    let parity = parts.iter().skip(sequence_len).rev().collect::<Vec<_>>();
    let mut terminal = None;
    for part in parity.iter().chain(parity.iter().take(2)) {
        match decoder.receive(part) {
            Ok(progress) if progress.complete => {
                terminal = Some(progress);
                break;
            }
            Ok(_) => {}
            Err(error) => panic!("unexpected transport refusal: {error:?}"),
        }
    }
    let progress = terminal.expect("parity fragments recover the account");
    assert!(progress.complete);
    let text = decoder.finish().expect("validated account");
    assert!(text.as_str().contains(BIP84_XPUB));
    assert!(ImportedWallet::read(text.as_str()).is_ok());
    assert!(
        decoder
            .receive(&parts[sequence_len])
            .expect("repeat")
            .complete
    );
    assert_eq!(decoder.result(), Some(&text));
}

#[test]
fn ur_40_repeats_and_bad_parts_preserve_an_incomplete_wallet_scan() {
    let mut account_decoder = Decoder::new();
    account_decoder
        .receive(ACCOUNT_UR)
        .expect("official account");
    let (_, payload) = account_decoder.result().expect("account CBOR");
    let mut encoder = Encoder::new("crypto-account", &payload, 80, 10).expect("encoder");
    assert!(encoder.seq_len() > 1);
    let first_part = encoder.next_part();

    let mut decoder = WalletDecoder::new();
    let initial = decoder.receive(&first_part).expect("first valid part");
    assert!(!initial.complete);
    assert!(matches!(
        decoder.receive("not-a-ur"),
        Err(WalletRefusal::Transport(_))
    ));
    assert_eq!(decoder.receive(&first_part), Ok(initial));
    assert!(decoder.result().is_none());
    assert_eq!(decoder.finish(), Err(WalletRefusal::Incomplete));

    let mut repeated = WalletDecoder::new();
    for _ in 0..MAX_WALLET_PARTS {
        assert!(
            !repeated
                .receive(&first_part)
                .expect("repeat is a valid, non-completing scan")
                .complete
        );
    }
    assert_eq!(
        repeated.receive(&first_part),
        Err(WalletRefusal::SequenceLimit)
    );
    assert_eq!(repeated.finish(), Err(WalletRefusal::Incomplete));
}
