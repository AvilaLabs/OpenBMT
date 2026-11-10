//! Pairing a Box: the invitation, the claim and the welcome (BX-10 to BX-15,
//! [rules](../../../docs/BOX-RULES.md)).
//!
//! A Box has no screen, so the secret goes the other way: the Key app makes
//! an [`Invitation`] and the merchant types it into the Box. The Box answers
//! a relay request with a [`Claim`] that only someone holding the invitation
//! can check, and the Key app answers an approved claim with a [`Welcome`]
//! that only the Box holding the same invitation accepts.
//!
//! Everything here is pure: no clock, no randomness of its own (the caller
//! hands in the 17 bytes of an invitation), strict encodings. A MAC is
//! compared with `bitcoin::hashes::cmp::fixed_time_eq`, which looks at every
//! byte whatever the first difference is.

use std::fmt;

use bitcoin::hashes::{cmp::fixed_time_eq, sha256, Hmac, HmacEngine};
use bitcoin::hashes::{Hash, HashEngine};

use crate::batch::PinnedRoots;
use crate::hashing::tagged_hash;
use crate::hkdf;
use crate::identity::DeviceId;
use crate::wire::{self, DecodeError, Reader};

/// Bytes the caller supplies for an invitation. Only the first 130 bits are
/// used: the top 6 bits of the last byte are dropped.
pub const INVITATION_RANDOM_LEN: usize = 17;
/// Characters of the invitation itself in its text (BX-10).
pub const INVITATION_CHARS: usize = 26;
/// Check characters after them (BX-10).
pub const CHECK_CHARS: usize = 4;
/// All the characters of the text, not counting hyphens.
pub const TEXT_CHARS: usize = INVITATION_CHARS + CHECK_CHARS;
/// Characters in each group of the text.
pub const GROUP_CHARS: usize = 5;
/// The tag of the check characters (BX-10).
pub const TAG_INVITE_CHECK: &str = "OpenBMT/box-invite-check/v1";
/// The HKDF salt of the keys made from an invitation (BX-12).
pub const INVITE_SALT: &[u8] = b"OpenBMT/box-invite/v1";
/// The HKDF label of the claim key.
pub const LABEL_CLAIM: &[u8] = b"claim";
/// The HKDF label of the welcome key.
pub const LABEL_WELCOME: &[u8] = b"welcome";
/// Length of a claim in bytes.
pub const CLAIM_LEN: usize = 4 + 1 + 32 + 32 + 32;
/// Length of a welcome in bytes.
pub const WELCOME_LEN: usize = 4 + 1 + 32 + 32 + 32;

/// Crockford's base32 alphabet: no I, L, O or U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Why the text of an invitation was refused (BX-10). Each cause is its own
/// error, so a screen can say what to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteTextError {
    /// After case, `O`, `I`, `L`, hyphens and spaces are dealt with, there
    /// are not 30 characters. Carries how many there were.
    BadLength(usize),
    /// A character is not in the alphabet.
    BadCharacter(char),
    /// The 4 check characters do not match the other 26.
    BadCheck,
}

impl fmt::Display for InviteTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength(n) => write!(
                f,
                "BX-10: an invitation has {TEXT_CHARS} characters, this has {n}"
            ),
            Self::BadCharacter(c) => {
                write!(f, "BX-10: {c:?} is not a character of an invitation")
            }
            Self::BadCheck => write!(
                f,
                "BX-10: the check characters do not match: a character was mistyped"
            ),
        }
    }
}

impl std::error::Error for InviteTextError {}

/// A Box invitation: 130 bits that only the Key app and the merchant know
/// (BX-10). It prints as `XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX`. `Debug`
/// shows nothing of it, and the bytes are overwritten when it is dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Invitation {
    bytes: [u8; INVITATION_RANDOM_LEN],
}

impl fmt::Debug for Invitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invitation(..)")
    }
}

impl Drop for Invitation {
    fn drop(&mut self) {
        for b in self.bytes.iter_mut() {
            *b = 0;
        }
        std::hint::black_box(&mut self.bytes);
    }
}

/// Bit `i` (0 is the first, most significant) of the 130 bits of an
/// invitation: the first 16 bytes, then the low two bits of the last.
fn bit_of(bytes: &[u8; INVITATION_RANDOM_LEN], i: usize) -> u8 {
    if i < 128 {
        (bytes[i / 8] >> (7 - i % 8)) & 1
    } else {
        (bytes[16] >> (129 - i)) & 1
    }
}

fn symbol_value(c: char) -> Option<u8> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    let byte = u8::try_from(c).ok()?;
    ALPHABET
        .iter()
        .position(|a| *a == byte)
        .and_then(|p| u8::try_from(p).ok())
}

/// The 4 check characters of the 26 (BX-10): the first 20 bits of the
/// tagged hash of the 26 as ASCII.
fn check_chars(body: &[u8; INVITATION_CHARS]) -> [u8; CHECK_CHARS] {
    let h = tagged_hash(TAG_INVITE_CHECK, body);
    let word = (u32::from(h[0]) << 16) | (u32::from(h[1]) << 8) | u32::from(h[2]);
    let mut out = [0u8; CHECK_CHARS];
    for (k, slot) in out.iter_mut().enumerate() {
        let v = (word >> (15 - 5 * k)) & 31;
        *slot = ALPHABET[v as usize];
    }
    out
}

impl Invitation {
    /// An invitation from 17 random bytes the caller drew. The top 6 bits
    /// of the last byte are not part of it and are cleared, so two inputs
    /// that differ only there are the same invitation.
    pub fn from_random(random: &[u8; INVITATION_RANDOM_LEN]) -> Self {
        let mut bytes = *random;
        bytes[16] &= 0x03;
        Self { bytes }
    }

    /// The 17 bytes (the top 6 bits of the last are zero).
    pub fn as_bytes(&self) -> &[u8; INVITATION_RANDOM_LEN] {
        &self.bytes
    }

    /// The 26 characters, without the check or hyphens.
    fn body(&self) -> [u8; INVITATION_CHARS] {
        let mut out = [0u8; INVITATION_CHARS];
        for (k, slot) in out.iter_mut().enumerate() {
            let mut v = 0usize;
            for j in 0..5 {
                v = (v << 1) | usize::from(bit_of(&self.bytes, 5 * k + j));
            }
            *slot = ALPHABET[v];
        }
        out
    }

    /// The text to show: six groups of five, separated by hyphens (BX-10).
    pub fn to_text(&self) -> String {
        let body = self.body();
        let check = check_chars(&body);
        let all: Vec<u8> = body.iter().chain(check.iter()).copied().collect();
        let mut out = String::with_capacity(TEXT_CHARS + 5);
        for (i, group) in all.chunks(GROUP_CHARS).enumerate() {
            if i > 0 {
                out.push('-');
            }
            out.extend(group.iter().map(|b| char::from(*b)));
        }
        out
    }

    /// Read what a person typed (BX-10). Case does not matter, `O` is read
    /// as `0` and `I` and `L` as `1`, and hyphens and spaces are ignored.
    /// The length is checked first, then each character, then the check
    /// characters, and the first problem is the error.
    pub fn from_text(text: &str) -> Result<Self, InviteTextError> {
        let chars: Vec<char> = text.chars().filter(|c| *c != '-' && *c != ' ').collect();
        if chars.len() != TEXT_CHARS {
            return Err(InviteTextError::BadLength(chars.len()));
        }
        let mut values = [0u8; TEXT_CHARS];
        for (slot, c) in values.iter_mut().zip(&chars) {
            *slot = symbol_value(*c).ok_or(InviteTextError::BadCharacter(*c))?;
        }
        let mut body = [0u8; INVITATION_CHARS];
        for (slot, v) in body.iter_mut().zip(&values) {
            *slot = ALPHABET[usize::from(*v)];
        }
        let check = check_chars(&body);
        let given: Vec<u8> = values[INVITATION_CHARS..]
            .iter()
            .map(|v| ALPHABET[usize::from(*v)])
            .collect();
        if !fixed_time_eq(&check, &given) {
            return Err(InviteTextError::BadCheck);
        }
        let mut bytes = [0u8; INVITATION_RANDOM_LEN];
        for i in 0..130 {
            let bit = (values[i / 5] >> (4 - i % 5)) & 1;
            if i < 128 {
                bytes[i / 8] |= bit << (7 - i % 8);
            } else {
                bytes[16] |= bit << (129 - i);
            }
        }
        Ok(Self { bytes })
    }

    fn key(&self, label: &[u8]) -> [u8; 32] {
        hkdf::derive32(INVITE_SALT, &self.bytes, label)
    }

    /// The key of the claim's MAC (BX-12).
    fn claim_key(&self) -> [u8; 32] {
        self.key(LABEL_CLAIM)
    }

    /// The key of the welcome's MAC (BX-15).
    fn welcome_key(&self) -> [u8; 32] {
        self.key(LABEL_WELCOME)
    }
}

fn mac(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut engine = HmacEngine::<sha256::Hash>::new(key);
    for p in parts {
        engine.input(p);
    }
    Hmac::<sha256::Hash>::from_engine(engine).to_byte_array()
}

/// What a Box answers while it holds an invitation (BX-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    /// The Box's identity key.
    pub box_key: DeviceId,
    /// New for every invitation the Box is given.
    pub nonce: [u8; 32],
    /// HMAC-SHA256 under the claim key.
    pub mac: [u8; 32],
}

impl Claim {
    fn mac_of(invitation: &Invitation, box_key: &DeviceId, nonce: &[u8; 32]) -> [u8; 32] {
        mac(
            &invitation.claim_key(),
            &[
                &wire::MAGIC_BOX_CLAIM,
                &[wire::VERSION],
                &box_key.to_bytes(),
                nonce,
            ],
        )
    }

    /// The claim of a Box holding `invitation`.
    pub fn make(invitation: &Invitation, box_key: DeviceId, nonce: [u8; 32]) -> Self {
        Self {
            mac: Self::mac_of(invitation, &box_key, &nonce),
            box_key,
            nonce,
        }
    }

    /// `OBM5`, version 1, box key, box nonce, MAC.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_BOX_CLAIM, 96);
        out.extend_from_slice(&self.box_key.to_bytes());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.mac);
        out
    }

    /// Read a claim strictly. This does not check the MAC: that takes the
    /// invitation ([`Claim::verify`]).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_BOX_CLAIM)?;
        let box_key = r.device_id()?;
        let nonce = r.array()?;
        let mac = r.array()?;
        r.finish()?;
        Ok(Self {
            box_key,
            nonce,
            mac,
        })
    }

    /// Whether the claim's MAC is the one for `invitation` (BX-14), in
    /// constant time.
    pub fn verify(&self, invitation: &Invitation) -> bool {
        fixed_time_eq(
            &Self::mac_of(invitation, &self.box_key, &self.nonce),
            &self.mac,
        )
    }
}

/// What the Key app sends a Box once the owner approved its claim (BX-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Welcome {
    /// The owner phone's root.
    pub owner: DeviceId,
    /// The Key app's root.
    pub approver: DeviceId,
    /// HMAC-SHA256 under the welcome key, which also covers the Box's key
    /// and nonce.
    pub mac: [u8; 32],
}

impl Welcome {
    fn mac_of(
        invitation: &Invitation,
        box_key: &DeviceId,
        nonce: &[u8; 32],
        owner: &DeviceId,
        approver: &DeviceId,
    ) -> [u8; 32] {
        mac(
            &invitation.welcome_key(),
            &[
                &wire::MAGIC_BOX_WELCOME,
                &[wire::VERSION],
                &box_key.to_bytes(),
                nonce,
                &owner.to_bytes(),
                &approver.to_bytes(),
            ],
        )
    }

    /// The welcome for the Box that made `claim`, naming `roots`.
    pub fn make(invitation: &Invitation, claim: &Claim, roots: PinnedRoots) -> Self {
        Self {
            mac: Self::mac_of(
                invitation,
                &claim.box_key,
                &claim.nonce,
                &roots.owner,
                &roots.approver,
            ),
            owner: roots.owner,
            approver: roots.approver,
        }
    }

    /// `OBMM`, version 1, owner root, approver root, MAC. The Box's key and
    /// nonce are in the MAC and not in the frame.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_BOX_WELCOME, 96);
        out.extend_from_slice(&self.owner.to_bytes());
        out.extend_from_slice(&self.approver.to_bytes());
        out.extend_from_slice(&self.mac);
        out
    }

    /// Read a welcome strictly. This does not check the MAC.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_BOX_WELCOME)?;
        let owner = r.device_id()?;
        let approver = r.device_id()?;
        let mac = r.array()?;
        r.finish()?;
        Ok(Self {
            owner,
            approver,
            mac,
        })
    }

    /// The roots to pin, if the MAC is the one for `invitation`, this Box
    /// key and this Box nonce (BX-15), compared in constant time, and the
    /// two roots are different keys.
    pub fn verify(
        &self,
        invitation: &Invitation,
        box_key: &DeviceId,
        nonce: &[u8; 32],
    ) -> Option<PinnedRoots> {
        let expected = Self::mac_of(invitation, box_key, nonce, &self.owner, &self.approver);
        if !fixed_time_eq(&expected, &self.mac) {
            return None;
        }
        PinnedRoots::new(self.owner, self.approver).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::identity;

    fn sample() -> Invitation {
        let mut r = [0u8; INVITATION_RANDOM_LEN];
        for (i, b) in r.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        Invitation::from_random(&r)
    }

    fn other() -> Invitation {
        Invitation::from_random(&[0x5a; INVITATION_RANDOM_LEN])
    }

    fn box_key() -> DeviceId {
        identity(7).device_id()
    }

    fn roots() -> PinnedRoots {
        PinnedRoots::new(identity(1).device_id(), identity(2).device_id()).unwrap()
    }

    // ---- BX-10: the invitation and its text ----

    #[test]
    fn bx_10_text_has_six_groups_of_five() {
        let t = sample().to_text();
        assert_eq!(t.len(), 35);
        let groups: Vec<&str> = t.split('-').collect();
        assert_eq!(groups.len(), 6);
        assert!(groups.iter().all(|g| g.len() == 5));
        assert!(t.bytes().all(|b| b == b'-' || ALPHABET.contains(&b)));
    }

    #[test]
    fn bx_10_text_round_trips_and_uses_130_bits() {
        let i = sample();
        assert_eq!(Invitation::from_text(&i.to_text()), Ok(i.clone()));
        // The last byte's top six bits are not part of it.
        let mut r = *i.as_bytes();
        r[16] |= 0xfc;
        assert_eq!(Invitation::from_random(&r), i);
        assert_eq!(i.as_bytes()[16] & 0xfc, 0);
        for n in 0..200u8 {
            let mut r = [0u8; INVITATION_RANDOM_LEN];
            for (k, b) in r.iter_mut().enumerate() {
                *b = n.wrapping_mul(31).wrapping_add(k as u8).wrapping_mul(97);
            }
            let i = Invitation::from_random(&r);
            assert_eq!(Invitation::from_text(&i.to_text()), Ok(i));
        }
        // All ones and all zeros reach the ends of the alphabet.
        let ones = Invitation::from_random(&[0xff; INVITATION_RANDOM_LEN]);
        assert!(ones
            .to_text()
            .starts_with("ZZZZZ-ZZZZZ-ZZZZZ-ZZZZZ-ZZZZZ-Z"));
        assert_eq!(Invitation::from_text(&ones.to_text()), Ok(ones));
        let zeros = Invitation::from_random(&[0; INVITATION_RANDOM_LEN]);
        assert!(zeros
            .to_text()
            .starts_with("00000-00000-00000-00000-00000-0"));
        assert_eq!(Invitation::from_text(&zeros.to_text()), Ok(zeros));
    }

    #[test]
    fn bx_10_reading_is_forgiving_the_crockford_way() {
        let i = sample();
        let text = i.to_text();
        let same = |s: &str| assert_eq!(Invitation::from_text(s), Ok(i.clone()), "{s}");
        same(&text.to_lowercase());
        same(&text.replace('-', ""));
        same(&text.replace('-', " "));
        same(&format!("  {text}  "));
        same(&text.replace('-', " - "));
        // O reads as 0, I and L as 1: every such substitution gives the same
        // invitation. The sample's text is changed where it can be.
        let swapped: String = text
            .chars()
            .map(|c| match c {
                '0' => 'O',
                '1' => 'I',
                other => other,
            })
            .collect();
        same(&swapped);
        let swapped_l: String = text
            .chars()
            .map(|c| match c {
                '1' => 'l',
                '0' => 'o',
                other => other,
            })
            .collect();
        same(&swapped_l);
        // The check characters are read the same way.
        let zeros = Invitation::from_random(&[0; INVITATION_RANDOM_LEN]);
        let zt = zeros.to_text();
        assert_eq!(
            Invitation::from_text(&zt.replace('0', "O")),
            Ok(zeros.clone())
        );
        assert_eq!(Invitation::from_text(&zt.replace('0', "o")), Ok(zeros));
    }

    #[test]
    fn bx_10_each_refusal_is_its_own_error() {
        let text = sample().to_text();
        assert_eq!(
            Invitation::from_text(""),
            Err(InviteTextError::BadLength(0))
        );
        assert_eq!(
            Invitation::from_text(&text[..text.len() - 1]),
            Err(InviteTextError::BadLength(29))
        );
        assert_eq!(
            Invitation::from_text(&format!("{text}A")),
            Err(InviteTextError::BadLength(31))
        );
        // U is not in the alphabet and is not a stand-in for anything.
        let mut bad = text.clone();
        bad.replace_range(0..1, "U");
        assert_eq!(
            Invitation::from_text(&bad),
            Err(InviteTextError::BadCharacter('U'))
        );
        let mut bad = text.clone();
        bad.replace_range(2..3, "#");
        assert_eq!(
            Invitation::from_text(&bad),
            Err(InviteTextError::BadCharacter('#'))
        );
        // Hyphens are ignored only as hyphens: another character in their
        // place is a character that is not in the alphabet or a wrong length.
        assert_eq!(
            Invitation::from_text(&text.replace('-', "\u{e9}")),
            Err(InviteTextError::BadLength(35))
        );
        assert_eq!(
            Invitation::from_text(&text.replace('-', "")[..29].replace('0', "\u{e9}")),
            Err(InviteTextError::BadLength(29))
        );
        // A wrong check is refused after the length and characters pass.
        let mut bad = text.clone();
        let last = bad.pop().unwrap();
        bad.push(if last == '0' { '1' } else { '0' });
        assert_eq!(Invitation::from_text(&bad), Err(InviteTextError::BadCheck));
        // A length error wins over a character error.
        assert_eq!(
            Invitation::from_text("U"),
            Err(InviteTextError::BadLength(1))
        );
        for e in [
            InviteTextError::BadLength(3),
            InviteTextError::BadCharacter('U'),
            InviteTextError::BadCheck,
        ] {
            assert!(e.to_string().starts_with("BX-10"));
        }
    }

    #[test]
    fn bx_10_every_single_character_typo_is_caught_or_fails_the_claim() {
        let real = sample();
        let claim = Claim::make(&real, box_key(), [9u8; 32]);
        let text = real.to_text();
        let mut caught = 0usize;
        let mut other_invitation = 0usize;
        for (pos, original) in text.char_indices() {
            if original == '-' {
                continue;
            }
            for wrong in ALPHABET.iter().map(|b| char::from(*b)) {
                if wrong == original {
                    continue;
                }
                let mut typo = text.clone();
                typo.replace_range(pos..pos + 1, &wrong.to_string());
                match Invitation::from_text(&typo) {
                    Err(InviteTextError::BadCheck) => caught += 1,
                    Err(other) => panic!("unexpected {other:?} for {typo}"),
                    Ok(decoded) => {
                        // Not the same invitation, so the claim fails.
                        assert_ne!(decoded, real);
                        assert!(!claim.verify(&decoded));
                        other_invitation += 1;
                    }
                }
            }
        }
        // 30 characters, 31 wrong values each.
        assert_eq!(caught + other_invitation, 30 * 31);
        assert!(caught > 30 * 31 - 5);
    }

    #[test]
    fn bx_10_debug_shows_nothing_of_the_invitation() {
        let i = sample();
        let shown = format!("{i:?}");
        assert_eq!(shown, "Invitation(..)");
        assert!(!shown.contains(&i.to_text()[..5]));
    }

    #[test]
    fn bx_10_the_text_is_fixed() {
        // Written out so that the text format cannot drift.
        assert_eq!(sample().to_text(), VECTOR_TEXT);
        assert_eq!(
            Invitation::from_text(VECTOR_TEXT).unwrap().as_bytes(),
            sample().as_bytes()
        );
    }

    // ---- BX-12: the claim ----

    #[test]
    fn bx_12_claim_round_trips_and_verifies() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let bytes = c.to_bytes();
        assert_eq!(bytes.len(), CLAIM_LEN);
        assert_eq!(&bytes[..5], b"OBM5\x01");
        assert_eq!(Claim::from_bytes(&bytes), Ok(c));
        assert!(c.verify(&i));
        assert!(Claim::from_bytes(&bytes).unwrap().verify(&i));
    }

    #[test]
    fn bx_12_a_claim_for_another_invitation_fails() {
        let c = Claim::make(&sample(), box_key(), [3u8; 32]);
        assert!(!c.verify(&other()));
    }

    #[test]
    fn bx_12_any_flipped_bit_fails_the_claim_or_the_decode() {
        let i = sample();
        let good = Claim::make(&i, box_key(), [3u8; 32]).to_bytes();
        for bit in 0..good.len() * 8 {
            let mut b = good.clone();
            b[bit / 8] ^= 1 << (bit % 8);
            if let Ok(c) = Claim::from_bytes(&b) {
                assert!(!c.verify(&i), "bit {bit}");
            }
        }
    }

    #[test]
    fn bx_12_the_claim_binds_the_box_key_and_the_nonce() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let other_key = Claim {
            box_key: identity(8).device_id(),
            ..c
        };
        let other_nonce = Claim {
            nonce: [4u8; 32],
            ..c
        };
        assert!(!other_key.verify(&i));
        assert!(!other_nonce.verify(&i));
        // The nonce is what makes two invitations' claims differ for one Box.
        assert_ne!(
            Claim::make(&i, box_key(), [3u8; 32]).mac,
            Claim::make(&i, box_key(), [4u8; 32]).mac
        );
    }

    #[test]
    fn bx_12_claim_decoding_is_strict() {
        let good = Claim::make(&sample(), box_key(), [3u8; 32]).to_bytes();
        crate::testutil::assert_strict_decoding(&good, Claim::from_bytes, Claim::to_bytes);
        let mut bad_magic = good.clone();
        bad_magic[3] = b'6';
        assert_eq!(Claim::from_bytes(&bad_magic), Err(DecodeError::BadMagic));
        let mut bad_version = good.clone();
        bad_version[4] = 2;
        assert_eq!(
            Claim::from_bytes(&bad_version),
            Err(DecodeError::UnsupportedVersion(2))
        );
        // A key that is not on the curve.
        let mut bad_key = good;
        bad_key[5..37].fill(0);
        assert_eq!(Claim::from_bytes(&bad_key), Err(DecodeError::InvalidKey));
    }

    #[test]
    fn bx_12_the_claim_is_fixed() {
        // Written out so that the format and the keys cannot drift.
        let c = Claim::make(&sample(), box_key(), [0x42; 32]);
        assert_eq!(hex(&c.to_bytes()), VECTOR_CLAIM);
    }

    // ---- BX-14: constant-time comparison ----

    #[test]
    fn bx_14_the_comparison_is_the_fixed_time_one_over_all_32_bytes() {
        // A mac that differs only in its last byte, and one that differs
        // only in its first, both fail.
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        for at in [0usize, 31] {
            let mut m = c.mac;
            m[at] ^= 1;
            assert!(!Claim { mac: m, ..c }.verify(&i));
        }
    }

    // ---- BX-15: the welcome ----

    #[test]
    fn bx_15_welcome_round_trips_and_verifies() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let w = Welcome::make(&i, &c, roots());
        let bytes = w.to_bytes();
        assert_eq!(bytes.len(), WELCOME_LEN);
        assert_eq!(&bytes[..5], b"OBMM\x01");
        let back = Welcome::from_bytes(&bytes).unwrap();
        assert_eq!(back, w);
        assert_eq!(back.verify(&i, &c.box_key, &c.nonce), Some(roots()));
    }

    #[test]
    fn bx_15_a_welcome_for_another_invitation_fails() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let w = Welcome::make(&i, &c, roots());
        assert_eq!(w.verify(&other(), &c.box_key, &c.nonce), None);
        // One made with the wrong invitation does not pass the right one.
        let forged = Welcome::make(&other(), &c, roots());
        assert_eq!(forged.verify(&i, &c.box_key, &c.nonce), None);
    }

    #[test]
    fn bx_15_a_welcome_for_another_box_key_or_nonce_fails() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let w = Welcome::make(&i, &c, roots());
        assert_eq!(w.verify(&i, &identity(8).device_id(), &c.nonce), None);
        assert_eq!(w.verify(&i, &c.box_key, &[4u8; 32]), None);
    }

    #[test]
    fn bx_15_the_welcome_names_the_roots_in_its_mac() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let w = Welcome::make(&i, &c, roots());
        let swapped = Welcome {
            owner: w.approver,
            approver: w.owner,
            ..w
        };
        assert_eq!(swapped.verify(&i, &c.box_key, &c.nonce), None);
        let other_owner = Welcome {
            owner: identity(9).device_id(),
            ..w
        };
        assert_eq!(other_owner.verify(&i, &c.box_key, &c.nonce), None);
    }

    #[test]
    fn bx_15_two_roots_that_are_one_key_are_not_pinned() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let k = identity(1).device_id();
        let w = Welcome {
            owner: k,
            approver: k,
            mac: Welcome::mac_of(&i, &c.box_key, &c.nonce, &k, &k),
        };
        assert_eq!(w.verify(&i, &c.box_key, &c.nonce), None);
    }

    #[test]
    fn bx_15_any_flipped_bit_fails_the_welcome_or_the_decode() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let good = Welcome::make(&i, &c, roots()).to_bytes();
        for bit in 0..good.len() * 8 {
            let mut b = good.clone();
            b[bit / 8] ^= 1 << (bit % 8);
            if let Ok(w) = Welcome::from_bytes(&b) {
                assert_eq!(w.verify(&i, &c.box_key, &c.nonce), None, "bit {bit}");
            }
        }
    }

    #[test]
    fn bx_15_welcome_decoding_is_strict() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [3u8; 32]);
        let good = Welcome::make(&i, &c, roots()).to_bytes();
        crate::testutil::assert_strict_decoding(&good, Welcome::from_bytes, Welcome::to_bytes);
        let mut bad_magic = good.clone();
        bad_magic[3] = b'N';
        assert_eq!(Welcome::from_bytes(&bad_magic), Err(DecodeError::BadMagic));
        // A claim is not a welcome, even though the lengths are the same.
        assert_eq!(
            Welcome::from_bytes(&c.to_bytes()),
            Err(DecodeError::BadMagic)
        );
        assert_eq!(Claim::from_bytes(&good), Err(DecodeError::BadMagic));
    }

    #[test]
    fn bx_15_the_welcome_is_fixed() {
        let i = sample();
        let c = Claim::make(&i, box_key(), [0x42; 32]);
        let w = Welcome::make(&i, &c, roots());
        assert_eq!(hex(&w.to_bytes()), VECTOR_WELCOME);
    }

    // ---- BX-12, BX-15: the keys ----
    // The vectors below were also worked out apart from this code (HKDF,
    // HMAC and the base32 text) and agree.

    #[test]
    fn bx_12_the_two_keys_differ_and_come_from_the_invitation() {
        let i = sample();
        assert_ne!(i.claim_key(), i.welcome_key());
        assert_ne!(i.claim_key(), other().claim_key());
        assert_eq!(hex(&i.claim_key()), VECTOR_CLAIM_KEY);
        assert_eq!(hex(&i.welcome_key()), VECTOR_WELCOME_KEY);
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    const VECTOR_TEXT: &str = "1CR5A-YMZRK-MGWCT-RFPHC-FV0H6-V02EQ";
    const VECTOR_CLAIM_KEY: &str =
        "f7ceb36457d9c97c5d294202b455dc437ebc9b81a15891b8dc2c7ea32ff79d24";
    const VECTOR_WELCOME_KEY: &str =
        "1b03d2c41e20e936570daeb2cbbb8781ad4ad0655145d1dc6d2344513a11e3f8";
    const VECTOR_CLAIM: &str =
        "4f424d3501989c0b76cb563971fdc9bef31ec06c3560f3249d6ee9e5d83c57625596e05f6f424242424242424242424242424242424242424242424242424242424242424203ca4703326d8d614a6324152c7c9f095c1919a2d904bbe2110e244817830856";
    const VECTOR_WELCOME: &str =
        "4f424d4d011b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f4d4b6cd1361032ca9bd2aeb9d900aa4d45d9ead80ac9423374c451a7254d0766b1fc1db56ef511a9eb668dfe0673cbde4105724ab8ba3fff6d8248000eb633ef";
}
