//! The village request flag map, against the real save and against a synthetic one.
//!
//! The real dump is only ever read: `std::fs::read` hands back bytes, and every
//! write here lands in the parsed `Save`, which is an in-memory copy. Nothing in
//! this file opens the dump, or a copy of it, for writing.
//!
//! The count asserted below (742) is a property of the stable copy the other
//! real-data tests use, not of save files in general: it is the number of event
//! flags that character had set when the dump was taken.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mhnx_core::requests::{self, FLAG_BYTES, FLAGS, OFF_FLAGS};
use mhnx_core::save::{self, Save, SaveKind};

fn real_save() -> PathBuf {
    PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system")
}

/// The parsed real save, or None when this machine has no dump.
fn load_real() -> Option<Save> {
    let p = real_save();
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return None;
    }
    let bytes = std::fs::read(&p).expect("the dump must be readable");
    Some(Save::from_bytes(&bytes).expect("the dump must parse"))
}

#[test]
fn real_save_holds_742_request_flags() {
    let Some(s) = load_real() else { return };
    assert_eq!(s.kind(), SaveKind::Switch);

    for slot in 0..3 {
        assert_eq!(requests::count(&s, slot), 742, "slot {}", slot);
    }

    // The same bits, read one by one, must agree with the byte count.
    let all = requests::flags_near(&s, 0, 0, FLAGS);
    assert_eq!(all.len(), FLAGS);
    assert_eq!(all.iter().filter(|b| **b).count(), 742);
    for (flag, v) in all.iter().enumerate() {
        assert_eq!(requests::get(&s, 0, flag), Some(*v), "flag {}", flag);
    }

    // The map ends inside its 192 bytes on this dump: the highest set bit is
    // 1507, so nothing has leaked in from the bytes after it.
    let top = all.iter().rposition(|b| *b).expect("some flags are set");
    assert_eq!(top, 1507);

    // All three slots carry the same 192 bytes in this dump (slots 2 and 3 are
    // copies of 1), which is the cross-check the position was accepted on.
    let mut maps = Vec::new();
    for slot in 0..3 {
        let base = s.slot_base(slot).unwrap() as usize;
        maps.push(s.body()[base + OFF_FLAGS..base + OFF_FLAGS + FLAG_BYTES].to_vec());
    }
    assert_eq!(maps[0], maps[1]);
    assert_eq!(maps[1], maps[2]);
    assert_eq!(maps[0].len(), FLAG_BYTES);
}

#[test]
fn set_and_get_do_not_disturb_other_bits() {
    let Some(mut s) = load_real() else { return };
    let pristine = s.body().to_vec();
    let base = s.slot_base(0).unwrap() as usize;

    // A bit the save has never set, and one it has: the two writes must behave
    // the same way in both directions.
    let clear: usize = (0..FLAGS)
        .find(|f| requests::get(&s, 0, *f) == Some(false))
        .expect("the map is not full");
    let set_bit: usize = (0..FLAGS)
        .find(|f| requests::get(&s, 0, *f) == Some(true))
        .expect("the map is not empty");

    assert!(requests::set(&mut s, 0, clear, true));
    assert_eq!(requests::get(&s, 0, clear), Some(true));
    assert_eq!(requests::count(&s, 0), 743);
    assert_eq!(requests::get(&s, 0, set_bit), Some(true), "an untouched bit must keep its value");
    assert!(requests::set(&mut s, 0, set_bit, false));
    assert_eq!(requests::get(&s, 0, set_bit), Some(false));
    assert_eq!(requests::count(&s, 0), 742);

    // Only the bytes those two flags live in may differ, and each by exactly
    // the one bit - the two flags can share a byte, in which case that byte
    // differs by both bits and no other byte may move at all.
    let changed = s.body();
    let mut want: BTreeMap<usize, u8> = BTreeMap::new();
    *want.entry(base + OFF_FLAGS + clear / 8).or_insert(0) ^= 1 << (clear % 8);
    *want.entry(base + OFF_FLAGS + set_bit / 8).or_insert(0) ^= 1 << (set_bit % 8);
    let got: BTreeMap<usize, u8> = (0..changed.len())
        .filter(|i| changed[*i] != pristine[*i])
        .map(|i| (i, changed[i] ^ pristine[i]))
        .collect();
    assert_eq!(got, want, "a write must change exactly its own bit");

    // Putting both bits back restores the body byte for byte, which is the
    // promise the module makes: a request toggle is reversible.
    assert!(requests::set(&mut s, 0, clear, false));
    assert!(requests::set(&mut s, 0, set_bit, true));
    assert_eq!(s.body(), &pristine[..], "clearing and setting back must restore the map");
    assert_eq!(requests::count(&s, 0), 742);
}

#[test]
fn bits_run_least_significant_first() {
    let mut s = save::empty_switch_save();
    let base = s.slot_base(0).unwrap() as usize;
    assert!(requests::set(&mut s, 0, 0, true));
    assert!(requests::set(&mut s, 0, 9, true));
    let map = &s.body()[base + OFF_FLAGS..base + OFF_FLAGS + FLAG_BYTES];
    assert_eq!(map[0], 0x01);
    assert_eq!(map[1], 0x02);
    assert_eq!(requests::flags_near(&s, 0, 0, 3), vec![true, false, false]);
    assert_eq!(requests::flags_near(&s, 0, 8, 2), vec![false, true]);
}

#[test]
fn out_of_range_is_refused() {
    // A synthetic save, so this half of the test also runs on a clean checkout.
    let mut s = save::empty_switch_save();
    let before = s.body().to_vec();

    for flag in [FLAGS, FLAGS + 1, usize::MAX] {
        assert_eq!(requests::get(&s, 0, flag), None, "flag {}", flag);
        assert!(!requests::set(&mut s, 0, flag, true), "flag {}", flag);
        assert!(requests::flags_near(&s, 0, flag, 8).is_empty(), "flag {}", flag);
    }
    for slot in [3, 7, usize::MAX] {
        assert_eq!(requests::get(&s, slot, 0), None, "slot {}", slot);
        assert!(!requests::set(&mut s, slot, 0, true), "slot {}", slot);
        assert_eq!(requests::count(&s, slot), 0, "slot {}", slot);
        assert!(requests::flags_near(&s, slot, 0, 8).is_empty(), "slot {}", slot);
    }
    assert_eq!(s.body(), &before[..], "a refused write must not touch the body");

    // The window is clipped, not rejected: the last partial page still works.
    assert_eq!(requests::flags_near(&s, 0, FLAGS - 3, 10).len(), 3);
    assert!(requests::flags_near(&s, 0, 0, 0).is_empty());
    assert!(requests::flags_near(&s, 0, usize::MAX, usize::MAX).is_empty());
    assert_eq!(requests::count(&s, 0), 0);
}

#[test]
fn an_mhgu_save_is_refused_rather_than_guessed() {
    // A well formed MHGU container, so that only the kind check can reject it.
    let body_len = mhnx_core::MHGU_SIZE - mhnx_core::SWITCH_HEADER_SIZE;
    let mut data = vec![0u8; mhnx_core::MHGU_SIZE];
    data[..mhnx_core::SWITCH_HEADER_SIZE]
        .copy_from_slice(&save::switch_header("system", body_len));
    data[mhnx_core::SWITCH_HEADER_SIZE] = 0xC6;
    for (i, off) in [0x10usize, 0x14, 0x18].iter().enumerate() {
        let v = (save::FIRST_SLOT_BASE as usize + save::MHGU_SLOT_STRIDE * i) as u32;
        let at = mhnx_core::SWITCH_HEADER_SIZE + off;
        data[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    let mut s = Save::from_bytes(&data).expect("the MHGU save must parse");
    assert_eq!(s.kind(), SaveKind::Mhgu);

    let before = s.body().to_vec();
    assert_eq!(requests::get(&s, 0, 0), None);
    assert!(!requests::set(&mut s, 0, 0, true));
    assert_eq!(requests::count(&s, 0), 0);
    assert!(requests::flags_near(&s, 0, 0, 8).is_empty());
    assert_eq!(s.body(), &before[..]);
}
