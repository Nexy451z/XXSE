//! The third review's leftovers: fifteen findings, one test each.
//!
//! The console's dispatch cannot be driven from a host test, so what is pinned
//! here is the layer underneath it - the state machine, the strings and the
//! geometry it hands the console - plus the wiring in switch/src/ui.rs read as
//! text. Every test names the finding it belongs to.
//!
//! Nothing here writes a card: every save is built in memory.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::charm::{self, Charm};
use mhnx_core::hbedit::{
    self, caption_width, row_width, Editor, EquipEdit, Key, Screen, FOOTER_HINT_ROOM,
    PROGRESS_QUESTS, QUEST_TAB_BASIC, QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, QUEST_TAB_SUPPLIES,
};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};
use mhnx_core::questprogress::{self, Progress};
use mhnx_core::save;

// ---------------------------------------------------------------------------
// The three layers, as text
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

/// The body of one match arm: from its pattern to the next arm, or to the
/// brace that closes the match. An arm's body is indented 16 spaces and the
/// next arm (or the closing brace) 12, so the two cut points are unambiguous.
fn arm_body<'a>(src: &'a str, decl: &str) -> &'a str {
    let start = src
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} is not in the console sources"));
    let rest = &src[start + decl.len()..];
    let mut cut = rest.len();
    let mut line_start = true;
    let mut leading = 0usize;
    for (idx, c) in rest.char_indices() {
        if line_start {
            if c == ' ' {
                leading += 1;
                continue;
            }
            if (leading == 12 && c != '\n') || (leading == 8 && c == '}') {
                cut = idx - leading;
                break;
            }
            line_start = false;
            leading = 0;
        }
        if c == '\n' {
            line_start = true;
            leading = 0;
        }
    }
    &rest[..cut]
}

/// The body of one match arm inside one function (the same arm name can appear
/// on more than one screen).
fn arm_in<'a>(src: &'a str, scope: &str, decl: &str) -> &'a str {
    let start = src
        .find(scope)
        .unwrap_or_else(|| panic!("{scope} is not in the console sources"));
    arm_body(&src[start..], decl)
}

fn starts(chars: &[char], at: usize, pat: &str) -> bool {
    let mut i = at;
    for c in pat.chars() {
        if i >= chars.len() || chars[i] != c {
            return false;
        }
        i += 1;
    }
    true
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

/// A blank save with one character and one talisman, and the form open on it.
fn charm_editor() -> Editor {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    {
        let b = save.equipment_box_mut(0).expect("the box exists");
        b[..charm::CHARM_RECORD].copy_from_slice(&ten_record());
    }
    let charm = Charm {
        index: 0,
        slots: 3,
        skill1: 74,
        points1: 6,
        skill2: 70,
        points2: 10,
    };
    assert!(
        save.set_charm_with_kind(0, &charm, 10),
        "the seed talisman must be written"
    );
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.screen = Screen::Charms;
    ed.cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    ed
}

const HASH_SEM: u32 = 0x2553_701D;
const HASH_REM: u32 = 0x5B3C_302D;
const HASH_SUP: u32 = 0x5453_9AEE;
const HASH_MIB: u32 = 0x1BBF_D18E;
const HASH_GMD: u32 = 0x242B_B29A;

fn entry(name: &str, type_hash: u32, data: Vec<u8>) -> ArcEntry {
    ArcEntry {
        name: String::from(name),
        type_hash,
        data,
        original: None,
        original_offset: None,
    }
}

/// A .mib with the quest id and the time limit filled in.
fn mib_bytes(id: u32, minutes: u8) -> Vec<u8> {
    let mut mib = vec![0u8; MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
    mib[0x16] = minutes;
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

/// 168 byte supply box with two stacks.
fn sup_bytes() -> Vec<u8> {
    let mut v = vec![0u8; 168];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8..12].copy_from_slice(&[0xB4, 0x06, 3, 0]);
    v[12..16].copy_from_slice(&[0xB6, 0x06, 2, 0]);
    v
}

fn sem_bytes(area: u32) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[0x0C..0x10].copy_from_slice(&area.to_le_bytes());
    v
}

/// A whole quest, in the shape q1010165 has.
fn quest_arc(id: u32, minutes: u8) -> Arc {
    Arc {
        version: ARC_VERSION,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry(
                "quest\\boss\\setEmMain\\b_m00em000_00",
                HASH_SEM,
                sem_bytes(0),
            ),
            entry(
                "quest\\boss\\setEmMain\\b_m15em010_00",
                HASH_SEM,
                sem_bytes(1),
            ),
            entry("quest\\rem\\rem_000000", HASH_REM, vec![0u8; 184]),
            entry("quest\\rem\\rem_620114", HASH_REM, rem_bytes()),
            entry("quest\\supp\\supp_1010165", HASH_SUP, sup_bytes()),
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
fn quest_editor() -> Editor {
    let mut ed = Editor::new();
    let bytes = quest_arc(1_010_165, 50).to_bytes().expect("the archive rebuilds");
    ed.open_arc_bytes("sdmc:/quest/q1010165.arc", &bytes);
    ed.enter_quest_edit();
    assert_eq!(ed.screen, Screen::QuestEdit, "{}", ed.message);
    ed
}

// ---------------------------------------------------------------------------
// 1: a jump that goes through drops the form it asked about
// ---------------------------------------------------------------------------

/// form_leave_press promised that the second press "goes through, and the jump
/// itself drops the form" - but neither jump dropped the talisman or the
/// equipment form. The draft stayed in memory, so every later move asked the
/// same question again and [＋] named a form that was not on screen.
#[test]
fn a_jump_that_goes_through_drops_the_form() {
    let mut ed = charm_editor();
    // A draft the box does not have: the form's first skill value is raised.
    let mut form = ed.charm_edit.expect("the form is open");
    form.points1 += 1;
    ed.charm_edit = Some(form);
    assert_eq!(ed.draft_form_name(), Some("護石編集"), "the draft is dirty");

    // The first press only asks, and the form is still there.
    assert!(ed.form_leave_press(), "the first press is the question");
    assert!(ed.charm_edit.is_some(), "the question keeps the form");
    assert_eq!(ed.draft_form_name(), Some("護石編集"));

    // The press that goes through is the one that drops it.
    assert!(!ed.form_leave_press(), "the second press goes through");
    assert!(ed.drop_form_drafts(), "there was a form to drop");
    assert!(ed.charm_edit.is_none(), "the form is gone");
    assert_eq!(ed.draft_form_name(), None, "no draft is left to ask about");

    // Which is what [＋] reads. With the draft still in memory the quit asks
    // about a form nobody can see; with it dropped the same press quits.
    let mut stale = charm_editor();
    let mut form = stale.charm_edit.expect("the form is open");
    form.points1 += 1;
    stale.charm_edit = Some(form);
    stale.screen = Screen::SaveList;
    stale.dirty = false;
    assert!(!stale.press_quit(), "a draft is not a clean session");
    assert!(
        stale.message.contains("護石編集"),
        "the quit has to name the form: {}",
        stale.message
    );

    ed.screen = Screen::SaveList;
    ed.dirty = false;
    ed.message.clear();
    assert!(ed.press_quit(), "a clean session quits on one press");
    assert!(
        !ed.message.contains("下書き"),
        "the quit named a form nobody could see: {}",
        ed.message
    );

    // And the two jumps call it on the press that goes through.
    assert!(
        fn_body(ui_src(), "fn goto_section").contains("drop_form_drafts()"),
        "the section jump drops the form"
    );
    assert!(
        fn_body(ui_src(), "pub fn select_sub_tab").contains("drop_form_drafts()"),
        "the sub-tab jump drops the form"
    );
}

// ---------------------------------------------------------------------------
// 2: the picker modal leaves no writer armed
// ---------------------------------------------------------------------------

/// The shared picker has three writers and each one is a flag. Closing the
/// panel cleared two of them, so a 種別 list closed with [B] left
/// equip_kind_picker set and the next .mib / area list's [A] ran
/// apply_equip_kind instead of writing the row it was showing.
#[test]
fn the_picker_modal_leaves_no_writer_armed() {
    let ui = ui_src();
    let close = fn_body(ui, "fn close_picker_modal");
    for flag in [
        "self.mib_picker = None",
        "self.quest_area_slot = None",
        "self.equip_kind_picker = false",
    ] {
        assert!(close.contains(flag), "close_picker_modal misses {flag}: {close}");
    }
    // Every way in and every way out goes through it.
    for site in [
        "UiAction::MibPickerClose =>",
        "UiAction::SelectSlot(s) =>",
        "UiAction::OpenSlot(s) =>",
    ] {
        assert!(
            arm_body(ui, site).contains("close_picker_modal()"),
            "{site} leaves a writer armed"
        );
    }
    for site in [
        "pub fn open_mib_picker",
        "fn open_equip_kind_picker",
        "pub fn open_quest_area_picker",
    ] {
        assert!(
            fn_body(ui, site).contains("close_picker_modal()"),
            "{site} does not disarm the other writers"
        );
    }
    assert!(
        fn_body(ui, "pub fn handle_mib_picker_key").contains("close_picker_modal()"),
        "[B] in the modal disarms every writer"
    );
}

// ---------------------------------------------------------------------------
// 3: the decoration list goes back to the side that opened it
// ---------------------------------------------------------------------------

/// The list is one screen with two writers, and the side was inferred from the
/// talisman socket alone - a field only the talisman form ever set. A list
/// opened on the talisman, left with a tab press, and then opened from the
/// equipment form still carried the old socket: the pick wrote a form that was
/// gone and the screen went to 護石編集, which draws nothing without a form.
#[test]
fn the_decoration_list_returns_to_the_side_that_opened_it() {
    // The talisman form's list.
    let mut ed = charm_editor();
    let mut form = ed.charm_edit.expect("the form is open");
    form.field = 7; // 装飾品1
    ed.charm_edit = Some(form);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::JewelPick, "{}", ed.message);
    assert_eq!(ed.jewel_pick_return, Screen::CharmEdit, "the talisman opened it");
    assert_eq!(ed.jewel_pick_charm_socket, Some(0));
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::CharmEdit, "B goes back to the talisman form");

    // The tab press that closes the form but not the socket: the setup of the
    // bug. The equipment form's list must not inherit it.
    ed.drop_form_drafts();
    assert!(ed.charm_edit.is_none());
    ed.equip_edit = Some(EquipEdit {
        index: 0,
        kind: 7,
        id: 1,
        level: 1,
        transmog_id: 0,
        transmog_level: 1,
        decorations: [0; 3],
        field: 4, // 装飾品1
        new: false,
    });
    ed.screen = Screen::EquipEdit;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::JewelPick, "{}", ed.message);
    assert_eq!(ed.jewel_pick_return, Screen::EquipEdit, "the equipment opened it");
    assert_eq!(
        ed.jewel_pick_charm_socket, None,
        "the talisman socket is disarmed"
    );
    ed.input(Key::A); // pick the row under the cursor
    assert_eq!(
        ed.screen,
        Screen::EquipEdit,
        "the pick goes back to the equipment form, not to 護石編集"
    );

    // And a form that is gone lands on the list that owns it, never on a blank
    // screen.
    ed.equip_edit = None;
    ed.jewel_pick_return = Screen::CharmEdit;
    ed.screen = Screen::JewelPick;
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms, "no form: the list is the way back");
}

// ---------------------------------------------------------------------------
// 4 / 14: every status message speaks Japanese
// ---------------------------------------------------------------------------

/// The text of the source with its comments removed, keeping the strings.
fn strip_comments(src: &str) -> Vec<char> {
    let b: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            while i < b.len() && b[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < b.len() && b[i + 1] == '*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                out.push(if b[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '"' {
            out.push(c);
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i]);
                    out.push(b[i + 1]);
                    i += 2;
                    continue;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push('"');
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Every string literal a message assignment can print, with its line.
fn message_literals(src: &str) -> Vec<(usize, String)> {
    let chars = strip_comments(src);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !starts(&chars, i, "message") {
            i += 1;
            continue;
        }
        // Not the tail of a longer name.
        if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            i += 1;
            continue;
        }
        let mut j = i + "message".len();
        while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
            j += 1;
        }
        if j >= chars.len() || chars[j] != '=' {
            i += 1;
            continue;
        }
        if j + 1 < chars.len() && chars[j + 1] == '=' {
            i += 1;
            continue;
        }
        let line = 1 + chars[..i].iter().filter(|c| **c == '\n').count();
        // The statement runs to the semicolon at depth 0, and every literal in
        // it is one of the strings the line can print.
        let mut k = j + 1;
        let mut depth = 0i32;
        while k < chars.len() {
            let c = chars[k];
            if c == '"' {
                let start = k;
                k += 1;
                while k < chars.len() && chars[k] != '"' {
                    if chars[k] == '\\' {
                        k += 2;
                    } else {
                        k += 1;
                    }
                }
                let lit: String = chars[start + 1..k.min(chars.len())].iter().collect();
                if !lit.is_empty() {
                    out.push((line, lit));
                }
                k += 1;
                continue;
            }
            if c == '(' || c == '[' || c == '{' {
                depth += 1;
            } else if c == ')' || c == ']' || c == '}' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            } else if c == ';' && depth == 0 {
                break;
            }
            k += 1;
        }
        i = k.max(i + 1);
    }
    out
}

/// The pure-ASCII literals a message statement may still hold, and why.
///
/// Everything else on the status line is a sentence and is Japanese: the review
/// found seven that were not, and the test that guarded this looked at nine
/// hand-written strings instead of the messages themselves.
const ASCII_ALLOWED: &[&str] = &[
    // A number, not a sentence: the hex prefix of a value the line prints.
    "{}: 0x{:X}",
    // A file extension, in a match arm inside the same statement.
    "mib",
    // A language's own name, which is what the setting is called.
    "English",
    // The two states of a flag.
    "OFF",
    "ON",
];

/// The ASCII words a Japanese sentence may still carry.
///
/// A word here is a name this interface has no Japanese spelling for: a file
/// extension, a unit, a key, the game's own title. It is never an English word
/// standing in for a Japanese one - which is what "この tab は" was, and what
/// the old test could not see: the sentence had a Japanese word in it, so the
/// "at least one non-ASCII character" check passed.
const ASCII_WORDS_ALLOWED: &[&str] = &[
    // Keys and units.
    "ZR", "ID", "Lv", "HR", "EXP", "MAX", "Slot",
    // File formats and folders.
    "arc", "mib", "gmd", "sem", "sup", "bak", "CSV", "DLC", "SD",
    // The game and the two machines.
    "MHXX", "MHGU", "Switch", "DS",
    // A flag's two states, and the two keyboards' own words.
    "ON", "OFF", "YES", "UTF", "English",
    // Field names the game's own tables spell in English.
    "Wave", "Flag", "QDP", "BOX",
    // The shapes a typed value is asked for in.
    "xFFFF", "ss", "cc", "MM", "rec",
];

/// Every ASCII word of two or more letters in a string.
fn ascii_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_ascii_alphabetic() {
            word.push(c);
        } else {
            if word.len() >= 2 {
                out.push(core::mem::take(&mut word));
            } else {
                word.clear();
            }
        }
    }
    if word.len() >= 2 {
        out.push(word);
    }
    out
}

#[test]
fn every_status_message_speaks_japanese() {
    let mut checked = 0;
    for (name, src) in [
        ("hbedit.rs", hbedit_src()),
        ("editor.rs", editor_src()),
        ("ui.rs", ui_src()),
    ] {
        for (line, lit) in message_literals(src) {
            // A literal with no ASCII letter is a format string or a symbol
            // ("{}", " / ", "-") and has no language.
            if !lit.chars().any(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            checked += 1;
            if ASCII_ALLOWED.contains(&lit.as_str()) {
                continue;
            }
            assert!(
                lit.chars().any(|c| !c.is_ascii()),
                "{name}:{line} prints an English status message: {lit:?}"
            );
            // One Japanese word is not enough: an English word mixed into the
            // sentence is the same bug with a Japanese alibi ("この tab は").
            for word in ascii_words(&lit) {
                assert!(
                    ASCII_WORDS_ALLOWED.contains(&word.as_str()),
                    "{name}:{line} mixes the English word {word:?} into a status \
                     message: {lit:?}"
                );
            }
        }
    }
    assert!(checked > 100, "the scan found only {checked} messages");

    // The check has teeth: the sentence this review found is made of one
    // Japanese word and one English one, and the word list refuses it.
    let found = "検索は 報酬 / 支給品 タブで使います（この tab は [A] で編集）";
    assert!(found.chars().any(|c| !c.is_ascii()), "the old check passed it");
    assert_eq!(ascii_words(found), vec![String::from("tab")], "{found}");
    assert!(
        !ASCII_WORDS_ALLOWED.contains(&"tab"),
        "tab is not a name this interface has"
    );
}

// ---------------------------------------------------------------------------
// 5: the deck's pills fit their own labels
// ---------------------------------------------------------------------------

/// Every btn_text(c, x, y, w, h, "literal", ...): (line, label, width).
fn btn_text_literals(src: &str) -> Vec<(usize, String, i32)> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !starts(&chars, i, "btn_text(") {
            i += 1;
            continue;
        }
        let line = 1 + chars[..i].iter().filter(|c| **c == '\n').count();
        let mut args: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut depth = 0i32;
        let mut k = i + "btn_text(".len();
        while k < chars.len() {
            let c = chars[k];
            if c == '"' {
                cur.push(c);
                k += 1;
                while k < chars.len() && chars[k] != '"' {
                    if chars[k] == '\\' && k + 1 < chars.len() {
                        cur.push(chars[k]);
                        cur.push(chars[k + 1]);
                        k += 2;
                        continue;
                    }
                    cur.push(chars[k]);
                    k += 1;
                }
                if k < chars.len() {
                    cur.push('"');
                    k += 1;
                }
                continue;
            }
            if c == '(' || c == '[' || c == '{' {
                depth += 1;
            } else if c == ')' && depth == 0 {
                break;
            } else if c == ')' || c == ']' || c == '}' {
                depth -= 1;
            } else if c == ',' && depth == 0 {
                args.push(cur.trim().to_string());
                cur.clear();
                k += 1;
                continue;
            }
            cur.push(c);
            k += 1;
        }
        args.push(cur.trim().to_string());
        if args.len() >= 6 {
            if let (Ok(w), Some(lit)) = (
                args[3].parse::<i32>(),
                args[5]
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"')),
            ) {
                out.push((line, lit.to_string(), w));
            }
        }
        i = k + 1;
    }
    out
}

/// The オトモ deck's [Y] pill said "[Y] 列: なつき" - 122px in a 96px pill, 84px
/// of label box - and drew as "[Y] 列:..". The pill names the column now, and
/// the three of them are sized to their own labels.
#[test]
fn the_otomo_deck_pills_fit_their_labels() {
    // The measurement the review made, which is what the model below has to
    // reproduce: "[Y] 列: なつき" is 122px at the 17px caption size.
    assert_eq!(caption_width("[Y] 列: なつき"), 122);

    let ui = ui_src();
    let arm = arm_in(ui, "fn draw_sub_bar_quick_tools", "Screen::Roster => {");
    assert!(
        arm.contains("roster_column_label()"),
        "the pill asks the state machine for the column: {arm}"
    );
    // The three pills of the roster deck, in drawing order.
    let mut widths = Vec::new();
    for chunk in arm.split("c.round_rect(").skip(1) {
        let args: Vec<&str> = chunk.split(',').take(3).collect();
        if args.len() == 3 && args[1].trim() == "sy" {
            let w: i32 = args[2].trim().parse().expect("a literal width");
            widths.push(w);
        }
    }
    assert_eq!(widths.len(), 3, "three pills: {widths:?}");
    let labels = ["[Y] なつき", "[ZR] 装備", "[+] スキル"];
    for (label, w) in labels.iter().zip(widths.iter()) {
        assert!(
            caption_width(label) + 12 <= *w,
            "{label} is {}px in a {w}px pill",
            caption_width(label)
        );
    }
    assert!(
        widths.iter().sum::<i32>() + 12 <= 302,
        "the deck is 302px: {widths:?}"
    );

    // The other deck pills: every literal label the console draws has to fit
    // the box it is centred in (btn_text keeps 12px inside it).
    let mut checked = 0;
    for (line, label, w) in btn_text_literals(ui) {
        checked += 1;
        assert!(
            caption_width(&label) + 12 <= w,
            "ui.rs:{line} draws {label:?} ({}px) in a {w}px pill",
            caption_width(&label)
        );
    }
    assert!(checked > 20, "the scan found only {checked} literal labels");
}

// ---------------------------------------------------------------------------
// 6: the quest screen's footer names the save key and the way back
// ---------------------------------------------------------------------------

/// [X] 保存 and [B] were the two items the クエスト改造 footer never named, on
/// the one screen whose [X] writes the .arc rather than the save.
#[test]
fn the_quest_edit_footer_names_save_and_back() {
    let mut ed = quest_editor();
    for tab in [
        QUEST_TAB_BASIC,
        QUEST_TAB_MONSTERS,
        QUEST_TAB_REWARDS,
        QUEST_TAB_SUPPLIES,
    ] {
        ed.quest_set_tab(tab);
        let hint = ed.footer_hint();
        assert!(
            hint.contains("[X] 保存"),
            "tab {tab} does not name the save: {hint}"
        );
        assert!(
            hint.contains("[B] "),
            "tab {tab} does not name the way back: {hint}"
        );
        let w = caption_width(&hint);
        assert!(
            w <= FOOTER_HINT_ROOM,
            "tab {tab} is {w}px of {FOOTER_HINT_ROOM}px: {hint}"
        );
    }
}

// ---------------------------------------------------------------------------
// 7: the 既読 pill names what the press does
// ---------------------------------------------------------------------------

/// The press is a toggle, so the label has to follow the bit: it said
/// "既読にする" on a row that was already seen, and the press then took the bit
/// off - a button that names the opposite of what it does.
#[test]
fn the_seen_pill_names_what_the_press_does() {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    assert!(questprogress::set(&mut save, 0, Progress::Seen, 1, true));
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.screen = Screen::Progress;
    ed.progress_tab = PROGRESS_QUESTS;
    ed.quest_cursor = 0;

    assert_eq!(
        ed.progress_seen_label(),
        "既読を外す",
        "the row is already seen"
    );
    ed.progress_seen_press();
    assert_eq!(ed.progress_seen_label(), "既読にする", "the bit is off now");
    assert_eq!(
        questprogress::get(ed.save.as_ref().unwrap(), 0, Progress::Seen, 1),
        Some(false),
        "the press cleared the bit the label named"
    );

    // The console draws the state machine's answer, not a literal.
    let deck = arm_in(
        ui_src(),
        "fn draw_sub_bar_quick_tools",
        "Screen::Progress => {",
    );
    assert!(
        deck.contains("progress_seen_label()"),
        "the pill asks the state machine: {deck}"
    );
}

// ---------------------------------------------------------------------------
// 8: the 基本 tab answers the key that was pressed
// ---------------------------------------------------------------------------

/// [ZR] on 基本 said "このタブは [◀/▶] では変わりません" - a sentence about a key
/// the user had not pressed, and about the one key that does work there.
#[test]
fn the_basic_tab_answers_the_key_that_was_pressed() {
    let mut ed = quest_editor();
    ed.quest_set_tab(QUEST_TAB_BASIC);
    ed.input(Key::Z);
    assert!(
        ed.message.contains("[ZR]"),
        "the line has to answer [ZR]: {}",
        ed.message
    );
    assert!(
        !ed.message.contains("[◀/▶] では"),
        "the line answers for a key that was not pressed: {}",
        ed.message
    );
    // The other three tabs still say what [ZR] walks.
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    ed.input(Key::Z);
    assert!(
        ed.message.contains("[◀/▶] で変更する項目"),
        "{}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// 9: [＋] has one meaning on the line
// ---------------------------------------------------------------------------

/// "[＋] 新規 …（[＋]は[B]の後）" put the same key on one line with two
/// meanings. The exit is its own item now, and it does not reuse the glyph.
#[test]
fn the_plus_key_has_one_meaning_on_the_line() {
    let mut ed = Editor::new();
    for screen in [Screen::Charms, Screen::Equip, Screen::Roster] {
        ed.screen = screen;
        assert!(ed.plus_is_a_tool(), "{screen:?} spends [＋] on a tool");
        let hint = ed.footer_hint();
        assert_eq!(
            hint.matches("[＋]").count(),
            1,
            "{screen:?} gives [＋] two meanings on one line: {hint}"
        );
        // The note is bracketed off the key column now: it sat where the
        // chips' words sit and read as one more key.
        // The note names no key at all: the [B] item beside it already says
        // where that press goes, and [＋] is the tool on this line (the seventh
        // review: a note that named [B] read as "press [B] to quit").
        assert!(
            hint.contains("終了は一覧から"),
            "{screen:?} does not name the way out: {hint}"
        );
        assert!(
            !hint.contains("（[＋]は"),
            "{screen:?} still hangs the exit on the [＋] item: {hint}"
        );
        assert!(
            caption_width(&hint) <= FOOTER_HINT_ROOM,
            "{screen:?} is {}px: {hint}",
            caption_width(&hint)
        );
    }
    // A screen that does not spend [＋] still names the quit key.
    ed.screen = Screen::Slots;
    assert!(ed.footer_hint().contains("[＋] 終了"));
}

// ---------------------------------------------------------------------------
// 10: 生データ's guide follows the mode
// ---------------------------------------------------------------------------

/// The list's [◀/▶] is a ten-row jump and the byte editor's is the value. One
/// guide served both and described the editor's key on the list.
#[test]
fn the_save_map_guide_follows_the_mode() {
    let mut ed = Editor::new();
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    ed.save = Some(save);
    ed.screen = Screen::SaveMap;
    let list = ed.screen_guide();
    assert!(
        !list.contains("1バイト") && !list.contains("値の変更"),
        "the list's guide describes the byte editor: {list}"
    );
    // The footer's line carries the [◀/▶] word the guide has no room for: the
    // guide leads with [A] and the row pair now, and the ten-row jump is the
    // footer's to state.
    let footer = ed.footer_hint();
    assert!(footer.contains("[◀/▶] 10行"), "{footer}");

    // [A] opens the byte editor, and the guide changes with it.
    ed.input(Key::A);
    assert!(ed.map_edit_open(), "{}", ed.message);
    let edit = ed.screen_guide();
    assert!(edit.contains("1バイト"), "the editor's guide: {edit}");
    assert_ne!(list, edit, "the two modes read differently");
    let footer = ed.footer_hint();
    assert!(footer.contains("[◀/▶] 値の変更"), "{footer}");
}

// ---------------------------------------------------------------------------
// 11: the guide tail is measured once
// ---------------------------------------------------------------------------

/// guide_tail returned a suffix that fitted the room, and the renderer then
/// drew it in the room the marker left - so text_clipped cut it a second time
/// and the last item of the sentence lost its last glyph or two.
#[test]
fn the_guide_tail_is_measured_once() {
    let guide = "[▲/▼] 行  [A] 文字編集  [◀/▶] 制限時間  [X] 保存  [B] 戻る";
    let room = 400;
    let marker_w = row_width("…");
    // What the renderer does now: the marker's width is taken out first.
    let tail = hbedit::guide_tail(guide, room - marker_w, &mut |s| row_width(s));
    assert!(!tail.is_empty(), "something has to fit");
    assert!(
        row_width(tail) + marker_w <= room,
        "the tail and its marker have to fit the strip: {} + {} > {}",
        row_width(tail),
        marker_w,
        room
    );
    // The item at the end of the line survives whole.
    assert!(tail.ends_with("[B] 戻る"), "the way out is cut: {tail}");

    // And the renderer passes the reduced room, instead of measuring against
    // the whole strip and drawing in the rest.
    let body = fn_body(ui_src(), "fn draw_guide");
    assert!(body.contains("tail_room"), "{body}");
    // The tail is the *state's* tail: the keys are dropped before the state is
    // cut (the seventh review: a banner must not delete the position).
    assert!(
        body.contains("guide_tail(&state, tail_room"),
        "the tail is measured against the room it is drawn in: {body}"
    );
}

// ---------------------------------------------------------------------------
// 12: a new quest does not inherit the search
// ---------------------------------------------------------------------------

/// The item search survived the jump to another quest, and a quest whose
/// rewards do not carry the needle then said "報酬テーブルがありません" about a
/// table that was on screen all along.
#[test]
fn a_new_quest_does_not_inherit_the_search() {
    let mut ed = quest_editor();
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    let all = ed.quest_rewards().len();
    assert!(all > 0, "the sample quest carries a reward table");

    ed.apply_quest_item_filter(String::from("そんなアイテムはない"));
    assert_eq!(ed.quest_rewards().len(), 0, "the search narrows the table");

    // An empty tab says why it is empty.
    ed.input(Key::Right);
    assert!(ed.message.contains("検索"), "{}", ed.message);
    assert!(
        !ed.message.contains("報酬テーブルがありません"),
        "the table is there; the search is why the list is empty: {}",
        ed.message
    );

    // Coming back from a file screen is the same quest, so the search stays.
    ed.screen = Screen::ArcEntries;
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::QuestEdit, "{}", ed.message);
    assert_eq!(
        ed.quest_item_filter, "そんなアイテムはない",
        "the search belongs to this quest and survives the file screen"
    );

    // Another quest drops it: the needle named an item of the tables that were
    // open, and this quest has its own.
    let other = quest_arc(1_010_166, 30).to_bytes().expect("the archive rebuilds");
    ed.open_arc_bytes("sdmc:/quest/q1010166.arc", &other);
    ed.enter_quest_edit();
    assert!(
        ed.quest_item_filter.is_empty(),
        "the search survived the jump: {}",
        ed.quest_item_filter
    );
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    assert_eq!(ed.quest_rewards().len(), all, "every row is back");
}

// ---------------------------------------------------------------------------
// 13: the row count follows the measured line height
// ---------------------------------------------------------------------------

/// The window arithmetic was pinned with the 17px caption height, which is not
/// the height the font reports (20..22px measured). At the measured height a
/// second line of strip costs a row, and a test that assumed 17px never saw it.
#[test]
fn the_list_window_counts_rows_at_the_measured_line_height() {
    // 508px of body, 56px rows: nine rows, and the permanent notice costs one.
    for line_h in [20, 21, 22] {
        assert_eq!(
            hbedit::list_window_rows(508, 56, 0, line_h),
            9,
            "no strip at {line_h}px"
        );
        assert_eq!(
            hbedit::list_window_rows(508, 56, 1, line_h),
            8,
            "the screens with a permanent notice show eight at {line_h}px"
        );
    }
    // Two lines of strip: the boundary is real, and it is where the measured
    // height puts it - 20px still fits eight, 21 and 22 cost the row.
    assert_eq!(hbedit::list_window_rows(508, 56, 2, 20), 8);
    assert_eq!(hbedit::list_window_rows(508, 56, 2, 21), 7);
    assert_eq!(hbedit::list_window_rows(508, 56, 2, 22), 7);

    // The console measures the height instead of typing it, and the 生データ
    // list uses the same arithmetic instead of a literal nine.
    let draw = fn_body(ui_src(), "fn draw_adaptive_rows_warn");
    assert!(
        draw.contains("line_height_sized(FontSize::Small)"),
        "{draw}"
    );
    assert!(draw.contains("hbedit::list_window_rows"), "{draw}");
    let map = fn_body(ui_src(), "fn draw_save_map");
    assert!(map.contains("hbedit::list_window_rows"), "{map}");
    assert!(!map.contains("const VIS: usize = 9"), "{map}");
}

// ---------------------------------------------------------------------------
// 15: no UiAction is left unwired
// ---------------------------------------------------------------------------

/// Nine arms were dispatched but never constructed: a button that is not on any
/// screen. The rule is mechanical - every variant has a construction site.
#[test]
fn no_ui_action_is_left_unwired() {
    let ui = ui_src();
    let editor = editor_src();
    let start = ui.find("pub enum UiAction {").expect("the enum");
    let open = start + ui[start..].find('{').expect("its body");
    let mut depth = 0i32;
    let mut close = open;
    for (i, c) in ui[open..].char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                close = open + i;
                break;
            }
        }
    }
    let body = &ui[open + 1..close];
    let mut variants = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if !t.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        let mut name = String::new();
        for c in t.chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                name.push(c);
            } else {
                break;
            }
        }
        let rest = &t[name.len()..];
        if rest.starts_with('(') || rest.starts_with(',') || rest.starts_with('{') {
            variants.push(name);
        }
    }
    assert!(
        variants.len() > 60,
        "the enum has {} variants",
        variants.len()
    );

    let code = format!("{ui}\n{editor}");
    let mut dead = Vec::new();
    for name in &variants {
        let needle = format!("UiAction::{name}");
        let mut at = 0;
        let mut built = 0;
        while let Some(rel) = code[at..].find(&needle) {
            at += rel + needle.len();
            let after = &code[at..];
            if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            // A match arm is UiAction::X => or UiAction::X(..) =>; anything
            // else is a construction (a hit box, or a key the console sends).
            let rest = after.trim_start();
            let is_arm = if let Some(r) = rest.strip_prefix('(') {
                r.find(')')
                    .map(|i| r[i + 1..].trim_start().starts_with("=>"))
                    .unwrap_or(false)
            } else {
                rest.starts_with("=>")
            };
            if !is_arm {
                built += 1;
            }
        }
        if built == 0 {
            dead.push(name.clone());
        }
    }
    assert!(
        dead.is_empty(),
        "these UiActions are dispatched but never constructed: {dead:?}"
    );
}
