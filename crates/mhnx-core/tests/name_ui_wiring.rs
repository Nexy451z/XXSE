//! What the two screens do with a name: the keyboard, the length, the warnings.
//!
//! The core writers were already checked (see name_newline.rs). This file is the
//! layer above them - the one the console and the PC window both use - and it
//! fixes four things:
//!
//! * a name typed with a line break goes through the editor and comes back out
//!   of the save unchanged, for all six fields,
//! * the editors text request sentinels map back to the field they edit (and a
//!   palico is not mistaken for the guild card comment),
//! * the input filter - the one door into a name buffer - refuses every control
//!   character but U+000A, so a tab cannot be typed at all,
//! * the status line prints the length in bytes and the line count, and reports
//!   a leading / trailing / doubled break without repairing it.

use mhnx_core::guildcard;
use mhnx_core::hbedit::{
    name_fit_label, name_refusal_line, name_status_line, name_warning_lines, Editor, ItemSetEdit,
    Key, NameSpec, NameTarget, Screen, SetEdit, TEXT_CARD_NAME, TEXT_GUILD_COMMENT,
    TEXT_ITEM_FILTER, TEXT_ITEM_SET_NAME, TEXT_NAME, TEXT_PALICO_NAME, TEXT_SET_NAME,
};
use mhnx_core::myset;
use mhnx_core::palico;
use mhnx_core::save::{
    empty_switch_save, name_line_count, NameError, NameUnit, Save, NAME_CAPACITY,
};

fn blank() -> Save {
    let mut s = empty_switch_save();
    s.set_slot_used(0, true);
    s
}

fn editor() -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(blank());
    ed.slot = 0;
    ed.dirty = false;
    ed
}

/// The save as the bytes an X press would write, read back through the parser.
fn round_trip(ed: &Editor) -> Save {
    let bytes = ed.save.as_ref().expect("a save").to_bytes();
    Save::from_bytes(&bytes).expect("the bytes are a save")
}

#[test]
fn a_hunter_name_with_a_break_survives_the_editor_and_a_save_round_trip() {
    let mut ed = editor();
    let fit = ed
        .apply_name_target(NameTarget::Hunter, "ナナ\nハル")
        .expect("a line break is allowed");
    assert_eq!(fit.len, 13);
    assert_eq!(fit.capacity, NAME_CAPACITY);
    assert_eq!(fit.lines, 2);
    assert!(!fit.clamped);
    assert!(ed.dirty);
    // The status line carries the figures the console footer prints:
    // 13/31 bytes and two lines, and the message never hides a warning.
    assert!(ed.message.contains("13/31 バイト（2行）"), "{}", ed.message);

    let read = ed.save.as_ref().expect("a save").read_character(0).expect("slot 0");
    assert_eq!(read.name, "ナナ\nハル");
    assert_eq!(read.name_preview, "ナナ\nハル", "the save screen copy holds it too");
    assert_eq!(read.name_lines(), vec!["ナナ", "ハル"]);

    // And the same after the file is written and read again: this is the
    // round trip the task is about.
    let reopened = round_trip(&ed);
    let back = reopened.read_character(0).expect("slot 0");
    assert_eq!(back.name, "ナナ\nハル");
    assert_eq!(back.name_line_count(), 2);
    assert_eq!(name_line_count(&back.name), 2);
}

#[test]
fn every_name_field_takes_a_line_break_through_the_editor() {
    let mut ed = editor();
    let text = "ナナ\nハル";
    let targets = [
        NameTarget::Hunter,
        NameTarget::Palico(0),
        NameTarget::ItemSet(0),
        NameTarget::EquipSet(0),
        NameTarget::CardName,
        NameTarget::CardComment,
    ];
    for target in targets {
        let fit = ed
            .apply_name_target(target, text)
            .unwrap_or_else(|e| panic!("{:?} refused the name: {:?}", target, e));
        assert_eq!(fit.lines, 2, "{:?} counts two lines", target);
    }

    // Every field reads back through the save parser, not through the editor.
    let reopened = round_trip(&ed);
    let c = reopened.read_character(0).expect("slot 0");
    assert_eq!(c.name, text);
    assert_eq!(palico::roster(&reopened, 0)[0].name, text);
    assert_eq!(palico::name_lines(&palico::roster(&reopened, 0)[0].name), vec!["ナナ", "ハル"]);
    assert_eq!(myset::item_sets(&reopened, 0)[0].name, text);
    assert_eq!(myset::equip_sets(&reopened, 0)[0].name, text);
    assert_eq!(myset::item_sets(&reopened, 0)[0].name_lines(), vec!["ナナ", "ハル"]);
    assert_eq!(guildcard::card_name(&reopened, 0).expect("a card"), text);
    assert_eq!(
        guildcard::card_name_lines(&reopened, 0).expect("a card"),
        vec![String::from("ナナ"), String::from("ハル")]
    );
    assert_eq!(guildcard::comment(&reopened, 0).expect("a card"), text);
    assert_eq!(
        guildcard::comment_line_count(&reopened, 0),
        Some(2)
    );
}

#[test]
fn a_tab_cannot_be_typed_into_a_name_and_a_refusal_writes_nothing() {
    let mut ed = editor();
    let before = ed.save.as_ref().expect("a save").body().to_vec();

    // The input filter is the only door into a name buffer: it drops every
    // control character but the line break, so no keyboard can type one.
    let spec = NameTarget::Hunter.spec();
    let mut buffer = String::from("ナナ");
    for bad in ["\t", "\r", "\u{0000}", "\u{007f}", "\u{0085}"] {
        assert_eq!(
            spec.push_str(&mut buffer, bad),
            Some(bad.chars().next().unwrap()),
            "{bad:?} has to be refused"
        );
        assert_eq!(buffer, "ナナ", "{bad:?} must not reach the buffer");
    }
    assert_eq!(spec.push_str(&mut buffer, "\n"), None, "a line break is allowed");
    assert_eq!(buffer, "ナナ\n");

    // A one-line field refuses the line break too.
    let one = NameSpec::singleline(60);
    let mut filter = String::new();
    assert_eq!(one.push_str(&mut filter, "a\nb"), Some('\n'));
    assert_eq!(filter, "ab");

    // The checked writers are the second guard: text that skipped the filter is
    // refused with the code point, and the save keeps every byte it had.
    let bad = "ナナ\tハル";
    for target in [
        NameTarget::Hunter,
        NameTarget::Palico(0),
        NameTarget::ItemSet(0),
        NameTarget::EquipSet(0),
        NameTarget::CardName,
        NameTarget::CardComment,
    ] {
        assert_eq!(
            ed.apply_name_target(target, bad),
            Err(NameError::Control('\t')),
            "{:?} took a tab",
            target
        );
        let line = name_refusal_line(NameError::Control('\t'));
        assert!(line.contains("その文字は使えません"), "{}", line);
        assert!(line.contains("U+0009"), "{}", line);
        assert!(line.contains("何も書き込んでいません"), "{}", line);
    }
    assert_eq!(
        ed.save.as_ref().expect("a save").body(),
        &before[..],
        "a refused name must not move a byte"
    );
    assert!(!ed.dirty, "a refused name is not an edit");
}

#[test]
fn the_text_request_sentinels_map_back_to_the_field_they_edit() {
    let mut ed = editor();

    assert_eq!(ed.text_request_name(TEXT_NAME), Some(NameTarget::Hunter));
    assert_eq!(
        ed.text_request_name(TEXT_PALICO_NAME),
        Some(NameTarget::Palico(0))
    );
    // The second palico used to land on the guild card comment: its sentinel was
    // one above the palico base and inside the contiguous palico range.
    assert_eq!(
        ed.text_request_name(TEXT_PALICO_NAME + 1),
        Some(NameTarget::Palico(1))
    );
    assert_eq!(ed.text_request_name(TEXT_GUILD_COMMENT), Some(NameTarget::CardComment));
    assert_eq!(ed.text_request_name(TEXT_CARD_NAME), Some(NameTarget::CardName));
    assert_eq!(
        ed.text_request_name(TEXT_ITEM_SET_NAME + 3),
        Some(NameTarget::ItemSet(3))
    );
    assert_eq!(ed.text_request_name(TEXT_ITEM_FILTER), None, "a filter is not a name");

    // The equipment set request carries no index, so the form on screen owns it.
    ed.set_edit = Some(SetEdit {
        set: 2,
        ..Default::default()
    });
    assert_eq!(ed.text_request_name(TEXT_SET_NAME), Some(NameTarget::EquipSet(2)));

    // The item set form asks for its name with [ZR].
    ed.item_set_edit = Some(ItemSetEdit {
        set: 4,
        ..Default::default()
    });
    assert!(ed.request_item_set_name());
    match ed.take_text_request() {
        Some((idx, text)) => {
            assert_eq!(idx, TEXT_ITEM_SET_NAME + 4);
            assert_eq!(text, "");
            assert_eq!(ed.text_request_name(idx), Some(NameTarget::ItemSet(4)));
        }
        None => panic!("the item set form has to ask for its name"),
    }
}

#[test]
fn the_length_label_counts_the_bytes_the_field_stores() {
    let hunter = NameTarget::Hunter.spec();
    assert_eq!(hunter.unit, NameUnit::Bytes);
    assert_eq!(hunter.length_label("ナナ\nハル"), "13/31 バイト（2行）");
    assert_eq!(hunter.length_label(""), "0/31 バイト（1行）");
    assert_eq!(hunter.length_label("ナナ\n"), "7/31 バイト（2行）");

    // The guild card stores UTF-16, so the same text is ten bytes there.
    let card = NameTarget::CardName.spec();
    assert_eq!(card.unit, NameUnit::Utf16);
    assert_eq!(card.length_label("ナナ\nハル"), "10/22 バイト（2行）");
    assert_eq!(card.capacity, guildcard::NAME_CAPACITY);
    let comment = NameTarget::CardComment.spec();
    assert_eq!(comment.length_label("ナナ\nハル"), "10/56 バイト（2行）");

    // The label a finished write prints comes from the fit itself.
    let fit = card.fit("ナナ\nハル").expect("valid");
    assert_eq!(name_fit_label(&fit), "10/22 バイト（2行）");
}

#[test]
fn ragged_breaks_are_reported_in_the_status_line_and_never_repaired() {
    let mut ed = editor();
    let ragged = "\nナナ\n\nハル\n";
    let fit = ed
        .apply_name_target(NameTarget::Hunter, ragged)
        .expect("ragged breaks are allowed");
    assert_eq!(fit.lines, 5);
    let warnings = name_warning_lines(&fit);
    assert!(warnings.iter().any(|w| w.contains("先頭が改行")), "{:?}", warnings);
    assert!(warnings.iter().any(|w| w.contains("末尾が改行")), "{:?}", warnings);
    assert!(warnings.iter().any(|w| w.contains("3行目が空行")), "{:?}", warnings);
    // The status line carries them all, and the write still happened.
    assert!(ed.message.contains("先頭が改行"), "{}", ed.message);
    assert!(ed.message.contains("5行"), "{}", ed.message);
    let stored = ed.save.as_ref().expect("a save").read_character(0).expect("slot 0");
    assert_eq!(stored.name, ragged, "nothing is repaired");
    assert_eq!(stored.name_lines(), vec!["", "ナナ", "", "ハル", ""]);
    // The same text leaves the save as it went in.
    let reopened = round_trip(&ed);
    assert_eq!(reopened.read_character(0).expect("slot 0").name, ragged);
}

#[test]
fn an_over_long_name_is_cut_to_the_field_and_reported() {
    let mut ed = editor();
    let long: String = "あ".repeat(12);
    let fit = ed
        .apply_name_target(NameTarget::Hunter, &long)
        .expect("a long name is written short, not refused");
    assert!(fit.clamped);
    assert_eq!(fit.len, long.len());
    assert!(fit.warnings.iter().any(|w| matches!(
        w,
        mhnx_core::save::NameWarning::TooLong { .. }
    )));
    assert!(ed.message.contains("切り詰めます"), "{}", ed.message);
    assert_eq!(
        ed.save.as_ref().expect("a save").read_character(0).expect("slot 0").name,
        "あ".repeat(10)
    );
}

#[test]
fn the_guild_card_name_is_its_own_row_and_writes_the_card() {
    let mut ed = editor();
    ed.screen = Screen::Guild;
    ed.guild_field = mhnx_core::hbedit::GUILD_ROW_NAME;
    ed.input(Key::A);
    match ed.take_text_request() {
        Some((idx, _)) => assert_eq!(idx, TEXT_CARD_NAME),
        None => panic!("[A] on the name row has to ask for the card name"),
    }
    ed.apply_name_target(NameTarget::CardName, "カード\n名")
        .expect("a line break is allowed");
    let reopened = round_trip(&ed);
    assert_eq!(guildcard::card_name(&reopened, 0).expect("a card"), "カード\n名");
    // The hunter name is a different copy and was not touched.
    assert_eq!(reopened.read_character(0).expect("slot 0").name, "");
    // The comment row still asks for the comment.
    ed.guild_field = mhnx_core::hbedit::GUILD_ROW_COMMENT;
    ed.input(Key::A);
    match ed.take_text_request() {
        Some((idx, _)) => assert_eq!(idx, TEXT_GUILD_COMMENT),
        None => panic!("[A] on the comment row has to ask for the comment"),
    }
}

#[test]
fn the_status_line_names_the_field_and_the_write_says_so() {
    let fit = NameTarget::Palico(0).spec().fit("アイルー\n2号").expect("valid");
    let line = name_status_line("オトモ名", "アイルー\n2号", &fit);
    assert!(line.contains("オトモ名"), "{}", line);
    assert!(line.contains("アイルー/2号"), "{}", line);
    assert!(line.contains(&name_fit_label(&fit)), "{}", line);
    assert!(line.contains("2行"), "{}", line);
}
