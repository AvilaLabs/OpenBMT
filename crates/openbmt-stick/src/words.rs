//! The words of every screen (SK-15). Nothing on a screen is a sentence made
//! anywhere else: only these constants, the words of the list, numbers, hex
//! and key names. Each constant is distinct, and some screen shows each.

/// The first line of the first screen (SK-10).
pub const SK_10_TITLE: &str = "OpenBMT key maker";
/// The label before Secure Boot's state on the first screen (SK-06).
pub const SK_06_SECURE_BOOT: &str = "Secure Boot:";
/// Secure Boot's state when the firmware says it is on (SK-06).
pub const SK_06_ON: &str = "on";
/// Secure Boot's state when the firmware says it is off (SK-06, SK-07).
pub const SK_06_OFF: &str = "off. This computer did not check the stick.";
/// Secure Boot's state when the firmware gives no value (SK-06).
pub const SK_06_UNKNOWN: &str = "unknown. This computer did not say.";
/// The label before the kernel's lockdown mode on the first screen (SK-06).
pub const SK_06_LOCKDOWN: &str = "Kernel lockdown:";
/// The label before every network device the kernel has, all left off (SK-12).
pub const SK_12_INTERFACES: &str = "Network devices, none turned on:";
/// After that label when the kernel has no network device at all (SK-12).
pub const SK_12_NONE: &str = "none found";
/// The label before the program's own version on the first screen (SK-31).
pub const SK_31_VERSION: &str = "Version:";
/// The label before the SHA-256 of initrd.img as the kernel loaded it (SK-31).
pub const SK_31_INITRD_HASH: &str = "Fingerprint of this program:";
/// Under the facts: compare them with the release page (SK-30, SK-31).
pub const SK_31_COMPARE: &str =
    "Compare the version and fingerprint with the release page before you go on.";
/// The first screen's choice: the computer's random numbers and, if wanted, dice (SK-20).
pub const SK_20_MENU_MIX: &str = "Make a wallet, with dice if you like";
/// The first screen's choice: dice alone (SK-22).
pub const SK_22_MENU_DICE: &str = "Make a wallet from dice alone";
/// The first screen's choice: type 12 words to check them (SK-28).
pub const SK_28_MENU_CHECK: &str = "Check words I wrote";
/// On the first screen: how to choose (SK-10).
pub const SK_10_MENU_KEYS: &str = "Press 1, 2 or 3.";
/// The title of the mixed-rolls screen, before the count of rolls (SK-21).
pub const SK_21_ROLLS: &str = "Dice rolls so far:";
/// On that screen: the computer's numbers are mixed in, and 50 rolls are enough alone (SK-20, SK-21).
pub const SK_21_ENOUGH: &str = "The computer's own random numbers are mixed in, so any number of rolls helps. 50 rolls are enough on their own.";
/// On that screen: the keys (SK-21).
pub const SK_21_KEYS: &str =
    "Type 1 to 6 for each roll. Backspace undoes the last. Press Enter when you are done.";
/// The title of the dice-alone screen, before the count of rolls made (SK-22).
pub const SK_22_ROLLS: &str = "Rolls made:";
/// On that screen, the label before the rolls kept out of 50 (SK-22, KM-30).
pub const SK_22_VALUES: &str = "Kept:";
/// On that screen: why fewer are kept than made (KM-30).
pub const SK_22_WHY: &str = "Rolls are taken in threes. A three with a number repeated is not kept, so this takes about 270 rolls.";
/// On that screen: the keys (SK-22).
pub const SK_22_KEYS: &str =
    "Type 1 to 6 for each roll. Backspace undoes the last. Enter goes on once 50 are kept.";
/// The title of a screen of four numbered words, before the page number (SK-25).
pub const SK_25_WORDS: &str = "Write these on the card in pen. Page";
/// On that screen: never a photo (SK-25).
pub const SK_25_NO_PHOTO: &str = "Never take a photo of this screen.";
/// On that screen: Enter goes on (SK-25).
pub const SK_25_NEXT: &str = "Press Enter once they are written.";
/// On the words screen after a wrong answer, before the first wrong position (SK-25, KM-16).
pub const SK_25_WRONG: &str = "Check your card. The word that did not match is number";
/// The title of the check screen (SK-25).
pub const SK_25_CHECK: &str = "Check your card";
/// On the check screen, before the position asked (SK-25).
pub const SK_25_ASK: &str = "Word number";
/// On the check screen: what to type (SK-25, KM-16).
pub const SK_25_TYPE: &str = "Type its first four letters and press Enter.";
/// The title of the public result screen (SK-26).
pub const SK_26_RESULT: &str = "Your wallet. This part is safe to show.";
/// The label before the master fingerprint (SK-26).
pub const SK_26_FINGERPRINT: &str = "Fingerprint:";
/// The label before the first receive address (SK-26).
pub const SK_26_ADDRESS: &str = "First address:";
/// The label before the wallet's descriptor (SK-26).
pub const SK_26_DESCRIPTOR: &str = "Wallet description:";
/// The title of the descriptor's QR screen (SK-26).
pub const SK_26_SCAN: &str = "On the owner phone, open Connect your wallet and scan this.";
/// After "Enter": show the descriptor as a QR code (SK-26).
pub const SK_26_SHOW_QR: &str = "to show it as a QR code for the owner phone";
/// After "Enter" on a QR screen: back to the text (SK-26).
pub const SK_26_SHOW_TEXT: &str = "to go back";
/// After "S": show the words as a Compact SeedQR (SK-27).
pub const SK_27_SEEDQR: &str = "to show the words as a SeedQR, to copy onto steel";
/// On the SeedQR screen (SK-27).
pub const SK_27_WARNING: &str =
    "This code is your key. Anyone with a photo of it can take the money. Copy it by hand.";
/// The title of the typed-words screen, before the word number asked (SK-28).
pub const SK_28_TYPE: &str = "Type word number";
/// On that screen: the letters are no word of the list (SK-28).
pub const SK_28_BAD_WORD: &str = "That is not one of the words on the list. Check the spelling.";
/// On that screen: the 12 words fail the BIP39 checksum (SK-28).
pub const SK_28_BAD_CHECKSUM: &str =
    "These 12 words do not make a wallet. One of them is wrong or out of order.";
/// On that screen: the keys (SK-28).
pub const SK_28_KEYS: &str =
    "Type the word or its first four letters, then press Enter. Backspace erases.";
/// After "Esc", on the first screen and every screen after the words (SK-29).
pub const SK_29_TURN_OFF: &str = "to turn off. Nothing is kept.";

/// Every constant above, for the test that each is distinct and shown.
pub const ALL: &[&str] = &[
    SK_10_TITLE,
    SK_06_SECURE_BOOT,
    SK_06_ON,
    SK_06_OFF,
    SK_06_UNKNOWN,
    SK_06_LOCKDOWN,
    SK_12_INTERFACES,
    SK_12_NONE,
    SK_31_VERSION,
    SK_31_INITRD_HASH,
    SK_31_COMPARE,
    SK_20_MENU_MIX,
    SK_22_MENU_DICE,
    SK_28_MENU_CHECK,
    SK_10_MENU_KEYS,
    SK_21_ROLLS,
    SK_21_ENOUGH,
    SK_21_KEYS,
    SK_22_ROLLS,
    SK_22_VALUES,
    SK_22_WHY,
    SK_22_KEYS,
    SK_25_WORDS,
    SK_25_NO_PHOTO,
    SK_25_NEXT,
    SK_25_WRONG,
    SK_25_CHECK,
    SK_25_ASK,
    SK_25_TYPE,
    SK_26_RESULT,
    SK_26_FINGERPRINT,
    SK_26_ADDRESS,
    SK_26_DESCRIPTOR,
    SK_26_SCAN,
    SK_26_SHOW_QR,
    SK_26_SHOW_TEXT,
    SK_27_SEEDQR,
    SK_27_WARNING,
    SK_28_TYPE,
    SK_28_BAD_WORD,
    SK_28_BAD_CHECKSUM,
    SK_28_KEYS,
    SK_29_TURN_OFF,
];

/// What the first screen shows after [`SK_06_SECURE_BOOT`] for the
/// firmware's `SecureBoot` value (`1`, `0`, or `?` when there is none).
pub fn secure_boot_state(value: &str) -> &'static str {
    match value {
        "1" => SK_06_ON,
        "0" => SK_06_OFF,
        _ => SK_06_UNKNOWN,
    }
}
