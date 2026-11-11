//! Bytewords (BCR-2020-012), minimal style: each byte is the first and the
//! last letter of its word, and the CRC-32 of the bytes follows them
//! (big-endian) as four more. Upper and lower case are both read.

use crate::error::UrError;
use crate::hash::crc32;

/// The 256 words, four letters each, in alphabetical order (the index of a
/// word is the byte it stands for).
const WORDS: &str = "ableacidalsoapexaquaarchatomauntawayaxisbackbaldbarnbeltbetabiasbluebodybragbrewbulbbuzzcalmcashcatschefcityclawcodecolacookcostcruxcurlcuspcyandarkdatadaysdelidicedietdoordowndrawdropdrumdulldutyeacheasyechoedgeepicevenexamexiteyesfactfairfernfigsfilmfishfizzflapflewfluxfoxyfreefrogfuelfundgalagamegeargemsgiftgirlglowgoodgraygrimgurugushgyrohalfhanghardhawkheathelphighhillholyhopehornhutsicedideaidleinchinkyintoirisironitemjadejazzjoinjoltjowljudojugsjumpjunkjurykeepkenokeptkeyskickkilnkingkitekiwiknoblamblavalazyleaflegsliarlimplionlistlogoloudloveluaulucklungmainmanymathmazememomenumeowmildmintmissmonknailnavyneednewsnextnoonnotenumbobeyoboeomitonyxopenovalowlspaidpartpeckplaypluspoempoolposepuffpumapurrquadquizraceramprealredorichroadrockroofrubyruinrunsrustsafesagascarsetssilkskewslotsoapsolosongstubsurfswantacotasktaxitenttiedtimetinytoiltombtoystriptunatwinuglyundouniturgeuservastveryvetovialvibeviewvisavoidvowswallwandwarmwaspwavewaxywebswhatwhenwhizwolfworkyankyawnyellyogayurtzapszerozestzinczonezoom";

/// The word of a byte.
pub fn word(byte: u8) -> &'static str {
    let at = usize::from(byte) * 4;
    WORDS.get(at..at + 4).unwrap_or("")
}

/// Marks a pair of letters that is no word's first and last.
const NONE: u16 = 0xffff;

/// The byte of each pair of letters, by `(first - 'a') * 26 + (last - 'a')`.
/// Built at compile time: an index out of range would stop the build.
#[allow(clippy::indexing_slicing)]
const fn pair_table() -> [u16; 676] {
    let w = WORDS.as_bytes();
    let mut table = [NONE; 676];
    let mut i = 0;
    while i < 256 {
        let f = (w[i * 4] - b'a') as usize;
        let l = (w[i * 4 + 3] - b'a') as usize;
        table[f * 26 + l] = i as u16;
        i += 1;
    }
    table
}

static PAIRS: [u16; 676] = pair_table();

/// The byte of a minimal code (its first and last letter, lower case).
fn byte_of(first: u8, last: u8) -> Option<u8> {
    if !first.is_ascii_lowercase() || !last.is_ascii_lowercase() {
        return None;
    }
    let at = usize::from(first - b'a') * 26 + usize::from(last - b'a');
    match PAIRS.get(at).copied() {
        Some(v) if v != NONE => u8::try_from(v).ok(),
        _ => None,
    }
}

/// Bytes and their CRC-32, as minimal Bytewords.
pub fn encode_minimal(data: &[u8]) -> String {
    let crc = crc32(data).to_be_bytes();
    let mut out = String::with_capacity((data.len() + 4) * 2);
    for &b in data.iter().chain(crc.iter()) {
        let w = word(b).as_bytes();
        if let (Some(&f), Some(&l)) = (w.first(), w.last()) {
            out.push(char::from(f));
            out.push(char::from(l));
        }
    }
    out
}

/// Minimal Bytewords to the bytes, checking the CRC-32 (UR-05, UR-06).
pub fn decode_minimal(text: &str) -> Result<Vec<u8>, UrError> {
    let raw = text.as_bytes();
    if !raw.len().is_multiple_of(2) || raw.len() < 10 {
        return Err(UrError::Bytewords("odd length or under five bytes"));
    }
    let mut bytes = Vec::with_capacity(raw.len() / 2);
    for &[f, l] in raw.as_chunks::<2>().0 {
        match byte_of(f.to_ascii_lowercase(), l.to_ascii_lowercase()) {
            Some(b) => bytes.push(b),
            None => return Err(UrError::Bytewords("a letter pair is no word")),
        }
    }
    let split = bytes.len().saturating_sub(4);
    let (body, tail) = bytes.split_at(split);
    let Ok(crc) = <[u8; 4]>::try_from(tail) else {
        return Err(UrError::Bytewords("odd length or under five bytes"));
    };
    if crc32(body).to_be_bytes() != crc {
        return Err(UrError::BytewordsChecksum);
    }
    Ok(body.to_vec())
}
