//! Section 1 of the rules: the session file (AS-01 to AS-15).

use bitcoin::hashes::Hash;
use bitcoin::{ScriptBuf, WScriptHash};
use openbmt_core::batch::{AddressBatch, PinnedRoots};
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::{DeviceId, Identity, Sig};
use openbmt_core::pairing::{CounterPairing, PairOffer};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_core::wire::DecodeError;
use openbmt_session::*;

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

fn owner() -> Identity {
    id(1)
}

fn approver() -> Identity {
    id(2)
}

const SESSION: [u8; 32] = [7; 32];
const WALLET: [u8; 32] = [8; 32];

fn script(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32]))
}

fn spend() -> Item {
    Item::Spend {
        psbt: vec![1, 2, 3, 4],
        labels: vec![
            OutputLabel {
                output: 0,
                label: Label::Refund {
                    counter: id(9).device_id(),
                    sale_index: 12,
                    cashier: "Mia".into(),
                    shop_value: Some(ShopValue {
                        minor: 1850,
                        currency: *b"USD",
                    }),
                },
            },
            OutputLabel {
                output: 1,
                label: Label::CashOut {
                    destination: "Kraken".into(),
                },
            },
            OutputLabel {
                output: 2,
                label: Label::Change,
            },
        ],
    }
}

fn batch() -> Item {
    let scripts: Vec<ScriptBuf> = (0..3).map(script).collect();
    Item::BatchRefill {
        batch: AddressBatch {
            wallet_checksum: WALLET,
            counter: id(9).device_id(),
            start: 0,
            count: 3,
            scripts,
        },
        owner_sig: Sig([5; 64]),
    }
}

fn settings() -> Item {
    let s = Settings::defaults(1000);
    let owner_sig = s.sign(&owner());
    Item::Settings {
        signed: SignedSettings {
            settings: s,
            owner_sig,
            approver_sig: None,
        },
    }
}

fn pairing() -> Item {
    let offer = PairOffer::new(
        owner().device_id(),
        approver().device_id(),
        WALLET,
        bitcoin::Network::Regtest,
    )
    .unwrap();
    let (_, request) =
        CounterPairing::begin(&offer.to_bytes(), &id(9), "Counter 1", [3; 32], 100).unwrap();
    Item::Pairing {
        request,
        settings_version: 4,
    }
}

fn health() -> Item {
    Item::HealthCheck { nonce: [6; 32] }
}

fn items() -> Vec<RequestItem> {
    [spend(), batch(), settings(), pairing(), health()]
        .into_iter()
        .enumerate()
        .map(|(n, item)| RequestItem {
            number: n as u32 * 2,
            item,
        })
        .collect()
}

fn request() -> Request {
    Request::new(&owner(), SESSION, WALLET, items()).unwrap()
}

fn approved(request: &Request) -> Vec<ItemResult> {
    request
        .items()
        .iter()
        .map(|i| {
            let payload = match &i.item {
                Item::Spend { .. } => Payload::Spend { psbt: vec![9, 9] },
                Item::BatchRefill { .. } => Payload::Batch { sig: Sig([1; 64]) },
                Item::Settings { .. } => Payload::Settings { sig: Sig([2; 64]) },
                Item::Pairing { .. } => Payload::Pairing {
                    settings_version: 4,
                    sig: Sig([3; 64]),
                },
                Item::HealthCheck { nonce } => Payload::Health {
                    sig: health_proof(&approver(), &SESSION, i.number, nonce),
                },
                Item::BoxClaim { claim } => Payload::BoxWelcome {
                    welcome: Welcome::make(
                        &invitation(),
                        claim,
                        PinnedRoots::new(owner().device_id(), approver().device_id()).unwrap(),
                    ),
                },
            };
            ItemResult {
                number: i.number,
                verdict: Verdict::Approved(payload),
            }
        })
        .collect()
}

fn response(request: &Request) -> Response {
    Response::new(&approver(), SESSION, request.hash(), approved(request)).unwrap()
}

/// A tiny fixed-seed generator for the hostile-bytes tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

// ---- AS-01 ---------------------------------------------------------------

#[test]
fn as_01_the_magics_and_the_version_are_the_ones_of_the_rule() {
    let q = request();
    assert_eq!(&q.as_bytes()[..5], b"OBMF\x01");
    let r = response(&q);
    assert_eq!(&r.as_bytes()[..5], b"OBM2\x01");
}

#[test]
fn as_01_other_magics_and_versions_are_refused() {
    let q = request();
    let r = response(&q);
    assert_eq!(
        Request::from_bytes(r.as_bytes()).unwrap_err(),
        DecodeError::BadMagic
    );
    assert_eq!(
        Response::from_bytes(q.as_bytes()).unwrap_err(),
        DecodeError::BadMagic
    );
    let mut bytes = q.as_bytes().to_vec();
    bytes[4] = 2;
    assert_eq!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::UnsupportedVersion(2)
    );
    let mut bytes = r.as_bytes().to_vec();
    bytes[0] = b'X';
    assert_eq!(
        Response::from_bytes(&bytes).unwrap_err(),
        DecodeError::BadMagic
    );
}

// ---- AS-02 ---------------------------------------------------------------

#[test]
fn as_02_a_file_above_8_mib_is_refused_before_it_is_parsed() {
    let bytes = vec![0u8; MAX_SESSION_BYTES + 1];
    assert_eq!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::TooLarge
    );
    assert_eq!(
        Response::from_bytes(&bytes).unwrap_err(),
        DecodeError::TooLarge
    );
}

#[test]
fn as_02_building_a_file_above_8_mib_is_refused() {
    let big = |n: u32| RequestItem {
        number: n,
        item: Item::Spend {
            psbt: vec![0; 5 * 1024 * 1024],
            labels: Vec::new(),
        },
    };
    assert_eq!(
        Request::new(&owner(), SESSION, WALLET, vec![big(0), big(1)]).unwrap_err(),
        EncodeError::TooLarge
    );
    let q = request();
    let results = vec![ItemResult {
        number: 0,
        verdict: Verdict::Approved(Payload::Spend {
            psbt: vec![0; MAX_SESSION_BYTES + 1],
        }),
    }];
    assert_eq!(
        Response::new(&approver(), SESSION, q.hash(), results).unwrap_err(),
        EncodeError::TooLarge
    );
}

#[test]
fn as_02_a_file_just_under_the_limit_is_taken() {
    let item = RequestItem {
        number: 0,
        item: Item::Spend {
            psbt: vec![0; MAX_SESSION_BYTES - 1_000],
            labels: Vec::new(),
        },
    };
    let q = Request::new(&owner(), SESSION, WALLET, vec![item]).unwrap();
    assert!(q.as_bytes().len() <= MAX_SESSION_BYTES);
    assert_eq!(Request::from_bytes(q.as_bytes()).unwrap(), q);
}

// ---- AS-03 ---------------------------------------------------------------

#[test]
fn as_03_what_decodes_encodes_back_to_the_same_bytes() {
    let q = request();
    let again = Request::from_bytes(q.as_bytes()).unwrap();
    assert_eq!(again, q);
    assert_eq!(again.as_bytes(), q.as_bytes());
    let r = response(&q);
    let again = Response::from_bytes(r.as_bytes()).unwrap();
    assert_eq!(again, r);
    assert_eq!(again.as_bytes(), r.as_bytes());
}

#[test]
fn as_03_bytes_after_the_end_and_cut_files_are_refused() {
    let q = request();
    let mut bytes = q.as_bytes().to_vec();
    bytes.push(0);
    assert_eq!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::TrailingBytes
    );
    for cut in 0..q.as_bytes().len() {
        assert!(Request::from_bytes(&q.as_bytes()[..cut]).is_err(), "{cut}");
    }
    let r = response(&q);
    let mut bytes = r.as_bytes().to_vec();
    bytes.push(0);
    assert!(Response::from_bytes(&bytes).is_err());
    for cut in 0..r.as_bytes().len() {
        assert!(Response::from_bytes(&r.as_bytes()[..cut]).is_err(), "{cut}");
    }
}

#[test]
fn as_03_a_flag_that_is_not_0_or_1_is_refused() {
    // The refund label's "has a shop value" flag is the byte that follows
    // the cashier's name.
    let q = Request::new(
        &owner(),
        SESSION,
        WALLET,
        vec![RequestItem {
            number: 0,
            item: Item::Spend {
                psbt: vec![],
                labels: vec![OutputLabel {
                    output: 0,
                    label: Label::Refund {
                        counter: id(9).device_id(),
                        sale_index: 1,
                        cashier: "Mia".into(),
                        shop_value: None,
                    },
                }],
            },
        }],
    )
    .unwrap();
    let mut bytes = q.as_bytes().to_vec();
    let at = bytes.len() - 64 - 1;
    assert_eq!(bytes[at], 0);
    bytes[at] = 2;
    assert!(matches!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::NonCanonical(_)
    ));
}

#[test]
fn as_03_labels_out_of_order_are_refused_on_both_ends() {
    let label = |output| OutputLabel {
        output,
        label: Label::Change,
    };
    let item = Item::Spend {
        psbt: vec![],
        labels: vec![label(1), label(0)],
    };
    assert!(matches!(
        Request::new(
            &owner(),
            SESSION,
            WALLET,
            vec![RequestItem { number: 0, item }]
        )
        .unwrap_err(),
        EncodeError::BadItem(_)
    ));
    // Swap the two output numbers inside the bytes of a good request.
    let good = Request::new(
        &owner(),
        SESSION,
        WALLET,
        vec![RequestItem {
            number: 0,
            item: Item::Spend {
                psbt: vec![],
                labels: vec![label(0), label(1)],
            },
        }],
    )
    .unwrap();
    let mut bytes = good.as_bytes().to_vec();
    let end = bytes.len() - 64;
    // Each Change label is 4 bytes of output number and 1 of kind.
    let second = end - 5;
    let first = second - 5;
    bytes[first] = 1;
    bytes[second] = 0;
    assert!(matches!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::NonCanonical(_)
    ));
}

// ---- AS-04 ---------------------------------------------------------------

#[test]
fn as_04_a_request_holds_one_to_sixty_four_items() {
    let item = |n: u32| RequestItem {
        number: n,
        item: health(),
    };
    assert_eq!(
        Request::new(&owner(), SESSION, WALLET, vec![]).unwrap_err(),
        EncodeError::ItemCount
    );
    let sixty_four: Vec<RequestItem> = (0..64).map(item).collect();
    let q = Request::new(&owner(), SESSION, WALLET, sixty_four).unwrap();
    assert_eq!(Request::from_bytes(q.as_bytes()).unwrap().items().len(), 64);
    let sixty_five: Vec<RequestItem> = (0..65).map(item).collect();
    assert_eq!(
        Request::new(&owner(), SESSION, WALLET, sixty_five).unwrap_err(),
        EncodeError::ItemCount
    );
    // A count above the limit in the bytes is refused before anything is
    // allocated.
    let mut bytes = q.as_bytes().to_vec();
    bytes[5 + 96..5 + 100].copy_from_slice(&65u32.to_le_bytes());
    assert_eq!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::TooLarge
    );
    bytes[5 + 96..5 + 100].copy_from_slice(&0u32.to_le_bytes());
    assert!(Request::from_bytes(&bytes).is_err());
}

#[test]
fn as_04_item_numbers_must_strictly_ascend() {
    let item = |n: u32| RequestItem {
        number: n,
        item: health(),
    };
    for numbers in [[1, 0], [3, 3]] {
        assert_eq!(
            Request::new(
                &owner(),
                SESSION,
                WALLET,
                numbers.iter().map(|n| item(*n)).collect()
            )
            .unwrap_err(),
            EncodeError::ItemOrder
        );
    }
    let q = Request::new(&owner(), SESSION, WALLET, vec![item(0), item(1)]).unwrap();
    let mut bytes = q.as_bytes().to_vec();
    // The second item's number sits after the first item (4 + 1 + 4 + 32).
    let second = 5 + 100 + 41;
    bytes[second] = 0;
    assert!(matches!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::NonCanonical(_)
    ));
}

// ---- AS-05 ---------------------------------------------------------------

#[test]
fn as_05_the_request_names_its_session_wallet_and_owner() {
    let q = request();
    assert_eq!(q.session_id(), &SESSION);
    assert_eq!(q.wallet(), &WALLET);
    assert_eq!(q.owner(), owner().device_id());
    assert_eq!(q.verify(&owner().device_id()), Ok(()));
}

#[test]
fn as_05_a_request_signed_by_another_key_does_not_verify() {
    let q = Request::new(&id(30), SESSION, WALLET, items()).unwrap();
    let decoded = Request::from_bytes(q.as_bytes()).unwrap();
    assert_eq!(
        decoded.verify(&owner().device_id()),
        Err(VerifyError::WrongOwner)
    );
    // The owner's key named in the file, another key's signature.
    let good = request();
    let mut bytes = good.as_bytes().to_vec();
    let other_sig = q.as_bytes()[q.as_bytes().len() - 64..].to_vec();
    let n = bytes.len();
    bytes[n - 64..].copy_from_slice(&other_sig);
    let forged = Request::from_bytes(&bytes).unwrap();
    assert_eq!(
        forged.verify(&owner().device_id()),
        Err(VerifyError::BadSignature)
    );
}

#[test]
fn as_05_a_changed_byte_breaks_the_signature_and_changes_the_hash() {
    let good = request();
    let mut bytes = good.as_bytes().to_vec();
    bytes[10] ^= 1;
    let changed = Request::from_bytes(&bytes).unwrap();
    assert_eq!(
        changed.verify(&owner().device_id()),
        Err(VerifyError::BadSignature)
    );
    assert_ne!(changed.hash(), good.hash());
    assert_eq!(good.hash(), request().hash());
}

// ---- AS-06 to AS-11 --------------------------------------------------------

#[test]
fn as_06_a_spend_holds_its_psbt_and_labels_unread() {
    let q = request();
    let back = Request::from_bytes(q.as_bytes()).unwrap();
    assert_eq!(back.item(0).unwrap().item, spend());
    assert_eq!(back.item(0).unwrap().item.kind(), ItemKind::Spend);
}

#[test]
fn as_06_more_than_a_hundred_labels_are_refused() {
    let labels = (0..101)
        .map(|output| OutputLabel {
            output,
            label: Label::Change,
        })
        .collect();
    assert!(matches!(
        Request::new(
            &owner(),
            SESSION,
            WALLET,
            vec![RequestItem {
                number: 0,
                item: Item::Spend {
                    psbt: vec![],
                    labels
                }
            }]
        )
        .unwrap_err(),
        EncodeError::BadItem(_)
    ));
}

#[test]
fn as_07_label_texts_follow_the_text_rule() {
    let refund = |cashier: String| Item::Spend {
        psbt: vec![],
        labels: vec![OutputLabel {
            output: 0,
            label: Label::Refund {
                counter: id(9).device_id(),
                sale_index: 1,
                cashier,
                shop_value: None,
            },
        }],
    };
    let make = |item| {
        Request::new(
            &owner(),
            SESSION,
            WALLET,
            vec![RequestItem { number: 0, item }],
        )
    };
    assert!(make(refund("a".repeat(64))).is_ok());
    assert!(matches!(
        make(refund("a".repeat(65))).unwrap_err(),
        EncodeError::BadText(_)
    ));
    assert!(matches!(
        make(refund(String::new())).unwrap_err(),
        EncodeError::BadText(_)
    ));
    assert!(matches!(
        make(refund("a\nb".into())).unwrap_err(),
        EncodeError::BadText(_)
    ));
    // 64 characters of 4 bytes each pass the character count and fail the
    // byte count.
    assert!(matches!(
        make(refund("\u{1F600}".repeat(64))).unwrap_err(),
        EncodeError::BadText(_)
    ));
    let cash = Item::Spend {
        psbt: vec![],
        labels: vec![OutputLabel {
            output: 0,
            label: Label::CashOut {
                destination: "x\u{7}".into(),
            },
        }],
    };
    assert!(matches!(make(cash).unwrap_err(), EncodeError::BadText(_)));
}

#[test]
fn as_07_a_currency_code_is_three_capital_letters() {
    let item = Item::Spend {
        psbt: vec![],
        labels: vec![OutputLabel {
            output: 0,
            label: Label::Refund {
                counter: id(9).device_id(),
                sale_index: 1,
                cashier: "Mia".into(),
                shop_value: Some(ShopValue {
                    minor: 5,
                    currency: *b"usd",
                }),
            },
        }],
    };
    assert!(matches!(
        Request::new(
            &owner(),
            SESSION,
            WALLET,
            vec![RequestItem { number: 0, item }]
        )
        .unwrap_err(),
        EncodeError::BadText(_)
    ));
}

#[test]
fn as_08_a_batch_item_holds_the_batch_and_the_owners_signature_only() {
    let q = request();
    let back = Request::from_bytes(q.as_bytes()).unwrap();
    assert_eq!(back.item(2).unwrap().item, batch());
    // A script that is not 34 bytes cannot be encoded.
    let Item::BatchRefill { batch, owner_sig } = self::batch() else {
        unreachable!()
    };
    let mut bad = batch.clone();
    bad.scripts[1] = ScriptBuf::from_bytes(vec![0; 40]);
    assert!(matches!(
        Request::new(
            &owner(),
            SESSION,
            WALLET,
            vec![RequestItem {
                number: 0,
                item: Item::BatchRefill {
                    batch: bad,
                    owner_sig
                }
            }]
        )
        .unwrap_err(),
        EncodeError::BadItem(_)
    ));
}

#[test]
fn as_08_a_batch_item_with_a_key_app_signature_is_refused() {
    let q = Request::new(
        &owner(),
        SESSION,
        WALLET,
        vec![RequestItem {
            number: 0,
            item: batch(),
        }],
    )
    .unwrap();
    let mut bytes = q.as_bytes().to_vec();
    // The last 64 bytes before the request's own signature are the zero
    // placeholder of the Key app's signature.
    let end = bytes.len() - 64;
    bytes[end - 1] = 1;
    assert!(matches!(
        Request::from_bytes(&bytes).unwrap_err(),
        DecodeError::NonCanonical(_)
    ));
}

#[test]
fn as_09_a_settings_item_carries_no_key_app_signature() {
    let q = request();
    let back = Request::from_bytes(q.as_bytes()).unwrap();
    assert_eq!(back.item(4).unwrap().item, settings());
    let Item::Settings { mut signed } = settings() else {
        unreachable!()
    };
    signed.approver_sig = Some(Sig([1; 64]));
    assert!(matches!(
        Request::new(
            &owner(),
            SESSION,
            WALLET,
            vec![RequestItem {
                number: 0,
                item: Item::Settings { signed }
            }]
        )
        .unwrap_err(),
        EncodeError::BadItem(_)
    ));
}

#[test]
fn as_10_a_pairing_item_holds_the_request_and_the_claimed_version() {
    let q = request();
    let back = Request::from_bytes(q.as_bytes()).unwrap();
    let Item::Pairing {
        request,
        settings_version,
    } = &back.item(6).unwrap().item
    else {
        panic!("not a pairing")
    };
    assert_eq!(*settings_version, 4);
    assert_eq!(request.name, "Counter 1");
    assert_eq!(back.item(6).unwrap().item, pairing());
}

#[test]
fn as_11_a_health_item_holds_a_32_byte_number() {
    let q = request();
    let back = Request::from_bytes(q.as_bytes()).unwrap();
    assert_eq!(back.item(8).unwrap().item, health());
    // The body is not 32 bytes.
    let short = Request::new(
        &owner(),
        SESSION,
        WALLET,
        vec![RequestItem {
            number: 0,
            item: health(),
        }],
    )
    .unwrap();
    let mut bytes = short.as_bytes().to_vec();
    // Body length field of the item: after number (4) and kind (1).
    let at = 5 + 100 + 4 + 1;
    bytes[at..at + 4].copy_from_slice(&31u32.to_le_bytes());
    assert!(Request::from_bytes(&bytes).is_err());
}

// ---- AS-12 ---------------------------------------------------------------

#[test]
fn as_12_a_response_names_session_request_and_approver_and_signs() {
    let q = request();
    let r = response(&q);
    assert_eq!(r.session_id(), &SESSION);
    assert_eq!(r.request_hash(), &q.hash());
    assert_eq!(r.approver(), approver().device_id());
    assert_eq!(r.results().len(), q.items().len());
    let back = Response::from_bytes(r.as_bytes()).unwrap();
    assert_eq!(back, r);
    assert_eq!(back.check_against(&q, &approver().device_id()), Ok(()));
}

#[test]
fn as_12_declined_results_carry_a_code_and_a_short_text() {
    let q = request();
    let mut results = approved(&q);
    for (n, code) in [
        DeclineCode::ByPerson,
        DeclineCode::ChecksFailed,
        DeclineCode::NotOnThisDevice,
        DeclineCode::NotDecided,
    ]
    .into_iter()
    .enumerate()
    {
        results[n].verdict = Verdict::Declined {
            code,
            text: format!("SR-35: over the ceiling {n}"),
        };
    }
    let r = Response::new(&approver(), SESSION, q.hash(), results.clone()).unwrap();
    assert_eq!(
        Response::from_bytes(r.as_bytes()).unwrap().results(),
        &results
    );
    for text in [String::new(), "x".repeat(161), "a\tb".to_string()] {
        let bad = vec![ItemResult {
            number: 0,
            verdict: Verdict::Declined {
                code: DeclineCode::ByPerson,
                text,
            },
        }];
        assert!(matches!(
            Response::new(&approver(), SESSION, q.hash(), bad).unwrap_err(),
            EncodeError::BadText(_)
        ));
    }
    let longest = vec![ItemResult {
        number: 0,
        verdict: Verdict::Declined {
            code: DeclineCode::ByPerson,
            text: "x".repeat(160),
        },
    }];
    assert!(Response::new(&approver(), SESSION, q.hash(), longest).is_ok());
}

#[test]
fn as_12_a_response_needs_results_in_ascending_order() {
    let q = request();
    assert_eq!(
        Response::new(&approver(), SESSION, q.hash(), vec![]).unwrap_err(),
        EncodeError::ItemCount
    );
    let mut results = approved(&q);
    results.swap(0, 1);
    assert_eq!(
        Response::new(&approver(), SESSION, q.hash(), results).unwrap_err(),
        EncodeError::ItemOrder
    );
}

// ---- AS-13 ---------------------------------------------------------------

#[test]
fn as_13_a_response_from_another_approver_or_with_a_bad_signature_is_refused() {
    let q = request();
    let r = Response::new(&id(40), SESSION, q.hash(), approved(&q)).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::WrongApprover)
    );
    let good = response(&q);
    let mut bytes = good.as_bytes().to_vec();
    let n = bytes.len();
    bytes[n - 1] ^= 1;
    let bad = Response::from_bytes(&bytes).unwrap();
    assert_eq!(
        bad.check_against(&q, &approver().device_id()),
        Err(BindError::BadSignature)
    );
}

#[test]
fn as_13_a_response_replayed_into_another_session_is_refused() {
    let q = request();
    let other = Request::new(&owner(), [99; 32], WALLET, items()).unwrap();
    let r = response(&q);
    assert_eq!(
        r.check_against(&other, &approver().device_id()),
        Err(BindError::WrongSession)
    );
}

#[test]
fn as_13_a_response_to_another_request_of_the_same_session_is_refused() {
    let q = request();
    let mut changed = items();
    changed.pop();
    let other = Request::new(&owner(), SESSION, WALLET, changed).unwrap();
    let r = response(&q);
    assert_eq!(
        r.check_against(&other, &approver().device_id()),
        Err(BindError::WrongRequest)
    );
}

#[test]
fn as_13_a_result_for_an_item_that_was_never_asked_is_refused() {
    let q = request();
    let mut results = approved(&q);
    results.push(ItemResult {
        number: 99,
        verdict: Verdict::Approved(Payload::Spend { psbt: vec![1] }),
    });
    let r = Response::new(&approver(), SESSION, q.hash(), results).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::UnknownItem { item: 99 })
    );
}

#[test]
fn as_13_a_missing_result_is_refused() {
    let q = request();
    let mut results = approved(&q);
    results.remove(1);
    let r = Response::new(&approver(), SESSION, q.hash(), results).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::MissingItem { item: 2 })
    );
}

#[test]
fn as_13_a_result_of_a_kind_that_does_not_fit_its_item_is_refused() {
    let q = request();
    let mut results = approved(&q);
    results[0].verdict = Verdict::Approved(Payload::Batch { sig: Sig([1; 64]) });
    let r = Response::new(&approver(), SESSION, q.hash(), results).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::KindMismatch { item: 0 })
    );
}

// ---- AS-14 ---------------------------------------------------------------

#[test]
fn as_14_the_health_answer_is_checked_against_the_pinned_root() {
    let q = request();
    // The answer of the right key, for another number.
    let mut results = approved(&q);
    results[4].verdict = Verdict::Approved(Payload::Health {
        sig: health_proof(&approver(), &SESSION, 8, &[0; 32]),
    });
    let r = Response::new(&approver(), SESSION, q.hash(), results).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::BadHealthProof { item: 8 })
    );
    // The answer of the right key, for another item number.
    let mut results = approved(&q);
    results[4].verdict = Verdict::Approved(Payload::Health {
        sig: health_proof(&approver(), &SESSION, 6, &[6; 32]),
    });
    let r = Response::new(&approver(), SESSION, q.hash(), results).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::BadHealthProof { item: 8 })
    );
    // The right answer.
    assert_eq!(
        response(&q).check_against(&q, &approver().device_id()),
        Ok(())
    );
}

#[test]
fn as_14_a_declined_health_check_is_taken_as_declined() {
    let q = request();
    let mut results = approved(&q);
    results[4].verdict = Verdict::Declined {
        code: DeclineCode::NotDecided,
        text: "not decided".into(),
    };
    let r = Response::new(&approver(), SESSION, q.hash(), results).unwrap();
    assert_eq!(r.check_against(&q, &approver().device_id()), Ok(()));
}

// ---- AS-15 ---------------------------------------------------------------

#[test]
fn as_15_arbitrary_bytes_never_panic_the_decoders() {
    let q = request();
    let r = response(&q);
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for _ in 0..300 {
        let n = (rng.next() % 600) as usize;
        let mut bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let _ = Request::from_bytes(&bytes);
        let _ = Response::from_bytes(&bytes);
        if n >= 5 {
            bytes[..5].copy_from_slice(b"OBMF\x01");
            let _ = Request::from_bytes(&bytes);
            bytes[..5].copy_from_slice(b"OBM2\x01");
            let _ = Response::from_bytes(&bytes);
        }
    }
    // Flip each byte of a good file: an error or a value, never a panic, and
    // a value that decodes encodes back to the bytes.
    for source in [q.as_bytes(), r.as_bytes()] {
        for at in 0..source.len() {
            let mut bytes = source.to_vec();
            bytes[at] ^= 0x5a;
            if let Ok(back) = Request::from_bytes(&bytes) {
                assert_eq!(back.as_bytes(), bytes);
            }
            if let Ok(back) = Response::from_bytes(&bytes) {
                assert_eq!(back.as_bytes(), bytes);
            }
        }
    }
    let _ = DeviceId::from_bytes(&[0; 32]);
}

// ---- AS-78, AS-79: adding a Box ------------------------------------------

fn invitation() -> Invitation {
    Invitation::from_random(&[0x42; 17])
}

fn claim() -> Claim {
    Claim::make(&invitation(), id(7).device_id(), [9; 32])
}

fn box_request() -> Request {
    Request::new(
        &owner(),
        SESSION,
        WALLET,
        vec![
            RequestItem {
                number: 0,
                item: settings(),
            },
            RequestItem {
                number: 1,
                item: Item::BoxClaim { claim: claim() },
            },
            RequestItem {
                number: 2,
                item: health(),
            },
        ],
    )
    .unwrap()
}

#[test]
fn as_78_a_box_claim_item_holds_the_claim_and_nothing_else() {
    let q = box_request();
    let back = Request::from_bytes(q.as_bytes()).unwrap();
    assert_eq!(back, q);
    let Item::BoxClaim { claim: got } = &back.item(1).unwrap().item else {
        panic!("not a box claim")
    };
    assert_eq!(*got, claim());
    assert_eq!(back.item(1).unwrap().item.kind(), ItemKind::BoxClaim);
    assert_eq!(ItemKind::BoxClaim.byte(), 6);
    assert_eq!(ItemKind::from_byte(6), Some(ItemKind::BoxClaim));
    // The claim is not checked here: a claim with a wrong MAC is carried as it
    // is, for the Key app to refuse.
    let mut bad = claim();
    bad.mac[0] ^= 1;
    let carried = Request::new(
        &owner(),
        SESSION,
        WALLET,
        vec![RequestItem {
            number: 0,
            item: Item::BoxClaim { claim: bad },
        }],
    )
    .unwrap();
    assert!(Request::from_bytes(carried.as_bytes()).is_ok());
    // A body that is one byte short, one byte long or another frame is
    // refused. The body of the item starts after the header (5), the session
    // ID, the wallet and the owner (96), the count (4), the number (4), the
    // kind (1) and its length (4).
    let single = Request::new(
        &owner(),
        SESSION,
        WALLET,
        vec![RequestItem {
            number: 0,
            item: Item::BoxClaim { claim: claim() },
        }],
    )
    .unwrap();
    let bytes = single.as_bytes().to_vec();
    let len_at = 5 + 96 + 4 + 4 + 1;
    let body_at = len_at + 4;
    let sig_at = bytes.len() - 64;
    let mut short = bytes.clone();
    short[len_at..body_at].copy_from_slice(&100u32.to_le_bytes());
    short.remove(sig_at - 1);
    assert!(Request::from_bytes(&short).is_err());
    let mut long = bytes.clone();
    long[len_at..body_at].copy_from_slice(&102u32.to_le_bytes());
    long.insert(sig_at, 0);
    assert!(Request::from_bytes(&long).is_err());
    let mut frame = bytes.clone();
    frame[body_at..body_at + 4].copy_from_slice(b"OBMM");
    assert!(Request::from_bytes(&frame).is_err());
    let mut version = bytes;
    version[body_at + 4] = 2;
    assert!(Request::from_bytes(&version).is_err());
}

#[test]
fn as_79_a_box_welcome_answers_a_box_claim_and_only_that() {
    let q = box_request();
    let roots = PinnedRoots::new(owner().device_id(), approver().device_id()).unwrap();
    let welcome = Welcome::make(&invitation(), &claim(), roots);
    let mut results: Vec<ItemResult> = vec![
        ItemResult {
            number: 0,
            verdict: Verdict::Approved(Payload::Settings { sig: Sig([2; 64]) }),
        },
        ItemResult {
            number: 1,
            verdict: Verdict::Approved(Payload::BoxWelcome { welcome }),
        },
        ItemResult {
            number: 2,
            verdict: Verdict::Approved(Payload::Health {
                sig: health_proof(&approver(), &SESSION, 2, &[6; 32]),
            }),
        },
    ];
    let r = Response::new(&approver(), SESSION, q.hash(), results.clone()).unwrap();
    assert_eq!(r.check_against(&q, &approver().device_id()), Ok(()));
    let back = Response::from_bytes(r.as_bytes()).unwrap();
    assert_eq!(back, r);
    assert_eq!(
        back.results().get(1).map(|x| x.verdict.clone()),
        Some(Verdict::Approved(Payload::BoxWelcome { welcome }))
    );
    assert_eq!(Payload::BoxWelcome { welcome }.kind(), ItemKind::BoxClaim);
    // A welcome for a settings item, or settings for the claim, do not fit.
    let swapped = {
        let mut v = results.clone();
        v[0].verdict = Verdict::Approved(Payload::BoxWelcome { welcome });
        v
    };
    let r = Response::new(&approver(), SESSION, q.hash(), swapped).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::KindMismatch { item: 0 })
    );
    results[1].verdict = Verdict::Approved(Payload::Settings { sig: Sig([2; 64]) });
    let r = Response::new(&approver(), SESSION, q.hash(), results.clone()).unwrap();
    assert_eq!(
        r.check_against(&q, &approver().device_id()),
        Err(BindError::KindMismatch { item: 1 })
    );
    // The payload is exactly a welcome: one byte more or fewer is refused.
    results[1].verdict = Verdict::Approved(Payload::BoxWelcome { welcome });
    let good = Response::new(&approver(), SESSION, q.hash(), results).unwrap();
    let bytes = good.as_bytes();
    let mut longer = bytes.to_vec();
    // The payload length of result 1 is found by its welcome magic.
    let at = bytes.windows(4).position(|w| w == b"OBMM").unwrap() - 4;
    longer[at..at + 4].copy_from_slice(&102u32.to_le_bytes());
    longer.insert(at + 4 + 101, 0);
    assert!(Response::from_bytes(&longer).is_err());
    let mut shorter = bytes.to_vec();
    shorter[at..at + 4].copy_from_slice(&100u32.to_le_bytes());
    shorter.remove(at + 4 + 100);
    assert!(Response::from_bytes(&shorter).is_err());
}
