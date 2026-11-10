//! The guards an edit has to pass before it reaches the console's save.
//!
//! The UI review of the Switch build named six places where a press went
//! straight to the file: the snapshot restore, the buttons a tap could reach
//! under the keyboard, the equipment set's warning key, the whole-save undo,
//! the roster's bulk MAX and the empty talisman box's hint. What can be pinned
//! away from the console is pinned here, so the rule (one second, two presses,
//! one undo step) cannot drift back.

use mhnx_core::confirm::{ConfirmPress, HoldAction, HoldConfirm, HOLD_CONFIRM_FRAMES};
use mhnx_core::hbedit::{Editor, Key, Screen};
use mhnx_core::save;

/// A save with three palicos on the first character, and nothing on disk.
fn editor_with_palicos() -> Editor {
    let mut save = save::empty_switch_save();
    for (i, name) in ["アイルー", "メラルー", "トラ"] .iter().enumerate() {
        assert!(
            mhnx_core::palico::set_name(&mut save, 0, i, name),
            "the roster record {} has to be nameable",
            i
        );
    }
    let mut ed = Editor::new();
    ed.slot = 0;
    ed.save = Some(save);
    ed
}

#[test]
fn the_undo_hold_is_one_second_not_half_of_one() {
    // ~16ms a frame, which is what the console loop sleeps for.
    assert_eq!(HOLD_CONFIRM_FRAMES, 60);
    // Half a second was the old threshold and it was inside a normal press.
    let mut h = HoldConfirm::new();
    h.press();
    for frame in 0..HOLD_CONFIRM_FRAMES - 1 {
        assert_eq!(
            h.tick(),
            HoldAction::None,
            "frame {} is still inside a press that lingered",
            frame + 1
        );
    }
    assert_eq!(
        h.tick(),
        HoldAction::Arm,
        "the whole second is what makes it a hold"
    );
}

#[test]
fn the_undo_hold_asks_before_it_runs() {
    let mut h = HoldConfirm::new();
    // First full hold: offered, nothing done, and the release is not a tap.
    h.press();
    let mut last = HoldAction::None;
    for _ in 0..HOLD_CONFIRM_FRAMES {
        last = h.tick();
    }
    assert_eq!(last, HoldAction::Arm);
    assert!(h.is_armed());
    assert!(!h.release());
    // Second full hold: this one runs it.
    h.press();
    let mut last = HoldAction::None;
    for _ in 0..HOLD_CONFIRM_FRAMES {
        last = h.tick();
    }
    assert_eq!(last, HoldAction::Run);
    assert!(!h.is_armed(), "the offer is spent, not left standing");
}

#[test]
fn a_destructive_press_needs_the_second_one() {
    let mut c = ConfirmPress::new();
    assert!(!c.press(), "the first press only asks");
    assert!(c.press(), "the second one runs it");
    // And doing something else in between takes the offer away.
    assert!(!c.press());
    c.clear();
    assert!(!c.press());
}

#[test]
fn the_restore_dialog_names_the_snapshot_and_the_copy_it_kept() {
    let msg = mhnx_core::snapshot::restore_review_message(
        "system-20261104-012812.sav",
        "/switch/XXSE/backups/system_20261104-012900.bak",
    );
    assert!(msg.contains("system-20261104-012812.sav"), "{}", msg);
    assert!(
        msg.contains("/switch/XXSE/backups/system_20261104-012900.bak"),
        "the copy the save went to has to be named: {}",
        msg
    );
    assert!(msg.contains("戻します"), "{}", msg);
    assert!(
        msg.contains("[A] でもう一度"),
        "a restore is a write, so it waits for the second press: {}",
        msg
    );
    assert!(msg.contains("[B] 中止"), "{}", msg);
}

#[test]
fn every_modal_panel_stops_the_taps_that_miss_it() {
    use mhnx_core::confirm::needs_modal_guard;
    assert!(!needs_modal_guard(false, false, false));
    assert!(
        needs_modal_guard(true, false, false),
        "the MIB picker leaves the header and the undo pill live"
    );
    assert!(
        needs_modal_guard(false, true, false),
        "the keyboard leaves y0..44 and y676..720 live, and the undo pill is in the second"
    );
    assert!(needs_modal_guard(false, false, true));
    assert!(needs_modal_guard(true, true, true));
}

#[test]
fn the_roster_bulk_max_is_one_undo_step_that_can_be_taken_back() {
    let mut ed = editor_with_palicos();
    let before: Vec<u16> = ed.palicos().iter().map(|p| p.level).collect();
    assert_eq!(before, vec![1, 1, 1], "an empty record reads as level 1");

    assert_eq!(ed.maximize_all_palicos(), 3);
    assert!(ed.dirty);
    assert_eq!(ed.undo_depth(), 1, "three writes, one press, one step");
    assert_eq!(ed.undo_label(), Some("オトモ一括MAX"));
    for p in ed.palicos() {
        assert_eq!(p.level, 99);
        assert_eq!(p.enthusiasm, 100);
        assert_eq!(p.exp, 999_999);
    }
    assert!(ed.message.contains("3"), "{}", ed.message);

    assert!(ed.undo_last(), "the bulk MAX has to be undoable");
    let after: Vec<u16> = ed.palicos().iter().map(|p| p.level).collect();
    assert_eq!(after, vec![1, 1, 1]);
    assert_eq!(ed.undo_depth(), 0);
}

#[test]
fn an_empty_talisman_box_points_at_plus_and_never_at_the_delete_key() {
    let mut ed = Editor::new();
    ed.slot = 0;
    ed.save = Some(save::empty_switch_save());
    ed.open_charms();
    assert_eq!(ed.screen, Screen::Charms);
    assert!(ed.charms().is_empty());
    // [-] is the one destructive key on every screen now, so create cannot be
    // [-]: the line names [＋] (and [A], which opens the same form).
    assert!(
        ed.message.contains("[＋] で作成"),
        "the create key is plus: {}",
        ed.message
    );
    assert!(
        !ed.message.contains("[-] で作成"),
        "minus deletes a talisman on this screen: {}",
        ed.message
    );
    assert!(
        !ed.message.contains("Y で作成"),
        "Y edits a talisman on this screen: {}",
        ed.message
    );
}

#[test]
fn the_set_editor_re_reads_its_warnings_on_y_and_says_so() {
    let mut ed = Editor::new();
    ed.slot = 0;
    ed.save = Some(save::empty_switch_save());
    assert!(
        ed.open_set_editor(0),
        "the empty save still has the forty set records: {}",
        ed.message
    );
    // The screen the keys are read on; open_set_editor only fills the form.
    ed.screen = Screen::Sets;
    ed.message.clear();
    ed.input(Key::Y);
    assert!(
        ed.message.starts_with("警告"),
        "Y is the key that answers here: {}",
        ed.message
    );
    // The panel's own line and the key have to agree: the console chip sends Y
    // since X is the save key, and this line used to advertise X.
    //
    // The body names no key any more (the seventh review: the chip row under
    // it is the table, and a second copy in the body drifted), so the panel
    // line says how many warnings it has and the chip row names the key.
    let lines = ed.lines();
    let warn_line = lines
        .iter()
        .find(|l| l.text.contains("警告"))
        .map(|l| l.text.clone())
        .unwrap_or_default();
    assert!(
        warn_line.contains("警告") && !warn_line.contains("[Y]"),
        "the panel line counts the warnings and names no key: {}",
        warn_line
    );
    let key_line = ed
        .key_facts()
        .into_iter()
        .find(|f| f.key == "Y")
        .map(|f| f.label)
        .unwrap_or_default();
    assert_eq!(
        key_line, "警告",
        "the key guide has to name Y for the warnings"
    );
}
