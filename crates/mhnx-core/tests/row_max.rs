//! The inspector's [MAX], the one undo step a held key costs, the [＋] quit
//! and the keys that type a number instead of stepping it.
//!
//! The UI review of the Switch build found five screens where [MAX] answered
//! nothing at all (`_ => {}`), an undo stack that one held direction key filled
//! with the same edit four times, a [＋] that threw away unsaved work without a
//! word, and four rows whose numbers could only be stepped. Every rule below
//! lives in `hbedit` so it can be pinned here; the console only draws it.

use mhnx_core::charm::{self, Charm};
use mhnx_core::guildcard;
use mhnx_core::hunts;
use mhnx_core::hbedit::{
    self, Editor, Key, Screen, CHAR_NAME_MAX_BYTES, GUILD_HR_MAX, GUILD_TIME_MAX, HUNT_COUNT_MAX,
    HUNT_SIZE_MAX, OPTION_BRIGHTNESS_MAX, PALICO_ENTHUSIASM_MAX, PALICO_EXP_MAX, PALICO_LEVEL_MAX,
};
use mhnx_core::save;

/// An editor on a synthetic slot: no file behind it, so nothing can be written.
fn editor() -> Editor {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    let mut ed = Editor::new();
    ed.slot = 0;
    ed.save = Some(save);
    ed
}

/// The save inside an editor, for the module accessors the rows write through.
fn save_of(ed: &Editor) -> &save::Save {
    ed.save.as_ref().expect("the fixture has a save")
}

/// The same, with one palico so the roster has a row to MAX.
fn editor_with_palico() -> Editor {
    let mut ed = editor();
    assert!(
        mhnx_core::palico::set_name(ed.save.as_mut().unwrap(), 0, 0, "アイルー"),
        "the roster record has to be nameable"
    );
    ed
}

/// A talisman in box slot 0, seeded the way the game writes one.
fn seed_charm(ed: &mut Editor) -> Charm {
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
    {
        let b = ed
            .save
            .as_mut()
            .and_then(|s| s.equipment_box_mut(0))
            .expect("the box exists");
        b[..charm::CHARM_RECORD].copy_from_slice(&r);
    }
    let c = Charm {
        index: 0,
        slots: 3,
        skill1: 74,
        points1: 6,
        skill2: 70,
        points2: 10,
    };
    assert!(
        ed.save
            .as_mut()
            .unwrap()
            .set_charm_with_kind(0, &c, 10),
        "the seed talisman must be written"
    );
    c
}

// ---------------------------------------------------------------------------
// 1. [MAX] has an answer on every screen that draws it
// ---------------------------------------------------------------------------

#[test]
fn the_hunt_guild_roster_and_option_rows_jump_to_their_own_ceiling() {
    let mut ed = editor();
    // A record has to exist before the list has a row: the screen shows the
    // monsters that were hunted.
    assert!(ed
        .save
        .as_mut()
        .unwrap()
        .set_hunt_count(0, 3, 12));
    assert_eq!(ed.hunt_rows().len(), 1);

    ed.screen = Screen::Hunts;
    ed.hunt_cursor = 0;
    ed.hunt_field = 0;
    assert!(ed.hunt_row_max(0), "{}", ed.message);
    assert_eq!(hunts::killed(save_of(&ed), 0, 3), Some(HUNT_COUNT_MAX));
    assert!(
        ed.message.contains(&format!("{}", HUNT_COUNT_MAX)),
        "the row has to name its ceiling: {}",
        ed.message
    );

    ed.hunt_field = 1;
    assert!(ed.hunt_row_max(0));
    assert_eq!(hunts::captured(save_of(&ed), 0, 3), Some(HUNT_COUNT_MAX));

    // The size pair keeps its order: the edited end is the one that moves.
    ed.hunt_field = 3;
    assert!(ed.hunt_row_max(0));
    assert_eq!(
        hunts::size_range(save_of(&ed), 0, 3),
        Some((HUNT_SIZE_MAX, HUNT_SIZE_MAX))
    );

    ed.screen = Screen::Guild;
    ed.guild_field = 0;
    assert!(ed.guild_row_max(0));
    assert_eq!(guildcard::hunter_rank(save_of(&ed), 0), Some(GUILD_HR_MAX));
    ed.guild_field = 1;
    assert!(ed.guild_row_max(1));
    assert_eq!(guildcard::play_time(save_of(&ed), 0), Some(GUILD_TIME_MAX));
    assert!(ed.message.contains("上限"));

    // The comment row is text: MAX says which key has it, not "nothing".
    ed.guild_field = 2;
    assert!(!ed.guild_row_max(2));
    assert!(ed.message.contains("[A]"), "{}", ed.message);

    ed.screen = Screen::Options;
    assert!(ed.option_row_max(0));
    assert_eq!(save_of(&ed).options().brightness, OPTION_BRIGHTNESS_MAX);
    assert!(ed.option_row_max(1));
    assert_eq!(save_of(&ed).options().rumble, 1);

    let mut ed = editor_with_palico();
    ed.screen = Screen::Roster;
    ed.roster_field = 0;
    assert!(ed.roster_row_max(0));
    assert_eq!(ed.palicos()[0].level, PALICO_LEVEL_MAX);
    ed.roster_field = 1;
    assert!(ed.roster_row_max(0));
    assert_eq!(ed.palicos()[0].enthusiasm, PALICO_ENTHUSIASM_MAX);
    ed.roster_field = 2;
    assert!(ed.roster_row_max(0));
    assert_eq!(ed.palicos()[0].exp, PALICO_EXP_MAX);
}

#[test]
fn a_row_without_a_ceiling_says_so_instead_of_doing_nothing() {
    let mut ed = editor();
    ed.screen = Screen::Char;
    assert!(!ed.char_row_max(0), "the name row is text");
    assert!(
        ed.message.contains(&format!("{}", CHAR_NAME_MAX_BYTES)),
        "the message has to name the real ceiling (31 bytes): {}",
        ed.message
    );
    // The numeric rows all jump.
    for field in [1usize, 2, 3, 4, 5, 6, 9, 10, 16] {
        assert!(ed.char_row_max(field), "field {} must have a maximum", field);
    }

    // A talisman form that is not open cannot answer, and says why.
    assert!(!ed.charm_row_max(2));
    assert!(!ed.message.is_empty());
}

// ---------------------------------------------------------------------------
// 2. One gesture, one undo step
// ---------------------------------------------------------------------------

#[test]
fn a_held_direction_key_spends_one_undo_step_not_four() {
    let mut ed = editor();
    ed.screen = Screen::Char;
    ed.field = 1;
    ed.cursor = 1;

    // The press that starts the gesture.
    ed.input(Key::Left);
    assert_eq!(ed.undo_depth(), 1, "the first press opens the step");

    // The repeats the console loop labels as repeats.
    ed.set_repeating(true);
    for _ in 0..8 {
        ed.input(Key::Left);
    }
    assert_eq!(ed.undo_depth(), 1, "the hold folds into the open step");

    // Release, then the next press is its own step again.
    ed.set_repeating(false);
    ed.input(Key::Left);
    assert_eq!(ed.undo_depth(), 2, "a new gesture is a new step");
}

// ---------------------------------------------------------------------------
// 3. [＋] and the save it would throw away
// ---------------------------------------------------------------------------

#[test]
fn the_quit_key_asks_once_before_it_discards_unsaved_work() {
    let mut ed = editor();
    ed.screen = Screen::Char;

    // Nothing to lose: one press quits, as it always did.
    assert!(ed.press_quit());
    assert!(ed.quit);

    let mut ed = editor();
    ed.screen = Screen::Char;
    ed.dirty = true;
    assert!(!ed.press_quit(), "a dirty save is not discarded on one press");
    assert!(!ed.quit);
    assert!(ed.quit_armed());
    assert!(
        ed.message.contains("もう一度 [＋]"),
        "the offer has to name the key: {}",
        ed.message
    );
    assert!(ed.press_quit(), "the second press is the quit");
    assert!(ed.quit);

    // Any other key takes the offer back: the second press has to be the next
    // thing the user does.
    let mut ed = editor();
    ed.screen = Screen::Char;
    ed.dirty = true;
    assert!(!ed.press_quit());
    ed.input(Key::Down);
    assert!(!ed.quit_armed(), "a move takes the offer away");

    // And the screen's own [＋] goes through the same guard.
    let mut ed = editor();
    ed.screen = Screen::Char;
    ed.dirty = true;
    ed.input(Key::Plus);
    assert!(!ed.quit, "[＋] must not quit a dirty editor on one press");
    ed.input(Key::Plus);
    assert!(ed.quit, "the second [＋] quits");
}

// ---------------------------------------------------------------------------
// 4. Typing a number instead of stepping it
// ---------------------------------------------------------------------------

#[test]
fn a_typed_number_reaches_its_row_and_stops_at_the_ceiling() {
    let mut ed = editor();
    ed.screen = Screen::Guild;

    assert!(ed.apply_guild_hr_text("1200"));
    assert_eq!(guildcard::hunter_rank(save_of(&ed), 0), Some(GUILD_HR_MAX));
    assert!(ed.message.contains("上限"));

    assert!(ed.apply_guild_time_text("12:34"));
    assert_eq!(guildcard::play_time(save_of(&ed), 0), Some(12 * 3600 + 34 * 60));
    assert!(ed.apply_guild_time_text("7425"), "seconds are accepted too");
    assert_eq!(guildcard::play_time(save_of(&ed), 0), Some(7425));
    assert!(!ed.apply_guild_time_text("12:99"), "a minute is 0..=59");
    assert_eq!(
        guildcard::play_time(save_of(&ed), 0),
        Some(7425),
        "a refused value changes nothing"
    );

    let mut ed = editor();
    assert!(ed.save.as_mut().unwrap().set_hunt_count(0, 2, 5));
    ed.screen = Screen::Hunts;
    ed.hunt_cursor = 0;
    ed.hunt_field = 0;
    assert!(ed.apply_hunt_count_text("12000"));
    assert_eq!(hunts::killed(save_of(&ed), 0, 2), Some(HUNT_COUNT_MAX));

    // The item box: [R] types the count of the selected slot.
    let mut ed = editor();
    let mut box_ = vec![mhnx_core::itembox::ItemSlot::default(); mhnx_core::itembox::TOTAL_ITEM_SLOTS];
    box_[0] = mhnx_core::itembox::ItemSlot { id: 1, count: 1 };
    ed.save.as_mut().unwrap().write_item_box(0, &box_).unwrap();
    ed.screen = Screen::Items;
    ed.cursor = 0;
    assert!(ed.apply_item_count_text("120"));
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(after[0].count, mhnx_core::itembox::MAX_COUNT as u8);
    assert!(ed.message.contains("上限"));
    assert!(!ed.apply_item_count_text("abc"), "letters are not a count");
}

// ---------------------------------------------------------------------------
// 5. One delete key: [-] twice, on the talisman list too
// ---------------------------------------------------------------------------

#[test]
fn the_talisman_list_deletes_on_minus_and_never_on_a_or_y() {
    let mut ed = editor();
    seed_charm(&mut ed);
    ed.screen = Screen::Charms;
    ed.cursor = 0;

    // [A] opens the form and deletes nothing. [Y] is the list's search, not a
    // second route to the form and never a delete: neither of them deletes.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "[A] edits");
    assert_eq!(ed.charms().len(), 1, "nothing was deleted");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms);
    let before = ed.charms().len();
    ed.input(Key::Y);
    assert_eq!(
        ed.screen,
        Screen::Charms,
        "[Y] is the filter key, so the list stays"
    );
    assert_eq!(
        ed.take_text_request().map(|(idx, _)| idx),
        Some(mhnx_core::hbedit::TEXT_CHARM_FILTER),
        "[Y] asks for the filter text"
    );
    assert_eq!(ed.charms().len(), before);

    // [-] is the delete, twice, as one undo step.
    ed.input(Key::Minus);
    assert_eq!(ed.charms().len(), before, "the first press only asks");
    assert!(ed.message.contains("2回"), "{}", ed.message);
    assert_eq!(ed.undo_depth(), 0, "asking is not a step");
    ed.input(Key::Minus);
    assert_eq!(ed.charms().len(), before - 1, "[-] deletes");
    assert!(ed.message.contains("削除"), "{}", ed.message);
    assert_eq!(ed.undo_depth(), 1);
    assert!(ed.undo_last());
    assert_eq!(ed.charms().len(), before, "and it can be taken back");

    // [＋] is the create key, the same key the equipment box uses.
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::CharmEdit, "[＋] creates a talisman");
}

// ---------------------------------------------------------------------------
// 6. The talisman form's own ceiling
// ---------------------------------------------------------------------------

#[test]
fn the_talisman_form_max_uses_the_kind_ceiling_not_the_byte() {
    let mut ed = editor();
    seed_charm(&mut ed);
    ed.screen = Screen::Charms;
    ed.cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
    let form = ed.charm_edit.expect("the form is open");
    let limits = charm::kind_limits(form.kind).expect("a shipped kind");

    // Row 2 is the slot count.
    assert!(ed.charm_row_max(2));
    let after = ed.charm_edit.expect("still open");
    assert_eq!(
        after.slots,
        limits.max_slots.min(form.grade.max_slots()),
        "the slot row stops at the tighter of the kind and the grade"
    );

    // Rows 4 and 6 are the two point values.
    let first_cap = hbedit::charm_points_ceiling(&after, false);
    if let Some(cap) = first_cap {
        assert!(ed.charm_row_max(4));
        assert_eq!(ed.charm_edit.expect("open").points1, cap);
    }
    // A row with no value in it says why instead of silently doing nothing.
    assert!(!ed.charm_row_max(0));
    assert!(!ed.message.is_empty());
}
