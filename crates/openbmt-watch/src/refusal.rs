//! Why the watcher refused a piece of evidence.
//!
//! Every refusal names the rule of `docs/WATCHER-RULES.md` that it enforces
//! ([`Refusal::rule`]). The `Display` text starts with the rule ID. A
//! refusal means nothing was believed and nothing was recorded.

use std::fmt;

use openbmt_core::wire::DecodeError;

/// A reason the watcher will not believe a proof, a block or a sighting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// WR-01: the block is not on the best chain now.
    NotOnChain,
    /// WR-03: the header's hash does not meet the target its own bits state.
    HeaderProofOfWork,
    /// WR-04: the header store cannot be started for this network.
    NetworkNotSupported,
    /// WR-05: the header's parent is not held.
    UnknownParent,
    /// WR-05: the header's bits are not what the difficulty rules require
    /// on its branch.
    WrongBits {
        /// The bits the rules require.
        expected: u32,
        /// The bits the header states.
        found: u32,
    },
    /// WR-05: the header's hash does not meet its target.
    ChainProofOfWork,
    /// WR-05: the header's time is not after the median time of the
    /// headers before it.
    TimeNotAfterMedian {
        /// The median time of the previous headers on the branch.
        median: u32,
        /// The header's time.
        time: u32,
    },
    /// WR-05: a header the check needs on the branch is not held.
    AncestorMissing,
    /// WR-05: the chain would pass the largest height.
    HeightOverflow,
    /// WR-06: the header's time is more than 2 hours ahead of the clock.
    /// Nothing was stored; the header may be offered again later.
    TimeTooFarAhead {
        /// The header's time.
        time: u32,
    },
    /// WR-07: the header's parent is too far below the tip for the header
    /// to be held.
    SideHeaderTooDeep,
    /// WR-07: the store already holds the most headers off the best chain.
    SideHeadersFull,
    /// WR-08: the best chain has reached the stored-form header limit.
    HeaderStoreFull,
    /// WR-07: the total work does not fit in 256 bits.
    WorkOverflow,
    /// WR-08: the stored bytes are not a header store and nothing else.
    StoreMalformed(DecodeError),
    /// WR-08: the stored form is for another network.
    StoreWrongNetwork,
    /// WR-08: the stored form starts from another block.
    StoreWrongStart,
    /// WR-08: a stored header does not extend the tip.
    StoreNotChained,
    /// WR-08: a stored header was refused.
    StoreHeaderRefused {
        /// Position of the header after the start block, from 0.
        index: u32,
        /// Why it was refused.
        why: Box<Refusal>,
    },
    /// WR-10: the merkle proof is larger than the limit.
    ProofTooLarge {
        /// Size of the proof in bytes.
        len: usize,
    },
    /// WR-10: the proof is not a header and a partial merkle tree with
    /// nothing after them.
    ProofMalformed,
    /// WR-10: the coinbase transaction is larger than the limit.
    CoinbaseTooLarge {
        /// Size of the transaction in bytes.
        len: usize,
    },
    /// WR-10: the payment transaction is larger than the limit.
    PaymentTooLarge {
        /// Size of the transaction in bytes.
        len: usize,
    },
    /// WR-12: the tree states fewer than 2 or more than 16,666
    /// transactions.
    TreeTransactionCount {
        /// The count the proof states.
        count: u32,
    },
    /// WR-12: the tree has more hashes than transactions.
    TreeTooManyHashes,
    /// WR-12: the tree runs out of flag bits or hashes, or leaves a flag
    /// bit or a hash unused.
    TreeBitsAndHashes,
    /// WR-12: a node of the tree has two equal children.
    TreeEqualChildren,
    /// WR-12: the spare bits after the last flag bit are not all zero.
    TreePadding,
    /// WR-12: the root the tree gives is not the header's merkle root.
    TreeRootMismatch,
    /// WR-13: the tree does not prove exactly two transactions.
    ProofCoversWrongCount {
        /// How many transactions the tree proves.
        found: usize,
    },
    /// WR-13: the first proven transaction is not at position 0.
    CoinbaseNotFirst {
        /// The position of the first proven transaction.
        position: u32,
    },
    /// WR-14: the coinbase bytes are not a transaction.
    CoinbaseMalformed,
    /// WR-14: the coinbase handed in is not the transaction proven at
    /// position 0.
    CoinbaseIdMismatch,
    /// WR-14: the transaction proven at position 0 is not a coinbase.
    NotACoinbase,
    /// WR-15: the payment bytes are not a transaction.
    PaymentMalformed,
    /// WR-15: the payment handed in is not the transaction proven at the
    /// later position.
    PaymentIdMismatch,
    /// WR-15: the payment is a coinbase.
    PaymentIsCoinbase,
    /// WR-16: the coinbase is 64 bytes long without its witness.
    CoinbaseSixtyFourBytes,
    /// WR-16: the payment is 64 bytes long without its witness.
    PaymentSixtyFourBytes,
    /// WR-20: the block is larger than the limit.
    BlockTooLarge {
        /// Size of the block in bytes.
        len: usize,
    },
    /// WR-20: the bytes are not a block with nothing after it.
    BlockMalformed,
    /// WR-21: the merkle root of the block's transactions is not the
    /// header's.
    BlockMerkleRoot,
    /// WR-21: the block has no transactions, or its first is not a
    /// coinbase.
    BlockNoCoinbase,
    /// WR-21: a transaction after the first is a coinbase.
    BlockExtraCoinbase {
        /// Position of that transaction.
        position: u32,
    },
    /// WR-21: two transactions of the block have the same ID.
    BlockDuplicateTransaction {
        /// Position of the second one.
        position: u32,
    },
    /// WR-22: a transaction that pays the sale is 64 bytes long without its
    /// witness.
    BlockPaymentSixtyFourBytes {
        /// Position of that transaction.
        position: u32,
    },
    /// WR-30: the transaction pays the sale nothing and conflicts with
    /// nothing the sale knows.
    PaysNothing,
    /// WR-34: the sale already holds the most transactions.
    TooManyTransactions,
    /// WR-34: the transaction is larger than the limit.
    TransactionTooLarge {
        /// Size of the transaction in bytes.
        len: usize,
    },
    /// WR-40: the bytes are not a transaction.
    NotATransaction,
}

impl Refusal {
    /// The rule of `docs/WATCHER-RULES.md` this refusal enforces.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NotOnChain => "WR-01",
            Self::HeaderProofOfWork => "WR-03",
            Self::NetworkNotSupported => "WR-04",
            Self::UnknownParent
            | Self::WrongBits { .. }
            | Self::ChainProofOfWork
            | Self::TimeNotAfterMedian { .. }
            | Self::AncestorMissing
            | Self::HeightOverflow => "WR-05",
            Self::TimeTooFarAhead { .. } => "WR-06",
            Self::SideHeaderTooDeep | Self::SideHeadersFull | Self::WorkOverflow => "WR-07",
            Self::HeaderStoreFull => "WR-08",
            Self::StoreMalformed(_)
            | Self::StoreWrongNetwork
            | Self::StoreWrongStart
            | Self::StoreNotChained
            | Self::StoreHeaderRefused { .. } => "WR-08",
            Self::ProofTooLarge { .. }
            | Self::ProofMalformed
            | Self::CoinbaseTooLarge { .. }
            | Self::PaymentTooLarge { .. } => "WR-10",
            Self::TreeTransactionCount { .. }
            | Self::TreeTooManyHashes
            | Self::TreeBitsAndHashes
            | Self::TreeEqualChildren
            | Self::TreePadding
            | Self::TreeRootMismatch => "WR-12",
            Self::ProofCoversWrongCount { .. } | Self::CoinbaseNotFirst { .. } => "WR-13",
            Self::CoinbaseMalformed | Self::CoinbaseIdMismatch | Self::NotACoinbase => "WR-14",
            Self::PaymentMalformed | Self::PaymentIdMismatch | Self::PaymentIsCoinbase => "WR-15",
            Self::CoinbaseSixtyFourBytes | Self::PaymentSixtyFourBytes => "WR-16",
            Self::BlockTooLarge { .. } | Self::BlockMalformed => "WR-20",
            Self::BlockMerkleRoot
            | Self::BlockNoCoinbase
            | Self::BlockExtraCoinbase { .. }
            | Self::BlockDuplicateTransaction { .. } => "WR-21",
            Self::BlockPaymentSixtyFourBytes { .. } => "WR-22",
            Self::PaysNothing => "WR-30",
            Self::TooManyTransactions | Self::TransactionTooLarge { .. } => "WR-34",
            Self::NotATransaction => "WR-40",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::NotOnChain => write!(f, "the block is not on the best chain"),
            Self::HeaderProofOfWork => {
                write!(f, "the header does not meet the target its bits state")
            }
            Self::NetworkNotSupported => {
                write!(f, "the header store does not start on this network")
            }
            Self::UnknownParent => write!(f, "the header's parent is not held"),
            Self::WrongBits { expected, found } => write!(
                f,
                "the header states bits {found:#010x}, the rules require {expected:#010x}"
            ),
            Self::ChainProofOfWork => write!(f, "the header does not meet its target"),
            Self::TimeNotAfterMedian { median, time } => write!(
                f,
                "the header's time {time} is not after the median time {median}"
            ),
            Self::AncestorMissing => write!(f, "a header the check needs is not held"),
            Self::HeightOverflow => write!(f, "the chain would pass the largest height"),
            Self::TimeTooFarAhead { time } => write!(
                f,
                "the header's time {time} is more than 2 hours ahead of the clock; try again later"
            ),
            Self::SideHeaderTooDeep => write!(
                f,
                "the header's parent is too far below the tip for it to be held"
            ),
            Self::SideHeadersFull => {
                write!(
                    f,
                    "the store already holds the most headers off the best chain"
                )
            }
            Self::HeaderStoreFull => write!(
                f,
                "the best chain reached the stored-form limit of 2,000,000 headers after its start block"
            ),
            Self::WorkOverflow => write!(f, "the total work does not fit in 256 bits"),
            Self::StoreMalformed(why) => {
                write!(f, "the stored bytes are not a header store: {why}")
            }
            Self::StoreWrongNetwork => write!(f, "the stored form is for another network"),
            Self::StoreWrongStart => write!(f, "the stored form starts from another block"),
            Self::StoreNotChained => write!(f, "a stored header does not extend the tip"),
            Self::StoreHeaderRefused { index, why } => {
                write!(f, "the stored header {index} was refused: {why}")
            }
            Self::ProofTooLarge { len } => write!(f, "the proof is {len} bytes, over the limit"),
            Self::ProofMalformed => write!(
                f,
                "the proof is not a header and a partial merkle tree and nothing else"
            ),
            Self::CoinbaseTooLarge { len } => {
                write!(f, "the coinbase is {len} bytes, over the limit")
            }
            Self::PaymentTooLarge { len } => {
                write!(f, "the payment is {len} bytes, over the limit")
            }
            Self::TreeTransactionCount { count } => {
                write!(
                    f,
                    "the tree states {count} transactions, outside the limits"
                )
            }
            Self::TreeTooManyHashes => write!(f, "the tree has more hashes than transactions"),
            Self::TreeBitsAndHashes => {
                write!(f, "the tree does not use exactly its flag bits and hashes")
            }
            Self::TreeEqualChildren => write!(f, "a node of the tree has two equal children"),
            Self::TreePadding => write!(f, "the spare flag bits of the tree are not zero"),
            Self::TreeRootMismatch => {
                write!(f, "the tree does not give the header's merkle root")
            }
            Self::ProofCoversWrongCount { found } => {
                write!(f, "the tree proves {found} transactions, not exactly 2")
            }
            Self::CoinbaseNotFirst { position } => write!(
                f,
                "the first proven transaction is at position {position}, not 0"
            ),
            Self::CoinbaseMalformed => write!(f, "the coinbase bytes are not a transaction"),
            Self::CoinbaseIdMismatch => {
                write!(
                    f,
                    "the coinbase is not the transaction proven at position 0"
                )
            }
            Self::NotACoinbase => {
                write!(f, "the transaction proven at position 0 is not a coinbase")
            }
            Self::PaymentMalformed => write!(f, "the payment bytes are not a transaction"),
            Self::PaymentIdMismatch => {
                write!(f, "the payment is not the transaction proven in the tree")
            }
            Self::PaymentIsCoinbase => write!(f, "the payment is a coinbase"),
            Self::CoinbaseSixtyFourBytes => {
                write!(f, "the coinbase is 64 bytes long without its witness")
            }
            Self::PaymentSixtyFourBytes => {
                write!(f, "the payment is 64 bytes long without its witness")
            }
            Self::BlockTooLarge { len } => write!(f, "the block is {len} bytes, over the limit"),
            Self::BlockMalformed => write!(f, "the bytes are not a block and nothing else"),
            Self::BlockMerkleRoot => {
                write!(f, "the transactions do not give the header's merkle root")
            }
            Self::BlockNoCoinbase => write!(f, "the first transaction is not a coinbase"),
            Self::BlockExtraCoinbase { position } => {
                write!(f, "the transaction at position {position} is a coinbase")
            }
            Self::BlockDuplicateTransaction { position } => write!(
                f,
                "the transaction at position {position} repeats an earlier one"
            ),
            Self::BlockPaymentSixtyFourBytes { position } => write!(
                f,
                "the payment at position {position} is 64 bytes long without its witness"
            ),
            Self::PaysNothing => write!(
                f,
                "the transaction pays the sale nothing and conflicts with nothing known"
            ),
            Self::TooManyTransactions => {
                write!(
                    f,
                    "the sale already holds the most transactions; needs a person"
                )
            }
            Self::TransactionTooLarge { len } => {
                write!(
                    f,
                    "the transaction is {len} bytes, over the limit; needs a person"
                )
            }
            Self::NotATransaction => write!(f, "the bytes are not a transaction"),
        }
    }
}

impl std::error::Error for Refusal {}
