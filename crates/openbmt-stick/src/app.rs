//! The screens and what the keys do (SK-25 to SK-29). The state machine has
//! no I/O: keys come in, a [`Screen`] comes out, and the kernel's random
//! bytes come from a [`Chance`] the caller gives.

use openbmt_qr::{Ecc, QrCode};

use crate::check::{answer_matches, pick_positions, ASKED};
use crate::keys::Key;
use crate::mix::{
    dice_alone, dice_values, mix, DICE_VALUES, ENOUGH_ROLLS, ENTROPY_BYTES, KERNEL_BYTES,
};
use crate::outputs::{public_of, words_of, Public};
use crate::screen::Screen;
use crate::seedqr::compact_seedqr;
use crate::typed::{check_words, complete_word, TypedError, WORD_COUNT};
use crate::words::*;

/// What the program tells the owner on the first screen (SK-06, SK-12,
/// SK-31). Every value is text read from the machine; the labels are the
/// words constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The firmware's `SecureBoot` value (`1` or `0`, `?` if not readable).
    pub secure_boot: String,
    /// The kernel's lockdown mode (`none`, `integrity`, `confidentiality`,
    /// `?` if not readable).
    pub lockdown: String,
    /// Every interface the kernel has, by name. All are left down.
    pub interfaces: Vec<String>,
    /// The program's own version.
    pub version: String,
    /// The SHA-256 of `initrd.img` as the kernel loaded it, in hex (`?` if
    /// not readable).
    pub initrd_sha256: String,
}

/// The source of the kernel's random bytes (SK-20).
pub trait Chance {
    /// 32 bytes from the kernel's generator, once it is ready.
    fn kernel_bytes(&mut self) -> [u8; KERNEL_BYTES];
}

/// What the caller does after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    /// SK-29: the program has cleared what it holds; power the computer off.
    TurnOff,
}

/// The longest answer or word typed.
const MAX_TYPED: usize = 8;
/// Words shown on one screen (SK-25).
const PER_PAGE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Text,
    Qr,
    SeedQr,
}

#[derive(Debug)]
enum State {
    Start {
        selected: usize,
    },
    Mix {
        rolls: Vec<u8>,
    },
    Dice {
        rolls: Vec<u8>,
    },
    Type {
        words: Vec<&'static str>,
        buf: String,
        error: Option<&'static str>,
    },
    Words {
        page: usize,
        wrong: Option<usize>,
    },
    Check {
        answers: Vec<String>,
        buf: String,
    },
    Result {
        view: View,
    },
}

/// The whole program.
pub struct App<C: Chance> {
    facts: Facts,
    chance: C,
    state: State,
    entropy: Option<[u8; ENTROPY_BYTES]>,
    positions: [usize; ASKED],
    public: Option<Public>,
    descriptor_qr: Option<QrCode>,
    seed_qr: Option<QrCode>,
}

/// Overwrites bytes with zeros, in a way the compiler may not drop.
fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        *b = 0;
    }
    std::hint::black_box(&*bytes);
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

impl<C: Chance> App<C> {
    pub fn new(facts: Facts, chance: C) -> Self {
        Self {
            facts,
            chance,
            state: State::Start { selected: 0 },
            entropy: None,
            positions: [0; ASKED],
            public: None,
            descriptor_qr: None,
            seed_qr: None,
        }
    }

    /// SK-29: clears what the program holds of the key.
    fn clear(&mut self) {
        if let Some(e) = self.entropy.as_mut() {
            wipe(e);
        }
        self.entropy = None;
        match &mut self.state {
            State::Mix { rolls } | State::Dice { rolls } => wipe(rolls),
            State::Type { words, buf, .. } => {
                words.clear();
                buf.clear();
            }
            State::Check { answers, buf } => {
                answers.clear();
                buf.clear();
            }
            _ => {}
        }
        self.positions = [0; ASKED];
        self.public = None;
        self.descriptor_qr = None;
        self.seed_qr = None;
        self.state = State::Start { selected: 0 };
    }

    /// The screen for the state now.
    pub fn screen(&self) -> Screen {
        match &self.state {
            State::Start { selected } => self.start_screen(*selected),
            State::Mix { rolls } => Screen {
                id: "mix",
                lines: vec![
                    format!("{WORDS_NEEDED_SK_21_ROLLS} {}", rolls.len()),
                    WORDS_NEEDED_SK_21_ENOUGH.to_string(),
                    String::new(),
                    digits_of(rolls),
                    String::new(),
                    WORDS_NEEDED_SK_21_KEYS.to_string(),
                ],
                qr: None,
            },
            State::Dice { rolls } => {
                let values = dice_values(rolls).map(|v| v.len()).unwrap_or(0);
                Screen {
                    id: "dice",
                    lines: vec![
                        format!("{WORDS_NEEDED_SK_22_ROLLS} {}", rolls.len()),
                        format!("{WORDS_NEEDED_SK_22_VALUES} {values} {DICE_VALUES}"),
                        String::new(),
                        digits_of(rolls),
                        String::new(),
                        WORDS_NEEDED_SK_22_KEYS.to_string(),
                    ],
                    qr: None,
                }
            }
            State::Type { words, buf, error } => {
                let mut lines = vec![format!("{WORDS_NEEDED_SK_28_TYPE} {}", words.len() + 1)];
                if let Some(e) = error {
                    lines.push((*e).to_string());
                }
                lines.push(format!("> {buf}"));
                lines.push(String::new());
                lines.push(WORDS_NEEDED_SK_28_KEYS.to_string());
                Screen {
                    id: "type",
                    lines,
                    qr: None,
                }
            }
            State::Words { page, wrong } => {
                let mut lines = Vec::new();
                if let Some(p) = wrong {
                    lines.push(format!("{WORDS_NEEDED_SK_25_WRONG} {}", p + 1));
                }
                lines.push(format!("{WORDS_NEEDED_SK_25_WORDS} {}", page + 1));
                if let Some(words) = self.entropy.as_ref().and_then(|e| words_of(e).ok()) {
                    for (i, w) in words
                        .iter()
                        .enumerate()
                        .skip(page * PER_PAGE)
                        .take(PER_PAGE)
                    {
                        lines.push(format!("{} {w}", i + 1));
                    }
                }
                lines.push(String::new());
                lines.push(WORDS_NEEDED_SK_25_NEXT.to_string());
                Screen {
                    id: "words",
                    lines,
                    qr: None,
                }
            }
            State::Check { answers, buf } => Screen {
                id: "check",
                lines: vec![
                    WORDS_NEEDED_SK_25_CHECK.to_string(),
                    format!(
                        "{WORDS_NEEDED_SK_25_ASK} {}",
                        self.positions[answers.len()] + 1
                    ),
                    format!("> {buf}"),
                    String::new(),
                    WORDS_NEEDED_SK_25_TYPE.to_string(),
                ],
                qr: None,
            },
            State::Result { view } => self.result_screen(*view),
        }
    }

    fn start_screen(&self, selected: usize) -> Screen {
        let f = &self.facts;
        let mark = |i: usize| if i == selected { ">" } else { " " };
        let nets = if f.interfaces.is_empty() {
            "-".to_string()
        } else {
            f.interfaces.join(" ")
        };
        Screen {
            id: "start",
            lines: vec![
                format!("{WORDS_NEEDED_SK_06_SECURE_BOOT} {}", f.secure_boot),
                format!("{WORDS_NEEDED_SK_06_LOCKDOWN} {}", f.lockdown),
                format!("{WORDS_NEEDED_SK_12_INTERFACES} {nets}"),
                format!("{WORDS_NEEDED_SK_31_VERSION} {}", f.version),
                format!("{WORDS_NEEDED_SK_31_INITRD_HASH} {}", f.initrd_sha256),
                String::new(),
                format!("{} 1 {WORDS_NEEDED_SK_20_MENU_MIX}", mark(0)),
                format!("{} 2 {WORDS_NEEDED_SK_22_MENU_DICE}", mark(1)),
                format!("{} 3 {WORDS_NEEDED_SK_28_MENU_CHECK}", mark(2)),
                format!("Esc {WORDS_NEEDED_SK_29_TURN_OFF}"),
            ],
            qr: None,
        }
    }

    fn result_screen(&self, view: View) -> Screen {
        let off = format!("Esc {WORDS_NEEDED_SK_29_TURN_OFF}");
        match (view, &self.public) {
            (View::Text, Some(p)) => Screen {
                id: "result",
                lines: vec![
                    WORDS_NEEDED_SK_26_RESULT.to_string(),
                    format!("{WORDS_NEEDED_SK_26_FINGERPRINT} {}", p.fingerprint),
                    format!("{WORDS_NEEDED_SK_26_ADDRESS} {}", p.address),
                    format!("{WORDS_NEEDED_SK_26_DESCRIPTOR} {}", p.descriptor),
                    format!("Enter {WORDS_NEEDED_SK_26_SHOW_QR}"),
                    format!("S {WORDS_NEEDED_SK_27_SEEDQR}"),
                    off,
                ],
                qr: None,
            },
            (View::Qr, Some(_)) => Screen {
                id: "result-qr",
                lines: vec![
                    WORDS_NEEDED_SK_26_DESCRIPTOR.to_string(),
                    format!("Enter {WORDS_NEEDED_SK_26_SHOW_TEXT}"),
                    off,
                ],
                qr: self.descriptor_qr.clone(),
            },
            (View::SeedQr, Some(_)) => Screen {
                id: "seedqr",
                lines: vec![
                    WORDS_NEEDED_SK_27_WARNING.to_string(),
                    format!("Enter {WORDS_NEEDED_SK_26_SHOW_TEXT}"),
                    off,
                ],
                qr: self.seed_qr.clone(),
            },
            (_, None) => Screen {
                id: "result",
                lines: vec![off],
                qr: None,
            },
        }
    }

    /// One key. Returns what to do next.
    pub fn key(&mut self, key: Key) -> Outcome {
        let state = std::mem::replace(&mut self.state, State::Start { selected: 0 });
        self.state = match state {
            State::Start { selected } => match key {
                Key::Escape => {
                    self.clear();
                    return Outcome::TurnOff;
                }
                Key::Up => State::Start {
                    selected: (selected + 2) % 3,
                },
                Key::Down => State::Start {
                    selected: (selected + 1) % 3,
                },
                Key::Char(c @ '1'..='3') => self.start_choice(usize::from(c as u8 - b'1')),
                Key::Enter => self.start_choice(selected),
                _ => State::Start { selected },
            },
            State::Mix { mut rolls } => match key {
                Key::Char(c @ '1'..='6') => {
                    rolls.push(c as u8 - b'0');
                    State::Mix { rolls }
                }
                Key::Backspace => {
                    rolls.pop();
                    State::Mix { rolls }
                }
                Key::Escape => {
                    wipe(&mut rolls);
                    State::Start { selected: 0 }
                }
                Key::Enter => {
                    let mut kernel = self.chance.kernel_bytes();
                    let made = mix(&kernel, &rolls);
                    wipe(&mut kernel);
                    wipe(&mut rolls);
                    match made {
                        Ok(entropy) => self.begin_words(entropy),
                        Err(_) => State::Start { selected: 0 },
                    }
                }
                _ => State::Mix { rolls },
            },
            State::Dice { mut rolls } => {
                let kept = dice_values(&rolls).map(|v| v.len()).unwrap_or(0);
                match key {
                    // Once 50 values are kept no roll can change them.
                    Key::Char(c @ '1'..='6') if kept < DICE_VALUES => {
                        rolls.push(c as u8 - b'0');
                        State::Dice { rolls }
                    }
                    Key::Backspace => {
                        rolls.pop();
                        State::Dice { rolls }
                    }
                    Key::Escape => {
                        wipe(&mut rolls);
                        State::Start { selected: 0 }
                    }
                    Key::Enter if kept == DICE_VALUES => {
                        let made = dice_dust(&rolls);
                        wipe(&mut rolls);
                        match made {
                            Some(entropy) => self.begin_words(entropy),
                            None => State::Start { selected: 0 },
                        }
                    }
                    _ => State::Dice { rolls },
                }
            }
            State::Type { words, buf, error } => self.type_key(key, words, buf, error),
            State::Words { page, wrong } => match key {
                Key::Enter if (page + 1) * PER_PAGE >= WORD_COUNT => State::Check {
                    answers: Vec::new(),
                    buf: String::new(),
                },
                Key::Enter => State::Words {
                    page: page + 1,
                    wrong,
                },
                Key::Backspace if page > 0 => State::Words {
                    page: page - 1,
                    wrong,
                },
                _ => State::Words { page, wrong },
            },
            State::Check {
                mut answers,
                mut buf,
            } => match key {
                Key::Char(c) if c.is_ascii_lowercase() && buf.len() < MAX_TYPED => {
                    buf.push(c);
                    State::Check { answers, buf }
                }
                Key::Backspace => {
                    buf.pop();
                    State::Check { answers, buf }
                }
                Key::Enter if !buf.is_empty() => {
                    answers.push(std::mem::take(&mut buf));
                    if answers.len() < ASKED {
                        State::Check { answers, buf }
                    } else {
                        self.judge(&answers)
                    }
                }
                _ => State::Check { answers, buf },
            },
            State::Result { view } => match key {
                Key::Escape => {
                    self.clear();
                    return Outcome::TurnOff;
                }
                Key::Enter => State::Result {
                    view: if view == View::Text {
                        View::Qr
                    } else {
                        View::Text
                    },
                },
                Key::Char('s') => State::Result { view: View::SeedQr },
                _ => State::Result { view },
            },
        };
        Outcome::Continue
    }

    fn start_choice(&mut self, i: usize) -> State {
        match i {
            0 => State::Mix { rolls: Vec::new() },
            1 => State::Dice { rolls: Vec::new() },
            _ => State::Type {
                words: Vec::new(),
                buf: String::new(),
                error: None,
            },
        }
    }

    /// SK-25: the words are made; three positions are picked from fresh
    /// kernel bytes.
    fn begin_words(&mut self, entropy: [u8; ENTROPY_BYTES]) -> State {
        let mut bytes = self.chance.kernel_bytes();
        self.positions = pick_positions(&bytes);
        wipe(&mut bytes);
        self.entropy = Some(entropy);
        if self.make_public().is_err() {
            self.clear();
            return State::Start { selected: 0 };
        }
        State::Words {
            page: 0,
            wrong: None,
        }
    }

    /// SK-26 and SK-27: the public result and both codes, from the entropy.
    fn make_public(&mut self) -> Result<(), ()> {
        let entropy = self.entropy.ok_or(())?;
        let words = words_of(&entropy).map_err(|_| ())?;
        let public = public_of(&words).map_err(|_| ())?;
        self.descriptor_qr =
            Some(openbmt_qr::encode_text(&public.descriptor, Ecc::M).map_err(|_| ())?);
        self.seed_qr = Some(compact_seedqr(&entropy).map_err(|_| ())?);
        self.public = Some(public);
        Ok(())
    }

    /// SK-25: right answers go on to the result, a wrong one shows the words
    /// again with the first wrong position named.
    fn judge(&mut self, answers: &[String]) -> State {
        let words = match self.entropy.as_ref().and_then(|e| words_of(e).ok()) {
            Some(w) => w,
            None => return State::Start { selected: 0 },
        };
        let first_wrong = self
            .positions
            .iter()
            .zip(answers)
            .find(|(p, a)| !answer_matches(words[**p], a))
            .map(|(p, _)| *p);
        match first_wrong {
            Some(p) => State::Words {
                page: 0,
                wrong: Some(p),
            },
            None => State::Result { view: View::Text },
        }
    }

    fn type_key(
        &mut self,
        key: Key,
        mut words: Vec<&'static str>,
        mut buf: String,
        mut error: Option<&'static str>,
    ) -> State {
        match key {
            Key::Escape => {
                words.clear();
                return State::Start { selected: 0 };
            }
            Key::Char(c) if c.is_ascii_lowercase() && buf.len() < MAX_TYPED => {
                buf.push(c);
                error = None;
            }
            Key::Backspace => {
                error = None;
                if buf.pop().is_none() {
                    if let Some(w) = words.pop() {
                        buf.push_str(w);
                    }
                }
            }
            Key::Enter => match complete_word(&buf) {
                None => error = Some(WORDS_NEEDED_SK_28_BAD_WORD),
                Some(w) => {
                    words.push(w);
                    buf.clear();
                    error = None;
                    if words.len() == WORD_COUNT {
                        match check_words(&words) {
                            Ok(entropy) => {
                                words.clear();
                                self.entropy = Some(entropy);
                                return match self.make_public() {
                                    Ok(()) => State::Result { view: View::Text },
                                    Err(()) => {
                                        self.clear();
                                        State::Start { selected: 0 }
                                    }
                                };
                            }
                            Err(e) => {
                                // Back to the last word, to be corrected.
                                words.pop();
                                buf.push_str(w);
                                error = Some(match e {
                                    TypedError::BadChecksum => WORDS_NEEDED_SK_28_BAD_CHECKSUM,
                                    _ => WORDS_NEEDED_SK_28_BAD_WORD,
                                });
                            }
                        }
                    }
                }
            },
            _ => {}
        }
        State::Type { words, buf, error }
    }
}

fn dice_dust(rolls: &[u8]) -> Option<[u8; ENTROPY_BYTES]> {
    let mut values = dice_values(rolls).ok()?;
    let out = dice_alone(&values).ok();
    wipe(&mut values);
    out
}

/// The rolls as digits, for the owner to see what they entered.
fn digits_of(rolls: &[u8]) -> String {
    let mut s: String = rolls.iter().map(|r| char::from(b'0' + r)).collect();
    if s.len() > ENOUGH_ROLLS * 4 {
        s = s.split_off(s.len() - ENOUGH_ROLLS * 4);
    }
    s
}
