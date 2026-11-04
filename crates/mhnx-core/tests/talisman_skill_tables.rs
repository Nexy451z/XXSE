//! Which skills a talisman kind can carry, and the points each may hold.
//!
//! The four point tables the game ships (`romfs/nativeNX/table/
//! amuletSkillData00..07.amskl`) are what the editor clamps and warns with.
//! These tests are the evidence for the indexing, all from real data:
//!
//! * each file is an 8 byte header and then 4 byte rows (u16 skill id,
//!   s8 min, s8 max), and the header's own u16 at +4 is the row count;
//! * the pairs 00/01, 02/03, 04/05, 06/07 hold rarity 97 / 98 / 99 / 100,
//!   slot 1 / slot 2, with 59 / 0 / 71 / 74 / 56 / 105 / 62 / 94 rows - the
//!   same numbers `charm::KIND_LIMITS` carries;
//! * their cells reproduce the four `gamedata::TALISMAN_*_SKILL` tables the
//!   crate ships, cell for cell;
//! * every one of the 650 talismans of the sample save fits the row of the
//!   table its kind selects, and no observed value exceeds it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mhnx_core::charm;
use mhnx_core::save::Save;

/// The romfs table directory, when this machine has the game extracted.
fn table_dir() -> Option<PathBuf> {
    let p = PathBuf::from(r"C:\anyWorkspace\nx\mhxx\romfs\nativeNX\table");
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

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

/// The crate's point table of one rarity.
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
        other => panic!("no point table for rarity {}", other),
    }
}

/// One half of a "min~max:min~max" cell.
fn range(cell: &str, second: bool) -> (i8, i8) {
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

/// One row of an amskl file: (skill id, min, max).
type Row = (u16, i8, i8);

/// Parse one amskl file the way the game stores it.
fn parse_amskl(path: &Path) -> Option<Vec<Row>> {
    let bytes = std::fs::read(path).ok()?;
    assert!(bytes.len() >= 8, "{} is shorter than its header", path.display());
    assert_eq!(&bytes[0..4], &[0x00, 0x00, 0xF0, 0x41], "{}: magic", path.display());
    let declared = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let body = &bytes[8..];
    assert_eq!(body.len() % 4, 0, "{}: rows are 4 bytes", path.display());
    assert_eq!(
        body.len() / 4,
        declared,
        "{}: the header count and the row count disagree",
        path.display()
    );
    let mut out = Vec::with_capacity(declared);
    for i in 0..declared {
        let o = i * 4;
        out.push((
            u16::from_le_bytes([body[o], body[o + 1]]),
            body[o + 2] as i8,
            body[o + 3] as i8,
        ));
    }
    Some(out)
}

/// The row count each file has to hold, in file order.
const FILE_ROWS: [usize; 8] = [59, 0, 71, 74, 56, 105, 62, 94];

/// (slot-1 file, slot-2 file, rarity) for the four pairs.
const PAIRS: [(usize, usize, u8); 4] = [(0, 1, 97), (2, 3, 98), (4, 5, 99), (6, 7, 100)];

#[test]
fn the_romfs_point_files_are_the_four_tables_slot_by_slot() {
    let Some(dir) = table_dir() else {
        eprintln!("romfs tables not present - skipping the file check");
        return;
    };
    let mut parsed: Vec<Vec<Row>> = Vec::new();
    for i in 0..8usize {
        let p = dir.join(format!("amuletSkillData{:02}.amskl", i));
        let rows = parse_amskl(&p).unwrap_or_else(|| panic!("{} is missing", p.display()));
        assert_eq!(rows.len(), FILE_ROWS[i], "file {}", i);
        parsed.push(rows);
    }

    // The pairing is proven twice over: by the row counts and, below, by the
    // cells. A file lists only the skills its rarity allows in its slot, so
    // the row count of each file is the KIND_LIMITS first/second count.
    for (first, second, rarity) in PAIRS {
        let limits = charm::kind_limits(charm::canonical_kind(rarity).unwrap()).unwrap();
        assert_eq!(
            parsed[first].len(),
            limits.first_skills as usize,
            "rarity {} slot 1 row count",
            rarity
        );
        assert_eq!(
            parsed[second].len(),
            limits.second_skills as usize,
            "rarity {} slot 2 row count",
            rarity
        );

        // Every row appears in the crate table, and the crate table has no row
        // the file does not carry: the two are the same data.
        for (file, is_second) in [(first, false), (second, true)] {
            let mut in_file = vec![None::<(i8, i8)>; 256];
            for (skill, min, max) in &parsed[file] {
                assert!(*skill <= u8::MAX as u16, "skill id {} does not fit a u8", skill);
                assert!(in_file[*skill as usize].is_none(), "duplicate skill {}", skill);
                in_file[*skill as usize] = Some((*min, *max));
            }
            for (skill, cell) in table(rarity).iter().enumerate() {
                let want = range(cell, is_second);
                let got = in_file[skill];
                let expected = if want == (0, 0) { None } else { Some(want) };
                assert_eq!(
                    got, expected,
                    "rarity {} slot {} skill {}: file {:?} vs table {:?}",
                    rarity,
                    if is_second { 2 } else { 1 },
                    skill,
                    got,
                    want
                );
            }
        }
    }
}

#[test]
fn every_kind_offers_the_skills_of_its_own_table() {
    // The picker's pool, counted per rarity and slot. These are the numbers
    // the screens now list, and they are the game's file row counts.
    let expected = [(97u8, 59usize, 0usize), (98, 71, 74), (99, 56, 105), (100, 62, 94)];
    for (rarity, first, second) in expected {
        assert_eq!(
            charm::allowed_skill_ids_rarity(rarity, false).len(),
            first,
            "rarity {} slot 1",
            rarity
        );
        assert_eq!(
            charm::allowed_skill_ids_rarity(rarity, true).len(),
            second,
            "rarity {} slot 2",
            rarity
        );
    }
    // Every kind answers with the list of the rarity it is stored as, and the
    // lists are sorted and inside the 206 cell table.
    for kind in 1..=charm::TALISMAN_KIND_COUNT {
        let rarity = charm::kind_rarity(kind).unwrap();
        for second in [false, true] {
            let ids = charm::allowed_skill_ids(kind, second);
            assert_eq!(ids, charm::allowed_skill_ids_rarity(rarity, second), "kind {}", kind);
            assert!(ids.windows(2).all(|w| w[0] < w[1]), "kind {}: not sorted", kind);
            assert!(ids.iter().all(|id| (1..=205).contains(id)), "kind {}: out of the table", kind);
            let limits = charm::kind_limits(kind).unwrap();
            let counted = if second { limits.second_skills } else { limits.first_skills };
            assert_eq!(ids.len(), counted as usize, "kind {}: row count", kind);
            for id in ids {
                assert!(charm::skill_allowed(kind, id, second), "kind {}: {} refused", kind, id);
            }
        }
    }
    // Bytes the game does not ship have no list at all.
    assert!(charm::allowed_skill_ids(0, false).is_empty());
    assert!(charm::allowed_skill_ids(11, false).is_empty());
    assert!(charm::allowed_skill_ids(200, true).is_empty());
}

#[test]
fn a_soldier_cannot_pick_a_second_skill() {
    // amuletSkillData01 is a header and nothing else: rarity 97 has no slot-2
    // rows, so 兵士 / 闘士 cannot carry a second skill rather than merely
    // rolling one rarely.
    for kind in [1u8, 2] {
        assert!(!charm::second_skill_available(kind), "kind {}", kind);
        assert!(charm::allowed_skill_ids(kind, true).is_empty(), "kind {}", kind);
        assert_eq!(
            charm::allowed_points_for_kind(kind, 1, true),
            Some(mhnx_core::charm::AllowedPoints { min: 0, max: 0 }),
            "kind {}: slot 2 is refused for every id",
            kind
        );
        assert!(!charm::skill_allowed(kind, 1, true), "kind {}", kind);
        // The first slot still works, with the rarity-97 table.
        assert_eq!(charm::allowed_skill_ids(kind, false).len(), 59);
        assert!(charm::skill_allowed(kind, 1, false));
    }
    // Every other kind has one, and the two lists differ by rarity.
    for kind in 3..=charm::TALISMAN_KIND_COUNT {
        assert!(charm::second_skill_available(kind), "kind {}", kind);
        assert!(!charm::allowed_skill_ids(kind, true).is_empty(), "kind {}", kind);
    }
}

#[test]
fn the_picker_list_is_the_slot_not_the_skill_name_table() {
    // 攻撃 (18) is a second-slot skill of a 天: it cannot be the first skill,
    // which is exactly the mistake a 306 entry name picker invites.
    let sky_first = charm::allowed_skill_ids(10, false);
    let sky_second = charm::allowed_skill_ids(10, true);
    assert_eq!((sky_first.len(), sky_second.len()), (62, 94));
    assert!(!sky_first.contains(&18), "攻撃 is not a 天 first skill");
    assert!(sky_second.contains(&18), "攻撃 is a 天 second skill");
    // 匠 (35) is the same story, and 納刀 (42) sits in both slots.
    assert!(!sky_first.contains(&35) && sky_second.contains(&35));
    assert!(sky_first.contains(&42) && sky_second.contains(&42));
    // 毒 (1) is on neither row of a 天 at all.
    assert!(!sky_first.contains(&1) && !sky_second.contains(&1));
    // 聴覚保護 (5) and 痛撃 (71) are first-slot skills of a 天.
    assert!(sky_first.contains(&5) && sky_first.contains(&71));
    // The console and the desktop build the same pool: the list is the core's.
    for kind in 1..=charm::TALISMAN_KIND_COUNT {
        for second in [false, true] {
            for id in charm::allowed_skill_ids(kind, second) {
                assert!(
                    charm::allowed_points_for_kind(kind, id, second)
                        .map(|r| r.min != 0 || r.max != 0)
                        .unwrap_or(false),
                    "kind {} id {} slot {}",
                    kind,
                    id,
                    if second { 2 } else { 1 }
                );
            }
        }
    }
    // Past the end of the talisman tables no kind can carry a skill.
    assert!(!charm::skill_allowed(10, 210, false));
    assert!(charm::allowed_points_for_kind(10, 210, false).is_none());
}

#[test]
fn the_per_skill_ceiling_is_narrower_than_the_kind_ceiling() {
    // 天 reaches +10 on the first skill, but 聴覚保護 stops at +7: the table
    // row, not the kind, is what the field and the warning use.
    assert_eq!(
        charm::allowed_points_for_kind(10, 5, false),
        Some(mhnx_core::charm::AllowedPoints { min: 3, max: 7 })
    );
    // 納刀 reaches the kind's full +10.
    assert_eq!(
        charm::allowed_points_for_kind(10, 42, false),
        Some(mhnx_core::charm::AllowedPoints { min: 5, max: 10 })
    );
    // 女王 (rarity 99) stops its first skill at +8 for the skills that reach
    // it, and 聴覚保護 there is 1~5.
    assert_eq!(charm::kind_limits(5).unwrap().max_points(false), Some(8));
    assert_eq!(
        charm::allowed_points_for_kind(5, 5, false),
        Some(mhnx_core::charm::AllowedPoints { min: 1, max: 5 })
    );
    // A skill the slot cannot take answers "0~0", which is what disables the
    // field rather than clamping it to zero points.
    assert_eq!(
        charm::allowed_points_for_kind(10, 18, false),
        Some(mhnx_core::charm::AllowedPoints { min: 0, max: 0 })
    );
}

/// A talisman record of a kind, as the sample save holds them, with the pair
/// at +0x02 / +0x12 written from the kind table.
fn record_of_kind(kind: u8) -> [u8; charm::CHARM_RECORD] {
    let mut r = [0u8; charm::CHARM_RECORD];
    r[0x00] = 0x06;
    r[0x02] = kind;
    r[0x10] = 3;
    r[0x12] = charm::kind_rarity(kind).unwrap();
    r[0x13] = 1;
    r
}

#[test]
fn a_value_over_the_per_skill_ceiling_is_clamped_and_reported() {
    // 聴覚保護 +9 on a 天: legal for the kind (it reaches +10) but not for the
    // skill (3~7). The clamp has to bring it to +7 and the warning has to name
    // the row range, because +9 is exactly what a kind-only editor writes.
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    let mut r = record_of_kind(10);
    r[0x0C] = 5; // 聴覚保護
    r[0x0E] = 9;
    r[0x0D] = 0;
    r[0x0F] = 0;
    r[0x10] = 1;
    b[..charm::CHARM_RECORD].copy_from_slice(&r);

    let warnings = charm::grade_warnings(&b, 0, charm::TalismanGrade::Enduring);
    assert!(
        warnings.iter().any(|w| w.contains("スキル1") && w.contains("3〜7")),
        "the warning has to name the per-skill range: {:?}",
        warnings
    );
    assert!(charm::clamp_points(&mut b, 0, charm::TalismanGrade::Enduring));
    assert_eq!(b[0x0E] as i8, 7, "聴覚保護 stops at +7 on a 天");
    // Once clamped the record is ordinary for the kind and the grade.
    assert!(
        charm::grade_warnings(&b, 0, charm::TalismanGrade::Enduring).is_empty(),
        "{:?}",
        charm::grade_warnings(&b, 0, charm::TalismanGrade::Enduring)
    );

    // A 女王's first skill stops at +8 even for a skill whose own row is 1~8;
    // 聴覚保護 there is 1~5, so +7 is clamped to +5.
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    let mut r = record_of_kind(5);
    r[0x0C] = 5;
    r[0x0E] = 7;
    r[0x10] = 1;
    b[..charm::CHARM_RECORD].copy_from_slice(&r);
    assert!(charm::clamp_points(&mut b, 0, charm::TalismanGrade::Timeworn));
    assert_eq!(b[0x0E] as i8, 5, "the per-skill row wins over the kind");
}

#[test]
fn a_second_skill_on_a_soldier_is_written_and_reported() {
    // 兵士 cannot carry a second skill, but the value is kept rather than
    // silently deleted: the warning is the whole point.
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    let mut r = record_of_kind(1);
    r[0x0C] = 1;
    r[0x0E] = 5;
    r[0x0D] = 2;
    r[0x0F] = 5;
    r[0x10] = 1;
    b[..charm::CHARM_RECORD].copy_from_slice(&r);
    let warnings = charm::grade_warnings(&b, 0, charm::TalismanGrade::Mystery);
    assert!(
        warnings.iter().any(|w| w.contains("スキル2") && w.contains("第2スキルを持てません")),
        "{:?}",
        warnings
    );
    // Clamping to the kind leaves the second skill alone (it is the user's
    // call) and the second skill's +5 is already inside nothing at all.
    assert!(!charm::clamp_points_to_kind(&mut b, 0));
    assert_eq!(b[0x0D], 2);
}

#[test]
fn the_real_save_never_exceeds_the_table_of_its_kind() {
    let Some(save) = real_save() else {
        eprintln!("sample save not present - skipping");
        return;
    };
    let mut records = 0usize;
    let mut values = 0usize;
    let mut violations = 0usize;
    let mut fit_only_own = 0usize;
    // (rarity, second, skill) -> (low, high, samples)
    let mut observed: BTreeMap<(u8, bool, u8), (i8, i8, usize)> = BTreeMap::new();
    for slot in 0..3 {
        let Some(box_bytes) = save.equipment_box(slot) else { continue };
        for i in 0..charm::CHARM_COUNT {
            let o = i * charm::CHARM_RECORD;
            let rec = &box_bytes[o..o + charm::CHARM_RECORD];
            if !charm::is_talisman(rec) {
                continue;
            }
            records += 1;
            let Some(limits) = charm::kind_limits(rec[0x02]) else { continue };
            let rarity = limits.rarity;
            let fits = |rarity: u8| -> bool {
                for (skill, points_at, second) in [
                    (rec[0x0C], 0x0Eusize, false),
                    (rec[0x0D], 0x0F, true),
                ] {
                    if skill == 0 {
                        continue;
                    }
                    let Some(cell) = table(rarity).get(skill as usize) else {
                        return false;
                    };
                    let (min, max) = range(cell, second);
                    if (min, max) == (0, 0) {
                        return false;
                    }
                    let points = rec[points_at] as i8;
                    if points < min || points > max {
                        return false;
                    }
                }
                true
            };
            let accepted: Vec<u8> = [97u8, 98, 99, 100]
                .into_iter()
                .filter(|r| fits(*r))
                .collect();
            if accepted == [rarity] {
                fit_only_own += 1;
            }
            for (skill, points_at, second) in [
                (rec[0x0C], 0x0Eusize, false),
                (rec[0x0D], 0x0F, true),
            ] {
                if skill == 0 {
                    continue;
                }
                values += 1;
                let points = rec[points_at] as i8;
                match charm::allowed_points_for_kind(limits.kind, skill, second) {
                    Some(r) if r.min == 0 && r.max == 0 => violations += 1,
                    Some(r) => {
                        if points < r.min || points > r.max {
                            violations += 1;
                        }
                        let entry = observed.entry((rarity, second, skill)).or_insert((
                            points, points, 0,
                        ));
                        entry.0 = entry.0.min(points);
                        entry.1 = entry.1.max(points);
                        entry.2 += 1;
                    }
                    None => violations += 1,
                }
            }
        }
    }
    assert_eq!(records, 650, "the sample save carries 650 talismans");
    assert_eq!(values, 1292, "and 1292 skill values on the records with a kind");
    assert_eq!(violations, 0, "no value may fall outside its row");
    assert_eq!(
        fit_only_own, 609,
        "most records fit only their own table - the mapping is what pins it down"
    );

    // The table is an upper bound on every pair the save exercises, and the
    // maxima it reaches are the table's own numbers.
    let mut equal = 0usize;
    let mut below = 0usize;
    let mut above = 0usize;
    for ((rarity, second, skill), (_, high, _)) in &observed {
        let (_, table_max) = range(table(*rarity)[*skill as usize], *second);
        if *high == table_max {
            equal += 1;
        } else if *high < table_max {
            below += 1;
        } else {
            above += 1;
        }
    }
    assert_eq!(
        (observed.len(), equal, below, above),
        (116, 75, 41, 0),
        "the table is an upper bound the sample reaches but never passes"
    );
}
