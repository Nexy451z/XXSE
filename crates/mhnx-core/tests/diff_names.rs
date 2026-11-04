//! What the review calls the bytes it changes.
//!
//! The audit's complaint was the same for six rounds: a change the layout table
//! did not name was printed as 「その他」. The save map names 183 stretches of
//! the save, so the review reads it first and prints its names; these tests pin
//! that down on an empty save (deterministic) and on the sample save on this
//! machine (the edits the tabs really make).
//!
//! The tests that need the sample save are skipped, not failed, when it is
//! absent - the same convention tests/real_data.rs uses.

use mhnx_core::hbedit::{self, SaveDiff};
use mhnx_core::save::{self, Save, FIRST_SLOT_BASE, OFF_EQUIPMENT_BOX};

/// The console body and the window body, joined: every line either screen
/// draws for this difference.
fn bodies(diff: &SaveDiff) -> String {
    let mut text = diff.lines(64).join("\n");
    text.push('\n');
    text.push_str(&diff.window_lines(64).join("\n"));
    text
}

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

/// A byte of a character slot: the first byte of the equipment box.
fn equipment_box_byte() -> usize {
    FIRST_SLOT_BASE as usize + OFF_EQUIPMENT_BOX
}

/// One byte the layout has an entry for but the map names more narrowly.
#[test]
fn the_save_map_names_the_bytes_the_layout_leaves_out() {
    let old = save::empty_switch_save();
    let mut new = old.clone();
    // 依頼フラグ sit between the named parts: the layout has no entry for them
    // (the value walk looks for a 「その他」 range to know when to ask), the save
    // map has a row. Before the map was read, this was 「スロット1 その他」.
    assert!(mhnx_core::requests::set(&mut new, 0, 3, true));

    let diff = hbedit::diff_save(&old.to_bytes(), &new.to_bytes());
    assert_eq!(diff.changed, 1, "{}", bodies(&diff));
    assert_eq!(diff.ranges.len(), 1, "{:?}", diff.ranges);
    let range = &diff.ranges[0];
    assert_eq!(range.slot, Some(0));
    // The layout still has nothing there: that is what keeps the request walk
    // asking every slot (see diffvalues::diff_values).
    assert_eq!(range.part, "その他");
    assert_eq!(range.name, "依頼フラグ");
    assert_eq!(range.head(), "スロット1 依頼フラグ");
    // The feature line names it too, instead of echoing 「その他」.
    assert!(
        diff.summary.iter().any(|line| line == "スロット1: 依頼フラグ"),
        "{:?}",
        diff.summary
    );
    assert!(!bodies(&diff).contains("その他"), "{}", bodies(&diff));
}

/// "スロット1 装備ボックス +0x33C" is "スロット1 装備ボックス #24" now: the save
/// map knows the box is 2,000 records of 36 bytes, so the review can say which
/// one, and every wording says the same thing.
#[test]
fn a_record_is_named_by_its_number_in_every_wording() {
    let save = save::empty_switch_save();
    let old = save.to_bytes();
    let mut new_save = save.clone();
    // +0x33C is 23 whole records: record #24, counted from one the way the
    // value lines count ("装備ボックス #24 レベル").
    let at = equipment_box_byte() + 23 * save::EQUIPMENT_RECORD;
    new_save.body_mut()[at..at + save::EQUIPMENT_RECORD].fill(0x11);
    let new = new_save.to_bytes();

    let diff = hbedit::diff_save(&old, &new);
    assert_eq!(diff.ranges.len(), 1, "{:?}", diff.ranges);
    let range = &diff.ranges[0];
    assert_eq!(range.slot, Some(0));
    assert_eq!(range.part, "装備ボックス");
    assert_eq!(range.name, "装備ボックス #24");
    assert_eq!(range.rel, 23 * save::EQUIPMENT_RECORD);
    assert_eq!(range.records, Some((1, "枠")));
    // The three wordings the screens print: 変更一覧's 領域 column (head), the
    // console review (line) and the window / snapshot comparison (window_line).
    assert_eq!(range.head(), "スロット1 装備ボックス #24");
    assert_eq!(
        range.line(),
        format!("スロット1 装備ボックス #24 {} バイト（1 枠）", save::EQUIPMENT_RECORD)
    );
    assert_eq!(
        range.window_line(),
        format!(
            "スロット1 装備ボックス #24 +0x{:X} 付近 {} バイト（1 枠）",
            23 * save::EQUIPMENT_RECORD,
            save::EQUIPMENT_RECORD
        )
    );
    // And both review bodies carry the very same name.
    for body in [diff.lines(REVIEW_SHOWN), diff.window_lines(REVIEW_SHOWN)] {
        assert!(
            body.iter().any(|line| line.contains("装備ボックス #24")),
            "{:?}",
            body
        );
    }

    // A stretch that crosses a record boundary names both ends rather than
    // pretending it is one record.
    let mut two = save.clone();
    let at2 = equipment_box_byte() + 23 * save::EQUIPMENT_RECORD;
    two.body_mut()[at2..at2 + 2 * save::EQUIPMENT_RECORD].fill(0x22);
    let diff = hbedit::diff_save(&old, &two.to_bytes());
    assert_eq!(diff.ranges.len(), 1, "{:?}", diff.ranges);
    assert_eq!(diff.ranges[0].name, "装備ボックス #24〜#25");
    assert_eq!(diff.ranges[0].records, Some((2, "枠")));
}

/// How many ranges a review names before it counts the rest.
const REVIEW_SHOWN: usize = 64;

/// A byte neither table names still says so: the fallback is what the console
/// build uses for everything, so it has to keep working.
#[test]
fn a_byte_no_table_names_is_still_reported_as_other() {
    let save = save::empty_switch_save();
    let old = save.to_bytes();
    let mut new_save = save.clone();
    // The three bytes of a slot after the last shoutout: no writer names them,
    // and the map stops where the writers stop.
    let at = FIRST_SLOT_BASE as usize + save::SLOT_STRIDE - 1;
    new_save.body_mut()[at] = 0x5A;
    let diff = hbedit::diff_save(&old, &new_save.to_bytes());
    assert_eq!(diff.ranges.len(), 1, "{:?}", diff.ranges);
    let range = &diff.ranges[0];
    assert_eq!(range.part, "その他");
    assert_eq!(range.name, "その他");
    assert_eq!(range.head(), "その他");
    assert!(bodies(&diff).contains("その他"), "{}", bodies(&diff));
}

/// The edits the tabs make on the sample save: none of them may fall through
/// to 「その他」 any more. One edit per area the audit listed - 護石, 装備,
/// アイテム, ポーチ, オトモ, 狩猟記録, ギルド - and each is made through the
/// same function the screen calls.
#[test]
fn the_sample_saves_edits_are_named_by_the_save_map() {
    let Some(base) = sample() else {
        return;
    };
    let old = base.to_bytes();
    let edits: [(&str, fn(&mut Save)); 7] = [
        ("護石", edit_talisman),
        ("装備", edit_equipment),
        ("アイテム", edit_item_box),
        ("ポーチ", edit_pouch),
        ("オトモ", edit_palico),
        ("狩猟記録", edit_hunt_record),
        ("ギルド", edit_guild_card),
    ];
    for (what, edit) in edits {
        let mut save = base.clone();
        edit(&mut save);
        let new = save.to_bytes();
        assert_ne!(old, new, "{}: the edit wrote nothing", what);
        let diff = hbedit::diff_save(&old, &new);
        assert!(!diff.is_empty(), "{}", what);
        let unnamed: Vec<&str> = diff
            .ranges
            .iter()
            .filter(|range| range.name == "その他")
            .map(|range| range.name.as_str())
            .collect();
        assert!(
            unnamed.is_empty(),
            "{}: {} rows fell through to その他: {:?}",
            what,
            unnamed.len(),
            diff.ranges
        );
        let text = bodies(&diff);
        assert!(!text.contains("その他"), "{}:\n{}", what, text);
        eprintln!(
            "{}: {}",
            what,
            diff.ranges
                .iter()
                .map(|range| range.window_line())
                .collect::<Vec<String>>()
                .join(" / ")
        );
    }
}

/// The three edits the report shows side by side: one equipment record, one
/// talisman, one hunt record, all in the same save.
#[test]
fn three_edits_of_the_sample_save_are_listed_by_name() {
    let Some(mut save) = sample() else {
        return;
    };
    let old = save.to_bytes();
    edit_equipment(&mut save);
    edit_talisman(&mut save);
    edit_hunt_record(&mut save);
    let diff = hbedit::diff_save(&old, &save.to_bytes());
    assert!(diff.changed > 0);
    let text = diff.window_lines(64).join("\n");
    assert!(!text.contains("その他"), "{}", text);
    eprintln!("{}", text);
}

// ---------------------------------------------------------------- the edits

/// The first talisman of slot 1, one skill point higher.
fn edit_talisman(save: &mut Save) {
    let charms = mhnx_core::charm::decode_box(save.equipment_box(0).expect("slot 0"));
    let mut charm = *charms.first().expect("a talisman in the sample save");
    charm.points1 = charm.points1.wrapping_add(1);
    let box_bytes = save.equipment_box_mut(0).expect("slot 0");
    assert!(
        mhnx_core::charm::update_charm(box_bytes, &charm),
        "the talisman writes"
    );
}

/// The first equipment record of slot 1, one level higher.
fn edit_equipment(save: &mut Save) {
    let index = equipment_record(save);
    let box_bytes = save.equipment_box_mut(0).expect("slot 0");
    let at = index * save::EQUIPMENT_RECORD;
    let record = &mut box_bytes[at..at + save::EQUIPMENT_RECORD];
    let level = mhnx_core::equip::decode_record(index, record)
        .expect("a record")
        .level;
    assert!(
        mhnx_core::equip::set_level(record, if level >= 32 { level - 1 } else { level + 1 }),
        "the level writes"
    );
}

/// The record index of a real piece of equipment, not a talisman.
fn equipment_record(save: &Save) -> usize {
    let box_bytes = save.equipment_box(0).expect("slot 0");
    (0..save::TOTAL_EQUIP_SLOTS)
        .find(|index| {
            let at = index * save::EQUIPMENT_RECORD;
            let record = &box_bytes[at..at + save::EQUIPMENT_RECORD];
            !mhnx_core::equip::record_is_blank(record)
                && !mhnx_core::charm::is_talisman(record)
        })
        .expect("a piece of equipment in the sample save")
}

/// One stack in the item box, one higher (or one lower at the cap).
fn edit_item_box(save: &mut Save) {
    let mut items = save.read_item_box(0).expect("the item box");
    let at = items
        .iter()
        .position(|slot| !slot.is_blank())
        .expect("an item in the box");
    let count = items[at].count;
    items[at].count = if count >= mhnx_core::itembox::MAX_COUNT as u8 {
        count - 1
    } else {
        count + 1
    };
    save.write_item_box(0, &items).expect("the item box writes");
}

/// One pouch entry, one higher (or one lower at the cap).
fn edit_pouch(save: &mut Save) {
    let pouch = mhnx_core::myset::pouch(save, 0);
    let at = pouch
        .iter()
        .position(|item| item.id != 0)
        .expect("an item in the pouch");
    let item = pouch[at];
    let count = if item.count >= mhnx_core::itembox::MAX_COUNT {
        item.count - 1
    } else {
        item.count + 1
    };
    assert!(
        mhnx_core::myset::set_pouch_item(save, 0, at, item.id, count),
        "the pouch entry writes"
    );
}

/// One palico's experience, 1,000 higher.
fn edit_palico(save: &mut Save) {
    let roster = mhnx_core::palico::roster(save, 0);
    let at = roster
        .iter()
        .position(|palico| !palico.is_empty())
        .expect("a palico in the sample save");
    let exp = roster[at].exp;
    assert!(
        mhnx_core::palico::set_exp(save, 0, at, exp.wrapping_add(1_000)),
        "the experience writes"
    );
}

/// One monster's kill count, one higher.
fn edit_hunt_record(save: &mut Save) {
    let killed = mhnx_core::hunts::killed(save, 0, 5).expect("a hunt record");
    assert!(
        mhnx_core::hunts::set_killed(save, 0, 5, killed.wrapping_add(1)),
        "the hunt record writes"
    );
}

/// The guild card's hunter rank, one higher.
fn edit_guild_card(save: &mut Save) {
    let rank = mhnx_core::guildcard::hunter_rank(save, 0).expect("a guild card");
    assert!(
        mhnx_core::guildcard::set_hunter_rank(save, 0, rank.wrapping_add(1)),
        "the guild card writes"
    );
}
