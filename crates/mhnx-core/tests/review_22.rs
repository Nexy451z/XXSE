//! The sixth review: the key table, and the five surfaces that read it.
//!
//! The fifth review put every key meaning in one table
//! (hbedit::Editor::key_facts) and made the footer, the chips, the guide and
//! the deck render it. This review found what the merge lost and what the
//! remaining copies still say:
//!
//! * eight keys the pre-merge chip table named are missing from the table, so
//!   a key that works is a key no line says;
//! * the deck and the footer's second line are two more copies of the words;
//! * the guide prints the first two items of the table, so a screen whose line
//!   starts with a direction pair never names [A];
//! * [X] still means something other than save on five screens in the state
//!   machine (the console's global X had already taken it back);
//! * the two-press offers are not taken back by every other key;
//! * one screen's line is written twice, one panel's [+] is mislabelled, and an
//!   error does not say what to press next.
//!
//! Every test here is a reproduction: it fails on the commit before this one.

use mhnx_core::arc::{Arc, ArcEntry};
use mhnx_core::hbedit::{
    self, caption_width, CharmAction, Editor, EquipFormAction, Key, Screen, FOOTER_HINT_ROOM,
    GUILD_TAB_ARENA, GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS,
    QUEST_TAB_BASIC, QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, QUEST_TAB_SUPPLIES,
};
use mhnx_core::save;

const UI_SRC: &str = include_str!("../../../switch/src/ui.rs");
const EDITOR_SRC: &str = include_str!("../../../switch/src/editor.rs");
const README_SRC: &str = include_str!("../../../README.md");
const HBEDIT_SRC: &str = include_str!("../src/hbedit.rs");

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
    ed.save_path = Some(String::from("memory:review-22"));
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

fn names(ed: &Editor, key: &str) -> bool {
    named(ed).iter().any(|k| k == key)
}

fn label(ed: &Editor, key: &str) -> String {
    ed.key_facts()
        .into_iter()
        .find(|f| f.key == key)
        .map(|f| f.label)
        .unwrap_or_else(|| panic!("{key} is not on the line: {:?}", ed.key_facts()))
}

/// A reward table (.rem) and a supply box (.sup), for the two entry modes.
fn arc_with_tables() -> Arc {
    const HASH_REM: u32 = 0x5B3C_302D;
    const HASH_SUP: u32 = 0x5453_9AEE;
    fn entry(name: &str, type_hash: u32) -> ArcEntry {
        ArcEntry {
            name: String::from(name),
            type_hash,
            data: vec![0u8; 184],
            original: None,
            original_offset: None,
        }
    }
    Arc {
        version: 3,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry("quest\\rem\\rem_620114", HASH_REM),
            entry("quest\\supp\\supp_1010165", HASH_SUP),
        ],
    }
}

/// Every screen in every mode a line can be written in.
///
/// The union per screen is what the two audits read: a key that is only live in
/// a mode is named in that mode, and a key that is live on the screen at all
/// has to be named somewhere.
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
    // 生データ: the list and the byte editor.
    let mut ed = editor(Screen::SaveMap);
    ed.open_save_map(Screen::Char);
    add!("SaveMap", ed);
    let mut ed = editor(Screen::SaveMap);
    ed.open_save_map(Screen::Char);
    ed.input(Key::A);
    add!("SaveMap", ed);
    // .mib: a plain row and a monster row.
    let mut probe = editor(Screen::Mib);
    probe.arc = Some(arc_with_tables());
    let plain = (0..mhnx_core::questfields::MIB_FIELDS.len()).find(|i| {
        probe.mib_cursor = *i;
        !probe.mib_row_is_monster()
    });
    if let Some(i) = plain {
        let mut ed = editor(Screen::Mib);
        ed.arc = Some(arc_with_tables());
        ed.mib_cursor = i;
        add!("Mib", ed);
    }
    let mob = (0..mhnx_core::questfields::MIB_FIELDS.len()).find(|i| {
        probe.mib_cursor = *i;
        probe.mib_row_is_monster()
    });
    if let Some(i) = mob {
        let mut ed = editor(Screen::Mib);
        ed.arc = Some(arc_with_tables());
        ed.mib_cursor = i;
        add!("Mib", ed);
    }
    // ギルド: the four views and the two editors.
    for view in [GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, GUILD_TAB_ARENA] {
        let mut ed = editor(Screen::Guild);
        ed.set_guild_view(view);
        add!("Guild", ed);
    }
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_WEAPONS);
    ed.guild_wpn_edit = true;
    add!("Guild", ed);
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_ARENA);
    ed.arena_edit = true;
    add!("Guild", ed);
    // 進行状況: three views.
    for tab in 0..3 {
        let mut ed = editor(Screen::Progress);
        ed.progress_tab = tab;
        add!("Progress", ed);
    }
    // クエスト改造: four tabs, and the two rows of the monster tab.
    for tab in [QUEST_TAB_BASIC, QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, QUEST_TAB_SUPPLIES] {
        let mut ed = editor(Screen::QuestEdit);
        ed.arc = Some(arc_with_tables());
        ed.quest_set_tab(tab);
        add!("QuestEdit", ed);
    }
    for field in [0usize, hbedit::QUEST_MON_SEM_X] {
        let mut ed = editor(Screen::QuestEdit);
        ed.arc = Some(arc_with_tables());
        ed.quest_set_tab(QUEST_TAB_MONSTERS);
        ed.quest_edit_field = field;
        add!("QuestEdit", ed);
    }
    // エントリ: a reward row and a supply row.
    for entry in [0usize, 1] {
        let mut ed = editor(Screen::EntryView);
        ed.arc = Some(arc_with_tables());
        ed.entry = entry;
        add!("EntryView", ed);
    }
    // マイセット: the list and its two forms.
    add!("Sets", editor(Screen::Sets));
    let mut ed = editor(Screen::Sets);
    ed.set_edit = Some(hbedit::SetEdit::default());
    add!("Sets", ed);
    let mut ed = editor(Screen::Sets);
    ed.item_set_edit = Some(hbedit::ItemSetEdit::default());
    add!("Sets", ed);
    // 護石: the list and the form.
    add!("Charms", editor(Screen::Charms));
    let mut ed = editor(Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    add!("CharmEdit", ed);
    // アイテム: the list and the bulk menu.
    add!("Items", editor(Screen::Items));
    let mut ed = editor(Screen::Items);
    ed.open_item_menu();
    add!("Items", ed);
    // 拾い物: the count stage of the bulk add.
    let mut ed = editor(Screen::ItemPick);
    ed.item_pick_mode = 3;
    ed.item_pick_stage = 1;
    add!("ItemPick", ed);
    // オトモ: the two panels.
    add!("Roster", editor(Screen::Roster));
    let mut ed = editor(Screen::Roster);
    ed.palico_equip = Some(hbedit::PalicoEquip {
        palico: 0,
        field: 0,
        pick: [hbedit::PALICO_EQUIP_NONE; hbedit::PALICO_EQUIP_ROWS],
        clear_armed: None,
    });
    add!("Roster", ed);
    let mut ed = editor(Screen::Roster);
    ed.palico_skill = Some(hbedit::PalicoSkill {
        palico: 0,
        tab: 0,
        field: 0,
        picks: [0u8; hbedit::PALICO_MOVE_ROWS],
    });
    add!("Roster", ed);
    // CSV: the armed apply.
    let mut ed = editor(Screen::Csv);
    ed.csv_armed = Some(0);
    add!("Csv", ed);
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

/// The union of the keys a screen names, over every mode it has.
fn named_anywhere() -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for (name, ed) in states() {
        let keys = named(&ed);
        match out.iter_mut().find(|(n, _)| n == name) {
            Some((_, all)) => {
                for k in keys {
                    if !all.iter().any(|x| *x == k) {
                        all.push(k);
                    }
                }
            }
            None => out.push((name.to_string(), keys)),
        }
    }
    out
}

/// The state machine's handler for one screen, as source text.
fn handler_src(name: &str) -> String {
    // エントリ is an arm of Editor::input itself, not an input_* function: its
    // own arm is cut out of that body (searching the whole body would find
    // every other screen's keys too).
    let needle = if name == "input" {
        String::from("Screen::EntryView => match key {")
    } else {
        format!("fn {name}(&mut self, key: Key)")
    };
    let start = HBEDIT_SRC
        .find(&needle)
        .unwrap_or_else(|| panic!("{name} is gone - the audit must be updated"));
    let open = HBEDIT_SRC[start..].find('{').map(|i| start + i).expect("a body");
    let mut depth = 0i32;
    let mut end = open;
    for (i, ch) in HBEDIT_SRC[open..].char_indices() {
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
    HBEDIT_SRC[open..=end].to_string()
}

/// The chip a Key variant is printed as.
fn chip_of(key: &str) -> &'static str {
    match key {
        "Z" => "ZR",
        "Minus" => "-",
        "Plus" => "＋",
        "Left" | "Right" => "◀/▶",
        "Up" | "Down" => "▲/▼",
        "A" => "A",
        "B" => "B",
        "X" => "X",
        "Y" => "Y",
        "R" => "R",
        "L" => "L",
        _ => "?",
    }
}

/// The handler behind each screen, for the key audit.
fn handler_for(name: &str) -> &'static str {
    match name {
        "SaveList" => "input_save_list",
        "Slots" => "input_slots",
        "Char" => "input_char",
        "Items" => "input_items",
        "ItemPick" => "input_item_pick",
        "SkillPick" => "input_skill_pick",
        "JewelPick" => "input_jewel_pick",
        "EquipPick" => "input_equip_pick",
        "Charms" => "input_charms",
        "CharmEdit" => "input_charm_edit",
        "Equip" => "input_equip",
        "EquipEdit" => "input_equip_edit",
        "QuestEquip" => "input_quest_equip",
        "SaveQuests" => "input_save_quests",
        "ArcList" => "input_arc_list",
        "QuestEdit" => "input_quest_edit",
        "ArcEntries" => "input_arc_entries",
        // エントリ is an arm of Editor::input, not a function of its own.
        "EntryView" => "input",
        "Mib" => "input_mib",
        "Gmd" => "input_gmd",
        "Check" => "input_check",
        "Hunts" => "input_hunts",
        "Guild" => "input_guild",
        "Progress" => "input_progress",
        "Roster" => "input_roster",
        "Palico" => "input_palico",
        "Sets" => "input_sets",
        "Csv" => "input_csv",
        "Options" => "input_options",
        "SaveMap" => "input_save_map",
        "Review" => "input_review",
        "Convert" => "input_convert",
        other => panic!("no handler mapped for {other}"),
    }
}

// ---------------------------------------------------------------------------
// A. The eight keys the merge dropped
// ---------------------------------------------------------------------------

/// オトモ's row prints "Lv 50（上限 99）" and the pair that changes it was named
/// nowhere: not on the footer, not on the deck.
#[test]
fn the_roster_names_the_key_that_changes_the_number() {
    let ed = editor(Screen::Roster);
    assert!(names(&ed, "◀/▶"), "{:?}", ed.key_facts());
    assert_eq!(label(&ed, "◀/▶"), "値");
    // The pair really changes the column the [Y] row names.
    let mut ed = editor(Screen::Roster);
    let before = ed.palicos();
    if before.is_empty() {
        return;
    }
    let before = before[0].level;
    ed.roster_field = 0;
    ed.input(Key::Right);
    let after = ed.palicos()[0].level;
    assert_eq!(after, (before + 1).min(99), "the pair must move the value");
}

/// The reward / supply row is the third level of the quest section, and its
/// only exit was 終了: [B] goes back up and was named nowhere.
#[test]
fn the_reward_row_names_the_way_back() {
    let mut ed = editor(Screen::EntryView);
    ed.arc = Some(arc_with_tables());
    ed.entry = 0;
    assert!(ed.entry_has_numbers());
    assert!(names(&ed, "B"), "{:?}", ed.key_facts());
    assert_eq!(label(&ed, "B"), "戻る");
    // The key really goes back to the file list.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
}

/// セーブ内クエスト's delete is in README and was on the pre-merge chip table,
/// and the line did not name it.
#[test]
fn the_save_quest_list_names_the_delete_key() {
    let ed = editor(Screen::SaveQuests);
    assert!(names(&ed, "-"), "{:?}", ed.key_facts());
    assert!(label(&ed, "-").contains("削除"), "{:?}", ed.key_facts());
}

/// The card's [A] only ever adds a step, so the pair that takes a number back
/// down is the one key the view cannot do without - and it was not named.
#[test]
fn the_card_view_names_the_arrow_pair() {
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_CARD);
    assert!(names(&ed, "◀/▶"), "{:?}", ed.key_facts());
    assert!(names(&ed, "A"), "{:?}", ed.key_facts());
    // The pair moves the number the [A] raises, and takes it back.
    let before = ed.save.as_ref().expect("a save").to_bytes();
    ed.input(Key::Right);
    let up = ed.save.as_ref().expect("a save").to_bytes();
    assert_ne!(up, before, "▶ must move the value");
    ed.input(Key::Left);
    let back = ed.save.as_ref().expect("a save").to_bytes();
    assert_eq!(back, before, "◀ must take it back down");
}

/// The item box is the one list whose [Y] changes what the list shows, and the
/// key was named only on the deck's pill and in a message.
#[test]
fn the_item_box_names_the_empty_slot_toggle() {
    let mut ed = editor(Screen::Items);
    assert!(names(&ed, "Y"), "{:?}", ed.key_facts());
    assert_eq!(label(&ed, "Y"), "空き枠");
    let before = ed.show_empty;
    ed.input(Key::Y);
    assert_ne!(ed.show_empty, before, "[Y] must toggle the list");
}

/// The talisman form's [Y] is the search route into the skill and decoration
/// lists, and the line did not name it.
#[test]
fn the_talisman_form_names_the_search_key() {
    let mut ed = editor(Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(names(&ed, "Y"), "{:?}", ed.key_facts());
    assert_eq!(label(&ed, "Y"), "検索");
    // On a skill row it opens the picker with the search box up.
    ed.charm_edit.as_mut().expect("a form").field = 3;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert!(ed.take_text_request().is_some(), "[Y] must ask for the name");
}

/// The equipment form's [R] is the ID keyboard on the two identity rows, and
/// the line did not name it.
#[test]
fn the_equipment_form_names_the_id_key() {
    let mut ed = editor(Screen::EquipEdit);
    ed.equip_edit = Some(hbedit::EquipEdit {
        new: true,
        ..hbedit::EquipEdit::default()
    });
    assert!(names(&ed, "R"), "{:?}", ed.key_facts());
    assert_eq!(label(&ed, "R"), "ID入力");
    ed.equip_edit.as_mut().expect("a form").field = 1;
    ed.input(Key::R);
    assert!(ed.take_text_request().is_some(), "[R] must ask for the id");
}

/// 生データ edits bytes, so [X] is the save key there like everywhere else -
/// and neither mode named it.
#[test]
fn the_raw_data_screen_names_the_save_key_in_both_modes() {
    let mut ed = editor(Screen::SaveMap);
    ed.open_save_map(Screen::Char);
    assert_eq!(ed.screen, Screen::SaveMap);
    assert!(names(&ed, "X"), "the list: {:?}", ed.key_facts());
    ed.input(Key::A);
    assert!(ed.map_edit_open(), "{}", ed.message);
    assert!(names(&ed, "X"), "the byte editor: {:?}", ed.key_facts());
}

// ---------------------------------------------------------------------------
// A. The two audits
// ---------------------------------------------------------------------------

/// The pre-merge chip table, key by key.
///
/// This is switch/src/editor.rs actions at ccefa48~1, with the three global
/// keys dropped: [L/R] (the sub-tab pair), [X] (the save key, drawn on the
/// header pill) and [+] (the way out, always the last item of the table) are
/// documented once in README and are not one screen's promise. Everything else
/// the table named has to be on the line, because that is the list the merge
/// cut from.
const PRE_MERGE: &[(&str, &[&str])] = &[
    ("SaveList", &["A", "Y"]),
    ("Slots", &["A", "Y", "B"]),
    ("Char", &["A", "◀/▶", "Y", "B"]),
    ("Items", &["A", "◀/▶", "R", "Y", "ZR", "B"]),
    ("ItemPick", &["A", "◀/▶", "▲/▼", "Y", "B"]),
    ("SkillPick", &["A", "◀/▶", "▲/▼", "Y", "B"]),
    ("JewelPick", &["A", "◀/▶", "▲/▼", "Y", "B"]),
    ("EquipPick", &["A", "◀/▶", "▲/▼", "Y", "ZR", "B"]),
    ("Charms", &["A", "Y", "-", "B"]),
    ("CharmEdit", &["A", "◀/▶", "Y", "B"]),
    ("Equip", &["A", "Y", "-", "B"]),
    ("EquipEdit", &["A", "◀/▶", "Y", "R", "B"]),
    ("QuestEquip", &["A", "B"]),
    ("SaveQuests", &["A", "-", "Y", "ZR", "B"]),
    ("ArcList", &["A", "Y", "ZR", "B"]),
    ("QuestEdit", &["A", "◀/▶", "Y", "ZR", "R", "B"]),
    ("ArcEntries", &["A", "Y", "ZR", "B"]),
    ("EntryView", &["A", "◀/▶", "Y", "ZR", "R", "B"]),
    ("Mib", &["A", "◀/▶", "Y", "ZR", "B"]),
    ("Gmd", &["A", "B"]),
    ("Check", &["A", "B"]),
    ("Hunts", &["A", "◀/▶", "Y", "ZR", "R", "B"]),
    ("Guild", &["A", "◀/▶", "Y", "ZR", "R", "B"]),
    ("Progress", &["A", "◀/▶", "Y", "ZR", "R", "B"]),
    ("Roster", &["A", "◀/▶", "Y", "ZR", "B"]),
    ("Palico", &["A", "◀/▶", "Y", "ZR", "B"]),
    ("Sets", &["A", "◀/▶", "-", "Y", "B"]),
    ("Options", &["◀/▶", "Y", "B"]),
    ("Csv", &["A", "Y", "ZR", "B"]),
    ("SaveMap", &["A", "▲/▼", "◀/▶", "Y", "B"]),
    ("Review", &["A", "▲/▼", "◀/▶", "Y", "B"]),
];

/// Every key the pre-merge chip table named is on the line again.
///
/// This is the audit the review asked for: the merge cut eight keys, and the
/// only way to see that is to compare the table that had them with the table
/// that replaced it. The one key it cannot see is 生データ's [X] (the old table
/// did not name it either); the test above pins that one.
#[test]
fn every_key_the_pre_merge_chip_table_named_is_on_the_line() {
    let anywhere = named_anywhere();
    let mut checked = 0usize;
    for (screen, keys) in PRE_MERGE {
        let Some((_, have)) = anywhere.iter().find(|(n, _)| n == screen) else {
            panic!("{screen} is not in the state list - the audit must be updated");
        };
        for key in *keys {
            assert!(
                have.iter().any(|k| k == key),
                "{screen}: the pre-merge chip table named [{key}] and the key table does not: {have:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "the table was read: {checked} keys");
}

/// A key a screen answers has to be a key its line names.
///
/// The keys every screen shares are the ones README's global table documents -
/// the four directions (cursor and value), [L] / [R] (the sub-tab pair), [X]
/// (save), [+] (quit) - plus [B], which every screen answers as "go back" and
/// which the pre-merge audit above pins per screen. Everything else is a key
/// with a job of its own, and a job no line names is the bug this review is
/// about.
#[test]
fn no_key_is_pressable_without_a_line_in_the_table() {
    const GLOBAL: &[&str] = &["Up", "Down", "Left", "Right", "L", "R", "X", "Plus", "B"];
    // Keys a handler answers with a refusal rather than a job: the press says
    // why it cannot do anything, which is the honest answer, and a line has no
    // word for a key that does nothing.
    const SILENT: &[(&str, &str)] = &[("Sets", "Minus")];
    let anywhere = named_anywhere();
    let mut checked = 0usize;
    for (screen, keys) in &anywhere {
        let body = handler_src(handler_for(screen));
        let mut answered: Vec<String> = Vec::new();
        for line in body.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let mut rest = line;
            while let Some(at) = rest.find("Key::") {
                let tail = &rest[at + 5..];
                let name: String = tail
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if !name.is_empty() && !answered.iter().any(|k| *k == name) {
                    answered.push(name);
                }
                rest = tail;
            }
        }
        for key in answered {
            if GLOBAL.contains(&key.as_str()) {
                continue;
            }
            if SILENT.iter().any(|(s, k)| s == screen && *k == key) {
                continue;
            }
            let chip = chip_of(&key);
            assert!(
                keys.iter().any(|k| k == chip),
                "{screen}: the handler answers Key::{key} and the line names no such key: {keys:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 40, "the handlers were read: {checked} keys");
}

// ---------------------------------------------------------------------------
// B. The deck and the footer's second line
// ---------------------------------------------------------------------------

/// The deck's buttons and the inspector's buttons print the table's words.
///
/// The sixth copy of the key meanings was the deck's hand-written labels: six
/// of them named a different job than the footer did (the card's [B] スロット
/// over キャラ, マイセット's [◀/▶] 切替 over タブ, the item box's [A] アイテム変更
/// over 変更, the roster's [A] 名前変更 over 名前, the character screen's [A]
/// 名前変更 over 最大, the palico list's [Y] 全84匹を一括導入 over 全導入2回).
/// They borrow the word now, so a copy cannot drift.
#[test]
fn the_deck_borrows_the_tables_words() {
    for needle in [
        "ed.key_label_or(\"B\"",
        "ed.key_label_or(\"◀/▶\"",
        "ed.key_label_or(\"A\"",
        "ed.key_label_or(\"Y\"",
        "ed.key_label_or(\"R\"",
    ] {
        assert!(UI_SRC.contains(needle), "the deck must borrow {needle}");
    }
    for gone in ["\"[B] スロット\"", "\"アイテム変更\"", "\"名前変更\"", "一括導入(2回)"] {
        assert!(!UI_SRC.contains(gone), "the deck still writes its own copy: {gone}");
    }
    // The tables the buttons read really answer.
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_CARD);
    assert_eq!(ed.key_label("B").as_deref(), Some("キャラ"));
    let ed = editor(Screen::Sets);
    assert_eq!(ed.key_label("◀/▶").as_deref(), Some("タブ"));
    let ed = editor(Screen::Items);
    assert_eq!(ed.key_label("A").as_deref(), Some("変更"));
    let ed = editor(Screen::Roster);
    assert_eq!(ed.key_label("A").as_deref(), Some("名前"));
    let ed = editor(Screen::Palico);
    assert_eq!(ed.key_label("Y").as_deref(), Some("全導入2回"));
}

/// The footer's second line is the first line's table, not a sixth wording.
///
/// The pick lists' message said "10件送り / 名前で検索" while the footer above it
/// said "10件 / 検索", and the bulk add's message said "±10 / [A] 決定" while the
/// footer said "個数±10 / [A] この個数を追加".
#[test]
fn the_footer_second_line_is_the_key_table() {
    let mut ed = editor(Screen::ItemPick);
    assert_eq!(ed.pick_key_hint(), ed.footer_hint());
    assert!(ed.pick_key_hint().contains("[◀/▶] 10件"), "{}", ed.pick_key_hint());
    assert!(!ed.pick_key_hint().contains("10件送り"), "{}", ed.pick_key_hint());
    assert!(!ed.pick_key_hint().contains("名前で検索"), "{}", ed.pick_key_hint());

    // The bulk add's stage two: the item and the count, then the table.
    ed.item_pick_mode = 3;
    ed.item_pick_stage = 1;
    ed.item_pick_add_id = 1;
    ed.item_pick_add_count = 3;
    let msg = ed.item_add_message();
    assert!(msg.contains(" を 3 個追加"), "{msg}");
    assert!(msg.contains(&ed.footer_hint()), "{msg}");
    assert!(!msg.contains("[A] 決定"), "{msg}");
}

// ---------------------------------------------------------------------------
// C. The guide names the primary action
// ---------------------------------------------------------------------------

/// The guide always names [A] and the pair that steps it.
///
/// It printed the table's first two items, so eleven states whose line starts
/// with a direction pair (the card's four views, 闘技大会, 変更一覧, 生データ in
/// both modes, the pick list's count stage, マイセット's weapon rows and
/// クエスト改造's 基本 tab) showed no [A] at all.
#[test]
fn the_guide_always_names_the_primary_action() {
    let mut checked = 0usize;
    for (name, ed) in states() {
        let facts = ed.guide_facts();
        assert_eq!(facts.len(), 2, "{name}: the guide prints two items");
        let footer = ed.footer_hint();
        for fact in &facts {
            assert!(
                footer.contains(&fact.render()),
                "{name}: the guide names {} and the footer does not: {footer}",
                fact.render()
            );
        }
        if names(&ed, "A") {
            assert!(
                facts.iter().any(|f| f.key == "A"),
                "{name}: the guide does not name [A]: {:?}",
                facts.iter().map(|f| f.render()).collect::<Vec<_>>()
            );
            checked += 1;
        }
    }
    assert!(checked > 30, "the states were read: {checked}");
}

// ---------------------------------------------------------------------------
// D. [X] is the save key
// ---------------------------------------------------------------------------

/// [X] saves on every screen, in the state machine as well as on the console.
///
/// Five screens gave it a job of their own (設定, the item box, the save's quest
/// list, the .arc list and the file list's install), and the console's global X
/// had already taken the key back for the save - so the state machine's meaning
/// was one no pad could reach, and the line said 保存 over a key that did
/// something else.
#[test]
fn x_is_the_save_key_on_every_screen() {
    for screen in [
        Screen::SaveList,
        Screen::Slots,
        Screen::SaveQuests,
        Screen::ArcList,
        Screen::ArcEntries,
    ] {
        let mut ed = editor(screen);
        if screen == Screen::ArcEntries {
            ed.arc = Some(arc_with_tables());
        }
        ed.input(Key::X);
        assert_eq!(ed.screen, screen, "{screen:?}: [X] must not navigate");
        assert!(
            ed.take_request().is_some() || ed.take_write().is_some(),
            "{screen:?}: [X] must ask for a write: {}",
            ed.message
        );
    }
}

// ---------------------------------------------------------------------------
// E. The two-press offers
// ---------------------------------------------------------------------------

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

/// The talisman form's overwrite is taken back by any other press.
///
/// The offer was kept for every [A], so a press on a skill row (or on another
/// row a tap had moved the cursor to) came back to the box-slot row with the
/// offer still standing, and one press wrote a record the user had not
/// confirmed twice.
#[test]
fn a_press_on_another_row_takes_the_overwrite_offer_back() {
    let mut ed = editor_with_two_charms();
    let form = ed.charm_edit.expect("a form");
    ed.charm_edit = Some(hbedit::CharmEdit {
        index: if form.index == 0 { 1 } else { 0 },
        // Row 0 is the box-slot row, which is the one row that writes.
        field: 0,
        ..form
    });
    assert!(ed.charm_write_replaces(), "the press would replace a record");
    ed.input(Key::A);
    assert!(ed.charm_write_armed.is_some(), "the first press arms it");
    // [A] on a skill row is a different press: it must take the offer back.
    ed.charm_edit.as_mut().expect("a form").field = 3;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::SkillPick, "the skill row opens the list");
    assert!(
        ed.charm_write_armed.is_none(),
        "a press on another row must take the overwrite back"
    );
}

/// The equipment form's type change is taken back by any other press.
///
/// The form had no rule at all: the arm survived a cursor move, a pick from the
/// list and a [B] that cancelled a draft, and the next [A] on the write row
/// replaced the record without asking again.
#[test]
fn the_equipment_type_change_offer_is_taken_back() {
    let mut ed = editor(Screen::EquipEdit);
    ed.equip_edit = Some(hbedit::EquipEdit {
        new: true,
        ..hbedit::EquipEdit::default()
    });
    ed.equip_kind_armed = Some((0, 1));
    ed.input(Key::Down);
    assert!(
        ed.equip_kind_armed.is_none(),
        "a cursor move must take the type-change offer back"
    );
    ed.equip_kind_armed = Some((0, 1));
    ed.input(Key::Y);
    assert!(
        ed.equip_kind_armed.is_none(),
        "[Y] must take the type-change offer back"
    );
    // The confirmation itself keeps it: [A] on the write row is the press the
    // offer is for.
    ed.equip_kind_armed = Some((0, 1));
    let field = ed.equip_edit.expect("a form").write_row();
    ed.equip_edit.as_mut().expect("a form").field = field;
    assert_eq!(
        ed.equip_edit.expect("a form").a_action(),
        EquipFormAction::CreateRecord,
        "the new form's write row creates"
    );
}

/// The talisman form says the write costs two presses before it is pressed.
#[test]
fn the_talisman_form_says_two_presses_before_the_press() {
    let mut ed = editor_with_two_charms();
    let form = ed.charm_edit.expect("a form");
    ed.charm_edit = Some(hbedit::CharmEdit {
        index: if form.index == 0 { 1 } else { 0 },
        field: 0,
        ..form
    });
    assert!(ed.charm_write_replaces());
    assert_eq!(label(&ed, "A"), "書き込み（2回）");
    // A write that replaces nothing is one press, and the line says so.
    let mut ed = editor(Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    ed.charm_edit.as_mut().expect("a form").field = 0;
    assert!(!ed.charm_write_replaces());
    assert_eq!(label(&ed, "A"), CharmAction::WriteToBox.label());
}

// ---------------------------------------------------------------------------
// F. One line per screen
// ---------------------------------------------------------------------------

/// 護石一覧's line is written once.
///
/// It was in both screen_facts and static_facts, and the first always answered,
/// so the second copy was dead code - the shape of second table this whole
/// change exists to delete.
#[test]
fn the_talisman_list_has_one_line() {
    let start = HBEDIT_SRC.find("fn static_facts(&self)").expect("static_facts");
    let open = HBEDIT_SRC[start..].find('{').map(|i| start + i).expect("a body");
    let mut depth = 0i32;
    let mut end = open;
    for (i, ch) in HBEDIT_SRC[open..].char_indices() {
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
    let body = &HBEDIT_SRC[open..=end];
    let arm = body
        .find("Screen::Charms =>")
        .expect("the static table still lists the screen");
    let tail = &body[arm..];
    let stop = tail.find("Screen::CharmEdit =>").expect("the next arm");
    assert!(
        tail[..stop].contains("alloc::vec![]"),
        "the static table writes the talisman line twice: {}",
        &tail[..stop]
    );
    // And the screen still has a line.
    let ed = editor(Screen::Charms);
    assert!(names(&ed, "A") && names(&ed, "B"), "{:?}", ed.key_facts());
}

/// The palico equipment panel names the key that ends the session.
///
/// plus_is_a_tool tested "not palico_skill_open", which made the *equipment*
/// panel the tool and the skill panel the quit key - the opposite of the rule
/// its own doc comment states - so the roster's footer said the way out was [B]
/// over a [+] that quit.
#[test]
fn the_palico_equipment_panel_names_the_quit_key() {
    let mut ed = editor(Screen::Roster);
    ed.palico_equip = Some(hbedit::PalicoEquip {
        palico: 0,
        field: 0,
        pick: [hbedit::PALICO_EQUIP_NONE; hbedit::PALICO_EQUIP_ROWS],
        clear_armed: None,
    });
    assert!(!ed.plus_is_a_tool(), "[+] quits inside the panel");
    assert!(ed.footer_hint().contains("[＋] 終了"), "{}", ed.footer_hint());
    // The skill panel quits too.
    let mut ed = editor(Screen::Roster);
    ed.palico_skill = Some(hbedit::PalicoSkill {
        palico: 0,
        tab: 0,
        field: 0,
        picks: [0u8; hbedit::PALICO_MOVE_ROWS],
    });
    assert!(!ed.plus_is_a_tool(), "[+] quits there too");
    let ed = editor(Screen::Roster);
    assert!(ed.plus_is_a_tool(), "[+] opens the panel on the list");
}

/// An error says what to press next.
#[test]
fn the_draft_error_names_the_next_key() {
    for needle in [
        "枠を変えて [A] で書き込むか、[B] を2回で下書きを捨ててください",
        "書き込みの行で [A] をもう一度か、[B] を2回で下書きを捨ててください",
    ] {
        assert!(
            HBEDIT_SRC.contains(needle),
            "the refusal must say what to press: {needle}"
        );
    }
}

/// The key column holds keys, and the words that are not keys say what they are.
#[test]
fn the_key_column_holds_only_keys() {
    // The way-out note names no key of its own, so it is marked as a note.
    let mut ed = editor(Screen::Charms);
    let hint = ed.footer_hint();
    assert!(hint.contains("※終了は [B]"), "{hint}");
    assert!(!hint.contains(" 終了は [B] の後"), "{hint}");
    // 進行状況's pair moves rows, and the word says so.
    ed.screen = Screen::Progress;
    assert!(ed.footer_hint().contains("[◀/▶] ±50行"), "{}", ed.footer_hint());
    // The two-press forms say what the second press does.
    let mut ed = editor(Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert!(ed.footer_hint().contains("（2回で破棄）"), "{}", ed.footer_hint());
    // One name for the slot screen on both builds.
    assert!(EDITOR_SRC.contains("\"キャラ選択\""), "the text build's title");
    assert!(!EDITOR_SRC.contains("キャラクター選択"), "the long name is gone");
    // The reward row's [ZR] reads as an action.
    let mut ed = editor(Screen::EntryView);
    ed.arc = Some(arc_with_tables());
    ed.entry = 0;
    assert!(ed.footer_hint().contains("[ZR] 段送り（3段）"), "{}", ed.footer_hint());
}

/// README says the three things the sixth review found it did not.
#[test]
fn the_readme_names_the_three_cases() {
    assert!(
        README_SRC.contains("バナーが出ている間だけ"),
        "the banner's [A] is only live while the banner is up"
    );
    assert!(
        README_SRC.contains("装備パネル"),
        "the palico equipment panel quits on [+] too"
    );
    assert!(
        README_SRC.contains("全部は出ません"),
        "the footer does not name every key on every screen"
    );
}

/// Every line still fits the strip it is drawn in, with the eight keys back.
#[test]
fn every_line_still_fits_with_the_eight_keys_back() {
    for (name, ed) in states() {
        let hint = ed.footer_hint();
        assert!(
            caption_width(&hint) <= FOOTER_HINT_ROOM,
            "{name}: {}px of {FOOTER_HINT_ROOM}px: {hint}",
            caption_width(&hint)
        );
    }
}
