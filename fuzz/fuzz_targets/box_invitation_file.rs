#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_box::invitation_text;
use openbmt_core::boxpair::Invitation;

fuzz_target!(|data: &[u8]| {
    // The file is read as text, as `read_invitation_file` reads it.
    let Ok(contents) = std::str::from_utf8(data) else {
        return;
    };
    let text = invitation_text(contents);
    // One line, trimmed, and the same text again from itself.
    assert!(!text.contains('\n'));
    assert_eq!(text, text.trim());
    assert_eq!(invitation_text(&text), text);
    // What the Box then reads from it never panics, and an invitation that
    // is read reads back from its own text.
    if let Ok(i) = Invitation::from_text(&text) {
        assert_eq!(Invitation::from_text(&i.to_text()), Ok(i));
    }
});
