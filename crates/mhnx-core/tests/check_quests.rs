//! Run the quest checker over every shipped quest and over the real save.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mhnx_core::arc::Arc;
use mhnx_core::check::{check_quest, clear_one_player_only, Severity, OFF_CONDITION1, ONE_PLAYER_ONLY};
use mhnx_core::save::Save;

fn quest_dir() -> PathBuf {
    PathBuf::from(r"C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest")
}
fn real_save() -> PathBuf {
    PathBuf::from(r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system")
}

#[test]
fn checker_reports_the_one_player_only_flag_on_the_sample_quest() {
    let p = quest_dir().join("q1010165.arc");
    if !p.exists() {
        return;
    }
    let mut a = Arc::parse(&std::fs::read(&p).unwrap()).unwrap();

    // The shipped sample really does carry the value Quedixx forces.
    let mib = a.by_ext("mib").unwrap();
    assert_eq!(mib.data[OFF_CONDITION1], ONE_PLAYER_ONLY);

    let findings = check_quest(&a);
    let f = findings
        .iter()
        .find(|f| f.code == "one-player-only")
        .expect("the checker must flag it");
    assert_eq!(f.severity, Severity::Warn);
    assert!(f.fixable);

    // The fix changes exactly that one byte.
    let before = clear_one_player_only(&mut a, 0x00).unwrap();
    assert_eq!(before, ONE_PLAYER_ONLY);
    let after = a.by_ext("mib").unwrap();
    assert_eq!(after.data[OFF_CONDITION1], 0x00);
    assert!(check_quest(&a).iter().all(|f| f.code != "one-player-only"));

    // Everything else in the archive is untouched.
    let orig = Arc::parse(&std::fs::read(&p).unwrap()).unwrap();
    for (x, y) in a.entries.iter().zip(orig.entries.iter()) {
        if x.name == after.name {
            assert_ne!(x.data, y.data);
        } else {
            assert_eq!(x.data, y.data, "{} changed", y.name);
        }
    }
}

#[test]
fn checker_runs_over_every_shipped_quest() {
    let dir = quest_dir();
    if !dir.exists() {
        return;
    }
    let mut codes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut errors = 0usize;
    let mut n = 0usize;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) != Some("arc") {
            continue;
        }
        let a = Arc::parse(&std::fs::read(&p).unwrap()).unwrap();
        let findings = check_quest(&a);
        n += 1;
        for f in &findings {
            *codes.entry(f.code).or_default() += 1;
            if f.severity == Severity::Error {
                errors += 1;
            }
        }
    }
    eprintln!("checked {} quests, {} error level findings", n, errors);
    for (code, count) in &codes {
        eprintln!("  {:<20} {}", code, count);
    }
    assert!(n >= 100);
    // The shipped quest set is known good, so nothing may be an error.
    assert_eq!(errors, 0, "real quest files must not produce errors");
    // The archive has no quest link table issues and every quest has a .mib.
    assert!(!codes.contains_key("mib-missing"));
    assert!(!codes.contains_key("qdl-missing"));
}

#[test]
fn checker_reports_the_real_save_as_clean() {
    let p = real_save();
    if !p.exists() {
        return;
    }
    let s = Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap();
    let f = mhnx_core::check::check_save(&s);
    for x in &f {
        eprintln!("[{}] {}", x.code, x.message);
    }
    assert!(f.iter().all(|x| x.severity != Severity::Error));
}

#[test]
fn checker_detects_a_dummy_only_reward_set() {
    // Build a synthetic archive whose only .rem is the dummy.
    let src = quest_dir().join("q1010165.arc");
    if !src.exists() {
        return;
    }
    let mut a = Arc::parse(&std::fs::read(&src).unwrap()).unwrap();
    let before = a.entries.len();
    a.entries
        .retain(|e| !(e.name.contains("\\rem\\") && !e.name.ends_with("rem_000000")));
    assert!(a.entries.len() < before, "some reward files should have been dropped");
    let f = check_quest(&a);
    assert!(
        f.iter().any(|x| x.code == "reward-dummy-only"),
        "expected reward-dummy-only, got {:?}",
        f.iter().map(|x| x.code).collect::<Vec<_>>()
    );
}