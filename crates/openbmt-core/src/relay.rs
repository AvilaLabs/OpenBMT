//! Where a counter finds the relay it talks to the owner through, and how
//! the owner tells it (`docs/COUNTER-RULES.md`, CR-130 to CR-133).
//!
//! A [`RelayHint`] is a route and the relay's key. It gives no authority
//! (`docs/LINK-RULES.md`, LK-09): a counter that is given a wrong one only
//! fails to deliver, which its link line shows. The same bytes are carried
//! three ways:
//!
//! - at the end of the counter bundle (version 2 of `OBMs`, see
//!   [`crate::bundle`]);
//! - in the counter's saved pairing (version 2 of `OBMt`, in the counter node
//!   crate);
//! - alone, signed by the owner root, as the "relay for counters" a counter
//!   paired before the shop had a Box takes ([`SignedRelayHint`]).
//!
//! Wire form of the hint (strict, see [`crate::wire`]):
//!
//! ```text
//! route tag u8
//!   0 direct: len u8 | "ip:port" (canonical spelling, port above 0)
//!   1 onion:  name (56 bytes: a-z and 2-7) | port u16 LE (above 0)
//! relay key (32)
//! ```
//!
//! The wire form of the signed hint:
//!
//! ```text
//! "OBMy" | 1 | hint | owner signature (64)
//! ```
//!
//! The owner signs the hint's bytes under its own tag. A counter checks the
//! signature against the owner root it pinned at pairing.

use std::fmt;
use std::net::SocketAddr;

use crate::bundle::peer_is_valid;
use crate::identity::{DeviceId, Identity, Sig, TAG_RELAY_HINT};
use crate::wire::{self, DecodeError, Reader};

/// The length of the name of a Tor v3 onion service, without `.onion`.
pub const ONION_NAME_LEN: usize = 56;

/// How the relay is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayRoute {
    /// An address and port on the local network.
    Direct(SocketAddr),
    /// A Tor onion service: the 56 characters of its v3 name, without
    /// `.onion`, and the port.
    Onion {
        /// The name, 56 characters of a to z and 2 to 7.
        name: String,
        /// The port, above 0.
        port: u16,
    },
}

/// Why a hint could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayHintError {
    /// The relay's key is not a point on the curve.
    BadKey,
    /// A direct address with port 0.
    BadAddress,
    /// An onion name that is not 56 characters of a to z and 2 to 7, or a
    /// port of 0.
    BadOnion,
}

impl fmt::Display for RelayHintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadKey => write!(f, "CR-133: the relay's key is not a key"),
            Self::BadAddress => write!(f, "CR-133: the relay's address has port 0"),
            Self::BadOnion => write!(
                f,
                "CR-133: the relay's onion name is not a name of 56 characters, or its port is 0"
            ),
        }
    }
}

impl std::error::Error for RelayHintError {}

/// Whether `name` is 56 characters of a to z and 2 to 7.
fn onion_name_is_valid(name: &str) -> bool {
    name.len() == ONION_NAME_LEN && name.bytes().all(|b| matches!(b, b'a'..=b'z' | b'2'..=b'7'))
}

/// A relay: how to reach it and its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayHint {
    /// How to reach it.
    pub route: RelayRoute,
    /// Its key.
    pub key: [u8; 32],
}

impl RelayHint {
    /// A hint. The key must be a key, a direct address must have a port, and
    /// an onion name must be 56 characters of a to z and 2 to 7 with a port.
    pub fn new(route: RelayRoute, key: [u8; 32]) -> Result<Self, RelayHintError> {
        if DeviceId::from_bytes(&key).is_err() {
            return Err(RelayHintError::BadKey);
        }
        match &route {
            RelayRoute::Direct(addr) => {
                if addr.port() == 0 {
                    return Err(RelayHintError::BadAddress);
                }
            }
            RelayRoute::Onion { name, port } => {
                if *port == 0 || !onion_name_is_valid(name) {
                    return Err(RelayHintError::BadOnion);
                }
            }
        }
        Ok(Self { route, key })
    }

    /// The relay's key as a device.
    pub fn device(&self) -> Option<DeviceId> {
        DeviceId::from_bytes(&self.key).ok()
    }

    /// Append the wire form: see the module docs. Call it on a hint that
    /// came from [`RelayHint::new`] or from decoding.
    pub fn put(&self, out: &mut Vec<u8>) {
        match &self.route {
            RelayRoute::Direct(addr) => {
                let text = addr.to_string();
                out.push(0);
                out.push(u8::try_from(text.len()).unwrap_or(u8::MAX));
                out.extend_from_slice(text.as_bytes());
            }
            RelayRoute::Onion { name, port } => {
                out.push(1);
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&port.to_le_bytes());
            }
        }
        out.extend_from_slice(&self.key);
    }

    /// The wire form of the hint alone, which the owner signs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(96);
        self.put(&mut out);
        out
    }

    /// Read a hint from `r`, strictly: the address in its canonical
    /// spelling, the name and ports valid, the key a key.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let route = match r.u8()? {
            0 => {
                let len = usize::from(r.u8()?);
                let text = std::str::from_utf8(r.take(len)?)
                    .map_err(|_| DecodeError::NonCanonical("the relay address is not UTF-8"))?;
                if !peer_is_valid(text) {
                    return Err(DecodeError::NonCanonical("the relay address is not ip:port"));
                }
                let addr = text
                    .parse()
                    .map_err(|_| DecodeError::NonCanonical("the relay address is not ip:port"))?;
                RelayRoute::Direct(addr)
            }
            1 => {
                let raw = r.take(ONION_NAME_LEN)?;
                let name = std::str::from_utf8(raw)
                    .map_err(|_| DecodeError::NonCanonical("the onion name is not UTF-8"))?;
                if !onion_name_is_valid(name) {
                    return Err(DecodeError::NonCanonical("the onion name is not a v3 name"));
                }
                let port = r.u16_le()?;
                if port == 0 {
                    return Err(DecodeError::NonCanonical("the relay port is 0"));
                }
                RelayRoute::Onion {
                    name: name.to_owned(),
                    port,
                }
            }
            _ => return Err(DecodeError::NonCanonical("the relay route tag is unknown")),
        };
        let key: [u8; 32] = r.array()?;
        if DeviceId::from_bytes(&key).is_err() {
            return Err(DecodeError::InvalidKey);
        }
        Ok(Self { route, key })
    }
}

/// A relay hint and the owner root's signature on it: the "relay for
/// counters" (CR-132).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRelayHint {
    /// The hint.
    pub hint: RelayHint,
    /// The owner root's signature on the hint's bytes.
    pub owner_sig: Sig,
}

impl SignedRelayHint {
    /// Sign `hint` with the owner root's identity.
    pub fn sign(hint: RelayHint, owner: &Identity) -> Self {
        let owner_sig = owner.sign_tagged(TAG_RELAY_HINT, &hint.to_bytes());
        Self { hint, owner_sig }
    }

    /// Whether the signature is `owner`'s over the hint.
    pub fn verify(&self, owner: &DeviceId) -> bool {
        owner.verify_tagged(TAG_RELAY_HINT, &self.hint.to_bytes(), &self.owner_sig)
    }

    /// Wire form: see the module docs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_RELAY_FOR_COUNTERS, 160);
        self.hint.put(&mut out);
        out.extend_from_slice(&self.owner_sig.0);
        out
    }

    /// Parse the wire form strictly. This does not check the signature:
    /// [`SignedRelayHint::verify`] does.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_RELAY_FOR_COUNTERS)?;
        let hint = RelayHint::read(&mut r)?;
        let owner_sig = r.sig()?;
        r.finish()?;
        Ok(Self { hint, owner_sig })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::TAG_COMMAND;
    use crate::testutil::{assert_strict_decoding, identity};

    pub(crate) const NAME: &str = "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad";

    fn key() -> [u8; 32] {
        identity(40).device_id().to_bytes()
    }

    fn direct() -> RelayHint {
        RelayHint::new(RelayRoute::Direct("192.168.1.20:48333".parse().unwrap()), key()).unwrap()
    }

    fn onion() -> RelayHint {
        RelayHint::new(
            RelayRoute::Onion {
                name: NAME.to_string(),
                port: 48333,
            },
            key(),
        )
        .unwrap()
    }

    fn signed(hint: RelayHint) -> SignedRelayHint {
        SignedRelayHint::sign(hint, &identity(1))
    }

    #[test]
    fn cr_133_a_hint_round_trips_in_both_routes() {
        for hint in [direct(), onion()] {
            let bytes = hint.to_bytes();
            let mut r = Reader::new(&bytes);
            assert_eq!(RelayHint::read(&mut r).unwrap(), hint);
            r.finish().unwrap();
        }
        let v6 = RelayHint::new(RelayRoute::Direct("[::1]:1".parse().unwrap()), key()).unwrap();
        let bytes = v6.to_bytes();
        let mut r = Reader::new(&bytes);
        assert_eq!(RelayHint::read(&mut r).unwrap(), v6);
    }

    #[test]
    fn cr_133_a_hint_is_made_only_from_a_key_a_port_and_a_name() {
        let addr = |text: &str| RelayRoute::Direct(text.parse().unwrap());
        assert_eq!(
            RelayHint::new(addr("1.2.3.4:0"), key()).unwrap_err(),
            RelayHintError::BadAddress
        );
        assert_eq!(
            RelayHint::new(addr("1.2.3.4:5"), [0; 32]).unwrap_err(),
            RelayHintError::BadKey
        );
        let onion = |name: &str, port: u16| RelayRoute::Onion {
            name: name.to_string(),
            port,
        };
        assert_eq!(
            RelayHint::new(onion(NAME, 0), key()).unwrap_err(),
            RelayHintError::BadOnion
        );
        for bad in ["", "abc", &NAME.to_uppercase(), &NAME.replace('a', "1")] {
            assert_eq!(
                RelayHint::new(onion(bad, 1), key()).unwrap_err(),
                RelayHintError::BadOnion,
                "{bad:?}"
            );
        }
        let with_suffix = format!("{NAME}.onion");
        assert!(RelayHint::new(onion(&with_suffix, 1), key()).is_err());
        for e in [
            RelayHintError::BadKey,
            RelayHintError::BadAddress,
            RelayHintError::BadOnion,
        ] {
            assert!(e.to_string().starts_with("CR-133"));
        }
    }

    #[test]
    fn cr_133_a_hint_is_read_strictly() {
        let read = |bytes: &[u8]| {
            let mut r = Reader::new(bytes);
            RelayHint::read(&mut r)
        };
        // An unknown route tag.
        let mut bytes = direct().to_bytes();
        bytes[0] = 2;
        assert!(matches!(read(&bytes), Err(DecodeError::NonCanonical(_))));
        // An address in another spelling, a name, port 0.
        for text in ["01.2.3.4:5", "example.com:5", "1.2.3.4:0", "1.2.3.4", ""] {
            let mut bytes = vec![0, text.len() as u8];
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(&key());
            assert!(read(&bytes).is_err(), "{text:?}");
        }
        // An onion name with a capital, and port 0.
        let mut bytes = onion().to_bytes();
        bytes[1] = b'A';
        assert!(matches!(read(&bytes), Err(DecodeError::NonCanonical(_))));
        let mut bytes = onion().to_bytes();
        bytes[1 + ONION_NAME_LEN] = 0;
        bytes[2 + ONION_NAME_LEN] = 0;
        assert!(matches!(read(&bytes), Err(DecodeError::NonCanonical(_))));
        // A key that is not a point.
        let mut bytes = direct().to_bytes();
        let n = bytes.len();
        bytes[n - 32..].fill(0);
        assert_eq!(read(&bytes).unwrap_err(), DecodeError::InvalidKey);
        // Cut short at every length.
        for hint in [direct(), onion()] {
            let bytes = hint.to_bytes();
            for n in 0..bytes.len() {
                assert!(read(&bytes[..n]).is_err(), "{n}");
            }
        }
    }

    #[test]
    fn cr_132_the_frame_is_strict_and_has_its_own_magic() {
        for hint in [direct(), onion()] {
            let bytes = signed(hint).to_bytes();
            assert_eq!(&bytes[..5], b"OBMy\x01");
            assert_strict_decoding(&bytes, SignedRelayHint::from_bytes, SignedRelayHint::to_bytes);
        }
        let bytes = signed(direct()).to_bytes();
        let mut m = bytes.clone();
        m[..4].copy_from_slice(b"OBMs");
        assert_eq!(SignedRelayHint::from_bytes(&m).unwrap_err(), DecodeError::BadMagic);
        for v in [0u8, 2, 255] {
            let mut m = bytes.clone();
            m[4] = v;
            assert_eq!(
                SignedRelayHint::from_bytes(&m).unwrap_err(),
                DecodeError::UnsupportedVersion(v)
            );
        }
    }

    #[test]
    fn cr_132_only_the_owner_roots_signature_counts() {
        let good = signed(direct());
        assert!(good.verify(&identity(1).device_id()));
        // Another key's signature, and another hint under the same signature.
        assert!(!good.verify(&identity(2).device_id()));
        let other = SignedRelayHint {
            hint: onion(),
            owner_sig: good.owner_sig,
        };
        assert!(!other.verify(&identity(1).device_id()));
        // A signature made under another tag does not pass.
        let wrong_tag = SignedRelayHint {
            owner_sig: identity(1).sign_tagged(TAG_COMMAND, &direct().to_bytes()),
            hint: direct(),
        };
        assert!(!wrong_tag.verify(&identity(1).device_id()));
        // No flipped bit of the frame gives a hint the owner root has signed
        // that differs from the one it signed.
        let bytes = good.to_bytes();
        for i in 0..bytes.len() {
            for bit in 0..8 {
                let mut m = bytes.clone();
                m[i] ^= 1 << bit;
                if let Ok(s) = SignedRelayHint::from_bytes(&m) {
                    assert!(!s.verify(&identity(1).device_id()), "byte {i} bit {bit}");
                }
            }
        }
    }
}
