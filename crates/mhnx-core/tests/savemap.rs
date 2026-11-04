//! The save map: the named rows, the dump and the values.
//!
//! The tests that need the sample save on this machine are skipped, not
//! failed, when it is absent - the same convention tests/real_data.rs uses.

use std::collections::BTreeSet;

use mhnx_core::guildcard;
use mhnx_core::palico;
use mhnx_core::save::{self, Save, BODY_SIZE, FIRST_SLOT_BASE, SLOT_STRIDE};
use mhnx_core::savemap::{self, FieldKind, DUMP_BYTES, DUMP_COLUMNS};

/// The fixed copy of the sample save, when this machine has one.
fn sample() -> Option<Save> {
    let path = std::path::PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if !path.is_file() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    let bytes = std::fs::read(&path).expect("the sample save");
    Some(Save::from_bytes(&bytes).expect("the sample save parses"))
}

/// The row named `name` in `slot`.
fn row(name: &str, slot: Option<usize>) -> usize {
    savemap::fields()
        .iter()
        .position(|f| f.name == name && f.slot == slot)
        .unwrap_or_else(|| panic!("no row named {} for slot {:?}", name, slot))
}

/// The first character slot of the MHXX layout.
fn first_slot() -> usize {
    FIRST_SLOT_BASE as usize
}

/// The map has to cover every region the save has, and every row has to land
/// inside the body with a length its kind agrees with.
#[test]
fn the_map_covers_every_region_of_the_save() {
    let fields = savemap::fields();
    assert!(
        fields.len() >= 80,
        "the map names only {} rows; the reference has 858 and this has to be a map, not a sample",
        fields.len()
    );

    let regions: Vec<&'static str> = (0..fields.len()).map(savemap::region).collect();
    for want in ["本体", "DLC セクション", "スロット1", "スロット2", "スロット3"] {
        assert!(regions.contains(&want), "the map has no {} row", want);
    }
    // The three slots are the same table three times over.
    let slot_rows = regions.iter().filter(|r| r.starts_with("スロット")).count();
    assert_eq!(
        slot_rows % save::MAX_SLOTS,
        0,
        "{} slot rows do not divide by {}",
        slot_rows,
        save::MAX_SLOTS
    );

    let mut seen = BTreeSet::new();
    for field in fields {
        assert!(!field.name.is_empty());
        assert!(field.len > 0, "{} has no length", field.name);
        assert!(
            field.offset + field.len <= BODY_SIZE,
            "{} runs past the end of the body: 0x{:X} + {}",
            field.name,
            field.offset,
            field.len
        );
        if let Some(slot) = field.slot {
            assert!(slot < save::MAX_SLOTS, "{} names slot {}", field.name, slot);
            let base = first_slot() + slot * SLOT_STRIDE;
            assert!(field.offset >= base, "{} starts before its slot", field.name);
            assert!(
                field.offset - base + field.len <= SLOT_STRIDE,
                "{} crosses the end of its slot",
                field.name
            );
        }
        match field.kind {
            FieldKind::U16 => assert_eq!(field.len, 2, "{}", field.name),
            FieldKind::U32 => assert_eq!(field.len, 4, "{}", field.name),
            FieldKind::Record { size, count } => {
                assert_eq!(size * count, field.len, "{}", field.name);
            }
            FieldKind::Byte | FieldKind::BitMap | FieldKind::Text => {}
        }
        assert!(
            seen.insert((field.slot, field.name.clone())),
            "{} is listed twice for slot {:?}",
            field.name,
            field.slot
        );
    }
    // The table is built once: the same call hands out the same rows.
    assert!(core::ptr::eq(savemap::fields().as_ptr(), fields.as_ptr()));
}

/// A row has to be the constant's own offset, or the map would describe bytes
/// that nothing in the crate writes.
#[test]
fn each_row_lands_where_its_constant_says() {
    let first = first_slot();
    assert_eq!(
        savemap::fields()[row("所持金", Some(0))].offset,
        first + save::OFF_FUNDS
    );
    assert_eq!(
        savemap::fields()[row("装備ボックス", Some(2))].offset,
        first + 2 * SLOT_STRIDE + save::OFF_EQUIPMENT_BOX
    );
    assert_eq!(
        savemap::fields()[row("オトモ", Some(1))].len,
        palico::SLOTS * palico::RECORD
    );
    // The download section's base is the one the save carries (0xB280 on
    // MHXX), so its rows are the section offsets behind it.
    assert_eq!(
        savemap::fields()[row("DLC 導入済みのビット", None)].offset,
        0xB280 + mhnx_core::dlc::BITMASK_OFFSET
    );
    assert_eq!(
        savemap::fields()[row("DLC クエスト（205 枠）", None)].offset,
        0xB280 + mhnx_core::dlc::QUEST_ARRAY_OFFSET
    );
}

/// The dump is 16 bytes a line with the body offset in front, and a long row
/// says how much it left out.
#[test]
fn the_dump_is_addressed_and_capped() {
    let body = vec![0x5Au8; BODY_SIZE];
    let fields = savemap::fields();
    let eight = ["5a"; 8].join(" ");
    let sixteen = format!("{}  {}", eight, eight);

    // One byte: one line, one byte on it.
    let lines = savemap::dump(&body, row("振動", None)).expect("a dump");
    assert_eq!(lines, vec![format!("{:06X}  5a", save::OFF_RUMBLE)]);

    // 32 bytes: two full lines, the second addressed 16 further on.
    let lines = savemap::dump(&body, row("名前", Some(0))).expect("a dump");
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        format!("{:06X}  {}", first_slot() + save::OFF_NAME, sixteen)
    );
    assert_eq!(
        lines[1],
        format!("{:06X}  {}", first_slot() + save::OFF_NAME + 16, sixteen)
    );

    // A row longer than the cap stops there and counts the rest.
    let equipment = row("装備ボックス", Some(0));
    let lines = savemap::dump(&body, equipment).expect("a dump");
    assert_eq!(lines.len(), DUMP_BYTES / DUMP_COLUMNS + 1);
    let last = lines.last().expect("a line");
    assert!(last.starts_with('…'), "{}", last);
    assert!(
        last.contains(&(fields[equipment].len - DUMP_BYTES).to_string()),
        "{}",
        last
    );

    // Past the table, or a body too short for the row: None, not a panic.
    assert!(savemap::dump(&body, fields.len()).is_none());
    assert!(savemap::dump(&body[..16], equipment).is_none());
}

/// Every row of the sample save has to read, and the slot bases the map
/// computes have to be the ones the file carries.
#[test]
fn the_sample_save_fills_the_whole_map() {
    let Some(save) = sample() else {
        return;
    };
    let fields = savemap::fields();
    let mut readable = 0usize;
    for (index, field) in fields.iter().enumerate() {
        let value = savemap::value_text(&save, index)
            .unwrap_or_else(|| panic!("{} (slot {:?}) has no value", field.name, field.slot));
        assert!(!value.is_empty(), "{} is empty", field.name);
        assert!(
            savemap::dump_for(&save, index).is_some(),
            "{} has no dump",
            field.name
        );
        // MHXX stores exactly the bases the table is built on, so the row is
        // at the offset the file itself points at.
        assert_eq!(
            savemap::offset_in(&save, index),
            Some(field.offset),
            "{} is not where the save says",
            field.name
        );
        readable += 1;
    }
    assert!(
        readable >= 80,
        "only {} rows read out of the sample save",
        readable
    );
    eprintln!(
        "save map: {} rows, {} read out of the sample save",
        fields.len(),
        readable
    );
}

/// Where a getter can read the same bytes, the map has to say the same thing.
#[test]
fn the_values_agree_with_the_accessors() {
    let Some(save) = sample() else {
        return;
    };
    let character = save.read_character(0).expect("slot 0");
    let value = |name: &str, slot: Option<usize>| {
        savemap::value_text(&save, row(name, slot))
            .unwrap_or_else(|| panic!("{} has no value", name))
    };
    assert_eq!(value("名前", Some(0)), character.name);
    assert_eq!(
        value("名前（セーブ画面の写し）", Some(0)),
        character.name_preview
    );
    assert!(value("所持金", Some(0)).starts_with(&character.funds.to_string()));
    assert!(value("ハンターランク", Some(0)).starts_with(&character.hunter_rank.to_string()));
    assert!(value("プレイ時間（秒）", Some(0)).starts_with(&character.play_time.to_string()));
    assert!(value("村ポイント（ベルナ）", Some(0))
        .starts_with(&character.village_points[0].to_string()));
    // The card keeps its own copy of the rank.
    let hr = guildcard::hunter_rank(&save, 0).expect("the card");
    assert!(value("ギルドカード（HR）", Some(0)).starts_with(&hr.to_string()));
    // A row without a single value still says what it holds.
    assert_eq!(
        value("装備ボックス", Some(0)),
        format!(
            "{} バイト × {} 件",
            save::EQUIPMENT_RECORD,
            save::TOTAL_EQUIP_SLOTS
        )
    );
}

/// A text row that is not readable text shows its bytes rather than a wrong
/// string, and a slot the save does not have has no value at all.
#[test]
fn bytes_are_shown_when_nothing_can_decode_them() {
    let Some(mut save) = sample() else {
        return;
    };
    let base = save.slot_base(0).expect("slot 0") as usize;
    save.body_mut()[base + save::OFF_NAME] = 0xFF;
    let value = savemap::value_text(&save, row("名前", Some(0))).expect("a value");
    assert!(value.starts_with("FF "), "{}", value);

    // With no slot base the row is not in this save.
    let mut empty = mhnx_core::save::empty_switch_save();
    empty.body_mut()[0x10..0x14].fill(0);
    assert!(savemap::offset_in(&empty, row("名前", Some(0))).is_none());
    assert!(savemap::value_text(&empty, row("名前", Some(0))).is_none());
    assert!(savemap::dump_for(&empty, row("名前", Some(0))).is_none());
    // The body-wide rows still read.
    assert!(savemap::value_text(&empty, row("振動", None)).is_some());
    // A download section pointer of zero still reads as offset zero.
    assert!(savemap::value_text(&empty, row("DLC 導入済みのビット", None)).is_some());
}
