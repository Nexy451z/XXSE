//! The game's tables, read against the file this machine's dump produced.
//!
//! Every expectation is a fact about the shipped Japanese MHXX build: the
//! record counts, the exact division of each file's size by its stride, and the
//! range the rarity byte can take. The tables are skipped when the dump is not
//! present, the same way the other on-device tests are.

use std::path::PathBuf;

use mhnx_core::gametables::{self, ArmorTable, ItemTable, Tables, WeaponTable};

fn table(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(r"C:\anyWorkspace\nx\mhxx\extract_resident\table").join(name);
    std::fs::read(p).ok()
}

#[test]
fn the_item_table_is_two_thousand_nine_hundred_and_ninety_one_records() {
    let Some(bytes) = table("itemData.bin") else {
        eprintln!("skipping: no itemData.bin");
        return;
    };
    let t = ItemTable::parse(&bytes).expect("the shipped itemData parses");
    assert_eq!(t.len(), 2991, "one record per item id");
    // 8 + 2991 * 44 is exactly the file's size, which is how the stride was
    // measured rather than guessed.
    assert_eq!(8 + 2991 * 44, bytes.len());
}

#[test]
fn every_item_has_a_rarity_in_the_range_the_palette_covers() {
    let Some(bytes) = table("itemData.bin") else {
        eprintln!("skipping: no itemData.bin");
        return;
    };
    let t = ItemTable::parse(&bytes).expect("parse");
    for id in 0..t.len() as u16 {
        let look = t.look(id).expect("every id in range");
        assert!(
            (1..=11).contains(&look.rarity),
            "item {} has rarity {}, past the eleven colours",
            id,
            look.rarity
        );
        assert!(look.color < 17, "item {} colour {} is past the palette", id, look.color);
        assert!(look.icon <= 105, "item {} icon {} is past the sheet's list", id, look.icon);
    }
}

#[test]
fn a_known_item_lands_on_a_known_cell() {
    let Some(bytes) = table("itemData.bin") else {
        eprintln!("skipping: no itemData.bin");
        return;
    };
    let t = ItemTable::parse(&bytes).expect("parse");
    // Potion is item 1 in every MHXX table and sits on the first row of the
    // sheet; the point of the test is that the row is read at all, not which
    // cell the game chose.
    let potion = t.look(1).expect("item 1");
    assert!(potion.icon < 95, "the common items are on the plain grid");
    assert!((1..=11).contains(&potion.rarity));
}

#[test]
fn the_armour_table_is_twelve_hundred_and_eighty_seven_series() {
    let Some(bytes) = table("armorSeriesData.bin") else {
        eprintln!("skipping: no armorSeriesData.bin");
        return;
    };
    let t = ArmorTable::parse(&bytes).expect("parse");
    assert_eq!(t.len(), 1287);
    assert_eq!(8 + 1287 * 127, bytes.len());
}

#[test]
fn a_series_that_has_no_such_part_gives_no_rarity() {
    let Some(bytes) = table("armorSeriesData.bin") else {
        eprintln!("skipping: no armorSeriesData.bin");
        return;
    };
    let t = ArmorTable::parse(&bytes).expect("parse");
    // Every series has at least one part; the ones that do not carry a given
    // part must answer None rather than a number for a piece that does not exist.
    let mut present = 0;
    let mut absent = 0;
    for kind in 1u8..=5 {
        for id in 0..t.len() as u16 {
            match t.rarity_for(&bytes, kind, id) {
                Some(r) => {
                    assert!((1..=11).contains(&r));
                    present += 1;
                }
                None => absent += 1,
            }
        }
    }
    assert!(present > 0, "some series have parts");
    assert!(absent > 0, "some series are missing parts");
}

#[test]
fn the_weapon_tables_read_their_own_stride() {
    let Some(bytes) = table("weapon00BaseData.bin") else {
        eprintln!("skipping: no weapon00BaseData.bin");
        return;
    };
    let t = WeaponTable::parse(&bytes).expect("parse");
    assert_eq!(t.len(), 137);
    for id in 0..t.len() as u16 {
        let r = t.rarity(id).expect("every id in range");
        assert!((1..=11).contains(&r), "weapon {} rarity {}", id, r);
    }
    // A fourth class ships with a different record size, so the stride really
    // does have to come from the header.
    if let Some(other) = table("weapon14BaseData.bin") {
        let t14 = WeaponTable::parse(&other).expect("parse 14");
        assert_eq!(t14.len(), 81);
    }
}

#[test]
fn a_file_that_is_not_a_table_is_refused_rather_than_read_as_one() {
    assert!(ItemTable::parse(&[]).is_none());
    assert!(ItemTable::parse(&[0u8; 8]).is_none());
    // A count that does not divide the body is the wrong record size.
    let mut bad = vec![0u8; 8 + 100];
    bad[4..8].copy_from_slice(&3u32.to_le_bytes());
    assert!(ItemTable::parse(&bad).is_none());
}

#[test]
fn the_box_type_to_weapon_class_mapping_skips_the_class_that_has_no_table() {
    assert_eq!(gametables::weapon_class(7), Some(0));
    assert_eq!(gametables::weapon_class(12), Some(5), "class 5 is the empty one");
    assert_eq!(gametables::weapon_class(21), Some(14));
    assert_eq!(gametables::weapon_class(6), None, "a talisman is not a weapon");
    assert_eq!(gametables::weapon_class(1), None, "armour is not a weapon");
}

#[test]
fn the_whole_set_answers_for_the_kinds_it_has_tables_for() {
    let mut tables = Tables::default();
    if let Some(b) = table("armorSeriesData.bin") {
        tables.armor = ArmorTable::parse(&b).expect("armour");
        tables.armor_bytes = b;
    }
    if let Some(b) = table("weapon00BaseData.bin") {
        tables.weapons[0] = WeaponTable::parse(&b);
    }
    if let Some(b) = table("itemData.bin") {
        tables.items = ItemTable::parse(&b).expect("items");
    }
    assert!(!tables.is_empty());
    // A great sword gets its rarity from the class table.
    assert!(tables.equip_rarity(7, 0).is_some());
    // A head gets its from the armour series.
    assert!(tables.equip_rarity(1, 0).is_some());
    // A talisman has no table: its rarity is in the save.
    assert_eq!(tables.equip_rarity(6, 353), None);
}
