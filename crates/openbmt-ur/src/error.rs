//! The refusals. Each text starts with the rule's code (`docs/UR-RULES.md`).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrError {
    /// UR-01: a type that is not 1 to 64 characters of `[a-z0-9-]`.
    Type(&'static str),
    /// UR-02: a message that is empty or over 1 MiB.
    MessageSize,
    /// UR-03: a sequence length of 0 or over 10,000.
    SeqLen,
    /// UR-04: a fragment length outside 10 to 2000.
    FragmentLen(&'static str),
    /// UR-05: text that is not minimal Bytewords.
    Bytewords(&'static str),
    /// UR-06: Bytewords whose CRC-32 does not match.
    BytewordsChecksum,
    /// UR-07: text that is not the shape of a UR.
    Shape(&'static str),
    /// UR-08: a multi-part body that is not the strict five-item array.
    Envelope(&'static str),
    /// UR-09: a part's fields that cannot belong to any message.
    PartFields(&'static str),
    /// UR-10: a part that disagrees with the first accepted part.
    Disagrees(&'static str),
    /// UR-11: more mixed parts held than twice the sequence length.
    MixedFull,
    /// UR-12: the joined message fails its checksum or its padding.
    Message(&'static str),
    /// UR-13: a CBOR byte string that is not strict.
    Cbor(&'static str),
    /// UR-14: a type or content that is not a PSBT.
    Psbt(&'static str),
    /// UR-16: text longer than any UR within the bounds.
    TextTooLong,
    /// UR-32: an output descriptor that cannot be written.
    Output(&'static str),
}

impl UrError {
    /// The rule's code.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Type(_) => "UR-01",
            Self::MessageSize => "UR-02",
            Self::SeqLen => "UR-03",
            Self::FragmentLen(_) => "UR-04",
            Self::Bytewords(_) => "UR-05",
            Self::BytewordsChecksum => "UR-06",
            Self::Shape(_) => "UR-07",
            Self::Envelope(_) => "UR-08",
            Self::PartFields(_) => "UR-09",
            Self::Disagrees(_) => "UR-10",
            Self::MixedFull => "UR-11",
            Self::Message(_) => "UR-12",
            Self::Cbor(_) => "UR-13",
            Self::Psbt(_) => "UR-14",
            Self::TextTooLong => "UR-16",
            Self::Output(_) => "UR-32",
        }
    }
}

impl fmt::Display for UrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.rule();
        match self {
            Self::Type(why)
            | Self::FragmentLen(why)
            | Self::Bytewords(why)
            | Self::Shape(why)
            | Self::Envelope(why)
            | Self::PartFields(why)
            | Self::Disagrees(why)
            | Self::Message(why)
            | Self::Cbor(why)
            | Self::Psbt(why)
            | Self::Output(why) => write!(f, "{code}: {why}"),
            Self::MessageSize => write!(f, "{code}: the message is empty or over 1 MiB"),
            Self::SeqLen => write!(f, "{code}: the sequence length is 0 or over 10000"),
            Self::BytewordsChecksum => write!(f, "{code}: the Bytewords checksum does not match"),
            Self::MixedFull => write!(f, "{code}: too many mixed parts held"),
            Self::TextTooLong => write!(f, "{code}: the text is longer than any UR can be"),
        }
    }
}

impl std::error::Error for UrError {}
