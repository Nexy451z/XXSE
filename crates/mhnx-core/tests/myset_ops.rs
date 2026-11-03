//! Equipment set operations on a real save.
//!
//! The live save is only ever read. Every test that writes works on a copy in
//! the temp directory, named "system" because the Switch container header
//! carries a hash of the file name.

use std::path::{Path, PathBuf};

use mhnx_core::equip::{self, Equipment, EQUIP_RECORD};
use mhnx_core::myset::{
    self, EMPTY_NAME, EMPTY_TAIL, EQUIP_SETS, EQUIP_SET_SIZE, NONE, OFF_EQUIP_SETS,
};
use mhnx_core::save::{Save, MAX_SLOTS, TOTAL_EQUIP_SLOTS};

fn real_save_path() -> Option<PathBuf> {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        return Some(stable);
    }
    let ryujinx = PathBuf::from(
        r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
    );
    if ryujinx.exists() {
        Some(ryujinx)
    } else {
        None
    }
}

/// Copy the real save into a directory of its own and return (directory, copy).
fn copy_of_the_real_save(tag: &str) -> Option<(PathBuf, PathBuf)> {
    let src = real_save_path()?;
    let dir = std::env::temp_dir().join(format!("mhnx_myset_{}_{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("system");
    std::fs::copy(&src, &dst).ok()?;
    Some((dir, dst))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn open(copy: &Path) -> Save {
    Save::from_bytes(&std::fs::read(copy).expect("the copy reads")).expect("the copy parses")
}

/// The real save, for the read-only tests.
fn real() -> Option<Save> {
    Save::from_bytes(&std::fs::read(real_save_path()?).ok()?).ok()
}

fn u16_at(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}

fn first_of_kind(save: &Save, slot: usize, kind: u8) -> Option<usize> {
    let box_bytes = save.equipment_box(slot)?;
    for index in 0..TOTAL_EQUIP_SLOTS {
        let o = index * EQUIP_RECORD;
        let rec = &box_bytes[o..o + EQUIP_RECORD];
        let eq = equip::decode_record(index, rec)?;
        if !eq.is_empty() && eq.kind == kind {
            return Some(index);
        }
    }
    None
}

fn first_of_kind_with_decos(save: &Save, slot: usize, kind: u8) -> Option<usize> {
    let box_bytes = save.equipment_box(slot)?;
    for index in 0..TOTAL_EQUIP_SLOTS {
        let o = index * EQUIP_RECORD;
        let rec = &box_bytes[o..o + EQUIP_RECORD];
        let eq = equip::decode_record(index, rec)?;
        if !eq.is_empty() && eq.kind == kind && equip::socketed(rec) > 0 {
            return Some(index);
        }
    }
    None
}

fn first_blank(save: &Save, slot: usize) -> Option<usize> {
    let box_bytes = save.equipment_box(slot)?;
    for index in 0..TOTAL_EQUIP_SLOTS {
        let o = index * EQUIP_RECORD;
        let rec = &box_bytes[o..o + EQUIP_RECORD];
        if equip::record_is_blank(rec) {
            return Some(index);
        }
    }
    None
}

fn box_decos(save: &Save, slot: usize, index: usize) -> [u16; 3] {
    let box_bytes = save.equipment_box(slot).expect("the box reads");
    let o = index * EQUIP_RECORD;
    Equipment::decorations(index, &box_bytes[o..o + EQUIP_RECORD])
}

fn record_at(save: &Save, slot: usize, set: usize) -> usize {
    save.slot_base(slot).expect("the slot has a base") as usize
        + OFF_EQUIP_SETS
        + set * EQUIP_SET_SIZE
}

fn record_bytes(save: &Save, slot: usize, set: usize) -> Vec<u8> {
    let at = record_at(save, slot, set);
    save.body()[at..at + EQUIP_SET_SIZE].to_vec()
}

// ---------------------------------------------------------------------------
// Writing one part
// ---------------------------------------------------------------------------

#[test]
fn a_part_is_written_to_the_index_and_the_decoration_copy() {
    let Some((dir, copy)) = copy_of_the_real_save("part") else {
        return;
    };
    let src = real_save_path().expect("the copy came from a real save");
    let original = std::fs::read(&src).unwrap();
    assert_eq!(
        std::fs::read(&copy).unwrap(),
        original,
        "the copy starts identical to the real save"
    );

    let mut save = open(&copy);
    let index =
        first_of_kind_with_decos(&save, 0, 2).expect("the box holds a body piece with a gem");
    let want = box_decos(&save, 0, index);
    assert_ne!(want, [0, 0, 0]);

    // An unused record: what a player gets when they register a new set.
    let set = myset::equip_sets(&save, 0)
        .into_iter()
        .find(|s| s.is_empty())
        .map(|s| s.index)
        .expect("the sample has unused sets");
    let at = record_at(&save, 0, set);
    let before = save.to_bytes();

    assert!(myset::set_part(&mut save, 0, set, 2, index as u16));

    let read = &myset::equip_sets(&save, 0)[set];
    assert_eq!(read.parts[2], index as u16, "the index at +0x2A");
    assert_eq!(read.decos[2], want, "the copy at +0x38");

    // Both writes are in the raw record where the game reads them.
    let body = save.body();
    assert_eq!(
        u16_at(body, at + myset::EQUIP_SET_BODY + 2 * 2),
        index as u16
    );
    let dat = at + myset::EQUIP_SET_DECOS + 2 * myset::EQUIP_SET_DECO_SIZE;
    assert_eq!(
        [
            u16_at(body, dat),
            u16_at(body, dat + 2),
            u16_at(body, dat + 4)
        ],
        want
    );

    // Nothing else in the record, or the file, moved.
    let after = save.to_bytes();
    let header = after.len() - save.body().len();
    let mut allowed = Vec::new();
    for k in 0..2 {
        allowed.push(header + at + myset::EQUIP_SET_BODY + 2 * 2 + k);
    }
    for k in 0..6 {
        allowed.push(header + at + myset::EQUIP_SET_DECOS + 2 * myset::EQUIP_SET_DECO_SIZE + k);
    }
    let outside: Vec<usize> = (0..after.len())
        .filter(|i| after[*i] != before[*i])
        .filter(|i| !allowed.contains(i))
        .collect();
    assert!(outside.is_empty(), "wrote outside the part: {:?}", outside);

    // What the codec reads is what a reopened file holds.
    std::fs::write(&copy, save.to_bytes()).unwrap();
    let again = myset::equip_sets(&open(&copy), 0);
    assert_eq!(again[set].parts[2], index as u16);
    assert_eq!(again[set].decos[2], want);

    assert_eq!(
        std::fs::read(&src).unwrap(),
        original,
        "the real save must not be touched"
    );
    cleanup(&dir);
}

#[test]
fn a_refused_write_changes_nothing() {
    let Some((dir, copy)) = copy_of_the_real_save("refuse") else {
        return;
    };
    let mut save = open(&copy);
    let index = first_of_kind(&save, 0, 2).expect("a body piece");
    let blank = first_blank(&save, 0).expect("a blank record");
    assert_ne!(blank, index);
    let before = save.to_bytes();

    assert!(!myset::set_part(&mut save, 0, EQUIP_SETS, 0, index as u16));
    assert!(!myset::set_part(
        &mut save,
        0,
        0,
        myset::EQUIP_SET_PARTS,
        index as u16
    ));
    assert!(!myset::set_part(
        &mut save,
        0,
        0,
        0,
        TOTAL_EQUIP_SLOTS as u16
    ));
    assert!(
        !myset::set_part(&mut save, 0, 0, 0, 0xFFFE),
        "outside the box"
    );
    assert!(
        !myset::set_part(&mut save, 0, 0, 0, blank as u16),
        "a blank record names nothing"
    );
    assert!(
        !myset::set_part(&mut save, MAX_SLOTS, 0, 0, index as u16),
        "no such slot"
    );
    assert!(!myset::clear_part(&mut save, 0, 0, myset::EQUIP_SET_PARTS));
    assert!(!myset::clear_part(&mut save, 0, EQUIP_SETS, 0));
    assert!(
        !myset::set_colour(&mut save, 0, 0, 0, [1, 2, 3, 4]),
        "weapons have no colour"
    );
    assert!(
        !myset::set_colour(&mut save, 0, 0, myset::EQUIP_SET_PARTS - 1, [1, 2, 3, 4]),
        "talismans have no colour"
    );
    assert!(!myset::set_colour(
        &mut save,
        0,
        EQUIP_SETS,
        1,
        [1, 2, 3, 4]
    ));
    assert!(!myset::clear_set(&mut save, 0, EQUIP_SETS));
    assert!(!myset::set_name(&mut save, 0, EQUIP_SETS, "x"));
    assert!(myset::warnings(&save, 0, EQUIP_SETS).is_empty());
    assert!(myset::warnings(&save, MAX_SLOTS, 0).is_empty());

    assert_eq!(
        save.to_bytes(),
        before,
        "a refused write must not touch the save"
    );
    cleanup(&dir);
}

#[test]
fn clearing_a_part_blanks_the_index_and_its_decorations() {
    let Some((dir, copy)) = copy_of_the_real_save("clearpart") else {
        return;
    };
    let mut save = open(&copy);

    let (set, part) = {
        let sets = myset::equip_sets(&save, 0);
        let set = sets
            .iter()
            .find(|s| !s.is_empty() && s.decos.iter().any(|d| d.iter().any(|x| *x != 0)))
            .expect("the sample has a set with decorations");
        let part = (0..myset::EQUIP_SET_PARTS)
            .find(|&p| set.decos[p].iter().any(|d| *d != 0))
            .expect("that set has a decoration somewhere");
        (set.index, part)
    };
    let at = record_at(&save, 0, set);
    let before = save.to_bytes();
    assert!(myset::clear_part(&mut save, 0, set, part));

    let after_set = &myset::equip_sets(&save, 0)[set];
    assert_eq!(after_set.parts[part], NONE);
    assert_eq!(after_set.decos[part], [0, 0, 0]);

    let body = save.body();
    assert_eq!(u16_at(body, at + myset::EQUIP_SET_BODY + part * 2), NONE);
    let dat = at + myset::EQUIP_SET_DECOS + part * myset::EQUIP_SET_DECO_SIZE;
    assert_eq!(&body[dat..dat + 6], &[0u8; 6]);

    // Only those eight bytes changed.
    let after = save.to_bytes();
    let header = after.len() - save.body().len();
    let mut allowed = Vec::new();
    for k in 0..2 {
        allowed.push(header + at + myset::EQUIP_SET_BODY + part * 2 + k);
    }
    for k in 0..6 {
        allowed.push(header + at + myset::EQUIP_SET_DECOS + part * myset::EQUIP_SET_DECO_SIZE + k);
    }
    let outside: Vec<usize> = (0..after.len())
        .filter(|i| after[*i] != before[*i])
        .filter(|i| !allowed.contains(i))
        .collect();
    assert!(outside.is_empty(), "wrote outside the part: {:?}", outside);

    // NONE means the same thing as clear_part, so passing it writes nothing new.
    let cleared = save.to_bytes();
    assert!(myset::set_part(&mut save, 0, set, part, NONE));
    assert_eq!(
        save.to_bytes(),
        cleared,
        "clearing an empty part is a no-op"
    );
    cleanup(&dir);
}

#[test]
fn a_colour_goes_into_the_armour_part_it_names() {
    let Some((dir, copy)) = copy_of_the_real_save("colour") else {
        return;
    };
    let mut save = open(&copy);
    let before = myset::equip_sets(&save, 0)[0].colours;

    // Part 2 is the body, which is the second colour: the five colours are the
    // five armour pieces, not the seven slots a set names.
    assert!(myset::set_colour(
        &mut save,
        0,
        0,
        2,
        [0x11, 0x22, 0x33, 0xFF]
    ));
    let set = &myset::equip_sets(&save, 0)[0];
    assert_eq!(set.colours[1], [0x11, 0x22, 0x33, 0xFF]);
    for (i, colour) in before.iter().enumerate() {
        if i != 1 {
            assert_eq!(set.colours[i], *colour, "colour {} moved", i);
        }
    }
    let at = record_at(&save, 0, 0) + myset::EQUIP_SET_COLOURS + 4;
    assert_eq!(&save.body()[at..at + 4], &[0x11, 0x22, 0x33, 0xFF]);

    // Every armour part maps to its own colour.
    for part in 1..=5 {
        assert!(myset::set_colour(
            &mut save,
            0,
            0,
            part,
            [part as u8, 0, 0, 0xFF]
        ));
        assert_eq!(
            myset::equip_sets(&save, 0)[0].colours[part - 1][0],
            part as u8
        );
    }
    cleanup(&dir);
}

#[test]
fn clearing_a_set_leaves_the_record_the_game_writes() {
    let Some((dir, copy)) = copy_of_the_real_save("clearset") else {
        return;
    };
    let mut save = open(&copy);
    let sets = myset::equip_sets(&save, 0);
    let used = sets
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| s.index)
        .expect("the sample has a used set");
    let unused = sets
        .iter()
        .find(|s| s.is_empty())
        .map(|s| s.index)
        .expect("the sample has unused sets");

    let reference = record_bytes(&save, 0, unused);
    assert_eq!(&reference[0x78..0x88], &EMPTY_TAIL, "the default tail");
    assert_eq!(&reference[..EMPTY_NAME.len()], EMPTY_NAME.as_bytes());

    assert!(myset::clear_set(&mut save, 0, used));
    assert_eq!(
        record_bytes(&save, 0, used),
        reference,
        "a cleared record must look like one the game never used"
    );

    let set = &myset::equip_sets(&save, 0)[used];
    assert!(set.is_empty());
    assert_eq!(set.name, EMPTY_NAME);
    assert!(set.parts.iter().all(|p| *p == NONE));
    assert!(set.decos.iter().all(|d| d.iter().all(|x| *x == 0)));
    assert!(set.colours.iter().all(|c| *c == [0, 0, 0, 0]));
    assert_eq!(set.tail, EMPTY_TAIL);
    assert_eq!(set.own(), [1, 1, 1, 1, 1]);
    assert_eq!(set.style(), EMPTY_TAIL[10]);

    // The other thirty-nine records did not move.
    let after = myset::equip_sets(&save, 0);
    for s in &sets {
        if s.index != used {
            assert_eq!(&after[s.index], s, "set {} moved", s.index);
        }
    }

    // What the file holds after a reopen.
    std::fs::write(&copy, save.to_bytes()).unwrap();
    let again = myset::equip_sets(&open(&copy), 0);
    assert!(again[used].is_empty());
    assert_eq!(record_bytes(&open(&copy), 0, used), reference);
    cleanup(&dir);
}

#[test]
fn a_name_round_trips_and_never_splits_a_character() {
    let Some((dir, copy)) = copy_of_the_real_save("name") else {
        return;
    };
    let mut save = open(&copy);
    let parts_before = myset::equip_sets(&save, 0)[0].parts;

    assert!(myset::set_name(&mut save, 0, 0, "テスト装備"));
    assert_eq!(myset::equip_sets(&save, 0)[0].name, "テスト装備");
    assert_eq!(
        myset::equip_sets(&save, 0)[0].parts,
        parts_before,
        "renaming must not touch the equipment"
    );

    // Longer than the 31 byte field. The expected value is exactly what
    // write_name_field produces, and it has to be valid UTF-8.
    let long = "剣".repeat(20);
    let mut field = [0u8; 32];
    mhnx_core::write_name_field(&mut field, &long);
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    let want = std::str::from_utf8(&field[..end])
        .expect("a cut name stays valid UTF-8")
        .to_owned();
    assert!(want.len() <= 31);

    assert!(myset::set_name(&mut save, 0, 0, &long));
    let got = myset::equip_sets(&save, 0)[0].name.clone();
    assert_eq!(got, want);
    assert!(!got.contains('\u{FFFD}'), "the cut left a broken character");

    // An empty name clears the field rather than leaving the old one behind.
    assert!(myset::set_name(&mut save, 0, 0, ""));
    assert_eq!(myset::equip_sets(&save, 0)[0].name, "");
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// What the real save says
// ---------------------------------------------------------------------------

#[test]
fn the_real_sets_point_at_the_equipment_their_part_names() {
    let Some(save) = real() else { return };
    let mut checked = 0;
    let mut mismatched = 0;
    let mut talisman_in_armour = 0;

    for slot in 0..MAX_SLOTS {
        let sets = myset::equip_sets(&save, slot);
        assert_eq!(sets.len(), EQUIP_SETS, "slot {} record count", slot);
        for set in sets.iter().filter(|s| !s.is_empty()) {
            for part in 0..myset::EQUIP_SET_PARTS {
                let index = set.parts[part];
                if index == NONE {
                    continue;
                }
                checked += 1;
                assert!(
                    (index as usize) < TOTAL_EQUIP_SLOTS,
                    "set {} part {} names {}",
                    set.index,
                    part,
                    index
                );
                let eq = save
                    .equipment_at(slot, index as usize)
                    .unwrap_or_else(|| panic!("set {} part {} cannot be read", set.index, part));
                assert!(
                    !eq.is_empty(),
                    "set {} part {} names an empty record",
                    set.index,
                    part
                );
                // The part each index names, from the game's own numbering.
                let fits = match part {
                    0 => (7..=21).contains(&eq.kind) && eq.kind != 12,
                    1..=5 => eq.kind as usize == part,
                    6 => eq.kind == 6,
                    _ => false,
                };
                if !fits {
                    mismatched += 1;
                    assert!(
                        (1..=5).contains(&part) && eq.kind == 6,
                        "unexpected mismatch: set {} part {} holds kind {}",
                        set.index,
                        part,
                        eq.kind
                    );
                    talisman_in_armour += 1;
                }
            }
        }
    }

    assert!(
        checked > 100,
        "only {} part references were checked",
        checked
    );
    assert!(
        mismatched > 0,
        "the sample has a talisman in an armour part"
    );
    assert_eq!(
        mismatched, talisman_in_armour,
        "every mismatch is a talisman in an armour part"
    );
}

#[test]
fn unused_sets_are_recognised_and_carry_the_default_record() {
    let Some(save) = real() else { return };
    let sets = myset::equip_sets(&save, 0);
    assert_eq!(sets.len(), EQUIP_SETS);
    let unused: Vec<_> = sets.iter().filter(|s| s.is_empty()).collect();
    let used: Vec<_> = sets.iter().filter(|s| !s.is_empty()).collect();
    assert_eq!(unused.len() + used.len(), EQUIP_SETS);
    assert!(
        unused.len() > 20,
        "the sample keeps {} slots free",
        unused.len()
    );
    assert!(!used.is_empty());

    for set in &unused {
        assert_eq!(set.name, EMPTY_NAME, "set {}", set.index);
        assert!(set.parts.iter().all(|p| *p == NONE), "set {}", set.index);
        assert!(
            set.decos.iter().all(|d| d.iter().all(|x| *x == 0)),
            "set {} has decorations but no equipment",
            set.index
        );
        assert_eq!(set.tail, EMPTY_TAIL, "set {} tail", set.index);
    }
    for set in &used {
        assert_ne!(set.name, EMPTY_NAME);
        assert!(!set.name.is_empty());
        assert!(set.parts.iter().any(|p| *p != NONE), "set {}", set.index);
    }
}

#[test]
fn the_decoration_copy_is_read_from_the_set_record() {
    let Some(save) = real() else { return };
    let sets = myset::equip_sets(&save, 0);
    let mut with_decos = 0;
    for set in &sets {
        let at = record_at(&save, 0, set.index);
        for part in 0..myset::EQUIP_SET_PARTS {
            let o = at + myset::EQUIP_SET_DECOS + part * myset::EQUIP_SET_DECO_SIZE;
            let want = [
                u16_at(save.body(), o),
                u16_at(save.body(), o + 2),
                u16_at(save.body(), o + 4),
            ];
            assert_eq!(set.decos[part], want, "set {} part {}", set.index, part);
            if want.iter().any(|d| *d != 0) {
                with_decos += 1;
            }
        }
    }
    assert!(with_decos > 0, "the sample stores decorations in its sets");

    // Reading the copy is not a live read of the box: the sample already has
    // sets whose copy and equipment record disagree.
    let mut drift = 0;
    for set in sets.iter().filter(|s| !s.is_empty()) {
        for part in 0..myset::EQUIP_SET_PARTS {
            let index = set.parts[part];
            if index == NONE {
                continue;
            }
            if box_decos(&save, 0, index as usize) != set.decos[part] {
                drift += 1;
            }
        }
    }
    assert!(
        drift > 0,
        "the sample has copies the equipment record no longer matches"
    );
}

#[test]
fn a_set_keeps_the_style_and_arts_of_the_character() {
    let Some(save) = real() else { return };
    let base = save.slot_base(0).expect("slot 0 has a base") as usize;
    let body = save.body();

    // The worn set, as equiprefs reads it.
    let worn: [u16; myset::EQUIP_SET_PARTS] =
        std::array::from_fn(|i| u16_at(body, base + mhnx_core::equiprefs::WORN_OFFSET + i * 2));
    // The character's own copy: +0x23B4C is the hunting style, +0x2C..+0x32 the
    // three hunter arts (u16 each).
    let style = body[base + 0x23B4C];
    let arts: [u16; 3] = std::array::from_fn(|i| u16_at(body, base + 0x2C + i * 2));
    assert!(arts.iter().all(|a| *a <= u8::MAX as u16));

    let mirror = myset::equip_sets(&save, 0)
        .into_iter()
        .find(|s| s.parts == worn)
        .expect("a set mirrors the equipment the character wears");
    assert_eq!(mirror.style(), style, "the set style byte");
    assert_eq!(mirror.arts(), [arts[0] as u8, arts[1] as u8, arts[2] as u8]);

    // The ten bytes before the style are zero in every record of the sample,
    // and so are the two after the arts.
    assert!(mirror.modes().iter().all(|b| *b == 0));
    assert!(mirror.sp().iter().all(|b| *b == 0));
    assert_eq!(mirror.tail[style_index()], style);
}

/// Where the style sits inside the tail.
fn style_index() -> usize {
    myset::EQUIP_SET_STYLE - myset::EQUIP_SET_TAIL
}

// ---------------------------------------------------------------------------
// Warnings
// ---------------------------------------------------------------------------

#[test]
fn warnings_name_what_the_sample_already_does_wrong() {
    let Some((dir, copy)) = copy_of_the_real_save("warn") else {
        return;
    };
    let mut save = open(&copy);
    let sets = myset::equip_sets(&save, 0);

    // A set that holds only equipment of the kinds its parts name says nothing.
    let clean = sets
        .iter()
        .find(|s| !s.is_empty() && myset::warnings(&save, 0, s.index).is_empty())
        .map(|s| s.index)
        .expect("the sample has a well formed set");
    // An unused record says nothing either.
    let unused = sets
        .iter()
        .find(|s| s.is_empty())
        .map(|s| s.index)
        .expect("the sample has unused sets");
    assert!(myset::warnings(&save, 0, unused).is_empty());

    // The sample itself: a talisman in a head slot, and copies that drifted.
    let mut talisman_in_head = false;
    let mut drifted = false;
    for set in sets.iter().filter(|s| !s.is_empty()) {
        let w = myset::warnings(&save, 0, set.index);
        if w.iter().any(|x| x.contains("頭") && x.contains("護石")) {
            talisman_in_head = true;
        }
        if w.iter().any(|x| x.contains("装飾品")) {
            drifted = true;
        }
    }
    assert!(talisman_in_head, "the sample has a talisman in a head slot");
    assert!(drifted, "the sample has decoration copies that drifted");
    assert_ne!(clean, unused);

    // Make the copy stale on purpose: change the equipment record's gem and
    // the set, which agreed, no longer does.
    let set = sets[clean].clone();
    let mut made_stale = false;
    for part in 0..myset::EQUIP_SET_PARTS {
        let index = set.parts[part];
        if index == NONE || set.decos[part] != box_decos(&save, 0, index as usize) {
            continue;
        }
        let replacement = if set.decos[part][0] == 0 { 2773 } else { 0 };
        if let Some(rec) = save.equipment_record_mut(0, index as usize) {
            equip::set_decoration(rec, 0, replacement);
            made_stale = true;
        }
        break;
    }
    assert!(made_stale, "a set whose copy agreed with the box");
    let w = myset::warnings(&save, 0, clean);
    assert!(
        w.iter().any(|x| x.contains("装飾品")),
        "a stale copy must be reported: {:?}",
        w
    );

    // Decorations left behind on a part that holds nothing.
    let at = record_at(&save, 0, unused) + myset::EQUIP_SET_DECOS;
    save.body_mut()[at..at + 2].copy_from_slice(&2700u16.to_le_bytes());
    let w = myset::warnings(&save, 0, unused);
    assert!(
        w.iter().any(|x| x.contains("装飾品が残っています")),
        "a leftover decoration must be reported: {:?}",
        w
    );

    // A record that names nothing outside the box.
    let at = record_at(&save, 0, unused) + myset::EQUIP_SET_BODY;
    save.body_mut()[at..at + 2].copy_from_slice(&(TOTAL_EQUIP_SLOTS as u16).to_le_bytes());
    let w = myset::warnings(&save, 0, unused);
    assert!(
        w.iter().any(|x| x.contains("装備枠の外")),
        "an index outside the box must be reported: {:?}",
        w
    );

    cleanup(&dir);
}
