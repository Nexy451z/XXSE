//! The second review of the console's 構成・情報設計: twenty findings, one test
//! each.
//!
//! The console's dispatch cannot be driven from a host test, so what is pinned
//! here is the layer underneath it - the state machine, the strings it hands the
//! console, and the window arithmetic - plus the wiring in switch/src/ui.rs and
//! switch/src/editor.rs, read as text. Every test names the function the finding
//! was about, so a regression points at the code rather than at the screen.
//!
//! Nothing here writes a card: every save and every quest is built in memory.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::charm::{self, Charm};
use mhnx_core::equip;
use mhnx_core::hbedit::{
    self, charm_form_rows, quest_row_badge, Editor, EquipFormAction, Key, Screen, CHARM_EDIT_ROWS,
    QUEST_MON_SEM_X, QUEST_MON_SLOTS, QUEST_TAB_BASIC, QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS,
    QUEST_TAB_SUPPLIES, TEXT_QUEST_ITEM,
};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};
use mhnx_core::save::{self, Save};

const HASH_MIB: u32 = 0x1BBF_D18E;
const HASH_SEM: u32 = 0x2553_701D;
const HASH_REM: u32 = 0x5B3C_302D;
const HASH_GMD: u32 = 0x242B_B29A;
const QUEST_ID: i32 = 1_010_165;

// ---------------------------------------------------------------------------
// The two layers, as text
// ---------------------------------------------------------------------------

fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
}

fn editor_src() -> &'static str {
    include_str!("../../../switch/src/editor.rs")
}

fn hbedit_src() -> &'static str {
    include_str!("../src/hbedit.rs")
}

/// The body of one function, from its declaration to the next item.
fn fn_body<'a>(src: &'a str, decl: &str) -> &'a str {
    let start = src
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} is not in the console sources"));
    let rest = &src[start + decl.len()..];
    let mut end = rest.len();
    for pat in ["\n    fn ", "\n    pub fn ", "\n    #[allow(", "\nfn "] {
        if let Some(i) = rest.find(pat) {
            end = end.min(i);
        }
    }
    &rest[..end]
}

// ---------------------------------------------------------------------------
// Saves and quests, built in memory
// ---------------------------------------------------------------------------

fn mib_bytes(id: u32, minutes: u8) -> Vec<u8> {
    let mut mib = vec![0u8; MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
    mib[0x16] = minutes;
    let o = 100;
    mib[o..o + 2].copy_from_slice(&85u16.to_le_bytes());
    mib[o + 3] = 1;
    mib[o + 4] = 1;
    mib[o + 5] = 50;
    mib[o + 6] = 50;
    mib[o + 7] = 50;
    mib[o + 9..o + 11].copy_from_slice(&100u16.to_le_bytes());
    mib[o + 12] = 1;
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
        ],
    }
    .to_bytes()
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

/// A .mib and a .gmd: the two files the aggregate screen reads.
/// 184 byte reward table: one group of three rows whose chances add up to 100.
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

fn sem_bytes(area: u32, x: f32, y: f32, z: f32, w: f32, wave: u32) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8..12].copy_from_slice(&wave.to_le_bytes());
    v[0x0C..0x10].copy_from_slice(&area.to_le_bytes());
    v[0x10..0x14].copy_from_slice(&x.to_le_bytes());
    v[0x14..0x18].copy_from_slice(&y.to_le_bytes());
    v[0x18..0x1C].copy_from_slice(&z.to_le_bytes());
    v[0x1C..0x20].copy_from_slice(&w.to_le_bytes());
    v
}

fn quest_arc_bytes() -> Vec<u8> {
    Arc {
        version: ARC_VERSION,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry(
                "quest\\boss\\setEmMain\\b_m15em010_00",
                HASH_SEM,
                sem_bytes(1, 180.0, 23.0, -150.0, 90.0, 0),
            ),
            entry(
                "quest\\boss\\setEmMain\\b_m00em000_00",
                HASH_SEM,
                sem_bytes(0, 0.0, 0.0, 0.0, 0.0, 0),
            ),
            entry(
                "quest\\boss\\setEmMain\\b_m15em010_00_mib",
                HASH_MIB,
                mib_bytes(QUEST_ID as u32, 50),
            ),
            entry("quest\\rem\\rem_620114", HASH_REM, rem_bytes()),
            entry(
                "quest\\questData\\questData_1010165_jpn",
                HASH_GMD,
                gmd_bytes(),
            ),
        ],
    }
    .to_bytes()
    .expect("the synthetic quest must pack")
}

/// A save with one download quest in slot 0 of the DLC section.
fn save_with_one_quest() -> Save {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    let arc = quest_arc_bytes();
    let base = save.dlc_section_offset() as usize + mhnx_core::dlc::QUEST_ARRAY_OFFSET;
    let body = save.body_mut();
    body[base..base + 4].copy_from_slice(&QUEST_ID.to_le_bytes());
    body[base + 4..base + 8].copy_from_slice(&(arc.len() as i32).to_le_bytes());
    body[base + 8..base + 8 + arc.len()].copy_from_slice(&arc);
    save
}

/// The 天の護石 record the sample saves are full of.
fn ten_record() -> [u8; charm::CHARM_RECORD] {
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
    r
}

/// A save with three talismans in box slots 0..2 and one weapon at slot 3.
fn save_with_charms_and_a_weapon() -> Save {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    {
        let b = save.equipment_box_mut(0).expect("the box exists");
        for i in 0..3 {
            let o = i * charm::CHARM_RECORD;
            b[o..o + charm::CHARM_RECORD].copy_from_slice(&ten_record());
        }
        // A weapon: kind 7 (大剣), id 40, level 1.
        let o = 3 * equip::EQUIP_RECORD;
        equip::set_kind(&mut b[o..o + equip::EQUIP_RECORD], 7);
        equip::set_level(&mut b[o..o + equip::EQUIP_RECORD], 1);
        b[o + 2..o + 4].copy_from_slice(&40u16.to_le_bytes());
    }
    save
}

fn editor_with_quest() -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(save_with_one_quest());
    ed.save_path = Some(String::from("memory:review-2"));
    ed.slot = 0;
    ed.screen = Screen::Slots;
    ed
}

fn editor_with_charms() -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(save_with_charms_and_a_weapon());
    ed.save_path = Some(String::from("memory:review-2"));
    ed.slot = 0;
    ed.screen = Screen::Charms;
    ed
}

// ---------------------------------------------------------------------------
// 1: the monster row's meta named [Y] for a job [Y] no longer has
// ---------------------------------------------------------------------------

/// The row said 「▸詳細[Y]」, and [Y] is the step key on that screen: the meta
/// names the value [ZR] is on and the slot's placement instead.
#[test]
fn the_monster_row_meta_does_not_promise_y_for_detail() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    let meta = ed.quest_mon_meta(0);
    assert!(!meta.contains("[Y]"), "the row must not name [Y]: {meta}");
    assert!(!meta.contains("詳細"), "there is no detail row to open: {meta}");
    assert!(meta.contains("変更中:"), "{meta}");
    // The console's own meta string is the state machine's, and the literal it
    // used to carry is gone.
    let body = fn_body(ui_src(), "fn draw_quest_edit");
    assert!(!body.contains("詳細[Y]"), "{body}");
    assert!(body.contains("ed.quest_mon_meta("), "{body}");
}

// ---------------------------------------------------------------------------
// 2: 護石編集 opened the list, not the form
// ---------------------------------------------------------------------------

/// Both Gear tabs called open_charms(), so the form was one more [A] away and
/// the tab lit 護石一覧 instead of the one that was pressed.
#[test]
fn the_charm_edit_tab_opens_the_form_on_the_selected_record() {
    let mut ed = editor_with_charms();
    ed.open_charms();
    ed.cursor = 1;
    ed.open_charm_form_selected();
    assert_eq!(ed.screen, Screen::CharmEdit, "the tab opens the form");
    let form = ed.charm_edit.expect("the form is open");
    assert_eq!(form.index, 1, "on the record the list was on");

    // An empty list says so instead of opening a form on nothing.
    let mut ed = Editor::new();
    ed.save = Some(save::empty_switch_save());
    ed.screen = Screen::Charms;
    ed.open_charm_form_selected();
    assert_eq!(ed.screen, Screen::Charms);
    assert!(ed.charm_edit.is_none());
    assert!(ed.message.contains("護石を選んでください"), "{}", ed.message);
}

// ---------------------------------------------------------------------------
// 3: the cursor went back to the top, and the reward tab had no search
// ---------------------------------------------------------------------------

/// enter_quest_edit_keeping_return reset the row, so a pick for row 40 put the
/// list back at the top; and the two item tabs had no search at all.
#[test]
fn the_quest_row_survives_the_item_pick_and_the_item_tabs_can_be_searched() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    ed.quest_edit_cursor = 3;
    // The item picker comes back through the same entry point.
    ed.enter_quest_edit_from(Screen::SaveQuests);
    assert_eq!(ed.quest_edit_cursor, 3, "the row is kept, not reset");

    // [ZR] on 報酬 walks the row's three cells (アイテム → 個数 → 確率%), and
    // the search narrows the list. The cycle does not open the keyboard (the
    // third review's finding 5): it used to, every other press, which left no
    // way back to 個数. Its stops are cells, not the search (the fourth
    // review's finding 17, and the fifth's #4).
    // The sixth review's leftovers: 個数 ⇄ 確率% is the whole cycle. The item
    // cell is [A]'s, so [ZR] never lands on it - the third stop could only
    // refuse the arrows.
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    ed.input(Key::Z);
    assert!(ed.quest_edit_chance(), "the first stop is the chance column");
    ed.input(Key::Z);
    assert_eq!(
        ed.quest_reward_field,
        mhnx_core::hbedit::QUEST_REWARD_COUNT,
        "two stops: back to the count"
    );
    assert!(
        ed.quest_reward_field != mhnx_core::hbedit::QUEST_REWARD_ITEM,
        "the item cell is not a stop"
    );
    assert_eq!(ed.take_text_request(), None, "the cycle opens no keyboard");
    // The search is its own press: [R] on the same tab.
    ed.input(Key::R);
    assert_eq!(
        ed.take_text_request(),
        Some((TEXT_QUEST_ITEM, String::new())),
        "the search is [R], not a stop of the cycle"
    );
    ed.apply_quest_item_filter(String::from("zzz"));
    assert!(ed.quest_rewards().is_empty(), "the search narrows the list");
    assert!(ed.quest_item_heading().contains("検索: zzz"), "{}", ed.quest_item_heading());
    ed.apply_quest_item_filter(String::new());
    assert!(!ed.quest_rewards().is_empty(), "an empty answer clears it");
}

// ---------------------------------------------------------------------------
// 4: opening a quest landed on 構成(.arc), and [B] there was the way "on"
// ---------------------------------------------------------------------------

/// A quest is opened to be edited: the screen it opens is the one that edits
/// it. The file list is one press away ([ファイル] pill / [R] held).
#[test]
fn opening_a_quest_from_the_save_lands_on_the_editor() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    assert_eq!(ed.screen, Screen::QuestEdit, "{}", ed.message);
    assert!(!ed.mib_bytes().is_empty(), "the .mib is loaded");
    // [B] goes back to the list it came from, not to the file list.
    assert_eq!(ed.quest_return, Screen::SaveQuests);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::SaveQuests);
}

// ---------------------------------------------------------------------------
// 5: one talisman, two screens and two forms
// ---------------------------------------------------------------------------

/// The equipment box listed type 6, so a talisman had two forms - and the
/// equipment one offered レベル / 着せ替え, which mean nothing for a talisman.
/// The three decoration sockets are the part that did mean something, and they
/// moved to the talisman form.
#[test]
fn the_equipment_box_does_not_list_talismans_and_the_talisman_form_keeps_its_sockets() {
    let ed = editor_with_charms();
    assert_eq!(ed.equipment().len(), 4, "the box holds three talismans and a weapon");
    let rows = ed.equip_rows();
    assert_eq!(rows.len(), 1, "the list is the weapon and nothing else");
    assert_eq!(rows[0].kind, 7);
    assert!(
        rows.iter().all(|e| e.kind != equip::TALISMAN_KIND),
        "no talisman is a row of the equipment box"
    );

    // The talisman form has the three sockets as rows of its own.
    let mut ed = editor_with_charms();
    ed.open_charms();
    ed.cursor = 0;
    ed.open_charm_form_selected();
    let form = ed.charm_edit.expect("the form is open");
    assert_eq!(CHARM_EDIT_ROWS, 10, "ten rows: the sockets moved here");
    let rows = charm_form_rows(&form);
    for (i, sock) in (7usize..=9).enumerate() {
        assert_eq!(rows[sock].field, 7 + i);
        assert_eq!(rows[sock].title.contains("装飾品"), true, "{}", rows[sock].title);
    }
    // レベル and 着せ替え are not rows of it: a talisman has neither.
    for row in rows.iter() {
        assert!(!row.title.contains("レベル"), "{}", row.title);
        assert!(!row.title.contains("着せ替え"), "{}", row.title);
    }
}

// ---------------------------------------------------------------------------
// 6: 護石 and お守り in one screen
// ---------------------------------------------------------------------------

/// The game's own names are 「〜の護石」 (romfs amuletData_jpn.gmd), so 護石 is
/// the word the screens use - the item names beside it cannot be renamed.
#[test]
fn the_talisman_is_called_kanshi_on_every_screen() {
    for (name, src) in [
        ("hbedit.rs", hbedit_src()),
        ("ui.rs", ui_src()),
        ("editor.rs", editor_src()),
    ] {
        assert!(
            !src.contains("お守り"),
            "{name} still says お守り: the game's own item names are 〜の護石"
        );
    }
    // The game's own names are untouched: they are the values the rows print.
    assert_eq!(charm::kind_name(10), Some("天の護石"));
    assert_eq!(equip::type_name(6), "護石");
    // The tab and the pill use the same word.
    assert!(ui_src().contains("\"護石一覧\""), "the tab is 護石一覧");
    assert!(ui_src().contains("\"護石編集\""), "the tab is 護石編集");
}

// ---------------------------------------------------------------------------
// 7: [Y] moved the tab on two views and wrote the save on the third
// ---------------------------------------------------------------------------

/// [Y] is the tab key on all three views now; the 既読 write is the deck's pill.
#[test]
fn y_is_the_tab_key_on_every_progress_view() {
    let mut ed = editor_with_quest();
    ed.open_progress();
    // 勲章 -> 依頼 -> クエスト -> 勲章, all by [Y].
    let first = ed.progress_view();
    ed.input(Key::Y);
    assert_ne!(ed.progress_view(), first, "[Y] walks the views");
    ed.input(Key::Y);
    ed.input(Key::Y);
    assert_eq!(ed.progress_view(), first, "[Y] wraps after three views");

    // The write is the pill's, and it only works on the quest view.
    ed.progress_tab = mhnx_core::hbedit::PROGRESS_AWARDS;
    ed.progress_seen_press();
    assert!(ed.message.contains("クエスト ビュー"), "{}", ed.message);
    ed.progress_tab = mhnx_core::hbedit::PROGRESS_QUESTS;
    let before = ed.progress_quest_state(0);
    ed.progress_seen_press();
    assert_ne!(ed.progress_quest_state(0), before, "the pill writes the bit");

    // The footer names [Y] as the tab key on the quest view too.
    let hint = ed.footer_hint();
    assert!(hint.contains("[Y] タブ"), "{hint}");
    assert!(!hint.contains("[Y] 既読"), "{hint}");
}

// ---------------------------------------------------------------------------
// 8: the pressed quest tab lit a different one
// ---------------------------------------------------------------------------

/// The bar derived its lit tab from the screen, so pressing クエスト改造 with no
/// .arc open lit セーブ内 instead.
#[test]
fn the_pressed_quest_tab_is_the_lit_one() {
    let src = ui_src();
    assert!(
        src.contains("quest_bar_tab"),
        "the bar keeps the tab the user pressed"
    );
    let arm = fn_body(src, "fn select_sub_tab");
    assert!(arm.contains("self.quest_bar_tab = 0"), "{arm}");
    assert!(arm.contains("self.quest_bar_tab = idx"), "{arm}");
    // Entering the section lands on the save's list, and the bar lights the tab
    // that list belongs to (セーブ内) - one list, one tab. With no save it lands
    // on SD(.arc), which is that tab's own index.
    let jump = fn_body(src, "fn goto_section");
    assert!(jump.contains("self.quest_bar_tab = 1"), "{jump}");
    assert!(jump.contains("self.quest_bar_tab = 4"), "{jump}");
    // The bar itself reads that field rather than deriving 1 + quest_tab.
    let bar = fn_body(src, "fn sub_tabs_for");
    assert!(bar.contains("Screen::SaveQuests => self.quest_bar_tab.min(3)"), "{bar}");
    assert!(!bar.contains("1 + self.quest_tab"), "{bar}");
}

// ---------------------------------------------------------------------------
// 9: the new-record picker opened on row 1,608
// ---------------------------------------------------------------------------

/// The new form's default type is armour and the whole-box list starts with the
/// weapons, so finding the form's own (kind, id) put the cursor 1,608 rows in.
#[test]
fn the_new_equipment_picker_starts_at_the_head_of_the_list() {
    let all = hbedit::equip_choices(hbedit::EquipScope::All);
    assert!(all.len() > 1600);
    assert_eq!(
        Editor::equip_pick_start(&all, 1, 1, true),
        0,
        "a record being created starts at the head of the list"
    );
    // A record that exists starts on its own row, so reopening the list does
    // not lose it.
    let want = all
        .iter()
        .position(|c| c.kind == 7 && c.id == 40)
        .expect("大剣 40 is in the list");
    assert_eq!(Editor::equip_pick_start(&all, 7, 40, false), want);
    // An id the table does not carry starts at the head of its own type.
    let head = all.iter().position(|c| c.kind == 7).unwrap_or(0);
    assert_eq!(Editor::equip_pick_start(&all, 7, 9999, false), head);
}

// ---------------------------------------------------------------------------
// 10: the talisman list went back to the top after a write
// ---------------------------------------------------------------------------

/// [A] wrote the record and put the cursor on row 0, so the next visit to the
/// fortieth talisman cost forty presses.
#[test]
fn writing_a_talisman_leaves_the_cursor_on_its_row() {
    let mut ed = editor_with_charms();
    ed.open_charms();
    ed.cursor = 2;
    ed.open_charm_form_selected();
    assert!(ed.commit_charm_draft(), "{}", ed.message);
    assert_eq!(ed.screen, Screen::Charms);
    assert_eq!(ed.cursor, 2, "the cursor is on the record that was written");
    assert_eq!(ed.charms()[ed.cursor].index, 2);
}

// ---------------------------------------------------------------------------
// 11: 所持オトモ's [Y] was on neither the footer nor the deck
// ---------------------------------------------------------------------------

#[test]
fn the_roster_names_its_column_key() {
    let mut ed = Editor::new();
    ed.save = Some(save::empty_switch_save());
    ed.screen = Screen::Roster;
    let hint = ed.footer_hint();
    assert!(hint.contains("[Y] 列"), "the footer names [Y]: {hint}");
    assert!(
        hbedit::caption_width(&hint) <= hbedit::FOOTER_HINT_ROOM,
        "the guide still fits: {}px",
        hbedit::caption_width(&hint)
    );
    // The pill prints the table's word and the state, and no key: the footer is
    // the one surface that names keys (the sixth review's leftovers).
    let deck = fn_body(ui_src(), "fn draw_sub_bar_quick_tools");
    assert!(
        deck.contains("pill_word(ed, \"Y\", \"列\")"),
        "the pill borrows the table's word"
    );
    assert!(!deck.contains("[Y] 列"), "and prints no key");
}

// ---------------------------------------------------------------------------
// 12: the guild card's footer had no [B]
// ---------------------------------------------------------------------------

#[test]
fn the_guild_card_footer_names_the_way_out() {
    let mut ed = Editor::new();
    ed.save = Some(save::empty_switch_save());
    ed.screen = Screen::Guild;
    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_CARD);
    let hint = ed.footer_hint();
    // The destination's name: the card's [B] goes to the slot list, and the
    // bar calls it スロット (hbedit::screen_name).
    assert!(hint.contains("[B] スロット"), "the footer names [B] and where it goes: {hint}");
    assert!(
        hbedit::caption_width(&hint) <= hbedit::FOOTER_HINT_ROOM,
        "the guide still fits: {}px",
        hbedit::caption_width(&hint)
    );
}

// ---------------------------------------------------------------------------
// 13: three doors to the file screens
// ---------------------------------------------------------------------------

/// The pill, [R] held and a row every tab ended with. The row is gone; the pill
/// (and the pad's route to the same action) is the one door.
#[test]
fn the_quest_screen_has_one_door_to_the_file_screens() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    for tab in [
        QUEST_TAB_BASIC,
        QUEST_TAB_MONSTERS,
        QUEST_TAB_REWARDS,
        QUEST_TAB_SUPPLIES,
    ] {
        ed.quest_set_tab(tab);
        assert_eq!(
            ed.quest_row_count(),
            ed.quest_value_rows(),
            "tab {tab} spends no row on a button"
        );
    }
    // The pill is still there for this screen.
    ed.quest_set_tab(QUEST_TAB_BASIC);
    assert!(ed.detail_pill_label().contains("ファイル"), "{}", ed.detail_pill_label());
    // And the console no longer draws the row.
    assert!(!ui_src().contains("fn detail_row("), "the row builder is gone");
    assert!(!ui_src().contains("QuestPick::Detail"), "and its action is gone");
}

// ---------------------------------------------------------------------------
// 14: a long guide was cut under the update banner
// ---------------------------------------------------------------------------

/// The banner takes the right end of the strip, and text_clipped cuts the end -
/// which is where the way out is named. The tail is drawn instead.
#[test]
fn the_guide_keeps_its_last_items_under_the_banner() {
    let width = |s: &str| s.chars().count() as i32 * 10;
    let guide = "[A] 開く  [B] 戻る";
    assert_eq!(hbedit::guide_tail(guide, 1000, &mut |s| width(s)), guide);
    let tail = hbedit::guide_tail(guide, 80, &mut |s| width(s));
    assert!(tail.len() < guide.len(), "the head is what is lost");
    assert!(guide.ends_with(tail), "the tail is what survives: {tail}");
    assert!(tail.contains("戻る"), "{tail}");
    // The console draws it.
    let body = fn_body(ui_src(), "fn draw_guide");
    assert!(body.contains("guide_tail"), "{body}");
}

// ---------------------------------------------------------------------------
// 15: every list was eight rows, strip or no strip
// ---------------------------------------------------------------------------

/// The strip's reserve was already subtracted from the fit, so the extra
/// min(8) on top of it could never bite - and the panel's 10px padding kept
/// nine rows from ever fitting.
#[test]
fn a_list_shows_nine_rows_when_no_strip_is_drawn() {
    let body_h = 508;
    let row_h = 56;
    let line_h = 17;
    assert_eq!(
        hbedit::list_window_rows(body_h, row_h, 0, line_h),
        9,
        "nine rows with no strip"
    );
    assert_eq!(
        hbedit::list_window_rows(body_h, row_h, 1, line_h),
        8,
        "one line of strip costs a row"
    );
    assert_eq!(
        hbedit::list_window_rows(body_h, row_h, 2, line_h),
        8,
        "two lines cost the same one row, not two"
    );
    assert_eq!(hbedit::LIST_PAD_TOP, 4, "the padding is what gave the row back");
    // The console asks the same function.
    let body = fn_body(ui_src(), "fn draw_adaptive_rows_warn");
    assert!(body.contains("hbedit::list_window_rows"), "{body}");
    assert!(!body.contains("vis.min(8)"), "the dead rule is gone: {body}");
}

// ---------------------------------------------------------------------------
// 16: the status line spoke two languages
// ---------------------------------------------------------------------------

#[test]
fn the_status_lines_are_japanese() {
    for (name, src) in [("hbedit.rs", hbedit_src()), ("editor.rs", editor_src())] {
        // Only the code: a doc comment may quote the English it replaced.
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for english in [
            "not a MHXX save",
            "loaded {} bytes from",
            "wrote {} quest file(s)",
            "not an .arc",
            "is not in this save",
            "has no payload",
            "opened quest",
            "cannot read",
            "open a save first",
        ] {
            assert!(
                !code.contains(english),
                "{name} still says {english:?} on the status line"
            );
        }
    }
    // The messages that replaced them are the ones the screens show.
    let mut ed = Editor::new();
    ed.open_save_bytes("memory:bad", b"not a save at all");
    assert!(ed.message.contains("MHXX のセーブではありません"), "{}", ed.message);
}

// ---------------------------------------------------------------------------
// 17: four tabs, four badge schemes
// ---------------------------------------------------------------------------

/// #01 / S1 / G1 / #01, and the reward one repeated itself across files.
#[test]
fn every_quest_tab_numbers_its_rows_the_same_way() {
    assert_eq!(quest_row_badge(0), "#01");
    assert_eq!(quest_row_badge(9), "#10");
    let body = fn_body(ui_src(), "fn draw_quest_edit");
    assert_eq!(
        body.matches("quest_row_badge(").count(),
        4,
        "all four tabs use it"
    );
    assert!(!body.contains("format!(\"G{}\"") && !body.contains("format!(\"S{}\""));
    // The file name - what makes two rows of the same number different - is in
    // the meta line of the two item tabs.
    assert!(body.contains("r.file"), "{body}");
}

// ---------------------------------------------------------------------------
// 18: the 種別 row stepped one kind per press
// ---------------------------------------------------------------------------

/// Twenty kinds, one press each, and the row's [A] opened the *item* list.
#[test]
fn the_kind_row_opens_the_kind_list() {
    let choices = Editor::equip_kind_choices();
    // Nineteen: the twenty kinds a hunter's box holds, less the talisman, which
    // has a screen of its own and is not a row of this box (the third review's
    // finding 2).
    assert_eq!(choices.len(), 19, "the kinds a hunter's box lists");
    assert!(
        !choices.iter().any(|(v, _)| *v == 6),
        "護石 is not one of them"
    );
    assert!(choices.iter().any(|(v, label)| *v == 7 && label.contains("大剣")));

    let mut ed = Editor::new();
    ed.save = Some(save_with_charms_and_a_weapon());
    ed.slot = 0;
    ed.screen = Screen::Equip;
    ed.equip_cursor = 0;
    ed.input(Key::A); // opens the form on the weapon
    assert_eq!(ed.screen, Screen::EquipEdit);
    {
        let e = ed.equip_edit.as_mut().expect("the form is open");
        e.field = 7; // 種別
    }
    assert_eq!(
        ed.equip_edit.expect("form").a_action(),
        EquipFormAction::PickKind
    );
    ed.input(Key::A);
    assert!(ed.equip_kind_pick_pending, "the list is asked for");
    assert_eq!(ed.screen, Screen::EquipEdit, "the console owns that modal");
    ed.equip_kind_pick_pending = false;
    assert!(ed.apply_equip_kind(14), "{}", ed.message);
    assert_eq!(ed.equip_edit.expect("form").kind, 14);
}

// ---------------------------------------------------------------------------
// 19: opening a monster's detail pushed slots 4 and 5 off the list
// ---------------------------------------------------------------------------

/// The five ▸詳細 rows are values of the slot's own row now: [ZR] walks eleven,
/// and the placement is printed on the inspector's second line.
#[test]
fn the_monster_tab_is_five_slots_and_the_placement_is_a_value() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    assert_eq!(ed.quest_mon_rows(), vec![0, 1, 2, 3, 4]);
    assert_eq!(ed.quest_row_count(), QUEST_MON_SLOTS, "no row is spent on a detail");
    assert_eq!(hbedit::QUEST_MON_FIELDS, 11, "the .sem five are values");
    // The last five are the placement, and stepping one writes the .sem.
    ed.quest_edit_cursor = 0;
    ed.quest_edit_field = QUEST_MON_SEM_X;
    assert_eq!(ed.quest_field_label(), "X座標");
    let before = ed.quest_sem_line(0);
    ed.input(Key::Right);
    assert_ne!(ed.quest_sem_line(0), before, "the row's value moves");
    // A slot with no .sem says so rather than inventing a zero.
    assert!(ed.quest_sem_line(4).is_none());
}

// ---------------------------------------------------------------------------
// 20: 公式(189) / 没クエ(34) were literals
// ---------------------------------------------------------------------------

/// The tabs count what the bundle really carries; a checkout without the
/// generated bundle carries none, and the guide says why the list is empty.
#[test]
fn the_bundle_tabs_count_what_the_build_carries() {
    let (official, dead) = mhnx_core::dlc::bundled_quest_counts();
    let labels = Editor::quest_section_tab_labels();
    assert_eq!(labels.len(), 6);
    assert_eq!(labels[0], "クエスト改造");
    assert_eq!(labels[1], "セーブ内");
    assert_eq!(labels[2], format!("公式({official})"));
    assert_eq!(labels[3], format!("没クエ({dead})"));
    assert_eq!(labels[4], "SD(.arc)");
    assert_eq!(labels[5], "固定装備");
    // No literal is left in the console's bar.
    assert!(!ui_src().contains("公式(189)"), "the count is not a literal");
    assert!(!ui_src().contains("没クエ(34)"), "the count is not a literal");
    assert!(ui_src().contains("quest_section_tab_labels"), "the bar asks the core");

    // Zero of each is a real state, and the guide names the reason.
    let mut ed = Editor::new();
    ed.save = Some(save::empty_switch_save());
    ed.screen = Screen::SaveQuests;
    if official == 0 && dead == 0 {
        assert_eq!(labels[2], "公式(0)");
        assert_eq!(labels[3], "没クエ(0)");
        let guide = ed.screen_guide();
        assert!(guide.contains("DLC バンドル未生成"), "{guide}");
    }
    assert!(!Editor::bundled_quests_available() || official + dead > 0);
}
