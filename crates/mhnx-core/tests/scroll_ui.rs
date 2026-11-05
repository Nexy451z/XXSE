//! The scroll model every list shares.
//!
//! The Switch layer draws what these functions answer and the pad, the right
//! stick, a finger on the rows and a finger on the bar all go through them. The
//! tests below pin the three things a scrollbar can get wrong: a thumb that is
//! not the share of the list that is on screen, a thumb that cannot reach the
//! ends, and a drag that does not land on the window the thumb showed.

use mhnx_core::hbedit::{
    list_end, page_move, scroll_track, KeyRepeat, ScrollBar, SCROLL_BAR_W, SCROLL_HIT_W,
    SCROLL_MIN_THUMB, SCROLL_TEXT_GAP,
};

/// The track the equipment box uses: nine 56px rows.
const TRACK: i32 = 504;

#[test]
fn a_thumb_is_the_share_of_the_list_that_is_on_screen() {
    // Ten rows, nine of them visible: the thumb covers nine tenths of the track.
    let bar = ScrollBar::new(10, 9, 0);
    assert!(bar.active());
    assert_eq!(bar.thumb_len(TRACK), TRACK * 9 / 10);

    // A list that fits has nothing to scroll and draws no thumb at all.
    let fits = ScrollBar::new(9, 9, 0);
    assert!(!fits.active());
    assert_eq!(fits.thumb_len(TRACK), TRACK);

    // 340 rows in a 504px track is 13px, which no finger can hold: the bar
    // keeps the minimum instead of shrinking to a line.
    let long = ScrollBar::new(340, 9, 0);
    assert_eq!(long.thumb_len(TRACK), SCROLL_MIN_THUMB);
    assert!(long.thumb_len(TRACK) < TRACK / 10);

    // The thumb never leaves the track, whatever the numbers say.
    let huge = ScrollBar::new(usize::MAX / 4, 1, 0);
    assert_eq!(huge.thumb_len(TRACK), SCROLL_MIN_THUMB);
    assert_eq!(huge.thumb_len(0), 0);
}

#[test]
fn the_thumb_reaches_both_ends_of_the_track_exactly() {
    let bar = ScrollBar::new(340, 9, 0);
    let usable = TRACK - bar.thumb_len(TRACK);
    assert_eq!(bar.thumb_offset(TRACK), 0);

    // The last window puts the thumb flush against the bottom of the track.
    let last = ScrollBar::new(340, 9, 331);
    assert_eq!(last.start(), 331);
    assert_eq!(last.thumb_offset(TRACK), usable);

    // A window past the end is pulled back so the last screen is full.
    let over = ScrollBar::new(340, 9, 1_000);
    assert_eq!(over.start(), 340 - 9);

    // The offset only ever grows as the window moves down.
    let mut prev = 0;
    for start in 0..=(340 - 9) {
        let off = ScrollBar::new(340, 9, start).thumb_offset(TRACK);
        assert!(off >= prev, "offset went backwards at {}: {} < {}", start, off, prev);
        assert!(off <= usable);
        prev = off;
    }
}

#[test]
fn dragging_the_thumb_lands_on_the_window_the_thumb_showed() {
    // A drag reads the thumb's own position and asks for the row it means. The
    // thumb is shorter than its share of the table here (the minimum), so not
    // every row can be named; the answer has to be the nearest one, not a jump
    // somewhere else in the list.
    let total = 340;
    let visible = 9;
    let bar = ScrollBar::new(total, visible, 0);
    for start in 0..=(total - visible) {
        let window = ScrollBar::new(total, visible, start);
        let off = window.thumb_offset(TRACK);
        let back = window.start_for_offset(TRACK, off);
        let diff = if back > start { back - start } else { start - back };
        assert!(diff <= 1, "start {} came back as {}", start, back);
    }

    // The two ends are exact, and beyond them the answer clamps instead of
    // wrapping to the other end of the table.
    assert_eq!(bar.start_for_offset(TRACK, 0), 0);
    assert_eq!(bar.start_for_offset(TRACK, TRACK), total - visible);
    assert_eq!(bar.start_for_offset(TRACK, -500), 0);
    assert_eq!(bar.start_for_offset(TRACK, 5_000), total - visible);

    // A list that fits cannot be dragged anywhere.
    assert_eq!(ScrollBar::new(4, 9, 0).start_for_offset(TRACK, 100), 0);
}

#[test]
fn a_page_is_what_the_screen_shows_and_stops_at_the_ends() {
    // Nine rows on screen means nine rows a page, both ways.
    assert_eq!(page_move(0, 340, 9, 1), 9);
    assert_eq!(page_move(40, 340, 9, -1), 31);

    // The ends clamp: a page that would run off either end stops on the last
    // row, it never wraps to the other end of the list.
    assert_eq!(page_move(335, 340, 9, 1), 339);
    assert_eq!(page_move(3, 340, 9, -1), 0);
    assert_eq!(page_move(0, 0, 9, 1), 0);

    // A screen that shows one row pages by one, not by zero.
    assert_eq!(page_move(5, 340, 0, 1), 6);

    // dir 0 is not a page: the cursor stays where it is, inside the list.
    assert_eq!(page_move(7, 340, 9, 0), 7);
    assert_eq!(page_move(999, 340, 9, 0), 339);
}

#[test]
fn the_first_and_last_row_are_one_step_away() {
    assert_eq!(list_end(340, false), 0);
    assert_eq!(list_end(340, true), 339);
    assert_eq!(list_end(0, false), 0);
    assert_eq!(list_end(0, true), 0);
    assert_eq!(list_end(1, true), 0);
}

#[test]
fn a_held_direction_repeats_after_eight_frames_then_every_three_or_four() {
    let mut repeat = KeyRepeat::new();
    let mut fires = Vec::new();
    for frame in 1..=60u32 {
        if repeat.tick(true) {
            fires.push(frame);
        }
    }

    // 133ms to the first repeat, then 66ms each, then 50ms once it is running.
    assert_eq!(fires[0], KeyRepeat::FIRST);
    assert_eq!(fires[1] - fires[0], KeyRepeat::NEXT);
    for pair in fires.windows(2).take(8) {
        assert_eq!(pair[1] - pair[0], KeyRepeat::NEXT, "{:?}", fires);
    }
    let last = fires.len() - 1;
    assert_eq!(fires[last] - fires[last - 1], KeyRepeat::FAST, "{:?}", fires);

    // The old cadence took 12 frames to start and never sped up.
    assert!(KeyRepeat::FIRST < 12);
    assert!(KeyRepeat::FAST < KeyRepeat::NEXT);

    // Letting go puts the timer back: the next press is a single row again.
    assert!(!repeat.tick(false));
    assert_eq!(repeat.emitted(), 0);
    let mut fired = 0;
    for _ in 0..KeyRepeat::FIRST - 1 {
        if repeat.tick(true) {
            fired += 1;
        }
    }
    assert_eq!(fired, 0);
    assert!(repeat.tick(true));
}

#[test]
fn the_bar_never_sits_on_a_rows_value() {
    // The list panel is 796px wide at x = 20. The row's value is right aligned
    // at `text_right`, which sits one gap left of the bar's touch area, so the
    // two can never overlap whatever the value holds - and the row's own tap
    // area stops at the same line, so a tap can never mean both.
    const PANEL_X: i32 = 20;
    const PANEL_W: i32 = 796;
    const ROW_X: i32 = PANEL_X + 8;

    let track = scroll_track(PANEL_X, PANEL_W);
    assert_eq!(track.text_right, track.hit_x - SCROLL_TEXT_GAP);
    assert!(SCROLL_TEXT_GAP >= 8, "no glyph may touch the thumb");
    assert!(
        track.bar_x >= track.text_right + 8,
        "bar at {} on value ending {}",
        track.bar_x,
        track.text_right
    );
    assert_eq!(track.bar_x + track.bar_w, PANEL_X + PANEL_W - 8);
    assert_eq!(track.hit_x + track.hit_w, PANEL_X + PANEL_W);
    assert_eq!(track.hit_w, SCROLL_HIT_W);
    // 40px of thumb is what a finger can hold; the old 20 is not.
    assert_eq!(SCROLL_HIT_W, 40);
    assert_eq!(track.bar_w, SCROLL_BAR_W);
    assert_eq!(track.bar_w, 6, "the bar itself stays a line");

    // The row - its tap area and its value - stops before the bar's zone.
    let row_w = track.row_right - ROW_X;
    assert!(track.row_ends_before_the_bar(ROW_X, row_w));
    assert!(!track.hits(ROW_X + row_w - 1), "the row ends outside the zone");
    assert!(track.hits(track.hit_x));

    // The hit area is the bar plus a margin on both sides, and the bar is
    // inside it.
    assert!(track.hits(track.bar_x));
    assert!(track.hits(track.hit_x));
    assert!(!track.hits(track.hit_x - 1));
    assert!(!track.hits(PANEL_X + PANEL_W));
    assert!(track.hit_x < track.bar_x && track.bar_x + track.bar_w < track.hit_x + track.hit_w);
}
