//! The sixth review's leftovers: the three surfaces that explain keys, and the
//! words they disagreed about.
//!
//! The sixth review put every key meaning in one table (`hbedit::Editor::
//! key_facts`) and made the footer, the guide, the deck and the text build read
//! it. It left the *surfaces* alone: the sub-bar's pills and the deck's buttons
//! still printed a key name themselves, in their own words, so one press could
//! be described four ways - the item box's [ZR] was 一括操作 on the bar,
//! 一括操作メニュー and 検索 in the deck and 一括 on the footer, and the 検索
//! pill named a key that does not search at all.
//!
//! What this file pins, item by item:
//!
//! * one surface names keys (the footer); the bar and the deck print the action
//!   and borrow the word from the table (A1, B3);
//! * the guide strip shows state the footer cannot, instead of repeating the
//!   footer's first two items (A2);
//! * the reward tab's [ZR] walks the two cells the arrows can move (B4);
//! * the step the strip prints is the step the arrows take (B5);
//! * the save key saves and nothing else - it no longer completes a two-press
//!   overwrite (B6) and it commits the my-set draft before writing (B7);
//! * the character screen's [A] word follows the row (B8);
//! * the lines that named the wrong key name the right one (C9-C13);
//! * the two dead footer renderers are gone and no surface drops a note (D14,
//!   D15), the item menu names the key that closes it (D16) and the step pills
//!   are drawn only where they change something (D17);
//! * every [B] names the screen it goes to (E18) and the PC build reads the
//!   console's words (E19).
//!
//! Every test here is a reproduction: it fails on the commit before this one.

use mhnx_core::hbedit::{
    self, caption_width, Editor, Key, Screen, FOOTER_HINT_ROOM, GUILD_TAB_ARENA, GUILD_TAB_CARD,
    GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, QUEST_TAB_REWARDS,
};
use mhnx_core::save;

const UI_SRC: &str = include_str!("../../../switch/src/ui.rs");
const EDITOR_SRC: &str = include_str!("../../../switch/src/editor.rs");
const README_SRC: &str = include_str!("../../../README.md");
const HBEDIT_SRC: &str = include_str!("../src/hbedit.rs");
const GUI_SRC: &str = include_str!("../../mhnx-gui/src/app.rs");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn blank_save() -> save::Save {
    let mut s = save::empty_switch_save();
    s.set_slot_used(0, true);
    s
}

fn editor(screen: Screen) -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(blank_save());
    ed.save_path = Some(String::from("memory:review-23"));
    ed.slot = 0;
    ed.screen = screen;
    ed
}

/// The keys a screen's line names, in the footer's order.
fn named(ed: &Editor) -> Vec<String> {
    ed.key_facts()
        .into_iter()
        .filter(|f| !f.key.is_empty())
        .map(|f| f.key.to_string())
        .collect()
}

fn label(ed: &Editor, key: &str) -> String {
    ed.key_facts()
        .into_iter()
        .find(|f| f.key == key)
        .map(|f| f.label)
        .unwrap_or_else(|| panic!("{key} is not on the line: {:?}", ed.key_facts()))
}

/// The body of one fn in a source text, from its signature to its closing brace.
fn fn_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} is not in the source"));
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[start..=i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    &src[start..]
}

/// The pad keys a pill must never name, as they are bracketed.
const PAD_KEYS: [&str; 12] = [
    "[A]", "[B]", "[X]", "[Y]", "[ZR]", "[ZL]", "[L]", "[R]", "[-]", "[+]", "[◀/▶]", "[▲/▼]",
];

/// Every screen in every mode a line can be written in.
fn states() -> Vec<(&'static str, Editor)> {
    let mut out: Vec<(&'static str, Editor)> = Vec::new();
    macro_rules! add {
        ($name:expr, $ed:expr) => {
            out.push(($name, $ed))
        };
    }
    for screen in ALL_SCREENS {
        add!(screen_name(*screen), editor(*screen));
    }
    // ギルド: the four views.
    for view in [GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, GUILD_TAB_ARENA] {
        let mut ed = editor(Screen::Guild);
        ed.set_guild_view(view);
        add!("Guild", ed);
    }
    // クエスト改造: the four tabs.
    for tab in 0..4 {
        let mut ed = editor(Screen::QuestEdit);
        ed.quest_set_tab(tab);
        add!("QuestEdit", ed);
    }
    // マイセット: the two forms.
    let mut ed = editor(Screen::Sets);
    ed.set_edit = Some(hbedit::SetEdit::default());
    add!("Sets+form", ed);
    let mut ed = editor(Screen::Sets);
    ed.item_set_edit = Some(hbedit::ItemSetEdit::default());
    add!("Sets+item", ed);
    // 護石: the list and the form.
    let mut ed = editor(Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    add!("CharmEdit", ed);
    // アイテム: the list and the bulk menu.
    let mut ed = editor(Screen::Items);
    ed.open_item_menu();
    add!("Items+menu", ed);
    // オトモ: the two panels.
    let mut ed = editor(Screen::Roster);
    ed.palico_equip = Some(hbedit::PalicoEquip {
        palico: 0,
        field: 0,
        pick: [hbedit::PALICO_EQUIP_NONE; hbedit::PALICO_EQUIP_ROWS],
        clear_armed: None,
    });
    add!("Roster+equip", ed);
    let mut ed = editor(Screen::Roster);
    ed.palico_skill = Some(hbedit::PalicoSkill {
        palico: 0,
        tab: 0,
        field: 0,
        picks: [0u8; hbedit::PALICO_MOVE_ROWS],
    });
    add!("Roster+skill", ed);
    // 生データ: the list and the byte editor.
    let mut ed = editor(Screen::SaveMap);
    ed.open_save_map(Screen::Char);
    add!("SaveMap", ed);
    let mut ed = editor(Screen::SaveMap);
    ed.open_save_map(Screen::Char);
    ed.input(Key::A);
    add!("SaveMap+edit", ed);
    out
}

const ALL_SCREENS: &[Screen] = &[
    Screen::SaveList,
    Screen::Slots,
    Screen::Char,
    Screen::Items,
    Screen::ItemPick,
    Screen::SkillPick,
    Screen::JewelPick,
    Screen::EquipPick,
    Screen::Charms,
    Screen::CharmEdit,
    Screen::Equip,
    Screen::EquipEdit,
    Screen::QuestEquip,
    Screen::SaveQuests,
    Screen::ArcList,
    Screen::QuestEdit,
    Screen::ArcEntries,
    Screen::EntryView,
    Screen::Mib,
    Screen::Gmd,
    Screen::Check,
    Screen::Hunts,
    Screen::Guild,
    Screen::Progress,
    Screen::Roster,
    Screen::Palico,
    Screen::Sets,
    Screen::Csv,
    Screen::Options,
    Screen::SaveMap,
    Screen::Review,
    Screen::Convert,
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

/// A box with two talismans in it, for the overwrite tests.
fn editor_with_two_charms() -> Editor {
    let mut ed = editor(Screen::Charms);
    {
        let b = ed
            .save
            .as_mut()
            .expect("a save")
            .equipment_box_mut(0)
            .expect("a box");
        for i in 0..2 {
            let o = i * mhnx_core::charm::CHARM_RECORD;
            b[o] = 0x06;
            b[o + 0x02] = 10;
        }
    }
    ed.open_charms();
    ed.charm_a_press();
    ed
}

// ---------------------------------------------------------------------------
// A1. One surface names keys
// ---------------------------------------------------------------------------

/// The bar and the deck print the action, and never a key name.
///
/// The sixth review counted four wordings for one [ZR] press on the item box
/// alone: 一括操作 on the bar, 一括操作メニュー and 検索 in the deck, 一括 on the
/// footer. The 検索 pill was the worst of them - it named [ZR] over a button
/// that opens the name search, which [ZR] does not do (the search is a row
/// inside the menu [ZR] opens). Both surfaces print the action now, and the key
/// is the footer's to name.
#[test]
fn no_pill_names_a_key() {
    for (name, body) in [
        (
            "draw_sub_bar_quick_tools",
            fn_body(UI_SRC, "fn draw_sub_bar_quick_tools"),
        ),
        (
            "draw_studio_inspector",
            fn_body(UI_SRC, "fn draw_studio_inspector"),
        ),
    ] {
        // The comments in these two functions talk about keys (they explain
        // why each button exists), so only the code lines are read.
        let code: String = body
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for key in PAD_KEYS {
            let needle = format!("\"{key} ");
            assert!(
                !code.contains(&needle),
                "{name} still names {key}: a pill is a button, not a key"
            );
        }
    }
    // The two the review named by hand.
    assert!(!UI_SRC.contains("[ZR] 一括操作"), "the bar's pill names [ZR]");
    assert!(!UI_SRC.contains("[ZR] 検索"), "the search pill names [ZR]");
    assert!(!UI_SRC.contains("[ZR] 一括操作メニュー"), "the deck's pill names [ZR]");
    // And the words they print come from the table.
    for needle in [
        "pill_word(ed, \"ZR\"",
        "pill_word(ed, \"Y\"",
        "pill_word(ed, \"X\"",
        "pill_word(ed, \"A\"",
        "ed.b_word()",
    ] {
        assert!(UI_SRC.contains(needle), "the pills must borrow {needle}");
    }
}

/// The three surfaces say one thing about one key.
///
/// This is the review's core: the sub-bar, the deck and the footer each
/// explained keys, and each said something different. The footer names the key,
/// the two button surfaces print the table's word for it, and the guide prints
/// the state. Nothing may describe a key the footer does not.
#[test]
fn the_three_surfaces_agree_about_a_key() {
    // 1. Every key the guide names is a key the footer names, in the footer's
    // own words: the guide borrows the table, it does not write a second one.
    let mut checked = 0usize;
    for (name, ed) in states() {
        let guide = ed.screen_guide();
        let footer = ed.footer_hint();
        for token in key_tokens(&guide) {
            assert!(
                footer.contains(&token),
                "{name}: the guide names {token} and the footer does not: {footer}"
            );
            checked += 1;
        }
    }
    assert!(checked > 20, "the guide named keys: {checked}");

    // 2. The deck's word for a key is the table's word for it: the two the
    // review counted four ways are one word now.
    let ed = editor(Screen::Items);
    assert_eq!(label(&ed, "ZR"), "一括");
    assert_eq!(ed.key_label("ZR").as_deref(), Some("一括"));
    let ed = editor(Screen::Palico);
    assert_eq!(label(&ed, "Y"), "全導入2回");
    assert_eq!(label(&ed, "◀/▶"), "言語");
    // 3. And the item box's search pill is a different button with its own
    // word, because it does a different thing.
    let deck = fn_body(UI_SRC, "fn draw_studio_inspector");
    assert!(
        deck.contains("UiAction::FilterItems"),
        "the search is its own button"
    );
    assert!(
        deck.contains("\"検索\"") && !deck.contains("[ZR] 検索"),
        "and it prints 検索 without a key"
    );
}

/// The key tokens a line names, as "[A]" / "[ZR]" / "[◀/▶]".
fn key_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find('[') {
        let tail = &rest[at..];
        if let Some(end) = tail.find(']') {
            let token = &tail[..=end];
            if token.chars().count() <= 6 {
                out.push(String::from(token));
            }
            rest = &tail[end..];
        } else {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A2. The guide is a state line
// ---------------------------------------------------------------------------

/// The guide shows state the footer cannot, instead of the footer's first two
/// items over again.
///
/// The strip is 28px of the list's height, and it printed the screen's sentence
/// plus two items of the footer - a repeat with nothing of its own. It shows
/// what is true now: the selected row's place, the search that is narrowing the
/// list, the draft a form is holding, whether the save has unsaved edits.
#[test]
fn the_guide_shows_state_the_footer_cannot() {
    // The item box with a search on it: the strip says which search.
    let mut ed = editor(Screen::Items);
    ed.apply_item_box_filter(String::from("回復薬"));
    let guide = ed.screen_guide();
    assert!(guide.contains("検索「回復薬」"), "{guide}");
    assert!(!ed.footer_hint().contains("回復薬"), "{}", ed.footer_hint());
    assert!(
        !ed.footer_hint().contains(&guide),
        "the guide is not a piece of the footer: {guide}"
    );

    // A form with a draft: the strip says which form is holding it.
    let mut ed = editor_with_two_charms();
    ed.charm_edit.as_mut().expect("a form").points1 = 5;
    let guide = ed.screen_guide();
    assert!(guide.contains("下書きあり"), "{guide}");
    assert!(guide.contains("護石編集"), "{guide}");

    // A dirty save: the strip says so.
    let mut ed = editor(Screen::Char);
    ed.dirty = true;
    assert!(ed.screen_guide().contains("未保存"), "{}", ed.screen_guide());

    // And the row's place, which the footer never prints.
    let ed = editor(Screen::Char);
    assert!(ed.screen_guide().contains("1 / "), "{}", ed.screen_guide());
}

// ---------------------------------------------------------------------------
// B3. The item box's search is a row of the menu
// ---------------------------------------------------------------------------

/// [ZR] opens the bulk menu, and the search is one of its rows.
///
/// The pill named [ZR] over the search, which is the one thing the key does not
/// do: it opens the menu, and the menu's last row opens the keyboard.
#[test]
fn the_item_box_search_is_a_row_of_the_menu() {
    let mut ed = editor(Screen::Items);
    assert_eq!(ed.zr_job(), hbedit::ZrJob::Screen);
    ed.input(Key::Z);
    assert!(ed.item_menu_open, "[ZR] opens the menu: {}", ed.message);
    ed.item_menu_cursor = hbedit::ITEM_OP_FIND;
    ed.input(Key::A);
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(hbedit::TEXT_ITEM_BOX_FILTER),
        "the menu's 名前で探す row is the search"
    );
    // The line names the menu, not the search.
    let ed = editor(Screen::Items);
    assert_eq!(label(&ed, "ZR"), "一括");
}

// ---------------------------------------------------------------------------
// B4. The reward tab walks two stops
// ---------------------------------------------------------------------------

/// [ZR] on 報酬 walks 個数 ⇄ 確率%, and never lands on アイテム.
///
/// アイテム is [A]'s cell: the third stop could only answer the arrows with a
/// refusal, so one press in three did nothing.
#[test]
fn the_reward_tab_walks_two_stops() {
    let mut ed = editor(Screen::QuestEdit);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    assert_eq!(ed.quest_reward_field, hbedit::QUEST_REWARD_COUNT);
    let mut seen = Vec::new();
    for _ in 0..4 {
        ed.input(Key::Z);
        seen.push(ed.quest_reward_field);
    }
    assert_eq!(
        seen,
        vec![
            hbedit::QUEST_REWARD_CHANCE,
            hbedit::QUEST_REWARD_COUNT,
            hbedit::QUEST_REWARD_CHANCE,
            hbedit::QUEST_REWARD_COUNT,
        ],
        "個数 ⇄ 確率%, and アイテム is never a stop"
    );
    assert!(!ed.quest_edit_item_cell());
    assert_eq!(label(&ed, "ZR"), "段送り（2段）");
    // The sub-bar's chip shows the state and names no key.
    assert!(!UI_SRC.contains("[ZR] 確率%"), "the chip names [ZR]");
    assert!(!UI_SRC.contains("[ZR] 個数"), "the chip names [ZR]");
}

// ---------------------------------------------------------------------------
// B5. One step table
// ---------------------------------------------------------------------------

/// The strip above the list and the arrows below it print one number.
///
/// The sub-bar printed `STEPS[self.step]` raw while the arrows took the value
/// clamped to the table the pills draw ([Editor::step_value]): on a reward row
/// with the ±1000 pill lit the strip said ±1000 and the press moved ±100.
#[test]
fn the_step_pill_reads_the_step_table() {
    let mut ed = editor(Screen::EntryView);
    ed.arc = Some(arc_with_a_reward_table());
    ed.entry = 0;
    assert!(ed.entry_has_numbers());
    ed.step = hbedit::STEPS.len() - 1;
    assert_eq!(ed.step_value(), 100, "the table stops at the value range");
    assert_eq!(ed.step_value(), ed.entry_step(), "one number, two names");
    assert!(ed.step_value() < hbedit::STEPS[ed.step], "the raw index is not it");
    // The sub-bar reads the same answer, and never the raw table.
    let bar = fn_body(UI_SRC, "fn draw_sub_bar_quick_tools");
    assert!(bar.contains("ed.step_value()"), "the strip reads the table: {bar}");
    assert!(
        !bar.contains("STEPS[ed.step]") && !bar.contains("HUNT_STEPS[ed.step]"),
        "the strip must not print the raw index: {bar}"
    );
}

/// A .rem entry, for the reward-row tests.
fn arc_with_a_reward_table() -> mhnx_core::arc::Arc {
    const HASH_REM: u32 = 0x5B3C_302D;
    mhnx_core::arc::Arc {
        version: 3,
        header8: 0xFFFF_FFFE,
        entries: vec![mhnx_core::arc::ArcEntry {
            name: String::from("quest\\rem\\rem_620114"),
            type_hash: HASH_REM,
            data: vec![0u8; 184],
            original: None,
            original_offset: None,
        }],
    }
}

// ---------------------------------------------------------------------------
// B6. The save key does not complete an overwrite
// ---------------------------------------------------------------------------

/// [X] saves; the overwrite is [A]'s second press and nothing else.
///
/// The arm was kept for `key != Key::X`, so the save key ran the same write and
/// completed an overwrite the form had only offered - and the message that
/// offered it named [A], a key the user had not pressed.
#[test]
fn the_save_key_does_not_complete_the_overwrite() {
    let mut ed = editor_with_two_charms();
    let form = ed.charm_edit.expect("a form");
    ed.charm_edit = Some(hbedit::CharmEdit {
        index: if form.index == 0 { 1 } else { 0 },
        field: 0,
        ..form
    });
    assert!(ed.charm_write_replaces(), "the press would replace a record");
    ed.input(Key::A);
    assert!(ed.charm_write_armed.is_some(), "the first [A] arms it");
    let before = ed.charms().len();
    ed.quick_save();
    assert!(
        ed.take_write().is_none(),
        "the save key must not write a file it cannot hold"
    );
    assert_eq!(ed.charms().len(), before, "and must not write the record");
    assert!(ed.charm_write_armed.is_none(), "it takes the offer back");
    assert!(
        ed.message.contains("[A] の2回目"),
        "the line names the key that completes it: {}",
        ed.message
    );
    assert!(
        ed.message.contains("保存はしていません"),
        "and says the file was not written: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// B7. The save key commits the my-set draft
// ---------------------------------------------------------------------------

/// [X] on マイセット編集 writes the pending parts before it writes the file.
///
/// The save key had no arm for the screen at all, so it saved a file that did
/// not hold the parts on screen and then said 保存しました.
#[test]
fn the_save_key_commits_the_set_draft() {
    // A save with one item in the box, so the item-set form has a candidate to
    // pick: a form on an empty box cannot move a row and would test nothing.
    let mut ed = editor(Screen::Sets);
    {
        let mut slots = vec![
            mhnx_core::itembox::ItemSlot { id: 0, count: 0 };
            mhnx_core::itembox::TOTAL_ITEM_SLOTS
        ];
        slots[0] = mhnx_core::itembox::ItemSlot { id: 1, count: 1 };
        ed.save
            .as_mut()
            .expect("a save")
            .write_item_box(0, &slots)
            .expect("the box");
    }
    assert!(ed.open_item_set_editor(0), "{}", ed.message);
    assert_eq!(ed.screen, Screen::Sets);
    // Walk the first row's candidates: the form's pick runs ahead of the record
    // until [A] writes it.
    ed.input(Key::Right);
    assert!(
        ed.item_set_draft_dirty(),
        "the form holds an entry the record does not have: {}",
        ed.message
    );
    let before = ed
        .item_sets()
        .into_iter()
        .next()
        .map(|s| s.items[0])
        .expect("the set");
    ed.quick_save();
    assert!(
        !ed.item_set_draft_dirty(),
        "the save key writes the draft: {}",
        ed.message
    );
    let after = ed
        .item_sets()
        .into_iter()
        .next()
        .map(|s| s.items[0])
        .expect("the set");
    assert_ne!(before, after, "the record holds the row the form showed");
    assert!(ed.take_write().is_some(), "and then asks for the file write");
}

// ---------------------------------------------------------------------------
// B8. The character screen's [A] word follows the row
// ---------------------------------------------------------------------------

/// One key, three jobs, and the line names the row's job.
///
/// Row 0 is the name (the keyboard), row 13 is 性別 (a flip) and every other row
/// is a ceiling the press fills. The word is [Editor::char_a_label], and the
/// footer and the inspector's button both print it.
#[test]
fn the_char_a_word_follows_the_row() {
    let mut ed = editor(Screen::Char);
    ed.field = 0;
    assert_eq!(label(&ed, "A"), "名前変更");
    ed.field = 13;
    assert_eq!(label(&ed, "A"), "性別切替");
    ed.field = 3;
    assert_eq!(label(&ed, "A"), "最大（2回）");
    assert_eq!(ed.char_a_label(), "最大（2回）");
    // The inspector's button borrows the same answer.
    assert!(UI_SRC.contains("ed.key_label_or(\"A\""));
    assert!(!UI_SRC.contains("\"最大値\""), "the old second word is gone");
}

// ---------------------------------------------------------------------------
// C9. The palico language line names the arrow pair
// ---------------------------------------------------------------------------

/// The line said "L/R で名前の言語" over a pair that is ◀ / ▶.
///
/// [L] and [R] are the sub-tab keys on this console - [R] is the press that got
/// to the screen - so the sentence named the key that leaves instead of the one
/// that changes the names.
#[test]
fn the_palico_language_line_names_the_arrow_pair() {
    assert!(
        !HBEDIT_SRC.contains("L/R で名前の言語"),
        "the line names the sub-tab pair"
    );
    assert!(
        HBEDIT_SRC.contains("◀/▶ で名前の言語を切り替え"),
        "the line names the pair that switches the language"
    );
    // And the pair really switches it.
    let mut ed = editor(Screen::Palico);
    ed.input(Key::Left);
    assert_eq!(ed.palico_lang, 1);
    ed.input(Key::R);
    assert_eq!(ed.palico_lang, 1, "[R] is the sub-tab, not the language");
}

// ---------------------------------------------------------------------------
// C10. The empty hunt list names the display key
// ---------------------------------------------------------------------------

/// 狩猟記録's empty line promised [Y], and [Y] walks the four numbers.
///
/// The display switch is [ZR]: the line sent the user at a key that changed a
/// column and left the list empty.
#[test]
fn the_empty_hunt_list_names_the_display_key() {
    let mut ed = editor(Screen::Hunts);
    ed.hunt_show_all = false;
    let line = ed.empty_list_line();
    assert!(line.contains("[ZR]"), "the display key is [ZR]: {line}");
    assert!(!line.contains("[Y]"), "and not [Y]: {line}");
    assert_eq!(label(&ed, "ZR"), "表示");
    assert_eq!(label(&ed, "Y"), "列");
}

// ---------------------------------------------------------------------------
// C11. The item box's empty search names the way out
// ---------------------------------------------------------------------------

/// A search that hides every row says how to clear it.
///
/// The line said "[Y] で空き枠も表示", and the search wins over the toggle: an
/// empty slot has no name, so no press of [Y] could bring a row back.
#[test]
fn the_item_box_empty_search_names_the_way_out() {
    let mut ed = editor(Screen::Items);
    ed.apply_item_box_filter(String::from("そんなアイテムはない"));
    assert_eq!(ed.active_search(), Some("そんなアイテムはない"));
    let line = ed.empty_list_line();
    assert!(line.contains("そんなアイテムはない"), "{line}");
    assert!(line.contains("名前で探す"), "the way out is the menu row: {line}");
    assert!(!line.contains("空き枠"), "the toggle cannot help: {line}");
}

// ---------------------------------------------------------------------------
// C12. The overwrite offer names the pressed key
// ---------------------------------------------------------------------------

/// The offer names the key that was pressed, not a fixed [A].
#[test]
fn the_overwrite_offer_names_the_pressed_key() {
    let mut ed = editor_with_two_charms();
    let form = ed.charm_edit.expect("a form");
    ed.charm_edit = Some(hbedit::CharmEdit {
        index: if form.index == 0 { 1 } else { 0 },
        field: 0,
        ..form
    });
    ed.input(Key::A);
    assert!(ed.charm_write_armed.is_some());
    assert!(
        ed.message.contains("もう一度 [A] で上書き"),
        "the pressed key is named: {}",
        ed.message
    );
    // The message is built from the key the caller passed, so a different
    // caller cannot print a key nobody pressed.
    assert!(HBEDIT_SRC.contains("key\n        )") || HBEDIT_SRC.contains("            key"));
    assert!(!HBEDIT_SRC.contains("もう一度 [A] で上書き（[B] で取りやめ）\",\n            e.index"));
}

// ---------------------------------------------------------------------------
// C13. The OSK legend and its buttons are one table
// ---------------------------------------------------------------------------

/// The legend and the button row print the same key and the same word.
///
/// The legend said "[B] 削除" over a button that said "[B] 1字削除", called [X]
/// 全消去 where the button said クリア, and named neither [Y] nor [-] while the
/// row carried both.
#[test]
fn the_osk_legend_is_the_button_row() {
    let draw = fn_body(UI_SRC, "fn draw_osk");
    assert!(
        draw.contains("osk_key_label"),
        "the legend and the buttons are the same table: {draw}"
    );
    assert!(!draw.contains("[B] 削除"), "the old legend word is gone");
    assert!(!draw.contains("[X] 全消去"), "the old legend word is gone");
    assert!(
        draw.contains("[A] 入力") && draw.contains("[L/R] 文字種"),
        "the two keys that are not buttons are still named"
    );
    // The table itself.
    for pair in [
        "(\"B\", \"1字削除\"",
        "(\"X\", \"クリア（2回）\"",
        "(\"Y\", \"濁点/小字\"",
        "(\"-\", \"取消\"",
    ] {
        assert!(draw.contains(pair), "the row carries {pair}");
    }
}

// ---------------------------------------------------------------------------
// D14. One renderer per footer
// ---------------------------------------------------------------------------

/// The two footer renderers no screen called are gone.
///
/// `CharmEdit::footer` and `Editor::quest_edit_footer` rendered the same tables
/// a second time and only the tests read them: a second path is a second answer
/// waiting to happen.
#[test]
fn the_two_dead_footers_are_gone() {
    assert!(
        !HBEDIT_SRC.contains("pub fn quest_edit_footer"),
        "the second quest footer is gone"
    );
    assert!(
        !HBEDIT_SRC.contains("pub fn footer(&self) -> String"),
        "the talisman form's second footer is gone"
    );
    // The one path answers for both screens.
    let ed = editor(Screen::QuestEdit);
    assert!(ed.footer_hint().contains("[X] 保存"), "{}", ed.footer_hint());
    let mut ed = editor_with_two_charms();
    ed.charm_edit.as_mut().expect("a form").field = 0;
    assert!(ed.footer_hint().contains("[A] 書き込み"), "{}", ed.footer_hint());
}

// ---------------------------------------------------------------------------
// D15. No surface drops a note
// ---------------------------------------------------------------------------

/// The two keyless notes are rendered by every surface.
///
/// The guide and the text build's chip row filtered them out, so the one
/// sentence that says how to leave a screen whose [＋] is a tool appeared on the
/// footer and nowhere else.
#[test]
fn no_surface_drops_a_note() {
    // The three screens whose [＋] is a tool say so, and the note is in the
    // table the text build renders.
    for screen in [Screen::Charms, Screen::Equip, Screen::Roster] {
        let ed = editor(screen);
        assert!(ed.plus_is_a_tool(), "{screen:?}: [＋] is a tool here");
        assert!(
            ed.footer_hint().contains("※終了は [B]"),
            "{screen:?}: {}",
            ed.footer_hint()
        );
    }
    // The text build renders every item, notes included.
    let actions = fn_body(EDITOR_SRC, "pub(crate) fn actions(");
    assert!(
        !actions.contains(".filter("),
        "the chip row must keep the notes: {actions}"
    );
    assert!(
        EDITOR_SRC.contains("chip_text"),
        "and print them without a bracket"
    );
    // The section pair is a note on the first screen, and it is rendered.
    let ed = editor(Screen::SaveList);
    assert!(ed.footer_hint().contains("[ZL/ZR] セクション"), "{}", ed.footer_hint());
}

// ---------------------------------------------------------------------------
// D16. The item menu names the key that closes it
// ---------------------------------------------------------------------------

/// [ZR] opened the bulk menu and [ZR] closes it again - and nobody said so.
#[test]
fn the_item_menu_names_the_key_that_closes_it() {
    let mut ed = editor(Screen::Items);
    ed.open_item_menu();
    assert!(ed.item_menu_open);
    let keys = named(&ed);
    assert!(keys.iter().any(|k| k == "ZR"), "{keys:?}");
    assert_eq!(label(&ed, "ZR"), "閉じる");
    // The key really closes it.
    ed.input(Key::Z);
    assert!(!ed.item_menu_open, "[ZR] closes the menu");
    // And the line still fits the strip.
    let mut ed = editor(Screen::Items);
    ed.open_item_menu();
    let hint = ed.footer_hint();
    assert!(
        caption_width(&hint) <= FOOTER_HINT_ROOM,
        "the menu's line is {}px: {hint}",
        caption_width(&hint)
    );
}

// ---------------------------------------------------------------------------
// D17. The step pills are drawn where they change something
// ---------------------------------------------------------------------------

/// オトモ and 環境設定 answered the arrows with a fixed step, so the five pills
/// changed nothing at all.
#[test]
fn the_step_pills_are_not_drawn_where_they_do_nothing() {
    assert!(!editor(Screen::Roster).step_pills_live(), "オトモ moves by one");
    assert!(!editor(Screen::Options).step_pills_live(), "the flags are 0 or 1");
    assert!(editor(Screen::Char).step_pills_live(), "the character screen steps");
    assert!(editor(Screen::Items).step_pills_live(), "the item box steps");
    assert!(editor(Screen::EntryView).step_pills_live(), "the reward row steps");
    // The deck asks before it draws them.
    let deck = fn_body(UI_SRC, "fn draw_studio_inspector");
    assert!(deck.contains("ed.step_pills_live()"), "{deck}");
}

// ---------------------------------------------------------------------------
// E18. Every [B] names the screen it goes to
// ---------------------------------------------------------------------------

/// The [B] word is the destination's name, on every screen and in every mode.
///
/// The review counted twelve words for one key across the app, three of them on
/// the guild card alone (カード / カードへ / キャラ) for two destinations.
#[test]
fn every_b_word_is_the_destination_name() {
    let mut checked = 0usize;
    for (name, ed) in states() {
        if !named(&ed).iter().any(|k| k == "B") {
            continue;
        }
        let dest = ed
            .b_destination()
            .unwrap_or_else(|| panic!("{name}: [B] is named but goes nowhere"));
        let word = label(&ed, "B");
        let want = hbedit::screen_name(dest);
        assert!(
            word.starts_with(want),
            "{name}: [B] says {word} and goes to {want}"
        );
        // The only suffix is the cost of the press: a draft a form would drop,
        // or the byte the editor stepped and did not confirm.
        let rest = &word[want.len()..];
        assert!(
            rest.is_empty() || rest == "（2回で破棄）" || rest == "（取消）",
            "{name}: [B] says {word} - a destination and its cost, nothing else"
        );
        checked += 1;
    }
    assert!(checked > 25, "the [B] items were read: {checked}");

    // The guild card's three words are two destinations, and the two record
    // views say one thing.
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_CARD);
    assert_eq!(label(&ed, "B"), "スロット");
    for view in [GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, GUILD_TAB_ARENA] {
        ed.set_guild_view(view);
        assert_eq!(label(&ed, "B"), "ギルドカード", "view {view}");
    }
    assert!(!HBEDIT_SRC.contains("\"カードへ\""), "the verb is gone");
}

// ---------------------------------------------------------------------------
// E19. The PC build reads the console's words
// ---------------------------------------------------------------------------

/// The PC build's shortcut table is the console's vocabulary.
///
/// The two builds perform the same five operations and their tables shared no
/// word at all: the PC said 上書き保存 where the console's footer said 保存.
#[test]
fn the_pc_build_reads_the_consoles_words() {
    assert!(
        GUI_SRC.contains("mhnx_core::hbedit::WORD_SAVE"),
        "the PC build reads the console's words"
    );
    for word in ["WORD_OPEN", "WORD_UNDO", "WORD_REDO", "WORD_SEARCH"] {
        assert!(GUI_SRC.contains(word), "the PC build reads {word}");
    }
    assert!(
        !GUI_SRC.contains("(\"Ctrl+S\", \"上書き保存\")"),
        "the second vocabulary is gone"
    );
    // The words are the ones the console prints.
    assert_eq!(hbedit::WORD_SAVE, "保存");
    assert_eq!(hbedit::WORD_SEARCH, "検索");
    let ed = editor(Screen::Char);
    assert!(ed.footer_hint().contains(hbedit::WORD_SAVE), "{}", ed.footer_hint());
}

// ---------------------------------------------------------------------------
// The strip still fits
// ---------------------------------------------------------------------------

/// Every line still fits the 763px strip, with the destination names in it.
#[test]
fn every_line_still_fits_the_strip() {
    for (name, ed) in states() {
        let hint = ed.footer_hint();
        assert!(
            caption_width(&hint) <= FOOTER_HINT_ROOM,
            "{name}: {}px of {FOOTER_HINT_ROOM}px: {hint}",
            caption_width(&hint)
        );
    }
}

/// README says what the three surfaces do.
#[test]
fn the_readme_says_which_surface_names_keys() {
    assert!(
        README_SRC.contains("キーの意味を書くのはフッター 1 面だけ"),
        "the README says which surface names keys"
    );
    assert!(
        README_SRC.contains("キー名を名乗りません"),
        "and that the pills name no key"
    );
    assert!(
        README_SRC.contains("個数 ⇄ 確率% の 2 段"),
        "and that the reward tab walks two stops"
    );
    assert!(
        README_SRC.contains("行き先の画面名"),
        "and that [B] names its destination"
    );
}
