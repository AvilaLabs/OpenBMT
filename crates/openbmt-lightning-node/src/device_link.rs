//! The second device over the shop's own link (SL-74): the Box keeps the
//! sealed blobs, and the counter talks to it with Store and Stored messages
//! (`docs/LINK-RULES.md` LK-80 to LK-84).
//!
//! Every call is one or more exchanges. An exchange seals a Store for the
//! Box, takes the next number of the Store count (LK-81) after storing it,
//! sends it through the [`StoreTransport`] and opens the answer with the
//! Box's key alone. A write counts only when the answer is a Written one that
//! names the same request and the SHA-256 of the same bytes (SL-74); every
//! other answer is an error, and so is an answer that is not the Box's.
//!
//! One operation at a time: the lock is held from the first exchange of an
//! operation to its last, so the numbers go out in order and the parts of two
//! writes never mix.

use crate::device::{name_is_valid, DeviceError, DeviceFuture, SecondDevice};
use openbmt_core::hkdf::sha256_of;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_link::shopln::{
    Part, StoreBody, StoreMessage, StoredBody, StoredMessage, MAX_BLOB_LEN, MAX_PARTS,
    MAX_PART_LEN, MAX_STORED_NAME_LEN,
};
use openbmt_link::{os_random, seal, Inbox, Kind, Outbox, Role};
use std::sync::Arc;

/// The most names a list may add up to: the Box holds no more (BX-92).
const MAX_LIST_NAMES: usize = 10_000;

/// How a sealed Store reaches the Box and its sealed answer comes back: the
/// relay request of LK-82 on the counter's connection to the Box.
pub trait StoreTransport: Send + Sync + 'static {
    /// Sends `sealed` and returns the sealed answer. An `Err` is the Box
    /// being away, or refusing to answer.
    fn call(&self, sealed: Vec<u8>) -> DeviceFuture<Vec<u8>>;
}

/// Stores the bytes of a table of numbers where they survive a restart. It
/// returns only when they are stored (LK-40).
pub type Persist = Arc<dyn Fn(&[u8]) -> Result<(), DeviceError> + Send + Sync>;

struct State {
    inbox: Inbox,
    outbox: Outbox,
}

/// The Box as the node's second device. A cheap handle: clones share the one
/// count and the one lock.
pub struct LinkDevice<T: StoreTransport> {
    inner: Arc<Inner<T>>,
}

impl<T: StoreTransport> Clone for LinkDevice<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

struct Inner<T: StoreTransport> {
    identity: Identity,
    second_device: DeviceId,
    transport: T,
    save_inbox: Persist,
    save_outbox: Persist,
    state: tokio::sync::Mutex<State>,
}

impl<T: StoreTransport> LinkDevice<T> {
    /// A device that talks to `second_device` as `identity`. `inbox` and
    /// `outbox` are the tables of the Store count as last stored (empty on
    /// the first start); `save_inbox` and `save_outbox` store a changed one.
    pub fn new(
        identity: Identity,
        second_device: DeviceId,
        transport: T,
        (inbox, save_inbox): (Inbox, Persist),
        (outbox, save_outbox): (Outbox, Persist),
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                identity,
                second_device,
                transport,
                save_inbox,
                save_outbox,
                state: tokio::sync::Mutex::new(State { inbox, outbox }),
            }),
        }
    }
}

impl<T: StoreTransport> Inner<T> {
    /// One exchange: `body` as a Store, and the Box's Stored for it.
    async fn exchange(
        &self,
        state: &mut State,
        body: StoreBody,
    ) -> Result<StoredBody, DeviceError> {
        let mut id = [0u8; 32];
        os_random(&mut id).map_err(|e| DeviceError(format!("randomness: {e}")))?;
        let mut request_id = [0u8; 16];
        request_id.copy_from_slice(&id[..16]);
        let message = StoreMessage { request_id, body };
        let action = message.action();
        let number = {
            let save = &self.save_outbox;
            state
                .outbox
                .next(&self.second_device, |bytes| save(bytes))
                .map_err(|e| DeviceError(e.to_string()))?
        };
        let sealed = seal(
            &self.identity,
            Role::Counter,
            &self.second_device,
            Role::Box,
            number,
            Kind::Store,
            &message.to_bytes(),
            &mut os_random,
        )
        .map_err(|e| DeviceError(e.to_string()))?;
        let answer = self.transport.call(sealed).await?;
        let second = self.second_device;
        let save = &self.save_inbox;
        let opened = state
            .inbox
            .open_store(
                &self.identity,
                Role::Counter,
                &answer,
                |d| (*d == second).then_some(Role::Box),
                |bytes| save(bytes),
            )
            .map_err(|e| DeviceError(e.to_string()))?;
        if opened.kind != Kind::Stored {
            return Err(DeviceError("the answer is not a Stored".into()));
        }
        let stored = StoredMessage::from_bytes(&opened.content)
            .map_err(|e| DeviceError(format!("the answer: {e}")))?;
        if stored.request_id != request_id {
            return Err(DeviceError("the answer is for another request".into()));
        }
        if stored.action != action {
            return Err(DeviceError("the answer is for another action".into()));
        }
        match stored.body {
            StoredBody::Refused(cause) => Err(DeviceError(format!("the Box refused: {cause}"))),
            body => Ok(body),
        }
    }

    async fn write_blob(&self, name: &str, blob: Vec<u8>) -> Result<(), DeviceError> {
        check_name(name)?;
        if blob.len() > MAX_BLOB_LEN {
            return Err(DeviceError("the blob is over 16 MiB".into()));
        }
        let sha256 = sha256_of(&blob);
        let chunks: Vec<&[u8]> = if blob.is_empty() {
            vec![&[][..]]
        } else {
            blob.chunks(MAX_PART_LEN).collect()
        };
        let parts = chunks.len() as u32;
        let mut state = self.state.lock().await;
        for (i, bytes) in chunks.into_iter().enumerate() {
            let number = i as u32;
            let body = StoreBody::Write {
                name: name.to_string(),
                part: Part {
                    number,
                    parts,
                    sha256,
                    bytes: bytes.to_vec(),
                },
            };
            let answer = self.exchange(&mut state, body).await?;
            if number + 1 < parts {
                if answer != (StoredBody::Taken { part: number }) {
                    return Err(DeviceError("the Box did not take the part".into()));
                }
            } else if answer != (StoredBody::Written { sha256 }) {
                return Err(DeviceError("the Box did not write the blob".into()));
            }
        }
        Ok(())
    }

    async fn read_blob(&self, name: &str) -> Result<Option<Vec<u8>>, DeviceError> {
        check_name(name)?;
        let mut state = self.state.lock().await;
        let mut first: Option<(u32, [u8; 32])> = None;
        let mut blob = Vec::new();
        let mut number = 0u32;
        loop {
            let body = StoreBody::Read {
                name: name.to_string(),
                part: number,
            };
            let answer = self.exchange(&mut state, body).await?;
            let part = match answer {
                StoredBody::None if number == 0 => return Ok(None),
                StoredBody::Part(p) => p,
                _ => return Err(DeviceError("the Box did not give the part".into())),
            };
            if part.number != number || part.bytes.len() > MAX_PART_LEN {
                return Err(DeviceError("the Box gave another part".into()));
            }
            match first {
                None => {
                    if part.parts == 0 || part.parts > MAX_PARTS {
                        return Err(DeviceError("the Box named a wrong count of parts".into()));
                    }
                    first = Some((part.parts, part.sha256));
                }
                Some((parts, sha)) => {
                    if part.parts != parts || part.sha256 != sha {
                        return Err(DeviceError("the parts do not belong together".into()));
                    }
                }
            }
            blob.extend_from_slice(&part.bytes);
            if blob.len() > MAX_BLOB_LEN {
                return Err(DeviceError("the blob is over 16 MiB".into()));
            }
            number += 1;
            let (parts, sha) = first.unwrap_or((1, [0; 32]));
            if number == parts {
                if sha256_of(&blob) != sha {
                    return Err(DeviceError("the blob is not the one named".into()));
                }
                return Ok(Some(blob));
            }
        }
    }

    async fn list_names(&self) -> Result<Vec<String>, DeviceError> {
        let mut state = self.state.lock().await;
        let mut all: Vec<String> = Vec::new();
        let mut after = String::new();
        loop {
            let body = StoreBody::List {
                after: after.clone(),
            };
            let StoredBody::Names { names, more } = self.exchange(&mut state, body).await? else {
                return Err(DeviceError("the Box did not give names".into()));
            };
            if names.is_empty() && more {
                return Err(DeviceError(
                    "the Box says more follow and gives none".into(),
                ));
            }
            for name in names {
                // Strictly after the last, so that the list always moves on.
                let last = all.last().map(String::as_str).unwrap_or(after.as_str());
                if name.as_str() <= last || !name_is_valid(&name) {
                    return Err(DeviceError("the Box gave a wrong name".into()));
                }
                all.push(name);
            }
            if all.len() > MAX_LIST_NAMES {
                return Err(DeviceError(
                    "the Box gave more names than it may hold".into(),
                ));
            }
            if !more {
                return Ok(all);
            }
            after = all.last().cloned().unwrap_or_default();
        }
    }

    async fn remove_blob(&self, name: &str) -> Result<(), DeviceError> {
        check_name(name)?;
        let mut state = self.state.lock().await;
        let body = StoreBody::Remove {
            name: name.to_string(),
        };
        match self.exchange(&mut state, body).await? {
            StoredBody::Removed => Ok(()),
            _ => Err(DeviceError("the Box did not remove the blob".into())),
        }
    }
}

/// A name the node's keys may have, short enough for the Box (BX-93).
fn check_name(name: &str) -> Result<(), DeviceError> {
    if name_is_valid(name) && name.len() <= MAX_STORED_NAME_LEN {
        Ok(())
    } else {
        Err(DeviceError("name".into()))
    }
}

impl<T: StoreTransport> SecondDevice for LinkDevice<T> {
    fn write(&self, name: &str, blob: Vec<u8>) -> DeviceFuture<()> {
        let (inner, name) = (self.inner.clone(), name.to_string());
        Box::pin(async move { inner.write_blob(&name, blob).await })
    }

    fn read(&self, name: &str) -> DeviceFuture<Option<Vec<u8>>> {
        let (inner, name) = (self.inner.clone(), name.to_string());
        Box::pin(async move { inner.read_blob(&name).await })
    }

    fn list(&self) -> DeviceFuture<Vec<String>> {
        let inner = self.inner.clone();
        Box::pin(async move { inner.list_names().await })
    }

    fn remove(&self, name: &str) -> DeviceFuture<()> {
        let (inner, name) = (self.inner.clone(), name.to_string());
        Box::pin(async move { inner.remove_blob(&name).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;
    use openbmt_box::{MemFiles, ShopStore};
    use openbmt_link::seal::inspect;
    use openbmt_link::shopln::StoreCause;
    use openbmt_link::{open, MemoryStore, Relay, RelayConfig, Request, Response};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    const NOW: u64 = 5_000_000;

    fn identity(byte: u8) -> Identity {
        Identity::from_secret_bytes(&[byte; 32]).unwrap()
    }

    fn counter() -> Identity {
        identity(0x41)
    }

    fn the_box() -> Identity {
        identity(0x42)
    }

    type Saves = Arc<Mutex<Vec<Vec<u8>>>>;

    /// What a test does to the sealed answer on its way back, given the
    /// sealed request it answers.
    type Tamper = Arc<dyn Fn(&[u8], Vec<u8>) -> Vec<u8> + Send + Sync>;

    /// The Box's relay, in process: a real relay with the Box's store behind
    /// the store hook, asked with the request a counter's client sends.
    struct InProcess {
        relay: Arc<Mutex<Relay<MemoryStore>>>,
        away: Arc<AtomicBool>,
        tamper: Option<Tamper>,
        seen: Saves,
        outbox_saves: Saves,
    }

    impl StoreTransport for InProcess {
        fn call(&self, sealed: Vec<u8>) -> DeviceFuture<Vec<u8>> {
            // The number of the request was stored before it left (LK-40).
            let number = inspect(&sealed).unwrap().number;
            let saved = self.outbox_saves.lock().unwrap().last().cloned().unwrap();
            assert_eq!(
                Outbox::from_bytes(&saved)
                    .unwrap()
                    .last(&the_box().device_id()),
                number
            );
            self.seen.lock().unwrap().push(sealed.clone());
            // A Box that is away never gets the request.
            if self.away.load(Ordering::SeqCst) {
                return Box::pin(async { Err(DeviceError("away".into())) });
            }
            let response = self.relay.lock().unwrap().handle(
                1,
                Request::Store {
                    sealed: sealed.clone(),
                },
                NOW,
                &mut os_random,
            );
            let answer = match response {
                Response::StoreAnswer { sealed: answer } => Ok(match &self.tamper {
                    Some(t) => t(&sealed, answer),
                    None => answer,
                }),
                other => Err(DeviceError(format!("{other:?}"))),
            };
            Box::pin(async move { answer })
        }
    }

    /// The Box's side, shared by every device a test makes.
    struct Parts {
        relay: Arc<Mutex<Relay<MemoryStore>>>,
        away: Arc<AtomicBool>,
        seen: Saves,
    }

    fn parts(name: &str, counter_is_tenant: bool) -> Parts {
        let dir = temp_dir(name);
        let mut store = ShopStore::open(MemFiles::new(), dir, the_box()).unwrap();
        store
            .sync(counter_is_tenant.then(|| counter().device_id()), NOW)
            .unwrap();
        let mut relay = Relay::new(the_box(), RelayConfig::default(), MemoryStore::new()).unwrap();
        relay.set_store_hook(Some(Box::new(move |sealed, now| {
            store.answer(sealed, now, &mut os_random)
        })));
        Parts {
            relay: Arc::new(Mutex::new(relay)),
            away: Arc::new(AtomicBool::new(false)),
            seen: Saves::default(),
        }
    }

    struct Rig {
        device: LinkDevice<InProcess>,
        inbox_saves: Saves,
        outbox_saves: Saves,
    }

    fn device_with(
        p: &Parts,
        tamper: Option<Tamper>,
        numbers: (Inbox, Outbox),
        outbox_fails: bool,
    ) -> Rig {
        let inbox_saves = Saves::default();
        let outbox_saves = Saves::default();
        let (i, o) = (inbox_saves.clone(), outbox_saves.clone());
        let save_inbox: Persist = Arc::new(move |b| {
            i.lock().unwrap().push(b.to_vec());
            Ok(())
        });
        let save_outbox: Persist = Arc::new(move |b| {
            if outbox_fails {
                return Err(DeviceError("disk".into()));
            }
            o.lock().unwrap().push(b.to_vec());
            Ok(())
        });
        let transport = InProcess {
            relay: p.relay.clone(),
            away: p.away.clone(),
            tamper,
            seen: p.seen.clone(),
            outbox_saves: outbox_saves.clone(),
        };
        let device = LinkDevice::new(
            counter(),
            the_box().device_id(),
            transport,
            (numbers.0, save_inbox),
            (numbers.1, save_outbox),
        );
        Rig {
            device,
            inbox_saves,
            outbox_saves,
        }
    }

    fn rig(name: &str, tamper: Option<Tamper>) -> (Rig, Parts) {
        let p = parts(name, true);
        let r = device_with(&p, tamper, Default::default(), false);
        (r, p)
    }

    /// The answer, opened, changed and sealed again as `who` with a number
    /// no answer of the Box has had.
    fn reseal(answer: &[u8], who: &Identity, change: impl Fn(&mut StoredMessage)) -> Vec<u8> {
        let opened = open(
            &counter(),
            Role::Counter,
            answer,
            |d| (*d == the_box().device_id()).then_some(Role::Box),
            |_| 0,
        )
        .unwrap();
        let mut message = StoredMessage::from_bytes(&opened.content).unwrap();
        change(&mut message);
        seal(
            who,
            Role::Box,
            &counter().device_id(),
            Role::Counter,
            1_000_000,
            Kind::Stored,
            &message.to_bytes(),
            &mut os_random,
        )
        .unwrap()
    }

    fn tamper(
        who: Identity,
        change: impl Fn(&mut StoredMessage) + Send + Sync + 'static,
    ) -> Option<Tamper> {
        Some(Arc::new(move |_, answer| reseal(&answer, &who, &change)))
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    #[tokio::test]
    async fn sl_74_a_blob_of_several_parts_an_empty_one_and_a_small_one_come_back_as_written() {
        let (r, p) = rig("sl74-roundtrip", None);
        let d = &r.device;
        let big = pattern(2 * MAX_PART_LEN + 123);
        d.write("monitor~a", big.clone()).await.unwrap();
        d.write("empty", Vec::new()).await.unwrap();
        d.write("small", b"hello".to_vec()).await.unwrap();
        assert_eq!(d.read("monitor~a").await.unwrap(), Some(big));
        assert_eq!(d.read("empty").await.unwrap(), Some(Vec::new()));
        assert_eq!(d.read("small").await.unwrap(), Some(b"hello".to_vec()));
        // Three parts, one, one for the writes; three, one, one for the reads.
        assert_eq!(p.seen.lock().unwrap().len(), 3 + 1 + 1 + 3 + 1 + 1);
        // Written again, it is replaced.
        d.write("small", b"other".to_vec()).await.unwrap();
        assert_eq!(d.read("small").await.unwrap(), Some(b"other".to_vec()));
    }

    #[tokio::test]
    async fn sl_74_a_blob_that_is_not_there_reads_as_none_and_removing_it_is_not_an_error() {
        let (r, _p) = rig("sl74-missing", None);
        let d = &r.device;
        assert_eq!(d.read("nothing").await.unwrap(), None);
        d.remove("nothing").await.unwrap();
        d.write("a", b"1".to_vec()).await.unwrap();
        d.remove("a").await.unwrap();
        assert_eq!(d.read("a").await.unwrap(), None);
        assert!(d.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn sl_74_a_list_gives_every_name_in_byte_order_across_pages() {
        let (r, p) = rig("sl74-list", None);
        let d = &r.device;
        let mut names: Vec<String> = (0..700)
            .map(|i| format!("{}~{i:04}", "n".repeat(100)))
            .collect();
        for n in &names {
            d.write(n, vec![1]).await.unwrap();
        }
        let before = p.seen.lock().unwrap().len();
        names.sort();
        assert_eq!(d.list().await.unwrap(), names);
        // 700 names of 106 bytes do not fit in one page of 60,000 bytes.
        assert!(p.seen.lock().unwrap().len() - before >= 2);
    }

    #[tokio::test]
    async fn sl_74_a_name_the_box_cannot_keep_is_refused_before_anything_is_sent() {
        let (r, p) = rig("sl74-names", None);
        let d = &r.device;
        let long = "a".repeat(MAX_STORED_NAME_LEN + 1);
        assert!(d.write(&long, vec![1]).await.is_err());
        assert!(d.read(&long).await.is_err());
        assert!(d.remove(&long).await.is_err());
        assert!(d.write("has space", vec![1]).await.is_err());
        assert!(d.write("", vec![1]).await.is_err());
        assert!(p.seen.lock().unwrap().is_empty());
        d.write(&"a".repeat(MAX_STORED_NAME_LEN), vec![1])
            .await
            .unwrap();
        // A blob over 16 MiB is refused too.
        assert!(d.write("big", vec![0; MAX_BLOB_LEN + 1]).await.is_err());
    }

    #[tokio::test]
    async fn sl_74_an_answer_signed_by_a_device_that_is_not_the_box_is_refused() {
        let (r, _p) = rig("sl74-impostor", tamper(identity(0x43), |_| {}));
        assert!(r.device.write("a", b"1".to_vec()).await.is_err());
    }

    #[tokio::test]
    async fn sl_74_an_answer_for_another_request_does_not_count() {
        let change = |m: &mut StoredMessage| m.request_id[0] ^= 1;
        let (r, _p) = rig("sl74-other-id", tamper(the_box(), change));
        let e = r.device.write("a", b"1".to_vec()).await.unwrap_err();
        assert!(e.0.contains("another request"), "{e}");
    }

    #[tokio::test]
    async fn sl_74_an_answer_with_the_sha256_of_other_bytes_does_not_count() {
        let change = |m: &mut StoredMessage| {
            if let StoredBody::Written { sha256 } = &mut m.body {
                sha256[0] ^= 1;
            }
        };
        let (r, _p) = rig("sl74-other-sha", tamper(the_box(), change));
        assert!(r.device.write("a", b"1".to_vec()).await.is_err());
    }

    #[tokio::test]
    async fn sl_74_an_answer_that_is_taken_for_the_last_part_or_not_written_does_not_count() {
        let change = |m: &mut StoredMessage| m.body = StoredBody::Taken { part: 0 };
        let (r, _p) = rig("sl74-taken", tamper(the_box(), change));
        assert!(r.device.write("a", b"1".to_vec()).await.is_err());
        let change = |m: &mut StoredMessage| m.body = StoredBody::Removed;
        let (r, _p) = rig("sl74-removed", tamper(the_box(), change));
        assert!(r.device.write("a", b"1".to_vec()).await.is_err());
    }

    #[tokio::test]
    async fn sl_74_a_read_whose_bytes_are_not_the_ones_named_is_an_error() {
        let flip = |m: &mut StoredMessage| {
            if let StoredBody::Part(p) = &mut m.body {
                p.bytes[0] ^= 1;
            }
        };
        let p = parts("sl74-flip", true);
        let writer = device_with(&p, None, Default::default(), false);
        writer.device.write("a", b"one".to_vec()).await.unwrap();
        let reader = device_with(
            &p,
            tamper(the_box(), flip),
            (
                Inbox::from_bytes(&writer.inbox_saves.lock().unwrap().last().cloned().unwrap())
                    .unwrap(),
                Outbox::from_bytes(&writer.outbox_saves.lock().unwrap().last().cloned().unwrap())
                    .unwrap(),
            ),
            false,
        );
        assert!(reader.device.read("a").await.is_err());
    }

    #[tokio::test]
    async fn sl_74_a_list_that_says_more_follow_and_gives_no_name_is_an_error() {
        let change = |m: &mut StoredMessage| {
            m.body = StoredBody::Names {
                names: Vec::new(),
                more: true,
            }
        };
        let (r, _p) = rig("sl74-empty-more", tamper(the_box(), change));
        assert!(r.device.list().await.is_err());
        // Names that do not go on from the cursor are an error as well.
        let change = |m: &mut StoredMessage| {
            m.body = StoredBody::Names {
                names: vec!["b".into(), "a".into()],
                more: false,
            }
        };
        let (r, _p) = rig("sl74-unsorted", tamper(the_box(), change));
        assert!(r.device.list().await.is_err());
    }

    #[tokio::test]
    async fn sl_74_a_refusal_of_the_box_and_a_box_that_does_not_keep_for_the_counter_are_errors() {
        let change = |m: &mut StoredMessage| m.body = StoredBody::Refused(StoreCause::StoreFull);
        let (r, _p) = rig("sl74-refused", tamper(the_box(), change));
        let e = r.device.write("a", b"1".to_vec()).await.unwrap_err();
        assert!(e.0.contains("refused"), "{e}");
        let p = parts("sl74-no-tenant", false);
        let r = device_with(&p, None, Default::default(), false);
        assert!(r.device.write("a", b"1".to_vec()).await.is_err());
        assert!(r.device.read("a").await.is_err());
        assert!(r.device.list().await.is_err());
        assert!(r.device.remove("a").await.is_err());
    }

    #[tokio::test]
    async fn sl_74_a_box_that_is_away_gives_an_error_for_every_call_and_is_found_again() {
        let (r, p) = rig("sl74-away", None);
        let d = &r.device;
        d.write("a", b"1".to_vec()).await.unwrap();
        p.away.store(true, Ordering::SeqCst);
        assert!(d.write("a", b"2".to_vec()).await.is_err());
        assert!(d.read("a").await.is_err());
        assert!(d.list().await.is_err());
        assert!(d.remove("a").await.is_err());
        p.away.store(false, Ordering::SeqCst);
        assert_eq!(d.read("a").await.unwrap(), Some(b"1".to_vec()));
    }

    #[tokio::test]
    async fn sl_74_the_number_is_stored_before_the_request_leaves_and_a_failed_store_sends_nothing()
    {
        // The transport checks, at every call, that the stored table already
        // holds the number of the request.
        let (r, p) = rig("sl74-numbers", None);
        r.device.write("a", b"1".to_vec()).await.unwrap();
        r.device.read("a").await.unwrap();
        assert_eq!(r.outbox_saves.lock().unwrap().len(), 2);
        assert_eq!(r.inbox_saves.lock().unwrap().len(), 2);
        let p2 = parts("sl74-numbers-fail", true);
        let failing = device_with(&p2, None, Default::default(), true);
        assert!(failing.device.write("a", b"1".to_vec()).await.is_err());
        assert!(p2.seen.lock().unwrap().is_empty());
        drop(p);
    }

    #[tokio::test]
    async fn sl_74_after_a_restart_the_numbers_go_on_and_the_box_takes_them() {
        let p = parts("sl74-restart", true);
        let first = device_with(&p, None, Default::default(), false);
        first.device.write("a", b"1".to_vec()).await.unwrap();
        let inbox =
            Inbox::from_bytes(&first.inbox_saves.lock().unwrap().last().cloned().unwrap()).unwrap();
        let outbox =
            Outbox::from_bytes(&first.outbox_saves.lock().unwrap().last().cloned().unwrap())
                .unwrap();
        assert_eq!(outbox.last(&the_box().device_id()), 1);
        let second = device_with(&p, None, (inbox, outbox), false);
        assert_eq!(second.device.read("a").await.unwrap(), Some(b"1".to_vec()));
        let numbers: Vec<u64> = p
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|s| inspect(s).unwrap().number)
            .collect();
        assert_eq!(numbers, vec![1, 2]);
        // A device that forgot its numbers is not taken by the Box again.
        let forgetful = device_with(&p, None, Default::default(), false);
        assert!(forgetful.device.read("a").await.is_err());
    }

    #[tokio::test]
    async fn sl_74_an_answer_given_again_is_not_taken_a_second_time() {
        let kept: Arc<Mutex<Option<Vec<u8>>>> = Arc::default();
        let k = kept.clone();
        let replay: Tamper = Arc::new(move |_, answer| {
            let mut kept = k.lock().unwrap();
            match kept.as_ref() {
                Some(first) => first.clone(),
                None => {
                    *kept = Some(answer.clone());
                    answer
                }
            }
        });
        let (r, _p) = rig("sl74-replay", Some(replay));
        r.device.write("a", b"1".to_vec()).await.unwrap();
        assert!(r.device.write("b", b"2".to_vec()).await.is_err());
    }

    #[test]
    fn sl_74_the_tcp_device_is_a_feature_only_the_nodes_own_tools_need() {
        let crates =
            std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("..");
        let own =
            std::fs::read_to_string(crates.join("openbmt-lightning-node/Cargo.toml")).unwrap();
        // The tools that use it say so, and so builds without the feature
        // leave them out.
        assert_eq!(
            own.matches("required-features = [\"tcp-device\"]").count(),
            2
        );
        let mut checked = 0;
        for entry in std::fs::read_dir(&crates).unwrap() {
            let dir = entry.unwrap().path();
            if dir.ends_with("openbmt-lightning-node") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
                continue;
            };
            checked += 1;
            assert!(!text.contains("tcp-device"), "{}", dir.display());
            if text.contains("openbmt-lightning-node") {
                panic!("{} depends on the node crate", dir.display());
            }
        }
        assert!(checked > 10);
        let top = std::fs::read_to_string(crates.join("../Cargo.toml")).unwrap();
        assert!(!top.contains("tcp-device"));
    }
}
