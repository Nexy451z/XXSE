//! The palico equipment box on a real save.
//!
//! The live save is only ever read. The test that writes to disk works on a copy
//! in the temp directory, named "system" because the Switch container header
//! carries a hash of the file name.

use std::path::{Path, PathBuf};

use mhnx_core::equip;
use mhnx_core::equipops::EquipError;
use mhnx_core::myset;
use mhnx_core::palico;
use mhnx_core::palicogear::{self, PALICO_EQUIP_COUNT, PALICO_EQUIP_OFFSET};
use mhnx_core::save::{Save, OFF_EQUIPMENT_BOX, SLOT_DATA_SIZE};

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

fn real_save() -> Option<Save> {
    Save::from_bytes(&std::fs::read(real_save_path()?).ok()?).ok()
}

/// Copy the real save into a directory of its own and return (directory, copy).
fn copy_of_the_real_save(tag: &str) -> Option<(PathBuf, PathBuf)> {
    let src = real_save_path()?;
    let dir = std::env::temp_dir().join(format!("mhnx_palicogear_{}_{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("system");
    std::fs::copy(&src, &dst).ok()?;
    Some((dir, dst))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn slot_base(save: &Save, slot: usize) -> usize {
    save.slot_base(slot).expect("the sample save has this slot") as usize
}

fn record_bytes(save: &Save, slot: usize, index: usize) -> Vec<u8> {
    palicogear::record(save, slot, index)
        .expect("the index is inside the box")
        .to_vec()
}

#[test]
fn the_box_holds_only_palico_gear() {
    let Some(save) = real_save() else { return };
    let used = palicogear::decode(&save, 0);
    assert!(!used.is_empty(), "the sample character has palico gear");
    assert_eq!(used.len(), palicogear::used(&save, 0));

    for e in &used {
        assert!(
            palicogear::is_palico_kind(e.kind),
            "record {} holds type {}, which the hunter's box would carry",
            e.index,
            e.kind
        );
        assert!(e.index < PALICO_EQUIP_COUNT);
        assert!(e.level >= 1 && e.level <= palicogear::MAX_LEVEL);
        // The record really is the one the index names.
        assert_eq!(palicogear::decode_one(&save, 0, e.index).as_ref(), Some(e));
    }

    // The box is neither empty nor nearly full: the whole point of the offset is
    // that it lands on a box with a believable fill level.
    let half = PALICO_EQUIP_COUNT / 2;
    assert!(
        used.len() < half,
        "{} of {} records in use, which is too many to be this box",
        used.len(),
        PALICO_EQUIP_COUNT
    );
    assert!(palicogear::decode(&save, 0).iter().all(|e| e.index < PALICO_EQUIP_COUNT));
}

#[test]
fn every_palico_points_at_a_record_of_the_right_kind() {
    let Some(save) = real_save() else { return };
    let mut checked = 0usize;
    for p in palico::used(&save, 0) {
        for (part, index) in p.equip.iter().enumerate() {
            if *index == 0xFFFF {
                continue;
            }
            let rec = palicogear::record(&save, 0, *index as usize)
                .expect("the worn index is inside the box");
            let eq = equip::decode_record(*index as usize, rec).expect("the record decodes");
            let want = palicogear::KINDS[part];
            assert_eq!(
                eq.kind, want,
                "palico {} wears box record {} for part {}, but that record holds type {}",
                p.index, index, part, eq.kind
            );
            // And the reverse direction: the box says this palico wears it.
            assert!(
                palicogear::worn_by_all(&save, 0, *index as usize).contains(&p.index),
                "box record {} should report palico {} as wearing it",
                index,
                p.index
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "the sample character has a palico wearing something");
}

#[test]
fn the_palico_box_starts_where_the_hunter_box_ends() {
    // 2000 records of 36 bytes are 72000 = 0x17C2E bytes, so the hunter's box
    // ends exactly where the palico box begins.
    let hunter_start = OFF_EQUIPMENT_BOX;
    let hunter_end = hunter_start + equip::EQUIP_COUNT * equip::EQUIP_RECORD;
    assert_eq!(hunter_end, PALICO_EQUIP_OFFSET);
    assert_eq!(hunter_end, 0x17C2E);

    // 1000 records of 36 bytes are 36000 = 0x208CE, and that is where the
    // equipment set array begins, which closes the layout.
    let palico_end = PALICO_EQUIP_OFFSET + PALICO_EQUIP_COUNT * palicogear::RECORD;
    assert_eq!(palico_end, myset::OFF_EQUIP_SETS);
    assert_eq!(palico_end, 0x208CE);

    // Neither box overlaps the other, and both fit in a character slot.
    let hunter = hunter_start..hunter_end;
    let palico = PALICO_EQUIP_OFFSET..palico_end;
    assert!(hunter.end <= palico.start || palico.end <= hunter.start);
    assert!(palico.end <= SLOT_DATA_SIZE, "the palico box must fit in a slot");
    assert!(myset::OFF_EQUIP_SETS + myset::EQUIP_SETS * myset::EQUIP_SET_SIZE <= SLOT_DATA_SIZE);
}

#[test]
fn one_record_off_does_not_read_as_this_box() {
    let Some(save) = real_save() else { return };
    let body = save.body();
    let start = slot_base(&save, 0) + PALICO_EQUIP_OFFSET;

    // Read the thousand records at `start`: how many are not blank, and how
    // many of those carry a type the palico box cannot hold.
    let scan = |start: usize| -> (usize, usize) {
        let mut used = 0usize;
        let mut foreign = 0usize;
        for i in 0..PALICO_EQUIP_COUNT {
            let o = start + i * palicogear::RECORD;
            let Some(rec) = body.get(o..o + palicogear::RECORD) else {
                continue;
            };
            if equip::record_is_blank(rec) {
                continue;
            }
            used += 1;
            let head = u16::from_le_bytes([rec[0], rec[1]]);
            if !palicogear::is_palico_kind((head & 0x1F) as u8) {
                foreign += 1;
            }
        }
        (used, foreign)
    };

    let (used, foreign) = scan(start);
    assert!(used > 0, "the sample character has palico gear");
    assert_eq!(foreign, 0, "as placed, the box holds palico types only");
    // One record early or late and a record of a type this box never holds comes
    // in - the last of the hunter's box, or the first of the set array. That is
    // what makes the offset a boundary a test can fail on rather than a number
    // that merely looks right.
    let (_, early) = scan(start - palicogear::RECORD);
    assert!(early > 0, "one record early still looks like the palico box");
    let (_, late) = scan(start + palicogear::RECORD);
    assert!(late > 0, "one record late still looks like the palico box");

    // Two bytes off every record is misaligned, and almost every record that
    // is not blank decodes as a type the box cannot hold.
    let (shifted_used, shifted_foreign) = scan(start + 2);
    assert!(shifted_used > 0);
    assert!(
        shifted_foreign * 10 >= shifted_used * 9,
        "a two byte shift read as gear: only {} of {} records have a foreign type",
        shifted_foreign,
        shifted_used
    );
}

#[test]
fn the_two_boxes_read_as_different_boxes_in_the_real_save() {
    let Some(save) = real_save() else { return };
    let base = slot_base(&save, 0);
    let hunter = save.equipment_box(0).expect("the hunter's box is in range");
    let palico = palicogear::box_bytes(&save, 0).expect("the palico box is in range");

    // The accessor in save.rs and the constant in this module describe the same
    // bytes.
    assert_eq!(palico.len(), PALICO_EQUIP_COUNT * palicogear::RECORD);
    assert_eq!(palico.len(), 36_000);
    assert_eq!(&palico[..], save.palico_equipment_box(0).expect("in range"));
    // Where the two slices sit, counted from the start of the body, is the
    // offset this module and save.rs each name.
    let origin = save.body().as_ptr() as usize;
    assert_eq!(palico.as_ptr() as usize - origin, base + PALICO_EQUIP_OFFSET);
    assert_eq!(hunter.as_ptr() as usize - origin, base + OFF_EQUIPMENT_BOX);

    // The byte ranges are adjacent, not shared.
    assert!(hunter.as_ptr() as usize + hunter.len() <= palico.as_ptr() as usize);
    assert!(palico.as_ptr() as usize + palico.len() <= origin + base + SLOT_DATA_SIZE);

    // And the contents say the same: the hunter's box holds hunter types, the
    // palico box palico types. Reading one as the other never happens.
    for e in equip::decode_box(hunter) {
        assert!(
            !palicogear::is_palico_kind(e.kind),
            "hunter box record {} holds palico type {}",
            e.index,
            e.kind
        );
    }
    let palico_types: Vec<u8> = palicogear::decode(&save, 0).iter().map(|e| e.kind).collect();
    assert!(palico_types.iter().all(|k| palicogear::is_palico_kind(*k)));
}

#[test]
fn setting_a_level_writes_only_the_level_bits() {
    let Some(mut save) = real_save() else { return };
    let Some(before_eq) = palicogear::decode(&save, 0).first().copied() else {
        return;
    };
    let base = slot_base(&save, 0);
    let at = base + PALICO_EQUIP_OFFSET + before_eq.index * palicogear::RECORD;
    let before = save.body().to_vec();

    assert!(palicogear::set_level(&mut save, 0, before_eq.index, 7));
    let after = palicogear::decode_one(&save, 0, before_eq.index).expect("still decodes");
    assert_eq!(after.level, 7);
    assert_eq!(after.kind, before_eq.kind, "the type must not move");
    assert_eq!(after.id, before_eq.id);
    assert_eq!(after.transmog, before_eq.transmog);
    assert_eq!(after.transmog_level, before_eq.transmog_level);

    // Only the two head bytes may differ, and the tail in particular may not.
    let body_after = save.body().to_vec();
    for (off, (a, b)) in before.iter().zip(body_after.iter()).enumerate() {
        if a != b {
            assert!(
                off == at || off == at + 1,
                "byte 0x{:X} changed, outside the level field",
                off
            );
        }
    }
    let rec = record_bytes(&save, 0, before_eq.index);
    assert_eq!(&rec[palicogear::WRITABLE..], &before[at + palicogear::WRITABLE..at + 36]);

    // A level the five bit field cannot show is refused, not clamped.
    assert!(!palicogear::set_level(&mut save, 0, before_eq.index, 0));
    assert!(!palicogear::set_level(&mut save, 0, before_eq.index, 33));
    assert_eq!(record_bytes(&save, 0, before_eq.index), rec);
}

#[test]
fn writing_a_decoration_leaves_the_tail_alone() {
    let Some(mut save) = real_save() else { return };
    let Some(eq) = palicogear::decode(&save, 0).first().copied() else {
        return;
    };
    let base = slot_base(&save, 0);
    let at = base + PALICO_EQUIP_OFFSET + eq.index * palicogear::RECORD;
    let before = record_bytes(&save, 0, eq.index);
    let body_before = save.body().to_vec();

    assert!(palicogear::set_decoration(&mut save, 0, eq.index, 1, 2639));
    let after = record_bytes(&save, 0, eq.index);
    assert_eq!(equip::Equipment::decorations(0, &after)[1], 2639);
    assert_eq!(
        equip::Equipment::decorations(0, &after)[0],
        equip::Equipment::decorations(0, &before)[0]
    );
    assert_eq!(
        equip::Equipment::decorations(0, &after)[2],
        equip::Equipment::decorations(0, &before)[2]
    );
    // Socket 1 is +0x08..+0x0A. Nothing else in the record, the tail included.
    assert_eq!(&after[..0x08], &before[..0x08]);
    assert_eq!(&after[0x0A..], &before[0x0A..], "the tail is type owned");
    let body_after = save.body().to_vec();
    for (off, (a, b)) in body_before.iter().zip(body_after.iter()).enumerate() {
        if a != b {
            assert!(
                off >= at + 0x08 && off < at + 0x0A,
                "byte 0x{:X} changed, outside the socket",
                off
            );
        }
    }

    // A fourth socket does not exist.
    assert!(!palicogear::set_decoration(&mut save, 0, eq.index, 3, 1));
    assert_eq!(record_bytes(&save, 0, eq.index), after);

    // Zero empties a socket again without moving anything else.
    assert!(palicogear::set_decoration(&mut save, 0, eq.index, 1, 0));
    assert_eq!(equip::Equipment::decorations(0, &record_bytes(&save, 0, eq.index)), [0; 3]);
}

#[test]
fn create_fills_the_first_blank_record_and_clear_releases_it() {
    let Some(mut save) = real_save() else { return };
    let base = slot_base(&save, 0);
    let body_before = save.body().to_vec();
    let used_before = palicogear::used(&save, 0);

    let free = palicogear::first_free(&save, 0, palicogear::WEAPON).expect("the box has room");
    let index = palicogear::create(&mut save, 0, palicogear::WEAPON, 431, 2).expect("writes");
    assert_eq!(index, free, "the first blank record is the one used");
    let eq = palicogear::decode_one(&save, 0, index).expect("decodes");
    assert_eq!((eq.kind, eq.id, eq.level), (palicogear::WEAPON, 431, 2));
    assert_eq!(palicogear::used(&save, 0), used_before + 1);

    // Nothing outside the new record moved, and its tail stayed zero.
    let at = base + PALICO_EQUIP_OFFSET + index * palicogear::RECORD;
    let body_after = save.body().to_vec();
    let mut touched = 0usize;
    for (off, (a, b)) in body_before.iter().zip(body_after.iter()).enumerate() {
        if a != b {
            assert!(off >= at && off < at + palicogear::WRITABLE, "byte 0x{:X} changed", off);
            touched += 1;
        }
    }
    assert!(touched > 0, "the create really wrote something");
    assert_eq!(&body_after[at + palicogear::WRITABLE..at + 36], &[0u8; 24][..]);

    // Clearing it takes the record back to blank and offers it again.
    assert!(palicogear::clear(&mut save, 0, index));
    assert!(equip::record_is_blank(&record_bytes(&save, 0, index)));
    assert_eq!(palicogear::used(&save, 0), used_before);
    assert_eq!(palicogear::first_free(&save, 0, palicogear::HEAD), Some(index));
    assert!(!palicogear::clear(&mut save, 0, index), "already blank");
}

#[test]
fn a_worn_record_is_refused_before_it_is_cleared() {
    let Some(mut save) = real_save() else { return };
    let worn = (0..PALICO_EQUIP_COUNT).find(|i| palicogear::worn_by(&save, 0, *i).is_some());
    // The test is about a worn record, and the sample save has one.
    let Some(worn) = worn else { return };
    let before = record_bytes(&save, 0, worn);
    assert!(!palicogear::clear(&mut save, 0, worn), "a worn record must be refused");
    assert_eq!(record_bytes(&save, 0, worn), before, "a refusal writes nothing");
    assert!(!palicogear::worn_by_all(&save, 0, worn).is_empty());

    // A record nobody wears can be cleared, and the sample save has one.
    let free = (0..PALICO_EQUIP_COUNT).find(|i| {
        palicogear::record(&save, 0, *i)
            .map(|r| !equip::record_is_blank(r))
            .unwrap_or(false)
            && palicogear::worn_by_all(&save, 0, *i).is_empty()
    });
    let Some(free) = free else { return };
    assert!(palicogear::clear(&mut save, 0, free));
    assert!(equip::record_is_blank(&record_bytes(&save, 0, free)));
}

#[test]
fn bad_requests_are_refused_without_writing() {
    let Some(mut save) = real_save() else { return };
    let before = save.body().to_vec();

    // Types that do not belong in the palico box.
    assert_eq!(
        palicogear::create(&mut save, 0, 7, 1, 1),
        Err(EquipError::OutOfRange)
    );
    assert_eq!(
        palicogear::create(&mut save, 0, 0, 1, 1),
        Err(EquipError::OutOfRange)
    );
    assert!(palicogear::first_free(&save, 0, 7).is_none());
    assert!(palicogear::first_free(&save, 0, 0).is_none());

    // An id of zero and a level the field cannot hold.
    assert_eq!(
        palicogear::create(&mut save, 0, palicogear::HEAD, 0, 1),
        Err(EquipError::OutOfRange)
    );
    assert_eq!(
        palicogear::create(&mut save, 0, palicogear::HEAD, 1, 0),
        Err(EquipError::OutOfRange)
    );
    assert_eq!(
        palicogear::create(&mut save, 0, palicogear::HEAD, 1, 33),
        Err(EquipError::OutOfRange)
    );

    // A slot that does not exist, and indices past the end of the box.
    assert_eq!(
        palicogear::create(&mut save, 9, palicogear::HEAD, 1, 1),
        Err(EquipError::OutOfRange)
    );
    assert!(palicogear::record(&save, 0, PALICO_EQUIP_COUNT).is_none());
    assert!(palicogear::box_bytes(&save, 9).is_none());
    assert!(palicogear::decode(&save, 9).is_empty());
    assert!(!palicogear::set_level(&mut save, 0, PALICO_EQUIP_COUNT, 1));
    assert!(!palicogear::set_decoration(&mut save, 0, 0, 3, 1));

    // A blank record takes neither a level nor a decoration, and cannot be
    // cleared again.
    let blank = palicogear::first_free(&save, 0, palicogear::WEAPON).expect("room");
    assert!(!palicogear::set_level(&mut save, 0, blank, 2));
    assert!(!palicogear::set_decoration(&mut save, 0, blank, 0, 1));
    assert!(!palicogear::clear(&mut save, 0, blank));

    assert_eq!(save.body(), &before[..], "a refused request writes nothing");
}

#[test]
fn a_copy_round_trips_through_disk() {
    let Some((dir, copy)) = copy_of_the_real_save("roundtrip") else {
        return;
    };
    let original = std::fs::read(&copy).expect("the copy exists");
    let mut save = Save::from_bytes(&original).expect("the copy parses");
    let base = slot_base(&save, 0);
    let before = save.body().to_vec();

    let index = palicogear::create(&mut save, 0, palicogear::HEAD, 464, 1).expect("the box has room");
    let bytes = save.to_bytes();
    assert_eq!(bytes.len(), original.len(), "the file size cannot change");
    std::fs::write(&copy, &bytes).expect("the copy is writable");

    let back = Save::from_bytes(&std::fs::read(&copy).expect("readable")).expect("parses again");
    let eq = palicogear::decode_one(&back, 0, index).expect("decodes after the round trip");
    assert_eq!((eq.kind, eq.id, eq.level), (palicogear::HEAD, 464, 1));

    // Every changed body byte lies in the record that was written: the hunter's
    // box, the roster and the equipment sets are untouched.
    let at = base + PALICO_EQUIP_OFFSET + index * palicogear::RECORD;
    let after = back.body();
    assert_eq!(after.len(), before.len());
    for (off, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        if a != b {
            assert!(
                off >= at && off < at + palicogear::WRITABLE,
                "byte 0x{:X} changed outside the written record",
                off
            );
        }
    }
    assert!(back.slot_base(0).is_some());

    cleanup(&dir);
}
