//! The equipment record layout.
//!
//! The first word packs three fields, and reading it as one wide level folded
//! the transmog level into the level. Nothing showed in the sample save, which
//! has never been transmogged, so these pin the field boundaries down.

use mhnx_core::equip;

fn record(kind: u16, stored_level: u16, stored_transmog: u16) -> [u8; 36] {
    let mut rec = [0u8; 36];
    let head: u16 = kind | (stored_level << 5) | (stored_transmog << 10);
    rec[..2].copy_from_slice(&head.to_le_bytes());
    rec
}

#[test]
fn the_three_fields_of_the_first_word_are_separate() {
    let rec = record(1, 4, 2);
    let e = equip::decode_record(0, &rec).expect("decodes");
    assert_eq!(e.kind, 1, "armour head");
    assert_eq!(e.level, 5, "stored 4 means level 5");
    assert_eq!(e.transmog_level, 3, "stored 2 means level 3");
}

#[test]
fn a_transmog_does_not_move_the_level() {
    let plain = equip::decode_record(0, &record(1, 9, 0)).unwrap();
    let transmogged = equip::decode_record(0, &record(1, 9, 7)).unwrap();
    assert_eq!(plain.level, 10);
    assert_eq!(transmogged.level, 10, "the level must not follow the transmog");
    assert_eq!(transmogged.transmog_level, 8);
}

#[test]
fn the_level_saturates_at_the_top_of_its_five_bits() {
    // 0x1F is the largest value the field can hold, so level 32.
    let e = equip::decode_record(0, &record(7, 0x1F, 0)).unwrap();
    assert_eq!(e.level, 32);
}

#[test]
fn decorations_are_three_u16s_and_zero_means_empty() {
    let mut rec = record(1, 0, 0);
    rec[0x06..0x08].copy_from_slice(&2639u16.to_le_bytes());
    rec[0x0A..0x0C].copy_from_slice(&2641u16.to_le_bytes());
    assert_eq!(equip::Equipment::decorations(0, &rec), [2639, 0, 2641]);
}

#[test]
fn a_blank_record_is_all_zero() {
    assert!(equip::record_is_blank(&[0u8; 36]));
    let mut rec = [0u8; 36];
    rec[0x0A] = 1; // something in the third decoration socket only
    assert!(!equip::record_is_blank(&rec));
}