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
pub const SK_10_MENU_KEYS: &str = "Press 1, 2, 3 or 4.";
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

/// The first screen's fourth choice: sign a payment (SK-50).
pub const SK_50_MENU_SIGN: &str = "Sign a payment from the owner phone";
/// After "F" on the result screens: show the refund float's key (SK-32).
pub const SK_32_MENU_FLOAT: &str = "to show the key of the refund float";
/// After "L" on the result screens: show Shop Lightning's key (SK-32).
pub const SK_32_MENU_LIGHTNING: &str = "to show the key of Shop Lightning";
/// The label before the refund float's index, 0 to 99, on its screen (SK-32).
pub const SK_32_FLOAT_INDEX: &str = "Float number:";
/// After the keys + and - on the refund float's screen: they change the index (SK-32).
pub const SK_32_FLOAT_INDEX_KEYS: &str =
    "to change the number. Use the one the owner phone asks for.";
/// The refund float's screen: what the code is (SK-33).
pub const SK_33_FLOAT_WHAT: &str =
    "This code is the key of the refund float, made from your 12 words.";
/// The refund float's screen: it can spend the float's money and nothing else (SK-33).
pub const SK_33_FLOAT_SPENDS: &str = "It can spend the money in the float and nothing else.";
/// The refund float's screen: it is scanned once by the owner phone (SK-33).
pub const SK_33_FLOAT_SCAN: &str =
    "Scan it once with the owner phone, when it asks for the float's key.";
/// The refund float's screen: never photographed (SK-33).
pub const SK_33_FLOAT_NO_PHOTO: &str =
    "Never take a photo of this code. Anyone with one can spend the float.";
/// Shop Lightning's screen: what the code is (SK-33).
pub const SK_33_LIGHTNING_WHAT: &str =
    "This code is the key of Shop Lightning, made from your 12 words.";
/// Shop Lightning's screen: it can spend the node's money and nothing else (SK-33).
pub const SK_33_LIGHTNING_SPENDS: &str =
    "It can spend the money of Shop Lightning and nothing else.";
/// Shop Lightning's screen: it is scanned once by the Lightning counter (SK-33).
pub const SK_33_LIGHTNING_SCAN: &str =
    "Scan it once with the counter or Box that runs Shop Lightning, when it asks for its key.";
/// Shop Lightning's screen: never photographed (SK-33).
pub const SK_33_LIGHTNING_NO_PHOTO: &str =
    "Never take a photo of this code. Anyone with one can spend the Lightning money.";
/// After "N" on Shop Lightning's screen: show the node's ID (SK-35).
pub const SK_35_MENU_NODE_ID: &str = "WORDS_NEEDED_SK35_MENU_NODE_ID";
/// The node ID screen: what the ID is, and that it is public and spends
/// nothing (SK-35).
pub const SK_35_PUBLIC: &str = "WORDS_NEEDED_SK35_PUBLIC";
/// The node ID screen: the owner phone scans it to turn Shop Lightning on
/// (SK-35).
pub const SK_35_SCAN: &str = "WORDS_NEEDED_SK35_SCAN";
/// The node ID screen: the label before the first 8 characters of the ID
/// (SK-35).
pub const SK_35_FIRST: &str = "WORDS_NEEDED_SK35_FIRST";
/// The node ID screen: the label before the last 8 characters of the ID
/// (SK-35).
pub const SK_35_LAST: &str = "WORDS_NEEDED_SK35_LAST";
/// The title of the screen that reads the payment's QR codes (SK-52).
pub const SK_52_SCAN_TITLE: &str =
    "Hold the owner phone's payment code up to this computer's camera.";
/// On that screen while no camera is open yet (SK-52).
pub const SK_52_LOOKING: &str = "Looking for a camera.";
/// The label before the parts read out of the parts needed (SK-52).
pub const SK_52_PARTS: &str = "Parts read:";
/// After "Esc" on that screen: back to the first screen (SK-52).
pub const SK_52_BACK: &str = "to stop and go back to the start";
/// No camera was found in the set time (SK-52).
pub const SK_52_NO_CAMERA: &str =
    "No camera was found. Turn off, plug a camera in and start again.";
/// All parts were read and they are not a payment the stick takes (SK-52).
pub const SK_52_BAD_FILE: &str = "These codes are not a payment this stick can sign.";
/// Above the typed-words screen of a payment: the wallet's 12 words are needed (SK-53).
pub const SK_53_WORDS: &str = "To sign, type the 12 words of the wallet that pays.";
/// The title of a refused payment (SK-54).
pub const SK_54_REFUSED: &str = "This payment is not signed:";
/// The label before the number of the input a refusal names (SK-54).
pub const SK_54_INPUT: &str = "Coin number";
/// Refused: the payment is not for real bitcoin (SK-54).
pub const SK_54_NOT_MAINNET: &str = "It is for a test network, not real bitcoin.";
/// Refused: an input lacks its previous output (SK-54).
pub const SK_54_NO_PREVOUT: &str = "It does not say what one of its coins holds.";
/// Refused: an input's previous outputs disagree (SK-54).
pub const SK_54_PREVOUT_MISMATCH: &str = "It says two different things about one of its coins.";
/// Refused: an input lacks its key path (SK-54).
pub const SK_54_NO_PATH: &str = "It does not say which key of the wallet one of its coins needs.";
/// Refused: an input is not of the wallet of these words (SK-53, SK-54).
pub const SK_54_OTHER_WALLET: &str = "It spends a coin that is not in the wallet of these words.";
/// Refused: an input's key path is not of the wallet's two chains (SK-54).
pub const SK_54_PATH_NOT_WALLET: &str = "It asks for a key outside this wallet's addresses.";
/// Refused: the key at an input's path is not the input's key (SK-54).
pub const SK_54_KEY_MISMATCH: &str = "The key it asks for does not match the coin it spends.";
/// Refused: an input asks for a signature other than SIGHASH_ALL (SK-54).
pub const SK_54_NOT_SIGHASH_ALL: &str =
    "It asks for a kind of signature that would let the payment be changed afterward.";
/// Refused: the outputs pay more than the inputs hold (SK-54).
pub const SK_54_OUTPUTS_EXCEED: &str = "It pays out more than its coins hold.";
/// The title of the review before signing (SK-55).
pub const SK_55_TITLE: &str = "Check every address and amount before you sign.";
/// After an output the stick derived itself from these words (SK-55).
pub const SK_55_OWN: &str = "(change, back to your wallet)";
/// The label before the total sent away (SK-55).
pub const SK_55_SENT: &str = "Total sent away:";
/// The label before the fee (SK-55).
pub const SK_55_FEE: &str = "Fee:";
/// The label before the fee per virtual byte (SK-55).
pub const SK_55_FEE_RATE: &str = "Fee rate:";
/// After an amount: bitcoin (SK-55).
pub const SK_55_UNIT_BTC: &str = "BTC";
/// After the fee per virtual byte: satoshis per virtual byte (SK-55).
pub const SK_55_UNIT_RATE: &str = "sat/vB";
/// After "Up" and "Down" when the outputs fill more than one page: the page (SK-55).
pub const SK_55_PAGE: &str = "to see more. Page";
/// After "Enter" on the review: sign (SK-55).
pub const SK_55_SIGN: &str = "to sign it";
/// After "Esc" on the review: refuse, nothing is signed (SK-55).
pub const SK_55_REFUSE: &str = "to refuse. Nothing is signed.";
/// The title of the signed payment's QR screen: scan it back with the owner phone (SK-56).
pub const SK_56_SHOW: &str = "Signed. Scan this with the owner phone to send it.";
/// The label before the part shown out of the parts (SK-56).
pub const SK_56_PART: &str = "Part";

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
    SK_50_MENU_SIGN,
    SK_32_MENU_FLOAT,
    SK_32_MENU_LIGHTNING,
    SK_32_FLOAT_INDEX,
    SK_32_FLOAT_INDEX_KEYS,
    SK_33_FLOAT_WHAT,
    SK_33_FLOAT_SPENDS,
    SK_33_FLOAT_SCAN,
    SK_33_FLOAT_NO_PHOTO,
    SK_33_LIGHTNING_WHAT,
    SK_33_LIGHTNING_SPENDS,
    SK_33_LIGHTNING_SCAN,
    SK_33_LIGHTNING_NO_PHOTO,
    SK_35_MENU_NODE_ID,
    SK_35_PUBLIC,
    SK_35_SCAN,
    SK_35_FIRST,
    SK_35_LAST,
    SK_52_SCAN_TITLE,
    SK_52_LOOKING,
    SK_52_PARTS,
    SK_52_BACK,
    SK_52_NO_CAMERA,
    SK_52_BAD_FILE,
    SK_53_WORDS,
    SK_54_REFUSED,
    SK_54_INPUT,
    SK_54_NOT_MAINNET,
    SK_54_NO_PREVOUT,
    SK_54_PREVOUT_MISMATCH,
    SK_54_NO_PATH,
    SK_54_OTHER_WALLET,
    SK_54_PATH_NOT_WALLET,
    SK_54_KEY_MISMATCH,
    SK_54_NOT_SIGHASH_ALL,
    SK_54_OUTPUTS_EXCEED,
    SK_55_TITLE,
    SK_55_OWN,
    SK_55_SENT,
    SK_55_FEE,
    SK_55_FEE_RATE,
    SK_55_UNIT_BTC,
    SK_55_UNIT_RATE,
    SK_55_PAGE,
    SK_55_SIGN,
    SK_55_REFUSE,
    SK_56_SHOW,
    SK_56_PART,
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

/// Whether `text` is a placeholder for words the rules' owner has not yet
/// written: `WORDS`, `_NEEDED_`, the rule's name and the text's name. The
/// tag is built from pieces so that this file holds it only where a text
/// is still a placeholder.
pub fn is_placeholder(text: &str) -> bool {
    text.starts_with(concat!("WORDS", "_NEEDED_"))
}

/// Every text of this file that is still a placeholder.
pub fn placeholders() -> Vec<&'static str> {
    ALL.iter().copied().filter(|t| is_placeholder(t)).collect()
}
