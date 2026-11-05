//! The equipment picker: the ID row answered with the game's own names.
//!
//! The hardware report was that a wanted weapon could only be found by stepping
//! an id - 1,606 weapons over 14 types and 6,435 armour pieces over five parts -
//! or by remembering that 40 is 爆砕の大剣. These tests pin what the picker
//! lists, how the search narrows it, what the header says about either, and the
//! keys it shares with the rest of the form ([Y] the name picker, [R] the typed
//! id), so the two cannot drift back to numbers alone.

use mhnx_core::equip;
use mhnx_core::hbedit::{self, EquipChoice, EquipEdit, EquipScope, Editor, Key, Screen};
use mhnx_core::save;

/// A form on the equipment type under test, on the row that names the id.
///
/// No save is needed: the picker lists the game's tables and writes the form's
/// working copy, and only [A] on the form touches the box.
fn form_on(kind: u8, id: u16, field: usize, new: bool) -> Editor {
    let mut ed = Editor::new();
    ed.equip_edit = Some(EquipEdit {
        index: 0,
        kind,
        id,
        level: 1,
        transmog_id: 0,
        transmog_level: 1,
        decorations: [0; 3],
        field,
        new,
    });
    ed.screen = Screen::EquipEdit;
    ed
}

/// The picker as the user opens it: [Y] on the 種別 / ID row.
fn open_picker(ed: &mut Editor) {
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EquipPick, "Y on an id row opens the list");
}

#[test]
fn the_picker_lists_every_named_row_of_the_box() {
    // The measured volume of the game's own tables: 1,606 weapons over the 14
    // types the box stores, 6,435 armour pieces over the five parts.
    let weapons = hbedit::equip_choices(EquipScope::Weapons);
    assert_eq!(weapons.len(), 1606);
    let armour = hbedit::equip_choices(EquipScope::Armor);
    assert_eq!(armour.len(), 6435);

    // One type is one block of the list, and every row of it is offered: 137
    // 大剣 and 127 太刀 are the counts the header prints.
    let great_swords = weapons.iter().filter(|c| c.kind == 7).count();
    assert_eq!(great_swords, 137);
    let long_swords = weapons.iter().filter(|c| c.kind == 14).count();
    assert_eq!(long_swords, 127);
    // The hole the JP numbering leaves (kind 12) has no table and no rows.
    assert_eq!(weapons.iter().filter(|c| c.kind == 12).count(), 0);
    // An armour id is a series and every part lists every series.
    for part in 1..=5u8 {
        assert_eq!(armour.iter().filter(|c| c.kind == part).count(), 1287);
    }

    // The names are the tables the box prints from, not a second list: the row
    // the box calls 爆砕の大剣 is the row the picker calls 爆砕の大剣.
    let burst = EquipChoice { kind: 7, id: 40 };
    assert_eq!(burst.name(), "爆砕の大剣");
    assert_eq!(burst.hex(), "0x28");
    assert_eq!(burst.label(), "爆砕の大剣  0x28");
    assert_eq!(equip::name(&equip::Equipment {
        index: 0,
        kind: 7,
        id: 40,
        level: 1,
        transmog_level: 1,
        transmog: 0,
    }), burst.name());
    // Armour: series 1 is レザー, and the part picks the piece.
    assert_eq!(EquipChoice { kind: 1, id: 1 }.name(), "レザーヘッド");
    assert_eq!(EquipChoice { kind: 5, id: 1 }.name(), "レザーパンツ");
}

#[test]
fn a_type_name_finds_that_type() {
    let mut ed = form_on(7, 1, 1, true);
    open_picker(&mut ed);
    // The picker opens narrowed to the type the form is on, so the first screen
    // is the 大剣 list rather than all 1,606 weapons.
    assert_eq!(ed.equip_pick_kind, 7);
    assert_eq!(ed.equip_pick_choices().len(), 137);
    assert_eq!(ed.equip_pick_search_line(137), "検索: 大剣（137件）");

    // [-] walks the filter: the rest of the types, then the whole scope.
    ed.input(Key::Minus);
    assert_eq!(ed.equip_pick_kind, 8);
    for _ in 0..(hbedit::WEAPON_KINDS.len() - 1) {
        ed.input(Key::Minus);
    }
    assert_eq!(ed.equip_pick_kind, 0, "the walk ends on 全種");
    assert_eq!(ed.equip_pick_choices().len(), 1606);
    assert_eq!(ed.equip_pick_search_line(1606), "全 1,606件");

    // The typed word answers the name *and* the type name, which is what makes
    // "太刀" the 127 of that type rather than the handful of names that happen
    // to contain the word.
    ed.apply_equip_pick_filter(String::from("太刀"));
    assert_eq!(ed.equip_pick_choices().len(), 127);
    assert_eq!(ed.equip_pick_search_line(127), "検索: 太刀（127件）");
    ed.apply_equip_pick_filter(String::from("大剣"));
    assert_eq!(ed.equip_pick_choices().len(), 137);
    // An empty answer is the whole list again: there is no separate clear.
    ed.apply_equip_pick_filter(String::new());
    assert_eq!(ed.equip_pick_choices().len(), 1606);
    assert_eq!(ed.equip_pick_search_line(1606), "全 1,606件");
}

#[test]
fn a_partial_name_finds_the_row_and_the_header_names_the_search() {
    let mut ed = form_on(7, 1, 1, true);
    open_picker(&mut ed);
    ed.input(Key::Minus); // widen to every weapon type
    for _ in 0..(hbedit::WEAPON_KINDS.len() - 1) {
        ed.input(Key::Minus);
    }
    ed.apply_equip_pick_filter(String::from("爆砕"));
    let choices = ed.equip_pick_choices();
    assert!(!choices.is_empty());
    assert!(choices.iter().all(|c| c.name().contains("爆砕")), "every row answers the word");
    assert!(ed.equip_pick_search_line(choices.len()).starts_with("検索: 爆砕（"));

    // A word nothing answers is an empty list, not a broken screen: no row, a
    // header that says 0 件, and a cursor that stays inside nothing.
    ed.apply_equip_pick_filter(String::from("そんな装備はない"));
    assert!(ed.equip_pick_choices().is_empty());
    assert_eq!(ed.equip_pick_search_line(0), "検索: そんな装備はない（0件）");
    assert_eq!(ed.equip_pick_cursor, 0);
    ed.input(Key::Down);
    ed.input(Key::Right);
    assert_eq!(ed.equip_pick_cursor, 0, "an empty list cannot be walked");
    assert_eq!(ed.screen, Screen::EquipPick, "and it stays on the list");
    // [A] on nothing selects nothing, leaves the form alone, and keeps the
    // list open so the word can be retyped.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipPick);
    assert!(ed.message.contains("選べる装備"), "{}", ed.message);
    let form = ed.equip_edit.expect("the form is still open");
    assert_eq!((form.kind, form.id), (7, 1));
}

#[test]
fn a_picked_row_writes_the_type_and_the_id_back_into_the_form() {
    // Creating: 種別 and ID come from create(), and the picker answers both -
    // id 40 is 爆砕の大剣 as a 大剣 and another weapon as a 太刀.
    let mut ed = form_on(7, 1, 1, true);
    open_picker(&mut ed);
    ed.input(Key::Minus);
    for _ in 0..(hbedit::WEAPON_KINDS.len() - 1) {
        ed.input(Key::Minus);
    }
    ed.apply_equip_pick_filter(String::from("爆砕の大剣"));
    let choices = ed.equip_pick_choices();
    assert_eq!(choices, vec![EquipChoice { kind: 7, id: 40 }]);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit, "A goes back to the form");
    let form = ed.equip_edit.expect("the form is still open");
    assert_eq!(form.kind, 7);
    assert_eq!(form.id, 40);
    assert!(ed.message.contains("爆砕の大剣"), "{}", ed.message);
    assert!(ed.message.contains("0x28"), "{}", ed.message);

    // Editing an existing record: the last two rows are 種別 / ID, and the
    // same pick writes the pair that makes [A] a 差し替え.
    let mut ed = form_on(1, 1, 8, false);
    open_picker(&mut ed);
    assert_eq!(ed.equip_pick_scope, EquipScope::Armor);
    assert_eq!(ed.equip_pick_kind, 1, "the form's own part narrows it first");
    // Widen to the whole armour family: the vest is a 胴 piece.
    for _ in 0..hbedit::ARMOR_KINDS.len() {
        ed.cycle_equip_pick_kind();
    }
    assert_eq!(ed.equip_pick_kind, 0);
    ed.apply_equip_pick_filter(String::from("レザーベスト"));
    ed.input(Key::A);
    let form = ed.equip_edit.expect("the form is still open");
    assert_eq!((form.kind, form.id), (2, 1));
    assert_eq!(EquipChoice { kind: form.kind, id: form.id }.name(), "レザーベスト");
}

#[test]
fn the_id_row_takes_a_typed_number_instead_of_stepping_to_it() {
    let mut ed = form_on(7, 1, 1, true);
    // [R] asks for the id, spelled the way the row prints it.
    ed.input(Key::R);
    assert_eq!(
        ed.take_text_request(),
        Some((hbedit::TEXT_EQUIP_ID, String::from("0x01")))
    );
    assert!(ed.apply_equip_id_text("0x28"));
    let form = ed.equip_edit.expect("the form is still open");
    assert_eq!(form.id, 40);
    assert!(ed.message.contains("爆砕の大剣"), "{}", ed.message);
    // A bare number is decimal, which is the one place this interface accepts
    // one; the row re-spells it in hex and names it, so a wrong reading shows.
    assert!(ed.apply_equip_id_text("40"));
    assert_eq!(ed.equip_edit.expect("form").id, 40);
    // An id the name table does not carry is accepted and said out loud.
    assert!(ed.apply_equip_id_text("0x7FFF"));
    assert_eq!(ed.equip_edit.expect("form").id, 0x7FFF);
    assert!(ed.message.contains("名前表に無い"), "{}", ed.message);
    // Text that is not a number changes nothing and says what the row wants.
    let before = ed.equip_edit.expect("form").id;
    assert!(!ed.apply_equip_id_text("大剣"));
    assert_eq!(ed.equip_edit.expect("form").id, before);
    assert!(ed.message.contains("0x28"), "{}", ed.message);

    // The other rows are not id rows: [R] says where it works rather than
    // asking for a number the row has no use for.
    let mut ed = form_on(7, 1, 1, false);
    ed.equip_edit.as_mut().unwrap().field = 1; // レベル
    ed.input(Key::R);
    assert_eq!(ed.take_text_request(), None);
    assert!(ed.message.contains("種別・ID"), "{}", ed.message);
}

#[test]
fn y_picks_by_name_on_the_id_rows_and_by_name_on_the_sockets() {
    // Two rows, two name pickers, one key: [Y] is "pick this row's name", and
    // which list opens is decided by the row.
    let mut ed = form_on(7, 1, 8, false);
    open_picker(&mut ed);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::EquipEdit, "B goes back to the form");
    ed.equip_edit.as_mut().unwrap().field = 7; // 種別
    open_picker(&mut ed);
    ed.input(Key::B);
    // A socket row still opens the decoration picker.
    ed.equip_edit.as_mut().unwrap().field = 4;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::JewelPick);
    // A row that is neither (the level) says which rows do have a name.
    ed.screen = Screen::EquipEdit;
    ed.equip_edit.as_mut().unwrap().field = 1;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EquipEdit);
    assert!(ed.message.contains("種別・ID"), "{}", ed.message);
}

#[test]
fn the_picker_leads_with_the_name_and_prints_the_id_in_hex() {
    // The row the hardware report was about: a name, and the id beside it in
    // hex. The decimal id is nowhere - the box, the form and this list spell an
    // equipment id one way.
    let mut ed = form_on(7, 1, 1, true);
    open_picker(&mut ed);
    ed.input(Key::Minus);
    for _ in 0..(hbedit::WEAPON_KINDS.len() - 1) {
        ed.input(Key::Minus);
    }
    ed.apply_equip_pick_filter(String::from("爆砕の大剣"));
    let all = ed
        .lines()
        .into_iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("
");
    assert!(all.contains("検索: 爆砕の大剣（1件）"), "{}", all);
    assert!(all.contains("爆砕の大剣  0x28"), "{}", all);

    // The choice itself prints the same pair, and the form row is the same
    // string with the level beside it.
    assert_eq!(
        EquipChoice { kind: 7, id: 40 }.label(),
        "爆砕の大剣  0x28"
    );
}

#[test]
fn the_picker_moves_one_row_ten_rows_and_answers_y_and_x() {
    let mut ed = form_on(7, 1, 1, true);
    open_picker(&mut ed);
    assert_eq!(ed.equip_pick_choices().len(), 137);
    // The cursor starts on the id the form already holds (id 1 is row 1), so
    // opening the picker twice does not lose the row.
    assert_eq!(ed.equip_pick_cursor, 1);
    // The pick list rule everywhere: [▲]/[▼] one, [◀]/[▶] ten.
    ed.input(Key::Right);
    assert_eq!(ed.equip_pick_cursor, 1 + hbedit::PICK_STEP);
    ed.input(Key::Left);
    assert_eq!(ed.equip_pick_cursor, 1);
    ed.input(Key::Down);
    assert_eq!(ed.equip_pick_cursor, 2);
    ed.input(Key::Up);
    assert_eq!(ed.equip_pick_cursor, 1);
    // [Y] asks for the word, [X] clears it without leaving the list.
    ed.input(Key::Y);
    assert_eq!(
        ed.take_text_request(),
        Some((hbedit::TEXT_EQUIP_PICK_FILTER, String::new()))
    );
    ed.apply_equip_pick_filter(String::from("爆"));
    let narrowed = ed.equip_pick_choices().len();
    assert!(narrowed > 0 && narrowed < 137);
    ed.input(Key::X);
    assert_eq!(ed.equip_pick_choices().len(), 137);
    assert_eq!(ed.equip_pick_search_line(137), "検索: 大剣（137件）");
    // The footer names the keys the list answers, including the type filter.
    let footer = ed.footer_hint();
    assert!(footer.contains("10件"), "{}", footer);
    assert!(footer.contains("種別"), "{}", footer);
    assert!(footer.contains("名前で絞る"), "{}", footer);
}

#[test]
fn a_wanted_weapon_is_created_from_its_name_alone() {
    // The whole report, end to end: a new record, the type picked on the form,
    // the name searched for, and the box holding the weapon - with no id typed
    // and no arrow key walked through 1,606 rows.
    let mut empty = save::empty_switch_save();
    empty.set_slot_used(0, true);
    let mut ed = Editor::new();
    ed.save = Some(empty);
    ed.screen = Screen::Equip;
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::EquipEdit);
    assert_eq!(ed.equip_edit.map(|e| e.new), Some(true));
    // 種別: 頭 (1) -> 大剣 (7).
    for _ in 0..6 {
        ed.input(Key::Right);
    }
    assert_eq!(ed.equip_edit.expect("form").kind, 7);
    ed.input(Key::Down);
    assert_eq!(ed.equip_edit.expect("form").field, 1, "the ID row");
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EquipPick);
    ed.input(Key::Minus);
    for _ in 0..(hbedit::WEAPON_KINDS.len() - 1) {
        ed.input(Key::Minus);
    }
    ed.apply_equip_pick_filter(String::from("爆砕の大剣"));
    ed.input(Key::A);
    let form = ed.equip_edit.expect("form");
    assert_eq!((form.kind, form.id), (7, 40));

    // [A] writes the record: the box now holds 爆砕の大剣.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Equip);
    assert!(ed.dirty);
    assert!(ed.message.contains("新規作成"), "{}", ed.message);
    let written = ed
        .equipment()
        .into_iter()
        .find(|e| e.kind == 7 && e.id == 40)
        .expect("the box holds the weapon that was chosen by name");
    assert_eq!(equip::name(&written), "爆砕の大剣");
}

#[test]
fn the_other_two_scopes_are_listed_too() {
    // A talisman form lists the eleven talisman names, and the armour scope
    // lists every part of every series.
    let mut ed = form_on(6, 1, 8, false);
    open_picker(&mut ed);
    assert_eq!(ed.equip_pick_scope, EquipScope::Talismans);
    let talismans = ed.equip_pick_choices().len();
    assert!(talismans >= 11, "{} talismans", talismans);
    assert!(ed.equip_pick_search_line(talismans).contains("検索: 護石（"));

    let mut ed = form_on(1, 1, 8, false);
    open_picker(&mut ed);
    assert_eq!(ed.equip_pick_choices().len(), 1287, "one part of the armour");
    // Widened to the whole armour family: five parts of 1,287 series.
    for _ in 0..hbedit::ARMOR_KINDS.len() {
        ed.cycle_equip_pick_kind();
    }
    assert_eq!(ed.equip_pick_kind, 0);
    assert_eq!(ed.equip_pick_choices().len(), 6435);
    assert_eq!(ed.equip_pick_search_line(6435), "全 6,435件");
}
