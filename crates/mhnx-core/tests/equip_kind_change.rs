//! Changing an equipment record's type (equipops::change_kind).
//!
//! The type is one five bit field, and everything behind +0x02 means
//! something only because of it. These tests pin the two halves of that: what
//! the writer is allowed to touch (the type, and the talisman pair the type
//! needs) and what it must leave alone, plus the warnings the two screens show
//! afterwards. The real-data tests read the sample save and work on an
//! in-memory copy of it; nothing on disk is written except the temporary copy
//! that proves a saved file still reads the converted record as a talisman.

use std::path::PathBuf;

use mhnx_core::charm;
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

fn record(s: &Save, slot: usize, index: usize) -> Vec<u8> {
    let b = s.equipment_box(slot).expect("the box reads");
    b[index * equip::EQUIP_RECORD..(index + 1) * equip::EQUIP_RECORD].to_vec()
}

/// A weapon record no worn item and no set names, so the test cannot depend on
/// what the character is carrying.
fn loose_weapon(s: &Save, slot: usize) -> Option<usize> {
    (0..equip::EQUIP_COUNT).find(|i| {
        let Some(e) = s.equipment_at(slot, *i) else {
            return false;
        };
        (7..=21).contains(&e.kind) && equiprefs::references(s, slot, *i).is_empty()
    })
}

#[test]
fn a_weapon_becomes_a_talisman_the_charm_reader_accepts() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = loose_weapon(&s, 0) else { return };
    let before = s.equipment_at(0, idx).unwrap();
    let before_rec = record(&s, 0, idx);

    let changed = equipops::change_kind(&mut s, 0, idx, charm::CHARM_TYPE, 10)
        .expect("a weapon can become a talisman");
    assert_eq!(changed.from, before.kind);
    assert_eq!(changed.to, charm::CHARM_TYPE);
    assert_eq!(changed.charm_code, Some(10));
    assert!(changed.changed_type());
    assert!(changed.text().contains("護石"), "{}", changed.text());

    // The type, and the id the type re-reads as: for a talisman +0x02 is the
    // 護石 grade, not a weapon style.
    let after = s.equipment_at(0, idx).unwrap();
    assert_eq!(after.kind, charm::CHARM_TYPE);
    assert_eq!(after.id, 10);
    assert_eq!(equip::type_name(after.kind), "護石");
    assert_eq!(equip::name(&after), "Creator Talisman", "the 10th row is 天");

    // The linked pair charm.rs insists on: kind code, rarity, high byte.
    let rec = record(&s, 0, idx);
    assert!(charm::is_talisman(&rec), "the charm reader has to see it");
    assert_eq!(rec[0x02], 10);
    assert_eq!(rec[0x12], 100, "天 is rarity 100");
    assert_eq!(rec[0x13], 1, "the u16 charm id 353..356 has 1 in its high byte");
    assert_eq!(charm::kind_of_record(&rec), Some(10));

    // And the two readers the screens use agree with each other.
    let box_bytes = s.equipment_box(0).unwrap();
    let charms = charm::decode_box(box_bytes);
    assert!(
        charms.iter().any(|c| c.index == idx),
        "the talisman list must show the converted record"
    );
    assert_eq!(charm::kind_name_at(box_bytes, idx), Some("天の護石"));
    assert_eq!(charm::charm_kind(&s, 0, idx), Some(10));

    // Everything the type and the pair do not own is still there: the
    // conversion is not a rewrite of the record. The bytes a talisman needs are
    // +0x00/+0x01 (type and level), +0x02 (the kind code) and +0x12/+0x13 (the
    // pair); every other byte must survive.
    let after_rec = record(&s, 0, idx);
    for off in 0..equip::EQUIP_RECORD {
        if matches!(off, 0x00 | 0x01 | 0x02 | 0x12 | 0x13) {
            continue;
        }
        assert_eq!(
            after_rec[off], before_rec[off],
            "byte {:#04x} must not move",
            off
        );
    }
    // The tail is exactly what makes the 護石 tab warn: the converted record
    // has no skills yet, because a weapon never had any.
    assert!(
        changed
            .warnings
            .iter()
            .any(|w| w.contains("スキルが1つも設定されていません")),
        "{:?}",
        changed.warnings
    );
    assert_eq!(after.level, before.level, "the level field shares the first word");
    assert_eq!(after.transmog, before.transmog);

    // The kind is consistent, so the only warning is the level one - a real
    // talisman keeps 0 there, which is Lv1 on this screen.
    assert!(
        !changed.warnings.iter().any(|w| w.contains("種別コード")),
        "a pair written by set_kind must not be reported as missing: {:?}",
        changed.warnings
    );
}

#[test]
fn the_converted_record_survives_being_written_and_read_again() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = loose_weapon(&s, 0) else { return };
    equipops::change_kind(&mut s, 0, idx, charm::CHARM_TYPE, 7).expect("a kind change");
    assert_eq!(charm::kind_name_at(s.equipment_box(0).unwrap(), idx), Some("龍の護石"));

    // A copy of the real file: the same bytes a 上書き保存 would leave.
    let bytes = s.to_bytes();
    let dir = std::env::temp_dir().join("mhnx_equip_kind_change");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("system");
    std::fs::write(&path, &bytes).unwrap();
    let reread = Save::from_bytes(&std::fs::read(&path).unwrap()).expect("the copy parses");
    assert_eq!(reread.to_bytes(), bytes, "the copy round trips byte for byte");
    let box_bytes = reread.equipment_box(0).expect("the box reads");
    let rec = &box_bytes[idx * equip::EQUIP_RECORD..(idx + 1) * equip::EQUIP_RECORD];
    assert!(charm::is_talisman(rec));
    assert_eq!(charm::kind_of_record(rec), Some(7));
    assert_eq!(rec[0x12], 99, "龍 is rarity 99");
    assert!(
        charm::decode_box(box_bytes).iter().any(|c| c.index == idx),
        "the file on disk still reads as a talisman"
    );
}

#[test]
fn a_talisman_becomes_a_weapon_and_its_old_bytes_stay() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = charm::decode_box(&s.equipment_box(0).unwrap())
        .iter()
        .map(|c| c.index)
        .find(|i| equiprefs::references(&s, 0, *i).is_empty())
    else {
        return;
    };
    let before = record(&s, 0, idx);
    let changed = equipops::change_kind(&mut s, 0, idx, 7, 0).expect("a talisman can become a weapon");
    assert_eq!((changed.from, changed.to), (charm::CHARM_TYPE, 7));
    assert_eq!(changed.charm_code, None, "no pair is written away from type 6");

    let after = record(&s, 0, idx);
    assert_eq!(after[0] & 0x1F, 7, "the type moved");
    assert_eq!(after[0x02], before[0x02], "the old kind code is kept, not erased");
    assert_eq!(after[0x12], before[0x12], "and so is the old rarity");
    assert!(!charm::is_talisman(&after));
    assert!(
        !charm::decode_box(&s.equipment_box(0).unwrap())
            .iter()
            .any(|c| c.index == idx),
        "the talisman list drops it"
    );
    // The leftover pair is reported rather than repaired.
    assert!(
        changed.warnings.iter().any(|w| w.contains("残っています")),
        "{:?}",
        changed.warnings
    );
}

#[test]
fn a_kind_change_refuses_what_it_cannot_write() {
    let Some(mut s) = real_save() else { return };
    let blank = (0..equip::EQUIP_COUNT).find(|i| {
        equip::record_is_blank(&record(&s, 0, *i))
    });
    if let Some(blank) = blank {
        assert_eq!(
            equipops::change_kind(&mut s, 0, blank, 7, 0),
            Err(EquipError::AlreadyBlank)
        );
    }
    let Some(idx) = loose_weapon(&s, 0) else { return };
    assert_eq!(
        equipops::change_kind(&mut s, 0, idx, 0, 0),
        Err(EquipError::KindIsBlank)
    );
    assert_eq!(
        equipops::change_kind(&mut s, 0, idx, 30, 0),
        Err(EquipError::UnknownKind(30))
    );
    // Type 6 without a kind code is the record the game never writes.
    for bad in [0u8, 11, 255] {
        assert_eq!(
            equipops::change_kind(&mut s, 0, idx, charm::CHARM_TYPE, bad),
            Err(EquipError::BadCharmKind),
            "charm code {}",
            bad
        );
    }
    // And none of the refusals wrote anything.
    let after = s.equipment_at(0, idx).unwrap();
    assert_ne!(after.kind, charm::CHARM_TYPE);
}

#[test]
fn replace_and_create_write_the_talisman_pair_as_well() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = loose_weapon(&s, 0) else { return };
    equipops::replace(&mut s, 0, idx, charm::CHARM_TYPE, 3, 1).expect("a talisman is replaceable");
    let rec = record(&s, 0, idx);
    assert_eq!(charm::kind_of_record(&rec), Some(3), "騎士の護石");
    assert_eq!(rec[0x12], 98, "騎士 is rarity 98");
    assert!(charm::is_talisman(&rec));

    // An id the kind table does not carry is written as it is and gets no
    // pair: the box holds two such talismans, the CSV export has to put them
    // back, and refusing them would break its round trip. The warning is what
    // says the record is odd.
    equipops::replace(&mut s, 0, idx, charm::CHARM_TYPE, 12, 1)
        .expect("a talisman with a code the table does not carry still writes");
    let rec = record(&s, 0, idx);
    assert_eq!(charm::kind_of_record(&rec), None, "12 is not a kind the game ships");
    assert!(
        equipops::kind_warnings_record(&rec)
            .iter()
            .any(|w| w.contains("種別コード")),
        "the odd code is reported, not repaired"
    );
    // change_kind is the operation that *asks* for a kind, so a code the table
    // does not carry is refused there: the screens pick it from the ten rows.
    assert_eq!(
        equipops::change_kind(&mut s, 0, idx, charm::CHARM_TYPE, 12),
        Err(EquipError::BadCharmKind)
    );
    assert_eq!(
        equipops::change_kind(&mut s, 0, idx, charm::CHARM_TYPE, 0),
        Err(EquipError::BadCharmKind)
    );

    // create() has to leave the same shape behind, because the console's new
    // talisman form goes through it.
    let new = equipops::create(&mut s, 0, charm::CHARM_TYPE, 1, 1).expect("a blank record exists");
    let rec = record(&s, 0, new);
    assert_eq!(charm::kind_of_record(&rec), Some(1), "兵士の護石");
    assert_eq!(rec[0x12], 97);
    assert_eq!(rec[0x13], 1);
    assert!(charm::decode_box(&s.equipment_box(0).unwrap())
        .iter()
        .any(|c| c.index == new));
}

#[test]
fn kind_warnings_name_what_does_not_fit_the_type() {
    // A hand made talisman record: the pair is what tells a record from one
    // the game wrote, so all four ways of getting it wrong are pinned here.
    let mut rec = [0u8; equip::EQUIP_RECORD];
    rec[0] = charm::CHARM_TYPE;
    rec[0x0C] = 1;
    rec[0x0E] = 3;
    rec[0x10] = 1;

    // No kind code at all.
    let w = equipops::kind_warnings_record(&rec);
    assert!(w.iter().any(|l| l.contains("種別コード") && l.contains("0")), "{:?}", w);

    // A kind code with the wrong rarity beside it.
    rec[0x02] = 10;
    rec[0x12] = 99;
    rec[0x13] = 1;
    let w = equipops::kind_warnings_record(&rec);
    assert!(w.iter().any(|l| l.contains("レア度") && l.contains("100")), "{:?}", w);

    // The right pair: nothing about it is worth a line.
    rec[0x12] = 100;
    assert!(equipops::kind_warnings_record(&rec).is_empty());

    // A weapon that still carries a talisman's pair.
    let mut weapon = [0u8; equip::EQUIP_RECORD];
    weapon[0] = 7;
    weapon[1] = 0x02;
    weapon[0x02] = 10;
    weapon[0x12] = 100;
    weapon[0x13] = 1;
    let w = equipops::kind_warnings_record(&weapon);
    assert!(w.iter().any(|l| l.contains("残っています")), "{:?}", w);

    // A type past the table names nothing at all.
    let mut odd = [0u8; equip::EQUIP_RECORD];
    odd[0] = 30;
    let w = equipops::kind_warnings_record(&odd);
    assert!(w.iter().any(|l| l.contains("種別表にありません")), "{:?}", w);

    // A blank record is not a problem to report.
    assert!(equipops::kind_warnings_record(&[0u8; equip::EQUIP_RECORD]).is_empty());
}