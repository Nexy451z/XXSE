//! A line break in a name, and the controls a name still refuses.
//!
//! MHXX draws U+000A in a name field as a second line (confirmed on hardware),
//! so the editor must let one through instead of treating the field as single
//! line text. Every field is a fixed width, NUL padded run of bytes, so this
//! file checks the same three things on all of them:
//!
//! * the line break survives the write and the read, byte for byte, with the
//!   terminator still behind it,
//! * the field splits into the lines the screen has to draw,
//! * every other control character is refused, leaving the save unchanged.
//!
//! The length limit is counted in the bytes the field stores, and a name that
//! does not fit is cut on a character boundary.

use mhnx_core::guildcard::{self, GC_NAME, GC_NAME_UNITS, GC_COMMENT, GC_COMMENT_CHARS};
use mhnx_core::myset;
use mhnx_core::palico;
use mhnx_core::save::{
    check_name, empty_switch_save, name_bytes, name_line_count, name_lines, NameError, NameUnit,
    NameWarning, Save, NAME_CAPACITY, NAME_LEN, OFF_NAME,
};

fn blank() -> Save {
    let mut s = empty_switch_save();
    s.set_slot_used(0, true);
    s
}

fn slot_base(save: &Save) -> usize {
    save.slot_base(0).expect("slot 0 has a base") as usize
}

/// The hunter name field of slot 0, as stored.
fn hunter_name_bytes(save: &Save) -> &[u8] {
    let at = slot_base(save) + OFF_NAME;
    &save.body()[at..at + NAME_LEN]
}

/// The palico name field of record 0, as stored.
fn palico_name_bytes(save: &Save) -> &[u8] {
    let at = slot_base(save) + palico::OFF_ROSTER + palico::NAME;
    &save.body()[at..at + palico::NAME_LEN]
}

/// The item set name field of set 0, as stored.
fn item_set_name_bytes(save: &Save) -> &[u8] {
    let at = slot_base(save) + myset::OFF_ITEM_SETS;
    &save.body()[at..at + myset::NAME_LEN]
}

/// The equipment set name field of set 0, as stored.
fn equip_set_name_bytes(save: &Save) -> &[u8] {
    let at = slot_base(save) + myset::OFF_EQUIP_SETS;
    &save.body()[at..at + myset::NAME_LEN]
}

/// The card name field of slot 0, as stored.
fn card_name_bytes(save: &Save) -> &[u8] {
    let at = slot_base(save) + guildcard::OFF_GUILD_CARD + GC_NAME;
    &save.body()[at..at + GC_NAME_UNITS * 2]
}

/// The card comment field of slot 0, as stored.
fn comment_bytes(save: &Save) -> &[u8] {
    let at = slot_base(save) + guildcard::OFF_GUILD_CARD + GC_COMMENT;
    &save.body()[at..at + GC_COMMENT_CHARS * 2]
}

fn hunter_name(save: &Save) -> String {
    save.read_character(0).expect("slot 0 reads").name
}

fn set_hunter_name(save: &mut Save, name: &str) {
    let mut c = save.read_character(0).expect("slot 0 reads");
    c.name = name.to_string();
    save.write_character(&c).expect("the name writes");
}

#[test]
fn a_line_break_survives_every_name_field() {
    let name = "ナナ\nハル";
    let mut save = blank();

    // The hunter carries two copies of the name; both have to hold it.
    set_hunter_name(&mut save, name);
    assert_eq!(hunter_name(&save), name);
    assert_eq!(&hunter_name_bytes(&save)[..name.len()], name.as_bytes());
    assert_eq!(hunter_name_bytes(&save)[name.len()], 0, "terminator");
    let preview = save.read_character(0).expect("slot 0 reads").name_preview;
    assert_eq!(preview, name, "the save screen copy holds it too");

    // A palico, through the plain and the checked writer.
    assert!(palico::set_name(&mut save, 0, 0, name));
    assert_eq!(palico::roster(&save, 0)[0].name, name);
    assert_eq!(&palico_name_bytes(&save)[..name.len()], name.as_bytes());
    assert_eq!(palico_name_bytes(&save)[name.len()], 0, "terminator");
    let fit = palico::set_name_checked(&mut save, 0, 1, name).expect("a line break is allowed");
    assert_eq!(fit.lines, 2);
    assert_eq!(palico::roster(&save, 0)[1].name, name);

    // Both my-set names, checked.
    myset::set_item_set_name_checked(&mut save, 0, 0, name).expect("allowed");
    assert_eq!(myset::item_sets(&save, 0)[0].name, name);
    assert_eq!(&item_set_name_bytes(&save)[..name.len()], name.as_bytes());
    assert_eq!(item_set_name_bytes(&save)[name.len()], 0, "terminator");
    myset::set_equip_set_name_checked(&mut save, 0, 0, name).expect("allowed");
    assert_eq!(myset::equip_sets(&save, 0)[0].name, name);
    assert_eq!(&equip_set_name_bytes(&save)[..name.len()], name.as_bytes());

    // The guild card stores UTF-16, so the newline is the unit 0x000A.
    guildcard::set_card_name_checked(&mut save, 0, name).expect("allowed");
    assert_eq!(guildcard::card_name(&save, 0).expect("a card"), name);
    assert_eq!(&card_name_bytes(&save)[4..6], &[0x0A, 0x00], "0x000A unit");
    assert_eq!(
        &card_name_bytes(&save)[10..12],
        &[0x00, 0x00],
        "terminator after the five units"
    );
    guildcard::set_comment_checked(&mut save, 0, name).expect("allowed");
    assert_eq!(guildcard::comment(&save, 0).expect("a card"), name);
    assert_eq!(&comment_bytes(&save)[4..6], &[0x0A, 0x00], "0x000A unit");
    assert_eq!(&comment_bytes(&save)[10..12], &[0x00, 0x00], "terminator");
}

#[test]
fn every_name_field_splits_into_the_lines_the_screen_draws() {
    let name = "ナナ\nハル";
    let mut save = blank();
    set_hunter_name(&mut save, name);
    palico::set_name_checked(&mut save, 0, 0, name).expect("allowed");
    myset::set_item_set_name_checked(&mut save, 0, 0, name).expect("allowed");
    myset::set_equip_set_name_checked(&mut save, 0, 0, name).expect("allowed");
    guildcard::set_card_name_checked(&mut save, 0, name).expect("allowed");
    guildcard::set_comment_checked(&mut save, 0, name).expect("allowed");

    let c = save.read_character(0).expect("slot 0 reads");
    assert_eq!(c.name_lines(), vec!["ナナ", "ハル"]);
    assert_eq!(c.name_line_count(), 2);

    let p = &palico::roster(&save, 0)[0];
    assert_eq!(p.name_lines(), vec!["ナナ", "ハル"]);
    assert_eq!(p.name_line_count(), 2);
    assert_eq!(palico::name_line_count(&p.name), 2);

    let item = &myset::item_sets(&save, 0)[0];
    assert_eq!(item.name_lines(), vec!["ナナ", "ハル"]);
    let equip = &myset::equip_sets(&save, 0)[0];
    assert_eq!(equip.name_line_count(), 2);

    assert_eq!(
        guildcard::card_name_lines(&save, 0).expect("a card"),
        vec!["ナナ".to_string(), "ハル".to_string()]
    );
    assert_eq!(guildcard::card_name_line_count(&save, 0), Some(2));
    assert_eq!(
        guildcard::comment_lines(&save, 0).expect("a card"),
        vec!["ナナ".to_string(), "ハル".to_string()]
    );
    assert_eq!(guildcard::comment_line_count(&save, 0), Some(2));

    // A name without a break is one line, and an empty name is not zero lines:
    // the screen always has a line to draw.
    assert_eq!(name_lines("ナナ"), vec!["ナナ"]);
    assert_eq!(name_line_count(""), 1);
    assert_eq!(name_lines(""), vec![""]);
}

#[test]
fn other_control_characters_are_refused_and_change_nothing() {
    let mut save = blank();
    let name = "ナナ\nハル";
    set_hunter_name(&mut save, name);
    palico::set_name_checked(&mut save, 0, 0, name).expect("allowed");
    myset::set_item_set_name_checked(&mut save, 0, 0, name).expect("allowed");
    myset::set_equip_set_name_checked(&mut save, 0, 0, name).expect("allowed");
    guildcard::set_card_name_checked(&mut save, 0, name).expect("allowed");
    guildcard::set_comment_checked(&mut save, 0, name).expect("allowed");
    let before = save.body().to_vec();

    for bad in ["ナナ\tハル", "ナナ\0ハル", "ナナ\rハル", "ナナ\u{7f}ハル"] {
        let control = bad.chars().find(|c| c.is_control() && *c != '\n').unwrap();

        // The hunter goes through the character record.
        let mut c = save.read_character(0).expect("slot 0 reads");
        c.name = bad.to_string();
        assert_eq!(
            save.write_character_checked(&c),
            Err(NameError::Control(control))
        );

        assert_eq!(
            palico::set_name_checked(&mut save, 0, 0, bad),
            Err(NameError::Control(control))
        );
        assert_eq!(
            myset::set_item_set_name_checked(&mut save, 0, 0, bad),
            Err(NameError::Control(control))
        );
        assert_eq!(
            myset::set_equip_set_name_checked(&mut save, 0, 0, bad),
            Err(NameError::Control(control))
        );
        assert_eq!(
            guildcard::set_card_name_checked(&mut save, 0, bad),
            Err(NameError::Control(control))
        );
        assert_eq!(
            guildcard::set_comment_checked(&mut save, 0, bad),
            Err(NameError::Control(control))
        );

        // The plain setters refuse the same characters: false means the
        // record was left alone, not that the write was silently partial.
        assert!(!palico::set_name(&mut save, 0, 0, bad));
        assert!(!myset::set_item_set_name(&mut save, 0, 0, bad));
        assert!(!myset::set_equip_set_name(&mut save, 0, 0, bad));
        assert!(!myset::set_name(&mut save, 0, 0, bad));
        assert!(!guildcard::set_card_name(&mut save, 0, bad));
        assert!(!guildcard::set_comment(&mut save, 0, bad));

        // A refused name leaves the field it would have overwritten alone, and
        // a raw field buffer untouched.
        assert_eq!(save.body(), &before[..], "nothing was written for {bad:?}");
        let mut field = [0xAAu8; 8];
        assert_eq!(
            mhnx_core::save::write_name_field_checked(&mut field, bad),
            Err(NameError::Control(control))
        );
        assert_eq!(field, [0xAAu8; 8]);
    }

    // A missing record is a different refusal, not a silent success.
    assert_eq!(
        palico::set_name_checked(&mut save, 0, palico::SLOTS, "ナナ"),
        Err(NameError::NoField)
    );
    assert_eq!(
        myset::set_item_set_name_checked(&mut save, 0, myset::ITEM_SETS, "ナナ"),
        Err(NameError::NoField)
    );
    assert_eq!(save.body(), &before[..]);
}

#[test]
fn a_long_name_is_cut_to_the_field_on_a_character_boundary() {
    let mut save = blank();

    // A three byte character is three of the field's bytes: ten of them fill
    // 30 of the 31 bytes the hunter name has, and the eleventh does not fit.
    let long = "あ".repeat(11);
    set_hunter_name(&mut save, &long);
    let got = hunter_name(&save);
    assert_eq!(got, "あ".repeat(10));
    assert_eq!(got.len(), 30, "counted in bytes, not characters");
    assert_eq!(hunter_name_bytes(&save)[30], 0, "terminator kept");

    // The tenth three byte character plus an ASCII one is exactly 31 bytes:
    // the limit is bytes, so this one fits whole.
    let mixed = format!("{}A", "あ".repeat(10));
    assert_eq!(mixed.len(), 31);
    set_hunter_name(&mut save, &mixed);
    assert_eq!(hunter_name(&save), mixed);
    assert_eq!(hunter_name_bytes(&save)[31], 0, "terminator kept");

    // The fit says what happened instead of leaving the cut silent.
    let fit = check_name(&"い".repeat(20), NAME_CAPACITY, NameUnit::Bytes).expect("valid");
    assert_eq!(fit.len, 60);
    assert_eq!(fit.capacity, NAME_CAPACITY);
    assert!(fit.clamped);
    assert!(fit.has(NameWarning::TooLong { len: 60, capacity: 31 }));

    // A palico record is the same width.
    let fit = palico::set_name_checked(&mut save, 0, 0, &"う".repeat(40)).expect("valid");
    assert!(fit.clamped);
    assert_eq!(palico::roster(&save, 0)[0].name, "う".repeat(10));

    // A my-set name is too.
    let fit = myset::set_equip_set_name_checked(&mut save, 0, 0, &"え".repeat(40)).expect("valid");
    assert!(fit.clamped);
    assert_eq!(myset::equip_sets(&save, 0)[0].name, "え".repeat(10));

    // The card name holds eleven units: the twelfth is dropped.
    let fit = guildcard::set_card_name_checked(&mut save, 0, &"お".repeat(12)).expect("valid");
    assert!(fit.clamped);
    assert_eq!(fit.len, 24, "UTF-16 units count two bytes each");
    assert_eq!(fit.capacity, GC_NAME_UNITS * 2);
    assert_eq!(
        guildcard::card_name(&save, 0).expect("a card"),
        "お".repeat(GC_NAME_UNITS)
    );

    // A name shorter than the field gets its terminator; one that fills the
    // eleven units has no room for it and leaves the bytes behind untouched.
    guildcard::set_card_name_checked(&mut save, 0, &"き".repeat(10)).expect("valid");
    assert_eq!(&card_name_bytes(&save)[20..22], &[0x00, 0x00], "terminator");

    // The comment holds twenty-eight.
    let fit = guildcard::set_comment_checked(&mut save, 0, &"か".repeat(30)).expect("valid");
    assert!(fit.clamped);
    assert_eq!(
        guildcard::comment(&save, 0).expect("a card"),
        "か".repeat(GC_COMMENT_CHARS)
    );
}

#[test]
fn ragged_line_breaks_are_warnings_not_edits() {
    let mut save = blank();

    // Leading, doubled and trailing breaks. None of them is repaired: the
    // player put them there, and the screen has to draw them as they are.
    let ragged = "\nナナ\n\nハル\n";
    set_hunter_name(&mut save, ragged);
    assert_eq!(hunter_name(&save), ragged);

    let fit = save
        .read_character(0)
        .expect("slot 0 reads")
        .name_fit()
        .expect("a line break is allowed");
    assert!(!fit.is_clean());
    assert!(fit.has(NameWarning::LeadingNewline));
    assert!(fit.has(NameWarning::TrailingNewline));
    assert!(fit.has(NameWarning::EmptyLine { line: 2 }));
    assert!(!fit.clamped);
    assert_eq!(fit.lines, 5);
    assert_eq!(
        save.read_character(0).expect("slot 0 reads").name_lines(),
        vec!["", "ナナ", "", "ハル", ""]
    );

    // The same remarks come back from the checked writers, for every field.
    let fit = palico::set_name_checked(&mut save, 0, 0, ragged).expect("allowed");
    assert!(fit.has(NameWarning::LeadingNewline));
    assert!(fit.has(NameWarning::EmptyLine { line: 2 }));
    assert_eq!(palico::roster(&save, 0)[0].name, ragged);

    let fit = myset::set_item_set_name_checked(&mut save, 0, 0, ragged).expect("allowed");
    assert!(fit.has(NameWarning::TrailingNewline));
    assert_eq!(myset::item_sets(&save, 0)[0].name, ragged);

    let fit = guildcard::set_card_name_checked(&mut save, 0, ragged).expect("allowed");
    assert_eq!(fit.lines, 5, "five lines on the card too");
    assert!(fit.has(NameWarning::EmptyLine { line: 2 }));
    assert_eq!(guildcard::card_name(&save, 0).expect("a card"), ragged);

    // A clean name has nothing to say.
    let fit = palico::name_check("ナナ").expect("valid");
    assert!(fit.is_clean());
    assert_eq!(fit.warnings, Vec::new());
    assert_eq!(fit.lines, 1);
}

#[test]
fn the_limit_is_counted_in_the_bytes_each_field_stores() {
    // The two units a field can store differ, so the same character count is a
    // different byte count. This is what keeps the capacity figures honest.
    assert_eq!(name_bytes("ナナ", NameUnit::Bytes), 6);
    assert_eq!(name_bytes("ナナ", NameUnit::Utf16), 4);
    assert_eq!(NameUnit::Bytes.width(), 1);
    assert_eq!(NameUnit::Utf16.width(), 2);

    // A UTF-8 field fits 31 bytes; a UTF-16 field of eleven units fits 22.
    assert!(!check_name(&"あ".repeat(10), 31, NameUnit::Bytes)
        .expect("valid")
        .clamped);
    assert!(check_name(&"あ".repeat(11), 31, NameUnit::Bytes)
        .expect("valid")
        .clamped);
    assert!(!check_name(&"あ".repeat(11), 22, NameUnit::Utf16)
        .expect("valid")
        .clamped);
    assert!(check_name(&"あ".repeat(12), 22, NameUnit::Utf16)
        .expect("valid")
        .clamped);

    // The newline is one byte in the UTF-8 field and two on the card.
    let fit = check_name("ナナ\nハル", 31, NameUnit::Bytes).expect("valid");
    assert_eq!(fit.len, 13);
    let fit = check_name("ナナ\nハル", 22, NameUnit::Utf16).expect("valid");
    assert_eq!(fit.len, 10);
}
