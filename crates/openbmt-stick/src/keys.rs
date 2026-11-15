//! The keys the program reads (SK-14): the kernel's input events, mapped
//! from the key codes of `linux/input-event-codes.h` (a US layout; the
//! codes name the key, not the letter on it).

/// A key the screens use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable ASCII character as a US keyboard types it: lower case, or
    /// upper case and the upper signs with Shift (SK-62), a digit, a sign or
    /// the space. Only the passphrase screen takes more than lower-case
    /// letters, digits, `+` and `-`.
    Char(char),
    Enter,
    Backspace,
    Escape,
    Up,
    Down,
}

/// The event type of a key (`EV_KEY`).
pub const EV_KEY: u16 = 1;

/// The key codes of Shift (the left and the right one).
pub const SHIFT_CODES: [u16; 2] = [42, 54];

/// The signs of a US layout by key code: the unshifted and the shifted
/// character.
const SIGNS: [(u16, char, char); 14] = [
    (12, '-', '_'),
    (13, '=', '+'),
    (26, '[', '{'),
    (27, ']', '}'),
    (39, ';', ':'),
    (40, '\'', '"'),
    (41, '`', '~'),
    (43, '\\', '|'),
    (51, ',', '<'),
    (52, '.', '>'),
    (53, '/', '?'),
    (57, ' ', ' '),
    (74, '-', '-'), // the keypad's minus
    (78, '+', '+'), // the keypad's plus
];

/// The key a code means with Shift not held, if the program uses it.
pub fn key_from_code(code: u16) -> Option<Key> {
    key_from_code_shift(code, false)
}

/// The key a code means, with Shift held or not (a US layout; the codes name
/// the key, not the letter on it).
pub fn key_from_code_shift(code: u16, shift: bool) -> Option<Key> {
    const TOP: &[u8; 10] = b"qwertyuiop"; // codes 16 to 25
    const HOME: &[u8; 9] = b"asdfghjkl"; // codes 30 to 38
    const BOTTOM: &[u8; 7] = b"zxcvbnm"; // codes 44 to 50
    const UPPER_DIGITS: &[u8; 10] = b")!@#$%^&*("; // shifted 0, 1 to 9
    let letter = |b: u8| char::from(if shift { b.to_ascii_uppercase() } else { b });
    let c = match code {
        1 => return Some(Key::Escape),
        14 => return Some(Key::Backspace),
        28 | 96 => return Some(Key::Enter), // Enter and keypad Enter
        103 => return Some(Key::Up),
        108 => return Some(Key::Down),
        2..=10 if shift => char::from(UPPER_DIGITS[usize::from(code - 1)]), // 1 to 9
        2..=10 => char::from(b'1' + (code - 2) as u8),
        11 if shift => ')',
        11 => '0',
        16..=25 => letter(TOP[usize::from(code - 16)]),
        30..=38 => letter(HOME[usize::from(code - 30)]),
        44..=50 => letter(BOTTOM[usize::from(code - 44)]),
        // The keypad digits.
        79 => '1',
        80 => '2',
        81 => '3',
        75 => '4',
        76 => '5',
        77 => '6',
        71 => '7',
        72 => '8',
        73 => '9',
        82 => '0',
        _ => {
            let (_, plain, upper) = SIGNS.iter().find(|(k, _, _)| *k == code)?;
            if shift {
                *upper
            } else {
                *plain
            }
        }
    };
    Some(Key::Char(c))
}

/// Turns raw input events into keys, remembering whether Shift is held.
#[derive(Debug, Default, Clone, Copy)]
pub struct Decoder {
    shift: bool,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The key pressed in one raw event, if it is one the program uses.
    /// Shift's own press and release only change what the next keys mean.
    pub fn event(&mut self, raw: &[u8; EVENT_SIZE]) -> Option<Key> {
        let kind = u16::from_ne_bytes([raw[16], raw[17]]);
        let code = u16::from_ne_bytes([raw[18], raw[19]]);
        let value = i32::from_ne_bytes([raw[20], raw[21], raw[22], raw[23]]);
        if kind != EV_KEY {
            return None;
        }
        if SHIFT_CODES.contains(&code) {
            self.shift = value != 0;
            return None;
        }
        if value == 1 {
            key_from_code_shift(code, self.shift)
        } else {
            None
        }
    }
}

/// Size of one input event on x86_64: a 16-byte time, the type, the code
/// and the value.
pub const EVENT_SIZE: usize = 24;

/// The key pressed (value 1) in one raw event, if it is one the program
/// uses, with Shift taken as not held. Releases and repeats give nothing.
pub fn key_from_event(raw: &[u8; EVENT_SIZE]) -> Option<Key> {
    Decoder::new().event(raw)
}
