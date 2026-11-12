//! The types of the ops for the spends of a shop with its own wallet
//! (`docs/SESSION-RULES.md` section 6, AS-100 to AS-107): the spends built,
//! the sessions that wait, the file for the shop's wallet and what was taken
//! from it. They are written like those of
//! [`signer_views`](crate::signer_views).

use bitcoin::hashes::Hash;
use openbmt_owner::hardware::{SignerSummary, SummaryLine};
use openbmt_owner::own_spend::{OwnBuilt, OwnFile, OwnItem, OwnTaken};

use crate::codec::{wire_record, wire_record_get};
use crate::signer_views::SignerLineView;
use crate::views::{SendRow, SkippedView, WordsView};

/// One transaction of an own-wallet session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSpendItemView {
    /// The item's number in its session.
    pub number: u32,
    /// The transaction's ID, 32 bytes.
    pub txid: Vec<u8>,
    /// The refunds it pays.
    pub refunds: Vec<u32>,
    /// The cash-out it pays, if it is one.
    pub cash_out: Option<u32>,
    /// Whether the file was given to the wallet.
    pub filed: bool,
    /// The payments, in output order.
    pub payments: Vec<SignerLineView>,
    /// The change back to the wallet.
    pub change: Option<SignerLineView>,
    /// The fee in satoshis.
    pub fee: u64,
}

wire_record!(OwnSpendItemView {
    number,
    txid,
    refunds,
    cash_out,
    filed,
    payments,
    change,
    fee
});

fn line(l: &SummaryLine) -> SignerLineView {
    SignerLineView {
        address: l.address.clone(),
        sats: l.sats,
    }
}

fn lines(s: &SignerSummary) -> (Vec<SignerLineView>, Option<SignerLineView>, u64) {
    (
        s.payments.iter().map(line).collect(),
        s.change.as_ref().map(line),
        s.fee,
    )
}

pub(crate) fn item_view(i: &OwnItem) -> OwnSpendItemView {
    let (payments, change, fee) = lines(&i.summary);
    OwnSpendItemView {
        number: i.number,
        txid: i.txid.to_byte_array().to_vec(),
        refunds: i.refunds.clone(),
        cash_out: i.cash_out,
        filed: i.filed,
        payments,
        change,
        fee,
    }
}

/// What building the spends of the wallet froze.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSpendBuiltView {
    /// The session's ID, 32 bytes, if anything was frozen.
    pub session: Option<Vec<u8>>,
    /// The transactions, in order.
    pub items: Vec<OwnSpendItemView>,
    /// The entries left out.
    pub skipped: Vec<SkippedView>,
}

wire_record!(OwnSpendBuiltView {
    session,
    items,
    skipped
});

pub(crate) fn built_view(
    b: &OwnBuilt,
    words: impl Fn(&openbmt_owner::Skipped) -> WordsView,
) -> OwnSpendBuiltView {
    OwnSpendBuiltView {
        session: b.session.map(|s| s.to_vec()),
        items: b.items.iter().map(item_view).collect(),
        skipped: b
            .skipped
            .iter()
            .map(|s| SkippedView {
                id: s.id,
                reason: s.reason.clone(),
                words: words(s),
            })
            .collect(),
    }
}

/// An open session of the wallet and its transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSpendSessionView {
    /// The session's ID, 32 bytes.
    pub session: Vec<u8>,
    /// The transactions, in order.
    pub items: Vec<OwnSpendItemView>,
}

wire_record!(OwnSpendSessionView { session, items });

/// The file for the shop's wallet and what to compare on its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSpendFileView {
    /// The PSBT, binary.
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
    /// What to compare on the wallet's screen.
    pub words: WordsView,
}

wire_record!(OwnSpendFileView {
    psbt,
    txid,
    file_name,
    payments,
    change,
    fee,
    words
});

pub(crate) fn file_view(f: OwnFile) -> OwnSpendFileView {
    let (payments, change, fee) = lines(&f.summary);
    OwnSpendFileView {
        txid: f.txid.to_byte_array().to_vec(),
        psbt: f.psbt,
        file_name: f.file_name,
        payments,
        change,
        fee,
        words: WordsView(openbmt_owner::own_spend::compare_words()),
    }
}

/// What taking the wallet's answer did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSpendTakenView {
    /// The finished transaction's number in the outbox.
    pub id: u32,
    /// Its ID, 32 bytes.
    pub txid: Vec<u8>,
    /// Its fee in satoshis.
    pub fee: u64,
    /// The payments the node follows now.
    pub sends: Vec<SendRow>,
}

wire_record!(OwnSpendTakenView {
    id,
    txid,
    fee,
    sends
});

pub(crate) fn taken_view(t: &OwnTaken, sends: Vec<SendRow>) -> OwnSpendTakenView {
    OwnSpendTakenView {
        id: t.id,
        txid: t.txid.to_byte_array().to_vec(),
        fee: t.fee,
        sends,
    }
}

/// The arguments of `own_spend_build`: the time, the fee rate in
/// satoshis per virtual byte and the numbers of the queued entries
/// picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSpendBuildArgs {
    /// The time, in seconds.
    pub now: u64,
    /// The fee rate.
    pub fee_rate: u64,
    /// The refunds and cash-outs picked.
    pub picked: Vec<u32>,
}

wire_record_get!(OwnSpendBuildArgs {
    now,
    fee_rate,
    picked
});
wire_record!(OwnSpendBuildArgs {
    now,
    fee_rate,
    picked
});

/// The arguments of `own_spend_take`: the item and what the wallet
/// returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSpendTakeArgs {
    /// The time, in seconds.
    pub now: u64,
    /// The session's ID, 32 bytes.
    pub session: Vec<u8>,
    /// The item's number in the session.
    pub number: u32,
    /// What was read from the wallet's codes or file.
    pub returned: Vec<u8>,
}

wire_record_get!(OwnSpendTakeArgs {
    now,
    session,
    number,
    returned
});
wire_record!(OwnSpendTakeArgs {
    now,
    session,
    number,
    returned
});
