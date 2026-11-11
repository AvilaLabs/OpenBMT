//! A cosigner key from a code (UR-23 to UR-30). The keys are made from a
//! fixed seed with an independent BIP 32 implementation (a few lines of
//! Python with its own secp256k1 and base58), and the codes are laid out
//! as each wallet writes them: Keystone, Jade and SeedSigner, Foundation
//! Passport's account profile for Liana, a lone hdkey, and the JSON of a
//! Coldcard-style export. The layouts are from the sources read for
//! `docs/UR-RULES.md`, section 7; the key bytes in them are the keys here.

use crate::base58::{decode_check, encode_check};
use crate::cbor::{encode_bytes, push_head};
use crate::cbor_read::{self, Item, MAX_DEPTH, MAX_ITEMS};
use crate::point::is_compressed_point;
use crate::{ur_to_key_text, KeyRefusal, ALL_KEY_REFUSALS, MAX_KEY_CBOR};

/// xprv, tprv, yprv, zprv, uprv, vprv, Yprv, Zprv, Uprv, Vprv (SLIP-0132):
/// the list of `openbmt-signer`'s key ceremony tests, which cannot be shared
/// as this crate has no dependency.
const SLIP_SECRET: [[u8; 4]; 10] = [
    [0x04, 0x88, 0xAD, 0xE4],
    [0x04, 0x35, 0x83, 0x94],
    [0x04, 0x9D, 0x78, 0x78],
    [0x04, 0xB2, 0x43, 0x0C],
    [0x04, 0x4A, 0x4E, 0x28],
    [0x04, 0x5F, 0x18, 0xBC],
    [0x02, 0x95, 0xB0, 0x05],
    [0x02, 0xAA, 0x7A, 0x99],
    [0x02, 0x42, 0x85, 0xB5],
    [0x02, 0x57, 0x50, 0x48],
];

// ---- the keys ------------------------------------------------------------

/// m/48'/0'/0'/2' of the master key of seed [3; 32].
pub(super) const MAIN_FP: u32 = 0x56c4_fac3;
pub(super) const MAIN_PARENT: u32 = 0xc074_36a2;
pub(super) const MAIN_KEY: &str =
    "02d44daf959d7b3bf9343e2aea84ea21391ccf306a2765c35859c94cecb870aaea";
pub(super) const MAIN_CHAIN: &str =
    "f4ef1818131a780976e9dc1f30a4bd39903a9b8bbffad099035c49f886e85aed";
pub(super) const MAIN_XPUB: &str = "xpub6Ewx2N9hNSArJyF35CUGhaZLuZxQPNmJzWVwmpoV9U7Xu5wqka93nd3zEzokew9MzkNV4u6TCVDkHHR6QHQuYEFaasKzWkrkncXHMXGNdZP";
const MAIN_ZPUB: &str = "Zpub75WZkwDyEkpBS8negaWVwq69yJHZUyRu91C4FsroHFhhdU9D2HrT7sEC67gQDBg73Vg5SSHh12KMB2Fz8uPtHBjTA1qF5yxjDnvSidAz9yt";

/// m/48'/1'/0'/2' of the same master key.
pub(super) const TEST_PARENT: u32 = 0x686a_e4bf;
pub(super) const TEST_KEY: &str =
    "0241ea5cb48ecc741308624baf221aa4e3057447056e4fa48749572714abff29f1";
pub(super) const TEST_CHAIN: &str =
    "d0b08dc903742e8e907aed44ced618ec55545adf33a8f3cfe444470bb19fe79b";
pub(super) const TEST_TPUB: &str = "tpubDEg3kqr2jo5ergkJbFqRHvCpiob7wR7Hi44J7y987G1JZfbzBND77XKTyPZzGvh3uyDf8kexMJnFD9W8FuraJ4wLMsx6YuZVXRSRRcx6QdD";

/// m/48'/0'/0'/2' of the master key of seed [7; 32].
pub(super) const OTHER_FP: u32 = 0xe286_7bb6;
pub(super) const OTHER_PARENT: u32 = 0xd51a_d7f4;
pub(super) const OTHER_KEY: &str =
    "037b73c092866bc4d0ed4a38bdeb8fd92e6f50da46b8e0df3ae383b2a84aeec26b";
pub(super) const OTHER_CHAIN: &str =
    "364c446c3033c79f7b7b30617e8abaf7aa02b67cd15a294dcadcf0866c37d7a3";
pub(super) const OTHER_XPUB: &str = "xpub6F6kdziwgeYBty6BhQQxf73MxQDNPcKKApsvENDUEx3mBPumWC4GcBFi7gk8wE7r2V4gKRmC92UXPWPxz7HxP3U4n46xutvWWLsUAWswLWe";

fn main_text() -> String {
    format!("[56c4fac3/48h/0h/0h/2h]{MAIN_XPUB}")
}

fn test_text() -> String {
    format!("[56c4fac3/48h/1h/0h/2h]{TEST_TPUB}")
}

fn other_text() -> String {
    format!("[e2867bb6/48h/0h/0h/2h]{OTHER_XPUB}")
}

// ---- building CBOR -------------------------------------------------------

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

fn head(major: u8, n: u64) -> Vec<u8> {
    let mut v = Vec::new();
    push_head(&mut v, major, n);
    v
}

fn uint(n: u64) -> Vec<u8> {
    head(0, n)
}

fn bytes(data: &[u8]) -> Vec<u8> {
    encode_bytes(data)
}

fn tag(t: u64, item: Vec<u8>) -> Vec<u8> {
    [head(6, t), item].concat()
}

fn array(items: &[Vec<u8>]) -> Vec<u8> {
    [head(4, items.len() as u64), items.concat()].concat()
}

/// A map from entries already in key order.
fn map(entries: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut v = head(5, entries.len() as u64);
    for (k, item) in entries {
        v.extend(uint(*k));
        v.extend(item);
    }
    v
}

const FALSE: [u8; 1] = [0xf4];
const TRUE: [u8; 1] = [0xf5];

/// The pieces of one hdkey, each one that can be left out or changed.
#[derive(Clone)]
struct Key {
    private: Option<bool>,
    key: Vec<u8>,
    chain: Option<Vec<u8>>,
    network: Option<(u64, u64)>,
    /// Tags of the coin info and of the origin: old or new.
    new_tags: bool,
    path: Vec<(u64, bool)>,
    origin_fp: Option<u32>,
    depth: Option<u64>,
    parent: Option<u32>,
}

fn path(coin: u64, account: u64) -> Vec<(u64, bool)> {
    vec![(48, true), (coin, true), (account, true), (2, true)]
}

impl Key {
    fn main() -> Self {
        Self {
            private: Some(false),
            key: unhex(MAIN_KEY),
            chain: Some(unhex(MAIN_CHAIN)),
            network: Some((0, 0)),
            new_tags: false,
            path: path(0, 0),
            origin_fp: Some(MAIN_FP),
            depth: Some(4),
            parent: Some(MAIN_PARENT),
        }
    }

    fn test() -> Self {
        Self {
            key: unhex(TEST_KEY),
            chain: Some(unhex(TEST_CHAIN)),
            network: Some((0, 1)),
            path: path(1, 0),
            parent: Some(TEST_PARENT),
            ..Self::main()
        }
    }

    fn other() -> Self {
        Self {
            key: unhex(OTHER_KEY),
            chain: Some(unhex(OTHER_CHAIN)),
            origin_fp: Some(OTHER_FP),
            parent: Some(OTHER_PARENT),
            ..Self::main()
        }
    }

    /// The map of the hdkey, with the tags of the coin info and the origin.
    fn map(&self) -> Vec<u8> {
        let (info_tag, path_tag) = if self.new_tags {
            (40305, 40304)
        } else {
            (305, 304)
        };
        let mut e: Vec<(u64, Vec<u8>)> = Vec::new();
        if let Some(p) = self.private {
            e.push((2, if p { TRUE } else { FALSE }.to_vec()));
        }
        e.push((3, bytes(&self.key)));
        if let Some(c) = &self.chain {
            e.push((4, bytes(c)));
        }
        if let Some((kind, net)) = self.network {
            e.push((5, tag(info_tag, map(&[(1, uint(kind)), (2, uint(net))]))));
        }
        let mut parts = Vec::new();
        for (index, hardened) in &self.path {
            parts.push(uint(*index));
            parts.push(if *hardened { TRUE } else { FALSE }.to_vec());
        }
        let mut origin = vec![(1, array(&parts))];
        if let Some(fp) = self.origin_fp {
            origin.push((2, uint(u64::from(fp))));
        }
        if let Some(d) = self.depth {
            origin.push((3, uint(d)));
        }
        e.push((6, tag(path_tag, map(&origin))));
        if let Some(p) = self.parent {
            e.push((8, uint(u64::from(p))));
        }
        map(&e)
    }

    /// `303(hdkey)`.
    fn tagged(&self) -> Vec<u8> {
        tag(303, self.map())
    }
}

/// The account of `fp` with these outputs.
fn account(fp: u32, outputs: &[Vec<u8>]) -> Vec<u8> {
    map(&[(1, uint(u64::from(fp))), (2, array(outputs))])
}

/// An output: the tags in front of `303(key)`, outermost first.
fn output(tags: &[u64], key: &Key) -> Vec<u8> {
    tags.iter()
        .rev()
        .fold(key.tagged(), |inner, t| tag(*t, inner))
}

fn read(ur_type: &str, cbor: &[u8]) -> Result<String, KeyRefusal> {
    ur_to_key_text(ur_type, cbor).map(|k| k.into_string())
}

fn account_text(cbor: &[u8]) -> Result<String, KeyRefusal> {
    read("crypto-account", cbor)
}

fn hdkey_text(cbor: &[u8]) -> Result<String, KeyRefusal> {
    read("crypto-hdkey", cbor)
}

// ---- UR-23: the strict reader -------------------------------------------------

#[test]
fn ur_23_the_subset_is_read() {
    let good = map(&[
        (1, uint(1_000_000)),
        (
            2,
            array(&[
                tag(303, bytes(&[1, 2, 3])),
                [0x63, b'a', b'b', b'c'].to_vec(),
            ]),
        ),
        (9, FALSE.to_vec()),
        (10, TRUE.to_vec()),
    ]);
    let item = cbor_read::read(&good).expect("read");
    assert_eq!(item.get(1).and_then(Item::uint), Some(1_000_000));
    assert!(matches!(item.get(2), Some(Item::Array(a)) if a.len() == 2));
    assert_eq!(item.get(9), Some(&Item::Bool(false)));
    assert_eq!(item.get(10), Some(&Item::Bool(true)));
    // Every width of an unsigned integer, in the shortest form.
    for (cbor, n) in [
        ("17", 23),
        ("1818", 24),
        ("18ff", 255),
        ("190100", 256),
        ("19ffff", 65_535),
        ("1a00010000", 65_536),
        ("1affffffff", 4_294_967_295),
        ("1b0000000100000000", 4_294_967_296),
    ] {
        assert_eq!(
            cbor_read::read(&unhex(cbor)).expect(cbor),
            Item::Uint(n),
            "{cbor}"
        );
    }
}

#[test]
fn ur_23_what_is_not_in_the_subset_is_refused() {
    for (what, cbor) in [
        ("a negative integer", "20"),
        ("a float", "f93c00"),
        ("a double", "fb3ff0000000000000"),
        ("null", "f6"),
        ("undefined", "f7"),
        ("a simple value of one byte", "f814"),
        ("a break", "ff"),
        ("an indefinite array", "9f01ff"),
        ("an indefinite map", "bf0101ff"),
        ("an indefinite byte string", "5f4101ff"),
        ("an indefinite text", "7f6161ff"),
        ("a longer head than needed", "1801"),
        ("a longer head than needed for 255", "190001"),
        ("a longer head than needed for 65535", "1a0000ffff"),
        ("a longer head than needed for a length", "5801aa"),
        ("bytes after the item", "0100"),
        ("a cut head", "19ff"),
        ("a byte string cut short", "4401"),
        ("a text that is not UTF-8", "62c328"),
        ("an empty message", ""),
        ("an array longer than the message", "8301"),
        ("a map longer than the message", "a2010203"),
        ("a text key", "a161610a"),
        ("a negative key", "a1200a"),
        ("a tagged key", "a1d9012f0a"),
        ("keys not ascending", "a2020a010a"),
        ("a duplicate key", "a2010a010b"),
    ] {
        assert!(cbor_read::read(&unhex(cbor)).is_err(), "{what}");
    }
}

#[test]
fn ur_23_depth_and_items_are_bounded() {
    // MAX_DEPTH arrays are read; one more is not.
    let nest = |n: usize| {
        let mut v = vec![0x81; n];
        v.push(0x00);
        v
    };
    assert!(cbor_read::read(&nest(MAX_DEPTH)).is_ok());
    assert!(cbor_read::read(&nest(MAX_DEPTH + 1)).is_err());
    let maps = |n: usize| {
        let mut v = Vec::new();
        for _ in 0..n {
            v.extend([0xa1, 0x01]);
        }
        v.push(0x00);
        v
    };
    assert!(cbor_read::read(&maps(MAX_DEPTH)).is_ok());
    assert!(cbor_read::read(&maps(MAX_DEPTH + 1)).is_err());
    // Tags in a row: eight, not nine.
    let tags = |n: usize| {
        let mut v = vec![0xc1; n];
        v.push(0x00);
        v
    };
    assert!(cbor_read::read(&tags(8)).is_ok());
    assert!(cbor_read::read(&tags(9)).is_err());
    // A message of a million bytes that is all items is refused for its
    // item count, and holds nothing like a million items.
    let mut many = vec![0x9a, 0x00, 0x0f, 0x42, 0x3f];
    many.extend(std::iter::repeat_n(0u8, 1_000_000));
    assert!(cbor_read::read(&many).is_err());
    let mut ok = head(4, (MAX_ITEMS - 1) as u64);
    ok.extend(std::iter::repeat_n(0u8, MAX_ITEMS - 1));
    assert!(cbor_read::read(&ok).is_ok());
    let mut over = head(4, MAX_ITEMS as u64);
    over.extend(std::iter::repeat_n(0u8, MAX_ITEMS));
    assert!(cbor_read::read(&over).is_err());
    // A length that claims more than a machine holds.
    assert!(cbor_read::read(&unhex("5bffffffffffffffff")).is_err());
    assert!(cbor_read::read(&unhex("9bffffffffffffffff")).is_err());
}

#[test]
fn ur_23_a_message_over_the_limit_is_refused_before_it_is_read() {
    let big = vec![0u8; MAX_KEY_CBOR + 1];
    for t in ["crypto-account", "crypto-hdkey", "bytes"] {
        assert_eq!(read(t, &big), Err(KeyRefusal::Malformed), "{t}");
    }
    assert_eq!(read("crypto-account", &[]), Err(KeyRefusal::Malformed));
    assert_eq!(
        read("crypto-hdkey", &unhex("f6")),
        Err(KeyRefusal::Malformed)
    );
    // Bytes after the account.
    let mut cbor = account(MAIN_FP, &[output(&[401], &Key::main())]);
    cbor.push(0);
    assert_eq!(account_text(&cbor), Err(KeyRefusal::Malformed));
}

// ---- UR-24: the codes that are read --------------------------------------------

/// The Keystone firmware's test vector for a P2WSH cosigner export
/// (keystone3-firmware, `rust/apps/bitcoin/src/multi_sig/mod.rs`): the
/// bytes of its layout, with the key of this file in it.
fn keystone() -> Vec<u8> {
    unhex(&format!(
        "a2011a56c4fac30281d90191d9012fa602f4035821{MAIN_KEY}045820{MAIN_CHAIN}\
         05d90131a20100020006d90130a301881830f500f500f502f5021a56c4fac3030408\
         1ac07436a2"
    ))
}

#[test]
fn ur_24_keystone_account_is_read() {
    assert_eq!(account_text(&keystone()), Ok(main_text()));
    // The same bytes built by the helpers of this file.
    let built = account(MAIN_FP, &[output(&[401], &Key::main())]);
    assert_eq!(built, keystone());
}

#[test]
fn ur_24_jade_and_seedsigner_accounts_are_read() {
    // Jade and SeedSigner: no is-private, no coin info on the main
    // network, the old tags, `401(303(key))` and no 308.
    let main = Key {
        private: None,
        network: None,
        ..Key::main()
    };
    let cbor = account(MAIN_FP, &[output(&[401], &main)]);
    assert_eq!(account_text(&cbor), Ok(main_text()));
    // On a test network the coin info is written, with network 1.
    let test = Key {
        private: None,
        ..Key::test()
    };
    let cbor = account(MAIN_FP, &[output(&[401], &test)]);
    assert_eq!(account_text(&cbor), Ok(test_text()));
    // A coin info that is an empty map is the main network.
    let body = map(&[
        (3, bytes(&unhex(MAIN_KEY))),
        (4, bytes(&unhex(MAIN_CHAIN))),
        (5, tag(305, map(&[]))),
        (
            6,
            tag(
                304,
                map(&[
                    (1, array(&path_items(&path(0, 0)))),
                    (2, uint(u64::from(MAIN_FP))),
                    (3, uint(4)),
                ]),
            ),
        ),
        (8, uint(u64::from(MAIN_PARENT))),
    ]);
    let cbor = account(MAIN_FP, &[tag(401, tag(303, body))]);
    assert_eq!(account_text(&cbor), Ok(main_text()));
}

fn path_items(path: &[(u64, bool)]) -> Vec<Vec<u8>> {
    path.iter()
        .flat_map(|(i, h)| [uint(*i), if *h { TRUE } else { FALSE }.to_vec()])
        .collect()
}

#[test]
fn ur_24_passport_liana_account_is_read() {
    // Passport's Liana profile: `308(401(410(303(key))))` with the new tags
    // 40305 and 40304 inside the key (passport2, `wallets/liana.py`).
    let key = Key {
        new_tags: true,
        ..Key::main()
    };
    let cbor = account(MAIN_FP, &[output(&[308, 401, 410], &key)]);
    assert_eq!(account_text(&cbor), Ok(main_text()));
    // Every spelling of the same output reads the same.
    for tags in [&[401][..], &[308, 401], &[401, 410], &[308, 401, 410]] {
        for new_tags in [false, true] {
            let key = Key {
                new_tags,
                ..Key::main()
            };
            let cbor = account(MAIN_FP, &[output(tags, &key)]);
            assert_eq!(account_text(&cbor), Ok(main_text()), "{tags:?} {new_tags}");
        }
    }
    // The hdkey under 40303 is the same key as under 303.
    let cbor = account(MAIN_FP, &[tag(401, tag(40303, Key::main().map()))]);
    assert_eq!(account_text(&cbor), Ok(main_text()));
    // An account in its own tag, 311, is the same account.
    let cbor = tag(311, account(MAIN_FP, &[output(&[401], &Key::main())]));
    assert_eq!(account_text(&cbor), Ok(main_text()));
}

#[test]
fn ur_24_a_lone_hdkey_is_read() {
    for key in [
        Key::main(),
        Key {
            new_tags: true,
            ..Key::main()
        },
    ] {
        let bare = key.map();
        assert_eq!(hdkey_text(&bare), Ok(main_text()));
        assert_eq!(hdkey_text(&tag(303, bare.clone())), Ok(main_text()));
        assert_eq!(hdkey_text(&tag(40303, bare.clone())), Ok(main_text()));
        // Another tag in front is not the type's own.
        assert_eq!(hdkey_text(&tag(311, bare.clone())), Err(KeyRefusal::Shape));
        assert_eq!(
            hdkey_text(&tag(303, tag(303, bare))),
            Err(KeyRefusal::Shape)
        );
    }
    assert_eq!(hdkey_text(&Key::test().map()), Ok(test_text()));
    assert_eq!(hdkey_text(&Key::other().map()), Ok(other_text()));
    // No account around it, so the fingerprint is the origin's alone.
    let mut no_fp = Key::main();
    no_fp.origin_fp = None;
    assert_eq!(hdkey_text(&no_fp.map()), Err(KeyRefusal::NoFingerprint));
}

#[test]
fn ur_24_the_newer_types_and_other_types_are_refused_with_their_own_words() {
    let cbor = Key::main().map();
    for t in ["hdkey", "account-descriptor", "output-descriptor"] {
        assert_eq!(read(t, &cbor), Err(KeyRefusal::NewKind), "{t}");
    }
    for t in [
        "crypto-output",
        "crypto-psbt",
        "psbt",
        "crypto-seed",
        "Crypto-Account",
        "",
        "x",
    ] {
        assert_eq!(read(t, &cbor), Err(KeyRefusal::OtherKind), "{t}");
    }
    assert_ne!(
        KeyRefusal::NewKind.words().what,
        KeyRefusal::OtherKind.words().what
    );
}

#[test]
fn ur_24_a_shape_that_is_not_read_is_refused() {
    let key = Key::main();
    for (what, cbor) in [
        ("an account that is an array", array(&[])),
        ("an account with a third key", {
            map(&[
                (1, uint(1)),
                (2, array(&[output(&[401], &key)])),
                (3, uint(1)),
            ])
        }),
        ("an account without its fingerprint", {
            map(&[(2, array(&[output(&[401], &key)]))])
        }),
        ("an account with no outputs", account(MAIN_FP, &[])),
        ("an account fingerprint that is zero", {
            account(0, &[output(&[401], &key)])
        }),
        ("an account fingerprint over 32 bits", {
            map(&[
                (1, uint(0x1_0000_0000)),
                (2, array(&[output(&[401], &key)])),
            ])
        }),
        (
            "an output that is not tagged",
            account(MAIN_FP, &[key.map()]),
        ),
        ("an output under the newer tag 40308", {
            account(MAIN_FP, &[tag(40308, tag(401, key.tagged()))])
        }),
        ("an output of an unknown tag", {
            account(MAIN_FP, &[tag(999, key.tagged())])
        }),
        ("an output of a number", account(MAIN_FP, &[uint(1)])),
        (
            "an hdkey that is a number",
            account(MAIN_FP, &[tag(401, tag(303, uint(1)))]),
        ),
    ] {
        assert_eq!(account_text(&cbor), Err(KeyRefusal::Shape), "{what}");
    }
    // A key of the hdkey that makes it a master key, and one that is not
    // defined.
    for (what, extra) in [("key 1", (1, TRUE.to_vec())), ("key 11", (11, uint(1)))] {
        let mut e = vec![(3, bytes(&unhex(MAIN_KEY)))];
        if extra.0 == 1 {
            e.insert(0, extra);
        } else {
            e.push(extra);
        }
        assert_eq!(hdkey_text(&map(&e)), Err(KeyRefusal::Shape), "{what}");
    }
}

// ---- UR-25: one key for the shared wallet --------------------------------------

#[test]
fn ur_25_the_other_outputs_are_ignored() {
    let wsh = output(&[401], &Key::main());
    // The account a signer writes for every script type of its first
    // account: wpkh, sh(wpkh), tr, wsh and sh(wsh).
    let wpkh_path = Key {
        path: vec![(84, true), (0, true), (0, true)],
        depth: Some(3),
        ..Key::main()
    };
    let cbor = account(
        MAIN_FP,
        &[
            output(&[404], &wpkh_path),
            output(&[400, 404], &wpkh_path),
            output(&[409], &wpkh_path),
            output(&[308, 400, 401, 410], &Key::main()),
            wsh.clone(),
            output(&[400, 401], &Key::main()),
            tag(
                401,
                tag(
                    407,
                    map(&[(1, uint(2)), (2, array(&[Key::main().tagged()]))]),
                ),
            ),
        ],
    );
    assert_eq!(account_text(&cbor), Ok(main_text()));
}

#[test]
fn ur_25_none_or_several_are_refused() {
    let wsh = |k: &Key| output(&[401], k);
    // None: only other scripts, or the wrong account.
    let wpkh = output(&[404], &Key::main());
    assert_eq!(
        account_text(&account(MAIN_FP, &[wpkh])),
        Err(KeyRefusal::NoSharedKey)
    );
    let second_account = Key {
        path: path(0, 1),
        ..Key::main()
    };
    assert_eq!(
        account_text(&account(MAIN_FP, &[wsh(&second_account)])),
        Err(KeyRefusal::NoSharedKey)
    );
    let single_sig_path = Key {
        path: vec![(84, true), (0, true), (0, true), (2, true)],
        ..Key::main()
    };
    assert_eq!(
        account_text(&account(MAIN_FP, &[wsh(&single_sig_path)])),
        Err(KeyRefusal::NoSharedKey)
    );
    // Several: the same key twice, or two keys.
    assert_eq!(
        account_text(&account(MAIN_FP, &[wsh(&Key::main()), wsh(&Key::main())])),
        Err(KeyRefusal::SeveralSharedKeys)
    );
    assert_eq!(
        account_text(&account(MAIN_FP, &[wsh(&Key::main()), wsh(&Key::test())])),
        Err(KeyRefusal::SeveralSharedKeys)
    );
    // One of them with the first account and others of another: one.
    assert_eq!(
        account_text(&account(
            MAIN_FP,
            &[wsh(&second_account), wsh(&Key::main())]
        )),
        Ok(main_text())
    );
    assert_ne!(
        KeyRefusal::NoSharedKey.words().heading,
        KeyRefusal::SeveralSharedKeys.words().heading
    );
}

// ---- UR-26: the key ---------------------------------------------------------------

#[test]
fn ur_26_a_secret_key_is_never_taken() {
    let secret = Key {
        private: Some(true),
        ..Key::main()
    };
    assert_eq!(hdkey_text(&secret.map()), Err(KeyRefusal::SecretKey));
    assert_eq!(
        account_text(&account(MAIN_FP, &[output(&[401], &secret)])),
        Err(KeyRefusal::SecretKey)
    );
    // Even with every other part right, and even for a secret key whose
    // data is not a public key.
    let with_private_data = Key {
        private: Some(true),
        key: vec![7; 32],
        ..Key::main()
    };
    assert_eq!(
        hdkey_text(&with_private_data.map()),
        Err(KeyRefusal::SecretKey)
    );
    // Key 2 that is not a boolean.
    let mut e = vec![(2, uint(0))];
    e.push((3, bytes(&unhex(MAIN_KEY))));
    assert_eq!(hdkey_text(&map(&e)), Err(KeyRefusal::Shape));
}

#[test]
fn ur_26_the_key_and_the_chain_code_must_be_there_and_valid() {
    let no_chain = Key {
        chain: None,
        ..Key::main()
    };
    assert_eq!(hdkey_text(&no_chain.map()), Err(KeyRefusal::NoChainCode));
    let short_chain = Key {
        chain: Some(vec![1; 31]),
        ..Key::main()
    };
    assert_eq!(hdkey_text(&short_chain.map()), Err(KeyRefusal::BadKeyBytes));
    let long_chain = Key {
        chain: Some(vec![1; 33]),
        ..Key::main()
    };
    assert_eq!(hdkey_text(&long_chain.map()), Err(KeyRefusal::BadKeyBytes));
    let mut uncompressed = vec![4u8];
    uncompressed.extend(unhex(MAIN_KEY).get(1..).expect("x"));
    for (what, key) in [
        ("an uncompressed prefix", uncompressed),
        ("32 bytes", unhex(MAIN_KEY)[1..].to_vec()),
        ("34 bytes", [unhex(MAIN_KEY), vec![0]].concat()),
        ("an empty key", vec![]),
        (
            "a prefix of 0",
            [vec![0], unhex(MAIN_KEY)[1..].to_vec()].concat(),
        ),
        (
            "an x that is on no curve",
            [vec![2], vec![0; 31], vec![5]].concat(),
        ),
        ("an x that is the prime", {
            let mut v = vec![2];
            v.extend(unhex(
                "fffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2f",
            ));
            v
        }),
        ("an x over the prime", {
            let mut v = vec![3];
            v.extend([0xff; 32]);
            v
        }),
    ] {
        let k = Key { key, ..Key::main() };
        assert_eq!(hdkey_text(&k.map()), Err(KeyRefusal::BadKeyBytes), "{what}");
    }
}

#[test]
fn ur_26_points_of_the_curve_are_told_from_the_rest() {
    let g = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    assert!(is_compressed_point(&unhex(g)));
    assert!(is_compressed_point(&unhex(&g.replacen("02", "03", 1))));
    assert!(is_compressed_point(&unhex(MAIN_KEY)));
    assert!(is_compressed_point(&unhex(OTHER_KEY)));
    // x of 1 to 4 are on the curve, 5 and 7 are not.
    let x = |n: u8| [vec![2], vec![0; 31], vec![n]].concat();
    for n in [1u8, 2, 3, 4, 6, 8] {
        assert!(is_compressed_point(&x(n)), "{n}");
    }
    for n in [5u8, 7, 9, 10, 11] {
        assert!(!is_compressed_point(&x(n)), "{n}");
    }
    assert!(!is_compressed_point(&[]));
    assert!(!is_compressed_point(&[2]));
}

#[test]
fn ur_26_the_field_arithmetic_is_right() {
    use crate::point::{mul_mod, P};
    let one = [1u64, 0, 0, 0];
    let minus_one = [P[0] - 1, P[1], P[2], P[3]];
    // (p - 1)^2 = 1, 1 * x = x, (p - 1) * 2 = p - 2.
    assert_eq!(mul_mod(&minus_one, &minus_one), one);
    let x = [
        0x0123_4567_89ab_cdef,
        0xfedc_ba98_7654_3210,
        7,
        0x7fff_ffff_ffff_fff0,
    ];
    assert_eq!(mul_mod(&one, &x), x);
    assert_eq!(mul_mod(&x, &one), x);
    assert_eq!(
        mul_mod(&minus_one, &[2, 0, 0, 0]),
        [P[0] - 2, P[1], P[2], P[3]]
    );
    // 2^255 * 2 = 2^256, which is 2^32 + 977 modulo p.
    assert_eq!(
        mul_mod(&[0, 0, 0, 1 << 63], &[2, 0, 0, 0]),
        [0x1_0000_03D1, 0, 0, 0]
    );
    // Commutative and associative on a spread of numbers.
    let mut v = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        v ^= v << 13;
        v ^= v >> 7;
        v ^= v << 17;
        v
    };
    for _ in 0..200 {
        let a = [next(), next(), next(), next() >> 1];
        let b = [next(), next(), next(), next() >> 1];
        let c = [next(), next(), next(), next() >> 1];
        assert_eq!(mul_mod(&a, &b), mul_mod(&b, &a));
        assert_eq!(mul_mod(&mul_mod(&a, &b), &c), mul_mod(&a, &mul_mod(&b, &c)));
    }
}

#[test]
fn ur_26_the_coin_is_bitcoin_on_main_or_test() {
    for (what, info) in [
        ("another coin", (1, 0)),
        ("another coin, test", (60, 1)),
        ("network 2", (0, 2)),
        ("network 255", (0, 255)),
    ] {
        let k = Key {
            network: Some(info),
            ..Key::main()
        };
        assert_eq!(hdkey_text(&k.map()), Err(KeyRefusal::BadCoinInfo), "{what}");
    }
    // A coin info that is not tagged or has a key of its own.
    let body = |info: Vec<u8>| {
        map(&[
            (3, bytes(&unhex(MAIN_KEY))),
            (4, bytes(&unhex(MAIN_CHAIN))),
            (5, info),
            (
                6,
                tag(
                    304,
                    map(&[
                        (1, array(&path_items(&path(0, 0)))),
                        (2, uint(u64::from(MAIN_FP))),
                    ]),
                ),
            ),
            (8, uint(u64::from(MAIN_PARENT))),
        ])
    };
    assert_eq!(
        hdkey_text(&body(map(&[(1, uint(0))]))),
        Err(KeyRefusal::Shape)
    );
    assert_eq!(
        hdkey_text(&body(tag(305, map(&[(3, uint(0))])))),
        Err(KeyRefusal::BadCoinInfo)
    );
    assert_eq!(
        hdkey_text(&body(tag(305, map(&[(2, uint(1))])))),
        Err(KeyRefusal::BadPath),
        "a test network with the path of the main one"
    );
    assert_eq!(
        hdkey_text(&body(tag(305, map(&[(1, TRUE.to_vec())])))),
        Err(KeyRefusal::BadCoinInfo)
    );
    assert_eq!(hdkey_text(&body(tag(40305, map(&[])))), Ok(main_text()));
}

// ---- UR-27: the origin -------------------------------------------------------------

#[test]
fn ur_27_the_origin_is_the_path_of_the_network() {
    // The main network with the test path, and the reverse.
    let wrong_coin = Key {
        path: path(1, 0),
        ..Key::main()
    };
    assert_eq!(hdkey_text(&wrong_coin.map()), Err(KeyRefusal::BadPath));
    let wrong_coin = Key {
        path: path(0, 0),
        ..Key::test()
    };
    assert_eq!(hdkey_text(&wrong_coin.map()), Err(KeyRefusal::BadPath));
    for (what, path) in [
        ("account 1", path(0, 1)),
        (
            "the script is 1",
            vec![(48, true), (0, true), (0, true), (1, true)],
        ),
        ("BIP 84", vec![(84, true), (0, true), (0, true), (2, true)]),
        ("three steps", vec![(48, true), (0, true), (0, true)]),
        (
            "five steps",
            vec![(48, true), (0, true), (0, true), (2, true), (0, false)],
        ),
        ("the root", vec![]),
        (
            "a step that is not hardened",
            vec![(48, true), (0, false), (0, true), (2, true)],
        ),
        (
            "the last step not hardened",
            vec![(48, true), (0, true), (0, true), (2, false)],
        ),
    ] {
        let k = Key {
            path,
            ..Key::main()
        };
        assert_eq!(hdkey_text(&k.map()), Err(KeyRefusal::BadPath), "{what}");
    }
    let wrong_depth = Key {
        depth: Some(3),
        ..Key::main()
    };
    assert_eq!(hdkey_text(&wrong_depth.map()), Err(KeyRefusal::BadPath));
    let no_depth = Key {
        depth: None,
        ..Key::main()
    };
    assert_eq!(hdkey_text(&no_depth.map()), Ok(main_text()));
}

#[test]
fn ur_27_the_origin_must_be_there_in_a_form_that_is_read() {
    // No key 6 at all.
    let body = map(&[
        (3, bytes(&unhex(MAIN_KEY))),
        (4, bytes(&unhex(MAIN_CHAIN))),
        (8, uint(u64::from(MAIN_PARENT))),
    ]);
    assert_eq!(hdkey_text(&body), Err(KeyRefusal::NoOrigin));
    // A keypath without components.
    let body = map(&[
        (3, bytes(&unhex(MAIN_KEY))),
        (4, bytes(&unhex(MAIN_CHAIN))),
        (6, tag(304, map(&[(2, uint(u64::from(MAIN_FP)))]))),
        (8, uint(u64::from(MAIN_PARENT))),
    ]);
    assert_eq!(hdkey_text(&body), Err(KeyRefusal::NoOrigin));
    // Components that are not pairs of an index and a flag: a range, an odd
    // count, a flag that is a number, an index over 31 bits.
    for parts in [
        array(&[array(&[uint(0), uint(1)]), TRUE.to_vec()]),
        array(&[uint(48), TRUE.to_vec(), uint(0)]),
        array(&[uint(48), uint(1)]),
        array(&[uint(0x8000_0000), TRUE.to_vec()]),
    ] {
        let body = map(&[
            (3, bytes(&unhex(MAIN_KEY))),
            (4, bytes(&unhex(MAIN_CHAIN))),
            (6, tag(304, map(&[(1, parts)]))),
            (8, uint(u64::from(MAIN_PARENT))),
        ]);
        assert_eq!(hdkey_text(&body), Err(KeyRefusal::Shape));
    }
    // An origin without its tag.
    let body = map(&[
        (3, bytes(&unhex(MAIN_KEY))),
        (4, bytes(&unhex(MAIN_CHAIN))),
        (6, map(&[(1, array(&path_items(&path(0, 0))))])),
        (8, uint(u64::from(MAIN_PARENT))),
    ]);
    assert_eq!(hdkey_text(&body), Err(KeyRefusal::Shape));
}

#[test]
fn ur_27_the_master_fingerprint_is_there_once_or_agrees() {
    // Only in the account.
    let only_account = Key {
        origin_fp: None,
        ..Key::main()
    };
    assert_eq!(
        account_text(&account(MAIN_FP, &[output(&[401], &only_account)])),
        Ok(main_text())
    );
    // Only in the origin: a lone key.
    assert_eq!(hdkey_text(&Key::main().map()), Ok(main_text()));
    // In both and the same.
    assert_eq!(
        account_text(&account(MAIN_FP, &[output(&[401], &Key::main())])),
        Ok(main_text())
    );
    // In both and not the same.
    assert_eq!(
        account_text(&account(OTHER_FP, &[output(&[401], &Key::main())])),
        Err(KeyRefusal::FingerprintsDiffer)
    );
    // The fingerprint of the origin that is zero or too large.
    let zero = Key {
        origin_fp: Some(0),
        ..Key::main()
    };
    assert_eq!(hdkey_text(&zero.map()), Err(KeyRefusal::Shape));
}

// ---- UR-28: the key written out ------------------------------------------------------

#[test]
fn ur_28_the_key_is_written_out_as_the_extended_public_key() {
    assert_eq!(hdkey_text(&Key::main().map()), Ok(main_text()));
    assert_eq!(hdkey_text(&Key::test().map()), Ok(test_text()));
    assert_eq!(hdkey_text(&Key::other().map()), Ok(other_text()));
    // The 78 bytes behind the text.
    let text = main_text();
    let raw = decode_check(text.split_once(']').expect("origin").1).expect("base58check");
    assert_eq!(raw.len(), 78);
    assert_eq!(&raw[..4], &[0x04, 0x88, 0xB2, 0x1E]);
    assert_eq!(raw[4], 4);
    assert_eq!(&raw[5..9], &MAIN_PARENT.to_be_bytes());
    assert_eq!(&raw[9..13], &0x8000_0002u32.to_be_bytes());
    assert_eq!(&raw[13..45], unhex(MAIN_CHAIN).as_slice());
    assert_eq!(&raw[45..], unhex(MAIN_KEY).as_slice());
    let test = test_text();
    let raw = decode_check(test.split_once(']').expect("origin").1).expect("base58check");
    assert_eq!(&raw[..4], &[0x04, 0x35, 0x87, 0xCF]);
}

#[test]
fn ur_28_without_a_parent_fingerprint_the_key_cannot_be_written() {
    let no_parent = Key {
        parent: None,
        ..Key::main()
    };
    assert_eq!(hdkey_text(&no_parent.map()), Err(KeyRefusal::NoParent));
    let zero = Key {
        parent: Some(0),
        ..Key::main()
    };
    assert_eq!(hdkey_text(&zero.map()), Err(KeyRefusal::Shape));
}

#[test]
fn ur_28_base58_check_round_trips_and_refuses_what_is_not() {
    for payload in [
        vec![0u8; 3],
        vec![0, 0, 1, 2, 3],
        vec![0xff; 78],
        (0..78).collect::<Vec<u8>>(),
        vec![1],
        vec![],
    ] {
        let text = encode_check(&payload).expect("encoded");
        assert_eq!(decode_check(&text), Some(payload), "{text}");
    }
    assert!(encode_check(&[0u8; 79]).is_none());
    // The known extended key.
    let raw = decode_check(MAIN_XPUB).expect("known");
    assert_eq!(encode_check(&raw).as_deref(), Some(MAIN_XPUB));
    // A changed letter, a letter outside the alphabet, a cut, nothing.
    let mut bad = MAIN_XPUB.to_string();
    bad.replace_range(10..11, if &bad[10..11] == "a" { "b" } else { "a" });
    assert!(decode_check(&bad).is_none());
    assert!(decode_check(&MAIN_XPUB.replace('x', "0")).is_none());
    assert!(decode_check(&MAIN_XPUB[1..]).is_none());
    assert!(decode_check(&format!("{MAIN_XPUB}1")).is_none());
    assert!(decode_check("").is_none());
    assert!(decode_check(&"1".repeat(200)).is_none());
    assert!(decode_check("é").is_none());
}

// ---- UR-29: the JSON export -----------------------------------------------------------

fn wrap(json: &str) -> Vec<u8> {
    encode_bytes(json.as_bytes())
}

fn json_text(cbor: &[u8]) -> Result<String, KeyRefusal> {
    read("bytes", cbor)
}

/// The export of a Coldcard-style wallet, with the key of this file.
fn coldcard(xfp: &str, deriv: &str, key: &str) -> String {
    format!(
        concat!(
            "{{\"p2sh_deriv\": \"m/45'\", \"p2sh\": \"{k}\", ",
            "\"p2wsh_p2sh_deriv\": \"m/48'/0'/0'/1'\", \"p2wsh_p2sh\": \"{k}\", ",
            "\"p2wsh_deriv\": \"{d}\", \"p2wsh\": \"{k}\", ",
            "\"account\": \"0\", \"xfp\": \"{x}\"}}"
        ),
        k = key,
        d = deriv,
        x = xfp
    )
}

#[test]
fn ur_29_a_coldcard_style_export_is_read() {
    let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", MAIN_XPUB);
    assert_eq!(json_text(&wrap(&json)), Ok(main_text()));
    // The same by `h` and with a lower-case fingerprint, white space around.
    let json = format!(
        "\n {}\r\n",
        coldcard("56c4fac3", "m/48h/0h/0h/2h", MAIN_XPUB)
    );
    assert_eq!(json_text(&wrap(&json)), Ok(main_text()));
    let json = coldcard("56c4fac3", "m/48'/1h/0'/2h", TEST_TPUB);
    assert_eq!(json_text(&wrap(&json)), Ok(test_text()));
    // A real Coldcard export writes p2wsh as a Zpub; it is the same key.
    let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", MAIN_ZPUB);
    assert_eq!(json_text(&wrap(&json)), Ok(main_text()));
    // Passport's file, with a firmware version and nested values the
    // reader walks past.
    let json = format!(
        "{{\"fw_version\": \"2.1.0\", \"xfp\": \"E2867BB6\", \"extra\": {{\"a\": [1, -2, true, null, {{\"b\": \"c\\u00e9\"}}]}}, \
         \"p2wsh_deriv\": \"m/48'/0'/0'/2'\", \"p2wsh\": \"{OTHER_XPUB}\", \"n\": 18446744073}}"
    );
    assert_eq!(json_text(&wrap(&json)), Ok(other_text()));
    // The same key as the account of a signer that writes the code.
    assert_eq!(
        json_text(&wrap(&coldcard("56C4FAC3", "m/48'/0'/0'/2'", MAIN_XPUB))),
        account_text(&keystone())
    );
}

#[test]
fn ur_29_json_that_is_not_strict_is_refused() {
    let good = coldcard("56C4FAC3", "m/48'/0'/0'/2'", MAIN_XPUB);
    for (what, json) in [
        ("empty", String::new()),
        ("an array", "[]".to_string()),
        ("a string", "\"x\"".to_string()),
        (
            "text",
            "BSMS 1.0\n00\n[56c4fac3/48h/0h/0h/2h]xpub\nlabel".to_string(),
        ),
        (
            "a trailing comma",
            good.replace("\"account\"", "\"account\" ,}")
                .replacen('}', "", 0),
        ),
        ("bytes after", format!("{good} x")),
        ("single quotes", good.replace('"', "'")),
        (
            "a duplicate key",
            good.replacen("\"account\"", "\"xfp\"", 1),
        ),
        (
            "a nested duplicate key",
            "{\"a\": {\"b\": 1, \"b\": 2}, \"xfp\": \"56c4fac3\"}".to_string(),
        ),
        ("a fraction", good.replace("\"0\"", "0.5")),
        ("an exponent", good.replace("\"0\"", "1e5")),
        ("a leading zero", good.replace("\"0\"", "01")),
        ("a bad escape", good.replace("\"0\"", "\"\\x\"")),
        ("a lone surrogate", good.replace("\"0\"", "\"\\ud800\"")),
        ("a control character", good.replace("\"0\"", "\"\u{1}\"")),
        ("a comment", format!("/* x */ {good}")),
        ("nested too deep", "{\"a\": [[[[[1]]]]]}".to_string()),
    ] {
        assert_eq!(
            json_text(&wrap(&json)),
            Err(KeyRefusal::JsonNotStrict),
            "{what}"
        );
    }
    // Not UTF-8.
    let mut bad = good.into_bytes();
    bad.push(0xff);
    assert_eq!(
        json_text(&encode_bytes(&bad)),
        Err(KeyRefusal::JsonNotStrict)
    );
    // Bytes that are not a CBOR byte string, or have more after.
    assert_eq!(json_text(&[]), Err(KeyRefusal::Malformed));
    assert_eq!(json_text(&uint(1)), Err(KeyRefusal::Malformed));
    let mut more = wrap("{}");
    more.push(0);
    assert_eq!(json_text(&more), Err(KeyRefusal::Malformed));
}

#[test]
fn ur_29_the_fields_must_be_there_and_in_form() {
    let good = coldcard("56C4FAC3", "m/48'/0'/0'/2'", MAIN_XPUB);
    let object = |fields: &[(&str, &str)]| {
        let body: Vec<String> = fields
            .iter()
            .map(|(k, v)| format!("\"{k}\": {v}"))
            .collect();
        format!("{{{}}}", body.join(", "))
    };
    let xfp = "\"56c4fac3\"";
    let deriv = "\"m/48'/0'/0'/2'\"";
    let key = format!("\"{MAIN_XPUB}\"");
    assert_eq!(
        json_text(&wrap(&object(&[
            ("xfp", xfp),
            ("p2wsh_deriv", deriv),
            ("p2wsh", &key)
        ]))),
        Ok(main_text())
    );
    for (what, json) in [
        ("no xfp", object(&[("p2wsh_deriv", deriv), ("p2wsh", &key)])),
        ("no path", object(&[("xfp", xfp), ("p2wsh", &key)])),
        ("no key", object(&[("xfp", xfp), ("p2wsh_deriv", deriv)])),
        ("nothing", "{}".to_string()),
        (
            "only the other keys",
            good.replace("p2wsh_deriv", "p2wsh_x")
                .replace("\"p2wsh\"", "\"p2wsh_y\"")
                .replace("\"xfp\"", "\"xfp_z\""),
        ),
    ] {
        assert_eq!(
            json_text(&wrap(&json)),
            Err(KeyRefusal::JsonFields),
            "{what}"
        );
    }
    for xfp in [
        "",
        "56C4FAC",
        "56C4FAC30",
        "56C4FAG3",
        "00000000",
        "+6C4FAC3",
        " 56C4FAC3",
    ] {
        let json = coldcard(xfp, "m/48'/0'/0'/2'", MAIN_XPUB);
        assert_eq!(
            json_text(&wrap(&json)),
            Err(KeyRefusal::JsonFields),
            "{xfp:?}"
        );
    }
    // A field that is a number or a list or null.
    for json in [
        good.replace("\"56C4FAC3\"", "1450707651"),
        good.replace("\"56C4FAC3\"", "[\"56C4FAC3\"]"),
        good.replace("\"m/48'/0'/0'/2'\"", "null"),
    ] {
        assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::JsonFields));
    }
}

#[test]
fn ur_29_the_path_must_be_the_shared_wallet_path_of_the_network() {
    for deriv in [
        "m/48'/0'/0'/1'",
        "m/48'/0'/1'/2'",
        "m/84'/0'/0'/2'",
        "m/48'/2'/0'/2'",
        "m/48'/0'/0'",
        "m/48'/0'/0'/2'/0",
        "m/48'/0'/0'/2",
        "48'/0'/0'/2'",
        "m/48'/0'/0'/02'",
        "m/48'/00'/0'/2'",
        "m/48'/0'/0'/2'x",
        "M/48'/0'/0'/2'",
        "m/48'//0'/0'/2'",
        "m/48'/-0'/0'/2'",
        "",
    ] {
        let json = coldcard("56C4FAC3", deriv, MAIN_XPUB);
        assert_eq!(
            json_text(&wrap(&json)),
            Err(KeyRefusal::BadPath),
            "{deriv:?}"
        );
    }
    // The main path with a test key and the reverse.
    let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", TEST_TPUB);
    assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::BadPath));
    let json = coldcard("56C4FAC3", "m/48'/1'/0'/2'", MAIN_XPUB);
    assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::BadPath));
}

#[test]
fn ur_29_the_key_must_be_a_plain_public_account_key() {
    // Zpub and Vpub are read as the plain key, as the key ceremony reads them.
    let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", MAIN_ZPUB);
    assert_eq!(json_text(&wrap(&json)), Ok(main_text()), "a Zpub");
    let vpub = "Vpub5mXykWQxJ7bcJsLbR1k4Vu2FmYEqH9JpeU5E7tZkP7L4LMsjNERAp5CHVeXWK3esYFX7tox5n6dbsZQ9itLFxeEWQ4vM6xjHHjzbUQ7UmBv";
    let json = coldcard("56C4FAC3", "m/48'/1'/0'/2'", vpub);
    assert_eq!(json_text(&wrap(&json)), Ok(test_text()), "a Vpub");
    // A Zpub for a test path, a Vpub for a main path: the network must match.
    let json = coldcard("56C4FAC3", "m/48'/1'/0'/2'", MAIN_ZPUB);
    assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::BadPath));
    let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", vpub);
    assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::BadPath));
    // Only the four version bytes change.
    let raw = decode_check(MAIN_XPUB).expect("xpub");
    let zraw = decode_check(MAIN_ZPUB).expect("Zpub");
    assert_eq!(&zraw[4..], &raw[4..]);
    // Every other SLIP-0132 public form is refused with its own words.
    for version in [
        [0x04u8, 0x9D, 0x7C, 0xB2],
        [0x04, 0xB2, 0x47, 0x46],
        [0x02, 0x95, 0xB4, 0x3F],
        [0x04, 0x4A, 0x52, 0x62],
        [0x04, 0x5F, 0x1C, 0xF6],
        [0x02, 0x42, 0x89, 0xEF],
    ] {
        let text = encode_check(&[version.as_slice(), &raw[4..]].concat()).expect("text");
        let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", &text);
        assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::JsonSpecialForm));
    }
    // A secret key of any of the ten versions of SLIP-0132 (the list of
    // the key ceremony's tests in openbmt-signer is the same).
    for version in SLIP_SECRET {
        let text = encode_check(&[version.as_slice(), &raw[4..]].concat()).expect("text");
        let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", &text);
        assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::SecretKey));
    }
    // Not a key at all, a cut key, a key of the wrong level, a key that
    // is not a point, a version nobody uses.
    let mut at_depth_3 = raw.clone();
    at_depth_3[4] = 3;
    let mut other_child = raw.clone();
    other_child[12] = 3;
    let mut bad_point = raw.clone();
    bad_point[45] = 4;
    let mut other_version = raw.clone();
    other_version[3] ^= 1;
    for (what, text) in [
        ("words", "not a key".to_string()),
        ("a cut key", MAIN_XPUB[..60].to_string()),
        ("a changed letter", MAIN_XPUB.replace("xpub6E", "xpub6F")),
        ("depth 3", encode_check(&at_depth_3).expect("text")),
        ("child 3", encode_check(&other_child).expect("text")),
        ("a prefix of 4", encode_check(&bad_point).expect("text")),
        (
            "a strange version",
            encode_check(&other_version).expect("text"),
        ),
        ("a short key", encode_check(&raw[..77]).expect("text")),
    ] {
        let json = coldcard("56C4FAC3", "m/48'/0'/0'/2'", &text);
        assert_eq!(json_text(&wrap(&json)), Err(KeyRefusal::JsonKey), "{what}");
    }
}

// ---- UR-30: the words, and no panic ---------------------------------------------------------

#[test]
fn ur_30_every_refusal_has_words_that_pass_the_screen_checks() {
    use openbmt_core::say::{check, check_distinct, Tone, Who, Words};
    let mut all = Vec::new();
    for r in ALL_KEY_REFUSALS {
        let w = r.words();
        let words =
            Words::new(Tone::Stop, w.heading, w.what, Who::KeyHolder, w.next).code(r.rule());
        check(&words).unwrap_or_else(|e| panic!("{r:?}: {e}"));
        all.push((format!("{r:?}"), words));
        // Rule numbers are for the log, not the sentences.
        for text in [w.heading, w.what, w.next] {
            assert!(!text.contains("UR-"), "{r:?}");
        }
        let shown = r.to_string();
        assert!(shown.starts_with(r.rule()), "{shown}");
        assert!(shown.len() > r.rule().len() + 2, "{shown}");
    }
    check_distinct(&all).expect("distinct");
    // The other apps' wording for a person who is not the key holder works
    // too: the sentences name no one.
    for r in ALL_KEY_REFUSALS {
        let w = r.words();
        let words = Words::new(Tone::Stop, w.heading, w.what, Who::Owner, w.next);
        check(&words).expect("owner");
    }
}

#[test]
fn ur_30_the_list_of_refusals_is_every_variant() {
    // An exhaustive match: a new variant does not compile until it is
    // counted here, and then the list must have it.
    let index = |r: KeyRefusal| -> usize {
        match r {
            KeyRefusal::Malformed => 0,
            KeyRefusal::NewKind => 1,
            KeyRefusal::OtherKind => 2,
            KeyRefusal::Shape => 3,
            KeyRefusal::NoSharedKey => 4,
            KeyRefusal::SeveralSharedKeys => 5,
            KeyRefusal::SecretKey => 6,
            KeyRefusal::NoChainCode => 7,
            KeyRefusal::BadKeyBytes => 8,
            KeyRefusal::BadCoinInfo => 9,
            KeyRefusal::NoOrigin => 10,
            KeyRefusal::BadPath => 11,
            KeyRefusal::NoFingerprint => 12,
            KeyRefusal::FingerprintsDiffer => 13,
            KeyRefusal::NoParent => 14,
            KeyRefusal::JsonNotStrict => 15,
            KeyRefusal::JsonFields => 16,
            KeyRefusal::JsonSpecialForm => 17,
            KeyRefusal::JsonKey => 18,
        }
    };
    let mut seen = [false; 19];
    for r in ALL_KEY_REFUSALS {
        seen[index(r)] = true;
    }
    assert!(seen.iter().all(|s| *s));
    assert_eq!(ALL_KEY_REFUSALS.len(), 19);
}

#[test]
fn ur_30_no_input_panics_and_what_is_taken_is_key_text() {
    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("crypto-account", keystone()),
        ("crypto-hdkey", Key::main().map()),
        ("crypto-hdkey", Key::test().tagged()),
        (
            "bytes",
            wrap(&coldcard("56C4FAC3", "m/48'/0'/0'/2'", MAIN_XPUB)),
        ),
    ];
    let mut taken = 0;
    for (t, seed) in &seeds {
        // Every cut, every changed byte, every byte inserted.
        for cut in 0..=seed.len() {
            check_outcome(t, &seed[..cut], &mut taken);
        }
        for at in 0..seed.len() {
            for value in [
                0x00u8, 0x01, 0x18, 0x1a, 0x5f, 0x7f, 0x9f, 0xa0, 0xbf, 0xd9, 0xf4, 0xf5, 0xf6,
                0xff,
            ] {
                let mut v = seed.clone();
                v[at] = value;
                check_outcome(t, &v, &mut taken);
            }
            let mut v = seed.clone();
            v.insert(at, 0x81);
            check_outcome(t, &v, &mut taken);
        }
    }
    assert!(taken > 4, "the seeds themselves are taken");
}

/// Whatever comes out is a refusal with words, or key text of the form the
/// ceremony reads.
fn check_outcome(ur_type: &str, cbor: &[u8], taken: &mut usize) {
    match ur_to_key_text(ur_type, cbor) {
        Ok(key) => {
            *taken += 1;
            let text = key.as_str();
            let (origin, xpub) = text.split_once(']').expect("an origin");
            assert!(origin.starts_with('['), "{text}");
            assert!(
                origin.ends_with("/48h/0h/0h/2h") || origin.ends_with("/48h/1h/0h/2h"),
                "{text}"
            );
            assert_eq!(origin.len(), "[56c4fac3/48h/0h/0h/2h".len(), "{text}");
            assert!(
                xpub.starts_with("xpub") || xpub.starts_with("tpub"),
                "{text}"
            );
            assert_eq!(decode_check(xpub).map(|r| r.len()), Some(78), "{text}");
        }
        Err(e) => {
            assert!(e.to_string().starts_with(e.rule()));
            assert!(!e.words().what.is_empty());
        }
    }
}
