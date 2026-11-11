//! The Key app's part of the key ceremony (`docs/DESIGN.md` §2.1 steps 2
//! and 5, `docs/SIGNER-RULES.md` SR-64 to SR-69): taking key 3, the public
//! answer for the owner phone and accepting the wallet setup. The checks
//! are in `openbmt-signer` (`ceremony.rs`) and `accept_wallet`; this file
//! holds the Key app's state around them.

use bitcoin::bip32::Xpub;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Network, NetworkKind};
use openbmt_core::identity::Identity;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wallet::WalletDescription;
use openbmt_signer::{
    check_key3, check_key3_in_wallet, CeremonyRefusal, KeyAnswer, KeyPart, WalletSetupBytes,
};

use crate::app::{KeyApp, Phase, WalletSetup, WalletView};
use crate::codec::wire_record;
use crate::envelope::{self, Material};
use crate::error::{internal, refused, store_failed, wrong_state, KeyError};
use crate::keys::KeyNetwork;

/// Key 3's public parts as the Key app holds them, for the screen that
/// compares them with the other wallet app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key3View {
    /// The kind of network the key is for: `true` for mainnet, `false` for
    /// the test networks.
    pub mainnet: bool,
    /// The master fingerprint, 8 hex characters.
    pub master_fingerprint: String,
    /// The origin path of the account key, for example `48h/1h/0h/2h`.
    pub origin_path: String,
    /// The account-level extended public key, in its plain form.
    pub account_xpub: String,
}

wire_record!(Key3View {
    mainnet,
    master_fingerprint,
    origin_path,
    account_xpub,
});

impl Key3View {
    fn new(k: &KeyPart) -> Self {
        Self {
            mainnet: k.kind() == NetworkKind::Main,
            master_fingerprint: k.fingerprint.to_string(),
            origin_path: k.origin_path(),
            account_xpub: k.xpub.to_string(),
        }
    }

    /// What the key holder is shown once key 3 is taken (SC-01).
    pub fn words(&self) -> Words {
        openbmt_signer::key3_taken_words()
    }
}

/// The words of the Key app's public answer, once it is ready to show.
pub fn answer_words() -> Words {
    openbmt_signer::answer_ready_words()
}

/// The words of a Key app that holds key 3 and waits for the wallet.
pub fn key3_waiting_words() -> Words {
    Words::new(
        Tone::Info,
        "Waiting for the wallet",
        "This app holds key 3 and its own key, and is waiting for the wallet from the owner's phone.",
        Who::KeyHolder,
        "Show this app's answer to the owner's phone, then scan the wallet code it shows.",
    )
}

fn network_of(n: Network) -> KeyNetwork {
    match n {
        Network::Bitcoin => KeyNetwork::Mainnet,
        Network::Testnet => KeyNetwork::Testnet,
        Network::Signet => KeyNetwork::Signet,
        _ => KeyNetwork::Regtest,
    }
}

/// Key 2 of this device as the public parts the ceremony passes on.
pub(crate) fn key2_part(material: &Material) -> Option<KeyPart> {
    material.key2.as_ref().map(|k| KeyPart {
        fingerprint: k.master_fingerprint,
        xpub: Xpub::from_priv(&Secp256k1::new(), &k.account),
    })
}

/// SR-66, SR-67: the key 3 the wallet is checked against, from what the
/// Key app holds and what `accept_wallet` was given. The key 3 held is the
/// one used; a key 3 given with the wallet must be the same key. With
/// none held, the old path takes the given one, and the ceremony refuses.
pub(crate) fn key3_for_accept(
    material: &Material,
    description: &WalletDescription,
    network: Network,
    given: Option<&str>,
    ceremony: bool,
) -> Result<Option<Xpub>, KeyError> {
    let given = given.map(crate::app::parse_xpub).transpose()?;
    let Some(held) = &material.key3 else {
        return match (given, ceremony) {
            (_, true) => Err(refused(CeremonyRefusal::NoKey3)),
            (given, false) => Ok(given),
        };
    };
    if given.is_some_and(|g| g.encode() != held.xpub.encode()) {
        return Err(refused(CeremonyRefusal::Key3Differs));
    }
    check_key3(held, key2_part(material).as_ref(), network).map_err(refused)?;
    check_key3_in_wallet(held, description).map_err(refused)?;
    Ok(Some(held.xpub))
}

impl KeyApp {
    /// SR-64 to SR-67, `docs/DESIGN.md` §2.1 step 2: take key 3 from the
    /// text the other wallet app shows, and keep it with the Key app's
    /// state before any wallet exists. A later key 3 replaces it, until the
    /// wallet is accepted. Nothing is changed when the text is refused.
    pub fn take_key3(&self, text: String) -> Result<Key3View, KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            let material = match &inner.phase {
                Phase::Identity(m) => m,
                Phase::Empty => {
                    return Err(wrong_state(
                        "empty",
                        "make the Key app's key first (create_from_entropy)",
                    ))
                }
                Phase::Ready(_) => {
                    return Err(wrong_state("wallet-set", "the wallet is accepted already"))
                }
            };
            let key3 = KeyPart::parse(&text).map_err(refused)?;
            if let Some(k) = &material.key2 {
                check_key3(&key3, key2_part(material).as_ref(), k.network).map_err(refused)?;
            }
            inner
                .store
                .save(envelope::encode_with(material, Some(&key3), None))
                .map_err(|e| store_failed(&e))?;
            let view = Key3View::new(&key3);
            if let Phase::Identity(m) = &mut inner.phase {
                m.key3 = Some(key3);
            }
            Ok(view)
        })
    }

    /// Key 3 as the Key app holds it, if it has been taken.
    pub fn key3_view(&self) -> Result<Option<Key3View>, KeyError> {
        self.run(|inner| {
            let Some(material) = inner.material() else {
                return Err(wrong_state("empty", "no key has been made yet"));
            };
            Ok(material.key3.as_ref().map(Key3View::new))
        })
    }

    /// SR-68, SR-69, `docs/DESIGN.md` §2.1 step 5: the Key app's public
    /// answer (`OBMv`) for the owner phone: the network, key 2's public parts
    /// (none on a device with no key 2), key 3's as taken, and the Key app's
    /// root. Refused until key 3 is taken.
    pub fn public_answer(&self) -> Result<Vec<u8>, KeyError> {
        self.run(|inner| {
            let Some(material) = inner.material() else {
                return Err(wrong_state("empty", "no key has been made yet"));
            };
            let Some(key3) = material.key3.clone() else {
                return Err(refused(CeremonyRefusal::NoKey3));
            };
            let network = match (&material.key2, key3.kind()) {
                (Some(k), _) => k.network,
                (None, NetworkKind::Main) => Network::Bitcoin,
                (None, _) => Network::Testnet,
            };
            let device_root = Identity::from_secret_bytes(material.identity.bytes())
                .map_err(internal)?
                .device_id();
            Ok(KeyAnswer {
                network,
                key2: key2_part(material),
                key3,
                device_root,
            }
            .to_bytes())
        })
    }

    /// SR-67, SR-68: accept the wallet setup (`OBMw`) the owner phone made.
    /// The bytes are read strictly and then go through `accept_wallet`
    /// with the key 3 held, so the key 2 and key 3 slots are checked byte
    /// for byte. Refused when no key 3 was taken. The result is what the
    /// comparison screen shows: the four words and the three fingerprints.
    pub fn accept_wallet_setup(&self, bytes: Vec<u8>) -> Result<WalletView, KeyError> {
        let setup = WalletSetupBytes::from_bytes(&bytes).map_err(refused)?;
        self.accept(
            WalletSetup {
                description: setup.description.canonical_string().to_string(),
                network: network_of(setup.network),
                owner_root: setup.owner_root.to_bytes().to_vec(),
                key3_xpub: None,
                batch_min_count: Some(setup.batch_min_count),
            },
            true,
        )
    }
}
