//! The address a customer sends for a return (RQ-27, RQ-28, RQ-33).
//!
//! Only a plain address is read here. A `bitcoin:` link is refused as "not
//! an address" until a strict BIP 321 reader is added; the amount of a link
//! is to be ignored when it is (RQ-28).

use std::str::FromStr;

use bitcoin::address::NetworkUnchecked;
use bitcoin::{Address, Network, Script, ScriptBuf};
use openbmt_counter::refund::is_payment_script;

use crate::error::AddressRefused;

/// The longest text that is read as an address, in bytes. A bech32 address
/// is at most 90 characters; a longer text cannot be one.
pub const MAX_ADDRESS_BYTES: usize = 128;

/// RQ-28, RQ-33: the script behind a pasted or scanned address. It must be
/// a valid mainnet address of a standard kind (P2WPKH, P2WSH, P2TR, P2SH,
/// P2PKH) and not one for which `is_own` is true (an address of the shop's
/// own wallet). Spaces around it are ignored. Any text at all gives an
/// answer and never a panic.
pub fn check_return_address(
    text: &str,
    is_own: impl Fn(&Script) -> bool,
) -> Result<ScriptBuf, AddressRefused> {
    let text = text.trim_ascii();
    if text.is_empty()
        || text.len() > MAX_ADDRESS_BYTES
        || !text.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(AddressRefused::NotAnAddress);
    }
    let parsed =
        Address::<NetworkUnchecked>::from_str(text).map_err(|_| AddressRefused::NotAnAddress)?;
    let checked = parsed
        .require_network(Network::Bitcoin)
        .map_err(|_| AddressRefused::TestNetwork)?;
    let script = checked.script_pubkey();
    if !is_payment_script(&script) {
        return Err(AddressRefused::NotAnAddress);
    }
    if is_own(&script) {
        return Err(AddressRefused::Own);
    }
    Ok(script)
}

/// RQ-28: the address of a script written in groups of four characters, for
/// the owner to compare with the customer's. `None` for a script that is
/// not of a standard kind.
pub fn address_in_groups(script: &Script) -> Option<String> {
    if !is_payment_script(script) {
        return None;
    }
    let address = Address::from_script(script, Network::Bitcoin)
        .ok()?
        .to_string();
    let mut out = String::with_capacity(address.len() + address.len() / 4);
    for (i, c) in address.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    Some(out)
}
