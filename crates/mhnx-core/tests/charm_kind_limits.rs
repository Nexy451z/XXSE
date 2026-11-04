//! The measured ceiling of each talisman kind.
//!
//! The game has no table per kind: it ships four point tables and four slot
//! tables, and a kind selects one through its rarity (1-2 -> 97, 3-4 -> 98,
//! 5-7 -> 99, 8-10 -> 100). `charm::KIND_LIMITS` unrolls that into eleven rows.
//! These tests are the evidence for it:
//!
//! * the eleven rows are recomputed here from the four point tables the crate
//!   ships (which are a cell-for-cell copy of the romfs `amuletSkillData*.amskl`
//!   files), so the constants cannot drift from the data;
//! * the slot ceiling agrees with the grade it belongs to, 1 / 2 / 3 / 3;
//! * every one of the 650 real talismans fits the row of the table its kind
//!   selects, and most of them fit *only* that table - a check the wrong
//!   mapping would fail.

use std::path::PathBuf;

use mhnx_core::charm::{self, KindLimits, TalismanGrade};
use mhnx_core::save::Save;

/// The dump the other charm tests read.
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

/// The point table of one rarity, from the crate's gamedata.
fn table(rarity: u8) -> &'static [&'static str] {
    use mhnx_core::gamedata::{
        TALISMAN_ENDURING_SKILL, TALISMAN_MYSTERY_SKILL, TALISMAN_SHINING_SKILL,
        TALISMAN_TIMEWORN_SKILL,
    };
    match rarity {
        97 => TALISMAN_MYSTERY_SKILL,
        98 => TALISMAN_SHINING_SKILL,
        99 => TALISMAN_TIMEWORN_SKILL,
        100 => TALISMAN_ENDURING_SKILL,
        other => panic!("no table for rarity {}", other),
    }
}

/// One half of a cell, "min~max".
fn range_of(cell: &str, second: bool) -> (i8, i8) {
    let half = if second {
        cell.split(':').nth(1).expect("both halves exist")
    } else {
        cell.split(':').next().expect("the first half exists")
    };
    let mut bits = half.split('~');
    let min: i8 = bits.next().unwrap().trim().parse().unwrap();
    let max: i8 = bits.next().unwrap().trim().parse().unwrap();
    (min, max)
}

/// Highest points and number of rows of one half of a rarity's table.
fn ceiling_of_table(rarity: u8, second: bool) -> (i8, u16) {
    let mut high = 0i8;
    let mut rows = 0u16;
    for cell in table(rarity) {
        let (min, max) = range_of(cell, second);
        // "0~0" is the table saying "this skill does not go here", not a
        // ceiling of zero.
        if min == 0 && max == 0 {
            continue;
        }
        rows += 1;
        high = high.max(max);
    }
    (high, rows)
}

/// Does this record fit the point table of a rarity, both skills included?
fn fits(record: &[u8], rarity: u8) -> bool {
    for (skill, points_at, second) in [
        (record[0x0C], 0x0Eusize, false),
        (record[0x0D], 0x0F, true),
    ] {
        if skill == 0 {
            continue;
        }
        let points = record[points_at] as i8;
        let cell = match table(rarity).get(skill as usize) {
            Some(c) => *c,
            None => return false,
        };
        let (min, max) = range_of(cell, second);
        if min == 0 && max == 0 {
            // The skill is not allowed in this slot at all.
            return false;
        }
        if points < min || points > max {
            return false;
        }
    }
    true
}

#[test]
fn the_eleven_rows_are_recomputed_from_the_point_tables() {
    for kind in 1..=charm::TALISMAN_KIND_COUNT {
        let limits: KindLimits = charm::kind_limits(kind).expect("a shipped kind");
        let (first_high, first_rows) = ceiling_of_table(limits.rarity, false);
        let (second_high, second_rows) = ceiling_of_table(limits.rarity, true);
        assert_eq!(
            (limits.max_first, limits.first_skills),
            (first_high, first_rows),
            "kind {}: slot 1",
            kind
        );
        assert_eq!(
            (limits.max_points(true), limits.second_skills),
            (if second_rows == 0 { None } else { Some(second_high) }, second_rows),
            "kind {}: slot 2",
            kind
        );
        // The kind byte the record carries is the one `kind_rarity` says.
        assert_eq!(limits.rarity, charm::kind_rarity(kind).unwrap());
        assert_eq!(limits.kind, kind);
    }
    assert!(charm::kind_limits(0).is_none(), "装備なし has no ceiling");
    assert!(charm::kind_limits(11).is_none());
}

#[test]
fn the_slot_ceiling_is_one_two_three_three() {
    let expected = [(97u8, 1u8), (98, 2), (99, 3), (100, 3)];
    for kind in 1..=charm::TALISMAN_KIND_COUNT {
        let limits = charm::kind_limits(kind).unwrap();
        let grade = TalismanGrade::from_rarity(limits.rarity).unwrap();
        let (_, want) = expected
            .iter()
            .find(|(rarity, _)| *rarity == limits.rarity)
            .copied()
            .unwrap();
        assert_eq!(limits.max_slots, want, "kind {}", kind);
        // The grade and the kind answer the same number, which is why the two
        // routines may clamp against either.
        assert_eq!(limits.max_slots, grade.max_slots(), "kind {}", kind);
    }
}

#[test]
fn a_soldier_kind_can_never_carry_a_second_skill() {
    // amuletSkillData01 is a header and nothing else: rarity 97 has no slot-2
    // rows, so 兵士 / 闘士 stop at one skill.
    let (_, rows) = ceiling_of_table(97, true);
    assert_eq!(rows, 0);
    for kind in [1u8, 2] {
        let limits = charm::kind_limits(kind).unwrap();
        assert!(!limits.has_second_skill());
        assert_eq!(limits.max_points(true), None);
    }
    // Every other kind does have one, and its ceiling is +13.
    for kind in 3..=charm::TALISMAN_KIND_COUNT {
        let limits = charm::kind_limits(kind).unwrap();
        assert!(limits.has_second_skill(), "kind {}", kind);
        assert_eq!(limits.max_points(true), Some(13), "kind {}", kind);
    }
}

#[test]
fn the_higher_kinds_can_carry_more_than_the_lower_ones() {
    // The point of the whole exercise: the ceiling is not one number for all.
    let soldier = charm::kind_limits(1).unwrap();
    let dragon = charm::kind_limits(7).unwrap();
    let sky = charm::kind_limits(10).unwrap();

    assert_eq!(soldier.max_slots, 1);
    assert_eq!(dragon.max_slots, 3);
    assert_eq!(sky.max_slots, 3);
    assert_eq!(soldier.max_points(true), None);
    assert_eq!(dragon.max_points(true), Some(13));
    assert_eq!(sky.max_points(true), Some(13));
    // 女王 / 王 / 龍 are the weaker tier on the first skill, not the second.
    assert!(dragon.max_first < sky.max_first);
    assert_eq!((dragon.max_first, dragon.max_second), (8, 13));
    assert_eq!((sky.max_first, sky.max_second), (10, 13));
    // And the number of skills each may roll follows the same grouping.
    assert_eq!((soldier.first_skills, soldier.second_skills), (59, 0));
    assert_eq!((dragon.first_skills, dragon.second_skills), (56, 105));
    assert_eq!((sky.first_skills, sky.second_skills), (62, 94));
}

#[test]
fn every_real_talisman_fits_the_ceiling_of_its_own_kind() {
    let Some(src) = real_save() else { return };
    let mut checked = 0usize;
    let mut per_kind = std::collections::BTreeMap::<u8, usize>::new();
    let mut observed_first = 0i8;
    let mut observed_second = 0i8;

    for slot in 0..3 {
        let Some(b) = src.equipment_box(slot) else { continue };
        for i in 0..charm::CHARM_COUNT {
            let o = i * charm::CHARM_RECORD;
            let rec = &b[o..o + charm::CHARM_RECORD];
            if !charm::is_talisman(rec) {
                continue;
            }
            checked += 1;
            *per_kind.entry(rec[0x02]).or_default() += 1;
            // Two of the 650 carry the 装備なし row with rarity 99; that pair
            // is what the ranking tests elsewhere complain about, not a kind.
            let Some(limits) = charm::kind_limits(rec[0x02]) else {
                continue;
            };
            // The two kind bytes are a linked pair, so the kind is what the
            // record says it is.
            assert_eq!(
                Some(rec[0x12]),
                charm::kind_rarity(rec[0x02]),
                "slot {} #{}: kind {:#04x} and rarity {:#04x} disagree",
                slot,
                i,
                rec[0x02],
                rec[0x12]
            );
            assert!(
                rec[0x10] <= limits.max_slots,
                "slot {} #{}: {} slots on a {}",
                slot,
                i,
                rec[0x10],
                limits.name()
            );
            for (skill, points_at, second) in [
                (rec[0x0C], 0x0Eusize, false),
                (rec[0x0D], 0x0F, true),
            ] {
                if skill == 0 {
                    continue;
                }
                let points = rec[points_at] as i8;
                if second {
                    observed_second = observed_second.max(points);
                } else {
                    observed_first = observed_first.max(points);
                }
                let cap = limits.max_points(second).unwrap_or_else(|| {
                    panic!(
                        "slot {} #{}: {} carries a second skill it cannot have",
                        slot, i, limits.name()
                    )
                });
                assert!(
                    points <= cap,
                    "slot {} #{}: {} {:+} on a {} whose ceiling is {:+}",
                    slot,
                    i,
                    skill,
                    points,
                    limits.name(),
                    cap
                );
            }
        }
    }
    assert_eq!(checked, 650, "the sample save carries 650 talismans");
    assert!(!per_kind.is_empty());
    // The measured maxima reach the constants, which is what makes them the
    // ceilings and not merely an upper bound: 天 reaches +10 / +13.
    assert_eq!(observed_first, 10);
    assert_eq!(observed_second, 13);
}

#[test]
fn the_rarity_table_its_kind_selects_is_the_one_that_accepts_the_record() {
    let Some(src) = real_save() else { return };
    let mut checked = 0usize;
    let mut only_theirs = 0usize;
    for slot in 0..3 {
        let Some(b) = src.equipment_box(slot) else { continue };
        for i in 0..charm::CHARM_COUNT {
            let o = i * charm::CHARM_RECORD;
            let rec = &b[o..o + charm::CHARM_RECORD];
            if !charm::is_talisman(rec) {
                continue;
            }
            let Some(limits) = charm::kind_limits(rec[0x02]) else {
                continue;
            };
            assert!(
                fits(rec, limits.rarity),
                "slot {} #{}: does not fit its own rarity {}",
                slot,
                i,
                limits.rarity
            );
            checked += 1;
            let accepted: Vec<u8> = [97u8, 98, 99, 100]
                .into_iter()
                .filter(|rarity| fits(rec, *rarity))
                .collect();
            if accepted == [limits.rarity] {
                only_theirs += 1;
            }
        }
    }
    assert!(checked > 600, "only {} records were checked", checked);
    // 609 of the 650 fit one table only; the rest fit two, and never a table
    // that is not their own. If the kind -> rarity mapping were wrong this
    // number would collapse.
    assert!(
        only_theirs > 550,
        "only {} records pin the mapping down",
        only_theirs
    );
}

#[test]
fn the_kind_ceiling_line_is_what_the_screens_print() {
    let line = charm::kind_limit_line(10).expect("a shipped kind");
    assert!(line.starts_with("天の護石:"), "{}", line);
    assert!(line.contains("スキル1 最大 +10"), "{}", line);
    assert!(line.contains("スキル2 最大 +13"), "{}", line);
    assert!(line.contains("装飾スロット 最大 3"), "{}", line);

    let line = charm::kind_limit_line(1).unwrap();
    assert!(line.contains("第2スキルなし"), "{}", line);
    assert!(line.contains("装飾スロット 最大 1"), "{}", line);

    assert!(charm::kind_limit_line(0).is_none());
}