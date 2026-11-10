//! The thirteen findings of the third-party review, pinned one by one.
//!
//! Each test names the function the finding was about, so a regression points
//! at the code rather than at the screen. Four of them read switch/src/ui.rs as
//! text: the drawing bugs cannot be driven from a host test, so what is pinned
//! is the wiring (the console uses the state machine's answer) rather than the
//! pixels.
//!
//! Nothing here reads or writes a card: the saves are built in memory.

use mhnx_core::hbedit::{
    caption_width, footer_hint_room, footer_undo_w, Editor, Key, Screen, ZrJob,
    FOOTER_HINT_ROOM, GUILD_TAB_ARENA, GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS,
    ITEM_OP_FIND, PROGRESS_AWARDS, PROGRESS_QUESTS, QUEST_TAB_BASIC, QUEST_TAB_MONSTERS,
    TEXT_CHARM_FILTER, TEXT_ITEM_BOX_FILTER, TEXT_PROGRESS_FILTER,
};
use mhnx_core::save::{Save, BODY_SIZE, SLOT_STRIDE};

/// Body offset of the first character slot (also stored at body[0x10]).
const FIRST_SLOT_BASE: u32 = 0x126474;

/// The Switch layer, as text: the drawing bugs are pinned as wiring.
fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
}

/// The editor loop, as text.
fn editor_src() -> &'static str {
    include_str!("../../../switch/src/editor.rs")
}

/// The body of one function in ui.rs, from its declaration to the next item.
fn fn_body<'a>(src: &'a str, decl: &str) -> &'a str {
    let start = src
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} is not in switch/src/ui.rs"));
    let rest = &src[start + decl.len()..];
    let mut end = rest.len();
    for pat in ["\n    fn ", "\n    pub fn ", "\n    #[allow("] {
        if let Some(i) = rest.find(pat) {
            end = end.min(i);
        }
    }
    &rest[..end]
}

/// A zeroed body with the marker and the three slot bases filled in.
fn blank_body() -> Vec<u8> {
    let mut body = vec![0u8; BODY_SIZE];
    body[0] = 0xC6;
    for slot in 0..3 {
        let base = FIRST_SLOT_BASE as usize + slot * SLOT_STRIDE;
        body[0x10 + 4 * slot..0x14 + 4 * slot].copy_from_slice(&(base as u32).to_le_bytes());
        body[0x04 + slot] = 1;
    }
    body
}

/// An editor with the synthetic save open, on the slot list.
///
/// One palico is named in it: the roster's two panels are modes of that screen
/// and their footers are part of the table, so the walk has to be able to open
/// them.
fn open_blank() -> Editor {
    let mut save = Save::from_bytes(&blank_body()).expect("the synthetic save must parse");
    let _ = mhnx_core::palico::set_name(&mut save, 0, 0, "テスト");
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.save_path = Some(String::from("memory:review-13"));
    ed.slot = 0;
    ed.screen = Screen::Slots;
    ed
}

// ---------------------------------------------------------------------------
// 1: the progress screen's search is a key the pad can press
// ---------------------------------------------------------------------------

/// The guide said "[検索] 名前" and the state machine answered `Key::Z`, but the
/// console spent ZR on the section bar and then dropped the key before the
/// editor saw it - so the one key the guide named could not be pressed.
#[test]
fn the_progress_search_is_a_key_the_pad_can_press() {
    let mut ed = open_blank();
    ed.screen = Screen::Progress;
    assert_eq!(
        ed.zr_job(),
        ZrJob::Screen,
        "the console must hand ZR to the state machine here"
    );
    ed.input(Key::Z);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_PROGRESS_FILTER),
        "ZR has to open the search the footer promises"
    );
    // And the guide names that key, not a word no button carries.
    let hint = ed.footer_hint();
    assert!(hint.contains("[ZR] 検索"), "{hint}");
    assert!(!hint.contains("[検索]"), "{hint}");
    // Every other screen keeps the section bar.
    ed.screen = Screen::SaveList;
    assert_eq!(ed.zr_job(), ZrJob::Sidebar);
    // The console asks the same question (switch/src/editor.rs).
    assert!(
        editor_src().contains("ed.zr_job() == mhnx_core::hbedit::ZrJob::Screen"),
        "the console must route ZR through zr_job"
    );
}

// ---------------------------------------------------------------------------
// 2 + 8: a slot card's button opens *that* card's character
// ---------------------------------------------------------------------------

/// All three cards drew the same button and every one of them sent a bare
/// [A], which the console answered with `ed.slot` - so the button on card 2
/// opened card 1's character.
#[test]
fn each_slot_card_opens_its_own_character() {
    let mut ed = open_blank();
    // Slot 0 is used, slot 1 is not, slot 2 is used.
    ed.save.as_mut().expect("save").body_mut()[0x05] = 0;
    assert!(ed.slot_has_character(0));
    assert!(!ed.slot_has_character(1), "the blank slot has no record");
    assert!(ed.slot_has_character(2));

    // Open the third card's character while the highlight is on the first.
    assert_eq!(ed.slot, 0);
    assert!(ed.open_slot(2), "{}", ed.message);
    assert_eq!(ed.slot, 2, "the card's own slot is the one that opens");
    assert_eq!(ed.screen, Screen::Char);
    assert!(ed.message.contains("Slot 3"), "{}", ed.message);

    // The blank card opens nothing and says why.
    ed.screen = Screen::Slots;
    assert!(!ed.open_slot(1));
    assert_eq!(ed.screen, Screen::Slots, "a blank card stays on the list");
    assert!(ed.message.contains("キャラがいません"), "{}", ed.message);

    // The console pushes the slot with the button and draws none on a blank
    // card (ui.rs draw_slots).
    let body = fn_body(ui_src(), "fn draw_slots");
    assert!(body.contains("UiAction::OpenSlot(s)"), "{body}");
    assert!(
        !body.contains("UiAction::Key(Key::A)"),
        "the card must not send a bare [A] any more"
    );
    assert!(body.contains("ed.slot_has_character(s)"), "{body}");
    assert!(body.contains("この枠にキャラがいません"), "{body}");
}

// ---------------------------------------------------------------------------
// 3: every footer fits the strip it is drawn in
// ---------------------------------------------------------------------------

/// The guide is right aligned in the strip between the undo pill and the 終了
/// Every footer the console can draw, with its width in the 17px caption font.
///
/// The walk is a function so the fit test and the printed table cannot disagree
/// about what "every footer" means. `cargo test -p mhnx-core --test review_13
/// the_footer_width_table -- --nocapture` prints it.
fn footer_table() -> Vec<(String, i32)> {
    let mut out: Vec<(String, i32)> = Vec::new();
    let mut check = |ed: &Editor, what: &str| {
        let hint = ed.footer_hint();
        let w = caption_width(&hint);
        out.push((format!("{what}  [{w}px]  {hint}"), w));
    };

    let mut ed = open_blank();
    ed.screen = Screen::SaveList;
    check(&ed, "SaveList");
    ed.screen = Screen::Slots;
    check(&ed, "Slots");
    ed.screen = Screen::Char;
    check(&ed, "Char");
    ed.screen = Screen::Items;
    check(&ed, "Items");
    ed.input(Key::Minus);
    ed.open_item_menu();
    check(&ed, "Items+menu");
    ed.input(Key::B);
    ed.screen = Screen::ItemPick;
    check(&ed, "ItemPick");
    ed.item_pick_mode = 3;
    ed.item_pick_stage = 1;
    check(&ed, "ItemPick+count");
    ed.item_pick_mode = 0;
    ed.item_pick_stage = 0;
    ed.screen = Screen::SkillPick;
    check(&ed, "SkillPick");
    ed.screen = Screen::JewelPick;
    check(&ed, "JewelPick");
    ed.screen = Screen::EquipPick;
    check(&ed, "EquipPick");
    ed.equip_pick_kind = 2;
    check(&ed, "EquipPick+kind");
    ed.equip_pick_kind = 0;
    ed.screen = Screen::Charms;
    check(&ed, "Charms");
    ed.input(Key::A);
    check(&ed, "CharmEdit");
    ed.screen = Screen::Charms;
    ed.screen = Screen::Equip;
    check(&ed, "Equip");
    // [+] opens the new-record form, which is the same screen and the same
    // footer as the edit form.
    ed.input(Key::Plus);
    check(&ed, "EquipEdit");
    ed.screen = Screen::Equip;
    ed.screen = Screen::Sets;
    check(&ed, "Sets");
    ed.open_set_editor(0);
    check(&ed, "Sets+equip-form");
    ed.close_set_editor();
    ed.open_item_set_editor(0);
    check(&ed, "Sets+item-form");
    ed.close_item_set_editor();
    ed.screen = Screen::Csv;
    check(&ed, "Csv");
    ed.csv_armed = Some(0);
    check(&ed, "Csv+armed");
    ed.csv_armed = None;
    ed.screen = Screen::Options;
    check(&ed, "Options");
    ed.screen = Screen::Roster;
    check(&ed, "Roster");
    ed.open_palico_equip(0);
    check(&ed, "Roster+equip");
    ed.close_palico_equip();
    ed.open_palico_skill(0);
    check(&ed, "Roster+skill");
    ed.close_palico_skill();
    ed.screen = Screen::Palico;
    check(&ed, "Palico");
    ed.screen = Screen::Hunts;
    check(&ed, "Hunts");
    ed.screen = Screen::Guild;
    ed.set_guild_view(GUILD_TAB_CARD);
    check(&ed, "Guild+card");
    ed.set_guild_view(GUILD_TAB_COUNTS);
    check(&ed, "Guild+counts");
    ed.set_guild_view(GUILD_TAB_WEAPONS);
    check(&ed, "Guild+weapons");
    ed.guild_wpn_edit = true;
    check(&ed, "Guild+weapons-edit");
    ed.guild_wpn_edit = false;
    ed.set_guild_view(GUILD_TAB_ARENA);
    check(&ed, "Guild+arena");
    ed.arena_edit = true;
    check(&ed, "Guild+arena-edit");
    ed.arena_edit = false;
    ed.screen = Screen::Progress;
    ed.progress_tab = PROGRESS_QUESTS;
    check(&ed, "Progress+quests");
    ed.progress_tab = PROGRESS_AWARDS;
    check(&ed, "Progress+awards");
    ed.screen = Screen::SaveQuests;
    check(&ed, "SaveQuests");
    ed.screen = Screen::ArcList;
    check(&ed, "ArcList");
    ed.screen = Screen::ArcEntries;
    check(&ed, "ArcEntries");
    ed.screen = Screen::Mib;
    check(&ed, "Mib");
    ed.screen = Screen::Gmd;
    check(&ed, "Gmd");
    ed.screen = Screen::EntryView;
    check(&ed, "EntryView");
    ed.screen = Screen::Check;
    check(&ed, "Check");
    ed.screen = Screen::QuestEquip;
    check(&ed, "QuestEquip");
    ed.screen = Screen::QuestEdit;
    ed.quest_edit_tab = QUEST_TAB_BASIC;
    check(&ed, "QuestEdit+basic");
    ed.quest_edit_tab = QUEST_TAB_MONSTERS;
    check(&ed, "QuestEdit+monsters");
    ed.screen = Screen::Convert;
    check(&ed, "Convert");
    ed.screen = Screen::SaveMap;
    check(&ed, "SaveMap");
    ed.input(Key::A);
    check(&ed, "SaveMap+edit");
    ed.map_edit = None;
    ed.screen = Screen::Review;
    check(&ed, "Review");
    out
}

#[test]
fn every_footer_fits_the_strip_it_is_drawn_in() {
    // The strip is a property of the geometry, not a number somebody typed.
    let longest_undo = "[タップ2回 / L 2回長押し] 元に戻す（0/4）";
    assert_eq!(footer_undo_w(longest_undo), 377);
    assert_eq!(footer_hint_room(longest_undo), FOOTER_HINT_ROOM);
    assert_eq!(FOOTER_HINT_ROOM, 763);

    let table = footer_table();
    assert!(table.len() >= 40, "the table covers every screen: {}", table.len());
    let over: Vec<&(String, i32)> = table.iter().filter(|(_, w)| *w > FOOTER_HINT_ROOM).collect();
    assert!(
        over.is_empty(),
        "{} of {} guides are wider than {FOOTER_HINT_ROOM}px: {over:?}",
        over.len(),
        table.len()
    );
}

/// Prints the table the review asked for. Run with `--nocapture`.
#[test]
fn the_footer_width_table() {
    let table = footer_table();
    println!("{} footers, strip = {FOOTER_HINT_ROOM}px", table.len());
    let mut worst = 0;
    for (line, w) in &table {
        worst = worst.max(*w);
        println!("{line}");
    }
    println!("widest = {worst}px of {FOOTER_HINT_ROOM}px");
}

/// The one item that must never be the one that is cut.
#[test]
fn the_way_out_is_on_every_guide_that_has_one() {
    let mut ed = open_blank();
    for screen in [
        Screen::Slots,
        Screen::Char,
        Screen::Items,
        Screen::Charms,
        Screen::Equip,
        Screen::EquipEdit,
        Screen::Sets,
        Screen::Hunts,
        Screen::Progress,
        Screen::SaveQuests,
        Screen::ArcList,
        Screen::ArcEntries,
        Screen::Mib,
        Screen::Gmd,
        Screen::EntryView,
        Screen::Check,
        Screen::QuestEquip,
        Screen::Convert,
        Screen::SaveMap,
        Screen::Review,
        Screen::Roster,
        Screen::Palico,
        Screen::Options,
        Screen::Csv,
    ] {
        ed.screen = screen;
        let hint = ed.footer_hint();
        assert!(
            hint.contains("[B] "),
            "{screen:?} does not name the way back: {hint}"
        );
    }
    // A form's draft costs two presses and the guide says so.
    ed.screen = Screen::Sets;
    assert!(ed.open_set_editor(0), "{}", ed.message);
    assert!(
        ed.footer_hint().contains("[B] 一覧（下書きは2回）"),
        "{}",
        ed.footer_hint()
    );
}

// ---------------------------------------------------------------------------
// 4: the equipment strip keeps the warning colour for warnings
// ---------------------------------------------------------------------------

/// The equipment box's permanent operating note was drawn in the warning colour
/// in the warning position, and it took both lines - so a record that does not
/// fit its own type had nowhere to be said.
#[test]
fn the_equipment_strip_keeps_the_warning_colour_for_warnings() {
    let src = ui_src();
    let body = fn_body(src, "fn draw_adaptive_rows_warn");
    assert!(body.contains("notice: &[String]"), "the strip takes two lists");
    assert!(body.contains("warnings: &[String]"));
    // Warnings are drawn in the warning colour, the explanation is not.
    assert!(
        body.contains("if *is_warning { t.accent_warn } else { t.text_secondary }"),
        "{body}"
    );
    // A warning is never pushed off by the explanation.
    assert!(body.contains("warnings.len().min(2) + notice.len().min(1)"), "{body}");
    // The equipment box passes warnings only: its permanent operating note was
    // the third review's finding 10 (a strip that is always drawn costs the
    // list a row), so the keys it named are the footer's job now.
    let equip = fn_body(src, "fn draw_equip");
    assert!(!equip.contains("notice.push"), "{equip}");
    assert!(equip.contains("warnings.push"), "{equip}");
    // The new-record form still has something to say (the last [A] creates the
    // record), and it says it as a notice, not as a warning.
    let edit = fn_body(src, "fn draw_equip_edit");
    assert!(edit.contains("&notice,"), "{edit}");
    assert!(edit.contains("&warnings,"), "{edit}");
}

// ---------------------------------------------------------------------------
// 5: the save review body is the row font
// ---------------------------------------------------------------------------

/// The dialog that exists to be read before a write was drawn at 17px, the
/// smallest type on the screen.
#[test]
fn the_save_review_body_is_the_row_font() {
    let body = fn_body(ui_src(), "fn draw_review");
    assert!(body.contains("const ROW: i32 = 28"), "{body}");
    assert!(body.contains("const GLYPH: i32 = 22"), "{body}");
    assert!(
        body.contains("c.text_clipped(mx + 30, y, mw - 60, line, color, FontSize::Medium)"),
        "{body}"
    );
}

// ---------------------------------------------------------------------------
// 6: the item box's search has a pad key, and both screens say the way back
// ---------------------------------------------------------------------------

/// The item box's search was a pill only ([Y] is 空きも表示 and every other key
/// is spoken for), and the talisman list's footer never named the way back.
///
/// The search has a key again, one press further in: [ZR] opens the bulk menu
/// (the review's finding was that the one screen whose whole point is bulk
/// edits could not reach them from the pad), and the search is a row of that
/// menu. The key is still the state machine's and the footer still names it.
#[test]
fn the_item_box_search_is_a_key_and_both_footers_name_the_way_back() {
    let mut ed = open_blank();
    ed.screen = Screen::Items;
    assert_eq!(ed.zr_job(), ZrJob::Screen);
    // The list's own line names the key; the menu has a line of its own.
    let hint = ed.footer_hint();
    assert!(hint.contains("[ZR] 一括"), "{hint}");
    assert!(hint.contains("[B] 戻る"), "{hint}");
    ed.input(Key::Z);
    assert!(ed.item_menu_open, "[ZR] opens the bulk menu: {}", ed.message);
    // The search is one of its rows, and it is the row that asks for a name.
    ed.item_menu_cursor = ITEM_OP_FIND;
    ed.input(Key::A);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_ITEM_BOX_FILTER),
        "the menu's 名前で探す row is the search"
    );

    // The talisman list already answered [Y]; what it did not do was name the
    // way back. It names it now, and by the screen it goes to: the fourth
    // review found that [B] here is the character select, not the screen the
    // user came from.
    ed.screen = Screen::Charms;
    ed.input(Key::Y);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_CHARM_FILTER),
        "the talisman search is a key and always was"
    );
    let hint = ed.footer_hint();
    assert!(hint.contains("[Y] 検索"), "{hint}");
    assert!(hint.contains("[B] 枠選択"), "{hint}");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots, "and the word is true");
}

// ---------------------------------------------------------------------------
// 7: the quest 基本 tab's button follows the row
// ---------------------------------------------------------------------------

/// The button said "テキスト編集" on all four rows, so pressing it on 制限時間
/// promised a keyboard and answered with a sentence.
#[test]
fn the_quest_basic_button_follows_the_row() {
    let mut ed = Editor::new();
    ed.screen = Screen::QuestEdit;
    ed.quest_edit_tab = QUEST_TAB_BASIC;
    ed.gmd = Some(mhnx_core::quest::Gmd {
        version: mhnx_core::quest::GMD_VERSION,
        tag: mhnx_core::quest::GMD_TAG,
        key: String::from("questData_1010165"),
        strings: vec![String::from("名前"), String::from("依頼主")],
    });
    // A .gmd row: [A] opens the keyboard.
    ed.quest_edit_cursor = 0;
    assert_eq!(ed.quest_basic_a_label(), "テキスト編集");
    // The .mib time limit: there is no text to edit and [A] says so.
    ed.quest_edit_cursor = ed.quest_value_rows() - 1;
    assert_eq!(ed.quest_basic_a_label(), "増減は [◀/▶]");
    // There is no 詳細 row any more: every tab is its values, and the file
    // screens are the [ファイル] pill / [R] held (the second review's 13). A
    // cursor past the end clamps to the last value row, which is the time limit.
    ed.quest_edit_cursor = ed.quest_value_rows();
    assert_eq!(ed.quest_row_count(), ed.quest_value_rows());
    assert_eq!(
        ed.quest_basic_a_label(),
        "増減は [◀/▶]",
        "the last row is the time limit, not a button"
    );
    // The footer follows the row as well: a text row names [A], the time row
    // names the arrows, and neither key is promised on a row that refuses it.
    ed.quest_edit_cursor = 0;
    let hint = ed.footer_hint();
    assert!(hint.contains("[A] テキスト編集"), "{hint}");
    assert!(!hint.contains("[◀/▶] 制限時間"), "{hint}");
    ed.quest_edit_cursor = ed.quest_value_rows() - 1;
    let hint = ed.footer_hint();
    assert!(hint.contains("[◀/▶] 制限時間"), "{hint}");
    assert!(!hint.contains("[A] テキスト編集"), "{hint}");
    // And the console draws the answer (ui.rs draw_quest_edit).
    let body = fn_body(ui_src(), "fn draw_quest_edit");
    assert!(body.contains("ed.quest_basic_a_label()"), "{body}");
    assert!(!body.contains("\"テキスト編集\""), "{body}");
}

// ---------------------------------------------------------------------------
// 9: the conversion asks before it writes, and speaks Japanese
// ---------------------------------------------------------------------------

/// [A] built the converted save and handed it to the I/O layer on one press,
/// with the news delivered afterwards in English.
#[test]
fn the_conversion_asks_before_it_writes() {
    let mut ed = open_blank();
    ed.screen = Screen::Convert;
    let path = ed.convert_output_path().expect("a path for the open save");
    assert_eq!(ed.take_write(), None);

    // The first press only asks.
    ed.input(Key::A);
    assert_eq!(ed.take_write(), None, "the first press must not write");
    assert!(ed.message.contains("もう一度 [A]"), "{}", ed.message);
    assert!(ed.message.contains(&path), "{}", ed.message);
    assert!(ed.message.contains("上書き"), "{}", ed.message);

    // Any other key takes the offer back.
    ed.input(Key::Left);
    ed.input(Key::A);
    assert_eq!(ed.take_write(), None, "the offer was taken back");

    // The second press writes, and says so in Japanese.
    ed.input(Key::A);
    let write = ed.take_write().expect("the second press writes");
    match write {
        mhnx_core::hbedit::Write::File { path: written, .. } => assert_eq!(written, path),
    }
    assert!(ed.message.contains("書き出しました"), "{}", ed.message);
    assert!(
        !ed.message.contains("writing") && !ed.message.contains("header"),
        "the status line is Japanese: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// 10: a .gmd paragraph keeps its line breaks
// ---------------------------------------------------------------------------

/// The 基本 tab's text rows are .gmd strings, and a .gmd string is a paragraph.
/// The console edited one with the *system* keyboard, which is single line, so
/// opening a two-line 依頼内容 and pressing 決定 wrote it back as one line.
#[test]
fn a_quest_text_field_keeps_its_line_breaks() {
    // The field accepts the break and stores it.
    let spec = mhnx_core::hbedit::NameSpec::name(512, mhnx_core::save::NameUnit::Bytes);
    assert!(spec.multiline);
    assert!(spec.accepts('\n'));
    assert_eq!(
        mhnx_core::hbedit::keyboard_route(spec, true),
        mhnx_core::hbedit::KeyboardRoute::Builtin,
        "a multiline field needs the keyboard with the line-break key"
    );
    // The writer keeps what it is given.
    let mut ed = Editor::new();
    ed.gmd = Some(mhnx_core::quest::Gmd {
        version: mhnx_core::quest::GMD_VERSION,
        tag: mhnx_core::quest::GMD_TAG,
        key: String::from("questData_1010165"),
        strings: vec![String::from("名前")],
    });
    ed.apply_text(1, String::from("一行目\n二行目"));
    assert_eq!(
        ed.gmd.as_ref().expect("gmd").strings[1],
        "一行目\n二行目",
        "the break is the player's"
    );
    // And the console asks for that keyboard. The field is the state machine's
    // now (hbedit::Editor::text_request_spec, the third review's finding 1):
    // a .gmd line is a paragraph there, and the console hands the answer to
    // hbedit::keyboard_route without deciding anything itself.
    let core = include_str!("../src/hbedit.rs");
    assert!(
        core.contains("None => NameSpec::name(GMD_TEXT_MAX, crate::save::NameUnit::Bytes)"),
        "the .gmd rows must not use NameSpec::singleline"
    );
    let src = editor_src();
    assert!(src.contains("let spec = ed.text_request_spec(idx);"), "{src}");
    assert!(
        src.contains("hbedit::keyboard_route(spec, crate::swkbd::is_available())"),
        "{src}"
    );
}

// ---------------------------------------------------------------------------
// 11: the tap rule says what a tap does
// ---------------------------------------------------------------------------

/// "1回=選択・2回=決定" was wrong about most of the screen: every button runs on
/// one tap and only a row that is not already selected needs the second one.
#[test]
fn the_tap_rule_says_what_a_tap_does() {
    let body = fn_body(ui_src(), "fn draw_footer");
    assert!(!body.contains("2回=決定"), "{body}");
    assert!(body.contains("ボタン=即実行"), "{body}");
    assert!(body.contains("行=選択→再タップ"), "{body}");
}

// ---------------------------------------------------------------------------
// 12: [-] is the delete key and nothing else
// ---------------------------------------------------------------------------

/// [-] carried nine meanings, one of which was 削除. A key that deletes a record
/// on one screen and opens a menu on the next is a key nobody can learn.
#[test]
fn minus_is_the_delete_key_and_nothing_else() {
    let mut ed = open_blank();
    // The screens that can delete keep it, and it is the only thing it does
    // there: with nothing under the cursor the answer is "pick a record", never
    // a second job.
    ed.screen = Screen::Charms;
    ed.input(Key::Minus);
    assert!(ed.message.contains("削除する護石"), "{}", ed.message);
    assert_eq!(ed.screen, Screen::Charms, "[-] never leaves the screen");

    // The screens that used to give it a second job answer [ZR] for that job
    // and nothing for [-].
    let mut ed = open_blank();
    ed.screen = Screen::Items;
    ed.input(Key::Minus);
    assert!(!ed.item_menu_open, "[-] does not open the bulk menu any more");
    ed.input(Key::Z);
    assert!(ed.item_menu_open, "[ZR] is the bulk menu");
    // The search it used to be is a row of that menu.
    ed.item_menu_cursor = ITEM_OP_FIND;
    ed.input(Key::A);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_ITEM_BOX_FILTER),
        "[ZR] then 名前で探す is the search"
    );
    ed.screen = Screen::Hunts;
    let before = ed.hunt_show_all;
    ed.input(Key::Minus);
    assert_eq!(ed.hunt_show_all, before, "[-] is not the list toggle any more");
    ed.input(Key::Z);
    assert_ne!(ed.hunt_show_all, before, "[ZR] is");

    // The save list does not rescan on it either: [Y] 再読込 is the key.
    ed.screen = Screen::SaveList;
    assert!(!ed.minus_rescans());
    // The console's dispatch asks the state machine where ZR belongs.
    assert!(editor_src().contains("mhnx_core::hbedit::ZrJob::Screen"));
    // And every line that names [-] names it as the delete. The quest list's
    // footer names [ZR] 一括 instead (the strip has 763px and one of the two
    // had to go - the delete is drawn on the deck's own button and named by the
    // guide), so it is checked through those two routes.
    for screen in [Screen::Charms, Screen::Equip] {
        ed.screen = screen;
        let hint = ed.footer_hint();
        assert!(hint.contains("[-] 削除（2回）"), "{screen:?}: {hint}");
    }
    ed.screen = Screen::SaveQuests;
    ed.set_quest_tab(0);
    if Editor::bundled_quests_available() {
        let guide = ed.screen_guide();
        assert!(guide.contains("[-] で削除（2回）"), "{guide}");
    }
    assert!(
        ui_src().contains("[-] 選択中のクエストを削除（2回）"),
        "the deck's button names the delete"
    );
}

// ---------------------------------------------------------------------------
// 13: the guide line is the row font
// ---------------------------------------------------------------------------

/// The one sentence a first-time player reads was 17px - smaller than the row
/// values it explains.
#[test]
fn the_guide_line_is_the_row_font() {
    let src = ui_src();
    assert!(src.contains("const GUIDE_H: i32 = 28;"), "the strip is 28px tall");
    assert!(src.contains("const CONTENT_Y: i32 = GUIDE_Y + GUIDE_H; // 120"));
    let body = fn_body(src, "fn draw_guide");
    assert!(
        body.contains("FontSize::Medium"),
        "the guide is the row font: {body}"
    );
    // The six pixels the strip took are given back by the list's own padding:
    // nine rows again when no warning strip is drawn (the second review's 15).
    assert!(
        src.contains("nine rows"),
        "the trade is written down where the constant is"
    );
    assert!(
        body.contains("guide_tail"),
        "a long guide keeps its last items: {body}"
    );
}
