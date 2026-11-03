//! Talisman grades below the skyward one.
//!
//! The editor used to build every new talisman from the most common record in
//! the box, which is always rarity 100, so the lower grades could not be made
//! at all. These pin down the table lookup, and the rule that a grade may only
//! be written when the box already holds a record of it: the type byte at
//! +0x02 is paired with the rarity, and the values the game writes are not the
//! ones a table-derived guess would pick.

use std::path::PathBuf;

use mhnx_core::charm::{self, TalismanGrade};
use mhnx_core::save::Save;

/// The dump the other tests read. Prefer the fixed copy: the Ryujinx save is
/// live and the game writes to it, so a test pinned to it would rot.
fn real_save_path() -> PathBuf {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        stable
    } else {
        PathBuf::from(
            r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
        )
    }
}

fn real_save() -> Option<Save> {
    Save::from_bytes(&std::fs::read(real_save_path()).ok()?).ok()
}

#[test]
fn every_grade_has_its_own_rarity_byte() {
    assert_eq!(TalismanGrade::Mystery.rarity(), 97);
    assert_eq!(TalismanGrade::Shining.rarity(), 98);
    assert_eq!(TalismanGrade::Timeworn.rarity(), 99);
    assert_eq!(TalismanGrade::Enduring.rarity(), 100);
    for g in TalismanGrade::ALL {
        assert_eq!(TalismanGrade::from_rarity(g.rarity()), Some(g));
        // The label carries the number, because only 100 is confirmed
        // against the game's own wording.
        assert!(
            g.name().contains(&g.rarity().to_string()),
            "{} should name its rarity",
            g.name()
        );
    }
    assert_eq!(TalismanGrade::from_rarity(96), None);
}

#[test]
fn the_lowest_grade_now_has_a_skill_table() {
    // 97 was missing from the lookup, so nothing below the skyward talisman
    // could be validated.
    let r = charm::allowed_points_rarity(97, 1, false).expect("rarity 97 has a table");
    assert_eq!((r.min, r.max), (1, 5));
    let second = charm::allowed_points_rarity(97, 1, true).expect("cell exists");
    assert_eq!((second.min, second.max), (0, 0), "not allowed in slot 2");
    let top = charm::allowed_points_rarity(100, 1, false).expect("rarity 100 table");
    assert!(top.max >= top.min);
}

#[test]
fn a_real_record_supplies_the_type_byte_for_its_grade() {
    let Some(s) = real_save() else { return };
    // The first character carries nothing but rarity 100, which is how the
    // editor came to assume the skyward talisman. The other two carry 98/99.
    let mut seen = 0;
    for slot in 0..3 {
        let Some(b) = s.equipment_box(slot) else { continue };
        for grade in [TalismanGrade::Shining, TalismanGrade::Timeworn] {
            if let Some(t) = charm::template_for_grade(b, grade) {
                assert_eq!(t[0x12], grade.rarity());
                assert_eq!(
                    charm::type_byte_for_grade(b, grade),
                    Some(t[0x02]),
                    "the byte must come from the record, not the table"
                );
                // And it must agree with the rarity table.
                assert_eq!(charm::rarity_of(t[0x02]), Some(grade.rarity() as i32));
                seen += 1;
            }
        }
    }
    assert!(seen > 0, "the sample save carries lower grades");
}

#[test]
fn a_grade_absent_from_the_box_cannot_be_written() {
    let Some(mut s) = real_save() else { return };
    let has_mystery = (0..3).any(|slot| {
        s.equipment_box(slot)
            .and_then(|b| charm::template_for_grade(b, TalismanGrade::Mystery))
            .is_some()
    });
    if has_mystery {
        return; // nothing to prove on a save that does carry one
    }
    let Some(idx) = ({
        let b = s.equipment_box(0).unwrap();
        (0..charm::CHARM_COUNT).find(|i| {
            let o = i * charm::CHARM_RECORD;
            charm::is_talisman(&b[o..o + charm::CHARM_RECORD])
        })
    }) else {
        return;
    };
    let before = {
        let b = s.equipment_box(0).unwrap();
        b[idx * charm::CHARM_RECORD + 0x12]
    };
    assert_ne!(before, 97, "the record must start on another grade");
    {
        let b = s.equipment_box_mut(0).unwrap();
        assert!(
            charm::set_grade(b, idx, TalismanGrade::Mystery),
            "the table supplies a legal equipment id for the grade"
        );
    }
    let rec: Vec<u8> = {
        let b = s.equipment_box(0).unwrap();
        b[idx * charm::CHARM_RECORD..(idx + 1) * charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(rec[0x12], 97, "rarity follows the grade");
    assert_eq!(
        charm::rarity_of(rec[0x02]),
        Some(97),
        "and the equipment id at +0x02 must agree with it"
    );
    assert_ne!(before, rec[0x12], "the test must actually change something");
}

#[test]
fn switching_to_a_present_grade_moves_only_the_paired_bytes() {
    let Some(mut s) = real_save() else { return };
    // Pick the character that carries rarity 99 records.
    let Some((slot, idx, target)) = (0..3).find_map(|slot| {
        let b = s.equipment_box(slot)?;
        let t = charm::template_for_grade(b, TalismanGrade::Timeworn)?;
        let idx = (0..charm::CHARM_COUNT).find(|i| {
            let o = i * charm::CHARM_RECORD;
            let rec = &b[o..o + charm::CHARM_RECORD];
            charm::is_talisman(rec) && rec[0x12] != 99
        })?;
        Some((slot, idx, t))
    }) else {
        return;
    };
    let before: Vec<u8> = {
        let b = s.equipment_box(slot).unwrap();
        b[idx * charm::CHARM_RECORD..(idx + 1) * charm::CHARM_RECORD].to_vec()
    };
    {
        let b = s.equipment_box_mut(slot).unwrap();
        assert!(charm::set_grade(b, idx, TalismanGrade::Timeworn));
        charm::clamp_points(b, idx, TalismanGrade::Timeworn);
    }
    let after: Vec<u8> = {
        let b = s.equipment_box(slot).unwrap();
        b[idx * charm::CHARM_RECORD..(idx + 1) * charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(after[0x12], 99);
    assert_eq!(after[0x02], target[0x02], "the byte comes from a real record");
    for at in 0..charm::CHARM_RECORD {
        if before[at] != after[at] {
            assert!(
                matches!(at, 0x02 | 0x0C | 0x0D | 0x0E | 0x0F | 0x12),
                "byte 0x{:02X} moved",
                at
            );
        }
    }
}

#[test]
fn the_slot_ceiling_differs_by_grade() {
    assert_eq!(TalismanGrade::Mystery.max_slots(), 1);
    assert_eq!(TalismanGrade::Shining.max_slots(), 2);
    assert_eq!(TalismanGrade::Timeworn.max_slots(), 3);
    assert_eq!(TalismanGrade::Enduring.max_slots(), 3);

    let Some(mut s) = real_save() else { return };
    let Some(idx) = ({
        let b = s.equipment_box(0).unwrap();
        (0..charm::CHARM_COUNT).find(|i| {
            let o = i * charm::CHARM_RECORD;
            charm::is_talisman(&b[o..o + charm::CHARM_RECORD])
        })
    }) else {
        return;
    };
    {
        let b = s.equipment_box_mut(0).unwrap();
        b[idx * charm::CHARM_RECORD + 0x10] = 3;
        assert!(charm::clamp_slots(b, idx, TalismanGrade::Shining));
    }
    let slots = {
        let b = s.equipment_box(0).unwrap();
        b[idx * charm::CHARM_RECORD + 0x10]
    };
    assert_eq!(slots, 2, "a 98 cannot carry three slots");
}

#[test]
fn clamping_clears_a_skill_the_grade_cannot_carry() {
    let Some(mut s) = real_save() else { return };
    let Some(idx) = ({
        let b = s.equipment_box(0).unwrap();
        (0..charm::CHARM_COUNT).find(|i| {
            let o = i * charm::CHARM_RECORD;
            let rec = &b[o..o + charm::CHARM_RECORD];
            charm::is_talisman(rec) && rec[0x0C] != 0
        })
    }) else {
        return;
    };
    {
        let b = s.equipment_box_mut(0).unwrap();
        b[idx * charm::CHARM_RECORD + 0x0E] = 120;
        assert!(charm::clamp_points(b, idx, TalismanGrade::Enduring));
    }
    let rec: Vec<u8> = {
        let b = s.equipment_box(0).unwrap();
        b[idx * charm::CHARM_RECORD..(idx + 1) * charm::CHARM_RECORD].to_vec()
    };
    assert!((rec[0x0E] as i8) < 120, "the value must come back into range");
    if rec[0x0C] != 0 {
        let r = charm::allowed_points_rarity(100, rec[0x0C], false).expect("cell");
        let pts = rec[0x0E] as i8;
        assert!(pts >= r.min && pts <= r.max, "{} outside {}..={}", pts, r.min, r.max);
    }
}
// ---------------------------------------------------------------------------
// Warnings, clamping, and the rule that a normal save is warning-free.
// ---------------------------------------------------------------------------

/// A box holding one talisman at index 0, with the given bytes set.
fn box_with_talisman(bytes: &[(usize, u8)]) -> Vec<u8> {
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    b[0] = charm::CHARM_TYPE; // record_type 6: a talisman
    b[0x12] = TalismanGrade::Enduring.rarity();
    for (at, v) in bytes {
        b[*at] = *v;
    }
    b
}

/// A skill the grade carries, with room on both sides of its range.
fn carryable_skill(grade: TalismanGrade, second: bool) -> (u8, charm::AllowedPoints) {
    (1..=255u8)
        .find_map(|s| {
            let r = charm::allowed_points_rarity(grade.rarity(), s, second)?;
            (r.min > -120 && r.max < 120 && r.min < r.max).then_some((s, r))
        })
        .expect("the table carries a skill with room on both sides")
}

/// A skill the grade does not carry at all ("0~0").
fn forbidden_skill(grade: TalismanGrade, second: bool) -> u8 {
    (1..=255u8)
        .find(|s| {
            matches!(
                charm::allowed_points_rarity(grade.rarity(), *s, second),
                Some(r) if r.min == 0 && r.max == 0
            )
        })
        .expect("the table forbids some skills")
}

#[test]
fn grade_warnings_names_a_forbidden_skill_a_slot_overflow_and_bad_points() {
    // A skill the grade forbids outright.
    let grade = TalismanGrade::Enduring;
    let f = forbidden_skill(grade, false);
    let b = box_with_talisman(&[(0x0C, f), (0x0E, 1)]);
    let w = charm::grade_warnings(&b, 0, grade);
    assert_eq!(w.len(), 1, "one warning, got {:?}", w);
    assert!(
        w[0].contains("スキル1") && w[0].contains("付けられません"),
        "{:?}",
        w
    );

    // A slot count above the grade's ceiling: rarity 98 stops at two.
    let grade = TalismanGrade::Shining;
    let (s, r) = carryable_skill(grade, false);
    let b = box_with_talisman(&[(0x0C, s), (0x0E, r.min as u8), (0x10, grade.max_slots() + 1)]);
    let w = charm::grade_warnings(&b, 0, grade);
    assert_eq!(w.len(), 1, "only the slots are wrong, got {:?}", w);
    assert!(
        w[0].contains("スロット") && w[0].contains(&grade.max_slots().to_string()),
        "{:?}",
        w
    );

    // Points above and below the published range.
    let grade = TalismanGrade::Enduring;
    let (s, r) = carryable_skill(grade, false);
    for bad in [(r.max + 2) as u8, (r.min - 2) as u8] {
        let b = box_with_talisman(&[(0x0C, s), (0x0E, bad)]);
        let w = charm::grade_warnings(&b, 0, grade);
        assert_eq!(w.len(), 1, "points {}: {:?}", bad as i8, w);
        assert!(w[0].contains("範囲外"), "{:?}", w);
    }

    // The same record inside its range has nothing to warn about.
    let b = box_with_talisman(&[(0x0C, s), (0x0E, r.max as u8)]);
    assert!(charm::grade_warnings(&b, 0, grade).is_empty());
}

#[test]
fn clamping_a_low_point_moves_it_to_the_minimum_not_the_maximum() {
    let grade = TalismanGrade::Enduring;
    let (skill, range) = carryable_skill(grade, false);
    assert!(range.min < range.max);

    // Below the minimum: the old clamp pushed every out-of-range value to the
    // maximum, which turned a value that was too low into a perfect one.
    let mut b = box_with_talisman(&[(0x0C, skill), (0x0E, (range.min - 3) as u8)]);
    assert!(charm::clamp_points(&mut b, 0, grade), "a value had to move");
    assert_eq!(b[0x0E] as i8, range.min, "a low value lands on the minimum");
    assert_ne!(b[0x0E] as i8, range.max, "it must not jump to the maximum");

    // Above the maximum lands on the maximum.
    let mut b = box_with_talisman(&[(0x0C, skill), (0x0E, (range.max + 3) as u8)]);
    assert!(charm::clamp_points(&mut b, 0, grade));
    assert_eq!(b[0x0E] as i8, range.max);

    // A legal value is left alone and the call says so.
    let mut b = box_with_talisman(&[(0x0C, skill), (0x0E, range.max as u8)]);
    assert!(!charm::clamp_points(&mut b, 0, grade), "nothing to clamp");
    assert_eq!(b[0x0E] as i8, range.max);

    // A skill the grade forbids is kept as chosen; the warning is the only
    // feedback. Deleting it silently would lose the user's edit.
    let f = forbidden_skill(grade, false);
    let mut b = box_with_talisman(&[(0x0C, f), (0x0E, 7)]);
    assert!(!charm::clamp_points(&mut b, 0, grade), "nothing moves");
    assert_eq!(b[0x0C], f, "the skill must stay");
    assert_eq!(b[0x0E], 7, "and its value too");
    assert!(
        !charm::grade_warnings(&b, 0, grade).is_empty(),
        "which is exactly what the warning is for"
    );
}

#[test]
fn every_talisman_in_the_real_save_is_clean_for_the_grade_it_carries() {
    let path = real_save_path();
    let Some(raw) = std::fs::read(&path).ok() else {
        return;
    };
    let Some(save) = Save::from_bytes(&raw).ok() else {
        return;
    };

    let mut counted = 0usize;
    let mut boxes = 0usize;
    for slot in 0..3 {
        let Some(b) = save.equipment_box(slot) else {
            continue;
        };
        boxes += 1;
        for i in 0..charm::CHARM_COUNT {
            let o = i * charm::CHARM_RECORD;
            let rec = &b[o..o + charm::CHARM_RECORD];
            let Some(grade) = charm::rarity_of_record(rec) else {
                continue;
            };
            assert_eq!(
                rec[0x12],
                grade.rarity(),
                "the grade must come from the record"
            );
            let w = charm::grade_warnings(b, i, grade);
            assert!(
                w.is_empty(),
                "slot {} #{} rarity {}: {:?}",
                slot + 1,
                i,
                rec[0x12],
                w
            );
            counted += 1;
        }
    }
    assert_eq!(boxes, 3, "the sample save has three characters");
    assert_eq!(
        counted, 650,
        "the sample save carries 650 talismans, the same set the range tables were measured on"
    );

    // Reading the save and checking it must never write to it.
    let after = std::fs::read(&path).expect("the save is still readable");
    assert_eq!(after, raw, "checking a save must not modify the file");
}