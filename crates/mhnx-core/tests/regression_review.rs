//! Regression tests for the bugs the two review agents found.
//!
//! Each test names the failure it locks down. They read the real save but only
//! ever into memory.

use mhnx_core::charm::{self, Charm};
use mhnx_core::save::Save;

const EQ: usize = 0x62EE;
const REC: usize = 36;

const DEFAULT_SAVE: &str = r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system";

/// The real save, or None when MHNX_SKIP_REAL_SAVE is set.
///
/// A missing save is a hard failure rather than a quiet pass: six tests used
/// to go green on a machine that had no dump at all.
fn real_save() -> Option<Save> {
    let p = std::env::var("MHNX_REAL_SAVE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(DEFAULT_SAVE));
    if p.exists() {
        let bytes = std::fs::read(&p).expect("read the save");
        return Some(Save::from_bytes(&bytes).expect("parse the save"));
    }
    if std::env::var("MHNX_SKIP_REAL_SAVE").is_ok() {
        return None;
    }
    panic!(
        "no save at {} - point MHNX_REAL_SAVE at one, or set MHNX_SKIP_REAL_SAVE=1",
        p.display()
    );
}

/// E1: slot+0x62EE is the shared equipment box, not a charm box. Writing a
/// talisman into the first record used to destroy a Great Sword.
#[test]
fn writing_a_charm_never_touches_a_weapon() {
    let Some(src) = real_save() else { return };
    let eqbox = src.equipment_box(0).unwrap();
    let r0 = &eqbox[..REC];
    assert_eq!(charm::record_type(r0), 7, "record 0 is a weapon in this dump");
    assert!(!charm::is_writable(r0));

    let mut s = src.clone();
    let c = Charm { index: 0, slots: 3, skill1: 1, points1: 6, skill2: 200, points2: -2 };
    assert!(!s.set_charm(0, &c), "must refuse a weapon slot");
    assert_eq!(&s.equipment_box(0).unwrap()[..REC], r0, "weapon untouched");
    assert_eq!(s.body(), src.body(), "nothing at all changed");
}

/// E1 cont.: the box is mostly weapons and armour, so the "first free slot"
/// must look for a blank record rather than an index with no talisman.
#[test]
fn the_equipment_box_is_shared_with_weapons_and_armour() {
    let Some(src) = real_save() else { return };
    let eqbox = src.equipment_box(0).unwrap();
    let mut talismans = 0;
    let mut other = 0;
    let mut empty = 0;
    for i in 0..charm::CHARM_COUNT {
        let r = &eqbox[i * REC..(i + 1) * REC];
        if charm::is_empty_record(r) {
            empty += 1;
        } else if charm::is_talisman(r) {
            talismans += 1;
        } else {
            other += 1;
        }
    }
    // Counting them here must agree with the decoder, and both must be real:
    // a save with no talismans would make the rest of this test meaningless.
    assert!(talismans > 100, "the sample save holds hundreds of talismans");
    assert_eq!(talismans, src.charms(0).unwrap().len());
    assert!(other > 100, "the box holds {} weapon/armour records", other);
    assert!(empty > 100, "and {} blank ones", empty);
    // The picker must land on a blank record, never on equipment.
    let pick = charm::first_writable(eqbox).expect("a writable slot");
    assert!(charm::is_writable(&eqbox[pick * REC..(pick + 1) * REC]));
}

/// E2: the first byte is a bitfield. Testing it for exactly 0x06 loses every
/// talisman that carries a level in its top three bits.
#[test]
fn a_talisman_with_level_bits_is_still_found() {
    let Some(src) = real_save() else { return };
    let mut s = src.clone();
    let n_before = s.charms(0).unwrap().len();
    let idx = s.charms(0).unwrap()[0].index;
    let base = s.slot_base(0).unwrap() as usize;
    let o = base + EQ + idx * REC;
    // type 6 in the low five bits, level 1 in the top three.
    s.body_mut()[o] = 0x26;
    assert_eq!(charm::record_type(&s.body()[o..o + REC]), 6);
    assert_eq!(
        s.charms(0).unwrap().len(),
        n_before,
        "the talisman must still be found"
    );
}

/// E3: a freshly created talisman must look like a real one, not a half filled
/// record with rarity 0 and a stale level.
#[test]
fn a_new_charm_record_matches_a_real_one() {
    let Some(src) = real_save() else { return };
    let mut s = src.clone();
    let idx = (0..charm::CHARM_COUNT)
        .find(|i| charm::is_empty_record(&s.equipment_box(0).unwrap()[i * REC..(i + 1) * REC]))
        .expect("a blank equipment slot");
    let c = Charm { index: idx, slots: 3, skill1: 1, points1: 6, skill2: 200, points2: -2 };
    assert!(s.set_charm(0, &c));
    let base = s.slot_base(0).unwrap() as usize;
    let rec = &s.body()[base + EQ + idx * REC..base + EQ + (idx + 1) * REC];

    // Compare against a real talisman field by field.
    let real = &src.body()[base + EQ + 224 * REC..base + EQ + 225 * REC];
    for off in [0x00usize, 0x01, 0x02, 0x10, 0x11, 0x12, 0x13] {
        assert_eq!(rec[off], real[off], "byte {:#04x} differs from a real talisman", off);
    }
    assert!(rec[0x14..].iter().all(|b| *b == 0), "the tail must be clear");
    // Only that one record changed. A blank record only lights up the fields
    // a talisman actually uses, so count positions rather than assume 36.
    let start = base + EQ + idx * REC;
    let changed: Vec<usize> = (0..s.body().len())
        .filter(|i| s.body()[*i] != src.body()[*i])
        .collect();
    assert!(!changed.is_empty(), "the write must land somewhere");
    assert!(
        changed.iter().all(|i| *i >= start && *i < start + REC),
        "bytes outside the target record changed: {:?}",
        changed.iter().filter(|i| **i < start || **i >= start + REC).collect::<Vec<_>>()
    );
}

/// E6: a corrupt DLC section pointer of 0 used to underflow and panic.
#[test]
fn options_survive_a_zero_dlc_pointer() {
    let Some(src) = real_save() else { return };
    let mut s = src.clone();
    s.body_mut()[0x0C..0x10].copy_from_slice(&0u32.to_le_bytes());
    let o = s.options();
    assert_eq!(o.disable_3d, 0);
    assert_eq!(o.circle_pad_pro, 0);
    // And the real values still come out when the pointer is intact.
    assert_eq!(src.options().brightness, src.body()[0x28]);
}

/// E4: brightness is a plain byte. The sample save holds 48, well above the 31
/// the code used to clamp to.
#[test]
fn brightness_is_not_clamped() {
    let Some(src) = real_save() else { return };
    let v = src.options().brightness;
    assert!(
        v > 31,
        "the sample save holds {}, so 0..=31 would destroy it",
        v
    );
    // Prove the round trip moves the value rather than only reading it back.
    let mut probe = src.clone();
    let mut o = probe.options();
    o.brightness = 7;
    probe.set_options(&o);
    assert_eq!(probe.options().brightness, 7);
    let mut s = src.clone();
    let mut o = s.options();
    o.brightness = 200;
    s.set_options(&o);
    assert_eq!(s.options().brightness, 200);
}
/// The whole save-editor loop: open a quest, change nothing, write it back.
/// The save must come out byte for byte identical, otherwise the tool rewrites
/// bytes the game owns every time someone merely looks at a quest.
#[test]
fn writing_an_untouched_quest_back_is_a_no_op() {
    let Some(src) = real_save() else { return };
    let before = src.to_bytes();
    let mut s = src.clone();
    let q = mhnx_core::dlc::list_quests(&s)[0].clone();
    let slot = mhnx_core::dlc::install_quest(&mut s, q.quest_id, &q.data).unwrap();
    assert_eq!(slot, q.slot);
    let after = s.to_bytes();
    assert_eq!(after.len(), before.len(), "the save size must not change");
    let diff = (0..before.len()).filter(|i| before[*i] != after[*i]).count();
    assert_eq!(diff, 0, "{} bytes changed by a no-op write", diff);
}

/// And an edited quest changes only the bytes it has to.
#[test]
fn editing_a_quest_touches_only_its_own_slot() {
    let Some(src) = real_save() else { return };
    let before = src.to_bytes();
    let mut s = src.clone();
    let q = mhnx_core::dlc::list_quests(&s)[0].clone();
    let mut data = q.data.clone();
    data[0x40] ^= 0xFF;
    mhnx_core::dlc::install_quest(&mut s, q.quest_id, &data).unwrap();
    let after = s.to_bytes();

    // Every changed byte must sit inside that one slot.
    let sec = s.dlc_section_offset() as usize;
    let start = 0x24 + sec + mhnx_core::dlc::QUEST_ARRAY_OFFSET + q.slot * mhnx_core::dlc::QUEST_SLOT_SIZE;
    let end = start + mhnx_core::dlc::QUEST_SLOT_SIZE;
    let outside: Vec<usize> = (0..before.len())
        .filter(|i| before[*i] != after[*i])
        .filter(|i| *i < start || *i >= end)
        .collect();
    assert!(outside.is_empty(), "bytes outside the slot changed: {:?}", &outside[..outside.len().min(8)]);

    // The other quests are untouched.
    let a = mhnx_core::dlc::list_quests(&src);
    let b = mhnx_core::dlc::list_quests(&mhnx_core::save::Save::from_bytes(&after).unwrap());
    for (x, y) in a.iter().zip(b.iter()).skip(1) {
        assert_eq!(x.quest_id, y.quest_id);
        assert_eq!(x.data, y.data);
    }
}