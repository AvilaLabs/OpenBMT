//! CR-134, CR-136: how the counter reaches the relay it was told of, and
//! the start of its link.

use std::net::SocketAddr;

use openbmt_core::relay::{RelayHint, RelayRoute};
use openbmt_link::Route;

use super::kit::{id, Scratch};
use crate::relaylink::{link_route, start_link, LinkLine, LinkNotStarted};

const NAME: &str = "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad";

fn direct() -> RelayHint {
    RelayHint::new(
        RelayRoute::Direct("192.168.1.20:48333".parse().unwrap()),
        id(40).device_id().to_bytes(),
    )
    .unwrap()
}

fn onion() -> RelayHint {
    RelayHint::new(
        RelayRoute::Onion {
            name: NAME.to_string(),
            port: 48333,
        },
        id(40).device_id().to_bytes(),
    )
    .unwrap()
}

fn proxy() -> SocketAddr {
    "127.0.0.1:9050".parse().unwrap()
}

#[test]
fn cr_134_a_direct_relay_goes_direct_unless_the_settings_have_tor_on() {
    let addr: SocketAddr = "192.168.1.20:48333".parse().unwrap();
    // Tor off: direct, whether or not a proxy was given.
    assert_eq!(
        link_route(&direct(), None, false).unwrap(),
        Route::Direct(addr)
    );
    assert_eq!(
        link_route(&direct(), Some(proxy()), false).unwrap(),
        Route::Direct(addr)
    );
    // Tor on: through the proxy as an address, and never around it.
    assert_eq!(
        link_route(&direct(), Some(proxy()), true).unwrap(),
        Route::Proxied {
            proxy: proxy(),
            addr
        }
    );
    assert_eq!(
        link_route(&direct(), None, true).unwrap_err(),
        LinkNotStarted::NeedsProxy
    );
}

#[test]
fn cr_134_an_onion_relay_goes_through_the_proxy_and_needs_one() {
    for tor_on in [false, true] {
        assert_eq!(
            link_route(&onion(), Some(proxy()), tor_on).unwrap(),
            Route::Onion {
                proxy: proxy(),
                host: format!("{NAME}.onion"),
                port: 48333
            }
        );
        assert_eq!(
            link_route(&onion(), None, tor_on).unwrap_err(),
            LinkNotStarted::NeedsProxy
        );
    }
    assert!(LinkNotStarted::NeedsProxy.to_string().starts_with("CR-134"));
    assert!(LinkNotStarted::BadKey.to_string().starts_with("CR-134"));
    assert!(LinkNotStarted::Files("x".into())
        .to_string()
        .starts_with("CR-134"));
}

#[test]
fn cr_134_the_link_starts_on_the_data_folder_and_a_later_start_needs_its_files() {
    let dir = Scratch::new();
    let link = start_link(dir.path(), id(3), id(1).device_id(), &direct(), None, false).unwrap();
    // The owner is the one device it talks to.
    assert_eq!(link.courier().peers().len(), 1);
    assert!(link.courier().peers().contains_key(&id(1).device_id()));
    assert_eq!(link.courier().device_id(), id(3).device_id());
    assert!(dir.path().join("link-inbox").exists());
    assert!(dir.path().join("link-outbox").exists());
    // A second start on the same folder is not a first start and reads the
    // files; with one gone it needs a new pairing (LK-53).
    drop(link);
    assert!(start_link(dir.path(), id(3), id(1).device_id(), &direct(), None, false).is_ok());
    std::fs::remove_file(dir.path().join("link-outbox")).unwrap();
    let e = start_link(dir.path(), id(3), id(1).device_id(), &direct(), None, false)
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(e, LinkNotStarted::Files(_)), "{e:?}");
    // An onion relay with no proxy starts nothing and makes no file.
    let other = Scratch::new();
    assert_eq!(
        start_link(other.path(), id(3), id(1).device_id(), &onion(), None, false)
            .map(|_| ())
            .unwrap_err(),
        LinkNotStarted::NeedsProxy
    );
    assert!(!other.path().join("link-inbox").exists());
}

#[test]
fn cr_136_what_could_not_start_is_told_by_its_own_line() {
    assert_eq!(
        LinkLine::from(&LinkNotStarted::NeedsProxy),
        LinkLine::NeedsTor
    );
    for e in [LinkNotStarted::BadKey, LinkNotStarted::Files("x".into())] {
        assert_eq!(LinkLine::from(&e), LinkLine::NotStarted);
    }
}
