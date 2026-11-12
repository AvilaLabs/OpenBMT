//! A request writer and a response reader written separately from the
//! encoder under test: the field order is written out again here.

use crate::{OwnerNetwork, StartConfig};

/// Writes the fields of a request.
#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }

    pub fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u64(mut self, v: u64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn bytes(mut self, v: &[u8]) -> Self {
        self = self.u32(u32::try_from(v.len()).expect("fits"));
        self.0.extend_from_slice(v);
        self
    }

    pub fn string(self, v: &str) -> Self {
        self.bytes(v.as_bytes())
    }

    pub fn opt_bytes(self, v: Option<&[u8]>) -> Self {
        match v {
            None => self.u8(0),
            Some(b) => self.u8(1).bytes(b),
        }
    }

    pub fn opt_string(self, v: Option<&str>) -> Self {
        self.opt_bytes(v.map(str::as_bytes))
    }

    /// The fields of `start`, in the order of the protocol.
    pub fn start(self, c: &StartConfig) -> Self {
        let network = match c.network {
            OwnerNetwork::Mainnet => 0,
            OwnerNetwork::Testnet => 1,
            OwnerNetwork::Signet => 2,
            OwnerNetwork::Regtest => 3,
        };
        let mut w = self
            .string(&c.dir)
            .u8(network)
            .u64(c.now)
            .bytes(&c.owner_secret)
            .bytes(&c.approver_root)
            .string(&c.wallet)
            .bytes(&c.key1_account)
            .opt_string(c.key3_xpub.as_deref())
            .u32(c.batch_min_count)
            .u32(c.refund_depth)
            .u32(c.birthday)
            .opt_string(c.proxy.as_deref())
            .u32(u32::try_from(c.fixed_peers.len()).expect("fits"));
        for p in &c.fixed_peers {
            w = w.string(p);
        }
        w = w
            .opt_string(c.broadcast_proxy.as_deref())
            .u8(u8::from(c.without_tor));
        w = match &c.link {
            None => w.u8(0),
            Some(l) => {
                w = w.u8(1);
                w = match &l.route {
                    crate::RouteView::Direct { addr } => w.u8(0).string(addr),
                    crate::RouteView::Onion { proxy, host, port } => {
                        w.u8(1).string(proxy).string(host).u32(*port)
                    }
                };
                w = w.bytes(&l.relay_key).u32(l.counters.len() as u32);
                for c in &l.counters {
                    w = w.bytes(c);
                }
                w.u64(l.heartbeat_every_secs).u64(l.check_in_grace_secs)
            }
        };
        w.opt_bytes(c.own_wallet.as_deref())
    }

    /// The fields of `queue_refund`.
    pub fn refund(
        self,
        now: u64,
        counter: &[u8],
        sale: u32,
        cashier: &str,
        address: &str,
        sats: u64,
    ) -> Self {
        self.u64(now)
            .bytes(counter)
            .u32(sale)
            .string(cashier)
            .string(address)
            .u64(sats)
    }
}

/// A request frame.
pub fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBMk".to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

/// What an error response says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub status: u8,
    pub rule: String,
    pub text: String,
    pub words: Words,
}

/// The words of a response, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    pub tone: u8,
    pub who: u8,
    pub heading: String,
    pub what: String,
    pub next: String,
    pub code: Option<String>,
    pub at: Option<u64>,
    pub quoted: Option<(String, String)>,
    pub detail: Option<String>,
}

/// A send row, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Send {
    pub txid: String,
    pub state: u8,
    pub n: u32,
    pub words: Words,
}

/// A stage, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    pub kind: u8,
    pub detail: Option<String>,
    pub words: Words,
}

/// Reads the result of an op.
pub struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }

    /// The payload of an ok response.
    pub fn ok(response: &'a [u8]) -> Rd<'a> {
        assert_eq!(response.get(..4), Some(b"OBMl".as_slice()), "magic");
        assert_eq!(response.get(4), Some(&1), "version");
        assert_eq!(
            response.get(5),
            Some(&0),
            "status of {:?}",
            Self::error(response)
        );
        Rd::new(&response[6..])
    }

    /// The error of an error response.
    pub fn error(response: &'a [u8]) -> Option<Fault> {
        if response.get(5).copied() == Some(0) {
            return None;
        }
        assert_eq!(response.get(..4), Some(b"OBMl".as_slice()), "magic");
        assert_eq!(response.get(4), Some(&1), "version");
        let mut r = Rd::new(&response[6..]);
        let status = response[5];
        let rule = r.str16();
        let text = r.str16();
        let words = r.words();
        r.done();
        Some(Fault {
            status,
            rule,
            text,
            words,
        })
    }

    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = self.b.get(self.p..self.p + n).expect("enough bytes");
        self.p += n;
        s
    }

    pub fn pos(&self) -> usize {
        self.p
    }

    pub fn done(&self) {
        assert_eq!(self.p, self.b.len(), "bytes left over");
    }

    pub fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }

    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().expect("2"))
    }

    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("4"))
    }

    pub fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().expect("8"))
    }

    pub fn bool(&mut self) -> bool {
        match self.u8() {
            0 => false,
            1 => true,
            _ => panic!("not a bool"),
        }
    }

    pub fn bytes(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        self.take(n).to_vec()
    }

    pub fn string(&mut self) -> String {
        String::from_utf8(self.bytes()).expect("UTF-8")
    }

    pub fn str16(&mut self) -> String {
        let n = usize::from(self.u16());
        String::from_utf8(self.take(n).to_vec()).expect("UTF-8")
    }

    pub fn opt<T>(&mut self, f: impl Fn(&mut Self) -> T) -> Option<T> {
        match self.u8() {
            0 => None,
            1 => Some(f(self)),
            _ => panic!("not a flag"),
        }
    }

    pub fn list<T>(&mut self, f: impl Fn(&mut Self) -> T) -> Vec<T> {
        let n = self.u32();
        (0..n).map(|_| f(self)).collect()
    }

    pub fn words(&mut self) -> Words {
        Words {
            tone: self.u8(),
            who: self.u8(),
            heading: self.str16(),
            what: self.str16(),
            next: self.str16(),
            code: self.opt(Rd::str16),
            at: self.opt(Rd::u64),
            quoted: self.opt(|r| (r.str16(), r.str16())),
            detail: self.opt(Rd::str16),
        }
    }

    /// `PumpView`: notes, stopped.
    pub fn pump(&mut self) -> (Vec<String>, bool) {
        (self.list(Rd::string), self.bool())
    }

    pub fn send(&mut self) -> Send {
        Send {
            txid: self.string(),
            state: self.u8(),
            n: self.u32(),
            words: self.words(),
        }
    }

    pub fn stage(&mut self) -> Stage {
        Stage {
            kind: self.u8(),
            detail: self.opt(Rd::string),
            words: self.words(),
        }
    }

    /// `StatusView`; every words value is checked by the caller.
    pub fn status(&mut self) -> Status {
        Status {
            view: self.u8(),
            rescan_to: self.opt(Rd::u32),
            view_words: self.words(),
            receipts_words: self.words(),
            checked: self.u32(),
            tip: self.u32(),
            halted: self.opt(|r| (r.string(), r.words())),
            health: (self.bool(), self.bool(), self.bool(), self.words()),
            balance: self.u64(),
            recovery_pending: self.bool(),
            recovery_words: self.opt(Rd::words),
            stopped: self.bool(),
        }
    }

    /// `ReceiptsView`: kind, payments, cost, rate, words.
    pub fn receipts(&mut self) -> (u8, u64, u64, u64, Words) {
        (self.u8(), self.u64(), self.u64(), self.u64(), self.words())
    }

    /// `BuiltView`: session id, request, skipped (id, reason, words).
    #[allow(clippy::type_complexity)]
    pub fn built(&mut self) -> (Vec<u8>, Vec<u8>, Vec<(u32, String, Words)>) {
        (
            self.bytes(),
            self.bytes(),
            self.list(|r| (r.u32(), r.string(), r.words())),
        )
    }

    /// `TakenView`: session id, outcomes (item, kind, finished, reason,
    /// words), sends.
    #[allow(clippy::type_complexity)]
    pub fn taken(
        &mut self,
    ) -> (
        Vec<u8>,
        Vec<(u32, u8, u32, Option<String>, Words)>,
        Vec<Send>,
    ) {
        (
            self.bytes(),
            self.list(|r| (r.u32(), r.u8(), r.u32(), r.opt(Rd::string), r.words())),
            self.list(Rd::send),
        )
    }

    /// `QueueView`'s refunds, cash-outs and others.
    pub fn queue(&mut self) -> Queue {
        Queue {
            refunds: self.list(|r| RefundRow {
                id: r.u32(),
                counter: r.bytes(),
                sale: r.u32(),
                cashier: r.string(),
                address: r.string(),
                sats: r.u64(),
                stage: r.stage(),
                waiting: r.opt(Rd::bool),
                received: r.opt(Rd::u64),
            }),
            cash_outs: self.list(|r| CashRow {
                id: r.u32(),
                address: r.string(),
                amount: match r.u8() {
                    0 => Some(r.u64()),
                    1 => None,
                    t => panic!("amount tag {t}"),
                },
                label: r.string(),
                stage: r.stage(),
            }),
            others: self.list(|r| (r.u32(), r.u8(), r.stage())),
        }
    }

    /// `LinkView`.
    pub fn link(&mut self) -> Link {
        Link {
            reachable: self.bool(),
            refused: self.u64(),
            alerts: self.list(|r| (r.u8(), r.bytes(), r.u64(), r.words())),
            counters: self.list(|r| {
                (
                    r.bytes(),
                    r.u64(),
                    r.bytes(),
                    r.opt(Rd::u64),
                    r.opt(Rd::bool),
                    r.opt(Rd::u64),
                    r.words(),
                )
            }),
            batches: self.list(|r| (r.bytes(), r.u32(), r.u64(), r.words())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub view: u8,
    pub rescan_to: Option<u32>,
    pub view_words: Words,
    pub receipts_words: Words,
    pub checked: u32,
    pub tip: u32,
    pub halted: Option<(String, Words)>,
    /// known, healthy, behind, words
    pub health: (bool, bool, bool, Words),
    pub balance: u64,
    pub recovery_pending: bool,
    pub recovery_words: Option<Words>,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRow {
    pub id: u32,
    pub counter: Vec<u8>,
    pub sale: u32,
    pub cashier: String,
    pub address: String,
    pub sats: u64,
    pub stage: Stage,
    pub waiting: Option<bool>,
    pub received: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashRow {
    pub id: u32,
    pub address: String,
    /// `None` for all.
    pub amount: Option<u64>,
    pub label: String,
    pub stage: Stage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    pub refunds: Vec<RefundRow>,
    pub cash_outs: Vec<CashRow>,
    pub others: Vec<(u32, u8, Stage)>,
}

#[allow(clippy::type_complexity)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub reachable: bool,
    pub refused: u64,
    pub alerts: Vec<(u8, Vec<u8>, u64, Words)>,
    pub counters: Vec<(
        Vec<u8>,
        u64,
        Vec<u8>,
        Option<u64>,
        Option<bool>,
        Option<u64>,
        Words,
    )>,
    pub batches: Vec<(Vec<u8>, u32, u64, Words)>,
}
