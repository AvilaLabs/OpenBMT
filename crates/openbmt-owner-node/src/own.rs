//! The owner node of a shop that uses its own wallet
//! (`docs/OWNER-APP-RULES.md` section 17): the shop's state in its file, the
//! range top-up (OA-112), the settings carried to the counters paired
//! earlier (CR-236) and the gap limit the wallet needs (OA-113).
//!
//! The watching is not here. Every index of every range of the book is
//! watched (OA-107) by the owner node's live watch and deep check
//! ([`OwnerNode`](crate::OwnerNode), OC-22 to OC-24), which take the book's
//! ranges, each with the owner's clock when it was given, as the issued
//! ranges of OC-10 ([`OwnerParts::own_book`](crate::OwnerParts::own_book)).
//!
//! [`OwnNode`] holds an [`OwnShop`] and the folder its state is kept in. Every
//! change of the shop is stored, whole and by atomic steps, in the file
//! [`OWN_STATE_FILE`] before it takes effect (OA-105). The top-up runs next
//! to [`OwnerLink::step`](crate::OwnerLink::step): it reads what the copies
//! of the counters' chains show, and gives a counter that is running low the
//! next range, kept in the range outbox and sent as a message of kind Range
//! (LK-77).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bitcoin::hashes::{sha256, Hash};
use bitcoin::ScriptBuf;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::settings::Authority;
use openbmt_courier::Post;
use openbmt_owner::own_book::{gap_limit, needs_range, unused_indexes, AllocationBook};
use openbmt_owner::own_shop::{OwnError, OwnRestoreError, OwnShop};

use crate::link::{OwnerLink, SettingsNotCarried};
use crate::node::{OwnerNode, OwnerPort};
use crate::watcher::write_file;

/// The shop's state, in the node's folder.
pub const OWN_STATE_FILE: &str = "own-shop";

/// The key a shop with its own wallet gives the owner node's queue in place
/// of a Key app's: made from the owner's secret and a tag, so that no
/// device holds the key that goes with it. A shop with its own wallet has no
/// Key app; the queue that takes this key is never given anything to sign.
pub fn own_approver(owner_secret: &[u8; 32]) -> Result<DeviceId, OwnNodeError> {
    let mut seed = Vec::with_capacity(32 + 32);
    seed.extend_from_slice(b"OpenBMT/own-wallet-no-key-app/v1");
    seed.extend_from_slice(owner_secret);
    let digest = sha256::Hash::hash(&seed).to_byte_array();
    seed.fill(0);
    Identity::from_secret_bytes(&digest)
        .map(|i| i.device_id())
        .map_err(|_| OwnNodeError::NoApprover)
}

/// Why the node did not open, or a step did not go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnNodeError {
    /// The file could not be read or written.
    Io(String),
    /// The file does not read back as a state of this shop.
    State(OwnRestoreError),
    /// A script of the wallet could not be made.
    Derive(u32),
    /// The key for the queue's missing Key app could not be made.
    NoApprover,
}

impl fmt::Display for OwnNodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "OA-105: the shop's state: {e}"),
            Self::State(e) => write!(f, "{e}"),
            Self::Derive(i) => write!(f, "OA-107: the wallet has no address at index {i}"),
            Self::NoApprover => write!(f, "OA-105: the queue's placeholder key could not be made"),
        }
    }
}

impl std::error::Error for OwnNodeError {}

/// What the range top-up asks of the live watch (OC-22, OA-107).
pub trait Watching {
    /// Whether the live watch can follow the indexes up to `end` (one past
    /// the last), with what can be set aside now.
    fn room_for(&self, end: u32) -> bool;
    /// The book gave out a range: watch it from now on.
    fn written(&mut self, book: &AllocationBook);
}

/// No live watch to ask: every range has room and nothing is told.
#[derive(Debug, Clone, Copy)]
pub struct NotWatching;

impl Watching for NotWatching {
    fn room_for(&self, _: u32) -> bool {
        true
    }
    fn written(&mut self, _: &AllocationBook) {}
}

impl<P: OwnerPort> Watching for OwnerNode<P> {
    fn room_for(&self, end: u32) -> bool {
        self.own_room_for(end)
    }
    fn written(&mut self, book: &AllocationBook) {
        // A node that stopped says so in its own notes and status.
        let _ = self.set_own_book(book);
    }
}

/// One counter that was given a range by [`OwnNode::top_up`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToppedUp {
    /// The counter.
    pub counter: DeviceId,
    /// First index of the range sent.
    pub start: u32,
    /// One past the last index of the range sent.
    pub end: u32,
}

/// The owner node of a shop with its own wallet. See the module docs.
pub struct OwnNode {
    shop: OwnShop,
    dir: PathBuf,
}

impl fmt::Debug for OwnNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnNode")
            .field("shop", &self.shop)
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl OwnNode {
    /// Open the shop's state under `dir`: no file is a shop with nothing in
    /// it, and a file that does not read back as a state of this owner and
    /// this wallet is refused (OA-105). The folder is made if it is not
    /// there.
    pub fn open(
        dir: &Path,
        identity: Identity,
        wallet: PinnedWallet,
    ) -> Result<Self, OwnNodeError> {
        fs::create_dir_all(dir).map_err(|e| OwnNodeError::Io(e.to_string()))?;
        let shop = match fs::read(dir.join(OWN_STATE_FILE)) {
            Ok(bytes) => OwnShop::restore(identity, wallet, &bytes).map_err(OwnNodeError::State)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => OwnShop::new(identity, wallet),
            Err(e) => return Err(OwnNodeError::Io(format!("read: {e}"))),
        };
        Ok(Self {
            shop,
            dir: dir.to_path_buf(),
        })
    }

    /// The shop, to read.
    pub fn shop(&self) -> &OwnShop {
        &self.shop
    }

    /// The shop, and the store step that writes its file, for a call that
    /// changes it.
    pub fn shop_and_store(
        &mut self,
    ) -> (&mut OwnShop, impl Fn(&[u8]) -> Result<(), String> + 'static) {
        let dir = self.dir.clone();
        (&mut self.shop, move |bytes: &[u8]| {
            write_file(&dir, OWN_STATE_FILE, bytes)
        })
    }

    /// OA-107: the index of a script the book covers, for a payment the
    /// watcher saw. The coin view gives the index of a payment it applied;
    /// this looks a script up on its own, by making the book's scripts in
    /// order, so it is for a call that has no view.
    pub fn index_of(&self, script: &ScriptBuf) -> Result<Option<u32>, OwnNodeError> {
        for index in self.shop.book().indexes() {
            let made = self
                .shop
                .wallet()
                .receive_script(index)
                .map_err(|_| OwnNodeError::Derive(index))?;
            if made == *script {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    /// OA-112: the indexes `counter` holds that its records have not used:
    /// its first range (from its bundle, which it took before it wrote a
    /// record) and the ranges its range-held records name, less the indexes
    /// its sales opened. `None` for a counter whose bundle still waits or
    /// that the link does not follow.
    pub fn unused_of<R: Post>(&self, link: &OwnerLink<R>, counter: &DeviceId) -> Option<u64> {
        if self
            .shop
            .counters_waiting()
            .iter()
            .any(|w| w.counter == *counter)
        {
            return None;
        }
        let first = self.shop.first_range_of(counter)?;
        let (mut held, opened) = link.index_use(counter)?;
        held.push((first.start, first.end));
        held.sort_unstable();
        held.dedup();
        Some(unused_indexes(&held, &opened))
    }

    /// OA-112: give each counter with fewer than 300 unused indexes and no
    /// range waiting the next range, and send it as a message of kind Range.
    /// The range is written to the book first, then signed, kept in the
    /// outbox file and sent; the link sends it again until the counter's
    /// records show it held. At most one range waits for a counter.
    pub fn top_up<R: Post>(
        &mut self,
        link: &mut OwnerLink<R>,
        now: u64,
    ) -> Result<Vec<ToppedUp>, OwnNodeError> {
        self.top_up_with(link, now, &mut NotWatching)
    }

    /// [`OwnNode::top_up`], with the live watch asked at two moments:
    /// before a range is written, whether it can follow the indexes up to
    /// the range's end (OC-22), and after it is written and before it is
    /// sent, to watch it (OA-107). A counter whose range would not fit runs
    /// on what it has, and the next step looks again.
    pub fn top_up_with<R: Post>(
        &mut self,
        link: &mut OwnerLink<R>,
        now: u64,
        watching: &mut dyn Watching,
    ) -> Result<Vec<ToppedUp>, OwnNodeError> {
        link.scan_index_use();
        let mut done = Vec::new();
        let counters: Vec<DeviceId> = self.shop.book().devices().copied().collect();
        for counter in counters {
            if counter == self.shop.owner_id() {
                continue;
            }
            let Some(unused) = self.unused_of(link, &counter) else {
                continue;
            };
            if !needs_range(unused, link.ranges_waiting(&counter).is_some()) {
                continue;
            }
            let next_end = self
                .shop
                .book()
                .next_start()
                .saturating_add(openbmt_owner::own_book::RANGE_LEN);
            if !watching.room_for(next_end) {
                continue;
            }
            let (shop, store) = self.shop_and_store();
            let signed = match shop.allocate_top_up(counter, now, store) {
                Ok(signed) => signed,
                Err(OwnError::Store(e)) => return Err(OwnNodeError::Io(e)),
                // A book that is full gives no range; the counter runs on
                // what it has, and the next step looks again.
                Err(OwnError::Refused(_)) => continue,
            };
            // The range is watched before the counter can use an address of it.
            watching.written(self.shop.book());
            // A range that could not be sent now is kept and sent again.
            let _ = link.send_range(&counter, &signed, now);
            done.push(ToppedUp {
                counter,
                start: signed.range.start,
                end: signed.range.end,
            });
        }
        Ok(done)
    }

    /// OA-113: how far ahead the wallet must look, from the indexes that
    /// were paid and the highest index any counter handed out (its sales'
    /// opened indexes).
    pub fn gap_needed<R: Post>(
        &self,
        link: &OwnerLink<R>,
        paid: &std::collections::BTreeSet<u32>,
    ) -> u32 {
        let highest = self
            .shop
            .book()
            .devices()
            .filter_map(|d| link.index_use(d))
            .filter_map(|(_, opened)| opened.iter().next_back().copied())
            .max();
        gap_limit(paid, highest)
    }

    /// CR-236, LK-64: carry the settings in force to the devices on their
    /// list, so that a counter paired earlier gets the settings that name a
    /// counter paired later, and the owner's later changes. `carried` is the
    /// highest version already carried to every device, which the caller
    /// keeps between calls. The settings are checked against the owner root
    /// alone and go out as signed: a change that loosens is held by the
    /// counter for confirmation (CR-239). Returns whether a version was
    /// taken up by the link in this call.
    pub fn carry_settings<R: Post>(
        &self,
        link: &mut OwnerLink<R>,
        carried: &mut u32,
        now: u64,
    ) -> Result<bool, SettingsNotCarried> {
        let Some(signed) = self.shop.known_settings() else {
            return Ok(false);
        };
        let version = signed.settings.version;
        if version <= *carried {
            return Ok(false);
        }
        let authority = Authority::OwnWallet {
            owner: self.shop.owner_id(),
        };
        link.carry_settings_with(version, signed, &authority, None, now)?;
        if link.settings_missing(version).is_empty() {
            link.settings_done(version);
            *carried = version;
        }
        Ok(true)
    }

    /// LK-67: the counters the shop's settings in force name, for the link
    /// to follow. The owner's own key (the desk) is not one.
    pub fn counters_of_settings(&self) -> Vec<DeviceId> {
        self.shop
            .known_settings()
            .map(|s| s.settings.counters.keys().copied().collect())
            .unwrap_or_default()
    }

    /// One turn of the owner's side for a shop with its own wallet, after
    /// the link's step (OA-112): the live watch follows the book (OA-107,
    /// OC-22), the link follows the counters of the settings (LK-67), each
    /// counter running low is given its next range (OA-112), and the
    /// settings in force are carried to the counters (CR-236). `carried` is
    /// the version of the settings already carried to every counter, which
    /// the caller keeps between turns.
    pub fn tick<P: OwnerPort, R: Post>(
        &mut self,
        node: &mut OwnerNode<P>,
        link: &mut OwnerLink<R>,
        carried: &mut u32,
        now: u64,
    ) -> Result<OwnTick, OwnNodeError> {
        let mut notes = Vec::new();
        if let Err(e) = node.set_own_book(self.shop.book()) {
            notes.push(format!("OA-107: the live watch did not take the book: {e}"));
        }
        let counters = self.counters_of_settings();
        if !counters.is_empty() {
            if let Err(e) = link.follow(&counters, &[]) {
                notes.push(format!("LK-67: the link's counters: {e}"));
            }
        }
        let topped_up = self.top_up_with(link, now, node)?;
        let settings_taken = match self.carry_settings(link, carried, now) {
            Ok(taken) => taken,
            Err(e) => {
                notes.push(format!("CR-236: the settings were not carried: {e}"));
                false
            }
        };
        Ok(OwnTick {
            topped_up,
            settings_taken,
            notes,
        })
    }
}

/// What [`OwnNode::tick`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnTick {
    /// The counters given a range.
    pub topped_up: Vec<ToppedUp>,
    /// Whether the link took up settings in this turn.
    pub settings_taken: bool,
    /// What the turn could not do, with the rule.
    pub notes: Vec<String>,
}
