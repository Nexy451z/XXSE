//! The two name tables the review found in English, and the two key changes
//! that made [Y] mean one thing on the screens that list records.
//!
//! What is checked here is what the code now promises:
//!
//!  * `MONSTER_HUNT_NAMES` is the game's own 137 row hunt table in save order,
//!    with the table's placeholders left as placeholders (no invented names).
//!  * `PALICO_SUPPORT_MOVES` carries the Japanese names of the 58 support
//!    slots, indexed by the id the save stores.
//!  * [Y] on the item box is the 空きも表示 toggle, the heading says so, and the
//!    press no longer changes the step (the console layer used to override it).
//!  * [Y] on the talisman list is the search box, like every other list that
//!    can be narrowed, and not a second key for the form [A] already opens.

use mhnx_core::gamedata;
use mhnx_core::hbedit::{Editor, Key, Screen, TEXT_CHARM_FILTER, TEXT_EQUIP_FILTER};
use mhnx_core::itembox::TOTAL_ITEM_SLOTS;

fn editor() -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(mhnx_core::save::empty_switch_save());
    ed
}

/// A name with no ASCII letters is a Japanese name; the tables below are the
/// only two that are allowed to keep an English placeholder.
fn has_ascii_letter(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic())
}

#[test]
fn the_hunt_names_are_the_games_own_japanese_table() {
    let names = gamedata::MONSTER_HUNT_NAMES;
    assert_eq!(names.len(), 137, "one row per hunt record");
    // The order is the save's, and these four rows pin it: the table's rows
    // carry the offsets 0x5EA6 + 2i, 0x5FB8 + 2i and 0x60CA + 4i, so index 0 is
    // the first monster of the save and not the first row of some other list.
    assert_eq!(names[0], "リオレイア");
    assert_eq!(names[1], "リオレイア希少種");
    assert_eq!(names[3], "リオレウス");
    assert_eq!(names[136], "コンガ");
    // Eight rows are placeholders in the source table and must stay that way:
    // seven say 不明 (the English table had Unknown [105..111]) and the unused
    // row 133 is "ROBOT" there, which is not a Japanese name at all.
    let unknown = names.iter().filter(|n| **n == "不明").count();
    assert_eq!(unknown, 7, "the table's own unknown rows: {:?}", names);
    assert_eq!(names[133], "Unknown[133]", "no name was invented for ROBOT");
    let english: Vec<&str> = names.iter().copied().filter(|n| has_ascii_letter(n)).collect();
    assert_eq!(english, vec!["Unknown[133]"], "every other row is Japanese");
}

#[test]
fn the_support_moves_are_japanese_and_indexed_by_their_id() {
    let moves = gamedata::PALICO_SUPPORT_MOVES;
    assert_eq!(moves.len(), 58, "the table has one row per id");
    assert_eq!(moves[0], "-----", "slot 0 is the empty one");
    assert_eq!(moves[1], "薬草笛");
    assert_eq!(moves[2], "回復笛");
    assert_eq!(moves[45], "ネコ式火竜車");
    assert_eq!(moves[56], "キンダンドングリ");
    // Slot 57 is the table's blank row: it keeps the placeholder, and the two
    // rows are the only ones allowed to be ASCII.
    assert_eq!(moves[57], "NULL [57]");
    let english: Vec<&str> = moves.iter().copied().filter(|n| has_ascii_letter(n)).collect();
    assert_eq!(english, vec!["NULL [57]"], "no move name was invented");
    // The accessor the two UIs share reads the same table.
    assert_eq!(mhnx_core::palico::move_name(1), "薬草笛");
}

#[test]
fn y_toggles_the_empty_slots_and_the_heading_says_which_state_it_is_in() {
    let mut ed = editor();
    ed.screen = Screen::Items;
    // An empty box with the toggle off lists nothing; the heading says 所持品.
    assert!(!ed.show_empty);
    assert_eq!(ed.visible_items().len(), 0);
    assert_eq!(ed.item_box_heading(), "アイテムBOX（所持品）");

    let step = ed.step;
    ed.input(Key::Y);
    assert!(ed.show_empty, "[Y] must turn the empty slots on");
    assert_eq!(ed.step, step, "[Y] is not the step key on this screen");
    assert_eq!(ed.visible_items().len(), TOTAL_ITEM_SLOTS);
    assert_eq!(
        ed.item_box_heading(),
        "アイテムBOX（空きも表示）",
        "the heading has to name the state, or the list looks like a lost box"
    );
    assert!(ed.message.contains("空き枠"), "{}", ed.message);

    ed.input(Key::Y);
    assert!(!ed.show_empty, "the same key turns it back off");
    assert_eq!(ed.item_box_heading(), "アイテムBOX（所持品）");
}

#[test]
fn y_is_the_search_key_on_the_lists_that_can_be_narrowed() {
    // The talisman list: [Y] asks for the filter text. [A] is the form, and it
    // is the only route to it, so one key has one meaning on this screen.
    let mut ed = editor();
    ed.screen = Screen::Charms;
    ed.input(Key::Y);
    assert_eq!(
        ed.take_text_request().map(|(idx, _)| idx),
        Some(TEXT_CHARM_FILTER),
        "[Y] on the talisman list is the search box"
    );
    assert_eq!(ed.screen, Screen::Charms, "[Y] must not leave the list");

    // The equipment box, the same key and the same job.
    ed.screen = Screen::Equip;
    ed.input(Key::Y);
    assert_eq!(
        ed.take_text_request().map(|(idx, _)| idx),
        Some(TEXT_EQUIP_FILTER),
        "[Y] on the equipment box is the search box"
    );
}
