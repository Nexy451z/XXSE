//! The equipment screens: two-press delete, the new-record form, and the
//! replace path that swapping the type or the id takes.
//!
//! These drive the same `Editor` state machine the console build runs, against
//! the real save on this machine. They are skipped when the file is missing,
//! the same way the other on-device flow tests are.

use std::path::PathBuf;

use mhnx_core::equip;
use mhnx_core::equiprefs;
use mhnx_core::hbedit::{Editor, Key, Request, Screen};
use mhnx_core::save::Save;

fn real_save() -> Option<PathBuf> {
    // The fixed copy first: the Ryujinx save is live and the game writes to
    // it, so assertions pinned to its contents would move under the test.
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        return Some(stable);
    }
    let live = PathBuf::from(
        r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
    );
    live.exists().then_some(live)
}

/// Open the sample save the way the platform layer does.
fn open_editor() -> Option<Editor> {
    let path = real_save()?;
    let bytes = std::fs::read(&path).ok()?;
    let shown = path.to_string_lossy().to_string();
    let mut ed = Editor::new();
    ed.set_save_files(vec![shown.clone()]);
    ed.input(Key::A);
    match ed.take_request() {
        Some(Request::ReadSave(0)) => ed.open_save_bytes(&shown, &bytes),
        _ => return None,
    }
    if ed.screen != Screen::Slots {
        return None;
    }
    Some(ed)
}

fn used_records(save: &Save, slot: usize) -> usize {
    save.equipment_box(slot)
        .map(|b| {
            (0..equip::EQUIP_COUNT)
                .filter(|i| {
                    let o = i * equip::EQUIP_RECORD;
                    !equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
                })
                .count()
        })
        .unwrap_or(0)
}

fn is_blank(save: &Save, slot: usize, index: usize) -> bool {
    save.equipment_box(slot)
        .map(|b| {
            let o = index * equip::EQUIP_RECORD;
            equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
        })
        .unwrap_or(true)
}

fn record_bytes(save: &Save, slot: usize, index: usize) -> Vec<u8> {
    let b = save.equipment_box(slot).expect("a box");
    let o = index * equip::EQUIP_RECORD;
    b[o..o + equip::EQUIP_RECORD].to_vec()
}

/// The first record in the list that nothing points at, as (list position, box index).
fn unreferenced_row(ed: &Editor) -> Option<(usize, usize)> {
    let save = ed.save.as_ref()?;
    let slot = ed.slot;
    ed.equipment()
        .iter()
        .enumerate()
        .map(|(pos, e)| (pos, e.index))
        .find(|(_, index)| equiprefs::references(save, slot, *index).is_empty())
}

#[test]
fn the_equip_list_deletes_only_on_the_second_press() {
    let Some(mut ed) = open_editor() else { return };
    let slot = ed.slot;
    // The box is a section, not a key on the slot screen: [-] is the delete
    // key and nothing else (the review's twelfth finding).
    ed.screen = Screen::Equip;
    assert_eq!(ed.screen, Screen::Equip);

    let Some((pos, index)) = unreferenced_row(&ed) else {
        return;
    };
    ed.equip_cursor = pos;
    let before = used_records(ed.save.as_ref().unwrap(), slot);

    // First press: the record must still be there and the screen must still be
    // the list. This is the whole point of the two-press rule.
    ed.input(Key::Minus);
    assert_eq!(ed.screen, Screen::Equip);
    assert!(
        !is_blank(ed.save.as_ref().unwrap(), slot, index),
        "the first [-] must not clear the record"
    );
    assert_eq!(ed.equip_delete_armed, Some(index));
    assert!(
        ed.message.contains("もう一度"),
        "the first press has to say the second is the delete: {}",
        ed.message
    );
    let (pending, refs, text) = ed.equip_delete_pending().expect("a pending delete");
    assert_eq!(pending, index);
    assert_eq!(refs, 0, "the chosen record is not referenced");
    assert!(text.contains("削除"), "{}", text);

    // Second press: now it goes.
    ed.input(Key::Minus);
    assert!(
        is_blank(ed.save.as_ref().unwrap(), slot, index),
        "the second [-] clears the record"
    );
    assert_eq!(ed.equip_delete_armed, None, "the confirmation is spent");
    assert!(ed.dirty, "the save holds the deletion");
    assert_eq!(used_records(ed.save.as_ref().unwrap(), slot), before - 1);
    assert!(
        ed.message.contains("削除しました"),
        "message was {}",
        ed.message
    );
}

#[test]
fn a_referenced_record_shows_its_references_before_the_force_delete() {
    let Some(mut ed) = open_editor() else { return };
    let slot = ed.slot;
    ed.screen = Screen::Equip;

    let save = ed.save.as_ref().unwrap();
    let found = ed
        .equipment()
        .iter()
        .enumerate()
        .map(|(pos, e)| (pos, e.index))
        .find(|(_, index)| !equiprefs::references(save, slot, *index).is_empty());
    let Some((pos, index)) = found else { return };
    let refs = equiprefs::references(ed.save.as_ref().unwrap(), slot, index);
    ed.equip_cursor = pos;

    // First press: the references are named, and the record stays.
    ed.input(Key::Minus);
    assert_eq!(ed.equip_delete_armed, Some(index));
    assert!(
        ed.message.contains("もう一度") && ed.message.contains("強制"),
        "a referenced record must offer the forced second press: {}",
        ed.message
    );
    assert!(
        ed.message.contains(&refs[0].text()),
        "the first reference has to be on screen: {}",
        ed.message
    );
    assert!(!is_blank(ed.save.as_ref().unwrap(), slot, index));

    // Second press: force. The dangling index is the user's decision, which is
    // why the first press had to name it.
    ed.input(Key::Minus);
    assert!(is_blank(ed.save.as_ref().unwrap(), slot, index));
    assert!(ed.message.contains("削除しました"), "{}", ed.message);
}

#[test]
fn the_new_form_creates_a_record_and_selects_its_row() {
    let Some(mut ed) = open_editor() else { return };
    let slot = ed.slot;
    let Some(save) = ed.save.as_ref() else { return };
    let blank_index = (0..equip::EQUIP_COUNT).find(|i| is_blank(save, slot, *i));
    let Some(blank_index) = blank_index else {
        return;
    };
    let before = used_records(ed.save.as_ref().unwrap(), slot);

    ed.screen = Screen::Equip;
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::EquipEdit, "[+] opens the new form");
    let form = ed.equip_edit.expect("the form is open");
    assert!(form.new, "the form must know it is making a record");
    assert_eq!(form.field, 1, "the form starts on the ID row, whose list writes both halves");
    assert_eq!(form.kind, 1);

    // 種別 1 -> 2, ID 1 -> 11, レベル 1 -> 2. The id row takes the pick lists'
    // ten-row step (`hbedit::EQUIP_ID_STEP`), the other two rows one at a
    // time, which is the rule the row's own hint prints.
    ed.input(Key::Right);
    ed.input(Key::Down);
    ed.input(Key::Right);
    ed.input(Key::Down);
    ed.input(Key::Right);
    let form = ed.equip_edit.expect("still open");
    let id = 1 + mhnx_core::hbedit::EQUIP_ID_STEP as u16;
    assert_eq!((form.kind, form.id, form.level), (2, id, 2));

    // The cursor is back on 種別 after the wrap; the create row is レベル.
    ed.input(Key::Down);
    ed.input(Key::Down);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Equip, "[A] writes and returns to the list");
    assert_eq!(used_records(ed.save.as_ref().unwrap(), slot), before + 1);

    let created = ed
        .save
        .as_ref()
        .unwrap()
        .equipment_at(slot, blank_index)
        .expect("the first blank record is the one create() used");
    assert_eq!((created.kind, created.id, created.level), (2, id, 2));
    assert!(
        ed.message.contains("新規作成"),
        "message was {}",
        ed.message
    );
    // The row that was just written is selected, not the old cursor position.
    let list = ed.equipment();
    let pos = list
        .iter()
        .position(|q| q.index == blank_index)
        .expect("the created record is in the list");
    assert_eq!(ed.equip_cursor, pos);
    assert!(ed.dirty);
}

#[test]
fn changing_the_type_in_the_form_replaces_the_record() {
    let Some(mut ed) = open_editor() else { return };
    let slot = ed.slot;
    ed.screen = Screen::Equip;
    let Some((pos, index)) = unreferenced_row(&ed) else {
        return;
    };
    ed.equip_cursor = pos;

    // Put two decorations in so "replace empties them" is visible rather than
    // vacuous.
    {
        let save = ed.save.as_mut().unwrap();
        let rec = save.equipment_record_mut(slot, index).unwrap();
        equip::set_decoration(rec, 0, 2638);
        equip::set_decoration(rec, 1, 2639);
    }
    let before = ed.save.as_ref().unwrap().equipment_at(slot, index).unwrap();
    let record_before = record_bytes(ed.save.as_ref().unwrap(), slot, index);
    assert_ne!(
        equip::Equipment::decorations(0, &record_before),
        [0, 0, 0],
        "the test needs decorations to remove"
    );

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit);
    let mut guard = 0;
    while ed.equip_edit.map(|e| e.field) != Some(7) {
        ed.input(Key::Down);
        guard += 1;
        assert!(guard <= 12, "row 7 (種別) must be reachable");
    }
    let old_kind = ed.equip_edit.unwrap().kind;
    ed.input(Key::Right);
    let new_kind = ed.equip_edit.unwrap().kind;
    assert_ne!(new_kind, old_kind, "[▶] changes the type");

    // [A] on 種別 opens the name picker (that is the whole point of the row's
    // "▸ 選ぶ"), so the write comes from the row the form marks "▸ 書き込み":
    // 装備枠, row 0.
    while ed.equip_edit.map(|e| e.field) != Some(0) {
        ed.input(Key::Up);
    }
    // A type change asks once before it writes: the first [A] only says what
    // the change costs, so a record cannot be retyped by one press. The second
    // [A] is the replace (hbedit::Editor::equip_kind_pending).
    let depth = ed.undo_depth();
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    assert!(ed.equip_kind_pending().is_some(), "{}", ed.message);
    assert_eq!(ed.undo_depth(), depth, "the ask is not an edit");
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Equip);
    assert!(
        ed.message.contains("差し替え"),
        "the message has to name the replace path: {}",
        ed.message
    );

    let after = ed.save.as_ref().unwrap().equipment_at(slot, index).unwrap();
    assert_eq!(after.kind, new_kind);
    assert_eq!(after.id, before.id, "the id is untouched unless it is changed");
    assert_eq!(after.level, before.level);
    let record_after = record_bytes(ed.save.as_ref().unwrap(), slot, index);
    assert_eq!(
        equip::Equipment::decorations(0, &record_after),
        [0, 0, 0],
        "a replaced record has no decorations"
    );
    assert_eq!(
        &record_after[0x0C..],
        &record_before[0x0C..],
        "the tail at +0x0C belongs to the record, not to the item"
    );
    assert!(ed.dirty);
}

#[test]
fn the_box_screen_is_a_grid_and_a_direction_press_moves_one_cell() {
    let Some(mut ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    ed.open_equip_box();
    ed.use_equip_grid();
    assert_eq!(ed.equip_grid.slot(), 0);

    ed.input(Key::Right);
    assert_eq!(ed.equip_grid.slot(), 1, "[▶] moves one cell right");
    ed.input(Key::Down);
    assert_eq!(
        ed.equip_grid.slot(),
        mhnx_core::equipgrid::slot_at(0, 1, 1),
        "[▼] moves one cell down"
    );
    ed.input(Key::Left);
    ed.input(Key::Up);
    assert_eq!(ed.equip_grid.slot(), 0, "and back again");
}

#[test]
fn a_press_past_the_last_column_turns_the_page_rather_than_the_row() {
    let Some(mut ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    ed.open_equip_box();
    ed.use_equip_grid();
    ed.equip_grid.set(mhnx_core::equipgrid::slot_at(0, 3, 9));
    ed.equip_grid_moved = true;
    ed.input(Key::Right);
    assert_eq!(ed.equip_grid.page(), 1);
    assert_eq!((ed.equip_grid.row(), ed.equip_grid.column()), (3, 0));
}

#[test]
fn the_cursor_does_not_fall_off_the_first_cell() {
    let Some(mut ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    ed.open_equip_box();
    ed.use_equip_grid();
    ed.input(Key::Left);
    ed.input(Key::Up);
    assert_eq!(ed.equip_grid.slot(), 0);
}

#[test]
fn a_press_on_an_empty_cell_starts_a_new_record_in_that_cell() {
    let Some(mut ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    ed.open_equip_box();
    ed.use_equip_grid();
    // The last slot of the box is empty in every save this tool has seen; find
    // one that really is rather than assuming it.
    let empty = (0..equip::EQUIP_COUNT)
        .rev()
        .find(|i| ed.equip_at_slot(*i).is_none());
    let Some(empty) = empty else {
        eprintln!("skipping: the box has no empty slot");
        return;
    };
    ed.equip_grid.set(empty);
    ed.equip_grid_moved = true;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    let draft = ed.equip_edit.expect("a draft");
    assert!(draft.new, "the form is the new-record form");
    assert_eq!(draft.index, empty, "the record goes in the cell that was pressed");
}

#[test]
fn a_press_on_a_talisman_opens_the_talisman_form_on_that_record() {
    let Some(mut ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    ed.open_equip_box();
    ed.use_equip_grid();
    let talisman = (0..equip::EQUIP_COUNT)
        .find(|i| ed.equip_at_slot(*i).map(|e| e.kind == 6).unwrap_or(false));
    let Some(slot) = talisman else {
        eprintln!("skipping: the box holds no talisman");
        return;
    };
    ed.equip_grid.set(slot);
    ed.equip_grid_moved = true;
    ed.input(Key::A);
    // The box holds the talisman, so the press reaches 護石編集 rather than a
    // note pointing at another list.
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    let form = ed.charm_edit.expect("the talisman form");
    assert_eq!(form.index, slot, "the form is on the record that was pressed");
    // And it is not the equipment form, whose rows mean nothing for a talisman.
    assert!(ed.equip_edit.is_none());
}

