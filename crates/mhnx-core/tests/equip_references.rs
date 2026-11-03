//! Who points at an equipment record.
//!
//! Confirmed against the sample save: the worn indices at +0x23B39 land on
//! records of the part they name, which is what makes the offset believable.

use std::path::PathBuf;

use mhnx_core::equiprefs::{self, EquipRef};
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

#[test]
fn the_worn_indices_point_at_the_part_they_name() {
    let Some(s) = real_save() else { return };
    let mut checked = 0;
    for slot in 0..3 {
        for part in 0..equiprefs::WORN_PARTS {
            let refs = refs_at_worn(&s, slot, part);
            let Some(idx) = refs else { continue };
            let Some(e) = s.equipment_at(slot, idx as usize) else {
                continue;
            };
            if e.kind == 0 {
                continue;
            }
            // Weapon is any of the weapon kinds; the armour parts are 1..=5
            // and the talisman is 6, in the same order as the worn list.
            let expected: &[u8] = match part {
                0 => &[7, 8, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21],
                1 => &[1],
                2 => &[2],
                3 => &[3],
                4 => &[4],
                5 => &[5],
                _ => &[6],
            };
            assert!(
                expected.contains(&e.kind),
                "slot {} part {} points at kind {}",
                slot,
                part,
                e.kind
            );
            checked += 1;
        }
    }
    assert!(checked >= 7, "expected worn gear to check, got {}", checked);
}

/// The raw worn index for one part, or None when the slot is unused.
fn refs_at_worn(s: &Save, slot: usize, part: usize) -> Option<u16> {
    let base = s.slot_base(slot)? as usize;
    let at = base + equiprefs::WORN_OFFSET + part * 2;
    let b = s.body().get(at..at + 2)?;
    let v = u16::from_le_bytes([b[0], b[1]]);
    if v == equiprefs::NONE {
        None
    } else {
        Some(v)
    }
}

#[test]
fn a_worn_record_reports_itself_as_referenced() {
    let Some(s) = real_save() else { return };
    let Some(idx) = refs_at_worn(&s, 0, 0) else {
        return;
    };
    let refs = equiprefs::references(&s, 0, idx as usize);
    assert!(
        refs.iter()
            .any(|r| matches!(r, EquipRef::Worn { part: 0 })),
        "the weapon index must report itself worn"
    );
    assert!(!equiprefs::is_unreferenced(&s, 0, idx as usize));
    assert!(refs[0].text().contains("装備中"));
}

#[test]
fn a_blank_record_is_referenced_by_nothing() {
    let Some(s) = real_save() else { return };
    // Find a record that is all zero and confirm nothing names it.
    let Some(box_bytes) = s.equipment_box(0) else { return };
    let blank = (0..mhnx_core::equip::EQUIP_COUNT).find(|i| {
        let o = i * mhnx_core::equip::EQUIP_RECORD;
        mhnx_core::equip::record_is_blank(&box_bytes[o..o + mhnx_core::equip::EQUIP_RECORD])
    });
    let Some(idx) = blank else { return };
    // A zero index would collide with an index of 0, which is a real record;
    // index 0 itself is the only case where the answer is not obvious.
    if idx == 0 {
        return;
    }
    assert!(
        equiprefs::references(&s, 0, idx).is_empty(),
        "nothing should point at blank record {}",
        idx
    );
}