//! The equipment set editor (the mode the my-sets screen opens with A).
//!
//! The live save is only ever read: every test works on a copy in the temp
//! directory, named "system" because the Switch container header carries a
//! hash of the file name.

use std::path::{Path, PathBuf};

use mhnx_core::hbedit::{Editor, Key, Request, Screen, TEXT_SET_NAME};
use mhnx_core::myset::{self, EQUIP_SET_PARTS, NONE};
use mhnx_core::save::Save;

fn real_save_path() -> Option<PathBuf> {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        return Some(stable);
    }
    let ryujinx = PathBuf::from(
        r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
    );
    if ryujinx.exists() {
        Some(ryujinx)
    } else {
        None
    }
}

/// A private copy of the real save, so a test that writes writes nothing else.
fn copy_of_the_real_save(tag: &str) -> Option<(PathBuf, PathBuf)> {
    let src = real_save_path()?;
    let dir = std::env::temp_dir().join(format!("mhnx_setedit_{}_{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("system");
    std::fs::copy(&src, &dst).ok()?;
    Some((dir, dst))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Open the copy through the request/answer path the console uses.
fn open(ed: &mut Editor, copy: &Path) {
    ed.set_save_files(vec![copy.to_string_lossy().to_string()]);
    ed.input(Key::A);
    match ed.take_request() {
        Some(Request::ReadSave(_)) => {
            let bytes = std::fs::read(copy).expect("the copy is readable");
            ed.open_save_bytes(&copy.to_string_lossy(), &bytes);
        }
        other => panic!("expected a save read, got {:?}", other),
    }
    assert_eq!(ed.screen, Screen::Slots, "the save opens on the slot screen");
}

/// Slot screen -> character screen -> the my-sets screen.
fn open_sets(ed: &mut Editor) {
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Char);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::Sets);
}

/// Open the editor on the first equipment set that holds something.
fn open_editor(ed: &mut Editor) -> usize {
    let set = ed
        .equip_sets()
        .into_iter()
        .find(|s| !s.is_empty())
        .expect("the save holds a registered equipment set")
        .index;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Sets, "the editor is a mode, not a screen");
    assert!(ed.set_editor_open());
    assert_eq!(ed.editing_set().map(|s| s.index), Some(set));
    set
}

/// The set record as stored right now.
fn record_of(ed: &Editor, set: usize) -> myset::EquipSet {
    myset::equip_sets(ed.save.as_ref().expect("a save"), ed.slot)[set].clone()
}

/// Move the form's cursor to one part row.
fn goto_part(ed: &mut Editor, part: usize) {
    let want = part + 1;
    let mut guard = 0;
    while ed.set_edit.map(|e| e.field) != Some(want) && guard < SET_ROWS {
        ed.input(Key::Down);
        guard += 1;
    }
    assert_eq!(ed.set_edit.map(|e| e.field), Some(want), "the part row was not reached");
}

const SET_ROWS: u32 = 16;

#[test]
fn the_set_editor_walks_a_part_and_writes_it() {
    let Some((dir, copy)) = copy_of_the_real_save("part") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_sets(&mut ed);
    let set = open_editor(&mut ed);

    // The state machine keeps the minus button for the two-press clear.
    assert!(!ed.minus_rescans(), "the console must not rescan on Minus here");
    assert_eq!(ed.set_edit.map(|e| e.field), Some(0), "the form opens on the name");

    goto_part(&mut ed, 0);
    let before = record_of(&ed, set);
    let old = ed.set_pick(0);
    assert_eq!(old, before.parts[0], "the row starts on the stored record");
    ed.input(Key::Left);
    let other = ed.set_pick(0);
    assert_ne!(other, old, "Left must walk the box to another record");
    assert_ne!(other, NONE);
    // Walking the row must not write: only A does.
    assert_eq!(record_of(&ed, set).parts[0], old);
    assert!(!ed.dirty);

    ed.input(Key::A);
    assert!(ed.dirty, "A must write the part");
    let after = record_of(&ed, set);
    assert_eq!(after.parts[0], other);
    // The record it points at and the set now agree about decorations, which
    // is the second half of what set_part writes.
    let save = ed.save.as_ref().expect("a save");
    let box_bytes = save.equipment_box(ed.slot).expect("an equipment box");
    let at = other as usize * mhnx_core::equip::EQUIP_RECORD;
    let want = mhnx_core::equip::Equipment::decorations(
        other as usize,
        &box_bytes[at..at + mhnx_core::equip::EQUIP_RECORD],
    );
    assert_eq!(after.decos[0], want);
    // Nothing else in the set moved.
    assert_eq!(after.parts[1..], before.parts[1..]);

    // The console fallback draws the form, not the list.
    let text = ed
        .lines()
        .iter()
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("EDIT"), "{}", text);
    assert!(text.contains("武器"), "{}", text);
    cleanup(&dir);
}

#[test]
fn the_first_press_of_minus_only_names_the_part() {
    let Some((dir, copy)) = copy_of_the_real_save("clear") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_sets(&mut ed);
    let set = open_editor(&mut ed);

    // A part the set really uses: an empty one has nothing to clear.
    let rec = record_of(&ed, set);
    let part = (0..EQUIP_SET_PARTS)
        .find(|p| rec.parts[*p] != NONE)
        .expect("the set holds at least one part");
    goto_part(&mut ed, part);

    let before = ed.save.as_ref().expect("a save").body().to_vec();
    ed.input(Key::Minus);
    assert_eq!(
        ed.set_clear_armed,
        Some(part),
        "the first press must arm the clear, not perform it"
    );
    assert!(ed.message.contains("もう一度"), "{}", ed.message);
    assert_eq!(
        ed.save.as_ref().expect("a save").body(),
        before.as_slice(),
        "the first press must not touch the record"
    );
    assert!(!ed.dirty);

    ed.input(Key::Minus);
    assert!(ed.dirty, "the second press clears it");
    assert!(ed.set_clear_armed.is_none());
    let after = record_of(&ed, set);
    assert_eq!(after.parts[part], NONE);
    assert_eq!(after.decos[part], [0; 3], "the decoration copy goes with it");
    for other in 0..EQUIP_SET_PARTS {
        if other != part {
            assert_eq!(after.parts[other], rec.parts[other]);
            assert_eq!(after.decos[other], rec.decos[other]);
        }
    }
    assert_eq!(ed.set_pick(part), NONE, "the row shows the part as empty");
    cleanup(&dir);
}

#[test]
fn the_name_row_asks_the_keyboard_and_the_answer_renames_the_set() {
    let Some((dir, copy)) = copy_of_the_real_save("name") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_sets(&mut ed);
    let set = open_editor(&mut ed);
    let old = record_of(&ed, set).name;

    ed.input(Key::A);
    match ed.take_text_request() {
        Some((TEXT_SET_NAME, initial)) => assert_eq!(initial, old),
        other => panic!("expected the set-name prompt, got {:?}", other),
    }
    ed.apply_set_name(String::from("テスト装備"));
    assert!(ed.dirty);
    assert_eq!(record_of(&ed, set).name, "テスト装備");
    assert!(
        ed.set_editor_open(),
        "a rename must leave the form open on the same set"
    );
    cleanup(&dir);
}

#[test]
fn a_name_too_long_for_the_field_is_cut_on_a_character_boundary() {
    let Some((dir, copy)) = copy_of_the_real_save("clip") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_sets(&mut ed);
    let set = open_editor(&mut ed);

    // 40 three-byte characters into a 32 byte field.
    let long: String = "あ".repeat(40);
    ed.apply_set_name(long);
    let saved = record_of(&ed, set).name;
    assert!(saved.len() <= 31, "{} bytes is more than the field holds", saved.len());
    assert!(std::str::from_utf8(saved.as_bytes()).is_ok(), "the cut split a character");
    assert_eq!(saved.chars().count(), 10, "31 bytes hold ten 3-byte characters");
    cleanup(&dir);
}

#[test]
fn the_set_writes_survive_a_save_round_trip() {
    let Some((dir, copy)) = copy_of_the_real_save("round") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_sets(&mut ed);
    let set = open_editor(&mut ed);
    ed.apply_set_name(String::from("往復テスト"));
    goto_part(&mut ed, 0);
    ed.input(Key::Right);
    let pick = ed.set_pick(0);
    ed.input(Key::A);
    let before = record_of(&ed, set);

    // The bytes an X press would write, read back through the parser.
    let bytes = ed.save.as_ref().expect("a save").to_bytes();
    let reopened = Save::from_bytes(&bytes).expect("the bytes are a save");
    let after = myset::equip_sets(&reopened, ed.slot)[set].clone();
    assert_eq!(after.name, "往復テスト");
    assert_eq!(after.parts[0], pick);
    assert_eq!(after, before, "a round trip must not move a byte");
    cleanup(&dir);
}

#[test]
fn the_list_still_offers_the_editor_and_leaving_returns_to_it() {
    let Some((dir, copy)) = copy_of_the_real_save("leave") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_sets(&mut ed);
    let text = ed
        .lines()
        .iter()
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("MY SETS"), "{}", text);
    assert!(text.contains("EQUIPMENT SETS"), "{}", text);
    assert!(text.contains("編集"), "the list must say A edits: {}", text);

    open_editor(&mut ed);
    ed.input(Key::B);
    assert!(!ed.set_editor_open(), "B closes the form");
    assert_eq!(ed.screen, Screen::Sets, "and leaves the list where it was");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots, "B again leaves the screen");
    cleanup(&dir);
}
