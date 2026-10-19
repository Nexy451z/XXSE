//! Cross-checks against the values published by the MHXX/MHGU Save Manager
//! (https://github.com/SilverJolteon/SilverJolteon.github.io).
//!
//! The web app reads a few fields from different offsets than we did, and it
//! decodes two areas we treated as opaque. These tests pin down what is right.

use std::path::PathBuf;

use mhnx_core::charm::{self, Charm};
use mhnx_core::save::Save;

fn real_save() -> PathBuf {
    PathBuf::from(r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system")
}

#[test]
fn slot_offsets_come_from_the_save_itself() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let s = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    // The web app reads these three u32s at body +0x10 / +0x14 / +0x18, and
    // they must agree with the values our editor hard coded for MHXX.
    assert_eq!(s.slot_bases(), [0x126474, 0x244C34, 0x3633F4]);
    // body +0x0C is the download-quest section offset.
    assert_eq!(s.dlc_section_offset(), 0xB280);
    // Stride 0x11E7C0 = 0x11D088 (character) + 0x1738 (shoutouts).
    assert_eq!(s.slot_bases()[1] - s.slot_bases()[0], 0x11E7C0);
}

#[test]
fn options_match_the_web_app_layout() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let s = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    let o = s.options();
    // The three switches are single bytes holding 0 or 1; brightness is a
    // plain byte. Real saves hold brightness well above 31, so never clamp it.
    assert!(o.rumble <= 1);
    assert!(o.disable_3d <= 1);
    assert!(o.circle_pad_pro <= 1);

    // Writing back what we read changes nothing.
    let mut s2 = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    let o2 = s2.options();
    s2.set_options(&o2);
    assert_eq!(s2.options(), o2);

    // And a changed brightness comes back, leaving the others alone.
    let mut o3 = o2;
    o3.brightness = o2.brightness.wrapping_sub(7);
    s2.set_options(&o3);
    assert_eq!(s2.options().brightness, o3.brightness);
    assert_eq!(s2.options().rumble, o2.rumble);
}

#[test]
fn download_palicoes_are_found_in_the_shared_block() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let s = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    let cats = s.dlc_palicoes();
    assert_eq!(cats.len(), 50, "there are always 50 download Palico slots");
    // This save has two: ぽかぽか and ミケゴールド.
    assert_eq!(cats[0].id, 0x64);
    assert_eq!(cats[0].name, "ぽかぽか");
    assert_eq!(cats[1].id, 0x65);
    assert_eq!(cats[1].name, "ミケゴールド");
    assert_eq!(cats[2].name, "", "the rest are empty");
}

#[test]
fn talismans_live_in_the_shared_equipment_box() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let s = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    // The charm box is 2000 x 36 bytes at slot + 0x62EE and ends exactly where
    // the palico equipment block starts.
    assert_eq!(0x62EE + charm::CHARM_BOX_SIZE, 0x17C2E);
    // The accessor works whatever the box holds.
    let before = s.charms(0).unwrap();
    // 0x62EE is the shared equipment box, so a talisman may only go into a
    // record that is blank or already a talisman. Picking "the first index with
    // no decoded charm" would land on a weapon.
    let free = mhnx_core::charm::first_writable(s.equipment_box(0).unwrap())
        .expect("a record a talisman may be written to");
    assert!(mhnx_core::charm::is_writable(
        &s.equipment_box(0).unwrap()[free * 36..(free + 1) * 36]
    ));

    // Writing one must survive a save round trip.
    let mut s2 = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    let c = Charm {
        index: free,
        slots: 3,
        skill1: 1,
        points1: 6,
        skill2: 200,
        points2: -2,
    };
    assert!(s2.set_charm(0, &c));
    let bytes = s2.to_bytes();
    let back = Save::from_bytes(&bytes).unwrap();
    let list = back.charms(0).unwrap();
    assert_eq!(list.len(), before.len() + 1);
    let got = list.iter().find(|x| x.index == free).expect("the charm we wrote");
    assert_eq!(*got, c);
    assert_eq!(got.skill1_name(), Some("毒"));
    // Removing it again leaves the box as it was.
    let mut s3 = back;
    assert!(s3.clear_charm(0, free));
    assert_eq!(
        Save::from_bytes(&s3.to_bytes()).unwrap().charms(0).unwrap().len(),
        before.len()
    );
}

#[test]
fn gender_is_written_to_both_copies() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let src = std::fs::read(&p).unwrap();
    let mut s = Save::from_bytes(&src).unwrap();
    let mut c = s.read_character(0).unwrap();
    let before = c.gender;
    c.gender = if before == 0 { 1 } else { 0 };
    s.write_character(&c).unwrap();
    let out = s.to_bytes();
    // Slot 0 base + 0x2A (the character select copy) and + 0x23B4B (the full
    // record) must agree, or the slot list shows a stale value.
    let body = &out[0x24..];
    let base = 0x126474usize;
    assert_eq!(body[base + 0x2A], c.gender);
    assert_eq!(body[base + 0x23B4B], c.gender);
    // Everything else is untouched.
    let orig = Save::from_bytes(&src).unwrap();
    assert_eq!(orig.read_character(0).unwrap().name, s.read_character(0).unwrap().name);
}
// ---------------------------------------------------------------------------
// Features taken from the save manager: whole-slot files and download Palicoes.
// ---------------------------------------------------------------------------

use mhnx_core::dlcats::{self, Lang};

#[test]
fn slot_export_and_import_round_trip() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let src = std::fs::read(&p).unwrap();
    let s = Save::from_bytes(&src).unwrap();
    let stride = s.slot_stride();
    assert_eq!(stride, 0x11E7C0, "MHXX stride");

    let blob = s.export_slot(0).unwrap();
    assert_eq!(blob.len(), stride);

    // Import it into another slot of a fresh copy and check it arrives.
    let target = (0..3).find(|i| *i != 0).expect("a second slot");
    let mut dst = Save::from_bytes(&src).unwrap();
    dst.import_slot(target, &blob).unwrap();
    assert!(dst.slot_used(target));
    let a = dst.read_character(0).unwrap();
    let b = dst.read_character(target).unwrap();
    assert_eq!(a.name, b.name);
    assert_eq!(a.funds, b.funds);
    assert_eq!(a.play_time, b.play_time);

    // The item box is part of the slot, so it must come across too.
    assert_eq!(
        dst.read_item_box(0).unwrap(),
        dst.read_item_box(target).unwrap()
    );

    // And it survives a save/load round trip.
    let out = dst.to_bytes();
    let back = Save::from_bytes(&out).unwrap();
    assert_eq!(back.read_character(target).unwrap().name, a.name);
}

#[test]
fn slot_import_rejects_the_wrong_size() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let mut s = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    let bad = vec![0u8; 1234];
    assert!(s.import_slot(1, &bad).is_err());
    // The character-only size from the reference tool is accepted as well.
    let ok = vec![0u8; 0x11D088];
    assert!(s.import_slot(1, &ok).is_ok());
}

#[test]
fn download_palicoes_can_be_installed_and_removed() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let src = std::fs::read(&p).unwrap();
    let mut s = Save::from_bytes(&src).unwrap();
    let before = s.dlc_palicoes().iter().filter(|c| c.id != 0).count();

    // Pick an id this save does not have yet, so the test works on any dump.
    let have: Vec<u8> = s.dlc_palicoes().iter().map(|c| c.id).collect();
    let id = dlcats::all_ids()
        .into_iter()
        .find(|i| !have.contains(i))
        .expect("a Palico this save does not have");
    let want_name = dlcats::record_name(dlcats::record(Lang::Jp, id).unwrap());

    let first_free = s
        .dlc_palicoes()
        .iter()
        .find(|c| c.id == 0)
        .map(|c| c.index)
        .expect("a free Palico slot");
    let slot = s.install_dlc_palico(Lang::Jp, id).unwrap();
    assert_eq!(slot, first_free, "must take the first free slot");
    assert!(s.has_dlc_palico(id));
    let cats = s.dlc_palicoes();
    assert_eq!(cats[slot].name, want_name);
    assert_eq!(cats[slot].id, id);

    // Installing it twice is refused.
    assert!(s.install_dlc_palico(Lang::Jp, id).is_err());

    // Removing it frees the slot again.
    assert!(s.remove_dlc_palico(slot));
    assert!(!s.has_dlc_palico(id));
    assert_eq!(s.dlc_palicoes()[slot].id, 0);

    // And the change survives a file round trip.
    let out = s.to_bytes();
    let back = Save::from_bytes(&out).unwrap();
    assert_eq!(
        back.dlc_palicoes().iter().filter(|c| c.id != 0).count(),
        before
    );
}

#[test]
fn installed_palico_survives_a_save_round_trip() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let mut s = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    s.install_dlc_palico(Lang::En, 0x67).unwrap();
    let back = Save::from_bytes(&s.to_bytes()).unwrap();
    let c = back.dlc_palicoes().into_iter().find(|c| c.id == 0x67).unwrap();
    assert_eq!(c.name, "Nekomichi");
    // Everything else is untouched: the shared block around it must not move.
    let orig = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    assert_eq!(orig.options(), back.options());
    assert_eq!(orig.slot_bases(), back.slot_bases());
}
