//! A request writer and a response reader written separately from the
//! encoder under test: the field order is written out again here.

use crate::{CounterNetwork, StartConfig};

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

    /// An actor: the role tag (0 cashier, 1 manager) and the name.
    pub fn actor(self, manager: bool, name: &str) -> Self {
        self.u8(u8::from(manager)).string(name)
    }

    /// The fields of `start`, in the order of the protocol.
    pub fn start(self, c: &StartConfig) -> Self {
        let network = match c.network {
            CounterNetwork::Mainnet => 0,
            CounterNetwork::Testnet => 1,
            CounterNetwork::Signet => 2,
            CounterNetwork::Regtest => 3,
        };
        let mut w = self
            .string(&c.dir)
            .u8(network)
            .u64(c.now)
            .bytes(&c.identity_secret)
            .bytes(&c.owner_root)
            .bytes(&c.approver_root)
            .bytes(&c.wallet_checksum)
            .u32(c.batch_min_count)
            .bytes(&c.settings)
            .opt_bytes(c.first_batch.as_deref())
            .opt_string(c.proxy.as_deref())
            .u32(u32::try_from(c.fixed_peers.len()).expect("fits"));
        for p in &c.fixed_peers {
            w = w.string(p);
        }
        w
    }

    /// The fields of `open`.
    pub fn open(self, now: u64, minor: u64, currency: &str, rate: u64, label: &str) -> Self {
        self.u64(now)
            .u64(minor)
            .string(currency)
            .u64(rate)
            .string(label)
            .u64(now)
    }
}

/// A request frame.
pub fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBMm".to_vec();
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
        assert_eq!(response.get(..4), Some(b"OBMn".as_slice()), "magic");
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
        assert_eq!(response.get(..4), Some(b"OBMn".as_slice()), "magic");
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

    /// A payment: address, uri, copy uri, size, modules.
    pub fn payment(&mut self) -> Payment {
        Payment {
            address: self.string(),
            uri: self.string(),
            copy_uri: self.string(),
            size: self.u32(),
            bits: self.bytes(),
        }
    }

    pub fn report(&mut self) -> Report {
        match self.u8() {
            0 => Report::PosPaid(self.u32()),
            1 => Report::PosUnpaid(self.u32()),
            2 => Report::BooksFinal {
                index: self.u32(),
                amount: self.u64(),
                currency: self.string(),
                received: self.u64(),
            },
            3 => Report::SurplusOwed(self.u32(), self.u64()),
            4 => Report::LatePayment(self.u32(), self.u64()),
            5 => Report::Alarm(self.u32()),
            6 => Report::Dust(self.u32(), self.u64()),
            t => panic!("report tag {t}"),
        }
    }

    pub fn reports(&mut self) -> Vec<Report> {
        self.list(Rd::report)
    }

    /// An opened sale or a new quote.
    pub fn opened(&mut self) -> Opened {
        Opened {
            index: self.u32(),
            needs_sats: self.u64(),
            quote_rate: self.u64(),
            quote_from: self.u64(),
            quote_until: self.u64(),
            payment: self.payment(),
        }
    }

    pub fn screen(&mut self) -> Screen {
        Screen {
            state: self.u8(),
            words: self.words(),
            flags: self.list(Rd::words),
            received: self.u64(),
            needs: self.u64(),
            stuck: self.bool(),
            late: self.bool(),
            reopened: self.bool(),
            conflict_seen: self.bool(),
            reports: self.reports(),
            payment: self.opt(Rd::payment),
        }
    }

    pub fn pump(&mut self) -> (Vec<Report>, Vec<String>, bool) {
        (self.reports(), self.list(Rd::string), self.bool())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    pub address: String,
    pub uri: String,
    pub copy_uri: String,
    pub size: u32,
    pub bits: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    PosPaid(u32),
    PosUnpaid(u32),
    BooksFinal {
        index: u32,
        amount: u64,
        currency: String,
        received: u64,
    },
    SurplusOwed(u32, u64),
    LatePayment(u32, u64),
    Alarm(u32),
    Dust(u32, u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub index: u32,
    pub needs_sats: u64,
    pub quote_rate: u64,
    pub quote_from: u64,
    pub quote_until: u64,
    pub payment: Payment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub state: u8,
    pub words: Words,
    pub flags: Vec<Words>,
    pub received: u64,
    pub needs: u64,
    pub stuck: bool,
    pub late: bool,
    pub reopened: bool,
    pub conflict_seen: bool,
    pub reports: Vec<Report>,
    pub payment: Option<Payment>,
}
