//! Integration tests that run against the real data on this machine.
//!
//! They are skipped (not failed) when the referenced files are absent, so the
//! suite still passes on a clean checkout.

use std::path::{Path, PathBuf};

use mhnx_core::arc::Arc;
use mhnx_core::save::{self, Save, SaveKind, BODY_SIZE};

fn quest_dir() -> PathBuf {
    PathBuf::from(r"C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest")
}
fn real_save() -> PathBuf {
    PathBuf::from(r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system")
}

#[test]
fn parses_the_real_ryujinx_save() {
    let p = real_save();
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return;
    }
    let bytes = std::fs::read(&p).unwrap();
    let s = Save::from_bytes(&bytes).expect("save must parse");
    assert_eq!(s.kind(), SaveKind::Switch);
    assert_eq!(s.kind().body_size(), BODY_SIZE);

    // The container header is a pure function of the file name.
    assert_eq!(&s.header()[0x14..0x18], &[0x74, 0xEE, 0xB2, 0x36]);
    assert_eq!(&s.header()[0x1C..0x20], &(BODY_SIZE as u32).to_le_bytes());

    // Slot geometry. The bases live in the save, and the three slots are one
    // stride apart from each other.
    let bases = s.slot_bases();
    assert_eq!(bases[1] - bases[0], bases[2] - bases[1]);
    assert_eq!(s.slot_stride() as u32, bases[1] - bases[0]);
    assert!(s.slot_used(0), "the save always has a first character");

    // Whatever slot 0 holds must decode, and the copy the character select
    // screen reads must agree with the full record.
    let c = s.read_character(0).unwrap();
    assert_eq!(c.name, c.name_preview, "the slot list copy must agree");
    assert!(c.name.len() <= 32);

    // The download-quest section pointer is fixed for MHXX.
    assert_eq!(s.dlc_section_offset(), 0xB280);
    // Every quest it holds must be self describing.
    for q in mhnx_core::dlc::list_quests(&s) {
        assert!(q.size > 0 && (q.size as usize) <= mhnx_core::dlc::QUEST_MAX_PAYLOAD);
        assert_eq!(q.data.len(), q.size as usize);
    }

    // Round trip must be byte identical.
    assert_eq!(s.to_bytes(), bytes, "save round trip changed bytes");

    // Item box decode/encode round trip on real data.
    let items = s.read_item_box(0).unwrap();
    let mut s2 = s.clone();
    s2.write_item_box(0, &items).unwrap();
    assert_eq!(s2.body(), s.body(), "item box round trip changed bytes");
}

#[test]
fn writes_a_character_and_reads_it_back() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let bytes = std::fs::read(&p).unwrap();
    let mut s = Save::from_bytes(&bytes).unwrap();
    let mut c = s.read_character(0).unwrap();
    c.funds = 9_999_999;
    c.name = "TestHunter".to_string();
    s.write_character(&c).unwrap();
    let back = s.read_character(0).unwrap();
    assert_eq!(back.funds, 9_999_999);
    assert_eq!(back.name, "TestHunter");
    // Both copies must be updated.
    assert_eq!(back.name_preview, "TestHunter");
    assert_eq!(s.to_bytes().len(), bytes.len());
}

#[test]
fn three_ds_conversion_round_trips() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let bytes = std::fs::read(&p).unwrap();
    let s = Save::from_bytes(&bytes).unwrap();

    // Switch -> 3DS body -> Switch must reproduce the file exactly,
    // because the header is synthesised from the name alone.
    let body = s.to_3ds_bytes();
    assert_eq!(body.len(), BODY_SIZE);
    let rebuilt = Save::from_3ds_body(&body, "system").unwrap();
    assert_eq!(rebuilt.to_bytes(), bytes);
    let rebuilt_b = Save::from_3ds_body(&body, "system_backup").unwrap();
    assert_eq!(&rebuilt_b.to_bytes()[0x14..0x18], &[0x92, 0x9B, 0x81, 0xD0]);
    assert_eq!(rebuilt_b.to_bytes()[36..], bytes[36..]);
}

#[test]
fn parses_every_shipped_quest_archive() {
    let dir = quest_dir();
    if !dir.exists() {
        eprintln!("skipping: {} not found", dir.display());
        return;
    }
    let mut n_files = 0usize;
    let mut n_entries = 0usize;
    let mut reparsed = 0usize;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("arc") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let arc = Arc::parse(&bytes).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        n_files += 1;
        n_entries += arc.entries.len();
        assert!(arc.entries.iter().any(|e| e.name.contains("questData")), "{}", path.display());

        // Repack and reparse. The strongest check is byte identity: the writer
        // reuses the compressed streams and the original offsets, so a file
        // nobody edited must come back exactly. The first payload does NOT
        // always sit at align32(12 + 80 * entries) - q1010165 starts 16 bytes
        // later - which is why the offsets are preserved rather than recomputed.
        let out = arc.to_bytes().unwrap();
        assert_eq!(out.len(), bytes.len(), "repack changed the size of {}", path.display());
        assert_eq!(out, bytes, "repack changed the bytes of {}", path.display());
        let back = Arc::parse(&out).unwrap();
        assert_eq!(back.entries.len(), arc.entries.len());
        for (a, b) in arc.entries.iter().zip(back.entries.iter()) {
            assert_eq!(a.name, b.name, "{}", path.display());
            assert_eq!(a.type_hash, b.type_hash);
            assert_eq!(a.data, b.data, "payload mismatch in {}", path.display());
        }
        reparsed += 1;
    }
    eprintln!("parsed {} archives / {} entries / {} repacked", n_files, n_entries, reparsed);
    assert!(n_files >= 100, "expected the shipped quest set");
}

#[test]
fn quest_mib_ids_match_file_names() {
    let dir = quest_dir();
    if !dir.exists() {
        return;
    }
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !stem.starts_with('q') {
            continue;
        }
        let expected: u32 = match stem[1..].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let bytes = std::fs::read(&path).unwrap();
        let arc = Arc::parse(&bytes).unwrap();
        let mib = arc.by_ext("mib").expect("mib entry");
        let m = mhnx_core::quest::Mib::from_bytes(&mib.data).unwrap();
        assert_eq!(m.quest_id(), expected, "{}", path.display());
        checked += 1;
    }
    eprintln!("checked {} mib quest ids", checked);
    assert!(checked >= 100);
}

#[test]
fn gmd_round_trips_for_every_quest() {
    let dir = quest_dir();
    if !dir.exists() {
        return;
    }
    let mut n = 0usize;
    let mut titles = 0usize;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("arc") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let arc = Arc::parse(&bytes).unwrap();
        let gmd_entry = arc.by_ext("gmd").expect("gmd entry");
        let g = mhnx_core::quest::Gmd::from_bytes(&gmd_entry.data)
            .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        assert!(g.strings.len() >= 5, "{}: only {} strings", path.display(), g.strings.len());
        assert!(g.key.starts_with("questData_"), "{}: key {}", path.display(), g.key);
        if g.title().map(|s| !s.is_empty()).unwrap_or(false) {
            titles += 1;
        }
        // Byte exact round trip.
        assert_eq!(g.to_bytes(), gmd_entry.data, "gmd round trip failed for {}", path.display());
        n += 1;
    }
    eprintln!("gmd round tripped for {} quests, {} with a title", n, titles);
    assert!(n >= 100);
}

#[test]
fn blank_templates_agree_with_computed_headers() {
    // The blank saves shipped by MHGU-MHXX-Save-Converter-Script are an
    // independent source of truth for the container header.
    let base = Path::new(r"C:\anyWorkspace\nx\mhxx\save_editor\repos\MHGU-MHXX-Save-Converter-Script");
    let cases = [
        ("Blank_Switch_Save/MHXX Switch Version/system", "system", false),
        ("Blank_Switch_Save/MHXX Switch Version/system_backup", "system_backup", false),
        ("Blank_Switch_Save/MHGU/system", "system", true),
        ("Blank_Switch_Save/MHGU/system_backup", "system_backup", true),
    ];
    let mut checked = 0;
    for (rel, name, mhgu) in cases {
        let p = base.join(rel);
        if !p.exists() {
            continue;
        }
        let bytes = std::fs::read(&p).unwrap();
        let s = Save::from_bytes(&bytes).unwrap();
        assert_eq!(s.kind(), if mhgu { SaveKind::Mhgu } else { SaveKind::Switch });
        let body_len = bytes.len() - 36;
        let expect = save::switch_header(name, body_len);
        assert_eq!(&bytes[..36], &expect[..], "{}", p.display());
        assert_eq!(s.to_bytes(), bytes, "{}", p.display());
        checked += 1;
    }
    eprintln!("verified {} blank templates", checked);
}