//! The checks that stop a write before it can lose a save.
//!
//! Nothing here starts an emulator, and nothing here touches the real save: the
//! files are made in the temp directory and removed again.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mhnx_core::safety::{self, Fingerprint};

fn temp_path(tag: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "mhnx_safety_{}_{}_{}.sav",
        std::process::id(),
        tag,
        now
    ))
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).expect("the temp file is writable");
}

/// A fingerprint with a fixed timestamp and hash, for the comparisons.
fn at(size: u64, secs: u64, hash: u64) -> Fingerprint {
    Fingerprint {
        size,
        modified: Some(UNIX_EPOCH + Duration::from_secs(secs)),
        hash,
    }
}

#[test]
fn the_same_bytes_fingerprint_the_same_way() {
    let path = temp_path("same");
    write_file(&path, b"save");
    let first = safety::fingerprint(&path).expect("the file fingerprints");
    let second = safety::fingerprint(&path).expect("the file fingerprints");
    assert_eq!(first, second);
    assert_eq!(first.size, 4);
    assert!(first.modified.is_some(), "the temp filesystem reports a time");
    assert!(!safety::is_stale(&first, &second));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_deleted_file_cannot_be_fingerprinted() {
    let path = temp_path("missing");
    let _ = std::fs::remove_file(&path);
    let error = safety::fingerprint(&path).expect_err("a missing file is an error");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn an_unchanged_fingerprint_is_not_stale() {
    let before = at(100, 1_000, 7);
    assert!(!safety::is_stale(&before, &before.clone()));
    assert!(safety::changes(&before, &before.clone()).is_empty());
}

#[test]
fn a_size_change_is_stale() {
    let loaded = at(100, 1_000, 7);
    let current = at(101, 1_000, 7);
    assert!(safety::is_stale(&loaded, &current));
    let changed = safety::changes(&loaded, &current);
    assert!(
        changed.iter().any(|line| line.contains("サイズ")),
        "{:?}",
        changed
    );
}

#[test]
fn a_content_change_is_stale_even_at_the_same_size() {
    let loaded = at(100, 1_000, 7);
    let current = at(100, 1_000, 8);
    assert!(safety::is_stale(&loaded, &current));
    let changed = safety::changes(&loaded, &current);
    assert!(
        changed.iter().any(|line| line.contains("内容")),
        "{:?}",
        changed
    );
}

#[test]
fn a_timestamp_change_is_stale() {
    let loaded = at(100, 1_000, 7);
    let current = at(100, 1_001, 7);
    assert!(safety::is_stale(&loaded, &current));
    let changed = safety::changes(&loaded, &current);
    assert!(
        changed.iter().any(|line| line.contains("更新時刻")),
        "{:?}",
        changed
    );
}

#[test]
fn a_missing_timestamp_on_one_side_is_stale() {
    let loaded = Fingerprint {
        size: 100,
        modified: None,
        hash: 7,
    };
    let current = at(100, 1_000, 7);
    // One side cannot show the file is unchanged, so it counts as changed.
    assert!(safety::is_stale(&loaded, &current));
    assert!(safety::is_stale(&current, &loaded));
    // Two sides that both lack a timestamp are equal, and that is all the
    // check can say; the size and the hash still have to agree.
    let also_unknown = Fingerprint {
        size: 100,
        modified: None,
        hash: 7,
    };
    assert!(!safety::is_stale(&loaded, &also_unknown));
}

#[test]
fn a_same_size_edit_on_disk_is_stale() {
    let path = temp_path("edited");
    write_file(&path, b"aaaaa");
    let loaded = safety::fingerprint(&path).expect("the file fingerprints");
    write_file(&path, b"bbbbb");
    let current = safety::fingerprint(&path).expect("the file fingerprints");
    assert_eq!(loaded.size, current.size);
    assert_ne!(loaded.hash, current.hash);
    assert!(safety::is_stale(&loaded, &current));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_read_only_file_says_so() {
    let path = temp_path("readonly");
    write_file(&path, b"save");
    assert!(!safety::is_read_only(&path));

    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).expect("the mark can be set");
    assert!(safety::is_read_only(&path));

    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).expect("the mark can be cleared");
    assert!(!safety::is_read_only(&path));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_process_names_of_the_emulators_are_recognised() {
    // Versioned builds and forks carry the same token, which is why the match
    // is a substring and not an equality.
    for name in [
        "yuzu.exe",
        "yuzu-early-access.exe",
        "suyu.exe",
        "Sudachi.exe",
        "citron.exe",
        "Torzu.exe",
        "Nuzu.exe",
        "Ryujinx.exe",
        "Ryujinx.Canary.exe",
        "Ryubing.exe",
        "Kenji-NX.exe",
        "Pomelo.exe",
        "citra.exe",
        "citra-qt.exe",
        "Lime3DS.exe",
        "azahar.exe",
        "Panda3DS.exe",
        "Ryujinx",
        "YUZU.EXE",
    ] {
        assert!(
            safety::emulator_name(name).is_some(),
            "{} must be recognised as an emulator",
            name
        );
    }
}

#[test]
fn unrelated_processes_are_left_alone() {
    // These run on an ordinary desktop and must never be mistaken for an
    // emulator: a false match would block every write until --force.
    for name in [
        "steam.exe",
        "steamwebhelper.exe",
        "node.exe",
        "tasklist.exe",
        "explorer.exe",
        "powershell.exe",
        "python.exe",
        "mhnx.exe",
        "XXSE.nro",
        "Discord.exe",
        "msedgewebview2.exe",
    ] {
        assert!(
            safety::emulator_name(name).is_none(),
            "{} must not be taken for an emulator",
            name
        );
    }
}

#[test]
fn the_live_scan_only_names_documented_emulators() {
    // The test starts no emulator, so the only ones it can see are those the
    // user already has open. Whatever it reports has to be one this module
    // knows, and the list has to be sorted and duplicate free.
    let found = safety::running_emulators();
    for name in &found {
        assert!(
            safety::emulator_name(name).is_some(),
            "{} was reported but is not a documented emulator",
            name
        );
    }
    let mut sorted = found.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(found, sorted, "the scan returns a sorted, duplicate-free list");
}

#[test]
fn a_lock_needs_both_an_emulator_and_a_file() {
    let path = temp_path("locked");
    let running = !safety::running_emulators().is_empty();
    assert!(!safety::is_locked_by_emulator(&path), "no file, no lock");
    write_file(&path, b"save");
    assert_eq!(safety::is_locked_by_emulator(&path), running);
    let _ = std::fs::remove_file(&path);
    assert!(!safety::is_locked_by_emulator(&path));
}

/// The scan has to find nothing on a machine with no emulator open. This was
/// written on a machine that had "Ryujinx.exe --help" running, where an empty
/// result would be the scan's failure rather than its success, so the test is
/// ignored; run it on a clean machine with
/// "cargo test -p mhnx-core --test safety_checks -- --ignored".
#[test]
#[ignore = "the machine this was written on has Ryujinx.exe open"]
fn the_scan_is_empty_when_no_emulator_is_running() {
    assert_eq!(
        safety::running_emulators(),
        Vec::<String>::new(),
        "no emulator is started by this test"
    );
}
