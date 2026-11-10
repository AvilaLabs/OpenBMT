//! What a person is told (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! Every state and every refusal a person can meet is turned into one
//! [`Words`]: a heading, what happened, what to do next and who does it.
//! The texts with rule numbers that the `Display` impls give are for logs
//! and tests; a person is shown [`Words`]. [`check`] is the one test every
//! app runs over all of its words.

/// How a screen is coloured (CR-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Nothing is wrong and nothing is finished (blue).
    Info,
    /// It is done; at a counter, the goods may be handed over (green).
    Good,
    /// Stop; at a counter, do not hand over the goods (red).
    Stop,
    /// It ended and nothing came of it (neutral).
    Ended,
}

/// Who does the next step (SC-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    /// Nobody: it goes on by itself.
    Nobody,
    /// The customer.
    Customer,
    /// The person at the counter.
    Cashier,
    /// A manager at the counter.
    Manager,
    /// The owner, on the owner's phone.
    Owner,
    /// The person who holds the Key app.
    KeyHolder,
    /// The person at the Box.
    BoxKeeper,
}

/// Text that came from another device or from a scanned code (SC-08). It
/// is shown in quotation marks under the name of where it came from, never
/// as part of the app's own sentences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quoted {
    /// Where it came from, in the app's own words ("The cashier wrote").
    pub from: &'static str,
    /// The text, cleaned by [`clean_foreign`].
    pub text: String,
}

impl Quoted {
    /// Text from elsewhere, cleaned.
    pub fn new(from: &'static str, text: &str) -> Self {
        Self {
            from,
            text: clean_foreign(text),
        }
    }
}

/// What a person is told about one state or one refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    /// The colour.
    pub tone: Tone,
    /// A few words that name the state.
    pub heading: String,
    /// One sentence: what happened or what is so.
    pub what: String,
    /// One sentence: what to do next, or that there is nothing to do.
    pub next: String,
    /// Who does it.
    pub who: Who,
    /// The rule behind it, for the small print ("CR-12"). Never in the
    /// sentences.
    pub code: Option<&'static str>,
    /// A time the app shows in the person's own clock, in seconds since
    /// 1970. Never written into the sentences.
    pub at: Option<u64>,
    /// Text from another device, if the state carries any.
    pub quoted: Option<Quoted>,
    /// The program's own detail, for the small print only: what the
    /// system said, a number of a record. Never needed to act.
    pub detail: Option<String>,
}

impl Words {
    /// Words with no rule, time, quoted text or detail.
    pub fn new(
        tone: Tone,
        heading: impl Into<String>,
        what: impl Into<String>,
        who: Who,
        next: impl Into<String>,
    ) -> Self {
        Self {
            tone,
            heading: heading.into(),
            what: what.into(),
            next: next.into(),
            who,
            code: None,
            at: None,
            quoted: None,
            detail: None,
        }
    }

    /// With the rule behind it.
    #[must_use]
    pub fn code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    /// With a time for the app to show.
    #[must_use]
    pub fn at(mut self, at: u64) -> Self {
        self.at = Some(at);
        self
    }

    /// With text from another device.
    #[must_use]
    pub fn quoted(mut self, quoted: Quoted) -> Self {
        self.quoted = Some(quoted);
        self
    }

    /// With detail for the small print.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(clip(&detail.into(), MAX_DETAIL));
        self
    }
}

/// The longest heading, in characters.
pub const MAX_HEADING: usize = 28;
/// The longest sentence, in characters.
pub const MAX_SENTENCE: usize = 140;
/// The longest quoted text, in characters.
pub const MAX_QUOTED: usize = 64;
/// The longest detail, in characters.
pub const MAX_DETAIL: usize = 200;

/// Words a person is not expected to know (SC-04). Compared in lower
/// case.
pub const JARGON: &[&str] = &[
    "change mark",
    "receive mark",
    "dust",
    "slice",
    "exposure",
    "sighash",
    "p2wsh",
    "p2wpkh",
    "p2tr",
    "p2pkh",
    "p2sh",
    "witness",
    "outpoint",
    "psbt",
    "xpub",
    "descriptor",
    "mempool",
    "utxo",
    "nonce",
    "hash",
    "unix",
    "internal",
    "panic",
    "decode",
];

fn clip(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// Whether a character changes the direction of text or cannot be seen.
fn is_hidden(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}'
    )
}

/// SC-08: text from another device made safe to show. Control characters
/// and line ends become spaces, characters that hide or turn text around
/// are dropped, runs of spaces become one, and at most [`MAX_QUOTED`]
/// characters are kept.
pub fn clean_foreign(text: &str) -> String {
    let mut out = String::new();
    let mut space = true;
    for c in text.chars() {
        if is_hidden(c) {
            continue;
        }
        let c = if c.is_control() || c.is_whitespace() {
            ' '
        } else {
            c
        };
        if c == ' ' {
            if space {
                continue;
            }
            space = true;
        } else {
            space = false;
        }
        out.push(c);
        if out.chars().count() >= MAX_QUOTED {
            break;
        }
    }
    out.trim_end().to_string()
}

/// An amount of satoshis for a sentence: "12,345 sats".
pub fn sats(amount: u64) -> String {
    let digits = amount.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out.push_str(if amount == 1 { " sat" } else { " sats" });
    out
}

/// Whether `text` holds a rule number such as "CR-12" (SC-03).
fn has_rule_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(4).any(|w| {
        w[0].is_ascii_uppercase()
            && w[1].is_ascii_uppercase()
            && w[2] == b'-'
            && w[3].is_ascii_digit()
    })
}

/// Whether `text` holds 12 or more hex digits in a row with a letter and
/// a digit among them: a key, a hash or a transaction id (SC-03).
fn has_long_hex(text: &str) -> bool {
    let mut run = 0usize;
    let (mut digit, mut letter) = (false, false);
    for c in text.chars() {
        if c.is_ascii_hexdigit() {
            run += 1;
            digit |= c.is_ascii_digit();
            letter |= c.is_ascii_alphabetic();
            if run >= 12 && digit && letter {
                return true;
            }
        } else {
            run = 0;
            digit = false;
            letter = false;
        }
    }
    false
}

/// Whether `text` holds a number of 9 or more digits with no separator:
/// a time in seconds or an amount nobody can read (SC-03, SC-09).
fn has_raw_number(text: &str) -> bool {
    let mut run = 0usize;
    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
            if run >= 9 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn check_sentence(name: &'static str, text: &str) -> Result<(), String> {
    let fail = |why: &str| Err(format!("{name} {why}: {text:?}"));
    let count = text.chars().count();
    if count == 0 {
        return fail("is empty");
    }
    if count > MAX_SENTENCE {
        return fail("is too long");
    }
    if !text.ends_with('.') && !text.ends_with('?') {
        return fail("does not end with a full stop");
    }
    if text.chars().next().is_some_and(char::is_lowercase) {
        return fail("starts with a small letter");
    }
    check_plain(name, text)
}

fn check_plain(name: &'static str, text: &str) -> Result<(), String> {
    let fail = |why: &str| Err(format!("{name} {why}: {text:?}"));
    if text.chars().any(|c| c.is_control() || is_hidden(c)) {
        return fail("holds a control character");
    }
    if has_rule_number(text) {
        return fail("holds a rule number");
    }
    if has_long_hex(text) {
        return fail("holds a key or an id");
    }
    if has_raw_number(text) {
        return fail("holds a number nobody can read");
    }
    // A bracket straight after a letter is a call or a debug form.
    let call = text
        .char_indices()
        .any(|(i, c)| c == '(' && text[..i].chars().last().is_some_and(char::is_alphanumeric));
    if call || ["_", "::", "{", "}", "0x"].iter().any(|s| text.contains(s)) {
        return fail("holds the program's own notation");
    }
    let lower = text.to_lowercase();
    if let Some(word) = JARGON.iter().find(|w| lower.contains(**w)) {
        return Err(format!("{name} uses the word {word:?}: {text:?}"));
    }
    Ok(())
}

/// SC-12: whether these words keep the rules a program can check. The
/// error says which rule is broken and quotes the text.
pub fn check(words: &Words) -> Result<(), String> {
    let heading = &words.heading;
    let count = heading.chars().count();
    if count == 0 || count > MAX_HEADING {
        return Err(format!("the heading is empty or too long: {heading:?}"));
    }
    if heading.ends_with('.') {
        return Err(format!("the heading ends with a full stop: {heading:?}"));
    }
    check_plain("the heading", heading)?;
    check_sentence("what happened", &words.what)?;
    check_sentence("what to do next", &words.next)?;
    if words.what == words.next {
        return Err(format!("both sentences are the same: {:?}", words.what));
    }
    if let Some(code) = words.code {
        if !has_rule_number(code) || code.len() > 8 {
            return Err(format!("the code is not a rule number: {code:?}"));
        }
    }
    if let Some(q) = &words.quoted {
        if q.text != clean_foreign(&q.text) {
            return Err(format!("the quoted text is not cleaned: {:?}", q.text));
        }
        check_plain("where the quoted text is from", q.from)?;
    }
    if let Some(d) = &words.detail {
        if d.chars().count() > MAX_DETAIL || d.chars().any(char::is_control) {
            return Err(format!("the detail is too long or not plain: {d:?}"));
        }
    }
    Ok(())
}

/// SC-01, SC-07: whether every one of these words can be told from the
/// others. Two states of one app must not share a heading and a first
/// sentence. `all` pairs a name for the report with the words.
pub fn check_distinct(all: &[(String, Words)]) -> Result<(), String> {
    for (i, (name, words)) in all.iter().enumerate() {
        check(words).map_err(|e| format!("{name}: {e}"))?;
        for (other, theirs) in all.iter().skip(i + 1) {
            if words.heading == theirs.heading && words.what == theirs.what {
                return Err(format!(
                    "{name} and {other} say the same: {:?} / {:?}",
                    words.heading, words.what
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Words {
        Words::new(
            Tone::Info,
            "Waiting for payment",
            "Nothing has arrived yet.",
            Who::Customer,
            "The customer scans the code and pays.",
        )
    }

    #[test]
    fn sc_12_plain_words_pass() {
        assert_eq!(check(&good().code("CR-10").at(1_700_000_000)), Ok(()));
    }

    #[test]
    fn sc_03_program_notation_is_refused() {
        let cases = [
            "CR-12: a conflicting payment was seen.",
            "The payment 4a5e1e4baab89f3a32518a88c31bc87f was replaced.",
            "The rate was fetched at 1700000000.",
            "Call create_from_entropy first.",
            "The key is DeviceId(ab12).",
            "It failed with 0x1f.",
        ];
        for text in cases {
            let mut w = good();
            w.what = text.to_string();
            assert!(check(&w).is_err(), "{text}");
        }
    }

    #[test]
    fn sc_04_jargon_is_refused() {
        for word in JARGON {
            let mut w = good();
            w.next = format!("Look at the {word} again.");
            assert!(check(&w).is_err(), "{word}");
        }
    }

    #[test]
    fn sc_02_both_sentences_are_needed_and_end_properly() {
        let mut w = good();
        w.next = String::new();
        assert!(check(&w).is_err());
        let mut w = good();
        w.next = "wait".to_string();
        assert!(check(&w).is_err());
        let mut w = good();
        w.next = w.what.clone();
        assert!(check(&w).is_err());
        let mut w = good();
        w.heading = "A heading that goes on for far too long".to_string();
        assert!(check(&w).is_err());
    }

    #[test]
    fn sc_08_text_from_elsewhere_is_cleaned() {
        let q = Quoted::new("The cashier wrote", "re\u{202e}fund\n\n  for\tAnn\u{0}");
        assert_eq!(q.text, "refund for Ann");
        let long = "x".repeat(500);
        assert_eq!(clean_foreign(&long).chars().count(), MAX_QUOTED);
        assert_eq!(check(&good().quoted(q)), Ok(()));
        let mut w = good();
        w.quoted = Some(Quoted {
            from: "The owner wrote",
            text: "two\nlines".to_string(),
        });
        assert!(check(&w).is_err());
    }

    #[test]
    fn sc_09_amounts_are_written_to_be_read() {
        assert_eq!(sats(0), "0 sats");
        assert_eq!(sats(1), "1 sat");
        assert_eq!(sats(999), "999 sats");
        assert_eq!(sats(1_000), "1,000 sats");
        assert_eq!(sats(2_100_000_000_000_000), "2,100,000,000,000,000 sats");
        let mut w = good();
        w.what = format!("The sale is short by {}.", sats(123_456_789_012));
        assert_eq!(check(&w), Ok(()));
    }

    #[test]
    fn sc_07_two_states_that_say_the_same_are_found() {
        let a = ("a".to_string(), good());
        let mut other = good();
        other.next = "Something else is done.".to_string();
        let b = ("b".to_string(), other);
        assert!(check_distinct(&[a.clone(), b]).is_err());
        let mut third = good();
        third.what = "Part of the amount has arrived.".to_string();
        assert_eq!(check_distinct(&[a, ("c".to_string(), third)]), Ok(()));
    }
}
