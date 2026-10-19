//! Checks against a real 175 quest MHXX save.
//!
//! The save path can be overridden with MHNX_REAL_SAVE. If the file is missing
//! these tests FAIL rather than pass quietly; set MHNX_SKIP_REAL_SAVE=1 to skip
//! them on a machine that has no dump.

use std::path::PathBuf;

use mhnx_core::dlc;
use mhnx_core::save::Save;

const DEFAULT_SAVE: &str =
    r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system";

fn real_save() -> Option<PathBuf> {
    let p = std::env::var("MHNX_REAL_SAVE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SAVE));
    if p.exists() {
        return Some(p);
    }
    if std::env::var("MHNX_SKIP_REAL_SAVE").is_ok() {
        return None;
    }
    panic!(
        "no save at {} - point MHNX_REAL_SAVE at one, or set MHNX_SKIP_REAL_SAVE=1",
        p.display()
    );
}

fn load(p: &PathBuf) -> Save {
    Save::from_bytes(&std::fs::read(p).unwrap()).unwrap()
}

#[test]
fn the_real_save_has_the_expected_geometry() {
    let Some(p) = real_save() else { return };
    let s = load(&p);
    // The MHXX layout, taken from the MHX Data Manager source (which targets
    // the smaller MHX: 0x34 / 0x45D4 / 140 slots) and moved for MHXX.
    assert_eq!(s.dlc_section_offset(), 0xB280);
    assert_eq!(dlc::BITMASK_OFFSET, 0x4C);
    assert_eq!(dlc::QUEST_ARRAY_OFFSET, 0x45F4);
    // The two arrays end exactly on the first character slot, and the slot base
    // is read from the save rather than hard coded on both sides.
    assert_eq!(
        s.dlc_section_offset() as usize
            + dlc::QUEST_ARRAY_OFFSET
            + dlc::QUEST_SLOTS * dlc::QUEST_SLOT_SIZE
            + dlc::EQUIP_SLOTS * dlc::EQUIP_SLOT_SIZE,
        s.slot_base(0).unwrap() as usize
    );
    assert_eq!(s.slot_base(0), Some(0x126474));
}

#[test]
fn every_record_is_self_describing() {
    let Some(p) = real_save() else { return };
    let s = load(&p);
    let quests = dlc::list_quests(&s);
    assert!(quests.len() >= 100, "expected a populated save, got {}", quests.len());
    for q in &quests {
        assert!(q.quest_id > 1_000_000 && q.quest_id < 2_000_000, "id {}", q.quest_id);
        assert!(q.size > 0 && (q.size as usize) <= dlc::QUEST_MAX_PAYLOAD, "size {}", q.size);
        assert_eq!(q.data.len(), q.size as usize);
        assert_eq!(&q.data[..4], b"ARC\0", "slot {} is not an arc", q.slot);
    }
    let mut ids: Vec<i32> = quests.iter().map(|q| q.quest_id).collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "quest ids must be unique");
}

#[test]
fn the_installed_set_is_not_indexed_by_slot() {
    // This is the test that killed "bit i = slot i". The quest array has 205
    // slots, so a slot indexed set can never have a bit above 204 set.
    let Some(p) = real_save() else { return };
    let s = load(&p);
    let m = dlc::bitmask(&s).unwrap();
    let set: Vec<usize> = (0..dlc::BITMASK_LEN * 8)
        .filter(|i| (m[i / 8] >> (i % 8)) & 1 == 1)
        .collect();

    assert!(
        set.iter().any(|b| *b >= dlc::QUEST_SLOTS),
        "bits above slot 204 are set ({:?}), so the set is catalogue indexed",
        set.iter().filter(|b| **b >= dlc::QUEST_SLOTS).collect::<Vec<_>>()
    );

    // The set still describes this save: 175 set bits for 175 records.
    let quests = dlc::list_quests(&s);
    assert_eq!(set.len(), quests.len(), "one bit per installed quest");
    assert_eq!(dlc::installed_bit_count(&s), quests.len());
}

#[test]
fn replacing_a_quest_keeps_the_slot_the_id_and_the_mask() {
    let Some(p) = real_save() else { return };
    let src = std::fs::read(&p).unwrap();
    let mut s = Save::from_bytes(&src).unwrap();
    let before = dlc::list_quests(&s);
    let victim = before[0].clone();
    let mask_before = dlc::bitmask(&s).unwrap();

    let mut payload = victim.data.clone();
    payload[8] ^= 0xFF;
    let slot = dlc::install_quest(&mut s, victim.quest_id, &payload).unwrap();
    assert_eq!(slot, victim.slot, "must stay in the same slot");

    let after = dlc::list_quests(&s);
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0].quest_id, victim.quest_id);
    assert_eq!(after[0].data, payload);

    // A replace must not touch the catalogue indexed set at all.
    assert_eq!(dlc::bitmask(&s).unwrap(), mask_before);

    // And nothing else moved.
    let out = s.to_bytes();
    let orig = Save::from_bytes(&src).unwrap();
    for (a, b) in after.iter().zip(dlc::list_quests(&orig).iter()).skip(1) {
        assert_eq!(a.quest_id, b.quest_id);
        assert_eq!(a.data, b.data);
    }
    assert_eq!(
        orig.dlc_palicoes(),
        Save::from_bytes(&out).unwrap().dlc_palicoes()
    );
}

#[test]
fn adding_a_quest_is_refused_and_writes_nothing() {
    let Some(p) = real_save() else { return };
    let src = std::fs::read(&p).unwrap();
    let mut s = Save::from_bytes(&src).unwrap();
    let body_before = s.body().to_vec();

    // 9990001 is deliberately not in the save.
    assert_eq!(dlc::find_quest(&s, 9990001), None);
    let r = dlc::install_quest(&mut s, 9990001, b"ARC\0test-payload");
    assert!(r.is_err(), "adding a quest is not supported");
    assert_eq!(s.body(), &body_before[..], "a refused install must not write");
    assert_eq!(dlc::list_quests(&s).len(), dlc::list_quests(&Save::from_bytes(&src).unwrap()).len());
}

#[test]
fn a_full_replace_round_trips_through_a_file() {
    let Some(p) = real_save() else { return };
    let mut s = load(&p);
    let quests = dlc::list_quests(&s);
    let victim = quests[quests.len() / 2].clone();
    let mask_before = dlc::bitmask(&s).unwrap();

    let mut payload = victim.data.clone();
    for b in payload.iter_mut().take(4) {
        *b ^= 0xFF;
    }
    dlc::install_quest(&mut s, victim.quest_id, &payload).unwrap();

    let back = Save::from_bytes(&s.to_bytes()).unwrap();
    assert_eq!(dlc::bitmask(&back).unwrap(), mask_before);
    let got = dlc::list_quests(&back)
        .into_iter()
        .find(|q| q.quest_id == victim.quest_id)
        .expect("the quest is still there");
    assert_eq!(got.slot, victim.slot);
    assert_eq!(got.data, payload);
}
