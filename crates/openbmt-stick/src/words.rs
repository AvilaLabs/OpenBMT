//! The words of every screen. They are under `docs/SCREEN-RULES.md` (SK-15);
//! until they are written each is a placeholder that is its own name, so a
//! screen shows which rule's words are missing. Nothing on a screen is a
//! sentence made here: only these constants, the words of the list, numbers,
//! hex and key names.

/// The label before the firmware's SecureBoot value on the first screen (SK-06).
pub const WORDS_NEEDED_SK_06_SECURE_BOOT: &str = "WORDS_NEEDED_SK_06_SECURE_BOOT";
/// The label before the kernel's lockdown mode on the first screen (SK-06).
pub const WORDS_NEEDED_SK_06_LOCKDOWN: &str = "WORDS_NEEDED_SK_06_LOCKDOWN";
/// The label before the list of every network interface the kernel has, all down, on the first screen (SK-12).
pub const WORDS_NEEDED_SK_12_INTERFACES: &str = "WORDS_NEEDED_SK_12_INTERFACES";
/// The label before the program's own version on the first screen (SK-31).
pub const WORDS_NEEDED_SK_31_VERSION: &str = "WORDS_NEEDED_SK_31_VERSION";
/// The label before the SHA-256 of initrd.img as the kernel loaded it, on the first screen (SK-31).
pub const WORDS_NEEDED_SK_31_INITRD_HASH: &str = "WORDS_NEEDED_SK_31_INITRD_HASH";
/// The first screen's choice: a key from the kernel's random bytes and, if wanted, the owner's die rolls (SK-20).
pub const WORDS_NEEDED_SK_20_MENU_MIX: &str = "WORDS_NEEDED_SK_20_MENU_MIX";
/// The first screen's choice: dice alone, no kernel bytes (SK-22).
pub const WORDS_NEEDED_SK_22_MENU_DICE: &str = "WORDS_NEEDED_SK_22_MENU_DICE";
/// The first screen's choice: type 12 words to check them (SK-28).
pub const WORDS_NEEDED_SK_28_MENU_CHECK: &str = "WORDS_NEEDED_SK_28_MENU_CHECK";
/// The title of the die-rolls-with-the-computer's screen, shown with the count of rolls so far (SK-21).
pub const WORDS_NEEDED_SK_21_ROLLS: &str = "WORDS_NEEDED_SK_21_ROLLS";
/// On that screen: 50 rolls are enough on their own (SK-21).
pub const WORDS_NEEDED_SK_21_ENOUGH: &str = "WORDS_NEEDED_SK_21_ENOUGH";
/// On that screen: the keys 1 to 6 add a roll, Backspace undoes the last, Enter goes on (SK-21).
pub const WORDS_NEEDED_SK_21_KEYS: &str = "WORDS_NEEDED_SK_21_KEYS";
/// The title of the dice-alone screen, shown with the count of rolls made (SK-22).
pub const WORDS_NEEDED_SK_22_ROLLS: &str = "WORDS_NEEDED_SK_22_ROLLS";
/// On that screen, the label before the values kept out of 50 (SK-22, KM-30).
pub const WORDS_NEEDED_SK_22_VALUES: &str = "WORDS_NEEDED_SK_22_VALUES";
/// On that screen: the keys 1 to 6 add a roll, Backspace undoes the last, Enter goes on once 50 values are kept (SK-22).
pub const WORDS_NEEDED_SK_22_KEYS: &str = "WORDS_NEEDED_SK_22_KEYS";
/// The title of a screen of four numbered words to write by hand, shown with the page number (SK-25).
pub const WORDS_NEEDED_SK_25_WORDS: &str = "WORDS_NEEDED_SK_25_WORDS";
/// On that screen: Enter shows the next four, or goes to the check (SK-25).
pub const WORDS_NEEDED_SK_25_NEXT: &str = "WORDS_NEEDED_SK_25_NEXT";
/// On the words screen after a wrong answer: the check was wrong, shown with the first wrong position (SK-25, KM-16).
pub const WORDS_NEEDED_SK_25_WRONG: &str = "WORDS_NEEDED_SK_25_WRONG";
/// The title of the check screen: three positions are asked back (SK-25).
pub const WORDS_NEEDED_SK_25_CHECK: &str = "WORDS_NEEDED_SK_25_CHECK";
/// On the check screen, the label before the position asked (SK-25).
pub const WORDS_NEEDED_SK_25_ASK: &str = "WORDS_NEEDED_SK_25_ASK";
/// On the check screen: type the first four letters of that word and press Enter (SK-25).
pub const WORDS_NEEDED_SK_25_TYPE: &str = "WORDS_NEEDED_SK_25_TYPE";
/// The title of the public result screen (SK-26).
pub const WORDS_NEEDED_SK_26_RESULT: &str = "WORDS_NEEDED_SK_26_RESULT";
/// The label before the master fingerprint (SK-26).
pub const WORDS_NEEDED_SK_26_FINGERPRINT: &str = "WORDS_NEEDED_SK_26_FINGERPRINT";
/// The label before the first receive address (SK-26).
pub const WORDS_NEEDED_SK_26_ADDRESS: &str = "WORDS_NEEDED_SK_26_ADDRESS";
/// The label before the wallet's descriptor (SK-26).
pub const WORDS_NEEDED_SK_26_DESCRIPTOR: &str = "WORDS_NEEDED_SK_26_DESCRIPTOR";
/// The hint that Enter shows the descriptor as a QR code for the owner phone (SK-26).
pub const WORDS_NEEDED_SK_26_SHOW_QR: &str = "WORDS_NEEDED_SK_26_SHOW_QR";
/// On the QR screen: the hint that Enter returns to the text (SK-26).
pub const WORDS_NEEDED_SK_26_SHOW_TEXT: &str = "WORDS_NEEDED_SK_26_SHOW_TEXT";
/// The hint that S shows the words as a Compact SeedQR (SK-27).
pub const WORDS_NEEDED_SK_27_SEEDQR: &str = "WORDS_NEEDED_SK_27_SEEDQR";
/// On the SeedQR screen: the code is the key itself and must never be photographed (SK-27).
pub const WORDS_NEEDED_SK_27_WARNING: &str = "WORDS_NEEDED_SK_27_WARNING";
/// The title of the typed-words screen, shown with the word number asked (SK-28).
pub const WORDS_NEEDED_SK_28_TYPE: &str = "WORDS_NEEDED_SK_28_TYPE";
/// On that screen: the letters are no word of the list (SK-28).
pub const WORDS_NEEDED_SK_28_BAD_WORD: &str = "WORDS_NEEDED_SK_28_BAD_WORD";
/// On that screen: the 12 words fail the BIP39 checksum (SK-28).
pub const WORDS_NEEDED_SK_28_BAD_CHECKSUM: &str = "WORDS_NEEDED_SK_28_BAD_CHECKSUM";
/// On that screen: type a word, Enter keeps it, Backspace erases (SK-28).
pub const WORDS_NEEDED_SK_28_KEYS: &str = "WORDS_NEEDED_SK_28_KEYS";
/// The hint that Esc clears the program's memory of the words and turns the computer off (SK-29).
pub const WORDS_NEEDED_SK_29_TURN_OFF: &str = "WORDS_NEEDED_SK_29_TURN_OFF";

/// Every placeholder above, for the test that every one is shown by some screen.
pub const ALL: &[&str] = &[
    WORDS_NEEDED_SK_06_SECURE_BOOT,
    WORDS_NEEDED_SK_06_LOCKDOWN,
    WORDS_NEEDED_SK_12_INTERFACES,
    WORDS_NEEDED_SK_31_VERSION,
    WORDS_NEEDED_SK_31_INITRD_HASH,
    WORDS_NEEDED_SK_20_MENU_MIX,
    WORDS_NEEDED_SK_22_MENU_DICE,
    WORDS_NEEDED_SK_28_MENU_CHECK,
    WORDS_NEEDED_SK_21_ROLLS,
    WORDS_NEEDED_SK_21_ENOUGH,
    WORDS_NEEDED_SK_21_KEYS,
    WORDS_NEEDED_SK_22_ROLLS,
    WORDS_NEEDED_SK_22_VALUES,
    WORDS_NEEDED_SK_22_KEYS,
    WORDS_NEEDED_SK_25_WORDS,
    WORDS_NEEDED_SK_25_NEXT,
    WORDS_NEEDED_SK_25_WRONG,
    WORDS_NEEDED_SK_25_CHECK,
    WORDS_NEEDED_SK_25_ASK,
    WORDS_NEEDED_SK_25_TYPE,
    WORDS_NEEDED_SK_26_RESULT,
    WORDS_NEEDED_SK_26_FINGERPRINT,
    WORDS_NEEDED_SK_26_ADDRESS,
    WORDS_NEEDED_SK_26_DESCRIPTOR,
    WORDS_NEEDED_SK_26_SHOW_QR,
    WORDS_NEEDED_SK_26_SHOW_TEXT,
    WORDS_NEEDED_SK_27_SEEDQR,
    WORDS_NEEDED_SK_27_WARNING,
    WORDS_NEEDED_SK_28_TYPE,
    WORDS_NEEDED_SK_28_BAD_WORD,
    WORDS_NEEDED_SK_28_BAD_CHECKSUM,
    WORDS_NEEDED_SK_28_KEYS,
    WORDS_NEEDED_SK_29_TURN_OFF,
];
