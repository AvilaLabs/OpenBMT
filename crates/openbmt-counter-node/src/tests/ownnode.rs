//! The own-node setting (CR-100 to CR-102).

use bitcoin::Network;
use openbmt_core::say::check;
use openbmt_net_io::OwnNodeHost;

use super::kit::OWN_NAME;
use crate::ownnode::{default_port, every_setting_refusal, own_node_setting};
use crate::OwnNodeSettingRefused;

fn set(text: &str) -> Result<crate::OwnNodeSetting, OwnNodeSettingRefused> {
    own_node_setting(text, Network::Regtest, true)
}

#[test]
fn cr_100_a_tor_v3_name_with_or_without_a_port_is_taken() {
    let with = set(&format!("{OWN_NAME}:8333")).unwrap();
    assert_eq!(with.text, format!("{OWN_NAME}:8333"));
    assert_eq!(with.node.port(), 8_333);
    assert!(with.node.is_authenticated());
    assert_eq!(*with.node.host(), OwnNodeHost::Onion(OWN_NAME.to_string()));
    // No port: the network's usual one, and the kept text says it.
    let without = set(OWN_NAME).unwrap();
    assert_eq!(without.text, format!("{OWN_NAME}:18444"));
    let main = own_node_setting(OWN_NAME, Network::Bitcoin, true).unwrap();
    assert_eq!(main.node.port(), 8_333);
    let signet = own_node_setting(OWN_NAME, Network::Signet, true).unwrap();
    assert_eq!(signet.node.port(), 38_333);
    assert_eq!(default_port(Network::Regtest), 18_444);
    // The text the manager typed may have spaces around it.
    assert_eq!(set(&format!("  {OWN_NAME} ")).unwrap(), without);
    // What is kept is read back to the same node.
    assert_eq!(set(&without.text).unwrap(), without);
}

#[test]
fn cr_100_a_name_that_is_not_a_tor_v3_name_or_a_bad_port_is_refused() {
    let upper = OWN_NAME.to_uppercase().replace(".ONION", ".onion");
    let wrong_sum = format!("{}b.onion", &OWN_NAME[..55]);
    let short = format!("{}.onion", &OWN_NAME[..40]);
    for bad in ["", ".onion", &upper, &wrong_sum, &short] {
        assert_eq!(set(bad), Err(OwnNodeSettingRefused::BadName), "{bad:?}");
    }
    for bad in [
        format!("{OWN_NAME}:0"),
        format!("{OWN_NAME}:65536"),
        format!("{OWN_NAME}:x"),
        format!("{OWN_NAME}:"),
    ] {
        assert_eq!(set(&bad), Err(OwnNodeSettingRefused::BadPort), "{bad:?}");
    }
}

#[test]
fn cr_101_a_home_network_or_clear_net_address_is_refused_with_its_own_words() {
    for bad in [
        "192.168.1.20",
        "192.168.1.20:8333",
        "10.0.0.5:8333",
        "127.0.0.1:18444",
        "[::1]:8333",
        "[fe80::1]:8333",
        "node.local:8333",
        "umbrel.local",
        "example.com:8333",
        "localhost",
        "abc",
    ] {
        assert_eq!(set(bad), Err(OwnNodeSettingRefused::NotTor), "{bad}");
    }
    // The same without Tor: the address is wrong before Tor is.
    assert_eq!(
        own_node_setting("192.168.1.20:8333", Network::Regtest, false),
        Err(OwnNodeSettingRefused::NotTor)
    );
    let words = OwnNodeSettingRefused::NotTor.words();
    assert!(words.what.contains("cannot prove which machine answers"));
    assert!(words.next.contains("ordinary peer"), "{}", words.next);
    assert_eq!(words.code, Some("CR-101"));
}

#[test]
fn cr_102_without_a_tor_proxy_the_own_node_is_refused_with_words_to_turn_on_orbot() {
    assert_eq!(
        own_node_setting(OWN_NAME, Network::Regtest, false),
        Err(OwnNodeSettingRefused::NeedsTor)
    );
    let words = OwnNodeSettingRefused::NeedsTor.words();
    assert_eq!(words.heading, "Turn on Orbot first");
    assert_eq!(words.code, Some("CR-102"));
    // A name that is wrong is told as wrong, whatever the proxy.
    assert_eq!(
        own_node_setting("nonsense.onion", Network::Regtest, false),
        Err(OwnNodeSettingRefused::BadName)
    );
}

#[test]
fn cr_100_every_refusal_has_words_that_keep_the_rules_and_a_rule_number() {
    for (name, refused) in every_setting_refusal() {
        let words = refused.words();
        assert_eq!(check(&words), Ok(()), "{name}");
        assert_eq!(words.code, Some(refused.rule()), "{name}");
        assert!(refused.to_string().starts_with(refused.rule()), "{name}");
    }
}
