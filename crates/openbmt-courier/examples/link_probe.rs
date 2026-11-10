//! Small checks against a relay, for `scripts/link-e2e-core.sh`.
//!
//! ```text
//! link_probe key <hex byte>
//! link_probe stranger <relay address> <relay key hex> <owner key hex>
//! ```
//!
//! **Test only, never use.** The identities are made from fixed bytes that
//! anyone can read here: the device whose secret is the byte `n` repeated 32
//! times is the one `key n` prints.
//!
//! - `key <hex byte>` prints the device key (64 hex characters) of the
//!   identity made from that byte repeated 32 times. The example owner uses
//!   11 and the example counter 13.
//! - `stranger` acts as a device whose key (15 repeated) is not a member of
//!   the relay, and tries what a member could do: make a mailbox (LK-21), put
//!   a sealed message for the owner (LK-20), and fetch its own mailbox
//!   (LK-23). It prints one line for each attempt, `make refused <text>` or
//!   `make accepted`, and the same for `put` and `fetch`.

use std::net::SocketAddr;
use std::process::ExitCode;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_link::{os_random, seal, Client, ClientConfig, Kind, Role, Route};

const USAGE: &str = "usage: link_probe key <hex byte> | \
                     link_probe stranger <relay address> <relay key hex> <owner key hex>";

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err("the hex is not whole bytes".to_string());
    }
    (0..text.len() / 2)
        .map(|i| {
            text.get(2 * i..2 * i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "the hex has a character that is not a digit".to_string())
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn device(text: &str) -> Result<DeviceId, String> {
    let bytes: [u8; 32] = unhex(text)?
        .try_into()
        .map_err(|_| "a key is not 32 bytes".to_string())?;
    DeviceId::from_bytes(&bytes).map_err(|e| e.to_string())
}

fn identity_of(byte: u8) -> Result<Identity, String> {
    Identity::from_secret_bytes(&[byte; 32]).map_err(|e| e.to_string())
}

fn report(what: &str, result: Result<String, impl std::fmt::Display>) {
    match result {
        Ok(text) => println!("{what} accepted {text}"),
        Err(e) => println!("{what} refused {e}"),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, byte] if cmd == "key" => {
            let byte = unhex(byte)?;
            let [byte] = byte.as_slice() else {
                return Err("give one byte".to_string());
            };
            println!("{}", hex(&identity_of(*byte)?.device_id().to_bytes()));
            Ok(())
        }
        [cmd, addr, relay, owner] if cmd == "stranger" => {
            let addr: SocketAddr = addr.parse().map_err(|_| format!("address: {addr}"))?;
            let relay = device(relay)?;
            let owner = device(owner)?;
            let me = identity_of(0x15)?;
            let route = Route::Direct(addr);
            let cfg = ClientConfig::default();
            let mut client =
                Client::connect(&route, Some(&relay), &cfg).map_err(|e| e.to_string())?;
            report("make", client.make_mailbox(&me).map(|()| String::new()));
            let sealed = seal(
                &me,
                Role::Counter,
                &owner,
                Role::Owner,
                1,
                Kind::Records,
                b"let me in",
                &mut os_random,
            )
            .map_err(|e| e.to_string())?;
            report("put", client.put(&sealed).map(|id| format!("id={id}")));
            report(
                "fetch",
                client
                    .fetch(&me, 0)
                    .map(|f| format!("{} messages", f.items.len())),
            );
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("link_probe: {why}");
            ExitCode::from(2)
        }
    }
}
