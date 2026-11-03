//! Quest progress bit maps: cleared / seen / failed.
//!
//! The synthetic saves are built in memory so this file runs anywhere. The
//! real-data tests read the verified save at `C:\anyWorkspace\nx\jksv_ready\system`
//! and are skipped (not failed) when it is absent. Nothing here writes to the
//! real save: `Save::from_bytes` copies the bytes into its own buffer, and the
//! one persistence test writes to a copy under the temp directory.

use std::path::PathBuf;

use mhnx_core::questprogress::{self, Progress};
use mhnx_core::save::{Save, SaveKind, BODY_SIZE, MHGU_SLOT_STRIDE, MHGU_SIZE, SLOT_STRIDE};

const FIRST_SLOT_BASE: u32 = 0x126474;

fn real_save_path() -> PathBuf {
    PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system")
}

/// Read the verified save, or `None` so the caller can skip.
fn real_save_bytes() -> Option<Vec<u8>> {
    let p = real_save_path();
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return None;
    }
    Some(std::fs::read(&p).unwrap())
}

fn real_save() -> Option<Save> {
    let bytes = real_save_bytes()?;
    Some(Save::from_bytes(&bytes).expect("the verified save must parse"))
}

/// A zeroed 3DS body with a valid marker and the three slot bases filled in.
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

fn blank_mhgu() -> Save {
    // A full MHGU file: the 36 byte container header plus the body.
    let mut data = vec![0u8; MHGU_SIZE];
    data[36] = 0xC6;
    for slot in 0..3 {
        let base = FIRST_SLOT_BASE as usize + slot * MHGU_SLOT_STRIDE;
        data[36 + 0x10 + 4 * slot..36 + 0x14 + 4 * slot].copy_from_slice(&(base as u32).to_le_bytes());
        data[36 + 0x04 + slot] = 1;
    }
    Save::from_bytes(&data).expect("the synthetic MHGU save must parse")
}

fn map(save: &Save, slot: usize, kind: Progress) -> Vec<u8> {
    let base = save.slot_base(slot).expect("slot base") as usize;
    let start = base + kind.offset();
    save.body()[start..start + questprogress::MAP_SIZE].to_vec()
}

/// Indices that differ between two maps.
fn diff_indices(a: &[u8], b: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..questprogress::BITS {
        let fa = (a[i >> 3] >> (i & 7)) & 1;
        let fb = (b[i >> 3] >> (i & 7)) & 1;
        if fa != fb {
            out.push(i);
        }
    }
    out
}

#[test]
fn bit_order_is_lsb_first() {
    let mut save = blank_save();
    let base = save.slot_base(0).unwrap() as usize + questprogress::OFF_CLEARED;
    // Bit 9 lives in byte 1 at mask 1 << 1.
    save.body_mut()[base + 1] = 0b0000_0010;
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 9), Some(true));
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 8), Some(false));
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 10), Some(false));
    // Byte 1 bit 0 is index 8, byte 2 bit 0 is index 16.
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 17), Some(false));
}

#[test]
fn index_zero_and_out_of_range_are_refused() {
    let mut save = blank_save();
    let cleared_before = map(&save, 0, Progress::Cleared);
    let seen_before = map(&save, 0, Progress::Seen);
    let failed_before = map(&save, 0, Progress::Failed);
    for kind in [Progress::Cleared, Progress::Seen, Progress::Failed] {
        assert_eq!(questprogress::get(&save, 0, kind, 0), None, "index 0");
        assert_eq!(questprogress::get(&save, 0, kind, questprogress::BITS), None);
        assert_eq!(questprogress::get(&save, 0, kind, 1512), None);
        assert_eq!(questprogress::get(&save, 0, kind, usize::MAX), None);
        assert!(!questprogress::set(&mut save, 0, kind, 0, true), "index 0 written");
        assert!(!questprogress::set(&mut save, 0, kind, questprogress::BITS, true));
    }
    // The refused calls changed nothing at all.
    assert_eq!(map(&save, 0, Progress::Cleared), cleared_before);
    assert_eq!(map(&save, 0, Progress::Seen), seen_before);
    assert_eq!(map(&save, 0, Progress::Failed), failed_before);
    // The last valid index is still accepted.
    assert_eq!(questprogress::BITS - 1, 1508);
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 1508), Some(false));
    assert!(questprogress::set(&mut save, 0, Progress::Cleared, 1508, true));
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 1508), Some(true));
    assert_eq!(diff_indices(&cleared_before, &map(&save, 0, Progress::Cleared)), vec![1508]);
}

#[test]
fn setting_cleared_also_sets_seen() {
    let mut save = blank_save();
    assert!(questprogress::set(&mut save, 0, Progress::Cleared, 500, true));
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 500), Some(true));
    assert_eq!(questprogress::get(&save, 0, Progress::Seen, 500), Some(true));
    assert_eq!(questprogress::get(&save, 0, Progress::Failed, 500), Some(false));
    assert!(questprogress::clears_without_seen(&save, 0).is_empty());
}

#[test]
fn failed_is_read_only() {
    let mut save = blank_save();
    let before = save.to_bytes();
    assert!(!questprogress::set(&mut save, 0, Progress::Failed, 500, true));
    assert!(!questprogress::set(&mut save, 0, Progress::Failed, 500, false));
    assert!(!questprogress::set(&mut save, 1, Progress::Failed, 1, true));
    assert_eq!(save.to_bytes(), before, "a failed write must not touch the save");
    assert_eq!(questprogress::get(&save, 0, Progress::Failed, 500), Some(false));
    assert_eq!(questprogress::count(&save, 0, Progress::Failed), 0);
}

#[test]
fn one_write_touches_only_its_own_bit() {
    let mut save = blank_save();
    let cleared0 = map(&save, 0, Progress::Cleared);
    let seen0 = map(&save, 0, Progress::Seen);
    let failed0 = map(&save, 0, Progress::Failed);

    // 1000 -> byte 125, bit 0.
    assert!(questprogress::set(&mut save, 0, Progress::Cleared, 1000, true));
    let cleared = map(&save, 0, Progress::Cleared);
    let seen = map(&save, 0, Progress::Seen);
    let failed = map(&save, 0, Progress::Failed);

    assert_eq!(diff_indices(&cleared0, &cleared), vec![1000]);
    assert_eq!(diff_indices(&seen0, &seen), vec![1000]);
    assert_eq!(diff_indices(&failed0, &failed), Vec::<usize>::new());
    assert_eq!(cleared[125], 0b0000_0001);
    assert_eq!(seen[125], 0b0000_0001);
    assert!(cleared.iter().enumerate().all(|(i, b)| i == 125 || *b == 0));

    // The other two slots stay empty, and so do the neighbouring bits.
    assert_eq!(questprogress::count(&save, 1, Progress::Cleared), 0);
    assert_eq!(questprogress::count(&save, 2, Progress::Cleared), 0);
    assert_eq!(questprogress::get(&save, 0, Progress::Seen, 999), Some(false));
    assert_eq!(questprogress::get(&save, 0, Progress::Seen, 1001), Some(false));
}

#[test]
fn clearing_seen_is_refused_while_cleared() {
    let mut save = blank_save();
    assert!(questprogress::set(&mut save, 0, Progress::Cleared, 77, true));
    let before = save.to_bytes();
    assert!(!questprogress::set(&mut save, 0, Progress::Seen, 77, false));
    assert_eq!(save.to_bytes(), before, "the refused call must not change the save");
    assert_eq!(questprogress::get(&save, 0, Progress::Seen, 77), Some(true));

    // Once the clear is gone the seen bit can be cleared.
    assert!(questprogress::set(&mut save, 0, Progress::Cleared, 77, false));
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 77), Some(false));
    assert_eq!(questprogress::get(&save, 0, Progress::Seen, 77), Some(true));
    assert!(questprogress::set(&mut save, 0, Progress::Seen, 77, false));
    assert_eq!(questprogress::get(&save, 0, Progress::Seen, 77), Some(false));
}

#[test]
fn count_and_clears_without_seen_track_poked_bits() {
    let mut save = blank_save();
    for index in [3, 5, 900] {
        assert!(questprogress::set(&mut save, 0, Progress::Cleared, index, true));
    }
    assert!(questprogress::set(&mut save, 0, Progress::Seen, 7, true));
    assert_eq!(questprogress::count(&save, 0, Progress::Cleared), 3);
    assert_eq!(questprogress::count(&save, 0, Progress::Seen), 4);
    assert!(questprogress::clears_without_seen(&save, 0).is_empty());

    // Poke a cleared bit past the API, without its seen bit.
    let base = save.slot_base(0).unwrap() as usize + questprogress::OFF_CLEARED;
    save.body_mut()[base + (6 >> 3)] |= 1 << (6 & 7);
    assert_eq!(questprogress::clears_without_seen(&save, 0), vec![6]);
    assert_eq!(questprogress::count(&save, 0, Progress::Cleared), 4);
}

#[test]
fn mhgu_saves_are_refused() {
    let mut save = blank_mhgu();
    assert_eq!(save.kind(), SaveKind::Mhgu);
    for kind in [Progress::Cleared, Progress::Seen, Progress::Failed] {
        assert_eq!(questprogress::get(&save, 0, kind, 10), None);
        assert!(!questprogress::set(&mut save, 0, kind, 10, true));
        assert_eq!(questprogress::count(&save, 0, kind), 0);
    }
    assert!(questprogress::clears_without_seen(&save, 0).is_empty());
}

#[test]
fn a_slot_without_a_base_is_refused() {
    let mut body = vec![0u8; BODY_SIZE];
    body[0] = 0xC6;
    let mut save = Save::from_bytes(&body).unwrap();
    assert_eq!(questprogress::get(&save, 0, Progress::Cleared, 10), None);
    assert!(!questprogress::set(&mut save, 0, Progress::Cleared, 10, true));
    assert!(questprogress::clears_without_seen(&save, 0).is_empty());
}

// ---- real save -------------------------------------------------------------

#[test]
fn real_save_counts_are_755_1220_72() {
    let Some(save) = real_save() else { return };
    assert_eq!(save.kind(), SaveKind::Switch);
    for slot in 0..3 {
        assert_eq!(questprogress::count(&save, slot, Progress::Cleared), 755, "slot {slot}");
        assert_eq!(questprogress::count(&save, slot, Progress::Seen), 1220, "slot {slot}");
        assert_eq!(questprogress::count(&save, slot, Progress::Failed), 72, "slot {slot}");
    }
    // Index 0 is unused in every map, and 1509..1511 are clear too.
    for slot in 0..3 {
        let base = save.slot_base(slot).unwrap() as usize;
        for kind in [Progress::Cleared, Progress::Seen, Progress::Failed] {
            assert_eq!(questprogress::get(&save, slot, kind, 0), None, "index 0 is not addressable");
            let start = base + kind.offset();
            assert_eq!(save.body()[start] & 1, 0, "bit 0 must be clear (slot {slot})");
            assert_eq!(save.body()[start + 188] & 0b1110_0000, 0, "bits 1509..1511 must be clear (slot {slot})");
            // Index 1508 (byte 188, bit 4) is addressable.
            let raw = save.body()[start + 188] & 0b0001_0000 != 0;
            assert_eq!(questprogress::get(&save, slot, kind, 1508), Some(raw), "slot {slot}");
        }
    }
}

#[test]
fn real_save_clears_without_seen_is_the_known_three() {
    let Some(save) = real_save() else { return };
    for slot in 0..3 {
        assert_eq!(questprogress::clears_without_seen(&save, slot), vec![821, 1309, 1392], "slot {slot}");
    }
}

#[test]
fn real_save_accessors_match_a_raw_scan() {
    let Some(save) = real_save() else { return };
    for slot in 0..3 {
        let base = save.slot_base(slot).unwrap() as usize;
        for kind in [Progress::Cleared, Progress::Seen, Progress::Failed] {
            let start = base + kind.offset();
            let mut checked = 0usize;
            for index in 1..questprogress::BITS {
                let raw = (save.body()[start + (index >> 3)] >> (index & 7)) & 1 == 1;
                assert_eq!(questprogress::get(&save, slot, kind, index), Some(raw), "slot {slot} index {index}");
                if raw {
                    checked += 1;
                }
            }
            assert!(checked > 0);
        }
    }
}

#[test]
fn editing_a_copy_leaves_the_real_save_alone() {
    let Some(bytes) = real_save_bytes() else { return };
    let original = Save::from_bytes(&bytes).unwrap();
    let mut copy = Save::from_bytes(&bytes).unwrap();

    // A bit that is clear in all three maps, so the expected diff is exactly
    // the cleared and seen bits of one index.
    let index = (1..questprogress::BITS)
        .find(|i| {
            [Progress::Cleared, Progress::Seen, Progress::Failed]
                .iter()
                .all(|k| questprogress::get(&copy, 0, *k, *i) == Some(false))
        })
        .expect("the maps are not full");

    let cleared_before = map(&copy, 0, Progress::Cleared);
    let seen_before = map(&copy, 0, Progress::Seen);
    let failed_before = map(&copy, 0, Progress::Failed);
    assert!(questprogress::set(&mut copy, 0, Progress::Cleared, index, true));
    assert_eq!(diff_indices(&cleared_before, &map(&copy, 0, Progress::Cleared)), vec![index]);
    assert_eq!(diff_indices(&seen_before, &map(&copy, 0, Progress::Seen)), vec![index]);
    assert_eq!(diff_indices(&failed_before, &map(&copy, 0, Progress::Failed)), Vec::<usize>::new());
    assert_eq!(questprogress::count(&copy, 0, Progress::Cleared), 756);
    assert_eq!(questprogress::count(&copy, 0, Progress::Seen), 1221);
    assert_eq!(questprogress::clears_without_seen(&copy, 0), vec![821, 1309, 1392]);

    // The in-memory copy was the only thing edited; the file on disk still
    // holds the original counts.
    let on_disk = Save::from_bytes(&std::fs::read(real_save_path()).unwrap()).unwrap();
    assert_eq!(questprogress::count(&on_disk, 0, Progress::Cleared), 755);
    assert_eq!(questprogress::count(&on_disk, 0, Progress::Seen), 1220);
    assert_eq!(original.to_bytes(), on_disk.to_bytes());

    // Persisting the edit goes to a temp file, never to the real save.
    let out = std::env::temp_dir().join("mhnx_quest_progress_copy.bin");
    std::fs::write(&out, copy.to_bytes()).unwrap();
    let reread = Save::from_bytes(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(questprogress::get(&reread, 0, Progress::Cleared, index), Some(true));
    assert_eq!(questprogress::get(&reread, 0, Progress::Seen, index), Some(true));
    assert_eq!(questprogress::count(&reread, 0, Progress::Cleared), 756);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn real_save_seen_without_cleared_can_be_cleared() {
    let Some(save) = real_save() else { return };
    // The save has 468 indices that are seen but not cleared; clear the first
    // one, and skip the assertion if a future save has none.
    let Some(index) = (1..questprogress::BITS).find(|i| {
        questprogress::get(&save, 0, Progress::Seen, *i) == Some(true)
            && questprogress::get(&save, 0, Progress::Cleared, *i) == Some(false)
    }) else {
        return;
    };
    let mut copy = save.clone();
    let before = questprogress::count(&copy, 0, Progress::Seen);
    assert!(questprogress::set(&mut copy, 0, Progress::Seen, index, false));
    assert_eq!(questprogress::get(&copy, 0, Progress::Seen, index), Some(false));
    assert_eq!(questprogress::count(&copy, 0, Progress::Seen), before - 1);
}
