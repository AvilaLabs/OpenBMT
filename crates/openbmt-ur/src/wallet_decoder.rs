//! A bounded scan of a wallet description (UR-39 and UR-40).

use crate::decoder::{parse, Decoder, Progress};
use crate::fountain::decode_part;
use crate::wallet::{ur_to_wallet_text, WalletRefusal, WalletText, MAX_WALLET_CBOR};

/// The most fragments in a wallet scan (UR-39). This also bounds the
/// transport's mixed-row index storage, independently of the message size.
pub const MAX_WALLET_SEQ_LEN: usize = 512;
/// The most calls to receive in one scan, including repeats and refusals
/// (UR-40). A stream that makes no progress cannot keep a scan open forever.
pub const MAX_WALLET_PARTS: usize = 4096;
/// A single-part wallet message, its Bytewords checksum and URI overhead.
/// Multipart fragments are smaller than this (UR-39).
pub const MAX_WALLET_TEXT: usize = 2 * (MAX_WALLET_CBOR + 64) + 128;

/// Reads one wallet's animated codes, with explicit end-of-input and work
/// bounds. This is separate from the general-purpose [`Decoder`]: a wallet
/// scan has smaller limits and validates the completed descriptor.
#[derive(Default)]
pub struct WalletDecoder {
    transport: Decoder,
    attempts: usize,
    result: Option<WalletText>,
    failed: Option<WalletRefusal>,
}

impl WalletDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read a code. A malformed part leaves transport state unchanged;
    /// every call still consumes one of the scan's allowed attempts.
    /// A completed but invalid payload is terminal until a new reader is
    /// made, so it can never be exposed as a wallet.
    pub fn receive(&mut self, text: &str) -> Result<Progress, WalletRefusal> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.attempts >= MAX_WALLET_PARTS {
            return Err(WalletRefusal::SequenceLimit);
        }
        self.attempts += 1;
        if text.len() > MAX_WALLET_TEXT {
            return Err(WalletRefusal::TooLarge);
        }
        let part = parse(text).map_err(WalletRefusal::Transport)?;
        if !matches!(
            part.ur_type.as_str(),
            "crypto-output" | "output" | "output-descriptor" | "crypto-account" | "bytes"
        ) {
            return Err(WalletRefusal::Kind);
        }
        if part.seq.is_some() {
            let fields = decode_part(&part.bytes).map_err(WalletRefusal::Transport)?;
            if fields.message_len > MAX_WALLET_CBOR as u64
                || fields.seq_len > MAX_WALLET_SEQ_LEN as u64
            {
                return Err(WalletRefusal::TooLarge);
            }
        } else if part.bytes.len() > MAX_WALLET_CBOR {
            return Err(WalletRefusal::TooLarge);
        }
        let progress = self
            .transport
            .receive_parsed(part)
            .map_err(WalletRefusal::Transport)?;
        if progress.complete && self.result.is_none() {
            let (kind, message) = self.transport.result().ok_or(WalletRefusal::Incomplete)?;
            match ur_to_wallet_text(&kind, &message) {
                Ok(wallet) => self.result = Some(wallet),
                Err(error) => {
                    self.failed = Some(error.clone());
                    return Err(error);
                }
            }
        }
        Ok(progress)
    }

    /// A validated wallet only after all fragments and payload checks pass.
    pub fn result(&self) -> Option<&WalletText> {
        self.result.as_ref()
    }

    /// End a finite scan. Missing or dependent fragments are an explicit
    /// refusal, never a partial descriptor. The library has no clock; a
    /// caller ends a scan on its own timeout or cancellation.
    pub fn finish(&self) -> Result<WalletText, WalletRefusal> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        self.result.clone().ok_or(WalletRefusal::Incomplete)
    }
}
