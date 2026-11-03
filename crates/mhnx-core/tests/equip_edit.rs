//! Editing an equipment record through the save API.

use std::path::PathBuf;

use mhnx_core::equip;
use mhnx_core::equiprefs;
use mhnx_core::save::Save;

fn real_save() -> Option<Save> {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    let p = if stable.exists() {
        stable
    } else {
        PathBuf::from(
            r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
        )
    };
    Save::from_bytes(&std::fs::read(p).ok()?).ok()
}

fn first_used(s: &Save, slot: usize) -> Option<usize> {
    let b = s.equipment_box(slot)?;
    (0..equip::EQUIP_COUNT).find(|i| {
        let o = i * equip::EQUIP_RECORD;
        let rec = &b[o..o + equip::EQUIP_RECORD];
        !equip::record_is_blank(rec)
    })
}

#[test]
fn setting_a_level_changes_only_the_level_bits() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = first_used(&s, 0) else { return };
    let before = s.equipment_at(0, idx).expect("decodes");
    let body_before = s.body().to_vec();
    {
        let rec = s.equipment_record_mut(0, idx).expect("record");
        assert!(equip::set_level(rec, 7));
    }
    let after = s.equipment_at(0, idx).expect("decodes");
    assert_eq!(after.level, 7);
    assert_eq!(after.kind, before.kind, "the type must not move");
    assert_eq!(after.transmog, before.transmog);
    assert_eq!(after.transmog_level, before.transmog_level);
    assert_eq!(after.id, before.id);

    // Only the two head bytes may differ.
    let body_after = s.body().to_vec();
    let base = s.slot_base(0).unwrap() as usize + 0x62EE;
    let rec_at = base + idx * equip::EQUIP_RECORD;
    for (at, (a, b)) in body_before.iter().zip(body_after.iter()).enumerate() {
        if a != b {
            assert!(
                at == rec_at || at == rec_at + 1,
                "byte 0x{:X} changed, outside the level field",
                at
            );
        }
    }
}

#[test]
fn writing_a_decoration_leaves_the_rest_of_the_record_alone() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = first_used(&s, 0) else { return };
    let before = s
        .equipment_box(0)
        .map(|b| b[idx * equip::EQUIP_RECORD..(idx + 1) * equip::EQUIP_RECORD].to_vec())
        .expect("box");
    {
        let rec = s.equipment_record_mut(0, idx).expect("record");
        assert!(equip::set_decoration(rec, 1, 2639));
    }
    let after = s
        .equipment_box(0)
        .map(|b| b[idx * equip::EQUIP_RECORD..(idx + 1) * equip::EQUIP_RECORD].to_vec())
        .expect("box");
    assert_eq!(equip::Equipment::decorations(0, &after)[1], 2639);
    // The type-owned tail (charms store skills there) must be untouched.
    assert_eq!(&before[0x0C..], &after[0x0C..], "the tail is type-owned");
}

#[test]
fn a_worn_record_reports_a_reference_before_it_is_cleared() {
    let Some(s) = real_save() else { return };
    let Some(idx) = first_used(&s, 0) else { return };
    // Not every used record is worn, so only assert the implication one way.
    let refs = equiprefs::references(&s, 0, idx);
    if refs.is_empty() {
        assert!(equiprefs::is_unreferenced(&s, 0, idx));
    } else {
        assert!(!equiprefs::is_unreferenced(&s, 0, idx));
    }
}

#[test]
fn the_equipment_box_accessors_agree_with_each_other() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = first_used(&s, 0) else { return };
    let via_get = s.equipment_at(0, idx).expect("decodes");
    let via_record = {
        let b = s.equipment_box(0).unwrap();
        equip::decode_record(idx, &b[idx * equip::EQUIP_RECORD..(idx + 1) * equip::EQUIP_RECORD])
            .expect("decodes")
    };
    assert_eq!(via_get, via_record);
    // A write through the mutable accessor is visible through the immutable one.
    {
        let rec = s.equipment_record_mut(0, idx).expect("record");
        assert!(equip::set_level(rec, 3));
    }
    assert_eq!(s.equipment_at(0, idx).unwrap().level, 3);
}