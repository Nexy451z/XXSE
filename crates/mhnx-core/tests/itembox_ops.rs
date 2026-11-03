//! Item box operations on a real save.
//!
//! The live save is only ever read. Every test that writes works on a copy in
//! the temp directory, named "system" because the Switch container header
//! carries a hash of the file name.

use std::path::{Path, PathBuf};

use mhnx_core::itembox::{self, count_of, ItemSlot, MAX_COUNT, TOTAL_ITEM_SLOTS};
use mhnx_core::save::Save;

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
    let dir = std::env::temp_dir().join(format!("mhnx_itembox_{}_{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("system");
    std::fs::copy(&src, &dst).ok()?;
    Some((dir, dst))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// The box of the first character in a copy of the real save.
fn real_box(copy: &Path) -> Option<(Save, Vec<ItemSlot>)> {
    let save = Save::from_bytes(&std::fs::read(copy).ok()?).ok()?;
    let items = save.read_item_box(0)?;
    Some((save, items))
}

fn used(items: &[ItemSlot]) -> usize {
    items.iter().filter(|s| !s.is_blank()).count()
}

#[test]
fn adding_on_a_copy_raises_the_owned_total() {
    let Some((dir, copy)) = copy_of_the_real_save("add_total") else {
        return;
    };
    let Some(src) = real_save_path() else {
        cleanup(&dir);
        return;
    };
    let original = std::fs::read(&src).unwrap();
    assert_eq!(
        std::fs::read(&copy).unwrap(),
        original,
        "the copy starts identical to the real save"
    );

    let (mut save, mut items) = real_box(&copy).expect("the copy parses and decodes");
    assert_eq!(items.len(), TOTAL_ITEM_SLOTS);

    // An item that is already in the box but whose stack has room left.
    let target = items
        .iter()
        .find(|s| !s.is_blank() && s.count < MAX_COUNT as u8)
        .copied()
        .expect("the real box holds a stack that is not full");
    let before = count_of(&items, target.id);
    assert!(before > 0);

    let added = itembox::add(&mut items, target.id, 41);
    assert_eq!(added, 41, "there was room for everything");
    assert_eq!(count_of(&items, target.id), before + 41);
    assert!(
        items.iter().all(|s| s.count as u16 <= MAX_COUNT),
        "no stack may pass the cap"
    );

    // Write it back to the copy and read the copy again from disk.
    save.write_item_box(0, &items).expect("the box encodes");
    std::fs::write(&copy, save.to_bytes()).unwrap();
    let after = Save::from_bytes(&std::fs::read(&copy).unwrap())
        .expect("the edited copy parses")
        .read_item_box(0)
        .expect("the edited box decodes");
    assert_eq!(count_of(&after, target.id), before + 41);
    assert_eq!(after.len(), TOTAL_ITEM_SLOTS);

    assert_eq!(
        std::fs::read(&src).unwrap(),
        original,
        "the real save must not be touched"
    );
    cleanup(&dir);
}

#[test]
fn adding_stops_at_the_stack_cap_and_uses_free_records() {
    let Some((dir, copy)) = copy_of_the_real_save("add_cap") else {
        return;
    };
    let (mut save, mut items) = real_box(&copy).expect("the copy parses and decodes");

    // An id the box does not hold yet, inside the twelve bit range.
    let mut id = 4095u16;
    while items.iter().any(|s| s.id == id) {
        id -= 1;
    }
    let blanks_before = items.iter().filter(|s| s.is_blank()).count();
    assert!(blanks_before >= 3, "the real box has free records");

    let added = itembox::add(&mut items, id, 250);
    assert_eq!(added, 250, "99 + 99 + 52 fit");
    let stacks: Vec<u8> = items
        .iter()
        .filter(|s| s.id == id)
        .map(|s| s.count)
        .collect();
    assert_eq!(stacks, vec![99, 99, 52], "stacks are opened in box order");
    assert_eq!(count_of(&items, id), 250);
    assert_eq!(
        items.iter().filter(|s| s.is_blank()).count(),
        blanks_before - 3
    );

    // What the codec reads back is what was written.
    save.write_item_box(0, &items).expect("the box encodes");
    let again = save.read_item_box(0).expect("the box decodes");
    assert_eq!(count_of(&again, id), 250);
    assert_eq!(again, items);
    cleanup(&dir);
}

#[test]
fn adding_is_refused_when_every_record_is_taken() {
    let Some((dir, copy)) = copy_of_the_real_save("add_full") else {
        return;
    };
    let (_save, mut items) = real_box(&copy).expect("the copy parses and decodes");

    // Fill the free records so not one blank is left.
    let mut filler = 4095u16;
    while items.iter().any(|s| s.id == filler) {
        filler -= 1;
    }
    for slot in items.iter_mut() {
        if slot.is_blank() {
            *slot = ItemSlot {
                id: filler,
                count: MAX_COUNT as u8,
            };
        }
    }
    assert!(items.iter().all(|s| !s.is_blank()));

    // Another item has nowhere to go.
    let mut other = filler - 1;
    while items.iter().any(|s| s.id == other) {
        other -= 1;
    }
    let before = items.clone();
    assert_eq!(itembox::add(&mut items, other, 10), 0, "the box is full");
    assert_eq!(items, before, "a refused add changes nothing");

    // A stack that already sits at the cap takes nothing either.
    assert_eq!(itembox::add(&mut items, filler, 5), 0);
    assert_eq!(items, before);
    cleanup(&dir);
}

#[test]
fn dedupe_leaves_one_record_per_item() {
    let Some((dir, copy)) = copy_of_the_real_save("dedupe") else {
        return;
    };
    let (mut save, before) = real_box(&copy).expect("the copy parses and decodes");
    let mut items = before.clone();

    let used_before = used(&before);
    let freed = itembox::dedupe(&mut items);
    assert!(freed > 0, "the real box holds duplicate ids");
    assert_eq!(used(&items) + freed, used_before, "only duplicates go");

    let ids: Vec<u16> = items
        .iter()
        .filter(|s| !s.is_blank())
        .map(|s| s.id)
        .collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(ids.len(), unique.len(), "one record per id");

    // The record that stays is at the first place the id was seen, and the
    // rest of the box keeps its order.
    let mut first_seen: Vec<u16> = Vec::new();
    for slot in &before {
        if !slot.is_blank() && !first_seen.contains(&slot.id) {
            first_seen.push(slot.id);
        }
    }
    assert_eq!(ids, first_seen);

    // The total is the sum of the stacks, capped at what one record can hold.
    let mut shrunk = 0;
    for id in &unique {
        let sum: u32 = before
            .iter()
            .filter(|s| s.id == *id)
            .map(|s| s.count as u32)
            .sum();
        let want = sum.min(MAX_COUNT as u32);
        assert_eq!(count_of(&items, *id), want, "id {id}");
        if sum <= MAX_COUNT as u32 {
            assert_eq!(count_of(&items, *id), sum, "id {id} keeps its total");
        } else {
            shrunk += 1;
        }
    }
    assert!(
        shrunk > 0,
        "the real box holds more than 99 of some item, which the cap trims"
    );

    save.write_item_box(0, &items).expect("the box encodes");
    assert_eq!(save.read_item_box(0).unwrap(), items);
    cleanup(&dir);
}

#[test]
fn compact_closes_the_holes_and_keeps_the_order() {
    let Some((dir, copy)) = copy_of_the_real_save("compact") else {
        return;
    };
    let (mut save, mut items) = real_box(&copy).expect("the copy parses and decodes");

    // The real box keeps its holes at the end, so it is a deletion that leaves
    // one in the middle: blank two records well inside the used area.
    let holes: Vec<usize> = (10..500).filter(|&i| !items[i].is_blank()).take(2).collect();
    assert_eq!(holes.len(), 2);
    for &h in &holes {
        items[h] = ItemSlot::default();
    }
    let before = items.clone();

    let used_before: Vec<ItemSlot> = before
        .iter()
        .filter(|s| !s.is_blank())
        .copied()
        .collect();
    let last_before = before
        .iter()
        .rposition(|s| !s.is_blank())
        .expect("the box holds items");
    // How many records actually have to move for the holes to close.
    let mut expected = 0usize;
    let mut write = 0usize;
    for (i, slot) in before.iter().enumerate() {
        if slot.is_blank() {
            continue;
        }
        if i != write {
            expected += 1;
        }
        write += 1;
    }

    let moved = itembox::compact(&mut items);
    assert_eq!(moved, expected);

    let used_after: Vec<ItemSlot> = items
        .iter()
        .filter(|s| !s.is_blank())
        .copied()
        .collect();
    assert_eq!(used_after, used_before, "order and stacks are kept");
    assert_eq!(used_after.len(), used_before.len(), "nothing is lost");
    assert!(
        items[..used_after.len()].iter().all(|s| !s.is_blank()),
        "the used records are at the front"
    );
    assert!(
        items[used_after.len()..]
            .iter()
            .all(|s| s.is_blank() && s.count == 0),
        "the rest is blank, stale counts included"
    );

    let last_after = items
        .iter()
        .rposition(|s| !s.is_blank())
        .expect("the box still holds items");
    assert_eq!(last_after + 1, used_after.len());
    assert!(last_after < last_before, "the holes moved the end in");

    save.write_item_box(0, &items).expect("the box encodes");
    assert_eq!(save.read_item_box(0).unwrap(), items);
    cleanup(&dir);
}

#[test]
fn clamp_counts_pulls_a_broken_stack_back_to_the_cap() {
    let Some((dir, copy)) = copy_of_the_real_save("clamp") else {
        return;
    };
    let (mut save, mut items) = real_box(&copy).expect("the copy parses and decodes");

    // The real box is already within the cap.
    assert_eq!(itembox::clamp_counts(&mut items, MAX_COUNT), 0);

    // What a broken save or a hand edit leaves behind.
    let targets: Vec<usize> = (0..items.len())
        .filter(|&i| !items[i].is_blank())
        .take(2)
        .collect();
    assert_eq!(targets.len(), 2, "the box holds at least two items");
    let mut expect = items.clone();
    items[targets[0]].count = 127;
    items[targets[1]].count = 120;
    expect[targets[0]].count = 99;
    expect[targets[1]].count = 99;
    // A stale count in a blank record is not read by the game and is left be.
    if let Some(blank) = (0..items.len()).find(|&i| items[i].is_blank()) {
        items[blank].count = 127;
        expect[blank].count = 127;
    }

    let changed = itembox::clamp_counts(&mut items, MAX_COUNT);
    assert_eq!(changed, 2);
    assert!(
        items
            .iter()
            .filter(|s| !s.is_blank())
            .all(|s| s.count as u16 <= MAX_COUNT),
        "every used record is within the cap"
    );
    assert_eq!(items, expect, "only the over-cap records move");

    save.write_item_box(0, &items).expect("the box encodes");
    assert_eq!(save.read_item_box(0).unwrap(), items);
    cleanup(&dir);
}

#[test]
fn fill_all_tops_every_used_stack_and_ignores_blanks() {
    let Some((dir, copy)) = copy_of_the_real_save("fill") else {
        return;
    };
    let (_save, mut items) = real_box(&copy).expect("the copy parses and decodes");

    let used_count = used(&items);
    let already_full = items
        .iter()
        .filter(|s| !s.is_blank() && s.count == MAX_COUNT as u8)
        .count();

    // 999 is past the cap and is pulled down to it.
    let changed = itembox::fill_all(&mut items, 999);
    assert_eq!(changed, used_count - already_full);
    assert!(items
        .iter()
        .filter(|s| !s.is_blank())
        .all(|s| s.count == MAX_COUNT as u8));
    assert!(
        items.iter().filter(|s| s.is_blank()).all(|s| s.count == 0),
        "a blank record is not filled"
    );
    assert_eq!(itembox::fill_all(&mut items, MAX_COUNT), 0, "nothing left to do");
    cleanup(&dir);
}

#[test]
fn the_preview_predicts_what_dedupe_would_drop() {
    // Three stacks of one id that together exceed the cap, and one id that
    // fits: the preview must count the overflow and the freed records, and
    // agree with what dedupe then does.
    let mut items = vec![ItemSlot::default(); 8];
    items[0] = ItemSlot { id: 9, count: 60 };
    items[1] = ItemSlot { id: 9, count: 60 };
    items[2] = ItemSlot { id: 9, count: 40 };
    items[3] = ItemSlot { id: 11, count: 5 };
    items[4] = ItemSlot { id: 11, count: 6 };

    let preview = itembox::dedupe_preview(&items);
    // id 9 keeps its first record and frees two; id 11 frees one.
    assert_eq!(preview.freed, 3);
    // 60 + 60 + 40 = 160, cap 99, so 61 would be lost; id 11 is under the cap.
    assert_eq!(preview.lost, 61);

    let before_total: u32 = count_of(&items, 9) + count_of(&items, 11);
    let freed = itembox::dedupe(&mut items);
    assert_eq!(freed, preview.freed);
    let after_total: u32 = count_of(&items, 9) + count_of(&items, 11);
    assert_eq!(
        before_total - after_total,
        preview.lost,
        "the preview must match the loss"
    );
}

#[test]
fn a_box_with_nothing_to_merge_previews_nothing() {
    let mut items = vec![ItemSlot::default(); 4];
    items[0] = ItemSlot { id: 3, count: 1 };
    items[1] = ItemSlot { id: 4, count: 1 };
    let preview = itembox::dedupe_preview(&items);
    assert_eq!(preview, itembox::DedupePreview::default());
}