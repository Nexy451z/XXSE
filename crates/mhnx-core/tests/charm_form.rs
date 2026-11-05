//! The drawn talisman form: what `[A]` means on each row, and where the
//! measured ceiling of a kind is printed.
//!
//! Two things went wrong on the console and both are pinned here.
//!
//! * The footer and the inspector button said `[A] BOXへ反映` on every row while
//!   the pad dispatch opened the skill list on the two skill rows and turned the
//!   kind on the kind row. The three answers now come from one value
//!   (`hbedit::CharmAction`): `label()` is what is printed, `key()` is what the
//!   console sends, and `of_row()` is which row gets which. A label can only be
//!   wrong here if the action it is the label of is wrong, and the second test
//!   presses the key and checks what actually happens.
//!
//! * The ceiling 「R100 / スキル1+10 / スキル2+13 / S3」 was written into the 種類
//!   row's *meta* string. The console's list draws a row's title and value only;
//!   meta is the inspector's second line and the inspector shows the selected
//!   row alone, so the text was never on the row it belonged to. It is printed
//!   beside the form's header now (`CharmEdit::header_note`), which the layout
//!   test below measures against the room that line really has.

use mhnx_core::charm::{self, Charm, TalismanGrade};
use mhnx_core::hbedit::{
    charm_form_rows, CharmAction, CharmEdit, Editor, Key, Screen, CHARM_EDIT_ROWS,
};

/// The two skills every 天の護石 in the sample saves carries.
const TEN_FIRST: u8 = 74;
const TEN_SECOND: u8 = 70;

/// A 天の護石 record with a legal shape, as the game writes one.
///
/// The box needs one real talisman before `set_charm_with_kind` will make
/// another: a record is copied from a talisman the game wrote, never invented.
fn ten_record() -> [u8; charm::CHARM_RECORD] {
    let mut r = [0u8; charm::CHARM_RECORD];
    r[0x00] = 0x06; // talisman, level 0
    r[0x02] = 10; // 天の護石
    r[0x0C] = TEN_FIRST;
    r[0x0D] = TEN_SECOND;
    r[0x0E] = 6;
    r[0x0F] = 10;
    r[0x10] = 3;
    r[0x12] = 100; // item id 356, low byte 100
    r[0x13] = 1; // and its high byte
    r
}

/// A save with one talisman of `kind` in box slot 0 of character slot 0.
fn editor_with_kind(kind: u8) -> Editor {
    let mut save = mhnx_core::save::empty_switch_save();
    save.set_slot_used(0, true);
    {
        let b = save.equipment_box_mut(0).expect("the box exists");
        b[..charm::CHARM_RECORD].copy_from_slice(&ten_record());
    }
    let charm = Charm {
        index: 0,
        slots: 3,
        skill1: TEN_FIRST,
        points1: 6,
        skill2: TEN_SECOND,
        points2: 10,
    };
    assert!(
        save.set_charm_with_kind(0, &charm, kind),
        "the seed talisman must be written"
    );
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.screen = Screen::Charms;
    ed.cursor = 0;
    ed.input(Key::A); // opens the form on the talisman under the cursor
    assert_eq!(ed.screen, Screen::CharmEdit, "the form is open");
    ed
}

/// Move the form's cursor to `row` with the pad key the footer names.
fn goto(ed: &mut Editor, row: usize) {
    for _ in 0..CHARM_EDIT_ROWS {
        if ed.charm_edit.expect("the form is open").row() == row {
            return;
        }
        ed.input(Key::Down);
    }
    panic!("row {} is not reachable on the form", row);
}

/// Press `[A]` the way the console does: the row's own key, nothing else.
fn press_a(ed: &mut Editor) {
    let key = ed.charm_edit.expect("the form is open").a_key();
    ed.input(key);
}

// ---------------------------------------------------------------------------
// 1. The label, the footer and the key are one answer.
// ---------------------------------------------------------------------------

#[test]
fn every_row_says_what_a_really_does() {
    // The four things the owner asked for, row by row: 種類 switches, スロット
    // switches, the two skill rows pick, the two point rows raise. Row 0 names
    // the box slot the form would go into, so it is the one row that writes.
    let expected = [
        (0usize, CharmAction::WriteToBox, "BOXへ反映"),
        (1, CharmAction::CycleKind, "種類を切替"),
        (2, CharmAction::CycleSlots, "スロットを切替"),
        (3, CharmAction::PickSkill, "スキルを選ぶ"),
        (4, CharmAction::RaisePoints, "数値を増やす"),
        (5, CharmAction::PickSkill, "スキルを選ぶ"),
        (6, CharmAction::RaisePoints, "数値を増やす"),
    ];
    assert_eq!(
        expected.len(),
        CHARM_EDIT_ROWS,
        "the table and the form must have the same number of rows"
    );

    for (row, action, label) in expected {
        assert_eq!(CharmAction::of_row(row), action, "row {}", row);
        let mut form = CharmEdit::default();
        form.field = row;
        assert_eq!(form.row(), row);
        assert_eq!(form.a_action(), action, "row {}", row);
        // The printed label is the action's own word - not a second table that
        // could fall out of step with it.
        assert_eq!(form.a_label(), action.label(), "row {}", row);
        assert_eq!(form.a_label(), label, "row {}", row);
        assert_eq!(form.a_key(), action.key(), "row {}", row);

        // The footer line and the inspector's [A] button carry that same word.
        let footer = form.footer();
        assert!(
            footer.contains(&format!("[A] {}", label)),
            "row {}: the footer does not name the row's [A]: {}",
            row,
            footer
        );
        // [X] is the save key on this screen (the console sends every X to
        // QuickSave), so the footer must not call it 刻み any more.
        assert!(
            footer.contains("[X] 保存"),
            "row {}: [X] is the save key: {}",
            row,
            footer
        );
        assert!(
            !footer.contains("刻み"),
            "row {}: the pad never sends 刻み from this form: {}",
            row,
            footer
        );
    }

    // A row past the end clamps like the highlight does, so a state that fell
    // out of the list can never print nothing.
    let mut past = CharmEdit::default();
    past.field = 99;
    assert_eq!(past.row(), CHARM_EDIT_ROWS - 1);
    assert_eq!(past.a_action(), CharmAction::RaisePoints);
}

// ---------------------------------------------------------------------------
// 2. The key the label names is the key that does it.
// ---------------------------------------------------------------------------

#[test]
fn a_on_each_row_does_the_thing_the_row_says() {
    // 種類: turning the row moves the kind and the rarity written with it.
    let mut ed = editor_with_kind(10);
    goto(&mut ed, 1);
    let before = ed.charm_edit.expect("open");
    assert_eq!(before.kind, 10);
    press_a(&mut ed);
    let after = ed.charm_edit.expect("open");
    assert_ne!(after.kind, before.kind, "the kind row's [A] must turn the kind");
    assert_eq!(
        after.grade,
        TalismanGrade::from_rarity(charm::kind_rarity(after.kind).unwrap()).unwrap(),
        "the rarity follows the kind"
    );
    assert_eq!(after.skill1, before.skill1, "turning the kind keeps the skills");

    // スロット数: the row's [A] cycles the count.
    let mut ed = editor_with_kind(10);
    goto(&mut ed, 2);
    let before = ed.charm_edit.expect("open");
    press_a(&mut ed);
    let after = ed.charm_edit.expect("open");
    assert_ne!(after.slots, before.slots, "the slot row's [A] must step the count");
    let cap = after.grade.max_slots();
    assert!(after.slots <= cap, "{} slots is past the ceiling {}", after.slots, cap);

    // 第1スキル / 第2スキル: the row's [A] opens the name picker.
    for row in [3usize, 5] {
        let mut ed = editor_with_kind(10);
        goto(&mut ed, row);
        press_a(&mut ed);
        assert_eq!(
            ed.screen,
            Screen::SkillPick,
            "row {}: [A] must open the skill picker",
            row
        );
        assert_eq!(
            ed.skill_pick_field,
            if row == 3 { 1 } else { 2 },
            "row {}: the picker writes the row it was opened from",
            row
        );
        // B returns to the form, still on the same row.
        ed.input(Key::B);
        assert_eq!(ed.screen, Screen::CharmEdit);
        assert_eq!(ed.charm_edit.expect("open").row(), row);
    }

    // 第1スキル値 / 第2スキル値: the row's [A] raises the number, and the box is
    // not written.
    for row in [4usize, 6] {
        let mut ed = editor_with_kind(10);
        goto(&mut ed, row);
        // From zero, so "raised" cannot be confused with a ceiling clamp.
        if let Some(e) = &mut ed.charm_edit {
            e.points1 = 0;
            e.points2 = 0;
        }
        let before = ed.charm_edit.expect("open");
        let box_before = ed.save.as_ref().and_then(|s| s.equipment_box(0)).map(|b| b.to_vec());
        press_a(&mut ed);
        let after = ed.charm_edit.expect("open");
        if row == 4 {
            assert!(after.points1 > before.points1, "row 4: [A] must raise the value");
            assert_eq!((after.points2, after.skill1), (before.points2, before.skill1));
        } else {
            assert!(after.points2 > before.points2, "row 6: [A] must raise the value");
            assert_eq!((after.points1, after.skill2), (before.points1, before.skill2));
        }
        assert_eq!(ed.screen, Screen::CharmEdit, "row {}: [A] stays on the form", row);
        let box_after = ed.save.as_ref().and_then(|s| s.equipment_box(0)).map(|b| b.to_vec());
        assert_eq!(box_before, box_after, "row {}: [A] must not write the box", row);
    }

    // 装備BOX枠: the one row that writes. It puts the form into the box slot it
    // names and hands the screen back to the list.
    let mut ed = editor_with_kind(10);
    goto(&mut ed, 0);
    // The values ten_record() carries, which the game accepts on a 天の護石:
    // the write itself clamps, so a made-up value could not be read back.
    if let Some(e) = &mut ed.charm_edit {
        e.skill1 = TEN_FIRST;
        e.points1 = 6;
        e.slots = 2;
    }
    press_a(&mut ed);
    assert_eq!(ed.screen, Screen::Charms, "row 0: [A] writes and returns");
    let written = ed
        .save
        .as_ref()
        .and_then(|s| s.charms(0))
        .and_then(|l| l.into_iter().find(|c| c.index == 0))
        .expect("the talisman is still in the box");
    assert_eq!(written.skill1, TEN_FIRST);
    assert_eq!(written.points1, 6);
    assert_eq!(written.slots, 2);
}

// ---------------------------------------------------------------------------
// 3. The ceiling is on the screen, not only in the inspector.
// ---------------------------------------------------------------------------

#[test]
fn the_kind_row_prints_the_measured_ceiling_of_the_kind() {
    let mut ed = editor_with_kind(10);
    goto(&mut ed, 1);
    let form = ed.charm_edit.expect("open");
    assert_eq!(
        form.header_note(),
        "R100 / スキル1+10 / スキル2+13 / S3",
        "the header note is the measured ceiling of 天の護石"
    );

    let rows = charm_form_rows(&form);
    assert_eq!(rows.len(), CHARM_EDIT_ROWS);
    // Row numbers are the highlight's, in order: the cursor and the drawn list
    // must never disagree about which row is which.
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row.field, i, "row {} has the wrong field number", i);
    }
    // The row's own title now carries the hint for its [A]: "種類  ▸ +1".
    assert!(rows[1].title.starts_with("種類"), "{}", rows[1].title);
    assert!(rows[1].title.contains("▸ +1"), "{}", rows[1].title);
    assert_eq!(rows[1].value, "天の護石");
    assert!(
        rows[1].meta.contains("R100 / スキル1+10 / スキル2+13 / S3"),
        "the inspector keeps the ceiling too: {}",
        rows[1].meta
    );

    // The note follows the kind as the row turns: 天 (10) wraps to 兵士 (1),
    // which carries one slot and no second skill.
    press_a(&mut ed);
    let after = ed.charm_edit.expect("open");
    assert_eq!(after.kind, 1);
    assert_eq!(
        after.header_note(),
        "R97 / スキル1+10 / 第2スキルなし / S1",
        "the note follows the kind row"
    );
    let rows = charm_form_rows(&after);
    assert_eq!(rows[1].value, "兵士の護石");
}

// ---------------------------------------------------------------------------
// 4. No preset survives on the form.
// ---------------------------------------------------------------------------

#[test]
fn y_is_the_skill_picker_and_no_longer_a_preset() {
    // On a point row [Y] used to write 合法最大値 or 痛撃6 達人10 into the form.
    // It is the state machine's own key now: a message, and the row untouched.
    let mut ed = editor_with_kind(10);
    goto(&mut ed, 4);
    let before = ed.charm_edit.expect("open");
    ed.input(Key::Y);
    let after = ed.charm_edit.expect("open");
    assert_eq!(
        (after.skill1, after.points1, after.slots),
        (before.skill1, before.points1, before.slots),
        "[Y] on a point row must not write a preset"
    );
    assert_eq!(ed.screen, Screen::CharmEdit, "[Y] must not leave the form");
    assert!(
        ed.message.contains("スキル"),
        "and it must say where it works: {}",
        ed.message
    );

    // On a skill row it still opens the picker, which is what the footer says.
    goto(&mut ed, 3);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
}

// ---------------------------------------------------------------------------
// 5. The lines fit the places they are drawn in.
// ---------------------------------------------------------------------------

/// Conservative width of one line, in the console's shared font.
///
/// The console draws Small at 17px and Medium at 22px per full-width glyph; a
/// half-width glyph never exceeds 60% of that. Anything above U+2000 (kanji,
/// kana, ◯, ▲▼) is counted full width, and every other character 12 / 9px,
/// which is wider than the font really is.
fn line_px(s: &str, wide: i32, narrow: i32) -> i32 {
    s.chars()
        .map(|c| if (c as u32) > 0x2000 { wide } else { narrow })
        .sum()
}

#[test]
fn the_form_text_fits_the_columns_it_is_drawn_in() {
    // Geometry from switch/src/ui.rs: the list spans MAIN_X(20)..MAIN_X+V1_LIST_W(796),
    // a row starts at +20, the value column is capped at 230px, the badge is at
    // most 88px and the gap after it 14px. The inspector is 428px wide, its
    // text column iw-40 = 388px. The header line gives the note everything
    // between the title and 96px before the list's right edge.
    const LIST: i32 = 796;
    const ROW_LEFT: i32 = 20;
    const VALUE_CAP: i32 = 230;
    const BADGE_CAP: i32 = 88;
    const BADGE_GAP: i32 = 14;
    const GAP_BEFORE_VALUE: i32 = 20;
    const INSPECTOR: i32 = 388;
    const HEADER_TITLE: i32 = 4 * 22 + 4; // 護石編集, Medium
    const HEADER_NOTE: i32 = LIST - 96 - (HEADER_TITLE + 18);

    for kind in 1u8..=charm::TALISMAN_KIND_COUNT {
        let mut form = CharmEdit::default();
        form.kind = kind;
        form.grade = TalismanGrade::from_rarity(charm::kind_rarity(kind).unwrap()).unwrap();
        let rows = charm_form_rows(&form);

        // The note is a single line, and it has to fit beside the header.
        let note = form.header_note();
        assert!(!note.contains('\n'), "the note is one line: {}", note);
        let note_w = line_px(&note, 17, 9);
        assert!(
            note_w <= HEADER_NOTE,
            "kind {}: the note is {}px in {}px: {}",
            kind, note_w, HEADER_NOTE, note
        );

        for row in rows.iter() {
            // Badge + title + value must fit the list row without the title
            // being clipped: that is what keeps the text off its neighbours.
            let badge = line_px(&row.badge, 17, 9) + 16;
            let badge = badge.clamp(44, BADGE_CAP);
            let used = badge
                + BADGE_GAP
                + line_px(&row.title, 22, 12)
                + GAP_BEFORE_VALUE
                + line_px(&row.value, 22, 12).min(VALUE_CAP);
            assert!(
                used <= LIST - ROW_LEFT - 22,
                "kind {} row {}: {}px of {}px used ({} / {})",
                kind,
                row.field,
                used,
                LIST - ROW_LEFT - 22,
                row.title,
                row.value
            );
            // The inspector's meta line is the only place meta is drawn, and it
            // is 388px wide at Small.
            assert!(
                line_px(&row.meta, 17, 9) <= INSPECTOR,
                "kind {} row {}: the inspector line is too wide: {}",
                kind,
                row.field,
                row.meta
            );
        }

        // The footer line is right-aligned at 1262 and the status message is
        // clipped to whatever is left of it, so the guide has to leave room.
        let footer = form.footer();
        let footer_w = line_px(&footer, 17, 9);
        assert!(
            footer_w <= 900,
            "kind {}: the footer guide is {}px, leaving no room for the message: {}",
            kind, footer_w, footer
        );
    }
}