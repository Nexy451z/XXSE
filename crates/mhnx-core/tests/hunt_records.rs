//! Monster hunt records: slain, captured, size, and the crown rule.
//!
//! Synthetic saves are built in memory so this file runs anywhere. The
//! real-data tests read the verified save at `C:\anyWorkspace\nx\jksv_ready\system`
//! and are skipped (not failed) when it is absent. Nothing here writes to it:
//! `Save::from_bytes` copies the bytes into its own buffer and every write test
//! works on a blank save held in memory.

use std::path::PathBuf;

use mhnx_core::hunts::{self, Crown, MAX_COUNT, MONSTERS};
use mhnx_core::save::{self, Save, BODY_SIZE, SLOT_STRIDE};

/// Body offset of the first character slot (also stored at body[0x10]).
const FIRST_SLOT_BASE: u32 = 0x126474;

fn real_save_path() -> PathBuf {
    PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system")
}

/// The verified save, or `None` so the caller can skip.
fn real_save() -> Option<Save> {
    let p = real_save_path();
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return None;
    }
    Some(Save::from_bytes(&std::fs::read(&p).unwrap()).expect("the verified save must parse"))
}

/// A zeroed 3DS body with the marker and the three slot bases filled in.
fn blank_save() -> Save {
    let mut body = vec![0u8; BODY_SIZE];
    body[0] = 0xC6;
    for slot in 0..3 {
        let base = FIRST_SLOT_BASE as usize + slot * SLOT_STRIDE;
        body[0x10 + 4 * slot..0x14 + 4 * slot].copy_from_slice(&(base as u32).to_le_bytes());
        body[0x04 + slot] = 1;
    }
    Save::from_bytes(&body).expect("the synthetic save must parse")
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

#[test]
fn the_monster_count_is_the_one_the_tables_hold() {
    // save.rs documents the count by arithmetic; this module names it too, so
    // the two must not drift apart.
    assert_eq!(MONSTERS, save::MONSTER_COUNT);
    assert_eq!(MONSTERS * 2, save::MONSTER_TABLE_SIZE);
    assert_eq!(MONSTERS * 4, save::MONSTER_SIZE_TABLE_SIZE);
}

#[test]
fn the_three_tables_abut_and_end_at_the_equipment_box() {
    // slain 0x5EA6 + 137*2 = captured 0x5FB8, + 137*2 = size 0x60CA,
    // + 137*4 = 0x62EE, which is exactly where the equipment box begins.
    assert_eq!(
        save::OFF_MONSTER_HUNT + MONSTERS * 2,
        save::OFF_MONSTER_CAPTURE
    );
    assert_eq!(
        save::OFF_MONSTER_CAPTURE + MONSTERS * 2,
        save::OFF_MONSTER_SIZE
    );
    assert_eq!(
        save::OFF_MONSTER_SIZE + MONSTERS * 4,
        save::OFF_EQUIPMENT_BOX
    );
}

#[test]
fn a_blank_slot_reads_zero_records() {
    let s = blank_save();
    for slot in 0..3 {
        assert_eq!(hunts::killed(&s, slot, 0), Some(0));
        assert_eq!(hunts::killed(&s, slot, MONSTERS - 1), Some(0));
        assert_eq!(hunts::captured(&s, slot, 7), Some(0));
        assert_eq!(hunts::size_range(&s, slot, 70), Some((0, 0)));
    }
}

#[test]
fn every_accessor_refuses_a_monster_past_the_table() {
    let mut s = blank_save();
    assert_eq!(hunts::killed(&s, 0, MONSTERS), None);
    assert_eq!(hunts::captured(&s, 0, MONSTERS), None);
    assert_eq!(hunts::size_range(&s, 0, MONSTERS), None);
    // A slot that does not exist is refused too, reads and writes alike.
    assert_eq!(hunts::killed(&s, save::MAX_SLOTS, 0), None);
    assert_eq!(hunts::size_range(&s, save::MAX_SLOTS, 0), None);
    assert!(!hunts::set_killed(&mut s, 0, MONSTERS, 1));
    assert!(!hunts::set_captured(&mut s, 0, MONSTERS, 1));
    assert!(!hunts::set_size_range(&mut s, 0, MONSTERS, (90, 120)));
    assert!(!hunts::set_killed(&mut s, save::MAX_SLOTS, 0, 1));
}

#[test]
fn a_record_written_to_a_synthetic_save_touches_only_its_own_bytes() {
    let mut s = blank_save();
    let before = s.to_bytes();
    let m = 37usize;
    assert!(hunts::set_killed(&mut s, 0, m, 1234));
    assert!(hunts::set_captured(&mut s, 0, m, 7));
    assert!(hunts::set_size_range(&mut s, 0, m, (95, 130)));
    assert_eq!(hunts::killed(&s, 0, m), Some(1234));
    assert_eq!(hunts::captured(&s, 0, m), Some(7));
    assert_eq!(hunts::size_range(&s, 0, m), Some((95, 130)));

    let after = s.to_bytes();
    let base = FIRST_SLOT_BASE as usize;
    let slain = base + save::OFF_MONSTER_HUNT + 2 * m;
    let caught = base + save::OFF_MONSTER_CAPTURE + 2 * m;
    let sized = base + save::OFF_MONSTER_SIZE + 4 * m;
    let diff: Vec<usize> = (0..after.len())
        .filter(|i| after[*i] != before[*i])
        .collect();
    assert!(!diff.is_empty());
    for d in diff {
        assert!(
            (d >= slain && d < slain + 2)
                || (d >= caught && d < caught + 2)
                || (d >= sized && d < sized + 4),
            "byte {d} moved outside the three records (slain {slain}, caught {caught}, size {sized})"
        );
    }
}

#[test]
fn counts_stop_at_the_game_maximum() {
    let mut s = blank_save();
    assert!(hunts::set_killed(&mut s, 0, 0, u16::MAX));
    assert_eq!(hunts::killed(&s, 0, 0), Some(MAX_COUNT));
    assert!(hunts::set_captured(&mut s, 0, 0, MAX_COUNT + 1));
    assert_eq!(hunts::captured(&s, 0, 0), Some(MAX_COUNT));
}

#[test]
fn a_size_record_is_written_as_a_whole_ordered_pair() {
    let mut s = blank_save();
    // One end alone stands for both ends.
    assert!(hunts::set_size_range(&mut s, 0, 1, (0, 110)));
    assert_eq!(hunts::size_range(&s, 0, 1), Some((110, 110)));
    // The wrong way round is swapped.
    assert!(hunts::set_size_range(&mut s, 0, 1, (120, 95)));
    assert_eq!(hunts::size_range(&s, 0, 1), Some((95, 120)));
    // (0, 0) keeps its meaning: no specimen.
    assert!(hunts::set_size_range(&mut s, 0, 1, (0, 0)));
    assert_eq!(hunts::size_range(&s, 0, 1), Some((0, 0)));
}

#[test]
fn crown_marks_follow_the_window() {
    // (mini_le, gold_ge) - the two thresholds of the 大小金 judgement.
    let window = (90u16, 123u16);
    assert_eq!(hunts::crown(0, window), Crown::default());
    assert_eq!(
        hunts::crown(90, window),
        Crown {
            mini: true,
            gold: false
        }
    );
    assert_eq!(
        hunts::crown(89, window),
        Crown {
            mini: true,
            gold: false
        }
    );
    assert_eq!(hunts::crown(91, window), Crown::default());
    assert_eq!(hunts::crown(122, window), Crown::default());
    assert_eq!(
        hunts::crown(123, window),
        Crown {
            mini: false,
            gold: true
        }
    );
    assert_eq!(
        hunts::crown(150, window),
        Crown {
            mini: false,
            gold: true
        }
    );
    // A whole record: the smallest specimen can be mini and the largest gold.
    assert_eq!(
        hunts::record_crown((88, 120), window),
        Crown {
            mini: true,
            gold: false
        }
    );
    assert_eq!(
        hunts::record_crown((95, 130), window),
        Crown {
            mini: false,
            gold: true
        }
    );
    assert_eq!(
        hunts::record_crown((85, 130), window),
        Crown {
            mini: true,
            gold: true
        }
    );
    assert_eq!(hunts::record_crown((0, 0), window), Crown::default());
}

#[test]
fn a_fixed_size_monster_shows_gold() {
    // All three thresholds are 0 for a monster the game gives one size, so any
    // recorded size is a gold crown and none is mini (the game's own rule).
    let window = (0u16, 0u16);
    assert_eq!(
        hunts::crown(100, window),
        Crown {
            mini: false,
            gold: true
        }
    );
    assert_eq!(
        hunts::record_crown((100, 100), window),
        Crown {
            mini: false,
            gold: true
        }
    );
    assert_eq!(hunts::crown(0, window), Crown::default());
    assert_eq!(hunts::record_crown((0, 0), window), Crown::default());
}

#[test]
fn the_verified_save_reads_back_like_a_raw_scan_of_the_tables() {
    let Some(s) = real_save() else {
        return;
    };
    let bytes = s.to_3ds_bytes();
    let base = s.slot_bases()[0] as usize;
    let (mut slain, mut caught, mut sized) = (0, 0, 0);
    for m in 0..MONSTERS {
        let k = u16_at(&bytes, base + save::OFF_MONSTER_HUNT + 2 * m);
        let c = u16_at(&bytes, base + save::OFF_MONSTER_CAPTURE + 2 * m);
        let lo = u16_at(&bytes, base + save::OFF_MONSTER_SIZE + 4 * m);
        let hi = u16_at(&bytes, base + save::OFF_MONSTER_SIZE + 4 * m + 2);
        assert_eq!(hunts::killed(&s, 0, m), Some(k), "slain {m}");
        assert_eq!(hunts::captured(&s, 0, m), Some(c), "captured {m}");
        assert_eq!(hunts::size_range(&s, 0, m), Some((lo, hi)), "size {m}");
        slain += (k > 0) as u32;
        caught += (c > 0) as u32;
        sized += (lo != 0 || hi != 0) as u32;
    }
    // What the verified save holds; a different save would change these numbers.
    assert_eq!((slain, caught, sized), (129, 47, 78));
}

#[test]
fn the_verified_save_records_look_like_what_the_game_shows() {
    let Some(s) = real_save() else {
        return;
    };
    for m in 0..MONSTERS {
        let (lo, hi) = hunts::size_range(&s, 0, m).unwrap();
        assert!(lo <= hi, "monster {m}: {lo} > {hi}");
        if lo != 0 || hi != 0 {
            // Percentages of the base size, the number the game prints.
            assert!(
                (20..=300).contains(&lo) && (20..=300).contains(&hi),
                "monster {m}: {lo}..{hi} is not a size percentage"
            );
        }
        assert!(hunts::killed(&s, 0, m).unwrap() <= MAX_COUNT);
        assert!(hunts::captured(&s, 0, m).unwrap() <= MAX_COUNT);
    }
}

#[test]
fn the_game_keeps_no_size_or_capture_record_for_small_and_fixed_size_monsters() {
    let Some(s) = real_save() else {
        return;
    };
    // 0-based 71..=104 are the small monsters (Aptonoth .. Moofah): hunted, never
    // captured and never sized, which is what the three arrays showed.
    let mut small_slain = 0u32;
    for m in 71..=104 {
        assert_eq!(
            hunts::captured(&s, 0, m),
            Some(0),
            "small monster {m} captured"
        );
        assert_eq!(
            hunts::size_range(&s, 0, m),
            Some((0, 0)),
            "small monster {m} has a size record"
        );
        small_slain += hunts::killed(&s, 0, m).unwrap() as u32;
    }
    assert!(
        small_slain > 1000,
        "small monsters are hunted a lot: {small_slain}"
    );
    // The only hunted large monsters without a size record are the ones whose
    // size the game fixes: Furious Rajang, Akantor, Ukanlos, Savage Deviljho,
    // Alatreon, Amatsu, Nakarkos.
    for m in [22usize, 29, 33, 35, 41, 46, 69] {
        assert!(
            hunts::killed(&s, 0, m).unwrap() > 0,
            "monster {m} not hunted"
        );
        assert_eq!(
            hunts::size_range(&s, 0, m),
            Some((0, 0)),
            "fixed-size monster {m} has a size record"
        );
    }
}
