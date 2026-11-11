//! The encoding of every type of the call protocol, against a decoder and an
//! encoder written separately here (the field order is written out again,
//! not taken from the code under test).

use crate::codec::Put;
use crate::Decision;
use crate::{
    AllowlistView, BatchDetail, BitProgress, CeilingsRemaining, CeilingsView, CheckStatus,
    CheckView, DepositCode, Direction, EntropySource, FeeCeilingsView, HealthDetail, Key2Public,
    KeyNetwork, KeyWordCount, KitCheck, LineDetail, LineKind, LineView, OutputKindView, OutputView,
    OwnerLabelKind, OwnerLabelView, PairingDetail, SettingChangeView, SettingsDetail, SetupState,
    SignerStatus, SlotView, SpendDetail, SpendReviewView, WalletSetup, WalletView,
};

// ---- a request writer ------------------------------------------------------------

/// Writes the fields of a request.
#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }

    pub fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u64(mut self, v: u64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn bool(self, v: bool) -> Self {
        self.u8(u8::from(v))
    }

    pub fn bytes(mut self, v: &[u8]) -> Self {
        self = self.u32(u32::try_from(v.len()).expect("fits"));
        self.0.extend_from_slice(v);
        self
    }

    pub fn string(self, v: &str) -> Self {
        self.bytes(v.as_bytes())
    }

    pub fn opt_string(self, v: Option<&str>) -> Self {
        match v {
            None => self.u8(0),
            Some(s) => self.u8(1).string(s),
        }
    }

    pub fn network(self, n: KeyNetwork) -> Self {
        self.u8(match n {
            KeyNetwork::Mainnet => 0,
            KeyNetwork::Testnet => 1,
            KeyNetwork::Signet => 2,
            KeyNetwork::Regtest => 3,
        })
    }

    pub fn words(self, n: KeyWordCount) -> Self {
        self.u8(match n {
            KeyWordCount::Twelve => 0,
            KeyWordCount::TwentyFour => 1,
        })
    }

    pub fn source(self, s: &EntropySource) -> Self {
        match s {
            EntropySource::Platform => self.u8(0),
            EntropySource::DiceHashed { rolls } => self.u8(1).string(rolls),
            EntropySource::CoinFlips { heads, words } => {
                let mut w = self.u8(2).u32(u32::try_from(heads.len()).expect("fits"));
                for h in heads {
                    w = w.bool(*h);
                }
                w.words(*words)
            }
            EntropySource::DiceSkewCorrected { rolls, words } => {
                self.u8(3).string(rolls).words(*words)
            }
            EntropySource::TypedWords { words } => {
                let mut w = self.u8(4).u32(u32::try_from(words.len()).expect("fits"));
                for x in words {
                    w = w.string(x);
                }
                w
            }
        }
    }

    pub fn setup(self, s: &WalletSetup) -> Self {
        let w = self
            .string(&s.description)
            .network(s.network)
            .bytes(&s.owner_root)
            .opt_string(s.key3_xpub.as_deref());
        match s.batch_min_count {
            None => w.u8(0),
            Some(n) => w.u8(1).u32(n),
        }
    }

    pub fn decision(self, d: &Decision) -> Self {
        match d {
            Decision::Approve => self.u8(0),
            Decision::Decline { reason } => self.u8(1).string(reason),
        }
    }

    pub fn ceilings(self, c: &CeilingsView) -> Self {
        self.u64(c.per_tx).u64(c.per_day).u64(c.per_week)
    }
}

/// A request frame.
pub fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBM8".to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

// ---- a response reader -------------------------------------------------------------

/// Reads the result of an op.
pub struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }

    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = self.b.get(self.p..self.p + n).expect("enough bytes");
        self.p += n;
        s
    }

    pub fn done(&self) {
        assert_eq!(self.p, self.b.len(), "bytes left over");
    }

    pub fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }

    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("4"))
    }

    pub fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().expect("8"))
    }

    pub fn bool(&mut self) -> bool {
        match self.u8() {
            0 => false,
            1 => true,
            _ => panic!("not a bool"),
        }
    }

    pub fn bytes(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        self.take(n).to_vec()
    }

    pub fn string(&mut self) -> String {
        String::from_utf8(self.bytes()).expect("UTF-8")
    }

    fn str16(&mut self) -> String {
        let n = usize::from(u16::from_le_bytes(self.take(2).try_into().expect("2")));
        String::from_utf8(self.take(n).to_vec()).expect("UTF-8")
    }

    /// Words as an error frame carries them (no quoted text is expected). The
    /// rule code is leaked: it is a `&'static str` in `Words`.
    pub fn words(&mut self) -> openbmt_core::say::Words {
        let tone_byte = self.u8();
        let tone = crate::say::TONES
            .iter()
            .find(|t| t.2 == tone_byte)
            .expect("a tone")
            .0;
        let who_byte = self.u8();
        let who = crate::say::WHOS
            .iter()
            .find(|w| w.2 == who_byte)
            .expect("a person")
            .0;
        let (heading, what, next) = (self.str16(), self.str16(), self.str16());
        let code = self
            .opt(|r| r.str16())
            .map(|c| &*Box::leak(c.into_boxed_str()));
        let at = self.opt(Rd::u64);
        assert_eq!(self.u8(), 0, "no quoted text");
        let detail = self.opt(|r| r.str16());
        openbmt_core::say::Words {
            tone,
            heading,
            what,
            next,
            who,
            code,
            at,
            quoted: None,
            detail,
        }
    }

    pub fn opt<T>(&mut self, f: impl Fn(&mut Self) -> T) -> Option<T> {
        match self.u8() {
            0 => None,
            1 => Some(f(self)),
            _ => panic!("not a flag"),
        }
    }

    pub fn list<T>(&mut self, f: impl Fn(&mut Self) -> T) -> Vec<T> {
        let n = self.u32();
        (0..n).map(|_| f(self)).collect()
    }

    pub fn strings(&mut self) -> Vec<String> {
        self.list(Rd::string)
    }

    pub fn u32s(&mut self) -> Vec<u32> {
        self.list(Rd::u32)
    }

    pub fn network(&mut self) -> KeyNetwork {
        match self.u8() {
            0 => KeyNetwork::Mainnet,
            1 => KeyNetwork::Testnet,
            2 => KeyNetwork::Signet,
            3 => KeyNetwork::Regtest,
            t => panic!("network tag {t}"),
        }
    }

    pub fn direction(&mut self) -> Direction {
        match self.u8() {
            0 => Direction::Tightening,
            1 => Direction::Loosening,
            t => panic!("direction tag {t}"),
        }
    }

    pub fn decision(&mut self) -> Decision {
        match self.u8() {
            0 => Decision::Approve,
            1 => Decision::Decline {
                reason: self.string(),
            },
            t => panic!("decision tag {t}"),
        }
    }

    pub fn checks(&mut self) -> Vec<CheckView> {
        self.list(|r| CheckView {
            name: r.string(),
            status: match r.u8() {
                0 => CheckStatus::Passed,
                1 => CheckStatus::Failed,
                2 => CheckStatus::NotChecked,
                t => panic!("status tag {t}"),
            },
        })
    }

    pub fn setup_state(&mut self) -> SetupState {
        match self.u8() {
            0 => SetupState::Empty,
            1 => SetupState::HasIdentity {
                holds_key2: self.bool(),
                words_waiting: self.bool(),
            },
            2 => SetupState::Ready {
                holds_key2: self.bool(),
            },
            3 => SetupState::WalletPending {
                holds_key2: self.bool(),
            },
            4 => SetupState::Locked,
            t => panic!("state tag {t}"),
        }
    }

    pub fn key2_public(&mut self) -> Key2Public {
        Key2Public {
            network: self.network(),
            master_fingerprint: self.string(),
            origin_path: self.string(),
            account_xpub: self.string(),
        }
    }

    pub fn wallet_view(&mut self) -> WalletView {
        WalletView {
            checksum_words: self.strings(),
            wallet_id: self.bytes(),
            slots: self.list(|r| SlotView {
                index: r.u32(),
                fingerprint: r.string(),
                xpub: r.string(),
            }),
            first_receive_address: self.string(),
            own_root_words: self.strings(),
            owner_root_words: self.strings(),
        }
    }

    pub fn kit_check(&mut self) -> KitCheck {
        match self.u8() {
            0 => KitCheck::Same,
            1 => KitCheck::Different {
                first_difference: self.u32(),
                same_checksum_words: self.bool(),
                parses: self.bool(),
            },
            t => panic!("kit tag {t}"),
        }
    }

    fn ceilings(&mut self) -> CeilingsView {
        CeilingsView {
            per_tx: self.u64(),
            per_day: self.u64(),
            per_week: self.u64(),
        }
    }

    pub fn signer_status(&mut self) -> SignerStatus {
        SignerStatus {
            ceilings_other: self.ceilings(),
            ceilings_allowlisted: self.ceilings(),
            fee_ceilings: FeeCeilingsView {
                sat_per_vb: self.u64(),
                absolute_sats: self.u64(),
            },
            max_fee_rate_sat_per_vb: self.u64(),
            max_fee_sats: self.u64(),
            allowlist_wait_secs: self.u64(),
            receive_mark: self.u32(),
            change_mark: self.u32(),
            latest_time_seen: self.u64(),
            effective_time_now: self.u64(),
            other_day: self.u64(),
            other_week: self.u64(),
            allowlisted_day: self.u64(),
            allowlisted_week: self.u64(),
            spend_log_count: self.u32(),
            allowlist: self.list(|r| AllowlistView {
                address: r.opt(Rd::string),
                script_hex: r.string(),
                label: r.string(),
                enrolled_at: r.u64(),
                active_at: r.opt(Rd::u64),
            }),
        }
    }

    pub fn progress(&mut self) -> BitProgress {
        BitProgress {
            have: self.u32(),
            still_needed: self.u32(),
        }
    }

    pub fn deposit(&mut self) -> DepositCode {
        DepositCode {
            address: self.string(),
            amount_sats: self.opt(Rd::u64),
            label: self.opt(Rd::string),
            ignored: self.strings(),
        }
    }

    fn owner_label(&mut self) -> OwnerLabelView {
        OwnerLabelView {
            is_claim: self.bool(),
            kind: match self.u8() {
                0 => OwnerLabelKind::Refund,
                1 => OwnerLabelKind::CashOut,
                2 => OwnerLabelKind::Change,
                t => panic!("label tag {t}"),
            },
            counter: self.opt(Rd::bytes),
            sale_index: self.opt(Rd::u32),
            cashier: self.opt(Rd::string),
            shop_value_minor: self.opt(Rd::u64),
            shop_currency: self.opt(Rd::string),
            destination: self.opt(Rd::string),
        }
    }

    fn output(&mut self) -> OutputView {
        OutputView {
            index: self.u32(),
            address: self.opt(Rd::string),
            script_hex: self.string(),
            amount_sats: self.u64(),
            kind: match self.u8() {
                0 => OutputKindView::Change { index: self.u32() },
                1 => OutputKindView::Allowlisted {
                    label: self.string(),
                },
                2 => OutputKindView::Other,
                t => panic!("output tag {t}"),
            },
            owner_phone_label: self.opt(Rd::owner_label),
        }
    }

    fn review(&mut self) -> SpendReviewView {
        SpendReviewView {
            outputs: self.list(Rd::output),
            change_index: self.opt(Rd::u32),
            change_sats: self.opt(Rd::u64),
            moves_coins_inside_wallet: self.bool(),
            fee_sats: self.u64(),
            fee_rate_sat_per_vb: self.u64(),
            input_count: self.u32(),
            input_total_sats: self.u64(),
            other_total_sats: self.u64(),
            allowlisted_total_sats: self.u64(),
            remaining: CeilingsRemaining {
                other_per_tx: self.u64(),
                other_day: self.u64(),
                other_week: self.u64(),
                allowlisted_per_tx: self.u64(),
                allowlisted_day: self.u64(),
                allowlisted_week: self.u64(),
            },
            already_counted: self.bool(),
            clock_behind_by: self.opt(Rd::u64),
        }
    }

    fn detail(&mut self) -> LineDetail {
        match self.u8() {
            0 => LineDetail::Spend {
                detail: SpendDetail {
                    review: self.opt(Rd::review),
                    outputs_without_label: self.u32s(),
                    labels_without_output: self.u32s(),
                    claimed_change_not_proven: self.u32s(),
                },
            },
            1 => LineDetail::Batch {
                detail: BatchDetail {
                    counter: self.bytes(),
                    counter_words: self.strings(),
                    start: self.u32(),
                    end: self.opt(Rd::u32),
                    count: self.u32(),
                    receive_mark: self.u32(),
                    checks: self.checks(),
                },
            },
            2 => LineDetail::Settings {
                detail: SettingsDetail {
                    new_version: self.u32(),
                    last_version: self.opt(Rd::u32),
                    changes: self.list(|r| SettingChangeView {
                        field: r.string(),
                        old: r.opt(Rd::string),
                        new: r.string(),
                        direction: r.direction(),
                    }),
                    overall: self.opt(Rd::direction),
                    checks: self.checks(),
                },
            },
            3 => LineDetail::Pairing {
                detail: PairingDetail {
                    name: self.string(),
                    counter_key: self.bytes(),
                    session_words: self.opt(Rd::strings),
                    claimed_settings_version: self.u32(),
                    settings_version_to_sign: self.u32(),
                },
            },
            4 => LineDetail::Health {
                detail: HealthDetail {
                    nonce: self.bytes(),
                },
            },
            t => panic!("detail tag {t}"),
        }
    }

    pub fn line(&mut self) -> LineView {
        LineView {
            item: self.u32(),
            kind: match self.u8() {
                0 => LineKind::Spend,
                1 => LineKind::BatchRefill,
                2 => LineKind::Settings,
                3 => LineKind::Pairing,
                4 => LineKind::HealthCheck,
                t => panic!("kind tag {t}"),
            },
            title: self.string(),
            can_approve: self.bool(),
            failure: self.opt(Rd::string),
            failure_words: self.opt(Rd::words),
            decision: self.opt(Rd::decision),
            detail: self.detail(),
        }
    }

    pub fn lines(&mut self) -> Vec<LineView> {
        self.list(Rd::line)
    }
}

// ---- values of every shape ----------------------------------------------------------

fn put<T: Put>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.put(&mut out);
    out
}

fn review() -> SpendReviewView {
    let label = OwnerLabelView {
        is_claim: true,
        kind: OwnerLabelKind::Refund,
        counter: Some(vec![7; 32]),
        sale_index: Some(3),
        cashier: Some("Mia \u{e9}".to_string()),
        shop_value_minor: Some(1_250),
        shop_currency: Some("EUR".to_string()),
        destination: None,
    };
    let cash_out = OwnerLabelView {
        is_claim: true,
        kind: OwnerLabelKind::CashOut,
        counter: None,
        sale_index: None,
        cashier: None,
        shop_value_minor: None,
        shop_currency: None,
        destination: Some("Exchange".to_string()),
    };
    let output = |index, kind, label| OutputView {
        index,
        address: Some("bcrt1qexample".to_string()),
        script_hex: "0014aa".to_string(),
        amount_sats: 10_000 + u64::from(index),
        kind,
        owner_phone_label: label,
    };
    SpendReviewView {
        outputs: vec![
            output(0, OutputKindView::Change { index: 4 }, None),
            output(
                1,
                OutputKindView::Allowlisted {
                    label: "Shop".to_string(),
                },
                Some(cash_out),
            ),
            output(2, OutputKindView::Other, Some(label)),
        ],
        change_index: Some(4),
        change_sats: Some(99),
        moves_coins_inside_wallet: false,
        fee_sats: 500,
        fee_rate_sat_per_vb: 3,
        input_count: 2,
        input_total_sats: 600_000,
        other_total_sats: 10_000,
        allowlisted_total_sats: 20_000,
        remaining: CeilingsRemaining {
            other_per_tx: 1,
            other_day: 2,
            other_week: 3,
            allowlisted_per_tx: 4,
            allowlisted_day: 5,
            allowlisted_week: 6,
        },
        already_counted: true,
        clock_behind_by: Some(12),
    }
}

/// A line of every detail, with every option both set and not set.
pub fn sample_lines() -> Vec<LineView> {
    let line = |item, kind, title: &str, detail, decision| LineView {
        item,
        kind,
        title: title.to_string(),
        can_approve: item % 2 == 0,
        failure: (item % 2 == 1).then(|| "AS-24: no".to_string()),
        failure_words: (item % 4 == 1).then(openbmt_approver::look_alike_words),
        decision,
        detail,
    };
    let checks = vec![
        CheckView {
            name: "owner signature".to_string(),
            status: CheckStatus::Passed,
        },
        CheckView {
            name: "values".to_string(),
            status: CheckStatus::Failed,
        },
        CheckView {
            name: "version".to_string(),
            status: CheckStatus::NotChecked,
        },
    ];
    vec![
        line(
            0,
            LineKind::Spend,
            "Transaction",
            LineDetail::Spend {
                detail: SpendDetail {
                    review: Some(review()),
                    outputs_without_label: vec![0, 1],
                    labels_without_output: vec![9],
                    claimed_change_not_proven: vec![],
                },
            },
            Some(Decision::Approve),
        ),
        line(
            1,
            LineKind::Spend,
            "Transaction",
            LineDetail::Spend {
                detail: SpendDetail {
                    review: None,
                    outputs_without_label: vec![],
                    labels_without_output: vec![],
                    claimed_change_not_proven: vec![5],
                },
            },
            None,
        ),
        line(
            2,
            LineKind::BatchRefill,
            "Address batch",
            LineDetail::Batch {
                detail: BatchDetail {
                    counter: vec![1; 32],
                    counter_words: vec!["a".into(), "b".into(), "c".into(), "d".into()],
                    start: 10,
                    end: Some(20),
                    count: 10,
                    receive_mark: 10,
                    checks: checks.clone(),
                },
            },
            Some(Decision::Decline {
                reason: "too many".to_string(),
            }),
        ),
        line(
            3,
            LineKind::BatchRefill,
            "Address batch",
            LineDetail::Batch {
                detail: BatchDetail {
                    counter: vec![],
                    counter_words: vec![],
                    start: 0,
                    end: None,
                    count: 0,
                    receive_mark: 0,
                    checks: vec![],
                },
            },
            None,
        ),
        line(
            4,
            LineKind::Settings,
            "Settings",
            LineDetail::Settings {
                detail: SettingsDetail {
                    new_version: 2,
                    last_version: Some(1),
                    changes: vec![
                        SettingChangeView {
                            field: "refund limit".to_string(),
                            old: Some("1".to_string()),
                            new: "2".to_string(),
                            direction: Direction::Loosening,
                        },
                        SettingChangeView {
                            field: "devices".to_string(),
                            old: None,
                            new: "x".to_string(),
                            direction: Direction::Tightening,
                        },
                    ],
                    overall: Some(Direction::Loosening),
                    checks: checks.clone(),
                },
            },
            None,
        ),
        line(
            5,
            LineKind::Settings,
            "Settings",
            LineDetail::Settings {
                detail: SettingsDetail {
                    new_version: 0,
                    last_version: None,
                    changes: vec![],
                    overall: None,
                    checks: vec![],
                },
            },
            None,
        ),
        line(
            6,
            LineKind::Pairing,
            "Pairing",
            LineDetail::Pairing {
                detail: PairingDetail {
                    name: "Counter 1".to_string(),
                    counter_key: vec![2; 32],
                    session_words: Some(vec!["w".into(), "x".into(), "y".into(), "z".into()]),
                    claimed_settings_version: 3,
                    settings_version_to_sign: 4,
                },
            },
            None,
        ),
        line(
            7,
            LineKind::Pairing,
            "Pairing",
            LineDetail::Pairing {
                detail: PairingDetail {
                    name: String::new(),
                    counter_key: vec![],
                    session_words: None,
                    claimed_settings_version: 0,
                    settings_version_to_sign: 0,
                },
            },
            None,
        ),
        line(
            8,
            LineKind::HealthCheck,
            "Health check",
            LineDetail::Health {
                detail: HealthDetail { nonce: vec![9; 32] },
            },
            None,
        ),
    ]
}

#[test]
fn every_line_shape_round_trips() {
    let lines = sample_lines();
    let bytes = put(&lines);
    let mut r = Rd::new(&bytes);
    assert_eq!(r.lines(), lines);
    r.done();
}

#[test]
fn the_small_records_and_enums_round_trip() {
    for state in [
        SetupState::Empty,
        SetupState::HasIdentity {
            holds_key2: true,
            words_waiting: false,
        },
        SetupState::HasIdentity {
            holds_key2: false,
            words_waiting: true,
        },
        SetupState::Ready { holds_key2: true },
    ] {
        let bytes = put(&state);
        let mut r = Rd::new(&bytes);
        assert_eq!(r.setup_state(), state);
        r.done();
    }
    for kit in [
        KitCheck::Same,
        KitCheck::Different {
            first_difference: 17,
            same_checksum_words: true,
            parses: false,
        },
    ] {
        let bytes = put(&kit);
        let mut r = Rd::new(&bytes);
        assert_eq!(r.kit_check(), kit);
        r.done();
    }
    let public = Key2Public {
        network: KeyNetwork::Signet,
        master_fingerprint: "73c5da0a".to_string(),
        origin_path: "48h/1h/0h/2h".to_string(),
        account_xpub: "xpub".to_string(),
    };
    for value in [Some(public.clone()), None] {
        let bytes = put(&value);
        let mut r = Rd::new(&bytes);
        assert_eq!(r.opt(Rd::key2_public), value);
        r.done();
    }
    let view = WalletView {
        checksum_words: vec!["a".into(), "b".into(), "c".into(), "d".into()],
        wallet_id: vec![5; 32],
        slots: vec![
            SlotView {
                index: 1,
                fingerprint: "aabbccdd".to_string(),
                xpub: "x1".to_string(),
            },
            SlotView {
                index: 2,
                fingerprint: "00000000".to_string(),
                xpub: "x2".to_string(),
            },
        ],
        first_receive_address: "bcrt1q".to_string(),
        own_root_words: vec!["e".into(); 4],
        owner_root_words: vec!["f".into(); 4],
    };
    let bytes = put(&view);
    let mut r = Rd::new(&bytes);
    assert_eq!(r.wallet_view(), view);
    r.done();

    let progress = BitProgress {
        have: 50,
        still_needed: 78,
    };
    let bytes = put(&progress);
    let mut r = Rd::new(&bytes);
    assert_eq!(r.progress(), progress);
    r.done();

    for code in [
        DepositCode {
            address: "bcrt1q".to_string(),
            amount_sats: Some(1),
            label: Some("L".to_string()),
            ignored: vec!["pj".to_string(), "lightning".to_string()],
        },
        DepositCode {
            address: String::new(),
            amount_sats: None,
            label: None,
            ignored: vec![],
        },
    ] {
        let bytes = put(&code);
        let mut r = Rd::new(&bytes);
        assert_eq!(r.deposit(), code);
        r.done();
    }

    let status = SignerStatus {
        ceilings_other: CeilingsView {
            per_tx: 1,
            per_day: 2,
            per_week: 3,
        },
        ceilings_allowlisted: CeilingsView {
            per_tx: 4,
            per_day: 5,
            per_week: 6,
        },
        fee_ceilings: FeeCeilingsView {
            sat_per_vb: 7,
            absolute_sats: 8,
        },
        max_fee_rate_sat_per_vb: 9,
        max_fee_sats: 10,
        allowlist_wait_secs: 11,
        receive_mark: 12,
        change_mark: 13,
        latest_time_seen: 14,
        effective_time_now: 15,
        other_day: 16,
        other_week: 17,
        allowlisted_day: 18,
        allowlisted_week: 19,
        spend_log_count: 20,
        allowlist: vec![
            AllowlistView {
                address: Some("bcrt1q".to_string()),
                script_hex: "0014".to_string(),
                label: "Exchange".to_string(),
                enrolled_at: 21,
                active_at: Some(22),
            },
            AllowlistView {
                address: None,
                script_hex: "6a".to_string(),
                label: "x".to_string(),
                enrolled_at: 0,
                active_at: None,
            },
        ],
    };
    let bytes = put(&status);
    let mut r = Rd::new(&bytes);
    assert_eq!(r.signer_status(), status);
    r.done();
}

#[test]
fn the_request_types_are_read_as_they_are_written() {
    use crate::codec::{Get, Reader};
    for source in [
        EntropySource::Platform,
        EntropySource::DiceHashed {
            rolls: "123456".to_string(),
        },
        EntropySource::CoinFlips {
            heads: vec![true, false, true],
            words: KeyWordCount::TwentyFour,
        },
        EntropySource::DiceSkewCorrected {
            rolls: "6".to_string(),
            words: KeyWordCount::Twelve,
        },
        EntropySource::TypedWords {
            words: vec!["abandon".to_string(), "about".to_string()],
        },
    ] {
        let bytes = W::new().source(&source).0;
        let mut r = Reader::new(&bytes);
        let got = EntropySource::get(&mut r).expect("reads");
        r.end().expect("all read");
        // `EntropySource` has no equality (it holds secrets): compare the
        // writing of what was read.
        assert_eq!(W::new().source(&got).0, bytes);
    }
    for setup in [
        WalletSetup {
            description: "wsh(...)".to_string(),
            network: KeyNetwork::Testnet,
            owner_root: vec![1; 32],
            key3_xpub: Some("xpub".to_string()),
            batch_min_count: Some(3),
        },
        WalletSetup {
            description: String::new(),
            network: KeyNetwork::Mainnet,
            owner_root: vec![],
            key3_xpub: None,
            batch_min_count: None,
        },
    ] {
        let bytes = W::new().setup(&setup).0;
        let mut r = Reader::new(&bytes);
        let got = WalletSetup::get(&mut r).expect("reads");
        r.end().expect("all read");
        assert_eq!(got.description, setup.description);
        assert_eq!(got.network, setup.network);
        assert_eq!(got.owner_root, setup.owner_root);
        assert_eq!(got.key3_xpub, setup.key3_xpub);
        assert_eq!(got.batch_min_count, setup.batch_min_count);
    }
    for d in [
        Decision::Approve,
        Decision::Decline {
            reason: "no".to_string(),
        },
    ] {
        let bytes = W::new().decision(&d).0;
        let mut r = Reader::new(&bytes);
        assert_eq!(Decision::get(&mut r).expect("reads"), d);
    }
    let c = CeilingsView {
        per_tx: 1,
        per_day: 2,
        per_week: 3,
    };
    let bytes = W::new().ceilings(&c).0;
    assert_eq!(
        CeilingsView::get(&mut Reader::new(&bytes)).expect("reads"),
        c
    );
}
