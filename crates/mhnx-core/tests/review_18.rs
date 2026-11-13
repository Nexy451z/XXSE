//! The fourth review: twenty findings, one test each, plus the audit that
//! closes the hole the third one left.
//!
//! The console layer (switch/src) is not part of this workspace, so the tests
//! that need it read it as text - but only for the *wiring*. Everything a state
//! transition can answer is driven here: the key map, the two forms' drafts,
//! the tab presses and the footers, including a walk that presses every key a
//! footer names and fails when nothing happens.
//!
//! Nothing here reads or writes a card: every save is built in memory.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::charm;
use mhnx_core::equip;
use mhnx_core::hbedit::{
    self, caption_width, CharmAction, Editor, EquipFormAction, Key, Screen, Shoulder, ZrHold, ZrJob,
    FOOTER_HINT_ROOM, FORM_STEPS, PROGRESS_AWARDS, PROGRESS_QUESTS, PROGRESS_REQUESTS,
    QUEST_REWARD_CHANCE, QUEST_REWARD_COUNT, QUEST_REWARD_FIELDS, QUEST_REWARD_ITEM,
    QUEST_TAB_BASIC,
    QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, QUEST_TAB_SUPPLIES, TEXT_QUEST_ITEM,
};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};
use mhnx_core::save::{self, Save};

const HASH_MIB: u32 = 0x1BBF_D18E;
const HASH_SEM: u32 = 0x2553_701D;
const HASH_REM: u32 = 0x5B3C_302D;
const HASH_GMD: u32 = 0x242B_B29A;
const QUEST_ID: i32 = 1_010_165;

// ---------------------------------------------------------------------------
// The console layer, as text (the wiring only)
// ---------------------------------------------------------------------------

fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
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
// Saves, built in memory
// ---------------------------------------------------------------------------

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

fn blank_save() -> Save {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    save
}

fn save_with_charms(n: usize) -> Save {
    let mut save = blank_save();
    {
        let b = save.equipment_box_mut(0).expect("the box exists");
        for i in 0..n {
            let o = i * charm::CHARM_RECORD;
            b[o..o + charm::CHARM_RECORD].copy_from_slice(&ten_record());
        }
    }
    save
}

fn save_with_equipment(n: usize) -> Save {
    let mut save = blank_save();
    for i in 0..n {
        // 大剣 (7), id 40 + i, level 1: records the box lists.
        mhnx_core::equipops::create(&mut save, 0, 7, 40 + i as u16, 1).expect("a blank record");
    }
    save
}

fn save_with_palicos() -> Save {
    let mut save = blank_save();
    for (i, name) in ["アイルー", "メラルー", "トラ"].iter().enumerate() {
        assert!(
            mhnx_core::palico::set_name(&mut save, 0, i, name),
            "the roster record {i} has to be nameable"
        );
    }
    save
}

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

/// One group of three reward rows whose chances add up to 100.
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

fn save_with_one_quest() -> Save {
    let mut save = blank_save();
    let arc = quest_arc_bytes();
    let base = save.dlc_section_offset() as usize + mhnx_core::dlc::QUEST_ARRAY_OFFSET;
    let body = save.body_mut();
    body[base..base + 4].copy_from_slice(&QUEST_ID.to_le_bytes());
    body[base + 4..base + 8].copy_from_slice(&(arc.len() as i32).to_le_bytes());
    body[base + 8..base + 8 + arc.len()].copy_from_slice(&arc);
    save
}

fn editor_with(save: Save, screen: Screen) -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.save_path = Some(String::from("memory:review-18"));
    ed.slot = 0;
    ed.screen = screen;
    ed
}

fn editor_with_charms(n: usize) -> Editor {
    editor_with(save_with_charms(n), Screen::Charms)
}

fn editor_with_equipment(n: usize) -> Editor {
    editor_with(save_with_equipment(n), Screen::Equip)
}

fn editor_with_palicos() -> Editor {
    editor_with(save_with_palicos(), Screen::Roster)
}

fn editor_with_quest() -> Editor {
    editor_with(save_with_one_quest(), Screen::Slots)
}

// ---------------------------------------------------------------------------
// A. data
// ---------------------------------------------------------------------------

/// A1: open_charm_editor(true) seeds a new talisman from a real record at
/// *another* index, and charm_draft_dirty compared the form with the record at
/// its own index - so a form nobody had touched read as an edit and one [X]
/// wrote a duplicate of a talisman the user had only opened the form for.
#[test]
fn a_fresh_talisman_is_clean_and_the_save_key_writes_no_duplicate() {
    let mut ed = editor_with_charms(3);
    ed.open_charms();
    assert_eq!(ed.charms().len(), 3);

    // [+] / [L] on the list: a new talisman, seeded from a real one.
    ed.cursor = 2;
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    let form = ed.charm_edit.expect("the form");
    assert_eq!(form.index, 3, "the first free record");
    assert!(
        form.skill1 != 0,
        "the seed carries a real skill - which is what made it read as an edit"
    );
    assert!(
        !ed.charm_draft_dirty(),
        "a form nobody touched holds no draft"
    );
    assert_eq!(form.seed, form.value(), "the form still matches its seed");

    // [X] saves the file and writes no record.
    ed.take_write();
    ed.quick_save();
    assert_eq!(
        ed.charms().len(),
        3,
        "no duplicate was written: {}",
        ed.message
    );
    assert!(
        ed.charms().iter().all(|c| c.index != 3),
        "the free record is still free"
    );
    assert!(
        ed.take_write().is_some(),
        "the save key still writes the file"
    );

    // A changed field is a draft, and [X] writes it.
    ed.input(Key::Right);
    assert!(ed.charm_draft_dirty(), "a changed field is a draft");
    ed.quick_save();
    assert_eq!(ed.charms().len(), 4, "the draft is written: {}", ed.message);
    assert!(!ed.charm_draft_dirty(), "and the form is clean again");
}

// ---------------------------------------------------------------------------
// B. what the screen promises and what it does
// ---------------------------------------------------------------------------

/// B2: 報酬 / 支給品 print 検索, and shoulder_job did not list
/// Screen::QuestEdit, so the press moved to the next tab and the search never
/// opened.
///
/// The search is the held [ZR] now: [R] is the tab key on both item tabs like
/// it is everywhere else, and the hold is the key the two tabs' footers name.
#[test]
fn the_reward_search_key_is_the_screens_held_zr() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    assert_eq!(
        ed.shoulder_job(Key::R),
        Shoulder::SubTab,
        "[R] is the tab key on the reward tab too"
    );
    assert_eq!(
        ed.zr_hold_job(),
        ZrHold::Action,
        "the footer names the held [ZR] 検索, so the hold is the screen's"
    );
    ed.take_text_request();
    assert!(ed.zr_hold_press(), "the hold runs the search");
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_QUEST_ITEM),
        "the hold opens the item search: {}",
        ed.message
    );
    // [L] keeps the tab move, and the two tabs that name no search keep it too.
    assert_eq!(ed.shoulder_job(Key::L), Shoulder::SubTab);
    ed.quest_set_tab(QUEST_TAB_BASIC);
    assert_eq!(
        ed.zr_hold_job(),
        ZrHold::None,
        "基本 names no search"
    );
    ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(
        ed.footer_hint().contains("[ZR長] 検索"),
        "{}",
        ed.footer_hint()
    );
}

/// B3: [Y] became the tab key, so the quest view's 既読 bit was left with a
/// touch pill only. It has a pad key now: the held [ZR], which is the key that
/// view's footer names.
#[test]
fn the_seen_bit_has_a_pad_key() {
    let mut ed = editor_with_quest();
    ed.open_progress();
    ed.progress_tab = PROGRESS_QUESTS;
    ed.set_progress_cursor(0);
    assert_eq!(ed.shoulder_job(Key::R), Shoulder::SubTab);
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    let (_, seen, _) = ed.progress_quest_state(0);
    assert!(!seen);
    assert!(ed.zr_hold_press());
    let (_, seen, _) = ed.progress_quest_state(0);
    assert!(seen, "the hold writes the seen bit: {}", ed.message);
    assert!(ed.dirty, "and the save is dirty");
    // The other two views have one bit per row: the hold has no job there and
    // [R] walks them.
    ed.progress_tab = PROGRESS_AWARDS;
    assert_eq!(ed.zr_hold_job(), ZrHold::None);
    assert!(!ed.zr_hold_press(), "the hold does nothing on the award view");
    assert_eq!(ed.shoulder_job(Key::R), Shoulder::SubTab);
    let view = ed.progress_view();
    ed.input(Key::R);
    assert_ne!(ed.progress_view(), view, "[R] is the tab key there");
    // And the footer names the key it now answers.
    ed.progress_tab = PROGRESS_QUESTS;
    assert!(
        ed.footer_hint().contains("[ZR長] 既読"),
        "{}",
        ed.footer_hint()
    );
}

/// B4: the pad drops a [＋] quit offer on every key that is not [＋]
/// (Editor::input), and a touch action did not - so a 終了 tapped once and
/// re-tapped a minute later quit without asking again.
#[test]
fn a_touch_action_takes_the_quit_offer_back() {
    // The state machine's half: one key, one press.
    let mut ed = editor_with_charms(1);
    ed.open_charms();
    ed.dirty = true;
    assert!(!ed.press_quit(), "a dirty save asks first");
    assert!(ed.quit_armed());
    ed.input(Key::A);
    assert!(!ed.quit_armed(), "any key takes the offer back");
    ed.clear_quit_arm();
    assert!(!ed.quit_armed());

    // The console's half: every action that is not the 終了 pill drops it, and
    // the pill keeps its own offer (a second tap has to be able to quit).
    let body = fn_body(ui_src(), "fn apply_action");
    let cancel = &body[..body.find("match action").expect("the dispatch")];
    assert!(
        cancel.contains("ed.clear_quit_arm()"),
        "the cancel block has to drop the offer: {cancel}"
    );
    assert!(
        cancel.contains("UiAction::Quit"),
        "and the 終了 pill has to keep its own: {cancel}"
    );
}

/// B5: switch_slot dropped every open form with nothing on screen saying so -
/// the same loss [B] asks about, through a slot card.
#[test]
fn a_slot_switch_asks_before_it_drops_a_draft() {
    let mut ed = editor_with_charms(2);
    ed.open_charms();
    ed.cursor = 1;
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    ed.input(Key::Right);
    assert!(ed.charm_draft_dirty());
    let slot = ed.slot;
    assert!(!ed.switch_slot(1), "the first press only asks");
    assert_eq!(ed.slot, slot, "the slot did not move");
    assert!(ed.charm_edit.is_some(), "the form is still on screen");
    assert!(ed.form_discard_armed());
    assert!(ed.message.contains("下書き"), "{}", ed.message);
    assert!(ed.switch_slot(1), "the second press switches");
    assert_eq!(ed.slot, 1);
    assert!(ed.charm_edit.is_none(), "and the form is dropped");

    // A clean form switches at once: asking there would be a dialog about
    // nothing.
    let mut ed = editor_with_charms(2);
    ed.open_charms();
    ed.charm_a_press();
    assert!(!ed.charm_draft_dirty());
    assert!(ed.switch_slot(1), "a clean form does not ask");
}

/// B6: direct_input_available answered matches!(row, 1 | 8) - the creating
/// form's ID row and the editing form's ID row - so the editing form drew the
/// [R] pill on its レベル row, which answers "種別・ID の行で使います".
#[test]
fn the_number_pill_is_only_on_the_rows_that_take_a_number() {
    let mut ed = editor_with_equipment(1);
    ed.open_equip_box();
    ed.equip_cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    let form = ed.equip_edit.expect("the form");
    assert!(!form.new, "editing an existing record");
    assert!(ed.direct_input_available(7), "種別 takes an id");
    assert!(ed.direct_input_available(8), "ID takes an id");
    for row in [0usize, 1, 2, 3, 4, 5, 6] {
        assert!(
            !ed.direct_input_available(row),
            "row {row} is not a typed number"
        );
    }
    // The key agrees with the pill.
    ed.equip_edit.as_mut().expect("form").field = 1;
    ed.input(Key::R);
    assert!(
        ed.take_text_request().is_none(),
        "the level row has no keyboard: {}",
        ed.message
    );
    ed.equip_edit.as_mut().expect("form").field = 8;
    ed.input(Key::R);
    assert!(
        ed.take_text_request().is_some(),
        "the ID row opens the keyboard"
    );

    // The creating form is 0 種別 / 1 ID / 2 レベル.
    let mut ed = editor_with_equipment(1);
    ed.input(Key::Plus);
    assert!(ed.equip_edit.expect("the creator").new);
    assert!(ed.direct_input_available(0));
    assert!(ed.direct_input_available(1));
    assert!(
        !ed.direct_input_available(2),
        "the creator's level row is not a typed number"
    );
}

/// B7: the 種別 list holds BOX_KINDS.len() rows and the title said 20, the
/// core's comment said "Twenty-one kinds" - three numbers for one table.
#[test]
fn the_kind_list_counts_its_own_rows() {
    let choices = Editor::equip_kind_choices();
    assert_eq!(
        choices.len(),
        equip::BOX_KINDS.len(),
        "the list is the table"
    );
    let ui = ui_src();
    assert!(
        ui.contains("mhnx_core::equip::BOX_KINDS.len()"),
        "the title counts the table it lists"
    );
    assert!(
        !ui.contains("ハンターの箱に入る 20 種"),
        "the typed 20 is gone"
    );
    let hb = hbedit_src();
    assert!(!hb.contains("Twenty-one kinds"), "the third number is gone");
    assert!(!hb.contains("the twenty kinds"), "and so is the second");
}

/// B8: the bulk install of the bundled quests was a touch-only button: the
/// state machine answered Key::Z all along, but zr_job did not list
/// Screen::SaveQuests, so the console spent ZR on the section bar.
#[test]
fn the_bulk_quest_install_is_a_pad_key() {
    let mut ed = editor_with_quest();
    ed.screen = Screen::SaveQuests;
    assert_eq!(
        ed.zr_job(),
        ZrJob::Screen,
        "the console has to hand ZR to the screen"
    );
    let hint = ed.footer_hint();
    assert!(hint.contains("[ZR] 一括"), "the footer names the key: {hint}");
    ed.message = String::from("before");
    ed.input(Key::Z);
    assert_ne!(ed.message, "before", "the key answers");
    assert!(
        ed.message.contains("公式") || ed.message.contains("クエスト"),
        "and it answers about the bundle: {}",
        ed.message
    );
}

/// B9: the クエスト改造 footer named [A] and [◀/▶] on every row, and each tab has
/// rows that refuse one of the two.
#[test]
fn the_quest_footer_follows_the_row() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_BASIC);
    ed.quest_edit_cursor = 0;
    let text_row = ed.footer_hint();
    assert!(text_row.contains("[A] テキスト編集"), "{text_row}");
    assert!(
        !text_row.contains("[◀/▶] 制限時間"),
        "a text row does not promise the arrows: {text_row}"
    );
    ed.quest_edit_cursor = ed.quest_value_rows() - 1;
    let time_row = ed.footer_hint();
    assert!(time_row.contains("[◀/▶] 制限時間"), "{time_row}");
    assert!(
        !time_row.contains("[A] テキスト編集"),
        "the time row has no text to edit: {time_row}"
    );

    // モンスター: the five .mib numbers have a list behind them, the five .sem
    // placement values do not.
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    ed.quest_edit_field = 0;
    let mib_row = ed.footer_hint();
    assert!(mib_row.contains("[A] 選択"), "{mib_row}");
    ed.quest_edit_field = mhnx_core::hbedit::QUEST_MON_SEM_X;
    let sem_row = ed.footer_hint();
    assert!(
        !sem_row.contains("[A] 選択"),
        "a .sem value has no list: {sem_row}"
    );
    assert!(sem_row.contains("[◀/▶] 値"), "{sem_row}");

    // 報酬: the held [ZR] 検索 is named; 支給品 has one value, so [ZR] there is
    // the search on both the tap and the hold.
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    let rewards = ed.footer_hint();
    assert!(rewards.contains("[ZR長] 検索"), "{rewards}");
    // Two stops: アイテム is [A]'s cell, so [ZR] walks 個数 ⇄ 確率% (the sixth
    // review's leftovers - the third stop could only refuse the arrows).
    assert!(
        rewards.contains("[ZR] 段送り（2段）"),
        "the two cells are named: {rewards}"
    );
    ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    let supplies = ed.footer_hint();
    assert!(supplies.contains("[ZR長] 検索"), "{supplies}");

    // Every one of them fits the strip the console gives the footer.
    for tab in [
        QUEST_TAB_BASIC,
        QUEST_TAB_MONSTERS,
        QUEST_TAB_REWARDS,
        QUEST_TAB_SUPPLIES,
    ] {
        ed.quest_set_tab(tab);
        for field in [0usize, mhnx_core::hbedit::QUEST_MON_SEM_X] {
            ed.quest_edit_field = field;
            let hint = ed.footer_hint();
            assert!(
                caption_width(&hint) <= FOOTER_HINT_ROOM,
                "tab {tab} field {field} does not fit: {}px {}",
                caption_width(&hint),
                hint
            );
        }
    }
}

/// B10: "この tab は" mixed an English word into a Japanese sentence, and the
/// audit that guards the status lines only asked for *one* non-ASCII character -
/// which a sentence with a Japanese word in it always has.
#[test]
fn the_tab_word_is_japanese() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_BASIC);
    ed.input(Key::R);
    assert!(ed.message.contains("このタブは"), "{}", ed.message);
    assert!(!ed.message.contains("tab"), "{}", ed.message);
    // The audit that guards this is review_17's, and it now refuses an English
    // word of two or more letters in a status line.
    assert!(
        !hbedit_src().contains("この tab は"),
        "the mixed spelling is gone from the core"
    );
}

// ---------------------------------------------------------------------------
// C. regressions
// ---------------------------------------------------------------------------

/// C11: select_sub_tab's Gear arm called open_charm_form_selected, so [R] - the
/// shoulder that walks the tab strip - opened an edit form on whatever row the
/// cursor was on. A tab press is navigation; the form is [A]'s.
#[test]
fn the_charm_tab_lights_and_leaves_the_list() {
    let mut ed = editor_with_charms(3);
    ed.open_charms();
    ed.cursor = 1;
    ed.charm_tab_press();
    assert_eq!(ed.screen, Screen::Charms, "the tab press opens no form");
    assert!(ed.charm_edit.is_none());
    assert_eq!(ed.charm_tab, 1, "the pressed tab is lit");
    assert_eq!(ed.cursor, 1, "and the row is kept");
    assert!(ed.message.contains("[A]"), "{}", ed.message);
    // The list's [A] is still the way in.
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    // The console's tab strip calls the tab method, not the form opener.
    let body = fn_body(ui_src(), "pub fn select_sub_tab");
    assert!(body.contains("ed.charm_tab_press()"), "{body}");
    assert!(
        !body.contains("ed.open_charm_form_selected()"),
        "a tab press must not open a form: {body}"
    );
}

/// C12: select_sub_tab exempted クエスト改造 from the "the lit tab is not a
/// move" guard, so a tap on the lit tab put the cursor back on row 0.
#[test]
fn the_lit_quest_tab_keeps_the_row() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    ed.quest_edit_cursor = 3;
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    assert_eq!(
        ed.quest_edit_cursor, 3,
        "a press on the lit tab is not a move"
    );
    // A real move still resets the row: the tab owns its own cursor.
    ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    assert_eq!(ed.quest_edit_cursor, 0);
    // The console returns before the jump, with no screen exempt.
    let body = fn_body(ui_src(), "pub fn select_sub_tab");
    let guard = &body[..body.find("form_leave_press").unwrap_or(body.len())];
    assert!(
        guard.contains("if idx == self.sub_tabs_for(ed).1"),
        "{guard}"
    );
    assert!(
        !guard.contains("Screen::QuestEdit"),
        "no screen is exempt from the guard: {guard}"
    );
}

// ---------------------------------------------------------------------------
// D. consistency
// ---------------------------------------------------------------------------

/// D13: form_discard_press (the form's [B]) and form_leave_press (a jump)
/// shared one flag, so the question one key asked was spent by the other.
#[test]
fn the_discard_question_belongs_to_the_key_that_asked() {
    let mut ed = editor_with_charms(2);
    ed.open_charms();
    ed.charm_a_press();
    ed.input(Key::Right);
    assert!(ed.charm_draft_dirty());

    // [B] asks.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::CharmEdit, "the first [B] only asks");
    assert!(ed.form_discard_armed());
    // A jump asks its own question: it does not answer [B]'s.
    assert!(ed.form_leave_press(), "the jump asks too");
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(ed.form_discard_armed());
    // [B] again asks [B]'s question rather than spending the jump's.
    ed.input(Key::B);
    assert_eq!(
        ed.screen,
        Screen::CharmEdit,
        "the jump's answer is not [B]'s to spend"
    );
    // And now [B]'s own second press goes through.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms, "the second [B] leaves");
    assert!(!ed.form_discard_armed());
}

/// D14: the progress screen named the same keys three different ways - the
/// footer said [Y] タブ, the guide said [L/R], and the text build's key line
/// still said "Y 既読".
#[test]
fn the_progress_keys_agree_in_all_three_places() {
    let mut ed = editor_with_quest();
    ed.open_progress();
    ed.progress_tab = PROGRESS_QUESTS;

    // 1. The footer.
    let hint = ed.footer_hint();
    assert!(hint.contains("[A] ビット"), "{hint}");
    assert!(hint.contains("[ZR長] 既読"), "{hint}");
    assert!(hint.contains("[Y] タブ"), "{hint}");
    assert!(hint.contains("[ZR] 検索"), "{hint}");

    // 2. The guide: the screen's sentence plus the first two facts, printed
    // exactly as the footer prints them. The guide and the footer are one
    // table (Editor::key_facts), so the two cannot disagree about a key.
    let guide = ed.screen_guide();
    for fact in ed.guide_facts() {
        assert!(guide.contains(&fact.render()), "{guide}");
    }
    assert!(!guide.contains("[L/R]"), "the guide names no shoulder pair: {guide}");

    // 3. The text build's key row: the body names no key any more (the seventh
    // review), so the row under it - the table - is what the console prints.
    let keys: String = ed
        .key_facts()
        .into_iter()
        .map(|f| f.render())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(keys.contains("[ZR長] 既読"), "{keys}");
    assert!(keys.contains("[Y] タブ"), "{keys}");
    assert!(keys.contains("[ZR] 検索"), "{keys}");
    assert!(!keys.contains("[Y] 既読"), "{keys}");

    // The award view names the tab key and not the second bit.
    ed.progress_tab = PROGRESS_AWARDS;
    let hint = ed.footer_hint();
    assert!(hint.contains("[Y] タブ"), "{hint}");
    assert!(!hint.contains("既読"), "{hint}");
}

/// D15: the talisman list's [＋] called open_charms first, which put the list
/// cursor back at the top; the equipment box's [＋] has always kept its row.
#[test]
fn the_new_talisman_key_keeps_the_list_row() {
    let mut ed = editor_with_charms(5);
    ed.open_charms();
    ed.cursor = 4;
    ed.input(Key::L); // what the console's [＋] arm sends
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    assert_eq!(ed.cursor, 4, "the list row is kept");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms);
    assert_eq!(ed.cursor, 4, "and it is still there after [B]");

    // The state machine's own [＋] answers the same way.
    let mut ed = editor_with_charms(5);
    ed.open_charms();
    ed.cursor = 3;
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert_eq!(ed.cursor, 3);

    // The console no longer goes through open_charms (which resets both).
    let body = fn_body(ui_src(), "Screen::Charms => match key");
    assert!(!body.contains("ed.open_charms()"), "{body}");
}

/// D16: a tab step set the list's cursor to 0 (装備 / 狩猟記録 / オトモ), so
/// crossing to a tab and back cost the row the user had reached.
#[test]
fn a_tab_step_keeps_the_row() {
    // 装備BOX.
    let mut ed = editor_with_equipment(3);
    ed.open_equip_box();
    ed.equip_cursor = 2;
    ed.screen = Screen::Slots;
    ed.open_equip_box();
    assert_eq!(ed.equip_cursor, 2, "the box keeps its row");

    // 狩猟記録: the list shows every monster when the filter is off.
    let mut ed = editor_with_equipment(1);
    ed.hunt_show_all = true;
    ed.hunt_cursor = 5;
    ed.screen = Screen::Slots;
    ed.open_hunts();
    assert_eq!(ed.hunt_cursor, 5, "the hunt list keeps its row");

    // 所持オトモ.
    let mut ed = editor_with_palicos();
    ed.roster_cursor = 2;
    ed.screen = Screen::Slots;
    ed.open_roster();
    assert_eq!(ed.roster_cursor, 2, "the roster keeps its row");
}

/// D17: the 報酬 tab's [ZR] walks the row's three cells - アイテム / 個数 /
/// 確率% - the same count the row's own screen walks.
#[test]
fn the_reward_value_switch_has_two_stops() {
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    // The row still holds three cells; [ZR] walks the two the arrows can move.
    assert_eq!(QUEST_REWARD_FIELDS, 3, "アイテム / 個数 / 確率%");
    assert_eq!(ed.quest_reward_field, QUEST_REWARD_COUNT);
    ed.input(Key::Z);
    assert_eq!(ed.quest_reward_field, QUEST_REWARD_CHANCE);
    ed.input(Key::Z);
    assert_eq!(
        ed.quest_reward_field, QUEST_REWARD_COUNT,
        "two stops, and back to the count - アイテム is [A]'s"
    );
    assert!(ed.quest_reward_field != QUEST_REWARD_ITEM);
    // The search is its own press on both tabs.
    ed.input(Key::R);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_QUEST_ITEM)
    );
    let hint = ed.footer_hint();
    assert!(hint.contains("[ZR長] 検索"), "{hint}");
    assert!(hint.contains("[ZR] 段送り（2段）"), "{hint}");
}

/// D18: the two record forms' pills offered the shared table's ±100 / ±1000 /
/// ±10000, and their arrows clamp to ±10 - three pills that changed nothing.
#[test]
fn the_forms_offer_only_the_steps_they_take() {
    assert_eq!(FORM_STEPS, [1, 10]);
    assert_eq!(hbedit::form_step(0), 1);
    assert_eq!(hbedit::form_step(1), 10);
    assert_eq!(
        hbedit::form_step(4),
        10,
        "the shared table's 10,000 is a 10 on a form"
    );

    let mut ed = editor_with_charms(2);
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(
        ed.step_table().0,
        &FORM_STEPS,
        "the pills are the steps the arrows take"
    );
    // The arrow really takes the step the lit pill names.
    ed.step = 4;
    ed.charm_edit.as_mut().expect("form").field = 0;
    ed.charm_edit.as_mut().expect("form").index = 5;
    ed.input(Key::Right);
    assert_eq!(ed.charm_edit.expect("form").index, 15, "10, not 10,000");

    let mut ed = editor_with_equipment(1);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit);
    assert_eq!(ed.step_table().0, &FORM_STEPS);
}

/// D19: the talisman form said "BOXへ反映" and the equipment form said
/// "書き込み" for the same press.
#[test]
fn the_two_forms_use_one_word_for_the_write() {
    assert_eq!(CharmAction::WriteToBox.label(), "書き込み");
    assert_eq!(EquipFormAction::WriteForm.label(), "書き込み");
    assert_eq!(
        CharmAction::WriteToBox.label(),
        EquipFormAction::WriteForm.label(),
        "one word for one action"
    );
    let mut ed = editor_with_charms(1);
    ed.open_charms();
    ed.charm_a_press();
    ed.charm_edit.as_mut().expect("form").field = 0;
    assert!(
        ed.footer_hint().contains("[A] 書き込み"),
        "{}",
        ed.footer_hint()
    );
    assert!(!hbedit_src().contains("BOXへ反映"), "the second word is gone");
}

/// D20: the footer said "[R] 数値" and the deck's button said "[R] 数値入力".
///
/// The word is the key table's now, and the inspector's pill borrows it
/// (`Editor::key_label`), so one key has one name on one screen - which is the
/// finding. The key moved too: the number keyboard is the held [ZR] on every
/// screen now (the shoulders are both tab keys), so the chip is one token -
/// "ZR長" - with one short word beside it, and the pill borrows that.
#[test]
fn the_number_key_has_one_name() {
    let mut ed = editor_with_equipment(1);
    ed.screen = Screen::Items;
    let label = ed
        .key_label("ZR長")
        .expect("the item box names the held [ZR]");
    assert!(
        ed.footer_hint().contains(&format!("[ZR長] {label}")),
        "{}",
        ed.footer_hint()
    );
    ed.screen = Screen::Hunts;
    assert!(
        ed.footer_hint().contains("[ZR長] 数値"),
        "{}",
        ed.footer_hint()
    );
    // A reward / supply row holds numbers too.
    let mut ed = editor_with_quest();
    ed.open_quest_from_save(QUEST_ID);
    let rem = ed
        .arc
        .as_ref()
        .expect("the arc")
        .entries
        .iter()
        .position(|e| e.name.ends_with("rem_620114"))
        .expect("the reward table");
    ed.entry = rem;
    ed.screen = Screen::EntryView;
    assert!(ed.entry_has_numbers());
    assert!(
        ed.footer_hint().contains("[ZR長] 数値"),
        "{}",
        ed.footer_hint()
    );
    // The console's pill prints the table's word instead of its own copy, so
    // the two surfaces cannot drift apart again.
    let ui = ui_src();
    // Every 数値入力 button borrows the table's word: the inspector pill, the
    // reward / supply panel's button and the card's button.
    // The sixth review's leftovers: the buttons name no key at all, so they
    // borrow the word through pill_word (the table's word, no bracket).
    // Four: the card's inspector, the reward / supply panel, the arena
    // inspector and the weapon inspector (the seventh review added the last).
    assert_eq!(
        ui.matches("pill_word(ed, \"ZR長\"").count(),
        4,
        "every 数値入力 button borrows the table's word"
    );
    // Only the code lines: the comments explain the key (they say why each
    // button exists).
    let code: String = ui
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!code.contains("[ZR長] 数値"), "no button prints the key");
    assert!(!code.contains("[R] 数値"), "and no button promises [R]");
    assert!(
        !hbedit_src().contains("[R] 数値  "),
        "and the footers do not carry two of them"
    );
}

// ---------------------------------------------------------------------------
// The hole the third review left: a footer is a claim, so press the key
// ---------------------------------------------------------------------------

/// The keys a footer line names, as Key variant names.
fn named_keys(text: &str) -> Vec<String> {
    let table: &[(&str, &[&str])] = &[
        ("[A]", &["A"]),
        ("[B]", &["B"]),
        ("[X]", &["X"]),
        ("[Y]", &["Y"]),
        ("[ZR]", &["Z"]),
        // The held [ZR]: the job [R] used to carry. It is pressed the way the
        // console presses it (Editor::zr_hold_press), not as a bare Z.
        ("[ZR長]", &["ZR長"]),
        ("[＋]", &["Plus"]),
        ("[-]", &["Minus"]),
        ("[◀/▶]", &["Left", "Right"]),
        ("[▲/▼]", &["Up", "Down"]),
        ("[L/R]", &["L", "R"]),
        ("[R]", &["R"]),
        ("[R長押し]", &["R"]),
    ];
    let mut out: Vec<String> = Vec::new();
    for (token, keys) in table {
        if text.contains(token) {
            for k in *keys {
                if !out.iter().any(|h| h == k) {
                    out.push((*k).to_string());
                }
            }
        }
    }
    out
}

fn key_of(name: &str) -> Key {
    match name {
        "A" => Key::A,
        "B" => Key::B,
        "X" => Key::X,
        "Y" => Key::Y,
        "Z" => Key::Z,
        "Plus" => Key::Plus,
        "Minus" => Key::Minus,
        "Left" => Key::Left,
        "Right" => Key::Right,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "L" => Key::L,
        "R" => Key::R,
        other => panic!("no key called {other}"),
    }
}

/// The keys the console answers before the state machine sees them
/// (switch/src/ui.rs handle_pad_key), per screen. The same list
/// tests/key_hints.rs documents; a key here is a key the console acts on, so
/// the state machine is not the layer that has to answer it.
const CONSOLE_KEYS: &[(&str, &[&str])] = &[
    ("SaveList", &["Y"]),
    ("Slots", &["Up", "Down", "Left", "Right", "A", "Y", "B"]),
    ("Char", &["Up", "Down", "A", "Y"]),
    ("Items", &["A", "Y", "R"]),
    ("ItemPick", &["A"]),
    ("Charms", &["A", "Minus", "Plus"]),
    ("Roster", &["Y"]),
    ("Palico", &["Y", "Left", "Right"]),
    ("Hunts", &["Y", "A", "R"]),
    ("Sets", &["Up", "Down", "A", "Left", "Right", "Y", "B"]),
    ("ArcList", &["Y", "Z", "B"]),
    ("ArcEntries", &["A", "Y"]),
    ("QuestEdit", &["A"]),
    ("Mib", &["A", "Z", "Y", "Left", "Right"]),
    ("Gmd", &["B"]),
    ("Options", &["Y"]),
    ("EntryView", &["Up", "Down", "Y", "Left", "Right", "A"]),
];

fn screen_name(screen: Screen) -> &'static str {
    match screen {
        Screen::SaveList => "SaveList",
        Screen::Slots => "Slots",
        Screen::Char => "Char",
        Screen::Items => "Items",
        Screen::ItemPick => "ItemPick",
        Screen::SkillPick => "SkillPick",
        Screen::JewelPick => "JewelPick",
        Screen::EquipPick => "EquipPick",
        Screen::Charms => "Charms",
        Screen::CharmEdit => "CharmEdit",
        Screen::Equip => "Equip",
        Screen::EquipEdit => "EquipEdit",
        Screen::QuestEquip => "QuestEquip",
        Screen::SaveQuests => "SaveQuests",
        Screen::ArcList => "ArcList",
        Screen::QuestEdit => "QuestEdit",
        Screen::ArcEntries => "ArcEntries",
        Screen::EntryView => "EntryView",
        Screen::Mib => "Mib",
        Screen::Gmd => "Gmd",
        Screen::Check => "Check",
        Screen::Hunts => "Hunts",
        Screen::Guild => "Guild",
        Screen::Progress => "Progress",
        Screen::Roster => "Roster",
        Screen::Palico => "Palico",
        Screen::Sets => "Sets",
        Screen::Csv => "Csv",
        Screen::Options => "Options",
        Screen::SaveMap => "SaveMap",
        Screen::Review => "Review",
        Screen::Convert => "Convert",
    }
}

fn console_answers(screen: Screen, key: &str) -> bool {
    let name = screen_name(screen);
    CONSOLE_KEYS
        .iter()
        .any(|(s, keys)| *s == name && keys.contains(&key))
}

/// Everything a key press can change, as one string.
///
/// Tuples of ten, because format! takes one argument per placeholder and a
/// mistyped count is a compile error - which is the point: a field left out of
/// this list would make a press look like it did nothing.
fn fingerprint(ed: &Editor) -> String {
    format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        (
            ed.screen,
            ed.message.clone(),
            ed.cursor,
            ed.field,
            ed.dirty,
            ed.step,
            ed.undo_depth(),
            ed.quit,
            ed.quit_armed(),
            ed.form_discard_armed(),
        ),
        (
            ed.item_menu_open,
            ed.charm_tab,
            ed.quest_edit_tab,
            ed.quest_edit_cursor,
            ed.quest_edit_field,
            ed.quest_reward_field,
            ed.progress_tab,
            ed.guild_view(),
            ed.guild_field,
            ed.equip_cursor,
        ),
        (
            ed.hunt_cursor,
            ed.roster_cursor,
            ed.palico_cursor,
            ed.save_quest_cursor,
            ed.csv_cursor,
            ed.map_cursor,
            ed.rev_cursor,
            ed.option_cursor,
            ed.mib_cursor,
            ed.gmd_cursor,
        ),
        (
            ed.check_cursor,
            ed.entry_row_cursor,
            ed.item_pick_cursor,
            ed.skill_pick_cursor,
            ed.jewel_pick_cursor,
            ed.equip_pick_cursor,
            ed.item_menu_cursor,
            ed.charm_edit.is_some(),
            ed.equip_edit.is_some(),
            ed.set_edit.is_some(),
        ),
        (
            ed.item_set_edit.is_some(),
            ed.palico_equip.is_some(),
            ed.palico_skill.is_some(),
            ed.qe_open,
            ed.arena_edit,
            ed.guild_wpn_edit,
            ed.map_edit.is_some(),
            ed.review.is_some(),
            ed.convert_armed,
            ed.csv_armed.is_some(),
        ),
        (
            ed.save_quest_armed.is_some(),
            ed.charm_delete_armed.is_some(),
            ed.equip_delete_armed.is_some(),
            ed.item_armed.is_some(),
            ed.hunt_show_all,
            ed.show_empty,
            ed.arena_row,
            ed.arena_rank,
            ed.guild_count_cursor,
            ed.guild_wpn_row,
        ),
        (
            ed.award_cursor,
            ed.request_cursor,
            ed.quest_cursor,
            ed.guild_wpn_venue,
            ed.item_pick_stage,
            ed.quest_tab,
            ed.palico_lang,
            ed.item_pick_mode,
            ed.item_pick_add_id,
            ed.item_pick_add_count,
        ),
        (
            ed.equip_kind_pick_pending,
            ed.palico_delete_armed.is_some(),
            ed.mib_preset_armed.is_some(),
            ed.sem_preset_armed.is_some(),
            ed.set_clear_armed.is_some(),
            ed.item_set_armed.is_some(),
            ed.equip_kind_armed.is_some(),
            ed.rev_armed,
            ed.item_pick_target,
            ed.qe_item_cursor,
        ),
        // The forms' own values: a key that steps a field inside a form changes
        // nothing else, so a fingerprint of the screen alone would call it dead.
        (
            ed.charm_edit.map(|c| {
                (
                    c.index, c.kind, c.slots, c.skill1, c.points1, c.skill2, c.points2, c.decorations,
                    c.field,
                )
            }),
            ed.equip_edit.map(|e| {
                (
                    e.index,
                    e.kind,
                    e.id,
                    e.level,
                    e.transmog_id,
                    e.transmog_level,
                    e.decorations,
                    e.field,
                    e.new,
                )
            }),
            ed.set_edit.map(|e| (e.set, e.field, e.pick)),
            ed.item_set_edit.map(|e| (e.set, e.field, e.pick)),
            ed.palico_equip.map(|p| (p.palico, p.field, p.pick)),
            ed.palico_skill.map(|p| (p.palico, p.tab, p.field, p.picks)),
            ed.map_edit.map(|m| (m.byte, m.pending)),
            ed.qe_target,
        ),
    )
}

/// Press one key the way the console would, and answer whether anything
/// happened.
fn key_arrives(ed: &mut Editor, key: &str) -> bool {
    let before = fingerprint(ed);
    match key {
        // [X] is the console's QuickSave and [＋] its quit key: the main loop
        // answers both before the state machine (switch/src/editor.rs), so the
        // probe presses what the console presses.
        "X" => ed.quick_save(),
        "Plus" => {
            ed.press_quit();
        }
        "L" | "R" => {
            let k = if key == "R" { Key::R } else { Key::L };
            match ed.shoulder_job(k) {
                // The console moves the tab: the answer is the arrival.
                Shoulder::SubTab => return true,
                // A form that owns the screen; a footer must not name it.
                Shoulder::Swallow => return false,
                Shoulder::Move => ed.input(if key == "R" { Key::Right } else { Key::Left }),
                // A panel that owns every key (the palico skill panel).
                Shoulder::Panel => ed.input(k),
            }
        }
        // The held [ZR]'s job, run the way the console runs it.
        "ZR長" => {
            ed.zr_hold_press();
        }
        other => ed.input(key_of(other)),
    }
    fingerprint(ed) != before
        || ed.take_text_request().is_some()
        || ed.take_write().is_some()
        || ed.take_request().is_some()
}

/// One state to audit: a label and the setup that puts an editor in it.
type Setup = fn(&mut Editor);

const STATES: &[(&str, Setup)] = &[
    ("SaveList", |ed| ed.screen = Screen::SaveList),
    ("Slots", |ed| ed.screen = Screen::Slots),
    ("Char", |ed| ed.screen = Screen::Char),
    ("Items", |ed| ed.screen = Screen::Items),
    ("Items menu", |ed| {
        ed.screen = Screen::Items;
        ed.open_item_menu();
    }),
    // The four pickers are opened the way the screens open them: a bare
    // Screen::ItemPick has no list behind it, and the footer describes a list.
    // The four pickers are opened the way the screens open them (a bare
    // Screen::ItemPick has no list behind it), and the cursor starts in the
    // middle: [◀] saturates at the top of a list, so a probe on row 0 would
    // read as a dead key.
    ("ItemPick", |ed| {
        ed.screen = Screen::Items;
        // [A] is the picker on the item box (the console's chip table says
        // "[A] 種類変更"); [R] is the count keyboard.
        ed.input(Key::A);
        ed.item_pick_cursor = 20;
    }),
    ("SkillPick", |ed| {
        ed.open_charms();
        ed.charm_a_press();
        ed.charm_edit.as_mut().expect("form").field = 3;
        ed.input(Key::A);
        ed.skill_pick_cursor = 20;
    }),
    ("JewelPick", |ed| {
        ed.open_charms();
        ed.charm_a_press();
        ed.charm_edit.as_mut().expect("form").field = 7;
        ed.input(Key::A);
        ed.jewel_pick_cursor = 20;
    }),
    ("EquipPick", |ed| {
        ed.open_equip_box();
        ed.equip_cursor = 0;
        ed.input(Key::A);
        ed.equip_edit.as_mut().expect("form").field = 8;
        ed.input(Key::A);
        ed.equip_pick_cursor = 20;
    }),
    ("Charms", |ed| ed.open_charms()),
    // The three answers the talisman list's [A] gives, as three states: the
    // selected talisman's form, the first record of an empty box, and the
    // refusal a search with no hits gets. The button said 編集 over all three.
    ("Charms 空", |ed| {
        ed.save = Some(save_with_charms(0));
        ed.open_charms();
    }),
    ("Charms 検索0件", |ed| {
        ed.open_charms();
        ed.charm_filter = String::from("zzzznothing");
    }),
    // The item box with a search that hides every row: [A] and the arrows have
    // nothing to act on, and both now say why instead of answering silence.
    ("Items 検索0件", |ed| {
        ed.screen = Screen::Items;
        ed.item_box_filter = String::from("zzzznothing");
    }),
    ("CharmEdit", |ed| {
        ed.open_charms();
        ed.charm_a_press();
    }),
    ("Equip", |ed| ed.open_equip_box()),
    ("EquipEdit", |ed| {
        ed.open_equip_box();
        ed.input(Key::A);
    }),
    ("QuestEquip", |ed| ed.screen = Screen::QuestEquip),
    ("QuestEquip 枠", |ed| {
        ed.screen = Screen::QuestEquip;
        ed.qe_open = true;
    }),
    ("SaveQuests", |ed| ed.screen = Screen::SaveQuests),
    ("ArcList", |ed| ed.screen = Screen::ArcList),
    ("QuestEdit 基本", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.quest_set_tab(QUEST_TAB_BASIC);
    }),
    ("QuestEdit モンスター", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.quest_set_tab(QUEST_TAB_MONSTERS);
    }),
    ("QuestEdit 報酬", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.quest_set_tab(QUEST_TAB_REWARDS);
    }),
    ("QuestEdit 支給品", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    }),
    ("ArcEntries", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.screen = Screen::ArcEntries;
    }),
    ("EntryView", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.screen = Screen::EntryView;
    }),
    // The reward table: the row holds three cells and [A] is the cell's own
    // action (hbedit::EntryAction), so the footer's [A] word follows the cell.
    ("EntryView 報酬", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.entry = ed
            .arc
            .as_ref()
            .and_then(|a| a.entries.iter().position(|e| e.name.contains("rem_")))
            .expect("the reward table");
        ed.entry_field = 0;
        ed.entry_row_cursor = 0;
        ed.screen = Screen::EntryView;
    }),
    // .mib on the two kinds of row: [ZR] walks a monster slot's eight values and
    // has nothing to do on the other ninety, so the footer names it only there.
    ("Mib モンスター行", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.screen = Screen::Mib;
        ed.mib_cursor = mhnx_core::questfields::MIB_FIELDS
            .iter()
            .position(|f| f.key.starts_with("mob_slot["))
            .expect("a monster row");
    }),
    ("Mib 通常行", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.screen = Screen::Mib;
        ed.mib_cursor = mhnx_core::questfields::MIB_FIELDS
            .iter()
            .position(|f| !f.key.starts_with("mob_slot["))
            .expect("a plain row");
    }),
    ("Gmd", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.screen = Screen::Gmd;
    }),
    ("Check", |ed| {
        ed.open_quest_from_save(QUEST_ID);
        ed.screen = Screen::Check;
    }),
    // 狩猟記録 lists every monster once the filter is off; the default list is
    // the records the save holds, and this save holds none.
    ("Hunts", |ed| {
        ed.hunt_show_all = true;
        ed.open_hunts();
        ed.hunt_cursor = 5;
    }),
    ("Guild カード", |ed| {
        ed.screen = Screen::Guild;
        ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_CARD);
    }),
    ("Guild 回数", |ed| {
        ed.screen = Screen::Guild;
        ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_COUNTS);
    }),
    ("Guild 武器", |ed| {
        ed.screen = Screen::Guild;
        ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_WEAPONS);
    }),
    ("Guild アリーナ", |ed| {
        ed.screen = Screen::Guild;
        ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_ARENA);
    }),
    // The three views page by 50 rows and clamp at both ends, so the probe
    // starts in the middle of each list.
    ("Progress 勲章", |ed| {
        ed.open_progress();
        ed.set_progress_cursor(60);
    }),
    ("Progress 依頼", |ed| {
        ed.open_progress();
        ed.progress_tab = PROGRESS_REQUESTS;
        ed.set_progress_cursor(60);
    }),
    ("Progress クエスト", |ed| {
        ed.open_progress();
        ed.progress_tab = PROGRESS_QUESTS;
        ed.set_progress_cursor(60);
    }),
    ("Roster", |ed| ed.open_roster()),
    ("Palico", |ed| ed.screen = Screen::Palico),
    ("Sets", |ed| ed.screen = Screen::Sets),
    ("Sets 装備フォーム", |ed| {
        ed.screen = Screen::Sets;
        ed.open_set_editor(0);
    }),
    ("Sets アイテムフォーム", |ed| {
        ed.screen = Screen::Sets;
        ed.open_item_set_editor(0);
    }),
    ("Csv", |ed| ed.open_csv()),
    ("Options", |ed| ed.screen = Screen::Options),
    ("SaveMap", |ed| ed.open_save_map(Screen::Char)),
    ("SaveMap 編集", |ed| {
        ed.open_save_map(Screen::Char);
        ed.map_edit_enter();
    }),
    // 変更一覧 compares the open save with the bytes the card held, so the
    // probe hands over a reference, makes one edit and opens the list on a row
    // that can move up.
    ("Review", |ed| {
        let bytes = ed.save.as_ref().expect("the save").to_bytes();
        ed.set_review_reference(&bytes);
        ed.maximize_character();
        ed.open_review();
        ed.rev_cursor = 1;
    }),
    ("Convert", |ed| ed.screen = Screen::Convert),
];

/// A save with every record the screens read: talismans, equipment, palicos
/// and one download quest.
fn rich_editor() -> Editor {
    let mut save = save_with_charms(3);
    for i in 0..3 {
        mhnx_core::equipops::create(&mut save, 0, 7, 40 + i as u16, 1).expect("a record");
    }
    for (i, name) in ["アイルー", "メラルー", "トラ"].iter().enumerate() {
        mhnx_core::palico::set_name(&mut save, 0, i, name);
    }
    // One item in the pouch, so the item box has a row to step.
    let mut items = save.read_item_box(0).expect("a blank box");
    items[0] = mhnx_core::itembox::ItemSlot { id: 1, count: 5 };
    save.write_item_box(0, &items).expect("write the box");
    let arc = quest_arc_bytes();
    let base = save.dlc_section_offset() as usize + mhnx_core::dlc::QUEST_ARRAY_OFFSET;
    let body = save.body_mut();
    body[base..base + 4].copy_from_slice(&QUEST_ID.to_le_bytes());
    body[base + 4..base + 8].copy_from_slice(&(arc.len() as i32).to_le_bytes());
    body[base + 8..base + 8 + arc.len()].copy_from_slice(&arc);
    editor_with(save, Screen::Slots)
}

/// The hole the third review left: the key audits read the footer and the
/// handler as *text*, so a footer could name a key the screen answers with a
/// sentence and nothing else. This one presses the key and requires the state to
/// move.
#[test]
fn every_key_the_footer_names_arrives() {
    let mut checked = 0usize;
    for (label, setup) in STATES {
        // The footer of this state, read once.
        let mut probe = rich_editor();
        setup(&mut probe);
        let hint = probe.footer_hint();
        assert!(
            caption_width(&hint) <= FOOTER_HINT_ROOM,
            "{label}: the footer does not fit the strip: {}px {hint}",
            caption_width(&hint)
        );
        for key in named_keys(&hint) {
            if console_answers(probe.screen, &key) {
                // The console answers it before the state machine (and
                // tests/key_hints.rs pins that list against the source).
                continue;
            }
            // Every key is pressed on its own copy of the state: a press that
            // closes a menu must not make the next key look dead.
            let mut ed = rich_editor();
            setup(&mut ed);
            // The setup leaves the editor idle: a request queued here would
            // make the press look like it answered.
            assert!(
                ed.take_text_request().is_none(),
                "{label}: the setup queued a text request"
            );
            assert!(
                ed.take_write().is_none(),
                "{label}: the setup queued a write"
            );
            assert!(
                ed.take_request().is_none(),
                "{label}: the setup queued an I/O request"
            );
            assert!(
                key_arrives(&mut ed, &key),
                "{label}: the footer names [{key}] and nothing happens: {hint}"
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "the footers were pressed: {checked} keys");
}
