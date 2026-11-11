//! The types of the ops for a hardware signer as key 2
//! (`docs/SESSION-RULES.md`, AS-90 to AS-99): the spends built for it, the
//! file it is given, what was taken from it, the registration of the wallet
//! on it and the record of the first address check. They are written like
//! those of [`views`](crate::views).

use openbmt_owner::hardware::{
    FirstAddressRecord, FirstAddressState, Registration, SignerFile, SignerTaken,
};

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::views::{BuiltView, SendRow, WordsView};

/// One address and amount, as the signer's screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerLineView {
    /// The address.
    pub address: String,
    /// The amount in satoshis.
    pub sats: u64,
}

wire_record!(SignerLineView { address, sats });

/// The session built for the Key app, and the spends for the signer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerBuiltView {
    /// What the Key app takes (it holds no spend).
    pub built: BuiltView,
    /// The session of the spends, 32 bytes, if there is any spend. It is
    /// never carried to the Key app.
    pub signer_session: Option<Vec<u8>>,
}

wire_record!(SignerBuiltView {
    built,
    signer_session
});

/// The file for the hardware signer and what to compare on its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerFileView {
    /// The PSBT, binary, signed by key 1.
    pub psbt: Vec<u8>,
    /// The transaction's ID, 32 bytes.
    pub txid: Vec<u8>,
    /// A name for the file, `<8 characters>.psbt`.
    pub file_name: String,
    /// The payments, in output order.
    pub payments: Vec<SignerLineView>,
    /// The change back to the wallet.
    pub change: Option<SignerLineView>,
    /// The fee in satoshis.
    pub fee: u64,
    /// What to compare on the signer's screen.
    pub words: WordsView,
}

wire_record!(SignerFileView {
    psbt,
    txid,
    file_name,
    payments,
    change,
    fee,
    words
});

fn line(l: &openbmt_owner::hardware::SummaryLine) -> SignerLineView {
    SignerLineView {
        address: l.address.clone(),
        sats: l.sats,
    }
}

pub(crate) fn file_view(file: SignerFile) -> SignerFileView {
    use bitcoin::hashes::Hash;
    SignerFileView {
        txid: file.txid.to_byte_array().to_vec(),
        psbt: file.psbt,
        file_name: file.file_name,
        payments: file.summary.payments.iter().map(line).collect(),
        change: file.summary.change.as_ref().map(line),
        fee: file.summary.fee,
        words: WordsView(openbmt_owner::hardware::compare_words()),
    }
}

/// What taking the signer's answer did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerTakenView {
    /// The finished transaction's number in the outbox.
    pub id: u32,
    /// Its ID, 32 bytes.
    pub txid: Vec<u8>,
    /// Its fee in satoshis.
    pub fee: u64,
    /// The payments the node follows now.
    pub sends: Vec<SendRow>,
}

wire_record!(SignerTakenView {
    id,
    txid,
    fee,
    sends
});

pub(crate) fn taken_view(t: &SignerTaken, sends: Vec<SendRow>) -> SignerTakenView {
    use bitcoin::hashes::Hash;
    SignerTakenView {
        id: t.id,
        txid: t.txid.to_byte_array().to_vec(),
        fee: t.fee,
        sends,
    }
}

/// The wallet in the forms a signer takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationView {
    /// The name in the text file.
    pub name: String,
    /// The multisig text file.
    pub text_file: String,
    /// The descriptor with both chains.
    pub descriptor_both: String,
    /// The descriptor of the receive addresses, the form of the UR.
    pub descriptor_receive: String,
    /// The descriptor of the change addresses.
    pub descriptor_change: String,
    /// The CBOR of the `crypto-output`.
    pub output_cbor: Vec<u8>,
    /// Receive address 0, to compare with the signer's address explorer.
    pub first_address: String,
    /// Whether the text file fits one plain-text code.
    pub text_fits_plain_qr: bool,
    /// Whether the receive descriptor fits one plain-text code.
    pub descriptor_fits_plain_qr: bool,
}

wire_record!(RegistrationView {
    name,
    text_file,
    descriptor_both,
    descriptor_receive,
    descriptor_change,
    output_cbor,
    first_address,
    text_fits_plain_qr,
    descriptor_fits_plain_qr
});

pub(crate) fn registration_view(r: Registration, first_address: String) -> RegistrationView {
    RegistrationView {
        text_fits_plain_qr: r.text_fits_plain_qr(),
        descriptor_fits_plain_qr: r.descriptor_fits_plain_qr(),
        name: r.name,
        text_file: r.text_file,
        descriptor_both: r.descriptor_both,
        descriptor_receive: r.descriptor_receive,
        descriptor_change: r.descriptor_change,
        output_cbor: r.output_cbor,
        first_address,
    }
}

/// What was done about the first address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstAddressKind {
    /// Compared with the signer's and it matched.
    Checked,
    /// Left for later.
    Skipped,
}

wire_unit_enum!(FirstAddressKind {
    Checked = 1,
    Skipped = 2
});

impl From<FirstAddressKind> for FirstAddressState {
    fn from(k: FirstAddressKind) -> Self {
        match k {
            FirstAddressKind::Checked => Self::Checked,
            FirstAddressKind::Skipped => Self::Skipped,
        }
    }
}

/// The arguments of `first_address_record`: what was done and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstAddressArgs {
    /// What was done.
    pub kind: FirstAddressKind,
    /// The time, in seconds.
    pub at: u64,
}

wire_record_get!(FirstAddressArgs { kind, at });
wire_record!(FirstAddressArgs { kind, at });

/// The record read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstAddressView {
    /// What was done.
    pub kind: FirstAddressKind,
    /// The time, in seconds.
    pub at: u64,
}

wire_record!(FirstAddressView { kind, at });

pub(crate) fn record_view(r: &FirstAddressRecord) -> FirstAddressView {
    FirstAddressView {
        kind: match r.state {
            FirstAddressState::Checked => FirstAddressKind::Checked,
            FirstAddressState::Skipped => FirstAddressKind::Skipped,
        },
        at: r.at,
    }
}

/// The arguments of `signer_export`: the session of the spends and the
/// item in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerExportArgs {
    /// The time, in seconds.
    pub now: u64,
    /// The session's ID, 32 bytes.
    pub session: Vec<u8>,
    /// The item's number in the session.
    pub number: u32,
}

wire_record_get!(SignerExportArgs {
    now,
    session,
    number
});
wire_record!(SignerExportArgs {
    now,
    session,
    number
});

/// The arguments of `signer_take`: the item and what the signer returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerTakeArgs {
    /// The time, in seconds.
    pub now: u64,
    /// The session's ID, 32 bytes.
    pub session: Vec<u8>,
    /// The item's number in the session.
    pub number: u32,
    /// What was read from the signer's codes or file.
    pub returned: Vec<u8>,
}

wire_record_get!(SignerTakeArgs {
    now,
    session,
    number,
    returned
});
wire_record!(SignerTakeArgs {
    now,
    session,
    number,
    returned
});
