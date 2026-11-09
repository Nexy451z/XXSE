//! The B items of the third review: the buttons that fired on one press, the
//! keys a finger could not reach, and the lists that could not be narrowed.
//!
//! Every rule below lives in `hbedit` (or in `confirm`), so the console only
//! draws what these tests pin. The Switch layer's own half - which button is
//! drawn where, and that the review dialog scrolls - is the one part a host
//! test cannot reach; what it *sends* is pinned here.

use mhnx_core::confirm::ArmedAction;
use mhnx_core::hbedit::{
    BulkAction, Editor, Key, Screen, PROGRESS_QUESTS, PROGRESS_REQUESTS, REVIEW_DIALOG_ROWS,
    TEXT_EQUIP_PICK_FILTER, TEXT_ITEM_FILTER, TEXT_JEWEL_FILTER, TEXT_SKILL_FILTER,
};
use mhnx_core::save;

/// An editor on a synthetic slot: no file behind it, so nothing is written.
fn editor() -> Editor {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    let mut ed = Editor::new();
    ed.slot = 0;
    ed.save = Some(save);
    ed
}

fn save_of(ed: &Editor) -> &save::Save {
    ed.save.as_ref().expect("the fixture has a save")
}

// ---------------------------------------------------------------------------
// B11. Confirmations for the bulk buttons
// ---------------------------------------------------------------------------

/// [Y] on the slot screen wrote eleven fields on one press.
#[test]
fn the_character_bulk_max_asks_before_it_writes() {
    let mut ed = editor();
    ed.screen = Screen::Slots;
    assert_eq!(ed.char_value(1), 0, "the fixture starts empty");

    assert!(!ed.char_max_all_press(), "the first press only asks");
    assert_eq!(ed.char_value(1), 0, "nothing was written");
    assert_eq!(ed.char_value(3), 0);
    assert_eq!(ed.bulk_armed(), Some(BulkAction::CharMaxAll));
    assert!(ed.message.contains("もう一度"), "{}", ed.message);

    assert!(ed.char_max_all_press(), "the second press runs it");
    assert_eq!(ed.char_value(1), 9_999_999);
    assert_eq!(ed.char_value(3), 999);
    assert_eq!(ed.undo_depth(), 1, "one press is still one undo step");
    assert_eq!(ed.bulk_armed(), None, "the offer is spent, not left standing");
}

/// [A] on a character row wrote that row's ceiling on one press, and the row
/// it wrote was whichever the cursor happened to be on.
#[test]
fn the_character_row_max_asks_first_and_the_row_is_part_of_the_offer() {
    let mut ed = editor();
    ed.screen = Screen::Char;

    assert!(!ed.char_row_max_press(1), "the first press only asks");
    assert_eq!(ed.char_value(1), 0);
    assert_eq!(ed.bulk_armed(), Some(BulkAction::CharRowMax(1)));

    // Moving the cursor is a different row, so it is a different offer: the
    // second press on row 3 must not spend the one row 1 made.
    assert!(!ed.char_row_max_press(3), "another row has to ask for itself");
    assert_eq!(ed.char_value(3), 0, "nothing was written");
    assert_eq!(ed.char_value(1), 0);

    assert!(ed.char_row_max_press(3), "the second press on the same row runs");
    assert_eq!(ed.char_value(3), 999);
    assert_eq!(ed.char_value(1), 0, "only the row that asked was written");

    // The name row is text and the gender row is a toggle: neither is a bulk
    // write, so neither asks (they do their own thing).
    assert!(!ed.char_row_max_press(0), "the name row refuses either way");
    assert_eq!(ed.bulk_armed(), None);
}

/// [Y] on 配信オトモ installed the whole downloadable roster on one press, and
/// the same key on the roster beside it only walks a column.
#[test]
fn the_palico_bulk_install_asks_before_it_writes() {
    let mut ed = editor();
    ed.screen = Screen::Palico;
    let before = save_of(&ed).dlc_palicoes().iter().filter(|p| p.id != 0).count();

    assert!(!ed.install_all_palicoes_press(), "the first press only asks");
    assert_eq!(ed.bulk_armed(), Some(BulkAction::PalicoInstallAll));
    let mid = save_of(&ed).dlc_palicoes().iter().filter(|p| p.id != 0).count();
    assert_eq!(mid, before, "nothing was installed by the first press");
    assert!(ed.message.contains("もう一度"), "{}", ed.message);

    assert!(ed.install_all_palicoes_press(), "the second press installs");
    let after = save_of(&ed).dlc_palicoes().iter().filter(|p| p.id != 0).count();
    assert!(after > before, "the bulk install wrote {} records", after - before);
}

/// [Y] on the .arc list installed a file into the save on one press.
#[test]
fn the_arc_install_asks_before_it_writes() {
    let mut ed = editor();
    ed.screen = Screen::ArcList;
    ed.arc_files = alloc_vec(&["/switch/XXSE/mhxx/quest/arc/q1010001.arc"]);

    assert!(!ed.arc_install_press(0), "the first press only asks");
    assert_eq!(ed.bulk_armed(), Some(BulkAction::ArcInstall(0)));
    assert!(ed.message.contains("もう一度"), "{}", ed.message);
    assert!(ed.message.contains("q1010001.arc"), "{}", ed.message);

    assert!(ed.arc_install_press(0), "the second press runs it");
    assert_eq!(ed.bulk_armed(), None);
}

/// [A] on a bundled quest removed it from the save on one press.
#[test]
fn a_bundled_quest_is_removed_on_the_second_press() {
    use mhnx_core::dlc;
    let mut ed = editor();
    ed.screen = Screen::SaveQuests;
    let qid = mhnx_core::gamedata::DLC_CATALOG[0];
    assert!(
        dlc::add_quest(ed.save.as_mut().unwrap(), qid, &[1, 2, 3, 4]).is_ok(),
        "the fixture installs one quest"
    );
    assert!(ed.save_quests().iter().any(|q| q.quest_id == qid));

    assert!(!ed.bundled_quest_press(qid), "the first press only asks");
    assert!(
        ed.save_quests().iter().any(|q| q.quest_id == qid),
        "the quest is still in the save"
    );
    assert_eq!(ed.bulk_armed(), Some(BulkAction::QuestRemove(qid)));
    assert!(ed.message.contains("もう一度"), "{}", ed.message);

    assert!(ed.bundled_quest_press(qid), "the second press removes it");
    assert!(
        !ed.save_quests().iter().any(|q| q.quest_id == qid),
        "the quest is gone"
    );
}

/// Installing is additive, so it stays one press: only the removal asks.
#[test]
fn installing_a_bundled_quest_does_not_ask() {
    let mut ed = editor();
    ed.screen = Screen::SaveQuests;
    let qid = mhnx_core::gamedata::DLC_CATALOG[0];
    assert!(!ed.save_quests().iter().any(|q| q.quest_id == qid));
    // True means "run it now": the install half of the same key.
    assert!(ed.bundled_quest_press(qid), "installing is not destructive");
}

/// The snapshot restore already goes through the review dialog: the [Y] press
/// stages the write and only the dialog's [A] produces it.
#[test]
fn the_snapshot_restore_is_staged_as_a_review_not_written() {
    let mut ed = editor();
    ed.save_path = Some(String::from(mhnx_core::hbedit::CONSOLE_SAVE));
    let old = ed.save.as_ref().unwrap().to_bytes();
    let mut new = old.clone();
    // One byte that really differs, so the diff is not empty.
    let at = new.len() - 1;
    new[at] = new[at].wrapping_add(1);
    assert!(
        ed.stage_save_review(mhnx_core::hbedit::CONSOLE_SAVE, &old, &new),
        "a differing restore has to wait for the dialog"
    );
    assert!(ed.review_open(), "the dialog is what the [Y] press opens");
    assert!(ed.take_write().is_none(), "nothing was written by that press");
    assert!(ed.confirm_save_review().is_some(), "the [A] press writes");
    assert!(!ed.review_open());
}

// ---------------------------------------------------------------------------
// B11.7. The forms ask before [B] drops a draft
// ---------------------------------------------------------------------------

/// The talisman form's [B] dropped an unwritten draft with one press.
#[test]
fn the_charm_form_asks_before_b_drops_its_draft() {
    let mut ed = editor();
    ed.open_charms();
    ed.input(Key::L); // [＋] / [L]: a new talisman, seeded from the box
    assert_eq!(ed.screen, Screen::CharmEdit);
    let form = ed.charm_edit.expect("the form is open");
    {
        let e = ed.charm_edit.as_mut().unwrap();
        e.field = 2;
        e.slots = (form.slots + 1) % 4;
    }
    assert!(ed.charm_draft_dirty(), "the form holds an edit");

    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::CharmEdit, "the first [B] only asks");
    assert!(ed.form_discard_armed());
    assert!(ed.message.contains("もう一度"), "{}", ed.message);

    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms, "the second [B] goes back");
    assert!(!ed.form_discard_armed());
}

/// A form whose draft is already in the save leaves at once: asking there
/// would be a dialog about nothing.
#[test]
fn a_clean_form_leaves_on_the_first_b() {
    let mut ed = editor();
    ed.open_charms();
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(!ed.charm_draft_dirty(), "the fresh form matches its seed");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms, "a clean form does not ask");
}

/// The equipment form's [B] dropped the working copy with one press.
#[test]
fn the_equipment_form_asks_before_b_drops_its_draft() {
    let mut ed = editor();
    // One record in the box, so the form has something to edit.
    let idx = mhnx_core::equipops::create(ed.save.as_mut().unwrap(), 0, 7, 26, 5)
        .expect("a blank record");
    ed.screen = Screen::Equip;
    ed.equip_cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit);
    assert!(!ed.equip_draft_dirty(), "just opened, nothing changed");
    {
        let e = ed.equip_edit.as_mut().unwrap();
        e.level = 32;
    }
    assert!(ed.equip_draft_dirty());
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::EquipEdit, "the first [B] only asks");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Equip);
    let after = save_of(&ed).equipment_at(0, idx).expect("record");
    assert_eq!(after.level, 5, "the draft was never written");
}

/// The my-sets form's [B] dropped the pending parts with one press.
#[test]
fn the_set_form_asks_before_b_drops_its_draft() {
    let mut ed = editor();
    let idx = mhnx_core::equipops::create(ed.save.as_mut().unwrap(), 0, 7, 26, 5)
        .expect("a blank record");
    ed.screen = Screen::Sets;
    assert!(ed.open_set_editor(0), "the set form opens");
    assert!(!ed.set_draft_dirty(), "the picks start from the record");
    {
        let e = ed.set_edit.as_mut().unwrap();
        // Any part that is not the one the record holds: the form's picks are
        // the draft, and [A] is what writes them into the set.
        e.pick[0] = (idx as u16) + 5;
    }
    assert!(ed.set_draft_dirty());
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Sets, "the first [B] only asks");
    assert!(ed.set_edit.is_some(), "the form is still open");
    ed.input(Key::B);
    assert!(ed.set_edit.is_none(), "the second [B] closes it");
}

/// The item-set form has the same rule.
#[test]
fn the_item_set_form_asks_before_b_drops_its_draft() {
    let mut ed = editor();
    ed.screen = Screen::Sets;
    assert!(ed.open_item_set_editor(0), "the item set form opens");
    assert!(!ed.item_set_draft_dirty(), "the picks start from the record");
    {
        let e = ed.item_set_edit.as_mut().unwrap();
        e.pick[0] = mhnx_core::myset::SetItem { id: 1, count: 3 };
    }
    assert!(ed.item_set_draft_dirty());
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Sets, "the first [B] only asks");
    assert!(ed.item_set_edit.is_some());
    ed.input(Key::B);
    assert!(ed.item_set_edit.is_none());
}

/// Any other key takes the "throw it away" offer back: the second [B] has to
/// be the next thing the user does.
#[test]
fn another_key_takes_the_discard_offer_back() {
    let mut ed = editor();
    ed.open_charms();
    ed.input(Key::L);
    {
        let e = ed.charm_edit.as_mut().unwrap();
        e.slots = 3;
    }
    ed.input(Key::B);
    assert!(ed.form_discard_armed());
    ed.input(Key::Down); // looking at another row is not "yes, throw it away"
    assert!(!ed.form_discard_armed());
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::CharmEdit, "it has to be asked again");
}

// ---------------------------------------------------------------------------
// B12. The keys the touch buttons send are the pad's keys
// ---------------------------------------------------------------------------

/// The three pick lists' [Y] / [X] and the name picker's [-]: the deck's new
/// buttons send these, so this is what a tap does.
#[test]
fn the_picker_buttons_send_the_search_clear_and_kind_keys() {
    let mut ed = editor();
    // Item picker: [Y] asks for the search, [X] clears it.
    ed.screen = Screen::ItemPick;
    ed.item_pick_filter = String::from("ポーション");
    ed.input(Key::X);
    assert!(ed.item_pick_filter.is_empty(), "[X] clears the search");
    ed.input(Key::Y);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_ITEM_FILTER),
        "[Y] opens the search box"
    );

    // Skill picker: the same two.
    ed.screen = Screen::SkillPick;
    ed.skill_pick_filter = String::from("攻撃");
    ed.input(Key::X);
    assert!(ed.skill_pick_filter.is_empty());
    ed.input(Key::Y);
    assert_eq!(ed.take_text_request().map(|(id, _)| id), Some(TEXT_SKILL_FILTER));

    // Decoration picker: the same two.
    ed.screen = Screen::JewelPick;
    ed.jewel_pick_filter = String::from("攻撃");
    ed.input(Key::X);
    assert!(ed.jewel_pick_filter.is_empty());
    ed.input(Key::Y);
    assert_eq!(ed.take_text_request().map(|(id, _)| id), Some(TEXT_JEWEL_FILTER));

    // The name picker also has [-] for the type filter, which the deck draws.
    ed.screen = Screen::EquipPick;
    ed.equip_pick_scope = mhnx_core::hbedit::EquipScope::Weapons;
    let first = ed.equip_pick_kind;
    ed.input(Key::Minus);
    assert_ne!(ed.equip_pick_kind, first, "[-] walks the type filter");
    ed.input(Key::Y);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_EQUIP_PICK_FILTER)
    );
}

/// The talisman list's delete is the state machine's two-press key; the deck's
/// button sends exactly that key.
#[test]
fn the_charm_delete_button_is_the_same_two_press_delete() {
    let mut ed = editor();
    seed_charm(&mut ed);
    ed.open_charms();
    assert_eq!(ed.charms().len(), 1);
    ed.input(Key::Minus);
    assert_eq!(ed.charms().len(), 1, "the first press only asks");
    assert!(ed.charm_delete_armed.is_some());
    ed.input(Key::Minus);
    assert!(ed.charms().is_empty(), "the second press deletes");
    assert!(ed.charm_delete_armed.is_none());
}

/// The roster's [Y] walks the three editable columns; the deck's button sends
/// that key.
#[test]
fn the_roster_column_button_walks_the_three_columns() {
    let mut ed = editor();
    assert!(mhnx_core::palico::set_name(ed.save.as_mut().unwrap(), 0, 0, "アイルー"));
    ed.screen = Screen::Roster;
    assert_eq!(ed.roster_field, 0);
    ed.input(Key::Y);
    assert_eq!(ed.roster_field, 1);
    ed.input(Key::Y);
    assert_eq!(ed.roster_field, 2);
    ed.input(Key::Y);
    assert_eq!(ed.roster_field, 0, "three columns, round again");
}

/// The CSV screen's [Y] writes the two boxes out and [-] re-reads the folder.
#[test]
fn the_csv_buttons_write_and_rescan() {
    let mut ed = editor();
    ed.open_csv();
    assert!(ed.take_csv_rescan(), "opening the screen asks for the folder");
    ed.input(Key::Y);
    assert!(ed.take_csv_export(), "[Y] asks for the export");
    assert!(!ed.take_csv_export(), "and only once");
    ed.input(Key::Z);
    assert!(ed.take_csv_rescan(), "[ZR] asks for the folder again");
    // [-] is the delete key and a folder list has nothing to delete.
    ed.input(Key::Minus);
    assert!(!ed.take_csv_rescan(), "[-] is not the rescan any more");
}

/// The byte editor's four directions: the panel's four new buttons send these.
#[test]
fn the_byte_editor_moves_and_steps_with_the_four_directions() {
    let mut ed = editor();
    ed.open_save_map(Screen::Char);
    assert_eq!(ed.screen, Screen::SaveMap);
    assert!(ed.map_edit_enter(), "the editor opens on the selected row");
    let start = ed.map_edit_position();
    ed.input(Key::Down);
    assert_ne!(ed.map_edit_position(), start, "[▼] moves the caret");
    let moved = ed.map_edit_position();
    ed.input(Key::Up);
    assert_eq!(ed.map_edit_position(), start, "[▲] moves it back");
    ed.input(Key::Down);
    assert_eq!(ed.map_edit_position(), moved);
    // [▶] proposes a value, [◀] proposes the one below it again.
    ed.input(Key::Right);
    assert!(ed.map_edit.and_then(|e| e.pending).is_some(), "[▶] proposes");
    ed.input(Key::Left);
    assert!(ed.map_edit.and_then(|e| e.pending).is_some());
}

// ---------------------------------------------------------------------------
// B13.1. The progress screen's name search
// ---------------------------------------------------------------------------

/// The quest view listed 1,508 rows and the request view 380 with no way to
/// narrow either.
#[test]
fn the_progress_quest_list_can_be_narrowed_by_name() {
    let mut ed = editor();
    ed.screen = Screen::Progress;
    ed.progress_tab = PROGRESS_QUESTS;
    assert_eq!(ed.progress_rows().len(), mhnx_core::questprogress::BITS - 1);

    // A name the bundled table really carries, taken from the table itself so
    // the test does not depend on one quest's wording.
    let (row, name) = (0..ed.progress_len())
        .find_map(|row| {
            let name = ed.progress_quest_name(row);
            if name.is_empty() {
                None
            } else {
                Some((row, name))
            }
        })
        .expect("the bundled table names at least one quest");
    let needle = name.chars().take(3).collect::<String>();
    ed.apply_progress_filter(needle.clone());
    let rows = ed.progress_rows();
    assert!(!rows.is_empty(), "the search kept the row it was named after");
    assert!(rows.len() < ed.progress_len(), "and dropped the others");
    assert!(rows.contains(&row), "row {} answers {:?}", row, needle);
    assert!(ed.message.contains("件"), "{}", ed.message);

    // The row number is searchable too, the pair the save's quest list uses.
    ed.apply_progress_filter(String::from("1508"));
    assert!(ed.progress_rows().contains(&1507));

    // An empty answer is the whole list again.
    ed.apply_progress_filter(String::new());
    assert_eq!(ed.progress_rows().len(), ed.progress_len());
}

/// The request view has the same search.
#[test]
fn the_progress_request_list_can_be_narrowed() {
    let mut ed = editor();
    ed.screen = Screen::Progress;
    ed.progress_tab = PROGRESS_REQUESTS;
    assert_eq!(ed.progress_rows().len(), ed.progress_len());
    let flag = ed.request_flag_at(0).expect("the first request row has a flag");
    ed.apply_progress_filter(format!("{}", flag));
    let rows = ed.progress_rows();
    assert!(!rows.is_empty());
    assert!(rows.len() < ed.progress_len(), "the search narrowed the list");
}

/// A narrowed list must not leave the cursor - or [A]'s write - on a row it
/// hid.
#[test]
fn the_progress_cursor_stays_on_a_row_the_search_shows() {
    let mut ed = editor();
    ed.screen = Screen::Progress;
    ed.progress_tab = PROGRESS_QUESTS;
    ed.set_progress_cursor(1507);
    ed.apply_progress_filter(String::from("1508"));
    assert_eq!(ed.progress_cursor(), 1507, "the row the search kept");
    ed.apply_progress_filter(String::from("古代林の特産ゼンマイ"));
    let rows = ed.progress_rows();
    assert!(!rows.is_empty());
    assert!(
        rows.contains(&ed.progress_cursor()),
        "the cursor has to be on a row the list shows"
    );
    // One press of [▼] lands on the next row that is shown, not on a hidden
    // one.
    let before = ed.progress_cursor();
    ed.input(Key::Down);
    let after = ed.progress_cursor();
    if rows.len() > 1 {
        assert_ne!(before, after);
        assert!(rows.contains(&after));
    }
}

// ---------------------------------------------------------------------------
// B13.2. The undo pill does not promise [L] where [L] is the row step
// ---------------------------------------------------------------------------

#[test]
fn the_undo_pill_does_not_promise_l_on_a_picker() {
    let mut ed = editor();
    for screen in [
        Screen::ItemPick,
        Screen::SkillPick,
        Screen::JewelPick,
        Screen::EquipPick,
    ] {
        ed.screen = screen;
        let label = ed.undo_pill_label();
        assert!(
            !label.contains("L 2回長押し"),
            "{:?} spends L on the ten-row step, so the pill must not name it: {}",
            screen,
            label
        );
        assert!(label.contains("タップ2回"), "{}", label);
    }
    // Every other screen keeps both routes: a held [L] there really is the
    // whole-save undo.
    ed.screen = Screen::Char;
    assert!(ed.undo_pill_label().contains("L 2回長押し"));
}

// ---------------------------------------------------------------------------
// B13.3. The review dialog's body is longer than the panel
// ---------------------------------------------------------------------------

/// The dialog stopped at the panel's edge and said "ほか N 行" with no way to
/// reach them.
#[test]
fn the_review_body_can_be_longer_than_the_dialog_and_is_reachable() {
    let mut ed = editor();
    let old = ed.save.as_ref().unwrap().to_bytes();
    // Change enough values that the body outgrows the panel.
    for field in 1..=9 {
        ed.set_char_value(field, 1 + field as i64);
    }
    let new = ed.save.as_ref().unwrap().to_bytes();
    assert!(
        ed.stage_save_review(mhnx_core::hbedit::CONSOLE_SAVE, &old, &new),
        "the write differs, so it is reviewed"
    );
    let review = ed.review.as_ref().expect("the dialog");
    assert!(
        review.needs_scroll(),
        "the body is {} lines and the panel holds {}",
        review.len(),
        REVIEW_DIALOG_ROWS
    );
    // The last line is reachable: the window's offset clamps to it rather than
    // stopping at the panel's edge.
    let last = review.len() - 1;
    assert_eq!(review.scroll_to(usize::MAX), last);
    assert_eq!(review.scroll_to(0), 0);
    assert_eq!(review.scroll_to(REVIEW_DIALOG_ROWS), REVIEW_DIALOG_ROWS);
}

// ---------------------------------------------------------------------------
// B13.4. The conversion screen names where it writes
// ---------------------------------------------------------------------------

/// The line printed the *source* path in the brackets beside the output name.
#[test]
fn the_conversion_screen_names_the_output_path() {
    let mut ed = editor();
    ed.save_path = Some(String::from(mhnx_core::hbedit::CONSOLE_SAVE));
    let out = ed.convert_output_path().expect("a console save converts");
    assert_eq!(out, "/switch/mhnxeditor/system.3ds");
    assert!(
        !out.contains("nand:"),
        "the console save is never written to: {}",
        out
    );

    // A save on the card converts beside itself, and the chosen name is used.
    ed.save_path = Some(String::from("/switch/XXSE/saves/system"));
    ed.convert_name = String::from("backup");
    assert_eq!(
        ed.convert_output_path().as_deref(),
        Some("/switch/XXSE/saves/backup.3ds")
    );

    // A 3DS save converts the other way, and the extension follows the kind.
    let mut three_ds = editor();
    // A 3DS save is the bare body: no 36 byte header, so the kind is ThreeDs
    // and the conversion goes the other way.
    three_ds.save = Some(
        mhnx_core::save::Save::from_bytes(save::empty_switch_save().body())
            .expect("a 3DS body"),
    );
    three_ds.save_path = Some(String::from("/switch/XXSE/saves/system.3ds"));
    assert_eq!(
        three_ds.convert_output_path().as_deref(),
        Some("/switch/XXSE/saves/system.switch")
    );
}

// ---------------------------------------------------------------------------
// The one shape all of the above share
// ---------------------------------------------------------------------------

/// Two different bulk buttons must not confirm each other.
#[test]
fn two_bulk_buttons_cannot_confirm_each_other() {
    let mut a: ArmedAction<BulkAction> = ArmedAction::new();
    assert!(!a.press(BulkAction::CharMaxAll), "the first press asks");
    assert!(
        !a.press(BulkAction::PalicoInstallAll),
        "a different action only asks for itself"
    );
    assert_eq!(a.armed(), Some(BulkAction::PalicoInstallAll));
    assert!(a.press(BulkAction::PalicoInstallAll), "the same one runs");
    assert_eq!(a.armed(), None);
    a.clear();
    assert_eq!(a.armed(), None);
}

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

/// A talisman in box slot 0, seeded the way the game writes one.
fn seed_charm(ed: &mut Editor) {
    use mhnx_core::charm;
    let mut r = [0u8; charm::CHARM_RECORD];
    r[0x00] = 0x06;
    r[0x02] = 10;
    r[0x0C] = 74;
    r[0x0D] = 70;
    r[0x0E] = 6;
    r[0x0F] = 10;
    r[0x10] = 3;
    r[0x12] = 100;
    r[0x13] = 1;
    let b = ed
        .save
        .as_mut()
        .and_then(|s| s.equipment_box_mut(0))
        .expect("the box is writable");
    b[..charm::CHARM_RECORD].copy_from_slice(&r);
}

fn alloc_vec(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| String::from(*s)).collect()
}
