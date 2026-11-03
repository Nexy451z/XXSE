//! Adding, replacing and clearing equipment records.

use std::path::PathBuf;

use mhnx_core::equip;
use mhnx_core::equipops::{self, EquipError};
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

fn first_blank(s: &Save, slot: usize) -> Option<usize> {
    let b = s.equipment_box(slot)?;
    (0..equip::EQUIP_COUNT).find(|i| {
        let o = i * equip::EQUIP_RECORD;
        equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
    })
}

#[test]
fn create_fills_the_first_blank_record() {
    let Some(mut s) = real_save() else { return };
    let Some(blank) = first_blank(&s, 0) else { return };
    let used = s
        .equipment_box(0)
        .map(|b| {
            (0..equip::EQUIP_COUNT)
                .filter(|i| {
                    let o = i * equip::EQUIP_RECORD;
                    !equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
                })
                .count()
        })
        .unwrap_or(0);
    let idx = equipops::create(&mut s, 0, 7, 26, 5).expect("a blank record exists");
    assert_eq!(idx, blank, "the first blank record is the one used");
    let e = s.equipment_at(0, idx).expect("decodes");
    assert_eq!((e.kind, e.id, e.level), (7, 26, 5));

    // The box now holds one more used record.
    let after = s
        .equipment_box(0)
        .map(|b| {
            (0..equip::EQUIP_COUNT)
                .filter(|i| {
                    let o = i * equip::EQUIP_RECORD;
                    !equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
                })
                .count()
        })
        .unwrap_or(0);
    assert_eq!(after, used + 1);
}

#[test]
fn a_record_nothing_points_at_can_be_cleared() {
    let Some(mut s) = real_save() else { return };
    // Find a used record that no worn item and no set names.
    let free = {
        let b = match s.equipment_box(0) {
            Some(b) => b,
            None => return,
        };
        (0..equip::EQUIP_COUNT).find(|i| {
            let o = i * equip::EQUIP_RECORD;
            !equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
                && equiprefs::references(&s, 0, *i).is_empty()
        })
    };
    let Some(idx) = free else { return };
    assert!(equipops::clear(&mut s, 0, idx, false).is_ok());
    let rec = {
        let b = s.equipment_box(0).unwrap();
        b[idx * equip::EQUIP_RECORD..(idx + 1) * equip::EQUIP_RECORD].to_vec()
    };
    assert!(equip::record_is_blank(&rec));
    // And clearing it again says so rather than pretending.
    assert_eq!(
        equipops::clear(&mut s, 0, idx, false),
        Err(EquipError::AlreadyBlank)
    );
}

#[test]
fn a_worn_record_is_refused_then_cleared_with_force() {
    let Some(mut s) = real_save() else { return };
    let worn_now = s.slot_base(0).and_then(|base| {
        let at = base as usize + equiprefs::WORN_OFFSET;
        let b = s.body().get(at..at + 2)?;
        let v = u16::from_le_bytes([b[0], b[1]]);
        if v == equiprefs::NONE {
            None
        } else {
            Some(v as usize)
        }
    });
    let Some(worn) = worn_now else {
        return;
    };
    match equipops::clear(&mut s, 0, worn, false) {
        Err(EquipError::Referenced(refs)) => {
            assert!(!refs.is_empty());
            assert!(refs[0].text().contains("装備中"));
        }
        other => panic!("the worn weapon must be refused, got {:?}", other),
    }
    // The record is untouched after the refusal.
    assert!(!{
        let b = s.equipment_box(0).unwrap();
        let o = worn * equip::EQUIP_RECORD;
        equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
    });
    // And force goes through.
    assert!(equipops::clear(&mut s, 0, worn, true).is_ok());
}

#[test]
fn replace_keeps_the_tail_and_empties_the_decorations() {
    let Some(mut s) = real_save() else { return };
    let found = s.equipment_box(0).and_then(|b| {
        (0..equip::EQUIP_COUNT).find(|i| {
            let o = i * equip::EQUIP_RECORD;
            !equip::record_is_blank(&b[o..o + equip::EQUIP_RECORD])
        })
    });
    let Some(idx) = found else {
        return;
    };
    {
        let rec = s.equipment_record_mut(0, idx).unwrap();
        equip::set_decoration(rec, 0, 2638);
    }
    let tail_before = {
        let b = s.equipment_box(0).unwrap();
        let o = idx * equip::EQUIP_RECORD;
        b[o + 0x0C..o + equip::EQUIP_RECORD].to_vec()
    };
    assert!(equipops::replace(&mut s, 0, idx, 1, 100, 9).is_ok());
    let e = s.equipment_at(0, idx).unwrap();
    assert_eq!((e.kind, e.id, e.level), (1, 100, 9));
    let rec = {
        let b = s.equipment_box(0).unwrap();
        b[idx * equip::EQUIP_RECORD..(idx + 1) * equip::EQUIP_RECORD].to_vec()
    };
    assert_eq!(equip::Equipment::decorations(0, &rec), [0, 0, 0], "old gems go");
    assert_eq!(&rec[0x0C..], &tail_before[..], "the tail belongs to the record");
}

#[test]
fn replacing_a_blank_record_is_refused() {
    let Some(mut s) = real_save() else { return };
    let Some(blank) = first_blank(&s, 0) else { return };
    assert_eq!(
        equipops::replace(&mut s, 0, blank, 7, 1, 1),
        Err(EquipError::AlreadyBlank)
    );
}