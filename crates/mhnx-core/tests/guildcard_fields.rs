//! Guild card fields that were missing: title, weapon usage, quest history and
//! the cards received from other hunters.
//!
//! The numbers the real-save tests assert were read out of
//! C:\anyWorkspace\nx\jksv_ready\system (read only, never written) while the
//! offsets were being verified; the tests keep them so a regression in the
//! layout shows up as a wrong value instead of silently plausible garbage.
//! Everything else runs on a blank in-memory save.

use std::path::PathBuf;

use mhnx_core::guildcard::{
    self, CardList, Title, Venue, GC_CARDS, GC_CARDS_INFO, GC_HISTORY, GC_SIZE, TITLE_LINKS,
    TITLE_WORDS, WEAPON_TYPES,
};
use mhnx_core::save::{empty_switch_save, Save};

/// The sample save, when it is on this machine.
fn real_save() -> Option<Save> {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    let p = if stable.exists() {
        stable
    } else {
        // The same save under Ryujinx, built in two steps so the 0 directory
        // name needs no escaping.
        PathBuf::from(r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001")
            .join("0")
            .join("system")
    };
    Save::from_bytes(&std::fs::read(p).ok()?).ok()
}

/// A blank save with slot 0 usable, and the base the tests write at.
fn blank() -> (Save, usize) {
    let mut s = empty_switch_save();
    s.set_slot_used(0, true);
    let base = s.slot_base(0).expect("slot base") as usize;
    (s, base)
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn the_card_block_is_sized_up_to_the_received_list() {
    let Some(s) = real_save() else { return };
    for slot in 0..3 {
        let card = guildcard::full_card(&s, slot).expect("card block");
        assert_eq!(card.len(), GC_SIZE, "slot {slot}");
        // The received-card info records start right behind the card, so a
        // larger or smaller card would put them out of step.
        let base = s.slot_base(slot).expect("slot base") as usize;
        assert_eq!(
            base + guildcard::OFF_GUILD_CARD + GC_SIZE,
            base + guildcard::GC_CARDS_INFO,
            "slot {slot}"
        );
    }
}

#[test]
fn title_and_unlock_maps_match_the_sample_save() {
    let Some(s) = real_save() else { return };
    for slot in 0..3 {
        assert_eq!(
            guildcard::title(&s, slot),
            Some(Title {
                first: 4,
                link: 0,
                last: 6
            }),
            "slot {slot}"
        );
        assert_eq!(
            guildcard::scene_and_pose(&s, slot),
            Some((0, 0)),
            "slot {slot}"
        );
        // The two words and the empty connector really are unlocked.
        for word in [4usize, 6, 502] {
            assert!(
                guildcard::title_word_unlocked(&s, slot, word),
                "slot {slot} word {word}"
            );
        }
        assert!(guildcard::title_link_unlocked(&s, slot, 0), "slot {slot} link 0");
        assert_eq!(
            guildcard::unlocked_title_words(&s, slot).len(),
            919,
            "slot {slot}"
        );
        assert_eq!(
            guildcard::unlocked_title_links(&s, slot).len(),
            117,
            "slot {slot}"
        );
    }
    // Out of range ids are refused instead of reading past the map.
    assert!(!guildcard::title_word_unlocked(&s, 0, TITLE_WORDS));
    assert!(!guildcard::title_link_unlocked(&s, 0, TITLE_LINKS));
}

#[test]
fn weapon_usage_matches_the_sample_save() {
    let Some(s) = real_save() else { return };
    let village = [15u32, 15, 0, 0, 4, 1, 283, 0, 0, 0, 9, 0, 0, 0, 54];
    let hub = [210u32, 20, 1, 0, 44, 78, 664, 1, 1, 1, 47, 1, 20, 0, 27];
    let arena = [4u32, 2, 5, 0, 0, 0, 2, 0, 0, 6, 5, 0, 1, 0, 0];
    for slot in 0..3 {
        assert_eq!(
            guildcard::venue_usage(&s, slot, Venue::Village),
            village,
            "slot {slot}"
        );
        assert_eq!(
            guildcard::venue_usage(&s, slot, Venue::Hub),
            hub,
            "slot {slot}"
        );
        assert_eq!(
            guildcard::venue_usage(&s, slot, Venue::Arena),
            arena,
            "slot {slot}"
        );
        let all = guildcard::weapon_usage(&s, slot);
        assert_eq!(all.len(), WEAPON_TYPES, "slot {slot}");
        for w in 0..WEAPON_TYPES {
            assert_eq!(
                all[w],
                village[w] + hub[w] + arena[w],
                "slot {slot} weapon {w}"
            );
        }
        // 381 + 1115 + 25: the three venue sums agree with the totals.
        assert_eq!(all.iter().sum::<u32>(), 1521, "slot {slot}");
        assert_eq!(all[6], 949, "slot {slot} long sword is the main weapon");
    }
}

#[test]
fn quest_counters_match_the_sample_save() {
    let Some(s) = real_save() else { return };
    for slot in 0..3 {
        assert_eq!(
            guildcard::quest_counts(&s, slot),
            vec![273, 106, 125, 230, 382, 233, 24],
            "slot {slot}"
        );
        assert_eq!(guildcard::quest_total(&s, slot), 1373, "slot {slot}");
    }
}

#[test]
fn quest_history_matches_the_sample_save() {
    let Some(s) = real_save() else { return };
    let h = guildcard::history(&s, 0);
    assert_eq!(h.len(), 10, "ten records in use");
    let first = &h[0];
    assert_eq!(
        (first.day, first.month, first.year),
        (22, 8, 2022),
        "newest record"
    );
    assert_eq!(first.kind, 9, "abandoned quest");
    assert_eq!(first.quest_id, 11473);
    assert_eq!(first.name, "金と銀がもたらす悲哀");
    assert_eq!(first.highlights, [36, 36, 36]);
    assert_eq!(first.party.len(), 3, "an empty fourth slot is left out");
    assert_eq!(first.party[0].name, "ナナ");
    assert_eq!(first.party[0].weapon, 6, "long sword");
    assert_eq!(first.party[1].weapon, guildcard::PARTY_PALICO);
    // Newest first: the dates never go up, and every record has a name.
    for pair in h.windows(2) {
        assert!(
            (pair[1].year, pair[1].month, pair[1].day)
                <= (pair[0].year, pair[0].month, pair[0].day),
            "history is newest first"
        );
    }
    for r in &h {
        assert!(!r.name.is_empty(), "a used record has a quest name");
        assert_ne!(r.kind, guildcard::HISTORY_UNSET);
    }
    // The most recent quests in the sample save include a special permit one.
    assert!(h.iter().any(|r| r.name.contains("特殊許可")));
}

#[test]
fn the_kept_card_list_decodes_and_the_inbox_is_empty() {
    let Some(s) = real_save() else { return };
    let cards = guildcard::hunter_cards(&s, 0);
    assert_eq!(cards.len(), 24, "cards kept in the sample save");
    let first = &cards[0];
    assert_eq!(first.list, CardList::Stored);
    assert_eq!(first.slot, 0);
    assert_eq!(first.hunter_rank, 693);
    assert_eq!(first.name, "☆T&H☆");
    assert_eq!(first.id, [0x09, 0x39, 0xeb, 0xe5, 0x8b, 0xc1, 0x1a, 0xf3]);
    assert_eq!(first.play_time, 1_899_977);
    assert_eq!(first.date, (14, 11, 2021));
    assert_eq!(first.unity, 76, "76 per quest together");
    assert_eq!(first.kind, Some(0));
    assert_eq!(first.compressed, 1607);
    assert_eq!(cards.last().map(|c| c.name.as_str()), Some("特攻の拓"));
    // The slots stay ordered and every kept card has a name and an owner id.
    for (i, c) in cards.iter().enumerate() {
        assert_eq!(c.slot, i);
        assert!(!c.name.is_empty(), "card {i} has no name");
        assert_ne!(c.id, [0u8; 8], "card {i} has no id");
        assert!(c.hunter_rank <= 999);
    }
    assert!(guildcard::card_inbox(&s, 0).is_empty(), "inbox is empty");
}

#[test]
fn a_kept_card_inflates_and_carries_its_own_fields() {
    let Some(s) = real_save() else { return };
    let data = guildcard::card_data(&s, 0, CardList::Stored, 0).expect("first card");
    assert_eq!(data.len(), GC_SIZE);
    assert_eq!(
        guildcard::title_in(&data),
        Some(Title {
            first: 357,
            link: 7,
            last: 336
        })
    );
    assert_eq!(
        guildcard::quest_counts_in(&data),
        vec![291, 118, 191, 391, 749, 272, 29]
    );
    assert_eq!(guildcard::weapon_usage_in(&data).len(), WEAPON_TYPES);
    // The received card carries its own greeting, not the receiver's comment.
    assert_eq!(guildcard::comment_in(&data), "見ちゃやーよww");
    let history = guildcard::history_in(&data);
    assert!(!history.is_empty(), "a received card carries its history");
    // Slot 24 is empty, so there is no card to inflate there.
    assert!(guildcard::card_data(&s, 0, CardList::Stored, 24).is_none());
}

#[test]
fn weapon_usage_round_trips_and_rejects_bad_indices() {
    let (mut s, _) = blank();
    assert_eq!(guildcard::weapon_usage(&s, 0), vec![0u32; WEAPON_TYPES]);
    for (w, venue) in [
        (3usize, Venue::Village),
        (14, Venue::Hub),
        (0, Venue::Arena),
    ] {
        assert!(guildcard::set_weapon_usage(&mut s, 0, venue, w, 700 + w as u16));
    }
    let village = guildcard::venue_usage(&s, 0, Venue::Village);
    assert_eq!(village[3], 703);
    assert_eq!(village[14], 0, "each venue is a separate array");
    let hub = guildcard::venue_usage(&s, 0, Venue::Hub);
    assert_eq!(hub[14], 714);
    let all = guildcard::weapon_usage(&s, 0);
    assert_eq!(all[3], 703);
    assert_eq!(all[14], 714);
    assert_eq!(all[0], 700);
    assert_eq!(all.iter().sum::<u32>(), 703 + 714 + 700);
    // Out of range weapon: nothing to write, and no panic.
    assert!(!guildcard::set_weapon_usage(
        &mut s,
        0,
        Venue::Village,
        WEAPON_TYPES,
        1
    ));
    // Slot 1 has no character, but its base is valid, so it reads as zeros
    // rather than as the other slot's counts.
    assert_eq!(guildcard::weapon_usage(&s, 1), vec![0u32; WEAPON_TYPES]);
}

#[test]
fn title_round_trips_and_unlocking_sets_both_copies() {
    let (mut s, base) = blank();
    assert_eq!(guildcard::title(&s, 0), Some(Title::default()));
    let t = Title {
        first: 140,
        link: 5,
        last: 502,
    };
    assert!(guildcard::set_title(&mut s, 0, t));
    assert_eq!(guildcard::title(&s, 0), Some(t));

    assert!(!guildcard::title_word_unlocked(&s, 0, 140));
    assert!(guildcard::set_title_word_unlocked(&mut s, 0, 140, true));
    assert!(guildcard::title_word_unlocked(&s, 0, 140));
    // The NEW copy the game sets at unlock time follows.
    let byte = base + guildcard::GC_TITLE_WORD_MAP_NEW + 140 / 8;
    assert_eq!(s.body()[byte] & (1 << (140 % 8)), 1 << (140 % 8));

    assert!(guildcard::set_title_link_unlocked(&mut s, 0, 5, true));
    assert!(guildcard::title_link_unlocked(&s, 0, 5));
    assert_eq!(guildcard::unlocked_title_words(&s, 0), vec![140]);
    assert_eq!(guildcard::unlocked_title_links(&s, 0), vec![5]);

    // Clearing takes the bit out of both copies again.
    assert!(guildcard::set_title_word_unlocked(&mut s, 0, 140, false));
    assert!(!guildcard::title_word_unlocked(&s, 0, 140));
    assert_eq!(s.body()[byte], 0);
    assert!(guildcard::unlocked_title_words(&s, 0).is_empty());

    // Out of range: refused, not wrapped onto the next bit.
    assert!(!guildcard::set_title_word_unlocked(
        &mut s,
        0,
        TITLE_WORDS,
        true
    ));
    assert!(!guildcard::set_title_link_unlocked(
        &mut s,
        0,
        TITLE_LINKS,
        true
    ));
    assert_eq!(guildcard::scene_and_pose(&s, 0), Some((0, 0)));
}

#[test]
fn history_round_trips_and_skips_unused_records() {
    let (mut s, base) = blank();
    let at = base + guildcard::OFF_GUILD_CARD + GC_HISTORY;
    // Record 0: a completed quest with a two hunter party and a Palico.
    let mut r = vec![0u8; guildcard::HISTORY_RECORD];
    r[0] = 19;
    r[1] = 9;
    r[2..4].copy_from_slice(&2026u16.to_le_bytes());
    r[4..6].copy_from_slice(&7u16.to_le_bytes());
    r[6..8].copy_from_slice(&201u16.to_le_bytes());
    r[8..8 + utf16le("Harvest Tour").len()].copy_from_slice(&utf16le("Harvest Tour"));
    r[0x28] = 36;
    r[0x29] = 6;
    r[0x2A] = 22;
    r[0x44..0x44 + utf16le("Scrimas").len()].copy_from_slice(&utf16le("Scrimas"));
    r[0x44 + 22..0x44 + 22 + utf16le("Suds").len()].copy_from_slice(&utf16le("Suds"));
    r[0x9C] = 13;
    r[0x9D] = guildcard::PARTY_PALICO;
    r[0x9E] = guildcard::PARTY_NONE;
    r[0x9F] = guildcard::PARTY_NONE;
    s.body_mut()[at..at + r.len()].copy_from_slice(&r);
    // Record 1 stays as the constructor leaves it: kind 11, all zero.
    let at1 = at + guildcard::HISTORY_RECORD;
    s.body_mut()[at1 + 4..at1 + 6].copy_from_slice(&guildcard::HISTORY_UNSET.to_le_bytes());
    // Record 2 is cleared: a kind but no date, which must not show as year 0.
    let at2 = at + 2 * guildcard::HISTORY_RECORD;
    s.body_mut()[at2 + 4..at2 + 6].copy_from_slice(&7u16.to_le_bytes());

    let h = guildcard::history(&s, 0);
    assert_eq!(h.len(), 1, "only the written record is shown");
    let first = &h[0];
    assert_eq!((first.day, first.month, first.year), (19, 9, 2026));
    assert_eq!((first.kind, first.quest_id), (7, 201));
    assert_eq!(first.name, "Harvest Tour");
    assert_eq!(first.highlights, [36, 6, 22]);
    assert_eq!(first.party.len(), 2, "the two empty slots are left out");
    assert_eq!(first.party[0].name, "Scrimas");
    assert_eq!(first.party[0].weapon, 13);
    assert_eq!(first.party[1].name, "Suds");
    assert_eq!(first.party[1].weapon, guildcard::PARTY_PALICO);
}

#[test]
fn card_list_walk_reads_the_trailer_and_skips_empty_elements() {
    let (mut s, base) = blank();
    // Element 0: a filled element whose payload is not a valid zlib stream at
    // all. cards() only needs the length and the trailer, so it still decodes.
    let at = base + GC_CARDS;
    let payload = [0xAAu8; 32];
    s.body_mut()[at..at + 4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    s.body_mut()[at + 4..at + 4 + payload.len()].copy_from_slice(&payload);
    let state = at + 4 + payload.len();
    s.body_mut()[state..state + 4].copy_from_slice(&guildcard::CARD_FILLED.to_le_bytes());
    let tr = state + 4;
    let mut trailer = [0u8; guildcard::CARD_TRAILER];
    trailer[0..2].copy_from_slice(&42u16.to_le_bytes());
    trailer[2..2 + utf16le("Ann").len()].copy_from_slice(&utf16le("Ann"));
    trailer[24..32].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    trailer[32..36].copy_from_slice(&1234u32.to_le_bytes());
    s.body_mut()[tr..tr + 36].copy_from_slice(&trailer);
    // Its info record: date, comment, unity, type.
    let info = base + GC_CARDS_INFO;
    let mut inf = [0u8; guildcard::CARD_INFO_LEN];
    inf[0] = 3;
    inf[1] = 4;
    inf[2..4].copy_from_slice(&2025u16.to_le_bytes());
    inf[4..4 + utf16le("hello").len()].copy_from_slice(&utf16le("hello"));
    inf[0x1C..0x20].copy_from_slice(&152u32.to_le_bytes());
    inf[0x20] = 25;
    s.body_mut()[info..info + inf.len()].copy_from_slice(&inf);

    let cards = guildcard::hunter_cards(&s, 0);
    assert_eq!(cards.len(), 1, "empty elements are not cards");
    let c = &cards[0];
    assert_eq!(c.slot, 0);
    assert_eq!(c.hunter_rank, 42);
    assert_eq!(c.name, "Ann");
    assert_eq!(c.id, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(c.play_time, 1234);
    assert_eq!(c.date, (3, 4, 2025));
    assert_eq!(c.comment, "hello");
    assert_eq!(c.unity, 152);
    assert_eq!(c.kind, Some(25));
    // A payload that is not zlib cannot be inflated, so no card data.
    assert!(guildcard::card_data(&s, 0, CardList::Stored, 0).is_none());

    // A length larger than one card means the bytes are not a list: stop.
    let (mut bad, base) = blank();
    let at = base + GC_CARDS;
    bad.body_mut()[at..at + 4]
        .copy_from_slice(&((guildcard::CARD_DATA_LEN + 1) as u32).to_le_bytes());
    assert!(guildcard::hunter_cards(&bad, 0).is_empty());
}

#[test]
fn an_empty_element_inflates_to_a_blank_card_but_is_not_a_card() {
    let (mut s, base) = blank();
    // The exact 29 byte stream the game writes into an element it never filled
    // (core/src/cards.rs of the reference tree, and every empty element of the
    // sample save).
    const EMPTY_Z: [u8; 29] = [
        0x78, 0x9c, 0xed, 0xc1, 0x31, 0x01, 0x00, 0x00, 0x00, 0xc2, 0xa0, 0xf5, 0x4f, 0x6d, 0x0d,
        0x0f, 0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3e, 0x0c, 0x18, 0xb8, 0x00, 0x01,
    ];
    let at = base + GC_CARDS;
    s.body_mut()[at..at + 4].copy_from_slice(&(EMPTY_Z.len() as u32).to_le_bytes());
    s.body_mut()[at + 4..at + 4 + EMPTY_Z.len()].copy_from_slice(&EMPTY_Z);
    let state = at + 4 + EMPTY_Z.len();
    // State 1 is "empty", so the list has no card and index 0 cannot be inflated.
    s.body_mut()[state..state + 4].copy_from_slice(&1u32.to_le_bytes());
    assert!(guildcard::hunter_cards(&s, 0).is_empty());
    assert!(guildcard::card_data(&s, 0, CardList::Stored, 0).is_none());
    // The same stream marked filled inflates to one blank card.
    s.body_mut()[state..state + 4].copy_from_slice(&guildcard::CARD_FILLED.to_le_bytes());
    let data = guildcard::card_data(&s, 0, CardList::Stored, 0).expect("blank card");
    assert_eq!(data.len(), GC_SIZE);
    assert!(data.iter().all(|&b| b == 0));
    assert_eq!(guildcard::title_in(&data), Some(Title::default()));
    assert!(guildcard::history_in(&data).is_empty());
}

/// A minimal .gmd holding the strings, enough for romfstables::gmd_strings.
fn gmd(strings: &[&str]) -> Vec<u8> {
    let mut b = vec![0u8; 0x28];
    b[0..4].copy_from_slice(b"GMD\0");
    b[4..8].copy_from_slice(&0x0001_0302u32.to_le_bytes());
    b[0x18..0x1c].copy_from_slice(&(strings.len() as u32).to_le_bytes());
    b[0x20..0x24].copy_from_slice(&0u32.to_le_bytes());
    b[0x24..0x28].copy_from_slice(&0u32.to_le_bytes());
    b.push(0); // the empty key's terminator
    for s in strings {
        b.extend_from_slice(s.as_bytes());
        b.push(0);
    }
    b
}

#[test]
fn the_title_table_writes_word_connector_word() {
    let words = gmd(&["（なし）", "かけだし", "ハンター"]);
    let links = gmd(&["（なし）", "の"]);
    let t = guildcard::TitleTable::from_gmd(&words, &links);
    assert!(!t.is_empty());
    assert_eq!(t.word(2), Some("ハンター"));
    assert_eq!(t.link(1), Some("の"));
    assert_eq!(t.word(9_999), None);
    // The sample save's shape: a word, no connector, a word.
    assert_eq!(
        t.text(Title {
            first: 1,
            link: 0,
            last: 2
        }),
        "かけだしハンター"
    );
    // Id 0 is "(なし)" in both tables and is left out.
    assert_eq!(t.text(Title::default()), "");
    assert_eq!(
        t.text(Title {
            first: 1,
            link: 1,
            last: 2
        }),
        "かけだしのハンター"
    );
    // Bytes that are not a .gmd give an empty table rather than nonsense.
    let empty = guildcard::TitleTable::from_gmd(b"not a gmd", b"");
    assert!(empty.is_empty());
    assert_eq!(
        empty.text(Title {
            first: 1,
            link: 0,
            last: 2
        }),
        ""
    );
    // A table with fewer entries than the game's simply has no name there.
    assert_eq!(t.word(TITLE_WORDS - 1), None);
}

#[test]
fn comment_decodes_utf16_including_astral_characters() {
    let (mut s, _) = blank();
    assert!(guildcard::set_comment(&mut s, 0, "hello"));
    assert_eq!(guildcard::comment(&s, 0).as_deref(), Some("hello"));
    // An astral character cannot fit the field's one unit, so set_comment
    // writes the katakana middle dot; the reader must still round trip it.
    assert!(guildcard::set_comment(&mut s, 0, "a\u{1F600}b"));
    assert_eq!(guildcard::comment(&s, 0).as_deref(), Some("a\u{30FB}b"));
    // More than 28 characters is cut, not overflowed.
    let long: String = std::iter::repeat('あ').take(40).collect();
    assert!(guildcard::set_comment(&mut s, 0, &long));
    assert_eq!(
        guildcard::comment(&s, 0).map(|c| c.chars().count()),
        Some(28)
    );
}
