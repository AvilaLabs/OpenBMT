//! What the owner is told when a payment cannot be planned, written,
//! joined or finished (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The reader is the owner: they know the shop and its money and a little
//! bitcoin, not coins, scripts or keys. The causes are grouped by what the
//! owner does about them:
//!
//! - the wallet or the app has a fault: nothing was sent, and the owner
//!   tells support (SC-10);
//! - the amount or the fee is outside what the wallet allows: the words
//!   say which limit, and that it is built into the wallet (the limits the
//!   Key app's own screen sets are applied by the Key app, not here);
//! - there is not enough money that is old enough to spend;
//! - the two signatures do not belong together: the approval starts again.
//!
//! Raw coin identifiers, keys and the inner text of a check go in `detail`.
//! Two ways to the same fault give one sentence: a coin that does not
//! check out reads the same whether it was found while planning or while
//! writing the file, and the same holds for the faults found both when
//! joining and when finishing.

use openbmt_core::say::{sats, Tone, Who, Words};

use crate::combine::Source;
use crate::{CoinError, CombineError, FinalizeError, PlanError, PsbtError};

/// The sentence every fault of the app itself ends in (SC-10).
const TELL_SUPPORT: &str = "Nothing was sent. Start again; if it repeats, stop and report it to the OpenBMT project with the details.";

fn stop(heading: &str, what: impl Into<String>, who: Who, next: impl Into<String>) -> Words {
    Words::new(Tone::Stop, heading, what, who, next)
}

fn fault(heading: &str, what: impl Into<String>) -> Words {
    stop(heading, what, Who::Owner, TELL_SUPPORT)
}

/// The place a problem was found, for the details.
fn place(source: Source, input: usize) -> String {
    format!("{source}, input {input}")
}

impl CoinError {
    /// What the owner is told when a coin of the wallet does not check out.
    /// The coin's identifier is in the details.
    pub fn words(&self) -> Words {
        match self {
            Self::PreviousTransactionMismatch { outpoint } => fault(
                "A coin does not check out",
                "The record of the payment that gave the shop a coin is not the one the coin names.",
            )
            .code("SP-01")
            .detail(format!("coin {outpoint}")),
            Self::OutputMissing { outpoint } => fault(
                "A coin points nowhere",
                "A coin names a place in an earlier payment that is not there.",
            )
            .code("SP-01")
            .detail(format!("coin {outpoint}")),
            Self::ScriptNotInWallet { outpoint } => fault(
                "A coin is not this wallet's",
                "The address a coin sits on is not one this wallet makes at the place the coin names.",
            )
            .code("SP-02")
            .detail(format!("coin {outpoint}")),
        }
    }
}

impl PlanError {
    /// What the owner is told when no payment could be planned. Nothing was
    /// sent and nothing was stored.
    pub fn words(&self) -> Words {
        match self {
            Self::Coin(e) => e.words(),
            Self::DuplicateCoin(outpoint) => fault(
                "A coin was listed twice",
                "The coin list holds the same coin twice, so no payment was planned.",
            )
            .code("SP-03")
            .detail(format!("coin {outpoint}")),
            Self::NoPayments => stop(
                "No payment to make",
                "The request has no payment in it, so there is nothing to plan.",
                Who::Owner,
                "Enter an amount and where it goes, then try again.",
            )
            .code("SP-25"),
            Self::SendAllNeedsOnePayment => stop(
                "Send everything: one address",
                "Sending everything takes exactly one address to send it to, and this request has a different number of them.",
                Who::Owner,
                "Choose one address for the cash-out of everything and try again.",
            )
            .code("SP-12"),
            Self::SendAllAmountGiven => fault(
                "Cash-out given an amount",
                "A cash-out of everything was given an amount, but it takes its amount from the coins.",
            )
            .code("SP-12"),
            Self::ZeroFeeRate => stop(
                "Fee rate is zero",
                "The fee rate is zero, so the payment would never be picked up by the network.",
                Who::Owner,
                "Choose a fee rate above zero and try again.",
            )
            .code("SP-32"),
            Self::ChangeScript => fault(
                "No change address made",
                "The wallet could not make the address that the change would go to.",
            )
            .code("SP-21"),
            Self::PaymentScriptKind { payment } => stop(
                "Address not accepted",
                "An address in the request is not of a kind this wallet will pay.",
                Who::Owner,
                "Ask the person to be paid for a standard bitcoin address and enter it again.",
            )
            .code("SP-22")
            .detail(format!("payment {payment}")),
            Self::PaymentBelowDust {
                payment,
                sats: amount,
                dust,
            } => stop(
                "Payment too small to send",
                format!(
                    "A payment of {} is below the smallest amount that its address can take, {}.",
                    sats(*amount),
                    sats(*dust)
                ),
                Who::Owner,
                "Raise the payment to at least that, or leave it out.",
            )
            .code("SP-22")
            .detail(format!("payment {payment}")),
            Self::AmountTooLarge => stop(
                "Amount too large",
                "An amount, or the total of the amounts, is more than all the bitcoin that can exist.",
                Who::Owner,
                "Check the digits of each amount and enter them again.",
            )
            .code("SP-22"),
            Self::LockTimeNotHeight { lock_height } => lock_time(*lock_height, "SP-37"),
            Self::PaymentToOwnScript { payment } => stop(
                "Payment to the shop's wallet",
                "An address in the request belongs to this wallet, so the payment would only move money inside it.",
                Who::Owner,
                "Enter the outside address you meant to pay, then try again.",
            )
            .code("SP-24")
            .detail(format!("payment {payment}")),
            Self::TooManyOutputs => stop(
                "Too many payments at once",
                "One transaction can pay at most 100 addresses, change included, and this one has more.",
                Who::Owner,
                "Pay fewer addresses now and queue the rest for another approval. This limit is built in.",
            )
            .code("SP-25"),
            Self::TooManyInputs => stop(
                "Too many coins needed",
                "Paying this would take more than 1,000 coins, which is more than one transaction can hold.",
                Who::Owner,
                "Pay a smaller amount now and the rest later. This limit is built into the wallet.",
            )
            .code("SP-33"),
            Self::NoEligibleCoins => Words::new(
                Tone::Stop,
                "No money is ready to spend",
                "None of the shop's coins is old enough to spend yet.",
                Who::Owner,
                "Wait until newly received money is in a block, about ten minutes, then try again.",
            )
            .code("SP-10"),
            Self::InsufficientFunds { available, needed } => stop(
                "Not enough money to spend",
                format!(
                    "The money ready to spend is {}, and this payment with its fee needs {}.",
                    sats(*available),
                    sats(*needed)
                ),
                Who::Owner,
                "Pay less, or wait until newly received money is in a block; it can be spent once it is.",
            ),
            Self::MustSpendNotEligible {
                outpoint,
                confirmations,
                needed,
            } => stop(
                "A coin to replace is too new",
                "A coin that must be spent to replace a dropped payment is not old enough to spend yet.",
                Who::Owner,
                "Wait until that coin is in a block, then try again.",
            )
            .code("SP-13")
            .detail(format!(
                "coin {outpoint} has {confirmations} confirmations, {needed} are needed"
            )),
            Self::SendAllBelowDust { sats: left, dust } => stop(
                "Too little left after fee",
                format!(
                    "After the fee only {} would be left, below the smallest amount that can be sent, {}.",
                    sats(*left),
                    sats(*dust)
                ),
                Who::Owner,
                "Wait until more money has arrived, or cancel this cash-out of everything.",
            )
            .code("SP-12"),
            Self::FeeTooHigh { fee, limit } => stop(
                "Fee is over the limit",
                format!(
                    "The fee would be {}, above the highest fee the wallet allows, {}.",
                    sats(*fee),
                    sats(*limit)
                ),
                Who::Owner,
                "Choose a lower fee rate. The wallet's top limit is fixed; the key holder can set a lower one on the Key app.",
            )
            .code("SP-32"),
            Self::FeeRateTooHigh { fee, vsize, limit } => stop(
                "Fee rate is over the limit",
                format!(
                    "The fee would be more than {} for each byte of the payment, the most the wallet allows.",
                    sats(*limit)
                ),
                Who::Owner,
                "Choose a lower fee rate. The wallet's top limit is fixed; the key holder can set a lower one on the Key app.",
            )
            .code("SP-32")
            .detail(format!("fee {fee} sat over {vsize} vB")),
            Self::TooHeavy { weight, limit } => too_large(*weight, *limit, "SP-33"),
            Self::Overflow => fault(
                "Amounts do not add up",
                "An amount in the request was too large to add up, so no payment was planned.",
            )
            .code("SP-34"),
            Self::Internal(what) => own_check(what),
        }
    }
}

/// The payment is too large for the network to pass on: found when it is
/// planned and again when it is finished, the same fault.
fn too_large(weight: u64, limit: u64, code: &'static str) -> Words {
    stop(
        "Payment too large to send",
        "The payment would be too large for the network to pass on.",
        Who::Owner,
        "Pay fewer refunds or fewer coins at a time. This limit is built into the wallet.",
    )
    .code(code)
    .detail(format!("weight {weight}, limit {limit}"))
}

/// The lock time asked for is not a block height: the same fault whether it
/// is found while planning or while finishing.
fn lock_time(value: u32, code: &'static str) -> Words {
    fault(
        "Payment time not accepted",
        "The earliest time the payment may be sent was given in a form the wallet does not take.",
    )
    .code(code)
    .detail(format!("lock time {value}"))
}

/// A check the app makes of its own work did not pass (SC-10).
fn own_check(what: &str) -> Words {
    fault(
        "The app's check did not pass",
        "A check the app makes of its own work did not pass, so no payment was made.",
    )
    .detail(what)
}

impl PsbtError {
    /// What the owner is told when a plan could not be written as the file
    /// the Key app signs. Nothing was sent.
    pub fn words(&self) -> Words {
        match self {
            Self::Coin(e) => e.words(),
            Self::Derivation => fault(
                "Wallet keys not made",
                "The wallet could not work out the keys for the payment file, so none was made.",
            )
            .code("SP-40"),
            Self::Internal(what) => fault(
                "Payment file not made",
                "The app could not turn the planned payment into a file for the Key app.",
            )
            .detail(*what),
        }
    }
}

/// The file is not a payment file of the shape the Key app signs: the same
/// fault when joining and when finishing.
fn file_shape() -> Words {
    fault(
        "Payment file is not usable",
        "A file that should be a payment for signing is not in the form the wallet expects.",
    )
}

/// The file already holds a finished coin: the same fault when joining and
/// when finishing.
fn already_final() -> Words {
    stop(
        "A file was already finished",
        "A coin in one of the files is already finished, so it cannot take more signatures.",
        Who::Owner,
        "Start the approval again so both signatures are made on a fresh copy.",
    )
}

/// A signature does not cover the whole payment.
fn partial_signature() -> Words {
    stop(
        "Signature covers too little",
        "A signature does not cover the whole payment, so it could be reused for another one.",
        Who::Owner,
        "Do not use this file. Start the approval again.",
    )
}

/// A signature is by a key that does not belong to the coin.
fn foreign_key() -> Words {
    stop(
        "Signature by the wrong key",
        "A signature on a coin is by a key that is not one of the three keys that guard it.",
        Who::Owner,
        "Do not use this file. Start the approval again, and check that the right Key app signs.",
    )
}

/// A signature does not check out.
fn bad_signature() -> Words {
    stop(
        "Signature does not check out",
        "A signature on a coin is not valid for this payment.",
        Who::Owner,
        "Do not use this file. Start the approval again.",
    )
}

/// The base file lacks or garbles what the wallet needs about a coin.
fn coin_data(heading: &str, what: &str) -> Words {
    fault(heading, what)
}

fn missing_previous() -> Words {
    coin_data(
        "A coin's history is missing",
        "The file lacks the earlier payment that gave the shop one of its coins.",
    )
}

fn previous_mismatch() -> Words {
    coin_data(
        "A coin's history is wrong",
        "The earlier payment stored in the file is not the one the coin comes from.",
    )
}

fn output_missing() -> Words {
    coin_data(
        "A coin points nowhere",
        "The file's note of where a coin sits in its earlier payment points at nothing.",
    )
}

fn value_mismatch() -> Words {
    coin_data(
        "A coin's value is in doubt",
        "The file holds two records of what a coin is worth, and they do not agree.",
    )
}

impl CombineError {
    /// What the owner is told when the signatures of the two phones could
    /// not be joined. Nothing was sent. The file and the input are in the
    /// details.
    pub fn words(&self) -> Words {
        match self {
            Self::NotVersionZero { source } => file_shape().detail(format!("{source}")),
            Self::DifferentTransaction { copy } => stop(
                "Copy is for another payment",
                "A signed copy holds a different payment than the one that was planned.",
                Who::Owner,
                "Do not use it. Start the approval again so both copies come from the same payment.",
            )
            .code("SP-50")
            .detail(format!("signed copy {copy}")),
            Self::DifferentInputData { copy, input } => stop(
                "Copy changed coin details",
                "A signed copy has other details for one of the coins than the original has.",
                Who::Owner,
                "Do not use it. Start the approval again so both copies come from the same payment.",
            )
            .code("SP-50")
            .detail(format!("signed copy {copy}, input {input}")),
            Self::InputAlreadyFinal { source, input } => already_final()
                .code("SP-50")
                .detail(place(*source, *input)),
            Self::MissingPreviousTransaction { input } => {
                missing_previous().detail(format!("base file, input {input}"))
            }
            Self::PreviousTransactionMismatch { input } => {
                previous_mismatch().detail(format!("base file, input {input}"))
            }
            Self::PreviousOutputMissing { input } => {
                output_missing().detail(format!("base file, input {input}"))
            }
            Self::WitnessUtxoMismatch { input } => {
                value_mismatch().detail(format!("base file, input {input}"))
            }
            Self::WitnessScriptShape { input } => coin_data(
                "Coin's signing rule wrong",
                "The file does not hold the wallet's rule of two keys out of three for one of the coins.",
            )
            .detail(format!("base file, input {input}")),
            Self::WitnessScriptNotSpent { input } => coin_data(
                "Wrong signing rule for coin",
                "The signing rule in the file is not the one that guards the coin it is for.",
            )
            .detail(format!("base file, input {input}")),
            Self::SighashType { source, input } => partial_signature()
                .code("SP-52")
                .detail(place(*source, *input)),
            Self::ForeignKey { source, input, key } => foreign_key()
                .code("SP-51")
                .detail(format!("{}, key {key}", place(*source, *input))),
            Self::BadSignature { source, input, key } => bad_signature()
                .code("SP-53")
                .detail(format!("{}, key {key}", place(*source, *input))),
            Self::Internal(what) => own_check(what),
        }
    }
}

/// What the owner is told for the coin hints and rules of a file that are
/// not this wallet's.
fn not_this_wallet(heading: &str, what: &str, input: usize) -> Words {
    stop(
        heading,
        what,
        Who::Owner,
        "Do not use this file. Start the approval again; if it repeats, stop and report it to the OpenBMT project.",
    )
    .code("SP-61")
    .detail(format!("input {input}"))
}

impl FinalizeError {
    /// What the owner is told when the two signatures could not be made into
    /// a payment ready to send. Nothing was sent. The input and the numbers
    /// are in the details.
    pub fn words(&self) -> Words {
        match self {
            Self::NotVersionZero => file_shape(),
            Self::MissingPreviousTransaction { input } => {
                missing_previous().detail(format!("input {input}"))
            }
            Self::PreviousTransactionMismatch { input } => {
                previous_mismatch().detail(format!("input {input}"))
            }
            Self::PreviousOutputMissing { input } => {
                output_missing().detail(format!("input {input}"))
            }
            Self::WitnessUtxoMismatch { input } => {
                value_mismatch().detail(format!("input {input}"))
            }
            Self::NoWalletKeyHint { input } => not_this_wallet(
                "File does not name the keys",
                "The file does not say where in this wallet the keys of a coin come from.",
                *input,
            ),
            Self::HintsInconsistent { input } => not_this_wallet(
                "File's key notes disagree",
                "The file's notes about the keys of a coin disagree with each other or with the wallet.",
                *input,
            ),
            Self::ScriptNotInWallet { input } => not_this_wallet(
                "A coin is not this wallet's",
                "The address a coin sits on is not one this wallet makes at the place the file names.",
                *input,
            ),
            Self::WitnessScriptMismatch { input } => not_this_wallet(
                "File's signing rule is wrong",
                "The signing rule in the file for a coin is not this wallet's rule.",
                *input,
            ),
            Self::InputAlreadyFinal { input } => {
                already_final().detail(format!("input {input}"))
            }
            Self::SighashType { input } => partial_signature().detail(format!("input {input}")),
            Self::ForeignKey { input, key } => {
                foreign_key().detail(format!("input {input}, key {key}"))
            }
            Self::BadSignature { input, key } => {
                bad_signature().detail(format!("input {input}, key {key}"))
            }
            Self::NotEnoughSignatures { input, have } => stop(
                "Payment is not fully signed",
                "A coin does not yet have valid signatures from two different keys.",
                Who::Owner,
                "Finish the approval on the Key app, take its answer here again, then finish the payment.",
            )
            .code("SP-60")
            .detail(format!("input {input} has {have}, it needs 2")),
            Self::AmountOutOfRange => fault(
                "An amount is out of range",
                "An amount in the file is outside what can exist, so the payment was not finished.",
            )
            .code("SP-62"),
            Self::OutputsExceedInputs => fault(
                "Payment sends too much",
                "The payment would send out more than the coins it spends hold.",
            )
            .code("SP-62"),
            Self::TooHeavy { weight } => too_large(*weight, 400_000, "SP-63"),
            Self::FinalCheckFailed(what) => fault(
                "The last check did not pass",
                "The app's last check of the finished payment did not pass, so it was not made.",
            )
            .code("SP-62")
            .detail(*what),
            Self::LockTimeNotHeight { lock_time: value } => lock_time(*value, "SP-37"),
            Self::NotThePlan => stop(
                "Not the planned payment",
                "The signed file holds a different payment than the one the shop planned.",
                Who::Owner,
                "Do not send it. Start the approval again.",
            )
            .code("SP-64"),
            Self::FeeNotThePlan { found, planned } => stop(
                "Fee is not the planned fee",
                format!(
                    "The signed file carries a fee of {}, not the {} that was planned.",
                    sats(*found),
                    sats(*planned)
                ),
                Who::Owner,
                "Do not send it. Start the approval again.",
            )
            .code("SP-64"),
        }
    }
}

/// SC-12: the words of every refusal of this crate, with at least one value
/// for every variant and every variant inside it. A fault found both when
/// planning and when writing, or when joining and when finishing, has one
/// entry, named for the cause. Built by hand so that a new variant is
/// noticed.
pub fn all_words() -> Vec<(String, Words)> {
    let outpoint = bitcoin::OutPoint::null();
    let Some(key) = sample_key() else {
        return Vec::new();
    };
    let mut all: Vec<(String, Words)> = Vec::new();
    let mut add = |name: &str, words: Words| all.push((name.to_string(), words));

    add(
        "CoinError::PreviousTransactionMismatch",
        CoinError::PreviousTransactionMismatch { outpoint }.words(),
    );
    add(
        "CoinError::OutputMissing",
        CoinError::OutputMissing { outpoint }.words(),
    );
    add(
        "CoinError::ScriptNotInWallet",
        CoinError::ScriptNotInWallet { outpoint }.words(),
    );

    let plan: Vec<(&str, PlanError)> = vec![
        (
            "PlanError::DuplicateCoin",
            PlanError::DuplicateCoin(outpoint),
        ),
        ("PlanError::NoPayments", PlanError::NoPayments),
        (
            "PlanError::SendAllNeedsOnePayment",
            PlanError::SendAllNeedsOnePayment,
        ),
        (
            "PlanError::SendAllAmountGiven",
            PlanError::SendAllAmountGiven,
        ),
        ("PlanError::ZeroFeeRate", PlanError::ZeroFeeRate),
        ("PlanError::ChangeScript", PlanError::ChangeScript),
        (
            "PlanError::PaymentScriptKind",
            PlanError::PaymentScriptKind { payment: 1 },
        ),
        (
            "PlanError::PaymentBelowDust",
            PlanError::PaymentBelowDust {
                payment: 1,
                sats: 100,
                dust: 294,
            },
        ),
        ("PlanError::AmountTooLarge", PlanError::AmountTooLarge),
        (
            "PlanError::LockTimeNotHeight",
            PlanError::LockTimeNotHeight {
                lock_height: 600_000_000,
            },
        ),
        (
            "PlanError::PaymentToOwnScript",
            PlanError::PaymentToOwnScript { payment: 0 },
        ),
        ("PlanError::TooManyOutputs", PlanError::TooManyOutputs),
        ("PlanError::TooManyInputs", PlanError::TooManyInputs),
        ("PlanError::NoEligibleCoins", PlanError::NoEligibleCoins),
        (
            "PlanError::InsufficientFunds",
            PlanError::InsufficientFunds {
                available: 10_000,
                needed: 12_345,
            },
        ),
        (
            "PlanError::MustSpendNotEligible",
            PlanError::MustSpendNotEligible {
                outpoint,
                confirmations: 0,
                needed: 1,
            },
        ),
        (
            "PlanError::SendAllBelowDust",
            PlanError::SendAllBelowDust {
                sats: 200,
                dust: 330,
            },
        ),
        (
            "PlanError::FeeTooHigh",
            PlanError::FeeTooHigh {
                fee: 3_000_000,
                limit: 2_000_000,
            },
        ),
        (
            "PlanError::FeeRateTooHigh",
            PlanError::FeeRateTooHigh {
                fee: 500_000,
                vsize: 200,
                limit: 1_000,
            },
        ),
        (
            "PlanError::TooHeavy",
            PlanError::TooHeavy {
                weight: 500_000,
                limit: 400_000,
            },
        ),
        ("PlanError::Overflow", PlanError::Overflow),
        ("PlanError::Internal", PlanError::Internal("a sample check")),
    ];
    for (name, e) in plan {
        add(name, e.words());
    }

    add("PsbtError::Derivation", PsbtError::Derivation.words());
    add(
        "PsbtError::Internal",
        PsbtError::Internal("input count").words(),
    );

    let combine: Vec<(&str, CombineError)> = vec![
        (
            "CombineError::NotVersionZero",
            CombineError::NotVersionZero {
                source: Source::Base,
            },
        ),
        (
            "CombineError::DifferentTransaction",
            CombineError::DifferentTransaction { copy: 0 },
        ),
        (
            "CombineError::DifferentInputData",
            CombineError::DifferentInputData { copy: 1, input: 0 },
        ),
        (
            "CombineError::InputAlreadyFinal",
            CombineError::InputAlreadyFinal {
                source: Source::Copy(0),
                input: 0,
            },
        ),
        (
            "CombineError::MissingPreviousTransaction",
            CombineError::MissingPreviousTransaction { input: 0 },
        ),
        (
            "CombineError::PreviousTransactionMismatch",
            CombineError::PreviousTransactionMismatch { input: 0 },
        ),
        (
            "CombineError::PreviousOutputMissing",
            CombineError::PreviousOutputMissing { input: 0 },
        ),
        (
            "CombineError::WitnessUtxoMismatch",
            CombineError::WitnessUtxoMismatch { input: 0 },
        ),
        (
            "CombineError::WitnessScriptShape",
            CombineError::WitnessScriptShape { input: 0 },
        ),
        (
            "CombineError::WitnessScriptNotSpent",
            CombineError::WitnessScriptNotSpent { input: 0 },
        ),
        (
            "CombineError::SighashType",
            CombineError::SighashType {
                source: Source::Copy(1),
                input: 0,
            },
        ),
        (
            "CombineError::ForeignKey",
            CombineError::ForeignKey {
                source: Source::Copy(1),
                input: 0,
                key,
            },
        ),
        (
            "CombineError::BadSignature",
            CombineError::BadSignature {
                source: Source::Copy(0),
                input: 0,
                key,
            },
        ),
    ];
    for (name, e) in combine {
        add(name, e.words());
    }

    let finalize: Vec<(&str, FinalizeError)> = vec![
        (
            "FinalizeError::NoWalletKeyHint",
            FinalizeError::NoWalletKeyHint { input: 0 },
        ),
        (
            "FinalizeError::HintsInconsistent",
            FinalizeError::HintsInconsistent { input: 0 },
        ),
        (
            "FinalizeError::ScriptNotInWallet",
            FinalizeError::ScriptNotInWallet { input: 0 },
        ),
        (
            "FinalizeError::WitnessScriptMismatch",
            FinalizeError::WitnessScriptMismatch { input: 0 },
        ),
        (
            "FinalizeError::NotEnoughSignatures",
            FinalizeError::NotEnoughSignatures { input: 0, have: 1 },
        ),
        (
            "FinalizeError::AmountOutOfRange",
            FinalizeError::AmountOutOfRange,
        ),
        (
            "FinalizeError::OutputsExceedInputs",
            FinalizeError::OutputsExceedInputs,
        ),
        (
            "FinalizeError::FinalCheckFailed",
            FinalizeError::FinalCheckFailed("a sample check"),
        ),
        ("FinalizeError::NotThePlan", FinalizeError::NotThePlan),
        (
            "FinalizeError::FeeNotThePlan",
            FinalizeError::FeeNotThePlan {
                found: 2_500,
                planned: 2_000,
            },
        ),
    ];
    for (name, e) in finalize {
        add(name, e.words());
    }
    all
}

/// A key to put in the details of a sample: the generator point.
fn sample_key() -> Option<bitcoin::PublicKey> {
    let bytes = [
        0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87,
        0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16,
        0xf8, 0x17, 0x98,
    ];
    bitcoin::PublicKey::from_slice(&bytes).ok()
}

/// SC-12: the rows of this crate's words in the one table of
/// `docs/screens/owner.md`, one line each, in the order of [`all_words`].
pub fn screens_markdown() -> String {
    let cell = |t: &str| t.replace('|', "/");
    let mut out = String::new();
    for (name, w) in all_words() {
        out.push_str(&format!(
            "| spend: {name} | {} | {} | {} | {:?} | {} |\n",
            cell(&w.heading),
            cell(&w.what),
            cell(&w.next),
            w.who,
            w.code.unwrap_or("")
        ));
    }
    out
}

#[cfg(test)]
#[path = "say_tests.rs"]
mod tests;
