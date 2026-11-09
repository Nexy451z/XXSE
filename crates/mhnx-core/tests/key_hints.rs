//! The key guide and the keys that work, checked against each other.
//!
//! The review that moved every job off [-] found the same bug in four places:
//! a line on screen named a key, and the key did something else (or nothing).
//! A guide is a claim about the dispatch, and a claim can be checked: these
//! tests read the strings the console draws and the handlers that answer them,
//! and fail when the two disagree.
//!
//! Two sources are read as text because the crates cannot call each other:
//! `switch/src/editor.rs` needs the Nintendo SDK and is not part of this
//! workspace, so its chip table is included as a string the same way
//! `hbedit_flow` does it.

use mhnx_core::hbedit::{Editor, Screen, ZrJob};

const HBEDIT_SRC: &str = include_str!("../src/hbedit.rs");
const SWITCH_EDITOR_SRC: &str = include_str!("../../../switch/src/editor.rs");

// ---------------------------------------------------------------------------
// What the console answers before the state machine sees the key
// ---------------------------------------------------------------------------

/// The keys `switch/src/ui.rs handle_pad_key` answers on a screen without
/// passing them to `Editor::input`.
///
/// This is the one split a text audit cannot read out of the state machine, so
/// it is written down once. Every entry is a key the console really handles:
///
/// * Sets - the list's ◀ / ▶ / Y switch tabs (the set form owns them instead,
///   and its footer names them as its own keys).
/// * Palico - ◀ / ▶ run `UiAction::TogglePalicoLang`.
/// * ArcList - [B] goes back to セーブ内 (the state machine has no arm).
/// * Mib - [Y] is the step and [ZR] walks a monster slot's eight values.
/// * Options / EntryView - [Y] is the snapshot restore and the step.
/// * Slots - ◀ / ▶ pick the previous / next slot (the state machine only
///   answers ▲ / ▼).
const CONSOLE_ONLY: &[(&str, &[&str])] = &[
    ("Slots", &["Left", "Right"]),
    ("Sets", &["Left", "Right", "Y"]),
    ("Palico", &["Left", "Right"]),
    ("ArcList", &["B"]),
    ("Mib", &["Y", "Z"]),
    ("Options", &["Y"]),
    ("EntryView", &["Y"]),
];

/// The state machine's handler for one screen, as source text.
///
/// The pick lists and the forms each have an `input_*` function; `EntryView` is
/// an arm of `Editor::input` itself, so its own arm is cut out of that body
/// (searching the whole body would find every other screen's keys too).
fn handler_src(screen: &str) -> String {
    let (name, arm) = match screen {
        "EntryView" => ("input", Some("Screen::EntryView => match key {")),
        "SaveList" => ("input_save_list", None),
        "Slots" => ("input_slots", None),
        "Char" => ("input_char", None),
        "Items" => ("input_items", None),
        "ItemPick" => ("input_item_pick", None),
        "SkillPick" => ("input_skill_pick", None),
        "JewelPick" => ("input_jewel_pick", None),
        "EquipPick" => ("input_equip_pick", None),
        "Charms" => ("input_charms", None),
        "CharmEdit" => ("input_charm_edit", None),
        "Equip" => ("input_equip", None),
        "EquipEdit" => ("input_equip_edit", None),
        "QuestEquip" => ("input_quest_equip", None),
        "SaveQuests" => ("input_save_quests", None),
        "ArcList" => ("input_arc_list", None),
        "QuestEdit" => ("input_quest_edit", None),
        "ArcEntries" => ("input_arc_entries", None),
        "Mib" => ("input_mib", None),
        "Gmd" => ("input_gmd", None),
        "Check" => ("input_check", None),
        "Hunts" => ("input_hunts", None),
        "Guild" => ("input_guild", None),
        "Progress" => ("input_progress", None),
        "Roster" => ("input_roster", None),
        "Palico" => ("input_palico", None),
        "Sets" => ("input_sets", None),
        "SetEdit" => ("input_set_edit", None),
        "ItemSetEdit" => ("input_item_set_edit", None),
        "Csv" => ("input_csv", None),
        "Options" => ("input_options", None),
        "SaveMap" => ("input_save_map", None),
        "Review" => ("input_review", None),
        "Convert" => ("input_convert", None),
        other => panic!("no handler mapped for {other} - the audit must be updated"),
    };
    let needle = match arm {
        Some(a) => a.to_string(),
        None => format!("fn {name}(&mut self, key: Key)"),
    };
    let start = HBEDIT_SRC
        .find(&needle)
        .unwrap_or_else(|| panic!("{name} is gone - the audit must be updated"));
    let open = HBEDIT_SRC[start..]
        .find('{')
        .map(|i| start + i)
        .expect("a body");
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

/// Does the screen answer this key, in either layer?
fn answers(screen: &str, key: &str) -> bool {
    // [X] is the save key on every screen and [＋] quits on every screen that
    // does not give it a tool of its own; the console answers both before the
    // state machine (handle_pad_key's QuickSave and the main loop's BTN_PLUS).
    // The shoulders are always something (sub-tab, ten-row move or the screen's
    // own key), so a line that names them is never a dead key.
    if matches!(key, "X" | "Plus" | "L" | "R") {
        return true;
    }
    let mut body = handler_src(screen);
    // The set screen has three modes (the list and its two forms), and the
    // chips name the form's keys as well as the list's.
    if screen == "Sets" {
        body.push_str(&handler_src("SetEdit"));
        body.push_str(&handler_src("ItemSetEdit"));
    }
    if body.contains(&format!("Key::{key}")) {
        return true;
    }
    // The pick lists move through one helper (hbedit::pick_move), so the four
    // directions are answered without a Key arm of their own.
    if body.contains("pick_move(") && matches!(key, "Left" | "Right" | "Up" | "Down") {
        return true;
    }
    CONSOLE_ONLY
        .iter()
        .any(|(s, keys)| *s == screen && keys.contains(&key))
}

/// The keys a guide line names, as `Key` variant names.
///
/// Only the spellings this interface uses are read: a pill ([ファイル],
/// [生データ], [MAX]) is a button, not a key, and [ZL/ZR] is the section bar -
/// neither is one screen's answer.
fn named_keys(text: &str) -> Vec<String> {
    let table: &[(&str, &[&str])] = &[
        ("[A]", &["A"]),
        ("[B]", &["B"]),
        ("[X]", &["X"]),
        ("[Y]", &["Y"]),
        ("[ZR]", &["Z"]),
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

// ---------------------------------------------------------------------------
// 1. [-] never has a job that is not a delete
// ---------------------------------------------------------------------------

/// Every [-] the user can read is a delete, and nothing else.
///
/// The review's finding was that [-] carried nineteen meanings and five of them
/// destroyed something, so the key that removes a record was also the key that
/// opens a menu. It deletes now; this walks every string literal in the core
/// and in the console's chip table and refuses any other reading.
#[test]
fn audit_no_hint_gives_the_delete_key_any_other_job() {
    // The words that make a [-] line a delete. A two-press delete says
    // 削除 / 解除 / 確定, and the one formatted line ("もう一度 [-] で{}")
    // fills the blank with 削除 or 強制削除.
    let delete_words = ["削除", "解除", "確定", "で{}"];
    let mut seen = 0usize;
    for (i, line) in HBEDIT_SRC.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        // The key's own name in the key_name table is not a hint.
        if line.contains("Key::Minus =>") {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("[-]") {
            let after = &rest[at + 3..];
            let window: String = after.chars().take(14).collect();
            assert!(
                delete_words.iter().any(|w| window.contains(w)),
                "hbedit.rs:{}: [-] is the delete key, but this line gives it \
                 another job: {}",
                i + 1,
                line.trim()
            );
            seen += 1;
            rest = after;
        }
    }
    assert!(seen >= 10, "the delete lines are still there: {seen}");

    // The console's chip table: a "-" chip is a delete chip.
    for (screen, entries) in switch_hint_arms() {
        for (key, label) in entries {
            if key != "-" {
                continue;
            }
            assert!(
                label.contains("削除") || label.contains("解除"),
                "{screen}: the [-] chip says {label:?}, which is not a delete"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. every key the footer names is a key the screen answers
// ---------------------------------------------------------------------------

/// The footer's line for each screen, checked key by key.
///
/// This is the check that would have caught the name picker: its footer named
/// [ZR] 種別 while the handler still answered `Key::Minus`, so the pad pressed
/// a key nothing on that screen heard.
#[test]
fn audit_every_key_the_footer_names_is_one_the_screen_answers() {
    let mut ed = Editor::new();
    let mut checked = 0usize;
    for screen in AUDIT_SCREENS {
        ed.screen = *screen;
        // The guild screen writes a different line per view, so every view is
        // checked; the others have one line.
        let views: &[usize] = if *screen == Screen::Guild {
            &[0, 1, 2, 3]
        } else {
            &[0]
        };
        for view in views {
            if *screen == Screen::Guild {
                ed.set_guild_view(*view);
            }
            let hint = ed.footer_hint();
            let name = screen_name(*screen);
            for key in named_keys(&hint) {
                assert!(
                    answers(name, &key),
                    "{name} (view {view}): the footer names {key} but the \
                     screen does not answer it: {hint}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "the footers were read: {checked} keys");
}

/// The [ZR] the footer names is a [ZR] the console delivers.
///
/// `zr_job` is the one answer to "whose key is ZR", and the console asks it
/// before it sends the key: a screen whose footer says [ZR] must be a screen
/// that answers `ZrJob::Screen`, or the press becomes a section jump.
#[test]
fn audit_a_footer_that_names_zr_is_a_screen_zr_reaches() {
    let mut ed = Editor::new();
    for screen in AUDIT_SCREENS {
        ed.screen = *screen;
        let hint = ed.footer_hint();
        if !hint.contains("[ZR]") {
            continue;
        }
        assert_eq!(
            ed.zr_job(),
            ZrJob::Screen,
            "{:?}: the footer names [ZR] but the console spends it on the \
             section bar: {hint}",
            screen
        );
    }
}

// ---------------------------------------------------------------------------
// 3. the console's chips name keys the console answers
// ---------------------------------------------------------------------------

/// The Switch build's chip table, read straight out of its source.
fn switch_hint_arms() -> Vec<(String, Vec<(String, String)>)> {
    let start = SWITCH_EDITOR_SRC
        .find("fn actions(screen: mhnx_core::hbedit::Screen)")
        .expect("actions() is gone - the audit must be updated");
    let body = &SWITCH_EDITOR_SRC[start..];
    let body = &body[..body.find("\n}").expect("actions() has no closing brace")];
    let mut arms = Vec::new();
    let mut rest = body;
    loop {
        let Some(pos) = rest.find("=>") else { break };
        let name: String = rest[..pos]
            .trim_end()
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let after = rest[pos + 2..].trim_start();
        if name.is_empty() || !after.starts_with("&[") {
            rest = &rest[pos + 2..];
            continue;
        }
        let arr = &after[1..];
        let close = arr.find(']').expect("an arm must end with ]");
        let mut entries = Vec::new();
        let mut tail = &arr[..close];
        while let Some(k) = tail.find("(\"") {
            let t1 = &tail[k + 2..];
            let Some(e1) = t1.find('"') else { break };
            let key = &t1[..e1];
            let t2 = &t1[e1 + 1..];
            let Some(q2) = t2.find('"') else { break };
            let t3 = &t2[q2 + 1..];
            let Some(e2) = t3.find('"') else { break };
            entries.push((key.to_string(), t3[..e2].to_string()));
            tail = &t3[e2..];
        }
        arms.push((name, entries));
        rest = &after[close + 2..];
    }
    arms
}

/// The chip names a key, so the key has to be a key this build answers.
#[test]
fn audit_the_chips_name_keys_the_console_answers() {
    let mut checked = 0usize;
    for (screen, entries) in switch_hint_arms() {
        for (key, label) in entries {
            let keys: Vec<String> = match key.as_str() {
                "A" => vec!["A".into()],
                "B" => vec!["B".into()],
                "X" => vec!["X".into()],
                "Y" => vec!["Y".into()],
                "ZR" => vec!["Z".into()],
                "+" => vec!["Plus".into()],
                "-" => vec!["Minus".into()],
                "R" => vec!["R".into()],
                "L" => vec!["L".into()],
                "L/R" => vec!["L".into(), "R".into()],
                "</>" => vec!["Left".into(), "Right".into()],
                "◀/▶" => vec!["Left".into(), "Right".into()],
                "▲/▼" => vec!["Up".into(), "Down".into()],
                // The section bar and the 詳細 pill are not one screen's key.
                "ZL/ZR" | "検索" | "ファイル" => continue,
                other => panic!("{screen}: unknown chip key {other:?}"),
            };
            for k in keys {
                assert!(
                    answers(&screen, &k),
                    "{screen}: the chip {key} {label:?} names {k}, which this \
                     screen does not answer"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "the chips were read: {checked} keys");
}

// ---------------------------------------------------------------------------
// the screen list
// ---------------------------------------------------------------------------

/// Every screen, in the order the state machine declares them.
const AUDIT_SCREENS: &[Screen] = &[
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
