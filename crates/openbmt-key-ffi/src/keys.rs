//! Making the identity and key 2 (`docs/DESIGN.md` §2.1 step 3, §5.4).

use std::sync::Arc;

use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use openbmt_core::entropy::{
    bits_from_coin, bits_from_dice, face_counts, words_from_bits, words_from_dice_hashed, Flip,
    WordCount,
};
use openbmt_core::identity::Identity;
use openbmt_core::wallet::coin_type;

use crate::codec::{bad_tag, wire_record, wire_unit_enum, Define, Get, Reader, Shaped, TypeDef};
use crate::envelope::{wipe_string, Key2, Material, Secret32};
use crate::error::{internal, refused_by, KeyError};
use crate::store::KeyStore;
use crate::words::mnemonic_from_typed;

/// A network the Key app works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyNetwork {
    /// Bitcoin mainnet.
    Mainnet,
    /// Testnet.
    Testnet,
    /// Signet.
    Signet,
    /// Regtest.
    Regtest,
}

impl From<KeyNetwork> for Network {
    fn from(n: KeyNetwork) -> Self {
        match n {
            KeyNetwork::Mainnet => Network::Bitcoin,
            KeyNetwork::Testnet => Network::Testnet,
            KeyNetwork::Signet => Network::Signet,
            KeyNetwork::Regtest => Network::Regtest,
        }
    }
}

/// How many words a skew-corrected key has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyWordCount {
    /// 12 words, 128 bits.
    Twelve,
    /// 24 words, 256 bits.
    TwentyFour,
}

impl From<KeyWordCount> for WordCount {
    fn from(c: KeyWordCount) -> Self {
        match c {
            KeyWordCount::Twelve => WordCount::Twelve,
            KeyWordCount::TwentyFour => WordCount::TwentyFour,
        }
    }
}

/// Where key 2's randomness comes from (`docs/DESIGN.md` §5.4). With every
/// source but `Platform` nothing else is mixed in, so the same throws give
/// the same words in another tool.
#[derive(Debug, Clone)]
pub enum EntropySource {
    /// "Let this device make it": 32 bytes from the platform's random
    /// source, 24 words.
    Platform,
    /// Dice, hashed: at least 99 rolls written as the digits 1 to 6; 24
    /// words from their SHA-256 hash.
    DiceHashed {
        /// The digits, in the order rolled.
        rolls: String,
    },
    /// Coin, skew-corrected (von Neumann).
    CoinFlips {
        /// `true` for heads, in the order flipped.
        heads: Vec<bool>,
        /// 12 or 24 words.
        words: KeyWordCount,
    },
    /// Dice, skew-corrected (von Neumann).
    DiceSkewCorrected {
        /// The digits 1 to 6, in the order rolled.
        rolls: String,
        /// 12 or 24 words.
        words: KeyWordCount,
    },
    /// Twelve or 24 words the owner already has, typed in (BIP39 English,
    /// no passphrase). Each word is trimmed and lower-cased. The words are
    /// not kept and `key_words` returns nothing for this source. The
    /// signer starts with empty history, as with every source.
    TypedWords {
        /// The words, in order.
        words: Vec<String>,
    },
}

/// How often each die face came up in `rolls`, ones first (the hashed
/// method shows this). Wraps `openbmt_core::entropy::face_counts`.
pub fn dice_face_counts(rolls: String) -> Result<Vec<u32>, KeyError> {
    face_counts(&rolls)
        .map(|c| c.to_vec())
        .map_err(|e| refused_by("entropy", e))
}

/// How many skew-corrected bits `heads` gives, and how many more `words`
/// need, for a progress display. Wraps `openbmt_core::entropy`.
pub fn coin_bits(heads: Vec<bool>, words: KeyWordCount) -> BitProgress {
    let flips: Vec<Flip> = heads
        .iter()
        .map(|h| if *h { Flip::Heads } else { Flip::Tails })
        .collect();
    progress(bits_from_coin(&flips).len(), words)
}

/// The same for die rolls.
pub fn dice_bits(rolls: String, words: KeyWordCount) -> Result<BitProgress, KeyError> {
    let bits = bits_from_dice(&rolls).map_err(|e| refused_by("entropy", e))?;
    Ok(progress(bits.len(), words))
}

/// Bits gathered and bits still needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitProgress {
    /// Bits so far.
    pub have: u32,
    /// Bits still needed for the chosen word count; 0 once there are enough.
    pub still_needed: u32,
}

wire_unit_enum!(KeyNetwork {
    Mainnet = 0,
    Testnet = 1,
    Signet = 2,
    Regtest = 3,
});
wire_unit_enum!(KeyWordCount {
    Twelve = 0,
    TwentyFour = 1,
});
wire_record!(BitProgress { have, still_needed });

const SOURCE_PLATFORM: u8 = 0;
const SOURCE_DICE_HASHED: u8 = 1;
const SOURCE_COIN_FLIPS: u8 = 2;
const SOURCE_DICE_SKEW_CORRECTED: u8 = 3;
const SOURCE_TYPED_WORDS: u8 = 4;

impl Shaped for EntropySource {
    fn shape() -> String {
        "EntropySource".to_string()
    }
}

impl Get for EntropySource {
    fn get(r: &mut Reader<'_>) -> Result<Self, KeyError> {
        match r.u8()? {
            SOURCE_PLATFORM => Ok(Self::Platform),
            SOURCE_DICE_HASHED => Ok(Self::DiceHashed {
                rolls: String::get(r)?,
            }),
            SOURCE_COIN_FLIPS => Ok(Self::CoinFlips {
                heads: Vec::get(r)?,
                words: KeyWordCount::get(r)?,
            }),
            SOURCE_DICE_SKEW_CORRECTED => Ok(Self::DiceSkewCorrected {
                rolls: String::get(r)?,
                words: KeyWordCount::get(r)?,
            }),
            SOURCE_TYPED_WORDS => Ok(Self::TypedWords {
                words: Vec::get(r)?,
            }),
            t => Err(bad_tag("EntropySource", t)),
        }
    }
}

impl Define for EntropySource {
    fn define() -> TypeDef {
        TypeDef {
            name: "EntropySource",
            body: "enum: Platform | DiceHashed{rolls: string} \
                   | CoinFlips{heads: list<bool>, words: KeyWordCount} \
                   | DiceSkewCorrected{rolls: string, words: KeyWordCount} \
                   | TypedWords{words: list<string>}"
                .to_string(),
            tags: vec![
                ("Platform", SOURCE_PLATFORM),
                ("DiceHashed", SOURCE_DICE_HASHED),
                ("CoinFlips", SOURCE_COIN_FLIPS),
                ("DiceSkewCorrected", SOURCE_DICE_SKEW_CORRECTED),
                ("TypedWords", SOURCE_TYPED_WORDS),
            ],
        }
    }
}

fn progress(have: usize, words: KeyWordCount) -> BitProgress {
    let need = openbmt_core::entropy::bits_still_needed(have, words.into());
    BitProgress {
        have: u32::try_from(have).unwrap_or(u32::MAX),
        still_needed: u32::try_from(need).unwrap_or(u32::MAX),
    }
}

/// `n` bytes from the app's random source, refused if the count is wrong.
pub(crate) fn random_bytes(store: &Arc<dyn KeyStore>, n: usize) -> Result<Vec<u8>, KeyError> {
    let want = u32::try_from(n).map_err(|_| internal("random: count"))?;
    let bytes = store
        .random(want)
        .map_err(|e| crate::error::store_failed(&e))?;
    if bytes.len() != n {
        return Err(KeyError::Store {
            rule: "random".to_string(),
            text: format!("the platform gave {} random bytes, not {n}", bytes.len()),
        });
    }
    Ok(bytes)
}

/// A fresh identity secret from the platform's random source (device
/// identity keys always use it, `docs/DESIGN.md` §5.4).
pub(crate) fn new_identity_secret(store: &Arc<dyn KeyStore>) -> Result<Secret32, KeyError> {
    for _ in 0..8 {
        let mut bytes = random_bytes(store, 32)?;
        let mut secret = [0u8; 32];
        secret.copy_from_slice(bytes.get(..32).ok_or_else(|| internal("random: short"))?);
        bytes.fill(0);
        if Identity::from_secret_bytes(&secret).is_ok() {
            return Ok(Secret32::new(secret));
        }
        secret.fill(0);
    }
    Err(KeyError::Store {
        rule: "random".to_string(),
        text: "the platform's random source gave no valid identity key in 8 tries".to_string(),
    })
}

/// Key 2 and its words from `source`.
pub(crate) fn make_key2(
    store: &Arc<dyn KeyStore>,
    source: &EntropySource,
    network: Network,
) -> Result<(Key2, Vec<String>), KeyError> {
    let mnemonic = match source {
        EntropySource::Platform => {
            let mut bytes = random_bytes(store, 32)?;
            let m = bip39::Mnemonic::from_entropy(&bytes).map_err(internal);
            bytes.fill(0);
            m?
        }
        EntropySource::DiceHashed { rolls } => words_from_dice_hashed(rolls)
            .map_err(|e| refused_by("entropy", e))?
            .into_mnemonic(),
        EntropySource::CoinFlips { heads, words } => {
            let flips: Vec<Flip> = heads
                .iter()
                .map(|h| if *h { Flip::Heads } else { Flip::Tails })
                .collect();
            words_from_bits(&bits_from_coin(&flips), (*words).into())
                .map_err(|e| refused_by("entropy", e))?
                .into_mnemonic()
        }
        EntropySource::DiceSkewCorrected { rolls, words } => {
            let bits = bits_from_dice(rolls).map_err(|e| refused_by("entropy", e))?;
            words_from_bits(&bits, (*words).into())
                .map_err(|e| refused_by("entropy", e))?
                .into_mnemonic()
        }
        EntropySource::TypedWords { words } => mnemonic_from_typed(words)?,
    };
    // The owner typed these words in: they are not kept or returned.
    let words: Vec<String> = if matches!(source, EntropySource::TypedWords { .. }) {
        Vec::new()
    } else {
        mnemonic.words().map(str::to_string).collect()
    };
    let mut seed = mnemonic.to_seed_normalized("");
    let key = key2_from_seed(&seed, network);
    seed.fill(0);
    std::hint::black_box(&mut seed);
    match key {
        Ok(k) => Ok((k, words)),
        Err(e) => {
            words.into_iter().for_each(wipe_string);
            Err(e)
        }
    }
}

/// The account key `m/48'/coin'/0'/2'` of a BIP39 seed, and the master key's
/// fingerprint (the origin of the slot in the wallet description).
fn key2_from_seed(seed: &[u8; 64], network: Network) -> Result<Key2, KeyError> {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(network, seed).map_err(internal)?;
    let path = DerivationPath::from(vec![
        ChildNumber::Hardened { index: 48 },
        ChildNumber::Hardened {
            index: coin_type(network),
        },
        ChildNumber::Hardened { index: 0 },
        ChildNumber::Hardened { index: 2 },
    ]);
    let account = master.derive_priv(&secp, &path).map_err(internal)?;
    Ok(Key2 {
        network,
        master_fingerprint: master.fingerprint(&secp),
        account,
    })
}

/// A new identity and, when `source` is given, key 2 with its words.
pub(crate) fn make_material(
    store: &Arc<dyn KeyStore>,
    source: Option<(&EntropySource, Network)>,
) -> Result<(Material, Option<Vec<String>>), KeyError> {
    let identity = new_identity_secret(store)?;
    match source {
        Some((s, network)) => {
            let (key2, words) = make_key2(store, s, network)?;
            Ok((
                Material {
                    identity,
                    key2: Some(key2),
                    key3: None,
                },
                Some(words).filter(|w| !w.is_empty()),
            ))
        }
        None => Ok((
            Material {
                identity,
                key2: None,
                key3: None,
            },
            None,
        )),
    }
}

#[cfg(test)]
mod vector {
    use super::*;
    use bitcoin::bip32::Xpub;

    /// The well-known all-"abandon" 12-word phrase, as other wallets derive
    /// it: master fingerprint 73c5da0a and the account key at m/48'/0'/0'/2'.
    #[test]
    fn key_2_is_derived_as_other_wallets_derive_it() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon \
                      abandon abandon abandon about";
        let mnemonic = bip39::Mnemonic::parse_normalized(phrase).unwrap();
        let seed = mnemonic.to_seed_normalized("");
        let key = key2_from_seed(&seed, Network::Bitcoin).unwrap();
        assert_eq!(key.master_fingerprint.to_string(), "73c5da0a");
        let secp = Secp256k1::new();
        assert_eq!(
            Xpub::from_priv(&secp, &key.account).to_string(),
            "xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf"
        );
    }
}
