//! The 132 award bits and the three copies the game keeps of them.
//!
//! Every test here reads the sample save and works on the bytes in memory; the
//! file at C:\anyWorkspace\nx\jksv_ready\system is opened read only, and a
//! save written out by a test would be the one in memory, not that file.

use std::path::PathBuf;

use mhnx_core::awards::{
    self, AWARDS, CARD_LEN, CARD_OFF, MAP_LEN, MAP_OFF, NOTICE_LEN, NOTICE_OFF,
};
use mhnx_core::save::{empty_switch_save, Save};

const COPIES: [(usize, usize); 3] = [
    (CARD_OFF, CARD_LEN),
    (MAP_OFF, MAP_LEN),
    (NOTICE_OFF, NOTICE_LEN),
];

fn real_save() -> Option<Save> {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    let p = if stable.exists() {
        stable
    } else {
        PathBuf::from(
            r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
        )
    };
    Save::from_bytes(&std::fs::read(p).ok()?).ok()
}

/// The raw bytes of one of the three copies.
fn raw(s: &Save, slot: usize, off: usize, len: usize) -> Vec<u8> {
    let base = s.slot_base(slot).expect("slot base") as usize;
    s.body()[base + off..base + off + len].to_vec()
}

/// One bit, LSB first - the rule the whole module is written to.
fn bit(b: &[u8], award: usize) -> bool {
    b[award >> 3] & (1 << (award & 7)) != 0
}

/// A save with one slot marked in use and every byte zero.
fn blank_used_slot() -> Save {
    let mut s = empty_switch_save();
    s.set_slot_used(0, true);
    s
}

#[test]
fn the_real_save_holds_eighty_awards_in_every_slot() {
    let Some(s) = real_save() else { return };
    for slot in 0..3 {
        assert!(s.slot_used(slot), "slot {} should be in use", slot);
        assert_eq!(awards::count(&s, slot), 80, "awards held in slot {}", slot);
        assert_eq!(awards::missing(&s, slot).len(), AWARDS - 80);
        // The two lists have to be exact complements of each other.
        let missing = awards::missing(&s, slot);
        let mut held = 0;
        for a in 0..AWARDS {
            match awards::has(&s, slot, a) {
                Some(true) => held += 1,
                Some(false) => assert!(
                    missing.contains(&a),
                    "slot {} award {} is not in the missing list",
                    slot,
                    a
                ),
                None => panic!("slot {} award {} came back None", slot, a),
            }
        }
        assert_eq!(held, 80);
    }
}

#[test]
fn the_three_copies_agree_in_the_real_save() {
    let Some(s) = real_save() else { return };
    for slot in 0..3 {
        assert!(awards::copies_agree(&s, slot), "slot {}", slot);
        // Stronger than the 132 bits: the bytes themselves match, which is
        // what the offset hunt actually found.
        let card = raw(&s, slot, CARD_OFF, CARD_LEN);
        assert_eq!(card, raw(&s, slot, MAP_OFF, MAP_LEN)[..CARD_LEN]);
        assert_eq!(card, raw(&s, slot, NOTICE_OFF, NOTICE_LEN)[..CARD_LEN]);
    }
}

#[test]
fn setting_an_award_lights_all_three_copies() {
    let Some(mut s) = real_save() else { return };
    for slot in 0..3 {
        let Some(award) = awards::missing(&s, slot).first().copied() else {
            panic!("slot {} has no missing award to try", slot);
        };
        let before = awards::count(&s, slot);
        assert!(
            awards::set(&mut s, slot, award, true),
            "slot {} award {}",
            slot,
            award
        );
        assert_eq!(awards::has(&s, slot, award), Some(true));
        for (off, len) in COPIES {
            assert!(
                bit(&raw(&s, slot, off, len), award),
                "copy at {:x} missed slot {} award {}",
                off,
                slot,
                award
            );
        }
        assert_eq!(awards::count(&s, slot), before + 1);
        assert!(awards::copies_agree(&s, slot));
        assert!(!awards::missing(&s, slot).contains(&award));
        // Put it back so the next slot starts from the save as it was.
        assert!(awards::set(&mut s, slot, award, false));
    }
}

#[test]
fn clearing_an_award_removes_it_from_all_three_copies() {
    let Some(mut s) = real_save() else { return };
    for slot in 0..3 {
        let award = (0..AWARDS)
            .find(|&a| awards::has(&s, slot, a) == Some(true))
            .expect("an award the hunter holds");
        let before = awards::count(&s, slot);
        assert!(awards::set(&mut s, slot, award, false));
        assert_eq!(awards::has(&s, slot, award), Some(false));
        for (off, len) in COPIES {
            assert!(
                !bit(&raw(&s, slot, off, len), award),
                "copy at {:x} kept slot {} award {}",
                off,
                slot,
                award
            );
        }
        assert_eq!(awards::count(&s, slot), before - 1);
        assert!(awards::copies_agree(&s, slot));
        assert!(awards::set(&mut s, slot, award, true));
    }
}

#[test]
fn clearing_every_award_and_setting_the_held_ones_back_restores_the_bytes() {
    let Some(mut s) = real_save() else { return };
    let before = s.to_3ds_bytes();
    let pristine = original_save(&before);
    for slot in 0..3 {
        let held: Vec<usize> = (0..AWARDS)
            .filter(|&a| awards::has(&s, slot, a) == Some(true))
            .collect();
        assert_eq!(held.len(), 80);
        for a in 0..AWARDS {
            assert!(awards::set(&mut s, slot, a, false));
        }
        assert_eq!(awards::count(&s, slot), 0);
        assert_eq!(awards::missing(&s, slot).len(), AWARDS);
        assert!(awards::copies_agree(&s, slot));
        for a in &held {
            assert!(awards::set(&mut s, slot, *a, true));
        }
        assert_eq!(awards::count(&s, slot), 80);
        // Byte for byte, including the four spare bits of the last byte.
        for (off, len) in COPIES {
            assert_eq!(
                raw(&s, slot, off, len),
                raw(&pristine, slot, off, len),
                "copy at {:x} of slot {} was not restored",
                off,
                slot
            );
        }
    }
}

/// The sample save rebuilt from its body, for byte comparisons after edits.
fn original_save(body: &[u8]) -> Save {
    Save::from_3ds_body(body, "system").expect("body of the sample save")
}

#[test]
fn indexes_outside_the_map_are_refused_and_leave_the_save_alone() {
    let Some(mut s) = real_save() else { return };
    let before = s.to_3ds_bytes();
    for bad in [AWARDS, AWARDS + 1, 1000, usize::MAX] {
        assert_eq!(awards::has(&s, 0, bad), None, "award {}", bad);
        assert!(!awards::set(&mut s, 0, bad, true), "award {}", bad);
        assert!(!awards::set(&mut s, 0, bad, false), "award {}", bad);
    }
    // The last award is inside the map, the one past it is not.
    assert_eq!(awards::has(&s, 0, AWARDS - 1), Some(false));
    assert_eq!(s.to_3ds_bytes(), before);
}

#[test]
fn a_slot_that_does_not_exist_answers_with_nothing() {
    let Some(mut s) = real_save() else { return };
    for slot in [3, 4, usize::MAX] {
        assert_eq!(awards::has(&s, slot, 0), None, "slot {}", slot);
        assert!(!awards::set(&mut s, slot, 0, true), "slot {}", slot);
        assert_eq!(awards::count(&s, slot), 0);
        assert!(awards::missing(&s, slot).is_empty());
        assert!(!awards::copies_agree(&s, slot));
    }
    // A save whose slot bases are all zero has no slots at all.
    let mut broken = empty_switch_save();
    for off in [0x10, 0x14, 0x18] {
        broken.body_mut()[off..off + 4].fill(0);
    }
    assert_eq!(awards::has(&broken, 0, 0), None);
    assert!(!awards::set(&mut broken, 0, 0, true));
    assert_eq!(awards::count(&broken, 0), 0);
}

#[test]
fn an_unused_slot_is_treated_as_having_no_awards() {
    // A blank slot keeps three identical zero copies, so the bits read as
    // "none held" and the copies still agree.
    let mut s = empty_switch_save();
    s.set_slot_used(0, true);
    assert_eq!(awards::has(&s, 0, 0), Some(false));
    assert_eq!(awards::count(&s, 0), 0);
    assert_eq!(awards::missing(&s, 0).len(), AWARDS);
    assert!(awards::copies_agree(&s, 0));

    // The bit rule on a blank slot, top and bottom of the range.
    for a in [0, 1, 8, 9, 127, AWARDS - 1] {
        assert!(awards::set(&mut s, 0, a, true), "award {}", a);
        assert_eq!(awards::has(&s, 0, a), Some(true));
        for (off, len) in COPIES {
            assert!(
                bit(&raw(&s, 0, off, len), a),
                "copy {:x} missed award {}",
                off,
                a
            );
        }
        assert!(awards::copies_agree(&s, 0));
        assert!(awards::set(&mut s, 0, a, false));
    }

    // A slot the save marks unused holds no character: same answer as an out
    // of range slot, so nothing is invented for a slot that does not exist.
    s.set_slot_used(0, false);
    assert_eq!(awards::has(&s, 0, 0), None);
    assert!(!awards::set(&mut s, 0, 0, true));
    assert_eq!(awards::count(&s, 0), 0);
    assert!(awards::missing(&s, 0).is_empty());
    assert!(!awards::copies_agree(&s, 0));
}

#[test]
fn a_disagreeing_copy_is_reported_and_the_next_write_heals_it() {
    let mut s = blank_used_slot();
    assert!(awards::copies_agree(&s, 0));

    // A half applied edit: the map copy alone claims award 28.
    let base = s.slot_base(0).unwrap() as usize;
    let award = 28;
    s.body_mut()[base + MAP_OFF + (award >> 3)] |= 1u8 << (award & 7);
    assert!(
        !awards::copies_agree(&s, 0),
        "copies_agree must catch this before a write"
    );
    // has() reads the card copy, so the stale copy is visible as a mismatch.
    assert_eq!(awards::has(&s, 0, award), Some(false));

    // Writing the award the map already claims makes all three agree.
    assert!(awards::set(&mut s, 0, award, true));
    assert!(awards::copies_agree(&s, 0));
    for (off, len) in COPIES {
        assert!(
            bit(&raw(&s, 0, off, len), award),
            "copy {:x} still behind",
            off
        );
    }

    // A cleared award in one copy only is the same story in reverse.
    s.body_mut()[base + NOTICE_OFF + (award >> 3)] &= !(1u8 << (award & 7));
    assert!(!awards::copies_agree(&s, 0));
    assert!(awards::set(&mut s, 0, award, false));
    assert!(awards::copies_agree(&s, 0));
}
