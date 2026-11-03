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
fn setting_the_level_leaves_the_type_and_the_transmog_alone() {
    let mut rec = record(4, 3, 6);
    assert!(equip::set_level(&mut rec, 9));
    let e = equip::decode_record(0, &rec).unwrap();
    assert_eq!(e.kind, 4, "the type must not move");
    assert_eq!(e.level, 9);
    assert_eq!(e.transmog_level, 7, "the transmog level must not move");
}

#[test]
fn the_level_is_clamped_to_the_range_the_field_holds() {
    let mut rec = record(1, 0, 0);
    assert!(equip::set_level(&mut rec, 0));
    assert_eq!(equip::decode_record(0, &rec).unwrap().level, 1, "zero clamps up");
    assert!(equip::set_level(&mut rec, 99));
    assert_eq!(equip::decode_record(0, &rec).unwrap().level, 32, "and the top clamps down");
}

#[test]
fn decorations_can_be_written_one_socket_at_a_time() {
    let mut rec = record(1, 1, 0);
    assert!(equip::set_decoration(&mut rec, 0, 2638));
    assert!(equip::set_decoration(&mut rec, 2, 2640));
    assert_eq!(equip::Equipment::decorations(0, &rec), [2638, 0, 2640]);
    assert_eq!(equip::socketed(&rec), 2);
    // Emptying one leaves the others alone.
    assert!(equip::set_decoration(&mut rec, 0, 0));
    assert_eq!(equip::Equipment::decorations(0, &rec), [0, 0, 2640]);
    // A fourth socket does not exist.
    assert!(!equip::set_decoration(&mut rec, 3, 1));
}

#[test]
fn clearing_returns_a_record_to_empty() {
    let mut rec = record(2, 5, 0);
    rec[0x06..0x08].copy_from_slice(&2639u16.to_le_bytes());
    assert!(equip::clear(&mut rec));
    assert!(equip::record_is_blank(&rec));
}

#[test]
fn the_palico_kinds_have_labels() {
    // The hunter's box stops at 21; the palico box uses 22, 23 and 24.
    assert_eq!(equip::type_name(22), "オトモ武器");
    assert_eq!(equip::type_name(23), "オトモ防具(頭)");
    assert_eq!(equip::type_name(24), "オトモ防具(胴)");
    assert_eq!(equip::type_name(99), "?");
}

#[test]
fn a_blank_record_is_all_zero() {
    assert!(equip::record_is_blank(&[0u8; 36]));
    let mut rec = [0u8; 36];
    rec[0x0A] = 1; // something in the third decoration socket only
    assert!(!equip::record_is_blank(&rec));
}