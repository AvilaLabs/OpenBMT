//! The owner node of a shop that uses its own wallet
//! (`docs/OWNER-APP-RULES.md` section 17): the shop's state in its file, the
//! watch set of every index of every range (OA-107), the range top-up
//! (OA-112) and the gap limit the wallet needs (OA-113).
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

use bitcoin::ScriptBuf;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::wallet::Keychain;
use openbmt_counter_node::DriverPort;
use openbmt_courier::Post;
use openbmt_owner::own_book::{gap_limit, needs_range, unused_indexes};
use openbmt_owner::own_shop::{OwnError, OwnRestoreError, OwnShop};

use crate::coins::ScriptSet;
use crate::link::OwnerLink;
use crate::watcher::write_file;

/// The shop's state, in the node's folder.
pub const OWN_STATE_FILE: &str = "own-shop";

/// Why the node did not open, or a step did not go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnNodeError {
    /// The file could not be read or written.
    Io(String),
    /// The file does not read back as a state of this shop.
    State(OwnRestoreError),
    /// A script of the wallet could not be made.
    Derive(u32),
}

impl fmt::Display for OwnNodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "OA-105: the shop's state: {e}"),
            Self::State(e) => write!(f, "{e}"),
            Self::Derive(i) => write!(f, "OA-107: the wallet has no address at index {i}"),
        }
    }
}

impl std::error::Error for OwnNodeError {}

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
    /// The scripts handed to the driver so far (OA-107).
    watched: ScriptSet,
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
            watched: ScriptSet::new(),
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

    /// OA-107: the scripts of every index of every range in the book.
    pub fn watch_scripts(&self) -> Result<ScriptSet, OwnNodeError> {
        let mut set = ScriptSet::new();
        for index in self.shop.book().indexes() {
            let script = self
                .shop
                .wallet()
                .receive_script(index)
                .map_err(|_| OwnNodeError::Derive(index))?;
            set.insert(script, (Keychain::Receive, index));
        }
        Ok(set)
    }

    /// OA-107: hand the driver every script of the book that it was not
    /// given yet. It is called after anything that allocates, and at start,
    /// so a payment to any address a counter or a request handed out is
    /// seen. Returns how many scripts were added.
    pub fn sync_watch<P: DriverPort>(&mut self, port: &P) -> Result<usize, OwnNodeError> {
        let all = self.watch_scripts()?;
        let added: Vec<ScriptBuf> = all
            .keys()
            .filter(|s| !self.watched.contains_key(*s))
            .cloned()
            .collect();
        for script in &added {
            port.watch_script(script.clone());
        }
        self.watched = all;
        Ok(added.len())
    }

    /// OA-107: the index and chain of a script the book covers, for a
    /// payment the watcher saw.
    pub fn index_of(&self, script: &ScriptBuf) -> Result<Option<u32>, OwnNodeError> {
        let set = self.watch_scripts()?;
        Ok(set.get(script).map(|(_, index)| *index))
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
            let (shop, store) = self.shop_and_store();
            let signed = match shop.allocate_top_up(counter, store) {
                Ok(signed) => signed,
                Err(OwnError::Store(e)) => return Err(OwnNodeError::Io(e)),
                // A book that is full gives no range; the counter runs on
                // what it has, and the next step looks again.
                Err(OwnError::Refused(_)) => continue,
            };
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
}
