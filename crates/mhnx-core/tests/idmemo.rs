//! The bundled name tables: 勲章 / 依頼 / クエスト / ステージ.
//!
//! What these tests are for is the *shape* rather than the wording. A name
//! table that is one row short puts the wrong name on every id after it, and
//! the mistake looks like data; so every table is checked against the count
//! the crate's own bit maps fix (awards::AWARDS, the request records of
//! activityData.atd, questprogress::BITS - 1), and the spot checks below pin
//! rows a different source already named.

use mhnx_core::idmemo;
use mhnx_core::{awards, questprogress};

#[test]
fn the_award_table_covers_every_bit_the_card_has() {
    assert_eq!(idmemo::AWARD_NAMES.len(), awards::AWARDS);
    assert_eq!(idmemo::AWARD_NAMES.len(), 132);
    for (i, name) in idmemo::AWARD_NAMES.iter().enumerate() {
        assert!(!name.trim().is_empty(), "award {} has no name", i);
        assert!(
            !name.contains('\t') && !name.contains('\n'),
            "award {} breaks a single line: {:?}",
            i,
            name
        );
    }
    // Two awards with one name would make the list useless for finding a bit.
    let mut seen: Vec<&str> = idmemo::AWARD_NAMES.to_vec();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), before, "award names repeat");

    // The card's first and last rows, as the game lists them.
    assert_eq!(idmemo::award_name(0), Some("ベルナ村長の飾り帯"));
    assert_eq!(idmemo::award_name(5), Some("黄金のメダル"));
    assert!(idmemo::award_name(131).is_some());
    // Outside the map there is no award, so there is no name either.
    assert_eq!(idmemo::award_name(132), None);
    assert_eq!(idmemo::award_name(usize::MAX), None);
}

#[test]
fn the_request_table_is_the_one_activity_data_carries() {
    // activityData.atd carries 184 records: 167 that point at a quest, 16 that
    // do not, and one closing sentinel.
    assert_eq!(idmemo::REQUEST_NAMES.len(), 184);
    assert_eq!(idmemo::REQUEST_KINDS.len(), 184);
    assert_eq!(idmemo::REQUEST_QUEST_IDS.len(), 184);
    assert_eq!(idmemo::REQUEST_ACCEPT_FLAGS.len(), 184);
    assert_eq!(idmemo::REQUEST_COMPLETE_FLAGS.len(), 184);

    let mut kinds = [0usize; 4];
    for kind in idmemo::REQUEST_KINDS {
        kinds[*kind as usize] += 1;
    }
    assert_eq!(kinds, [167, 13, 3, 1], "167 quest rows, 16 without, 1 sentinel");

    for i in 0..184 {
        let accept = idmemo::REQUEST_ACCEPT_FLAGS[i];
        let complete = idmemo::REQUEST_COMPLETE_FLAGS[i];
        if idmemo::REQUEST_KINDS[i] == 3 {
            // The sentinel owns no flag at all.
            assert_eq!((accept, complete), (0, 0), "sentinel row {}", i);
            continue;
        }
        assert_eq!(
            complete,
            accept + 1,
            "row {} is not an accept / complete pair",
            i
        );
        // The two bands the request view lists: 300..555 and 1270..1395.
        assert!(
            (300..555).contains(&accept) || (1270..1395).contains(&accept),
            "row {} has a flag outside the bands: {}",
            i,
            accept
        );
    }
    // Every row without a quest id is a row with a non-zero kind, and the other
    // way round. That is what makes the quest id column trustworthy.
    for i in 0..184 {
        assert_eq!(
            idmemo::REQUEST_QUEST_IDS[i] == 0,
            idmemo::REQUEST_KINDS[i] != 0,
            "row {} disagrees about carrying a quest",
            i
        );
    }
    assert_eq!(idmemo::request_name(0), Some("しっかり者の村人の依頼１"));
    assert_eq!(idmemo::request_name(184), None);
}

#[test]
fn a_flag_belongs_to_one_request_or_to_none() {
    // The first record: 300 accepts it, 301 completes it.
    assert_eq!(
        idmemo::request_flag_owner(300),
        Some((0, false, "しっかり者の村人の依頼１"))
    );
    assert_eq!(
        idmemo::request_flag_owner(301),
        Some((0, true, "しっかり者の村人の依頼１"))
    );
    // The last pair the table names.
    let last = idmemo::REQUEST_ACCEPT_FLAGS[182];
    assert_eq!(last, 1394);
    assert_eq!(
        idmemo::request_flag_owner(last).map(|(i, complete, _)| (i, complete)),
        Some((182, false))
    );
    assert_eq!(
        idmemo::request_flag_owner(1395).map(|(i, complete, _)| (i, complete)),
        Some((182, true))
    );
    // The closing sentinel row is not a request: it carries flag 0, which the
    // save keeps clear, and naming it would invent a request that never was.
    assert_eq!(idmemo::REQUEST_KINDS[183], 3);
    assert_eq!(idmemo::request_flag_owner(0), None);
    // Not a request either: the save shares the flag space with other unlocks,
    // and naming one of those would be a guess.
    assert_eq!(idmemo::request_flag_owner(1), None);
    assert_eq!(idmemo::request_flag_owner(1535), None);
}

#[test]
fn every_quest_index_but_zero_has_a_name() {
    // Index 0 is the unused dummy the save keeps clear.
    assert_eq!(idmemo::QUEST_NAMES.len(), questprogress::BITS);
    assert_eq!(idmemo::QUEST_NAMES.len(), 1509);
    assert_eq!(idmemo::quest_name(0), None);
    assert_eq!(idmemo::quest_name(1), Some("古代林の特産ゼンマイ"));
    assert_eq!(idmemo::quest_name(1508), Some("イベント：大連続狩猟５"));
    assert_eq!(idmemo::quest_name(1509), None);
    for index in 1..questprogress::BITS {
        let name = idmemo::quest_name(index).expect("a row for every index");
        assert!(!name.trim().is_empty(), "quest {} has no name", index);
    }
}

#[test]
fn the_special_permit_band_sits_where_the_module_documents_it() {
    // questprogress.rs: 228 entries starting at index 947, the 18 deviants in
    // id order, twelve with 16 quests and six with 6. If the row order of the
    // table were off by even one row this band would not line up.
    let band = &idmemo::QUEST_NAMES[947..=1174];
    assert_eq!(band.len(), 228);
    for name in band {
        assert!(
            name.starts_with("【特殊許可】") || name.starts_with("【超特殊許可】"),
            "unexpected row in the permit band: {:?}",
            name
        );
    }
    assert_eq!(idmemo::quest_name(947), Some("【特殊許可】紅兜狩猟依頼１"));
    assert_eq!(idmemo::quest_name(1174), Some("【超特殊許可】鏖魔狩猟依頼"));
    // Eighteen deviants, one 超 special permit row each, so 18 超 rows and
    // 210 ordinary ones: 12 * (15 + 1) + 6 * (5 + 1) = 228.
    let ultra = band.iter().filter(|n| n.starts_with("【超特殊許可】")).count();
    let normal = band.iter().filter(|n| n.starts_with("【特殊許可】")).count();
    assert_eq!(ultra, 18, "one 超 special permit row per deviant");
    assert_eq!(normal, 210);
    // The deviants run in the game's own order, 紅兜 first and 鏖魔 last.
    assert!(idmemo::quest_name(947).unwrap().contains("紅兜"));
    assert!(idmemo::quest_name(1174).unwrap().contains("鏖魔"));
}

#[test]
fn the_arena_band_is_the_seventeen_arena_quests() {
    // arena.tsv lists 17 quests and the progress map gives them 1288..1304.
    let band = &idmemo::QUEST_NAMES[1288..=1304];
    assert_eq!(band.len(), 17);
    for name in band {
        assert!(name.ends_with("討伐"), "unexpected arena row: {:?}", name);
    }
    assert_eq!(idmemo::quest_name(1288), Some("ホロロホルル討伐"));
}

#[test]
fn stage_names_resolve_by_id_and_by_quest() {
    assert_eq!(idmemo::STAGE_IDS.len(), idmemo::STAGE_NAMES.len());
    assert_eq!(idmemo::STAGE_NAMES.len(), 31);
    assert_eq!(idmemo::stage_name(1), Some("古代林"));
    assert_eq!(idmemo::stage_name(2), Some("森丘"));
    // The ids are not dense: 28..100 are not stages the table names.
    assert_eq!(idmemo::stage_name(28), None);
    assert_eq!(idmemo::stage_name(0), None);
    assert_eq!(idmemo::stage_name(255), None);
    // The first quest is a 古代林 quest, and the quest table says so.
    assert_eq!(idmemo::quest_stage_name(1), Some("古代林"));
    assert_eq!(idmemo::quest_stage_name(0), None);
    assert_eq!(idmemo::quest_stage_name(1509), None);
    // A quest that names a stage names one the stage table can resolve. Some
    // rows do not (the event quests, 27 of the 1508, have no area in the
    // table), and those stay blank rather than borrowing their neighbour's.
    let mut with_stage = 0usize;
    for index in 1..questprogress::BITS {
        let id = idmemo::QUEST_STAGE_IDS[index];
        if id == 0 {
            assert_eq!(idmemo::quest_stage_name(index), None);
            continue;
        }
        with_stage += 1;
        assert!(
            idmemo::stage_name(id).is_some(),
            "quest {} points at stage {} which the table does not name",
            index,
            id
        );
    }
    assert_eq!(with_stage, 1481, "1481 of the 1508 rows carry an area");
}

#[test]
fn the_provenance_is_recorded_in_the_module() {
    // The generated file states where the rows came from, so a reader of the
    // code does not have to find the generator to know.
    assert!(idmemo::SOURCE_REPO.contains("github.com/Nexy451z/mhxx-id-memo"));
    assert_eq!(idmemo::SOURCE_COMMIT.len(), 40);
    assert!(idmemo::SOURCE_COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
}
