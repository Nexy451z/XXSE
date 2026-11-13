//! The equipment box grid: ten by ten, twenty pages, 2000 slots.

use mhnx_core::equipgrid::{self, Cursor};

#[test]
fn the_box_is_twenty_pages_of_a_hundred_slots() {
    assert_eq!(equipgrid::SLOTS, 2000);
    assert_eq!(equipgrid::PER_PAGE, 100);
    assert_eq!(equipgrid::PAGES, 20);
}

#[test]
fn a_page_row_and_column_name_one_slot() {
    assert_eq!(equipgrid::slot_at(0, 0, 0), 0);
    assert_eq!(equipgrid::slot_at(0, 0, 9), 9);
    assert_eq!(equipgrid::slot_at(0, 1, 0), 10);
    assert_eq!(equipgrid::slot_at(0, 9, 9), 99);
    assert_eq!(equipgrid::slot_at(1, 0, 0), 100);
    assert_eq!(equipgrid::slot_at(19, 9, 9), 1999);
}

#[test]
fn slot_page_row_and_column_are_the_inverse() {
    for slot in [0usize, 9, 10, 99, 100, 1234, 1999] {
        let page = equipgrid::page_of(slot);
        let row = equipgrid::row_of(slot);
        let column = equipgrid::column_of(slot);
        assert_eq!(
            equipgrid::slot_at(page, row, column),
            slot,
            "slot {} did not round trip",
            slot
        );
    }
}

#[test]
fn out_of_range_arguments_clamp_into_the_page_rather_than_the_neighbour() {
    // A cursor that ran off the right edge of page 3 has to stay on page 3.
    assert_eq!(equipgrid::slot_at(3, 0, 99), equipgrid::slot_at(3, 0, 9));
    assert_eq!(equipgrid::page_of(equipgrid::slot_at(3, 0, 99)), 3);
    assert_eq!(equipgrid::slot_at(999, 0, 0), 1900);
    assert_eq!(equipgrid::slot_at(0, 99, 99), 99);
}

#[test]
fn the_cursor_starts_on_the_first_slot() {
    let c = Cursor::new();
    assert_eq!(c.slot(), 0);
    assert_eq!(c.page(), 0);
    assert_eq!(c.row(), 0);
    assert_eq!(c.column(), 0);
    assert!(c.at_page_start());
}

#[test]
fn a_horizontal_press_stops_at_the_page_edge_instead_of_wrapping() {
    let mut c = Cursor::new();
    c.step(-1, 0);
    assert_eq!(c.slot(), 0, "left from the first cell must not go anywhere");
    c.set(9);
    c.step(1, 0);
    assert_eq!(c.slot(), 9, "right from the last cell of row 0 stays on row 0");
}

#[test]
fn a_vertical_press_stops_at_the_page_edge_instead_of_turning_it() {
    let mut c = Cursor::new();
    c.step(0, -1);
    assert_eq!(c.slot(), 0);
    c.set(99);
    c.step(0, 1);
    assert_eq!(c.slot(), 99, "down from the last row must not spill onto page 1");
    assert!(c.at_page_end());
}

#[test]
fn the_cursor_walks_a_page_and_keeps_its_cell_across_a_page_turn() {
    let mut c = Cursor::new();
    c.step(0, 3);
    c.step(4, 0);
    assert_eq!((c.row(), c.column()), (3, 4));
    c.turn_page(1);
    assert_eq!(c.page(), 1);
    assert_eq!((c.row(), c.column()), (3, 4), "the cell is kept across a turn");
    c.turn_page(-1);
    assert_eq!(c.page(), 0);
}

#[test]
fn page_turns_clamp_at_both_ends() {
    let mut c = Cursor::new();
    c.turn_page(-1);
    assert_eq!(c.page(), 0);
    c.go_to_page(19);
    assert_eq!(c.slot(), 1900);
    c.turn_page(1);
    assert_eq!(c.page(), 19);
    c.go_to_page(999);
    assert_eq!(c.page(), 19);
}

#[test]
fn every_page_starts_on_its_own_hundred() {
    for page in 0..equipgrid::PAGES {
        assert_eq!(equipgrid::first_slot_of(page), page * 100);
    }
}
