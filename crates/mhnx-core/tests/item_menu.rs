//! The item box operation menu and the two-stage bulk add.
//!
//! The live save is only ever read: every test works on a copy in the temp
//! directory, named "system" because the Switch container header carries a
//! hash of the file name.

use std::path::{Path, PathBuf};

use mhnx_core::hbedit::{
    Editor, Key, Request, Screen, ITEM_OP_ADD, ITEM_OP_CLAMP, ITEM_OP_COMPACT, ITEM_OP_DEDUPE,
    ITEM_OP_FILL, ITEM_OP_UNDO, ITEM_OPS,
};
use mhnx_core::itembox::{self, count_of, ItemSlot, MAX_COUNT, TOTAL_ITEM_SLOTS};

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

/// A private copy of the real save, so a test that writes writes nothing else.
fn copy_of_the_real_save(tag: &str) -> Option<(PathBuf, PathBuf)> {
    let src = real_save_path()?;
    let dir = std::env::temp_dir().join(format!("mhnx_itemmenu_{}_{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("system");
    std::fs::copy(&src, &dst).ok()?;
    Some((dir, dst))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Open the copy through the request/answer path the console uses.
fn open(ed: &mut Editor, copy: &Path) {
    ed.set_save_files(vec![copy.to_string_lossy().to_string()]);
    ed.input(Key::A);
    match ed.take_request() {
        Some(Request::ReadSave(_)) => {
            let bytes = std::fs::read(copy).expect("the copy is readable");
            ed.open_save_bytes(&copy.to_string_lossy(), &bytes);
        }
        other => panic!("expected a save read, got {:?}", other),
    }
    assert_eq!(ed.screen, Screen::Slots, "the save opens on the slot screen");
}

fn open_items(ed: &mut Editor) {
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::Items);
}

fn open_menu(ed: &mut Editor) {
    ed.input(Key::Minus);
    assert!(ed.item_menu_open, "Minus must open the operation menu");
}

fn box_of(ed: &Editor) -> Vec<ItemSlot> {
    ed.save
        .as_ref()
        .expect("a save")
        .read_item_box(ed.slot)
        .expect("a box")
}

fn total(items: &[ItemSlot]) -> u32 {
    items
        .iter()
        .filter(|s| !s.is_blank())
        .map(|s| s.count as u32)
        .sum()
}

fn used(items: &[ItemSlot]) -> usize {
    items.iter().filter(|s| !s.is_blank()).count()
}

fn select(ed: &mut Editor, op: usize) {
    ed.item_menu_cursor = op;
    ed.item_armed = None;
}

/// Press [A] twice: the first press only arms a destructive row, the second
/// one is the edit. Both presses are the same key, which is the whole point.
fn press_twice(ed: &mut Editor) {
    ed.input(Key::A);
    assert!(
        ed.item_armed == Some(ed.item_menu_cursor),
        "the first press must only arm the row: {}",
        ed.message
    );
    ed.input(Key::A);
}

#[test]
fn the_menu_offers_every_row_and_says_how_much_each_would_change() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_rows") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);
    open_menu(&mut ed);

    // The console renders the same six rows the state machine runs.
    assert_eq!(ITEM_OPS.len(), 6);
    let text: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = text.join("\n");
    assert!(joined.contains("ITEM OPS"), "{}", joined);
    for (label, _) in ITEM_OPS.iter() {
        assert!(joined.contains(label), "row {} missing: {}", label, joined);
    }
    // The dedupe row shows what the merge would free and drop before any key
    // is pressed: that number is the whole point of the preview.
    let items = box_of(&ed);
    let preview = itembox::dedupe_preview(&items);
    assert!(preview.freed > 0, "the real box holds duplicate ids");
    assert!(
        joined.contains(&format!("{} 枠解放 / {} 個消失", preview.freed, preview.lost)),
        "the preview must be on screen: {}",
        joined
    );
    // The menu keeps the save clean until a row is actually run.
    assert!(!ed.dirty, "opening the menu must not mark the save dirty");
    assert!(ed.take_write().is_none());
    cleanup(&dir);
}

#[test]
fn the_dedupe_row_shows_the_loss_and_needs_a_second_press() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_dedupe") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);
    open_menu(&mut ed);

    let before = box_of(&ed);
    let preview = itembox::dedupe_preview(&before);
    assert!(preview.freed > 0 && preview.lost > 0, "{:?}", preview);

    select(&mut ed, ITEM_OP_DEDUPE);
    ed.input(Key::A);
    assert_eq!(
        ed.item_armed,
        Some(ITEM_OP_DEDUPE),
        "the first press only arms the merge"
    );
    assert_eq!(box_of(&ed), before, "the first press must not touch the box");
    assert!(!ed.dirty);
    assert!(
        ed.message.contains(&format!("{} 枠解放", preview.freed))
            && ed.message.contains(&format!("{} 個消失", preview.lost)),
        "message was {}",
        ed.message
    );
    assert!(
        ed.message.contains("もう一度 [A]"),
        "the second press must be asked for: {}",
        ed.message
    );

    // Moving off the row drops the arm: a press must never land on a row the
    // user has stopped looking at.
    ed.input(Key::Down);
    assert_eq!(ed.item_armed, None, "moving off the row drops the arm");
    ed.input(Key::Up);
    assert_eq!(ed.item_menu_cursor, ITEM_OP_DEDUPE);
    assert_eq!(ed.item_armed, None);

    ed.input(Key::A);
    assert_eq!(ed.item_armed, Some(ITEM_OP_DEDUPE));
    ed.input(Key::A);
    assert_eq!(ed.item_armed, None, "the merge ran");

    let after = box_of(&ed);
    let lost = total(&before).saturating_sub(total(&after));
    assert_eq!(lost, preview.lost, "the merge drops what the preview said");
    assert_eq!(
        used(&before) - used(&after),
        preview.freed,
        "the merge frees what the preview said"
    );
    assert!(
        ed.message.contains(&format!("{}枠解放・{}個消失", preview.freed, lost)),
        "the result line must be N枠解放・M個消失: {}",
        ed.message
    );
    assert!(ed.dirty);

    // And the box it replaced is kept: the last row puts it back.
    select(&mut ed, ITEM_OP_UNDO);
    ed.input(Key::A);
    assert_eq!(box_of(&ed), before, "undo restores the untouched box");
    assert!(ed.message.contains("元に戻しました"), "{}", ed.message);
    cleanup(&dir);
}

#[test]
fn the_bulk_rows_fill_clamp_and_pack_the_box() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_bulk") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);
    open_menu(&mut ed);
    let start = box_of(&ed);

    // すべて 99 に: every used record reaches the cap, blanks are left alone.
    select(&mut ed, ITEM_OP_FILL);
    press_twice(&mut ed);
    let filled = box_of(&ed);
    assert!(filled
        .iter()
        .filter(|s| !s.is_blank())
        .all(|s| s.count == MAX_COUNT as u8));
    assert_eq!(
        filled.iter().filter(|s| !s.is_blank()).count(),
        used(&start),
        "filling changes counts, never the record set"
    );
    assert!(ed.message.contains("×99"), "{}", ed.message);

    // 上限を丸める: a stack above the cap comes back down, and only that one.
    let mut over = box_of(&ed);
    let targets: Vec<usize> = (0..over.len()).filter(|&i| !over[i].is_blank()).take(2).collect();
    assert_eq!(targets.len(), 2);
    over[targets[0]].count = 127;
    over[targets[1]].count = 120;
    ed.save
        .as_mut()
        .expect("a save")
        .write_item_box(ed.slot, &over)
        .expect("the box encodes");
    select(&mut ed, ITEM_OP_CLAMP);
    press_twice(&mut ed);
    let clamped = box_of(&ed);
    assert!(clamped
        .iter()
        .filter(|s| !s.is_blank())
        .all(|s| s.count as u16 <= MAX_COUNT));
    assert_eq!(clamped, over.iter().map(|s| ItemSlot {
        id: s.id,
        count: if s.count as u16 > MAX_COUNT { MAX_COUNT as u8 } else { s.count },
    }).collect::<Vec<_>>());
    assert!(ed.message.contains("99 を超えていた 2 枠"), "{}", ed.message);

    // 空きを詰める: no hole survives in front of the last used record.
    let mut holed = box_of(&ed);
    let holes: Vec<usize> = (10..500).filter(|&i| !holed[i].is_blank()).take(2).collect();
    assert_eq!(holes.len(), 2);
    for &h in &holes {
        holed[h] = ItemSlot::default();
    }
    ed.save
        .as_mut()
        .expect("a save")
        .write_item_box(ed.slot, &holed)
        .expect("the box encodes");
    let used_before: Vec<ItemSlot> = holed.iter().filter(|s| !s.is_blank()).copied().collect();
    select(&mut ed, ITEM_OP_COMPACT);
    press_twice(&mut ed);
    let packed = box_of(&ed);
    let used_after: Vec<ItemSlot> = packed.iter().filter(|s| !s.is_blank()).copied().collect();
    assert_eq!(used_after, used_before, "order and stacks are kept");
    assert!(
        packed[..used_after.len()].iter().all(|s| !s.is_blank()),
        "the used records are at the front"
    );
    assert!(ed.message.contains("空きを詰めました"), "{}", ed.message);

    // Every row left the save writable and the box the right shape.
    assert_eq!(packed.len(), TOTAL_ITEM_SLOTS);
    assert!(ed.dirty);
    cleanup(&dir);
}

#[test]
fn the_add_row_picks_the_item_then_the_count() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_add") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);
    open_menu(&mut ed);

    select(&mut ed, ITEM_OP_ADD);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);
    assert_eq!(ed.item_pick_mode, 3, "the picker is in its add mode");
    assert_eq!(ed.item_pick_stage, 0, "the item comes first");

    ed.apply_item_filter(String::from("回復薬"));
    let matches = ed.item_pick_matches();
    assert!(!matches.is_empty(), "the name table must find it");
    let want = matches[0];
    let before = box_of(&ed);
    let owned = count_of(&before, want);

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "nothing is written yet");
    assert_eq!(ed.item_pick_stage, 1);
    assert_eq!(ed.item_pick_add_id, want, "stage 2 is about that item");
    assert_eq!(ed.item_pick_add_count, 1, "one by default");
    assert_eq!(box_of(&ed), before, "choosing the item writes nothing");
    assert!(ed.message.contains("◀/▶"), "{}", ed.message);

    // The number keeps the pick lists' rule: [◀] / [▶] take the big step
    // (ten) and [▲] / [▼] the small one. It used to be the other way round,
    // which is what made the same key pair change meaning inside one screen.
    ed.input(Key::Right);
    assert_eq!(ed.item_pick_add_count, 11, "Right is the ten step");
    ed.input(Key::Up);
    assert_eq!(ed.item_pick_add_count, 12, "Up is the one step");
    ed.input(Key::Down);
    assert_eq!(ed.item_pick_add_count, 11);
    ed.input(Key::Left);
    assert_eq!(ed.item_pick_add_count, 1, "Left comes back down by ten");
    // Five of it: the one step, four times.
    for _ in 0..4 {
        ed.input(Key::Up);
    }
    assert_eq!(ed.item_pick_add_count, 5);
    // The cap is the game's own 99, so holding Right cannot pass it.
    for _ in 0..200 {
        ed.input(Key::Right);
    }
    assert_eq!(ed.item_pick_add_count, MAX_COUNT);
    for _ in 0..200 {
        ed.input(Key::Left);
    }
    assert_eq!(ed.item_pick_add_count, 1, "and it never reaches zero");
    for _ in 0..4 {
        ed.input(Key::Up);
    }

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Items, "the add returns to the box");
    assert!(ed.item_menu_open, "and to the menu it started from");
    let after = box_of(&ed);
    assert_eq!(
        count_of(&after, want),
        owned + 5,
        "the chosen number must land in the box"
    );
    assert!(
        ed.message.contains("5 個追加しました"),
        "message was {}",
        ed.message
    );
    assert!(ed.dirty);
    cleanup(&dir);
}

#[test]
fn the_minus_key_and_the_back_button_belong_to_the_menu() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_keys") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);

    // Minus is the menu's, and the folder rescan never fires on this screen.
    assert!(!ed.minus_rescans(), "the item screen must not rescan on Minus");

    open_menu(&mut ed);
    let cursor = ed.item_menu_cursor;
    ed.input(Key::Down);
    assert_eq!(ed.item_menu_cursor, (cursor + 1) % ITEM_OPS.len());
    // B closes the menu and stays on the item box; it does not leave the screen.
    ed.input(Key::B);
    assert!(!ed.item_menu_open, "B closes the menu");
    assert_eq!(ed.screen, Screen::Items);
    // With the menu closed B goes back to the slot screen, as it always did.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
    cleanup(&dir);
}

/// The save stays byte-identical when only the menu was looked at.
#[test]
fn looking_at_the_menu_writes_nothing() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_readonly") else {
        return;
    };
    let before = std::fs::read(&copy).unwrap();
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);
    open_menu(&mut ed);
    for _ in 0..(ITEM_OPS.len() * 2) {
        ed.input(Key::Down);
        ed.input(Key::Up);
    }
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Items);
    assert!(!ed.dirty);
    assert!(ed.take_write().is_none(), "no write was queued");
    assert_eq!(std::fs::read(&copy).unwrap(), before, "the file is untouched");
    // And the real save was never the one we opened.
    if let Some(src) = real_save_path() {
        assert_ne!(src, copy);
    }
    cleanup(&dir);
}

/// [X] over the bulk menu is the save key, not the [A] key.
///
/// The footer promised "[X] 保存" while the menu was open, and pressing it sent
/// [A] to the menu instead: the highlighted bulk row ran and no file was
/// written, so a user who saved and quit lost every edit. The menu is put away
/// first now and the ordinary write request goes out, with the box untouched.
#[test]
fn the_save_key_over_the_menu_saves_and_runs_no_bulk_row() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_save_key") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);
    open_menu(&mut ed);
    // Arm the one row that would be the most expensive mistake ("すべて 99 に").
    select(&mut ed, ITEM_OP_FILL);
    ed.input(Key::A);
    assert_eq!(ed.item_armed, Some(ITEM_OP_FILL));
    let armed = box_of(&ed);

    // This is the whole press the console layer does for [X].
    assert!(
        ed.item_screen_save(),
        "the save key must queue a write: {}",
        ed.message
    );
    assert!(!ed.item_menu_open, "the menu is put away by the save key");
    assert_eq!(ed.item_armed, None, "and the armed row is dropped");
    assert_eq!(box_of(&ed), armed, "no bulk row may run on the save key");
    assert!(!ed.dirty, "nothing was edited, so nothing is unsaved");
    let write = ed.take_write().expect("a save write was queued");
    let mhnx_core::hbedit::Write::File { path, bytes } = write;
    assert_eq!(path, copy.to_string_lossy(), "the open save is the target");
    assert_eq!(
        bytes,
        std::fs::read(&copy).unwrap(),
        "the bytes written are the bytes on the card"
    );
    cleanup(&dir);
}

/// A section / sub-tab jump cannot leave the menu open behind it.
#[test]
fn leaving_the_item_screen_puts_the_menu_away() {
    let Some((dir, copy)) = copy_of_the_real_save("menu_leaves") else {
        return;
    };
    let mut ed = Editor::new();
    open(&mut ed, &copy);
    open_items(&mut ed);
    open_menu(&mut ed);

    // The console calls this on every key; a screen change without it is what
    // used to bring the menu back open on the next visit.
    ed.screen = Screen::Hunts;
    ed.input(Key::Down);
    assert!(!ed.item_menu_open, "the menu belongs to the item box alone");

    ed.screen = Screen::Items;
    assert!(
        !ed.item_menu_open,
        "and coming back shows the list, not the menu"
    );
    assert_eq!(ed.item_armed, None);
    cleanup(&dir);
}
