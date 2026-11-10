//! Address batches for a counter.
//!
//! A counter never sees the wallet description. It holds batches: a
//! contiguous range of receive addresses, derived by Owner mode and signed
//! twice, once by the owner root and once by the Key app root. The counter
//! pinned both roots when it was paired and accepts a batch only with both
//! signatures. The Key app signs only after deriving the same range from its
//! own copy of the wallet and finding the same hash (§5.1).
//!
//! Hash and signatures cover the canonical bytes below, so a changed script,
//! range, counter or wallet gives a different hash and both signatures fail.
//!
//! ```text
//! wallet_checksum (32) | counter (32) | start u32 LE | count u32 LE
//!   | for each script: len u16 LE | script bytes
//! ```

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::fmt;

use bitcoin::ScriptBuf;

use crate::identity::{tagged_hash, DeviceId, Identity, Sig, TAG_BATCH};
use crate::wallet::{WalletDescription, WalletError};

/// Default smallest batch: about 8 weeks of sales, at least 1,000 addresses.
pub const DEFAULT_MIN_COUNT: u32 = 1_000;
/// Hard upper bound on one batch, to bound memory and message size.
pub const MAX_COUNT: u32 = 100_000;
/// Indexes at or above this are hardened and cannot be derived from an xpub.
const INDEX_LIMIT: u32 = 1 << 31;

/// Rules for how big a batch must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchPolicy {
    /// Fewest addresses a batch may hold.
    pub min_count: u32,
}

impl Default for BatchPolicy {
    fn default() -> Self {
        Self {
            min_count: DEFAULT_MIN_COUNT,
        }
    }
}

/// Why a batch was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchError {
    /// Fewer addresses than the policy minimum.
    CountTooSmall { min: u32, got: u32 },
    /// More addresses than [`MAX_COUNT`].
    CountTooLarge,
    /// The range reaches hardened indexes or overflows.
    RangeOutOfBounds,
    /// `scripts.len()` is not `count`.
    ScriptCountMismatch,
    /// A script appears twice in the batch or was already issued.
    DuplicateScript,
    /// A script is not a P2WSH output.
    UnexpectedScriptType,
    /// The owner root did not sign this hash.
    OwnerSignatureInvalid,
    /// The Key app root did not sign this hash.
    ApproverSignatureInvalid,
    /// The batch is for another counter.
    WrongCounter,
    /// The batch is for another wallet.
    WrongWallet,
    /// The batch is for a counter that is not on the Key app's device list.
    UnknownCounter,
    /// The range overlaps one already issued, active or retired.
    OverlapsExisting,
    /// Owner and Key app roots must be different keys.
    SameRoot,
    /// The Key app derived different scripts than the batch holds.
    HashMismatch,
    /// The range starts above the high-water mark: addresses would be skipped.
    Gap { expected: u32, found: u32 },
    /// The range starts below the high-water mark: addresses would repeat.
    Overlap { expected: u32, found: u32 },
    /// Address derivation failed.
    Wallet(WalletError),
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountTooSmall { min, got } => write!(f, "batch has {got} addresses, needs {min}"),
            Self::CountTooLarge => write!(f, "batch is too large"),
            Self::RangeOutOfBounds => write!(f, "address range out of bounds"),
            Self::ScriptCountMismatch => write!(f, "batch count does not match its scripts"),
            Self::DuplicateScript => write!(f, "batch repeats an address"),
            Self::UnexpectedScriptType => write!(f, "batch holds a non-P2WSH script"),
            Self::OwnerSignatureInvalid => write!(f, "owner signature missing or wrong"),
            Self::ApproverSignatureInvalid => write!(f, "Key app signature missing or wrong"),
            Self::WrongCounter => write!(f, "batch is for another counter"),
            Self::WrongWallet => write!(f, "batch is for another wallet"),
            Self::UnknownCounter => {
                write!(f, "batch is for a counter that is not on the device list")
            }
            Self::OverlapsExisting => write!(f, "batch overlaps addresses already issued"),
            Self::SameRoot => write!(f, "owner and Key app roots must differ"),
            Self::HashMismatch => write!(f, "addresses differ from the Key app's own derivation"),
            Self::Gap { expected, found } => {
                write!(
                    f,
                    "batch starts at {found}, expected {expected}: addresses skipped"
                )
            }
            Self::Overlap { expected, found } => {
                write!(
                    f,
                    "batch starts at {found}, expected {expected}: addresses repeated"
                )
            }
            Self::Wallet(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BatchError {}

impl From<WalletError> for BatchError {
    fn from(e: WalletError) -> Self {
        Self::Wallet(e)
    }
}

/// The two roots a counter pinned at pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedRoots {
    /// Owner phone root.
    pub owner: DeviceId,
    /// Key app root.
    pub approver: DeviceId,
}

impl PinnedRoots {
    /// Pin two roots; they must be different keys.
    pub fn new(owner: DeviceId, approver: DeviceId) -> Result<Self, BatchError> {
        if owner == approver {
            return Err(BatchError::SameRoot);
        }
        Ok(Self { owner, approver })
    }
}

/// A contiguous range of receive addresses for one counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressBatch {
    /// SHA-256 of the wallet's canonical string.
    pub wallet_checksum: [u8; 32],
    /// The counter this batch is for.
    pub counter: DeviceId,
    /// First receive index.
    pub start: u32,
    /// Number of addresses.
    pub count: u32,
    /// The receive scripts for `start..start + count`, in order.
    pub scripts: Vec<ScriptBuf>,
}

impl AddressBatch {
    /// The canonical bytes that are hashed and signed.
    ///
    /// Every script must be a 34-byte P2WSH script, which `check_shape`
    /// enforces; every path that encodes a batch it did not build itself runs
    /// `check_shape` first. Calling this directly on a hand-built batch with
    /// a script longer than `u16::MAX` bytes panics rather than truncating
    /// the length.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(72 + self.scripts.len() * 36);
        out.extend_from_slice(&self.wallet_checksum);
        out.extend_from_slice(&self.counter.to_bytes());
        out.extend_from_slice(&self.start.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        for s in &self.scripts {
            let b = s.as_bytes();
            let len = u16::try_from(b.len()).expect("script length checked by check_shape");
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(b);
        }
        out
    }

    /// Tagged hash (`OpenBMT/batch/v1`) of the canonical bytes. This is what
    /// both roots sign.
    pub fn hash(&self) -> [u8; 32] {
        tagged_hash(TAG_BATCH, &self.encode())
    }

    /// One past the last index, or `None` if the range is out of bounds.
    pub fn end(&self) -> Option<u32> {
        let end = self.start.checked_add(self.count)?;
        (end <= INDEX_LIMIT).then_some(end)
    }

    /// Sign this batch with `identity` (owner or Key app).
    pub fn sign(&self, identity: &Identity) -> Sig {
        identity.sign_digest(&self.hash())
    }

    fn check_shape(&self, policy: &BatchPolicy) -> Result<(), BatchError> {
        if self.count < policy.min_count {
            return Err(BatchError::CountTooSmall {
                min: policy.min_count,
                got: self.count,
            });
        }
        if self.count > MAX_COUNT {
            return Err(BatchError::CountTooLarge);
        }
        self.end().ok_or(BatchError::RangeOutOfBounds)?;
        if self.scripts.len() != self.count as usize {
            return Err(BatchError::ScriptCountMismatch);
        }
        let mut seen = HashSet::with_capacity(self.scripts.len());
        for s in &self.scripts {
            if !s.is_p2wsh() {
                return Err(BatchError::UnexpectedScriptType);
            }
            if !seen.insert(s) {
                return Err(BatchError::DuplicateScript);
            }
        }
        Ok(())
    }
}

/// Derives batches from a wallet.
#[derive(Debug, Clone, Copy, Default)]
pub struct BatchBuilder {
    policy: BatchPolicy,
}

impl BatchBuilder {
    /// A builder that enforces `policy`.
    pub fn new(policy: BatchPolicy) -> Self {
        Self { policy }
    }

    /// Derive `count` receive scripts starting at `start` for `counter`.
    pub fn derive(
        &self,
        wallet: &WalletDescription,
        counter: DeviceId,
        start: u32,
        count: u32,
    ) -> Result<AddressBatch, BatchError> {
        if count < self.policy.min_count {
            return Err(BatchError::CountTooSmall {
                min: self.policy.min_count,
                got: count,
            });
        }
        if count > MAX_COUNT {
            return Err(BatchError::CountTooLarge);
        }
        let end = start
            .checked_add(count)
            .ok_or(BatchError::RangeOutOfBounds)?;
        if end > INDEX_LIMIT {
            return Err(BatchError::RangeOutOfBounds);
        }
        let scripts = (start..end)
            .map(|i| wallet.receive_script(i))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AddressBatch {
            wallet_checksum: wallet.checksum32(),
            counter,
            start,
            count,
            scripts,
        })
    }
}

/// A batch with both signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedBatch {
    /// The batch.
    pub batch: AddressBatch,
    /// Owner root signature over `batch.hash()`.
    pub owner_sig: Sig,
    /// Key app root signature over `batch.hash()`.
    pub approver_sig: Sig,
}

impl SignedBatch {
    /// Check shape and both signatures against the pinned roots. One
    /// signature, the wrong signer, or changed scripts all fail.
    pub fn verify(&self, roots: &PinnedRoots, policy: &BatchPolicy) -> Result<(), BatchError> {
        self.batch.check_shape(policy)?;
        let h = self.batch.hash();
        if !roots.owner.verify_digest(&h, &self.owner_sig) {
            return Err(BatchError::OwnerSignatureInvalid);
        }
        if !roots.approver.verify_digest(&h, &self.approver_sig) {
            return Err(BatchError::ApproverSignatureInvalid);
        }
        Ok(())
    }
}

/// The Key app's monotonic mark: the first index it has not yet signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HighWater(u32);

impl HighWater {
    /// A fresh mark: nothing signed yet.
    pub fn new() -> Self {
        Self(0)
    }

    /// Restore a stored mark.
    pub fn from_next_index(next: u32) -> Self {
        Self(next)
    }

    /// The only `start` the next batch may have.
    pub fn next_index(&self) -> u32 {
        self.0
    }
}

/// The Key app's batch check (§5.1), as a pure function over its own state.
pub struct Approver {
    identity: Identity,
    builder: BatchBuilder,
}

impl Approver {
    /// An approver that signs with `identity` and enforces `policy`.
    pub fn new(identity: Identity, policy: BatchPolicy) -> Self {
        Self {
            identity,
            builder: BatchBuilder::new(policy),
        }
    }

    /// This approver's public root.
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// Check the batch is for a counter in `approved_counters` (the Key app's
    /// device list), re-derive its range from `own_wallet`, compare the hash,
    /// check the range starts exactly at the high-water mark (no gap, no
    /// overlap), then co-sign and advance the mark. On any failure nothing is
    /// signed and the mark does not move.
    pub fn check_and_cosign(
        &self,
        own_wallet: &WalletDescription,
        batch: &AddressBatch,
        high_water: &mut HighWater,
        approved_counters: &BTreeSet<DeviceId>,
    ) -> Result<Sig, BatchError> {
        if batch.wallet_checksum != own_wallet.checksum32() {
            return Err(BatchError::WrongWallet);
        }
        if !approved_counters.contains(&batch.counter) {
            return Err(BatchError::UnknownCounter);
        }
        let end = batch.end().ok_or(BatchError::RangeOutOfBounds)?;
        match batch.start.cmp(&high_water.0) {
            std::cmp::Ordering::Greater => {
                return Err(BatchError::Gap {
                    expected: high_water.0,
                    found: batch.start,
                })
            }
            std::cmp::Ordering::Less => {
                return Err(BatchError::Overlap {
                    expected: high_water.0,
                    found: batch.start,
                })
            }
            std::cmp::Ordering::Equal => {}
        }
        // `batch` is untrusted: its scripts must pass the shape check before
        // they are encoded and hashed, and before any derivation work is done.
        batch.check_shape(&self.builder.policy)?;
        let own = self
            .builder
            .derive(own_wallet, batch.counter, batch.start, batch.count)?;
        if own.hash() != batch.hash() {
            return Err(BatchError::HashMismatch);
        }
        let sig = self.identity.sign_digest(&own.hash());
        high_water.0 = end;
        Ok(sig)
    }
}

#[derive(Debug)]
struct ActiveBatch {
    start: u32,
    scripts: Vec<ScriptBuf>,
    next: usize,
}

/// The counter's verified batches and its `next_address` state.
#[derive(Debug)]
pub struct CounterBatches {
    counter: DeviceId,
    roots: PinnedRoots,
    wallet_checksum: [u8; 32],
    policy: BatchPolicy,
    active: VecDeque<ActiveBatch>,
    ranges: Vec<(u32, u32)>,
    known: HashSet<ScriptBuf>,
    watched: Vec<ScriptBuf>,
}

impl CounterBatches {
    /// State for `counter`, with the roots and wallet checksum pinned at
    /// pairing.
    pub fn new(
        counter: DeviceId,
        roots: PinnedRoots,
        wallet_checksum: [u8; 32],
        policy: BatchPolicy,
    ) -> Self {
        Self {
            counter,
            roots,
            wallet_checksum,
            policy,
            active: VecDeque::new(),
            ranges: Vec::new(),
            known: HashSet::new(),
            watched: Vec::new(),
        }
    }

    /// Verify and store a batch.
    pub fn add(&mut self, signed: SignedBatch) -> Result<(), BatchError> {
        if signed.batch.counter != self.counter {
            return Err(BatchError::WrongCounter);
        }
        if signed.batch.wallet_checksum != self.wallet_checksum {
            return Err(BatchError::WrongWallet);
        }
        signed.verify(&self.roots, &self.policy)?;
        let start = signed.batch.start;
        let end = signed.batch.end().ok_or(BatchError::RangeOutOfBounds)?;
        if self.ranges.iter().any(|&(s, e)| start < e && s < end) {
            return Err(BatchError::OverlapsExisting);
        }
        if signed.batch.scripts.iter().any(|s| self.known.contains(s)) {
            return Err(BatchError::DuplicateScript);
        }
        self.known.extend(signed.batch.scripts.iter().cloned());
        self.ranges.push((start, end));
        let pos = self.active.partition_point(|b| b.start < start);
        self.active.insert(
            pos,
            ActiveBatch {
                start,
                scripts: signed.batch.scripts,
                next: 0,
            },
        );
        Ok(())
    }

    /// The next unused script and its index. Each script is returned once.
    /// `None` means the range is empty: the caller must not offer Bitcoin.
    pub fn next_script(&mut self) -> Option<(u32, ScriptBuf)> {
        loop {
            let front = self.active.front_mut()?;
            if front.next < front.scripts.len() {
                let i = front.start + front.next as u32;
                let s = front.scripts[front.next].clone();
                front.next += 1;
                self.watched.push(s.clone());
                return Some((i, s));
            }
            self.active.pop_front();
        }
    }

    /// How many unused scripts are left.
    pub fn remaining(&self) -> usize {
        self.active.iter().map(|b| b.scripts.len() - b.next).sum()
    }

    /// Retire every unused script (after a reset or restore). They are
    /// still watched, but never handed out, and their index range stays
    /// blocked so a fresh batch must cover new indexes. Returns how many
    /// were retired.
    pub fn retire_unused(&mut self) -> usize {
        let mut n = 0;
        for b in self.active.drain(..) {
            n += b.scripts.len() - b.next;
            self.watched.extend(b.scripts.into_iter().skip(b.next));
        }
        n
    }

    /// Every script that was shown or retired. These stay watched forever.
    pub fn watched_scripts(&self) -> &[ScriptBuf] {
        &self.watched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{identity, wallet};
    use bitcoin::Network;

    const SMALL: BatchPolicy = BatchPolicy { min_count: 3 };

    struct Fx {
        wallet: WalletDescription,
        owner: Identity,
        key_app: Approver,
        counter: DeviceId,
        roots: PinnedRoots,
        approved: BTreeSet<DeviceId>,
    }

    fn fx() -> Fx {
        let owner = identity(10);
        let key_app = Approver::new(identity(11), SMALL);
        let counter = identity(12).device_id();
        let roots = PinnedRoots::new(owner.device_id(), key_app.device_id()).unwrap();
        Fx {
            wallet: wallet(Network::Testnet),
            owner,
            key_app,
            counter,
            roots,
            approved: BTreeSet::from([counter]),
        }
    }

    fn signed(f: &Fx, start: u32, count: u32, hw: &mut HighWater) -> SignedBatch {
        let batch = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, start, count)
            .unwrap();
        let approver_sig = f
            .key_app
            .check_and_cosign(&f.wallet, &batch, hw, &f.approved)
            .unwrap();
        SignedBatch {
            owner_sig: batch.sign(&f.owner),
            approver_sig,
            batch,
        }
    }

    fn counter_state(f: &Fx) -> CounterBatches {
        CounterBatches::new(f.counter, f.roots, f.wallet.checksum32(), SMALL)
    }

    #[test]
    fn default_policy_minimum_is_1000() {
        assert_eq!(BatchPolicy::default().min_count, 1_000);
        let f = fx();
        let b = BatchBuilder::default();
        assert_eq!(
            b.derive(&f.wallet, f.counter, 0, 999).unwrap_err(),
            BatchError::CountTooSmall {
                min: 1000,
                got: 999
            }
        );
    }

    #[test]
    fn default_policy_thousand_address_batch_end_to_end() {
        let f = fx();
        let approver = Approver::new(identity(11), BatchPolicy::default());
        let batch = BatchBuilder::default()
            .derive(&f.wallet, f.counter, 0, 1_000)
            .unwrap();
        assert_eq!(batch.scripts.len(), 1_000);
        let mut hw = HighWater::new();
        let sig = approver
            .check_and_cosign(&f.wallet, &batch, &mut hw, &f.approved)
            .unwrap();
        assert_eq!(hw.next_index(), 1_000);
        let sb = SignedBatch {
            owner_sig: batch.sign(&f.owner),
            approver_sig: sig,
            batch,
        };
        sb.verify(&f.roots, &BatchPolicy::default()).unwrap();
    }

    #[test]
    fn builder_matches_wallet_scripts() {
        let f = fx();
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 7, 5)
            .unwrap();
        assert_eq!(b.start, 7);
        assert_eq!(b.count, 5);
        for (k, s) in b.scripts.iter().enumerate() {
            assert_eq!(*s, f.wallet.receive_script(7 + k as u32).unwrap());
        }
        assert_eq!(b.wallet_checksum, f.wallet.checksum32());
    }

    #[test]
    fn builder_rejects_bad_ranges() {
        let f = fx();
        let b = BatchBuilder::new(SMALL);
        assert!(matches!(
            b.derive(&f.wallet, f.counter, 0, 2),
            Err(BatchError::CountTooSmall { .. })
        ));
        assert_eq!(
            b.derive(&f.wallet, f.counter, 0, MAX_COUNT + 1)
                .unwrap_err(),
            BatchError::CountTooLarge
        );
        assert_eq!(
            b.derive(&f.wallet, f.counter, u32::MAX - 1, 5).unwrap_err(),
            BatchError::RangeOutOfBounds
        );
        assert_eq!(
            b.derive(&f.wallet, f.counter, (1 << 31) - 2, 5)
                .unwrap_err(),
            BatchError::RangeOutOfBounds
        );
    }

    #[test]
    fn hash_covers_every_field() {
        let f = fx();
        let base = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        let h = base.hash();
        assert_eq!(h, base.clone().hash());
        let mut m = base.clone();
        m.counter = identity(99).device_id();
        assert_ne!(m.hash(), h);
        let mut m = base.clone();
        m.wallet_checksum[0] ^= 1;
        assert_ne!(m.hash(), h);
        let mut m = base.clone();
        m.start += 1;
        assert_ne!(m.hash(), h);
        let mut m = base.clone();
        m.scripts.swap(0, 1);
        assert_ne!(m.hash(), h);
        let mut m = base;
        m.count += 1;
        assert_ne!(m.hash(), h);
    }

    #[test]
    fn two_valid_signatures_verify() {
        let f = fx();
        let sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.verify(&f.roots, &SMALL).unwrap();
    }

    #[test]
    fn one_signature_is_rejected() {
        let f = fx();
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.approver_sig = Sig([0; 64]);
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::ApproverSignatureInvalid)
        );
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.owner_sig = Sig([0; 64]);
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn owner_signing_twice_is_rejected() {
        let f = fx();
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.approver_sig = sb.batch.sign(&f.owner);
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::ApproverSignatureInvalid)
        );
    }

    #[test]
    fn swapped_or_unpinned_signers_are_rejected() {
        let f = fx();
        let sb = signed(&f, 0, 3, &mut HighWater::new());
        let swapped = SignedBatch {
            owner_sig: sb.approver_sig,
            approver_sig: sb.owner_sig,
            batch: sb.batch.clone(),
        };
        assert_eq!(
            swapped.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
        let rogue = identity(77);
        let forged = SignedBatch {
            owner_sig: sb.batch.sign(&rogue),
            approver_sig: sb.approver_sig,
            batch: sb.batch,
        };
        assert_eq!(
            forged.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn tampered_scripts_are_rejected() {
        let f = fx();
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        // an attacker's address, same type
        sb.batch.scripts[1] = f.wallet.receive_script(500).unwrap();
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn shape_errors_are_rejected_even_when_signed() {
        let f = fx();
        let make = |edit: &dyn Fn(&mut AddressBatch)| {
            let mut sb = signed(&f, 0, 3, &mut HighWater::new());
            edit(&mut sb.batch);
            sb.owner_sig = sb.batch.sign(&f.owner);
            sb.approver_sig = sb.batch.sign(&f.key_app.identity);
            sb.verify(&f.roots, &SMALL)
        };
        assert_eq!(make(&|b| b.count = 4), Err(BatchError::ScriptCountMismatch));
        assert_eq!(
            make(&|b| b.scripts[2] = b.scripts[0].clone()),
            Err(BatchError::DuplicateScript)
        );
        assert_eq!(
            make(&|b| b.scripts[0] = ScriptBuf::new()),
            Err(BatchError::UnexpectedScriptType)
        );
        assert!(matches!(
            make(&|b| {
                b.scripts.truncate(2);
                b.count = 2
            }),
            Err(BatchError::CountTooSmall { .. })
        ));
        assert_eq!(
            make(&|b| b.start = u32::MAX),
            Err(BatchError::RangeOutOfBounds)
        );
    }

    #[test]
    fn policy_minimum_applies_when_verifying() {
        let f = fx();
        let sb = signed(&f, 0, 3, &mut HighWater::new());
        assert!(matches!(
            sb.verify(&f.roots, &BatchPolicy::default()),
            Err(BatchError::CountTooSmall { min: 1000, got: 3 })
        ));
    }

    #[test]
    fn pinned_roots_must_differ() {
        let a = identity(1).device_id();
        assert_eq!(PinnedRoots::new(a, a).unwrap_err(), BatchError::SameRoot);
    }

    #[test]
    fn approver_advances_high_water_contiguously() {
        let f = fx();
        let mut hw = HighWater::new();
        signed(&f, 0, 3, &mut hw);
        assert_eq!(hw.next_index(), 3);
        signed(&f, 3, 4, &mut hw);
        assert_eq!(hw.next_index(), 7);
    }

    #[test]
    fn approver_rejects_gap_and_overlap_without_moving_the_mark() {
        let f = fx();
        let mut hw = HighWater::from_next_index(10);
        let mk = |start| {
            BatchBuilder::new(SMALL)
                .derive(&f.wallet, f.counter, start, 3)
                .unwrap()
        };
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &mk(11), &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::Gap {
                expected: 10,
                found: 11
            }
        );
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &mk(9), &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::Overlap {
                expected: 10,
                found: 9
            }
        );
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &mk(0), &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::Overlap {
                expected: 10,
                found: 0
            }
        );
        assert_eq!(hw.next_index(), 10);
        // the same range cannot be signed twice
        f.key_app
            .check_and_cosign(&f.wallet, &mk(10), &mut hw, &f.approved)
            .unwrap();
        assert!(f
            .key_app
            .check_and_cosign(&f.wallet, &mk(10), &mut hw, &f.approved)
            .is_err());
        assert_eq!(hw.next_index(), 13);
    }

    #[test]
    fn approver_rejects_attacker_scripts() {
        let f = fx();
        let mut hw = HighWater::new();
        let mut b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        b.scripts[1] = f.wallet.change_script(1).unwrap();
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::HashMismatch
        );
        assert_eq!(hw.next_index(), 0);
    }

    #[test]
    fn approver_rejects_other_wallet_and_hardened_range() {
        let f = fx();
        let mut hw = HighWater::new();
        let other = crate::wallet::WalletDescription::new(
            Network::Testnet,
            [
                crate::testutil::slot_from_mnemonic(
                    crate::testutil::MNEMONICS[1],
                    1,
                    Network::Testnet,
                ),
                crate::testutil::slot_from_mnemonic(
                    crate::testutil::MNEMONICS[2],
                    2,
                    Network::Testnet,
                ),
                crate::testutil::slot_from_mnemonic(
                    crate::testutil::MNEMONICS[3],
                    3,
                    Network::Testnet,
                ),
            ],
        )
        .unwrap();
        let b = BatchBuilder::new(SMALL)
            .derive(&other, f.counter, 0, 3)
            .unwrap();
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::WrongWallet
        );
        let mut b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        b.start = u32::MAX;
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::RangeOutOfBounds
        );
    }

    #[test]
    fn approver_refuses_an_unknown_counter_and_keeps_the_mark() {
        let f = fx();
        let stranger = identity(99).device_id();
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, stranger, 0, 3)
            .unwrap();
        let mut hw = HighWater::from_next_index(0);
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::UnknownCounter
        );
        assert_eq!(hw.next_index(), 0);
        // An empty device list refuses everything.
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &b, &mut hw, &BTreeSet::new())
                .unwrap_err(),
            BatchError::UnknownCounter
        );
        assert_eq!(hw.next_index(), 0);
    }

    #[test]
    fn approver_signs_for_a_known_counter() {
        let f = fx();
        let mut approved = f.approved.clone();
        approved.insert(identity(99).device_id());
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        let mut hw = HighWater::new();
        f.key_app
            .check_and_cosign(&f.wallet, &b, &mut hw, &approved)
            .unwrap();
        assert_eq!(hw.next_index(), 3);
    }

    #[test]
    fn approver_runs_the_shape_check_before_hashing() {
        let f = fx();
        let mut b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        // An oversized script would otherwise reach `encode`.
        b.scripts[1] = ScriptBuf::from_bytes(vec![0x51; 70_000]);
        let mut hw = HighWater::new();
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::UnexpectedScriptType
        );
        assert_eq!(hw.next_index(), 0);
    }

    #[test]
    fn approver_enforces_its_minimum() {
        let f = fx();
        let strict = Approver::new(identity(11), BatchPolicy::default());
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        assert!(matches!(
            strict.check_and_cosign(&f.wallet, &b, &mut HighWater::new(), &f.approved),
            Err(BatchError::CountTooSmall { .. })
        ));
    }

    #[test]
    fn counter_issues_each_script_once_in_order() {
        let f = fx();
        let mut c = counter_state(&f);
        assert_eq!(c.next_script(), None);
        c.add(signed(&f, 0, 3, &mut HighWater::new())).unwrap();
        assert_eq!(c.remaining(), 3);
        let mut seen = HashSet::new();
        for i in 0..3 {
            let (idx, s) = c.next_script().unwrap();
            assert_eq!(idx, i);
            assert_eq!(s, f.wallet.receive_script(i).unwrap());
            assert!(seen.insert(s));
            assert_eq!(c.remaining(), (2 - i) as usize);
        }
        assert_eq!(c.next_script(), None);
        assert_eq!(c.next_script(), None);
        assert_eq!(c.watched_scripts().len(), 3);
    }

    #[test]
    fn counter_moves_on_to_the_next_batch_in_index_order() {
        let f = fx();
        let mut hw = HighWater::new();
        let b1 = signed(&f, 0, 3, &mut hw);
        let b2 = signed(&f, 3, 3, &mut hw);
        let mut c = counter_state(&f);
        c.add(b2).unwrap(); // added out of order
        c.add(b1).unwrap();
        let idx: Vec<u32> = std::iter::from_fn(|| c.next_script().map(|x| x.0)).collect();
        assert_eq!(idx, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn counter_rejects_bad_batches() {
        let f = fx();
        let mut c = counter_state(&f);
        let good = signed(&f, 0, 3, &mut HighWater::new());

        let mut tampered = good.clone();
        tampered.batch.scripts[0] = f.wallet.receive_script(99).unwrap();
        assert_eq!(
            c.add(tampered).unwrap_err(),
            BatchError::OwnerSignatureInvalid
        );

        let mut one_sig = good.clone();
        one_sig.owner_sig = Sig([0; 64]);
        assert!(c.add(one_sig).is_err());

        // a batch for some other counter, validly signed
        let mut other = good.batch.clone();
        other.counter = identity(55).device_id();
        let sb = SignedBatch {
            owner_sig: other.sign(&f.owner),
            approver_sig: other.sign(&f.key_app.identity),
            batch: other,
        };
        assert_eq!(c.add(sb).unwrap_err(), BatchError::WrongCounter);

        // a batch for another wallet
        let mut other = good.batch.clone();
        other.wallet_checksum = [9; 32];
        let sb = SignedBatch {
            owner_sig: other.sign(&f.owner),
            approver_sig: other.sign(&f.key_app.identity),
            batch: other,
        };
        assert_eq!(c.add(sb).unwrap_err(), BatchError::WrongWallet);

        assert_eq!(c.remaining(), 0);
        c.add(good).unwrap();
    }

    #[test]
    fn counter_rejects_overlapping_and_replayed_batches() {
        let f = fx();
        let mut hw = HighWater::new();
        let b1 = signed(&f, 0, 4, &mut hw);
        let mut c = counter_state(&f);
        c.add(b1.clone()).unwrap();
        assert_eq!(c.add(b1).unwrap_err(), BatchError::OverlapsExisting);
        // overlapping range, validly double-signed by someone holding both keys
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 2, 4)
            .unwrap();
        let sb = SignedBatch {
            owner_sig: b.sign(&f.owner),
            approver_sig: b.sign(&f.key_app.identity),
            batch: b,
        };
        assert_eq!(c.add(sb).unwrap_err(), BatchError::OverlapsExisting);
        assert_eq!(c.remaining(), 4);
    }

    #[test]
    fn replayed_batch_cannot_reissue_a_used_address() {
        let f = fx();
        let b1 = signed(&f, 0, 3, &mut HighWater::new());
        let mut c = counter_state(&f);
        c.add(b1.clone()).unwrap();
        while c.next_script().is_some() {}
        assert_eq!(c.add(b1).unwrap_err(), BatchError::OverlapsExisting);
        assert_eq!(c.next_script(), None);
    }

    #[test]
    fn retire_unused_blocks_reuse_and_keeps_watching() {
        let f = fx();
        let mut hw = HighWater::new();
        let b1 = signed(&f, 0, 5, &mut hw);
        let mut c = counter_state(&f);
        c.add(b1.clone()).unwrap();
        let (_, first) = c.next_script().unwrap();
        let (_, second) = c.next_script().unwrap();
        assert_eq!(c.retire_unused(), 3);
        assert_eq!(c.remaining(), 0);
        assert_eq!(
            c.next_script(),
            None,
            "reset counter must not offer bitcoin"
        );
        // old batch cannot come back
        assert_eq!(c.add(b1).unwrap_err(), BatchError::OverlapsExisting);
        // all five stay watched
        let w = c.watched_scripts();
        assert_eq!(w.len(), 5);
        assert!(w.contains(&first) && w.contains(&second));
        for i in 0..5 {
            assert!(w.contains(&f.wallet.receive_script(i).unwrap()));
        }
        // a fresh batch from the next range works
        let b2 = signed(&f, 5, 3, &mut hw);
        c.add(b2).unwrap();
        assert_eq!(c.next_script().unwrap().0, 5);
    }

    #[test]
    fn retire_with_nothing_unused_is_zero() {
        let f = fx();
        let mut c = counter_state(&f);
        assert_eq!(c.retire_unused(), 0);
    }
}
