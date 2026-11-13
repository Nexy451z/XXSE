//! The third review of the console: eighteen findings, one test each (some
//! findings share a test where they share a cause).
//!
//! The console's dispatch cannot be driven from a host test, so what is pinned
//! here is the layer underneath it - the state machine, the strings it hands the
//! console, the window arithmetic - plus the wiring in switch/src/ui.rs,
//! switch/src/editor.rs and the PC build's app.rs, read as text. Every test
//! names the finding it belongs to, so a regression points at the code rather
//! than at the screen.
//!
//! Nothing here writes a card: every save is built in memory.

use mhnx_core::charm;
use mhnx_core::equip;
use mhnx_core::equipops::{self, EquipError};
use mhnx_core::hbedit::{
    self, Editor, EquipScope, Key, Request, Screen, TextSentinel, GMD_TEXT_MAX, QUEST_REWARD_CHANCE,
    QUEST_REWARD_COUNT, QUEST_REWARD_FIELDS, QUEST_REWARD_ITEM, QUEST_TAB_BASIC,
    QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, QUEST_TAB_SUPPLIES,
};
use mhnx_core::save::{self, Save};

// ---------------------------------------------------------------------------
// The three layers, as text
// ---------------------------------------------------------------------------

fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
}

fn editor_src() -> &'static str {
    include_str!("../../../switch/src/editor.rs")
}

fn gui_src() -> &'static str {
    include_str!("../../mhnx-gui/src/app.rs")
}

/// The state machine's source: the one table of screen names and key words.
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

/// A blank save with n talismans in box slots 0..n.
fn save_with_charms(n: usize) -> Save {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    {
        let b = save.equipment_box_mut(0).expect("the box exists");
        for i in 0..n {
            let o = i * charm::CHARM_RECORD;
            b[o..o + charm::CHARM_RECORD].copy_from_slice(&ten_record());
        }
    }
    save
}

fn editor_with(save: Save, screen: Screen) -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.save_path = Some(String::from("memory:review-16"));
    ed.slot = 0;
    ed.screen = screen;
    ed
}

fn editor_with_charms(n: usize) -> Editor {
    editor_with(save_with_charms(n), Screen::Charms)
}

fn editor_on_a_blank_save() -> Editor {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    editor_with(save, Screen::Slots)
}
// ---------------------------------------------------------------------------
// 1: the text sentinel table
// ---------------------------------------------------------------------------

/// Every sentinel the editor can queue is one row of the core's table: a title,
/// a width and an apply function. The three hand-written copies this replaces
/// are how TEXT_QUEST_ITEM came to be in the queue and in neither of the
/// console's two chains.
#[test]
fn every_text_sentinel_has_a_title_and_an_apply() {
    // Every constant, by name. A sentinel added to hbedit.rs has to be added
    // here too, which is the point: the list is the review's own count.
    let ids: [(&str, usize); 28] = [
        ("TEXT_NAME", hbedit::TEXT_NAME),
        ("TEXT_ITEM_FILTER", hbedit::TEXT_ITEM_FILTER),
        ("TEXT_SKILL_FILTER", hbedit::TEXT_SKILL_FILTER),
        ("TEXT_QUEST_FILTER", hbedit::TEXT_QUEST_FILTER),
        ("TEXT_SAVE_NAME", hbedit::TEXT_SAVE_NAME),
        ("TEXT_PALICO_NAME", hbedit::TEXT_PALICO_NAME),
        ("TEXT_GUILD_COMMENT", hbedit::TEXT_GUILD_COMMENT),
        ("TEXT_EQUIP_FILTER", hbedit::TEXT_EQUIP_FILTER),
        ("TEXT_JEWEL_FILTER", hbedit::TEXT_JEWEL_FILTER),
        ("TEXT_CHARM_FILTER", hbedit::TEXT_CHARM_FILTER),
        ("TEXT_QUEST_ITEM", hbedit::TEXT_QUEST_ITEM),
        ("TEXT_SET_NAME", hbedit::TEXT_SET_NAME),
        ("TEXT_ARENA_TIME", hbedit::TEXT_ARENA_TIME),
        ("TEXT_ITEM_COUNT", hbedit::TEXT_ITEM_COUNT),
        ("TEXT_GUILD_HR", hbedit::TEXT_GUILD_HR),
        ("TEXT_GUILD_TIME", hbedit::TEXT_GUILD_TIME),
        ("TEXT_HUNT_COUNT", hbedit::TEXT_HUNT_COUNT),
        ("TEXT_REWARD_PROB", hbedit::TEXT_REWARD_PROB),
        ("TEXT_REWARD_COUNT", hbedit::TEXT_REWARD_COUNT),
        ("TEXT_CARD_NAME", hbedit::TEXT_CARD_NAME),
        ("TEXT_EQUIP_PICK_FILTER", hbedit::TEXT_EQUIP_PICK_FILTER),
        ("TEXT_EQUIP_ID", hbedit::TEXT_EQUIP_ID),
        ("TEXT_ITEM_BOX_FILTER", hbedit::TEXT_ITEM_BOX_FILTER),
        ("TEXT_GUILD_QUEST_COUNT", hbedit::TEXT_GUILD_QUEST_COUNT),
        ("TEXT_GUILD_WEAPON_COUNT", hbedit::TEXT_GUILD_WEAPON_COUNT),
        ("TEXT_ITEM_SET_NAME", hbedit::TEXT_ITEM_SET_NAME),
        ("TEXT_SAVE_MAP_FILTER", hbedit::TEXT_SAVE_MAP_FILTER),
        ("TEXT_PROGRESS_FILTER", hbedit::TEXT_PROGRESS_FILTER),
    ];
    assert_eq!(
        hbedit::TEXT_SENTINELS.len(),
        ids.len(),
        "the table and the constants are the same set"
    );
    let mut ed = editor_on_a_blank_save();
    for (name, id) in ids {
        let row: &TextSentinel = hbedit::text_sentinel(id)
            .unwrap_or_else(|| panic!("{name} has no row in TEXT_SENTINELS"));
        // The title the keyboard would show, through the same call the console
        // makes.
        let title = ed.text_request_title(id);
        assert!(!title.is_empty(), "{name} has no keyboard title");
        // The field: a sentinel is one line and holds bytes; a name answers
        // from its own spec.
        let spec = ed.text_request_spec(id);
        assert!(spec.capacity > 0, "{name} has no width");
        if ed.text_request_name(id).is_none() {
            assert_eq!(row.max_bytes, spec.capacity, "{name} width");
            assert!(!spec.multiline, "{name} is a one-line field");
        }
        // The apply function runs and leaves a message behind (every one of
        // them says what it did or why it could not).
        (row.apply)(&mut ed, id, "");
        assert!(
            !ed.message.is_empty(),
            "{name} applied nothing and said nothing"
        );
    }
    // No id is answered by two rows.
    for a in hbedit::TEXT_SENTINELS {
        for b in hbedit::TEXT_SENTINELS {
            if !std::ptr::eq(a, b) {
                assert!(!b.holds(a.first), "two rows answer {}", a.first);
            }
        }
    }
}

/// The finding itself: the 報酬 / 支給品 search was queued, was titled
/// 「クエスト文章」 (the .gmd fallback) and wrote nothing at all.
#[test]
fn the_reward_search_is_titled_and_writes_the_filter() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::QuestEdit;
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    ed.request_quest_item_filter();
    let (idx, initial) = ed.take_text_request().expect("the search is queued");
    assert_eq!(idx, hbedit::TEXT_QUEST_ITEM);
    assert!(initial.is_empty());
    let title = ed.text_request_title(idx);
    assert_ne!(title, "クエスト文章", "the .gmd fallback is not a title here");
    assert!(title.contains("報酬"), "{title}");
    assert!(title.contains("検索"), "{title}");
    // 決定 writes the filter, and the list narrows.
    ed.apply_text_request(idx, "回復薬");
    assert_eq!(ed.quest_item_filter, "回復薬");
    assert!(ed.message.contains("回復薬"), "{}", ed.message);
    // An empty answer is the whole table again.
    ed.apply_text_request(idx, "");
    assert!(ed.quest_item_filter.is_empty());
    assert!(ed.message.contains("解除"), "{}", ed.message);
}
// ---------------------------------------------------------------------------
// 2: the equipment box could create a talisman and then not show it
// ---------------------------------------------------------------------------

/// One table answers "which kinds does this box hold", and the list, the 種別
/// list, the arrow keys, the picker and the create path all read it.
#[test]
fn the_equipment_box_cannot_create_a_talisman() {
    // The kind list the picker shows: 護石 is in it, because a slot of the box
    // can hold one and a list that cannot name what a slot holds is the bug the
    // screen was reported with.
    let choices = Editor::equip_kind_choices();
    assert_eq!(choices.len(), equip::BOX_KINDS.len());
    assert!(
        choices
            .iter()
            .any(|(k, _)| *k == u32::from(equip::TALISMAN_KIND)),
        "護石 has to be nameable: the box holds them"
    );
    // Listed is not creatable: the form has no row for a talisman's skills.
    assert!(equip::box_holds_kind(equip::TALISMAN_KIND));
    assert!(!equip::form_creates_kind(equip::TALISMAN_KIND));
    assert!(equip::form_creates_kind(7));
    // The create path.
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    assert_eq!(
        equipops::create_in_box(&mut save, 0, equip::TALISMAN_KIND, 1, 1),
        Err(EquipError::NotBoxKind(equip::TALISMAN_KIND))
    );
    assert!(
        equipops::create_in_box(&mut save, 0, 7, 40, 1).is_ok(),
        "a weapon is still creatable"
    );
    // The whole-box picker no longer offers one either.
    let all = hbedit::equip_choices(EquipScope::All);
    assert!(
        !all.iter().any(|c| c.kind == equip::TALISMAN_KIND),
        "the new-record picker cannot offer a talisman"
    );
    // The form's 種別 row refuses it, and says where a talisman is made.
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::Equip;
    ed.input(Key::Plus);
    assert!(ed.equip_edit.is_some(), "{}", ed.message);
    assert!(!ed.apply_equip_kind(equip::TALISMAN_KIND));
    assert!(ed.message.contains("護石一覧"), "{}", ed.message);
    // The arrow keys walk the box's kinds, and every kind they reach is one the
    // list can name.
    // The 種別 row is row 0; the form opens on the ID row, so get there first -
    // the old version of this test pressed [▶] on the ID row and never touched
    // a kind at all.
    let mut guard = 0;
    while ed.equip_edit.map(|e| e.field) != Some(0) {
        ed.input(Key::Up);
        guard += 1;
        assert!(guard <= 12, "row 0 (種別) must be reachable");
    }
    let mut seen_talisman = false;
    for _ in 0..(equip::BOX_KINDS.len() * 2 + 3) {
        ed.input(Key::Right);
        let kind = ed.equip_edit.expect("the form").kind;
        assert!(
            equip::box_holds_kind(kind),
            "the 種別 row reached kind {kind}"
        );
        if kind == equip::TALISMAN_KIND {
            seen_talisman = true;
        }
    }
    assert!(seen_talisman, "護石 is reachable from the 種別 row");
}

/// Every kind the box can create is a kind the box lists.
#[test]
fn every_record_the_box_creates_is_a_row_of_its_list() {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    // Only what the form can make: a talisman is listed but is made by 護石編集.
    let creatable: Vec<u8> = equip::BOX_KINDS
        .iter()
        .copied()
        .filter(|k| equip::form_creates_kind(*k))
        .collect();
    for (n, kind) in creatable.iter().enumerate() {
        let index = equipops::create_in_box(&mut save, 0, *kind, 1, 1)
            .unwrap_or_else(|e| panic!("kind {kind}: {}", e.text()));
        assert_eq!(index, n, "one blank record per kind, in order");
    }
    let ed = editor_with(save, Screen::Equip);
    let rows = ed.equip_rows();
    assert_eq!(rows.len(), creatable.len());
    for kind in creatable {
        assert!(
            rows.iter().any(|e| e.kind == kind),
            "kind {kind} was created and is not in the list"
        );
    }
    // A talisman written by 護石編集 is a row of this list too: the box holds it,
    // and a list that hides what the box holds is the bug this screen was
    // reported with. What it is not is a record this form edits - the press says
    // so (see the test above).
    let ed = editor_with_charms(3);
    assert_eq!(
        ed.equip_rows().len(),
        3,
        "the box lists the talismans it holds"
    );
    assert!(
        ed.equip_rows().iter().all(|e| e.kind == equip::TALISMAN_KIND),
        "and names their type"
    );
}

// ---------------------------------------------------------------------------
// 3: choosing a kind jumped the cursor to the row whose [A] writes
// ---------------------------------------------------------------------------

/// The kind is one half of what names a record; the id is the other. The cursor
/// stays on the ID row, so the next [A] asks which item instead of creating one
/// with the id the form happened to hold.
#[test]
fn choosing_a_kind_leaves_the_cursor_on_the_id_row() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::Equip;
    ed.input(Key::Plus);
    let e = ed.equip_edit.expect("the new form");
    assert!(e.new);
    assert_eq!(e.field, e.id_row(), "the form starts on the ID row");
    assert!(ed.apply_equip_kind(7), "{}", ed.message);
    let e = ed.equip_edit.expect("the form");
    assert_eq!(e.kind, 7);
    assert_eq!(e.field, e.id_row(), "the cursor stays on the ID row");
    assert_eq!(e.id_row(), 1);
    // [A] there opens the name picker; it does not create a record.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipPick, "{}", ed.message);
    let box_ = ed
        .save
        .as_ref()
        .and_then(|s| s.equipment_box(0))
        .expect("the box");
    assert!(
        equip::record_is_blank(&box_[..equip::EQUIP_RECORD]),
        "nothing was created"
    );
    // The editing form's ID row is the other one.
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    equipops::create_in_box(&mut save, 0, 1, 40, 1).expect("an armour record");
    let mut ed = editor_with(save, Screen::Equip);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit);
    assert!(ed.apply_equip_kind(7), "{}", ed.message);
    assert_eq!(ed.equip_edit.expect("the form").field, 8);
}
// ---------------------------------------------------------------------------
// 4: 公式 / 没クエ wrote a quest the list was not showing
// ---------------------------------------------------------------------------

/// The extract button reads the row of the tab on screen, not the save's list.
#[test]
fn the_extract_button_writes_the_quest_the_list_shows() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::SaveQuests;
    ed.set_quest_tab(0);
    ed.set_quest_list_cursor(0);
    // The save's own list is empty, so there is nothing to write and the
    // message says so.
    ed.request_extract_selected_quest();
    assert_eq!(ed.take_request(), None);
    assert!(
        ed.message.contains("書き出すクエストがありません"),
        "{}",
        ed.message
    );

    // The two catalogue tabs hold the bundle, not the save: with no bundle in
    // this checkout the list is empty, and the button must not fall back to the
    // save's cursor (which is what it did: it wrote the save's first quest).
    for tab in [1usize, 2] {
        ed.set_quest_tab(tab);
        assert_eq!(ed.quest_tab, tab);
        assert_eq!(ed.quest_list_len(), ed.bundled_quests_for(tab).len());
        ed.request_extract_selected_quest();
        assert_eq!(ed.take_request(), None, "tab {tab} wrote a save quest");
        assert!(
            ed.message.contains("書き出すクエストがありません"),
            "{}",
            ed.message
        );
    }
    // With the bundle compiled in, the row that is written is the row the tab
    // shows.
    if !ed.bundled_quests_for(1).is_empty() {
        ed.set_quest_tab(1);
        ed.set_quest_list_cursor(5);
        let want = ed.bundled_quests_for(1)[5].quest_id;
        ed.request_extract_selected_quest();
        assert_eq!(
            ed.take_request(),
            Some(Request::ExtractQuests { ids: vec![want] })
        );
    }
    // The cursor of one tab is not the cursor of another.
    ed.set_quest_tab(1);
    ed.set_quest_list_cursor(3);
    ed.set_quest_tab(2);
    assert_eq!(ed.quest_list_cursor(), 0);
    ed.set_quest_tab(0);
    assert_eq!(ed.quest_list_cursor(), 0);
}

// ---------------------------------------------------------------------------
// 5: [ZR] opened a keyboard every other press on 報酬
// ---------------------------------------------------------------------------

/// Three cells that cycle, and no keyboard on any of them.
///
/// The cycle used to have a third stop (検索) that held no value at all: the
/// arrows there could only say "this stop is for the search". The item cell is
/// a real cell with its own action ([A] opens the picker), so the cycle is the
/// row's own three cells: アイテム / 個数 / 確率%. The search is [R] and the
/// 検索 pill.
#[test]
fn zr_walks_the_three_reward_values_without_a_keyboard() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::QuestEdit;
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    assert_eq!(ed.quest_reward_field, QUEST_REWARD_COUNT);
    assert_eq!(
        QUEST_REWARD_FIELDS, 3,
        "アイテム / 個数 / 確率%, and nothing else"
    );
    // The sixth review's leftovers: the tab walks the two cells the arrows can
    // move. アイテム is [A]'s, so a third stop could only refuse the arrows.
    let mut seen = Vec::new();
    for _ in 0..2 {
        ed.input(Key::Z);
        seen.push(ed.quest_reward_field);
        assert_eq!(
            ed.take_text_request(),
            None,
            "[ZR] must not open a keyboard"
        );
    }
    assert_eq!(
        seen,
        vec![QUEST_REWARD_CHANCE, QUEST_REWARD_COUNT],
        "個数 ⇄ 確率%"
    );
    assert!(
        ed.quest_reward_field != QUEST_REWARD_ITEM,
        "アイテム is [A]'s cell, not a stop"
    );
    // The search is the explicit press: [R] on the same tab, and it is not one
    // of the stops.
    ed.input(Key::R);
    let (idx, _) = ed.take_text_request().expect("[R] opens the search");
    assert_eq!(idx, hbedit::TEXT_QUEST_ITEM);
    assert!(
        ed.quest_reward_field == QUEST_REWARD_COUNT
            || ed.quest_reward_field == QUEST_REWARD_CHANCE
            || ed.quest_reward_field == QUEST_REWARD_ITEM,
        "the search is not a stop: {}",
        ed.quest_reward_field
    );
    // 支給品 has one value and nothing to switch, so [ZR] is still the search.
    ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    ed.input(Key::Z);
    assert_eq!(
        ed.take_text_request().map(|r| r.0),
        Some(hbedit::TEXT_QUEST_ITEM)
    );
}

// ---------------------------------------------------------------------------
// 6: [B] on the talisman form put the list back at the top
// ---------------------------------------------------------------------------

/// The list comes back on the row the form was opened on.
#[test]
fn leaving_the_talisman_form_returns_to_its_row() {
    let mut ed = editor_with_charms(45);
    ed.open_charms();
    assert_eq!(ed.charms().len(), 45);
    ed.cursor = 39;
    ed.open_charm_form_selected();
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    assert_eq!(ed.charm_edit.expect("the form").index, 39);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms);
    assert_eq!(ed.cursor, 39, "the fortieth talisman, not the first");
    assert_eq!(ed.charms().get(ed.cursor).map(|c| c.index), Some(39));
}

// ---------------------------------------------------------------------------
// 7: 制限時間 could not be changed with a finger
// ---------------------------------------------------------------------------

/// The 基本 tab draws the stepper the other three tabs have.
#[test]
fn the_basic_tab_has_a_stepper() {
    let body = fn_body(ui_src(), "fn draw_quest_edit");
    assert!(
        !body.contains("cur, false, a_label)"),
        "no tab draws the list without a stepper: {body}"
    );
    assert_eq!(
        body.matches("cur, true, a_label)").count(),
        2,
        "基本 and モンスター both draw it: {body}"
    );
}

// ---------------------------------------------------------------------------
// 8: [R] answered on the release, [L] on the press
// ---------------------------------------------------------------------------

/// The two keys of one band answer on the same frame.
#[test]
fn both_shoulders_answer_on_the_release() {
    let src = editor_src();
    assert!(
        !src.contains("l_press"),
        "the press-time shoulder is gone from the console"
    );
    assert!(src.contains("let mut l_tap = mhnx_core::hbedit::ShoulderTap::new()"));
    assert!(src.contains("if l_tap.release()"), "L answers on the release");
    assert!(src.contains("if r_tap.release()"), "R answers on the release");
    // Both holds keep their second job.
    assert!(src.contains("match l_tap.tick()"));
    assert!(src.contains("match r_tap.tick()"));
}
// ---------------------------------------------------------------------------
// 9: the talisman form's skill rows walked the raw 0..=255
// ---------------------------------------------------------------------------

/// [◀/▶] on a skill row only reaches ids the picker lists.
#[test]
fn the_skill_rows_walk_only_the_allowed_ids() {
    let mut ed = editor_with_charms(1);
    ed.open_charms();
    ed.cursor = 0;
    ed.open_charm_form_selected();
    assert_eq!(ed.screen, Screen::CharmEdit);
    let e = ed.charm_edit.expect("the form");
    let allowed = charm::allowed_skill_ids(e.kind, false);
    assert!(allowed.len() > 3);
    // The form lands on the first skill row; walk it a whole table's worth.
    ed.charm_edit.as_mut().expect("form").field = 3;
    let mut seen = Vec::new();
    for _ in 0..(allowed.len() * 2 + 5) {
        ed.input(Key::Right);
        let id = ed.charm_edit.expect("form").skill1;
        assert!(
            allowed.contains(&id),
            "the skill row reached {id}, which the picker does not list"
        );
        seen.push(id);
    }
    // It really moved: the walk is not stuck on one value.
    assert!(seen.iter().any(|id| *id != seen[0]));
    // The second skill row uses the second slot's table.
    let e = ed.charm_edit.expect("form");
    let second = charm::allowed_skill_ids(e.kind, true);
    ed.charm_edit.as_mut().expect("form").field = 5;
    for _ in 0..(second.len() + 3) {
        ed.input(Key::Right);
        let id = ed.charm_edit.expect("form").skill2;
        assert!(second.contains(&id), "the second skill row reached {id}");
    }
}

// ---------------------------------------------------------------------------
// 10: the equipment box was always eight rows
// ---------------------------------------------------------------------------

/// With nothing to warn about the list shows nine rows, and the box no longer
/// passes a permanent note that costs it one.
#[test]
fn the_equipment_box_shows_nine_rows_when_nothing_warns() {
    assert_eq!(hbedit::list_window_rows(508, 56, 0, 17), 9);
    assert_eq!(hbedit::LIST_MAX_ROWS, 9);
    let box_ = fn_body(ui_src(), "fn draw_equip");
    assert!(
        !box_.contains("2回で削除 / [＋] 新規"),
        "the permanent note costs the list a row: {box_}"
    );
    assert!(box_.contains("&[],"), "the box passes no notice: {box_}");
    let form = fn_body(ui_src(), "fn draw_equip_edit");
    assert!(
        !form.contains("[A] は行ごとの操作"),
        "the nine-row form keeps its ninth row: {form}"
    );
}

// ---------------------------------------------------------------------------
// 11: the 刻み [Y] label named a key that opens a picker
// ---------------------------------------------------------------------------

/// The step pills name no key on the two forms, because [Y] there is a picker.
#[test]
fn the_step_pills_name_no_key_on_the_two_forms() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::CharmEdit;
    assert_eq!(ed.step_table().1, "", "護石編集 spends [Y] on the picker");
    ed.screen = Screen::EquipEdit;
    assert_eq!(ed.step_table().1, "", "装備編集 spends [Y] on the picker");
    // The screens that really do cycle the step keep naming it.
    ed.screen = Screen::Hunts;
    assert_eq!(ed.step_table().1, "A");
    ed.screen = Screen::QuestEdit;
    assert_eq!(ed.step_table().1, "Y");
    // The console draws the label from that answer and from nothing else.
    assert!(ui_src().contains("ed.step_table()"), "the pills ask the core");
}

// ---------------------------------------------------------------------------
// 12: the 護石編集 tab did not light when the form could not open
// ---------------------------------------------------------------------------

/// The pressed tab stays lit, the rule the quest bar follows.
#[test]
fn the_gear_bar_lights_the_tab_that_was_pressed() {
    // An empty box is the case the finding is about: the form cannot open, the
    // list stays on screen - and the tab that was pressed is still the lit one.
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::Charms;
    ed.open_charm_form_selected();
    assert!(ed.charm_edit.is_none());
    assert_eq!(ed.screen, Screen::Charms);
    assert!(ed.message.contains("護石"), "{}", ed.message);
    assert_eq!(ed.charm_tab, 1, "護石編集 was pressed and stays lit");
    // 護石一覧 puts it back.
    ed.open_charms();
    assert_eq!(ed.charm_tab, 0);
    // A form that really opens lights the same tab, and closing it goes back.
    let mut ed = editor_with_charms(2);
    ed.open_charms();
    ed.open_charm_form_selected();
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert_eq!(ed.charm_tab, 1);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms);
    assert_eq!(ed.charm_tab, 0, "the list is the screen, so its tab is lit");
    // The console's bar reads that answer and keeps no copy of its own.
    let bar = fn_body(ui_src(), "fn sub_tabs_for");
    assert!(bar.contains("ed.charm_tab.min(1)"), "{bar}");
    assert!(!ui_src().contains("gear_bar_tab"), "one answer, not two");
}

// ---------------------------------------------------------------------------
// 13: セーブマップ and 生データ, two names for one screen
// ---------------------------------------------------------------------------

/// One word for the screen: 生データ, the one the pill and the footer use.
#[test]
fn the_save_map_screen_has_one_name() {
    let ui = ui_src();
    let editor = editor_src();
    assert!(ui.contains("\"生データ\","), "the section tab says 生データ");
    assert!(!ui.contains("\"セーブマップ\""), "no label says セーブマップ");
    assert!(
        !ui.contains("生データ（セーブマップ）"),
        "the title does not print both names"
    );
    assert!(!editor.contains("生データ（セーブマップ）"));
    // The name is the state machine's now (hbedit::screen_name), so the text
    // build's title is one table with the footer's [B] words.
    assert!(editor.contains("mhnx_core::hbedit::screen_name"));
    assert!(hbedit_src().contains("Screen::SaveMap => \"生データ\""));
}

// ---------------------------------------------------------------------------
// 14: the equipment box did not say where the talismans are
// ---------------------------------------------------------------------------

/// The heading says it, filtered or not.
#[test]
fn the_equipment_heading_says_where_the_talismans_are() {
    let mut ed = editor_on_a_blank_save();
    assert!(
        ed.equip_filter_heading().contains("護石一覧"),
        "{}",
        ed.equip_filter_heading()
    );
    ed.equip_filter = String::from("大剣");
    let heading = ed.equip_filter_heading();
    assert!(heading.contains("護石一覧"), "{heading}");
    assert!(heading.contains("大剣"), "{heading}");
}

// ---------------------------------------------------------------------------
// 15: [Y] on the new form's レベル row talked about a row that is not there
// ---------------------------------------------------------------------------

/// The line talks about the row the cursor is on.
#[test]
fn y_on_the_level_row_talks_about_the_level() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::Equip;
    ed.input(Key::Plus);
    ed.equip_edit.as_mut().expect("form").field = 2;
    ed.input(Key::Y);
    assert!(ed.message.contains("レベル"), "{}", ed.message);
    assert!(
        !ed.message.contains("装飾品"),
        "the new form has no 装飾品 row: {}",
        ed.message
    );
    // The 種別 / ID rows still open their list from [Y].
    ed.equip_edit.as_mut().expect("form").field = 1;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EquipPick, "{}", ed.message);
}

// ---------------------------------------------------------------------------
// 16: pressing the lit sub-tab threw the cursor away
// ---------------------------------------------------------------------------

/// A press on the tab that is already lit is not a move.
#[test]
fn pressing_the_lit_tab_does_nothing() {
    let arm = fn_body(ui_src(), "pub fn select_sub_tab");
    assert!(
        arm.contains("idx == self.sub_tabs_for(ed).1"),
        "the same-tab press is recognised: {arm}"
    );
    let stop = arm.find("form_leave_press").unwrap_or(arm.len());
    let guard = &arm[..stop];
    assert!(
        guard.contains("return;"),
        "and it returns before the jump: {guard}"
    );
}

// ---------------------------------------------------------------------------
// 17: the bundle tabs' guide promised a delete they refuse
// ---------------------------------------------------------------------------

/// The guide follows the tab, and the key really refuses.
#[test]
fn the_bundle_tabs_do_not_promise_the_delete() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::SaveQuests;
    ed.set_quest_tab(0);
    let save_guide = ed.screen_guide();
    if Editor::bundled_quests_available() {
        assert!(save_guide.contains("[-] で削除（2回）"), "{save_guide}");
    } else {
        // No bundle in this checkout: the guide explains the two empty tabs
        // instead, and promises no delete either.
        assert!(save_guide.contains("DLC バンドル未生成"), "{save_guide}");
        assert!(!save_guide.contains("[-] で削除"), "{save_guide}");
    }
    for tab in [1usize, 2] {
        ed.set_quest_tab(tab);
        let guide = ed.screen_guide();
        assert!(
            !guide.contains("[-] で削除"),
            "tab {tab} promises a delete it refuses: {guide}"
        );
        // A checkout without the generated bundle has no rows on these two
        // tabs at all, and the guide says so instead (the second review's
        // finding 20) - which is also a guide that promises no delete.
        if Editor::bundled_quests_available() {
            assert!(guide.contains("この一覧では使いません"), "{guide}");
        } else {
            assert!(guide.contains("DLC バンドル未生成"), "{guide}");
        }
        assert!(guide.contains("[A]"), "{guide}");
    }
    // The key answers the same way.
    ed.set_quest_tab(1);
    ed.input(Key::Minus);
    assert!(ed.message.contains("この一覧では"), "{}", ed.message);
    assert_eq!(ed.save_quest_armed, None);
}

// ---------------------------------------------------------------------------
// 18: the PC build called the talisman お守り
// ---------------------------------------------------------------------------

/// The game's own word is 護石, and both builds say it.
#[test]
fn the_pc_build_calls_the_talisman_kanshi() {
    let src = gui_src();
    assert!(
        !src.contains("お守り"),
        "the PC build still says お守り: the game's own names are 〜の護石"
    );
    assert!(src.contains("Tab::Charm => \"護石\""), "the tab is 護石");
    assert_eq!(charm::kind_name(10), Some("天の護石"));
    assert_eq!(equip::type_name(equip::TALISMAN_KIND), "護石");
}

// ---------------------------------------------------------------------------
// The two numbers the console and the tests share
// ---------------------------------------------------------------------------

/// The 基本 tab's .gmd lines are paragraphs: the field the keyboard gets for one
/// is multiline, which is why it takes the built-in keyboard.
#[test]
fn a_quest_text_line_is_a_paragraph() {
    let mut ed = editor_on_a_blank_save();
    ed.screen = Screen::QuestEdit;
    ed.quest_set_tab(QUEST_TAB_BASIC);
    // A .gmd line index is not a sentinel: the title is the line's label and
    // the field is a paragraph.
    let spec = ed.text_request_spec(0);
    assert!(spec.multiline);
    assert_eq!(spec.capacity, GMD_TEXT_MAX);
    assert_eq!(ed.text_request_title(0), mhnx_core::quest::Gmd::LABELS[0]);
    // A one-line sentinel is not.
    let spec = ed.text_request_spec(hbedit::TEXT_ITEM_COUNT);
    assert!(!spec.multiline);
    assert_eq!(spec.capacity, 4);
    // The tabs are the four the bar names.
    assert_eq!(QUEST_TAB_BASIC, 0);
    assert_eq!(QUEST_TAB_MONSTERS, 1);
    assert_eq!(QUEST_TAB_REWARDS, 2);
    assert_eq!(QUEST_TAB_SUPPLIES, 3);
}
