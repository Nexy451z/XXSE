//! The icon sheet's geometry and tints.
//!
//! The numbers here come from the game's own tables (`u8[25]` equipment icon
//! table, the rarity palette at 0x18994b4 and the item palette at 0x562060), and
//! the anchors were checked against the decoded sheet: cell 55 is the great
//! sword, 57 the sword and shield, 58 the dual blades, 69 the long sword.

use mhnx_core::icons;

#[test]
fn the_grid_starts_where_the_sheet_says_and_runs_ten_wide() {
    assert_eq!(icons::cell_for(0), icons::Rect { x: 0, y: 416, size: 48 });
    assert_eq!(icons::cell_for(9), icons::Rect { x: 432, y: 416, size: 48 });
    assert_eq!(icons::cell_for(10), icons::Rect { x: 0, y: 464, size: 48 });
    assert_eq!(icons::cell_for(94), icons::Rect { x: 192, y: 848, size: 48 });
}

#[test]
fn the_cells_past_the_grid_are_the_ones_the_game_lists() {
    assert_eq!(icons::cell_for(95), icons::Rect { x: 240, y: 848, size: 48 });
    assert_eq!(icons::cell_for(96), icons::Rect { x: 288, y: 848, size: 48 });
    assert_eq!(icons::cell_for(99), icons::Rect { x: 0, y: 896, size: 48 });
    assert_eq!(icons::cell_for(100), icons::Rect { x: 48, y: 896, size: 48 });
    // anything the game does not list falls back to its own default cell
    assert_eq!(icons::cell_for(200), icons::Rect { x: 288, y: 608, size: 48 });
}

#[test]
fn every_equipment_type_lands_on_the_cell_the_table_names() {
    // The table is indexed by the save's own type byte.
    assert_eq!(icons::cell_for_equipment(1), icons::cell_for(70)); // head
    assert_eq!(icons::cell_for_equipment(2), icons::cell_for(71)); // chest
    assert_eq!(icons::cell_for_equipment(3), icons::cell_for(73)); // arms
    assert_eq!(icons::cell_for_equipment(4), icons::cell_for(72)); // waist
    assert_eq!(icons::cell_for_equipment(5), icons::cell_for(74)); // legs
    assert_eq!(icons::cell_for_equipment(6), icons::cell_for(75)); // talisman
    assert_eq!(icons::cell_for_equipment(7), icons::cell_for(55)); // great sword
    assert_eq!(icons::cell_for_equipment(8), icons::cell_for(57)); // sword and shield
    // 18 is dual blades, the anchor the table was checked against.
    assert_eq!(icons::cell_for_equipment(18), icons::cell_for(58));
    assert_eq!(icons::cell_for_equipment(21), icons::cell_for(69)); // charge blade
}

#[test]
fn an_unknown_equipment_type_shows_the_empty_cell() {
    assert_eq!(icons::cell_for_equipment(0), icons::cell_for(46));
    assert_eq!(icons::cell_for_equipment(99), icons::cell_for(46));
}

#[test]
fn every_equipment_cell_fits_on_the_512_by_1024_sheet() {
    for kind in 0u8..=24 {
        let r = icons::cell_for_equipment(kind);
        assert!(
            r.x + r.size <= 512 && r.y + r.size <= 1024,
            "kind {} lands at {:?}, outside the sheet",
            kind,
            r
        );
    }
}

#[test]
fn the_rarity_palette_is_indexed_by_rarity_minus_one() {
    assert_eq!(icons::rarity_tint(0), icons::RARITY_COLORS[0]);
    assert_eq!(icons::rarity_tint(1), icons::RARITY_COLORS[0]);
    assert_eq!(icons::rarity_tint(9), icons::RARITY_COLORS[8]);
    assert_eq!(icons::rarity_tint(11), icons::RARITY_COLORS[10]);
    // past the table it clamps rather than reading out of bounds
    assert_eq!(icons::rarity_tint(200), icons::RARITY_COLORS[10]);
}

#[test]
fn the_item_palette_wraps_at_seventeen() {
    assert_eq!(icons::item_tint(0), icons::ITEM_COLORS[0]);
    assert_eq!(icons::item_tint(5), icons::ITEM_COLORS[5]);
    assert_eq!(icons::item_tint(22), icons::ITEM_COLORS[5]);
}

#[test]
fn a_colour_word_splits_into_rgba_in_canvas_order() {
    // 0xAABBGGRR: the low byte is red, which is what libnx RGBA8 wants first.
    assert_eq!(icons::to_rgba(0xFF5D_8CEE), [0xEE, 0x8C, 0x5D, 0xFF]);
    assert_eq!(icons::to_rgba(0xFFF5_F5F5), [0xF5, 0xF5, 0xF5, 0xFF]);
}

#[test]
fn a_white_pixel_takes_the_tint_and_a_black_one_stays_black() {
    let tint = 0xFF5D_8CEE;
    assert_eq!(icons::tint_pixel([255, 255, 255, 255], tint), [0xEE, 0x8C, 0x5D, 255]);
    assert_eq!(icons::tint_pixel([0, 0, 0, 255], tint), [0, 0, 0, 255]);
    // a transparent pixel stays transparent whatever the colour
    assert_eq!(icons::tint_pixel([255, 255, 255, 0], tint), [0xEE, 0x8C, 0x5D, 0]);
    // half brightness halves the colour
    assert_eq!(icons::tint_pixel([128, 128, 128, 255], tint), [0x77, 0x46, 0x2E, 255]);
}
