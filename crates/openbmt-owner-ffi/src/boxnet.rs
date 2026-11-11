//! Connecting a Box (`docs/OWNER-APP-RULES.md` OA-40 to OA-48, on
//! `docs/BOX-RULES.md` BX-13, BX-15 and BX-17): the types of the ops that
//! fetch a Box's claim and hand it the welcome, and the calls to the Box
//! that those ops make. The calls are the link client's `claim` and
//! `welcome`; nothing of the protocol is written again here.

use std::net::SocketAddr;
use std::str::FromStr;

use openbmt_core::boxpair::{Claim, Invitation, INVITATION_RANDOM_LEN, INVITATION_SECS};
use openbmt_core::identity::DeviceId;
use openbmt_link::proto::Refusal;
use openbmt_link::{Client, ClientConfig, ClientError, Route};

use crate::codec::{wire_record, wire_record_get};
use crate::error::{refused_by, store_by, AppError};

/// Where a Box is, and the proxy that reaches an onion name (OA-41).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxLinkArgs {
    /// `host:port` of the Box on the local network, or a v3 `.onion` name
    /// and its port.
    pub address: String,
    /// The SOCKS5 proxy (Orbot) as `host:port`; used only for an onion
    /// name.
    pub proxy: String,
}

wire_record!(BoxLinkArgs { address, proxy });
wire_record_get!(BoxLinkArgs { address, proxy });

/// The claim a Box offered, fetched and read (OA-41).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxFoundView {
    /// The claim (`OBM5`), to carry to the Key app.
    pub claim: Vec<u8>,
    /// The Box's key, 32 bytes. It is also the relay key of the Box.
    pub box_key: Vec<u8>,
    /// The phone holds no settings yet: queuing the Box needs today's rate
    /// in satoshis to a dollar.
    pub needs_rate: bool,
}

wire_record!(BoxFoundView {
    claim,
    box_key,
    needs_rate
});

/// The arguments of `box_queue` (OA-42).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxQueueArgs {
    /// The claim, from `box_fetch_claim`.
    pub claim: Vec<u8>,
    /// The price of one bitcoin in whole dollars, for the starting settings when the phone holds
    /// none; ignored when it does.
    pub usd_per_btc: u64,
}

wire_record!(BoxQueueArgs { claim, usd_per_btc });
wire_record_get!(BoxQueueArgs { claim, usd_per_btc });

/// What `box_queue` queued (OA-42).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxQueuedView {
    /// The claim's number in the queue.
    pub claim_id: u32,
    /// The settings' number in the queue.
    pub settings_id: u32,
    /// The settings' version.
    pub settings_version: u32,
    /// The Box's key, 32 bytes.
    pub box_key: Vec<u8>,
    /// The four words of the Box's key, which the Key app and the Box show
    /// too.
    pub words: Vec<String>,
}

wire_record!(BoxQueuedView {
    claim_id,
    settings_id,
    settings_version,
    box_key,
    words
});

/// A Box whose finished welcome waits to be handed over (OA-43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingBoxView {
    /// The Box's key, 32 bytes.
    pub box_key: Vec<u8>,
}

wire_record!(WaitingBoxView { box_key });

/// A Box that has a claim queued or a welcome waiting (OA-43, OA-47).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBoxView {
    /// The Box's key, 32 bytes.
    pub box_key: Vec<u8>,
    /// The four words of the Box's key.
    pub words: Vec<String>,
    /// Whether its finished welcome waits to be handed over.
    pub welcome_ready: bool,
}

wire_record!(PendingBoxView {
    box_key,
    words,
    welcome_ready
});

/// The arguments of `box_deliver` (OA-43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxDeliverArgs {
    /// Where the Box is.
    pub link: BoxLinkArgs,
    /// The Box's key, 32 bytes.
    pub box_key: Vec<u8>,
}

wire_record!(BoxDeliverArgs { link, box_key });
wire_record_get!(BoxDeliverArgs { link, box_key });

/// A Box that took its welcome (OA-43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxDeliveredView {
    /// The Box's key, 32 bytes: the relay key the link is made for.
    pub box_key: Vec<u8>,
    /// The counters the settings name, 32 bytes each, in key order.
    pub counters: Vec<Vec<u8>>,
    /// The version of the settings the Box holds now.
    pub settings_version: u32,
}

wire_record!(BoxDeliveredView {
    box_key,
    counters,
    settings_version
});

/// An invitation the owner phone made for itself (OA-46, BX-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxOwnInvitationView {
    /// The 30 characters in six groups of five.
    pub text: String,
    /// When it runs out, in seconds since 1970 on this phone's clock.
    pub expires_at: u64,
}

wire_record!(BoxOwnInvitationView { text, expires_at });

/// The arguments of `box_own_check` (OA-46).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxOwnCheckArgs {
    /// The invitation's text, as `box_own_invitation` gave it.
    pub invitation: String,
    /// The claim (`OBM5`) the Box offered.
    pub claim: Vec<u8>,
}

wire_record!(BoxOwnCheckArgs { invitation, claim });
wire_record_get!(BoxOwnCheckArgs { invitation, claim });

/// A claim that answers this phone's own invitation (OA-46).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxOwnCheckView {
    /// The Box's key, 32 bytes.
    pub box_key: Vec<u8>,
    /// The four words of the Box's key.
    pub words: Vec<String>,
}

wire_record!(BoxOwnCheckView { box_key, words });

// ---- the address -------------------------------------------------------------

fn bad_address(text: impl std::fmt::Display) -> AppError {
    refused_by("box-address", text)
}

/// A v3 onion name: 56 characters of `a` to `z` and `2` to `7`, then
/// `.onion`.
fn is_v3_onion(host: &str) -> bool {
    host.strip_suffix(".onion").is_some_and(|name| {
        name.len() == 56
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    })
}

/// The route the typed address names (LK-31): an address and port on the
/// local network, or a v3 onion name and its port through `proxy`. A name
/// that is neither is refused; nothing here looks a name up.
pub(crate) fn route_for(address: &str, proxy: &str) -> Result<Route, AppError> {
    let text = address.trim();
    if text.is_empty() || text.len() > 100 {
        return Err(bad_address("the address is empty or too long"));
    }
    let lower = text.to_ascii_lowercase();
    if let Some((host, port)) = lower.rsplit_once(':') {
        if host.ends_with(".onion") {
            if !is_v3_onion(host) {
                return Err(bad_address("the onion name is not a version 3 name"));
            }
            let port = port
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or_else(|| bad_address("the port is not a port"))?;
            let proxy = SocketAddr::from_str(proxy.trim())
                .map_err(|e| bad_address(format!("the proxy {proxy:?}: {e}")))?;
            return Ok(Route::Onion {
                proxy,
                host: host.to_string(),
                port,
            });
        }
    }
    let addr = SocketAddr::from_str(text)
        .ok()
        .filter(|a| a.port() != 0 && !a.ip().is_unspecified())
        .ok_or_else(|| bad_address(format!("{text:?} is not an address and a port")))?;
    Ok(Route::Direct(addr))
}

// ---- the calls ---------------------------------------------------------------

/// How a failed call is told: a Box that cannot be reached, or something
/// at that address that is not a Box.
fn client_error(e: &ClientError) -> AppError {
    match e {
        ClientError::Connect(_) | ClientError::Socks(_) | ClientError::Io(_) => {
            refused_by("box-unreachable", e)
        }
        ClientError::WrongRelay(_) => refused_by("box-wrong-key", e),
        ClientError::Refused(Refusal::WelcomeRefused) => refused_by("box-welcome-refused", e),
        ClientError::Refused(Refusal::NoClaim) => refused_by("box-no-invitation", e),
        _ => refused_by("box-not-a-box", e),
    }
}

/// What a Box said to "claim".
pub(crate) struct Fetched {
    /// The key the relay proved in its hello.
    pub relay: DeviceId,
    /// Its claim, or `None` when it holds no open invitation.
    pub claim: Option<Vec<u8>>,
}

/// Ask the Box at `route` for its claim (BX-13). It blocks for the time the
/// link client's timeouts allow.
pub(crate) fn fetch(route: &Route) -> Result<Fetched, AppError> {
    let mut client =
        Client::connect(route, None, &ClientConfig::default()).map_err(|e| client_error(&e))?;
    let relay = *client.relay();
    match client.claim() {
        Ok(bytes) => Ok(Fetched {
            relay,
            claim: Some(bytes),
        }),
        Err(ClientError::Refused(Refusal::NoClaim)) => Ok(Fetched { relay, claim: None }),
        Err(e) => Err(client_error(&e)),
    }
}

/// Read what the Box offered as a claim, and require that it is of the Box
/// that answered: a claim names the key that signed the hello (BX-01).
pub(crate) fn read_claim(fetched: &Fetched, bytes: &[u8]) -> Result<Claim, AppError> {
    let claim = Claim::from_bytes(bytes)
        .map_err(|e| refused_by("box-not-a-box", format!("the claim does not read: {e}")))?;
    if claim.box_key != fetched.relay {
        return Err(refused_by(
            "box-not-a-box",
            "the claim names another key than the one that answered",
        ));
    }
    Ok(claim)
}

/// Hand the Box at `route` its welcome and the settings (BX-15). The Box
/// must be the one with key `box_key`.
pub(crate) fn send_welcome(
    route: &Route,
    box_key: &DeviceId,
    welcome: &[u8],
    settings: &[u8],
) -> Result<(), AppError> {
    let mut client = Client::connect(route, Some(box_key), &ClientConfig::default())
        .map_err(|e| client_error(&e))?;
    client
        .welcome(welcome, settings)
        .map_err(|e| client_error(&e))
}

/// The error of a Box that holds no claim, for a Box the shop knows or not.
pub(crate) fn no_claim(known: bool) -> AppError {
    if known {
        refused_by("box-paired", "this Box is already paired with this shop")
    } else {
        refused_by("box-no-invitation", "the Box holds no open invitation")
    }
}

/// The error of a Box that the shop already knows.
pub(crate) fn already_paired() -> AppError {
    refused_by("box-paired", "this Box is already paired with this shop")
}

// ---- BX-17: the owner phone alone ------------------------------------------

fn random17() -> Result<[u8; INVITATION_RANDOM_LEN], AppError> {
    use std::io::Read;
    let mut out = [0u8; INVITATION_RANDOM_LEN];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut out))
        .map_err(|e| store_by("box-random", format!("the random source: {e}")))?;
    Ok(out)
}

/// BX-17: an invitation made on the owner phone, good for 30 minutes from
/// `now` on this phone's clock. The phone keeps it in memory only.
pub(crate) fn own_invitation(now: u64) -> Result<BoxOwnInvitationView, AppError> {
    let mut random = random17()?;
    let invitation = Invitation::from_random(&random);
    random.fill(0);
    std::hint::black_box(&mut random);
    Ok(BoxOwnInvitationView {
        text: invitation.to_text(),
        expires_at: now.saturating_add(INVITATION_SECS),
    })
}

/// BX-17: check a Box's claim against this phone's own invitation, in
/// constant time, and give the Box's key and its four words.
pub(crate) fn own_check(a: BoxOwnCheckArgs) -> Result<BoxOwnCheckView, AppError> {
    let invitation = Invitation::from_text(&a.invitation)
        .map_err(|e| refused_by("box-own-invitation", format!("BX-10: {e}")))?;
    let claim = Claim::from_bytes(&a.claim)
        .map_err(|e| refused_by("box-not-a-box", format!("the claim does not read: {e}")))?;
    if !claim.verify(&invitation) {
        return Err(refused_by(
            "box-claim-mismatch",
            "BX-14: the claim was not made with this invitation",
        ));
    }
    Ok(BoxOwnCheckView {
        box_key: claim.box_key.to_bytes().to_vec(),
        words: claim
            .box_key
            .fingerprint_words()
            .iter()
            .map(|w| (*w).to_string())
            .collect(),
    })
}
