//! The crate's own-node code against a running LND and a running Core
//! Lightning on regtest, over real HTTPS. `scripts/lightning-e2e-core.sh`
//! starts the nodes and runs this.
//!
//! ```text
//! lightning_regtest run       --key value ...
//! lightning_regtest jvm-check --key value ...
//! ```
//!
//! Each check prints `PASS <rule> <what>` or `FAIL <rule> <what>: <evidence>`;
//! lines that start with spaces are what the nodes answered. The exit status
//! is 0 only if every check passed.
//!
//! The crate does no TLS, so the transport here is `openssl s_client` (it
//! takes the certificate the node shows) and `curl`: a request is sent only
//! if `CertPin::accepts` says yes to the certificate that was shown, which is
//! what the app's transport does (LN-12). The JVM test of the counter app's
//! `LightningTransport` does the same on the platform's own HTTPS; `run`
//! writes the requests it sends and `jvm-check` reads what it got back.
//!
//! **Test only.** Everything is regtest on 127.0.0.1 with keys made for the
//! run.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::Network;
use openbmt_json::{parse, Limits};
use openbmt_lightning::bip321::PaymentUri;
use openbmt_lightning::bolt11::{self, Checked};
use openbmt_lightning::connect::{Connected, ConnectionTest, Next};
use openbmt_lightning::invoice::{Ask, CreateOutcome, Created, NotPaid, Payment};
use openbmt_lightning::node::check_created;
use openbmt_lightning::{
    CertPin, ClnCredential, ConnectionRefused, Credential, Endpoint, InvoiceRefused, LndCredential,
    Method, Reply, Request,
};

/// The first four bytes of the counter's id in the test.
const COUNTER: [u8; 4] = [0x0b, 0x0b, 0x0b, 0x0b];
/// The amount of the test sales, in satoshis.
const SALE_SATS: u64 = 21_000;
/// The lifetime asked for, in seconds (the counter's quote, CR-05).
const EXPIRY: u32 = 300;

type Res<T> = Result<T, String>;

// ---- the report -----------------------------------------------------------------

#[derive(Default)]
struct Report {
    passed: u32,
    failed: u32,
}

impl Report {
    fn pass(&mut self, rule: &str, what: &str) {
        self.passed += 1;
        println!("PASS {rule} {what}");
    }

    fn fail(&mut self, rule: &str, what: &str, evidence: &str) {
        self.failed += 1;
        println!("FAIL {rule} {what}: {evidence}");
    }

    fn check(&mut self, rule: &str, what: &str, result: Res<()>) {
        match result {
            Ok(()) => self.pass(rule, what),
            Err(e) => self.fail(rule, what, &e),
        }
    }
}

fn note(text: &str) {
    println!("      {text}");
}

/// What a node answered, for the printout: the status and the body, cut.
fn shown(reply: &Reply) -> String {
    let body = String::from_utf8_lossy(&reply.body);
    let body: String = body.chars().filter(|c| !c.is_control()).take(240).collect();
    let state = json_of(reply).and_then(|v| {
        let own = v.get("state").and_then(|s| s.as_str()).map(str::to_string);
        own.or_else(|| {
            v.get("invoices")
                .and_then(|l| l.as_array())
                .and_then(|l| l.first())
                .and_then(|i| i.get("status"))
                .and_then(|s| s.as_str())
                .map(str::to_string)
        })
    });
    match state {
        Some(state) => format!("HTTP {} [state {state}] {body}", reply.status),
        None => format!("HTTP {} {body}", reply.status),
    }
}

// ---- the arguments ----------------------------------------------------------------

struct Args(HashMap<String, String>);

impl Args {
    fn get(&self, key: &str) -> Res<&str> {
        self.0
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("missing --{key}"))
    }

    fn file(&self, key: &str) -> Res<Vec<u8>> {
        let path = self.get(key)?;
        fs::read(path).map_err(|e| format!("{path}: {e}"))
    }

    fn text(&self, key: &str) -> Res<String> {
        String::from_utf8(self.file(key)?).map_err(|e| e.to_string())
    }
}

fn parse_args(list: &[String]) -> Res<Args> {
    let mut map = HashMap::new();
    let mut it = list.iter();
    while let Some(k) = it.next() {
        let key = k.strip_prefix("--").ok_or("an argument is not a --key")?;
        let value = it.next().ok_or("a --key has no value")?;
        map.insert(key.to_string(), value.clone());
    }
    Ok(Args(map))
}

fn node_key(hex: &str) -> Res<[u8; 33]> {
    let hex = hex.trim();
    if hex.len() != 66 {
        return Err(format!("a node key is 66 hex digits: {hex}"));
    }
    let mut out = [0u8; 33];
    for (i, byte) in out.iter_mut().enumerate() {
        let pair = hex.get(i * 2..i * 2 + 2).ok_or("short key")?;
        *byte = u8::from_str_radix(pair, 16).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn unhex32(hex: &str) -> Res<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err("a hash is 64 hex digits".into());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let pair = hex.get(i * 2..i * 2 + 2).ok_or("short hash")?;
        *byte = u8::from_str_radix(pair, 16).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- the transport: the certificate is pinned, then curl ------------------------

/// The certificate the node at `host_port` shows, as DER.
fn presented_der(host_port: &str) -> Res<Vec<u8>> {
    let shown = Command::new("openssl")
        .args([
            "s_client",
            "-connect",
            host_port,
            "-servername",
            "localhost",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("openssl s_client: {e}"))?;
    let mut x509 = Command::new("openssl")
        .args(["x509", "-outform", "DER"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("openssl x509: {e}"))?;
    if let Some(mut stdin) = x509.stdin.take() {
        stdin
            .write_all(&shown.stdout)
            .map_err(|e| format!("openssl x509: {e}"))?;
    }
    let der = x509
        .wait_with_output()
        .map_err(|e| format!("openssl x509: {e}"))?;
    if der.stdout.is_empty() {
        return Err(format!("{host_port} showed no certificate"));
    }
    Ok(der.stdout)
}

/// Send one request over HTTPS, only to a node that shows the pinned
/// certificate (LN-12).
fn send(endpoint: &Endpoint, request: &Request) -> Res<Reply> {
    let host_port = endpoint.host_port();
    let der = presented_der(&host_port)?;
    if !endpoint.pin().accepts(&der) {
        return Err(format!(
            "pin refused: {host_port} showed a certificate with sha256 {}",
            hex(&sha256::Hash::hash(&der).to_byte_array())
        ));
    }
    let mut curl = Command::new("curl");
    curl.args(["-s", "-k", "--http1.1", "-m", "30", "-X"])
        .arg(match request.method {
            Method::Get => "GET",
            Method::Post => "POST",
        })
        .arg("-H")
        .arg(format!("{}: {}", request.header_name, request.header_value));
    if let Some(body) = &request.body {
        curl.args(["-H", "Content-Type: application/json", "--data-binary"])
            .arg(body);
    }
    curl.args(["-w", "\n%{http_code}"])
        .arg(format!("https://{host_port}{}", request.path));
    let out = curl.output().map_err(|e| format!("curl: {e}"))?;
    let text = out.stdout;
    let cut = text
        .iter()
        .rposition(|b| *b == b'\n')
        .ok_or("curl gave no status")?;
    let status: u16 = String::from_utf8_lossy(text.get(cut + 1..).unwrap_or(&[]))
        .trim()
        .parse()
        .map_err(|_| "curl gave no status".to_string())?;
    if status == 0 {
        return Err(format!("no answer from {host_port}"));
    }
    Ok(Reply {
        status,
        body: text.get(..cut).unwrap_or(&[]).to_vec(),
    })
}

// ---- helpers over the crate -----------------------------------------------------

fn json_of(reply: &Reply) -> Option<openbmt_json::Value> {
    parse(
        &reply.body,
        &Limits {
            max_bytes: 256 * 1024,
            max_depth: 16,
            max_values: 10_000,
        },
    )
    .ok()
}

/// Whether the reply is exactly what `status`, `code` and a message that
/// `message_ok` accepts say.
fn answered(reply: &Reply, status: u16, code: i64, message_ok: impl Fn(&str) -> bool) -> bool {
    let Some(v) = json_of(reply) else {
        return false;
    };
    reply.status == status
        && v.get("code").and_then(|c| c.as_i64()) == Some(code)
        && v.get("message")
            .and_then(|m| m.as_str())
            .is_some_and(message_ok)
}

enum Connect {
    Done(Connected),
    Refused(ConnectionRefused),
    Transport(String),
}

/// Run the three requests of LN-20 against the node, keeping what it said.
fn connect(
    credential: &Credential,
    node: [u8; 33],
    test_id: u64,
    trace: &mut Vec<(String, Reply)>,
) -> Connect {
    let mut test =
        ConnectionTest::new(credential.clone(), Network::Regtest, node, COUNTER, test_id);
    loop {
        let request = test.request();
        let reply = match send(credential.endpoint(), &request) {
            Ok(r) => r,
            Err(e) => return Connect::Transport(e),
        };
        trace.push((
            format!("{:?} {}", request.method, request.path),
            reply.clone(),
        ));
        match test.accept(&reply) {
            Ok(Next::More(next)) => test = next,
            Ok(Next::Done(done)) => return Connect::Done(done),
            Err(e) => return Connect::Refused(e),
        }
    }
}

fn print_trace(trace: &[(String, Reply)]) {
    for (call, reply) in trace {
        note(&format!("{call} -> {}", shown(reply)));
    }
}

/// One node as the checks see it.
struct Node {
    name: &'static str,
    key: [u8; 33],
    endpoint_pem: String,
    credential: Credential,
    /// Runs the other node's payment of an invoice.
    pays: String,
    is_lnd: bool,
}

fn ask_for(sale: u64, attempt: u32, expiry: u32) -> Res<Ask> {
    Ask::sale(COUNTER, sale, attempt, SALE_SATS * 1_000, expiry).map_err(|e| e.to_string())
}

/// Make the invoice of a sale through the crate and check it (LN-13, LN-17).
fn make_invoice(node: &Node, ask: &Ask) -> Res<(Created, Checked)> {
    let request = node.credential.create_request(ask);
    let reply = send(node.credential.endpoint(), &request)?;
    let created = match node
        .credential
        .parse_create(&reply)
        .map_err(|e| format!("{e}; {}", shown(&reply)))?
    {
        CreateOutcome::Created(c) => c,
        CreateOutcome::LabelExists => return Err("the label is used".into()),
    };
    let checked = check_created(&created, ask, Network::Regtest, node.key)
        .map_err(|e| format!("{e}; {}", shown(&reply)))?;
    Ok((created, checked))
}

fn lookup(node: &Node, hash: &[u8; 32]) -> Res<(Reply, Payment)> {
    let reply = send(
        node.credential.endpoint(),
        &node.credential.lookup_request(hash),
    )?;
    // With LNREAL_CAPTURE set, every lookup reply is kept whole, one per line,
    // so that tests can use what the nodes really wrote.
    if let Some(path) = std::env::var_os("LNREAL_CAPTURE") {
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let line = format!(
                "{} {} {} {}\n",
                node.name,
                hex(hash),
                reply.status,
                String::from_utf8_lossy(&reply.body).replace('\n', " ")
            );
            let _ = f.write_all(line.as_bytes());
        }
    }
    let payment = node
        .credential
        .parse_lookup(&reply, hash, SALE_SATS * 1_000)
        .map_err(|e| format!("{e}; {}", shown(&reply)))?;
    Ok((reply, payment))
}

fn pay(node: &Node, bolt11: &str) -> Res<()> {
    let out = Command::new(&node.pays)
        .arg(bolt11)
        .output()
        .map_err(|e| format!("{}: {e}", node.pays))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "the other node did not pay: {}",
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(300)
                .collect::<String>()
        ))
    }
}

// ---- the checks of one node -----------------------------------------------------

/// TLS (LN-12): the pin accepts the certificate the node really shows and
/// refuses another one.
fn tls_checks(r: &mut Report, node: &Node, other: &str, other_pem: &str, fingerprint: &str) {
    let host_port = node.credential.endpoint().host_port();
    let res = (|| {
        let der = presented_der(&host_port)?;
        let pin = CertPin::from_pem(&node.endpoint_pem).map_err(|e| e.to_string())?;
        if !pin.accepts(&der) {
            return Err("the PEM the node wrote does not match the certificate it shows".into());
        }
        let by_hash = CertPin::from_sha256_hex(fingerprint).map_err(|e| e.to_string())?;
        if !by_hash.accepts(&der) {
            return Err(format!(
                "the fingerprint {fingerprint} of openssl does not match what is shown"
            ));
        }
        let reply = send(
            node.credential.endpoint(),
            &node.credential.lookup_request(&[7; 32]),
        )?;
        note(&format!(
            "{} over the pinned certificate: {}",
            node.name,
            shown(&reply)
        ));
        Ok(())
    })();
    r.check(
        "LN-12",
        &format!(
            "{}: the pinned path accepts the node's real certificate (PEM and SHA-256)",
            node.name
        ),
        res,
    );
    let res = (|| {
        let wrong_pin = CertPin::from_pem(other_pem).map_err(|e| e.to_string())?;
        let ep = Endpoint::parse(&host_port, wrong_pin).map_err(|e| e.to_string())?;
        match send(&ep, &node.credential.lookup_request(&[7; 32])) {
            Err(e) if e.starts_with("pin refused") => {
                note(&e);
                Ok(())
            }
            Err(e) => Err(format!("failed in another way: {e}")),
            Ok(r) => Err(format!("the request was sent: {}", shown(&r))),
        }?;
        // A server that shows another certificate, against the node's pin.
        let seen = presented_der(other)?;
        if node.credential.endpoint().pin().accepts(&seen) {
            return Err("the pin accepted another certificate".into());
        }
        match send(
            &Endpoint::parse(other, node.credential.endpoint().pin().clone())
                .map_err(|e| e.to_string())?,
            &node.credential.lookup_request(&[7; 32]),
        ) {
            Err(e) if e.starts_with("pin refused") => {
                note(&e);
                Ok(())
            }
            other => Err(format!(
                "a different certificate was not refused: {other:?}"
            )),
        }
    })();
    r.check(
        "LN-12",
        &format!(
            "{}: a different certificate is refused before any request is sent",
            node.name
        ),
        res,
    );
}

/// What a sale does against the node: invoice, unpaid, paid by the other
/// node, expired, not found (LN-13, LN-14, LN-17, LN-18).
fn sale_checks(
    r: &mut Report,
    node: &Node,
    other_key: [u8; 33],
    address: &str,
    sale: u64,
) -> Option<(Checked, [u8; 32])> {
    let name = node.name;
    let ask = match ask_for(sale, 0, EXPIRY) {
        Ok(a) => a,
        Err(e) => {
            r.fail("LN-13", &format!("{name}: the ask"), &e);
            return None;
        }
    };
    let made = make_invoice(node, &ask);
    let (created, checked) = match made {
        Ok(v) => v,
        Err(e) => {
            r.fail("LN-17", &format!("{name}: an invoice for {SALE_SATS} sat made through the crate passes the check"), &e);
            return None;
        }
    };
    let detail = bolt11::decode(&checked.bolt11).map_err(|e| e.to_string());
    r.check(
        "LN-17",
        &format!("{name}: the invoice has the amount, the expiry, the description and the node's key as signer"),
        detail.and_then(|d| {
            note(&format!(
                "{name} invoice: network {} amount {:?} msat expiry {} s signer {} description {:?}",
                d.network,
                d.amount_msat,
                d.expiry_secs,
                hex(&d.signer),
                d.description
            ));
            if d.amount_msat != Some(SALE_SATS * 1_000) {
                return Err(format!("amount {:?}", d.amount_msat));
            }
            if d.expiry_secs != u64::from(EXPIRY) {
                return Err(format!("expiry {}", d.expiry_secs));
            }
            if d.signer != node.key {
                return Err("the signer is not the node key".into());
            }
            if d.payment_hash != created.payment_hash {
                return Err("the payment hash is not the node's".into());
            }
            Ok(())
        }),
    );
    r.check(
        "LN-17",
        &format!("{name}: the same invoice is refused against another node's key"),
        match check_created(&created, &ask, Network::Regtest, other_key) {
            Err(InvoiceRefused::WrongNode) => Ok(()),
            other => Err(format!("{other:?}")),
        },
    );
    // The unified code.
    r.check(
        "LN-18",
        &format!("{name}: the BIP 321 code holds the invoice the node made"),
        (|| {
            let uri = PaymentUri::new(address, Network::Regtest, SALE_SATS, &checked)
                .map_err(|e| e.to_string())?;
            note(&format!(
                "{name} code: {}",
                uri.text().chars().take(120).collect::<String>()
            ));
            let want = format!("&lightning={}", created.bolt11.to_ascii_lowercase());
            if !uri.text().ends_with(&want) || !uri.text().contains("?amount=0.00021&") {
                return Err(format!("the code is {}", uri.text()));
            }
            let qr = uri.qr_text().map_err(|e| e.to_string())?;
            if !qr.contains(&created.bolt11.to_ascii_uppercase()) {
                return Err("the QR text lacks the invoice".into());
            }
            Ok(())
        })(),
    );
    // Not paid yet.
    r.check(
        "LN-14",
        &format!("{name}: an unpaid invoice is not reported paid"),
        lookup(node, &created.payment_hash).and_then(|(reply, p)| {
            note(&format!(
                "{name} lookup of the unpaid invoice -> {}",
                shown(&reply)
            ));
            match p {
                Payment::NotPaid(NotPaid::Waiting) => Ok(()),
                other => Err(format!("{other:?}")),
            }
        }),
    );
    // Paid by the other node.
    let paid = pay(node, &checked.bolt11).and_then(|()| {
        for _ in 0..30 {
            let (reply, p) = lookup(node, &created.payment_hash)?;
            if let Payment::Paid(facts) = p {
                note(&format!(
                    "{name} lookup of the paid invoice -> {}",
                    shown(&reply)
                ));
                if facts.amount_received_msat < SALE_SATS * 1_000 {
                    return Err(format!("received {}", facts.amount_received_msat));
                }
                if facts.settled_at.is_none() {
                    return Err("no settle time".into());
                }
                return Ok(());
            }
            sleep(Duration::from_secs(1));
        }
        Err("not reported paid within 30 s of the payment".into())
    });
    r.check(
        "LN-14",
        &format!("{name}: the other node pays it and the crate's paid check sees it paid"),
        paid,
    );
    // An invoice the node does not have.
    r.check(
        "LN-14",
        &format!("{name}: a payment hash the node does not know is 'not found'"),
        lookup(node, &[0x5a; 32]).and_then(|(reply, p)| {
            note(&format!(
                "{name} lookup of an unknown hash -> {}",
                shown(&reply)
            ));
            match p {
                Payment::NotPaid(NotPaid::NotFound) => Ok(()),
                other => Err(format!("{other:?}")),
            }
        }),
    );
    // Expired.
    r.check(
        "LN-14",
        &format!(
            "{name}: an expired invoice is reported as {}",
            if node.is_lnd { "cancelled" } else { "expired" }
        ),
        (|| {
            let short = ask_for(sale + 1, 0, 1)?;
            let (c, _) = make_invoice(node, &short)?;
            let mut last = String::new();
            for _ in 0..40 {
                sleep(Duration::from_secs(1));
                let (reply, p) = lookup(node, &c.payment_hash)?;
                last = shown(&reply);
                match (&p, node.is_lnd) {
                    (Payment::NotPaid(NotPaid::Cancelled), true)
                    | (Payment::NotPaid(NotPaid::Expired), false) => {
                        note(&format!("{name} lookup of the expired invoice -> {last}"));
                        return Ok(());
                    }
                    (Payment::NotPaid(NotPaid::Waiting), _) => {}
                    (other, _) => return Err(format!("{other:?}; {last}")),
                }
            }
            Err(format!("still waiting 40 s after a 1 s expiry; {last}"))
        })(),
    );
    Some((checked, created.payment_hash))
}

// ---- LND ------------------------------------------------------------------------

fn lnd_node(a: &Args) -> Res<Node> {
    let pem = a.text("lnd-cert")?;
    let pin = CertPin::from_pem(&pem).map_err(|e| e.to_string())?;
    let endpoint = Endpoint::parse(a.get("lnd-host")?, pin).map_err(|e| e.to_string())?;
    Ok(Node {
        name: "LND",
        key: node_key(&a.text("lnd-key")?)?,
        endpoint_pem: pem,
        credential: Credential::Lnd(
            LndCredential::new(endpoint, &a.file("lnd-baked-mac")?).map_err(|e| e.to_string())?,
        ),
        pays: a.get("cln-pays")?.to_string(),
        is_lnd: true,
    })
}

fn lnd_checks(r: &mut Report, a: &Args, node: &Node, test_id: &mut u64) -> Res<()> {
    let endpoint = node.credential.endpoint().clone();
    let key = node.key;
    let mut next_id = || {
        *test_id += 1;
        *test_id
    };

    // LN-06: the macaroons.
    let probe_trace = |mac: &[u8], id: u64| -> (Connect, Vec<(String, Reply)>) {
        let mut trace = Vec::new();
        let cred = match LndCredential::new(endpoint.clone(), mac) {
            Ok(c) => Credential::Lnd(c),
            Err(e) => return (Connect::Transport(format!("credential: {e}")), trace),
        };
        let out = connect(&cred, key, id, &mut trace);
        (out, trace)
    };
    let too_strong = |c: &Connect, call: &str| match c {
        Connect::Refused(ConnectionRefused::CredentialTooStrong { call: got, .. })
            if *got == call =>
        {
            Ok(())
        }
        Connect::Refused(e) => Err(format!("refused with another reason: {e}")),
        Connect::Done(_) => Err("accepted".to_string()),
        Connect::Transport(e) => Err(e.clone()),
    };

    // Before the setup tests make any invoice: a node with none at all.
    r.check(
        "LN-14",
        "LND: a lookup on a node with no invoice at all is 'not found'",
        lookup(node, &[0x5a; 32]).and_then(|(reply, p)| {
            note(&format!(
                "LND lookup with no invoice on the node -> {}",
                shown(&reply)
            ));
            match p {
                Payment::NotPaid(NotPaid::NotFound) => Ok(()),
                other => Err(format!("{other:?}")),
            }
        }),
    );

    let admin = a.file("lnd-admin-mac")?;
    let (out, trace) = probe_trace(&admin, next_id());
    print_trace(&trace);
    r.check(
        "LN-06",
        "LND: the admin macaroon is refused by the setup probe (getinfo allowed)",
        too_strong(&out, "GET /v1/getinfo"),
    );

    let stock = a.file("lnd-stock-mac")?;
    let (out, trace) = probe_trace(&stock, next_id());
    print_trace(&trace);
    r.check(
        "LN-05",
        "LND: the stock invoice.macaroon is refused (getinfo denied, balance/blockchain allowed)",
        (|| {
            if !trace
                .first()
                .is_some_and(|(_, rep)| answered(rep, 500, 2, |m| m == "permission denied"))
            {
                return Err("the first probe was not denied as LN-06 says".into());
            }
            too_strong(&out, "GET /v1/balance/blockchain")
        })(),
    );

    let baked = a.file("lnd-baked-mac")?;
    let (out, trace) = probe_trace(&baked, next_id());
    print_trace(&trace);
    r.check(
        "LN-06",
        "LND: both probe calls get HTTP 500, code 2, 'permission denied' for the baked invoices:read invoices:write macaroon",
        (|| {
            if trace.len() < 2 {
                return Err(format!("only {} probe replies", trace.len()));
            }
            for (call, rep) in trace.iter().take(2) {
                if !answered(rep, 500, 2, |m| m == "permission denied") {
                    return Err(format!("{call} answered {}", shown(rep)));
                }
            }
            Ok(())
        })(),
    );
    r.check(
        "LN-20",
        "LND: the baked macaroon is accepted: the test invoice is made and signed by the node key",
        match &out {
            Connect::Done(d) => {
                note(&format!(
                    "test invoice {}",
                    d.test_invoice.bolt11.chars().take(60).collect::<String>()
                ));
                Ok(())
            }
            Connect::Refused(e) => Err(e.to_string()),
            Connect::Transport(e) => Err(e.clone()),
        },
    );

    // LN-11: the connect string, built from the real certificate and macaroon.
    r.check(
        "LN-11",
        "LND: the lndconnect:// string made from the node's files gives the same credential",
        (|| {
            let uri = a.text("lnd-connect")?;
            let got = LndCredential::from_lndconnect(uri.trim()).map_err(|e| e.to_string())?;
            let want = LndCredential::new(endpoint.clone(), &baked).map_err(|e| e.to_string())?;
            if got.macaroon() != want.macaroon()
                || got.endpoint().pin().sha256() != endpoint.pin().sha256()
            {
                return Err("the credential differs".into());
            }
            Ok(())
        })(),
    );
    Ok(())
}

// ---- Core Lightning -------------------------------------------------------------

fn cln_node(a: &Args) -> Res<Node> {
    let pem = a.text("cln-cert")?;
    let pin = CertPin::from_pem(&pem).map_err(|e| e.to_string())?;
    let endpoint = Endpoint::parse(a.get("cln-host")?, pin).map_err(|e| e.to_string())?;
    Ok(Node {
        name: "CLN",
        key: node_key(&a.text("cln-key")?)?,
        endpoint_pem: pem,
        credential: Credential::Cln(
            ClnCredential::new(endpoint, a.text("cln-rune-good")?.trim())
                .map_err(|e| e.to_string())?,
        ),
        pays: a.get("lnd-pays")?.to_string(),
        is_lnd: false,
    })
}

fn refused_or(
    rune_what: &str,
    result: Result<ClnCredential, openbmt_lightning::CredentialRefused>,
) -> Res<()> {
    match result {
        Err(e) => {
            note(&format!("{rune_what}: refused: {e}"));
            Ok(())
        }
        Ok(_) => Err("accepted".into()),
    }
}

fn cln_checks(r: &mut Report, a: &Args, test_id: &mut u64) -> Res<()> {
    let pem = a.text("cln-cert")?;
    let key = node_key(&a.text("cln-key")?)?;
    let pin = CertPin::from_pem(&pem).map_err(|e| e.to_string())?;
    let endpoint = Endpoint::parse(a.get("cln-host")?, pin).map_err(|e| e.to_string())?;
    let (open, good, wide) = (
        a.text("cln-rune-open")?,
        a.text("cln-rune-good")?,
        a.text("cln-rune-wide")?,
    );
    let (open, good, wide) = (open.trim(), good.trim(), wide.trim());

    // LN-08: what the crate refuses by reading the rune.
    r.check(
        "LN-08",
        "CLN: an unrestricted rune is refused",
        refused_or("unrestricted", ClnCredential::new(endpoint.clone(), open)),
    );
    r.check(
        "LN-08",
        "CLN: a rune for invoice, listinvoices and one method more is refused",
        refused_or(
            "one method more",
            ClnCredential::new(endpoint.clone(), wide),
        ),
    );
    // LN-09: what the node itself says to those runes (the reason the probe exists).
    for (what, rune) in [("unrestricted", open), ("one method more", wide)] {
        let req = Request {
            method: Method::Post,
            path: "/v1/getinfo".into(),
            header_name: "Rune",
            header_value: rune.to_string(),
            body: Some("{}".into()),
        };
        r.check(
            "LN-09",
            &format!(
                "CLN: the {what} rune is allowed getinfo by the node, so the probe would refuse it"
            ),
            send(&endpoint, &req).and_then(|rep| {
                note(&format!(
                    "POST /v1/getinfo with the {what} rune -> {}",
                    shown(&rep).chars().take(80).collect::<String>()
                ));
                if rep.is_success() {
                    Ok(())
                } else {
                    Err(shown(&rep))
                }
            }),
        );
    }
    let credential = match ClnCredential::new(endpoint.clone(), good) {
        Ok(c) => Credential::Cln(c),
        Err(e) => {
            r.fail(
                "LN-08",
                "CLN: the rune restricted to invoice and listinvoices is accepted",
                &e.to_string(),
            );
            return Ok(());
        }
    };
    r.pass(
        "LN-08",
        "CLN: the rune restricted to invoice and listinvoices is accepted",
    );
    *test_id += 1;
    let mut trace = Vec::new();
    let out = connect(&credential, key, *test_id, &mut trace);
    print_trace(&trace);
    r.check(
        "LN-09",
        "CLN: both probe calls get HTTP 401, code 1502, 'Not permitted:' for the invoices-only rune",
        (|| {
            if trace.len() < 2 {
                return Err(format!("only {} probe replies", trace.len()));
            }
            for (call, rep) in trace.iter().take(2) {
                if !answered(rep, 401, 1502, |m| m.starts_with("Not permitted:")) {
                    return Err(format!("{call} answered {}", shown(rep)));
                }
            }
            Ok(())
        })(),
    );
    r.check(
        "LN-20",
        "CLN: the invoices-only rune is accepted: the test invoice is made and signed by the node key",
        match out {
            Connect::Done(d) => {
                note(&format!("test invoice {}", d.test_invoice.bolt11.chars().take(60).collect::<String>()));
                Ok(())
            }
            Connect::Refused(e) => Err(e.to_string()),
            Connect::Transport(e) => Err(e),
        },
    );

    // LN-09: a rune of another node, a revoked rune, and a rune whose text has padding.
    for (file, what, rule_what) in [
        (
            "cln-rune-forged",
            "a rune that is not from this node is told apart (RuneNotFromNode)",
            "forged",
        ),
        (
            "cln-rune-revoked",
            "a revoked rune is told apart (RuneRevoked)",
            "revoked",
        ),
        (
            "cln-rune-padded",
            "a rune with base64 padding (a unique id of two digits) is accepted",
            "padded",
        ),
    ] {
        let rune = a.text(file)?;
        let rune = rune.trim();
        *test_id += 1;
        let mut trace = Vec::new();
        let outcome = match ClnCredential::new(endpoint.clone(), rune) {
            Ok(c) => connect(&Credential::Cln(c), key, *test_id, &mut trace),
            Err(e) => Connect::Transport(format!("the crate refused the {rule_what} rune: {e}")),
        };
        note(&format!(
            "{rule_what} rune ends {:?}",
            rune.chars()
                .rev()
                .take(4)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
        print_trace(&trace);
        r.check(
            "LN-09",
            &format!("CLN: {what}"),
            match (rule_what, outcome) {
                ("forged", Connect::Refused(ConnectionRefused::RuneNotFromNode)) => Ok(()),
                ("revoked", Connect::Refused(ConnectionRefused::RuneRevoked)) => Ok(()),
                ("padded", Connect::Done(_)) => Ok(()),
                (_, Connect::Refused(e)) => Err(e.to_string()),
                (_, Connect::Transport(e)) => Err(e),
                (_, Connect::Done(_)) => Err("accepted".into()),
            },
        );
    }
    Ok(())
}

/// LN-15: the label of an invoice at Core Lightning, and a repeated request.
fn label_checks(r: &mut Report, cln: &Node, lnd: &Node, a_path_any: &str) {
    r.check(
        "LN-15",
        "CLN: the same request again answers error 900 and the invoice is found again by its label",
        (|| {
            let ask = ask_for(70, 0, EXPIRY)?;
            let (first, _) = make_invoice(cln, &ask)?;
            let again = send(
                cln.credential.endpoint(),
                &cln.credential.create_request(&ask),
            )?;
            note(&format!("second request -> {}", shown(&again)));
            match cln.credential.parse_create(&again) {
                Ok(CreateOutcome::LabelExists) => {}
                other => return Err(format!("{other:?}")),
            }
            let by_label = cln
                .credential
                .label_request(&ask)
                .ok_or("no label request")?;
            let reply = send(cln.credential.endpoint(), &by_label)?;
            let found = cln
                .credential
                .parse_existing(&reply, &ask)
                .map_err(|e| format!("{e}; {}", shown(&reply)))?;
            if found.payment_hash != first.payment_hash {
                return Err("another invoice came back".into());
            }
            check_created(&found, &ask, Network::Regtest, cln.key).map_err(|e| e.to_string())?;
            Ok(())
        })(),
    );
    r.check(
        "LN-15",
        "CLN: after a payment the label finds the paid invoice only for the restart reader (parse_restored)",
        (|| {
            let ask = ask_for(72, 0, EXPIRY)?;
            let (c, checked) = make_invoice(cln, &ask)?;
            pay(cln, &checked.bolt11)?;
            for _ in 0..30 {
                if let (_, Payment::Paid(_)) = lookup(cln, &c.payment_hash)? {
                    break;
                }
                sleep(Duration::from_secs(1));
            }
            let reply = send(
                cln.credential.endpoint(),
                &cln.credential.label_request(&ask).ok_or("no label request")?,
            )?;
            note(&format!("label lookup of a paid invoice -> {}", shown(&reply).chars().take(100).collect::<String>()));
            match cln.credential.parse_existing(&reply, &ask) {
                Err(openbmt_lightning::ReplyRefused::LabelTaken) => {}
                other => return Err(format!("parse_existing: {other:?}")),
            }
            let restored = cln.credential.parse_restored(&reply, &ask).map_err(|e| e.to_string())?;
            if restored.payment_hash != c.payment_hash {
                return Err("another invoice".into());
            }
            Ok(())
        })(),
    );
    r.check(
        "LN-14",
        "CLN: an invoice that takes any amount has no amount_msat and is refused",
        (|| {
            let hash = unhex32(&fs::read_to_string(a_path_any).map_err(|e| e.to_string())?)?;
            let reply = send(
                cln.credential.endpoint(),
                &cln.credential.lookup_request(&hash),
            )?;
            note(&format!(
                "lookup of an any-amount invoice -> {}",
                shown(&reply).chars().take(120).collect::<String>()
            ));
            match cln
                .credential
                .parse_lookup(&reply, &hash, SALE_SATS * 1_000)
            {
                Err(openbmt_lightning::ReplyRefused::Missing("amount_msat")) => Ok(()),
                other => Err(format!("{other:?}")),
            }
        })(),
    );
    r.check(
        "LN-15",
        "LND: a repeated request makes a second invoice (no labels)",
        (|| {
            let ask = ask_for(71, 0, EXPIRY)?;
            let (a, _) = make_invoice(lnd, &ask)?;
            let (b, _) = make_invoice(lnd, &ask)?;
            if a.payment_hash == b.payment_hash {
                return Err("the same invoice twice".into());
            }
            Ok(())
        })(),
    );
}

// ---- the JVM part ---------------------------------------------------------------

/// The sale numbers of the invoices the JVM test makes.
const JVM_SALE: u64 = 900;

fn write_req(
    dir: &Path,
    name: &str,
    endpoint: &Endpoint,
    req: &Request,
    pin_der: &Path,
) -> Res<()> {
    let text = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        endpoint.host_port(),
        match req.method {
            Method::Get => "GET",
            Method::Post => "POST",
        },
        req.path,
        req.header_name,
        req.header_value,
        pin_der.display(),
        req.body.clone().unwrap_or_default()
    );
    fs::write(dir.join(name), text).map_err(|e| e.to_string())
}

/// After the main run: the requests the JVM transport sends, and the hashes
/// of the invoices it asks about.
fn jvm_plan(a: &Args, dir: &Path, node: &Node, paid: &[u8; 32], unpaid: &[u8; 32]) -> Res<()> {
    let tag = node.name.to_ascii_lowercase();
    let der = PathBuf::from(a.get(&format!("{tag}-der"))?);
    let wrong = PathBuf::from(a.get("other-der")?);
    let ep = node.credential.endpoint();
    let ask = ask_for(JVM_SALE, 0, EXPIRY)?;
    write_req(
        dir,
        &format!("{tag}-1.req"),
        ep,
        &node.credential.create_request(&ask),
        &der,
    )?;
    write_req(
        dir,
        &format!("{tag}-2.req"),
        ep,
        &node.credential.lookup_request(paid),
        &der,
    )?;
    write_req(
        dir,
        &format!("{tag}-3.req"),
        ep,
        &node.credential.lookup_request(unpaid),
        &der,
    )?;
    write_req(
        dir,
        &format!("{tag}-4.req"),
        ep,
        &node.credential.create_request(&ask),
        &wrong,
    )?;
    fs::write(dir.join(format!("{tag}-paid.hash")), hex(paid)).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("{tag}-unpaid.hash")), hex(unpaid)).map_err(|e| e.to_string())
}

fn jvm_reply(dir: &Path, tag: &str, n: u32) -> Res<Result<Reply, String>> {
    let status = fs::read_to_string(dir.join(format!("{tag}-{n}.status")))
        .map_err(|e| format!("{tag}-{n}: the JVM test left no answer ({e})"))?;
    let status = status.trim();
    match status.parse::<u16>() {
        Ok(status) => Ok(Ok(Reply {
            status,
            body: fs::read(dir.join(format!("{tag}-{n}.body"))).map_err(|e| e.to_string())?,
        })),
        Err(_) => Ok(Err(status.to_string())),
    }
}

fn jvm_checks(r: &mut Report, dir: &Path, node: &Node) {
    let tag = node.name.to_ascii_lowercase();
    let name = node.name;
    let ask = match ask_for(JVM_SALE, 0, EXPIRY) {
        Ok(a) => a,
        Err(e) => {
            r.fail("CR-114", &format!("{name}: the ask"), &e);
            return;
        }
    };
    r.check(
        "CR-114",
        &format!("{name}: the JVM transport makes an invoice the crate reads and checks (LN-17)"),
        (|| {
            let reply = jvm_reply(dir, &tag, 1)??;
            note(&format!(
                "JVM create -> {}",
                shown(&reply).chars().take(100).collect::<String>()
            ));
            let created = match node
                .credential
                .parse_create(&reply)
                .map_err(|e| e.to_string())?
            {
                CreateOutcome::Created(c) => c,
                CreateOutcome::LabelExists => return Err("the label is used".into()),
            };
            check_created(&created, &ask, Network::Regtest, node.key).map_err(|e| e.to_string())?;
            Ok(())
        })(),
    );
    for (n, file, want_paid) in [(2, "paid", true), (3, "unpaid", false)] {
        r.check(
            "CR-114",
            &format!(
                "{name}: the JVM transport's lookup of the {file} invoice reads as {}",
                if want_paid { "paid" } else { "not paid" }
            ),
            (|| {
                let hash = unhex32(
                    &fs::read_to_string(dir.join(format!("{tag}-{file}.hash")))
                        .map_err(|e| e.to_string())?,
                )?;
                let reply = jvm_reply(dir, &tag, n)??;
                note(&format!(
                    "JVM lookup of the {file} invoice -> {}",
                    shown(&reply).chars().take(100).collect::<String>()
                ));
                let p = node
                    .credential
                    .parse_lookup(&reply, &hash, SALE_SATS * 1_000)
                    .map_err(|e| e.to_string())?;
                match (p, want_paid) {
                    (Payment::Paid(_), true) | (Payment::NotPaid(NotPaid::Waiting), false) => {
                        Ok(())
                    }
                    (other, _) => Err(format!("{other:?}")),
                }
            })(),
        );
    }
    r.check(
        "CR-114",
        &format!("{name}: the JVM transport pinned to another certificate sends nothing"),
        match jvm_reply(dir, &tag, 4) {
            Ok(Err(why)) if why == "PinRefused" => Ok(()),
            Ok(other) => Err(format!("{other:?}")),
            Err(e) => Err(e),
        },
    );
}

// ---- main -----------------------------------------------------------------------

fn run(a: &Args, jvm_only: bool) -> Res<bool> {
    let mut r = Report::default();
    let address = a.get("address")?.to_string();
    let lnd = lnd_node(a)?;
    let cln = cln_node(a).ok();
    let (lnd_key, cln_key) = (lnd.key, node_key(&a.text("cln-key")?)?);
    let mut test_id = u64::from(std::process::id()) * 100;
    let dir = PathBuf::from(a.get("jvm-dir")?);
    if jvm_only {
        // Only the reading of what the JVM test got back.
        jvm_checks(&mut r, &dir, &lnd);
        if let Some(cln) = &cln {
            jvm_checks(&mut r, &dir, cln);
        }
        println!("checks: {} passed, {} failed", r.passed, r.failed);
        return Ok(r.failed == 0);
    }
    lnd_checks(&mut r, a, &lnd, &mut test_id)?;
    cln_checks(&mut r, a, &mut test_id)?;
    let other = a.get("other-host")?.to_string();
    let other_pem = a.text("other-cert")?;
    let mut sale = 10;
    let mut plans = Vec::new();
    for (node, other_key, fingerprint) in [
        (Some(&lnd), cln_key, a.get("lnd-fingerprint")?),
        (
            cln.as_ref(),
            lnd_key,
            a.get("cln-fingerprint").unwrap_or(""),
        ),
    ] {
        let Some(node) = node else { continue };
        tls_checks(&mut r, node, &other, &other_pem, fingerprint);
        let made = sale_checks(&mut r, node, other_key, &address, sale);
        sale += 10;
        // An unpaid invoice for the JVM test to ask about.
        let unpaid = ask_for(sale, 0, EXPIRY)
            .and_then(|ask| make_invoice(node, &ask))
            .map(|(c, _)| c.payment_hash);
        sale += 10;
        if let (Some((_, paid)), Ok(unpaid)) = (made, unpaid) {
            plans.push((node, paid, unpaid));
        }
    }
    if let Some(cln) = &cln {
        label_checks(&mut r, cln, &lnd, a.get("cln-any-hash")?);
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (node, paid, unpaid) in plans {
        jvm_plan(a, &dir, node, &paid, &unpaid)?;
    }
    println!("checks: {} passed, {} failed", r.passed, r.failed);
    Ok(r.failed == 0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((mode, rest)) = args.split_first() else {
        eprintln!("usage: lightning_regtest run|jvm-check --key value ...");
        std::process::exit(2);
    };
    let outcome = parse_args(rest).and_then(|a| match mode.as_str() {
        "run" => run(&a, false),
        "jvm-check" => run(&a, true),
        _ => Err("the mode is run or jvm-check".into()),
    });
    match outcome {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            println!("FAIL setup the example could not run: {e}");
            std::process::exit(2);
        }
    }
}
