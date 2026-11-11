//! A strict reader for the part of CBOR (RFC 8949) that the key exports of
//! signers use (UR-23): unsigned integers, byte and text strings, arrays
//! and maps with unsigned integer keys, tags, and the simple values false
//! and true. Everything else is refused, and so is anything not in the
//! shortest form.

use crate::cbor::read_head;

/// The most nested arrays and maps (UR-23).
pub(crate) const MAX_DEPTH: usize = 8;
/// The most tags in front of one item (UR-23).
pub(crate) const MAX_TAGS: usize = 8;
/// The most items in one message, counting every item at every depth. A
/// reader that keeps at most this many items cannot be made to hold more
/// than a few hundred kilobytes, whatever the length of the message.
pub(crate) const MAX_ITEMS: usize = 4096;

/// One item of the message. Strings borrow from the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item<'a> {
    Uint(u64),
    Bytes(&'a [u8]),
    Text(&'a str),
    Bool(bool),
    Array(Vec<Item<'a>>),
    /// Entries in strictly ascending order of key.
    Map(Vec<(u64, Item<'a>)>),
    Tag(u64, Box<Item<'a>>),
}

impl<'a> Item<'a> {
    /// The value under `key`, when this is a map that has it.
    pub(crate) fn get(&self, key: u64) -> Option<&Item<'a>> {
        match self {
            Self::Map(entries) => entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The unsigned integer, when this is one.
    pub(crate) fn uint(&self) -> Option<u64> {
        match self {
            Self::Uint(n) => Some(*n),
            _ => None,
        }
    }
}

/// Why a message is not in the subset. The reader gives no more than this
/// because a person is told one thing for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NotStrict(pub(crate) &'static str);

struct Reader<'a> {
    rest: &'a [u8],
    items: usize,
}

/// The one item that is all of `cbor`.
pub(crate) fn read(cbor: &[u8]) -> Result<Item<'_>, NotStrict> {
    let mut r = Reader {
        rest: cbor,
        items: 0,
    };
    let item = r.item(0)?;
    if !r.rest.is_empty() {
        return Err(NotStrict("bytes follow the item"));
    }
    Ok(item)
}

impl<'a> Reader<'a> {
    fn item(&mut self, depth: usize) -> Result<Item<'a>, NotStrict> {
        // Tags wrap one item each; they are walked here, not by recursion,
        // so that a long chain costs no stack.
        let mut tags: Vec<u64> = Vec::new();
        let inner = loop {
            self.items += 1;
            if self.items > MAX_ITEMS {
                return Err(NotStrict("too many items"));
            }
            let (major, arg, rest) = read_head(self.rest)
                .ok_or(NotStrict("a head is cut, indefinite or not shortest"))?;
            self.rest = rest;
            if major == 6 {
                if tags.len() >= MAX_TAGS {
                    return Err(NotStrict("too many tags in a row"));
                }
                tags.push(arg);
                continue;
            }
            break self.body(major, arg, depth)?;
        };
        Ok(tags
            .into_iter()
            .rev()
            .fold(inner, |item, tag| Item::Tag(tag, Box::new(item))))
    }

    fn take(&mut self, n: u64) -> Result<&'a [u8], NotStrict> {
        let n = usize::try_from(n).map_err(|_| NotStrict("a length does not fit"))?;
        let (head, rest) = self
            .rest
            .split_at_checked(n)
            .ok_or(NotStrict("a string is cut short"))?;
        self.rest = rest;
        Ok(head)
    }

    fn body(&mut self, major: u8, arg: u64, depth: usize) -> Result<Item<'a>, NotStrict> {
        match major {
            0 => Ok(Item::Uint(arg)),
            2 => Ok(Item::Bytes(self.take(arg)?)),
            3 => {
                let bytes = self.take(arg)?;
                std::str::from_utf8(bytes)
                    .map(Item::Text)
                    .map_err(|_| NotStrict("a text is not UTF-8"))
            }
            4 => {
                if depth >= MAX_DEPTH {
                    return Err(NotStrict("nested too deep"));
                }
                // Each item takes at least one byte, so a count above the
                // bytes left is a lie; checked before anything is kept.
                if arg > self.rest.len() as u64 {
                    return Err(NotStrict("an array is longer than the message"));
                }
                let mut out = Vec::new();
                for _ in 0..arg {
                    out.push(self.item(depth + 1)?);
                }
                Ok(Item::Array(out))
            }
            5 => {
                if depth >= MAX_DEPTH {
                    return Err(NotStrict("nested too deep"));
                }
                if arg.saturating_mul(2) > self.rest.len() as u64 {
                    return Err(NotStrict("a map is longer than the message"));
                }
                let mut out: Vec<(u64, Item<'a>)> = Vec::new();
                for _ in 0..arg {
                    let key = match self.item(depth + 1)? {
                        Item::Uint(k) => k,
                        _ => return Err(NotStrict("a map key is not an unsigned integer")),
                    };
                    if out.last().is_some_and(|(last, _)| *last >= key) {
                        return Err(NotStrict("map keys are not strictly ascending"));
                    }
                    out.push((key, self.item(depth + 1)?));
                }
                Ok(Item::Map(out))
            }
            7 => match arg {
                20 => Ok(Item::Bool(false)),
                21 => Ok(Item::Bool(true)),
                _ => Err(NotStrict("a simple value other than false and true")),
            },
            // Negative integers (1) and the rest.
            _ => Err(NotStrict("a type of item that is not read")),
        }
    }
}
