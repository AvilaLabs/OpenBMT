#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::entropy::{bits_from_dice, face_counts, words_from_dice_hashed};

fn only_faces(text: &str) -> bool {
    text.bytes().all(|b| (b'1'..=b'6').contains(&b))
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(words) = words_from_dice_hashed(text) {
        assert!(only_faces(text));
        assert_eq!(words.word_count(), 24);
    }
    if let Ok(counts) = face_counts(text) {
        assert!(only_faces(text));
        let total: u64 = counts.iter().map(|c| u64::from(*c)).sum();
        assert_eq!(total, text.len() as u64);
    }
    if let Ok(bits) = bits_from_dice(text) {
        assert!(only_faces(text));
        assert!(bits.len() <= text.len() / 2);
    }
});
