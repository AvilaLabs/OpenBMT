//! What the owner is told when the text they scanned is refused
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12; `docs/WALLET-RULES.md`, WL-15).
//!
//! The words carry nothing of the text: it may hold a private key.

use super::{Tone, Who, Words};
use crate::wallet_single::ImportRefused;

/// Every refusal.
pub const ALL_REFUSED: [ImportRefused; 4] = [
    ImportRefused::HasPrivateKey,
    ImportRefused::Damaged,
    ImportRefused::TestNetwork,
    ImportRefused::Unsupported,
];

/// The name of a refusal in the list of screens.
pub fn name(r: &ImportRefused) -> &'static str {
    match r {
        ImportRefused::HasPrivateKey => "ImportRefused::HasPrivateKey",
        ImportRefused::Damaged => "ImportRefused::Damaged",
        ImportRefused::TestNetwork => "ImportRefused::TestNetwork",
        ImportRefused::Unsupported => "ImportRefused::Unsupported",
    }
}

impl ImportRefused {
    /// What the owner is told.
    pub fn words(&self) -> Words {
        let (heading, what, next, code) = match self {
            Self::HasPrivateKey => (
                "This holds a private key",
                "What you scanned can spend the money, not just watch it. Anyone who has seen it can take the bitcoin.",
                "Do not use this wallet for the shop. Move any bitcoin in it to a new wallet, then scan the new wallet's public description.",
                "WL-08",
            ),
            Self::Damaged => (
                "This description is damaged",
                "Its checksum does not match, so part of it was misread or changed.",
                "Scan or export it again from your wallet.",
                "WL-10",
            ),
            Self::TestNetwork => (
                "This is a test-network wallet",
                "It is for testnet or signet, not real bitcoin.",
                "Export the description of your real bitcoin account and scan that.",
                "WL-11",
            ),
            Self::Unsupported => (
                "This wallet type is not supported",
                "OpenBMT can check native SegWit single-key wallets and its own 2-of-3 multisig. This is a different kind.",
                "In your wallet, export the public description of a native SegWit account (it starts with wpkh) and scan that.",
                "WL-12",
            ),
        };
        Words::new(Tone::Stop, heading, what, Who::Owner, next).code(code)
    }
}

/// The words of every refusal, named, for SC-12.
pub fn all_words() -> Vec<(String, Words)> {
    ALL_REFUSED
        .iter()
        .map(|r| (name(r).to_string(), r.words()))
        .collect()
}

/// The list of screens as it is written to `docs/screens/core.md`.
pub fn screens_markdown() -> String {
    let who = |w: Who| match w {
        Who::Nobody => "nobody",
        Who::Customer => "the customer",
        Who::Cashier => "the cashier",
        Who::Manager => "a manager",
        Who::Owner => "the owner",
        Who::KeyHolder => "the key holder",
        Who::BoxKeeper => "the Box keeper",
    };
    let cell = |text: &str| text.replace('|', "/");
    let mut out = String::from(
        "# Core: what a person is told\n\n\
         Made by `screens_markdown` in `crates/openbmt-core/src/say/import.rs`. Do\n\
         not edit it by hand: run the test `sc_12_the_list_is_on_disk` with the\n\
         environment variable `OPENBMT_WRITE_SCREENS=1` to write it again.\n\n\
         | State | Heading | What happened | What to do next | Who | Code |\n\
         |---|---|---|---|---|---|\n",
    );
    for (name, words) in all_words() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell(&name),
            cell(&words.heading),
            cell(&words.what),
            cell(&words.next),
            who(words.who),
            words.code.unwrap_or("")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::say::check_distinct;

    #[test]
    fn sc_12_every_word_passes_the_check_and_none_repeats() {
        check_distinct(&all_words()).expect("the words pass");
    }

    #[test]
    fn sc_12_a_variant_missing_from_the_list_fails() {
        // An exhaustive match: a new variant does not compile until it is
        // named here, and then it must be in the list.
        let listed = |r: &ImportRefused| match r {
            ImportRefused::HasPrivateKey
            | ImportRefused::Damaged
            | ImportRefused::TestNetwork
            | ImportRefused::Unsupported => ALL_REFUSED.contains(r),
        };
        for r in [
            ImportRefused::HasPrivateKey,
            ImportRefused::Damaged,
            ImportRefused::TestNetwork,
            ImportRefused::Unsupported,
        ] {
            assert!(listed(&r));
        }
        assert_eq!(all_words().len(), ALL_REFUSED.len());
    }

    #[test]
    fn sc_05_the_owner_takes_the_next_step() {
        for (name, w) in all_words() {
            assert_eq!(w.who, Who::Owner, "{name}");
            assert_eq!(w.tone, Tone::Stop, "{name}");
        }
    }

    #[test]
    fn sc_12_the_list_is_on_disk() {
        let path = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR")
                .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
        )
        .join("../../docs/screens/core.md");
        let want = screens_markdown();
        if std::env::var_os("OPENBMT_WRITE_SCREENS").is_some() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).expect("make docs/screens");
            }
            std::fs::write(&path, &want).expect("write docs/screens/core.md");
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            have == want,
            "docs/screens/core.md is not what screens_markdown makes: run this test with \
             OPENBMT_WRITE_SCREENS=1 to write it again"
        );
    }
}
