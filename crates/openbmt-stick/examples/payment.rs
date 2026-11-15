//! The payment side of `stick/test-qemu.sh` (SK-58): what the owner phone
//! shows, and what the stick must give back. Test words only; the payment is
//! for real bitcoin and is never broadcast.
//!
//!   payment make <dir>
//!       writes `sent.psbt`, `ur.frames` and `bbqr.frames` (a frame's QR
//!       text a line) of a payment of the test words
//!   payment verify <sent.psbt> <ur|bbqr> <matrices>
//!       reads the QR matrices of the stick's screens (blocks of `#` and `.`
//!       rows separated by a blank line), decodes them, and checks that they
//!       make the signed payment: the form it came in, every signature valid
//!       under the own-spend finishing (`openbmt_spend::take_wallet_signature`
//!       and `finalize`, which `openbmt-owner`'s `take_own` calls), and the
//!       file otherwise unchanged
//!   payment message-make <dir>
//!       writes `message.code` (a settings change as the owner phone shows
//!       it, for the account 3 key of the test words with a passphrase) and
//!       prints what the stick must show, worked out here from the core
//!   payment message-verify <dir> <signature> <matrix>
//!       checks the signature with the core's check of AP-07 and that the
//!       stick's code holds the same text

#[path = "../tests/common/mod.rs"]
mod common;

use std::fs;
use std::process::ExitCode;

use bitcoin::psbt::Psbt;
use openbmt_qr::decode_luma;
use openbmt_spend::{check_finished, finalize, take_wallet_signature};
use openbmt_stick::frames::{texts_of, Collector, Feed, Form};

fn form_of(name: &str) -> Option<Form> {
    match name {
        "ur" => Some(Form::Ur),
        "bbqr" => Some(Form::Bbqr),
        _ => None,
    }
}

/// A matrix of rows as the grey frame a camera would give: 6 pixels a module
/// and a quiet zone.
fn luma_of(rows: &[&str]) -> (usize, usize, Vec<u8>) {
    let scale = 6;
    let n = rows.len();
    let side = (n + 8) * scale;
    let mut out = vec![255u8; side * side];
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c == '#' {
                for dy in 0..scale {
                    for dx in 0..scale {
                        out[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0;
                    }
                }
            }
        }
    }
    (side, side, out)
}

const MESSAGE_PATH: &str = "m/84h/0h/3h/0/0";
const MESSAGE_VERSION: u32 = 12;
const PASSPHRASE: &str = "TREZOR 1!";

/// The address the stick must sign for and the message, worked out with the
/// core: the test words and the passphrase, the key at `MESSAGE_PATH`.
fn settings_change() -> (String, String) {
    use std::str::FromStr;
    let m = bip39::Mnemonic::parse_normalized(&common::WORDS.join(" ")).expect("words");
    let seed = m.to_seed_normalized(PASSPHRASE);
    let master =
        bitcoin::bip32::Xpriv::new_master(bitcoin::NetworkKind::Main, &seed).expect("master");
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let path = bitcoin::bip32::DerivationPath::from_str(MESSAGE_PATH).expect("path");
    let key = master.derive_priv(&secp, &path).expect("key");
    let pk = bitcoin::CompressedPublicKey(key.private_key.public_key(&secp));
    let address = bitcoin::Address::p2wpkh(&pk, bitcoin::Network::Bitcoin);
    let message =
        openbmt_core::approval::wallet_message(MESSAGE_VERSION, Some(&[3u8; 33]), &[0xcd; 32]);
    (address.to_string(), message)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("make") => {
            let dir = &args[1];
            let psbt = common::big_payment(4);
            let bytes = psbt.serialize();
            fs::write(format!("{dir}/sent.psbt"), &bytes).expect("write");
            // What the review must show, worked out here from the file.
            let out_sum: u64 = psbt
                .unsigned_tx
                .output
                .iter()
                .map(|o| o.value.to_sat())
                .sum();
            let in_sum: u64 = psbt
                .inputs
                .iter()
                .map(|i| i.witness_utxo.as_ref().map_or(0, |o| o.value.to_sat()))
                .sum();
            let btc = |v: u64| format!("{}.{:08}", v / 100_000_000, v % 100_000_000);
            let first = &psbt.unsigned_tx.output[0];
            println!(
                "first-output {} {}",
                bitcoin::Address::from_script(&first.script_pubkey, bitcoin::Network::Bitcoin)
                    .expect("address"),
                btc(first.value.to_sat())
            );
            println!("fee {}", btc(in_sum - out_sum));
            println!("outputs {}", psbt.unsigned_tx.output.len());
            for (name, form) in [("ur", Form::Ur), ("bbqr", Form::Bbqr)] {
                let texts = texts_of(form, &bytes).expect("frames");
                fs::write(format!("{dir}/{name}.frames"), texts.join("\n") + "\n").expect("write");
                println!("{name} {}", texts.len());
            }
            ExitCode::SUCCESS
        }
        Some("verify") => {
            let sent = Psbt::deserialize(&fs::read(&args[1]).expect("sent")).expect("psbt");
            let form = form_of(&args[2]).expect("form");
            let text = fs::read_to_string(&args[3]).expect("matrices");
            let mut collector = Collector::new();
            let mut done = None;
            let mut frames = 0;
            for block in text.split("\n\n").filter(|b| !b.trim().is_empty()) {
                let rows: Vec<&str> = block.lines().map(str::trim).collect();
                let (w, h, luma) = luma_of(&rows);
                let Ok(d) = decode_luma(w, h, &luma) else {
                    println!("FAIL a screen's code does not decode");
                    return ExitCode::FAILURE;
                };
                frames += 1;
                let t = String::from_utf8(d.bytes).expect("text");
                if let Feed::Done { form: f, psbt } = collector.feed(&t) {
                    done = Some((f, psbt));
                    break;
                }
            }
            let Some((got_form, bytes)) = done else {
                println!("FAIL the frames never made a whole file");
                return ExitCode::FAILURE;
            };
            if got_form != form {
                println!("FAIL the form came back as {got_form:?}");
                return ExitCode::FAILURE;
            }
            println!("frames-read {frames}");
            let back = Psbt::deserialize(&bytes).expect("signed psbt");
            let wallet = common::wallet();
            let both = match take_wallet_signature(&wallet, &sent, &back) {
                Ok(b) => b,
                Err(e) => {
                    println!("FAIL the signatures: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let tx = match finalize(&wallet, &both) {
                Ok(t) => t,
                Err(e) => {
                    println!("FAIL the finishing: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match check_finished(&wallet, &sent, &tx) {
                Ok(fee) => println!("finished txid {} fee {fee}", tx.compute_txid()),
                Err(e) => {
                    println!("FAIL the finished transaction: {e}");
                    return ExitCode::FAILURE;
                }
            }
            let mut stripped = back.clone();
            for i in &mut stripped.inputs {
                i.partial_sigs.clear();
            }
            if stripped != sent {
                println!("FAIL the file changed in more than its signatures");
                return ExitCode::FAILURE;
            }
            println!("OK");
            ExitCode::SUCCESS
        }
        Some("message-make") => {
            let dir = &args[1];
            let (address, message) = settings_change();
            let code = format!("signmessage {MESSAGE_PATH} ascii:{message}");
            fs::write(format!("{dir}/message.code"), code + "\n").expect("write");
            println!("passphrase {PASSPHRASE}");
            println!("address {address}");
            println!("version {MESSAGE_VERSION}");
            println!("node {}", "03".repeat(33));
            println!("hash {}", ["cdcd"; 16].join(" "));
            ExitCode::SUCCESS
        }
        Some("message-verify") => {
            let sig = &args[2];
            let (address, message) = settings_change();
            let address: bitcoin::Address = address
                .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
                .expect("address")
                .assume_checked();
            if let Err(e) = openbmt_core::approval::verify_wallet_message(&address, &message, sig) {
                println!("FAIL the signature: {e}");
                return ExitCode::FAILURE;
            }
            let text = fs::read_to_string(&args[3]).expect("matrix");
            let rows: Vec<&str> = text.lines().map(str::trim).collect();
            let (w, h, luma) = luma_of(&rows);
            let Ok(d) = decode_luma(w, h, &luma) else {
                println!("FAIL the signature's code does not decode");
                return ExitCode::FAILURE;
            };
            if d.bytes != sig.as_bytes() {
                println!("FAIL the code holds another text than the signature");
                return ExitCode::FAILURE;
            }
            println!("signature of {address} verifies with the core (AP-07); the code holds the same text");
            println!("OK");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: payment make <dir> | payment verify <sent.psbt> <ur|bbqr> <matrices>"
            );
            ExitCode::from(2)
        }
    }
}
