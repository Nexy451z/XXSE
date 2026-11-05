//! クエスト改造 (Screen::QuestEdit): one screen over the files a quest owns.
//!
//! Every test builds its own archive out of the same byte layouts the shipped
//! quests use - a 249 byte .mib, a .gmd, a 184 byte .rem, a 168 byte .sup and
//! 32 byte .sem placements - so the screen is checked against the formats and
//! not against one file that happens to sit on this machine.
//!
//! The point of the screen is that it has no offsets of its own: each test that
//! edits also reads the byte back out of the archive the file screen would
//! write, so the two can never disagree.

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::hbedit::{
    item_name_or_none, monster_name_of, Editor, Key, QuestMonRow, QuestPick, Screen,
    QUEST_MON_AREA,
    QUEST_MON_HP, QUEST_MON_SLOTS, QUEST_TAB_BASIC, QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS,
    QUEST_TAB_SUPPLIES,
};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};

const HASH_SEM: u32 = 0x2553_701D;
const HASH_REM: u32 = 0x5B3C_302D;
const HASH_SUP: u32 = 0x5453_9AEE;
const HASH_QDL: u32 = 0x00E0_BB1C;
const HASH_MIB: u32 = 0x1BBF_D18E;
const HASH_GMD: u32 = 0x242B_B29A;

/// A .mib with the quest id, the time limit and three monster slots filled in.
fn mib_bytes(id: u32, minutes: u8) -> Vec<u8> {
    let mut mib = vec![0u8; MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
    mib[0x16] = minutes;
    // slot 0 / slot 1 hold monsters, slot 2 is empty.
    let slots: [(u16, u8, u8, u8, u16, u8); 3] = [
        (85, 5, 5, 5, 300, 1),
        (32, 9, 3, 3, 100, 2),
        (0, 0, 0, 0, 0, 0),
    ];
    for (k, (mid, hp, atk, def, size, count)) in slots.iter().enumerate() {
        let o = 100 + 13 * k;
        mib[o..o + 2].copy_from_slice(&mid.to_le_bytes());
        mib[o + 3] = 1;
        mib[o + 4] = *count;
        mib[o + 5] = *hp;
        mib[o + 6] = *atk;
        mib[o + 7] = *def;
        mib[o + 9..o + 11].copy_from_slice(&size.to_le_bytes());
        mib[o + 12] = 1;
    }
    mib
}

fn gmd_bytes() -> Vec<u8> {
    Gmd {
        version: GMD_VERSION,
        tag: GMD_TAG,
        key: String::from("questData_1010165"),
        strings: vec![
            String::from("テストクエスト"),
            String::from("受付嬢"),
            String::from("依頼の内容"),
            String::from("リオレウス"),
            String::from("リオレウス1頭の狩猟"),
            String::from("3回力尽きる"),
            String::from("サブ依頼"),
        ],
    }
    .to_bytes()
}

/// 184 byte reward table: one group of three rows whose chances add up to 100.
fn rem_bytes() -> Vec<u8> {
    let mut v = vec![0u8; 184];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8] = 1;
    v[9] = 3;
    let rows: [[u8; 4]; 4] = [
        [0x63, 0x01, 1, 50],
        [0x55, 0x03, 2, 30],
        [0xB4, 0x06, 1, 20],
        [0, 0, 0, 0xFF],
    ];
    for (i, r) in rows.iter().enumerate() {
        v[24 + i * 4..24 + i * 4 + 4].copy_from_slice(r);
    }
    v
}

/// 168 byte supply box: two stacks, a gap, then one more - a .sup keeps the
/// records after an empty slot, which is why the editor stores positions.
fn sup_bytes() -> Vec<u8> {
    let mut v = vec![0u8; 168];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8..12].copy_from_slice(&[0xB4, 0x06, 3, 0]);
    v[12..16].copy_from_slice(&[0xB6, 0x06, 2, 0]);
    v[24..28].copy_from_slice(&[0x1C, 0x07, 1, 0]);
    v
}

/// One 32 byte .sem: wave +0x08, area +0x0C, x/y/z/w +0x10..+0x20.
fn sem_bytes(area: u32, x: f32, y: f32, z: f32, w: f32, wave: u32) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8..12].copy_from_slice(&wave.to_le_bytes());
    v[0x0C..0x10].copy_from_slice(&area.to_le_bytes());
    v[0x10..0x14].copy_from_slice(&x.to_le_bytes());
    v[0x14..0x18].copy_from_slice(&y.to_le_bytes());
    v[0x18..0x1C].copy_from_slice(&z.to_le_bytes());
    v[0x1C..0x20].copy_from_slice(&w.to_le_bytes());
    v
}

fn entry(name: &str, type_hash: u32, data: Vec<u8>) -> ArcEntry {
    ArcEntry {
        name: String::from(name),
        type_hash,
        data,
        original: None,
        original_offset: None,
    }
}

/// A whole quest, in the shape q1010165 has.
fn quest_arc(id: u32, minutes: u8) -> Arc {
    Arc {
        version: ARC_VERSION,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry(
                "quest\\boss\\setEmMain\\b_m00em000_00",
                HASH_SEM,
                sem_bytes(0, 0.0, 0.0, 0.0, 0.0, 0),
            ),
            entry(
                "quest\\boss\\setEmMain\\b_m15em010_00",
                HASH_SEM,
                sem_bytes(1, 180.0, 23.0, -150.0, 90.0, 0),
            ),
            entry(
                "quest\\boss\\setEmMain\\b_m15em046_00",
                HASH_SEM,
                sem_bytes(2, 147.0, 0.0, 0.0, 45.0, 1),
            ),
            entry("quest\\rem\\rem_000000", HASH_REM, vec![0u8; 184]),
            entry("quest\\rem\\rem_620114", HASH_REM, rem_bytes()),
            entry("quest\\supp\\supp_1010165", HASH_SUP, sup_bytes()),
            entry("quest\\questLink\\questLink_1010165", HASH_QDL, vec![0u8; 308]),
            entry(
                "quest\\questData\\questData_1010165_jpn",
                HASH_GMD,
                gmd_bytes(),
            ),
            entry(
                "quest\\questData\\questData_1010165",
                HASH_MIB,
                mib_bytes(id, minutes),
            ),
        ],
    }
}

/// Open a quest through the byte path the console uses.
fn open_quest(id: u32, minutes: u8) -> Editor {
    let mut ed = Editor::new();
    let bytes = quest_arc(id, minutes).to_bytes().expect("the archive rebuilds");
    ed.open_arc_bytes("sdmc:/quest/q1010165.arc", &bytes);
    ed.enter_quest_edit();
    assert_eq!(ed.screen, Screen::QuestEdit, "{}", ed.message);
    ed
}

/// The bytes of one archive entry, by name suffix.
fn entry_bytes(ed: &Editor, suffix: &str) -> Vec<u8> {
    ed.arc
        .as_ref()
        .expect("an archive")
        .entries
        .iter()
        .find(|e| e.name.ends_with(suffix))
        .map(|e| e.data.clone())
        .unwrap_or_else(|| panic!("no entry ending in {}", suffix))
}

/// The label the screen must print for a monster id.
fn monster_label(id: u16) -> String {
    match monster_name_of(id) {
        Some(name) if !name.is_empty() => format!("{} 0x{:X}", name, id),
        _ => format!("0x{:X}（名前不明）", id),
    }
}

/// The label the screen must print for an item id.
fn item_label(id: u16) -> String {
    match item_name_or_none(id) {
        Some(name) => format!("{} 0x{:X}", name, id),
        None => format!("0x{:X}（名前不明）", id),
    }
}

#[test]
fn one_screen_reads_every_file_of_the_quest_by_itself() {
    let mut ed = open_quest(1_010_165, 50);
    assert!(ed.quest_is_open());
    assert_eq!(ed.quest_name(), "テストクエスト");
    assert_eq!(ed.quest_id_hex(), "0xF69F5");
    assert!(ed.message.contains("クエスト改造"), "{}", ed.message);

    // 基本: three .gmd strings and the .mib time limit, in one list.
    let basic = ed.quest_basic_rows();
    assert_eq!(basic.len(), 4);
    assert_eq!(basic[0].label, "クエスト名");
    assert_eq!(basic[0].value, "テストクエスト");
    assert_eq!(basic[1].label, "依頼主");
    assert_eq!(basic[1].value, "受付嬢");
    assert_eq!(basic[2].label, "目的（成功条件）");
    assert_eq!(basic[2].value, "リオレウス1頭の狩猟");
    assert_eq!(basic[3].label, "制限時間");
    assert_eq!(basic[3].value, "50 分");

    // モンスター: five slots, each named with the hex id beside the name.
    let monsters = ed.quest_monsters();
    assert_eq!(monsters.len(), QUEST_MON_SLOTS);
    assert_eq!(monsters[0].id, 85);
    assert_eq!(monsters[0].label, monster_label(85));
    assert!(monsters[0].label.ends_with(" 0x55"), "{}", monsters[0].label);
    assert_eq!((monsters[0].hp, monsters[0].atk, monsters[0].def), (5, 5, 5));
    assert_eq!((monsters[0].size, monsters[0].count), (300, 1));
    assert_eq!(monsters[1].id, 32);
    assert_eq!(monsters[1].label, monster_label(32));
    assert_eq!(monsters[2].id, 0, "slot 3 is empty");
    assert_eq!(monsters[2].label, "なし", "an empty slot is not a name hunt");
    // The .sem of the same slot supplies the area and the wave.
    assert_eq!(monsters[0].area, Some(1));
    assert_eq!(monsters[0].wave, Some(0));
    assert_eq!(monsters[1].area, Some(2));
    assert_eq!(monsters[1].wave, Some(1));
    assert_eq!(monsters[3].area, None, "a slot with no .sem has no area");

    // 報酬: the real table, with its group total; the dummy stays out because
    // it holds no rows at all.
    let rewards = ed.quest_rewards();
    assert_eq!(rewards.len(), 3);
    assert!(rewards.iter().all(|r| r.file == "rem_620114"));
    assert_eq!(rewards[0].item, 0x163);
    assert_eq!(rewards[0].count, 1);
    assert_eq!(rewards[0].chance, 50);
    assert_eq!(rewards[0].label, item_label(0x163));
    assert_eq!(
        (rewards[0].group, rewards[0].group_len, rewards[0].group_total),
        (0, 3, 100)
    );
    assert!(!rewards[0].dummy);

    // 支給品: every record of the .sup, including the one after the gap.
    let supplies = ed.quest_supplies();
    assert_eq!(supplies.len(), 3, "a .sup keeps records after an empty slot");
    assert_eq!(supplies[0].count, 3);
    assert_eq!(supplies[2].count, 1);
    assert_eq!(supplies[0].file, "supp_1010165");
    assert_eq!(supplies[0].label, item_label(0x6B4));

    // The tabs are what L / R walks, and each tab counts its own rows plus
    // the 詳細 row every tab ends with.
    assert_eq!(ed.quest_edit_tab, QUEST_TAB_BASIC);
    assert_eq!(ed.quest_value_rows(), 4);
    assert_eq!(ed.quest_row_count(), 5);
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    assert_eq!(ed.quest_row_count(), QUEST_MON_SLOTS + 1);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    assert_eq!(ed.quest_row_count(), 4);
    ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    assert_eq!(ed.quest_row_count(), 4);
}

#[test]
fn a_name_that_cannot_be_looked_up_falls_back_to_the_id_and_says_so() {
    let mut arc = quest_arc(1_010_165, 50);
    let mib = arc
        .entries
        .iter_mut()
        .find(|e| e.type_hash == HASH_MIB)
        .expect("a .mib");
    // 0xF00D is in no monster table.
    mib.data[100..102].copy_from_slice(&0xF00Du16.to_le_bytes());
    // 0xF00D is in no item table either.
    let rem = arc
        .entries
        .iter_mut()
        .find(|e| e.name.ends_with("rem_620114"))
        .expect("a reward table");
    rem.data[24..26].copy_from_slice(&0xF00Du16.to_le_bytes());

    let bytes = arc.to_bytes().expect("the archive rebuilds");
    let mut ed = Editor::new();
    ed.open_arc_bytes("sdmc:/quest/q1010165.arc", &bytes);
    ed.enter_quest_edit();

    let monsters = ed.quest_monsters();
    assert_eq!(monsters[0].id, 0xF00D);
    assert_eq!(monsters[0].name, "");
    assert_eq!(monsters[0].label, "0xF00D（名前不明）");
    assert_eq!(monster_label(0xF00D), monsters[0].label);

    let rewards = ed.quest_rewards();
    assert_eq!(rewards[0].item, 0xF00D);
    assert_eq!(rewards[0].label, "0xF00D（名前不明）");
    assert_eq!(item_label(0xF00D), rewards[0].label);
}

#[test]
fn the_monster_tab_steps_the_value_it_is_on_and_no_other_byte() {
    let mut ed = open_quest(1_010_165, 50);
    ed.quest_set_tab(QUEST_TAB_MONSTERS);

    // The area is the row the screen leads with: [-] selects it, [◀/▶] steps
    // the .sem byte the .sem screen steps.
    ed.quest_edit_field = QUEST_MON_AREA;
    assert_eq!(ed.quest_field_label(), "エリア番号");
    let before = ed.quest_monsters()[0].area;
    assert!(ed.quest_adjust(1));
    let after = ed.quest_monsters()[0].area;
    assert_eq!(after, before.map(|a| a + 1));
    assert!(ed.message.contains("初期エリア番号"), "{}", ed.message);
    // Exactly the four area bytes moved, and nothing else in that .sem.
    let sem = entry_bytes(&ed, "b_m15em010_00");
    assert_eq!(&sem[0x0C..0x10], &2u32.to_le_bytes());
    assert_eq!(&sem[0x10..0x14], &180.0f32.to_le_bytes(), "X must not move");
    assert_eq!(&sem[0x1C..0x20], &90.0f32.to_le_bytes(), "向き must not move");

    // The .mib values go through the .mib writer: 体力 +5, 攻撃力 +6, 防御力 +7,
    // サイズ% +9 (u16), 出現数 +4.
    ed.quest_edit_field = QUEST_MON_HP;
    assert!(ed.quest_adjust(3));
    assert_eq!(ed.quest_monsters()[0].hp, 8);
    ed.store_entry();
    let mib = entry_bytes(&ed, "questData_1010165");
    assert_eq!(mib[100 + 5], 8, "体力 is the byte the .mib screen steps");
    assert_eq!(mib[100 + 4], 1, "出現数 must not move");
    assert_eq!(mib[100 + 9..100 + 11], 300u16.to_le_bytes(), "サイズ must not move");

    // A value that is clamped says the number it stopped at.
    ed.quest_edit_field = 3; // サイズ%
    assert!(ed.quest_adjust(1000));
    assert_eq!(ed.quest_monsters()[0].size, 500);
    assert!(ed.message.contains("500"), "{}", ed.message);
}

#[test]
fn the_detail_rows_open_under_their_slot_and_step_the_placement() {
    let mut ed = open_quest(1_010_165, 50);
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    assert_eq!(ed.quest_row_count(), QUEST_MON_SLOTS + 1);

    ed.quest_edit_cursor = 0;
    ed.input(Key::Y);
    assert!(ed.quest_edit_open[0], "[Y] opens the ▸詳細 rows");
    let rows = ed.quest_mon_rows();
    assert_eq!(rows.len(), QUEST_MON_SLOTS + 5);
    assert_eq!(rows[1], QuestMonRow::Detail(0, 1));

    // The five rows are the five numbers of the .sem, in the .sem screen order.
    assert_eq!(ed.quest_monster_detail(0, 1).0, "X座標");
    assert_eq!(ed.quest_monster_detail(0, 1).1, "180.0");
    assert_eq!(ed.quest_monster_detail(0, 2).1, "23.0");
    assert_eq!(ed.quest_monster_detail(0, 3).1, "-150.0");
    assert_eq!(ed.quest_monster_detail(0, 4).1, "90.0°");
    assert_eq!(ed.quest_monster_detail(0, 5).1, "Wave 0");

    // Stepping a detail row writes that coordinate and only that one.
    ed.quest_edit_cursor = 1;
    assert!(ed.quest_adjust(10));
    assert_eq!(ed.quest_monster_detail(0, 1).1, "190.0");
    assert_eq!(ed.quest_monster_detail(0, 2).1, "23.0");
    let sem = entry_bytes(&ed, "b_m15em010_00");
    assert_eq!(&sem[0x10..0x14], &190.0f32.to_le_bytes());

    // The last detail row is 出現ウェーブ, the .sem byte at +0x08.
    ed.quest_edit_cursor = 5;
    assert!(ed.quest_adjust(1));
    assert_eq!(ed.quest_monster_detail(0, 5).1, "Wave 1");
    let sem = entry_bytes(&ed, "b_m15em010_00");
    assert_eq!(&sem[0x08..0x0C], &1u32.to_le_bytes());

    // Closing it puts the list back the way it was.
    ed.quest_edit_cursor = 0;
    ed.input(Key::Y);
    assert!(!ed.quest_edit_open[0]);
    assert_eq!(ed.quest_row_count(), QUEST_MON_SLOTS + 1);

    // A slot with no .sem says so instead of inventing a zero coordinate.
    assert_eq!(ed.quest_monster_detail(4, 1).1, "—");
    assert!(!ed.adjust_sem_placement(4, 0, 1));
    assert!(ed.message.contains(".sem がありません"), "{}", ed.message);
}

#[test]
fn a_reward_row_steps_its_count_and_its_chance_through_the_rem_writer() {
    let mut ed = open_quest(1_010_165, 50);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    ed.quest_edit_cursor = 0;
    assert!(!ed.quest_edit_chance, "個数 is the field the tab starts on");

    assert!(ed.quest_adjust(2));
    assert_eq!(ed.quest_rewards()[0].count, 3);
    let rem = entry_bytes(&ed, "rem_620114");
    assert_eq!(rem[24 + 2], 3, "the count byte of row 0");
    assert_eq!(rem[24 + 3], 50, "the chance byte must not move");

    // [-] moves the same key pair to 確率%, like the .rem screen does.
    ed.quest_cycle_field();
    assert!(ed.quest_edit_chance);
    assert!(ed.quest_adjust(-10));
    assert_eq!(ed.quest_rewards()[0].chance, 40);
    let rewards = ed.quest_rewards();
    assert_eq!(rewards[0].group_total, 90, "the group total follows the edit");
    assert!(ed.message.contains("90"), "{}", ed.message);
    let rem = entry_bytes(&ed, "rem_620114");
    assert_eq!(rem[24 + 3], 40);
    assert_eq!(rem[24 + 2], 3, "the count must not move back");

    // A chance cannot go past 100, and the screen says the number it wrote.
    assert!(ed.quest_adjust(100));
    assert_eq!(ed.quest_rewards()[0].chance, 100);
}

#[test]
fn the_basic_tab_asks_for_the_keyboard_and_writes_the_gmd_text() {
    let mut ed = open_quest(1_010_165, 50);
    ed.quest_edit_cursor = 0;
    ed.input(Key::A);
    // The same request the .gmd screen makes: (string index, current text).
    assert_eq!(
        ed.take_text_request(),
        Some((0, String::from("テストクエスト")))
    );
    ed.apply_text(0, String::from("改造クエスト"));
    assert_eq!(ed.quest_name(), "改造クエスト");
    assert_eq!(ed.quest_basic_rows()[0].value, "改造クエスト");
    assert!(ed.dirty);

    // The write reaches the archive through the same write-back the file
    // screens use, even though the cursor is on the basic tab and not on the
    // .gmd entry.
    ed.store_entry();
    let gmd = entry_bytes(&ed, "questData_1010165_jpn");
    let parsed = Gmd::from_bytes(&gmd).expect("the .gmd still parses");
    assert_eq!(parsed.title(), Some("改造クエスト"));

    // 依頼主 and 目的 are the other two text rows.
    ed.quest_edit_cursor = 1;
    ed.input(Key::A);
    assert_eq!(ed.take_text_request().map(|t| t.0), Some(1));
    ed.quest_edit_cursor = 2;
    ed.input(Key::A);
    assert_eq!(ed.take_text_request().map(|t| t.0), Some(4));

    // 制限時間 is not text: [A] says which key does change it.
    ed.quest_edit_cursor = 3;
    ed.input(Key::A);
    assert_eq!(ed.take_text_request(), None);
    assert!(ed.message.contains("制限時間"), "{}", ed.message);
    assert!(ed.quest_adjust(10));
    assert_eq!(ed.quest_time_limit(), 60);
    ed.store_entry();
    assert_eq!(entry_bytes(&ed, "questData_1010165")[0x16], 60);
}

#[test]
fn a_supply_row_has_only_a_count_to_step() {
    let mut ed = open_quest(1_010_165, 50);
    ed.quest_set_tab(QUEST_TAB_SUPPLIES);
    ed.quest_edit_cursor = 0;
    assert!(ed.quest_adjust(1));
    assert_eq!(ed.quest_supplies()[0].count, 4);
    assert_eq!(entry_bytes(&ed, "supp_1010165")[8 + 2], 4);
    // [-] cannot switch to a chance column a .sup does not have.
    ed.quest_cycle_field();
    assert!(!ed.quest_edit_chance);
    assert!(ed.message.contains("確率はありません"), "{}", ed.message);
    assert!(ed.quest_adjust(-1));
    assert_eq!(ed.quest_supplies()[0].count, 3);
}

#[test]
fn the_item_chooser_writes_back_into_the_row_the_screen_named() {
    let mut ed = open_quest(1_010_165, 50);
    ed.quest_set_tab(QUEST_TAB_REWARDS);
    ed.quest_edit_cursor = 0;
    // [A] opens the same item chooser the .rem screen opens, in the mode that
    // writes into the archive entry instead of the item box.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);
    assert_eq!(ed.item_pick_mode, 1);
    assert_eq!(ed.entry_item_row, 0);
    let first = ed.item_pick_matches()[0];
    ed.input(Key::A);
    assert_eq!(ed.quest_rewards()[0].item, first);
    assert_eq!(ed.quest_rewards()[0].label, item_label(first));
    assert_eq!(
        entry_bytes(&ed, "rem_620114")[24..26],
        first.to_le_bytes()[..]
    );
}

#[test]
fn leaving_for_the_file_screens_keeps_the_edit_and_the_detail_stays_one_press() {
    let mut ed = open_quest(1_010_165, 50);
    ed.quest_edit_cursor = 0;
    ed.input(Key::A);
    ed.take_text_request();
    ed.apply_text(0, String::from("保存テスト"));

    // 詳細 is the archive entry list, one screen, with the cursor on the .mib.
    ed.screen = Screen::ArcEntries;
    ed.cursor = ed.entry;
    // Coming back goes through the same entry point, and the edit is still
    // there because the working copy was flushed on the way out.
    ed.enter_quest_edit();
    assert_eq!(ed.screen, Screen::QuestEdit);
    assert_eq!(ed.quest_name(), "保存テスト");
    let gmd = entry_bytes(&ed, "questData_1010165_jpn");
    assert_eq!(
        Gmd::from_bytes(&gmd).expect("parses").title(),
        Some("保存テスト")
    );

    // [B] leaves the screen and flushes: the file is still only written on [X].
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcList);
    assert_eq!(
        Gmd::from_bytes(&entry_bytes(&ed, "questData_1010165_jpn"))
            .expect("parses")
            .title(),
        Some("保存テスト")
    );
}

#[test]
fn the_last_row_of_every_tab_is_the_detail_button() {
    let mut ed = open_quest(1_010_165, 50);
    // 基本: four values, then 詳細.
    assert_eq!(ed.quest_value_rows(), 4);
    assert!(!ed.quest_on_detail_row());
    ed.quest_edit_cursor = ed.quest_value_rows();
    assert!(ed.quest_on_detail_row());
    assert_eq!(ed.quest_activate(), QuestPick::Detail);
    // Nothing on it takes a step, and the screen says which key it does take.
    assert!(!ed.quest_adjust(1));
    assert!(ed.message.contains("詳細行"), "{}", ed.message);

    // The same last row on the tabs that hold a list.
    for tab in [QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, QUEST_TAB_SUPPLIES] {
        ed.quest_set_tab(tab);
        let last = ed.quest_row_count() - 1;
        assert_eq!(last, ed.quest_value_rows(), "tab {}", tab);
        ed.quest_edit_cursor = last;
        assert!(ed.quest_on_detail_row(), "tab {}", tab);
        assert_eq!(ed.quest_activate(), QuestPick::Detail, "tab {}", tab);
    }

    // On the monster tab it is still not a monster row: [Y] cannot open it.
    ed.quest_set_tab(QUEST_TAB_MONSTERS);
    ed.quest_edit_cursor = ed.quest_row_count() - 1;
    ed.quest_toggle_detail();
    assert!(ed.message.contains("詳細行"), "{}", ed.message);
    assert!(!ed.quest_edit_open.iter().any(|o| *o));

    // The console fallback prints it as its last line, so the ASCII route has
    // the same button the canvas draws.
    ed.quest_set_tab(QUEST_TAB_BASIC);
    let rows = ed.quest_console_rows();
    assert_eq!(rows.len(), ed.quest_row_count());
    assert!(rows.last().unwrap().contains("詳細"), "{:?}", rows);
}