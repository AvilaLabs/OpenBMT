//! The owner phone's part of the key ceremony (`docs/DESIGN.md` §2.1 steps
//! 4 and 5, `docs/OWNER-COINS-RULES.md` OC-40 to OC-46): ops 80 to 85.
//!
//! The ops work on no node: the ceremony happens before the node starts.
//! Ops 80 to 85 are the steps; op 86 gives the values the last step is
//! given besides the owner's word.
//! What is kept between two steps is the sealed record (`OBMu`, see
//! `openbmt_owner::ceremony`), which the app seals and hands back with each
//! call. It holds the owner's secret and key 1's account key, and it leaves
//! the library only in these results.

use std::fmt;

use bitcoin::Network;
use openbmt_core::say::Words;
use openbmt_counter_node::BATCH_MIN_COUNT;
use openbmt_owner::ceremony::{CeremonyError, Holder, OwnerSetup, SetupStage};
use openbmt_signer::{KeyAnswer, KeyPart};
use openbmt_watch::headers::MAINNET_CHECKPOINT_HEIGHT;

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_words, AppError, Kind};
use crate::views::{OwnerNetwork, WordsView};

/// What `ceremony_make_key1` is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key1Args {
    /// The network the shop is on.
    pub network: OwnerNetwork,
    /// Return key 1's 24 words, once. By default key 1 has none to show
    /// (`docs/DESIGN.md` §5.4).
    pub show_words: bool,
}

wire_record_get!(Key1Args {
    network,
    show_words
});
wire_record!(Key1Args {
    network,
    show_words
});

/// A key's public parts, for a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartView {
    /// The master fingerprint, 8 hex characters.
    pub fingerprint: String,
    /// The origin path of the account key, for example `48h/1h/0h/2h`.
    pub origin_path: String,
    /// The account-level extended public key.
    pub account_xpub: String,
}

wire_record!(PartView {
    fingerprint,
    origin_path,
    account_xpub,
});

impl PartView {
    fn new(k: &KeyPart) -> Self {
        Self {
            fingerprint: k.fingerprint.to_string(),
            origin_path: k.origin_path(),
            account_xpub: k.xpub.to_string(),
        }
    }
}

/// What `ceremony_make_key1` returns.
#[derive(Clone, PartialEq, Eq)]
pub struct Key1Made {
    /// The sealed record at stage 1, for the app to seal and keep. It holds
    /// secrets.
    pub record: Vec<u8>,
    /// Key 1's public parts: the short fingerprint is shown.
    pub key1: PartView,
    /// The owner's root, 32 bytes.
    pub owner_root: Vec<u8>,
    /// The owner's root as four words, to compare.
    pub owner_root_words: Vec<String>,
    /// Key 1's 24 words, only when asked for. Shown once, never kept.
    pub words: Option<Vec<String>>,
    /// The words for this stage of the setup.
    pub state_words: WordsView,
}

impl fmt::Debug for Key1Made {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Key1Made")
            .field(
                "record",
                &format_args!("<{} bytes, not shown>", self.record.len()),
            )
            .field("key1", &self.key1)
            .field("owner_root", &self.owner_root)
            .field("owner_root_words", &self.owner_root_words)
            .field(
                "words",
                &self
                    .words
                    .as_ref()
                    .map(|w| format!("<{} words, not shown>", w.len())),
            )
            .field("state_words", &self.state_words)
            .finish()
    }
}

wire_record!(Key1Made {
    record,
    key1,
    owner_root,
    owner_root_words,
    words,
    state_words,
});

/// What `ceremony_read_answer` returns: the Key app's answer, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerView {
    /// The network the answer is for. A Key app with no key 2 can say only
    /// the kind of network: mainnet, or testnet for all the others.
    pub network: OwnerNetwork,
    /// Key 2, when the Key app holds it.
    pub key2: Option<PartView>,
    /// Key 3, as the Key app took it.
    pub key3: PartView,
    /// The Key app's root, 32 bytes.
    pub key_app_root: Vec<u8>,
    /// The Key app's root as four words, to compare.
    pub key_app_root_words: Vec<String>,
}

wire_record!(AnswerView {
    network,
    key2,
    key3,
    key_app_root,
    key_app_root_words,
});

/// What `ceremony_build_wallet` is given.
#[derive(Clone, PartialEq, Eq)]
pub struct BuildArgs {
    /// The sealed record, stage 1 or 2.
    pub record: Vec<u8>,
    /// The Key app's answer, as scanned.
    pub answer: Vec<u8>,
    /// Key 2 as text, `[fingerprint/path]key`, when the Key app holds none.
    pub key2_text: Option<String>,
    /// The fewest addresses a batch may hold.
    pub batch_min_count: u32,
}

impl fmt::Debug for BuildArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildArgs")
            .field(
                "record",
                &format_args!("<{} bytes, not shown>", self.record.len()),
            )
            .field(
                "answer",
                &format_args!("<{} bytes, not shown>", self.answer.len()),
            )
            .field("key2_text", &self.key2_text)
            .field("batch_min_count", &self.batch_min_count)
            .finish()
    }
}

wire_record_get!(BuildArgs {
    record,
    answer,
    key2_text,
    batch_min_count,
});
wire_record!(BuildArgs {
    record,
    answer,
    key2_text,
    batch_min_count,
});

/// One pair of keys. The holders are 1 the owner's phone, 2 the Key app,
/// 3 a signer made by someone else that holds key 2, 4 the wallet app of
/// another publisher that made key 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairView {
    /// The first key of the pair, 1 to 3.
    pub key_a: u32,
    /// The second key of the pair.
    pub key_b: u32,
    /// Who holds the first key.
    pub holder_a: u8,
    /// Who holds the second key.
    pub holder_b: u8,
}

wire_record!(PairView {
    key_a,
    key_b,
    holder_a,
    holder_b,
});

fn holder_code(h: Holder) -> u8 {
    match h {
        Holder::OwnerPhone => 1,
        Holder::KeyApp => 2,
        Holder::OtherSigner => 3,
        Holder::OtherWalletApp => 4,
    }
}

/// What `ceremony_build_wallet` returns.
#[derive(Clone, PartialEq, Eq)]
pub struct WalletBuiltView {
    /// The sealed record at stage 2, for the app to seal and keep until the
    /// owner confirms.
    pub record: Vec<u8>,
    /// The wallet setup (`OBMw`) for the Key app, to show as a code.
    pub wallet_setup: Vec<u8>,
    /// The wallet description with its checksum, as text.
    pub description: String,
    /// The four checksum words to compare on every device.
    pub checksum_words: Vec<String>,
    /// The three master fingerprints, keys 1, 2 and 3.
    pub fingerprints: Vec<String>,
    /// The first receive address, to compare.
    pub first_receive_address: String,
    /// The pairs screen: every pair of keys and who holds each.
    pub pairs: Vec<PairView>,
    /// The words of the pairs screen.
    pub pairs_words: WordsView,
    /// The words for this stage of the setup.
    pub state_words: WordsView,
}

impl fmt::Debug for WalletBuiltView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WalletBuiltView")
            .field(
                "record",
                &format_args!("<{} bytes, not shown>", self.record.len()),
            )
            .field(
                "wallet_setup",
                &format_args!("<{} bytes>", self.wallet_setup.len()),
            )
            .field("description", &self.description)
            .field("checksum_words", &self.checksum_words)
            .field("fingerprints", &self.fingerprints)
            .field("first_receive_address", &self.first_receive_address)
            .field("pairs", &self.pairs)
            .field("pairs_words", &self.pairs_words)
            .field("state_words", &self.state_words)
            .finish()
    }
}

wire_record!(WalletBuiltView {
    record,
    wallet_setup,
    description,
    checksum_words,
    fingerprints,
    first_receive_address,
    pairs,
    pairs_words,
    state_words,
});

/// What `ceremony_finish` is given.
#[derive(Clone, PartialEq, Eq)]
pub struct FinishArgs {
    /// The sealed record at stage 2.
    pub record: Vec<u8>,
    /// Confirmations at which a payment is done, 1 to 100.
    pub refund_depth: u32,
    /// The wallet's first block.
    pub birthday: u32,
}

impl fmt::Debug for FinishArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FinishArgs")
            .field(
                "record",
                &format_args!("<{} bytes, not shown>", self.record.len()),
            )
            .field("refund_depth", &self.refund_depth)
            .field("birthday", &self.birthday)
            .finish()
    }
}

wire_record_get!(FinishArgs {
    record,
    refund_depth,
    birthday,
});
wire_record!(FinishArgs {
    record,
    refund_depth,
    birthday,
});

/// What `ceremony_finish` returns.
#[derive(Clone, PartialEq, Eq)]
pub struct FinishedView {
    /// The sealed record at stage 3: what the app keeps from now on.
    pub record: Vec<u8>,
    /// The words for this stage of the setup.
    pub state_words: WordsView,
}

impl fmt::Debug for FinishedView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FinishedView")
            .field(
                "record",
                &format_args!("<{} bytes, not shown>", self.record.len()),
            )
            .field("state_words", &self.state_words)
            .finish()
    }
}

wire_record!(FinishedView {
    record,
    state_words
});

/// The values `start` is made from, as the setup text holds them. They hold
/// the owner's secret and key 1's account key.
#[derive(Clone, PartialEq, Eq)]
pub struct SetupFields {
    /// The network.
    pub network: OwnerNetwork,
    /// The owner root's secret, 32 bytes.
    pub owner_secret: Vec<u8>,
    /// The Key app's root, 32 bytes.
    pub approver_root: Vec<u8>,
    /// The wallet description, canonical, with its checksum.
    pub wallet: String,
    /// Key 1's account private key, the 78 bytes of BIP 32 serialization.
    pub key1_account: Vec<u8>,
    /// Key 3's account public key as text.
    pub key3_xpub: Option<String>,
    /// The fewest addresses a batch may hold.
    pub batch_min_count: u32,
    /// Confirmations at which a payment is done.
    pub refund_depth: u32,
    /// The wallet's first block.
    pub birthday: u32,
}

impl fmt::Debug for SetupFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupFields")
            .field("network", &self.network)
            .field(
                "owner_secret",
                &format_args!("<{} bytes, not shown>", self.owner_secret.len()),
            )
            .field("approver_root", &self.approver_root)
            .field("wallet", &self.wallet)
            .field(
                "key1_account",
                &format_args!("<{} bytes, not shown>", self.key1_account.len()),
            )
            .field("key3_xpub", &self.key3_xpub)
            .field("batch_min_count", &self.batch_min_count)
            .field("refund_depth", &self.refund_depth)
            .field("birthday", &self.birthday)
            .finish()
    }
}

wire_record!(SetupFields {
    network,
    owner_secret,
    approver_root,
    wallet,
    key1_account,
    key3_xpub,
    batch_min_count,
    refund_depth,
    birthday,
});

/// What `ceremony_stage` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupStageView {
    /// 1 key 1 made, 2 wallet built and not yet confirmed, 3 finished.
    pub stage: u8,
    /// The words for this stage of the setup.
    pub words: WordsView,
}

wire_record!(SetupStageView { stage, words });

/// The depth at which a payment is done, as the node starts with it
/// (`OwnerParts::new`): the owner's setup is not asked for it.
pub const DEFAULT_REFUND_DEPTH: u32 = 6;

/// What `ceremony_defaults` returns: the values `ceremony_build_wallet` and
/// `ceremony_finish` are given besides the owner's word, as the library
/// holds them, so that an app copies no number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeremonyDefaults {
    /// The fewest addresses a batch may hold: the counter's own minimum.
    pub batch_min_count: u32,
    /// Confirmations at which a payment is done.
    pub refund_depth: u32,
    /// The wallet's first block on this network: the height of the header
    /// checkpoint the watch starts from, 0 where it starts at the genesis.
    pub birthday: u32,
}

wire_record!(CeremonyDefaults {
    batch_min_count,
    refund_depth,
    birthday
});

fn refused(e: CeremonyError) -> AppError {
    from_words(Kind::Refused, e.rule(), &e, e.words())
}

fn network_view(n: Network) -> OwnerNetwork {
    match n {
        Network::Bitcoin => OwnerNetwork::Mainnet,
        Network::Testnet => OwnerNetwork::Testnet,
        Network::Signet => OwnerNetwork::Signet,
        _ => OwnerNetwork::Regtest,
    }
}

fn stage_view(stage: SetupStage) -> (u8, Words) {
    let n = match stage {
        SetupStage::KeyMade => 1,
        SetupStage::WalletBuilt => 2,
        SetupStage::Finished => 3,
    };
    (n, stage.words())
}

fn words4(w: [&'static str; 4]) -> Vec<String> {
    w.iter().map(|s| (*s).to_string()).collect()
}

/// 64 bytes read straight from the system's random source. There is no
/// fallback: a read that fails or comes up short is refused (OC-40).
fn random64() -> Result<[u8; 64], AppError> {
    random64_from("/dev/urandom")
}

fn random64_from(path: &str) -> Result<[u8; 64], AppError> {
    use std::io::Read;
    let mut out = [0u8; 64];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut out))
        .map_err(|_| refused(CeremonyError::Random))?;
    Ok(out)
}

/// Op 80: make key 1 and the owner's root on this phone.
pub(crate) fn make_key1(a: Key1Args) -> Result<Key1Made, AppError> {
    let mut random = random64()?;
    let made = OwnerSetup::make_key1(&random, a.network.network(), a.show_words);
    random.fill(0);
    std::hint::black_box(&mut random);
    let (setup, words) = made.map_err(refused)?;
    let owner_root = setup.owner_root().map_err(refused)?;
    Ok(Key1Made {
        record: setup.to_bytes(),
        key1: PartView::new(&setup.key1_public()),
        owner_root: owner_root.to_bytes().to_vec(),
        owner_root_words: words4(owner_root.fingerprint_words()),
        words,
        state_words: WordsView(SetupStage::KeyMade.words()),
    })
}

/// Op 81: read the Key app's answer.
pub(crate) fn read_answer(bytes: Vec<u8>) -> Result<AnswerView, AppError> {
    let a = KeyAnswer::from_bytes(&bytes).map_err(|e| refused(e.into()))?;
    Ok(AnswerView {
        network: network_view(a.network),
        key2: a.key2.as_ref().map(PartView::new),
        key3: PartView::new(&a.key3),
        key_app_root: a.device_root.to_bytes().to_vec(),
        key_app_root_words: words4(a.device_root.fingerprint_words()),
    })
}

/// Op 82: build the wallet from key 1, key 2 and key 3.
pub(crate) fn build_wallet(a: BuildArgs) -> Result<WalletBuiltView, AppError> {
    let setup = OwnerSetup::from_bytes(&a.record).map_err(refused)?;
    let answer = KeyAnswer::from_bytes(&a.answer).map_err(|e| refused(e.into()))?;
    let built = setup
        .build_wallet(&answer, a.key2_text.as_deref(), a.batch_min_count)
        .map_err(refused)?;
    let f = &built.facts;
    Ok(WalletBuiltView {
        record: built.setup.to_bytes(),
        wallet_setup: built.for_key_app.to_bytes(),
        description: built.for_key_app.description.canonical_string().to_string(),
        checksum_words: words4(f.checksum_words),
        fingerprints: f.fingerprints.iter().map(|p| p.to_string()).collect(),
        first_receive_address: f.first_receive_address.clone(),
        pairs: f
            .pairs
            .iter()
            .map(|p| PairView {
                key_a: u32::from(p.keys[0]),
                key_b: u32::from(p.keys[1]),
                holder_a: holder_code(p.holders[0]),
                holder_b: holder_code(p.holders[1]),
            })
            .collect(),
        pairs_words: WordsView(openbmt_owner::ceremony::pairs_words()),
        state_words: WordsView(SetupStage::WalletBuilt.words()),
    })
}

/// Op 83: the owner confirmed the comparison.
pub(crate) fn finish(a: FinishArgs) -> Result<FinishedView, AppError> {
    let setup = OwnerSetup::from_bytes(&a.record).map_err(refused)?;
    let done = setup.finish(a.refund_depth, a.birthday).map_err(refused)?;
    Ok(FinishedView {
        record: done.to_bytes(),
        state_words: WordsView(SetupStage::Finished.words()),
    })
}

/// Op 84: the values `start` is made from, out of a finished record.
pub(crate) fn open(record: Vec<u8>) -> Result<SetupFields, AppError> {
    let setup = OwnerSetup::from_bytes(&record).map_err(refused)?;
    let f = setup.start_fields().map_err(refused)?;
    Ok(SetupFields {
        network: network_view(f.network),
        owner_secret: f.owner_secret.to_vec(),
        approver_root: f.approver_root.to_vec(),
        wallet: f.wallet.clone(),
        key1_account: f.key1_account.to_vec(),
        key3_xpub: Some(f.key3_xpub.clone()),
        batch_min_count: f.batch_min_count,
        refund_depth: f.refund_depth,
        birthday: f.birthday,
    })
}

/// Op 85: where the setup is, from a record.
pub(crate) fn stage(record: Vec<u8>) -> Result<SetupStageView, AppError> {
    let setup = OwnerSetup::from_bytes(&record).map_err(refused)?;
    let (n, words) = stage_view(setup.stage());
    Ok(SetupStageView {
        stage: n,
        words: WordsView(words),
    })
}

/// Op 86: the defaults of the last two steps on a network.
pub(crate) fn defaults(network: OwnerNetwork) -> Result<CeremonyDefaults, AppError> {
    Ok(CeremonyDefaults {
        batch_min_count: BATCH_MIN_COUNT,
        refund_depth: DEFAULT_REFUND_DEPTH,
        birthday: match network.network() {
            Network::Bitcoin => MAINNET_CHECKPOINT_HEIGHT,
            _ => 0,
        },
    })
}

#[cfg(test)]
mod random_tests {
    use super::*;

    #[test]
    fn oc_40_a_failing_random_source_is_refused() {
        let nothing = random64_from("/nonexistent/random").expect_err("no source");
        assert!(nothing.to_string().contains("OC-40"), "{nothing}");
        // A source that ends early gives too few bytes.
        let short = random64_from("/dev/null").expect_err("short");
        assert!(short.to_string().contains("OC-40"), "{short}");
        assert_ne!(random64().expect("urandom"), [0u8; 64]);
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;
    use openbmt_core::say::{Tone, Who};

    fn words() -> WordsView {
        WordsView(Words::new(
            Tone::Good,
            "Heading",
            "What.",
            Who::Nobody,
            "Next.",
        ))
    }

    #[test]
    fn oc_40_the_debug_text_of_a_ceremony_result_shows_no_secret() {
        let made = Key1Made {
            record: vec![0xAB; 321],
            key1: PartView {
                fingerprint: "deadbeef".to_string(),
                origin_path: "48h/1h/0h/2h".to_string(),
                account_xpub: "xpub".to_string(),
            },
            owner_root: vec![1; 32],
            owner_root_words: vec!["one".to_string()],
            words: Some(vec!["abandon".to_string(); 24]),
            state_words: words(),
        };
        let shown = format!("{made:?}");
        assert!(shown.contains("<321 bytes, not shown>"), "{shown}");
        assert!(shown.contains("<24 words, not shown>"), "{shown}");
        assert!(
            !shown.contains("abandon") && !shown.contains("171"),
            "{shown}"
        );

        let fields = SetupFields {
            network: OwnerNetwork::Regtest,
            owner_secret: vec![0x5A; 32],
            approver_root: vec![2; 32],
            wallet: "wsh(...)".to_string(),
            key1_account: vec![0x77; 78],
            key3_xpub: None,
            batch_min_count: 3,
            refund_depth: 6,
            birthday: 0,
        };
        let shown = format!("{fields:?}");
        assert!(shown.contains("<32 bytes, not shown>"), "{shown}");
        assert!(shown.contains("<78 bytes, not shown>"), "{shown}");
        assert!(!shown.contains("90,") && !shown.contains("119,"), "{shown}");

        let record = vec![0xCD; 5];
        let finished = FinishedView {
            record: record.clone(),
            state_words: words(),
        };
        assert!(!format!("{finished:?}").contains("205"));
        let args = FinishArgs {
            record,
            refund_depth: 6,
            birthday: 0,
        };
        assert!(!format!("{args:?}").contains("205"));
    }
}
