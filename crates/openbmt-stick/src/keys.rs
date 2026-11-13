//! The keys the program reads (SK-14): the kernel's input events, mapped
//! from the key codes of `linux/input-event-codes.h` (a US layout; the
//! codes name the key, not the letter on it).

/// A key the screens use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A letter (lower case), a digit, or `+` or `-` (the main row's `=` and
    /// `-` keys and the keypad's; a shifted key is not told apart).
    Char(char),
    Enter,
    Backspace,
    Escape,
    Up,
    Down,
}

/// The event type of a key (`EV_KEY`).
pub const EV_KEY: u16 = 1;

/// The key a code means, if the program uses it.
pub fn key_from_code(code: u16) -> Option<Key> {
    const TOP: &[u8; 10] = b"qwertyuiop"; // codes 16 to 25
    const HOME: &[u8; 9] = b"asdfghjkl"; // codes 30 to 38
    const BOTTOM: &[u8; 7] = b"zxcvbnm"; // codes 44 to 50
    let c = match code {
        1 => return Some(Key::Escape),
        14 => return Some(Key::Backspace),
        28 | 96 => return Some(Key::Enter), // Enter and keypad Enter
        103 => return Some(Key::Up),
        108 => return Some(Key::Down),
        12 | 74 => '-', // the main row's minus and the keypad's
        13 | 78 => '+', // the main row's equals (plus, unshifted) and the keypad's
        2..=10 => char::from(b'1' + (code - 2) as u8), // 1 to 9
        11 => '0',
        16..=25 => char::from(TOP[usize::from(code - 16)]),
        30..=38 => char::from(HOME[usize::from(code - 30)]),
        44..=50 => char::from(BOTTOM[usize::from(code - 44)]),
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
        _ => return None,
    };
    Some(Key::Char(c))
}

/// Size of one input event on x86_64: a 16-byte time, the type, the code
/// and the value.
pub const EVENT_SIZE: usize = 24;

/// The key pressed (value 1) in one raw event, if it is one the program
/// uses. Releases and repeats give nothing.
pub fn key_from_event(raw: &[u8; EVENT_SIZE]) -> Option<Key> {
    let kind = u16::from_ne_bytes([raw[16], raw[17]]);
    let code = u16::from_ne_bytes([raw[18], raw[19]]);
    let value = i32::from_ne_bytes([raw[20], raw[21], raw[22], raw[23]]);
    if kind == EV_KEY && value == 1 {
        key_from_code(code)
    } else {
        None
    }
}
