//! Why the signer refused, and the error types of its calls.
//!
//! Every refusal names the rule of `docs/SIGNER-RULES.md` that it enforces
//! ([`Refusal::rule`]) and carries the numbers a screen needs to say what
//! was wrong.

use std::fmt;

use openbmt_core::wallet::WalletError;

/// One of the six ceilings of SR-35 and SR-36.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ceiling {
    /// Other destinations, per transaction (SR-35).
    OtherPerTx,
    /// Other destinations plus fee, per day (SR-36).
    OtherDay,
    /// Other destinations plus fee, per week (SR-36).
    OtherWeek,
    /// Allowlisted destinations, per transaction (SR-35).
    AllowlistedPerTx,
    /// Allowlisted destinations, per day (SR-36).
    AllowlistedDay,
    /// Allowlisted destinations, per week (SR-36).
    AllowlistedWeek,
}

impl Ceiling {
    /// The rule this ceiling belongs to.
    pub fn rule(self) -> &'static str {
        match self {
            Self::OtherPerTx | Self::AllowlistedPerTx => "SR-35",
            _ => "SR-36",
        }
    }

    /// A short plain name.
    pub fn name(self) -> &'static str {
        match self {
            Self::OtherPerTx => "per-transaction ceiling for other destinations",
            Self::OtherDay => "day ceiling for other destinations",
            Self::OtherWeek => "week ceiling for other destinations",
            Self::AllowlistedPerTx => "per-transaction ceiling for allowlisted destinations",
            Self::AllowlistedDay => "day ceiling for allowlisted destinations",
            Self::AllowlistedWeek => "week ceiling for allowlisted destinations",
        }
    }
}

/// A reason the signer will not sign a transaction file. Nothing was signed
/// and no state changed.
///
/// Indexes are positions in the transaction's inputs and outputs, counted
/// from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// SR-01: the bytes are not a BIP174 PSBT of version 0.
    NotAPsbt,
    /// SR-02: the file is larger than the limit.
    FileTooLarge {
        /// Size of the file in bytes.
        len: usize,
    },
    /// SR-02: no inputs, or more than the limit.
    InputCount {
        /// Number of inputs found.
        found: usize,
    },
    /// SR-02: no outputs, or more than the limit.
    OutputCount {
        /// Number of outputs found.
        found: usize,
    },
    /// SR-03: the transaction version is not 2.
    TransactionVersion {
        /// The version found.
        found: i32,
    },
    /// SR-03: the lock time is a time, not a block height.
    LockTimeNotHeight {
        /// The lock time found.
        lock_time: u32,
    },
    /// SR-03: an input's sequence number allows a relative time lock.
    RelativeTimeLock {
        /// The input.
        input: usize,
        /// Its sequence number.
        sequence: u32,
    },
    /// SR-04: an input appears twice.
    DuplicateInput {
        /// The second appearance.
        input: usize,
    },
    /// SR-05: an input already has a final script or witness.
    InputAlreadyFinal {
        /// The input.
        input: usize,
    },
    /// SR-06: an input carries a Taproot, redeem script or preimage field.
    ForbiddenInputField {
        /// The input.
        input: usize,
        /// The field.
        field: &'static str,
    },
    /// SR-06: an output carries a Taproot or redeem script field.
    ForbiddenOutputField {
        /// The output.
        output: usize,
        /// The field.
        field: &'static str,
    },
    /// SR-07: an input asks for a signature-hash type other than ALL.
    SighashType {
        /// The input.
        input: usize,
        /// The value found.
        found: u32,
    },
    /// SR-08: a global extended public key is not a key of the registered
    /// wallet with its origin.
    ForeignXpub,
    /// SR-10: an input has no full previous transaction.
    MissingPreviousTransaction {
        /// The input.
        input: usize,
    },
    /// SR-10: the previous transaction does not hash to the input's
    /// previous transaction ID.
    PreviousTransactionMismatch {
        /// The input.
        input: usize,
    },
    /// SR-10: the previous transaction has no output at the index spent.
    PreviousOutputMissing {
        /// The input.
        input: usize,
        /// The output index asked for.
        index: u32,
    },
    /// SR-11: the input's witness UTXO differs from the previous output.
    WitnessUtxoMismatch {
        /// The input.
        input: usize,
    },
    /// SR-12: no key-path hint of the input names a key of the wallet.
    NoWalletKeyHint {
        /// The input.
        input: usize,
    },
    /// SR-12: a hint names the wallet's key slot but not the key the signer
    /// derives there.
    HintKeyMismatch {
        /// The input.
        input: usize,
    },
    /// SR-12: the hints of one input name different keychains or indexes.
    HintsDisagree {
        /// The input.
        input: usize,
    },
    /// SR-12: the script the signer derives for the claimed keychain and
    /// index is not the script being spent.
    ScriptNotInWallet {
        /// The input.
        input: usize,
    },
    /// SR-13: the input's witness script is not the one the signer derived.
    WitnessScriptMismatch {
        /// The input.
        input: usize,
    },
    /// SR-14: a signature is for a key that is not one of the input's keys.
    ForeignSignature {
        /// The input.
        input: usize,
    },
    /// SR-14: a signature does not use SIGHASH_ALL.
    SignatureSighashType {
        /// The input.
        input: usize,
    },
    /// SR-14: a signature does not verify.
    BadSignature {
        /// The input.
        input: usize,
    },
    /// SR-14: this device's signature is already on every input.
    AlreadySigned,
    /// SR-15: an input amount is above 21,000,000 BTC.
    InputAmountTooLarge {
        /// The input.
        input: usize,
        /// The amount in satoshis.
        amount: u64,
    },
    /// SR-15: the input total overflows.
    InputTotalOverflow,
    /// SR-20: a hint on an output names the wallet's key slot but not the
    /// key the signer derives there.
    OutputHintKeyMismatch {
        /// The output.
        output: usize,
    },
    /// SR-20: the hints of one output name different keychains or indexes.
    OutputHintsDisagree {
        /// The output.
        output: usize,
    },
    /// SR-22: an output script is not P2WPKH, P2WSH, P2TR, P2PKH or P2SH.
    OutputScriptKind {
        /// The output.
        output: usize,
    },
    /// SR-24: an output has a zero amount.
    OutputAmountZero {
        /// The output.
        output: usize,
    },
    /// SR-24: an output amount is above 21,000,000 BTC.
    OutputAmountTooLarge {
        /// The output.
        output: usize,
        /// The amount in satoshis.
        amount: u64,
    },
    /// SR-24: the output total overflows.
    OutputTotalOverflow,
    /// SR-24: the outputs add up to more than the inputs.
    OutputsExceedInputs {
        /// Output total in satoshis.
        outputs: u64,
        /// Input total in satoshis.
        inputs: u64,
    },
    /// SR-34: the change index is too far ahead of the change mark.
    ChangeIndexTooFar {
        /// The output.
        output: usize,
        /// The change index asked for.
        index: u32,
        /// The first index that is not allowed: change mark plus 20.
        limit: u64,
    },
    /// SR-35 or SR-36: the amount would be above this ceiling.
    OverCeiling {
        /// Which ceiling.
        ceiling: Ceiling,
        /// The amount or total that was asked for, in satoshis.
        amount: u64,
        /// The ceiling in satoshis.
        limit: u64,
    },
    /// SR-36: a total overflows.
    TotalsOverflow,
    /// SR-33: the spend log has no room for this transaction.
    SpendLogFull,
    /// SR-40: the fee rate is above the fee-rate ceiling.
    FeeRateTooHigh {
        /// The fee in satoshis.
        fee: u64,
        /// The signed size in virtual bytes.
        vsize: u64,
        /// The ceiling in satoshis per virtual byte.
        limit: u64,
    },
    /// SR-41: the fee is above the absolute fee ceiling.
    FeeTooHigh {
        /// The fee in satoshis.
        fee: u64,
        /// The ceiling in satoshis.
        limit: u64,
    },
    /// SR-43: the signed transaction is too heavy.
    WeightTooHigh {
        /// The weight in weight units.
        weight: u64,
        /// The limit in weight units.
        limit: u64,
    },
    /// SR-51: an output is not what the review showed.
    ReviewOutdated {
        /// The first output whose kind differs.
        output: usize,
    },
    /// SR-51: the review was made for another wallet.
    ReviewForOtherWallet,
    /// SR-52: the signing key is not this device's key of the wallet.
    WrongKey,
    /// SR-53 or SR-54: the signer could not make or check its own
    /// signature. This is a fault in the signer, not in the file.
    Internal(&'static str),
}

impl Refusal {
    /// The ID of the rule in `docs/SIGNER-RULES.md` that was broken.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NotAPsbt => "SR-01",
            Self::FileTooLarge { .. } | Self::InputCount { .. } | Self::OutputCount { .. } => {
                "SR-02"
            }
            Self::TransactionVersion { .. }
            | Self::LockTimeNotHeight { .. }
            | Self::RelativeTimeLock { .. } => "SR-03",
            Self::DuplicateInput { .. } => "SR-04",
            Self::InputAlreadyFinal { .. } => "SR-05",
            Self::ForbiddenInputField { .. } | Self::ForbiddenOutputField { .. } => "SR-06",
            Self::SighashType { .. } => "SR-07",
            Self::ForeignXpub => "SR-08",
            Self::MissingPreviousTransaction { .. }
            | Self::PreviousTransactionMismatch { .. }
            | Self::PreviousOutputMissing { .. } => "SR-10",
            Self::WitnessUtxoMismatch { .. } => "SR-11",
            Self::NoWalletKeyHint { .. }
            | Self::HintKeyMismatch { .. }
            | Self::HintsDisagree { .. }
            | Self::ScriptNotInWallet { .. } => "SR-12",
            Self::WitnessScriptMismatch { .. } => "SR-13",
            Self::ForeignSignature { .. }
            | Self::SignatureSighashType { .. }
            | Self::BadSignature { .. }
            | Self::AlreadySigned => "SR-14",
            Self::InputAmountTooLarge { .. } | Self::InputTotalOverflow => "SR-15",
            Self::OutputHintKeyMismatch { .. } | Self::OutputHintsDisagree { .. } => "SR-20",
            Self::OutputScriptKind { .. } => "SR-22",
            Self::OutputAmountZero { .. }
            | Self::OutputAmountTooLarge { .. }
            | Self::OutputTotalOverflow
            | Self::OutputsExceedInputs { .. } => "SR-24",
            Self::ChangeIndexTooFar { .. } => "SR-34",
            Self::OverCeiling { ceiling, .. } => ceiling.rule(),
            Self::TotalsOverflow => "SR-36",
            Self::SpendLogFull => "SR-33",
            Self::FeeRateTooHigh { .. } => "SR-40",
            Self::FeeTooHigh { .. } => "SR-41",
            Self::WeightTooHigh { .. } => "SR-43",
            Self::ReviewOutdated { .. } | Self::ReviewForOtherWallet => "SR-51",
            Self::WrongKey => "SR-52",
            Self::Internal(_) => "SR-54",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::NotAPsbt => write!(f, "this is not a version 0 PSBT file"),
            Self::FileTooLarge { len } => write!(f, "the file is too large ({len} bytes)"),
            Self::InputCount { found } => write!(f, "the transaction has {found} inputs"),
            Self::OutputCount { found } => write!(f, "the transaction has {found} outputs"),
            Self::TransactionVersion { found } => {
                write!(f, "the transaction version is {found}, not 2")
            }
            Self::LockTimeNotHeight { lock_time } => {
                write!(f, "the lock time {lock_time} is not a block height")
            }
            Self::RelativeTimeLock { input, sequence } => write!(
                f,
                "input {input} has sequence number {sequence:#x}, which allows a relative time lock"
            ),
            Self::DuplicateInput { input } => write!(f, "input {input} is listed twice"),
            Self::InputAlreadyFinal { input } => write!(f, "input {input} is already final"),
            Self::ForbiddenInputField { input, field } => {
                write!(f, "input {input} carries {field}")
            }
            Self::ForbiddenOutputField { output, field } => {
                write!(f, "output {output} carries {field}")
            }
            Self::SighashType { input, found } => write!(
                f,
                "input {input} asks for signature-hash type {found}, not ALL"
            ),
            Self::ForeignXpub => write!(f, "the file names an extended key that is not the wallet's"),
            Self::MissingPreviousTransaction { input } => {
                write!(f, "input {input} has no full previous transaction")
            }
            Self::PreviousTransactionMismatch { input } => write!(
                f,
                "the previous transaction of input {input} has another ID than the input names"
            ),
            Self::PreviousOutputMissing { input, index } => write!(
                f,
                "the previous transaction of input {input} has no output {index}"
            ),
            Self::WitnessUtxoMismatch { input } => write!(
                f,
                "input {input} has an output record that differs from the previous transaction"
            ),
            Self::NoWalletKeyHint { input } => {
                write!(f, "input {input} does not say which wallet address it spends")
            }
            Self::HintKeyMismatch { input } => write!(
                f,
                "input {input} names a wallet key that is not the key at that address"
            ),
            Self::HintsDisagree { input } => {
                write!(f, "the key hints of input {input} name different addresses")
            }
            Self::ScriptNotInWallet { input } => {
                write!(f, "input {input} does not spend an address of this wallet")
            }
            Self::WitnessScriptMismatch { input } => write!(
                f,
                "the witness script of input {input} is not the wallet's script"
            ),
            Self::ForeignSignature { input } => {
                write!(f, "input {input} carries a signature from an unknown key")
            }
            Self::SignatureSighashType { input } => write!(
                f,
                "input {input} carries a signature that is not SIGHASH_ALL"
            ),
            Self::BadSignature { input } => {
                write!(f, "input {input} carries a signature that does not verify")
            }
            Self::AlreadySigned => write!(f, "this device has already signed every input"),
            Self::InputAmountTooLarge { input, amount } => {
                write!(f, "input {input} has an impossible amount ({amount} sat)")
            }
            Self::InputTotalOverflow => write!(f, "the input amounts overflow"),
            Self::OutputHintKeyMismatch { output } => write!(
                f,
                "output {output} names a wallet key that is not the key at that address"
            ),
            Self::OutputHintsDisagree { output } => {
                write!(f, "the key hints of output {output} name different addresses")
            }
            Self::OutputScriptKind { output } => write!(
                f,
                "output {output} is not a standard payment script the signer accepts"
            ),
            Self::OutputAmountZero { output } => write!(f, "output {output} has no amount"),
            Self::OutputAmountTooLarge { output, amount } => {
                write!(f, "output {output} has an impossible amount ({amount} sat)")
            }
            Self::OutputTotalOverflow => write!(f, "the output amounts overflow"),
            Self::OutputsExceedInputs { outputs, inputs } => write!(
                f,
                "the outputs ({outputs} sat) are more than the inputs ({inputs} sat)"
            ),
            Self::ChangeIndexTooFar {
                output,
                index,
                limit,
            } => write!(
                f,
                "output {output} sends change to index {index}; the signer allows indexes below {limit}"
            ),
            Self::OverCeiling {
                ceiling,
                amount,
                limit,
            } => write!(
                f,
                "{amount} sat is above the {} of {limit} sat",
                ceiling.name()
            ),
            Self::TotalsOverflow => write!(f, "the spending totals overflow"),
            Self::SpendLogFull => write!(f, "the spend log is full; wait for old entries to age out"),
            Self::FeeRateTooHigh { fee, vsize, limit } => write!(
                f,
                "the fee of {fee} sat for {vsize} vB is above {limit} sat/vB"
            ),
            Self::FeeTooHigh { fee, limit } => {
                write!(f, "the fee of {fee} sat is above the limit of {limit} sat")
            }
            Self::WeightTooHigh { weight, limit } => write!(
                f,
                "the signed transaction weighs {weight} units; the limit is {limit}"
            ),
            Self::ReviewOutdated { output } => write!(
                f,
                "output {output} is no longer what the review showed; review again"
            ),
            Self::ReviewForOtherWallet => write!(f, "the review was made for another wallet"),
            Self::WrongKey => write!(f, "the signing key is not this device's key of the wallet"),
            Self::Internal(what) => write!(f, "internal error: {what}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Why [`SignerState::sign`](crate::SignerState::sign) returned no
/// signature: a rule refused, or the new state could not be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError<E> {
    /// A rule failed. Nothing was stored or signed.
    Refused(Refusal),
    /// The caller's store failed. Nothing was signed and the state is
    /// unchanged.
    Persist(E),
}

impl<E> From<Refusal> for SignError<E> {
    fn from(r: Refusal) -> Self {
        Self::Refused(r)
    }
}

impl<E: fmt::Display> fmt::Display for SignError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Persist(e) => write!(f, "could not store the new state: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for SignError<E> {}

/// Why an owner call on the Key app's own screen was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeRefusal {
    /// SR-30: the script is not P2WPKH, P2WSH, P2TR, P2PKH or P2SH.
    ScriptKind,
    /// SR-30: the label is empty, longer than 64 characters (or 255 bytes),
    /// or holds a control character.
    InvalidLabel,
    /// SR-30: the allowlist already holds 64 entries.
    AllowlistFull,
    /// SR-30: the script is already on the allowlist.
    AlreadyEnrolled,
    /// SR-30: the script is not on the allowlist.
    NotEnrolled,
    /// SR-37: a ceiling is above 21,000,000 BTC.
    CeilingAboveMoney,
    /// SR-40: the fee-rate ceiling is 0 or above the fixed bound.
    FeeRateOutOfBounds,
    /// SR-41: the absolute fee ceiling is 0 or above the fixed bound.
    FeeOutOfBounds,
    /// SR-34: the new change mark is not higher than the current one.
    MarkNotHigher,
    /// SR-34: the new change mark is above 2^31.
    MarkTooHigh,
    /// SR-70: the new receive mark is not higher than the current one.
    ReceiveMarkNotHigher,
    /// SR-70: the new receive mark is above 2^31.
    ReceiveMarkTooHigh,
}

impl ChangeRefusal {
    /// The ID of the rule in `docs/SIGNER-RULES.md` that was broken.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::ScriptKind
            | Self::InvalidLabel
            | Self::AllowlistFull
            | Self::AlreadyEnrolled
            | Self::NotEnrolled => "SR-30",
            Self::CeilingAboveMoney => "SR-37",
            Self::FeeRateOutOfBounds => "SR-40",
            Self::FeeOutOfBounds => "SR-41",
            Self::MarkNotHigher | Self::MarkTooHigh => "SR-34",
            Self::ReceiveMarkNotHigher | Self::ReceiveMarkTooHigh => "SR-70",
        }
    }
}

impl fmt::Display for ChangeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::ScriptKind => write!(f, "the destination is not a standard payment script"),
            Self::InvalidLabel => write!(
                f,
                "the label must be 1 to 64 characters with no control characters"
            ),
            Self::AllowlistFull => write!(f, "the allowlist is full"),
            Self::AlreadyEnrolled => write!(f, "the destination is already on the allowlist"),
            Self::NotEnrolled => write!(f, "the destination is not on the allowlist"),
            Self::CeilingAboveMoney => write!(f, "a ceiling is above all bitcoin there can be"),
            Self::FeeRateOutOfBounds => {
                write!(f, "the fee-rate ceiling is outside its fixed bounds")
            }
            Self::FeeOutOfBounds => write!(f, "the fee ceiling is outside its fixed bounds"),
            Self::MarkNotHigher => write!(f, "the change mark can only go up"),
            Self::MarkTooHigh => write!(f, "the change mark is too high"),
            Self::ReceiveMarkNotHigher => write!(f, "the receive mark can only go up"),
            Self::ReceiveMarkTooHigh => write!(f, "the receive mark is too high"),
        }
    }
}

impl std::error::Error for ChangeRefusal {}

/// Why an owner call did not change the state: it was refused, or the new
/// state could not be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeError<E> {
    /// A rule failed. Nothing was stored and the state is unchanged.
    Refused(ChangeRefusal),
    /// The caller's store failed. The state is unchanged.
    Persist(E),
}

impl<E> From<ChangeRefusal> for ChangeError<E> {
    fn from(r: ChangeRefusal) -> Self {
        Self::Refused(r)
    }
}

impl<E: fmt::Display> fmt::Display for ChangeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Persist(e) => write!(f, "could not store the new state: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for ChangeError<E> {}

/// Why a wallet could not be registered (SR-60 to SR-62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// SR-60: the description is not a valid wallet description.
    Wallet(WalletError),
    /// SR-60: the network is not mainnet, testnet, signet or regtest.
    UnsupportedNetwork,
    /// SR-61: no key slot holds this device's account public key in full.
    OwnKeyNotInWallet,
    /// SR-62: no other key slot holds the key given for key 3 in full.
    Key3NotInWallet,
}

impl RegisterError {
    /// The ID of the rule in `docs/SIGNER-RULES.md` that was broken.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Wallet(_) | Self::UnsupportedNetwork => "SR-60",
            Self::OwnKeyNotInWallet => "SR-61",
            Self::Key3NotInWallet => "SR-62",
        }
    }
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Wallet(e) => write!(f, "{e}"),
            Self::UnsupportedNetwork => write!(f, "the network is not supported"),
            Self::OwnKeyNotInWallet => write!(f, "this device's key is not in the wallet"),
            Self::Key3NotInWallet => write!(f, "key 3 is not in the wallet"),
        }
    }
}

impl std::error::Error for RegisterError {}
