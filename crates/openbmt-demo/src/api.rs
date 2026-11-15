//! The requests the page makes and the replies it gets, as JSON. A request
//! is an object with an `op`; a reply is an object with `ok`, and when `ok`
//! is false an `error` that says what went wrong.
//!
//! | op | other fields | what it does |
//! |---|---|---|
//! | `set_up` | `seed` | the shop is set up, with the demo wallet and a fixed price |
//! | `new_sale` | `cents` | the counter opens a sale for that amount |
//! | `customer_pays` | | the customer pays the sale on screen, once |
//! | `tick` | `secs` | the clock moves on |
//! | `mine` | | a block is mined, ten minutes later |
//! | `state` | | nothing changes; the screens are given again |
//!
//! Every reply also has `chain`: `pool` (each waiting payment, `sats` and
//! `ours`) and `blocks` (each `height`, `txs` and `ours`).

use openbmt_books::format_minor;
use openbmt_counter::say::state_name;
use openbmt_counter::{Color, Currency, Screen};
use openbmt_json::{parse, Limits, Value};

use crate::json::Obj;
use crate::shop::{OwnerSale, Shop, Shown};
use crate::words::words_json;

/// The state of the counter app after the shop is set up (the app's own
/// word list, `demo/words/counter-app.json`).
pub const STATE_SET_UP: &str = "pairingSaved";

/// The most one request may ask, in cents: a million dollars.
const MAX_CENTS: u64 = 100_000_000;
/// The longest the clock moves in one request: a day.
const MAX_TICK_SECS: u64 = 86_400;

fn color_text(c: Color) -> &'static str {
    match c {
        Color::Blue => "blue",
        Color::Green => "green",
        Color::Red => "red",
        Color::Neutral => "neutral",
    }
}

/// Answer one request. `shop` is the page's session: `set_up` makes it, and
/// every other op needs it.
pub fn handle(shop: &mut Option<Shop>, request: &[u8]) -> String {
    match run(shop, request) {
        Ok(text) => text,
        Err(e) => Obj::new().bool("ok", false).str("error", &e).done(),
    }
}

fn run(shop: &mut Option<Shop>, request: &[u8]) -> Result<String, String> {
    let v =
        parse(request, &Limits::DEFAULT).map_err(|e| format!("the request is not JSON: {e:?}"))?;
    let op = v
        .get("op")
        .and_then(Value::as_str)
        .ok_or("the request has no op")?;
    if op == "set_up" {
        let seed = v
            .get("seed")
            .and_then(Value::as_u64)
            .ok_or("set_up needs a seed")?;
        *shop = Some(Shop::set_up(seed)?);
        return reply(op, shop.as_mut().ok_or("no shop")?, None, STATE_SET_UP);
    }
    let s = shop.as_mut().ok_or("the shop is not set up")?;
    let shown = match op {
        "new_sale" => {
            let cents = v
                .get("cents")
                .and_then(Value::as_u64)
                .ok_or("new_sale needs cents")?;
            if cents == 0 || cents > MAX_CENTS {
                return Err("the amount is not between one cent and a million dollars".to_string());
            }
            s.new_sale(cents)?
        }
        "customer_pays" => s.customer_pays()?,
        "tick" => {
            let secs = v
                .get("secs")
                .and_then(Value::as_u64)
                .ok_or("tick needs secs")?;
            s.tick(secs.min(MAX_TICK_SECS))?
        }
        "mine" => s.mine()?,
        "state" => s.shown()?,
        _ => return Err(format!("unknown op {op}")),
    };
    reply(op, s, Some(shown), STATE_SET_UP)
}

/// The reply: the clock and the chain, what the counter shows, the payment
/// request, and the owner's view of the sale.
fn reply(op: &str, shop: &Shop, shown: Option<Shown>, app_state: &str) -> Result<String, String> {
    let (screen, owner) = match shown {
        Some(s) => (s.screen, s.owner),
        None => (None, None),
    };
    let counter = match &screen {
        Some(screen) => sale_screen(screen),
        None => Obj::new().str("kind", "app").str("state", app_state),
    };
    let counter = match shop.sale() {
        Some(sale) => counter.raw(
            "sale",
            &Obj::new()
                .num("index", u64::from(sale.index))
                .num("cents", shop.cents())
                .str("amount", &amount_text(shop.cents()))
                .num("needs_sats", sale.needs_sats)
                .bool("customer_paid", shop.customer_paid())
                .done(),
        ),
        None => counter,
    };
    let payment = match shop.payment() {
        Some(p) => Obj::new()
            .str("address", &p.address)
            .str("uri", &p.uri)
            .str("copy_uri", &p.copy_uri)
            .num("qr_size", u64::try_from(p.qr_size).unwrap_or(0))
            .str("qr", &p.qr)
            .done(),
        None => "null".to_string(),
    };
    let view = shop.chain_view();
    let pool: Vec<String> = view
        .pool
        .iter()
        .map(|(sats, ours)| Obj::new().num("sats", *sats).bool("ours", *ours).done())
        .collect();
    let blocks: Vec<String> = view
        .blocks
        .iter()
        .map(|(height, txs, ours)| {
            Obj::new()
                .num("height", u64::from(*height))
                .num("txs", u64::try_from(*txs).unwrap_or(0))
                .bool("ours", *ours)
                .done()
        })
        .collect();
    Ok(Obj::new()
        .bool("ok", true)
        .str("op", op)
        .num("clock", shop.now())
        .num("height", u64::from(shop.height()))
        .num("mempool", u64::try_from(shop.mempool()).unwrap_or(0))
        .raw(
            "chain",
            &Obj::new()
                .raw("pool", &format!("[{}]", pool.join(",")))
                .raw("blocks", &format!("[{}]", blocks.join(",")))
                .done(),
        )
        .raw("counter", &counter.done())
        .raw("payment", &payment)
        .raw(
            "owner",
            &owner.as_ref().map_or("null".to_string(), owner_json),
        )
        .done())
}

fn amount_text(cents: u64) -> String {
    Currency::new(*b"USD").map_or_else(|| cents.to_string(), |c| format_minor(cents, c))
}

/// The counter's screen for a sale: the state's name as the library names
/// it, its colour, and the library's own words.
fn sale_screen(screen: &Screen) -> Obj {
    let words = words_json(&screen.words());
    Obj::new()
        .str("kind", "sale")
        .str("state", state_name(screen.state))
        .str("color", color_text(screen.state.color()))
        .raw("words", &words.done())
        .num("received_sats", screen.received_sats)
        .num("needs_sats", screen.needs_sats)
}

fn owner_json(o: &OwnerSale) -> String {
    let (sats, why) = match &o.sats_received {
        Ok(sats) => (Some(*sats), None),
        Err(why) => (None, Some(why.as_str())),
    };
    Obj::new()
        .str("id", &o.id)
        .str("amount", &o.amount)
        .str("currency", &o.currency)
        .str("state", o.state)
        .str("counting", o.counting)
        .opt_num("sats_received", sats)
        .opt_str("sats_received_why", why)
        .opt_str("address", o.address.as_deref())
        .done()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(shop: &mut Option<Shop>, request: &str) -> Value {
        let text = handle(shop, request.as_bytes());
        parse(text.as_bytes(), &Limits::DEFAULT).unwrap()
    }

    fn ok(v: &Value) {
        assert_eq!(v.get("ok").and_then(Value::as_bool), Some(true), "{v:?}");
    }

    fn state(v: &Value) -> String {
        v.get("counter")
            .and_then(|c| c.get("state"))
            .and_then(Value::as_str)
            .unwrap()
            .to_string()
    }

    fn owner_state(v: &Value) -> Option<String> {
        v.get("owner")
            .and_then(|o| o.get("state"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    #[test]
    fn the_shop_flow_runs_from_a_sale_to_final() {
        let mut shop = None;
        let v = call(&mut shop, r#"{"op":"set_up","seed":42}"#);
        ok(&v);
        assert_eq!(state(&v), STATE_SET_UP);

        // A sale of $5.00 at $100,000 a bitcoin: 5,000 sats, a regtest address.
        let v = call(&mut shop, r#"{"op":"new_sale","cents":500}"#);
        ok(&v);
        assert_eq!(state(&v), "NotPaidYet");
        let pay = v.get("payment").unwrap();
        let address = pay.get("address").and_then(Value::as_str).unwrap();
        assert!(address.starts_with("bcrt1"));
        assert_eq!(
            pay.get("uri").and_then(Value::as_str).unwrap(),
            format!("BITCOIN:{}?amount=0.00005", address.to_ascii_uppercase())
        );
        let size = pay.get("qr_size").and_then(Value::as_u64).unwrap() as usize;
        assert_eq!(
            pay.get("qr").and_then(Value::as_str).unwrap().len(),
            size * size
        );
        assert_eq!(owner_state(&v).as_deref(), Some("open"));

        // The customer pays: seen, not confirmed: the counter waits and watches.
        let v = call(&mut shop, r#"{"op":"customer_pays"}"#);
        ok(&v);
        assert_eq!(state(&v), "Waiting");
        assert_eq!(v.get("mempool").and_then(Value::as_u64), Some(1));
        let pool = v
            .get("chain")
            .and_then(|c| c.get("pool"))
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].get("sats").and_then(Value::as_u64), Some(5_000));
        assert_eq!(pool[0].get("ours").and_then(Value::as_bool), Some(true));

        // The watch for a conflicting payment runs out: accepted at once.
        let v = call(&mut shop, r#"{"op":"tick","secs":10}"#);
        assert_eq!(state(&v), "PaidInstant");
        assert_eq!(owner_state(&v).as_deref(), Some("paid at once"));

        // One block: confirmed at the shop's depth.
        let v = call(&mut shop, r#"{"op":"mine"}"#);
        ok(&v);
        assert_eq!(state(&v), "Confirmed");
        assert_eq!(v.get("mempool").and_then(Value::as_u64), Some(0));
        let owner = v.get("owner").unwrap();
        assert_eq!(
            owner.get("sats_received").and_then(Value::as_u64),
            Some(5_000)
        );
        assert_eq!(owner.get("amount").and_then(Value::as_str), Some("5.00"));

        // Five more blocks: final, and counted in the books.
        let mut v = v;
        for _ in 0..5 {
            v = call(&mut shop, r#"{"op":"mine"}"#);
        }
        assert_eq!(state(&v), "Final");
        assert_eq!(owner_state(&v).as_deref(), Some("final"));
        assert_eq!(
            v.get("owner")
                .and_then(|o| o.get("counting"))
                .and_then(Value::as_str),
            Some("counted")
        );
        assert_eq!(v.get("height").and_then(Value::as_u64), Some(106));
        let blocks = v
            .get("chain")
            .and_then(|c| c.get("blocks"))
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(blocks.len(), 6);
        let ours: Vec<bool> = blocks
            .iter()
            .map(|b| b.get("ours").and_then(Value::as_bool).unwrap())
            .collect();
        assert_eq!(ours, [true, false, false, false, false, false]);
        assert_eq!(blocks[0].get("txs").and_then(Value::as_u64), Some(1));
        assert_eq!(blocks[0].get("height").and_then(Value::as_u64), Some(101));
    }

    #[test]
    fn the_words_are_the_librarys() {
        let mut shop = None;
        call(&mut shop, r#"{"op":"set_up","seed":1}"#);
        let v = call(&mut shop, r#"{"op":"new_sale","cents":500}"#);
        let words = v.get("counter").and_then(|c| c.get("words")).unwrap();
        assert_eq!(
            words.get("heading").and_then(Value::as_str),
            Some("Not paid yet")
        );
        assert_eq!(words.get("tone").and_then(Value::as_str), Some("info"));
        assert_eq!(
            words.get("who").and_then(Value::as_str),
            Some("the customer")
        );
    }

    #[test]
    fn a_sale_above_the_limit_waits_for_a_block() {
        // The light check accepts $10 a sale at once; $50 waits for a block.
        let mut shop = None;
        call(&mut shop, r#"{"op":"set_up","seed":1}"#);
        call(&mut shop, r#"{"op":"new_sale","cents":5000}"#);
        call(&mut shop, r#"{"op":"customer_pays"}"#);
        let v = call(&mut shop, r#"{"op":"tick","secs":60}"#);
        assert_eq!(state(&v), "Waiting");
        let v = call(&mut shop, r#"{"op":"mine"}"#);
        assert_eq!(state(&v), "Confirmed");
    }

    #[test]
    fn nothing_works_before_the_shop_is_set_up() {
        let mut shop = None;
        for op in ["new_sale", "customer_pays", "tick", "mine", "state"] {
            let v = call(
                &mut shop,
                &format!(r#"{{"op":"{op}","cents":500,"secs":1}}"#),
            );
            assert_eq!(v.get("ok").and_then(Value::as_bool), Some(false), "{op}");
        }
    }

    #[test]
    fn bad_requests_are_refused_with_a_reason() {
        let mut shop = None;
        call(&mut shop, r#"{"op":"set_up","seed":1}"#);
        for request in [
            "not json",
            r#"{"x":1}"#,
            r#"{"op":"fly"}"#,
            r#"{"op":"new_sale"}"#,
            r#"{"op":"new_sale","cents":0}"#,
            r#"{"op":"new_sale","cents":999999999999}"#,
            r#"{"op":"customer_pays"}"#,
            r#"{"op":"tick"}"#,
        ] {
            let v = call(&mut shop, request);
            assert_eq!(
                v.get("ok").and_then(Value::as_bool),
                Some(false),
                "{request}"
            );
            assert!(v
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|e| !e.is_empty()));
        }
    }

    #[test]
    fn the_customer_pays_a_sale_once() {
        let mut shop = None;
        call(&mut shop, r#"{"op":"set_up","seed":1}"#);
        call(&mut shop, r#"{"op":"new_sale","cents":500}"#);
        ok(&call(&mut shop, r#"{"op":"customer_pays"}"#));
        let v = call(&mut shop, r#"{"op":"customer_pays"}"#);
        assert_eq!(v.get("ok").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn a_second_sale_gets_the_next_address_and_the_first_stays_in_the_books() {
        let mut shop = None;
        call(&mut shop, r#"{"op":"set_up","seed":9}"#);
        let a = call(&mut shop, r#"{"op":"new_sale","cents":500}"#);
        let b = call(&mut shop, r#"{"op":"new_sale","cents":700}"#);
        let address = |v: &Value| {
            v.get("payment")
                .and_then(|p| p.get("address"))
                .and_then(Value::as_str)
                .unwrap()
                .to_string()
        };
        assert_ne!(address(&a), address(&b));
        assert_eq!(
            b.get("counter")
                .and_then(|c| c.get("sale"))
                .and_then(|s| s.get("index"))
                .and_then(Value::as_u64),
            Some(1)
        );
    }

    #[test]
    fn the_same_requests_give_the_same_replies() {
        let run = || {
            let mut shop = None;
            let mut out = Vec::new();
            for r in [
                r#"{"op":"set_up","seed":5}"#,
                r#"{"op":"new_sale","cents":500}"#,
                r#"{"op":"customer_pays"}"#,
                r#"{"op":"mine"}"#,
            ] {
                out.push(handle(&mut shop, r.as_bytes()));
            }
            out
        };
        assert_eq!(run(), run());
    }
}
