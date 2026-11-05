//! The romfs name tables.
//!
//! The award names and the village request names are game data, so they are
//! not in this repository. These tests read them out of the machine's own MHXX
//! dump when it is present and skip, rather than fail, when it is not. The
//! real-file cases assert the counts and the structure measured on a retail
//! Japanese dump (see the romfstables module docs); the synthetic cases run
//! everywhere and pin down the parsers themselves.

use std::collections::BTreeSet;
use std::path::PathBuf;

use mhnx_core::romfstables::{
    award_names, cat_skill_names, gmd_strings, monster_list_rows, otomo_skill_names, request_names,
    request_records, ATD_ACCEPT_OFFSET, ATD_COMPLETE_OFFSET, ATD_COUNT_OFFSET, ATD_HEADER_SIZE,
    ATD_KIND_OFFSET, ATD_QUEST_ID_OFFSET, ATD_RECORD_SIZE, AWARD_COUNT, CAT_SKILL_COUNT,
    GMD_BLOCK_LEN_OFFSET, GMD_COUNT_OFFSET, GMD_HEADER_SIZE, GMD_MAGIC, MONSTER_LIST_HEADER,
    MONSTER_LIST_ROWS, OTOMO_SKILL_COUNT, OTOMO_SKILL_STRIDE, REQUEST_STRIDE,
};

/// Strings in GC_Medal: 132 names, then 132 descriptions.
const MEDAL_STRINGS: usize = 2 * AWARD_COUNT;
/// Records in activityData.atd.
const REQUESTS: usize = 184;
/// Strings in activityData_jpn.gmd.
const REQUEST_STRINGS: usize = REQUESTS * REQUEST_STRIDE;
/// Accept flag of the first record; the flag space starts here.
const FIRST_ACCEPT_FLAG: u16 = 300;

/// Directory holding the language tables, overridable for another dump.
fn table_dir() -> PathBuf {
    match std::env::var_os("MHNX_ROMFS_TABLE") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(r"C:\anyWorkspace\nx\mhxx\romfs\nativeNX\table"),
    }
}

/// Directory holding the shipped quest archives, overridable for another dump.
fn quest_dir() -> PathBuf {
    match std::env::var_os("MHNX_QUEST_DIR") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(r"C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest"),
    }
}

/// Read one table, or report that this machine has no dump to read.
fn read_table(name: &str) -> Option<Vec<u8>> {
    let path = table_dir().join(name);
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e)))
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// A minimal well formed .gmd, built here so the parser can be tested without
/// any game data. The layout matches the module docs exactly.
fn synth_gmd(key: &str, strings: &[&str]) -> Vec<u8> {
    let mut text: Vec<u8> = Vec::new();
    for s in strings {
        text.extend_from_slice(s.as_bytes());
        text.push(0);
    }
    let count = strings.len() as u32;
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&GMD_MAGIC);
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

/// A synthetic activityData.atd using the offsets measured on the real file.
fn synth_atd(count: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0x00, 0x00, 0xA0, 0x40]);
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for i in 0..count {
        let mut rec = [0u8; ATD_RECORD_SIZE];
        rec[0..4].copy_from_slice(&(i as u32).to_le_bytes());
        rec[ATD_QUEST_ID_OFFSET..ATD_QUEST_ID_OFFSET + 4]
            .copy_from_slice(&(1000 + i as u32).to_le_bytes());
        let accept = FIRST_ACCEPT_FLAG + 2 * i as u16;
        rec[ATD_ACCEPT_OFFSET..ATD_ACCEPT_OFFSET + 2].copy_from_slice(&accept.to_le_bytes());
        rec[ATD_COMPLETE_OFFSET..ATD_COMPLETE_OFFSET + 2]
            .copy_from_slice(&(accept + 1).to_le_bytes());
        out.extend_from_slice(&rec);
    }
    out
}

#[test]
fn medal_table_holds_132_names_then_132_descriptions() {
    let gmd = match read_table("GC_Medal_jpn.gmd") {
        Some(bytes) => bytes,
        None => return,
    };
    let strings = gmd_strings(&gmd);
    assert_eq!(
        strings.len(),
        MEDAL_STRINGS,
        "the medal table must decode completely"
    );
    assert!(
        strings.iter().all(|s| !s.contains('\u{FFFD}')),
        "the medal table must be valid UTF-8"
    );

    let awards = award_names(&gmd);
    assert_eq!(awards.names.len(), AWARD_COUNT, "132 medals");
    assert_eq!(
        awards.names,
        strings[..AWARD_COUNT],
        "the names are the first half"
    );

    // Names are one line each; every description wraps (measured on the real
    // dump: 132 of 132). The two halves cannot be confused.
    assert!(awards
        .names
        .iter()
        .all(|n| !n.is_empty() && !n.contains('\r') && !n.contains('\n')));
    assert!(strings[AWARD_COUNT..].iter().all(|d| d.contains("\r\n")));

    // The real table lists each award once.
    let unique: BTreeSet<&String> = awards.names.iter().collect();
    assert_eq!(unique.len(), AWARD_COUNT, "award names must be distinct");
}

#[test]
fn request_table_holds_184_records_with_paired_flags() {
    let atd = match read_table("activityData.atd") {
        Some(bytes) => bytes,
        None => return,
    };
    // The record size in the parser is the stride the file actually uses.
    assert_eq!(
        (atd.len() - ATD_HEADER_SIZE) % REQUESTS,
        0,
        "the records must divide the file evenly"
    );
    assert_eq!(
        (atd.len() - ATD_HEADER_SIZE) / REQUESTS,
        ATD_RECORD_SIZE,
        "records are 55 bytes"
    );

    let records = request_records(&atd);
    assert_eq!(records.len(), REQUESTS);

    // Every record's own index field agrees with its position, so the two can
    // be used interchangeably.
    for (i, r) in records.iter().enumerate() {
        assert_eq!(r.index, i);
        assert_eq!(
            u32_at(&atd, ATD_HEADER_SIZE + i * ATD_RECORD_SIZE),
            i as u32
        );
    }

    // Accept and complete are one consecutive pair per record: 300/301,
    // 302/303, ... The numbers are not dense (the game skips values, and the
    // second band starts at 1270), so the pair is a measured value, not an
    // index. The last record is the all zero sentinel.
    let mut pairs: BTreeSet<u16> = BTreeSet::new();
    let mut zero = 0usize;
    for r in &records {
        // The kind is the stored byte, and it agrees with the quest id: only
        // the quest rows carry one.
        assert_eq!(
            r.kind,
            atd[ATD_HEADER_SIZE + r.index * ATD_RECORD_SIZE + ATD_KIND_OFFSET]
        );
        assert_eq!(
            r.quest_id != 0,
            r.kind == 0,
            "record {}: kind {} with quest {}",
            r.index,
            r.kind,
            r.quest_id
        );
        if r.accept_flag == 0 {
            zero += 1;
            assert_eq!(r.index, REQUESTS - 1, "only the sentinel may be zero");
            assert_eq!(r.complete_flag, 0);
            assert_eq!(r.kind, 3, "the sentinel closes the table");
            continue;
        }
        assert_eq!(
            r.complete_flag,
            r.accept_flag + 1,
            "record {} pair",
            r.index
        );
        assert!(
            pairs.insert(r.accept_flag),
            "record {} reuses a flag pair",
            r.index
        );
    }
    assert_eq!(zero, 1, "the file ends with one unused record");
    assert_eq!(pairs.len(), REQUESTS - 1, "every record owns its own pair");
    assert_eq!(
        records[0].accept_flag, FIRST_ACCEPT_FLAG,
        "the flag space starts at 300"
    );
    // The quest rows are the majority; the rest are events with no quest.
    assert_eq!(
        records.iter().filter(|r| r.kind == 0).count(),
        167,
        "measured: 167 quest rows, 16 event rows, 1 sentinel"
    );
}

#[test]
fn request_names_are_the_first_column_of_each_five() {
    let gmd = match read_table("activityData_jpn.gmd") {
        Some(bytes) => bytes,
        None => return,
    };
    let strings = gmd_strings(&gmd);
    assert_eq!(strings.len(), REQUEST_STRINGS, "184 records x 5 strings");
    assert!(
        strings.iter().all(|s| !s.contains('\u{FFFD}')),
        "the request table must be valid UTF-8"
    );

    let names = request_names(&gmd);
    assert_eq!(names.len(), REQUESTS);
    for (i, name) in names.iter().enumerate() {
        assert_eq!(
            name,
            &strings[i * REQUEST_STRIDE],
            "name column of record {}",
            i
        );
        assert!(!name.is_empty(), "record {} has an empty name", i);
        assert!(
            !name.contains('\r') && !name.contains('\n'),
            "record {} name is one line",
            i
        );
    }
    // The filler columns are not names: no record is named by the literal
    // placeholder the other four strings of a row use.
    assert!(names.iter().all(|n| n != "dummy"));

    // One name per record, and the record table agrees on how many there are.
    if let Some(atd) = read_table("activityData.atd") {
        assert_eq!(names.len(), request_records(&atd).len());
    }
}

#[test]
fn every_gmd_in_the_table_directory_decodes_completely() {
    let dir = table_dir();
    if !dir.exists() {
        eprintln!("skipping: {} not found", dir.display());
        return;
    }
    let mut files = 0usize;
    let mut strings_seen = 0usize;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("gmd") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        if bytes.len() < GMD_HEADER_SIZE || bytes[..4] != GMD_MAGIC {
            continue;
        }
        let declared = u32_at(&bytes, GMD_COUNT_OFFSET) as usize;
        let strings = gmd_strings(&bytes);
        assert_eq!(
            strings.len(),
            declared,
            "{}: the header count and the decoded strings disagree",
            path.display()
        );
        assert!(
            strings.iter().all(|s| !s.contains('\u{FFFD}')),
            "{}: the string block must be valid UTF-8",
            path.display()
        );
        files += 1;
        strings_seen += strings.len();
    }
    eprintln!("decoded {} .gmd files / {} strings", files, strings_seen);
    assert!(
        files >= 100,
        "expected the shipped table set, saw {}",
        files
    );
}

#[test]
fn quest_archives_agree_with_the_general_parser() {
    // The quest .gmd files have no line table, so their text starts right after
    // the key. The tail based reader must return the same strings as the
    // dedicated parser in quest.rs; if it ever stops doing that, one of the two
    // splitting rules is wrong.
    let dir = quest_dir();
    if !dir.exists() {
        eprintln!("skipping: {} not found", dir.display());
        return;
    }
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("arc") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let arc = mhnx_core::arc::Arc::parse(&bytes)
            .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        let gmd_entry = match arc.by_ext("gmd") {
            Some(entry) => entry,
            None => continue,
        };
        let general = gmd_strings(&gmd_entry.data);
        let quest = mhnx_core::quest::Gmd::from_bytes(&gmd_entry.data)
            .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        assert_eq!(general, quest.strings, "{}", path.display());
        checked += 1;
    }
    if checked > 0 {
        assert!(
            checked >= 100,
            "expected the shipped quest set, saw {}",
            checked
        );
    }
}

#[test]
fn synthetic_tables_parse_to_what_was_written() {
    // A .gmd with no line table: the text starts right after the key.
    let strings = ["one", "two", "three", ""];
    let gmd = synth_gmd("TEST", &strings);
    assert_eq!(gmd_strings(&gmd), strings);

    // A .gmd with a line table before the text: the tail is used instead.
    let mut with_table = synth_gmd("TEST", &strings);
    let text_len = u32_at(&with_table, 0x20) as usize;
    let key_len = u32_at(&with_table, 0x24) as usize;
    let text_start = with_table.len() - text_len;
    let mut table: Vec<u8> = Vec::new();
    for i in 0..strings.len() {
        let mut record = [0u8; 20];
        record[0..4].copy_from_slice(&(i as u32).to_le_bytes());
        table.extend_from_slice(&record);
    }
    let mut rebuilt = with_table[..GMD_HEADER_SIZE + key_len + 1].to_vec();
    rebuilt.extend_from_slice(&table);
    rebuilt.extend_from_slice(&with_table[text_start..]);
    with_table = rebuilt;
    // The block length is unchanged, so the tail is still the text block.
    assert_eq!(gmd_strings(&with_table), strings);

    // atd round trip.
    let atd = synth_atd(3);
    let records = request_records(&atd);
    assert_eq!(records.len(), 3);
    for (i, r) in records.iter().enumerate() {
        assert_eq!(r.index, i);
        assert_eq!(r.quest_id, 1000 + i as u32);
        assert_eq!(r.accept_flag, FIRST_ACCEPT_FLAG + 2 * i as u16);
        assert_eq!(r.complete_flag, r.accept_flag + 1);
    }

    // A file whose length is not a multiple of the row stride is refused: the
    // name column would shift.
    assert!(request_names(&synth_gmd("TEST", &["a", "b", "c"])).is_empty());
    // A medal table with the wrong string count is refused outright.
    assert!(award_names(&synth_gmd("GC_Medal", &["a", "b", "c"]))
        .names
        .is_empty());
}

/// A .gmd holding `count` rows of `stride` strings, each name unique.
fn synth_rows(key: &str, count: usize, stride: usize) -> Vec<u8> {
    let mut owned: Vec<String> = Vec::new();
    for row in 0..count {
        owned.push(format!("name{}", row));
        for col in 1..stride {
            owned.push(format!("column{} of row{}", col, row));
        }
    }
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    synth_gmd(key, &refs)
}

#[test]
fn a_skill_table_reads_its_name_column_and_refuses_a_shifted_one() {
    // ot_skl: a name and a description per skill, so the names are every other
    // string from the first. The parser must not be able to return descriptions
    // in their place.
    let gmd = synth_rows("ot_skl", OTOMO_SKILL_COUNT, OTOMO_SKILL_STRIDE);
    let names = otomo_skill_names(&gmd);
    assert_eq!(names.len(), OTOMO_SKILL_COUNT);
    for (i, name) in names.iter().enumerate() {
        assert_eq!(name, &format!("name{}", i), "row {}", i);
        assert!(!name.starts_with("column"), "row {} is a description", i);
    }
    // A table with one row too few or too many would put a description where a
    // name belongs past that point, so it is refused whole, not partly read.
    assert!(otomo_skill_names(&synth_rows(
        "ot_skl",
        OTOMO_SKILL_COUNT - 1,
        OTOMO_SKILL_STRIDE
    ))
    .is_empty());
    assert!(otomo_skill_names(&synth_rows(
        "ot_skl",
        OTOMO_SKILL_COUNT + 1,
        OTOMO_SKILL_STRIDE
    ))
    .is_empty());
    // A file that is not a .gmd at all yields nothing.
    assert!(otomo_skill_names(&[]).is_empty());
    assert!(otomo_skill_names(&[0xAAu8; 512]).is_empty());

    // catSkillData: a name and two note columns per row.
    let cat = synth_rows("catSkillData", CAT_SKILL_COUNT, 3);
    let names = cat_skill_names(&cat);
    assert_eq!(names.len(), CAT_SKILL_COUNT);
    assert_eq!(names[1], "name1");
    assert!(cat_skill_names(&synth_rows("catSkillData", CAT_SKILL_COUNT - 1, 3)).is_empty());
}

#[test]
fn the_dumps_skill_table_names_the_96_skills_the_editor_shows() {
    // The measurement this reader is adopted on: ot_skl names the otomon 術, and
    // it names them exactly where the editor's bundled table has them.
    let gmd = match read_table("ot_skl_jpn.gmd") {
        Some(bytes) => bytes,
        None => return,
    };
    let strings = gmd_strings(&gmd);
    assert_eq!(
        strings.len(),
        OTOMO_SKILL_COUNT * OTOMO_SKILL_STRIDE,
        "96 skills x (name, description)"
    );
    let names = otomo_skill_names(&gmd);
    assert_eq!(names.len(), OTOMO_SKILL_COUNT, "96 skills");

    // Names are one line each; the descriptions wrap. The two columns cannot be
    // confused by eye or by index.
    assert!(names
        .iter()
        .all(|n| !n.is_empty() && !n.contains('\r') && !n.contains('\n')));
    assert_eq!(names[0], "スキル無し");
    assert_eq!(names[1], "体力強化の術【小】");

    // Position for position with the bundled table, 0..96. The bundled table's
    // 97th row is the "?" placeholder the file has no skill for.
    let bundled = mhnx_core::gamedata::PALICO_SKILL_NAMES;
    assert_eq!(
        names[..],
        bundled[..OTOMO_SKILL_COUNT],
        "romfs and bundled agree"
    );
    assert_eq!(bundled[OTOMO_SKILL_COUNT], "?");

    // And it is not the support action table: not one of the 58 is a whole
    // string of the file, so no id of that table can be read from here.
    for name in mhnx_core::gamedata::PALICO_SUPPORT_MOVES {
        assert!(
            !strings.iter().any(|s| s == name),
            "{} is a support action, not a skill",
            name
        );
        assert!(
            !names.iter().any(|n| n == name),
            "{} is not a skill name",
            name
        );
    }
    // Eleven of them do occur *inside* the descriptions ("笛上手の術" mentions
    // 回復笛, "ビースト延長の術" mentions ビースト変化, ...). That is exactly why
    // the column is read by stride and never by searching the file: a search
    // would return a support action as if it were a skill.
    let inside = mhnx_core::gamedata::PALICO_SUPPORT_MOVES
        .iter()
        .filter(|m| strings.iter().any(|s| s.contains(*m)))
        .count();
    assert_eq!(inside, 11, "measured: 11 support actions appear in prose");
}

#[test]
fn the_cat_skill_table_is_not_the_table_the_editor_shows() {
    let gmd = match read_table("catSkillData_jpn.gmd") {
        Some(bytes) => bytes,
        None => return,
    };
    let names = cat_skill_names(&gmd);
    assert_eq!(names.len(), CAT_SKILL_COUNT, "66 cat skills");
    assert!(names
        .iter()
        .all(|n| !n.is_empty() && !n.contains('\r') && !n.contains('\n')));
    assert_eq!(names[0], "スキル無し");
    assert_eq!(names[1], "ネコの解体術【大】");

    // 66 distinct rows: this file is not a stride misread of something else.
    let unique: BTreeSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), CAT_SKILL_COUNT);

    // It is not the editor's skill table, so it is read but not adopted: the
    // only string the two lists share is the "スキル無し" placeholder.
    let bundled = mhnx_core::gamedata::PALICO_SKILL_NAMES;
    let shared: Vec<&String> = names
        .iter()
        .filter(|n| bundled.contains(&n.as_str()))
        .collect();
    assert_eq!(shared.len(), 1, "only the placeholder is shared");
    assert_eq!(shared[0], "スキル無し");
    for name in mhnx_core::gamedata::PALICO_SUPPORT_MOVES {
        assert!(
            !names.iter().any(|n| n == name),
            "{} is not a cat skill",
            name
        );
    }
}

#[test]
fn the_monster_list_is_descriptions_and_tips_and_names_nothing() {
    let gmd = match read_table("HN_MonsterListMsg_jpn.gmd") {
        Some(bytes) => bytes,
        None => return,
    };
    let strings = gmd_strings(&gmd);
    assert_eq!(
        strings.len(),
        MONSTER_LIST_HEADER + MONSTER_LIST_ROWS * 2,
        "ten labels, then 105 pairs"
    );
    // The labels are interface words, not monsters.
    assert_eq!(strings[0], "モンスターリスト");
    assert_eq!(strings[2], "狩猟数(捕獲数)");

    let rows = monster_list_rows(&gmd);
    assert_eq!(rows.len(), MONSTER_LIST_ROWS, "105 monsters");
    // The note's order is not the save's, so the two lists cannot be zipped:
    // the note opens with the Deviants (row 0 is 燼滅刃ディノバルド) while the
    // hunt record opens with リオレイア, and the note covers 105 monsters
    // against the record's 137 rows.
    assert!(
        rows[0].description.contains("燼滅刃"),
        "{}",
        rows[0].description
    );
    assert!(rows[0].description.contains("ディノバルド"));
    assert_eq!(mhnx_core::gamedata::MONSTER_HUNT_NAMES[0], "リオレイア");
    // Every entry is prose: a description that wraps, and a tip that wraps.
    assert!(rows.iter().all(|r| r.description.contains("\r\n")));
    assert!(rows.iter().all(|r| r.tip.contains("\r\n")));
    // And none of them is a name: not one row is the short one-line string the
    // hunt record shows for that monster.
    let monsters = mhnx_core::gamedata::MONSTER_HUNT_NAMES;
    for row in &rows {
        assert!(
            !monsters.contains(&row.description.as_str()),
            "{} is a monster name, not a description",
            row.description
        );
    }
    // A file with the wrong count is refused whole: a shifted pair would put a
    // tip where a description belongs.
    assert!(monster_list_rows(&synth_gmd("HN_MonsterListMsg", &["a", "b"])).is_empty());
    assert!(monster_list_rows(&[]).is_empty());
}

/// Every `.gmd` under a directory, recursively.
fn gmd_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            gmd_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("gmd") {
            out.push(path);
        }
    }
}

#[test]
fn no_table_of_the_dump_names_the_hunt_record_monsters() {
    // The measurement the bundled 137 row table is kept on. A name table would
    // have to hold the rows the save indexes; no file of the dump does, and the
    // few that hold many names hold them as titles or as the 48 monster demo,
    // neither of which is the hunt record.
    let dir = table_dir();
    if !dir.exists() {
        eprintln!("skipping: {} not found", dir.display());
        return;
    }
    let mut files: Vec<PathBuf> = Vec::new();
    gmd_files(&dir, &mut files);
    if files.is_empty() {
        return;
    }

    let names: Vec<&str> = mhnx_core::gamedata::MONSTER_HUNT_NAMES
        .iter()
        .copied()
        .filter(|n| *n != "不明" && *n != "ROBOT" && !n.starts_with("Unknown["))
        .collect();
    assert_eq!(
        names.len(),
        129,
        "129 real names, 7 不明 rows and one placeholder"
    );

    let mut best = (0usize, String::new());
    let mut whole: BTreeSet<usize> = BTreeSet::new();
    let mut corpus = String::new();
    for path in &files {
        let strings = gmd_strings(&std::fs::read(path).unwrap());
        if strings.is_empty() {
            continue;
        }
        let set: BTreeSet<&String> = strings.iter().collect();
        let mut hits = 0usize;
        for (i, name) in names.iter().enumerate() {
            if set.iter().any(|s| s.as_str() == *name) {
                hits += 1;
                whole.insert(i);
            }
        }
        if hits > best.0 {
            best = (hits, path.display().to_string());
        }
        for s in &strings {
            corpus.push_str(s);
            corpus.push('\n');
        }
    }

    // No file holds more names than the guild-card title table, and that one
    // holds 57 of the 129: there is no 105 or 137 row name list to read.
    assert_eq!(
        best.0, 57,
        "measured: {} holds {} of the 129 names",
        best.1, best.0
    );
    assert!(best.1.ends_with("GC_Title_1_jpn.gmd"), "{}", best.1);
    assert_eq!(whole.len(), 73, "measured: 73 names occur as whole strings");
    // And four names the editor shows are not in the dump in any form - the
    // editor composes them from a base name and a variant word.
    let absent: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| !corpus.contains(n))
        .collect();
    assert_eq!(
        absent,
        vec![
            "怒り喰らうイビルジョー",
            "ミラボレアス【紅龍】",
            "ミラボレアス【祖龍】",
            "猛り爆ぜるブラキディオス",
        ],
        "the four names the dump never spells out"
    );
}

#[test]
fn broken_input_yields_empty_tables_instead_of_panicking() {
    // Nothing at all.
    assert!(gmd_strings(&[]).is_empty());
    assert!(award_names(&[]).names.is_empty());
    assert!(request_names(&[]).is_empty());
    assert!(request_records(&[]).is_empty());

    // Right size, wrong magic.
    let junk = vec![0xAAu8; 512];
    assert!(gmd_strings(&junk).is_empty());
    assert!(request_records(&junk).is_empty());

    // Right magic, nonsense everywhere else.
    let mut noise = junk.clone();
    noise[..4].copy_from_slice(&GMD_MAGIC);
    assert!(gmd_strings(&noise).is_empty());

    // Every truncation of a valid file must be safe, and must never invent
    // more strings than the file declares.
    let gmd = synth_gmd("TEST", &["one", "two", "three"]);
    for n in 0..gmd.len() {
        assert!(gmd_strings(&gmd[..n]).len() <= 3, "truncation at {}", n);
        let _ = award_names(&gmd[..n]);
        let _ = request_names(&gmd[..n]);
    }

    // A header claiming four billion strings: the text bounds the answer.
    let mut huge = gmd.clone();
    huge[GMD_COUNT_OFFSET..GMD_COUNT_OFFSET + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(gmd_strings(&huge).len() <= 3);

    // A string block whose last terminator is missing still yields that
    // string, and nothing else. The header keeps describing the shorter block.
    let mut unterminated = synth_gmd("TEST", &["one", "two"]);
    let last = unterminated.len() - 1;
    assert_eq!(unterminated[last], 0);
    unterminated.remove(last);
    let key_len = u32_at(&unterminated, 0x24) as usize;
    let text_len = unterminated.len() - (GMD_HEADER_SIZE + key_len + 1);
    unterminated[GMD_BLOCK_LEN_OFFSET..GMD_BLOCK_LEN_OFFSET + 4]
        .copy_from_slice(&(text_len as u32).to_le_bytes());
    assert_eq!(gmd_strings(&unterminated), ["one", "two"]);

    // atd: truncated header, absurd count, every truncation.
    let atd = synth_atd(4);
    for n in 0..atd.len() {
        assert!(request_records(&atd[..n]).len() <= 4, "truncation at {}", n);
    }
    let mut many = atd.clone();
    many[ATD_COUNT_OFFSET..ATD_COUNT_OFFSET + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(request_records(&many).is_empty());
}
