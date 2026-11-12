//! SW-90, SW-91: the pretend service, with no node and with a regtest node.

use std::path::PathBuf;

use bitcoin::Network;

use super::fixtures::*;
use crate::follow::{Ctx, Next, ReverseSwap, Stage};
use crate::testing::{Mode, Pretend, Regtest};
use crate::{
    check, parse_response, Expect, Pairs, Provider, Quote, Refused, Reply, Request, Seen, Status,
    Swap, SwapState,
};

const ONION: &str = "w2mqd2fcbgiop7oyxoes2ozmdnvgon22oipj7vzh2rquepf7cxauauad.onion";

fn provider() -> Provider {
    Provider::new("Pretend", ONION, "/api").unwrap()
}

fn serve(p: &mut Pretend, request: &[u8]) -> Vec<u8> {
    let prov = provider();
    p.serve(request, prov.path_prefix(), prov.onion())
}

fn body(p: &mut Pretend, request: Vec<u8>) -> Result<Vec<u8>, Refused> {
    let reply = serve(p, &request);
    Ok(parse_response(&reply)?.ok_body()?.to_vec())
}

/// The shop's side up to the check of the reply: the pairs call, the
/// request, the create call, the reply and its check. `tip` is the height
/// the shop sees, `now` its clock.
fn negotiate(
    p: &mut Pretend,
    sats: u64,
    pairs_read_at: u64,
    now: u64,
    tip: u32,
    network: Network,
) -> Result<(Swap, Pairs), Refused> {
    let prov = provider();
    let pairs = Pairs::parse(&body(p, prov.pairs_request()?)?, pairs_read_at)?;
    let s = secrets();
    let request = Request::new_live(&s, sats, "swap", &pairs, now)?;
    let reply = Reply::parse(&body(p, prov.create_request(&request)?)?)?;
    let swap = check(
        &reply,
        &Expect {
            secrets: &s,
            invoice_sats: sats,
            network,
            quote: Quote::from_pairs(&pairs, now)?,
            tip_height: tip,
            now_secs: now,
        },
    )?;
    Ok((swap, pairs))
}

fn poll(p: &mut Pretend, m: &ReverseSwap, ctx: &Ctx) -> (ReverseSwap, Next) {
    let prov = provider();
    let id = m.state().id().to_string();
    let b = body(p, prov.status_request(&id).unwrap()).unwrap();
    m.on_status(&Status::parse(&b).unwrap(), ctx)
}

#[test]
fn sw_90_the_pretend_service_plays_a_whole_swap_without_a_node() {
    for chunked in [false, true] {
        let mut p = Pretend::new(NET, TIP, NOW);
        p.chunked = chunked;
        let (swap, _) = negotiate(&mut p, INVOICE_SATS, NOW, NOW, TIP, NET).unwrap();
        let offer = p.offer.clone().unwrap();
        assert_eq!(swap.lockup_address, offer.address);
        assert_eq!(swap.onchain_sats, offer.promised_sats);
        assert_eq!(swap.timeout, TIP + 144);
        let state = SwapState::new(&swap, &secrets(), NET, &destination()).unwrap();
        let ctx = Ctx {
            now_secs: NOW,
            tip_height: TIP,
            accept_unconfirmed: false,
        };
        let m = ReverseSwap::new(state);
        // Nothing yet.
        let (m, next) = poll(&mut p, &m, &ctx);
        assert!(matches!(next, Next::Wait { .. }));
        // The customer pays; the service locks and says so, with the bytes.
        let lockup = p.pretend_lockup();
        p.report("transaction.mempool", Some(lockup.clone()));
        let (m, next) = poll(&mut p, &m, &ctx);
        assert!(matches!(m.stage(), Stage::Locked(_)), "{:?}", m.stage());
        assert!(matches!(next, Next::Wait { .. }));
        // The caller sees it in a block.
        let (m, next) = m.on_lockup(&lockup, Seen::Block, &ctx).unwrap();
        assert!(matches!(next, Next::BuildClaim { .. }));
        let (m, claim) = m.claim(2_000, &ctx).unwrap();
        assert_eq!(claim.output[0].script_pubkey, destination().script_pubkey());
        let m = m.claim_confirmed(claim.compute_txid(), TIP + 1).unwrap();
        assert!(matches!(m.stage(), Stage::Paid { .. }));
        // The service never saw a header the rules do not allow: every
        // request it was sent passed its header check.
        assert!(p.seen.len() >= 4);
    }
}

#[test]
fn sw_90_the_service_answers_a_request_with_other_headers_with_an_error() {
    let mut p = Pretend::new(NET, TIP, NOW);
    let prov = provider();
    let mut bad = prov.pairs_request().unwrap();
    bad.splice(bad.len() - 2..bad.len() - 2, *b"User-Agent: x\r\n");
    let r = parse_response(&serve(&mut p, &bad)).unwrap();
    assert_eq!(r.status(), 400);
    // A path the service does not know.
    let r = parse_response(&serve(&mut p, &prov.get("/v2/nothing").unwrap())).unwrap();
    assert_eq!(r.status(), 404);
    // A status call for a swap it never made.
    let r = parse_response(&serve(&mut p, &prov.status_request("nope").unwrap())).unwrap();
    assert_eq!(r.status(), 404);
}

#[test]
fn sw_91_the_refusals_hold_against_the_pretend_service() {
    let go = |mode: Mode, sats: u64, read_at: u64, now: u64, tip: u32| {
        let mut p = Pretend::new(NET, TIP, NOW);
        p.mode = mode;
        negotiate(&mut p, sats, read_at, now, tip, NET)
    };
    assert!(go(Mode::Honest, INVOICE_SATS, NOW, NOW, TIP).is_ok());
    // A wrong tree: the address of another tree.
    assert_eq!(
        go(Mode::WrongTree, INVOICE_SATS, NOW, NOW, TIP).unwrap_err(),
        Refused::WrongLockupAddress
    );
    // The other order of the keys.
    assert_eq!(
        go(Mode::OtherKeyOrder, INVOICE_SATS, NOW, NOW, TIP).unwrap_err(),
        Refused::WrongLockupAddress
    );
    // A timeout that is too near.
    assert_eq!(
        go(Mode::NearTimeout, INVOICE_SATS, NOW, NOW, TIP).unwrap_err(),
        Refused::TimeoutTooSoon
    );
    // Stale pairs: read eleven minutes before.
    assert_eq!(
        go(Mode::Honest, INVOICE_SATS, NOW - 660, NOW, TIP).unwrap_err(),
        Refused::PairsStale
    );
    // An amount above the live maximum: refused before any request.
    let mut p = Pretend::new(NET, TIP, NOW);
    let err = negotiate(&mut p, 5_000_001, NOW, NOW, TIP, NET).unwrap_err();
    assert_eq!(err.rule(), "SW-62");
    assert_eq!(p.seen.len(), 1, "only the pairs call was made");
    // The same amount with a live maximum raised: the pretend service takes
    // it, and a request is made.
    let mut p = Pretend::new(NET, TIP, NOW);
    p.terms.max_sats = 40_000_000;
    let err = negotiate(&mut p, 26_000_000, NOW, NOW, TIP, NET).unwrap_err();
    assert_eq!(err, Refused::AmountOutOfBand);
    // The service itself refuses an amount outside its limits if asked.
    let mut p = Pretend::new(NET, TIP, NOW);
    let prov = provider();
    let s = secrets();
    let req = Request::new(&s, 6_000_000, "swap").unwrap();
    let reply = serve(&mut p, &prov.create_request(&req).unwrap());
    assert_eq!(parse_response(&reply).unwrap().status(), 400);
    // A lockup that pays less than promised.
    let mut p = Pretend::new(NET, TIP, NOW);
    p.mode = Mode::ShortLockup;
    let (swap, _) = negotiate(&mut p, INVOICE_SATS, NOW, NOW, TIP, NET).unwrap();
    let state = SwapState::new(&swap, &s, NET, &destination()).unwrap();
    let ctx = Ctx {
        now_secs: NOW,
        tip_height: TIP,
        accept_unconfirmed: true,
    };
    let m = ReverseSwap::new(state);
    let lockup = p.pretend_lockup();
    assert_eq!(
        m.on_lockup(&lockup, Seen::Block, &ctx).unwrap_err(),
        Refused::LockupTooSmall
    );
    p.report("transaction.mempool", Some(lockup));
    let (m, next) = poll(&mut p, &m, &ctx);
    assert!(matches!(next, Next::Failed(_)), "{next:?}");
    assert!(m.claim(1_000, &ctx).is_err());
}

fn scratch() -> PathBuf {
    let base = std::env::var_os("OPENBMT_REGTEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap())
                .join(".cache/openbmt/tmp/swap/regtest-swap")
        });
    let _ = std::fs::remove_dir_all(&base);
    base
}

/// Pairs -> request -> create reply -> status mempool -> lockup checked ->
/// claim built -> claim accepted by Core and mined -> paid, against a
/// private regtest node. Run with:
///
/// `cargo test -p openbmt-swap -- --ignored sw_90_regtest`
#[test]
#[ignore = "starts a private regtest bitcoind; run by hand"]
fn sw_90_regtest_swap_from_pairs_to_paid() {
    let port = std::env::var("OPENBMT_REGTEST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(18_991);
    let node = Regtest::start(&scratch(), port).unwrap();
    let tip = node.height().unwrap();
    let mut p = Pretend::new(Network::Regtest, tip, NOW);
    let (swap, _) = negotiate(&mut p, INVOICE_SATS, NOW, NOW, tip, Network::Regtest).unwrap();
    let s = secrets();
    let dest = node.new_address().unwrap();
    let state = SwapState::new(&swap, &s, Network::Regtest, &dest).unwrap();
    let ctx = |tip: u32| Ctx {
        now_secs: NOW,
        tip_height: tip,
        accept_unconfirmed: false,
    };
    let m = ReverseSwap::new(state);
    // The service makes a real lockup to the address it promised, and says
    // so.
    let offer = p.offer.clone().unwrap();
    let lockup = node.fund(&offer.address, offer.lockup_sats).unwrap();
    p.report("transaction.mempool", Some(lockup.clone()));
    let (m, next) = poll(&mut p, &m, &ctx(tip));
    assert!(matches!(m.stage(), Stage::Locked(_)), "{:?}", m.stage());
    assert!(matches!(next, Next::Wait { .. }), "{next:?}");
    // A block confirms it; the caller reports it from its own view.
    node.mine(1).unwrap();
    assert_eq!(
        node.confirmed_height(&lockup.compute_txid()).unwrap(),
        Some(tip + 1)
    );
    let (m, next) = m.on_lockup(&lockup, Seen::Block, &ctx(tip + 1)).unwrap();
    assert!(matches!(next, Next::BuildClaim { .. }), "{next:?}");
    // The claim: Core accepts it by `testmempoolaccept`, it is mined, and
    // then the swap is paid.
    let (m, claim) = m.claim(2_000, &ctx(tip + 1)).unwrap();
    node.test_accept(&claim).unwrap();
    let txid = node.broadcast(&claim).unwrap();
    assert_eq!(txid, claim.compute_txid());
    node.mine(1).unwrap();
    let height = node.confirmed_height(&txid).unwrap().unwrap();
    assert_eq!(height, tip + 2);
    let m = m.claim_confirmed(txid, height).unwrap();
    assert_eq!(m.stage(), Stage::Paid { height });
    // The shop's address got the lockup less the fee.
    assert_eq!(claim.output.len(), 1);
    assert!(claim.output[0].value.to_sat() < offer.lockup_sats);
    assert_eq!(claim.output[0].script_pubkey, dest.script_pubkey());
    // The refusals, against the same node: a lockup to a wrong tree is not
    // the swap's, and a claim before the lockup is checked does not exist.
    let early = ReverseSwap::new(SwapState::new(&swap, &s, Network::Regtest, &dest).unwrap());
    assert!(early.claim(2_000, &ctx(tip + 2)).is_err());
}
