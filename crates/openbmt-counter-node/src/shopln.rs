//! Shop Lightning at the counter, the parts that need no network
//! (`docs/SHOP-LIGHTNING-RULES.md` section 11, SL-70 to SL-78).
//!
//! - [`role`]: whether this counter is the Lightning counter (SL-70).
//! - [`SeedRecord`] and [`take_seed`]: the seed a manager scans from the
//!   stick, checked against the pinned node ID before anything is stored
//!   (SL-72, SL-76). The record's bytes are what the app seals with its
//!   Keystore key and hands back at every start, as it does for the Lightning
//!   connection (CR-115); the node never writes them to a file.
//! - [`Book`]: what the counter keeps about the node in plain, and nothing
//!   secret: the settings block the node was started from (a node that is
//!   being closed needs it after the settings stopped naming it, SL-77), the
//!   invoices it made for sales, the opening fees paid, the time of the
//!   last sweep, the address index the sweeps and closes pay to and the
//!   payments that found no sale.
//! - [`Why`]: why a sale shows the on-chain code alone (SL-41).
//!
//! The node itself, its runtime and its link to the Box are in
//! [`crate::shopln_node`].

use std::fs;
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;

use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{Settings, ShopLightning, LIGHTNING_NODE_BYTES};
use openbmt_core::shop_lightning::{check_seed, node_id, SeedRefused};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_lightning_node::room::OnChainReason;
use openbmt_link::shopln::NotOffered;

/// The magic of the seed record, which the app seals (version 1).
pub const MAGIC_SEED_RECORD: [u8; 4] = *b"OBM\x1d";
/// The magic of the book (version 1).
pub const MAGIC_BOOK: [u8; 4] = *b"OBM\x1b";
/// The file of the book, in the node's own folder.
pub const BOOK_FILE: &str = "book";
/// The most invoices the book keeps: past it no new invoice is made.
pub const MAX_INVOICES: usize = 512;
/// The most payments with no sale the book keeps. A payment past it is not
/// acknowledged to the node, which keeps it.
pub const MAX_UNMATCHED: usize = 512;
/// How long an invoice stays in the book unpaid, in seconds: a day past the
/// hour the node lets it live.
pub const INVOICE_KEPT_SECS: u64 = 25 * 3600;
/// The seed code is 64 bytes in hexadecimal.
pub const SEED_HEX_LEN: usize = 128;

/// SL-70: what the settings in force make of this counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// The settings have no `shop_lightning`: Shop Lightning is off.
    Off,
    /// The settings name another counter: this one takes on-chain payments
    /// only.
    Other,
    /// This counter is the Lightning counter.
    Here(Box<ShopLightning>),
}

/// SL-70: only the counter the settings name offers Lightning.
pub fn role(settings: &Settings, me: &DeviceId) -> Role {
    match &settings.shop_lightning {
        None => Role::Off,
        Some(sl) if sl.counter == *me => Role::Here(Box::new(*sl)),
        Some(_) => Role::Other,
    }
}

/// The node's seed, as the app seals it.
#[derive(Clone, PartialEq, Eq)]
pub struct SeedRecord {
    seed: [u8; 64],
}

impl std::fmt::Debug for SeedRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SeedRecord(..)")
    }
}

impl Drop for SeedRecord {
    fn drop(&mut self) {
        self.seed = [0; 64];
    }
}

impl SeedRecord {
    /// The record of a seed.
    pub fn new(seed: [u8; 64]) -> Self {
        Self { seed }
    }

    /// The seed.
    pub fn seed(&self) -> &[u8; 64] {
        &self.seed
    }

    /// The node ID this seed gives (SL-72).
    pub fn node_id(&self) -> [u8; LIGHTNING_NODE_BYTES] {
        node_id(&self.seed)
    }

    /// The bytes the app seals: the magic, version 1, the node ID and the
    /// seed.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_SEED_RECORD, 97);
        out.extend_from_slice(&self.node_id());
        out.extend_from_slice(&self.seed);
        out
    }

    /// Read the record back. The node ID written beside the seed must be the
    /// one the seed gives, or the record is refused.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_SEED_RECORD)?;
        let id: [u8; LIGHTNING_NODE_BYTES] = r.array()?;
        let seed: [u8; 64] = r.array()?;
        r.finish()?;
        if node_id(&seed) != id {
            return Err(DecodeError::NonCanonical(
                "the seed does not give the node ID written beside it",
            ));
        }
        Ok(Self { seed })
    }
}

/// Why a scanned seed was not taken. Nothing was stored (SL-72).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedNotTaken {
    /// This counter is not the Lightning counter of the settings, or Shop
    /// Lightning is off (SL-70).
    NotHere,
    /// The text is not 128 hexadecimal characters.
    NotASeed,
    /// The seed gives another node ID than the settings pin (SL-72).
    WrongNode(SeedRefused),
    /// A seed is already stored. It is replaced only after the node it
    /// belongs to is closed (SL-13, SL-77).
    AlreadyStored,
}

impl SeedNotTaken {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NotHere | Self::NotASeed | Self::AlreadyStored => "SL-76",
            Self::WrongNode(_) => "SL-72",
        }
    }
}

impl std::fmt::Display for SeedNotTaken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::NotHere => f.write_str("this counter is not the Lightning counter"),
            Self::NotASeed => f.write_str("the code is not a Lightning key"),
            Self::WrongNode(e) => write!(f, "{e}"),
            Self::AlreadyStored => f.write_str("a Lightning key is already stored"),
        }
    }
}

impl std::error::Error for SeedNotTaken {}

/// The 64 bytes of a seed code: 128 hexadecimal characters, either case,
/// with nothing else but spaces at the ends.
pub fn read_seed_text(text: &str) -> Option<[u8; 64]> {
    let text = text.trim();
    if text.len() != SEED_HEX_LEN || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 64];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// SL-72, SL-76: read a scanned seed and check it against the pinned node
/// ID of the settings, before anything is stored. `stored` says a seed is
/// already held.
pub fn take_seed(
    text: &str,
    settings: &Settings,
    me: &DeviceId,
    stored: bool,
) -> Result<SeedRecord, SeedNotTaken> {
    let Role::Here(sl) = role(settings, me) else {
        return Err(SeedNotTaken::NotHere);
    };
    let seed = read_seed_text(text).ok_or(SeedNotTaken::NotASeed)?;
    check_seed(&seed, &sl.node_id).map_err(SeedNotTaken::WrongNode)?;
    if stored {
        return Err(SeedNotTaken::AlreadyStored);
    }
    Ok(SeedRecord::new(seed))
}

/// One invoice the node made for a sale, kept until its payment is
/// acknowledged, so that a paid event after a restart finds its sale (SL-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvoiceEntry {
    /// The payment hash.
    pub payment_hash: [u8; 32],
    /// The sale's index.
    pub sale: u32,
    /// The amount of the invoice, in millisatoshis.
    pub amount_msat: u64,
    /// The provider's fee if the payment opens a channel, in millisatoshis.
    pub opening_fee_msat: u64,
    /// When the invoice was made, in seconds since 1970.
    pub made_at: u64,
}

/// A payment the node claimed that no sale could take: kept, never dropped,
/// so that the money the node holds for it stays accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unmatched {
    /// The payment hash.
    pub payment_hash: [u8; 32],
    /// The sale the invoice was made for, when the counter knew one.
    pub sale: Option<u32>,
    /// What the shop was paid, in millisatoshis, the skimmed fee included.
    pub amount_msat: u64,
    /// When the counter kept it, in seconds since 1970.
    pub at: u64,
}

/// The address index of the shop's wallet that the sweeps, the closes and the
/// node's output sweeper pay to now (SL-33). It is taken from the counter's
/// ranges, as a sale's address is, and is the address of one wallet only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepAddress {
    /// The SHA-256 of the wallet description the index belongs to.
    pub wallet: [u8; 32],
    /// The index.
    pub index: u32,
}

/// What the counter keeps about the node, in plain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Book {
    /// The settings block the node runs from (SL-77: a node being closed
    /// needs it after the settings stopped naming it).
    pub plan: Option<ShopLightning>,
    /// The opening fees paid so far, in satoshis (SL-78).
    pub opening_fees_paid_sats: u64,
    /// The time of the last sweep, in seconds since 1970; 0 if none.
    pub last_sweep_at: u64,
    /// The invoices not yet acknowledged.
    pub invoices: Vec<InvoiceEntry>,
    /// The address the next sweep or close pays to, and the node's output
    /// sweeper, until a transaction has paid it.
    pub sweep: Option<SweepAddress>,
    /// The claimed payments no sale took.
    pub unmatched: Vec<Unmatched>,
}

fn put_plan(out: &mut Vec<u8>, sl: &ShopLightning) {
    out.extend_from_slice(&sl.counter.to_bytes());
    out.extend_from_slice(&sl.node_id);
    out.extend_from_slice(&sl.second_device.to_bytes());
    out.extend_from_slice(&sl.provider_node_id);
    match sl.provider_addr.ip() {
        IpAddr::V4(a) => {
            out.push(4);
            out.extend_from_slice(&a.octets());
        }
        IpAddr::V6(a) => {
            out.push(6);
            out.extend_from_slice(&a.octets());
        }
    }
    out.extend_from_slice(&sl.provider_addr.port().to_le_bytes());
    out.extend_from_slice(&sl.cap_sats.to_le_bytes());
    out.extend_from_slice(&sl.max_opening_fee_sats.to_le_bytes());
}

fn read_plan(r: &mut Reader<'_>) -> Result<ShopLightning, DecodeError> {
    let counter = r.device_id()?;
    let node_id = r.array()?;
    let second_device = r.device_id()?;
    let provider_node_id = r.array()?;
    let ip = match r.u8()? {
        4 => IpAddr::V4(Ipv4Addr::from(r.array::<4>()?)),
        6 => IpAddr::V6(Ipv6Addr::from(r.array::<16>()?)),
        _ => {
            return Err(DecodeError::NonCanonical(
                "the address family is not 4 or 6",
            ))
        }
    };
    let port = r.u16_le()?;
    Ok(ShopLightning {
        counter,
        node_id,
        second_device,
        provider_node_id,
        provider_addr: SocketAddr::new(ip, port),
        cap_sats: r.u64_le()?,
        max_opening_fee_sats: r.u64_le()?,
    })
}

impl Book {
    /// The bytes of the book.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_BOOK, 256);
        match &self.plan {
            Some(sl) => {
                out.push(1);
                put_plan(&mut out, sl);
            }
            None => out.push(0),
        }
        out.extend_from_slice(&self.opening_fees_paid_sats.to_le_bytes());
        out.extend_from_slice(&self.last_sweep_at.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.invoices.len()).to_le_bytes());
        for e in &self.invoices {
            out.extend_from_slice(&e.payment_hash);
            out.extend_from_slice(&e.sale.to_le_bytes());
            out.extend_from_slice(&e.amount_msat.to_le_bytes());
            out.extend_from_slice(&e.opening_fee_msat.to_le_bytes());
            out.extend_from_slice(&e.made_at.to_le_bytes());
        }
        match &self.sweep {
            Some(a) => {
                out.push(1);
                out.extend_from_slice(&a.wallet);
                out.extend_from_slice(&a.index.to_le_bytes());
            }
            None => out.push(0),
        }
        out.extend_from_slice(&wire::len_u32(self.unmatched.len()).to_le_bytes());
        for u in &self.unmatched {
            out.extend_from_slice(&u.payment_hash);
            match u.sale {
                Some(sale) => {
                    out.push(1);
                    out.extend_from_slice(&sale.to_le_bytes());
                }
                None => {
                    out.push(0);
                    out.extend_from_slice(&0u32.to_le_bytes());
                }
            }
            out.extend_from_slice(&u.amount_msat.to_le_bytes());
            out.extend_from_slice(&u.at.to_le_bytes());
        }
        out
    }

    /// Read the book back, strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_BOOK)?;
        let plan = if r.flag()? {
            Some(read_plan(&mut r)?)
        } else {
            None
        };
        let opening_fees_paid_sats = r.u64_le()?;
        let last_sweep_at = r.u64_le()?;
        let n = r.count(MAX_INVOICES, 60)?;
        let mut invoices = Vec::with_capacity(n);
        for _ in 0..n {
            invoices.push(InvoiceEntry {
                payment_hash: r.array()?,
                sale: r.u32_le()?,
                amount_msat: r.u64_le()?,
                opening_fee_msat: r.u64_le()?,
                made_at: r.u64_le()?,
            });
        }
        let sweep = if r.flag()? {
            Some(SweepAddress {
                wallet: r.array()?,
                index: r.u32_le()?,
            })
        } else {
            None
        };
        let n = r.count(MAX_UNMATCHED, 53)?;
        let mut unmatched = Vec::with_capacity(n);
        for _ in 0..n {
            let payment_hash = r.array()?;
            let has_sale = r.flag()?;
            let sale = r.u32_le()?;
            if !has_sale && sale != 0 {
                return Err(DecodeError::NonCanonical(
                    "a payment with no sale names a sale",
                ));
            }
            unmatched.push(Unmatched {
                payment_hash,
                sale: has_sale.then_some(sale),
                amount_msat: r.u64_le()?,
                at: r.u64_le()?,
            });
        }
        r.finish()?;
        Ok(Self {
            plan,
            opening_fees_paid_sats,
            last_sweep_at,
            invoices,
            sweep,
            unmatched,
        })
    }

    /// The book in `dir`, or an empty one when there is no file. A file that
    /// is there and does not read is an error, never an empty book: the node
    /// is not started from it and the seed is kept.
    pub fn load(dir: &Path) -> Result<Self, String> {
        match fs::read(dir.join(BOOK_FILE)) {
            Ok(bytes) => Self::from_bytes(&bytes).map_err(|e| format!("the book: {e}")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("the book: {e}")),
        }
    }

    /// Write the book whole: a temporary file, flushed, renamed over the
    /// old one, and the folder flushed.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let temp = dir.join(format!("{BOOK_FILE}.tmp"));
        let mut f = fs::File::create(&temp)?;
        f.write_all(&self.to_bytes())?;
        f.sync_all()?;
        fs::rename(&temp, dir.join(BOOK_FILE))?;
        fs::File::open(dir)?.sync_all()
    }

    /// Drops the entries older than [`INVOICE_KEPT_SECS`] at `now`: the node
    /// claims no invoice after its expiry, so none of them will be paid.
    pub fn prune(&mut self, now: u64) {
        self.invoices
            .retain(|e| now < e.made_at.saturating_add(INVOICE_KEPT_SECS));
    }

    /// The entry of a payment hash.
    pub fn invoice(&self, hash: &[u8; 32]) -> Option<&InvoiceEntry> {
        self.invoices.iter().find(|e| &e.payment_hash == hash)
    }

    /// Takes the invoices of a sale out of the book, except those whose
    /// hash is in `keep` (a payment of theirs is claimed and waits to be
    /// recorded). Returns the hashes taken out. A sale that is over, cancelled
    /// or quoted again has no use for them, and they would otherwise wait for
    /// [`INVOICE_KEPT_SECS`] against the cap of [`MAX_INVOICES`].
    pub fn drop_sale(&mut self, sale: u32, keep: &[[u8; 32]]) -> Vec<[u8; 32]> {
        let mut gone = Vec::new();
        self.invoices.retain(|e| {
            let drop = e.sale == sale && !keep.contains(&e.payment_hash);
            if drop {
                gone.push(e.payment_hash);
            }
            !drop
        });
        gone
    }

    /// Keeps a claimed payment no sale took. `false` when the book is full
    /// of them; one that is kept already is kept once.
    pub fn keep_unmatched(&mut self, u: Unmatched) -> bool {
        if self
            .unmatched
            .iter()
            .any(|k| k.payment_hash == u.payment_hash)
        {
            return true;
        }
        if self.unmatched.len() >= MAX_UNMATCHED {
            return false;
        }
        self.unmatched.push(u);
        true
    }
}

/// Why a sale shows the on-chain code alone, or why Lightning is not on at
/// all on this counter (SL-41, SL-76, SL-77). The words are in
/// [`crate::say`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The seed has not been scanned yet (SL-76).
    NoSeed,
    /// The node is starting.
    Starting,
    /// The network is the real one (SL-60).
    Mainnet,
    /// The counter holds no description of the shop's wallet, so there is
    /// nowhere to sweep to (SL-33).
    NoWallet,
    /// The relay of the counter's pairing is not the Box the settings name as
    /// the second device (SL-73).
    NotTheBox,
    /// The relay cannot be reached the way the settings need (Tor).
    NoRoute,
    /// There is no address of the shop's wallet to sweep to: no range of the
    /// pairing has a free index (SL-33).
    NoSweepAddress,
    /// The node did not start; the text is the node's own.
    NotStarted(String),
    /// The node is closing its channels (SL-77).
    Closing,
    /// The node's own answer for an amount (SL-29, SL-40, SL-41).
    Node(OnChainReason),
    /// The invoice could not be stored on both devices (SL-20).
    NotStored,
    /// The node could not make the invoice.
    Library(String),
}

impl Why {
    /// The cause the Lightning report names (LK-84, SL-78).
    pub fn not_offered(&self) -> NotOffered {
        match self {
            Self::Node(OnChainReason::SecondDeviceAway) | Self::NotStored => {
                NotOffered::SecondDeviceAway
            }
            Self::Node(OnChainReason::ChainBehind) => NotOffered::ChainBehind,
            Self::Node(OnChainReason::ProviderAway) => NotOffered::ProviderSilent,
            _ => NotOffered::NoUsableChannel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbmt_core::identity::Identity;

    fn id(n: u8) -> DeviceId {
        Identity::from_secret_bytes(&[n; 32]).unwrap().device_id()
    }

    fn seed(n: u8) -> [u8; 64] {
        std::array::from_fn(|i| n.wrapping_add(i as u8))
    }

    fn hex(seed: &[u8; 64]) -> String {
        seed.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn sl(n: u8) -> ShopLightning {
        ShopLightning {
            counter: id(3),
            node_id: node_id(&seed(n)),
            second_device: id(8),
            provider_node_id: node_id(&seed(99)),
            provider_addr: "127.0.0.1:9735".parse().unwrap(),
            cap_sats: 100_000,
            max_opening_fee_sats: 3_000,
        }
    }

    fn settings(sl: Option<ShopLightning>) -> Settings {
        let mut s = Settings::defaults(1_000);
        s.shop_lightning = sl;
        s
    }

    #[test]
    fn sl_70_only_the_counter_the_settings_name_is_the_lightning_counter() {
        let s = settings(Some(sl(1)));
        assert_eq!(role(&s, &id(3)), Role::Here(Box::new(sl(1))));
        assert_eq!(role(&s, &id(4)), Role::Other);
        assert_eq!(role(&settings(None), &id(3)), Role::Off);
    }

    #[test]
    fn sl_72_a_seed_of_another_id_is_refused_and_a_pinned_one_is_taken() {
        let s = settings(Some(sl(1)));
        let ok = take_seed(&hex(&seed(1)), &s, &id(3), false).unwrap();
        assert_eq!(ok.node_id(), sl(1).node_id);
        let e = take_seed(&hex(&seed(2)), &s, &id(3), false).unwrap_err();
        assert!(matches!(e, SeedNotTaken::WrongNode(_)));
        assert_eq!(e.rule(), "SL-72");
        // The text may be upper case and have spaces at the ends.
        let upper = format!(" {} ", hex(&seed(1)).to_uppercase());
        assert!(take_seed(&upper, &s, &id(3), false).is_ok());
    }

    #[test]
    fn sl_76_a_code_that_is_not_a_seed_or_comes_to_the_wrong_counter_is_refused() {
        let s = settings(Some(sl(1)));
        for bad in [
            "",
            "zz",
            &hex(&seed(1))[2..],
            &format!("{}00", hex(&seed(1))),
        ] {
            assert_eq!(
                take_seed(bad, &s, &id(3), false),
                Err(SeedNotTaken::NotASeed),
                "{bad}"
            );
        }
        let nonhex = "g".repeat(128);
        assert_eq!(
            take_seed(&nonhex, &s, &id(3), false),
            Err(SeedNotTaken::NotASeed)
        );
        assert_eq!(
            take_seed(&hex(&seed(1)), &s, &id(4), false),
            Err(SeedNotTaken::NotHere)
        );
        assert_eq!(
            take_seed(&hex(&seed(1)), &settings(None), &id(3), false),
            Err(SeedNotTaken::NotHere)
        );
        assert_eq!(
            take_seed(&hex(&seed(1)), &s, &id(3), true),
            Err(SeedNotTaken::AlreadyStored)
        );
    }

    #[test]
    fn sl_72_the_record_reads_back_and_a_record_for_another_id_does_not() {
        let r = SeedRecord::new(seed(5));
        let bytes = r.to_bytes();
        assert_eq!(SeedRecord::from_bytes(&bytes).unwrap(), r);
        assert!(!format!("{r:?}").contains("5"));
        let mut other = bytes.clone();
        other[10] ^= 1;
        assert!(SeedRecord::from_bytes(&other).is_err());
        let mut tail = bytes.clone();
        tail.push(0);
        assert!(SeedRecord::from_bytes(&tail).is_err());
        assert!(SeedRecord::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn sl_77_the_book_reads_back_with_its_plan_and_its_invoices() {
        let book = Book {
            plan: Some(sl(1)),
            opening_fees_paid_sats: 321,
            last_sweep_at: 1_700_000_000,
            invoices: vec![InvoiceEntry {
                payment_hash: [7; 32],
                sale: 4,
                amount_msat: 5_000_000,
                opening_fee_msat: 2_000,
                made_at: 1_700_000_001,
            }],
            sweep: Some(SweepAddress {
                wallet: [3; 32],
                index: 77,
            }),
            unmatched: vec![
                Unmatched {
                    payment_hash: [8; 32],
                    sale: Some(9),
                    amount_msat: 6_000,
                    at: 5,
                },
                Unmatched {
                    payment_hash: [9; 32],
                    sale: None,
                    amount_msat: 7_000,
                    at: 6,
                },
            ],
        };
        assert_eq!(Book::from_bytes(&book.to_bytes()).unwrap(), book);
        let mut v6 = book.clone();
        v6.plan.as_mut().unwrap().provider_addr = "[::1]:9735".parse().unwrap();
        assert_eq!(Book::from_bytes(&v6.to_bytes()).unwrap(), v6);
        let empty = Book::default();
        assert_eq!(Book::from_bytes(&empty.to_bytes()).unwrap(), empty);
        let mut bytes = book.to_bytes();
        bytes.push(1);
        assert!(Book::from_bytes(&bytes).is_err());
    }

    #[test]
    fn sl_77_the_book_is_written_whole_and_a_missing_one_is_empty() {
        let dir = std::env::temp_dir().join(format!("shopln-book-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(Book::load(&dir).unwrap(), Book::default());
        let book = Book {
            plan: Some(sl(2)),
            ..Book::default()
        };
        book.save(&dir).unwrap();
        assert_eq!(Book::load(&dir).unwrap(), book);
        fs::write(dir.join(BOOK_FILE), b"nonsense").unwrap();
        assert!(Book::load(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sl_21_the_invoices_of_a_closed_sale_leave_the_book_but_not_one_that_is_claimed() {
        let entry = |hash: u8, sale: u32| InvoiceEntry {
            payment_hash: [hash; 32],
            sale,
            amount_msat: 1,
            opening_fee_msat: 0,
            made_at: 10,
        };
        let mut book = Book {
            invoices: vec![entry(1, 4), entry(2, 4), entry(3, 5)],
            ..Book::default()
        };
        assert_eq!(book.drop_sale(4, &[[2; 32]]), vec![[1; 32]]);
        assert_eq!(book.invoices, vec![entry(2, 4), entry(3, 5)]);
        assert!(book.drop_sale(9, &[]).is_empty());
        assert_eq!(book.drop_sale(4, &[]), vec![[2; 32]]);
    }

    #[test]
    fn sl_21_a_payment_no_sale_took_is_kept_once_and_the_book_has_a_limit() {
        let mut book = Book::default();
        let u = |n: u8| Unmatched {
            payment_hash: [n; 32],
            sale: None,
            amount_msat: 1_000,
            at: 1,
        };
        assert!(book.keep_unmatched(u(1)));
        assert!(book.keep_unmatched(u(1)));
        assert_eq!(book.unmatched.len(), 1);
        for n in 1..MAX_UNMATCHED {
            let mut v = u(0);
            v.payment_hash[..8].copy_from_slice(&(n as u64).to_le_bytes());
            assert!(book.keep_unmatched(v));
        }
        assert_eq!(book.unmatched.len(), MAX_UNMATCHED);
        let mut over = u(7);
        over.payment_hash[2] = 9;
        assert!(!book.keep_unmatched(over));
    }

    #[test]
    fn sl_78_each_reason_has_a_cause_the_report_can_name() {
        assert_eq!(
            Why::Node(OnChainReason::SecondDeviceAway).not_offered(),
            NotOffered::SecondDeviceAway
        );
        assert_eq!(
            Why::Node(OnChainReason::ChainBehind).not_offered(),
            NotOffered::ChainBehind
        );
        assert_eq!(
            Why::Node(OnChainReason::ProviderAway).not_offered(),
            NotOffered::ProviderSilent
        );
        assert_eq!(Why::NoSeed.not_offered(), NotOffered::NoUsableChannel);
        assert_eq!(
            Why::NoSweepAddress.not_offered(),
            NotOffered::NoUsableChannel
        );
        assert_eq!(Why::NotStored.not_offered(), NotOffered::SecondDeviceAway);
    }
}
