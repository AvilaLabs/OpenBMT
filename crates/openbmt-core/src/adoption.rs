//! Owner approval for putting local checkout authority around an existing
//! installation.
//!
//! This protocol deliberately grants only one narrow action: enrolling local
//! checkout authority over one exact snapshot of an existing counter's POS
//! records. It does not carry a wallet, transaction, payment destination, or
//! spend API.
//!
//! A counter first signs an [`AdoptionRequest`]. The request binds its identity,
//! its pinned owner root, the exact five-part [`CheckoutScope`], a fixed
//! [`AdoptionManifest`], a caller-supplied creation time, and a fresh random
//! nonce. The owner root then signs the entire canonical request to make a
//! [`SignedAdoptionGrant`]. Request and grant signatures have different BIP340
//! tags, so neither can be replayed as the other.
//!
//! The nonce must be 32 fresh bytes from a cryptographic random source. This
//! I/O-free crate rejects the all-zero value and lets a verifier compare the
//! nonce exactly, but the caller remains responsible for generating it and
//! recording it as consumed.

use std::fmt;

use crate::identity::{
    tagged_hash, DeviceId, Identity, Sig, TAG_CHECKOUT_ADOPTION_GRANT,
    TAG_CHECKOUT_ADOPTION_MANIFEST, TAG_CHECKOUT_ADOPTION_REQUEST,
};
use crate::wire::{self, DecodeError, Reader};

/// Number of identifiers in the checkout authority scope.
pub const CHECKOUT_SCOPE_PARTS: usize = 5;
/// Maximum byte length of each scope identifier.
pub const MAX_SCOPE_ID_BYTES: usize = 64;
/// Number of files in the only accepted adoption manifest.
pub const ADOPTION_FILE_COUNT: usize = 5;
/// The fixed names and order of files covered by an adoption request.
pub const ADOPTION_FILE_NAMES: [&str; ADOPTION_FILE_COUNT] = [
    "checkout-ledger.bin",
    "checkout-catalog.bin",
    "checkout-cash-sessions.bin",
    "checkout-inventory.bin",
    "pairing.obks",
];

const PURPOSE_EXISTING_INSTALL_LOCAL_AUTHORITY: u8 = 1;
const MAX_ADOPTION_REQUEST_BYTES: usize = 2_048;

/// Why an adoption protocol value or signature was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptionError {
    /// One of the five checkout identifiers is outside the canonical alphabet
    /// or length.
    InvalidScope,
    /// The manifest does not have exactly the fixed files in the fixed order,
    /// the pairing file is absent, or an absent file carries a nonzero length
    /// or digest.
    InvalidManifest,
    /// The request nonce is the forbidden all-zero value.
    ZeroNonce,
    /// The request is not signed by its named counter under the request tag.
    BadCounterSignature,
    /// The identity asked to grant the request is not its named owner root.
    OwnerMismatch,
    /// The grant is not signed by its named owner root under the grant tag.
    BadOwnerSignature,
    /// Exact verification expected another counter.
    CounterMismatch,
    /// Exact verification expected another checkout scope.
    ScopeMismatch,
    /// Exact verification expected another manifest root.
    ManifestMismatch,
    /// Exact verification expected another pending nonce.
    NonceMismatch,
}

impl fmt::Display for AdoptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScope => write!(f, "checkout adoption scope is not canonical"),
            Self::InvalidManifest => write!(f, "checkout adoption manifest is not canonical"),
            Self::ZeroNonce => write!(f, "checkout adoption nonce is all zero"),
            Self::BadCounterSignature => {
                write!(f, "checkout adoption request signature is invalid")
            }
            Self::OwnerMismatch => write!(f, "checkout adoption owner root does not match"),
            Self::BadOwnerSignature => write!(f, "checkout adoption grant signature is invalid"),
            Self::CounterMismatch => write!(f, "checkout adoption counter does not match"),
            Self::ScopeMismatch => write!(f, "checkout adoption scope does not match"),
            Self::ManifestMismatch => write!(f, "checkout adoption manifest does not match"),
            Self::NonceMismatch => write!(f, "checkout adoption nonce does not match"),
        }
    }
}

impl std::error::Error for AdoptionError {}

fn scope_id_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=MAX_SCOPE_ID_BYTES).contains(&bytes.len())
        && matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9')
        && bytes
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.'))
}

fn put_scope_id(out: &mut Vec<u8>, value: &str) {
    debug_assert!(scope_id_is_valid(value));
    out.push(u8::try_from(value.len()).expect("scope id is at most 64 bytes"));
    out.extend_from_slice(value.as_bytes());
}

fn read_scope_id(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let len = usize::from(r.u8()?);
    if !(1..=MAX_SCOPE_ID_BYTES).contains(&len) {
        return Err(DecodeError::NonCanonical(
            "checkout adoption scope id length is outside 1..=64",
        ));
    }
    let value = std::str::from_utf8(r.take(len)?)
        .map_err(|_| DecodeError::NonCanonical("checkout adoption scope id is not UTF-8"))?;
    if !scope_id_is_valid(value) {
        return Err(DecodeError::NonCanonical(
            "checkout adoption scope id uses a forbidden character",
        ));
    }
    Ok(value.to_owned())
}

/// The installation-local authority scope, byte-for-byte equivalent to the
/// Android checkout's five-part scope and independent of Android types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutScope {
    merchant_id: String,
    store_id: String,
    location_id: String,
    register_id: String,
    device_id: String,
}

impl CheckoutScope {
    /// Make an exact scope. Every part is 1 to 64 ASCII bytes, starts with a
    /// lowercase letter or digit, and otherwise uses only `a-z`, `0-9`, `.`,
    /// `_`, or `-`.
    pub fn new(
        merchant_id: impl Into<String>,
        store_id: impl Into<String>,
        location_id: impl Into<String>,
        register_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> Result<Self, AdoptionError> {
        let scope = Self {
            merchant_id: merchant_id.into(),
            store_id: store_id.into(),
            location_id: location_id.into(),
            register_id: register_id.into(),
            device_id: device_id.into(),
        };
        if scope.parts().into_iter().all(scope_id_is_valid) {
            Ok(scope)
        } else {
            Err(AdoptionError::InvalidScope)
        }
    }

    /// Merchant identifier.
    pub fn merchant_id(&self) -> &str {
        &self.merchant_id
    }

    /// Store identifier.
    pub fn store_id(&self) -> &str {
        &self.store_id
    }

    /// Location identifier.
    pub fn location_id(&self) -> &str {
        &self.location_id
    }

    /// Register identifier.
    pub fn register_id(&self) -> &str {
        &self.register_id
    }

    /// Device identifier.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The five identifiers in canonical order.
    pub fn parts(&self) -> [&str; CHECKOUT_SCOPE_PARTS] {
        [
            &self.merchant_id,
            &self.store_id,
            &self.location_id,
            &self.register_id,
            &self.device_id,
        ]
    }

    fn put(&self, out: &mut Vec<u8>) {
        for part in self.parts() {
            put_scope_id(out, part);
        }
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let merchant_id = read_scope_id(r)?;
        let store_id = read_scope_id(r)?;
        let location_id = read_scope_id(r)?;
        let register_id = read_scope_id(r)?;
        let device_id = read_scope_id(r)?;
        Self::new(merchant_id, store_id, location_id, register_id, device_id)
            .map_err(|_| DecodeError::NonCanonical("checkout adoption scope is invalid"))
    }
}

/// One fixed-name file in an adoption manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionFile {
    name: String,
    present: bool,
    length: u64,
    sha256: [u8; 32],
}

impl AdoptionFile {
    /// Describe a present file and the SHA-256 of its exact stored bytes.
    pub fn present(
        name: impl Into<String>,
        length: u64,
        sha256: [u8; 32],
    ) -> Result<Self, AdoptionError> {
        let file = Self {
            name: name.into(),
            present: true,
            length,
            sha256,
        };
        if ADOPTION_FILE_NAMES.contains(&file.name.as_str()) {
            Ok(file)
        } else {
            Err(AdoptionError::InvalidManifest)
        }
    }

    /// Describe an absent fixed-name file. Absence always has zero length and
    /// a zero digest; a present empty file instead carries SHA-256(empty).
    pub fn absent(name: impl Into<String>) -> Result<Self, AdoptionError> {
        let file = Self {
            name: name.into(),
            present: false,
            length: 0,
            sha256: [0; 32],
        };
        if ADOPTION_FILE_NAMES.contains(&file.name.as_str()) {
            Ok(file)
        } else {
            Err(AdoptionError::InvalidManifest)
        }
    }

    /// Fixed file name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the file existed in the snapshot.
    pub fn is_present(&self) -> bool {
        self.present
    }

    /// Exact byte length; zero for an absent file.
    pub fn length(&self) -> u64 {
        self.length
    }

    /// SHA-256 of the exact bytes; all zero for an absent file.
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// The five fixed business/pairing files and their exact snapshot metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionManifest {
    entries: [AdoptionFile; ADOPTION_FILE_COUNT],
}

impl AdoptionManifest {
    /// Make the manifest. It must contain exactly [`ADOPTION_FILE_NAMES`] in
    /// that order, absent entries must have zero length and digest, and the
    /// pairing file must be present. Automatic adoption without an existing
    /// pairing has no already-pinned owner root.
    pub fn new(entries: [AdoptionFile; ADOPTION_FILE_COUNT]) -> Result<Self, AdoptionError> {
        let manifest = Self { entries };
        if manifest.is_valid() {
            Ok(manifest)
        } else {
            Err(AdoptionError::InvalidManifest)
        }
    }

    /// The five entries in canonical order.
    pub fn entries(&self) -> &[AdoptionFile; ADOPTION_FILE_COUNT] {
        &self.entries
    }

    /// Domain-separated root of the exact canonical manifest body.
    pub fn root(&self) -> [u8; 32] {
        tagged_hash(TAG_CHECKOUT_ADOPTION_MANIFEST, &self.to_bytes())
    }

    fn is_valid(&self) -> bool {
        self.entries[ADOPTION_FILE_COUNT - 1].present
            && self
                .entries
                .iter()
                .zip(ADOPTION_FILE_NAMES)
                .all(|(file, expected_name)| {
                    file.name == expected_name
                        && (file.present || (file.length == 0 && file.sha256 == [0; 32]))
                })
    }

    fn put(&self, out: &mut Vec<u8>) {
        debug_assert!(self.is_valid());
        out.push(ADOPTION_FILE_COUNT as u8);
        for file in &self.entries {
            out.push(u8::try_from(file.name.len()).expect("fixed file name fits u8"));
            out.extend_from_slice(file.name.as_bytes());
            out.push(u8::from(file.present));
            out.extend_from_slice(&file.length.to_le_bytes());
            out.extend_from_slice(&file.sha256);
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(320);
        self.put(&mut out);
        out
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        if usize::from(r.u8()?) != ADOPTION_FILE_COUNT {
            return Err(DecodeError::NonCanonical(
                "checkout adoption manifest does not have five files",
            ));
        }
        let mut entries = Vec::with_capacity(ADOPTION_FILE_COUNT);
        for expected_name in ADOPTION_FILE_NAMES {
            let name_len = usize::from(r.u8()?);
            let raw_name = r.take(name_len)?;
            if raw_name != expected_name.as_bytes() {
                return Err(DecodeError::NonCanonical(
                    "checkout adoption manifest file name or order is wrong",
                ));
            }
            let present = r.flag()?;
            let length = r.u64_le()?;
            let sha256 = r.array()?;
            if !present && (length != 0 || sha256 != [0; 32]) {
                return Err(DecodeError::NonCanonical(
                    "absent checkout adoption file has length or digest",
                ));
            }
            entries.push(AdoptionFile {
                name: expected_name.to_owned(),
                present,
                length,
                sha256,
            });
        }
        let entries: [AdoptionFile; ADOPTION_FILE_COUNT] = entries
            .try_into()
            .map_err(|_| DecodeError::NonCanonical("checkout adoption manifest size is wrong"))?;
        Self::new(entries)
            .map_err(|_| DecodeError::NonCanonical("checkout adoption manifest is invalid"))
    }
}

/// A counter-signed request for one existing-install local-authority adoption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionRequest {
    counter: DeviceId,
    owner: DeviceId,
    scope: CheckoutScope,
    manifest: AdoptionManifest,
    nonce: [u8; 32],
    created_at: u64,
    counter_sig: Sig,
}

impl AdoptionRequest {
    /// Sign a request with the existing counter identity. `owner` must be the
    /// root pinned by the counter's already-verified pairing. `nonce` must be
    /// fresh random bytes supplied by the caller; `created_at` is only a
    /// display/replay aid and is not a substitute for nonce consumption.
    pub fn sign(
        counter: &Identity,
        owner: DeviceId,
        scope: CheckoutScope,
        manifest: AdoptionManifest,
        nonce: [u8; 32],
        created_at: u64,
    ) -> Result<Self, AdoptionError> {
        if nonce == [0; 32] {
            return Err(AdoptionError::ZeroNonce);
        }
        let mut request = Self {
            counter: counter.device_id(),
            owner,
            scope,
            manifest,
            nonce,
            created_at,
            counter_sig: Sig([0; 64]),
        };
        request.counter_sig =
            counter.sign_tagged(TAG_CHECKOUT_ADOPTION_REQUEST, &request.unsigned_bytes());
        Ok(request)
    }

    /// Counter named by the request.
    pub fn counter(&self) -> DeviceId {
        self.counter
    }

    /// Owner root named by the request.
    pub fn owner(&self) -> DeviceId {
        self.owner
    }

    /// Exact checkout authority scope.
    pub fn scope(&self) -> &CheckoutScope {
        &self.scope
    }

    /// Fixed file snapshot.
    pub fn manifest(&self) -> &AdoptionManifest {
        &self.manifest
    }

    /// Manifest root carried and signed by the request.
    pub fn manifest_root(&self) -> [u8; 32] {
        self.manifest.root()
    }

    /// Checkout-generated fresh nonce.
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    /// Caller-supplied Unix time used for display and replay diagnostics.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Counter signature.
    pub fn counter_signature(&self) -> &Sig {
        &self.counter_sig
    }

    fn put_unsigned(&self, out: &mut Vec<u8>) {
        out.push(PURPOSE_EXISTING_INSTALL_LOCAL_AUTHORITY);
        out.extend_from_slice(&self.counter.to_bytes());
        out.extend_from_slice(&self.owner.to_bytes());
        self.scope.put(out);
        self.manifest.put(out);
        out.extend_from_slice(&self.manifest.root());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.created_at.to_le_bytes());
    }

    fn unsigned_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_ADOPTION_REQUEST, 768);
        self.put_unsigned(&mut out);
        out
    }

    /// Verify the request's counter signature and structural invariants.
    pub fn verify(&self) -> Result<(), AdoptionError> {
        if self.nonce == [0; 32] {
            return Err(AdoptionError::ZeroNonce);
        }
        if !self.scope.parts().into_iter().all(scope_id_is_valid) {
            return Err(AdoptionError::InvalidScope);
        }
        if !self.manifest.is_valid() {
            return Err(AdoptionError::InvalidManifest);
        }
        if !self.counter.verify_tagged(
            TAG_CHECKOUT_ADOPTION_REQUEST,
            &self.unsigned_bytes(),
            &self.counter_sig,
        ) {
            return Err(AdoptionError::BadCounterSignature);
        }
        Ok(())
    }

    /// Canonical version-1 wire form.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.unsigned_bytes();
        out.extend_from_slice(&self.counter_sig.0);
        out
    }

    /// Strictly decode the canonical version-1 wire form. This checks all
    /// structural invariants and the manifest root, but [`Self::verify`] must
    /// still be called before the request is trusted.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_ADOPTION_REQUEST)?;
        if r.u8()? != PURPOSE_EXISTING_INSTALL_LOCAL_AUTHORITY {
            return Err(DecodeError::NonCanonical(
                "checkout adoption purpose is unknown",
            ));
        }
        let counter = r.device_id()?;
        let owner = r.device_id()?;
        let scope = CheckoutScope::read(&mut r)?;
        let manifest = AdoptionManifest::read(&mut r)?;
        let encoded_root: [u8; 32] = r.array()?;
        if encoded_root != manifest.root() {
            return Err(DecodeError::NonCanonical(
                "checkout adoption manifest root does not match",
            ));
        }
        let nonce = r.array()?;
        if nonce == [0; 32] {
            return Err(DecodeError::NonCanonical(
                "checkout adoption nonce is all zero",
            ));
        }
        let created_at = r.u64_le()?;
        let counter_sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            counter,
            owner,
            scope,
            manifest,
            nonce,
            created_at,
            counter_sig,
        })
    }
}

/// The owner root's approval of an entire counter-signed adoption request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedAdoptionGrant {
    request: AdoptionRequest,
    owner_sig: Sig,
}

impl SignedAdoptionGrant {
    /// Verify the counter request and sign its complete canonical bytes with
    /// the exact owner root named by it.
    pub fn sign(request: AdoptionRequest, owner: &Identity) -> Result<Self, AdoptionError> {
        request.verify()?;
        if owner.device_id() != request.owner {
            return Err(AdoptionError::OwnerMismatch);
        }
        let owner_sig = owner.sign_tagged(TAG_CHECKOUT_ADOPTION_GRANT, &request.to_bytes());
        Ok(Self { request, owner_sig })
    }

    /// The approved request.
    pub fn request(&self) -> &AdoptionRequest {
        &self.request
    }

    /// Owner signature.
    pub fn owner_signature(&self) -> &Sig {
        &self.owner_sig
    }

    /// Verify both signatures using the identities named by the request.
    pub fn verify(&self) -> Result<(), AdoptionError> {
        self.request.verify()?;
        if !self.request.owner.verify_tagged(
            TAG_CHECKOUT_ADOPTION_GRANT,
            &self.request.to_bytes(),
            &self.owner_sig,
        ) {
            return Err(AdoptionError::BadOwnerSignature);
        }
        Ok(())
    }

    /// Verify both signatures and bind the grant to the caller's already
    /// trusted pairing, pending checkout scope, manifest snapshot, and nonce.
    pub fn verify_exact(
        &self,
        expected_owner: &DeviceId,
        expected_counter: &DeviceId,
        expected_scope: &CheckoutScope,
        expected_manifest_root: &[u8; 32],
        expected_nonce: &[u8; 32],
    ) -> Result<(), AdoptionError> {
        if self.request.owner != *expected_owner {
            return Err(AdoptionError::OwnerMismatch);
        }
        if self.request.counter != *expected_counter {
            return Err(AdoptionError::CounterMismatch);
        }
        if self.request.scope != *expected_scope {
            return Err(AdoptionError::ScopeMismatch);
        }
        if self.request.manifest.root() != *expected_manifest_root {
            return Err(AdoptionError::ManifestMismatch);
        }
        if self.request.nonce != *expected_nonce {
            return Err(AdoptionError::NonceMismatch);
        }
        self.verify()
    }

    /// Canonical version-1 wire form.
    pub fn to_bytes(&self) -> Vec<u8> {
        let request = self.request.to_bytes();
        let mut out = wire::start(&wire::MAGIC_ADOPTION_GRANT, request.len() + 66);
        out.extend_from_slice(
            &u16::try_from(request.len())
                .expect("adoption request fits its u16 wire length")
                .to_le_bytes(),
        );
        out.extend_from_slice(&request);
        out.extend_from_slice(&self.owner_sig.0);
        out
    }

    /// Strictly decode the canonical version-1 wire form. This does not check
    /// either signature; call [`Self::verify`] or [`Self::verify_exact`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_ADOPTION_GRANT)?;
        let request_len = usize::from(r.u16_le()?);
        if request_len > MAX_ADOPTION_REQUEST_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let request_bytes = r.take(request_len)?;
        let request = AdoptionRequest::from_bytes(request_bytes)?;
        if request.to_bytes() != request_bytes {
            return Err(DecodeError::NonCanonical(
                "embedded checkout adoption request is not canonical",
            ));
        }
        let owner_sig = r.sig()?;
        r.finish()?;
        Ok(Self { request, owner_sig })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{assert_strict_decoding, identity};

    fn scope() -> CheckoutScope {
        CheckoutScope::new(
            "merchant-7",
            "store_2",
            "location.3",
            "register-4",
            "device_5",
        )
        .unwrap()
    }

    fn manifest() -> AdoptionManifest {
        AdoptionManifest::new([
            AdoptionFile::present(ADOPTION_FILE_NAMES[0], 41, [1; 32]).unwrap(),
            AdoptionFile::present(ADOPTION_FILE_NAMES[1], 0, [2; 32]).unwrap(),
            AdoptionFile::absent(ADOPTION_FILE_NAMES[2]).unwrap(),
            AdoptionFile::present(ADOPTION_FILE_NAMES[3], 91, [4; 32]).unwrap(),
            AdoptionFile::present(ADOPTION_FILE_NAMES[4], 111, [5; 32]).unwrap(),
        ])
        .unwrap()
    }

    fn request() -> AdoptionRequest {
        AdoptionRequest::sign(
            &identity(1),
            identity(2).device_id(),
            scope(),
            manifest(),
            [9; 32],
            1_800_000_000,
        )
        .unwrap()
    }

    #[test]
    fn request_and_grant_round_trip_strictly_and_verify_exactly() {
        let request = request();
        request.verify().unwrap();
        let request_bytes = request.to_bytes();
        assert_eq!(&request_bytes[..5], b"OBAR\x01");
        assert_eq!(
            AdoptionRequest::from_bytes(&request_bytes).unwrap(),
            request
        );
        assert_strict_decoding(
            &request_bytes,
            AdoptionRequest::from_bytes,
            AdoptionRequest::to_bytes,
        );

        let owner = identity(2);
        let grant = SignedAdoptionGrant::sign(request.clone(), &owner).unwrap();
        grant.verify().unwrap();
        grant
            .verify_exact(
                &owner.device_id(),
                &identity(1).device_id(),
                request.scope(),
                &request.manifest_root(),
                request.nonce(),
            )
            .unwrap();
        let grant_bytes = grant.to_bytes();
        assert_eq!(&grant_bytes[..5], b"OBAG\x01");
        assert_eq!(
            SignedAdoptionGrant::from_bytes(&grant_bytes).unwrap(),
            grant
        );
        assert_strict_decoding(
            &grant_bytes,
            SignedAdoptionGrant::from_bytes,
            SignedAdoptionGrant::to_bytes,
        );
    }

    #[test]
    fn scope_uses_the_exact_android_length_and_alphabet() {
        let make = |merchant: &str| CheckoutScope::new(merchant, "s", "l", "r", "d");
        assert!(make("a").is_ok());
        assert!(make(&"a".repeat(64)).is_ok());
        for bad in ["", "-merchant", "Merchant", "merchant/7", "merchant 7", "é"] {
            assert_eq!(make(bad), Err(AdoptionError::InvalidScope));
        }
        assert_eq!(make(&"a".repeat(65)), Err(AdoptionError::InvalidScope));
    }

    #[test]
    fn manifest_is_fixed_and_distinguishes_absent_from_empty() {
        let m = manifest();
        assert!(!m.entries()[2].is_present());
        assert_eq!(m.entries()[2].length(), 0);
        assert_eq!(m.entries()[2].sha256(), &[0; 32]);
        assert!(m.entries()[1].is_present());
        assert_eq!(m.entries()[1].length(), 0);
        assert_ne!(m.entries()[1].sha256(), &[0; 32]);

        let mut wrong_order = m.entries.clone();
        wrong_order.swap(0, 1);
        assert_eq!(
            AdoptionManifest::new(wrong_order),
            Err(AdoptionError::InvalidManifest)
        );
        let mut malformed_absent = m.entries.clone();
        malformed_absent[2].length = 1;
        assert_eq!(
            AdoptionManifest::new(malformed_absent),
            Err(AdoptionError::InvalidManifest)
        );
        let mut missing_pairing = m.entries.clone();
        missing_pairing[4] = AdoptionFile::absent(ADOPTION_FILE_NAMES[4]).unwrap();
        assert_eq!(
            AdoptionManifest::new(missing_pairing),
            Err(AdoptionError::InvalidManifest)
        );
        assert_eq!(
            AdoptionFile::present("other.bin", 1, [1; 32]),
            Err(AdoptionError::InvalidManifest)
        );
    }

    #[test]
    fn zero_nonce_and_wrong_signers_are_refused() {
        assert_eq!(
            AdoptionRequest::sign(
                &identity(1),
                identity(2).device_id(),
                scope(),
                manifest(),
                [0; 32],
                1,
            ),
            Err(AdoptionError::ZeroNonce)
        );
        assert_eq!(
            SignedAdoptionGrant::sign(request(), &identity(3)),
            Err(AdoptionError::OwnerMismatch)
        );

        let mut bad_request = request();
        bad_request.nonce[0] ^= 1;
        assert_eq!(
            bad_request.verify(),
            Err(AdoptionError::BadCounterSignature)
        );

        let mut bad_grant = SignedAdoptionGrant::sign(request(), &identity(2)).unwrap();
        bad_grant.owner_sig.0[0] ^= 1;
        assert_eq!(bad_grant.verify(), Err(AdoptionError::BadOwnerSignature));
    }

    #[test]
    fn request_and_grant_signature_domains_cannot_be_crossed() {
        let counter = identity(1);
        let mut wrong_request = request();
        wrong_request.counter_sig =
            counter.sign_tagged(TAG_CHECKOUT_ADOPTION_GRANT, &wrong_request.unsigned_bytes());
        assert_eq!(
            wrong_request.verify(),
            Err(AdoptionError::BadCounterSignature)
        );

        let owner = identity(2);
        let mut wrong_grant = SignedAdoptionGrant::sign(request(), &owner).unwrap();
        wrong_grant.owner_sig = owner.sign_tagged(
            TAG_CHECKOUT_ADOPTION_REQUEST,
            &wrong_grant.request.to_bytes(),
        );
        assert_eq!(wrong_grant.verify(), Err(AdoptionError::BadOwnerSignature));
    }

    #[test]
    fn exact_verification_rejects_every_changed_binding() {
        let request = request();
        let grant = SignedAdoptionGrant::sign(request.clone(), &identity(2)).unwrap();
        let owner = request.owner();
        let counter = request.counter();
        let scope = request.scope().clone();
        let root = request.manifest_root();
        let nonce = *request.nonce();

        assert_eq!(
            grant.verify_exact(&identity(3).device_id(), &counter, &scope, &root, &nonce),
            Err(AdoptionError::OwnerMismatch)
        );
        assert_eq!(
            grant.verify_exact(&owner, &identity(3).device_id(), &scope, &root, &nonce),
            Err(AdoptionError::CounterMismatch)
        );
        assert_eq!(
            grant.verify_exact(
                &owner,
                &counter,
                &CheckoutScope::new("x", "s", "l", "r", "d").unwrap(),
                &root,
                &nonce
            ),
            Err(AdoptionError::ScopeMismatch)
        );
        let mut other_root = root;
        other_root[0] ^= 1;
        assert_eq!(
            grant.verify_exact(&owner, &counter, &scope, &other_root, &nonce),
            Err(AdoptionError::ManifestMismatch)
        );
        let mut other_nonce = nonce;
        other_nonce[0] ^= 1;
        assert_eq!(
            grant.verify_exact(&owner, &counter, &scope, &root, &other_nonce),
            Err(AdoptionError::NonceMismatch)
        );
    }

    #[test]
    fn decoder_rejects_root_nonce_manifest_and_purpose_mutations() {
        let bytes = request().to_bytes();

        let mut bad_purpose = bytes.clone();
        bad_purpose[5] = 2;
        assert!(matches!(
            AdoptionRequest::from_bytes(&bad_purpose),
            Err(DecodeError::NonCanonical(_))
        ));

        let root = request().manifest_root();
        let root_offset = bytes
            .windows(root.len())
            .position(|window| window == root)
            .expect("manifest root is present");
        let mut bad_root = bytes.clone();
        bad_root[root_offset] ^= 1;
        assert!(matches!(
            AdoptionRequest::from_bytes(&bad_root),
            Err(DecodeError::NonCanonical(_))
        ));

        let mut zero_nonce = bytes.clone();
        let nonce_offset = bytes.len() - 64 - 8 - 32;
        zero_nonce[nonce_offset..nonce_offset + 32].fill(0);
        assert!(matches!(
            AdoptionRequest::from_bytes(&zero_nonce),
            Err(DecodeError::NonCanonical(_))
        ));

        let manifest_count_offset =
            5 + 1 + 32 + 32 + scope().parts().iter().map(|p| 1 + p.len()).sum::<usize>();
        let mut wrong_count = bytes;
        wrong_count[manifest_count_offset] = 4;
        assert!(matches!(
            AdoptionRequest::from_bytes(&wrong_count),
            Err(DecodeError::NonCanonical(_))
        ));
    }
}
