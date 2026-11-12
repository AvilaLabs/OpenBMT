//! What LND v0.21.8-beta and Core Lightning v26.06.10 really answered on
//! regtest, captured by `scripts/lightning-e2e-core.sh`, against the readers
//! of the crate: LN-06, LN-09, LN-14, LN-15. The bodies are verbatim; the keys,
//! payment hashes and invoices in them belong to nodes of one regtest run.

use super::*;
use crate::connect::Probe;
use crate::error::ReplyRefused;
use crate::invoice::{NotPaid, Payment};
use crate::{cln as cln_node, lnd as node, CreateOutcome};

const ASKED_MSAT: u64 = 21_000_000;

fn h(hex_text: &str) -> [u8; 32] {
    let bytes: Vec<u8> = (0..32)
        .map(|i| u8::from_str_radix(&hex_text[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    <[u8; 32]>::try_from(bytes).unwrap()
}

const LND_OPEN_HASH: &str = "d0c9128c005abd8c35e2b3c2fc2a3e6dedae6cb3050d1e2096927652c8346417";
const LND_OPEN: &str = r#"{"memo":"Sale 10","r_preimage":"nhKxAff22/dYTRtWehkjhqIS7/0ANbEE0VYB7ymrweQ=","r_hash":"0MkSjABavYw14rPC/Co+be2ubLMFDR4glpJ2Usg0ZBc=","value":"21000","value_msat":"21000000","settled":false,"creation_date":"1794524801","settle_date":"0","payment_request":"lnbcrt210u1p40v35ppp56ry39rqqt27ccd0zk0p0c237dhk6um9nq5x3ugykjfm99jp5vstsdqv2dskcefqxycqcqzzsxqzfvsp50ff28ht6h9drd8plq3uc8efew8k6vflmkd3wmz6thma9dem3vy5q9qxpqysgqkcszvswm4unf79g74ucyeu83vte9p8y4f8pyxp78u9u7ruqught95ccxha2q3hp9trca744asrl6l4t949s79jaq2s5dquxguej3mpsp23exc9","description_hash":"","expiry":"300","fallback_addr":"","cltv_expiry":"80","route_hints":[],"private":false,"add_index":"2","settle_index":"0","amt_paid":"0","amt_paid_sat":"0","amt_paid_msat":"0","state":"OPEN","htlcs":[],"features":{"8":{"name":"tlv-onion","is_required":true,"is_known":true},"14":{"name":"payment-addr","is_required":true,"is_known":true},"17":{"name":"multi-path-payments","is_required":false,"is_known":true},"25":{"name":"route-blinding","is_required":false,"is_known":true}},"is_keysend":false,"payment_addr":"elKj3Xq5WjacPwR5g+U5ce2mJ/uzYu2LS776VudxYSg=","is_amp":false,"amp_invoice_state":{},"is_blinded":false,"blinded_path_config":null}"#;

const LND_SETTLED_HASH: &str = "d0c9128c005abd8c35e2b3c2fc2a3e6dedae6cb3050d1e2096927652c8346417";
const LND_SETTLED: &str = r#"{"memo":"Sale 10","r_preimage":"nhKxAff22/dYTRtWehkjhqIS7/0ANbEE0VYB7ymrweQ=","r_hash":"0MkSjABavYw14rPC/Co+be2ubLMFDR4glpJ2Usg0ZBc=","value":"21000","value_msat":"21000000","settled":true,"creation_date":"1794524801","settle_date":"1794524801","payment_request":"lnbcrt210u1p40v35ppp56ry39rqqt27ccd0zk0p0c237dhk6um9nq5x3ugykjfm99jp5vstsdqv2dskcefqxycqcqzzsxqzfvsp50ff28ht6h9drd8plq3uc8efew8k6vflmkd3wmz6thma9dem3vy5q9qxpqysgqkcszvswm4unf79g74ucyeu83vte9p8y4f8pyxp78u9u7ruqught95ccxha2q3hp9trca744asrl6l4t949s79jaq2s5dquxguej3mpsp23exc9","description_hash":"","expiry":"300","fallback_addr":"","cltv_expiry":"80","route_hints":[],"private":false,"add_index":"2","settle_index":"1","amt_paid":"21000000","amt_paid_sat":"21000","amt_paid_msat":"21000000","state":"SETTLED","htlcs":[{"chan_id":"135239930281984","htlc_index":"0","amt_msat":"21000000","accept_height":129,"accept_time":"1794524801","resolve_time":"1794524801","expiry_height":209,"state":"SETTLED","custom_records":{},"mpp_total_amt_msat":"21000000","amp":null,"custom_channel_data":""}],"features":{"8":{"name":"tlv-onion","is_required":true,"is_known":true},"14":{"name":"payment-addr","is_required":true,"is_known":true},"17":{"name":"multi-path-payments","is_required":false,"is_known":true},"25":{"name":"route-blinding","is_required":false,"is_known":true}},"is_keysend":false,"payment_addr":"elKj3Xq5WjacPwR5g+U5ce2mJ/uzYu2LS776VudxYSg=","is_amp":false,"amp_invoice_state":{},"is_blinded":false,"blinded_path_config":null}"#;

const LND_CANCELED_HASH: &str = "2af49b796b252f8fc8fd2984cae6c66f2ff369e2d19eb529446e3f7d323a3483";
const LND_CANCELED: &str = r#"{"memo":"Sale 11","r_preimage":"ZF4n1Q6mkPi8decYtoeIAdabc1h68dDTqxOp4shDlJ4=","r_hash":"KvSbeWslL4/I/SmEyubGby/zaeLRnrUpRG4/fTI6NIM=","value":"21000","value_msat":"21000000","settled":false,"creation_date":"1794524802","settle_date":"0","payment_request":"lnbcrt210u1p40v35zpp59t6fk7tty5hclj8a9xzv4ekxduhlx60z6x0t222ydclh6v36xjpsdqv2dskcefqxycscqzzsxqppsp5uusuq8vzflh33w4lv5586mscxkrmcxqrvx0ufz0n3x5l5x9caz3s9qxpqysgqtdgrwmzra72zn25msnrj3pvjmsuqangsd3knxuwc7gl755t80usyx5ljhvldvnqck5naz4n3tt5jjfefsg5rg4u2pw8hfdt8gs24qgsqlz9txl","description_hash":"","expiry":"1","fallback_addr":"","cltv_expiry":"80","route_hints":[],"private":false,"add_index":"3","settle_index":"0","amt_paid":"0","amt_paid_sat":"0","amt_paid_msat":"0","state":"CANCELED","htlcs":[],"features":{"8":{"name":"tlv-onion","is_required":true,"is_known":true},"14":{"name":"payment-addr","is_required":true,"is_known":true},"17":{"name":"multi-path-payments","is_required":false,"is_known":true},"25":{"name":"route-blinding","is_required":false,"is_known":true}},"is_keysend":false,"payment_addr":"5yHAHYJP7xi6v2UofW4YNYe8GANhn8SJ84mp+hi46KM=","is_amp":false,"amp_invoice_state":{},"is_blinded":false,"blinded_path_config":null}"#;

const CLN_UNPAID_HASH: &str = "c2c46427008a431d60a539d08a80a41d0e002ff9fd5c63b9d94bf883819663c9";
const CLN_UNPAID: &str = r#"{"invoices":[{"amount_msat":21000000,"bolt11":"lnbcrt210u1p40v35rsp52w9uwwllert6565mtm9ys7c3w5wnhwaqcey7sj68z89udjme78sspp5ctzxgfcq3fp36c9988gg4q9yr58qqtlel4wx8wwef0ug8qvkv0ysdqv2dskcefqxvcqxqzfvcqp2rzjqtymfc5utec56zty2ejfhpkr5fkfnzxjgkq94nvw8l9udpuzckhjqqqqw5qqqqgqqqqqqqlgqqqqqqgq2q9qxpqysgq2vzh23req9lvrxawxk62mvknakek35c3n260rt8wjmmtz4j8mcts428fca4muhcg4rtucjje0ktju3pm5esk9f7ykrmpwx86pfvn20sqpvmqey","created_index":4,"description":"Sale 30","expires_at":1794525103,"label":"obmt-0b0b0b0b-30-0","payment_hash":"c2c46427008a431d60a539d08a80a41d0e002ff9fd5c63b9d94bf883819663c9","status":"unpaid"}]}"#;

const CLN_PAID_HASH: &str = "c2c46427008a431d60a539d08a80a41d0e002ff9fd5c63b9d94bf883819663c9";
const CLN_PAID: &str = r#"{"invoices":[{"amount_msat":21000000,"amount_received_msat":21000000,"bolt11":"lnbcrt210u1p40v35rsp52w9uwwllert6565mtm9ys7c3w5wnhwaqcey7sj68z89udjme78sspp5ctzxgfcq3fp36c9988gg4q9yr58qqtlel4wx8wwef0ug8qvkv0ysdqv2dskcefqxvcqxqzfvcqp2rzjqtymfc5utec56zty2ejfhpkr5fkfnzxjgkq94nvw8l9udpuzckhjqqqqw5qqqqgqqqqqqqlgqqqqqqgq2q9qxpqysgq2vzh23req9lvrxawxk62mvknakek35c3n260rt8wjmmtz4j8mcts428fca4muhcg4rtucjje0ktju3pm5esk9f7ykrmpwx86pfvn20sqpvmqey","created_index":4,"description":"Sale 30","expires_at":1794525103,"label":"obmt-0b0b0b0b-30-0","paid_at":1794524803,"pay_index":1,"payment_hash":"c2c46427008a431d60a539d08a80a41d0e002ff9fd5c63b9d94bf883819663c9","payment_preimage":"922b64634cfd59f809844d67e0dba98c0b1604c6200ca2d48db83aaa7661b75b","status":"paid","updated_index":1}]}"#;

const CLN_EXPIRED_HASH: &str = "472823cee6a992c08331708892d0e5cbe267b4cf9acd4117c645fed3096d8b96";
const CLN_EXPIRED: &str = r#"{"invoices":[{"amount_msat":21000000,"bolt11":"lnbcrt210u1p40v35rsp5dhf6hwky2ksj2cwlpn3m7an45wz2ev74xnm28m9s7jprpmm8khfqpp5gu5z8nhx4xfvpqe3wzyf9589e03x0dx0ntx5z97xghldxztd3wtqdqv2dskcefqxvcsxqppcqp2rzjqtymfc5utec56zty2ejfhpkr5fkfnzxjgkq94nvw8l9udpuzckhjqqqqw5qqqqgqqqqqqqlgqqqqqqgq2q9qxpqysgqf594vgfc896wmal26g6rz8vtpzyc0vzaphcrmckg3as0exr429u9u93u599xzw6qwvzfggzrldspn5vpmlndntep60cdhjfpmc2ek8gqtk63rt","created_index":5,"description":"Sale 31","expires_at":1794524804,"label":"obmt-0b0b0b0b-31-0","payment_hash":"472823cee6a992c08331708892d0e5cbe267b4cf9acd4117c645fed3096d8b96","status":"expired","updated_index":2}]}"#;

const LND_NONE: &str = r#"{"code":5,"message":"there are no existing invoices","details":[]}"#;
const LND_UNKNOWN: &str = r#"{"code":5,"message":"unable to locate invoice","details":[]}"#;
const LND_DENIED: &str = r#"{"code":2,"message":"permission denied","details":[]}"#;
const LND_NO_MACAROON: &str = r#"{"code":2,"message":"expected 1 macaroon, got 0","details":[]}"#;
const CLN_NOT_PERMITTED: &str = r#"{"code":1502,"data":null,"message":"Not permitted: method is not equal to invoice AND method is not equal to listinvoices"}"#;
const CLN_NOT_FROM_NODE: &str =
    r#"{"code":1501,"data":null,"message":"Not authorized: Not derived from master"}"#;
const CLN_MISSING_RUNE: &str =
    r#"{"code":1501,"data":null,"message":"Not authorized: Missing rune"}"#;
const CLN_REVOKED: &str =
    r#"{"code":1503,"data":null,"message":"Not authorized: Blacklisted rune"}"#;
const CLN_DUPLICATE: &str =
    r#"{"code":900,"data":null,"message":"Duplicate label 'obmt-0b0b0b0b-70-0'"}"#;
const CLN_NONE: &str = r#"{"invoices":[]}"#;

#[test]
fn ln_06_real_lnd_denies_a_call_the_macaroon_does_not_allow_with_500_code_2() {
    assert_eq!(node::judge_probe(&reply(500, LND_DENIED)), Probe::Denied);
    // The other 500 that LND writes for a macaroon problem is not the denial.
    assert_eq!(
        node::judge_probe(&reply(500, LND_NO_MACAROON)),
        Probe::Unclear
    );
}

#[test]
fn ln_09_real_clnrest_answers_401_with_code_1502_to_a_call_the_rune_does_not_allow() {
    assert_eq!(
        cln_node::judge_probe(&reply(401, CLN_NOT_PERMITTED)),
        Probe::Denied
    );
}

#[test]
fn ln_09_real_clnrest_tells_a_foreign_rune_a_revoked_rune_and_a_missing_rune_apart() {
    assert_eq!(
        cln_node::judge_probe(&reply(401, CLN_NOT_FROM_NODE)),
        Probe::NotFromNode
    );
    assert_eq!(
        cln_node::judge_probe(&reply(403, CLN_MISSING_RUNE)),
        Probe::NotFromNode
    );
    assert_eq!(
        cln_node::judge_probe(&reply(401, CLN_REVOKED)),
        Probe::Revoked
    );
}

#[test]
fn ln_14_real_lnd_open_settled_and_canceled_invoices_read_as_the_rule_says() {
    let paid = lnd()
        .parse_lookup(&reply(200, LND_SETTLED), &h(LND_SETTLED_HASH), ASKED_MSAT)
        .unwrap();
    match paid {
        Payment::Paid(f) => {
            assert_eq!(f.amount_received_msat, ASKED_MSAT);
            assert!(f.settled_at.is_some());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        lnd()
            .parse_lookup(&reply(200, LND_OPEN), &h(LND_OPEN_HASH), ASKED_MSAT)
            .unwrap(),
        Payment::NotPaid(NotPaid::Waiting)
    );
    assert_eq!(
        lnd()
            .parse_lookup(&reply(200, LND_CANCELED), &h(LND_CANCELED_HASH), ASKED_MSAT)
            .unwrap(),
        Payment::NotPaid(NotPaid::Cancelled)
    );
}

#[test]
fn ln_14_real_lnd_404_code_5_is_not_found_with_either_of_its_two_messages() {
    for body in [LND_NONE, LND_UNKNOWN] {
        assert_eq!(
            lnd()
                .parse_lookup(&reply(404, body), &[0x5a; 32], ASKED_MSAT)
                .unwrap(),
            Payment::NotPaid(NotPaid::NotFound)
        );
    }
}

#[test]
fn ln_14_real_cln_unpaid_paid_and_expired_invoices_read_as_the_rule_says() {
    let paid = cln()
        .parse_lookup(&reply(201, CLN_PAID), &h(CLN_PAID_HASH), ASKED_MSAT)
        .unwrap();
    match paid {
        Payment::Paid(f) => {
            assert_eq!(f.amount_received_msat, ASKED_MSAT);
            assert!(f.settled_at.is_some());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        cln()
            .parse_lookup(&reply(201, CLN_UNPAID), &h(CLN_UNPAID_HASH), ASKED_MSAT)
            .unwrap(),
        Payment::NotPaid(NotPaid::Waiting)
    );
    assert_eq!(
        cln()
            .parse_lookup(&reply(201, CLN_EXPIRED), &h(CLN_EXPIRED_HASH), ASKED_MSAT)
            .unwrap(),
        Payment::NotPaid(NotPaid::Expired)
    );
    assert_eq!(
        cln()
            .parse_lookup(&reply(201, CLN_NONE), &[0x5a; 32], ASKED_MSAT)
            .unwrap(),
        Payment::NotPaid(NotPaid::NotFound)
    );
}

#[test]
fn ln_15_real_cln_answers_error_900_to_a_label_that_is_used() {
    assert_eq!(
        cln().parse_create(&reply(500, CLN_DUPLICATE)).unwrap(),
        CreateOutcome::LabelExists
    );
    assert!(matches!(
        lnd().parse_lookup(&reply(500, CLN_DUPLICATE), &[0; 32], ASKED_MSAT),
        Err(ReplyRefused::NodeSaid { status: 500, .. })
    ));
}
