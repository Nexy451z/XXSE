//! The seventh review: the places that named a key, and the guide strip that
//! printed a position the list was not showing.
//!
//! The sixth review put every key meaning in one table (hbedit::Editor::
//! key_facts) and made the footer, the guide and the deck read it. It left the
//! *buttons* alone: the equipment box's deck said "[＋] 新規装備を追加", the two
//! guild inspectors said "[A] 編集を開始", the review panel said "[B] 戻る", the
//! guild card's warnings said "[Y] クエスト回数 → 武器使用数", the bulk menu's
//! heading said "もう一度 [A] で実行（[B] 中止）" - one press, described by the
//! footer and by the glass in two vocabularies, and the guide strip printed a
//! position the list it sat above was not showing.
//!
//! What this file pins, item by item:
//!
//! * no draw function names a key (A1) and the scan covers the full-width [＋]
//!   the old one missed (A2);
//! * the strip's position is the drawn list's (B3), its search is every
//!   screen's (B4), it names the armed offer (B5, B6), it keeps the state when
//!   the banner takes the room (B7) and it does not repeat 未保存 (B8);
//! * the way out is not the back key (C9), the pickers draw no save pill (C10),
//!   the reward tab names the arrow pair again (C11), the guild edit mode's
//!   button says what it does (C12) and the talisman form can show its second
//!   press (C13);
//! * one job is one item (D14, D15, D16);
//! * the discard word follows the draft (E17);
//! * the footer is the modal's footer (F18) and the keyboard's legend is on its
//!   own row (F19);
//! * the README says what the build does (G20, G21, G22).
//!
//! Every test here is a reproduction: it fails on the commit before this one.

use mhnx_core::hbedit::{
    self, caption_width, Editor, ItemSetOp, Key, PalicoEquip, Screen, Shoulder, FOOTER_HINT_ROOM,
    GUILD_TAB_ARENA, GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, QUEST_TAB_REWARDS,
    QUEST_TAB_SUPPLIES,
};
use mhnx_core::save;

const UI_SRC: &str = include_str!("../../../switch/src/ui.rs");
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
    ed.save_path = Some(String::from("memory:review-24"));
    ed.slot = 0;
    ed.screen = screen;
    ed
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

/// Every fn draw_<name> in the console, as (name, body).
fn draw_functions(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find("fn draw_") {
        let tail = &rest[at..];
        let name: String = tail[8..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            rest = &tail[8..];
            continue;
        }
        let body = fn_body(tail, &format!("fn draw_{name}"));
        out.push((name, String::from(body)));
        let advance = tail.find('{').map(|i| i + 1).unwrap_or(1);
        rest = &tail[advance..];
    }
    out
}

/// The pad keys a surface must never name, as the footer brackets them.
///
/// The seventh review's own finding: the old list carried the ASCII [+], and
/// the deck printed the full-width [＋], so the scan walked past the one button
/// it was written to catch.
const PAD_KEYS: [&str; 16] = [
    "[A]", "[B]", "[X]", "[Y]", "[ZR]", "[ZL]", "[L]", "[R]", "[-]", "[+]", "[＋]",
    "[◀/▶]", "[▲/▼]", "[L/R]", "[ZL/ZR]", "[◀▶]",
];

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
// A1. No draw function names a key
// ---------------------------------------------------------------------------

/// The buttons, headings and warnings print the action; the footer prints the
/// key.
///
/// The seventh review found eight surfaces that named a key in their own words
/// (the equipment box's deck, both guild inspectors, the review panel, the
/// guild card's two warning strips, the bulk menu's heading, the sub-bar's two
/// buttons and the text build's body lines), each disagreeing with the footer
/// about one press.
#[test]
fn no_draw_function_names_a_key() {
    let mut checked = 0usize;
    for (name, body) in draw_functions(UI_SRC) {
        // The footer is the one surface that names keys.
        if name == "draw_footer" {
            continue;
        }
        checked += 1;
        for (line_no, line) in body.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || !line.contains('"') {
                continue;
            }
            for key in PAD_KEYS {
                assert!(
                    !line.contains(key),
                    "{name} names {key} (body line {line_no}): {}",
                    line.trim()
                );
            }
        }
    }
    assert!(checked > 40, "the scan read the draw functions: {checked}");

    // The text build's body is the same rule: the chip row under it is the
    // table, and a second copy inside the body drifted (it said "Y 既読" long
    // after [Y] became the tab key).
    let lines = fn_body(HBEDIT_SRC, "pub fn lines(&self) -> Vec<Line>");
    for line in lines.lines() {
        let code = line.trim_start();
        if code.starts_with("//") || !line.contains('"') {
            continue;
        }
        for key in ["[A]", "[B]", "[X]", "[Y]", "[ZR]", "[-]", "[＋]"] {
            assert!(
                !line.contains(key),
                "the text build's body names {key}: {}",
                line.trim()
            );
        }
    }
}

/// The scan covers the full-width [＋] the deck really printed.
///
/// The old list had the ASCII plus and the button said "[＋] 新規装備を追加", so
/// the test passed over the one literal it existed for.
#[test]
fn the_key_scan_covers_the_full_width_plus() {
    assert!(PAD_KEYS.contains(&"[＋]"), "the scan knows the full-width plus");
    assert!(
        !UI_SRC.contains("[＋] 新規装備を追加"),
        "the equipment box's deck names [＋]"
    );
    // And the word it prints now comes from the table.
    let deck = fn_body(UI_SRC, "fn draw_studio_inspector");
    assert!(
        deck.contains("pill_word(ed, \"＋\", \"新規\")"),
        "the deck borrows the table's word"
    );
}

// ---------------------------------------------------------------------------
// B3. The strip's position is the drawn list's
// ---------------------------------------------------------------------------

/// The three screens whose position was a different list's.
///
/// 進行状況 printed "1 / 132" wherever the cursor was (the row came from a field
/// no key wrote), マイセット printed "6 / 64" over a list of zero rows (both
/// tabs added, the console's row ignored) and キャラ printed "13 / 17" beside a
/// heading that said "3 / 7 件" (the whole field table against the tab's seven
/// rows).
#[test]
fn the_guide_position_is_the_drawn_list() {
    // 進行状況: the row the cursor is on, in the rows the search left.
    let mut ed = editor(Screen::Progress);
    ed.set_progress_cursor(100);
    assert_eq!(ed.status_position().as_deref(), Some("101 / 132"));
    ed.set_progress_cursor(131);
    assert_eq!(ed.status_position().as_deref(), Some("132 / 132"));

    // マイセット: an empty save has no row on either tab, so the strip has no
    // place to print - it used to print "6 / 64" over the empty list.
    let mut ed = editor(Screen::Sets);
    ed.cursor = 5;
    assert_eq!(ed.sets_rows(), 0, "an empty save has no set rows");
    assert_eq!(ed.status_position(), None, "no rows, no position");
    assert_eq!(ed.active_list_len(), 0, "the list the console draws");

    // キャラ: the tab's window, which is what the heading counts.
    let mut ed = editor(Screen::Char);
    ed.field = 12;
    assert_eq!(ed.status_position().as_deref(), Some("3 / 7"));
    ed.field = 3;
    assert_eq!(ed.status_position().as_deref(), Some("4 / 10"));
    assert_eq!(ed.char_window(), (0, 10));
    ed.field = 16;
    assert_eq!(ed.char_window(), (10, 17));
    assert_eq!(ed.status_position().as_deref(), Some("7 / 7"));

    // The console draws the same window: one implementation, two readers.
    let draw = fn_body(UI_SRC, "fn draw_char");
    assert!(
        draw.contains("ed.char_window()"),
        "the character list is built from the same range"
    );
    // And the second implementation is gone.
    assert!(
        !HBEDIT_SRC.contains("fn status_row_count"),
        "the guide's own row count is deleted"
    );
    assert!(
        !HBEDIT_SRC.contains("fn status_cursor"),
        "the guide's own cursor is deleted"
    );
    assert!(
        !UI_SRC.contains("fn list_len_of") && !UI_SRC.contains("fn active_cursor_of"),
        "the console's copy is deleted: one implementation"
    );
}

// ---------------------------------------------------------------------------
// B4. The search is on every screen that has one
// ---------------------------------------------------------------------------

/// 進行状況 and 生データ both narrow their list and both name the key that does
/// it, and the strip said nothing about the search on either.
#[test]
fn the_guide_names_the_search_on_every_screen_that_has_one() {
    let mut ed = editor(Screen::Progress);
    ed.apply_progress_filter(String::from("1"));
    assert_eq!(ed.active_search(), Some("1"));
    assert!(
        ed.screen_guide().contains("検索「1」"),
        "{}",
        ed.screen_guide()
    );
    // The rows the search left are the rows the strip counts.
    let shown = ed.progress_rows().len();
    assert!(shown > 0 && shown < 132, "the search narrowed the list: {shown}");
    let pos = ed.status_position().unwrap_or_default();
    assert!(
        pos.ends_with(&format!("/ {shown}")),
        "{pos} over {shown} rows"
    );

    let mut ed = editor(Screen::SaveMap);
    ed.map_filter = String::from("装備");
    assert_eq!(ed.active_search(), Some("装備"));
    assert!(
        ed.screen_guide().contains("検索「装備」"),
        "{}",
        ed.screen_guide()
    );
}

// ---------------------------------------------------------------------------
// B5. Every two-press offer is on the strip
// ---------------------------------------------------------------------------

/// The five offers the strip did not know about.
///
/// armed_offer listed eleven of the sixteen states the state machine can be in,
/// so the one place that says "a second press is waiting" was silent for the
/// equipment form's type change, the item set's two operations, the palico
/// panel's release and the two presets.
#[test]
fn the_armed_offer_covers_every_two_press_state() {
    let cases: Vec<(&str, Box<dyn Fn(&mut Editor)>)> = vec![
        (
            "装備の種別変更",
            Box::new(|ed: &mut Editor| ed.equip_kind_armed = Some((0, 1))),
        ),
        (
            "アイテムセットの操作",
            Box::new(|ed: &mut Editor| ed.item_set_armed = Some(ItemSetOp::FromPouch)),
        ),
        (
            "部位の解除",
            Box::new(|ed: &mut Editor| {
                ed.palico_equip = Some(PalicoEquip {
                    palico: 0,
                    field: 0,
                    pick: [hbedit::PALICO_EQUIP_NONE; hbedit::PALICO_EQUIP_ROWS],
                    clear_armed: Some(0),
                });
            }),
        ),
        (
            ".mib プリセット",
            Box::new(|ed: &mut Editor| ed.mib_preset_armed = Some(0)),
        ),
        (
            ".sem プリセット",
            Box::new(|ed: &mut Editor| ed.sem_preset_armed = Some(0)),
        ),
    ];
    for (what, set) in cases {
        let mut ed = editor(Screen::Hunts);
        set(&mut ed);
        let parts = ed.status_parts();
        assert!(
            parts.iter().any(|p| p.contains("2回目待ち")),
            "{what}: the strip says nothing about the waiting press: {parts:?}"
        );
        assert!(
            parts.iter().any(|p| p.contains(what)),
            "{what}: the strip names another offer: {parts:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// B6. The strip says what is waiting
// ---------------------------------------------------------------------------

/// "2回目待ち" alone left the reader to guess which of the sixteen offers was
/// armed; the message that armed it is the footer's second line, which a long
/// message pushes out of sight.
#[test]
fn the_armed_offer_says_what_is_waiting() {
    let mut ed = editor(Screen::Charms);
    ed.charm_delete_armed = Some(0);
    let parts = ed.status_parts();
    assert!(
        parts.iter().any(|p| p == "2回目待ち（護石の削除）"),
        "{parts:?}"
    );
    assert!(
        ed.screen_guide().contains("2回目待ち（護石の削除）"),
        "{}",
        ed.screen_guide()
    );
}

// ---------------------------------------------------------------------------
// B7. The banner takes the keys, not the state
// ---------------------------------------------------------------------------

/// While the update banner is up the strip is narrower, and the old renderer
/// took the tail of the whole line - so the banner deleted the state and kept
/// the two keys the footer already prints.
#[test]
fn the_banner_drops_the_keys_and_keeps_the_state() {
    let mut ed = editor(Screen::Items);
    ed.apply_item_box_filter(String::from("回復薬"));
    let state = ed.guide_state();
    let keys = ed.guide_keys();
    assert!(state.contains("検索「回復薬」"), "{state}");
    assert!(key_tokens(&state).is_empty(), "the state names no key: {state}");
    assert!(!key_tokens(&keys).is_empty(), "the key half names keys: {keys}");
    assert_eq!(
        ed.screen_guide(),
        format!("{state}{keys}"),
        "the strip is the two halves"
    );

    // The renderer prefers the state when the line does not fit.
    let draw = fn_body(UI_SRC, "fn draw_guide");
    assert!(draw.contains("ed.guide_state()"), "the strip reads the state");
    assert!(draw.contains("ed.guide_keys()"), "and the keys");
    let state_branch = draw
        .find("width(&state) <= room")
        .expect("the state-only branch");
    let tail = draw.find("guide_tail(&state").expect("the tail of the state");
    assert!(
        state_branch < tail,
        "the keys are dropped before the state is cut"
    );
    assert!(
        !draw.contains("guide_tail(&guide"),
        "the whole line is never the thing that is cut"
    );
}

// ---------------------------------------------------------------------------
// B8. 未保存 is not on the strip
// ---------------------------------------------------------------------------

/// The header pill and the footer's dot both say it; the strip is 28px of the
/// list's height.
#[test]
fn the_guide_does_not_repeat_the_unsaved_flag() {
    let mut ed = editor(Screen::Char);
    ed.dirty = true;
    assert!(!ed.screen_guide().contains("未保存"), "{}", ed.screen_guide());
    assert!(!ed.status_parts().iter().any(|p| p == "未保存"));
    // The two surfaces that do say it still do.
    assert!(UI_SRC.contains("String::from(\"未保存\")"), "the header pill");
}

// ---------------------------------------------------------------------------
// C9. The way out is not the back key
// ---------------------------------------------------------------------------

/// "※終了は [B]" sat on the same line as "[B] スロット", and [B] only went back
/// one screen: the note promised the session would end and it did not.
#[test]
fn the_way_out_note_does_not_name_the_back_key() {
    for screen in [Screen::Charms, Screen::Equip, Screen::Roster] {
        let ed = editor(screen);
        assert!(ed.plus_is_a_tool(), "{screen:?}: [＋] is a tool here");
        let hint = ed.footer_hint();
        assert!(
            !hint.contains("※終了は [B]"),
            "{screen:?}: the note names [B]: {hint}"
        );
        assert!(
            hint.contains("終了は一覧から"),
            "{screen:?}: the way out is named: {hint}"
        );
        // And the [B] item on the same line is its destination.
        assert!(hint.contains("[B] スロット"), "{screen:?}: {hint}");
    }
}

// ---------------------------------------------------------------------------
// C10. The pickers draw no save pill
// ---------------------------------------------------------------------------

/// The pill ran quick_save on the four pick lists, where [X] clears the search:
/// a tap on it answered 書き込むセーブがありません while the key beside it
/// emptied the filter.
#[test]
fn the_pickers_draw_no_save_pill() {
    let nav = fn_body(UI_SRC, "fn draw_top_nav");
    assert!(
        nav.contains("Screen::ItemPick | Screen::SkillPick | Screen::JewelPick | Screen::EquipPick"),
        "the pill is skipped on the four pick lists"
    );
    assert!(
        !nav.contains("\"[X] 保存\"") && !nav.contains("\"[X] 未保存\""),
        "the pill names no key and the old literal is gone"
    );
    // The four screens really do answer [X] with the search.
    for screen in [
        Screen::ItemPick,
        Screen::SkillPick,
        Screen::JewelPick,
        Screen::EquipPick,
    ] {
        let ed = editor(screen);
        assert_eq!(label(&ed, "X"), "クリア", "{screen:?}: [X] is the search clear");
    }
}

// ---------------------------------------------------------------------------
// C11. The reward tab names the arrow pair again
// ---------------------------------------------------------------------------

/// 報酬 exists to change 個数 / 確率%, and the line named no key that changes a
/// number. The strip had 132px to spare (631 of 763).
#[test]
fn the_reward_tab_names_the_arrow_pair() {
    let mut ed = editor(Screen::QuestEdit);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    assert_eq!(label(&ed, "◀/▶"), "値");
    let hint = ed.footer_hint();
    assert!(hint.contains("[◀/▶] 値"), "{hint}");
    assert!(
        caption_width(&hint) <= FOOTER_HINT_ROOM,
        "{}px of {FOOTER_HINT_ROOM}px: {hint}",
        caption_width(&hint)
    );
    // The comment that said the pair could not fit is gone.
    assert!(
        !HBEDIT_SRC.contains("The arrow pair is not in this line either"),
        "the reason the pair was dropped was wrong"
    );
}

// ---------------------------------------------------------------------------
// C12. The guild edit mode's back button says what it does
// ---------------------------------------------------------------------------

/// In the two edit modes [B] closes the mode and stays on the view, so a button
/// that printed the destination's name ("ギルドカード") sent the user to a
/// screen the press does not reach.
#[test]
fn the_guild_edit_mode_names_the_back_button() {
    for (name, needle) in [
        ("weapon", "fn draw_guild_weapon_inspector"),
        ("arena", "fn draw_arena_inspector"),
    ] {
        let body = fn_body(UI_SRC, needle);
        assert!(
            body.contains("String::from(\"編集を終了\")"),
            "{name}: the button says what the press does"
        );
        assert!(
            body.contains("if ed.guild_wpn_edit") || body.contains("if ed.arena_edit"),
            "{name}: it is the edit mode's word"
        );
    }
    // The footer does not name [B] in either mode, so the button is the only
    // place the press is described - and it is described truly.
    let mut ed = editor(Screen::Guild);
    ed.set_guild_view(GUILD_TAB_WEAPONS);
    ed.guild_wpn_edit = true;
    assert!(
        !ed.key_facts().iter().any(|f| f.key == "B"),
        "[B] is not on the line in the edit mode"
    );
}

// ---------------------------------------------------------------------------
// C13. The talisman form can show its second press
// ---------------------------------------------------------------------------

/// The overwrite offer is only ever armed *while* the form holds a draft, so an
/// else-if made the one state the strip exists for unreachable there.
#[test]
fn the_talisman_form_can_show_the_second_press() {
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
    let form = ed.charm_edit.expect("a form");
    ed.charm_edit = Some(hbedit::CharmEdit {
        index: if form.index == 0 { 1 } else { 0 },
        // The box-slot row is the one [A] writes from; the form opens on a
        // skill row when the record has one.
        field: 0,
        points1: 5,
        ..form
    });
    ed.screen = Screen::CharmEdit;
    ed.input(Key::A);
    assert!(ed.charm_write_armed.is_some(), "the first [A] arms it");
    let parts = ed.status_parts();
    assert!(
        parts.iter().any(|p| p.contains("下書きあり")),
        "the draft is on the strip: {parts:?}"
    );
    assert!(
        parts.iter().any(|p| p.contains("2回目待ち（護石の上書き）")),
        "and so is the waiting press: {parts:?}"
    );
}

// ---------------------------------------------------------------------------
// D14. One search item on the supply tab
// ---------------------------------------------------------------------------

/// The [ZR] tap and the held [ZR] open the same keyboard on 支給品, so the line
/// printed 検索 twice for one job.
#[test]
fn one_search_item_on_the_supply_tab() {
    let mut ed = editor(Screen::QuestEdit);
    ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    let searches: Vec<String> = ed
        .key_facts()
        .into_iter()
        .filter(|f| f.label == hbedit::WORD_SEARCH)
        .map(|f| f.key.to_string())
        .collect();
    assert_eq!(searches, vec![String::from("ZR長")], "one search item");
    let hint = ed.footer_hint();
    assert_eq!(hint.matches("検索").count(), 1, "{hint}");
}

// ---------------------------------------------------------------------------
// D15. One save item on the options screen
// ---------------------------------------------------------------------------

/// [A] and [X] both end in request_write_save, so the line said 保存 twice and
/// a reader could not tell the two apart.
#[test]
fn one_save_item_on_the_options_screen() {
    let ed = editor(Screen::Options);
    let saves: Vec<String> = ed
        .key_facts()
        .into_iter()
        .filter(|f| f.label == hbedit::WORD_SAVE)
        .map(|f| f.key.to_string())
        .collect();
    assert_eq!(saves, vec![String::from("X")], "one save item");
    // [A] still saves - it is the same write, and the message says so.
    let mut ed = editor(Screen::Options);
    ed.input(Key::A);
    assert!(ed.take_write().is_some(), "[A] still writes the card");
}

// ---------------------------------------------------------------------------
// D16. One [Y] word on the guild views
// ---------------------------------------------------------------------------

/// [Y] walks the three views that hold the card's numbers, and the table said
/// 表示 / 回数/武器 / 武器 for the same press while the two warning strips said
/// "クエスト回数 → 武器使用数" and "武器使用数へ" in their own words.
///
/// The walk is the shoulder pair now - the four views are the bar's tabs
/// (sub_tabs_for) - so the line spends no item on [Y] at all: the tab strip is
/// on the glass and the footer would only repeat it. [Y] still walks them.
#[test]
fn one_y_word_on_the_guild_views() {
    let mut ed = editor(Screen::Guild);
    for view in [GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS] {
        ed.set_guild_view(view);
        assert_eq!(
            ed.key_label("Y"),
            None,
            "view {view}: the walk is the shoulders, not [Y]"
        );
        assert_eq!(ed.shoulder_job(Key::R), Shoulder::SubTab, "view {view}");
    }
    // The arena view's [Y] is the keyboard, and the table says so.
    ed.set_guild_view(GUILD_TAB_ARENA);
    assert_eq!(label(&ed, "Y"), "入力");

    // The two warning strips name no key.
    for needle in ["fn draw_guild(", "fn draw_guild_counts"] {
        let body = fn_body(UI_SRC, needle);
        for key in ["[Y]", "[R]", "[L]", "[B]"] {
            assert!(
                !body.contains(&format!("\"{key}")),
                "{needle} names {key} in its warning strip"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// E17. The discard word follows the draft
// ---------------------------------------------------------------------------

/// The word promised a second press on a form with no draft, and the second
/// press then ran the [B] of the screen underneath (護石編集 → 護石一覧 →
/// スロット).
#[test]
fn the_discard_word_follows_the_draft() {
    let mut ed = editor(Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert!(!ed.charm_draft_dirty(), "a fresh form holds no draft");
    assert_eq!(ed.b_word(), "護石一覧", "one press leaves");
    assert!(!ed.b_word().contains("2回で破棄"), "{}", ed.b_word());

    ed.charm_edit.as_mut().expect("a form").points1 = 5;
    assert!(ed.charm_draft_dirty(), "the form holds a draft now");
    assert_eq!(ed.b_word(), "護石一覧（2回で破棄）");
}

// ---------------------------------------------------------------------------
// F18. The footer is the modal's footer
// ---------------------------------------------------------------------------

/// The keyboard owns the pad while it is up, and the footer kept naming the
/// screen's keys: it said "[X] 保存" over a key that empties the field.
#[test]
fn the_footer_names_the_modal_keys() {
    // The keyboard's line fits the strip and names the keys whose job changed.
    for multiline in [false, true] {
        let hint = hbedit::osk_hint(multiline);
        assert!(
            caption_width(&hint) <= FOOTER_HINT_ROOM,
            "{}px of {FOOTER_HINT_ROOM}px: {hint}",
            caption_width(&hint)
        );
        assert!(hint.contains("[X] クリア（2回）"), "{hint}");
        assert!(hint.contains("[＋] 確定"), "{hint}");
        assert!(hint.contains("[B] 1字削除"), "{hint}");
        assert!(!hint.contains("保存"), "the screen's save key is gone: {hint}");
    }
    // One table for the legend, the buttons and the footer.
    assert!(UI_SRC.contains("hbedit::osk_hint"), "the footer reads it");
    assert!(UI_SRC.contains("hbedit::osk_facts"), "the legend reads it");
    assert!(UI_SRC.contains("hbedit::osk_word"), "the buttons read it");
    let footer = fn_body(UI_SRC, "fn draw_footer");
    assert!(footer.contains("self.modal_hint(ed)"), "the footer asks first");
    // The picker and the two dialogs are modals too.
    assert!(UI_SRC.contains("hbedit::mib_picker_hint"));
    assert!(UI_SRC.contains("hbedit::review_hint"));
    assert!(UI_SRC.contains("hbedit::update_hint"));
}

// ---------------------------------------------------------------------------
// F19. The keyboard's legend is on its own row
// ---------------------------------------------------------------------------

/// The legend grew to 884px and started at 272, so it was drawn over the title
/// (which starts at 124 and may run to 764).
#[test]
fn the_osk_legend_is_on_the_row_under_the_title() {
    let draw = fn_body(UI_SRC, "fn draw_osk");
    assert!(
        draw.contains("c.text_right(mx + mw - 24, my + 38, &legend"),
        "the legend is on the row under the title"
    );
    assert!(
        !draw.contains("my + 20, &legend"),
        "the legend is not on the title's row"
    );
    // The note shares the title's line instead of taking the legend's.
    assert!(
        draw.contains("let note_x = mx + 24 + title_w + 16;"),
        "the note sits right of the title"
    );
    // And the legend really is that wide: the old row could not hold it.
    let legend = hbedit::render_facts(&hbedit::osk_facts(true));
    assert!(
        caption_width(&legend) > 800,
        "the legend is {}px, which is why it needs its own row",
        caption_width(&legend)
    );
}

// ---------------------------------------------------------------------------
// G20. The README does not promise every key
// ---------------------------------------------------------------------------

/// "[B] と [X] は必ず出ます" was false: five lines have no [X] and three have no
/// [B].
#[test]
fn the_readme_does_not_promise_every_key() {
    assert!(
        !README_SRC.contains("必ず出ます"),
        "the promise is gone"
    );
    assert!(
        README_SRC.contains("出ない画面があります"),
        "the README says which screens drop them"
    );
    assert!(
        README_SRC.contains("保存できる画面だけ"),
        "and why [X] is missing on five lines"
    );
    // The count is real: five lines have no [X], three have no [B].
    let mut no_x = 0usize;
    let mut no_b = 0usize;
    for (name, ed) in [
        ("SaveList", editor(Screen::SaveList)),
        ("ArcList", editor(Screen::ArcList)),
        ("Convert", editor(Screen::Convert)),
        ("ItemPick+count", {
            let mut ed = editor(Screen::ItemPick);
            ed.item_pick_mode = 3;
            ed.item_pick_stage = 1;
            ed
        }),
        ("Csv+armed", {
            let mut ed = editor(Screen::Csv);
            ed.csv_armed = Some(0);
            ed
        }),
    ] {
        assert!(
            !ed.key_facts().iter().any(|f| f.key == "X"),
            "{name} has no [X]"
        );
        no_x += 1;
    }
    for (name, ed) in [
        ("SaveList", editor(Screen::SaveList)),
        ("Guild+weapons-edit", {
            let mut ed = editor(Screen::Guild);
            ed.set_guild_view(GUILD_TAB_WEAPONS);
            ed.guild_wpn_edit = true;
            ed
        }),
        ("Guild+arena-edit", {
            let mut ed = editor(Screen::Guild);
            ed.set_guild_view(GUILD_TAB_ARENA);
            ed.arena_edit = true;
            ed
        }),
    ] {
        assert!(
            !ed.key_facts().iter().any(|f| f.key == "B"),
            "{name} has no [B]"
        );
        no_b += 1;
    }
    assert_eq!((no_x, no_b), (5, 3), "the README's two counts");
}

// ---------------------------------------------------------------------------
// G21. The README's [B] words are the screens' names
// ---------------------------------------------------------------------------

/// The README said キャラ選択 / クエスト (.arc) / クエストの中身; the footer says
/// スロット / SD(.arc) / 構成(.arc).
#[test]
fn the_readme_b_words_are_the_screens_names() {
    for word in ["スロット", "SD(.arc)", "構成(.arc)"] {
        assert!(README_SRC.contains(word), "the README names {word}");
    }
    for wrong in ["クエスト (.arc)", "クエストの中身"] {
        assert!(!README_SRC.contains(wrong), "the old word is gone: {wrong}");
    }
    // And the README's words are the ones the footer prints.
    let ed = editor(Screen::Charms);
    assert_eq!(label(&ed, "B"), hbedit::screen_name(Screen::Slots));
    let ed = editor(Screen::SaveQuests);
    assert_eq!(label(&ed, "B"), "SD(.arc)");
    let ed = editor(Screen::Mib);
    assert_eq!(label(&ed, "B"), "構成(.arc)");
    // The discard suffix is conditional, and the README says so.
    assert!(
        README_SRC.contains("下書きがあるときだけ"),
        "the README says when the suffix appears"
    );
}

// ---------------------------------------------------------------------------
// G22. The README's keyboard words and the detail pill
// ---------------------------------------------------------------------------

/// The README said [X] 全消去 and [Y] 濁点 / 半濁点 / 小字 where the keyboard
/// prints クリア（2回） and 濁点/小字, and the PC build's [詳細] had no row in
/// the read-across table.
#[test]
fn the_readme_reads_across_the_detail_pill_and_the_osk_words() {
    assert!(!README_SRC.contains("全消去"), "the old clear word is gone");
    assert!(
        README_SRC.contains("クリア（2 回押し"),
        "the README prints the keyboard's word"
    );
    assert!(
        README_SRC.contains("濁点 / 小字"),
        "and the keyboard's [Y] word"
    );
    // The read-across table carries the detail pill.
    assert!(
        README_SRC.contains("[詳細]"),
        "the PC build's [詳細] is in the table"
    );
    assert!(
        README_SRC.contains("[生データ]"),
        "and so is the console's word for it"
    );
    // The two words are the ones the build prints.
    let ed = editor(Screen::Char);
    assert!(
        UI_SRC.contains("ed.detail_pill_label()"),
        "the pill prints the state machine's word"
    );
    assert_eq!(ed.detail_pill_label(), "[生データ]");
    // The keyboard's words are the table's.
    assert_eq!(hbedit::osk_word(false, "X"), "クリア（2回）");
    assert_eq!(hbedit::osk_word(false, "Y"), "濁点/小字");
    assert_eq!(hbedit::osk_word(false, "B"), "1字削除");
}

// ---------------------------------------------------------------------------
// The changed lines still fit
// ---------------------------------------------------------------------------

/// Every line this review changed still fits the 763px strip.
#[test]
fn the_changed_lines_still_fit() {
    let mut states: Vec<(&str, Editor)> = Vec::new();
    for screen in [
        Screen::Options,
        Screen::Charms,
        Screen::Equip,
        Screen::Roster,
        Screen::SaveList,
        Screen::ArcList,
        Screen::Convert,
        Screen::ItemPick,
        Screen::Guild,
        Screen::Progress,
        Screen::SaveMap,
        Screen::Review,
        Screen::Sets,
    ] {
        states.push((hbedit::screen_name(screen), editor(screen)));
    }
    for view in [GUILD_TAB_CARD, GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, GUILD_TAB_ARENA] {
        let mut ed = editor(Screen::Guild);
        ed.set_guild_view(view);
        states.push(("Guild", ed));
    }
    for tab in 0..4 {
        let mut ed = editor(Screen::QuestEdit);
        ed.quest_set_tab(tab);
        states.push(("QuestEdit", ed));
    }
    let mut ed = editor(Screen::Items);
    ed.open_item_menu();
    states.push(("Items+menu", ed));
    let mut ed = editor(Screen::Csv);
    ed.csv_armed = Some(0);
    states.push(("Csv+armed", ed));
    for (name, ed) in states {
        let hint = ed.footer_hint();
        assert!(
            caption_width(&hint) <= FOOTER_HINT_ROOM,
            "{name}: {}px of {FOOTER_HINT_ROOM}px: {hint}",
            caption_width(&hint)
        );
    }
}
