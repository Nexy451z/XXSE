//! The fourth review's leftovers, one test per finding.
//!
//! The console layer (switch/src) is not part of this workspace, so the tests
//! that need it read it as text - but only for the *wiring*. Everything a state
//! transition can answer is driven here: the key map, the two forms, the
//! footers and the labels the buttons print.
//!
//! Nothing here reads or writes a card: every save is built in memory.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::charm;
use mhnx_core::hbedit::{caption_width, Editor, Key, Screen, FOOTER_HINT_ROOM, PROGRESS_QUESTS};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};
use mhnx_core::questprogress::{self, Progress};
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

fn switch_editor_src() -> &'static str {
    include_str!("../../../switch/src/editor.rs")
}

fn swkbd_src() -> &'static str {
    include_str!("../../../switch/src/swkbd.rs")
}

fn gui_src() -> &'static str {
    include_str!("../../mhnx-gui/src/app.rs")
}

fn readme() -> &'static str {
    include_str!("../../../README.md")
}

/// The body of one \`fn\` in a source text, from its signature to its closing
/// brace (depth counting, the same way tests/key_hints.rs cuts a handler out).
fn fn_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} is gone - the audit must be updated"));
    let open = src[start..]
        .find('{')
        .map(|i| start + i)
        .expect("a body");
    let mut depth = 0i32;
    let mut end = open;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    &src[start..=end]
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn blank_save() -> Save {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
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

fn entry(name: &str, type_hash: u32, data: Vec<u8>) -> ArcEntry {
    ArcEntry {
        name: String::from(name),
        type_hash,
        data,
        original: None,
        original_offset: None,
    }
}

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

fn sem_bytes() -> Vec<u8> {
    let mut v = vec![0u8; 32];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v
}

fn mib_bytes() -> Vec<u8> {
    let mut mib = vec![0u8; MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&(QUEST_ID as u32).to_le_bytes());
    mib
}

fn gmd_bytes() -> Vec<u8> {
    Gmd {
        version: GMD_VERSION,
        tag: GMD_TAG,
        key: String::from("questData_1010165"),
        strings: vec![String::from("テストクエスト")],
    }
    .to_bytes()
}

fn quest_arc_bytes() -> Vec<u8> {
    Arc {
        version: ARC_VERSION,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry("quest\\boss\\setEmMain\\b_m15em010_00", HASH_SEM, sem_bytes()),
            entry(
                "quest\\boss\\setEmMain\\b_m15em010_00_mib",
                HASH_MIB,
                mib_bytes(),
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
    ed.save_path = Some(String::from("memory:review-19"));
    ed.slot = 0;
    ed.screen = screen;
    ed
}

/// The editor on a reward table: the .rem entry of the synthetic quest.
fn editor_on_the_reward_table() -> Editor {
    let mut ed = editor_with(save_with_one_quest(), Screen::Slots);
    ed.open_quest_from_save(QUEST_ID);
    ed.entry = ed
        .arc
        .as_ref()
        .and_then(|a| a.entries.iter().position(|e| e.name.contains("rem_")))
        .expect("the reward table");
    ed.entry_field = 0;
    ed.entry_row_cursor = 0;
    ed.screen = Screen::EntryView;
    ed
}

fn reward_count(ed: &Editor) -> u8 {
    let e = &ed.arc.as_ref().expect("an arc").entries[ed.entry];
    mhnx_core::entries::ItemTable::parse(&e.data)
        .expect("the table parses")
        .records[0]
        .count
}

fn reward_chance(ed: &Editor) -> u8 {
    let e = &ed.arc.as_ref().expect("an arc").entries[ed.entry];
    mhnx_core::entries::ItemTable::parse(&e.data)
        .expect("the table parses")
        .records[0]
        .slot
}

// ---------------------------------------------------------------------------
// A1. An empty list still draws the panel that holds its way out
// ---------------------------------------------------------------------------

/// The inspector is drawn with \`None\` on a list with no rows.
///
/// The panel is where a finger reaches 全モンスターを表示 (狩猟記録), 検索を解除
/// and [＋] 新規 (装備BOX) - so returning before it left the only way out of an
/// empty list on a screen that had just said there was nothing to do.
#[test]
fn an_empty_list_still_draws_the_inspector_and_its_buttons() {
    let src = ui_src();
    // The panel takes an optional row, and the empty branch passes None.
    assert!(
        src.contains("row: Option<&(usize, String, String, String, String)>,"),
        "the inspector must be drawable without a row"
    );
    let body = fn_body(src, "fn draw_adaptive_rows_warn(");
    let empty = body
        .find("if total == 0 {")
        .expect("the empty-list branch");
    let after = &body[empty..];
    let end = after.find("            return;").expect("the branch returns");
    assert!(
        after[..end].contains("draw_studio_inspector"),
        "the empty branch returns before the inspector is drawn"
    );
    assert!(
        after[..end].contains("None,"),
        "the empty branch must hand the inspector no row"
    );
    // And the deck's own buttons are still drawn: the two the review named.
    let deck = fn_body(src, "fn draw_studio_inspector(");
    assert!(deck.contains("全モンスターを表示"), "the hunt toggle must stay");
    assert!(deck.contains("検索を解除（全件）"), "the filter clear must stay");
    assert!(deck.contains("[＋] 新規装備を追加"), "the new-record button must stay");
}

// ---------------------------------------------------------------------------
// B2. [R] on the item set form arrives
// ---------------------------------------------------------------------------

/// The console lets [R] through to the set form, and the form applies it.
#[test]
fn r_on_the_item_set_form_reaches_the_pouch_copy() {
    let src = ui_src();
    let arm = fn_body(src, "pub fn handle_pad_key(");
    let sets = arm
        .find("Screen::Sets if ed.set_editor_open() => match key {")
        .expect("the set form's arm");
    let tail = &arm[sets..];
    let end = tail.find("_ => {},").expect("the arm's catch-all");
    assert!(
        tail[..end].contains("Key::R"),
        "the set form's allow list must name [R]"
    );

    // And the state machine answers it: the first press arms, the second one
    // writes the pouch.
    let mut ed = editor_with(blank_save(), Screen::Sets);
    assert!(ed.open_item_set_editor(0), "{}", ed.message);
    assert!(ed.set_editor_open());
    ed.input(Key::R);
    assert!(
        ed.message.contains("ポーチ"),
        "the first [R] must offer the copy: {}",
        ed.message
    );
    ed.input(Key::R);
    assert!(
        !ed.message.contains("もう一度"),
        "the second [R] must run it: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// B3. The download palico's language is named everywhere a key is named
// ---------------------------------------------------------------------------

/// [◀/▶] is the language on the palico screen: footer, guide and a button.
#[test]
fn the_palico_language_is_named_in_the_footer_the_guide_and_on_the_deck() {
    let mut ed = editor_with(blank_save(), Screen::Palico);
    ed.palico_cursor = 0;
    let footer = ed.footer_hint();
    assert!(footer.contains("[◀/▶] 言語"), "{footer}");
    assert!(
        caption_width(&footer) <= FOOTER_HINT_ROOM,
        "the footer does not fit: {}px {footer}",
        caption_width(&footer)
    );
    let guide = ed.screen_guide();
    assert!(guide.contains("言語"), "{guide}");

    // The deck: a touch route for the same key (UiAction::TogglePalicoLang).
    let src = ui_src();
    let deck = fn_body(src, "fn draw_studio_inspector(");
    let palico = deck
        .find("Screen::Palico => {")
        .expect("the palico deck branch");
    let tail = &deck[palico..];
    let end = tail.find("Screen::SaveQuests").expect("the next branch");
    assert!(
        tail[..end].contains("UiAction::TogglePalicoLang"),
        "the deck must draw the language button"
    );
    assert!(
        tail[..end].contains("言語"),
        "the button must say what it switches"
    );
}

// ---------------------------------------------------------------------------
// B4. The .mib footer follows the row
// ---------------------------------------------------------------------------

/// [ZR] is named only on a monster row, where it really switches something.
#[test]
fn the_mib_footer_names_zr_only_where_it_works() {
    let mut ed = editor_with(save_with_one_quest(), Screen::Mib);
    ed.open_quest_from_save(QUEST_ID);
    ed.screen = Screen::Mib;

    ed.mib_cursor = mhnx_core::questfields::MIB_FIELDS
        .iter()
        .position(|f| f.key.starts_with("mob_slot["))
        .expect("a monster row");
    assert!(ed.mib_row_is_monster());
    let monster = ed.footer_hint();
    assert!(monster.contains("[ZR] 項目"), "{monster}");

    ed.mib_cursor = mhnx_core::questfields::MIB_FIELDS
        .iter()
        .position(|f| !f.key.starts_with("mob_slot["))
        .expect("a plain row");
    assert!(!ed.mib_row_is_monster());
    let plain = ed.footer_hint();
    assert!(
        !plain.contains("[ZR]"),
        "a row with no sub-field must not promise the key: {plain}"
    );
    assert!(plain.contains("[X] 保存"), "{plain}");
    // The guide says where the key does work.
    assert!(ed.screen_guide().contains("モンスター行"), "{}", ed.screen_guide());
}

// ---------------------------------------------------------------------------
// B5. [A] on an item box a search emptied says why
// ---------------------------------------------------------------------------

/// A search that hides every row leaves [A] with nothing to open - and it says
/// so instead of answering silence.
#[test]
fn a_on_an_empty_item_box_says_why_it_cannot_open_the_picker() {
    let mut ed = editor_with(blank_save(), Screen::Items);
    ed.item_box_filter = String::from("zzzznothing");
    assert!(ed.visible_items().is_empty());
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Items, "nothing to open");
    assert!(!ed.message.is_empty(), "the key must answer");
    assert!(
        ed.message.contains("検索"),
        "the message must name the search: {}",
        ed.message
    );
    // The arrows are silent in the same state, and they say so too.
    ed.message.clear();
    ed.input(Key::Right);
    assert!(!ed.message.is_empty(), "the arrow must answer as well");
}

// ---------------------------------------------------------------------------
// C6. 既読を外す is a label the press can keep
// ---------------------------------------------------------------------------

/// A cleared quest is always also seen, so the pill must not offer to clear it.
#[test]
fn the_seen_pill_does_not_offer_a_press_the_core_refuses() {
    let mut save = blank_save();
    // Index 1 is row 0: cleared writes both bits.
    assert!(questprogress::set(&mut save, 0, Progress::Cleared, 1, true));
    let mut ed = editor_with(save, Screen::Progress);
    ed.open_progress();
    ed.progress_tab = PROGRESS_QUESTS;
    ed.set_progress_cursor(0);
    assert_eq!(ed.progress_quest_state(0), (true, true, false));
    let label = ed.progress_seen_label();
    assert!(
        label.contains("外せません"),
        "a cleared row cannot lose its seen bit: {label}"
    );
    // The press says the same thing, and the two agree.
    ed.input(Key::R);
    assert!(
        ed.message.contains("クリア済み"),
        "the refusal must name the reason: {}",
        ed.message
    );

    // A row that is seen but not cleared keeps the toggle.
    let mut save = blank_save();
    assert!(questprogress::set(&mut save, 0, Progress::Seen, 2, true));
    let mut ed = editor_with(save, Screen::Progress);
    ed.open_progress();
    ed.progress_tab = PROGRESS_QUESTS;
    ed.set_progress_cursor(1);
    assert_eq!(ed.progress_seen_label(), "既読を外す");
}

// ---------------------------------------------------------------------------
// C7. A quest the tables cannot name says so
// ---------------------------------------------------------------------------

/// The README's rule: a row with no name prints 「名前不明」, not a bare number.
#[test]
fn an_unnamed_quest_row_says_the_name_is_unknown() {
    let src = ui_src();
    assert!(
        src.contains("format!(\"クエスト {} 名前不明\", q.quest_id)"),
        "the save's quest list must print 名前不明"
    );
    assert!(
        !src.contains("format!(\"Quest {}\", q.quest_id)"),
        "a bare \"Quest 12345\" reads like a title the game has"
    );
    // The progress screen and the PC build already used the same words.
    assert!(src.contains("クエスト {:04}  名前不明"));
    assert!(gui_src().contains("クエスト {} 名前不明"));
}

// ---------------------------------------------------------------------------
// C8. [A] on an empty talisman box says 作成
// ---------------------------------------------------------------------------

/// The word follows the list: 編集 / 作成 / 0件.
#[test]
fn the_talisman_a_word_follows_the_list() {
    let mut ed = editor_with(save_with_charms(3), Screen::Charms);
    ed.open_charms();
    assert_eq!(ed.charm_a_label(), "編集");
    assert!(ed.footer_hint().contains("[A] 編集"), "{}", ed.footer_hint());

    let mut ed = editor_with(save_with_charms(0), Screen::Charms);
    ed.open_charms();
    assert_eq!(ed.charm_a_label(), "作成");
    assert!(ed.footer_hint().contains("[A] 作成"), "{}", ed.footer_hint());
    assert!(
        ed.screen_guide().contains("作成"),
        "the guide must say the same: {}",
        ed.screen_guide()
    );
    // The empty box really creates on [A].
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);

    let mut ed = editor_with(save_with_charms(3), Screen::Charms);
    ed.open_charms();
    ed.charm_filter = String::from("zzzznothing");
    assert_eq!(ed.charm_a_label(), "0件");
}

// ---------------------------------------------------------------------------
// C9. The talisman form's [B] names one destination
// ---------------------------------------------------------------------------

/// The footer and the guide say the same word for [B].
#[test]
fn the_talisman_form_names_one_destination_for_b() {
    let mut ed = editor_with(save_with_charms(3), Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    let footer = ed.footer_hint();
    let guide = ed.screen_guide();
    assert!(footer.contains("[B] 一覧へ"), "{footer}");
    assert!(guide.contains("[B] で一覧へ"), "{guide}");
    assert!(
        !footer.contains("[B] 戻る"),
        "the two must not name two destinations: {footer}"
    );
}

// ---------------------------------------------------------------------------
// D10. The state machine's key table is the console's
// ---------------------------------------------------------------------------

/// The five screens the review named answer the key the console's table names.
#[test]
fn the_state_machine_answers_the_keys_the_console_table_names() {
    // セーブ一覧 [Y] is 再読込, not クエスト一覧.
    let mut ed = editor_with(blank_save(), Screen::SaveList);
    ed.set_save_files(vec![String::from("a"), String::from("b")]);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SaveList, "[Y] must not leave the save list");
    assert!(ed.take_rescan(), "[Y] must ask for the card to be read again");

    // スロット [Y] is 一括MAX（2回）, not the quest list.
    let mut ed = editor_with(blank_save(), Screen::Slots);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::Slots);
    assert!(
        ed.message.contains("最大"),
        "the first press offers the bulk max: {}",
        ed.message
    );

    // キャラ [Y] is 刻み, not マイセット.
    let mut ed = editor_with(blank_save(), Screen::Char);
    let step = ed.step;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::Char, "[Y] must not leave the character");
    assert_ne!(ed.step, step, "[Y] is the step key");
    assert!(ed.message.contains("刻み"), "{}", ed.message);

    // アイテム [A] is the picker and [R] the count keyboard.
    let mut save = blank_save();
    let mut items = save.read_item_box(0).expect("a box");
    items[0] = mhnx_core::itembox::ItemSlot { id: 9, count: 1 };
    save.write_item_box(0, &items).expect("write the box");
    let mut ed = editor_with(save, Screen::Items);
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Items, "[R] is not the picker");
    assert!(
        ed.take_text_request().is_some(),
        "[R] must ask for the count"
    );
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "[A] must open the picker");

    // 狩猟記録 [Y] 列 / [A] 刻み / [R] 数値入力.
    let mut ed = editor_with(blank_save(), Screen::Hunts);
    ed.hunt_show_all = true;
    ed.open_hunts();
    let field = ed.hunt_field;
    ed.input(Key::Y);
    assert_eq!(ed.hunt_field, (field + 1) % 4, "[Y] walks the column");
    let step = ed.step;
    ed.input(Key::A);
    assert_ne!(ed.step, step, "[A] is the step");
    assert!(ed.message.contains("刻み"), "{}", ed.message);
    ed.input(Key::R);
    assert!(
        ed.take_text_request().is_some(),
        "[R] must ask for the number"
    );
}

/// The console's own chip table says the same words the state machine answers.
#[test]
fn the_console_chip_table_names_the_same_job_for_the_same_key() {
    let src = switch_editor_src();
    let body = fn_body(src, "pub(crate) fn actions(");
    for (screen, key, word) in [
        ("SaveList => &[", "(\"Y\", \"再読込\")", "再読込"),
        ("Slots => &[", "(\"Y\", \"一括MAX(2回)\")", "一括MAX"),
        ("Char => &[", "(\"Y\", \"刻み\")", "刻み"),
        ("Items => &[", "(\"A\", \"種類変更\")", "種類変更"),
        ("Items => &[", "(\"R\", \"数値入力\")", "数値入力"),
        ("Hunts => &[", "(\"Y\", \"列切替\")", "列切替"),
        ("Hunts => &[", "(\"A\", \"刻み\")", "刻み"),
        ("Hunts => &[", "(\"R\", \"数値入力\")", "数値入力"),
    ] {
        let at = body
            .find(screen)
            .unwrap_or_else(|| panic!("{screen} is gone from the chip table"));
        let tail = &body[at..];
        let end = tail.find("],").expect("the arm's end");
        assert!(
            tail[..end].contains(key),
            "{screen} must name {word} on that key"
        );
    }
}

// ---------------------------------------------------------------------------
// D11. The guild bar lights the view that is showing
// ---------------------------------------------------------------------------

/// The four guild views are the bar's tabs while that screen is showing.
#[test]
fn the_guild_bar_names_all_four_views_and_lights_the_one_showing() {
    let src = ui_src();
    let body = fn_body(src, "fn sub_tabs_for(");
    let guild = body
        .find("if ed.screen == Screen::Guild {")
        .expect("the guild's own tab row");
    let tail = &body[guild..];
    let end = tail.find("return (Self::statics(labels), active);").expect("the row");
    let row = &tail[..end];
    for label in ["カード", "回数", "武器", "闘技大会"] {
        assert!(row.contains(label), "the bar must name {label}: {row}");
    }
    for (const_name, idx) in [
        ("GUILD_TAB_COUNTS", "1"),
        ("GUILD_TAB_WEAPONS", "2"),
        ("GUILD_TAB_ARENA", "3"),
    ] {
        assert!(
            row.contains(&format!("{const_name} => {idx}")),
            "{const_name} must light its own tab"
        );
    }
    // And a press on one of them switches the view instead of doing nothing.
    let select = fn_body(src, "pub fn select_sub_tab(");
    assert!(
        select.contains("MainSection::PalicoRecord if ed.screen == Screen::Guild"),
        "the guild's tabs must be pressable"
    );
}

// ---------------------------------------------------------------------------
// D12. [B] on the guild card unwinds one level at a time
// ---------------------------------------------------------------------------

/// The footer names one step per view, and the guide says the key has two.
#[test]
fn the_guild_b_is_one_level_and_the_guide_says_so() {
    let mut ed = editor_with(blank_save(), Screen::Guild);
    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_CARD);
    assert!(ed.footer_hint().contains("[B] 枠選択"), "{}", ed.footer_hint());
    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_COUNTS);
    assert!(ed.footer_hint().contains("[B] カード"), "{}", ed.footer_hint());
    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_ARENA);
    assert!(ed.footer_hint().contains("[B] カードへ"), "{}", ed.footer_hint());
    let guide = ed.screen_guide();
    assert!(guide.contains("1段ずつ"), "{guide}");
    assert!(guide.contains("枠選択"), "{guide}");
}

// ---------------------------------------------------------------------------
// D13. One name for the byte screen
// ---------------------------------------------------------------------------

/// 生データ is the name on both builds and in the README.
#[test]
fn the_byte_screen_has_one_name_on_both_builds() {
    let mut ed = Editor::new();
    ed.screen = Screen::SaveMap;
    assert_eq!(ed.detail_pill_label(), "[生データ]");
    assert!(gui_src().contains("Tab::SaveMap => \"生データ\""));
    assert!(
        !gui_src().contains("Tab::SaveMap => \"セーブマップ\""),
        "the PC build must not keep the other name"
    );
    assert!(readme().contains("### 生データ"));
    assert!(
        !readme().contains("### セーブマップ（生データ）"),
        "the README must not keep the two names side by side"
    );
}

// ---------------------------------------------------------------------------
// D14. The step table knows the value it steps
// ---------------------------------------------------------------------------

/// A reward row holds a count and a chance; ±1000 and ±10000 are not steps of
/// either, so the table stops at ±100 and the press uses the same slice.
#[test]
fn the_reward_step_table_stops_at_the_value_range() {
    let ed = editor_on_the_reward_table();
    let (steps, key) = ed.step_table();
    assert_eq!(key, "Y");
    assert_eq!(steps, &[1, 10, 100], "the pills must fit the range");
    assert!(steps.iter().all(|s| *s <= 100), "{steps:?}");

    // The press really takes the step the pill shows: 50 + 100 stops at 100.
    let mut ed = editor_on_the_reward_table();
    ed.entry_field = 2; // 確率%
    assert_eq!(reward_chance(&ed), 50);
    ed.step = 2; // ±100
    ed.input(Key::Right);
    assert_eq!(reward_chance(&ed), 100, "a chance stops at 100");
    // And the index is clamped to the table, so a stale step cannot overshoot.
    ed.step = 4;
    assert_eq!(ed.entry_step(), 100);
}

// ---------------------------------------------------------------------------
// D15. [A] on a reward row is the cell's own action
// ---------------------------------------------------------------------------

/// アイテム opens the picker, 個数 and 確率% take a step - the same shape the
/// talisman and equipment forms have.
#[test]
fn the_reward_a_is_the_cells_own_action() {
    let mut ed = editor_on_the_reward_table();
    assert_eq!(ed.entry_field_label(), "アイテム");
    assert_eq!(ed.entry_a_label(), "アイテム");
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "アイテム picks");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::EntryView);

    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "個数");
    assert_eq!(ed.entry_a_label(), "個数+1");
    let before = reward_count(&ed);
    ed.input(Key::A);
    assert_eq!(reward_count(&ed), before + 1, "[A] raises the count");

    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "確率%");
    assert_eq!(ed.entry_a_label(), "確率+1");
    let before = reward_chance(&ed);
    ed.input(Key::A);
    assert_eq!(reward_chance(&ed), before + 1, "[A] raises the chance");

    // The footer's [A] word is the same value.
    let footer = ed.footer_hint();
    assert!(footer.contains("[A] 確率+1"), "{footer}");
    assert!(footer.contains("[ZR] 項目"), "{footer}");
    assert!(
        caption_width(&footer) <= FOOTER_HINT_ROOM,
        "the footer does not fit: {}px {footer}",
        caption_width(&footer)
    );
}

// ---------------------------------------------------------------------------
// D16. Re-entering the quest section moves the list with the bar
// ---------------------------------------------------------------------------

/// The section jump lights クエスト改造 and lands on セーブ内: the list's own tab
/// has to move with the bar.
#[test]
fn the_quest_section_jump_resets_the_list_tab() {
    let src = ui_src();
    let body = fn_body(src, "fn goto_section(");
    let quest = body
        .find("MainSection::Quest => {")
        .expect("the quest section's arm");
    let tail = &body[quest..];
    let end = tail.find("MainSection::Tools =>").expect("the next arm");
    assert!(
        tail[..end].contains("ed.set_quest_tab(0)"),
        "the jump must move the list to the tab the bar lights"
    );
    assert!(
        tail[..end].contains("self.quest_bar_tab = 0"),
        "and the bar to the list"
    );
}

// ---------------------------------------------------------------------------
// D17. A cancelled keyboard and one that never opened are two sentences
// ---------------------------------------------------------------------------

/// The keyboard's three answers are three values, and the two failures say
/// different things.
#[test]
fn a_cancelled_keyboard_and_a_failed_one_say_different_things() {
    let src = swkbd_src();
    assert!(src.contains("pub enum SwkbdAnswer"));
    assert!(src.contains("Cancelled,"), "cancel is its own answer");
    assert!(src.contains("Failed,"), "a launch failure is its own answer");
    assert!(
        src.contains("LIBAPPLET_BAD_EXIT"),
        "the cancel code is what tells the two apart"
    );
    assert!(
        src.contains("SwkbdAnswer::Cancelled")
            && src.contains("SwkbdAnswer::Failed"),
        "both answers must be produced"
    );

    let ed_src = switch_editor_src();
    assert!(ed_src.contains("キャンセルしました"), "cancel keeps its sentence");
    assert!(
        ed_src.contains("システムキーボードを開けませんでした"),
        "a keyboard that never opened must not claim the user cancelled"
    );
}
