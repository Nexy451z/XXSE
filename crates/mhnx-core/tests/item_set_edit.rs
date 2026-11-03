//! Item set contents and the pouch field, on a real save.
//!
//! The live save is only ever read. Every test that writes works on a copy in
//! the temp directory, named "system" because the Switch container header
//! carries a hash of the file name.

use std::ops::Range;
use std::path::{Path, PathBuf};

use mhnx_core::itembox::MAX_COUNT;
use mhnx_core::myset::{self, SetItem, EMPTY_NAME};
use mhnx_core::save::{Save, MAX_SLOTS};

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
    let dir = std::env::temp_dir().join(format!("mhnx_item_set_{}_{}", std::process::id(), tag));
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

/// Where an item set record starts in the save body.
fn set_at(save: &Save, slot: usize, set: usize) -> usize {
    save.slot_base(slot).expect("the slot has a base") as usize
        + myset::OFF_ITEM_SETS
        + set * myset::ITEM_SET_SIZE
}

/// Where the pouch field starts in the save body.
fn pouch_at(save: &Save, slot: usize) -> usize {
    save.slot_base(slot).expect("the slot has a base") as usize + myset::OFF_POUCH
}

/// An unused record, optionally skipping one index.
fn unused_set(save: &Save, slot: usize, skip: Option<usize>) -> usize {
    myset::item_sets(save, slot)
        .into_iter()
        .find(|s| s.is_empty() && Some(s.index) != skip)
        .map(|s| s.index)
        .expect("the sample has unused item sets")
}

/// A pair holding nothing.
fn empty_item() -> SetItem {
    SetItem { id: 0, count: 0 }
}

/// The bytes that changed outside every allowed range, as body offsets.
fn changed_beyond(before: &[u8], after: &[u8], allowed: &[Range<usize>]) -> Vec<usize> {
    (0..after.len())
        .filter(|i| after.get(*i) != before.get(*i))
        .filter(|i| !allowed.iter().any(|r| r.contains(i)))
        .collect()
}

// ---------------------------------------------------------------------------
// One entry of a set
// ---------------------------------------------------------------------------

#[test]
fn an_item_set_entry_is_written_read_back_and_refused_out_of_range() {
    let Some((dir, copy)) = copy_of_the_real_save("setitem") else {
        return;
    };
    let mut save = open(&copy);
    let set = unused_set(&save, 0, None);
    let at = set_at(&save, 0, set);
    let before = save.body().to_vec();

    assert!(myset::set_item(&mut save, 0, set, 2, 70, 99));
    let read = &myset::item_sets(&save, 0)[set];
    assert_eq!(read.items[2], SetItem { id: 70, count: 99 });

    // The pair sits where the game would look for it: +0x2A + 4 * index.
    let entry = at + myset::ITEM_SET_BODY + 2 * 4;
    assert_eq!(u16_at(save.body(), entry), 70);
    assert_eq!(u16_at(save.body(), entry + 2), 99);

    // Only those four bytes moved.
    let outside = changed_beyond(&before, save.body(), &[entry..entry + 4]);
    assert!(
        outside.is_empty(),
        "wrote outside the entry: {:?}",
        outside
    );

    // Everything out of range is refused, and a refusal changes nothing.
    let kept = save.body().to_vec();
    assert!(!myset::set_item(&mut save, 0, set, myset::ITEM_SET_ITEMS, 70, 1));
    assert!(!myset::set_item(&mut save, 0, myset::ITEM_SETS, 0, 70, 1));
    assert!(!myset::set_item(&mut save, 0, set, 0, 70, MAX_COUNT + 1));
    assert!(!myset::set_item(
        &mut save,
        0,
        set,
        0,
        mhnx_core::gamedata::LAST_ITEM_ID as u16 + 1,
        1
    ));
    assert!(
        !myset::set_item(&mut save, 0, set, 0, 0, 3),
        "an empty entry carries no count"
    );
    assert!(!myset::set_item(&mut save, MAX_SLOTS, set, 0, 70, 1));
    assert!(!myset::clear_item(
        &mut save,
        0,
        set,
        myset::ITEM_SET_ITEMS
    ));
    assert!(!myset::clear_item(&mut save, 0, myset::ITEM_SETS, 0));
    assert_eq!(
        save.body(),
        kept.as_slice(),
        "a refused write must not touch the save"
    );

    // Clearing zeroes both halves and leaves the name alone.
    let name = myset::item_sets(&save, 0)[set].name.clone();
    assert!(myset::clear_item(&mut save, 0, set, 2));
    let cleared = &myset::item_sets(&save, 0)[set];
    assert_eq!(cleared.items[2], empty_item());
    assert_eq!(cleared.name, name);

    // What a reopened file holds.
    assert!(myset::set_item(&mut save, 0, set, 5, 70, 9));
    std::fs::write(&copy, save.to_bytes()).unwrap();
    let again = myset::item_sets(&open(&copy), 0);
    assert_eq!(again[set].items[5], SetItem { id: 70, count: 9 });
    assert_eq!(again[set].items[2], empty_item());
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// The pouch field
// ---------------------------------------------------------------------------

/// The pouch is not a guess: it is the field the item set array ends at.
#[test]
fn the_pouch_is_the_field_the_item_sets_end_at() {
    let Some(save) = real() else { return };
    assert_eq!(myset::POUCH_SLOTS, 32);
    assert_eq!(myset::POUCH_SIZE, 76);
    assert_eq!(myset::OFF_POUCH, 0x27BF);
    assert_eq!(
        myset::OFF_ITEM_SETS + myset::ITEM_SETS * myset::ITEM_SET_SIZE,
        myset::OFF_POUCH,
        "the set array ends where the pouch begins"
    );
    assert_eq!(
        myset::OFF_POUCH + myset::POUCH_SIZE,
        mhnx_core::save::OFF_HR_POINTS,
        "the pouch ends where the hunter rank points begin"
    );

    let pouch = myset::pouch(&save, 0);
    assert_eq!(pouch.len(), myset::POUCH_SLOTS);
    let used: Vec<SetItem> = pouch.iter().copied().filter(|i| i.id != 0).collect();
    assert!(
        used.len() >= 20,
        "the sample carries a full pouch, only {} entries are used",
        used.len()
    );
    for item in &used {
        assert!(
            item.id as usize <= mhnx_core::gamedata::LAST_ITEM_ID,
            "pouch holds id {}",
            item.id
        );
        assert!(
            (1..=MAX_COUNT).contains(&item.count),
            "pouch holds count {}",
            item.count
        );
    }

    // The sets were registered from this pouch, which is what ties the offset
    // to the game and not to arithmetic alone: one of them agrees with the
    // pouch entry for entry on nearly everything it holds.
    let mut best = 0usize;
    let mut best_set = 0usize;
    for set in myset::item_sets(&save, 0).iter().filter(|s| !s.is_empty()) {
        let matched = set
            .items
            .iter()
            .copied()
            .filter(|i| i.id != 0 && used.contains(i))
            .count();
        if matched > best {
            best = matched;
            best_set = set.index;
        }
    }
    assert!(
        best >= 8,
        "no item set agrees with the pouch (best {} in set {})",
        best,
        best_set
    );

    // A slot the file does not hold has no pouch either.
    assert!(myset::pouch(&save, MAX_SLOTS).is_empty());
}

#[test]
fn a_pouch_entry_is_written_and_out_of_range_is_refused() {
    let Some((dir, copy)) = copy_of_the_real_save("pouchwrite") else {
        return;
    };
    let mut save = open(&copy);
    let at = pouch_at(&save, 0);
    let before = save.body().to_vec();

    assert!(myset::set_pouch_item(&mut save, 0, 5, 70, 9));
    assert_eq!(myset::pouch(&save, 0)[5], SetItem { id: 70, count: 9 });

    // The write stays inside the 76 bytes of the field.
    let outside = changed_beyond(&before, save.body(), &[at..at + myset::POUCH_SIZE]);
    assert!(
        outside.is_empty(),
        "wrote outside the pouch: {:?}",
        outside
    );

    let kept = save.body().to_vec();
    assert!(!myset::set_pouch_item(
        &mut save,
        0,
        myset::POUCH_SLOTS,
        70,
        1
    ));
    assert!(!myset::set_pouch_item(
        &mut save,
        0,
        0,
        70,
        MAX_COUNT + 1
    ));
    assert!(!myset::set_pouch_item(
        &mut save,
        0,
        0,
        mhnx_core::gamedata::LAST_ITEM_ID as u16 + 1,
        1
    ));
    assert!(!myset::set_pouch_item(&mut save, 0, 0, 0, 4));
    assert!(!myset::set_pouch_item(&mut save, MAX_SLOTS, 0, 70, 1));
    assert_eq!(save.body(), kept.as_slice());

    std::fs::write(&copy, save.to_bytes()).unwrap();
    assert_eq!(myset::pouch(&open(&copy), 0)[5], SetItem { id: 70, count: 9 });
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// Copying between the two
// ---------------------------------------------------------------------------

#[test]
fn the_pouch_and_a_set_round_trip_both_ways() {
    let Some((dir, copy)) = copy_of_the_real_save("roundtrip") else {
        return;
    };
    let mut save = open(&copy);
    let set = unused_set(&save, 0, None);
    let at = set_at(&save, 0, set);
    let pouch_before = myset::pouch(&save, 0);
    let used = pouch_before.iter().filter(|i| i.id != 0).count();
    let before = save.body().to_vec();

    assert_eq!(myset::copy_from_pouch(&mut save, 0, set), used);
    assert_eq!(myset::item_sets(&save, 0)[set].items, pouch_before);

    // The copy wrote the 128 byte body and nothing else.
    let body = at + myset::ITEM_SET_BODY
        ..at + myset::ITEM_SET_BODY + myset::ITEM_SET_ITEMS * 4;
    let outside = changed_beyond(&before, save.body(), &[body]);
    assert!(
        outside.is_empty(),
        "copy_from_pouch wrote outside the body: {:?}",
        outside
    );

    // Change the set, then load it back into the pouch.
    assert!(myset::set_item(&mut save, 0, set, 0, 9, 7));
    assert!(myset::clear_item(&mut save, 0, set, 1));
    let want = myset::item_sets(&save, 0)[set].items.clone();
    let want_used = want.iter().filter(|i| i.id != 0).count();
    let before_apply = save.body().to_vec();
    assert_eq!(myset::apply_to_pouch(&mut save, 0, set), want_used);
    assert_eq!(myset::pouch(&save, 0), want, "the pouch holds the set");

    let start = pouch_at(&save, 0);
    let outside = changed_beyond(
        &before_apply,
        save.body(),
        &[start..start + myset::POUCH_SIZE],
    );
    assert!(
        outside.is_empty(),
        "apply_to_pouch wrote outside the pouch: {:?}",
        outside
    );

    // An index that does not exist writes nothing.
    let kept = save.body().to_vec();
    assert_eq!(myset::copy_from_pouch(&mut save, 0, myset::ITEM_SETS), 0);
    assert_eq!(myset::apply_to_pouch(&mut save, 0, myset::ITEM_SETS), 0);
    assert_eq!(myset::copy_from_pouch(&mut save, MAX_SLOTS, 0), 0);
    assert_eq!(myset::apply_to_pouch(&mut save, MAX_SLOTS, 0), 0);
    assert_eq!(save.body(), kept.as_slice());

    // The record the game never used is not a loadout: loading it would empty
    // what the character carries, so the pouch keeps what it had.
    let other = unused_set(&save, 0, Some(set));
    assert_eq!(myset::item_sets(&save, 0)[other].name, EMPTY_NAME);
    let kept = save.body().to_vec();
    assert_eq!(myset::apply_to_pouch(&mut save, 0, other), 0);
    assert_eq!(save.body(), kept.as_slice());

    // What a reopened file holds.
    std::fs::write(&copy, save.to_bytes()).unwrap();
    let reopened = open(&copy);
    assert_eq!(myset::pouch(&reopened, 0), want);
    assert_eq!(myset::item_sets(&reopened, 0)[set].items, want);
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// Warnings
// ---------------------------------------------------------------------------

#[test]
fn warnings_report_what_a_broken_set_holds() {
    let Some((dir, copy)) = copy_of_the_real_save("warn") else {
        return;
    };
    let mut save = open(&copy);
    let set = unused_set(&save, 0, None);
    let at = set_at(&save, 0, set);

    // States the game does not write, put there by hand the way a broken save
    // or an older tool leaves them.
    let pairs = [
        (0usize, 0u16, 7u16), // a count on an empty entry
        (1, 9, 5),
        (2, 9, 3),    // the same id twice
        (3, 11, 120), // past the 99 cap
        (4, 12, 0),   // a used entry with no count
        (5, 3000, 1), // outside the item table
    ];
    for (index, id, count) in pairs {
        let o = at + myset::ITEM_SET_BODY + index * 4;
        save.body_mut()[o..o + 2].copy_from_slice(&id.to_le_bytes());
        save.body_mut()[o + 2..o + 4].copy_from_slice(&count.to_le_bytes());
    }
    let w = myset::item_set_warnings(&save, 0, set);
    let joined = w.join("\n");
    for needle in ["空きなのに個数", "重複", "上限", "個数が 0", "範囲外"] {
        assert!(joined.contains(needle), "{} missing from {:?}", needle, w);
    }
    assert!(
        joined.contains("'－－－'"),
        "the name is still the unused marker: {}",
        joined
    );

    // A record the game never used says nothing, and so does an index outside
    // the array.
    let clean = unused_set(&save, 0, Some(set));
    assert!(myset::item_set_warnings(&save, 0, clean).is_empty());
    assert!(myset::item_set_warnings(&save, 0, myset::ITEM_SETS).is_empty());
    assert!(myset::item_set_warnings(&save, MAX_SLOTS, 0).is_empty());

    // And the sets the sample itself holds are well formed: every count inside
    // the cap, no repeated id, nothing outside the table.
    for set in myset::item_sets(&save, 0).iter().filter(|s| !s.is_empty()) {
        let w = myset::item_set_warnings(&save, 0, set.index);
        assert!(
            w.is_empty(),
            "set {} of the sample warns: {:?}",
            set.index,
            w
        );
    }
    cleanup(&dir);
}
