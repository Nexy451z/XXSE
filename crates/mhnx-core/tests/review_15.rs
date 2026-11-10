//! The sixteen findings of the fifth review, pinned one by one.
//!
//! Three groups: writes that happened when nothing was on screen to write (A),
//! regressions the previous round of fixes introduced (B), and keys or states
//! that were left behind (C, D, E). Each test names the function the finding was
//! about, so a regression points at the code rather than at the screen.
//!
//! Four of them read switch/src/ui.rs and switch/src/editor.rs as text: the
//! console's dispatch cannot be driven from a host test, so what is pinned there
//! is the wiring (the console uses the state machine's answer) rather than the
//! pixels.
//!
//! Nothing here writes a card: the saves and quests are built in memory.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::confirm::{HoldAction, HOLD_CONFIRM_FRAMES};
use mhnx_core::hbedit::{
    Editor, Key, Screen, Shoulder, ShoulderTap, ZrJob, ITEM_OP_FIND, ITEM_OP_UNDO,
    PROGRESS_QUESTS, PROGRESS_REQUESTS, QUEST_TAB_REWARDS,
};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};
use mhnx_core::save::{Save, BODY_SIZE, SLOT_STRIDE};

/// Body offset of the first character slot (also stored at body[0x10]).
const FIRST_SLOT_BASE: u32 = 0x126474;

const HASH_REM: u32 = 0x5B3C_302D;
const HASH_MIB: u32 = 0x1BBF_D18E;
const HASH_GMD: u32 = 0x242B_B29A;

// ---------------------------------------------------------------------------
// The two layers, as text
// ---------------------------------------------------------------------------

fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
}

fn editor_src() -> &'static str {
    include_str!("../../../switch/src/editor.rs")
}

/// The body of one function, from its declaration to the next item.
fn fn_body<'a>(src: &'a str, decl: &str) -> &'a str {
    let start = src
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} is not in the console sources"));
    let rest = &src[start + decl.len()..];
    let mut end = rest.len();
    for pat in ["\n    fn ", "\n    pub fn ", "\n    #[allow("] {
        if let Some(i) = rest.find(pat) {
            end = end.min(i);
        }
    }
    &rest[..end]
}

// ---------------------------------------------------------------------------
// Saves and quests, built in memory
// ---------------------------------------------------------------------------

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
fn open_blank() -> Editor {
    let save = Save::from_bytes(&blank_body()).expect("the synthetic save must parse");
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.save_path = Some(String::from("memory:review-15"));
    ed.slot = 0;
    ed.screen = Screen::Slots;
    ed
}

fn mib_bytes(id: u32, minutes: u8) -> Vec<u8> {
    let mut mib = vec![0u8; MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
    mib[0x16] = minutes;
    for (k, (mid, count)) in [(85u16, 1u8), (32, 2)].iter().enumerate() {
        let o = 100 + 13 * k;
        mib[o..o + 2].copy_from_slice(&mid.to_le_bytes());
        mib[o + 3] = 1;
        mib[o + 4] = *count;
        mib[o + 5] = 50;
        mib[o + 6] = 50;
        mib[o + 7] = 50;
        mib[o + 9..o + 11].copy_from_slice(&100u16.to_le_bytes());
        mib[o + 12] = 1;
    }
    mib
}

fn gmd_bytes() -> Vec<u8> {
    Gmd {
        version: GMD_VERSION,
        tag: GMD_TAG,
        key: String::from("questData_1010165"),
        strings: vec![
            String::from("テストクエスト"),
            String::from("受付嬢"),
            String::from("依頼の内容"),
            String::from("リオレウス"),
            String::from("リオレウス1頭の狩猟"),
            String::from("3回力尽きる"),
            String::from("サブ依頼"),
        ],
    }
    .to_bytes()
}

/// 184 byte reward table: one group of three rows.
fn rem_bytes() -> Vec<u8> {
    let mut v = vec![0u8; 184];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8] = 1;
    v[9] = 3;
    let rows: [[u8; 4]; 4] = [
        [0x63, 0x01, 1, 50],
        [0x55, 0x03, 2, 30],
        [0xB4, 0x06, 1, 20],
        [0, 0, 0, 0xFF],
    ];
    for (i, r) in rows.iter().enumerate() {
        v[24 + i * 4..24 + i * 4 + 4].copy_from_slice(r);
    }
    v
}

fn entry(name: &str, type_hash: u32, data: Vec<u8>) -> ArcEntry {
    ArcEntry {
        name: String::from(name),
        type_hash,
        data,
        original: None,
        original_offset: None,
    }
}

/// A whole quest, in the shape q1010165 has.
fn quest_arc(id: u32, minutes: u8) -> Arc {
    Arc {
        version: ARC_VERSION,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry("quest\\rem\\rem_000000", HASH_REM, vec![0u8; 184]),
            entry("quest\\rem\\rem_620114", HASH_REM, rem_bytes()),
            entry(
                "quest\\questData\\questData_1010165_jpn",
                HASH_GMD,
                gmd_bytes(),
            ),
            entry(
                "quest\\questData\\questData_1010165",
                HASH_MIB,
                mib_bytes(id, minutes),
            ),
        ],
    }
}

/// Open a quest through the byte path the console uses.
fn open_quest(id: u32, minutes: u8) -> Editor {
    let mut ed = Editor::new();
    let bytes = quest_arc(id, minutes).to_bytes().expect("the archive rebuilds");
    ed.open_arc_bytes("sdmc:/quest/q1010165.arc", &bytes);
    ed.enter_quest_edit();
    assert_eq!(ed.screen, Screen::QuestEdit, "{}", ed.message);
    ed
}

// ---------------------------------------------------------------------------
// A1: a progress search with no matches writes nothing
// ---------------------------------------------------------------------------

/// progress_cursor falls back to the unfiltered row when the search matches
/// nothing, and progress_toggle wrote that row: [A] on a list that said "no
/// matches" flipped the clear bit of a quest nobody could see.
#[test]
fn a_progress_search_with_no_matches_writes_nothing() {
    let mut ed = open_blank();
    ed.screen = Screen::Progress;
    ed.progress_tab = PROGRESS_QUESTS;
    ed.set_progress_cursor(0);
    let before = ed.save.as_ref().expect("a save").body().to_vec();

    ed.apply_progress_filter(String::from("qqq"));
    assert_eq!(ed.progress_visible_len(), 0, "the search matched nothing");
    assert!(
        ed.progress_row_under_cursor().is_none(),
        "an empty list has no row under the cursor"
    );

    ed.input(Key::A);
    assert!(!ed.dirty, "[A] on an empty list must not mark the save dirty");
    assert_eq!(
        ed.save.as_ref().expect("a save").body(),
        &before[..],
        "a press on an empty list must not move a byte"
    );
    assert!(
        ed.message.contains("一致する行がありません"),
        "the press has to say why it did nothing: {}",
        ed.message
    );

    // [Y] is the second writer on that view and follows the same rule.
    ed.input(Key::Y);
    assert!(!ed.dirty);
    assert_eq!(ed.save.as_ref().expect("a save").body(), &before[..]);

    // The unfiltered list still writes: the guard is the empty list, not [A].
    ed.apply_progress_filter(String::new());
    ed.input(Key::A);
    assert!(ed.dirty, "the bit is written when the row is on screen");
}

// ---------------------------------------------------------------------------
// A2: a talisman search with no matches does not create a record
// ---------------------------------------------------------------------------

/// The console asked charms().is_empty() - the *filtered* list - and sent [L]
/// (create), so a search with no hits started a brand new talisman.
#[test]
fn a_charm_search_with_no_matches_does_not_create_a_record() {
    let mut ed = open_blank();
    ed.open_charms();
    ed.apply_charm_filter(String::from("qqq"));
    assert!(ed.charms().is_empty(), "the search matched nothing");

    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::Charms, "the list stays where it is");
    assert!(ed.charm_edit.is_none(), "no form may open");
    assert!(
        ed.message.contains("一致する護石がありません"),
        "{}",
        ed.message
    );

    // An empty box still creates: that is what the empty list's own line says.
    ed.apply_charm_filter(String::new());
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    assert!(ed.charm_edit.is_some());

    // The console asks the state machine instead of deciding for itself.
    let body = fn_body(ui_src(), "pub fn handle_pad_key");
    assert!(body.contains("ed.charm_a_press()"), "{body}");
    assert!(
        !body.contains("ed.charms().is_empty()"),
        "the console must not branch on the filtered list: {body}"
    );
}

// ---------------------------------------------------------------------------
// B3: a held [R] does not run its tap first
// ---------------------------------------------------------------------------

/// R's tap used to be answered on the press, so holding R to read 詳細 also
/// moved the sub-tab: the screen changed under the hold.
#[test]
fn a_held_shoulder_does_not_run_its_tap_first() {
    // A short press owes its tap, and the tap arrives on the release.
    let mut tap = ShoulderTap::new();
    assert!(tap.press(), "the press starts the hold");
    assert_eq!(tap.tick(), HoldAction::None);
    assert!(tap.release(), "a short press owes its tap on the release");

    // A hold is not also a tap.
    let mut tap = ShoulderTap::new();
    assert!(tap.press());
    let mut last = HoldAction::None;
    for _ in 0..HOLD_CONFIRM_FRAMES {
        last = tap.tick();
    }
    assert_eq!(last, HoldAction::Arm);
    assert!(!tap.release(), "the hold opened 詳細, so it owes no tap");

    // A press answered on the press itself (a pick list) owes nothing.
    let mut tap = ShoulderTap::new();
    assert!(tap.press());
    tap.spend();
    assert!(!tap.release());

    // A repeat frame is not a new press, and another key cancels the tap.
    let mut tap = ShoulderTap::new();
    assert!(tap.press());
    assert!(!tap.press());
    tap.cancel();
    assert!(!tap.release());

    // The console defers R and keeps the screen it went down on.
    let run = fn_body(editor_src(), "pub fn run");
    assert!(run.contains("r_tap.press()"), "the console uses the state machine");
    assert!(
        run.contains("r_target = Some(ed.detail_target("),
        "the detail target is captured on the press"
    );
    assert!(
        run.contains("if r_tap.release()"),
        "the tap runs on the release"
    );
    assert!(
        !run.contains("if down & BTN_R != 0 && r_press.press()"),
        "R must not be answered on the press any more"
    );
}

/// 詳細 opens on the screen the key went down on, not on the release frame.
#[test]
fn the_detail_target_is_the_screen_the_key_went_down_on() {
    let mut ed = open_blank();
    ed.screen = Screen::Progress;
    ed.progress_tab = PROGRESS_REQUESTS;
    let target = ed.detail_target(0);
    assert_eq!(target.screen, Screen::Progress);
    assert_eq!(target.progress_view, PROGRESS_REQUESTS);

    let requests = ed.detail_row_of(target);
    assert_eq!(
        requests,
        Editor::save_map_row_named("依頼フラグ", 0),
        "the snapshot resolves the 依頼 view's row"
    );
    // The live view moves on; the snapshot does not.
    ed.progress_tab = PROGRESS_QUESTS;
    assert_eq!(ed.detail_row_of(target), requests);
    assert_ne!(
        ed.detail_row_of(ed.detail_target(0)),
        requests,
        "a fresh target follows the view that is showing"
    );
    // マイセット's two arrays travel with the console's tab.
    ed.screen = Screen::Sets;
    assert_eq!(
        ed.detail_row_of(ed.detail_target(1)),
        Editor::save_map_row_named("装備マイセット", 0)
    );
    // The console asks the state machine for the row.
    let body = fn_body(ui_src(), "pub fn open_detail_at");
    assert!(body.contains("ed.detail_row_of(target)"), "{body}");
}

// ---------------------------------------------------------------------------
// B4: [X] saves the draft, it does not close the form
// ---------------------------------------------------------------------------

/// The commit was [A]'s, and [A] leaves for the list: saving threw the user off
/// the form they were editing.
#[test]
fn the_save_key_does_not_close_the_talisman_form() {
    let mut ed = open_blank();
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(!ed.charm_draft_dirty(), "a fresh form holds no draft");
    ed.take_write();

    ed.quick_save();
    assert_eq!(
        ed.screen,
        Screen::CharmEdit,
        "the save key must not close the form: {}",
        ed.message
    );
    assert!(ed.charm_edit.is_some(), "the form is still the one on screen");
    // A new talisman that [A] has not written is not in the box: the file
    // still goes out, and the line says what is missing (the fifth review's
    // #8: it said 保存しました about a talisman that was never added).
    assert!(ed.take_write().is_some(), "the save still goes out");
    assert!(
        ed.message.contains("まだ書き込まれていません"),
        "the save key must say what is missing: {}",
        ed.message
    );

    // [A] on the write row: a write that cannot land (this box holds no
    // talisman to copy a shape from) keeps the form and says why, rather than
    // closing on a record that was never written.
    while ed.charm_edit.map(|e| e.field) != Some(0) {
        ed.input(Key::Up);
    }
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    assert!(ed.charm_edit.is_some(), "a refused write keeps the draft");
}

// ---------------------------------------------------------------------------
// B5: the item box's bulk menu is a pad key, and its undo row is reachable
// ---------------------------------------------------------------------------

/// [-] was made the delete key and the menu lost its key with it: the one
/// screen whose whole point is bulk edits could not open them from the pad.
#[test]
fn the_item_box_bulk_menu_is_a_pad_key_and_its_undo_row_is_reachable() {
    let mut ed = open_blank();
    ed.screen = Screen::Items;
    assert_eq!(ed.zr_job(), ZrJob::Screen, "the console hands ZR to the screen");

    // The list's own line names the key; the menu has its own line.
    let hint = ed.footer_hint();
    assert!(hint.contains("[ZR] 一括"), "the footer names the key: {hint}");
    ed.input(Key::Z);
    assert!(ed.item_menu_open, "[ZR] opens the bulk menu: {}", ed.message);

    // 元に戻す is a row of that menu and the pad can reach it.
    ed.item_menu_cursor = ITEM_OP_UNDO;
    ed.input(Key::A);
    assert!(
        ed.message.contains("取り消せる操作がありません"),
        "the undo row runs: {}",
        ed.message
    );

    // The search it used to be is a row too, so nothing was lost.
    ed.item_menu_cursor = ITEM_OP_FIND;
    ed.input(Key::A);
    assert!(
        ed.take_text_request().is_some(),
        "the menu's 名前で探す row is the search"
    );

    // [ZR] again closes what it opened.
    ed.input(Key::Z);
    assert!(!ed.item_menu_open, "[ZR] is a toggle on its own menu");

    // [-] still opens nothing: the delete key did not take the menu back.
    ed.input(Key::Minus);
    assert!(!ed.item_menu_open);
}

// ---------------------------------------------------------------------------
// B6: the header button does not call itself ZR
// ---------------------------------------------------------------------------

/// The header's ZR button cycled the sections while the pad's ZR is the
/// screen's own second action on a dozen screens: one label, two meanings.
#[test]
fn the_header_button_does_not_call_itself_zr() {
    let body = fn_body(ui_src(), "fn draw_top_nav");
    // The comments explain what the button used to be called, so only the code
    // is searched for the label.
    let code: String = body
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("\"ZR\""),
        "the header button must not claim the ZR key: {code}"
    );
    assert!(
        body.contains("セクション送り"),
        "it says what the tap does: {body}"
    );
    // [ZL] keeps its name: the pad really delivers ZL to the section bar.
    assert!(body.contains("\"ZL\""), "{body}");
    // The badge starts after the button, whatever width the label needs.
    assert!(body.contains("let slot_x = zr_x + zr_w + 6;"), "{body}");
}

// ---------------------------------------------------------------------------
// B7: the item set's ポーチへ適用 is a pad key
// ---------------------------------------------------------------------------

/// The copy into the pouch was the deck's pill alone: a finger could press it
/// and the pad could not.
#[test]
fn the_item_set_form_can_copy_to_the_pouch_from_the_pad() {
    let mut ed = open_blank();
    ed.screen = Screen::Sets;
    assert!(ed.open_item_set_editor(0), "{}", ed.message);
    assert!(ed.item_set_editor_open());

    assert_eq!(
        ed.shoulder_job(Key::R),
        Shoulder::Screen,
        "the form's [R] is its own key"
    );
    assert_eq!(ed.shoulder_job(Key::L), Shoulder::SubTab, "[L] keeps the sub-tab");

    let hint = ed.footer_hint();
    assert!(hint.contains("[R] ポーチへ"), "the footer names the key: {hint}");

    // Two presses, like every other replace: the first one only offers it.
    ed.input(Key::R);
    assert!(
        ed.message.contains("もう一度 [R]"),
        "the first press asks: {}",
        ed.message
    );
    ed.input(Key::R);
    assert!(
        !ed.message.contains("もう一度 [R]"),
        "the second press runs it instead of asking again: {}",
        ed.message
    );
    assert!(
        ed.message.contains("適用") || ed.message.contains("ポーチ"),
        "and it says what happened: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// C8: the talisman form's [B] closes the form
// ---------------------------------------------------------------------------

/// [B] changed the screen and the tail of the handler put charm_edit back, so
/// the list came back with a form still attached to it.
#[test]
fn the_talisman_form_closes_when_b_closes_it() {
    let mut ed = open_blank();
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);

    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms);
    assert!(
        ed.charm_edit.is_none(),
        "[B] closes the form, not just the screen"
    );

    // The equipment form has always done this; the two agree now.
    ed.screen = Screen::Equip;
    ed.input(Key::Plus); // the new-record form
    assert!(ed.equip_edit.is_some());
    ed.input(Key::B);
    assert!(ed.equip_edit.is_none(), "the equipment form's [B] clears its own");
}

// ---------------------------------------------------------------------------
// C9 + C11: the item picker comes back where it was opened from
// ---------------------------------------------------------------------------

/// The console's quest_pick_return flag was never cleared when the pick was
/// cancelled, so the next .rem pick jumped to クエスト改造 - and a cancelled
/// reward pick landed on the file screen.
#[test]
fn the_item_picker_comes_back_to_the_screen_it_was_opened_from() {
    // From クエスト改造's 報酬 tab: [B] cancels back to the aggregate screen.
    let mut ed = open_quest(1_010_165, 50);
    ed.enter_quest_edit_from(Screen::SaveQuests);
    ed.quest_edit_tab = QUEST_TAB_REWARDS;
    ed.quest_edit_cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "{}", ed.message);
    assert_eq!(ed.item_pick_return, Screen::QuestEdit);
    ed.input(Key::B);
    assert_eq!(
        ed.screen,
        Screen::QuestEdit,
        "[B] cancels back to the screen that opened the picker"
    );

    // From the .rem file screen: the same key comes back there, and the flag
    // the console used to keep cannot be left behind.
    let rem = ed
        .arc
        .as_ref()
        .expect("an archive")
        .entries
        .iter()
        .position(|e| e.name.ends_with("rem_620114"))
        .expect("the reward table");
    ed.screen = Screen::ArcEntries;
    ed.cursor = rem;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    ed.entry_row_cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);
    assert_eq!(ed.item_pick_return, Screen::EntryView);
    ed.input(Key::B);
    assert_eq!(
        ed.screen,
        Screen::EntryView,
        "a cancelled .rem pick must not jump to クエスト改造"
    );

    // The console keeps no flag of its own any more.
    let ui = ui_src();
    assert!(!ui.contains("quest_pick_return"), "the console's flag is gone");
}

// ---------------------------------------------------------------------------
// C10: a quest opened from the card comes back to the card list
// ---------------------------------------------------------------------------

/// Request::ReadArc opened the aggregate screen with the bare enter_quest_edit,
/// which keeps whatever the last quest set: a quest opened from the SD list
/// came back to セーブ内.
#[test]
fn a_quest_opened_from_the_card_comes_back_to_the_card_list() {
    let mut ed = open_quest(1_010_165, 50);
    ed.enter_quest_edit_from(Screen::SaveQuests);
    assert_eq!(ed.quest_return, Screen::SaveQuests);
    // The console's ReadArc handler passes the list the quest came from.
    ed.enter_quest_edit_from(Screen::ArcList);
    assert_eq!(ed.quest_return, Screen::ArcList);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcList);

    // The ReadArc answer is in the I/O layer (service), not in the loop.
    assert!(
        editor_src().contains("ed.enter_quest_edit_from(mhnx_core::hbedit::Screen::ArcList)"),
        "the ReadArc answer has to name the list it opened"
    );
}

// ---------------------------------------------------------------------------
// D12: the .mib's [ZR] says why it does nothing off a monster row
// ---------------------------------------------------------------------------

/// The footer promised [ZR] 項目 on every row; the key only had a job on the
/// monster slots and did nothing at all on the other ninety.
#[test]
fn the_mib_zr_says_why_it_does_nothing_off_a_monster_row() {
    let mut ed = open_quest(1_010_165, 50);
    ed.screen = Screen::Mib;
    ed.mib_cursor = 0; // 予約: not a monster slot
    assert!(
        !mhnx_core::questfields::MIB_FIELDS[0]
            .key
            .starts_with("mob_slot["),
        "row 0 is not a monster slot"
    );
    ed.input(Key::Z);
    assert!(ed.message.contains("[ZR]"), "{}", ed.message);
    assert!(
        ed.message.contains("モンスター行"),
        "the press names the rows the key does work on: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// D13: the palico skill panel answers [-] and [ZR] with a reason
// ---------------------------------------------------------------------------

/// Both keys were swallowed in silence, which reads as a broken panel.
#[test]
fn the_palico_skill_panel_says_what_minus_and_zr_do() {
    let mut ed = open_blank();
    ed.screen = Screen::Roster;
    ed.palico_skill = Some(mhnx_core::hbedit::PalicoSkill {
        palico: 0,
        tab: 0,
        field: 0,
        picks: [0u8; mhnx_core::hbedit::PALICO_MOVE_ROWS],
    });

    ed.input(Key::Minus);
    assert!(
        ed.message.contains("削除できるものがありません"),
        "[-] says why it does nothing: {}",
        ed.message
    );
    ed.input(Key::Z);
    assert!(
        ed.message.contains("[ZR]"),
        "[ZR] says what the panel's keys are: {}",
        ed.message
    );
    assert!(ed.palico_skill.is_some(), "neither key may close the panel");
}

// ---------------------------------------------------------------------------
// D14: the 性別 row's [A] says what it did
// ---------------------------------------------------------------------------

/// The one row whose [A] is neither a MAX nor a keyboard flipped the byte and
/// said nothing, which reads as a key that did nothing.
#[test]
fn the_gender_row_says_what_a_did() {
    let mut ed = open_blank();
    ed.screen = Screen::Char;
    ed.field = 13;
    let before = ed.char_value(13);
    ed.char_gender_flip();
    assert_ne!(ed.char_value(13), before, "the byte is flipped");
    assert!(ed.message.contains("性別"), "{}", ed.message);
    assert!(
        ed.message.contains("男") || ed.message.contains("女"),
        "the line names the value it wrote: {}",
        ed.message
    );

    // The console calls it instead of flipping the byte itself.
    let body = fn_body(ui_src(), "pub fn handle_pad_key");
    assert!(body.contains("ed.char_gender_flip()"), "{body}");
    assert!(
        !body.contains("ed.set_char_value(13"),
        "the console must not write the byte behind the state machine: {body}"
    );
}

// ---------------------------------------------------------------------------
// D15: [＋] asks about a form's draft
// ---------------------------------------------------------------------------

/// press_quit looked at dirty alone, so a clean save with a form still in
/// memory quit on one press and threw the draft away without a word.
#[test]
fn the_quit_key_asks_about_a_forms_draft() {
    let mut ed = open_blank();
    ed.screen = Screen::Equip;
    ed.input(Key::Plus); // [＋] opens the new-record form on this screen
    let mut form = ed.equip_edit.expect("the creator opens a form");
    form.kind = 2; // not the value the creator started on
    ed.equip_edit = Some(form);
    assert!(ed.equip_draft_dirty(), "the form holds a draft");
    assert!(!ed.dirty, "the save itself is clean");

    assert!(!ed.press_quit(), "the first [＋] only asks");
    assert!(ed.message.contains("装備編集"), "{}", ed.message);
    assert!(ed.message.contains("下書き"), "{}", ed.message);
    assert!(ed.press_quit(), "the second [＋] quits");
    assert!(ed.quit);

    // A clean editor still quits on one press.
    let mut ed = open_blank();
    assert!(ed.press_quit());
}

// ---------------------------------------------------------------------------
// E16: the way back names the screen it goes to
// ---------------------------------------------------------------------------

/// Five screens said "[B] 戻る" and went somewhere else; two of them bounced
/// between each other and never left the section.
#[test]
fn the_back_key_names_the_screen_it_goes_to() {
    // The word is the destination screen's name, and it is the name the
    // sub-tab bar prints (hbedit::screen_name): スロット / 護石一覧 /
    // SD(.arc) / クエスト改造. The sixth review's leftovers took the last
    // verbs (改造へ, カードへ) out.
    let mut ed = open_blank();
    ed.screen = Screen::Equip;
    assert!(ed.footer_hint().contains("[B] スロット"), "{}", ed.footer_hint());
    ed.open_charms();
    assert!(ed.footer_hint().contains("[B] スロット"), "{}", ed.footer_hint());
    ed.screen = Screen::Sets;
    assert!(ed.footer_hint().contains("[B] 護石一覧"), "{}", ed.footer_hint());
    ed.screen = Screen::SaveQuests;
    assert!(ed.footer_hint().contains("[B] SD(.arc)"), "{}", ed.footer_hint());
    ed.screen = Screen::ArcEntries;
    assert!(
        ed.footer_hint().contains("[B] クエスト改造"),
        "{}",
        ed.footer_hint()
    );

    // No [B] bounces: セーブ内 -> SD(.arc) -> セーブ一覧, and the section is left.
    ed.screen = Screen::SaveQuests;
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::B);
    assert_eq!(
        ed.screen,
        Screen::SaveList,
        "the second [B] leaves the quest section instead of bouncing back"
    );

    // The console's dispatch asks the state machine for the two it used to
    // answer itself.
    let body = fn_body(ui_src(), "pub fn handle_pad_key");
    assert!(
        body.contains("Key::B => ed.input(Key::B)"),
        "the console hands [B] to the state machine: {body}"
    );
}
