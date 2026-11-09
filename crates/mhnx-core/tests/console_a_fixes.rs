//! The ten A items of the third-party review, pinned by the state machine.
//!
//! Every one of them was one of two shapes: a key that did the wrong thing
//! (A1, A2, A3, A8), or a label that named a key that did something else
//! (A4..A7, A9, A10). The console draws what these tests drive, so the fixes
//! live where a test can reach them: the key map, the two labels and the two
//! pieces of chrome geometry are mhnx_core::hbedit's now, and the Switch layer
//! only asks. The ui.rs reads below are there because drawing code cannot be
//! driven from a host test - they pin the *wiring* (the console uses the
//! answer) rather than the answer itself.
//!
//! Nothing here reads or writes a card: the saves are built in memory.

use mhnx_core::hbedit::{
    deck_stack, footer_guide_right, footer_quit_x, CharmEdit, Editor, EquipEdit, ItemSetEdit, Key,
    PalicoEquip, PalicoSkill, Request, Screen, SetEdit, Shoulder, FOOTER_GUIDE_GAP, FOOTER_QUIT_W,
    FOOTER_RIGHT, GUILD_ROW_COMMENT, GUILD_ROW_NAME, GUILD_TAB_ARENA, GUILD_TAB_CARD,
    GUILD_TAB_COUNTS, GUILD_TAB_WEAPONS, PALICO_EQUIP_NONE, PALICO_EQUIP_ROWS, PALICO_MOVE_ROWS,
};
use mhnx_core::save::{Save, BODY_SIZE, SLOT_STRIDE};

/// Body offset of the first character slot (also stored at body[0x10]).
const FIRST_SLOT_BASE: u32 = 0x126474;

/// A zeroed body with the marker and the three slot bases filled in: the same
/// in-memory save the hunt-record tests build, so nothing here depends on a
/// card being in the machine.
fn open_blank(ed: &mut Editor) {
    let mut body = vec![0u8; BODY_SIZE];
    body[0] = 0xC6;
    for slot in 0..3 {
        let base = FIRST_SLOT_BASE as usize + slot * SLOT_STRIDE;
        body[0x10 + 4 * slot..0x14 + 4 * slot].copy_from_slice(&(base as u32).to_le_bytes());
        body[0x04 + slot] = 1;
    }
    let save = Save::from_bytes(&body).expect("the synthetic save must parse");
    ed.save = Some(save);
    ed.save_path = Some(String::from("memory:review-a"));
    ed.slot = 0;
    ed.screen = Screen::Slots;
}

/// The Switch layer, as text. The drawing bugs cannot be driven from a host
/// test, so their wiring is checked here; the numbers themselves are the hbedit
/// helpers' tests.
fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
}

/// The body of one function in ui.rs, from its declaration to the next item.
fn fn_body<'a>(src: &'a str, decl: &str) -> &'a str {
    let start = src
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} is not in switch/src/ui.rs"));
    let rest = &src[start + decl.len()..];
    let mut end = rest.len();
    for pat in ["\n    fn ", "\n    pub fn ", "\n    #[allow("] {
        if let Some(i) = rest.find(pat) {
            end = end.min(i);
        }
    }
    &rest[..end]
}

// ---------------------------------------------------------------------------
// A1: [R] on the item box / the hunt record is the number key, not the tab key
// ---------------------------------------------------------------------------

/// A1: [R] was answered by the sub-tab rule before the screen's own arm, so the
/// "[R] 数値入力" both footers print opened nothing: the BOX tab moved instead.
#[test]
fn the_two_screens_that_promise_a_number_take_r_for_typing() {
    let mut ed = Editor::new();

    ed.screen = Screen::Items;
    assert_eq!(
        ed.shoulder_job(Key::R),
        Shoulder::Screen,
        "the item box's footer promises [R] 数値入力"
    );
    assert_eq!(
        ed.shoulder_job(Key::L),
        Shoulder::SubTab,
        "[L] still moves the BOX tab"
    );

    ed.screen = Screen::Hunts;
    assert_eq!(
        ed.shoulder_job(Key::R),
        Shoulder::Screen,
        "the hunt record's footer promises [R] 数値入力"
    );
    assert_eq!(ed.shoulder_job(Key::L), Shoulder::SubTab);

    // Every screen that does not hold a number keeps the tab moves, which is
    // the rule the whole navigation rests on.
    for screen in [
        Screen::Char,
        Screen::Charms,
        Screen::Equip,
        Screen::Sets,
        Screen::Roster,
        Screen::Progress,
        Screen::Csv,
    ] {
        ed.screen = screen;
        assert_eq!(ed.shoulder_job(Key::R), Shoulder::SubTab, "{screen:?}");
        assert_eq!(ed.shoulder_job(Key::L), Shoulder::SubTab, "{screen:?}");
    }

    // The four pick lists are the ten-row step, and the screens that already
    // had a keyboard keep it.
    ed.screen = Screen::ItemPick;
    assert_eq!(ed.shoulder_job(Key::L), Shoulder::Move);
    ed.screen = Screen::EquipEdit;
    assert_eq!(ed.shoulder_job(Key::R), Shoulder::Screen);
    ed.screen = Screen::Guild;
    ed.set_guild_view(GUILD_TAB_CARD);
    assert_eq!(ed.shoulder_job(Key::R), Shoulder::Screen);
    ed.set_guild_view(GUILD_TAB_ARENA);
    assert_eq!(ed.shoulder_job(Key::R), Shoulder::SubTab, "the arena keeps [Y] for its time");

    // The console asks instead of keeping its own list of exceptions.
    let ui = ui_src();
    assert!(
        ui.contains("ed.shoulder_job(key)"),
        "the L / R block must be the state machine's answer"
    );
}

// ---------------------------------------------------------------------------
// A2: [X] saves, and does not run the row's own [A]
// ---------------------------------------------------------------------------

/// A2 (talisman form): [X] on a skill row ran the row's [A], which opened the
/// skill picker, and then pressed [X] on the picker - so the save never
/// happened and a list opened instead.
#[test]
fn the_save_key_on_the_talisman_form_only_saves() {
    let mut ed = Editor::new();
    open_blank(&mut ed);
    ed.open_charms();
    let mut form = CharmEdit::default();
    form.field = 3; // スキル1: the row whose [A] opens the skill list
    ed.charm_edit = Some(form);
    ed.screen = Screen::CharmEdit;

    ed.quick_save();

    assert_eq!(
        ed.screen,
        Screen::CharmEdit,
        "the save key must not run the row's [A] (that opened the picker)"
    );
    assert!(ed.charm_edit.is_some(), "the form the user was on is still on");
    assert!(
        ed.take_text_request().is_none(),
        "no keyboard may open from the save key"
    );
    assert!(
        ed.take_write().is_some(),
        "the save request is the one thing [X] does"
    );
}

/// A2 (equipment form): the same key on an identity row opened the equipment
/// picker, and on the 装備枠 row it replaced the record, before it saved.
///
/// The form is built the way the console builds one - create a record, then
/// open the form on it - because the blank save holds no equipment: a form
/// pointing at an empty row cannot be committed, and the save key correctly
/// refuses to write a file that lacks what is on screen.
#[test]
fn the_save_key_on_the_equipment_form_only_saves() {
    let mut ed = Editor::new();
    open_blank(&mut ed);
    ed.screen = Screen::Equip;
    ed.input(Key::Plus); // [＋] opens the new-record form on this screen
    let mut new = ed.equip_edit.expect("the creator opens a form");
    new.kind = 1;
    new.id = 1;
    new.level = 1;
    new.field = new.write_row();
    ed.equip_edit = Some(new);
    ed.input(Key::A); // レベル: 作成
    assert!(
        ed.equip_edit.is_none(),
        "[A] creates and leaves the form: {}",
        ed.message
    );
    assert_eq!(ed.screen, Screen::Equip);

    // Now edit it: the form holds a level the record does not have.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    let mut form = ed.equip_edit.expect("the form is open");
    form.level = 3;
    form.field = 0; // 装備枠: the row whose [A] writes
    ed.equip_edit = Some(form);
    ed.take_write();

    ed.quick_save();

    // The save key still never opens the picker (that was the finding). It does
    // commit the form now, which is the fourth review's fix: a form whose draft
    // is not in the save must not be saved as if it were.
    assert_ne!(
        ed.screen,
        Screen::EquipPick,
        "the save key must not open the equipment picker"
    );
    assert!(
        ed.take_text_request().is_none(),
        "and it must not open a keyboard"
    );
    // The fifth review's fix: [X] saves, it does not leave. The commit used to
    // be [A]'s, and [A] closes the form - so saving threw the user back to the
    // list they had just left.
    assert_eq!(
        ed.screen,
        Screen::EquipEdit,
        "the save key must not close the form: {}",
        ed.message
    );
    assert!(ed.equip_edit.is_some(), "the form is still the one on screen");
    assert!(
        ed.take_write().is_some(),
        "the save request is the one thing [X] does"
    );
    assert!(
        !ed.equip_draft_dirty(),
        "the draft the form was holding is in the save: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// A3: choosing the 護石編集 sub-tab must not press [Y] into the new screen
// ---------------------------------------------------------------------------

/// A3: the sub-tab arm forwarded [Y] (the search key) into the list it had just
/// opened, so picking 護石編集 popped the search keyboard - and on an empty box
/// it forwarded [L], which created a talisman.
#[test]
fn the_talisman_edit_sub_tab_spends_its_key() {
    // The list the tab opens asks for nothing.
    let mut ed = Editor::new();
    ed.open_charms();
    assert_eq!(ed.screen, Screen::Charms);
    assert!(
        ed.take_text_request().is_none(),
        "opening the list must not open the search keyboard"
    );

    // The tab switch itself is the console's (select_sub_tab), so the source is
    // the only place that can pin "the press is spent on the tab".
    let body = fn_body(ui_src(), "pub fn select_sub_tab");
    assert!(
        !body.contains("Key::Y"),
        "the tab press must not become the new screen's [Y]"
    );
    assert!(
        !body.contains("Key::L"),
        "the tab press must not become the new screen's [L] (it created a record)"
    );
}

// ---------------------------------------------------------------------------
// A4: the footer's key guide stops before the [終了] pill
// ---------------------------------------------------------------------------

/// A4: the hint was right aligned at 1262, which is the [終了] pill's own right
/// edge, so a hint short enough to "fit" was still drawn under the pill.
#[test]
fn the_footer_guide_stops_left_of_the_quit_pill() {
    let quit_x = footer_quit_x();
    assert_eq!(FOOTER_RIGHT, 1262, "the bar's right edge");
    assert_eq!(quit_x, 1262 - FOOTER_QUIT_W, "the pill ends at the bar's edge");
    assert_eq!(quit_x, 1178);
    assert_eq!(footer_guide_right(), quit_x - FOOTER_GUIDE_GAP);
    assert!(
        footer_guide_right() < quit_x,
        "the guide's right edge must be left of the pill's left edge"
    );
    assert!(
        FOOTER_RIGHT > quit_x,
        "1262 is inside the pill: that is the overlap the review found"
    );

    // And the console draws the hint at that edge, not at 1262. (Line 2's tap
    // note is still right aligned at the bar's edge: it has no pill beside it.)
    let body = fn_body(ui_src(), "fn draw_footer");
    assert!(
        body.contains("footer_guide_right()"),
        "the hint's draw call must take the guide's edge"
    );
    assert!(
        body.contains("text_right(guide_right, FOOTER_Y + 9"),
        "the hint is right aligned at the guide's edge"
    );
    assert!(
        !body.contains("text_right(1262, FOOTER_Y + 9"),
        "the hint must not be right aligned at the pill's own edge"
    );
}

// ---------------------------------------------------------------------------
// A5: the item box's tool deck ends inside the inspector panel
// ---------------------------------------------------------------------------

/// A5: five full-width rows of 36 + 6 = 204px were stacked from 466, and the
/// panel ends at 658: the fifth row was drawn at 658..694, outside the panel,
/// where it was clipped by the frame.
#[test]
fn the_item_box_deck_ends_inside_the_inspector_panel() {
    // draw_adaptive_rows_warn: body_y = 132, body_h = 526 -> the panel is
    // 132..658. The deck's heading sits at iy + 334 and the first pill at
    // iy + 358, so the stack starts at 490.
    let (panel_y, panel_h) = (132, 526);
    let deck_top = panel_y + 358;
    let panel_bottom = panel_y + panel_h;
    assert_eq!(deck_top, 490);
    assert_eq!(panel_bottom, 658);

    // The stack the console used to draw: five rows, 36 tall, 6 apart. Poor
    // 36 + 6, the fifth row started exactly on the panel's edge - 658..694.
    const PITCH: i32 = 36 + 6;
    let old_fifth = deck_top + 4 * PITCH;
    assert_eq!(old_fifth, panel_bottom, "the fifth row was drawn at 658..694");
    assert!(old_fifth + 36 > panel_bottom);
    let old = deck_stack(deck_top, panel_bottom - 6, 5, 36, 6);
    assert!(
        old[0].1 < 36,
        "five rows only fit by shrinking ({}px), which is the overflow",
        old[0].1
    );

    // The deck the console draws now: four rows of two buttons, full height.
    let rows = deck_stack(deck_top, panel_bottom - 6, 4, 36, 6);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].1, 36, "the rows keep their full height");
    for pair in rows.windows(2) {
        assert!(
            pair[0].0 + pair[0].1 < pair[1].0,
            "deck rows overlap: {:?} / {:?}",
            pair[0],
            pair[1]
        );
    }
    let last = *rows.last().unwrap();
    assert!(
        last.0 + last.1 <= panel_bottom,
        "the last deck row ends at {} (the panel ends at {})",
        last.0 + last.1,
        panel_bottom
    );

    let body = fn_body(ui_src(), "fn draw_studio_inspector");
    assert!(
        body.contains("deck_stack(cur_y, iy + ih - 6, 4, 36, 6)"),
        "the item box's deck must be measured against the panel"
    );
}

// ---------------------------------------------------------------------------
// A6: the save's quest list guide names [-], the delete, not the export
// ---------------------------------------------------------------------------

/// A6: input_save_quests answers [-] with the two-press delete and moved the
/// export to its own button, but the guide still said "[-] で書き出し".
#[test]
fn the_save_quest_guide_names_the_delete_key() {
    let mut ed = Editor::new();
    ed.screen = Screen::SaveQuests;
    let guide = ed.screen_guide();
    assert!(guide.contains("[-] で削除（2回）"), "{guide}");
    assert!(!guide.contains("[-] で書き出し"), "{guide}");
    assert!(guide.contains("書き出しは右のボタン"), "{guide}");

    // The export really is another key's job: [R] asks the I/O layer for it,
    // and [-] on this list is the delete (see the state machine's arm).
    let mut ed = Editor::new();
    open_blank(&mut ed);
    ed.screen = Screen::SaveQuests;
    ed.input(Key::R);
    assert!(
        matches!(ed.take_request(), Some(Request::ExtractQuests { .. })),
        "書き出し is the [R] request / deck pill, not [-]"
    );
}

// ---------------------------------------------------------------------------
// A7: the options panel names the keys it really has
// ---------------------------------------------------------------------------

/// A7: the inspector said "[A] 切替" (the key saves) and "刻み [Y]" (the key
/// restores a snapshot), and the options screen steps by one whatever the step
/// says.
#[test]
fn the_options_panel_names_the_keys_it_really_has() {
    let mut ed = Editor::new();
    ed.screen = Screen::Options;
    let (steps, key) = ed.step_table();
    assert_eq!(steps.len(), 5);
    assert_eq!(key, "", "環境設定 spends [Y] on the snapshot restore");
    assert_eq!(ed.options_a_label(), "保存", "[A] writes the save");

    // The screens that really cycle the step with a key still say it.
    ed.screen = Screen::Char;
    assert_eq!(ed.step_table().1, "Y");
    ed.screen = Screen::Hunts;
    assert_eq!(ed.step_table().1, "A", "the hunt record cycles the step with [A]");

    // [Y] is 空きも表示 on the item box and the view walk on the card, so those
    // pills name no key either.
    ed.screen = Screen::Items;
    assert_eq!(ed.step_table().1, "");
    assert_eq!(ed.step_table().0.len(), 5);
    ed.screen = Screen::Guild;
    ed.set_guild_view(GUILD_TAB_CARD);
    assert_eq!(ed.step_table().1, "");
    // クエスト改造 had no step key while [Y] opened its ▸詳細 rows; the rows
    // moved to the deck and the 詳細 row's [A], so the key is back.
    ed.screen = Screen::QuestEdit;
    assert_eq!(ed.step_table().1, "Y");

    let body = fn_body(ui_src(), "fn draw_options");
    assert!(
        body.contains("ed.options_a_label()"),
        "the [A] button's word comes from the state machine"
    );
    assert!(
        !body.contains("\"切替\""),
        "the panel must not say 切替 next to a key that saves"
    );
}

// ---------------------------------------------------------------------------
// A8: a slot change closes every form and picker that was open
// ---------------------------------------------------------------------------

/// A8: the badge re-pointed ed.slot under an open form, so the form's next [A]
/// wrote the other character's same frame.
#[test]
fn a_slot_change_puts_every_open_form_away() {
    let mut ed = Editor::new();
    open_blank(&mut ed);
    let first = ed.slot;
    let other = (first + 1) % 3;

    // 護石編集: the form goes back to the list.
    ed.charm_edit = Some(CharmEdit::default());
    ed.screen = Screen::CharmEdit;
    assert!(ed.switch_slot(other));
    assert_eq!(ed.slot, other, "the slot really moved");
    assert!(ed.charm_edit.is_none(), "the talisman form was closed");
    assert_eq!(ed.screen, Screen::Charms, "and the screen is its list");

    // 装備編集: the same rule.
    ed.equip_edit = Some(EquipEdit::default());
    ed.screen = Screen::EquipEdit;
    assert!(ed.switch_slot(first));
    assert!(ed.equip_edit.is_none());
    assert_eq!(ed.screen, Screen::Equip);

    // The four pick lists land on the list they were opened from.
    ed.screen = Screen::ItemPick;
    assert!(ed.switch_slot(other));
    assert_eq!(ed.screen, Screen::Items);
    ed.screen = Screen::SkillPick;
    assert!(ed.switch_slot(first));
    assert_eq!(ed.screen, Screen::Charms);
    assert!(ed.charm_edit.is_none());
    ed.screen = Screen::JewelPick;
    assert!(ed.switch_slot(other));
    assert_eq!(ed.screen, Screen::Equip);
    ed.screen = Screen::EquipPick;
    assert!(ed.switch_slot(first));
    assert_eq!(ed.screen, Screen::Equip);

    // The modes that sit on top of a screen: both set forms, the item bulk
    // menu and the two palico panels.
    ed.screen = Screen::Sets;
    ed.set_edit = Some(SetEdit::default());
    ed.item_set_edit = Some(ItemSetEdit::default());
    ed.open_item_menu();
    ed.palico_equip = Some(PalicoEquip {
        palico: 0,
        field: 0,
        pick: [PALICO_EQUIP_NONE; PALICO_EQUIP_ROWS],
        clear_armed: None,
    });
    ed.palico_skill = Some(PalicoSkill {
        palico: 0,
        tab: 0,
        field: 0,
        picks: [0u8; PALICO_MOVE_ROWS],
    });
    assert!(ed.switch_slot(other));
    assert!(ed.set_edit.is_none(), "the set form was closed");
    assert!(ed.item_set_edit.is_none(), "the item set form was closed");
    assert!(!ed.item_menu_open, "the bulk menu was closed");
    assert!(ed.palico_equip.is_none(), "the palico equipment panel was closed");
    assert!(ed.palico_skill.is_none(), "the palico skill panel was closed");

    // The same slot, and a slot that does not exist, change nothing.
    let here = ed.slot;
    assert!(!ed.switch_slot(here));
    assert!(!ed.switch_slot(3));
    assert_eq!(ed.slot, here);

    // The console goes through it, so a tap on the badge cannot re-point the
    // editor under a form.
    let body = fn_body(ui_src(), "UiAction::SelectSlot(s) =>");
    assert!(
        body.contains("ed.switch_slot(s)"),
        "the slot action must be the state machine's"
    );
}

// ---------------------------------------------------------------------------
// A9: the [R] pill is not drawn on rows that refuse the key
// ---------------------------------------------------------------------------

/// A9: supports_direct_input lit the pill on every row of the guild card, but
/// request_guild_text only answers the two numbers: the comment and the card
/// name rows printed a keyboard they refused.
#[test]
fn the_guild_pill_is_only_on_the_rows_that_take_a_number() {
    let mut ed = Editor::new();
    open_blank(&mut ed);
    ed.screen = Screen::Guild;
    ed.set_guild_view(GUILD_TAB_CARD);

    assert!(ed.direct_input_available(0), "HR is a number");
    assert!(ed.direct_input_available(1), "the clock is a number");
    assert!(
        !ed.direct_input_available(GUILD_ROW_COMMENT),
        "the comment is text that [A] opens"
    );
    assert!(
        !ed.direct_input_available(GUILD_ROW_NAME),
        "the card's own copy of the name is text"
    );

    // The key really refuses those two rows - which is why the pill must not
    // promise them.
    ed.guild_field = GUILD_ROW_COMMENT;
    ed.input(Key::R);
    assert!(ed.take_text_request().is_none(), "the comment row has no number");
    ed.guild_field = GUILD_ROW_NAME;
    ed.input(Key::R);
    assert!(ed.take_text_request().is_none());
    ed.guild_field = 0;
    ed.input(Key::R);
    assert!(ed.take_text_request().is_some(), "the HR row opens the keyboard");

    // The other three views keep the pill: their rows are all numbers, and the
    // arena view is the one that types with [Y] instead.
    ed.set_guild_view(GUILD_TAB_COUNTS);
    assert!(ed.direct_input_available(0));
    ed.set_guild_view(GUILD_TAB_WEAPONS);
    assert!(ed.direct_input_available(0));
    ed.set_guild_view(GUILD_TAB_ARENA);
    assert!(!ed.direct_input_available(0));

    let ui = ui_src();
    assert!(
        ui.contains("ed.direct_input_available(row_idx)"),
        "the [R] pill must be the state machine's answer"
    );
}

// ---------------------------------------------------------------------------
// A10: the 生データ pill is dark on the 生データ screen
// ---------------------------------------------------------------------------

/// A10: detail_available listed [Screen::SaveMap] as "has a detail view", so
/// the pill stayed lit on the screen it opens and could only answer "already
/// showing".
#[test]
fn the_raw_data_pill_goes_dark_on_the_raw_data_screen() {
    let mut ed = Editor::new();
    open_blank(&mut ed);

    ed.screen = Screen::SaveMap;
    assert!(
        !ed.detail_pill_live(),
        "生データ is the detail view; the pill has nowhere to go"
    );

    // The screens behind the pill still light it.
    for screen in [Screen::Items, Screen::Charms, Screen::Equip, Screen::Guild, Screen::Hunts] {
        ed.screen = screen;
        assert!(ed.detail_pill_live(), "{screen:?} has bytes behind it");
    }
    ed.screen = Screen::QuestEdit;
    assert!(ed.detail_pill_live(), "[ファイル] opens the quest's file screens");
    // ...and the three that have nothing behind them stay dark.
    ed.screen = Screen::SaveList;
    assert!(!ed.detail_pill_live());
    ed.screen = Screen::ItemPick;
    assert!(!ed.detail_pill_live(), "a pick list is a mode, not a record");

    let ui = ui_src();
    assert!(
        ui.contains("ed.detail_pill_live()"),
        "the pill's state must be the state machine's"
    );
}
