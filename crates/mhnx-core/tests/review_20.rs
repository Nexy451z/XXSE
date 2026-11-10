//! The fifth review, one test per finding - and a test that reads the pad.
//!
//! The review's first finding was about the tests themselves: the key table in
//! the text build (switch/src/editor.rs actions()) is the ASCII console's
//! fallback only, while the console draws hbedit::Editor::footer_hint and
//! dispatches through switch/src/ui.rs handle_pad_key. A test that compares
//! actions() with actions() therefore proves nothing about the machine.
//!
//! So the audit below reads handle_pad_key itself: it cuts the
//! "match ed.screen" arms out of the source, works out what the console really
//! does with each (screen, key), and then *drives* that answer on an Editor.
//! A key the table names that the dispatch ignores fails the test, and so does a
//! key the dispatch answers with nothing.
//!
//! Nothing here reads or writes a card: every save is built in memory.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::charm;
use mhnx_core::hbedit::{Editor, Key, Screen, QUEST_REWARD_CHANCE, QUEST_TAB_REWARDS};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};
use mhnx_core::save::{self, Save};

const HASH_MIB: u32 = 0x1BBF_D18E;
const HASH_SEM: u32 = 0x2553_701D;
const HASH_REM: u32 = 0x5B3C_302D;
const HASH_GMD: u32 = 0x242B_B29A;
const QUEST_ID: i32 = 1_010_165;

// ---------------------------------------------------------------------------
// The console layer, as text (the dispatch, not the table)
// ---------------------------------------------------------------------------

fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
}

fn switch_editor_src() -> &'static str {
    include_str!("../../../switch/src/editor.rs")
}

fn readme() -> &'static str {
    include_str!("../../../README.md")
}

/// The state machine's source: the one table of screen names and key words.
fn hbedit_src() -> &'static str {
    include_str!("../src/hbedit.rs")
}

/// The body of one fn in a source text, from its signature to its closing
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

/// What the console does with one key on one screen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Answer {
    /// ed.input(...): the state machine's own answer.
    Input,
    /// ed.<method>(...): a named core method.
    Call(String),
    /// self.apply_action(ed, UiAction::X): a console action.
    Action(String),
    /// {} / _ => {}: the console does nothing with the key.
    Nothing,
}

/// The arms of handle_pad_key's "match ed.screen", as (head, body).
fn console_arms() -> Vec<(String, String)> {
    let body = fn_body(ui_src(), "pub fn handle_pad_key(");
    let start = body
        .find("match ed.screen {")
        .expect("handle_pad_key no longer matches ed.screen");
    let rest = &body[start..];
    let mut arms: Vec<(String, String)> = Vec::new();
    let mut cur: Option<(String, usize)> = None;
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 12 && (trimmed.starts_with("Screen::") || trimmed.starts_with("_ =>")) {
            if let Some((head, at)) = cur.take() {
                arms.push((head, rest[at..offset].to_string()));
            }
            let head = trimmed.split("=>").next().unwrap_or("").trim().to_string();
            cur = Some((head, offset));
        }
        offset += line.len();
    }
    if let Some((head, at)) = cur {
        arms.push((head, rest[at..].to_string()));
    }
    arms
}

/// Does this arm head name that screen? (Screen::Save must not match
/// Screen::SaveQuests.)
fn names_screen(head: &str, screen: &str) -> bool {
    let needle = format!("Screen::{screen}");
    let mut from = 0usize;
    while let Some(i) = head[from..].find(&needle) {
        let at = from + i + needle.len();
        let next = head[at..].chars().next();
        if !matches!(next, Some(c) if c.is_ascii_alphanumeric() || c == '_') {
            return true;
        }
        from = at;
    }
    false
}

/// The indentation of the arms of a "match key {" body.
///
/// Not a constant: the same match sits at 16 spaces in most arms and at 20 in
/// the ones that open a block first (Screen::Char), and a fixed width read the
/// second shape as "no arms at all".
fn arm_pad(body: &str) -> String {
    // The indentation of the first Key:: arm: a match that opens on the arm's
    // own line (Screen::SaveList => match key {) has its arms at the usual 16,
    // and the line's own indent says nothing about them.
    let mut from = 0usize;
    while let Some(i) = body[from..].find("Key::") {
        let at = from + i;
        let line_start = body[..at].rfind('\n').map(|p| p + 1).unwrap_or(0);
        if body[line_start..at].chars().all(|c| c == ' ') && at - line_start >= 12 {
            return " ".repeat(at - line_start);
        }
        from = at + 5;
    }
    String::from("                ")
}

/// Every expression after "=>" for one key inside a "match key {" body.
///
/// A key can have two arms when the first one is guarded (Key::Y if
/// ed.item_menu_open => {} then Key::Y => ...), and the guarded one may be the
/// one that does not run: the audit keeps them all and takes the first that
/// answers.
fn key_exprs(body: &str, key: &str) -> Vec<String> {
    let pad = arm_pad(body);
    let needle = format!("Key::{key}");
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = body[from..].find(&needle) {
        let at = from + i + needle.len();
        let boundary = !matches!(body[at..].chars().next(), Some(c) if c.is_ascii_alphanumeric() || c == '_');
        if boundary {
            let after = &body[at..];
            if let Some(arrow) = after.find("=>") {
                out.push(expr_until_boundary(&after[arrow + 2..], &pad));
            }
        }
        from = at;
    }
    out
}

/// The catch-all of a "match key {" body (other => / _ =>).
fn catch_all_expr(body: &str) -> Option<String> {
    let pad = arm_pad(body);
    for pat in [format!("\n{pad}other =>"), format!("\n{pad}_ =>")] {
        if let Some(i) = body.find(&pat) {
            let start = i + pat.len();
            return Some(expr_until_boundary(&body[start..], &pad));
        }
    }
    None
}

/// One arm's expression, up to the next arm of the same match.
fn expr_until_boundary(s: &str, pad: &str) -> String {
    let mut end = s.len();
    // The brace that closes the match, at the match's own indent. The brace
    // matters: the bare indent is a prefix of every deeper line, so it matched
    // at position 0 and every block arm read as empty.
    let close = format!("\n{}}}", &pad[..pad.len().saturating_sub(4)]);
    for pat in [
        format!("\n{pad}Key::"),
        format!("\n{pad}other"),
        format!("\n{pad}_ =>"),
        close,
        String::from("\n            Screen::"),
    ] {
        if let Some(i) = s.find(&pat) {
            end = end.min(i);
        }
    }
    s[..end].to_string()
}

/// The first ed.<name>( in an expression.
fn first_ed_call(expr: &str) -> Option<String> {
    let mut rest = expr;
    while let Some(i) = rest.find("ed.") {
        let after = &rest[i + 3..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && after[name.len()..].starts_with('(') {
            return Some(name);
        }
        rest = after;
    }
    None
}

fn classify(expr: &str) -> Answer {
    let t = expr.trim();
    let inner = t
        .trim_start_matches('{')
        .trim_end_matches(|c: char| c == '}' || c == ',' || c.is_whitespace());
    if inner.is_empty() {
        return Answer::Nothing;
    }
    if inner.contains("ed.input(") {
        return Answer::Input;
    }
    if let Some(name) = first_ed_call(inner) {
        return Answer::Call(name);
    }
    if inner.contains("apply_action") {
        let name = inner
            .split("UiAction::")
            .nth(1)
            .map(|s| {
                s.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<String>()
            })
            .unwrap_or_default();
        return Answer::Action(name);
    }
    if inner.contains("ed.") {
        // A direct field write (ed.roster_field = ...): the console owns the
        // key, and the field it moves is the state machine's.
        return Answer::Call(String::from("field"));
    }
    if inner.contains("self.") {
        return Answer::Action(String::from("console"));
    }
    Answer::Nothing
}

/// What one arm does with a key.
fn arm_answer(arm: &str, key: &str) -> Answer {
    let Some(pos) = arm.find("=>") else {
        return Answer::Nothing;
    };
    let body = &arm[pos + 2..];
    if !body.contains("match key {") {
        // A one-line arm (Screen::Guild => ed.input(key),): every key is the
        // same answer.
        return classify(body);
    }
    for expr in key_exprs(body, key) {
        let a = classify(&expr);
        if a != Answer::Nothing {
            return a;
        }
    }
    if let Some(expr) = catch_all_expr(body) {
        return classify(&expr);
    }
    Answer::Nothing
}

/// The keys the console answers before any screen arm (the global rules).
fn global_answer(screen: &str, key: &str) -> Option<Answer> {
    match key {
        // hbedit::Editor::shoulder_job is always one of SubTab / Move / Screen /
        // Swallow: a shoulder press is never nothing.
        "L" | "R" => Some(Answer::Call(String::from("shoulder_job"))),
        // [X] is the save key everywhere except the four pickers, where it
        // clears the search (the screen's own arm).
        "X" if !matches!(
            screen,
            "ItemPick" | "SkillPick" | "JewelPick" | "EquipPick"
        ) =>
        {
            Some(Answer::Action(String::from("QuickSave")))
        }
        // [B] on the file list unwinds to クエスト改造 before the screen match.
        "B" if screen == "ArcEntries" => Some(Answer::Action(String::from("BackToQuestEdit"))),
        // [＋] is the quit key on every screen that does not give it a tool
        // (the main loop answers it before the state machine).
        "Plus" => Some(Answer::Action(String::from("Quit"))),
        _ => None,
    }
}

/// Every answer the console has for (screen, key), from the global rules and
/// from each arm that names the screen (a guarded arm may not be the one that
/// runs, so the union is what "the console answers this key" means).
fn console_answers(screen: &str, key: &str) -> Vec<Answer> {
    let mut out = Vec::new();
    if let Some(a) = global_answer(screen, key) {
        out.push(a);
    }
    let mut named = false;
    let mut catch_all: Option<String> = None;
    for (head, body) in console_arms() {
        if head == "_" {
            catch_all = Some(body);
            continue;
        }
        if names_screen(&head, screen) {
            named = true;
            let a = arm_answer(&body, key);
            if a != Answer::Nothing {
                out.push(a);
            }
        }
    }
    // A screen with no arm of its own falls through to the match's catch-all
    // (the last arm, "_ => ed.input(key)").
    if !named {
        if let Some(body) = catch_all {
            let a = classify(&body);
            if a != Answer::Nothing {
                out.push(a);
            }
        }
    }
    out
}

fn console_answers_key(screen: &str, key: &str) -> bool {
    !console_answers(screen, key).is_empty()
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

/// A talisman of one kind (the byte at +0x02), at that kind's rarity.
fn kind_record(kind: u8) -> [u8; charm::CHARM_RECORD] {
    let mut r = ten_record();
    r[0x02] = kind;
    r[0x12] = charm::kind_rarity(kind).unwrap_or(100);
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

fn save_with_one_kind(kind: u8) -> Save {
    let mut save = blank_save();
    {
        let b = save.equipment_box_mut(0).expect("the box exists");
        b[0..charm::CHARM_RECORD].copy_from_slice(&kind_record(kind));
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
    ed.save_path = Some(String::from("memory:review-20"));
    ed.slot = 0;
    ed.screen = screen;
    ed
}

/// The editor on a reward table, the way the file screen opens it.
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

/// Everything a press can move, as one string.
///
/// Every cursor and every mode flag, because a press that moves one of them is
/// a press that did something - and the point of the audit is to tell that
/// apart from a press that landed on nothing.
fn fingerprint(ed: &Editor) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ed.screen,
        ed.cursor,
        ed.message,
        ed.dirty,
        ed.field,
        ed.step,
        ed.slot,
        ed.entry_field,
        ed.entry_row_cursor,
        ed.hunt_field,
    ));
    out.push_str(&format!(
        "|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ed.roster_field,
        ed.show_empty,
        ed.item_pick_mode,
        ed.charm_edit.map(|c| c.field).unwrap_or(99),
        ed.equip_edit.map(|e| e.field).unwrap_or(99),
        ed.set_edit.map(|s| s.field).unwrap_or(99),
        ed.quest_edit_tab,
        ed.guild_tab,
        ed.findings.len(),
        ed.undo_depth(),
    ));
    out.push_str(&format!(
        "|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ed.item_pick_cursor,
        ed.skill_pick_cursor,
        ed.jewel_pick_cursor,
        ed.equip_pick_cursor,
        ed.equip_cursor,
        ed.palico_cursor,
        ed.map_cursor,
        ed.rev_cursor,
        ed.csv_cursor,
        ed.option_cursor,
    ));
    out.push_str(&format!(
        "|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ed.save_quest_cursor,
        ed.quest_cat_cursor[0],
        ed.quest_edit_cursor,
        ed.quest_edit_field,
        ed.quest_reward_field,
        ed.progress_tab,
        ed.award_cursor,
        ed.request_cursor,
        ed.quest_cursor,
        ed.qe_open,
    ));
    out.push_str(&format!(
        "|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ed.qe_item_cursor,
        ed.item_menu_open,
        ed.item_menu_cursor,
        ed.mib_cursor,
        ed.gmd_cursor,
        ed.check_cursor,
        ed.hunt_cursor,
        ed.charm_cursor,
        ed.charm_tab,
        ed.guild_field,
    ));
    out.push_str(&format!(
        "|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ed.guild_count_cursor,
        ed.guild_wpn_row,
        ed.arena_row,
        ed.arena_rank,
        ed.arena_edit,
        ed.guild_wpn_edit,
        ed.palico_lang,
        ed.item_pick_stage,
        ed.item_pick_add_count,
        ed.convert_armed,
    ));
    out.push_str(&format!(
        "|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ed.equip_kind_armed.is_some(),
        ed.charm_delete_armed.is_some(),
        ed.save_quest_armed.is_some(),
        ed.map_edit.is_some(),
        ed.rev_armed.is_some(),
        ed.csv_armed.is_some(),
        ed.item_set_armed.is_some(),
        ed.set_clear_armed.is_some(),
        ed.palico_equip.is_some(),
        ed.palico_skill.is_some(),
    ));
    // The two forms' values: a press that steps a row changes the draft, not
    // the form's cursor.
    out.push_str(&match ed.charm_edit {
        Some(c) => format!(
            "|{}|{}|{}|{}|{}|{}|{}|{:?}",
            c.kind,
            c.grade.rarity(),
            c.slots,
            c.skill1,
            c.points1,
            c.skill2,
            c.points2,
            c.decorations
        ),
        None => String::from("|form"),
    });
    out.push_str(&match ed.equip_edit {
        Some(e) => format!(
            "|{}|{}|{}|{}|{}|{:?}",
            e.kind, e.id, e.level, e.transmog_id, e.transmog_level, e.decorations
        ),
        None => String::from("|form"),
    });
    out.push_str(&format!("|{}|{}", ed.quit, ed.quit_armed));
    out
}
// ---------------------------------------------------------------------------
// The screen list and the fixtures
// ---------------------------------------------------------------------------

const ALL_SCREENS: &[Screen] = &[
    Screen::SaveList,
    Screen::Slots,
    Screen::Char,
    Screen::Items,
    Screen::ArcList,
    Screen::SaveQuests,
    Screen::ArcEntries,
    Screen::Mib,
    Screen::Gmd,
    Screen::Check,
    Screen::Charms,
    Screen::CharmEdit,
    Screen::EntryView,
    Screen::SkillPick,
    Screen::Convert,
    Screen::Equip,
    Screen::EquipEdit,
    Screen::JewelPick,
    Screen::EquipPick,
    Screen::QuestEquip,
    Screen::Hunts,
    Screen::Guild,
    Screen::Roster,
    Screen::Sets,
    Screen::ItemPick,
    Screen::Palico,
    Screen::Options,
    Screen::Csv,
    Screen::Progress,
    Screen::QuestEdit,
    Screen::SaveMap,
    Screen::Review,
];

/// The name the console's table and dispatch use for a screen.
fn screen_name(screen: Screen) -> &'static str {
    match screen {
        Screen::SaveList => "SaveList",
        Screen::Slots => "Slots",
        Screen::Char => "Char",
        Screen::Items => "Items",
        Screen::ArcList => "ArcList",
        Screen::SaveQuests => "SaveQuests",
        Screen::ArcEntries => "ArcEntries",
        Screen::Mib => "Mib",
        Screen::Gmd => "Gmd",
        Screen::Check => "Check",
        Screen::Charms => "Charms",
        Screen::CharmEdit => "CharmEdit",
        Screen::EntryView => "EntryView",
        Screen::SkillPick => "SkillPick",
        Screen::Convert => "Convert",
        Screen::Equip => "Equip",
        Screen::EquipEdit => "EquipEdit",
        Screen::JewelPick => "JewelPick",
        Screen::EquipPick => "EquipPick",
        Screen::QuestEquip => "QuestEquip",
        Screen::Hunts => "Hunts",
        Screen::Guild => "Guild",
        Screen::Roster => "Roster",
        Screen::Sets => "Sets",
        Screen::ItemPick => "ItemPick",
        Screen::Palico => "Palico",
        Screen::Options => "Options",
        Screen::Csv => "Csv",
        Screen::Progress => "Progress",
        Screen::QuestEdit => "QuestEdit",
        Screen::SaveMap => "SaveMap",
        Screen::Review => "Review",
    }
}

fn screen_by_name(name: &str) -> Option<Screen> {
    ALL_SCREENS
        .iter()
        .copied()
        .find(|s| screen_name(*s) == name)
}

/// An editor on the screen, with the state that screen needs to answer a key.
fn fixture(screen: Screen) -> Option<Editor> {
    let mut ed = match screen {
        Screen::SaveList => {
            let mut ed = Editor::new();
            ed.set_save_files(vec![String::from("switch/XXSE/saves/a.sav")]);
            ed.screen = Screen::SaveList;
            ed
        }
        Screen::Convert => editor_with(blank_save(), Screen::Convert),
        Screen::Slots => editor_with(blank_save(), Screen::Slots),
        Screen::Char => editor_with(blank_save(), Screen::Char),
        Screen::Items => editor_with(blank_save(), Screen::Items),
        Screen::ItemPick => {
            let mut ed = editor_with(blank_save(), Screen::Items);
            // The box lists nothing until 空きも表示 is on, and [A] on an empty
            // list says so instead of opening the picker.
            ed.show_empty = true;
            ed.input(Key::A);
            // Off the top row, so the ten-row step and the clear both have
            // something to move.
            ed.item_pick_cursor = 5;
            ed
        }
        Screen::Charms => {
            let mut ed = editor_with(save_with_charms(3), Screen::Charms);
            ed.open_charms();
            ed
        }
        Screen::CharmEdit => {
            let mut ed = editor_with(save_with_charms(3), Screen::Charms);
            ed.open_charms();
            ed.charm_a_press();
            ed
        }
        Screen::SkillPick => {
            let mut ed = editor_with(save_with_charms(3), Screen::Charms);
            ed.open_charms();
            ed.charm_a_press();
            if let Some(f) = ed.charm_edit.as_mut() {
                f.field = 3;
            }
            ed.input(Key::A);
            ed.skill_pick_cursor = 5;
            ed
        }
        Screen::JewelPick => {
            let mut ed = editor_with(save_with_charms(3), Screen::Charms);
            ed.open_charms();
            ed.charm_a_press();
            if let Some(f) = ed.charm_edit.as_mut() {
                f.field = 7;
            }
            ed.input(Key::A);
            ed.jewel_pick_cursor = 5;
            ed
        }
        Screen::Equip => {
            let mut ed = editor_with(blank_save(), Screen::Equip);
            ed.open_equip_box();
            ed
        }
        Screen::EquipEdit => {
            let mut ed = editor_with(blank_save(), Screen::Equip);
            ed.open_equip_box();
            ed.input(Key::Plus);
            ed
        }
        Screen::EquipPick => {
            let mut ed = editor_with(blank_save(), Screen::Equip);
            ed.open_equip_box();
            ed.input(Key::Plus);
            // The new form's ID row (a record that does not exist yet has no
            // 装備枠 row to point at).
            if let Some(f) = ed.equip_edit.as_mut() {
                f.field = 1;
            }
            ed.input(Key::Y);
            ed.equip_pick_cursor = 5;
            ed
        }
        Screen::Sets => {
            let mut ed = editor_with(blank_save(), Screen::Sets);
            ed.screen = Screen::Sets;
            ed
        }
        Screen::Hunts => {
            let mut ed = editor_with(blank_save(), Screen::Hunts);
            ed.open_hunts();
            // The list is 記録ありのみ by default, and a blank save has no
            // records: every row would be missing and no key could move.
            ed.toggle_hunt_show_all();
            ed
        }
        Screen::Guild => editor_with(blank_save(), Screen::Guild),
        Screen::Roster => {
            let mut ed = editor_with(blank_save(), Screen::Roster);
            ed.open_roster();
            ed
        }
        Screen::Palico => {
            let mut ed = editor_with(blank_save(), Screen::Palico);
            ed.screen = Screen::Palico;
            ed
        }
        Screen::Progress => {
            let mut ed = editor_with(blank_save(), Screen::Progress);
            ed.open_progress();
            ed
        }
        Screen::Options => editor_with(blank_save(), Screen::Options),
        Screen::Csv => {
            let mut ed = editor_with(blank_save(), Screen::Csv);
            ed.open_csv();
            ed
        }
        Screen::SaveMap => {
            let mut ed = editor_with(blank_save(), Screen::SaveMap);
            ed.open_save_map(Screen::Slots);
            ed
        }
        Screen::Review => {
            // The list is the difference between the open save and the bytes
            // the card holds: a blank reference and a save with one talisman
            // give it a row to move on.
            let mut changed = blank_save();
            changed.set_hunt_count(0, 0, 5);
            let mut ed = editor_with(changed, Screen::Review);
            ed.set_review_reference(blank_save().body());
            ed.open_review();
            ed
        }
        Screen::SaveQuests => {
            let mut ed = editor_with(save_with_one_quest(), Screen::SaveQuests);
            ed.set_quest_tab(0);
            ed.refresh_quest_titles();
            ed
        }
        Screen::QuestEquip => {
            let mut ed = editor_with(save_with_one_quest(), Screen::QuestEquip);
            ed.screen = Screen::QuestEquip;
            ed
        }
        Screen::ArcList => {
            let mut ed = editor_with(blank_save(), Screen::ArcList);
            ed.set_arc_files(vec![String::from("switch/XXSE/mhxx/quest/arc/a.arc")]);
            ed
        }
        Screen::ArcEntries => {
            let mut ed = editor_with(blank_save(), Screen::ArcEntries);
            ed.open_arc_bytes("a.arc", &quest_arc_bytes());
            ed
        }
        Screen::Mib => {
            let mut ed = editor_with(blank_save(), Screen::ArcEntries);
            ed.open_arc_bytes("a.arc", &quest_arc_bytes());
            ed.entry = ed
                .arc
                .as_ref()
                .and_then(|a| a.entries.iter().position(|e| e.name.contains("_mib")))
                .expect("the .mib entry");
            ed.cursor = ed.entry;
            ed.input(Key::A);
            ed
        }
        Screen::Gmd => {
            let mut ed = editor_with(blank_save(), Screen::ArcEntries);
            ed.open_arc_bytes("a.arc", &quest_arc_bytes());
            ed.entry = ed
                .arc
                .as_ref()
                .and_then(|a| a.entries.iter().position(|e| e.name.contains("questData")))
                .expect("the .gmd entry");
            // The list's [A] opens the row under the *cursor*.
            ed.cursor = ed.entry;
            ed.input(Key::A);
            ed
        }
        Screen::EntryView => editor_on_the_reward_table(),
        Screen::Check => {
            let mut ed = editor_with(blank_save(), Screen::ArcEntries);
            ed.open_arc_bytes("a.arc", &quest_arc_bytes());
            ed.run_check();
            ed
        }
        Screen::QuestEdit => {
            let mut ed = editor_with(save_with_one_quest(), Screen::Slots);
            ed.open_quest_from_save(QUEST_ID);
            ed.enter_quest_edit_from(Screen::SaveQuests);
            ed
        }
    };
    // The fixture is a settled state: the presses that built it left their
    // requests behind, and a test that compares "before" with "after" must not
    // count the same request twice.
    let _ = ed.take_request();
    let _ = ed.take_text_request();
    let _ = ed.take_write();
    Some(ed)
}

/// Everything a press can move, including the requests it leaves behind.
fn observe(ed: &mut Editor) -> String {
    let request = ed.take_request();
    let text = ed.take_text_request().is_some();
    let write = ed.take_write().is_some();
    format!(
        "{}|{:?}|{}|{}",
        fingerprint(ed),
        request,
        text,
        write
    )
}

fn key_of(name: &str) -> Key {
    match name {
        "A" => Key::A,
        "B" => Key::B,
        "X" => Key::X,
        "Y" => Key::Y,
        "L" => Key::L,
        "R" => Key::R,
        "Z" => Key::Z,
        "ZL" => Key::L,
        "Minus" => Key::Minus,
        "Plus" => Key::Plus,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Left" => Key::Left,
        "Right" => Key::Right,
        other => panic!("Key::{other} is not in key_of - add it"),
    }
}

/// Drive the answer the console's dispatch gives. Returns false when the
/// answer is one this audit cannot run (a console-only action).
fn run_answer(ed: &mut Editor, key: &str, answer: &Answer) -> bool {
    match answer {
        Answer::Input => {
            ed.input(key_of(key));
            true
        }
        Answer::Call(name) => match name.as_str() {
            "charm_a_press" => {
                ed.charm_a_press();
                true
            }
            "request_hunt_text" => {
                ed.request_hunt_text();
                true
            }
            "quest_activate" => {
                ed.quest_activate();
                true
            }
            "run_check" => {
                ed.run_check();
                true
            }
            // The arm reads the screen's own step table and advances it; the
            // table is the core's, so the audit drives the same value.
            "step_table" => {
                let n = ed.step_table().0.len().max(1);
                ed.step = (ed.step + 1) % n;
                true
            }
            "open_item_set_editor" => {
                ed.open_item_set_editor(0);
                true
            }
            // The console writes an Editor field itself (roster_field and the
            // like) or asks the shoulder rule: both are console-side, and the
            // state machine has no method to call.
            "field" | "shoulder_job" => false,
            other => panic!(
                "the console calls ed.{other}() and this audit does not know how to drive it - add it to run_answer"
            ),
        },
        Answer::Action(name) => match name.as_str() {
            "QuickSave" => {
                ed.quick_save();
                true
            }
            "Quit" => {
                ed.press_quit();
                true
            }
            "Rescan" => {
                ed.request_rescan();
                true
            }
            "BackToQuestEdit" => {
                ed.screen = Screen::QuestEdit;
                true
            }
            // The console's own actions, each driven through the core method
            // the console's arm calls (ui.rs apply_action).
            "SelectSlot" => {
                ed.switch_slot(1);
                true
            }
            "OpenSlot" => {
                ed.open_slot(ed.slot);
                true
            }
            "CharMaxAll" => {
                ed.char_max_all_press();
                true
            }
            "InstallAllPalicoes" => {
                ed.install_all_palicoes_press();
                true
            }
            "TogglePalicoLang" => {
                ed.palico_lang = 1 - ed.palico_lang;
                true
            }
            "ToggleHuntShowAll" => {
                ed.toggle_hunt_show_all();
                true
            }
            "ClearEquipFilter" => {
                ed.apply_equip_filter(String::new());
                true
            }
            "ToggleItemEmpty" => {
                ed.toggle_item_show_empty();
                true
            }
            "ProgressSeen" => {
                ed.progress_seen_press();
                true
            }
            "FilterQuestItems" => {
                ed.request_quest_item_filter();
                true
            }
            "FilterProgress" => {
                ed.request_progress_filter();
                true
            }
            "FilterItems" => {
                ed.request_item_box_filter();
                true
            }
            "FilterCharms" => {
                ed.request_charm_filter();
                true
            }
            "OpenItemMenu" => {
                ed.open_item_menu();
                true
            }
            // Console-only: the section jump, the 詳細 pill, the SD write and
            // the two-press installs are the console's own state.
            "console" | "OpenDetail" | "ExtractOfficialArcs" | "GotoSection" | "SelectSubTab"
            | "CycleSubTab" | "InstallArcRow" | "InstallDlcCategory" => false,
            other => panic!(
                "the console runs UiAction::{other} and this audit does not know how to drive it - add it to run_answer"
            ),
        },
        Answer::Nothing => false,
    }
}

/// The [KEY] tokens a footer prints, as chip text.
fn footer_key_tokens(hint: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = hint;
    while let Some(i) = rest.find('[') {
        let after = &rest[i + 1..];
        let Some(j) = after.find(']') else { break };
        let token = &after[..j];
        if !chip_keys(token).is_empty() {
            let token = token.to_string();
            if !out.contains(&token) {
                out.push(token);
            }
        }
        rest = &after[j + 1..];
    }
    out
}

// ---------------------------------------------------------------------------
// #1. The table, the dispatch and the footer
// ---------------------------------------------------------------------------

/// Every chip the text build's table names is a key the console's dispatch
/// really answers on that screen.
///
/// This is the finding the review opened with: the table is the ASCII
/// fallback's, and the machine's dispatch is handle_pad_key. The audit reads
/// the dispatch, so a chip that names a key the console ignores fails here.
#[test]
fn the_key_table_only_names_keys_the_console_answers() {
    let mut checked = 0usize;
    for (screen, chips) in switch_hint_arms() {
        assert!(!chips.is_empty(), "{screen} has no chips");
        assert!(
            screen_by_name(&screen).is_some(),
            "{screen} is not a Screen variant"
        );
        for (chip, label) in chips {
            let keys = chip_keys(&chip);
            assert!(
                !keys.is_empty(),
                "{screen}: the chip [{chip}] {label} is not a key this audit knows - add it to chip_keys()"
            );
            for key in keys {
                assert!(
                    console_answers_key(&screen, key),
                    "{screen}: the table names [{chip}] {label}, but handle_pad_key answers nothing for Key::{key}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "the audit only checked {checked} chips");
}

/// The footer's key guide names only keys the dispatch answers.
///
/// The console draws hbedit::Editor::footer_hint verbatim, so every key token
/// in it is a promise. The audit reads both sides: the string the footer
/// prints, and the arms of handle_pad_key.
#[test]
fn the_footer_names_only_keys_the_console_answers() {
    for screen in ALL_SCREENS {
        let Some(mut ed) = fixture(*screen) else { continue };
        let hint = ed.footer_hint();
        let mut seen = 0usize;
        for token in footer_key_tokens(&hint) {
            for key in chip_keys(&token) {
                assert!(
                    console_answers_key(screen_name(*screen), key),
                    "{:?}: the footer says [{token}] ({hint}), but handle_pad_key answers nothing for Key::{key}",
                    screen
                );
                seen += 1;
            }
        }
        assert!(seen > 0, "{:?}: the footer names no key at all ({hint})", screen);
        // The footer is drawn in a fixed strip; a line that does not fit is
        // clipped at its left end, which is where the way out lives.
        assert!(
            ed.footer_hint_width() <= mhnx_core::hbedit::FOOTER_HINT_ROOM,
            "{:?}: the footer is {}px of {} ({hint})",
            screen,
            ed.footer_hint_width(),
            mhnx_core::hbedit::FOOTER_HINT_ROOM
        );
    }
}

/// A chip's key, pressed the way the console presses it, changes something.
///
/// The table's words are claims about the dispatch, and the dispatch's answers
/// are claims about the state machine. This drives the answer and fails when a
/// chip names a key that lands on nothing.
#[test]
fn every_chip_key_changes_something_on_the_screen_it_names() {
    // The screens whose press needs a card this test cannot build. Each one
    // says why, so the list cannot quietly grow.
    const NO_STATE: &[(&str, &str)] = &[
        (
            "QuestEquip",
            "the 45 quest-equipment entries live in the card's DLC section and the synthetic save has none",
        ),
        (
            "SkillPick",
            "the picker writes into the form that opened it, and [A] on a row the form does not carry writes nothing",
        ),
        (
            "Roster",
            "the roster of a synthetic save is empty (a palico record is a card-sized structure), so no row can move",
        ),
        (
            "Review",
            "the change list walks the named values of a card; a synthetic save reports 0 items, so no row can move",
        ),
    ];
    let mut driven = 0usize;
    for (screen, chips) in switch_hint_arms() {
        if NO_STATE.iter().any(|(name, _)| *name == screen) {
            continue;
        }
        let Some(variant) = screen_by_name(&screen) else {
            panic!("{screen} is not a Screen variant")
        };
        for (chip, label) in chips {
            for key in chip_keys(&chip) {
                // [L] / [R] are the shoulder rule (hbedit::Editor::
                // shoulder_job): the console turns the answer into a tab
                // move, a ten-row step or the screen's own key, and the tab
                // machinery is the console's. The rule itself has its own
                // tests (tests/console_a_fixes.rs), and the chip check above
                // already proves the dispatch answers the key.
                if matches!(key, "L" | "R") {
                    continue;
                }
                // Some keys belong to the console itself: it writes an
                // Editor field (the hunt column, the roster field), moves its
                // own tab, or asks the shoulder rule. Those answers are not
                // the state machine's, so the audit cannot drive them - the
                // chip check above is what pins that the dispatch answers
                // them at all.
                if console_answers(&screen, key).iter().any(console_owned) {
                    continue;
                }
                // A direction key saturates at the end of its list, so the
                // audit tries the other direction before it calls the key
                // dead: one of the two must move the cursor.
                let mut tried = vec![key];
                if let Some(other) = opposite(key) {
                    tried.push(other);
                }
                let mut moved = false;
                let mut drivable = false;
                for k in tried {
                    // A screen can have two arms (a form and the list under
                    // it) and the audit cannot evaluate their guards, so every
                    // answer is tried: one of them is the state the fixture is
                    // in.
                    for answer in console_answers(&screen, k) {
                        let Some(mut ed) = fixture(variant) else { continue };
                        let before = observe(&mut ed);
                        if !run_answer(&mut ed, k, &answer) {
                            // A console-side answer (a field this layer
                            // writes, a tab move, the SD write): the dispatch
                            // really answers the key, and the chip check above
                            // pins that. What the audit cannot drive, it does
                            // not pretend to.
                            continue;
                        }
                        drivable = true;
                        let after = observe(&mut ed);
                        if before != after {
                            moved = true;
                            break;
                        }
                    }
                    if moved {
                        break;
                    }
                }
                if drivable {
                    assert!(
                        moved,
                        "{screen}: [{chip}] {label} (Key::{key}) is answered by handle_pad_key and moves nothing"
                    );
                    driven += 1;
                }
            }
        }
    }
    assert!(driven > 80, "the audit only drove {driven} presses");
}

// ---------------------------------------------------------------------------
// The text build's table, read out of its source
// ---------------------------------------------------------------------------

/// (screen name, [(key, label)]) for every screen's key table.
///
/// The table is the state machine's now (`hbedit::Editor::key_facts`): the text
/// build's actions() renders it, the canvas footer renders it, and the guide
/// prints its first items. The audit reads that one table instead of parsing a
/// second copy out of the console's source.
fn switch_hint_arms() -> Vec<(String, Vec<(String, String)>)> {
    ALL_SCREENS
        .iter()
        .filter_map(|&screen| {
            let mut ed = fixture(screen)?;
            ed.screen = screen;
            let chips = ed
                .key_facts()
                .into_iter()
                .filter(|f| !f.key.is_empty())
                .map(|f| (f.key.to_string(), f.label))
                .collect();
            Some((screen_name(screen).to_string(), chips))
        })
        .collect()
}

/// A chip's key ("L/R", "◀/▶", "ZR", "-", "+") as the Key names the console
/// dispatch uses.
fn chip_keys(chip: &str) -> Vec<&'static str> {
    match chip {
        "A" => vec!["A"],
        "B" => vec!["B"],
        "X" => vec!["X"],
        "Y" => vec!["Y"],
        "L" => vec!["L"],
        "R" => vec!["R"],
        "L/R" => vec!["L", "R"],
        "ZR" => vec!["Z"],
        "ZL" => vec!["ZL"],
        "-" => vec!["Minus"],
        "+" | "＋" => vec!["Plus"],
        "◀/▶" | "</>" => vec!["Left", "Right"],
        "▲/▼" => vec!["Up", "Down"],
        "◀" => vec!["Left"],
        "▶" => vec!["Right"],
        "▲" => vec!["Up"],
        "▼" => vec!["Down"],
        _ => Vec::new(),
    }
}

/// The direction key that moves the other way.
fn opposite(key: &str) -> Option<&'static str> {
    match key {
        "Left" => Some("Right"),
        "Right" => Some("Left"),
        "Up" => Some("Down"),
        "Down" => Some("Up"),
        _ => None,
    }
}

/// An answer the console owns outright: a field it writes, its own tab, or the
/// shoulder rule. The state machine has no method to call for those.
fn console_owned(a: &Answer) -> bool {
    match a {
        Answer::Action(n) => n == "console",
        Answer::Call(n) => n == "field" || n == "shoulder_job",
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// #2. An empty save list can still reload the card
// ---------------------------------------------------------------------------

/// The save list's only button was 保存, and with no save open it wrote
/// nothing at all. The reload is the screen's own key and the only thing that
/// can change the list, and it had no hit area.
#[test]
fn an_empty_save_list_still_offers_the_reload() {
    // The state machine: a save request with nothing to write says why.
    let mut ed = Editor::new();
    ed.screen = Screen::SaveList;
    ed.request_write_save();
    assert!(
        ed.message.contains("書き込むセーブがありません"),
        "the save key must not be silent: {}",
        ed.message
    );
    assert!(ed.take_write().is_none(), "nothing may be written");

    // The console: the deck draws 再読込 always and the save pill only when a
    // save is open.
    let deck = fn_body(ui_src(), "fn draw_studio_inspector(");
    let arm = deck
        .find("Screen::SaveList =>")
        .expect("the save list's deck arm");
    let arm = &deck[arm..];
    let arm = &arm[..arm.find("\n            Screen::").unwrap_or(arm.len())];
    assert!(
        arm.contains("UiAction::Rescan"),
        "the reload must have a button: {arm}"
    );
    assert!(
        arm.contains("ed.save.is_some()"),
        "the save pill must be drawn only when there is a save: {arm}"
    );
    assert!(
        arm.contains("セーブが見つかりません"),
        "an empty list must say where saves come from: {arm}"
    );
}

// ---------------------------------------------------------------------------
// #3. An empty talisman list still draws its [A] button
// ---------------------------------------------------------------------------

/// The word 作成 was computed and printed by the footer, and the panel drew no
/// button at all: the [A] button lived under a row.
#[test]
fn an_empty_talisman_list_still_draws_its_a_button() {
    let mut ed = editor_with(save_with_charms(0), Screen::Charms);
    ed.open_charms();
    assert_eq!(ed.empty_a_label(), Some("作成"));
    // It is the press the button runs.
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);

    // A screen whose [A] needs a row says so by drawing no button.
    let mut ed = editor_with(blank_save(), Screen::Hunts);
    assert_eq!(ed.empty_a_label(), None);

    // The console draws it in the empty branch, with the state machine's word.
    let deck = fn_body(ui_src(), "fn draw_studio_inspector(");
    let empty = deck
        .find("
        if empty {")
        .expect("the empty branch");
    let after = &deck[empty..];
    let end = after.find("} else {").expect("the else branch");
    assert!(
        after[..end].contains("empty_a_label()"),
        "the empty branch must draw the [A] button"
    );
}

// ---------------------------------------------------------------------------
// #4. Every empty list keeps its way out on the deck
// ---------------------------------------------------------------------------

/// The four screens the review named had no button for the key that leaves
/// them, so a list with no rows left 保存 as the only thing a finger could
/// press.
#[test]
fn an_empty_list_keeps_its_way_out_on_the_deck() {
    let deck = fn_body(ui_src(), "fn draw_studio_inspector(");
    let arm_of = |needle: &str| -> &str {
        let at = deck
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} is not on the deck"));
        let rest = &deck[at..];
        &rest[..rest.find("\n            Screen::").unwrap_or(rest.len())]
    };
    assert!(arm_of("Screen::SaveList =>").contains("UiAction::Rescan"));
    assert!(arm_of("Screen::Guild =>").contains("UiAction::Key(Key::B)"));
    assert!(arm_of("Screen::Check =>").contains("UiAction::Key(Key::B)"));
    // The button prints the destination's name and no key: the footer is the
    // one surface that names keys (the sixth review's leftovers), and the word
    // is the state machine's (Editor::b_word).
    let quests = arm_of("Screen::SaveQuests | Screen::ArcList =>");
    assert!(
        quests.contains("ed.b_word()") && quests.contains("UiAction::Key(Key::B)"),
        "both quest lists need their way out: {quests}"
    );
    // Only the code lines are read: the comments in this arm explain the keys
    // (they say why each button exists).
    let code: String = quests
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("[B] "),
        "the deck's buttons must not name keys: {code}"
    );
    // The keys those buttons send are the keys the dispatch answers.
    for (screen, key) in [
        ("SaveList", "Y"),
        ("Guild", "B"),
        ("Check", "B"),
        ("SaveQuests", "B"),
        ("ArcList", "B"),
    ] {
        assert!(console_answers_key(screen, key), "{screen}/{key}");
    }
}

// ---------------------------------------------------------------------------
// #5. A chance step leaves the cell on the chance
// ---------------------------------------------------------------------------

/// quest_adjust wrote the cell cursor from a bool, so it said 個数 (1) after a
/// chance step and the next [A] wrote a count.
#[test]
fn stepping_the_chance_leaves_the_cell_on_the_chance() {
    let mut ed = editor_on_the_reward_table();
    ed.quest_edit_tab = QUEST_TAB_REWARDS;
    ed.quest_edit_cursor = 0;
    ed.quest_reward_field = QUEST_REWARD_CHANCE;
    ed.entry_field = 0;
    let count = reward_count(&ed);
    let chance = reward_chance(&ed);
    assert!(ed.quest_adjust(1), "the chance must step");
    assert_eq!(reward_chance(&ed), chance + 1);
    assert_eq!(
        ed.entry_field, 2,
        "the cell cursor must be 確率% (2), not 個数 (1)"
    );
    assert_eq!(ed.entry_field_label(), "確率%");
    // The next [A] raises the chance, not the count.
    ed.input(Key::A);
    assert_eq!(reward_count(&ed), count, "[A] must not write a count");
    assert!(reward_chance(&ed) > chance + 1);
}

// ---------------------------------------------------------------------------
// #6. A refused talisman write leaves no undo step
// ---------------------------------------------------------------------------

/// push_undo ran before the write, so a slot that refused the record still left
/// a step behind that restored a save nothing had changed.
#[test]
fn a_refused_charm_write_leaves_no_undo_step() {
    // Record 0 holds a weapon and the box holds no talisman at all, so there is
    // nothing to copy a shape from: the write is refused.
    let mut save = blank_save();
    {
        let b = save.equipment_box_mut(0).expect("the box");
        b[0] = 0x47;
        b[1] = 0x01;
        b[2] = 0x1A;
    }
    let mut ed = editor_with(save, Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    // Point the form at the weapon.
    if let Some(f) = ed.charm_edit.as_mut() {
        f.index = 0;
    }
    let depth = ed.undo_depth();
    assert!(!ed.commit_charm_draft_here(), "the write must be refused");
    assert_eq!(
        ed.undo_depth(),
        depth,
        "a refused write must not leave an undo step"
    );

    // A write that lands is still one step.
    let mut ed = editor_with(save_with_charms(1), Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    let depth = ed.undo_depth();
    assert!(ed.commit_charm_draft_here());
    assert_eq!(ed.undo_depth(), depth + 1);
}

// ---------------------------------------------------------------------------
// #7. A full talisman box says it is full
// ---------------------------------------------------------------------------

/// first_writable falls back to the first talisman, so [＋] on a full box aimed
/// the form at a record the user already had.
#[test]
fn a_full_talisman_box_has_nowhere_to_put_a_new_one() {
    let mut ed = editor_with(save_with_charms(charm::CHARM_COUNT), Screen::Charms);
    ed.open_charms();
    let before = ed.charms().len();
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::Charms, "no form may open on a full box");
    assert!(ed.charm_edit.is_none());
    assert!(
        ed.message.contains("空き枠がありません"),
        "the press must say why: {}",
        ed.message
    );
    assert_eq!(ed.charms().len(), before);

    // One free record is enough.
    let mut ed = editor_with(save_with_charms(charm::CHARM_COUNT - 1), Screen::Charms);
    ed.open_charms();
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert_eq!(ed.charm_edit.map(|c| c.index), Some(charm::CHARM_COUNT - 1));
}

// ---------------------------------------------------------------------------
// #8. The save key says a new talisman is not written yet
// ---------------------------------------------------------------------------

/// A new talisman starts clean (nothing changed), so [X] found nothing to
/// commit and said 保存しました about a box the record was not in.
#[test]
fn the_save_key_says_a_new_talisman_is_not_written_yet() {
    let mut ed = editor_with(save_with_charms(1), Screen::Charms);
    ed.open_charms();
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(!ed.charm_draft_dirty(), "a new talisman starts clean");
    assert!(!ed.charm_draft_in_box(), "and it is not in the box yet");
    ed.quick_save();
    assert!(
        ed.message.contains("まだ書き込まれていません"),
        "the save key must not claim the talisman is in the save: {}",
        ed.message
    );
    // The file still goes out (the user's other edits are in it), and the line
    // says the record on screen is not.
    assert!(
        ed.take_write().is_some(),
        "the save itself still goes out"
    );

    // Once [A] writes it, the save key has nothing to warn about.
    assert!(ed.commit_charm_draft_here());
    assert!(ed.charm_draft_in_box());
    ed.quick_save();
    assert!(
        !ed.message.contains("まだ書き込まれていません"),
        "{}",
        ed.message
    );
    assert!(ed.take_write().is_some());
}

// ---------------------------------------------------------------------------
// #9. A new talisman starts from the record it copied
// ---------------------------------------------------------------------------

/// The grade, the kind and the sockets were read from the write target (an
/// empty record), so every new talisman opened on 天 whatever the box held -
/// while the comment said it started on the seed's grade.
#[test]
fn a_new_talisman_starts_from_the_record_it_copied() {
    for kind in [7u8, 6, 3] {
        let mut ed = editor_with(save_with_one_kind(kind), Screen::Charms);
        ed.open_charms();
        ed.input(Key::Plus);
        let form = ed.charm_edit.expect("the form");
        assert_eq!(
            form.kind, kind,
            "a new talisman must keep the kind it was seeded from"
        );
        assert_eq!(
            form.grade.rarity(),
            charm::kind_rarity(kind).expect("the kind has a rarity"),
            "and the grade that goes with it"
        );
        assert_ne!(
            form.grade.rarity(),
            100,
            "it must not open on 天 (rarity 100) whatever the box holds"
        );
        assert_eq!(form.index, 1, "and it must be written to the free record");
    }
}

// ---------------------------------------------------------------------------
// #10. The guild card's guide names one key for the four views
// ---------------------------------------------------------------------------

/// The guide said "[L/R] 4ビュー" while the footer said "[R] 数値入力": one key,
/// two jobs, on one screen.
#[test]
fn the_guild_guide_names_one_key_for_the_four_views() {
    let mut ed = editor_with(blank_save(), Screen::Guild);
    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_CARD);
    let footer = ed.footer_hint();
    // 数値, not 数値入力: the card's line carries seven items plus the arrow
    // pair the review asked for, and the 763px strip decides the word. The
    // inspector's pill borrows it (Editor::key_label_or), so the two agree.
    assert!(footer.contains("[R] 数値"), "{footer}");
    let src = ui_src();
    assert!(
        src.contains("[L] 4ビュー"),
        "the guide must name the key that really walks the views"
    );
    assert!(
        !src.contains("[L/R] 4ビュー"),
        "the guide claims [R] walks the views while the footer says it types a number"
    );
    // The guide prints the same table's first items as the footer, so it names
    // the view walk the same way (Y 回数/武器) and never the shoulder pair.
    let guide = ed.screen_guide();
    for fact in ed.guide_facts() {
        assert!(guide.contains(&fact.render()), "{guide}");
    }
    assert!(!guide.contains("[L/R]"), "{guide}");
    assert!(!guide.contains("[L]"), "{guide}");
}

// ---------------------------------------------------------------------------
// #11. The step pills name no key where the key is taken
// ---------------------------------------------------------------------------

/// オトモ spends [Y] on 項目切替 and クエスト装備 does not answer it at all, so
/// the pills that said "[Y] 刻み" named a key that did something else.
#[test]
fn the_step_pills_name_no_key_where_the_key_is_taken() {
    let mut ed = editor_with(blank_save(), Screen::Roster);
    assert_eq!(ed.step_table().1, "", "オトモ spends [Y] on 項目切替");
    ed.screen = Screen::QuestEquip;
    assert_eq!(ed.step_table().1, "", "クエスト装備 does not answer [Y]");
    // The screens that do answer it keep it.
    ed.screen = Screen::Hunts;
    assert_eq!(ed.step_table().1, "A");
    ed.screen = Screen::Char;
    assert_eq!(ed.step_table().1, "Y");
    ed.screen = Screen::QuestEdit;
    assert_eq!(ed.step_table().1, "Y");
}

// ---------------------------------------------------------------------------
// #12. The talisman [A] word is an action
// ---------------------------------------------------------------------------

/// "0件" is a count, not an action: the button names the press it runs.
#[test]
fn the_talisman_a_word_is_an_action() {
    let mut ed = editor_with(save_with_charms(3), Screen::Charms);
    ed.open_charms();
    ed.charm_filter = String::from("zzzznothing");
    assert_eq!(ed.charm_a_label(), "解除");
    assert!(
        ed.footer_hint().contains("[A] 解除"),
        "{}",
        ed.footer_hint()
    );
    ed.charm_a_press();
    assert!(ed.charm_filter.is_empty(), "the press drops the search");
    assert_eq!(ed.charms().len(), 3);
    assert_eq!(ed.charm_a_label(), "編集");
    // And the console's [A] is the same press (ui.rs handle_pad_key).
    assert_eq!(
        console_answers("Charms", "A").first(),
        Some(&Answer::Call(String::from("charm_a_press")))
    );
}

// ---------------------------------------------------------------------------
// #13. [MAX] on the item cell says so instead of refusing 99 times
// ---------------------------------------------------------------------------

/// The console sent [▶] ninety-nine times, which on the アイテム cell is
/// ninety-nine refusals and no write at all.
#[test]
fn max_on_the_item_cell_says_so_instead_of_refusing() {
    let mut ed = editor_on_the_reward_table();
    ed.entry_field = 0;
    let count = reward_count(&ed);
    let chance = reward_chance(&ed);
    ed.entry_row_max();
    assert_eq!(reward_count(&ed), count, "the item cell has no maximum");
    assert_eq!(reward_chance(&ed), chance);
    assert!(
        ed.message.contains("アイテム"),
        "the press must say what the cell does: {}",
        ed.message
    );

    ed.entry_field = 1;
    ed.entry_row_max();
    assert_eq!(reward_count(&ed), 99);
    ed.entry_field = 2;
    ed.entry_row_max();
    assert_eq!(reward_chance(&ed), 100);
}

// ---------------------------------------------------------------------------
// #14. A failed system keyboard opens the built-in one
// ---------------------------------------------------------------------------

/// The sentence sent the user to a keyboard nothing on that screen can open.
#[test]
fn a_failed_system_keyboard_opens_the_built_in_one() {
    let src = switch_editor_src();
    let body = fn_body(src, "fn service(");
    let failed = body
        .find("SwkbdAnswer::Failed")
        .expect("the failed answer");
    let after = &body[failed..];
    let end = after
        .find("KeyboardRoute::BuiltinFallback")
        .expect("the next route");
    let arm = &after[..end];
    assert!(
        arm.contains("open_osk_with_note"),
        "the press must fall back to the built-in keyboard: {arm}"
    );
    assert!(
        !src.contains("内蔵キーボードで入力してください"),
        "that sentence names a keyboard the screen cannot open"
    );
}

// ---------------------------------------------------------------------------
// #15. The my-sets list says where the release key is
// ---------------------------------------------------------------------------

/// [-] on the set list answered nothing while the text build's table said
/// 解除(2回): a key on the table and silent on the pad.
#[test]
fn the_sets_list_says_where_the_release_key_is() {
    let mut ed = editor_with(blank_save(), Screen::Sets);
    ed.input(Key::Minus);
    assert!(
        !ed.message.is_empty(),
        "[-] on the set list must not be silent"
    );
    assert!(ed.message.contains("編集中"), "{}", ed.message);
}

// ---------------------------------------------------------------------------
// #16. The guild card's deck reaches the section's other tabs
// ---------------------------------------------------------------------------

/// While the card is showing, the bar is the card's four views, so 狩猟記録 and
/// 進行状況 are not on it: the deck is the only way a finger can reach them.
#[test]
fn the_guild_deck_reaches_the_section_tabs() {
    let deck = fn_body(ui_src(), "fn draw_studio_inspector(");
    let at = deck.find("Screen::Guild =>").expect("the guild deck arm");
    let rest = &deck[at..];
    let arm = &rest[..rest.find("\n            Screen::").unwrap_or(rest.len())];
    assert!(arm.contains("進行状況"), "{arm}");
    assert!(arm.contains("狩猟記録"), "{arm}");
    assert!(arm.contains("UiAction::OpenHunts"), "{arm}");
    // The action opens the screen the section tab would have.
    let mut ed = editor_with(blank_save(), Screen::Guild);
    ed.open_hunts();
    assert_eq!(ed.screen, Screen::Hunts);
    // And the console's action runs it.
    assert!(ui_src().contains("UiAction::OpenHunts => ed.open_hunts()"));
}

// ---------------------------------------------------------------------------
// #17. The talisman [＋] is one press on both routes
// ---------------------------------------------------------------------------

/// The touch button called open_charms() first, so it put the list back at the
/// top and the pad's [＋] did not.
#[test]
fn the_talisman_plus_is_one_press_on_both_routes() {
    let src = ui_src();
    let at = src
        .find("UiAction::NewCharm =>")
        .expect("the new-charm action");
    let rest = &src[at..];
    let arm = &rest[..rest.find('\n').unwrap_or(rest.len())];
    assert!(arm.contains("ed.input(Key::L)"), "{arm}");
    assert!(
        !arm.contains("open_charms"),
        "the touch route must not reset the list: {arm}"
    );
    let mut ed = editor_with(save_with_charms(5), Screen::Charms);
    ed.open_charms();
    ed.cursor = 3;
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert_eq!(ed.cursor, 3, "[＋] must keep the row it was on");
}

// ---------------------------------------------------------------------------
// #18. The slot word is the one the screen prints
// ---------------------------------------------------------------------------

/// "枠選択" appeared nowhere else: not in the README, not on any screen.
#[test]
fn the_slot_word_is_the_one_the_screen_prints() {
    for screen in [Screen::Charms, Screen::Equip] {
        let mut ed = editor_with(save_with_charms(1), screen);
        if screen == Screen::Charms {
            ed.open_charms();
        }
        let footer = ed.footer_hint();
        // The slot list's name on the bar is スロット, and that is the word the
        // [B] item prints (hbedit::screen_name).
        assert!(footer.contains("[B] スロット"), "{screen:?}: {footer}");
        assert!(!footer.contains("枠選択"), "{screen:?}: {footer}");
    }
    let ed = editor_with(blank_save(), Screen::Guild);
    let footer = ed.footer_hint();
    assert!(footer.contains("[B] スロット"), "{footer}");
    // The guide prints the same table's first items, so it cannot name a
    // different destination for [B] than the footer does.
    let guide = ed.screen_guide();
    for fact in ed.guide_facts() {
        assert!(guide.contains(&fact.render()), "{guide}");
    }
    assert!(!guide.contains("[B] 戻る"), "{guide}");
    assert!(!ui_src().contains("枠選択"), "the console still says 枠選択");
    assert!(
        !switch_editor_src().contains("枠選択"),
        "the text build still says 枠選択"
    );
    // The destination screen prints the same word: one table of names
    // (hbedit::screen_name), read by the bar, the header and every [B].
    assert!(hbedit_src().contains("Screen::Slots => \"スロット\""));
    assert!(ui_src().contains("\"スロット\""));
}

// ---------------------------------------------------------------------------
// #19. The README names the three conditions
// ---------------------------------------------------------------------------

/// The README is where a user learns the keys, so the three conditions the
/// review found missing live there.
#[test]
fn the_readme_names_the_three_conditions() {
    let r = readme();
    assert!(
        r.contains("選択リストでは押した時点"),
        "the [R] note: a pick list answers on the press"
    );
    // The roster's [+] condition: the skill / support panel and the equipment
    // panel both quit, and the line says so (the sixth review found the
    // equipment panel missing from it).
    assert!(
        r.contains("オトモで終了になるのはパネルを開いている間だけ"),
        "the [＋] condition on the roster"
    );
    assert!(
        r.contains("装備パネル"),
        "the equipment panel quits on [+] too"
    );
    assert!(
        r.contains("表示の送りは") && r.contains("4 つ"),
        "the guild card's four views and the key that walks them"
    );
    assert!(!r.contains("枠選択"), "the word the screens never printed");
}
