//! Refusing a rewrite that would lose a save.
//!
//! The reference editor refuses two situations that this tool used to allow: a
//! write while an emulator is running (the emulator keeps the save open and
//! writes its own copy back when it next saves, so the edit disappears), and a
//! write of a file that changed after the command read it (the command would
//! then write the old contents back and the change made in between would be
//! gone).
//!
//! This module is desktop-only. The console build never rewrites the file it
//! booted from - it moves the current save into backups/ first - so it neither
//! needs these checks nor compiles them.

use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// What a file looked like at one moment.
///
/// The size and the hash of the contents are the portable part; the timestamp
/// is extra evidence, and is None when the filesystem does not report one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// Length in bytes.
    pub size: u64,
    /// Last modification time, when the filesystem reports it.
    pub modified: Option<SystemTime>,
    /// FNV-1a of the contents: a change detector, not a security hash.
    pub hash: u64,
}

/// Take the fingerprint of a file.
///
/// The metadata is read on both sides of the read of the contents, so a file
/// that changed while it was being read is reported as an error instead of
/// being fingerprinted with a mixture of two states.
pub fn fingerprint(path: &Path) -> io::Result<Fingerprint> {
    let before = std::fs::metadata(path)?;
    let bytes = std::fs::read(path)?;
    let after = std::fs::metadata(path)?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(io::Error::other("読んでいる途中でファイルが変わりました"));
    }
    Ok(Fingerprint {
        size: after.len(),
        modified: after.modified().ok(),
        hash: hash_bytes(&bytes),
    })
}

/// FNV-1a over the bytes.
///
/// It only has to notice that a save changed, so speed and portability matter
/// more than collision resistance; nothing here is a security check.
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Did the file change since it was fingerprinted?
///
/// A different timestamp alone counts as a change: the contents may still be
/// the same byte for byte, but something wrote the file after the command read
/// it, and this check exists to not overwrite whatever that was. When one side
/// has no timestamp at all the file cannot be shown to be unchanged, so it is
/// treated as changed. Both cases are what --force is for.
pub fn is_stale(loaded: &Fingerprint, current: &Fingerprint) -> bool {
    if loaded.size != current.size || loaded.hash != current.hash {
        return true;
    }
    match (loaded.modified, current.modified) {
        (Some(a), Some(b)) => a != b,
        (None, None) => false,
        _ => true,
    }
}

/// The fields that differ, in the words the refusal message uses.
pub fn changes(loaded: &Fingerprint, current: &Fingerprint) -> Vec<String> {
    let mut out = Vec::new();
    if loaded.size != current.size {
        out.push(format!("サイズ {} → {} バイト", loaded.size, current.size));
    }
    if loaded.hash != current.hash {
        out.push(format!(
            "内容のハッシュ 0x{:016X} → 0x{:016X}",
            loaded.hash, current.hash
        ));
    }
    if loaded.modified != current.modified {
        let ago = match ago_text(current.modified) {
            Some(text) => format!("（{}）", text),
            None => String::new(),
        };
        out.push(format!(
            "更新時刻 {} → {}{}",
            time_text(loaded.modified),
            time_text(current.modified),
            ago
        ));
    }
    out
}

/// A timestamp as the snapshot names spell it, marked as UTC so it cannot be
/// read as local time.
fn time_text(when: Option<SystemTime>) -> String {
    match when.and_then(|t| t.duration_since(UNIX_EPOCH).ok()) {
        Some(d) => format!("{} UTC", crate::snapshot::stamp(d.as_secs())),
        None => String::from("不明"),
    }
}

/// "12 秒前", for the moment the file was last written.
fn ago_text(when: Option<SystemTime>) -> Option<String> {
    let when = when?;
    let secs = SystemTime::now().duration_since(when).ok()?.as_secs();
    Some(match secs {
        0..=119 => format!("{} 秒前", secs),
        120..=7_199 => format!("{} 分前", secs / 60),
        7_200..=172_799 => format!("{} 時間前", secs / 3_600),
        _ => format!("{} 日前", secs / 86_400),
    })
}

/// The emulators whose process name is looked for, as (lower case token, name
/// to print).
///
/// Both console generations are here because MHXX is a 3DS game as well as a
/// Switch one: the save can be open in Citra or Azahar just as it can be open in
/// yuzu. The comparison is a substring of the name without ".exe", so versioned
/// builds and forks (yuzu-early-access.exe, Ryujinx.Canary.exe, citra-qt.exe)
/// are caught by one entry. A missed emulator costs the user a save and a
/// spurious match costs one --force, so the list errs on the side of matching.
const EMULATORS: &[(&str, &str)] = &[
    // Switch
    ("yuzu", "yuzu"),
    ("suyu", "suyu"),
    ("sudachi", "Sudachi"),
    ("citron", "Citron"),
    ("torzu", "Torzu"),
    ("nuzu", "Nuzu"),
    ("ryujinx", "Ryujinx"),
    ("ryubing", "Ryubing"),
    ("kenji", "Kenji-NX"),
    ("pomelo", "Pomelo"),
    // 3DS
    ("citra", "Citra"),
    ("lime3ds", "Lime3DS"),
    ("azahar", "Azahar"),
    ("panda3ds", "Panda3DS"),
];

/// Which emulator a process name belongs to, if any.
pub fn emulator_name(process: &str) -> Option<&'static str> {
    let lower = process.trim().to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    EMULATORS
        .iter()
        .find(|(token, _)| stem.contains(token))
        .map(|(_, name)| *name)
}

/// The emulator processes running now, sorted and without duplicates.
///
/// The list comes from the process table and not from a trial write: opening
/// the save for writing to see whether it is locked can itself change the file,
/// and a failure says nothing about which process holds it.
pub fn running_emulators() -> Vec<String> {
    try_running_emulators().unwrap_or_default()
}

/// The same list, but a process table that cannot be read is an error.
///
/// "I could not look" is not "nothing is running": the caller is expected to
/// refuse on the error rather than read it as an all-clear.
pub fn try_running_emulators() -> io::Result<Vec<String>> {
    let mut found: Vec<String> = process_names()?
        .into_iter()
        .filter(|name| emulator_name(name).is_some())
        .collect();
    found.sort();
    found.dedup();
    Ok(found)
}

/// True when an emulator is running and there is a file at the path for it to
/// hold.
pub fn is_locked_by_emulator(path: &Path) -> bool {
    path.exists() && !running_emulators().is_empty()
}

/// True when the file is marked read-only, so writing it would fail.
///
/// Only the owner's own read-only mark is checked. A file that a permission
/// entry forbids cannot be detected this way; the write then fails and the
/// command reports it, which is why the copy is kept before anything is
/// replaced.
pub fn is_read_only(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o222 == 0
    }
    #[cfg(not(unix))]
    {
        meta.permissions().readonly()
    }
}

/// Every process name this user can see.
#[cfg(windows)]
fn process_names() -> io::Result<Vec<String>> {
    use std::os::windows::process::CommandExt;

    /// The GUI must not flash a console window for every save it writes.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "tasklist が失敗しました（{}）",
            output.status
        )));
    }
    Ok(parse_tasklist(&String::from_utf8_lossy(&output.stdout)))
}

/// The image names out of the CSV output of "tasklist /FO CSV /NH".
///
/// Each row starts with the quoted image name; a row that does not is a notice
/// or a blank line and is skipped. Compiled under test too, so the parsing can
/// be checked on any host.
#[cfg(any(windows, test))]
fn parse_tasklist(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        out.push(String::from(&rest[..end]));
    }
    out
}

/// Every process name this user can see, from /proc.
#[cfg(unix)]
fn process_names() -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        // A process that exits mid-scan makes the iterator fail; skipping it is
        // what the kernel would have reported a moment later anyway.
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        let mut process = match std::fs::read_to_string(dir.join("comm")) {
            Ok(text) => text.trim_end().to_string(),
            Err(_) => String::new(),
        };
        if process.is_empty() {
            // comm is the kernel's short name; argv[0] still carries the name
            // the user sees for a process that set comm to something else.
            if let Ok(bytes) = std::fs::read(dir.join("cmdline")) {
                let first = bytes.split(|b| *b == 0u8).next().unwrap_or(&[]);
                let text = String::from_utf8_lossy(first);
                process = text
                    .rsplit(|c| c == '/' || c == '\\')
                    .next()
                    .unwrap_or("")
                    .to_string();
            }
        }
        if !process.is_empty() {
            out.push(process);
        }
    }
    Ok(out)
}

/// A platform with no process table this module knows how to read.
#[cfg(not(any(windows, unix)))]
fn process_names() -> io::Result<Vec<String>> {
    Err(io::Error::other(
        "この環境ではプロセスの一覧を読めません".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasklist_rows_give_the_image_name() {
        let text = "\"Ryujinx.exe\",\"26584\",\"Console\",\"1\",\"150,000 K\"\r\n\
                    \"yuzu-early-access.exe\",\"1234\",\"Console\",\"1\",\"10,000 K\"\r\n";
        assert_eq!(
            parse_tasklist(text),
            vec![
                String::from("Ryujinx.exe"),
                String::from("yuzu-early-access.exe")
            ]
        );
    }

    #[test]
    fn a_tasklist_notice_is_not_a_process() {
        let text = "INFO: No tasks are running which match the specified criteria.\r\n";
        assert!(parse_tasklist(text).is_empty());
    }
}
