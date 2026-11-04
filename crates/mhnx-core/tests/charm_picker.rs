//! The two skill pickers, as the console reaches them.
//!
//! The desktop combo and the console picker both read the pool from
//! `charm::allowed_skill_ids`, so what these tests pin down is that the state
//! machine the console draws from really asks for it: the old picker walked the
//! 306 entry name table and offered 攻撃 as a 天の護石's first skill, which the
//! game never writes. The second half is the 兵士 / 闘士 row: rarity 97 has no
//! slot-2 rows, so the row says 選択不可 instead of opening an empty list.

use mhnx_core::charm::{self, Charm, TalismanGrade};
use mhnx_core::hbedit::{charm_form_rows, Editor, Key, Screen, CHARM_EDIT_ROWS};

/// The two skills every 天の護石 in the sample saves carries.
const TEN_FIRST: u8 = 74;
const TEN_SECOND: u8 = 70;

fn ten_record() -> [u8; charm::CHARM_RECORD] {
    let mut r = [0u8; charm::CHARM_RECORD];
    r[0x00] = 0x06;
    r[0x02] = 10;
    r[0x0C] = TEN_FIRST;
    r[0x0D] = TEN_SECOND;
    r[0x0E] = 6;
    r[0x0F] = 10;
    r[0x10] = 3;
    r[0x12] = 100;
    r[0x13] = 1;
    r
}

/// A save with one talisman of `kind` in box slot 0, and the form open on it.
fn editor_with_kind(kind: u8, skill2: u8, points2: i8) -> Editor {
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
        skill2,
        points2,
    };
    assert!(
        save.set_charm_with_kind(0, &charm, kind),
        "the seed talisman must be written"
    );
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.screen = Screen::Charms;
    ed.cursor = 0;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::CharmEdit, "the form is open");
    ed
}

/// Move the form cursor to `row`.
fn goto(ed: &mut Editor, row: usize) {
    for _ in 0..CHARM_EDIT_ROWS {
        if ed.charm_edit.expect("the form is open").row() == row {
            return;
        }
        ed.input(Key::Down);
    }
    panic!("row {} is not reachable", row);
}

#[test]
fn the_console_picker_lists_only_the_kinds_skills() {
    let mut ed = editor_with_kind(10, TEN_SECOND, 10);
    goto(&mut ed, 3);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_field, 1);

    let pool = charm::allowed_skill_ids(10, false);
    assert_eq!(pool.len(), 62);
    assert_eq!(ed.skill_pick_matches(), pool, "the console lists the pool");
    // The three shapes the old 306 entry list got wrong.
    assert!(!ed.skill_pick_matches().contains(&18), "攻撃 is a second skill");
    assert!(ed.skill_pick_matches().contains(&5), "聴覚保護 is a first skill");
    assert!(ed.skill_pick_matches().contains(&42), "納刀 is a first skill");
    // The heading says which slot and kind the pool belongs to, so a short
    // list explains itself on screen.
    let heading = ed.skill_pick_heading();
    assert!(heading.contains("スキル1"), "{}", heading);
    assert!(heading.contains("天の護石"), "{}", heading);
    assert!(heading.contains("62"), "{}", heading);
    // Typing 攻撃 finds the skills whose *name* contains it (属性攻撃, 特殊攻撃,
    // 減気攻撃) but never 攻撃 itself, which is a second skill of a 天.
    ed.apply_skill_filter("攻撃".to_string());
    let filtered = ed.skill_pick_matches();
    assert!(!filtered.contains(&18), "攻撃 is not a first skill: {:?}", filtered);
    assert!(filtered.contains(&32) && filtered.contains(&33) && filtered.contains(&88));
    assert!(
        filtered.iter().all(|id| pool.contains(id)),
        "a filtered list may only hold ids of the pool: {:?}",
        filtered
    );
}

#[test]
fn the_second_skill_pool_is_the_other_slot() {
    let mut ed = editor_with_kind(7, 0, 0);
    goto(&mut ed, 5);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_field, 2);
    let pool = charm::allowed_skill_ids(7, true);
    assert_eq!(pool.len(), 105, "a 龍 offers 105 second skills");
    assert_eq!(ed.skill_pick_matches(), pool);
    assert!(pool.contains(&18), "攻撃 is a second skill");
    assert!(pool.contains(&42), "納刀 is a second skill of a 龍 too");
    assert!(!pool.contains(&1), "毒 is on neither row of a 龍");
    let heading = ed.skill_pick_heading();
    assert!(heading.contains("スキル2") && heading.contains("105"), "{}", heading);
}

#[test]
fn a_soldiers_second_skill_row_says_unavailable() {
    // 兵士 with no second skill: the row says 選択不可, Y explains instead of
    // opening a picker, and the value row has nothing to raise.
    let mut ed = editor_with_kind(1, 0, 0);
    let rows = charm_form_rows(&ed.charm_edit.expect("open"));
    assert_eq!(rows[5].value, "選択不可", "{}", rows[5].value);
    assert!(
        rows[5].meta.contains("第2スキルを持てません"),
        "{}",
        rows[5].meta
    );
    assert_eq!(rows[6].value, "—");

    goto(&mut ed, 5);
    ed.input(Key::Y);
    assert_eq!(
        ed.screen,
        Screen::CharmEdit,
        "the picker must not open for a slot the kind does not have"
    );
    assert!(
        ed.message.contains("第2スキル") && ed.message.contains("兵士の護石"),
        "message was {}",
        ed.message
    );
    assert!(charm::allowed_skill_ids(1, true).is_empty());
    assert!(!ed.charm_edit.expect("open").second_skill_available());

    // The same form for a kind that does have one opens as before.
    let mut ed = editor_with_kind(3, 0, 0);
    goto(&mut ed, 5);
    assert!(ed.charm_edit.expect("open").second_skill_available());
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_matches().len(), 74, "a 騎士 offers 74");
}

#[test]
fn the_console_picker_pool_follows_the_kind_row() {
    // Turning the kind row re-derives everything: the pool, the slot-2
    // availability and what the form prints. 天 -> 闘士 is one Right on the
    // kind row (the row cycles downwards only through the ten kinds).
    let mut ed = editor_with_kind(10, TEN_SECOND, 10);
    goto(&mut ed, 1);
    assert_eq!(ed.charm_edit.expect("open").kind, 10);
    ed.input(Key::Right);
    assert_eq!(ed.charm_edit.expect("open").kind, 1, "10 wraps to 1");
    assert!(!ed.charm_edit.expect("open").second_skill_available());
    let rows = charm_form_rows(&ed.charm_edit.expect("open"));
    // The record still carries skill 2 from before the kind was turned: the
    // form shows it rather than hiding data, and the warning line explains it.
    assert_eq!(
        rows[5].value,
        mhnx_core::skills::skill_name(TEN_SECOND).unwrap(),
        "the row keeps what the record holds"
    );
    assert!(rows[5].meta.contains("持てません"), "{}", rows[5].meta);
    // The pool is the soldier first-slot one now.
    goto(&mut ed, 3);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_matches(), charm::allowed_skill_ids(1, false));
    assert_eq!(ed.skill_pick_matches().len(), 59);
    // And a grade that does not exist cannot be reached through the form.
    assert_eq!(
        TalismanGrade::from_rarity(charm::kind_rarity(1).unwrap()),
        Some(TalismanGrade::Mystery)
    );
}
