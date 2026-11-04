//! The game's table list.
//!
//! The tables themselves are game data, so none of them are in this repository:
//! these tests read them out of the machine's own MHXX dump when it is present
//! and skip, rather than fail, when it is not. The measured cases pin the entry
//! counts of the retail Japanese dump every layout in `database` was taken from;
//! the synthetic cases pin the reader itself and run everywhere.

use std::path::PathBuf;

use mhnx_core::database::{
    can_read, filter, normalise, read, spec_for, summary, Layout, Table, ATD_EXT, PREVIEW, TABLES,
};

/// Directory holding the game tables, overridable for another dump.
fn table_dir() -> PathBuf {
    match std::env::var_os("MHNX_ROMFS_TABLE") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(r"C:\anyWorkspace\nx\mhxx\romfs\nativeNX\table"),
    }
}

/// Read one table of the dump, or report that this machine has no dump.
fn read_table(name: &str) -> Option<Table> {
    let path = table_dir().join(name);
    if !path.is_file() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
    Some(read(name, &bytes).unwrap_or_else(|| panic!("{} did not read as a table", name)))
}

/// A minimal well formed .gmd, built here so the reader can be tested without
/// any game data. The layout is the one `romfstables` documents.
fn synth_gmd(key: &str, strings: &[&str]) -> Vec<u8> {
    let mut text: Vec<u8> = Vec::new();
    for s in strings {
        text.extend_from_slice(s.as_bytes());
        text.push(0);
    }
    let count = strings.len() as u32;
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"GMD\0");
    out.extend_from_slice(&0x0001_0302u32.to_le_bytes()); // +0x04 version
    out.extend_from_slice(&0u32.to_le_bytes()); // +0x08
    out.extend_from_slice(&0x42D3_1A32u32.to_le_bytes()); // +0x0C tag
    out.extend_from_slice(&0u32.to_le_bytes()); // +0x10
    out.extend_from_slice(&count.to_le_bytes()); // +0x14
    out.extend_from_slice(&count.to_le_bytes()); // +0x18
    out.extend_from_slice(&0u32.to_le_bytes()); // +0x1C
    out.extend_from_slice(&(text.len() as u32).to_le_bytes()); // +0x20
    out.extend_from_slice(&(key.len() as u32).to_le_bytes()); // +0x24
    out.extend_from_slice(key.as_bytes());
    out.push(0);
    out.extend_from_slice(&text);
    out
}

/// The name of every .gmd / .atd file in the dump, sorted.
fn table_files() -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(table_dir()) else {
        eprintln!("skipping: {} is not a folder", table_dir().display());
        return Vec::new();
    };
    let mut names: Vec<String> = dir
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_owned()))
        .filter(|n| can_read(n))
        .collect();
    names.sort();
    names
}

#[test]
fn every_table_file_of_a_dump_reads_as_a_table() {
    let files = table_files();
    if files.is_empty() {
        return;
    }
    let mut tables: Vec<Table> = Vec::new();
    for name in &files {
        let path = table_dir().join(name);
        let bytes = std::fs::read(&path).expect("a table file reads");
        let table = read(name, &bytes)
            .unwrap_or_else(|| panic!("{} did not read as a table", name));
        // A table with no entry is not a table: every layout has to leave one.
        assert!(!table.rows.is_empty(), "{} listed no entry", name);
        // The rows cannot outnumber the strings they were taken from, and a
        // record table has no strings at all.
        if table.file.ends_with(ATD_EXT) {
            assert_eq!(table.strings, 0, "{}: a record table has no strings", name);
        } else {
            assert!(
                table.rows.len() <= table.strings,
                "{}: {} rows out of {} strings",
                name,
                table.rows.len(),
                table.strings
            );
            assert!(table.strings > 0, "{}: no strings decoded", name);
        }
        tables.push(table);
    }
    // A retail dump holds 104 .gmd files and one .atd. Anything below a hundred
    // means the walk, not the dump, is wrong.
    assert!(
        tables.len() >= 100,
        "only {} of {} table files read",
        tables.len(),
        files.len()
    );
    let line = summary(&tables);
    println!("read {} files: {}", tables.len(), line);
    assert!(line.contains("テーブル"));
    // The list is what the tab shows, so it has to be ordered by file name:
    // the same dump must not produce two different orders.
    let names: Vec<&str> = tables.iter().map(|t| t.file.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn the_measured_layouts_give_the_entry_counts_of_a_retail_dump() {
    // The numbers every layout in the module docs is taken from. They are the
    // point of the layouts: a table read with the wrong stride still "works"
    // and would quietly name the descriptions instead of the names.
    let expected: &[(&str, usize)] = &[
        ("GC_Medal_jpn.gmd", 132),
        ("GC_Title_1_jpn.gmd", 1309),
        ("GC_Title_2_jpn.gmd", 121),
        ("activityData_jpn.gmd", 184),
        ("activityData.atd", 184),
        ("itemData_jpn.gmd", 2991),
        ("skillData_jpn.gmd", 328),
        ("skillTypeData_jpn.gmd", 206),
        ("armorSeriesData_jpn.gmd", 6435),
        ("weapon00MsgData_jpn.gmd", 137),
        ("otArmorData_jpn.gmd", 1054),
        ("otWeaponData_jpn.gmd", 509),
        ("catSkillData_jpn.gmd", 66),
        ("ot_skl_jpn.gmd", 96),
        ("amuletData_jpn.gmd", 11),
        ("hunterArtsData_jpn.gmd", 191),
    ];
    let mut checked = 0;
    for (name, count) in expected {
        let Some(table) = read_table(name) else {
            continue;
        };
        assert_eq!(
            table.count(),
            *count,
            "{} holds {} entries, not {}",
            name,
            table.count(),
            count
        );
        checked += 1;
    }
    if checked == 0 {
        return;
    }
    assert_eq!(checked, expected.len(), "the dump is missing a table");
}

#[test]
fn the_item_table_names_one_entry_per_item_id() {
    let Some(table) = read_table("itemData_jpn.gmd") else {
        return;
    };
    // The catalogue is what the item box validates against: an id above
    // LAST_ITEM_ID is not an item, so the table has to hold exactly one name
    // per valid id, index = id.
    assert_eq!(
        table.count(),
        mhnx_core::gamedata::LAST_ITEM_ID + 1,
        "the item table and the item id space disagree"
    );
    // Index 0 is the blank slot and index 1 the first item; the string after
    // each name is its description, which the layout leaves out.
    assert_eq!(table.rows[0], "－－－－－－－－", "row 0 is the blank item");
    assert_eq!(table.rows[1], "調合書①入門編", "row 1 is the first item");
    assert_eq!(table.rows[2], "調合書②初級編", "row 2 is the second item");
    assert_eq!(
        table.rows[1],
        mhnx_core::gamedata::ITEM_NAME_LIST[1],
        "the table and the item catalogue agree on id 1"
    );
}

#[test]
fn the_award_and_request_tables_agree_with_romfstables() {
    let (Some(medal), Some(requests)) = (
        read_table("GC_Medal_jpn.gmd"),
        read_table("activityData_jpn.gmd"),
    ) else {
        return;
    };
    // The same bytes, read by the module that was already trusted with them:
    // the table list must not disagree with the names the editor writes with.
    let medal_bytes = std::fs::read(table_dir().join("GC_Medal_jpn.gmd")).expect("medal bytes");
    let awards = mhnx_core::romfstables::award_names(&medal_bytes);
    assert_eq!(awards.names.len(), mhnx_core::romfstables::AWARD_COUNT);
    assert_eq!(medal.rows, awards.names);
    assert_eq!(medal.count(), mhnx_core::romfstables::AWARD_COUNT);

    let request_bytes = std::fs::read(table_dir().join("activityData_jpn.gmd")).expect("request bytes");
    let names = mhnx_core::romfstables::request_names(&request_bytes);
    assert_eq!(requests.rows, names);
    assert_eq!(requests.count(), 184);

    // The record table is the other half of the same request table.
    let Some(records) = read_table("activityData.atd") else {
        return;
    };
    let atd_bytes = std::fs::read(table_dir().join("activityData.atd")).expect("atd bytes");
    let parsed = mhnx_core::romfstables::request_records(&atd_bytes);
    assert_eq!(records.count(), parsed.len());
    assert_eq!(records.count(), names.len(), "one row per request");
    for (row, record) in records.rows.iter().zip(parsed.iter()) {
        assert!(row.contains(&record.accept_flag.to_string()), "{}", row);
        assert!(row.contains(&record.complete_flag.to_string()), "{}", row);
    }
}

#[test]
fn the_catalogue_names_the_files_it_holds_and_no_others() {
    // A spec matches a prefix, so the order of the table and the specificity of
    // the prefix are the whole of the lookup. These are the cases that would
    // silently pick the wrong row of the list.
    let cases = [
        ("GC_Medal_jpn.gmd", "勲章名"),
        ("GC_Medal_eng.gmd", "勲章名"),
        ("GC_Title_1_jpn.gmd", "称号（語）"),
        ("GC_Title_2_jpn.gmd", "称号（接続語）"),
        ("activityData_jpn.gmd", "依頼名"),
        ("activityData.atd", "依頼レコード"),
        ("itemData_jpn.gmd", "アイテム名"),
        ("itemCategoryTypeData_jpn.gmd", "アイテムの分類"),
        ("skillData_jpn.gmd", "スキル名"),
        ("skillTypeData_jpn.gmd", "スキル系統名"),
        ("armorSeriesData_jpn.gmd", "防具名"),
        ("weapon03MsgData_jpn.gmd", "武器名"),
        ("otArmorData_jpn.gmd", "オトモ防具名"),
        ("ot_skl_jpn.gmd", "オトモスキル（術）"),
        ("catSkillData_jpn.gmd", "オトモスキル"),
        ("MonsterDemo_jpn.gmd", "モンスター名（デモ）"),
        ("hunterArtsData_jpn.gmd", "狩技名"),
        ("insectAbirity_jpn.gmd", "猟虫の能力"),
    ];
    for (file, title) in cases {
        let spec = spec_for(file).unwrap_or_else(|| panic!("{} has no catalogue row", file));
        assert_eq!(spec.title, title, "{}", file);
    }
    // A binary table this crate cannot read is not claimed by the catalogue
    // either: the list must not offer a row it cannot fill.
    assert!(spec_for("itemData.itm").is_none());
    assert!(spec_for("DLC_jpn.gmd").is_none());
    assert!(!can_read("weapon03MsgData.w03m"));
    assert!(can_read("activityData.atd"));
    assert!(can_read("GC_Medal_jpn.gmd"));
    assert!(!can_read("no extension"));
    // Every catalogue row has to answer to a file name built out of its own
    // prefix. A prefix with a wrong letter in it leaves the row silently dead:
    // the file then falls back to its stem and nothing looks broken. That is how
    // the monster table stood in the list under its raw file name for a while.
    for spec in TABLES {
        assert!(!spec.prefix.is_empty(), "a spec with no prefix matches all");
        assert!(!spec.title.is_empty(), "{} has no label", spec.prefix);
        let name = format!("{}{}_jpn.{}", spec.prefix, spec.needle, spec.ext);
        let found = spec_for(&name)
            .unwrap_or_else(|| panic!("{} is not matched by its own catalogue row", name));
        assert_eq!(found.title, spec.title, "{} reached the wrong row", name);
    }
}

#[test]
fn a_file_that_is_not_a_table_is_skipped_rather_than_listed_empty() {
    // Not a .gmd at all, a .gmd that is only a header, and a .gmd whose count
    // field lies about how much it holds: none of them may come back as a
    // table with zero rows, because the list would show a table that is empty.
    assert!(read("DLC_jpn.gmd", b"").is_none());
    assert!(read("DLC_jpn.gmd", b"GMD\0").is_none());
    assert!(read("activityData.atd", &[0u8; 4]).is_none());
    assert!(read("not-a-table.txt", b"GMD\0").is_none());
    let gmd = synth_gmd("key", &["a", "b", "c", "d"]);
    assert!(read("DLC_jpn.gmd", &gmd).is_some());

    // An unknown .gmd is still readable: its own stem is the label and every
    // string is a row, so a dump with a file this build has never heard of
    // still shows it instead of hiding it.
    let table = read("SomethingNew_jpn.gmd", &gmd).expect("an unknown gmd still reads");
    assert_eq!(table.title, "SomethingNew_jpn");
    assert_eq!(table.layout, Layout::Each);
    assert_eq!(table.count(), 4);
    assert_eq!(table.rows, ["a", "b", "c", "d"]);

    // The catalogue's layout is applied to the catalogue's table.
    let medal = synth_gmd("GC_Medal", &["name1", "name2", "desc1", "desc2"]);
    let table = read("GC_Medal_jpn.gmd", &medal).expect("a medal table");
    assert_eq!(table.layout, Layout::FirstHalf);
    assert_eq!(table.rows, ["name1", "name2"]);
}

#[test]
fn the_search_folds_ascii_case_and_keeps_the_file_position() {
    let rows: Vec<String> = vec![
        String::from("ベルダーブレイド"),
        String::from("Invalid Message"),
        String::from("dummy"),
        String::from("調合書①入門編"),
    ];
    // An empty needle is every row: the tab draws the first PREVIEW of them.
    assert_eq!(filter(&rows, "").len(), rows.len());

    // The rows keep the number their file gives them, not the number of the hit.
    let hits = filter(&rows, "DUM");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, 2, "the position has to be the row's own");
    assert_eq!(hits[0].1, "dummy");

    // A Japanese needle is a plain substring, and the edges are trimmed.
    let hits = filter(&rows, "  調合書  ");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, 3);

    // A needle nothing answers is an empty list, not every row.
    assert!(filter(&rows, "そんな名前はない").is_empty());
    assert_eq!(normalise("  Dummy "), "dummy");
}

#[test]
fn a_table_answers_a_needle_by_its_rows_and_its_own_name() {
    let table = Table {
        file: String::from("itemData_jpn.gmd"),
        title: String::from("アイテム名"),
        layout: Layout::Stride {
            stride: 2,
            offset: 0,
        },
        bytes: 1234,
        strings: 20,
        rows: vec![
            String::from("調合書①入門編"),
            String::from("dummy"),
            String::from("回復薬"),
        ],
    };
    // Through a row, through the label, through the file name, and not at all.
    assert!(table.answered_by("調合書"));
    assert!(table.answered_by("アイテム"));
    assert!(table.answered_by("ITEMDATA"));
    assert!(table.answered_by(""));
    assert!(!table.answered_by("称号"));
    assert_eq!(table.matching("dummy").len(), 1);
    // The preview stops at PREVIEW however long the table is.
    let long = Table {
        rows: (0..PREVIEW * 3).map(|i| i.to_string()).collect(),
        ..table
    };
    assert_eq!(long.preview().len(), PREVIEW);
    assert_eq!(long.count(), PREVIEW * 3);
    assert_eq!(long.matching("").len(), PREVIEW * 3);
}
