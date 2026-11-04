//! The eleven talisman kinds - the byte at +0x02.
//!
//! The names come out of `romfs/nativeNX/table/amuletData_jpn.gmd`, not
//! out of a guess: the file holds eleven names in this order and its companion
//! `amuletData.amlt` states the count as 11 (ids 0..10).
//!
//! Two bugs are pinned here, both of which made a talisman the user asked for
//! come back as a different one in game:
//!
//! 1. A rarity was written by *guessing* the kind from the rarity table's first
//!    row, so switching a talisman to rarity 100 wrote 0x08 (英雄の護石) while
//!    the editor said 天の護石. The table's first row for a rarity is not the
//!    kind of the record, and for the sample save it is never even a value the
//!    game wrote.
//! 2. A new talisman was always a copy of the most common record in the box,
//!    which is 天の護石 in every sample save, so no other kind - 龍の護石
//!    included - could be created at all.

use std::path::PathBuf;

use mhnx_core::charm::{self, Charm, TalismanGrade};
use mhnx_core::save::Save;

/// A box with one real talisman and nothing else.
fn one_talisman_box() -> Vec<u8> {
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    b[..charm::CHARM_RECORD].copy_from_slice(&ten_record());
    b
}

/// A 天の護石 record with a legal shape, as the game writes one.
fn ten_record() -> [u8; charm::CHARM_RECORD] {
    let mut r = [0u8; charm::CHARM_RECORD];
    r[0x00] = 0x06; // talisman, level 0
    r[0x02] = 10;   // 天の護石
    r[0x0C] = 74;   // 1-based skill id
    r[0x0D] = 70;
    r[0x0E] = 6;
    r[0x0F] = 10;
    r[0x10] = 3;
    r[0x12] = 100;  // item id 356, low byte 100
    r[0x13] = 1;    // and its high byte
    r
}

/// The stable copy of the sample save, when this machine has one.
fn real_save() -> Option<Save> {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    let fallback = PathBuf::from(
        r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
    );
    let path = if stable.exists() { stable } else { fallback };
    Save::from_bytes(&std::fs::read(path).ok()?).ok()
}

#[test]
fn the_eleven_amulet_names_are_the_ones_in_the_game_table() {
    // Read out of amuletData_jpn.gmd: eleven names, row 0 is "装備なし".
    assert_eq!(charm::TALISMAN_KINDS.len(), 11);
    assert_eq!(charm::TALISMAN_KINDS[0], "装備なし");
    assert_eq!(charm::TALISMAN_KINDS[1], "兵士の護石");
    assert_eq!(charm::TALISMAN_KINDS[2], "闘士の護石");
    assert_eq!(charm::TALISMAN_KINDS[3], "騎士の護石");
    assert_eq!(charm::TALISMAN_KINDS[4], "城塞の護石");
    assert_eq!(charm::TALISMAN_KINDS[5], "女王の護石");
    assert_eq!(charm::TALISMAN_KINDS[6], "王の護石");
    // The one the report said was missing entirely.
    assert_eq!(charm::TALISMAN_KINDS[7], "龍の護石");
    assert_eq!(charm::TALISMAN_KINDS[8], "英雄の護石");
    assert_eq!(charm::TALISMAN_KINDS[9], "伝説の護石");
    assert_eq!(charm::TALISMAN_KINDS[10], "天の護石");

    // 0 is the "装備なし" row, so it is not a kind a record may carry.
    assert!(!charm::is_kind(0));
    assert!(charm::is_kind(1));
    assert!(charm::is_kind(10));
    assert!(!charm::is_kind(11));
    assert_eq!(charm::kind_name(7), Some("龍の護石"));
    assert_eq!(charm::kind_name(11), None);
}

/// The kind tiers are the game's own table: `gamedata::TALISMAN_RARITY` is
/// indexed by the byte at +0x02 and gives 97, 98, 99 or 100.
#[test]
fn a_kind_carries_the_rarity_the_game_table_gives_it() {
    for (kind, expected) in [
        (1u8, 97u8), (2, 97), (3, 98), (4, 98), (5, 99),
        (6, 99), (7, 99), (8, 100), (9, 100), (10, 100),
    ] {
        assert_eq!(charm::kind_rarity(kind), Some(expected), "kind {}", kind);
        assert_eq!(
            mhnx_core::gamedata::TALISMAN_RARITY[kind as usize], expected as i32,
            "kind {}: the crate's table and the game's must not drift",
            kind
        );
    }
    assert_eq!(charm::kind_rarity(0), None);
    // And the top kind of each group is the one a grade falls back to.
    assert_eq!(charm::canonical_kind(97), Some(2));
    assert_eq!(charm::canonical_kind(98), Some(4));
    assert_eq!(charm::canonical_kind(99), Some(7));
    assert_eq!(charm::canonical_kind(100), Some(10));
}

/// A new talisman must be the kind that was asked for, not the kind of the
/// template it was copied from.
#[test]
fn a_new_talisman_gets_the_kind_it_was_asked_for() {
    let mut b = one_talisman_box();
    let template = ten_record();
    let c = Charm { index: 5, slots: 2, skill1: 30, points1: 4, skill2: 0, points2: 0 };
    // 龍の護石: the kind no sample save holds, so the pair has to be written.
    assert!(charm::create_charm_kind(&mut b, 5, &template, &c, 7));
    let rec = &b[5 * charm::CHARM_RECORD..6 * charm::CHARM_RECORD];
    assert_eq!(rec[0x02], 7, "the kind byte");
    assert_eq!(rec[0x12], 99, "the rarity that belongs to 龍の護石");
    assert_eq!(rec[0x13], 1, "the high byte of the charm item id");
    assert_eq!(charm::kind_name_of_record(rec), Some("龍の護石"));
    assert_eq!(charm::rarity_of_record(rec), Some(TalismanGrade::Timeworn));
    // The five edited fields still land.
    assert_eq!((rec[0x0C], rec[0x0E], rec[0x10]), (30, 4, 2));
    // And the editor's list sees it as a talisman of that kind.
    let list = charm::decode_box(&b);
    assert_eq!(list.len(), 2);
    assert!(list.iter().any(|x| x.index == 5));
}

/// The old bug: changing a talisman to rarity 100 wrote 0x08 at +0x02, which
/// is 英雄の護石, while the screen said 天の護石.
#[test]
fn regrading_to_rarity_100_never_renames_a_talisman_to_the_hero_charm() {
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    // A 女王の護石 (kind 5, rarity 99) with nothing else in the box, so the
    // editor cannot copy a real 天の護石 record for the grade.
    let mut r = ten_record();
    r[0x02] = 5;
    r[0x12] = 99;
    b[..charm::CHARM_RECORD].copy_from_slice(&r);

    assert!(charm::set_grade(&mut b, 0, TalismanGrade::Enduring));
    let rec = &b[..charm::CHARM_RECORD];
    assert_eq!(rec[0x12], 100);
    assert_ne!(rec[0x02], 8, "0x08 is 英雄の護石, not 天の護石");
    assert_eq!(rec[0x02], 10, "the kind the user asked for");
    assert_eq!(rec[0x13], 1);
    assert_eq!(charm::kind_name_of_record(rec), Some("天の護石"));
}

/// A kind that already belongs to the target rarity is kept: switching a
/// 龍の護石 to "rarity 99" must not rename it to another kind of the group.
#[test]
fn regrading_within_a_group_keeps_the_kind() {
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    let mut r = ten_record();
    r[0x02] = 7;   // 龍の護石
    r[0x12] = 99;
    b[..charm::CHARM_RECORD].copy_from_slice(&r);
    assert!(charm::set_grade(&mut b, 0, TalismanGrade::Timeworn));
    assert_eq!(b[0x02], 7, "a 龍 must stay a 龍");
    assert_eq!(b[0x12], 99);
}

/// Every kind writes both linked bytes, so the game's name and the charm item
/// id always agree.
#[test]
fn every_kind_writes_the_paired_bytes() {
    for kind in 1..=charm::TALISMAN_KIND_COUNT {
        let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
        b[..charm::CHARM_RECORD].copy_from_slice(&ten_record());
        assert!(charm::set_kind(&mut b, 0, kind));
        let rec = &b[..charm::CHARM_RECORD];
        let rarity = charm::kind_rarity(kind).unwrap();
        assert_eq!(rec[0x02], kind);
        assert_eq!(rec[0x12], rarity, "kind {} rarity", kind);
        assert_eq!(rec[0x13], 1);
        assert_eq!(u16::from_le_bytes([rec[0x12], rec[0x13]]), 353 + (rarity as u16 - 97));
        assert_eq!(charm::kind_name_of_record(rec), charm::kind_name(kind));
        assert_eq!(
            charm::rarity_of_record(rec),
            TalismanGrade::from_rarity(rarity)
        );
    }
    // A kind the game does not ship is refused, and nothing moves.
    let mut b = vec![0u8; charm::CHARM_BOX_SIZE];
    b[..charm::CHARM_RECORD].copy_from_slice(&ten_record());
    let before = b[..charm::CHARM_RECORD].to_vec();
    assert!(!charm::set_kind(&mut b, 0, 0));
    assert!(!charm::set_kind(&mut b, 0, 11));
    assert_eq!(&b[..charm::CHARM_RECORD], &before[..]);
}

/// The save-level writer the UIs call: kind in, clamped record out.
#[test]
fn the_save_writes_a_chosen_kind_and_clamps_to_its_grade() {
    let mut s = mhnx_core::save::empty_switch_save();
    s.set_slot_used(0, true);
    {
        let b = s.equipment_box_mut(0).expect("box");
        b[..charm::CHARM_RECORD].copy_from_slice(&ten_record());
        b[0x10] = 3; // more slots than a 兵士の護石 may carry
    }
    assert!(s.set_charm_kind(0, 0, 1), "兵士の護石");
    assert_eq!(s.charm_kind(0, 0), Some(1));
    {
        let b = s.equipment_box(0).expect("box");
        assert_eq!(b[0x02], 1);
        assert_eq!(b[0x12], 97);
        assert_eq!(b[0x13], 1);
        assert_eq!(b[0x10], 1, "rarity 97 stops at one slot");
    }
    assert_eq!(
        charm::charm_kind(&s, 0, 0),
        Some(1),
        "the accessor reads the same byte"
    );

    // A brand new record gets the kind too, and a kind outside 1..=10 is
    // refused before anything is written.
    let c = Charm { index: 7, slots: 0, skill1: 26, points1: 2, skill2: 0, points2: 0 };
    assert!(s.set_charm_with_kind(0, &c, 10));
    let rec: Vec<u8> = {
        let b = s.equipment_box(0).expect("box");
        b[7 * charm::CHARM_RECORD..8 * charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(rec[0x02], 10);
    assert_eq!(rec[0x12], 100);
    assert_eq!(rec[0x13], 1);
    assert_eq!(charm::kind_name_of_record(&rec), Some("天の護石"));
    let before = {
        let b = s.equipment_box(0).expect("box");
        b[7 * charm::CHARM_RECORD..8 * charm::CHARM_RECORD].to_vec()
    };
    let c2 = Charm { index: 7, slots: 0, skill1: 1, points1: 1, skill2: 0, points2: 0 };
    assert!(!s.set_charm_with_kind(0, &c2, 0));
    let after = {
        let b = s.equipment_box(0).expect("box");
        b[7 * charm::CHARM_RECORD..8 * charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(before, after);
}

/// Every talisman of the sample save can be named, and the names are the ones
/// the game ships - the GUI prints them, so a None here would be a blank row.
#[test]
fn every_talisman_of_a_real_save_has_a_game_name() {
    let Some(s) = real_save() else { return };
    let mut seen = 0;
    let mut unknown = 0;
    for slot in 0..3 {
        let Some(b) = s.equipment_box(slot) else { continue };
        for c in charm::decode_box(b) {
            let o = c.index * charm::CHARM_RECORD;
            let rec = &b[o..o + charm::CHARM_RECORD];
            let name = charm::kind_name_of_record(rec).unwrap_or_else(|| {
                panic!("slot {} record {} has no kind name: {:#04x}", slot, c.index, rec[0x02])
            });
            seen += 1;
            let Some(rarity) = charm::kind_rarity(rec[0x02]) else {
                // The two records the sample save holds with 0x00 at +0x02:
                // documented, named by the table's row 0, and not a kind.
                assert_eq!(rec[0x02], 0, "only the 装備なし row may be off-table");
                assert_eq!(name, "装備なし");
                unknown += 1;
                continue;
            };
            assert!(name.ends_with("の護石"), "{}", name);
            assert_eq!(
                rarity, rec[0x12],
                "slot {} record {}: +0x02 and +0x12 disagree in the file",
                slot, c.index
            );
        }
    }
    assert!(seen > 0, "the sample save carries talismans");
    assert!(unknown <= 2, "at most the two documented records: {}", unknown);
}
/// Retyping a real talisman of the sample save moves the three bytes of the
/// pair and nothing else the editor does not own: the box already holds the
/// shape the game wrote, so a kind switch must not disturb it.
#[test]
fn retyping_a_real_talisman_moves_only_the_pair() {
    let Some(mut s) = real_save() else { return };
    let Some((slot, index)) = (0..3).find_map(|slot| {
        let b = s.equipment_box(slot)?;
        charm::decode_box(b).first().map(|c| (slot, c.index))
    }) else {
        return;
    };
    let before: Vec<u8> = {
        let b = s.equipment_box(slot).expect("box");
        b[index * charm::CHARM_RECORD..(index + 1) * charm::CHARM_RECORD].to_vec()
    };
    let old_kind = charm::kind_of_record(&before).expect("a kind");
    // 龍の護石 unless it already is one, then 天の護石.
    let want = if old_kind == 7 { 10 } else { 7 };
    assert!(s.set_charm_kind(slot, index, want));
    let after: Vec<u8> = {
        let b = s.equipment_box(slot).expect("box");
        b[index * charm::CHARM_RECORD..(index + 1) * charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(after[0x02], want);
    assert_eq!(after[0x12], charm::kind_rarity(want).unwrap());
    assert_eq!(after[0x13], 1);
    assert_eq!(after[0x00], before[0x00], "the type/level word is untouched");
    assert_eq!(after[0x01], before[0x01]);
    for at in 0..charm::CHARM_RECORD {
        if before[at] != after[at] {
            // A rarity change clamps the slots and the points, so those may
            // move too; every other byte must stay where the game put it.
            assert!(
                matches!(at, 0x02 | 0x0E | 0x0F | 0x10 | 0x12 | 0x13),
                "byte 0x{:02X} moved",
                at
            );
        }
    }
    assert_eq!(s.charm_kind(slot, index), Some(want));
    assert_eq!(
        s.charm_kind_name(slot, index),
        charm::kind_name(want)
    );
}
