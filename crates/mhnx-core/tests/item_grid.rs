//! The item box grid: 2300 slots, twenty three pages of ten by ten.
//!
//! Same shape as the equipment box, over a bigger box, driven through the same
//! `Editor` state machine the console runs.

use std::path::PathBuf;

use mhnx_core::equipgrid::{self, Cursor};
use mhnx_core::hbedit::{Editor, Key, Request, Screen};
use mhnx_core::itembox;

fn open_editor() -> Option<Editor> {
    let path = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if !path.exists() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let shown = path.to_string_lossy().to_string();
    let mut ed = Editor::new();
    ed.set_save_files(vec![shown.clone()]);
    ed.input(Key::A);
    match ed.take_request() {
        Some(Request::ReadSave(0)) => ed.open_save_bytes(&shown, &bytes),
        _ => return None,
    }
    Some(ed)
}

#[test]
fn the_item_box_is_twenty_three_pages_of_a_hundred() {
    assert_eq!(itembox::TOTAL_ITEM_SLOTS, 2300);
    assert_eq!(equipgrid::pages_for(itembox::TOTAL_ITEM_SLOTS), 23);
    let c = Cursor::over(itembox::TOTAL_ITEM_SLOTS);
    assert_eq!(c.slots(), 2300);
    assert_eq!(c.pages(), 23);
}

#[test]
fn the_last_item_slot_is_on_the_last_page() {
    let slots = itembox::TOTAL_ITEM_SLOTS;
    assert_eq!(equipgrid::page_of_in(slots, 2299), 22);
    assert_eq!(equipgrid::row_of_in(slots, 2299), 9);
    assert_eq!(equipgrid::column_of_in(slots, 2299), 9);
    assert_eq!(equipgrid::slot_at_in(slots, 22, 9, 9), 2299);
    assert_eq!(equipgrid::first_slot_of_in(slots, 22), 2200);
}

#[test]
fn a_cursor_over_2300_slots_walks_to_the_end_and_stops() {
    let mut c = Cursor::over(itembox::TOTAL_ITEM_SLOTS);
    c.set(2299);
    c.step_paged(1, 0);
    assert_eq!(c.slot(), 2299, "there is no page 24");
    c.set(0);
    c.step_paged(0, -1);
    assert_eq!(c.slot(), 0);
    c.go_to_page(99);
    assert_eq!(c.page(), 22);
}

#[test]
fn the_item_screen_moves_a_cell_at_a_time_on_the_console() {
    let Some(mut ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    ed.screen = Screen::Items;
    ed.cursor = 0;
    ed.use_item_grid();
    assert_eq!(ed.item_grid.slot(), 0);
    ed.input(Key::Right);
    assert_eq!(ed.item_grid.slot(), 1, "[▶] moves one cell");
    ed.input(Key::Down);
    assert_eq!(ed.item_grid.slot(), equipgrid::slot_at_in(2300, 0, 1, 1));
    ed.input(Key::Left);
    ed.input(Key::Up);
    assert_eq!(ed.item_grid.slot(), 0);
}

#[test]
fn a_press_on_an_empty_cell_opens_the_picker_for_that_cell() {
    let Some(mut ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    ed.screen = Screen::Items;
    ed.use_item_grid();
    // Find a slot the box really has empty rather than assuming one.
    let empty = (0..ed.item_box_len())
        .find(|i| ed.item_at_slot(*i).map(|s| s.id == 0).unwrap_or(false));
    let Some(empty) = empty else {
        eprintln!("skipping: the box is full");
        return;
    };
    ed.item_grid.set(empty);
    ed.item_grid_moved = true;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "{}", ed.message);
    assert_eq!(
        ed.item_pick_target, empty,
        "the picker is on the cell that was pressed"
    );
}

#[test]
fn the_box_len_is_the_boxes_own_length() {
    let Some(ed) = open_editor() else {
        eprintln!("skipping: no sample save");
        return;
    };
    assert_eq!(ed.item_box_len(), itembox::TOTAL_ITEM_SLOTS);
}
