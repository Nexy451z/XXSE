//! The pick lists answer the same keys, and the item search works.
//!
//! The hardware report was that [◀] / [▶] paged ten rows on some screens and
//! not on others. The cause was visible in the code: the item picker, the
//! decoration picker and the skill picker each carried their own copy of the
//! scroll, the second stage of the bulk add had the two key pairs swapped, and
//! the console layer scrolled the shoulders on its own. Every route now moves
//! through hbedit::pick_move, and these tests pin that rule where a route can
//! reach it, plus the name/id search that sits on top of it.

use mhnx_core::hbedit::{
    self, filter_item_ids_in, CharmEdit, Editor, EquipEdit, Key, Screen, ITEM_OP_ADD,
};
use mhnx_core::itembox::ItemSlot;
use mhnx_core::save;

/// A save whose box holds one item, so the box screen has a row to pick for.
fn editor_on_the_item_box() -> Editor {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    let mut items = save.read_item_box(0).expect("the box decodes");
    items[0] = ItemSlot { id: 9, count: 1 }; // 回復薬
    save.write_item_box(0, &items).expect("the box encodes");
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.screen = Screen::Items;
    ed
}

/// The item box route: [R] opens the picker for the slot under the cursor.
fn open_item_picker(ed: &mut Editor) {
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::ItemPick);
}

#[test]
fn the_item_picker_moves_ten_rows_and_one_row() {
    let mut ed = editor_on_the_item_box();
    open_item_picker(&mut ed);
    assert!(ed.item_pick_matches().len() > 2 * hbedit::PICK_STEP);

    // The footer names the rule, so it cannot be a per-route secret.
    let footer = ed.footer_hint();
    assert!(footer.contains("10件"), "{}", footer);
    assert!(footer.contains("1件"), "{}", footer);

    ed.input(Key::Right);
    assert_eq!(ed.item_pick_cursor, hbedit::PICK_STEP);
    ed.input(Key::Right);
    assert_eq!(ed.item_pick_cursor, 2 * hbedit::PICK_STEP);
    ed.input(Key::Left);
    assert_eq!(ed.item_pick_cursor, hbedit::PICK_STEP);
    ed.input(Key::Down);
    assert_eq!(ed.item_pick_cursor, hbedit::PICK_STEP + 1);
    ed.input(Key::Up);
    assert_eq!(ed.item_pick_cursor, hbedit::PICK_STEP);
}

#[test]
fn the_decoration_picker_uses_the_same_ten_rows() {
    let mut ed = Editor::new();
    ed.save = Some(save::empty_switch_save());
    ed.screen = Screen::EquipEdit;
    ed.equip_edit = Some(EquipEdit {
        index: 0,
        kind: 1,
        id: 1,
        level: 1,
        transmog_id: 0,
        transmog_level: 0,
        decorations: [0; 3],
        field: 4, // 装飾品1
        new: false,
    });
    ed.input(Key::Y);
    assert_eq!(
        ed.screen,
        Screen::JewelPick,
        "Y on a socket row opens the picker"
    );
    let n = ed.jewel_pick_matches().len();
    assert!(n > 2 * hbedit::PICK_STEP, "{} decorations", n);

    ed.input(Key::Right);
    assert_eq!(ed.jewel_pick_cursor, hbedit::PICK_STEP);
    ed.input(Key::Left);
    assert_eq!(ed.jewel_pick_cursor, 0);
    ed.input(Key::Down);
    assert_eq!(ed.jewel_pick_cursor, 1);
    assert!(ed.footer_hint().contains("10件"), "{}", ed.footer_hint());
}

#[test]
fn the_skill_picker_uses_the_same_ten_rows() {
    let mut ed = Editor::new();
    ed.save = Some(save::empty_switch_save());
    // 天の護石's first skill pool is 62 ids, so ten rows is a real page.
    ed.charm_edit = Some(CharmEdit {
        kind: 10,
        field: 3,
        ..CharmEdit::default()
    });
    ed.skill_pick_field = 1;
    ed.screen = Screen::SkillPick;
    let n = ed.skill_pick_matches().len();
    assert!(n > 2 * hbedit::PICK_STEP, "{} skills", n);

    ed.input(Key::Right);
    assert_eq!(ed.skill_pick_cursor, hbedit::PICK_STEP);
    ed.input(Key::Left);
    assert_eq!(ed.skill_pick_cursor, 0);
    ed.input(Key::Down);
    assert_eq!(ed.skill_pick_cursor, 1);
}

#[test]
fn the_bulk_add_keeps_the_big_step_on_the_same_key_pair() {
    let mut ed = editor_on_the_item_box();
    ed.open_item_menu();
    assert!(ed.item_menu_open);
    ed.item_menu_cursor = ITEM_OP_ADD;
    ed.item_armed = None;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);
    assert_eq!(ed.item_pick_mode, 3, "the picker is in its add mode");

    // Stage 1 is the list, and it pages like the other two pickers.
    ed.input(Key::Right);
    assert_eq!(ed.item_pick_cursor, hbedit::PICK_STEP);
    ed.input(Key::Left);
    assert_eq!(ed.item_pick_cursor, 0);

    // Stage 2 is one number. It keeps the same shape: the big step stays on
    // [◀] / [▶] and the small one moves to [▲] / [▼]. Both used to be the
    // other way round, which is what made the same key pair mean one thing on
    // the list and another a press later.
    ed.input(Key::A);
    assert_eq!(ed.item_pick_stage, 1);
    assert_eq!(ed.item_pick_add_count, 1);
    ed.input(Key::Right);
    assert_eq!(ed.item_pick_add_count, 11);
    ed.input(Key::Up);
    assert_eq!(ed.item_pick_add_count, 12);
    ed.input(Key::Down);
    assert_eq!(ed.item_pick_add_count, 11);
    ed.input(Key::Left);
    assert_eq!(ed.item_pick_add_count, 1);
    let footer = ed.footer_hint();
    assert!(footer.contains("個数±10"), "{}", footer);
    assert!(footer.contains("個数±1"), "{}", footer);
}

#[test]
fn the_search_line_names_the_filter_and_the_count() {
    let mut ed = editor_on_the_item_box();
    open_item_picker(&mut ed);
    let all = ed.item_pick_matches().len();
    assert!(all > 2500, "{} items", all);
    assert_eq!(ed.item_pick_search_line(all), format!("全 {}件", all));

    ed.apply_item_filter(String::from("回復薬"));
    let hits = ed.item_pick_matches().len();
    assert!(hits > 0 && hits < all, "{} hits of {}", hits, all);
    assert_eq!(
        ed.item_pick_search_line(hits),
        format!("検索: 回復薬（{}件）", hits)
    );
    // The status row and the drawn/text header both carry it.
    assert!(ed.message.contains("検索: 回復薬"), "{}", ed.message);
    let drawn: Vec<String> = ed.lines().into_iter().map(|l| l.text).collect();
    let joined = drawn.join("\n");
    assert!(joined.contains("検索: 回復薬"), "{}", joined);

    // An empty filter is the whole table again.
    ed.apply_item_filter(String::new());
    let back = ed.item_pick_matches().len();
    assert_eq!(back, all);
    assert!(ed.item_pick_search_line(back).starts_with("全 "));
}

#[test]
fn a_name_filter_matches_part_of_a_name_and_ignores_case() {
    // A synthetic table: the ids are the row numbers, which is what the real
    // one is too.
    let names = ["(none)", "Recovery Potion", "回復薬", "Attack Up"];
    assert_eq!(filter_item_ids_in("recovery", Some(&names)), vec![1]);
    assert_eq!(filter_item_ids_in("RECOVERY", Some(&names)), vec![1]);
    assert_eq!(filter_item_ids_in("回復", Some(&names)), vec![2]);
    assert_eq!(filter_item_ids_in("  attack  ", Some(&names)), vec![3]);
    // Every id is listed when nothing is asked for.
    assert_eq!(filter_item_ids_in("", Some(&names)).len(), 3);
    assert!(filter_item_ids_in("nothing-like-this", Some(&names)).is_empty());
}

#[test]
fn a_build_without_names_searches_the_id() {
    // Without a table the needle is the decimal id, so the picker still
    // narrows instead of going blank.
    let hits = filter_item_ids_in("975", None);
    assert!(hits.contains(&975), "{:?}", hits);
    assert!(!hits.contains(&974));
    assert!(hits.contains(&1975) && hits.contains(&2975), "{:?}", hits);
    assert_eq!(filter_item_ids_in("#975", None), hits, "a '#' is allowed");
    // Nothing to name means a name cannot match; the list is empty, not broken.
    assert!(filter_item_ids_in("回復薬", None).is_empty());
    // The empty needle is still every id.
    assert_eq!(
        filter_item_ids_in("", None).len(),
        hbedit::PICK_LAST_ITEM_ID
    );
}

#[test]
fn a_filter_with_no_hits_leaves_the_picker_usable() {
    let mut ed = editor_on_the_item_box();
    open_item_picker(&mut ed);
    ed.apply_item_filter(String::from("zzz-no-such-item"));
    assert!(ed.item_pick_matches().is_empty());
    assert!(ed.item_pick_search_line(0).contains("0件"));

    let before = ed
        .save
        .as_ref()
        .expect("a save")
        .read_item_box(0)
        .expect("a box");
    // Every direction key, then the choose key: nothing may panic, move or
    // write while the list is empty.
    for k in [Key::Down, Key::Up, Key::Right, Key::Left] {
        ed.input(k);
    }
    ed.input(Key::A);
    assert_eq!(
        ed.screen,
        Screen::ItemPick,
        "A with nothing to pick stays put"
    );
    let after = ed
        .save
        .as_ref()
        .expect("a save")
        .read_item_box(0)
        .expect("a box");
    assert_eq!(before, after, "an empty list must write nothing");
    // The screen still renders, and clearing the filter brings the table back.
    assert!(!ed.lines().is_empty());
    ed.input(Key::X);
    assert!(ed.item_pick_matches().len() > 2500);
}
