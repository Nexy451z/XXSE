//! The shoulder band is one rule, and the number keyboards moved to [ZR] held.
//!
//! The report from the machine: on the guild card and the hunt record, [R]
//! opened a keyboard where [L] moved a tab, so the hunt record could not reach
//! the guild card at all - the same gesture meant two things depending on which
//! shoulder was used. The cause was one table: `Editor::shoulder_job` answered
//! `Shoulder::Screen` for [R] on seven screens (the item box, the hunt record,
//! the guild card, the reward rows, the quest editor's two item tabs, the
//! progress screen's quest view, the equipment form and the item set form).
//!
//! The fix is one rule - both shoulders are the tab keys on every screen, and no
//! screen can take one back - plus one key: the *held* [ZR] carries the jobs [R]
//! used to have (`Editor::zr_hold_job` / `zr_hold_press`). These tests pin both
//! halves, and the two walks the report asked for: 狩猟記録 → ギルドカード with
//! the shoulders, and the guild card's four views with the same pair.
//!
//! The console's dispatch is not part of this workspace, so the three tests that
//! need it read switch/src/ui.rs and switch/src/editor.rs as text: what they pin
//! is the *wiring* (the console asks the state machine and runs the hold), not
//! the pixels.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::hbedit::{
    self, Editor, Key, Screen, Shoulder, ZrHold, GUILD_TAB_ARENA, GUILD_TAB_CARD,
    GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, PROGRESS_QUESTS, TEXT_GUILD_HR,
    TEXT_GUILD_QUEST_COUNT, TEXT_GUILD_WEAPON_COUNT, TEXT_HUNT_COUNT, TEXT_ITEM_COUNT,
    TEXT_REWARD_COUNT,
};
use mhnx_core::save;

const UI_SRC: &str = include_str!("../../../switch/src/ui.rs");
const EDITOR_SRC: &str = include_str!("../../../switch/src/editor.rs");
const HBEDIT_SRC: &str = include_str!("../src/hbedit.rs");

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn blank_save() -> save::Save {
    let mut s = save::empty_switch_save();
    s.set_slot_used(0, true);
    s
}

fn editor(screen: Screen) -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(blank_save());
    ed.save_path = Some(String::from("memory:shoulder-rule"));
    ed.slot = 0;
    ed.screen = screen;
    ed
}

/// One item in the first pouch slot, so the count keyboard has a number to type.
fn save_with_item() -> save::Save {
    let mut s = blank_save();
    let mut items = s.read_item_box(0).expect("a blank box");
    items[0] = mhnx_core::itembox::ItemSlot { id: 1, count: 5 };
    s.write_item_box(0, &items).expect("write the box");
    s
}

const HASH_REM: u32 = 0x5B3C_302D;

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

fn arc_with_reward_table() -> Arc {
    Arc {
        version: ARC_VERSION,
        header8: 0xFFFF_FFFE,
        entries: vec![ArcEntry {
            name: String::from("quest\\rem\\rem_620114"),
            type_hash: HASH_REM,
            data: rem_bytes(),
            original: None,
            original_offset: None,
        }],
    }
}

/// Every screen the editor has, in the order the state machine declares them.
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

/// The body of one function in a console source, from its declaration to the
/// next item.
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

/// The labels of the palico section's sub-tab row, in the order L / R walk them.
fn palico_record_tabs() -> Vec<String> {
    let body = fn_body(UI_SRC, "fn sub_tabs_for(");
    let start = body
        .find("MainSection::PalicoRecord =>")
        .expect("the palico section's tab row");
    let tail = &body[start..];
    let end = tail
        .find("(Self::statics(labels), active)")
        .expect("the row's return");
    let row = &tail[..end];
    let mut out = Vec::new();
    let mut rest = row;
    while let Some(i) = rest.find('"') {
        let after = &rest[i + 1..];
        let Some(j) = after.find('"') else { break };
        out.push(after[..j].to_string());
        rest = &after[j + 1..];
    }
    out
}

/// The body of one tab press of the palico section's `select_sub_tab` arm.
fn palico_tab_arm(idx: usize) -> String {
    let body = fn_body(UI_SRC, "pub fn select_sub_tab(");
    let start = body
        .find("MainSection::PalicoRecord => match idx {")
        .expect("the palico section's tab presses");
    let tail = &body[start..];
    let open = format!("\n                {idx} =>");
    let at = tail
        .find(&open)
        .unwrap_or_else(|| panic!("tab {idx} has no arm:\n{tail}"));
    let rest = &tail[at + open.len()..];
    // The arm ends at the next line that is indented like an arm (16 spaces)
    // and starts with an arm: a digit or the catch-all. The body of a
    // multi-line arm is indented four spaces deeper, so it does not match.
    let mut end = rest.len();
    let mut from = 0usize;
    while let Some(i) = rest[from..].find("\n                ") {
        let at_line = from + i + 1 + 16;
        let next = rest[at_line..].chars().next().unwrap_or(' ');
        if next.is_ascii_digit() || next == '_' {
            end = from + i;
            break;
        }
        from = at_line;
    }
    rest[..end].to_string()
}

// ---------------------------------------------------------------------------
// 1. The band: one answer for both keys, on every screen
// ---------------------------------------------------------------------------

/// The rule the report asked for: [L] and [R] are the tab keys, on every screen
/// and both ways round. No screen may answer `Shoulder::Screen` - the variant is
/// gone, so a screen that took the key back would not compile.
#[test]
fn every_screen_answers_a_shoulder_with_the_tab_move() {
    let pick_lists = [
        Screen::ItemPick,
        Screen::SkillPick,
        Screen::JewelPick,
        Screen::EquipPick,
    ];
    for screen in ALL_SCREENS {
        let ed = editor(*screen);
        let left = ed.shoulder_job(Key::L);
        let right = ed.shoulder_job(Key::R);
        assert_eq!(
            left, right,
            "{screen:?}: the two shoulders of one band must answer the same"
        );
        let expected = if pick_lists.contains(screen) {
            Shoulder::Move
        } else {
            Shoulder::SubTab
        };
        assert_eq!(left, expected, "{screen:?}");
    }

    // The four pick lists are the ten-row step, and they answer it on both
    // keys - the same pair the arrows take.
    for screen in pick_lists {
        let ed = editor(screen);
        assert_eq!(ed.shoulder_job(Key::L), Shoulder::Move, "{screen:?}");
        assert_eq!(ed.shoulder_job(Key::R), Shoulder::Move, "{screen:?}");
    }

    // A form that owns the screen takes neither: a tab move under it would
    // leave the form open on a screen that no longer shows it.
    for ed in [
        {
            let mut ed = editor(Screen::Sets);
            ed.set_edit = Some(hbedit::SetEdit::default());
            ed
        },
        {
            let mut ed = editor(Screen::Sets);
            ed.item_set_edit = Some(hbedit::ItemSetEdit::default());
            ed
        },
        {
            let mut ed = editor(Screen::Roster);
            ed.palico_equip = Some(hbedit::PalicoEquip {
                palico: 0,
                field: 0,
                pick: [hbedit::PALICO_EQUIP_NONE; hbedit::PALICO_EQUIP_ROWS],
                clear_armed: None,
            });
            ed
        },
    ] {
        assert_eq!(ed.shoulder_job(Key::L), Shoulder::Swallow);
        assert_eq!(ed.shoulder_job(Key::R), Shoulder::Swallow);
    }

    // The one panel that reads the shoulders itself keeps them: it steps its
    // candidate list ten at a time, and the tab strip under it must not move.
    let mut ed = editor(Screen::Roster);
    ed.palico_skill = Some(hbedit::PalicoSkill {
        palico: 0,
        tab: 0,
        field: 0,
        picks: [0u8; hbedit::PALICO_MOVE_ROWS],
    });
    assert_eq!(ed.shoulder_job(Key::L), Shoulder::Panel);
    assert_eq!(ed.shoulder_job(Key::R), Shoulder::Panel);
    assert_eq!(ed.zr_hold_job(), ZrHold::None, "and no hold behind [ZR]");
}

/// The variant is gone: a screen cannot take a shoulder back any more, which is
/// a compile-time guarantee rather than a test - this pins that it stays gone.
#[test]
fn no_screen_takes_a_shoulder_back() {
    // The variant's name, spelled out of two pieces so that this test's own
    // source is not the match it is looking for.
    let gone = concat!("Shoulder::", "Screen");
    let src = HBEDIT_SRC;
    assert!(
        !src.contains(gone),
        "the variant that made [R] a keyboard must stay gone"
    );
    assert!(
        src.contains("pub enum Shoulder {"),
        "the enum is still the answer to \"whose key is this\""
    );
    // The console asks it, and the answer is the only one it uses.
    assert!(UI_SRC.contains("ed.shoulder_job(key)"));
    assert!(!UI_SRC.contains(gone));
}

// ---------------------------------------------------------------------------
// 2. 狩猟記録 -> ギルドカード with one shoulder
// ---------------------------------------------------------------------------

/// The walk the report could not make: from the hunt record, one shoulder press
/// lands on the guild card.
#[test]
fn the_hunt_record_reaches_the_guild_card_with_one_shoulder() {
    // The state machine: [R] on the hunt record is the tab move, so the console
    // runs its sub-tab walk instead of opening a keyboard.
    let mut ed = editor(Screen::Hunts);
    ed.hunt_show_all = true;
    ed.open_hunts();
    assert_eq!(ed.shoulder_job(Key::R), Shoulder::SubTab);
    assert_eq!(ed.shoulder_job(Key::L), Shoulder::SubTab);

    // The bar the console walks: the palico section's six tabs, with the hunt
    // record immediately before the guild card.
    let tabs = palico_record_tabs();
    assert_eq!(tabs.len(), 6, "the palico section has six tabs: {tabs:?}");
    let hunts = tabs
        .iter()
        .position(|t| t == "狩猟記録")
        .unwrap_or_else(|| panic!("no 狩猟記録 tab: {tabs:?}"));
    assert_eq!(
        tabs.get(hunts + 1).map(String::as_str),
        Some("ギルドカード"),
        "the guild card is the tab after the hunt record: {tabs:?}"
    );

    // And the press on that tab really lands on the card.
    let arm = palico_tab_arm(hunts + 1);
    assert!(arm.contains("Screen::Guild"), "the tab opens the card: {arm}");
    assert!(
        arm.contains("GUILD_TAB_CARD"),
        "and it opens the card view, not whichever view was last: {arm}"
    );
}

// ---------------------------------------------------------------------------
// 3. The guild card's four views
// ---------------------------------------------------------------------------

/// The card carries four views, and the shoulders walk them: they are the bar's
/// tabs while that screen is showing (the section bar is ZL / ZR).
#[test]
fn the_guild_card_walks_its_four_views_with_the_shoulders() {
    // The bar names the four views, in the order the shoulders walk them.
    let body = fn_body(UI_SRC, "fn sub_tabs_for(");
    let start = body
        .find("if ed.screen == Screen::Guild {")
        .expect("the card's own view row");
    let tail = &body[start..];
    let end = tail
        .find("return (Self::statics(labels), active);")
        .expect("the row's return");
    let row = &tail[..end];
    for label in ["カード", "回数", "武器", "闘技大会"] {
        assert!(row.contains(label), "the bar must name {label}: {row}");
    }

    // Every view answers the shoulders with the tab move, so the same pair
    // walks the four - and neither of them opens a keyboard.
    let mut ed = editor(Screen::Guild);
    for view in [GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, GUILD_TAB_ARENA] {
        ed.set_guild_view(view);
        assert_eq!(ed.shoulder_job(Key::L), Shoulder::SubTab, "view {view}");
        assert_eq!(ed.shoulder_job(Key::R), Shoulder::SubTab, "view {view}");
    }

    // A press on one of the four switches the view (the bar's own arm).
    let body = fn_body(UI_SRC, "pub fn select_sub_tab(");
    let start = body
        .find("MainSection::PalicoRecord if ed.screen == Screen::Guild")
        .expect("the card's own tab presses");
    let tail = &body[start..];
    for (tab, view) in [
        ("1 =>", "GUILD_TAB_COUNTS"),
        ("2 =>", "GUILD_TAB_WEAPONS"),
        ("3 =>", "GUILD_TAB_ARENA"),
    ] {
        let at = tail.find(tab).unwrap_or_else(|| panic!("no arm {tab}"));
        let rest = &tail[at..];
        let end = rest[1..].find("\n                ").map(|i| i + 1).unwrap_or(rest.len());
        let arm = &rest[..end];
        assert!(arm.contains(view), "the {tab} tab opens {view}: {arm}");
    }

    // The section bar is the other pair: ZL / ZR, not the shoulders.
    assert!(
        EDITOR_SRC.contains("cycle_sidebar_tab(&mut ed, false)")
            && EDITOR_SRC.contains("cycle_sidebar_tab(ed, true)"),
        "ZL / ZR are the section keys"
    );
}

// ---------------------------------------------------------------------------
// 4. The number keyboards are one held [ZR] away
// ---------------------------------------------------------------------------

/// The jobs [R] used to carry are reachable from the pad again - on the held
/// [ZR], which is the key those screens already own for their second action.
#[test]
fn the_number_keyboards_are_reachable_from_the_held_zr() {
    // アイテムBOX: the count of the slot under the cursor.
    let mut ed = Editor::new();
    ed.save = Some(save_with_item());
    ed.save_path = Some(String::from("memory:shoulder-rule"));
    ed.screen = Screen::Items;
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press(), "the hold runs the screen's job");
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_ITEM_COUNT),
        "the item box's count keyboard: {}",
        ed.message
    );

    // 狩猟記録: the count of the highlighted monster.
    let mut ed = editor(Screen::Hunts);
    ed.hunt_show_all = true;
    ed.open_hunts();
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press());
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_HUNT_COUNT),
        "the hunt record's number keyboard: {}",
        ed.message
    );

    // ギルドカード: HR on the card view, a counter on 回数, a cell on 武器.
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_CARD);
    ed.guild_field = 0;
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press());
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_GUILD_HR),
        "the card's HR keyboard: {}",
        ed.message
    );
    ed.set_guild_view(GUILD_TAB_COUNTS);
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press());
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_GUILD_QUEST_COUNT),
        "the counter keyboard: {}",
        ed.message
    );
    ed.set_guild_view(GUILD_TAB_WEAPONS);
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press());
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_GUILD_WEAPON_COUNT),
        "the weapon grid's keyboard: {}",
        ed.message
    );

    // 報酬 / 支給品: the two cells of the row under the cursor.
    let mut ed = editor(Screen::EntryView);
    ed.arc = Some(arc_with_reward_table());
    ed.entry = 0;
    ed.entry_row_cursor = 0;
    ed.entry_field = 1;
    assert!(ed.entry_has_numbers());
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press());
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_REWARD_COUNT),
        "the reward row's count keyboard: {}",
        ed.message
    );

    // クエスト改造's 報酬 tab: the item search.
    let mut ed = editor(Screen::QuestEdit);
    ed.quest_set_tab(hbedit::QUEST_TAB_REWARDS);
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press());
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(hbedit::TEXT_QUEST_ITEM)
    );

    // 進行状況's quest view: the 既読 bit.
    let mut ed = editor(Screen::Progress);
    ed.open_progress();
    ed.progress_tab = PROGRESS_QUESTS;
    ed.set_progress_cursor(0);
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    let (_, seen, _) = ed.progress_quest_state(0);
    assert!(!seen);
    assert!(ed.zr_hold_press());
    let (_, seen, _) = ed.progress_quest_state(0);
    assert!(seen, "the hold writes the seen bit: {}", ed.message);

    // 装備編集: the id keyboard on the two identity rows.
    let mut ed = editor(Screen::EquipEdit);
    ed.equip_edit = Some(hbedit::EquipEdit {
        new: true,
        ..hbedit::EquipEdit::default()
    });
    ed.equip_edit.as_mut().expect("a form").field = 1;
    assert_eq!(ed.zr_hold_job(), ZrHold::Action);
    assert!(ed.zr_hold_press());
    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(hbedit::TEXT_EQUIP_ID)
    );
}

/// The footer names the key the pad really presses: one token, "ZR長" (ZR held),
/// on every screen that has the job - and no line promises [R] for it.
#[test]
fn the_footer_names_the_hold_and_never_r() {
    let cases: &[(Screen, &str)] = &[
        (Screen::Items, "[ZR長] 数値"),
        (Screen::Hunts, "[ZR長] 数値"),
        (Screen::Guild, "[ZR長] 数値"),
        (Screen::EntryView, "[ZR長] 数値"),
        (Screen::Progress, "[ZR長] 既読"),
        (Screen::EquipEdit, "[ZR長] ID入力"),
    ];
    for (screen, needle) in cases {
        let mut ed = editor(*screen);
        match screen {
            Screen::Guild => ed.set_guild_view(GUILD_TAB_CARD),
            Screen::EntryView => {
                ed.arc = Some(arc_with_reward_table());
                ed.entry = 0;
            }
            Screen::Progress => {
                ed.open_progress();
                ed.progress_tab = PROGRESS_QUESTS;
            }
            Screen::EquipEdit => {
                ed.equip_edit = Some(hbedit::EquipEdit {
                    new: true,
                    ..hbedit::EquipEdit::default()
                });
            }
            _ => {}
        }
        let hint = ed.footer_hint();
        assert!(hint.contains(needle), "{screen:?}: {hint}");
        assert!(
            !hint.contains("[R]"),
            "{screen:?}: no line promises [R] any more: {hint}"
        );
    }
}

// ---------------------------------------------------------------------------
// 5. The hold is nothing where the screen has no second job
// ---------------------------------------------------------------------------

/// A held [ZR] on a screen with no such job does nothing at all - it does not
/// type, and it does not fall through to some other key.
#[test]
fn the_hold_does_nothing_where_there_is_no_second_job() {
    for screen in [
        Screen::SaveList,
        Screen::Slots,
        Screen::Char,
        Screen::Charms,
        Screen::Equip,
        Screen::Csv,
        Screen::Options,
        Screen::SaveMap,
        Screen::Review,
        Screen::Convert,
        Screen::Mib,
        Screen::Gmd,
        Screen::Check,
        Screen::ArcList,
        Screen::SaveQuests,
        Screen::Roster,
        Screen::Palico,
        Screen::QuestEquip,
    ] {
        let mut ed = editor(screen);
        assert_eq!(ed.zr_hold_job(), ZrHold::None, "{screen:?}");
        assert!(!ed.zr_hold_press(), "{screen:?}: the hold ran something");
        assert!(
            ed.take_text_request().is_none(),
            "{screen:?}: the hold opened a keyboard"
        );
    }

    // The arena view is the card's one view with no number to type, and the
    // item box's bulk menu owns the screen while it is open.
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_ARENA);
    assert_eq!(ed.zr_hold_job(), ZrHold::None);
    assert!(!ed.zr_hold_press());
    let mut ed = editor(Screen::Items);
    ed.open_item_menu();
    assert_eq!(ed.zr_hold_job(), ZrHold::None, "the menu owns the screen");
    assert!(!ed.zr_hold_press());
}

// ---------------------------------------------------------------------------
// 6. The console: the tap waits for the release, the hold is answered once
// ---------------------------------------------------------------------------

/// The console's half: [ZR]'s tap waits for the release on the screens that have
/// a hold (so a press that lingers is the hold and never also the tap), and the
/// hold runs the state machine's method.
#[test]
fn the_console_answers_the_zr_hold_before_the_tap() {
    // One tap object per key, the same shape [R] uses.
    assert!(
        EDITOR_SRC.contains("let mut zr_tap = mhnx_core::hbedit::ShoulderTap::new();"),
        "the hold is a ShoulderTap, like [R]'s 詳細"
    );
    assert!(EDITOR_SRC.contains("zr_tap.press()"), "the press arms it");
    assert!(EDITOR_SRC.contains("zr_tap.tick()"), "the hold is ticked");
    assert!(EDITOR_SRC.contains("zr_tap.release()"), "the tap runs on the release");
    assert!(
        EDITOR_SRC.contains("zr_tap.spend()"),
        "a screen with no hold answers its tap on the press, the way a pick list does"
    );
    // The hold is the state machine's, and the tap is the section bar or the
    // screen's own second action.
    assert!(
        EDITOR_SRC.contains("ed.zr_hold_press()"),
        "the hold runs Editor::zr_hold_press"
    );
    assert!(
        EDITOR_SRC.contains("ed.zr_hold_job() == mhnx_core::hbedit::ZrHold::None"),
        "the console asks whether the screen has a hold before it waits"
    );
    assert!(
        EDITOR_SRC.contains("fn zr_tap_job("),
        "the tap is one function, so the press and the release run the same thing"
    );
    // The modal branches forget the press, so a key that came up inside the
    // keyboard cannot reach the screen behind it later.
    assert_eq!(
        EDITOR_SRC
            .matches("\n                zr_tap = mhnx_core::hbedit::ShoulderTap::new();")
            .count(),
        3,
        "the keyboard, the picker and the review each forget it"
    );
    assert!(
        EDITOR_SRC.contains("zr_tap.cancel();"),
        "another key takes the press back"
    );

    // The touch route: the 数値入力 pill runs the same method, not a bare [R]
    // (which is a tab key now, so the pill would move the tab instead).
    assert!(
        UI_SRC.contains("UiAction::ZrHold"),
        "the pill is an action of its own"
    );
    assert!(
        UI_SRC.contains("ed.zr_hold_press()"),
        "and it runs the state machine's method"
    );
    assert!(
        !UI_SRC.contains("UiAction::Key(Key::R)"),
        "no button sends [R] any more - it would move the tab strip"
    );
    assert_eq!(
        UI_SRC.matches("UiAction::ZrHold").count(),
        4,
        "the action, its handler and the three pills (inspector, reward panel, card)"
    );
}
