//! Naming and rotation for save snapshots.
//!
//! The desktop build kept exactly one copy (`<save>.bak`) and the console build
//! kept none of its own, so an edit made two saves ago was already unreachable.
//! Nothing here touches the filesystem: the caller lists names, asks which to
//! drop, and does the deleting, which keeps the whole thing testable and lets
//! the same rules run on the console.

use alloc::string::String;
use alloc::vec::Vec;

/// A timestamp the names can be sorted by, from a Unix second count.
///
/// `YYYYMMDD-HHMMSS`, so plain string ordering is chronological.
pub fn stamp(unix: u64) -> String {
    use alloc::format;
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    // Civil-from-days, Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m,
        d,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// File name for a snapshot of `stem` taken at `unix`.
pub fn name(stem: &str, unix: u64) -> String {
    use alloc::format;
    format!("{}-{}.sav", stem, stamp(unix))
}

/// Which names to delete so that at most `keep` remain.
///
/// `existing` is the snapshot names in any order and `prefix` is the stem they
/// were made from; anything not matching the prefix is ignored. The indices
/// returned point into `existing`. Oldest go first, and `keep == 0` means keep
/// none of them.
pub fn older_than(existing: &[String], prefix: &str, keep: usize) -> Vec<usize> {
    let mut matching: Vec<usize> = existing
        .iter()
        .enumerate()
        .filter(|(_, n)| n.starts_with(prefix) && n.ends_with(".sav"))
        .map(|(i, _)| i)
        .collect();
    // The timestamp is fixed width inside the name, so a plain sort is
    // chronological; names are unique because snapshots are seconds apart.
    matching.sort_by(|a, b| existing[*a].cmp(&existing[*b]));
    if matching.len() <= keep {
        return Vec::new();
    }
    matching[..matching.len() - keep].to_vec()
}