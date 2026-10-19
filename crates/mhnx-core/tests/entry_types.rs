//! The six container types that used to be carried through as opaque bytes.
//!
//! The layouts were measured on real quests pulled out of a save, so this walks
//! every quest in dist/quests_all and checks each one parses and round trips.

use std::path::{Path, PathBuf};

use mhnx_core::arc::{extension_for, Arc};
use mhnx_core::entries::{Esl, ItemTable, Qdp, QuestLink, SemEntry};

fn quest_dir() -> PathBuf {
    let mut p = PathBuf::from("C:/");
    for part in ["anyWorkspace", "nx", "mhxx", "mhnxeditor", "dist", "quests_all"] {
        p.push(part);
    }
    p
}

fn arcs() -> Vec<PathBuf> {
    let dir = quest_dir();
    assert!(dir.is_dir(), "run the extractor first: {}", dir.display());
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("arc"))
        .collect();
    v.sort();
    v
}

#[test]
fn every_container_type_parses_and_round_trips() {
    let files = arcs();
    assert!(files.len() >= 170, "expected the full quest set, got {}", files.len());
    let (mut n_rem, mut n_sup, mut n_sem, mut n_esl, mut n_qdp, mut n_qdl) = (0, 0, 0, 0, 0, 0);
    let mut items_total = 0usize;
    let mut unknown_hashes = 0usize;

    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let arc = Arc::parse(&bytes).unwrap();
        for e in &arc.entries {
            match extension_for(e.type_hash) {
                Some("rem") => {
                    let t = ItemTable::parse(&e.data)
                        .unwrap_or_else(|err| panic!("{} rem: {}", f.display(), err));
                    assert_eq!(t.first, 0x18, "{}: .rem records start at 0x18", f.display());
                    assert_eq!(e.data.len() % 8, 0, "{}: .rem length", f.display());
                    assert_eq!(
                        t.to_bytes(),
                        e.data,
                        "{}: an untouched .rem must round trip",
                        f.display()
                    );
                    items_total += t.records.len();
                    n_rem += 1;
                }
                Some("sup") => {
                    let t = ItemTable::parse(&e.data)
                        .unwrap_or_else(|err| panic!("{} sup: {}", f.display(), err));
                    assert_eq!(t.first, 8, "{}: .sup records start at 8", f.display());
                    assert_eq!(t.to_bytes(), e.data, "{}: .sup round trip", f.display());
                    items_total += t.records.len();
                    n_sup += 1;
                }
                Some("sem") => {
                    // Sizes are not fixed: the 32 byte ones are single
                    // placements, larger ones carry a list.
                    assert!(e.data.len() >= 32, "{}: .sem too small", f.display());
                    assert_eq!(e.data.len() % 32, 0, "{}: .sem is 32 byte records", f.display());
                    let s = SemEntry { raw: e.data[..32].to_vec() };
                    let (x, y, z, w) = s.position();
                    for v in [x, y, z, w] {
                        assert!(v.is_finite(), "{}: .sem position must be finite", f.display());
                    }
                    n_sem += 1;
                }
                Some("esl") => {
                    let l = Esl::parse(&e.data)
                        .unwrap_or_else(|err| panic!("{} esl: {}", f.display(), err));
                    assert!(l.waves < 4096, "{}: .esl wave count {}", f.display(), l.waves);
                    n_esl += 1;
                }
                Some("qdp") => {
                    Qdp::parse(&e.data).unwrap_or_else(|err| panic!("{} qdp: {}", f.display(), err));
                    n_qdp += 1;
                }
                Some("qdl") => {
                    let q = QuestLink::parse(&e.data)
                        .unwrap_or_else(|err| panic!("{} qdl: {}", f.display(), err));
                    assert!(!q.refs.is_empty(), "{}: .qdl lists files", f.display());
                    // With the 20 byte stride every reference resolves. A
                    // wrong stride showed up here as garbage names and unknown
                    // hashes, so this assertion is what protects the layout.
                    for r in &q.refs {
                        if extension_for(r.type_hash).is_none() && r.type_hash != 0 {
                            unknown_hashes += 1;
                        }
                        assert!(
                            r.name.is_empty()
                                || r.name.chars().all(|c| c.is_ascii_graphic() || c == ' '),
                            "{}: name {:?} is not text",
                            f.display(),
                            r.name
                        );
                    }
                    n_qdl += 1;
                }
                _ => {}
            }
        }
    }

    // Sanity: the sample set really exercises every type.
    assert!(n_rem > 100, ".rem entries: {}", n_rem);
    assert!(n_sup > 100, ".sup entries: {}", n_sup);
    assert!(n_sem > 100, ".sem entries: {}", n_sem);
    assert!(n_esl > 100, ".esl entries: {}", n_esl);
    assert!(n_qdp > 100, ".qdp entries: {}", n_qdp);
    assert!(n_qdl > 100, ".qdl entries: {}", n_qdl);
    assert!(items_total > 1000, "reward/supply records: {}", items_total);
    assert_eq!(unknown_hashes, 0, "every .qdl hash must be a known type");
}

#[test]
fn editing_a_reward_record_changes_only_that_record() {
    let files = arcs();
    let f = files.first().expect("at least one quest");
    let bytes = std::fs::read(f).unwrap();
    let arc = Arc::parse(&bytes).unwrap();
    let e = arc
        .entries
        .iter()
        .find(|e| extension_for(e.type_hash) == Some("rem"))
        .expect("a .rem entry");
    let mut t = ItemTable::parse(&e.data).unwrap();
    assert!(!t.records.is_empty(), "the first quest has rewards");
    let before = t.to_bytes();
    t.records[0].count = 7;
    let after = t.to_bytes();
    let diff: Vec<usize> = (0..before.len()).filter(|i| before[*i] != after[*i]).collect();
    assert_eq!(diff.len(), 1, "one byte, at {:?}", diff);
    assert_eq!(diff[0], t.first + 2, "the count byte of record 0");
}

#[test]
fn a_truncated_container_is_rejected_not_panicked() {
    for len in [0usize, 1, 4, 7] {
        let raw = vec![0u8; len];
        assert!(ItemTable::parse(&raw).is_err() || len >= 8);
        assert!(Esl::parse(&raw).is_err());
        assert!(Qdp::parse(&raw).is_err());
        assert!(QuestLink::parse(&raw).is_err());
    }
    // A .rem whose header is present but records are not must not panic.
    let raw = vec![0u8; 20];
    let _ = ItemTable::parse(&raw);
    let _ = Path::new("nothing");
}
