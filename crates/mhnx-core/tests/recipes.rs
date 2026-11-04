//! The smithy's recipes.
//!
//! The recipe tables are game data, so these tests build the records they read
//! by hand first, and then - when this machine has a dump - check the real
//! tables against the numbers measured on one (see the module docs): 35 files,
//! 28,623 recipes, and every id landing inside the name table it belongs to.
//!
//! The synthetic cases run everywhere; the real-dump case skips, rather than
//! fails, when `MHNX_ROMFS_TABLE` (or the default path) is not there.

use std::path::PathBuf;

use mhnx_core::recipes::{
    kind_of, parse, Book, Material, Product, Provision, Recipe, TableKind, ARMOR_CREATE_RECORD,
    ARMOR_PROCESS_RECORD, DECO_CREATE_RECORD, FORGE, HEADER, MATERIALS, RETURNED, VERSION,
    WEAPON_CREATE_RECORD, WEAPON_PROCESS_RECORD,
};

/// Directory holding the game tables, overridable for another dump.
fn table_dir() -> PathBuf {
    match std::env::var_os("MHNX_ROMFS_TABLE") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(r"C:\anyWorkspace\nx\mhxx\romfs\nativeNX\table"),
    }
}

/// The 8 byte header every one of the five tables starts with.
fn header(count: usize) -> Vec<u8> {
    let mut out = VERSION.to_vec();
    out.extend_from_slice(&(count as u32).to_le_bytes());
    out
}

/// Build a table out of records that are already the right size.
fn table(records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = header(records.len());
    for record in records {
        out.extend_from_slice(record);
    }
    out
}

/// A `weaponCreateWNN` record, filled in where the reader looks.
fn weapon_create(
    id: u16,
    mats: [(u16, u8); MATERIALS],
    provision: Provision,
    scraps: [(u16, u8); RETURNED],
) -> Vec<u8> {
    let mut r = vec![0u8; WEAPON_CREATE_RECORD];
    r[0x00..0x02].copy_from_slice(&1u16.to_le_bytes()); // group, unread
    r[0x02..0x04].copy_from_slice(&id.to_le_bytes());
    for (slot, (item, count)) in mats.iter().enumerate() {
        let at = 0x06 + 4 * slot;
        r[at..at + 2].copy_from_slice(&item.to_le_bytes());
        r[at + 2] = *count;
        // The spare byte is a flag, not part of the count: setting it must not
        // change what the reader returns.
        r[at + 3] = 1;
    }
    r[0x16..0x18].copy_from_slice(&provision.group.to_le_bytes());
    r[0x18] = provision.value;
    for (slot, (item, count)) in scraps.iter().enumerate() {
        let at = 0x1A + 4 * slot;
        r[at..at + 2].copy_from_slice(&item.to_le_bytes());
        r[at + 2] = *count;
    }
    r
}

/// A `weaponProcessWNN` record.
fn weapon_process(
    id: u16,
    level: u16,
    mats: [(u16, u8); MATERIALS],
    provision: Provision,
) -> Vec<u8> {
    let mut r = vec![0u8; WEAPON_PROCESS_RECORD];
    r[0x00..0x02].copy_from_slice(&id.to_le_bytes());
    r[0x02..0x04].copy_from_slice(&level.to_le_bytes());
    for (slot, (item, count)) in mats.iter().enumerate() {
        let at = 0x04 + 3 * slot;
        r[at..at + 2].copy_from_slice(&item.to_le_bytes());
        r[at + 2] = *count;
    }
    r[0x10..0x12].copy_from_slice(&provision.group.to_le_bytes());
    r[0x12] = provision.value;
    r
}

/// An `armorCreateANN` record: up to four series sharing one recipe.
fn armor_create(series: [u16; MATERIALS], mats: [(u16, u8); MATERIALS]) -> Vec<u8> {
    let mut r = vec![0u8; ARMOR_CREATE_RECORD];
    for (slot, id) in series.iter().enumerate() {
        r[2 * slot..2 * slot + 2].copy_from_slice(&id.to_le_bytes());
    }
    for (slot, (item, count)) in mats.iter().enumerate() {
        let at = 0x0A + 4 * slot;
        r[at..at + 2].copy_from_slice(&item.to_le_bytes());
        r[at + 2] = *count;
    }
    r
}

/// An `armorProcessA00` record.
fn armor_process(series: [u16; MATERIALS], level: u16, mats: [(u16, u8); MATERIALS]) -> Vec<u8> {
    let mut r = vec![0u8; ARMOR_PROCESS_RECORD];
    for (slot, id) in series.iter().enumerate() {
        r[2 * slot..2 * slot + 2].copy_from_slice(&id.to_le_bytes());
    }
    r[0x08..0x0A].copy_from_slice(&level.to_le_bytes());
    for (slot, (item, count)) in mats.iter().enumerate() {
        let at = 0x0A + 3 * slot;
        r[at..at + 2].copy_from_slice(&item.to_le_bytes());
        r[at + 2] = *count;
    }
    r
}

/// A `decoCreate` record.
fn deco_create(item: u16, mats: [(u16, u8); MATERIALS]) -> Vec<u8> {
    let mut r = vec![0u8; DECO_CREATE_RECORD];
    r[0x00..0x02].copy_from_slice(&item.to_le_bytes());
    for (slot, (entry, count)) in mats.iter().enumerate() {
        let at = 0x04 + 3 * slot;
        r[at..at + 2].copy_from_slice(&entry.to_le_bytes());
        r[at + 2] = *count;
    }
    r
}

/// The four material slots of a recipe, as (item, count) pairs.
fn mats(recipe: &Recipe) -> Vec<(u16, u8)> {
    recipe
        .material_list()
        .iter()
        .map(|m| (m.item, m.count))
        .collect()
}

#[test]
fn a_weapon_create_record_becomes_a_forge_recipe() {
    let bytes = table(&[
        weapon_create(
            1,
            [(318, 4), (319, 1), (0, 0), (0, 0)],
            Provision::NONE,
            [(2158, 2), (0, 0), (0, 0)],
        ),
        weapon_create(
            45,
            [(975, 6), (407, 3), (571, 2), (391, 2)],
            Provision::NONE,
            [(0, 0); RETURNED],
        ),
    ]);
    let recipes = parse(TableKind::WeaponCreate(0), &bytes);
    assert_eq!(recipes.len(), 2);
    assert_eq!(recipes[0].product, Product::Weapon { class: 0, id: 1 });
    assert_eq!(recipes[0].level, FORGE);
    assert!(recipes[0].is_forge());
    assert_eq!(mats(&recipes[0]), vec![(318, 4), (319, 1)]);
    // The spare byte of each create entry is a flag: the count is 4, not 260.
    assert_eq!(recipes[0].materials[0].count, 4);
    assert_eq!(
        recipes[0].returned_list(),
        vec![Material {
            item: 2158,
            count: 2
        }]
    );
    assert!(!recipes[0].provision.is_used());
    assert_eq!(recipes[1].product, Product::Weapon { class: 0, id: 45 });
    assert_eq!(
        mats(&recipes[1]),
        vec![(975, 6), (407, 3), (571, 2), (391, 2)]
    );
}

#[test]
fn a_weapon_process_record_carries_its_level_and_provision() {
    let bytes = table(&[
        weapon_process(1, 1, [(0, 0); MATERIALS], Provision::NONE),
        weapon_process(
            1,
            2,
            [(407, 2), (571, 1), (0, 0), (0, 0)],
            Provision { group: 4, value: 4 },
        ),
    ]);
    let recipes = parse(TableKind::WeaponProcess(0), &bytes);
    assert_eq!(recipes.len(), 2);
    // Level 1 of a weapon is the link from its parent and can cost nothing at
    // all: 807 of the real dump's process rows list no material.
    assert_eq!(recipes[0].level, 1);
    assert!(recipes[0].material_list().is_empty());
    assert!(!recipes[0].is_forge());
    assert_eq!(recipes[1].level, 2);
    assert_eq!(mats(&recipes[1]), vec![(407, 2), (571, 1)]);
    assert_eq!(recipes[1].provision, Provision { group: 4, value: 4 });
    assert!(recipes[1].provision.is_used());
}

#[test]
fn one_armour_record_names_every_series_that_shares_its_recipe() {
    let create = table(&[
        armor_create([64, 65, 0, 0], [(380, 1), (807, 3), (365, 20), (0, 0)]),
        armor_create([0, 0, 0, 0], [(318, 1), (0, 0), (0, 0), (0, 0)]),
    ]);
    let recipes = parse(TableKind::ArmorCreate(1), &create);
    // Two series in one record, and the blank record produces nothing at all.
    assert_eq!(recipes.len(), 2);
    assert_eq!(
        recipes[0].product,
        Product::Armor {
            series: 64,
            part: 1
        }
    );
    assert_eq!(
        recipes[1].product,
        Product::Armor {
            series: 65,
            part: 1
        }
    );
    // Both pieces cost the same, which is why one record names both.
    assert_eq!(mats(&recipes[0]), mats(&recipes[1]));
    assert_eq!(mats(&recipes[0]), vec![(380, 1), (807, 3), (365, 20)]);
    assert!(parse(TableKind::ArmorCreate(1), &create)
        .iter()
        .all(|r| r.level == FORGE));

    let process = table(&[armor_process(
        [64, 65, 0, 0],
        2,
        [(319, 2), (0, 0), (0, 0), (0, 0)],
    )]);
    let recipes = parse(TableKind::ArmorProcess, &process);
    assert_eq!(recipes.len(), 2);
    assert_eq!(recipes[0].product, Product::ArmorSeries { series: 64 });
    assert_eq!(recipes[0].level, 2);
    assert_eq!(mats(&recipes[0]), vec![(319, 2)]);
    // An upgrade names the series, not a part: it applies to all five.
    assert_eq!(recipes[0].product.equip_kind(), 1);
}

#[test]
fn a_decoration_record_has_three_short_material_slots_and_no_scrap() {
    let bytes = table(&[deco_create(2638, [(339, 1), (616, 2), (0, 0), (0, 0)])]);
    let recipes = parse(TableKind::DecoCreate, &bytes);
    assert_eq!(recipes.len(), 1);
    assert_eq!(recipes[0].product, Product::Deco { item: 2638 });
    assert_eq!(mats(&recipes[0]), vec![(339, 1), (616, 2)]);
    assert!(!recipes[0].product.is_equipment());
    // The 16 byte record has no tail: a decoration never gives scrap back.
    assert!(recipes[0].returned_list().is_empty());
}

#[test]
fn a_file_that_is_not_the_table_is_refused_rather_than_half_read() {
    let mut good = table(&[weapon_create(
        1,
        [(318, 1), (0, 0), (0, 0), (0, 0)],
        Provision::NONE,
        [(0, 0); RETURNED],
    )]);
    assert_eq!(parse(TableKind::WeaponCreate(0), &good).len(), 1);

    // One byte short: the count no longer fills the file, so it is refused.
    good.truncate(good.len() - 1);
    assert!(parse(TableKind::WeaponCreate(0), &good).is_empty());

    // The right length, the wrong version word.
    let mut wrong = table(&[deco_create(2638, [(339, 1), (0, 0), (0, 0), (0, 0)])]);
    wrong[0] = 0xFF;
    assert!(parse(TableKind::DecoCreate, &wrong).is_empty());

    // Nothing at all, and a header with no records behind it.
    assert!(parse(TableKind::DecoCreate, &[]).is_empty());
    assert!(parse(TableKind::DecoCreate, &header(0)).is_empty());
    // A count that would overflow the length calculation.
    let mut huge = VERSION.to_vec();
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(TableKind::ArmorProcess, &huge).is_empty());
    assert_eq!(huge.len(), HEADER);
}

#[test]
fn file_names_are_recognised_by_stem_and_extension() {
    assert_eq!(
        kind_of("weaponCreateW00.wcd"),
        Some(TableKind::WeaponCreate(0))
    );
    assert_eq!(
        kind_of("WEAPONCREATEW14.WCD"),
        Some(TableKind::WeaponCreate(14))
    );
    assert_eq!(
        kind_of("weaponProcessW07.wpd"),
        Some(TableKind::WeaponProcess(7))
    );
    // The file names the part zero based; the recipe names it one based.
    assert_eq!(
        kind_of("armorCreateA00.acrd"),
        Some(TableKind::ArmorCreate(1))
    );
    assert_eq!(
        kind_of("armorCreateA04.acrd"),
        Some(TableKind::ArmorCreate(5))
    );
    assert_eq!(
        kind_of("armorProcessA00.apd"),
        Some(TableKind::ArmorProcess)
    );
    assert_eq!(kind_of("decoCreate.dcd"), Some(TableKind::DecoCreate));
    // A folder on the front does not change the answer.
    assert_eq!(
        kind_of("dump/nativeNX/table/decoCreate.dcd"),
        Some(TableKind::DecoCreate)
    );
    // Everything else in the folder is not a recipe table.
    assert_eq!(kind_of("itemData.itm"), None);
    assert_eq!(kind_of("armorCreateA05.acrd"), None);
    assert_eq!(kind_of("weaponCreateW5.wcd"), None);
    assert_eq!(kind_of("weaponCreateW00.wpd"), None);
    assert_eq!(kind_of("noextension"), None);
}

#[test]
fn the_book_answers_both_directions() {
    let mut book = Book::new();
    let create = table(&[weapon_create(
        1,
        [(318, 4), (319, 1), (0, 0), (0, 0)],
        Provision::NONE,
        [(0, 0); RETURNED],
    )]);
    assert_eq!(book.add("weaponCreateW00.wcd", &create), 1);
    let process = table(&[
        weapon_process(1, 2, [(318, 2), (0, 0), (0, 0), (0, 0)], Provision::NONE),
        weapon_process(2, 2, [(407, 1), (0, 0), (0, 0), (0, 0)], Provision::NONE),
    ]);
    assert_eq!(book.add("weaponProcessW00.wpd", &process), 2);
    // A file of another kind is not read, and is not counted as skipped
    // either: the folder is full of them.
    assert_eq!(book.add("itemData.itm", &create), 0);
    assert_eq!(book.skipped(), 0);
    assert_eq!(book.files(), 2);
    // A table by the right name with the wrong bytes *is* counted as skipped.
    assert_eq!(book.add("decoCreate.dcd", b"GMD\0"), 0);
    assert_eq!(book.skipped(), 1);

    assert_eq!(book.len(), 3);
    let forge = book.of_product(Product::Weapon { class: 0, id: 1 });
    // Forge first, then the upgrade, whatever order the files were read in.
    assert_eq!(forge.len(), 2);
    assert!(forge[0].is_forge());
    assert_eq!(forge[1].level, 2);

    // 318 is asked for by the forge of weapon 1 and by its level 2, 319 by the
    // forge alone, and 407 by a different weapon.
    assert_eq!(book.of_material(318).len(), 2);
    assert_eq!(book.of_material(319).len(), 1);
    assert_eq!(book.of_material(407).len(), 1);
    assert!(book.of_material(999).is_empty());
    assert_eq!(book.materials().len(), 3);
    assert_eq!(book.products().len(), 2);
    assert!(book.summary().contains("3 レシピ"));
}

#[test]
fn the_real_dump_holds_every_recipe_it_was_measured_to_hold() {
    let dir = table_dir();
    if !dir.exists() {
        eprintln!("skipping: {} not found", dir.display());
        return;
    }
    let mut book = Book::new();
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {}", dir.display(), e))
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if kind_of(&name).is_none() {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .unwrap_or_else(|e| panic!("{}: {}", entry.path().display(), e));
        book.add(&name, &bytes);
        names.push(name);
    }
    names.sort();
    // The five families: 14 weapon classes, 5 armour parts, the two process
    // tables and the decoration table.
    assert_eq!(book.files(), 35, "files: {:?}", names);
    assert_eq!(book.skipped(), 0, "files: {:?}", names);
    assert_eq!(book.len(), 28_623);
    assert_eq!(book.products().len(), 8_907);
    assert_eq!(book.materials().len(), 1_821);

    // Forging is finished at level 0: 1,152 weapons, 5,787 armour pieces and
    // 275 decorations, and nothing else carries that level.
    let forges = book.recipes().iter().filter(|r| r.is_forge()).count();
    assert_eq!(forges, 7_214);
    assert_eq!(
        book.recipes().iter().map(|r| r.level).max(),
        Some(15),
        "no recipe in the dump goes past level 15"
    );

    // Every id a recipe names lands inside the table it belongs to. This is the
    // check that ties the recipes to the name tables: an off-by-one in an
    // offset would put a material or a series outside its list.
    let last_item = mhnx_core::gamedata::LAST_ITEM_ID as u16;
    let series_limit = (mhnx_core::equipnames::ARMOR_NAMES.len() / 5) as u16;
    let mut weapons = 0usize;
    for recipe in book.recipes() {
        for material in recipe.material_list() {
            assert!(
                material.item <= last_item,
                "material {} is not an item",
                material.item
            );
            assert!(
                material.count > 0,
                "a material of 0 of item {}",
                material.item
            );
        }
        // The tail of a smithy recipe is scrap: 344 ids, all of them 端材.
        for scrap in recipe.returned_list() {
            assert!(
                (2_158..=2_501).contains(&scrap.item),
                "scrap {} is outside the 端材 range",
                scrap.item
            );
        }
        match recipe.product {
            Product::Weapon { class, id } => {
                weapons += 1;
                let table = mhnx_core::equipnames::WEAPON_NAMES
                    .get(class as usize)
                    .unwrap_or_else(|| panic!("no weapon class {}", class));
                assert!(
                    (id as usize) < table.len(),
                    "weapon {} of class {} has no name",
                    id,
                    class
                );
            }
            Product::Armor { series, part } => {
                assert!((1..=5).contains(&part));
                assert!(
                    series < series_limit,
                    "armour series {} has no name",
                    series
                );
            }
            Product::ArmorSeries { series } => {
                assert!(
                    series < series_limit,
                    "armour series {} has no name",
                    series
                )
            }
            Product::Deco { item } => assert!(item <= last_item),
        }
    }
    assert_eq!(weapons, 12_063);

    // The reverse index really is the reverse: a sample of the recipes is
    // found again from every one of its materials.
    for recipe in book.recipes().iter().step_by(79) {
        for material in recipe.material_list() {
            let back = book.of_material(material.item);
            assert!(
                back.iter()
                    .any(|other| other.product == recipe.product && other.level == recipe.level),
                "{} is used by {:?} but the reverse list does not say so",
                material.item,
                recipe.product
            );
        }
    }

    // A known chain, from the real dump: the armour upgrades of a series come
    // back in level order and the forge of one part is a forge.
    let series = Product::ArmorSeries { series: 1 };
    let upgrades = book.of_product(series);
    assert!(!upgrades.is_empty());
    let levels: Vec<u16> = upgrades.iter().map(|r| r.level).collect();
    let mut sorted = levels.clone();
    sorted.sort_unstable();
    assert_eq!(levels, sorted);
    let head = book.of_product(Product::Armor { series: 1, part: 1 });
    assert!(
        head.iter().any(|r| r.is_forge()),
        "every series can be forged"
    );
}
