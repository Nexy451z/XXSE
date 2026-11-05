//! What the 対象ID of a quest condition means, per 種類.
//!
//! A .mib keeps every target as two bytes: the 種類 (`tt1..tt10`) and the id
//! (`com0..com9`). The table here is measured on the 141 shipped quests, and
//! these tests pin both halves of it: the map itself, and the rule that a 種類
//! change never rewrites the id beside it.
//!
//! The last test reads the real dump when this machine has it and is skipped
//! (not failed) on a clean checkout, like the other real-data tests.

use std::path::PathBuf;

use mhnx_core::arc::Arc;
use mhnx_core::hbedit::{
    item_name_or_none, mib_field_index, mib_pick_kind, monster_name_of, quest_target_info,
    target_id_fits, target_id_kind, target_id_label, target_id_warning, target_row_title, Editor,
    Key, MibPickKind, QuestTargetId,
};
use mhnx_core::quest::MIB_SIZE;
use mhnx_core::questfields;

fn field(key: &str) -> &'static questfields::FieldDef {
    mhnx_core::hbedit::mib_field_index(key)
        .and_then(|i| questfields::MIB_FIELDS.get(i))
        .expect("a known .mib field")
}

fn put_u8(mib: &mut [u8], key: &str, value: u8) {
    let f = field(key);
    mib[f.offset] = value;
}

fn put_u16(mib: &mut [u8], key: &str, value: u16) {
    let f = field(key);
    mib[f.offset..f.offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(mib: &[u8], key: &str) -> u16 {
    let f = field(key);
    u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]])
}

/// A 249 byte .mib with the quest id and nothing else set.
fn empty_mib() -> Vec<u8> {
    let mut mib = vec![0u8; MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&1_010_165u32.to_le_bytes());
    mib
}

/// A delivery quest: 種類 納品 with an item id, and one hunt target beside it.
fn delivery_mib() -> Vec<u8> {
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 5); // 納品
    put_u16(&mut mib, "com0", 2111); // 肉食竜の卵
    put_u8(&mut mib, "tt2", 1); // 狩猟/討伐
    put_u16(&mut mib, "com1", 80); // ディノバルド
    put_u8(&mut mib, "tt3", 8); // 部位破壊
    put_u16(&mut mib, "com2", 80);
    mib
}

#[test]
fn a_delivery_target_is_an_item_and_a_hunt_target_is_a_monster() {
    let mib = delivery_mib();

    // 納品: the id is an item, and the row is headed 対象アイテム.
    let first = quest_target_info(&mib, "com0").expect("a 対象 row");
    assert_eq!(first.kind_key, "tt1");
    assert_eq!(first.kind_value, 5);
    assert_eq!(first.kind_label, "納品");
    assert_eq!(first.id_kind, QuestTargetId::Item);
    assert_eq!(first.id, 2111);
    assert_eq!(first.label, "肉食竜の卵 0x83F", "the item table names it");
    assert_eq!(
        first.row_title("ターゲット1 ID"),
        "対象アイテム（ターゲット1 ID）"
    );

    // 狩猟/討伐: a monster id in the same byte pair.
    let second = quest_target_info(&mib, "com1").expect("a 対象 row");
    assert_eq!(second.kind_label, "狩猟/討伐");
    assert_eq!(second.id_kind, QuestTargetId::Monster);
    assert_eq!(second.id, 80);
    assert_eq!(second.label, "ディノバルド 0x50");
    assert_eq!(
        second.row_title("ターゲット2 ID"),
        "対象モンスター（ターゲット2 ID）"
    );

    // 部位破壊 is a monster as well (42 rows of the dump).
    assert_eq!(
        quest_target_info(&mib, "com2").map(|i| i.id_kind),
        Some(QuestTargetId::Monster)
    );

    // The two tables really do disagree: the delivery id is not a monster id
    // and the hunt id is not an item id in any useful sense.
    assert!(target_id_fits(QuestTargetId::Item, 2111));
    assert!(!target_id_fits(QuestTargetId::Monster, 2111));
    assert!(target_id_fits(QuestTargetId::Monster, 80));
}

#[test]
fn the_kinds_that_carry_no_id_say_so() {
    // RRP納品 / ネコタク / 乗りダウン: the dump holds 2 + 2 + 16 rows of these
    // and every one of them stores id 0.
    for (value, label) in [(6u8, "RRP納品"), (7, "ネコタク"), (9, "乗りダウン")] {
        let mut mib = empty_mib();
        put_u8(&mut mib, "tt1", value);
        let info = quest_target_info(&mib, "com0").expect("a 対象 row");
        assert_eq!(info.kind_label, label);
        assert_eq!(info.id_kind, QuestTargetId::None, "{}", label);
        assert_eq!(info.label, "なし");
        assert_eq!(mib_pick_kind(&mib, mib_field_index("com0").unwrap()), MibPickKind::None);
        // Nothing to warn about: an id the kind does not use is not "wrong".
        assert_eq!(target_id_warning(&mib, "com0"), None);
    }
    assert!(target_row_title("ターゲット1 ID", QuestTargetId::None).contains("使いません"));

    // 大連続 is not "no target": the 25 shipped rows name their monsters in the
    // five mob_slot rows, so the id row says where the target really is.
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 3);
    let info = quest_target_info(&mib, "com0").expect("a 対象 row");
    assert_eq!(info.id_kind, QuestTargetId::MobSlots);
    assert!(info.label.contains("mob_slot"));
    let title = info.row_title("ターゲット1 ID");
    assert!(title.contains("出現モンスター（mob_slot）"), "{}", title);
    assert_eq!(mib_pick_kind(&mib, mib_field_index("com0").unwrap()), MibPickKind::None);
    assert_eq!(target_id_warning(&mib, "com0"), None);
}

#[test]
fn a_kind_change_warns_and_never_rewrites_the_id() {
    // 狩猟/討伐 4115 (ガブラス) -> 納品. The id is a monster; the item table has
    // no name for it, so the screen must say so and leave the number alone.
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 1);
    put_u16(&mut mib, "com0", 4115);

    let mut ed = Editor::new();
    ed.screen = mhnx_core::hbedit::Screen::Mib;
    ed.mib_bytes = mib.clone();
    ed.mib_cursor = mib_field_index("tt1").expect("tt1");

    // 1 -> 5 (納品) is four presses of [▶].
    for _ in 0..4 {
        ed.input(Key::Right);
    }
    let info = ed.mib_target_info(mib_field_index("com0").unwrap()).expect("a 対象 row");
    assert_eq!(info.kind_value, 5);
    assert_eq!(info.kind_label, "納品");
    assert_eq!(info.id_kind, QuestTargetId::Item);
    assert_eq!(info.id, 4115, "the id must not be rewritten by a 種類 change");
    assert_eq!(get_u16(ed.mib_bytes(), "com0"), 4115);
    assert!(ed.message.contains("アイテムの表にありません"), "{}", ed.message);
    assert!(ed.message.contains("そのまま"), "{}", ed.message);

    // The other direction: an item id left on a 狩猟/討伐 row.
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 5);
    put_u16(&mut mib, "com0", 2111);
    let mut ed = Editor::new();
    ed.screen = mhnx_core::hbedit::Screen::Mib;
    ed.mib_bytes = mib;
    ed.mib_cursor = mib_field_index("tt1").expect("tt1");
    for _ in 0..4 {
        ed.input(Key::Left); // 5 -> 1
    }
    let info = ed.mib_target_info(mib_field_index("com0").unwrap()).expect("a 対象 row");
    assert_eq!(info.kind_label, "狩猟/討伐");
    assert_eq!(info.id, 2111);
    assert!(ed.message.contains("モンスターの表にありません"), "{}", ed.message);
}

#[test]
fn the_placement_rows_read_the_syu2_table() {
    let mut mib = empty_mib();

    // 納品 (3) and 採取 (2) store item ids - the four rows the dump has.
    put_u8(&mut mib, "tt4", 3);
    put_u16(&mut mib, "com3", 2107); // 竜の卵
    put_u8(&mut mib, "tt5", 2);
    put_u16(&mut mib, "com4", 2111); // 肉食竜の卵
    for (id_key, kind_key, value) in [("com3", "tt4", 3u8), ("com4", "tt5", 2)] {
        let info = quest_target_info(&mib, id_key).expect("a 対象 row");
        assert_eq!(info.kind_key, kind_key);
        assert_eq!(info.kind_value, value);
        assert_eq!(info.id_kind, QuestTargetId::Item, "{}", id_key);
        assert!(info.label.contains("0x"), "{}", info.label);
    }
    assert_eq!(
        quest_target_info(&mib, "com3").map(|i| i.label),
        Some(String::from("竜の卵 0x83B"))
    );

    // 狩猟/討伐 (1) never appears in a 配置 row of the dump, and 配置4 = 5
    // (q1011010) is past the syu2_ table: neither is guessed.
    put_u8(&mut mib, "tt6", 1);
    put_u16(&mut mib, "com5", 80);
    put_u8(&mut mib, "tt7", 5);
    put_u16(&mut mib, "com6", 513);
    for id_key in ["com5", "com6"] {
        let info = quest_target_info(&mib, id_key).expect("a 対象 row");
        assert_eq!(info.id_kind, QuestTargetId::Unmeasured, "{}", id_key);
        assert!(info.label.starts_with("未確定"), "{}", info.label);
    }
}

#[test]
fn an_unmeasured_kind_is_reported_and_not_guessed() {
    // 討伐 (11) is a syu_ row the 141 shipped quests never use.
    assert_eq!(target_id_kind("tt1", 11), QuestTargetId::Unmeasured);
    assert_eq!(target_id_kind("tt1", 10), QuestTargetId::Unmeasured);
    assert_eq!(target_id_kind("tt1", 19), QuestTargetId::Unmeasured);
    // Past the end of the table entirely.
    assert_eq!(target_id_kind("tt1", 200), QuestTargetId::Unmeasured);
    // A key that is not a 対象 row at all.
    assert_eq!(target_id_kind("time_limit", 3), QuestTargetId::Unmeasured);

    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 11);
    put_u16(&mut mib, "com0", 80);
    let info = quest_target_info(&mib, "com0").expect("a 対象 row");
    assert_eq!(info.id_kind, QuestTargetId::Unmeasured);
    assert_eq!(info.label, "未確定 0x50");
    assert_eq!(
        mib_pick_kind(&mib, mib_field_index("com0").unwrap()),
        MibPickKind::None,
        "an unmeasured kind must not offer a table it cannot name"
    );
    assert!(target_row_title("ターゲット1 ID", QuestTargetId::Unmeasured).contains("未確定"));
    // No warning: nothing claims the id is wrong.
    assert_eq!(target_id_warning(&mib, "com0"), None);
}

#[test]
fn the_item_picker_writes_a_delivery_target_into_the_mib() {
    let mut ed = Editor::new();
    ed.screen = mhnx_core::hbedit::Screen::Mib;
    ed.mib_bytes = delivery_mib();
    put_u16(&mut ed.mib_bytes, "com0", 0);

    let idx = mib_field_index("com0").expect("com0");
    ed.mib_cursor = idx;
    // The route [A] takes on a 納品 row: an item list, not the monster table.
    assert_eq!(mib_pick_kind(&ed.mib_bytes, idx), MibPickKind::Item);
    assert!(ed.open_mib_item_pick(idx), "{}", ed.message);
    assert_eq!(ed.screen, mhnx_core::hbedit::Screen::ItemPick);
    assert_eq!(ed.item_pick_mode, 4);

    ed.apply_item_filter(String::from("肉食竜の卵"));
    let matches = ed.item_pick_matches();
    let pos = matches
        .iter()
        .position(|&id| id == 2111)
        .expect("the delivery item is in the picker");
    ed.item_pick_cursor = pos;
    ed.input(Key::A);

    assert_eq!(ed.screen, mhnx_core::hbedit::Screen::Mib, "{}", ed.message);
    assert_eq!(get_u16(ed.mib_bytes(), "com0"), 2111);
    assert!(ed.message.contains("肉食竜の卵"), "{}", ed.message);
    assert!(ed.dirty);

    // A monster-kind row is not routed here: it keeps the monster table.
    let mut ed = Editor::new();
    ed.mib_bytes = delivery_mib();
    let monster_idx = mib_field_index("com1").expect("com1");
    assert_eq!(mib_pick_kind(&ed.mib_bytes, monster_idx), MibPickKind::Monster);
    assert!(!ed.open_mib_item_pick(monster_idx));
}

#[test]
fn every_delivery_target_of_the_shipped_quests_is_a_named_item() {
    let dir = PathBuf::from(r"C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest");
    if !dir.exists() {
        eprintln!("skipping: {} not found", dir.display());
        return;
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the quest directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();

    let mut quests = 0usize;
    let mut item_rows = 0usize;
    let mut unnamed_items = 0usize;
    let mut monster_rows = 0usize;
    let mut unnamed_monsters = 0usize;
    let mut no_id_rows = 0usize;

    for path in &files {
        if path.extension().map(|e| e != "arc").unwrap_or(true) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(arc) = Arc::parse(&bytes) else { continue };
        let Some(mib) = arc.by_ext("mib") else { continue };
        if mib.data.len() != MIB_SIZE {
            continue;
        }
        quests += 1;
        for (_, id_key) in mhnx_core::hbedit::QUEST_TARGET_PAIRS {
            let Some(info) = quest_target_info(&mib.data, id_key) else { continue };
            // An empty slot is not a claim about the table.
            if info.id == 0 {
                continue;
            }
            match info.id_kind {
                QuestTargetId::Item => {
                    item_rows += 1;
                    if item_name_or_none(info.id).is_none() {
                        unnamed_items += 1;
                        eprintln!("{} {} = 0x{:X} has no item name", path.display(), id_key, info.id);
                    }
                }
                QuestTargetId::Monster => {
                    monster_rows += 1;
                    // 269 and 525 are real monster ids the shipped mint_ table
                    // does not list; the test allows for those two and no more.
                    if monster_name_of(info.id).is_none() {
                        unnamed_monsters += 1;
                    }
                }
                QuestTargetId::None => no_id_rows += 1,
                QuestTargetId::MobSlots => no_id_rows += 1,
                QuestTargetId::Unmeasured => no_id_rows += 1,
            }
        }
    }

    eprintln!(
        "{} quests: {} item-kind rows, {} monster-kind rows, {} no-id rows",
        quests, item_rows, monster_rows, no_id_rows
    );
    assert_eq!(quests, 141, "the dump holds 141 quests");
    // The delivery rows: 14 納品 rows on the three targets (tt1 6, tt3 8) and
    // 7 採取/納品 rows on the 配置 rows (tt4 3, tt5 1, tt6 2, tt7 1). Every one
    // of them is an item the item table names.
    assert_eq!(item_rows, 21, "the item-kind rows of the dump");
    assert_eq!(unnamed_items, 0, "a 納品 target that is not an item id");
    // 161 monster-kind rows: 狩猟/討伐 97 + 捕獲 1 + 討伐合計 3 + 部位破壊 42
    // (tt1) and 狩猟/討伐 9 + 討伐合計 3 + 狩猟/討伐 6 (tt2 / tt3).
    // 269 / 525 are the only two monster ids the mint_ table misses.
    assert_eq!(monster_rows, 161, "the monster-kind rows of the dump");
    assert!(unnamed_monsters <= 2, "{} monster ids had no name", unnamed_monsters);
    // The one row whose 種類 is past the syu2_ table: 配置4 = 5 in q1011010.
    // It is reported as 未確定 and never given a table.
    assert_eq!(no_id_rows, 1, "rows of an unmeasured / id-less kind that carry an id");

    // The two tables are separate: no item-kind id is named by the mint table
    // unless it is an id the item table also names (79 / 513 are both), so the
    // check above is the one that matters. The label function is the same one
    // the screens print.
    assert_eq!(
        target_id_label(QuestTargetId::Item, 2111),
        String::from("肉食竜の卵 0x83F")
    );
}
