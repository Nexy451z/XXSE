//! What the two bytes after a 部位破壊 target's id are.
//!
//! A .mib target is 種類 + id + 数, five bytes. On a 部位破壊 row the 数 is not a
//! count: it is a part code over that monster's own part numbers (1..=11 for one
//! part, 12..=66 for a pair, lexicographic). These tests pin the codec, the
//! measured (monster, code) table, the editor step that walks it, and the 大連続
//! answer (its targets are the mob_slot rows).
//!
//! The last test reads the real 141 quest dump when this machine has it and is
//! skipped (not failed) on a clean checkout, like the other real-data tests.

use std::path::PathBuf;

use mhnx_core::arc::Arc;
use mhnx_core::hbedit::{
    mib_field_index, mib_mob_slot_ids, monster_name_of, part_code, part_code_slots,
    quest_part_choices, quest_part_field, quest_part_label, Editor, Key, MibPickKind, PartStep,
    Screen, QUEST_PARTS, QUEST_SOURCE_QUEST_DUMP, RENZOKU_TARGET_NOTE,
};
use mhnx_core::quest::Gmd;
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
    let mut mib = vec![0u8; mhnx_core::quest::MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&1_010_165u32.to_le_bytes());
    mib
}

/// 部位破壊 of ディノバルド (80) with the given part code.
fn part_break_mib(monster: u16, code: u16) -> Vec<u8> {
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 8);
    put_u16(&mut mib, "com0", monster);
    put_u16(&mut mib, "t13", code);
    mib
}

#[test]
fn a_part_code_names_one_part_or_a_pair() {
    // 0 is "no part", 1..=11 one part.
    assert_eq!(part_code_slots(0), Some([0, 0]));
    assert_eq!(part_code_slots(1), Some([1, 0]));
    assert_eq!(part_code_slots(7), Some([7, 0]));
    assert_eq!(part_code_slots(11), Some([11, 0]));
    // 12..=66 a pair, lexicographic over p < q.
    assert_eq!(part_code_slots(12), Some([1, 2]));
    assert_eq!(part_code_slots(13), Some([1, 3]));
    assert_eq!(part_code_slots(21), Some([1, 11]));
    assert_eq!(part_code_slots(22), Some([2, 3]));
    // The two pairs the shipped quests pin down: タマミツネ 背ビレと尻尾 = 57 and
    // ラギアクルス 胸部と尻尾 = 37.
    assert_eq!(part_code_slots(57), Some([7, 8]));
    assert_eq!(part_code_slots(37), Some([3, 10]));
    assert_eq!(part_code_slots(66), Some([10, 11]));
    assert_eq!(part_code_slots(67), None);
    assert_eq!(part_code_slots(999), None);

    // Both directions agree on every code the format can hold.
    for code in 0..=66u16 {
        let [p, q] = part_code_slots(code).expect("a code in range");
        assert_eq!(part_code(p, q), Some(code), "code {}", code);
    }
    assert_eq!(part_code(0, 0), Some(0));
    assert_eq!(part_code(12, 0), None, "no part 12");
    assert_eq!(part_code(3, 2), None, "p must be below q");
    assert_eq!(part_code(2, 2), None, "a part is not a pair with itself");
}

#[test]
fn the_two_bytes_of_a_part_break_row_are_the_part() {
    // ディノバルドの尻尾切断 = 7, the pair q1010002 / q1111001 store.
    let mib = part_break_mib(80, 7);
    let pf = quest_part_field(&mib, "t13").expect("a 数 field of a target row");
    assert!(pf.is_part_break);
    assert_eq!(pf.kind_key, "tt1");
    assert_eq!(pf.count_key, "t13");
    assert_eq!(pf.monster, 80);
    assert_eq!(pf.code, 7);
    assert_eq!(pf.slots, [7, 0]);
    assert_eq!(pf.measured, Some("尻尾"));
    assert_eq!(pf.label, "尻尾 (7)");

    // 頭部と背中 = 12 = (1,2).
    let mib = part_break_mib(80, 12);
    let pf = quest_part_field(&mib, "t13").expect("a 数 field");
    assert_eq!(pf.slots, [1, 2]);
    assert_eq!(pf.label, "頭部と背中 (12)");

    // A code no shipped quest uses for that monster keeps its numbers:
    // nothing is invented, the row says 未確定.
    let mib = part_break_mib(80, 3);
    let pf = quest_part_field(&mib, "t13").expect("a 数 field");
    assert_eq!(pf.measured, None);
    assert_eq!(pf.slots, [3, 0]);
    assert_eq!(pf.label, "部位3（未確定） (3)");
    let mib = part_break_mib(80, 34);
    let pf = quest_part_field(&mib, "t13").expect("a 数 field");
    assert_eq!(pf.slots, [3, 7]);
    assert_eq!(pf.label, "部位3と7（未確定） (34)");

    // A 狩猟/討伐 row stores a real count in the same bytes (ガブラス１２頭).
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 1);
    put_u16(&mut mib, "com0", 4115);
    put_u16(&mut mib, "t13", 12);
    let pf = quest_part_field(&mib, "t13").expect("a 数 field");
    assert!(!pf.is_part_break);
    assert_eq!(pf.measured, None);
    assert_eq!(pf.slots, [0, 0]);
    assert_eq!(pf.label, "12");

    // Only the three target rows have a 数: a 配置 row's bytes are not a code.
    assert_eq!(quest_part_field(&mib, "o31"), None);
    assert_eq!(quest_part_field(&mib, "time_limit"), None);

    // The picker is offered for a measured part only.
    assert_eq!(
        mhnx_core::hbedit::mib_pick_kind(&mib, mib_field_index("t13").unwrap()),
        MibPickKind::None
    );
    let measured = part_break_mib(80, 7);
    assert_eq!(
        mhnx_core::hbedit::mib_pick_kind(&measured, mib_field_index("t13").unwrap()),
        MibPickKind::Part
    );
    let unmeasured = part_break_mib(999, 7);
    assert_eq!(
        mhnx_core::hbedit::mib_pick_kind(&unmeasured, mib_field_index("t13").unwrap()),
        MibPickKind::None,
        "a monster no shipped quest breaks has no list to offer"
    );
}

#[test]
fn a_part_step_walks_the_measured_parts_and_never_guesses() {
    let mut ed = Editor::new();
    ed.screen = Screen::Mib;
    ed.mib_bytes = part_break_mib(80, 7);
    ed.mib_cursor = mib_field_index("t13").expect("t13");

    // ディノバルド has two measured parts: 尻尾 (7) and 頭部と背中 (12).
    assert_eq!(quest_part_choices(80).len(), 2);
    assert_eq!(ed.step_mib_part(ed.mib_cursor, true), PartStep::Stepped);
    assert_eq!(get_u16(ed.mib_bytes(), "t13"), 12);
    assert!(ed.message.contains("頭部と背中"), "{}", ed.message);
    assert!(ed.message.contains("実測"), "{}", ed.message);
    assert!(ed.dirty);
    // …and wraps.
    assert_eq!(ed.step_mib_part(ed.mib_cursor, true), PartStep::Stepped);
    assert_eq!(get_u16(ed.mib_bytes(), "t13"), 7);
    assert_eq!(ed.step_mib_part(ed.mib_cursor, false), PartStep::Stepped);
    assert_eq!(get_u16(ed.mib_bytes(), "t13"), 12);

    // The same step through the screen: [▶] on the 数 row.
    let mut ed = Editor::new();
    ed.screen = Screen::Mib;
    ed.mib_bytes = part_break_mib(80, 7);
    ed.mib_cursor = mib_field_index("t13").expect("t13");
    ed.input(Key::Right);
    assert_eq!(get_u16(ed.mib_bytes(), "t13"), 12, "{}", ed.message);
    assert!(ed.message.contains("頭部と背中"), "{}", ed.message);

    // A monster no shipped quest breaks: the code is not renamed and the
    // screen says so; the number still moves so the row stays editable.
    let mut ed = Editor::new();
    ed.screen = Screen::Mib;
    ed.mib_bytes = part_break_mib(999, 7);
    ed.mib_cursor = mib_field_index("t13").expect("t13");
    assert_eq!(ed.step_mib_part(ed.mib_cursor, true), PartStep::Unmeasured);
    assert_eq!(get_u16(ed.mib_bytes(), "t13"), 7, "the step must not invent a code");
    assert!(ed.message.contains("未確定"), "{}", ed.message);

    // A 狩猟/討伐 row keeps its count step: 12 -> 13, not a part change.
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 1);
    put_u16(&mut mib, "com0", 4115);
    put_u16(&mut mib, "t13", 12);
    let mut ed = Editor::new();
    ed.screen = Screen::Mib;
    ed.mib_bytes = mib;
    ed.mib_cursor = mib_field_index("t13").expect("t13");
    assert_eq!(ed.step_mib_part(ed.mib_cursor, true), PartStep::NotAPartRow);
    ed.input(Key::Right);
    assert_eq!(get_u16(ed.mib_bytes(), "t13"), 13);

    // The .mib screen prints the part name in the value column.
    let mut ed = Editor::new();
    ed.screen = Screen::Mib;
    ed.mib_bytes = part_break_mib(80, 7);
    ed.mib_cursor = mib_field_index("t13").expect("t13");
    assert!(
        ed.lines().iter().any(|l| l.text.contains("尻尾 (7)")),
        "the 数 row must show the part it names"
    );
}

#[test]
fn a_renzoku_row_points_at_the_mob_slots() {
    // 大連続 (q1010153) lists five monsters and stores id 0.
    let mut mib = empty_mib();
    put_u8(&mut mib, "tt1", 3);
    for (slot, id) in [(0usize, 83u16), (1, 81), (2, 82), (3, 80), (4, 43)] {
        let f = field(&format!("mob_slot[{}]", slot));
        mib[f.offset..f.offset + 2].copy_from_slice(&id.to_le_bytes());
    }

    assert_eq!(mib_mob_slot_ids(&mib), vec![83, 81, 82, 80, 43]);

    let mut ed = Editor::new();
    ed.screen = Screen::Mib;
    ed.mib_bytes = mib.clone();
    let note = ed.quest_mob_slot_note().expect("a 大連続 note");
    assert!(note.contains(RENZOKU_TARGET_NOTE), "{}", note);
    assert!(note.contains("出現モンスター"), "{}", note);
    assert!(note.contains("ディノバルド"), "{}", note);
    assert!(
        ed.lines().iter().any(|l| l.text.contains("mob_slot")),
        "the .mib screen must say where a 大連続 target lives"
    );

    // The id row itself says the same, so the 0 cannot read as "no monster".
    let info = mhnx_core::hbedit::quest_target_info(&mib, "com0").expect("com0");
    assert_eq!(info.kind_label, "大連続");
    assert!(info.row_title("ターゲット1 ID").contains("出現モンスター（mob_slot）"));
    assert!(info.label.contains("mob_slot"));

    // A quest that is not 大連続 has no note.
    let mut other = empty_mib();
    put_u8(&mut other, "tt1", 1);
    put_u16(&mut other, "com0", 80);
    let mut ed = Editor::new();
    ed.mib_bytes = other;
    assert!(ed.quest_mob_slot_note().is_none());

    // 大連続 with an empty slot list still says what the target would be.
    let mut empty = empty_mib();
    put_u8(&mut empty, "tt1", 3);
    let mut ed = Editor::new();
    ed.mib_bytes = empty;
    let note = ed.quest_mob_slot_note().expect("a note even with no monsters");
    assert!(note.contains("空"), "{}", note);
}

/// The text of the 部位破壊 requirement of one quest, as the quest writes it.
fn part_text(gmd: &Gmd, monster: &str) -> String {
    gmd.strings
        .iter()
        .find(|s| {
            s.contains(monster)
                && (s.contains("破壊") || s.contains("切断"))
                && !s.contains("狩猟")
                && !s.contains("討伐")
                && !s.contains("納品")
                && !s.contains("捕獲")
        })
        .map(|s| s.replace("\r\n", ""))
        .unwrap_or_default()
}

/// The part words of that text, split the way the quest writes them.
fn part_words(text: &str, monster: &str) -> Vec<String> {
    let mut t = text;
    if let Some(rest) = t.strip_prefix(monster) {
        t = rest;
    }
    // 「ディノバルドの尻尾切断」 -> 「尻尾切断」 (one 「の」 belongs to the name).
    if let Some((_head, tail)) = t.split_once('の') {
        if tail.contains("破壊") || tail.contains("切断") {
            t = tail;
        }
    }
    t.split(['と', '、'])
        .map(|p| p.replace("破壊", "").replace("切断", "").trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

#[test]
fn the_measured_part_table_matches_every_shipped_quest() {
    let dir = PathBuf::from(QUEST_SOURCE_QUEST_DUMP);
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
    let mut rows = 0usize;
    let mut monsters: Vec<u16> = Vec::new();
    let mut pairs: Vec<(u16, u16)> = Vec::new();
    let mut exceptions: Vec<String> = Vec::new();

    for path in &files {
        if path.extension().map(|e| e != "arc").unwrap_or(true) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(arc) = Arc::parse(&bytes) else { continue };
        let (Some(mib), Some(gmd_entry)) = (arc.by_ext("mib"), arc.by_ext("gmd")) else {
            continue;
        };
        let Ok(gmd) = Gmd::from_bytes(&gmd_entry.data) else {
            continue;
        };
        quests += 1;
        for (kind_key, id_key) in [("tt1", "com0"), ("tt2", "com1"), ("tt3", "com2")] {
            let kind = mib.data[field(kind_key).offset];
            if kind != 8 {
                continue;
            }
            rows += 1;
            let count_key = match kind_key {
                "tt1" => "t13",
                "tt2" => "t23",
                _ => "t33",
            };
            let id = get_u16(&mib.data, id_key);
            let pf = quest_part_field(&mib.data, count_key).expect("a 数 field");
            assert!(pf.is_part_break, "{} {}", path.display(), kind_key);
            assert_eq!(pf.monster, id);
            let slots = part_code_slots(pf.code)
                .unwrap_or_else(|| panic!("{}: code {} does not decode", path.display(), pf.code));
            assert_eq!(slots, pf.slots, "{} {}", path.display(), count_key);
            assert_eq!(
                part_code(slots[0], slots[1]),
                Some(pf.code),
                "{}: the codec must round trip",
                path.display()
            );
            if !monsters.contains(&id) {
                monsters.push(id);
            }
            if !pairs.contains(&(id, pf.code)) {
                pairs.push((id, pf.code));
            }
            // Every shipped 部位破壊 row is in the measured table.
            let label = quest_part_label(id, pf.code).unwrap_or_else(|| {
                panic!("{}: monster {} code {} is not in QUEST_PARTS", path.display(), id, pf.code)
            });
            let name = monster_name_of(id).unwrap_or("?");
            let text = part_text(&gmd, name);
            let words = part_words(&text, name);
            let wanted = slots.iter().filter(|&&s| s != 0).count();
            if words.len() == wanted {
                assert_eq!(
                    label,
                    words.join("と"),
                    "{} {}: the table must say what the quest says",
                    path.display(),
                    count_key
                );
            } else {
                // Two rows of the dump name fewer (or more) words than the code
                // has parts: アマツマガツチ writes 「両前脚」 for the pair (2,4),
                // and q1111001 is a story quest whose text is prose.
                exceptions.push(format!("{} {}", path.display(), count_key));
            }
        }
    }

    assert_eq!(quests, 141, "the dump holds 141 quests");
    assert_eq!(rows, 42, "the 部位破壊 rows of the dump");
    assert_eq!(monsters.len(), 34, "monsters with a 部位破壊 row");
    assert_eq!(pairs.len(), 40, "distinct (monster, code) pairs");
    assert_eq!(
        QUEST_PARTS.len(),
        40,
        "one table row per measured pair, no duplicates"
    );
    assert_eq!(
        QUEST_PARTS.iter().map(|p| p.count as usize).sum::<usize>(),
        rows,
        "the counts add up to the rows they were read from"
    );
    for p in QUEST_PARTS {
        assert_eq!(
            part_code_slots(p.code),
            Some(p.slots),
            "monster {} code {}",
            p.monster,
            p.code
        );
        assert!(!p.label.is_empty());
    }
    assert_eq!(exceptions.len(), 2, "{:?}", exceptions);
}
