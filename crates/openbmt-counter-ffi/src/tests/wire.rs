//! A request writer and a response reader written separately from the
//! encoder under test: the field order is written out again here.

use crate::StartConfig;

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
        self.string(&c.dir)
            .u64(c.now)
            .bytes(&c.record)
            .opt_string(c.proxy.as_deref())
            .opt_string(c.own_node.as_deref())
            .opt_bytes(c.lightning.as_deref())
            .opt_string(c.link_proxy.as_deref())
    }

    /// The fields of `open`.
    pub fn open(self, now: u64, minor: u64, currency: &str, rate: u64, label: &str) -> Self {
        self.u64(now)
            .u64(minor)
            .string(currency)
            .u64(rate)
            .string(label)
            .u64(now)
            .u8(0)
    }

    /// The fields of `open` with a cashier at the till (CR-151).
    pub fn open_by(
        self,
        now: u64,
        minor: u64,
        currency: &str,
        rate: u64,
        label: &str,
        name: &str,
    ) -> Self {
        self.u64(now)
            .u64(minor)
            .string(currency)
            .u64(rate)
            .string(label)
            .u64(now)
            .u8(1)
            .actor(false, name)
    }

    /// The fields of `cancel`, with no person at the till.
    pub fn cancel(self, now: u64, index: u32) -> Self {
        self.u64(now).u32(index).u8(0)
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

/// The totals of a day, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub from: u64,
    pub to: u64,
    pub currency: Option<String>,
    pub paid: (u32, u64, u64),
    pub on_chain: (u32, u64, u64),
    pub lightning: (u32, u64, u64),
    pub waiting: (u32, u64, u64),
    pub stopped: u32,
    pub ended: u32,
    pub overrides: (u32, u64, u64),
    pub refunds: (u32, u64, u64),
    pub refunds_withdrawn: u32,
    pub cash_refunds: (u32, u64, u64),
    pub tips: (u32, u64),
    pub unnamed_sales: u32,
    pub people: Vec<Person>,
    pub people_left_out: u32,
}

/// One person's line of the day, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub manager: bool,
    pub name: String,
    pub sales: u32,
    pub overrides: u32,
    pub refunds: u32,
    pub cash_refunds: u32,
    pub tips_minor: u64,
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

    /// A tally: count, minor units, satoshis.
    pub fn tally(&mut self) -> (u32, u64, u64) {
        (self.u32(), self.u64(), self.u64())
    }

    /// The totals of a day (CR-153), in the order of the protocol.
    pub fn day_totals(&mut self) -> Day {
        Day {
            from: self.u64(),
            to: self.u64(),
            currency: self.opt(Rd::string),
            paid: self.tally(),
            on_chain: self.tally(),
            lightning: self.tally(),
            waiting: self.tally(),
            stopped: self.u32(),
            ended: self.u32(),
            overrides: self.tally(),
            refunds: self.tally(),
            refunds_withdrawn: self.u32(),
            cash_refunds: self.tally(),
            tips: (self.u32(), self.u64()),
            unnamed_sales: self.u32(),
            people: self.list(|r| Person {
                manager: r.bool_role(),
                name: r.string(),
                sales: r.u32(),
                overrides: r.u32(),
                refunds: r.u32(),
                cash_refunds: r.u32(),
                tips_minor: r.u64(),
            }),
            people_left_out: self.u32(),
        }
    }

    fn bool_role(&mut self) -> bool {
        match self.u8() {
            0 => false,
            1 => true,
            t => panic!("role tag {t}"),
        }
    }

    pub fn screen(&mut self) -> Screen {
        Screen {
            state: self.u8(),
            words: self.words(),
            flags: self.list(Rd::words),
            check: self.opt(Rd::words),
            received: self.u64(),
            needs: self.u64(),
            stuck: self.bool(),
            late: self.bool(),
            reopened: self.bool(),
            conflict_seen: self.bool(),
            reports: self.reports(),
            payment: self.opt(Rd::payment),
            lightning: self.opt(|r| LnSale {
                invoice: r.bool(),
                waiting: r.bool(),
                unavailable: r.opt(Rd::words),
                not_for_this_sale: r.bool(),
                failed: r.opt(Rd::words),
            }),
        }
    }

    /// The link's view.
    pub fn link(&mut self) -> LinkV {
        LinkV {
            words: self.words(),
            connected: self.bool(),
            since: self.opt(Rd::u64),
            refused: self.bool(),
            heartbeats_taken: self.u64(),
            owner_records: self.u64(),
            last_check_in_at: self.opt(Rd::u64),
            settings_version: self.u32(),
            restart_needed: self.bool(),
            settings_line: self.opt(Rd::words),
        }
    }

    pub fn pump(&mut self) -> (Vec<Report>, Vec<String>, bool) {
        (self.reports(), self.list(Rd::string), self.bool())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkV {
    pub words: Words,
    pub connected: bool,
    pub since: Option<u64>,
    pub refused: bool,
    pub heartbeats_taken: u64,
    pub owner_records: u64,
    pub last_check_in_at: Option<u64>,
    pub settings_version: u32,
    pub restart_needed: bool,
    pub settings_line: Option<Words>,
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
    pub check: Option<Words>,
    pub received: u64,
    pub needs: u64,
    pub stuck: bool,
    pub late: bool,
    pub reopened: bool,
    pub conflict_seen: bool,
    pub reports: Vec<Report>,
    pub payment: Option<Payment>,
    pub lightning: Option<LnSale>,
}

/// What a screen shows of Lightning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnSale {
    pub invoice: bool,
    pub waiting: bool,
    pub unavailable: Option<Words>,
    pub not_for_this_sale: bool,
    pub failed: Option<Words>,
}
