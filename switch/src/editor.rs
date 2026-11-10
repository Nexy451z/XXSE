//! Switch front-end: SD card I/O + button input on top of the shared editor
//! state machine in mhnx_core::hbedit.
//!
//! Everything that decides *what* happens lives in hbedit, which is tested on
//! the host. This file only:
//!
//!   * scans the SD card
//!   * turns libnx button presses into hbedit::Key values
//!   * answers read/write/text requests
//!   * draws the lines hbedit returns

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void, CStr};

use mhnx_core::hbedit::{Editor, Key, Request, Write};

use crate::sys::{
    self, FsDir, FsDirectoryEntry, FsFileSystem, FS_DIR_OPEN_MODE_READ_DIRS,
    FS_DIR_OPEN_MODE_READ_FILES, HID_NPAD_STYLE_SET_NPAD_STANDARD, PAD_ANY_ID_MASK,
};

extern "C" {
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    fn fclose(stream: *mut c_void) -> c_int;
    fn remove(path: *const c_char) -> c_int;
}

// HidNpadButton bits (services/hid.h:256-296).
const BTN_A: u64 = 1 << 0;
const BTN_B: u64 = 1 << 1;
const BTN_X: u64 = 1 << 2;
const BTN_Y: u64 = 1 << 3;
const BTN_L: u64 = 1 << 6;
const BTN_R: u64 = 1 << 7;
const BTN_ZL: u64 = 1 << 8;
const BTN_ZR: u64 = 1 << 9;
const BTN_PLUS: u64 = 1 << 10;
const BTN_MINUS: u64 = 1 << 11;
// D-Pad + Left Stick pseudo-buttons (HidNpadButton_AnyLeft/Up/Right/Down)
const BTN_LEFT: u64 = (1 << 12) | (1 << 16);
const BTN_UP: u64 = (1 << 13) | (1 << 17);
const BTN_RIGHT: u64 = (1 << 14) | (1 << 18);
const BTN_DOWN: u64 = (1 << 15) | (1 << 19);
// Right stick: the stick's two vertical pseudo-buttons turn pages, and its two
// horizontal ones jump to the ends of the list (services/hid.h, StickR*).
const BTN_RSTICK_LEFT: u64 = 1 << 20;
const BTN_RSTICK_UP: u64 = 1 << 21;
const BTN_RSTICK_RIGHT: u64 = 1 << 22;
const BTN_RSTICK_DOWN: u64 = 1 << 23;
// Only directional buttons auto-repeat when held so action buttons (A/B/X/Y/L/R) never misfire.
const BTN_REPEATABLE: u64 = BTN_LEFT | BTN_UP | BTN_RIGHT | BTN_DOWN;

/// Folders scanned for content.
///
/// The tool is called XXSE now, so it looks there first. The old
/// /switch/mhnxeditor paths stay in the list: files left by earlier versions
/// are still the user's saves and quests, and dropping them would look like
/// the tool had lost them.
const SAVE_DIRS: [&CStr; 5] = [
    c"/switch/XXSE/saves",
    c"/switch/XXSE",
    c"/switch/mhnxeditor/saves",
    c"/switch/mhnxeditor",
    c"/mhnxeditor/saves",
];
const QUEST_DIRS: [&CStr; 7] = [
    c"/switch/XXSE/mhxx/quest/arc", // where extract_quests writes
    c"/switch/XXSE/quest/arc",
    c"/switch/XXSE/quest",
    c"/switch/mhnxeditor/mhxx/quest/arc",
    c"/switch/mhnxeditor/quest/arc",
    c"/switch/mhnxeditor/quest",
    c"/mhnxeditor/quest",
];

/// Where quest arcs are written when extracting from a save.
///
/// The user asked for a folder beside the .nro, so the tree is
/// <sd>/switch/XXSE/mhxx/quest/arc/ - one place for everything the tool
/// produces, with room for other kinds of output later.
pub const OUT_QUEST_DIR: &str = "/switch/XXSE/mhxx/quest/arc";

// The auto-repeat timing itself lives in mhnx_core::hbedit::KeyRepeat, with
// tests: 8 frames to the first repeat (133ms), then every 4, then every 3 once
// the button has been held for a while. The old 12 / 4 cadence moved a 340 row
// list 15 rows a second and felt like it had not heard the press.

// ---------------------------------------------------------------------------
// file helpers
// ---------------------------------------------------------------------------

fn to_cstring(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let p = to_cstring(path);
    let mode = to_cstring("rb");
    let f = unsafe { fopen(p.as_ptr() as *const c_char, mode.as_ptr() as *const c_char) };
    if f.is_null() {
        return None;
    }
    let mut out: Vec<u8> = Vec::new();
    let mut buf = alloc::vec![0u8; 64 * 1024];
    loop {
        let n = unsafe { fread(buf.as_mut_ptr() as *mut c_void, 1, buf.len(), f) };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        // A cap has to exist: a stray path pointing at a video file would
        // exhaust the heap. Returning the truncated buffer silently was worse
        // than refusing, because everything downstream then failed on a
        // half-file with no clue where it came from.
        if out.len() > 12 * 1024 * 1024 {
            unsafe { fclose(f) };
            return None;
        }
    }
    unsafe { fclose(f) };
    Some(out)
}

fn write_file(path: &str, data: &[u8]) -> bool {
    let p = to_cstring(path);
    let mode = to_cstring("wb");
    let f = unsafe { fopen(p.as_ptr() as *const c_char, mode.as_ptr() as *const c_char) };
    if f.is_null() {
        return false;
    }
    let n = unsafe { fwrite(data.as_ptr() as *const c_void, 1, data.len(), f) };
    unsafe { fclose(f) };
    n == data.len()
}

extern "C" {
    fn mkdir(path: *const c_char, mode: u32) -> i32;
}

/// Create a directory, ignoring "already exists".
fn make_dir(path: &str) -> bool {
    let p = to_cstring(path);
    unsafe { mkdir(p.as_ptr() as *const c_char, 0o777) == 0 }
}

/// Create every component of a path under /switch.
///
/// libnx has no recursive mkdir, so walk the components and create each one.
/// An existing directory reports failure through mkdir, which is not an error
/// here, so the return value only says "we tried"; extract_quests decides
/// success by whether the file actually lands.
fn make_dirs(path: &str) {
    let mut cur = String::new();
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        cur.push('/');
        cur.push_str(part);
        make_dir(&cur);
    }
}

/// Write the quest arcs held in the open save out as files.
///
/// Returns how many were written. The payload is copied byte for byte: the
/// save already holds a finished arc, so nothing is re-packed.
pub fn extract_quests(save: &mhnx_core::save::Save, ids: &[i32]) -> usize {
    use mhnx_core::dlc;
    let list = dlc::list_quests(save);
    if list.is_empty() {
        return 0;
    }
    make_dirs(OUT_QUEST_DIR);
    let mut written = 0usize;
    let mut failed = 0usize;
    for q in &list {
        if !ids.is_empty() && !ids.contains(&q.quest_id) {
            continue;
        }
        if q.data.is_empty() {
            continue;
        }
        let path = format!("{}/q{:07}.arc", OUT_QUEST_DIR, q.quest_id);
        if write_file(&path, &q.data) {
            written += 1;
        } else {
            failed += 1;
        }
    }
    // A failed write is not "no matching quest": say which it was.
    LAST_EXTRACT_FAILED.store(failed, core::sync::atomic::Ordering::Relaxed);
    written
}

/// Writes that failed during the last extract, for the status line.
pub static LAST_EXTRACT_FAILED: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

struct Entry {
    name: String,
    size: i64,
    is_file: bool,
}

fn list_dir(path: &CStr) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut fs = FsFileSystem::ZERO;
    let rc = unsafe { sys::fsOpenSdCardFileSystem(&mut fs) };
    if !sys::r_succeeded(rc) {
        return out;
    }
    let mut dir = FsDir::ZERO;
    let rc = unsafe {
        sys::fsFsOpenDirectory(
            &mut fs,
            path.as_ptr(),
            FS_DIR_OPEN_MODE_READ_DIRS | FS_DIR_OPEN_MODE_READ_FILES,
            &mut dir,
        )
    };
    if sys::r_succeeded(rc) {
        loop {
            let mut entries = [FsDirectoryEntry::ZERO; 32];
            let mut total = 0i64;
            let rc =
                unsafe { sys::fsDirRead(&mut dir, &mut total, entries.len(), entries.as_mut_ptr()) };
            if !sys::r_succeeded(rc) {
                break;
            }
            let mut any = false;
            for e in entries.iter() {
                if e.is_empty() {
                    continue;
                }
                any = true;
                out.push(Entry {
                    name: String::from(e.name_str()),
                    size: e.file_size,
                    is_file: e.is_file(),
                });
            }
            if !any || (total as usize) <= out.len() {
                break;
            }
        }
        unsafe { sys::fsDirClose(&mut dir) };
    }
    unsafe { sys::fsFsClose(&mut fs) };
    out
}

/// The same scan, for a path the core names as a plain string.
///
/// list_dir takes a C string because libnx does; every folder this file knew
/// before was a constant here, but the CSV folder is named by the core so that
/// both builds agree on it and it is not written out twice.
fn list_dir_at(path: &str) -> Vec<Entry> {
    let p = to_cstring(path);
    match CStr::from_bytes_with_nul(&p) {
        Ok(c) => list_dir(c),
        Err(_) => Vec::new(),
    }
}

/// The CSV files on the SD card, in name order.
///
/// The extension is the whole contract of the folder, and the names carry a
/// timestamp, so a plain sort is chronological.
fn scan_csv() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in list_dir_at(mhnx_core::hbedit::CSV_DIR) {
        if !e.is_file || !e.name.to_ascii_lowercase().ends_with(".csv") {
            continue;
        }
        out.push(format!("{}/{}", mhnx_core::hbedit::CSV_DIR, e.name));
    }
    out.sort();
    out
}

/// Write the two CSV files the core built for the open save.
///
/// The core names them (items / equipment plus the timestamp) and hands back
/// the text; the card is the only thing this layer knows about. The folder is
/// created first: fopen on a missing directory fails, and that failure would
/// otherwise look like a full card.
fn export_csv_to_sd(ed: &mut Editor) {
    let files = ed.export_csv(crate::savedata::now_unix());
    if files.is_empty() {
        // The core already said why (no clock, or no save open).
        return;
    }
    make_dirs(mhnx_core::hbedit::CSV_DIR);
    let total = files.len();
    let mut written = 0usize;
    for (path, bytes) in files.iter() {
        if write_file(path, bytes) {
            written += 1;
        }
    }
    ed.csv_exported(written, total);
}

/// Read the CSV the editor asked for and hand the text back.
fn read_csv_into(ed: &mut Editor, path: &str) {
    let Some(bytes) = read_file(path) else {
        ed.message = format!("CSV を読み込めません: {}", path);
        return;
    };
    match String::from_utf8(bytes) {
        Ok(text) => {
            ed.apply_csv_text(path, &text);
        }
        Err(_) => {
            ed.message = format!("CSV が UTF-8 ではありません: {}", path);
        }
    }
}

/// Collect save candidates from every candidate folder.
fn scan_saves() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // The console's own save comes first: editing it in place is the point of
    // the tool, and the SD copies are for backups.
    out.push(String::from(mhnx_core::hbedit::CONSOLE_SAVE));
    for d in SAVE_DIRS.iter() {
        let dir = d.to_str().unwrap_or("/");
        for e in list_dir(d) {
            if !e.is_file {
                continue;
            }
            let known_size = matches!(e.size, 4_726_152 | 4_726_188 | 5_159_100);
            if !known_size && e.name != "system" && e.name != "system_backup" {
                continue;
            }
            // Never offer the backup copy: writing it would break the pair rule.
            if e.name.ends_with("_backup") {
                continue;
            }
            let full = format!("{}/{}", dir, e.name);
            if !out.contains(&full) {
                out.push(full);
            }
        }
    }
    out
}

/// Mount the console save and read it, with a Japanese message on failure.
fn read_console_save() -> Result<Vec<u8>, String> {
    let mount = crate::savedata::Mount::open().map_err(|e| e.describe())?;
    let bytes = mount.read().map_err(|e| e.describe())?;
    if !crate::savedata::looks_like_save(&bytes) {
        return Err(String::from("セーブのサイズが想定と違います"));
    }
    Ok(bytes)
}

/// Write all 175 bundled official quest `.arc` files out to `OUT_QUEST_DIR` on the SD card.
pub fn extract_official_quests_to_sd() -> usize {
    make_dirs(OUT_QUEST_DIR);
    let mut written = 0usize;
    for q in mhnx_core::dlc_bundle::OFFICIAL_QUESTS.iter() {
        let data = mhnx_core::dlc_bundle::arc_bytes(q);
        let path = format!("{}/q{:07}.arc", OUT_QUEST_DIR, q.quest_id);
        if write_file(&path, data) {
            written += 1;
        }
    }
    written
}

/// Read an `.arc` file from `ed.arc_files[idx]` and install/overwrite it directly into `ed.save`.
pub fn install_arc_file_into_save(ed: &mut Editor, idx: usize) {
    if ed.save.is_none() {
        ed.message = String::from("先にセーブデータを開いてください");
        return;
    }
    let Some(path) = ed.arc_files.get(idx).cloned() else {
        ed.message = String::from("クエストファイルが選択されていません");
        return;
    };
    let bytes = match (&ed.arc, ed.arc_path.as_deref()) {
        (Some(arc), Some(open)) if open == path => match arc.to_bytes() {
            Ok(b) => b,
            Err(_) => {
                ed.message = String::from("アーカイブの再パックに失敗しました");
                return;
            }
        },
        _ => match read_file(&path) {
            Some(b) => b,
            None => {
                ed.message = format!("ファイルを読み込めません: {}", path);
                return;
            }
        },
    };
    let Some(id) = mhnx_core::hbedit::quest_id_of_arc(&bytes) else {
        ed.message = String::from("クエストIDが読めません（.mib が含まれていません）");
        return;
    };
    ed.push_undo("クエストの追加");
    let Some(save) = &mut ed.save else {
        return;
    };
    let already = mhnx_core::dlc::list_quests(save)
        .iter()
        .any(|q| q.quest_id == id);
    let res = if already {
        mhnx_core::dlc::install_quest(save, id, &bytes).map(|slot| (slot, false))
    } else {
        mhnx_core::dlc::add_quest(save, id, &bytes).map(|slot| (slot, true))
    };
    match res {
        Ok((slot, added)) => {
            ed.dirty = true;
            ed.refresh_quest_titles();
            ed.message = if added {
                format!("クエスト {} をセーブの枠 {} に新規追加しました", id, slot)
            } else {
                format!("クエスト {} をセーブの枠 {} に上書きしました", id, slot)
            };
        }
        Err(e) => {
            ed.message = format!("クエストを追加できません: {}", e);
        }
    }
}

/// What a write to the console save actually did.
///
/// Built by `write_console_save` so the caller can say all of it in one
/// message: the write, the check afterwards, and where the way back is.
struct WriteReport {
    /// The .bak on the SD card holding the save as it was before this write.
    backup: String,
    /// The snapshot file name, or why there is none.
    snapshot: Result<String, String>,
    /// `Some(detail)` when the console did not give back the bytes written.
    /// The pre-write save has already been written back when this is set.
    mismatch: Option<String>,
}

/// Write the save back to the console, keeping a copy on the SD card first,
/// then read the same area back and compare it with what was written.
fn write_console_save(bytes: &[u8], pre_read: Option<&[u8]>) -> Result<WriteReport, String> {
    // The console save is a header plus a 4,726,152 byte body. A 3DS save is
    // the bare body, and writing one through the in place path would leave the
    // file at its old length with the tail of the old save still on the end.
    if bytes.len() != mhnx_core::save::SWITCH_SIZE {
        return Err(alloc::format!(
            "本体セーブは {} バイトの Switch 形式だけです（受け取ったのは {} バイト）",
            mhnx_core::save::SWITCH_SIZE,
            bytes.len()
        ));
    }
    let mount = crate::savedata::Mount::open().map_err(|e| e.describe())?;
    // Keep what the console holds now, so a bad edit can be undone without
    // having thought about it beforehand.
    make_dirs(SAFETY_DIR);
    // The review already read the file it compared against, so the common path
    // does not pay for a second 4.5MB read of the console save.
    let old = match pre_read {
        Some(b) => b.to_vec(),
        None => match mount.read() {
            Ok(b) => b,
            Err(_) => {
                return Err(String::from(
                    "いまのセーブを読み直せないため控えを作れません。セーブは変更していません",
                ))
            }
        },
    };
    let stamp = crate::savedata::stamp();
    let backup_path = alloc::format!("{}/system_{}.bak", SAFETY_DIR, stamp);
    // A copy that silently failed is worse than no copy, because the message
    // afterwards claims one exists. Refuse to write the save unless the copy
    // really is on the card and reads back the same length.
    if !write_file(&backup_path, &old) {
        return Err(alloc::format!(
            "控えを書けませんでした（{}）。セーブは変更していません",
            backup_path
        ));
    }
    prune_old_backups(10);
    // A second, rotating copy. The .bak above only covers the newest write,
    // and a mistake noticed two saves later needs one of these.
    let snapshot = save_snapshot(&old);
    // The write goes out in blocks and cannot be rolled back. It is refused
    // unless the copy above is on the card, so a failure half way through is
    // recoverable: say where from, in the message the user actually sees.
    let trip = match mount.write_diff(bytes, Some(&old)) {
        Ok(trip) => trip,
        Err(err) => {
            return Err(alloc::format!(
                "本体セーブへの書き込み、または書き込みの確認に失敗しました（{}）。内容が中途半端な可能性があるので、控え {} を SD カードから戻してください",
                err.describe(),
                backup_path
            ))
        }
    };
    if trip.is_identical() {
        return Ok(WriteReport {
            backup: backup_path,
            snapshot,
            mismatch: None,
        });
    }
    // The console returned something other than the bytes that were written.
    // Put the pre-write save back before saying anything else, and only then
    // report: an edit that did not land must never look like one that did.
    let detail = round_trip_detail(&trip);
    match restore_save(&mount, &backup_path, &old) {
        Ok(()) => Ok(WriteReport {
            backup: backup_path,
            snapshot,
            mismatch: Some(detail),
        }),
        Err(why) => Err(alloc::format!(
            "書き込み後の照合に失敗し、戻しにも失敗しました（{} / {}）。控え {} を SD カードから手動で戻してください",
            detail, why, backup_path
        )),
    }
}

/// Write the console save and put the result, the check and the way back into
/// one message.
///
/// Split out of the service loop because the save review writes on its own key
/// press now, and a write that landed must read the same whether it came from
/// the first press (nothing changed) or the second (a reviewed change).
fn finish_console_write(ed: &mut Editor, bytes: &[u8], pre_read: Option<&[u8]>) {
    let n = bytes.len();
    match write_console_save(bytes, pre_read) {
        Ok(report) => {
            ed.message = match report.mismatch {
                None => {
                    ed.dirty = false;
                    // The card now holds exactly these bytes, so 変更一覧 must
                    // compare against them and not against the file before the
                    // save: otherwise every value the write put out would still
                    // be listed as changed.
                    ed.note_saved(bytes);
                    alloc::format!(
                        "本体のセーブに {} バイト書き込みました（控え: {}）。書き込みと照合が完了しました。{}",
                        n,
                        report.backup,
                        snapshot_note(&report.snapshot)
                    )
                }
                // The edit is not on the console: leave the save marked dirty so
                // the user knows the change still only exists in memory.
                Some(detail) => alloc::format!(
                    "書き込み後の照合に失敗したので戻しました（{} / 控え: {}）。{}",
                    detail,
                    report.backup,
                    snapshot_note(&report.snapshot)
                ),
            };
        }
        Err(msg) => ed.message = msg,
    }
}

/// The A press of the review dialog: do what the dialog just showed.
///
/// The UI layer calls this from a tap on the dialog's button, so the key path
/// and the touch path run the same code. A snapshot restore is a write from a
/// different source, so it waits on the same press and is spent here.
pub(crate) fn finish_reviewed_write(ed: &mut Editor, gui: &mut crate::ui::GuiState) {
    if let Some(plan) = gui.pending_snapshot.take() {
        // The dialog has been answered, so the review it was built from is
        // done; the write below is the snapshot's, not the open save's, which
        // is why confirm_save_review must not be the one that runs it.
        ed.review = None;
        commit_snapshot_restore(ed, &plan);
        return;
    }
    if let Some(Write::File { bytes, .. }) = ed.confirm_save_review() {
        // No pre-read: the console may have changed between the review and this
        // press, and the copy write_console_save keeps has to be what is there.
        finish_console_write(ed, &bytes, None);
    }
}

/// A one line description of a mismatch, with a few offsets when there are any.
fn round_trip_detail(trip: &mhnx_core::verify::RoundTrip) -> String {
    let examples = trip.examples(3);
    if examples.is_empty() {
        trip.summary()
    } else {
        alloc::format!("{} / {}", trip.summary(), examples)
    }
}

/// Write `old` back over a save whose write did not verify.
///
/// The copy on the SD card is preferred over the in-memory one because that is
/// the file the failure message tells the user to fall back on, so restoring
/// from it also proves the card copy is good. The restored bytes are read back
/// and compared like any other write.
fn restore_save(
    mount: &crate::savedata::Mount,
    backup_path: &str,
    old: &[u8],
) -> Result<(), String> {
    let from_card = read_file(backup_path);
    let restore = from_card.as_deref().unwrap_or(old);
    // No diff: the point of a rollback is that every byte is written again.
    // Trusting the console's contents to match what we think is there is the
    // mistake that produced the mismatch in the first place.
    match mount.write_diff(restore, None) {
        Err(e) => Err(alloc::format!(
            "戻しの書き込みに失敗しました（{}）",
            e.describe()
        )),
        Ok(trip) if trip.is_identical() => Ok(()),
        Ok(trip) => Err(alloc::format!(
            "戻した内容の照合も失敗しました（{}）",
            round_trip_detail(&trip)
        )),
    }
}

/// Keep at most `max_keep` backup files in `SAFETY_DIR` so the SD card never fills up.
fn prune_old_backups(max_keep: usize) {
    let entries = list_dir(c"/switch/XXSE/backups");
    let mut baks: Vec<String> = entries
        .into_iter()
        .filter(|e| e.is_file && e.name.starts_with("system_") && e.name.ends_with(".bak"))
        .map(|e| e.name)
        .collect();
    if baks.len() <= max_keep {
        return;
    }
    baks.sort();
    let remove_count = baks.len() - max_keep;
    for name in baks.into_iter().take(remove_count) {
        let full = format!("{}/{}", SAFETY_DIR, name);
        let cpath = to_cstring(&full);
        unsafe {
            let _ = remove(cpath.as_ptr() as *const c_char);
        }
    }
}

/// Where the pre-write copies of the console save are kept.
/// Where a copy of the console save is kept before it is written.
///
/// This moved with the rename. Copies made by earlier versions stay in
/// /switch/mhnxeditor/backups and are untouched; nothing reads them
/// automatically, so if one is needed it has to be restored by hand.
const SAFETY_DIR: &str = "/switch/XXSE/backups";

/// Where the rotating snapshots of the console save are kept.
///
/// Separate from `SAFETY_DIR` on purpose: the .bak there is the one copy a
/// rollback of the *last* write needs, while a snapshot is what makes a write
/// from a day ago reachable again. Both are kept.
const SNAPSHOT_DIR: &str = "/switch/XXSE/snapshots";
/// The same folder as a C string, for `list_dir`.
const SNAPSHOT_DIR_C: &CStr = c"/switch/XXSE/snapshots";
/// Every snapshot of the console save is named from this stem.
const SNAPSHOT_STEM: &str = "system";
/// How many snapshots of the console save stay on the SD card.
const SNAPSHOT_KEEP: usize = 10;

/// Take one snapshot of the save as it is about to be replaced.
///
/// Returns the file name that now holds `bytes`, or a Japanese reason why
/// there is none. A snapshot that failed quietly would leave the user
/// believing they can undo a write that they cannot, so the caller is expected
/// to pass the reason on to the message the user sees.
fn save_snapshot(bytes: &[u8]) -> Result<String, String> {
    let unix = match crate::savedata::now_unix() {
        Some(t) => t,
        None => {
            return Err(String::from(
                "本体の時計から時刻を取得できないためスナップショットを作成できません",
            ))
        }
    };
    make_dirs(SNAPSHOT_DIR);
    let name = mhnx_core::snapshot::name(SNAPSHOT_STEM, unix);
    let path = format!("{}/{}", SNAPSHOT_DIR, name);
    if !write_file(&path, bytes) {
        return Err(alloc::format!("スナップショットを書き込めません: {}", path));
    }
    prune_snapshots();
    Ok(name)
}

/// Snapshot file names in `SNAPSHOT_DIR`, in no particular order.
fn snapshot_names() -> Vec<String> {
    list_dir(SNAPSHOT_DIR_C)
        .into_iter()
        .filter(|e| e.is_file && e.name.ends_with(".sav"))
        .map(|e| e.name)
        .collect()
}

/// Delete the oldest snapshots so that `SNAPSHOT_KEEP` stay.
fn prune_snapshots() {
    let names = snapshot_names();
    for idx in mhnx_core::snapshot::older_than(&names, SNAPSHOT_STEM, SNAPSHOT_KEEP) {
        let Some(name) = names.get(idx) else { continue };
        let full = format!("{}/{}", SNAPSHOT_DIR, name);
        let cpath = to_cstring(&full);
        unsafe {
            let _ = remove(cpath.as_ptr() as *const c_char);
        }
    }
}

/// A short note about the snapshot for the message after a write.
fn snapshot_note(snapshot: &Result<String, String>) -> String {
    match snapshot {
        Ok(name) => alloc::format!("スナップショット: {}", name),
        Err(reason) => alloc::format!("スナップショットは作成できませんでした（{}）", reason),
    }
}

/// The snapshot a review is waiting on.
///
/// Restoring used to write the console on the press that asked for it, which
/// made it the one write in the editor that skipped the review every other
/// write goes through. It is a write like any other now, and this is what the
/// dialog holds between the first [Y] and the second [A].
pub struct SnapshotRestore {
    /// The file in `SNAPSHOT_DIR` that goes back.
    pub name: String,
    /// The copy of the save it replaces, made before the dialog opened.
    pub backup: String,
}

/// The newest snapshot, read and checked, ready to be offered.
///
/// Split out of the restore so the review can be built from the same bytes the
/// write will use, and so every reason a snapshot cannot be used is one message
/// instead of four returns.
fn newest_snapshot() -> Result<(String, Vec<u8>), String> {
    let mut names = snapshot_names();
    if names.is_empty() {
        return Err(alloc::format!(
            "スナップショットがありません（本体セーブを保存すると {} に作られます）",
            SNAPSHOT_DIR
        ));
    }
    names.sort();
    let Some(name) = names.pop() else {
        return Err(String::from("スナップショットがありません"));
    };
    let path = format!("{}/{}", SNAPSHOT_DIR, name);
    let Some(bytes) = read_file(&path) else {
        return Err(alloc::format!("スナップショットを読み込めません: {}", path));
    };
    if !crate::savedata::looks_like_save(&bytes) {
        return Err(alloc::format!(
            "スナップショットのサイズが想定と違います（{} バイト）",
            bytes.len()
        ));
    }
    if let Some(finding) = mhnx_core::verify::check_shape(&bytes).first().copied() {
        return Err(alloc::format!(
            "スナップショットの形が不正なため戻しません: {}",
            finding.text()
        ));
    }
    Ok((name, bytes))
}

/// The [Y] press: keep what is about to be replaced, then ask.
///
/// Nothing is written to the console here. The save is copied into `SAFETY_DIR`
/// first - the dialog names that copy, so the user knows where the save being
/// replaced went - and the restore is then put through the very review dialog
/// the save key uses. Only the second [A] reaches [commit_snapshot_restore].
pub(crate) fn begin_snapshot_restore(ed: &mut Editor, gui: &mut crate::ui::GuiState) {
    gui.pending_snapshot = None;
    let (name, bytes) = match newest_snapshot() {
        Ok(v) => v,
        Err(msg) => {
            ed.message = msg;
            return;
        }
    };
    let mount = match crate::savedata::Mount::open() {
        Ok(m) => m,
        Err(e) => {
            ed.message = e.describe();
            return;
        }
    };
    let current = match mount.read() {
        Ok(b) => b,
        Err(e) => {
            ed.message = alloc::format!(
                "いまのセーブを読み直せないため戻しません（{}）",
                e.describe()
            );
            return;
        }
    };
    make_dirs(SAFETY_DIR);
    let backup = format!("{}/system_{}.bak", SAFETY_DIR, crate::savedata::stamp());
    if !write_file(&backup, &current) {
        ed.message = alloc::format!("控えを書けないため戻しませんでした（{}）", backup);
        return;
    }
    prune_old_backups(10);
    // A snapshot that is byte for byte what the console already holds is not a
    // write; saying so is friendlier than a dialog with nothing in it.
    if !ed.stage_save_review(mhnx_core::hbedit::CONSOLE_SAVE, &current, &bytes) {
        ed.message = alloc::format!(
            "スナップショット {} は今のセーブと同じ内容です（書き込みません）",
            name
        );
        return;
    }
    ed.message = mhnx_core::snapshot::restore_review_message(&name, &backup);
    // The dialog's body has to say what is being put back: the one-line
    // message below it is clipped to the footer, and the diff alone would not
    // name the file or the copy the current save went to.
    let headline_line = ed.message.clone();
    if let Some(review) = ed.review.as_mut() {
        review.lines.insert(0, String::new());
        review.lines.insert(0, headline_line);
    }
    gui.pending_snapshot = Some(SnapshotRestore { name, backup });
}

/// The second [A]: put the snapshot back and prove it landed.
///
/// Read from the card again rather than hold 4.5MB across the dialog: nothing
/// prunes the snapshot folder while the dialog is up, so the name still points
/// at the same bytes, and the shape is checked a second time because a file the
/// user can reach with a card reader is not a file this code can trust twice.
pub(crate) fn commit_snapshot_restore(ed: &mut Editor, plan: &SnapshotRestore) {
    let path = format!("{}/{}", SNAPSHOT_DIR, plan.name);
    let bytes = match read_file(&path) {
        Some(b) => b,
        None => {
            ed.message = alloc::format!("スナップショットを読み込めません: {}", path);
            return;
        }
    };
    if !crate::savedata::looks_like_save(&bytes) {
        ed.message = alloc::format!(
            "スナップショットのサイズが想定と違います（{} バイト）",
            bytes.len()
        );
        return;
    }
    let mount = match crate::savedata::Mount::open() {
        Ok(m) => m,
        Err(e) => {
            ed.message = e.describe();
            return;
        }
    };
    // The copy kept before the dialog opened is what the console still holds,
    // so it is the side of the diff. It is also the file the failure message
    // tells the user to fall back on.
    let current = read_file(&plan.backup);
    match mount.write_diff(&bytes, current.as_deref()) {
        Err(e) => {
            ed.message = alloc::format!(
                "スナップショットの書き込みに失敗しました（{}）。控え {} を SD カードから戻してください",
                e.describe(),
                plan.backup
            );
        }
        Ok(trip) if trip.is_identical() => {
            // Reload from what is now on the console, but stay on the screen
            // the user pressed the button on.
            let screen = ed.screen;
            let cursor = ed.cursor;
            ed.open_save_bytes(mhnx_core::hbedit::CONSOLE_SAVE, &bytes);
            ed.screen = screen;
            ed.cursor = cursor;
            ed.message = alloc::format!(
                "スナップショット {} を本体セーブに戻しました（書き込みと照合が完了しました / 控え: {}）",
                plan.name,
                plan.backup
            );
        }
        Ok(trip) => {
            let detail = round_trip_detail(&trip);
            let Some(old) = current else {
                // Without the copy there is nothing safe to roll back to, and
                // writing the snapshot again would fail the same way.
                ed.message = alloc::format!(
                    "スナップショットの照合に失敗し、控えも読めないため戻せませんでした（{}）。控え {} を SD カードから手動で戻してください",
                    detail,
                    plan.backup
                );
                return;
            };
            match restore_save(&mount, &plan.backup, &old) {
                Ok(()) => {
                    ed.message = alloc::format!(
                        "スナップショットの照合に失敗したので、戻す前のセーブに戻しました（{}）",
                        detail
                    );
                }
                Err(why) => {
                    ed.message = alloc::format!(
                        "スナップショットの照合に失敗し、戻しにも失敗しました（{} / {}）。控え {} を SD カードから手動で戻してください",
                        detail,
                        why,
                        plan.backup
                    );
                }
            }
        }
    }
}

/// Collect .arc quest files from every candidate folder.
fn scan_quests() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in QUEST_DIRS.iter() {
        let dir = d.to_str().unwrap_or("/");
        for e in list_dir(d) {
            if !e.is_file || !e.name.ends_with(".arc") {
                continue;
            }
            let full = format!("{}/{}", dir, e.name);
            if !out.contains(&full) {
                out.push(full);
            }
        }
    }
    // Numeric order so q1010001 comes before q1010002.
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------

/// Draws the editor through whichever back end is available.
///
/// The Japanese-capable canvas is preferred; until it is available this falls
/// back to libnx' ASCII console so the editor is always usable.
struct Painter {
    canvas: Option<crate::text::Canvas>,
    gui: crate::ui::GuiState,
}

impl Painter {
    fn new() -> Self {
        Painter {
            canvas: crate::text::Canvas::new(),
            gui: crate::ui::GuiState::new(),
        }
    }

    fn draw(&mut self, ed: &Editor) {
        match &mut self.canvas {
            None => {
                let lines = ed.lines();
                unsafe { sys::consoleClear() };
                sys::log_line("XXSE - MHXX save editor (Switch)");
                sys::log_line("=====================================================");
                // The same one-line guide the canvas draws under the bar, so
                // the fallback screen tells a first-time player the same thing.
                sys::log_line(ed.screen_guide());
                for l in &lines {
                    sys::log_line(&format!("{}{}", if l.cursor { ">" } else { " " }, l.text));
                }
                sys::log_line("");
                sys::log_line(&format!(">> {}", ed.message));
                // The canvas UI draws these two as a footer pill and a dialog;
                // the text console has neither, so it prints them here.
                sys::log_line(&ed.undo_line());
                if let Some(review) = ed.review.as_ref() {
                    for line in review.lines.iter() {
                        sys::log_line(line);
                    }
                    sys::log_line("[A] この内容で保存   [B] 中止");
                }
                let mut bar = alloc::string::String::new();
                for (k, label) in actions(ed.screen) {
                    if !bar.is_empty() {
                        bar.push_str("   ");
                    }
                    bar.push_str(k);
                    bar.push(' ');
                    bar.push_str(label);
                }
                sys::log_line(&bar);
                unsafe { sys::consoleUpdate(core::ptr::null_mut()) };
            }
            Some(c) => {
                self.gui.draw(c, ed);
            }
        }
    }
}

/// Screen title shown in the header.
pub(crate) fn title(screen: mhnx_core::hbedit::Screen) -> &'static str {
    use mhnx_core::hbedit::Screen::*;
    match screen {
        SaveList => "セーブを開く",
        Convert => "3DS <-> Switch 変換",
        Slots => "キャラクター選択",
        Char => "キャラクター編集",
        Items => "アイテムボックス",
        ItemPick => "アイテムを選ぶ",
        Charms => "護石",
        CharmEdit => "護石編集",
        SkillPick => "スキルを選ぶ",
        Equip => "装備",
        EquipEdit => "装備編集",
        JewelPick => "装飾品を選ぶ",
        EquipPick => "装備を名前で選ぶ",
        QuestEquip => "クエスト装備",
        SaveQuests => "クエスト (セーブ内)",
        ArcList => "クエスト (.arc)",
        QuestEdit => "クエスト改造",
        ArcEntries => "クエストの中身",
        EntryView => "エントリ",
        Mib => "クエスト条件",
        Gmd => "クエスト本文",
        Check => "クエスト検査",
        Hunts => "狩猟記録",
        Guild => "ギルドカード",
        Progress => "進行状況",
        Roster => "オトモ",
        Sets => "マイセット",
        Palico => "配信オトモ",
        Options => "設定",
        Csv => "CSV 入出力",
        // 詳細: the bytes behind the screen 詳細 was pressed on.
        SaveMap => "生データ",
        // 変更一覧: what the open save differs from the card in, one value a
        // line, each one restorable on its own.
        Review => "変更一覧",
    }
}

/// The buttons that do something on this screen, as (key, label).
///
/// The footer draws these as chips, so a screen never needs the user to
/// remember a key that is not on screen.
pub(crate) fn actions(screen: mhnx_core::hbedit::Screen) -> &'static [(&'static str, &'static str)] {
    use mhnx_core::hbedit::Screen::*;
    match screen {
        // Every line here is what the console's pad dispatch does, not what
        // the state machine would do with the same key: L / R are the sub-tabs
        // on this build (the transform screen is a section of its own) and X is
        // the global save, so neither can be a screen-local tool.
        SaveList => &[
            ("A", "開く"),
            ("Y", "再読込"),
            ("L/R", "サブタブ"),
            ("X", "保存"),
            ("+", "終了"),
        ],
        Convert => &[("A", "変換して保存"), ("Y", "名前"), ("B", "戻る")],
        // [Y] is the bulk MAX on this build (the console answers it before the
        // state machine sees it), and it asks for a second press now. The five
        // keys the state machine gives this screen (X items, L charms, R the
        // download pallet, - the equipment box) are not what the console does
        // with them: X is the save key everywhere, L / R are the sub-tab pair,
        // and [-] is the delete key. The sections - the way to the boxes the
        // state machine's own keys would have opened - are ZL / ZR.
        Slots => &[
            ("A", "キャラ"),
            ("Y", "一括MAX(2回)"),
            ("L/R", "サブタブ"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // [Y] is the step key on this build (the console answers it before the
        // state machine, whose Y opens マイセット). [-] is the delete key and a
        // character has nothing to delete: the hunt record it used to open is
        // the オトモ・記録 section, one ZL away.
        Char => &[
            ("A", "最大(2回)/編集"),
            ("</>", "値を変更"),
            ("Y", "刻み"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // [Y] is the list's own state (所持品のみ <-> 空きも表示) and [A] opens
        // the picker; the step the arrows use is the deck's button, because
        // every key on this screen is spoken for.
        // [-] is the delete key and the item box has nothing to delete, so the
        // bulk menu took [ZR]: the review's finding was that the one screen
        // whose whole point is bulk edits could not reach them from the pad.
        // [L] is the sub-tab, not the undo - the undo is the deck's button.
        Items => &[
            ("A", "種類変更"),
            ("</>", "個数"),
            ("R", "数値入力"),
            ("Y", "空き表示"),
            ("ZR", "一括操作"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // The same three chips on every pick list: the direction keys are one
        // rule on all routes (hbedit::pick_move), so the footer names the same
        // two steps rather than a route-specific wording.
        ItemPick => &[
            ("A", "決定/追加"),
            ("◀/▶", "10件"),
            ("▲/▼", "1件"),
            ("Y", "検索"),
            ("X", "クリア"),
            ("B", "戻る"),
        ],
        // The console's pad dispatch, not the bare state machine: [A] on the
        // charm list opens the form, [Y] is the list's search (the same key as
        // the equipment box and the quest list) and [-] is the two-press
        // delete. It said [Y] 削除 while [Y] opened the form and [-] deleted:
        // the one line that named the wrong key on the wrong action.
        Charms => &[
            // The A word follows the list (hbedit::Editor::charm_a_label):
            // 編集 / 作成 / 検索解除. This table is the text build's, and the
            // console prints the state machine's word, so the two must not
            // name two different jobs for one key.
            ("A", "編集/作成/解除"),
            ("Y", "検索"),
            ("-", "削除(2回)"),
            ("+", "新規（空き枠のみ）"),
            ("X", "保存"),
            ("B", "キャラ"),
        ],
        // The form's [A] is the selected row's own action (hbedit::CharmAction),
        // so this line cannot name a single one; the drawn build prints the
        // row's own word in its footer and on its [A] button.
        CharmEdit => &[
            ("A", "行の操作"),
            ("</>", "値"),
            ("Y", "検索"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        SkillPick => &[
            ("A", "決定"),
            ("◀/▶", "10件"),
            ("▲/▼", "1件"),
            ("Y", "検索"),
            ("X", "クリア"),
            ("B", "戻る"),
        ],
        Equip => &[
            ("A", "編集"),
            ("Y", "検索"),
            ("-", "削除(2回)"),
            ("+", "新規"),
            ("X", "保存"),
            ("L/R", "サブタブ"),
            ("B", "キャラ"),
        ],
        // [A] is the row's own action (hbedit::EquipFormAction): the three
        // sockets open the decoration list, 種別 / ID the equipment list, the
        // numeric rows step, the 装備枠 row writes and the new form's レベル
        // row creates. [Y] opens the same two lists with the search box up.
        EquipEdit => &[
            ("A", "選ぶ/書き込み"),
            ("</>", "10件/値"),
            ("Y", "検索"),
            ("R", "ID入力"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        JewelPick => &[
            ("A", "決定"),
            ("◀/▶", "10件"),
            ("▲/▼", "1件"),
            ("Y", "検索"),
            ("X", "クリア"),
            ("B", "戻る"),
        ],
        // The name picker: the same four movement chips as the other pick
        // lists, plus [ZR] for the type filter and the hex note, because the id
        // this list prints is the one it searches. [-] is the delete key and a
        // picker has nothing to delete.
        EquipPick => &[
            ("A", "決定"),
            ("◀/▶", "10件"),
            ("▲/▼", "1件"),
            ("Y", "検索"),
            ("ZR", "種別"),
            ("X", "クリア"),
            ("B", "戻る"),
        ],
        QuestEquip => &[("A", "中身を開く/変更"), ("B", "戻る")],
        // [-] is the two-press delete on every quest list, [Y] the search.
        // The whole-save SD write is the deck's button, because [R] (its old
        // key) is the sub-tab on this build.
        SaveQuests => &[
            ("A", "編集/導入(解除は2回)"),
            ("-", "削除(2回)"),
            ("Y", "検索"),
            ("ZR", "一括導入"),
            ("L/R", "サブタブ"),
            ("X", "保存"),
            ("B", "SD(.arc)"),
        ],
        // [ZR] writes the bundled quests out (the state machine's Key::Z); the
        // way back is [B], and both layers answer it: the save list. It used to
        // go to セーブ内, whose own [B] comes right back here.
        ArcList => &[
            ("A", "開く"),
            ("Y", "セーブへ導入(2回)"),
            ("ZR", "SDへ展開"),
            ("X", "保存"),
            ("B", "セーブ一覧"),
            ("+", "終了"),
        ],
        // クエスト改造: one row pair moves, the other steps, [A] opens the
        // row's chooser, [ZR] picks which value steps and [Y] is the step. The
        // file-by-file screens are the [ファイル] pill and [R 長押し], because
        // [ZR] is the value switch here.
        QuestEdit => &[
            ("</>", "値"),
            ("A", "選択"),
            ("Y", "刻み"),
            ("ZR", "項目"),
            ("R", "検索"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // [Y] is the checker on this build (the console answers it before the
        // state machine, whose Y is the decoded view) and [ZR] writes this one
        // quest out. [-] is the delete key and a file list has nothing to
        // delete, so the 抽出 it used to do is [ZR] now.
        ArcEntries => &[
            ("A", "開く"),
            ("Y", "検査"),
            ("ZR", "抽出"),
            ("X", "保存"),
            ("B", "改造へ"),
        ],
        // The reward / supply row's two numbers are [ZR]'s job (the state
        // machine's Key::Z) and [Y] is the step; [A] opens the item list on
        // those rows and steps the coordinate rows. [X] is the save key, and
        // on the quest screens the save key writes the .arc
        // (hbedit::Editor::quick_save).
        EntryView => &[
            ("</>", "値"),
            ("A", "アイテム/値変更"),
            ("Y", "刻み"),
            ("ZR", "個数/確率"),
            ("R", "数値入力"),
            ("X", ".arc 保存"),
            ("B", "戻る"),
        ],
        Mib => &[
            ("</>", "増減"),
            ("A", "一覧選択"),
            ("Y", "刻み"),
            ("ZR", "項目"),
            ("X", ".arc 保存"),
            ("B", "戻る"),
        ],
        Gmd => &[("A", "文章編集"), ("X", ".arc 保存"), ("B", "戻る")],
        Check => &[("A", "修正"), ("X", ".arc 保存"), ("B", "戻る")],
        // [ZR] is the list toggle (hbedit::Editor::toggle_hunt_show_all); [-]
        // is the delete key and a hunt record has nothing to delete.
        Hunts => &[
            ("</>", "値"),
            ("Y", "列切替"),
            ("A", "刻み"),
            ("R", "数値入力"),
            ("ZR", "表示"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // カード / クエスト回数 / 武器使用数 / 闘技大会 are the card's four views
        // and [Y] walks them (on the quest view it is the 既読 bit instead), so
        // the arena log is a view, not a shoulder. [ZR] opens the progress
        // screen (hbedit::Editor::open_progress), [R] types a number on the
        // three numeric views, and [-] is the delete key - the card has nothing
        // to delete.
        Guild => &[
            ("</>", "値"),
            ("R", "数値入力"),
            ("Y", "回数/武器"),
            ("A", "行の操作"),
            ("ZR", "進行状況"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // [Y] walks the three views and [R] is the quest view's second bit
        // (既読): the console takes [R] back from the sub-tab rule on that view
        // only (hbedit::Editor::shoulder_job), so the chip names it there. [ZR]
        // is the search on every view.
        Progress => &[
            ("A", "ビット切替"),
            ("R", "既読"),
            ("Y", "タブ"),
            ("</>", "±50行"),
            ("ZR", "検索"),
            ("L/R", "タブ"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // [ZR] opens the palico equipment panel on both palico screens, and
        // inside it [-] is the two-press release (the footer there says so).
        // Plus opens the skill / support move panel on the roster alone; it is
        // not the quit key there, the slot list one B away still is.
        Roster => &[
            ("</>", "値"),
            ("Y", "項目切替"),
            ("A", "名前"),
            ("ZR", "装備"),
            ("+", "スキル/サポ"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // Two modes on one screen: the list, and the set editor that [A] opens.
        // The chips name both, because this table is per screen and Y means
        // different things in the two (tab switch on the list, warnings in the
        // form). X saves on both.
        Sets => &[
            ("A", "編集"),
            ("</>", "装備を巡回"),
            // The minus key is the 解除 *inside* the form; the list says so
            // instead of naming a key that answered nothing there.
            ("-", "解除(編集で2回)"),
            ("Y", "警告/タブ切替"),
            ("X", "保存"),
            ("B", "護石一覧"),
        ],
        // The language is the arrow pair on this build (the console answers
        // ◀ / ▶ before the state machine, whose L / R are the sub-tabs), the
        // equipment panel is [ZR], and [-] is the delete key.
        Palico => &[
            ("A", "追加/削除"),
            ("Y", "全導入(2回)"),
            ("</>", "言語"),
            ("ZR", "装備"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        Options => &[
            ("</>", "値変更"),
            ("Y", "スナップショット"),
            ("X", "保存"),
            ("B", "戻る"),
        ],
        // Twice for the apply: it replaces a whole box.
        Csv => &[
            ("A", "適用(2回)"),
            ("Y", "書き出し"),
            ("ZR", "再読込"),
            ("B", "戻る"),
        ],
        // 生データ: the list's four keys, plus [A] opening the byte editor.
        // Inside the editor the four directions belong to the byte; this table
        // is per screen and static, so the drawn footer takes the editor's own
        // words from hbedit::Editor::footer_hint (which knows the mode).
        SaveMap => &[
            ("▲/▼", "行"),
            ("◀/▶", "10行"),
            ("A", "編集"),
            ("Y", "検索"),
            ("B", "戻る"),
        ],
        // 変更一覧: the two-press 戻す and the rebuild. [X] is the save key
        // every screen shares, and [B] leaves to the screen the tab came from.
        Review => &[
            ("▲/▼", "行"),
            ("◀/▶", "10行"),
            ("A", "戻す（2回）"),
            ("Y", "作り直す"),
            ("B", "戻る"),
        ],
    }
}

// ---------------------------------------------------------------------------
// main loop
// ---------------------------------------------------------------------------

fn key_from(buttons: u64) -> Option<Key> {
    if buttons & BTN_UP != 0 {
        Some(Key::Up)
    } else if buttons & BTN_DOWN != 0 {
        Some(Key::Down)
    } else if buttons & BTN_LEFT != 0 {
        Some(Key::Left)
    } else if buttons & BTN_RIGHT != 0 {
        Some(Key::Right)
    } else if buttons & BTN_A != 0 {
        Some(Key::A)
    } else if buttons & BTN_B != 0 {
        Some(Key::B)
    } else if buttons & BTN_X != 0 {
        Some(Key::X)
    } else if buttons & BTN_Y != 0 {
        Some(Key::Y)
    } else if buttons & BTN_L != 0 {
        Some(Key::L)
    } else if buttons & BTN_R != 0 {
        Some(Key::R)
    } else if buttons & BTN_MINUS != 0 {
        Some(Key::Minus)
    } else if buttons & BTN_ZR != 0 {
        Some(Key::Z)
    } else if buttons & BTN_PLUS != 0 {
        Some(Key::Plus)
    } else {
        None
    }
}

/// Apply a completed text input (from `swkbd` or the built-in OSK) to `Editor`.
///
/// One call, because the answer belongs to the request and the request is a row
/// of the core's table (hbedit::TextSentinel): the id says which writer runs,
/// and a sentinel added there cannot arrive here and be answered by nothing -
/// which is what happened to the 報酬 / 支給品 search, whose id was in the queue
/// and in neither of this function's two chains.
fn apply_text_result(ed: &mut Editor, idx: usize, s: String) {
    ed.apply_text_request(idx, &s);
}

/// Answer everything the editor asked for.
///
/// Returns true when the screen must be fully redrawn even if nothing else
/// changed - which is the case after the software keyboard applet has been
/// shown, because it takes over the display.
fn service(ed: &mut Editor, gui: &mut crate::ui::GuiState) -> bool {
    let mut force_redraw = false;

    if let Some((idx, s)) = gui.osk_result.take() {
        apply_text_result(ed, idx, s);
        force_redraw = true;
    }
    if gui.osk_cancelled {
        gui.osk_cancelled = false;
        ed.message = String::from("文字入力をキャンセルしました");
        force_redraw = true;
    }
    if gui.wants_extract_official_arcs {
        gui.wants_extract_official_arcs = false;
        let written = extract_official_quests_to_sd();
        ed.set_arc_files(scan_quests());
        ed.message = format!(
            "公式イベントクエスト {} 件を {} に展開しました",
            written, OUT_QUEST_DIR
        );
        force_redraw = true;
    }
    if let Some(arc_idx) = gui.wants_install_arc_idx.take() {
        install_arc_file_into_save(ed, arc_idx);
        force_redraw = true;
    }
    // The CSV folder: listing it, writing the two exports, and reading the one
    // file an apply asked for. The core names every path; this layer is the
    // only one that touches the card.
    if ed.take_csv_rescan() {
        ed.set_csv_files(scan_csv());
        force_redraw = true;
    }
    if ed.take_csv_export() {
        export_csv_to_sd(ed);
        force_redraw = true;
    }
    if let Some(path) = ed.take_csv_read() {
        read_csv_into(ed, &path);
        force_redraw = true;
    }
    if gui.wants_restore_snapshot {
        gui.wants_restore_snapshot = false;
        // Only the first half: the review dialog decides whether the write
        // happens, the same way the save key's write does.
        begin_snapshot_restore(ed, gui);
        force_redraw = true;
    }

    if let Some(req) = ed.take_request() {
        match req {
            Request::ReadSave(i) => {
                let path = ed.save_files.get(i).cloned();
                if let Some(p) = path {
                    if p == mhnx_core::hbedit::CONSOLE_SAVE {
                        match read_console_save() {
                            Ok(b) => ed.open_save_bytes(&p, &b),
                            Err(msg) => ed.message = msg,
                        }
                    } else {
                        match read_file(&p) {
                            Some(b) => ed.open_save_bytes(&p, &b),
                            None => ed.message = format!("{} を読めません", p),
                        }
                    }
                }
            }
            Request::ReadArc(i) => {
                let path = ed.arc_files.get(i).cloned();
                if let Some(p) = path {
                    match read_file(&p) {
                        Some(b) => {
                            ed.open_arc_bytes(&p, &b);
                            // A quest that was picked is a quest to edit, not
                            // fifteen files to choose from: the console lands
                            // on クエスト改造, and the [ファイル] pill goes back
                            // to the file list this used to open directly.
                            //
                            // The return target travels with the open: a quest
                            // picked from the SD (.arc) list comes back there on
                            // [B]. This used to call the bare enter_quest_edit,
                            // which keeps whatever the *last* quest set - so a
                            // quest opened from the card came back to セーブ内,
                            // a list that does not hold it.
                            if ed.arc.is_some() {
                                ed.enter_quest_edit_from(mhnx_core::hbedit::Screen::ArcList);
                            }
                        }
                        None => ed.message = format!("{} を読めません", p),
                    }
                }
            }
            Request::ExtractQuests { ids } => {
                let written = match &ed.save {
                    Some(s) => extract_quests(s, &ids),
                    None => {
                        ed.message = String::from("先にセーブデータを開いてください");
                        0
                    }
                };
                let failed = LAST_EXTRACT_FAILED.load(core::sync::atomic::Ordering::Relaxed);
                ed.apply_extracted(written, OUT_QUEST_DIR);
                if failed > 0 {
                    ed.message = format!("{} - {} 件を書き出せませんでした（フォルダがありません）", ed.message, failed);
                }
                // The folder is new, so refresh the quest list for the next scan.
                ed.set_arc_files(scan_quests());
                force_redraw = true;
            }
        }
    }

    if let Some(w) = ed.take_write() {
        match w {
            Write::File { path, bytes } => {
                let n = bytes.len();
                // Only the two files the mount actually covers may take this
                // path. Matching every "nand:" prefix meant the convert screen,
                // which writes "nand:/system.3ds", went through here too and
                // overwrote the console save with a 3DS body.
                if path == mhnx_core::hbedit::CONSOLE_SAVE
                    || path == "nand:/system_backup"
                {
                    // The console save is reviewed before it is written. The
                    // first press reads what the console holds now and shows
                    // what would change; the second press writes. A write that
                    // changes nothing goes out at once, the way it always did.
                    match read_console_save() {
                        Ok(old) => {
                            if ed.stage_save_review(&path, &old, &bytes) {
                                ed.take_request();
                                return true;
                            }
                            // Nothing differs: write it right away, and reuse
                            // the bytes just read as the copy to keep.
                            finish_console_write(ed, &bytes, Some(&old));
                        }
                        Err(msg) => {
                            // Without the current bytes there is no copy and no
                            // review, and write_console_save would refuse for
                            // the same reason. Say it once, write nothing.
                            ed.message = msg;
                        }
                    }
                    ed.take_request();
                    return force_redraw;
                }
                // Keep the file we are about to replace. Nothing used to hold
                // the previous contents, so one bad write lost the save.
                let mut kept = false;
                if let Some(old) = read_file(&path) {
                    let bak = format!("{}.bak", path);
                    kept = write_file(&bak, &old);
                }
                if write_file(&path, &bytes) {
                    let is_arc = path.ends_with(".arc");
                    if !is_arc || ed.save.is_none() {
                        ed.dirty = false;
                    }
                    // A save written to the SD card is the reference 変更一覧
                    // compares against, exactly like the console save above.
                    if !is_arc {
                        ed.note_saved(&bytes);
                    }
                    ed.message = if is_arc {
                        format!("クエストファイルを保存しました: {} ({} B)", path, n)
                    } else if kept {
                        format!("セーブを保存しました: {} ({} B / .bak作成済)", path, n)
                    } else {
                        format!("セーブを保存しました: {} ({} B)", path, n)
                    };
                    // Keep the Switch save pair consistent.
                    if path.ends_with("/system") {
                        let backup = format!("{}_backup", path);
                        if let Some(body) = save_body_of(ed) {
                            let mut hdr =
                                mhnx_core::save::switch_header("system_backup", body.len()).to_vec();
                            hdr.extend_from_slice(&body);
                            let _ = write_file(&backup, &hdr);
                        }
                    }
                } else {
                    ed.message = String::from("書き込みに失敗しました（SDカードの空き容量・保護を確認してください）");
                }
            }
        }
    }

    if let Some((idx, initial)) = ed.take_text_request() {
        force_redraw = true;
        // The title and the field come from the request itself
        // (hbedit::Editor::text_request_title / text_request_spec), which reads
        // the core's sentinel table: this layer no longer keeps a second copy of
        // the ids, the widths or the words, and a request added there arrives
        // here titled and sized. It used to keep that copy, and the 報酬 /
        // 支給品 search - added to the queue and to neither chain - opened a
        // keyboard titled 「クエスト文章」.
        let header = ed.text_request_title(idx);
        let spec = ed.text_request_spec(idx);
        // A name always opens the built-in keyboard, even when the applet is
        // available: only the built-in one has a key for the line break the
        // field holds, and the applet is single line. What came back from an
        // old keyboard is filtered first, so a stray tab or NUL does not even
        // reach the input box.
        let initial = if ed.text_request_name(idx).is_some() {
            let (filtered, _) = spec.filter(&initial);
            filtered
        } else {
            initial
        };
        // Which keyboard to use, decided by one function so the screen, this
        // dispatch and the host tests cannot each have their own answer
        // (hbedit::keyboard_route):
        //
        //   * a name holds U+000A, which the system keyboard cannot type, so a
        //     name always uses the built-in one - the one real reason the
        //     built-in keyboard exists;
        //   * a .gmd line is a paragraph for the same reason;
        //   * every other request is one line, so the system keyboard is
        //     preferred: it has the console's IME, flick input and the layout
        //     the player already knows;
        //   * where the applet cannot be launched at all (the process is a
        //     library applet, i.e. started from the album), the built-in
        //     keyboard is used and the modal says why.
        match mhnx_core::hbedit::keyboard_route(spec, crate::swkbd::is_available()) {
            mhnx_core::hbedit::KeyboardRoute::System => {
                // Three answers, three sentences. Cancel and "the keyboard
                // never opened" used to be one `None` and both printed
                // キャンセルしました, so a failed launch read as a user who
                // changed their mind - and the retry it invites fails the same
                // way.
                match crate::swkbd::input_text(&header, &initial, spec.capacity) {
                    crate::swkbd::SwkbdAnswer::Text(s) => apply_text_result(ed, idx, s),
                    crate::swkbd::SwkbdAnswer::Cancelled => {
                        ed.message = String::from(
                            "文字入力をキャンセルしました（もう一度押すとやり直せます）",
                        )
                    }
                    crate::swkbd::SwkbdAnswer::Failed => {
                        // The sentence used to send the user to the built-in
                        // keyboard, which nothing on this screen can open: a
                        // one-line field only reaches it through
                        // keyboard_route, and that answer was "system". The
                        // press falls back the way the library-applet route
                        // does - the keyboard opens, and both the modal and
                        // the status row say why it is the built-in one.
                        gui.open_osk_with_note(
                            idx,
                            &header,
                            &initial,
                            spec,
                            "システムキーボードを開けなかったため、内蔵キーボードで入力します",
                        );
                        ed.message = String::from(
                            "システムキーボードを開けなかったため内蔵キーボードで入力します（[+] で確定）",
                        );
                    }
                }
            }
            mhnx_core::hbedit::KeyboardRoute::BuiltinFallback => {
                // The system keyboard cannot be launched from this process, so
                // the built-in one opens for a one-line field too - and says so
                // on the modal, in the status row and nowhere silently.
                gui.open_osk_with_note(
                    idx,
                    &header,
                    &initial,
                    spec,
                    "システムキーボードが使えないため、内蔵キーボードで入力します",
                );
                ed.message = String::from(
                    "システムキーボードが使えないため内蔵キーボードで入力します（[+] で確定）",
                );
            }
            mhnx_core::hbedit::KeyboardRoute::Builtin => {
                gui.open_osk(idx, &header, &initial, spec);
            }
        }
    }

    force_redraw
}

/// The body of the currently open save, used to refresh the backup copy.
fn save_body_of(ed: &Editor) -> Option<Vec<u8>> {
    ed.save.as_ref().map(|s| s.body().to_vec())
}

/// Program entry point for the editor.
pub fn run() -> i32 {
    unsafe {
        sys::fsInitialize();
    }

    // Order matters: libnx' consoleInit() also grabs the default NWindow and
    // creates its own framebuffer, so the Canvas has to be created first.
    // Creating the Canvas afterwards would fail and silently disable Japanese
    // text. Only when the Canvas is unavailable do we fall back to the ASCII
    // console.
    let mut painter = Painter::new();
    let using_canvas = painter.canvas.is_some();
    if !using_canvas {
        unsafe { sys::consoleInit(core::ptr::null_mut()) };
    }

    let mut ed = Editor::new();
    ed.set_save_files(scan_saves());
    ed.set_arc_files(scan_quests());
    ed.message = format!(
        "セーブ {} 件 / クエスト {} 件を検出しました（タッチ・スティック・ボタン対応）",
        ed.save_files.len(),
        ed.arc_files.len()
    );

    // The direction held down and the repeat timer that goes with it. The timer
    // is the core's, so the frames it fires on are pinned by a host test.
    let mut held: u64 = 0;
    let mut key_repeat = mhnx_core::hbedit::KeyRepeat::new();
    // The shoulders: both taps are answered on the release.
    //
    // They were answered on the press for a while, because a tap that waits for
    // the release felt one press behind (the review called it もたつく) - but
    // the two keys of one band then answered on two different frames: [L] moved
    // the sub-tab the moment it went down and [R] moved it when the finger came
    // up, so the same gesture read as two different gestures depending on which
    // shoulder was used. Both wait for the release now, and both holds keep
    // their second job (L offers the whole-save undo, R opens 詳細) without ever
    // also moving the tab: a hold that also taps is worse than a tap that is one
    // frame late. hbedit::ShoulderTap owns that rule and its host test pins it;
    // the undo still costs two holds, so a press that lingers cannot throw a
    // save away (confirm::HoldConfirm).
    let mut l_tap = mhnx_core::hbedit::ShoulderTap::new();
    // [R]'s hold opens 詳細, so the target is the screen state at press time:
    // the detail view opens on the screen the key went down on
    // (hbedit::DetailTarget).
    let mut r_tap = mhnx_core::hbedit::ShoulderTap::new();
    let mut r_target: Option<mhnx_core::hbedit::DetailTarget> = None;
    // A held shoulder repeats on the four pick lists, where L / R are the ten
    // row step rather than the sub-tab: the same 8 / 4 / 3 cadence the
    // direction keys use, so holding L crosses a 6,435 row list instead of
    // moving ten rows once.
    let mut l_repeat = mhnx_core::hbedit::KeyRepeat::new();
    let mut r_repeat = mhnx_core::hbedit::KeyRepeat::new();

    unsafe {
        sys::padConfigureInput(1, HID_NPAD_STYLE_SET_NPAD_STANDARD);
        let mut pad = sys::PadState::ZERO;
        sys::padInitializeWithMask(&mut pad, PAD_ANY_ID_MASK);
        sys::hidInitializeTouchScreen();

        let mut need_draw = true;
        while sys::appletMainLoop() {
            // The optional update check, collected once per frame. The check
            // itself runs on its own thread (crate::update), so this call only
            // takes an answer that is already there: the frame never waits for
            // a socket. The first call also starts it, once, when the setting
            // allows it - the frame it starts on has already been drawn.
            painter.gui.update.poll();
            painter.gui.update.start_once();

            sys::padUpdate(&mut pad);
            let down = pad.buttons_down();
            let cur = pad.buttons_cur;

            // Action buttons are edge-triggered; only directional movement repeats when held.
            let mut key: Option<Key> = key_from(down);
            let cur_dir = cur & BTN_REPEATABLE;
            let mut repeated = false;
            if cur_dir != held {
                // A new direction is answered by its own edge press, and the
                // repeat timer starts again from zero.
                held = cur_dir;
                key_repeat.press();
            } else if key.is_none() && cur_dir != 0 {
                if key_repeat.tick(true) {
                    key = key_from(cur_dir);
                    repeated = true;
                }
            } else if key.is_none() {
                // Nothing held: the timer is idle, not paused.
                key_repeat.tick(false);
            }
            // One gesture, one undo step: the first press opens it, the repeats
            // this flag marks fold into it (Editor::push_undo), and the release
            // closes it again.
            ed.set_repeating(repeated);

            // The update dialog is modal: while it is up the pad answers it and
            // nothing else, so a press cannot edit a value on the screen behind
            // it. [A] is the dialog's own action, [B] puts the offer away for
            // this session, [Y] flips the start-up check.
            if painter.gui.update.dialog {
                if let Some(k) = key {
                    match k {
                        Key::A => painter.gui.update.primary(),
                        Key::B => painter.gui.update.dismiss(),
                        Key::Y => painter.gui.update.toggle_setting(),
                        _ => {}
                    }
                    need_draw = true;
                } else if painter.gui.update.busy() {
                    // The percentage moves: the bar has to be redrawn.
                    need_draw = true;
                }
                // A finger answers the dialog's three buttons and nothing else:
                // GuiState::handle_touch has the same modal rule the review's
                // dialog has, so a tap cannot reach the list underneath.
                let mut touch_state = sys::HidTouchScreenState::ZERO;
                let n_states = sys::hidGetTouchScreenStates(&mut touch_state, 1);
                let (touching, tx, ty) = if n_states > 0 && touch_state.count > 0 {
                    let t = touch_state.touches[0];
                    (true, t.x as i32, t.y as i32)
                } else {
                    (false, 0, 0)
                };
                if painter.gui.handle_touch(&mut ed, touching, tx, ty) {
                    need_draw = true;
                }
                if painter.gui.update.restart_pending {
                    break;
                }
                if need_draw {
                    painter.draw(&ed);
                    need_draw = false;
                }
                sys::svcSleepThread(16_000_000);
                continue;
            }

            // The banner's [A]. It is answered on the start screen only, and
            // only while the banner is up: everywhere else [A] is the screen's
            // own key (開く / 編集) and a press must never open a dialog in the
            // middle of an edit. The same dialog is reachable from any screen by
            // tapping the banner, and from the start screen at any time by
            // holding [R].
            if ed.screen == mhnx_core::hbedit::Screen::SaveList
                && painter.gui.update.banner_visible()
                && down & BTN_A != 0
            {
                painter.gui.update.open_dialog();
                need_draw = true;
                // The press belongs to the banner, not to the save under it.
                key = None;
            }

            if painter.gui.osk.is_some() {
                // L / R answer the keyboard here, not the list: forget the
                // holds so a press that came up inside the keyboard cannot
                // reach the screen behind it later.
                l_tap = mhnx_core::hbedit::ShoulderTap::new();
                r_tap = mhnx_core::hbedit::ShoulderTap::new();
                r_target = None;
                l_repeat.tick(false);
                r_repeat.tick(false);
                // ZL walks the character tabs; ZR inserts a line break (Key::Z
                // is the raw ZR and no longer remapped to [R] here, because a
                // name field has to be able to hold a second line).
                if down & BTN_ZL != 0 {
                    key = Some(Key::L);
                }
                if let Some(k) = key {
                    if painter.gui.handle_osk_key(k) {
                        need_draw = true;
                    }
                }
            } else if painter.gui.mib_picker.is_some() {
                l_tap = mhnx_core::hbedit::ShoulderTap::new();
                r_tap = mhnx_core::hbedit::ShoulderTap::new();
                r_target = None;
                l_repeat.tick(false);
                r_repeat.tick(false);
                if down & BTN_RSTICK_UP != 0 {
                    painter.gui.page_by(&mut ed, -1);
                    need_draw = true;
                } else if down & BTN_RSTICK_DOWN != 0 {
                    painter.gui.page_by(&mut ed, 1);
                    need_draw = true;
                } else if down & BTN_RSTICK_LEFT != 0 {
                    painter.gui.jump_end(&mut ed, false);
                    need_draw = true;
                } else if down & BTN_RSTICK_RIGHT != 0 {
                    painter.gui.jump_end(&mut ed, true);
                    need_draw = true;
                }
                if let Some(k) = key {
                    if painter.gui.handle_mib_picker_key(&mut ed, k) {
                        need_draw = true;
                    }
                }
            } else if ed.review_open() {
                l_tap = mhnx_core::hbedit::ShoulderTap::new();
                r_tap = mhnx_core::hbedit::ShoulderTap::new();
                r_target = None;
                l_repeat.tick(false);
                r_repeat.tick(false);
                // The save review is modal: A writes, B goes back, and nothing
                // else is heard. The UI drops taps on anything but its two
                // buttons for the same reason - a tap must not reach the list
                // under the dialog.
                // The right stick turns a page of the body, the same way it
                // does on every list: the dialog's body is longer than the
                // panel, and the stick is the one input the dialog has free.
                if down & BTN_RSTICK_UP != 0 {
                    painter.gui.scroll_review(&ed, -10);
                    need_draw = true;
                } else if down & BTN_RSTICK_DOWN != 0 {
                    painter.gui.scroll_review(&ed, 10);
                    need_draw = true;
                }
                if let Some(k) = key {
                    match k {
                        Key::A => {
                            finish_reviewed_write(&mut ed, &mut painter.gui);
                            need_draw = true;
                        }
                        Key::B => {
                            ed.cancel_save_review();
                            // A restore's offer dies with its dialog: the save
                            // it kept a copy of is still what the console
                            // holds, so there is nothing else to undo.
                            painter.gui.pending_snapshot = None;
                            need_draw = true;
                        }
                        // The body scrolls: one line on ▲ / ▼, ten on ◀ / ▶.
                        // The two buttons in the dialog's footer send the same
                        // two steps, so a finger and the pad cannot disagree.
                        Key::Up => {
                            painter.gui.scroll_review(&ed, -1);
                            need_draw = true;
                        }
                        Key::Down => {
                            painter.gui.scroll_review(&ed, 1);
                            need_draw = true;
                        }
                        Key::Left => {
                            painter.gui.scroll_review(&ed, -10);
                            need_draw = true;
                        }
                        Key::Right => {
                            painter.gui.scroll_review(&ed, 10);
                            need_draw = true;
                        }
                        _ => {}
                    }
                }
            } else {
                // Plus quits everywhere except the two screens that give it a
                // tool: the equipment list opens its new-record form with it, and
                // the roster opens the skill panel. While that panel is open Plus
                // is the quit key again, like the equipment panel above it.
                // The state machine's answer, not a second copy of the rule:
                // the footer is written from the same method
                // (hbedit::Editor::plus_is_a_tool), so the line and the key
                // cannot disagree about which screens spend [＋] on a tool.
                let plus_is_a_tool = ed.plus_is_a_tool();
                if down & BTN_PLUS != 0 && !plus_is_a_tool {
                    // A dirty save is not thrown away on one press: the first
                    // [＋] only says what would be lost, the second one quits.
                    if ed.press_quit() {
                        break;
                    }
                    // The press is spent. Every screen's own Key::Plus arm calls
                    // press_quit as well, so handing the same press to the state
                    // machine below answered the offer it had just made: one
                    // [＋] on a dirty save armed the quit and then took it, and
                    // the session ended with the edit still in memory.
                    if key == Some(Key::Plus) {
                        key = None;
                    }
                    need_draw = true;
                }

                // Any other button takes the [＋] offer back, including the
                // ones this loop answers itself (the sidebar and the touch
                // screen never reach Editor::input).
                if down != 0 && down & BTN_PLUS == 0 {
                    ed.clear_quit_arm();
                }

                // ZL walks the sidebar back; ZR is the sidebar's forward key
                // only on the screens that have no second action of their own.
                //
                // ZR used to be the section key everywhere and the key was then
                // dropped before the state machine saw it, so every screen that
                // answered `Key::Z` - the progress screen's search, the item
                // set name, the palico skill panel - was answering a key nobody
                // could press. hbedit::Editor::zr_job is the one answer to
                // "whose key is this", and it is the same answer the footer's
                // guide is written from.
                if down & BTN_ZL != 0 {
                    painter.gui.cycle_sidebar_tab(&mut ed, false);
                    need_draw = true;
                } else if down & BTN_ZR != 0 {
                    if ed.zr_job() == mhnx_core::hbedit::ZrJob::Screen {
                        painter.gui.stop_fling();
                        painter.gui.handle_pad_key(&mut ed, Key::Z, cur);
                    } else {
                        painter.gui.cycle_sidebar_tab(&mut ed, true);
                    }
                    need_draw = true;
                }

                // The right stick is the coarse scroll input: up / down turn a
                // page - as many rows as the screen shows - and left / right
                // jump to the ends of the list. It is the one input that was
                // free for it: ZL / ZR are the section keys, L / R the sub-tabs,
                // and [＋] / [－] are a tool of their own on most screens. It was
                // already the scroll key; it just moved a fixed eight rows,
                // which is not a page on any screen that shows nine.
                if down & BTN_RSTICK_UP != 0 {
                    painter.gui.page_by(&mut ed, -1);
                    need_draw = true;
                } else if down & BTN_RSTICK_DOWN != 0 {
                    painter.gui.page_by(&mut ed, 1);
                    need_draw = true;
                } else if down & BTN_RSTICK_LEFT != 0 {
                    painter.gui.jump_end(&mut ed, false);
                    need_draw = true;
                } else if down & BTN_RSTICK_RIGHT != 0 {
                    painter.gui.jump_end(&mut ed, true);
                    need_draw = true;
                }

                // One press, one thing. The global rescan fires only where the
                // state machine says Minus has no other job (the save list);
                // every other screen either owns the key or refreshes itself as
                // part of its own action (the quest list's 展開 re-reads the
                // quest folder). The old test was a negative list, so every
                // Minus consumer added later fired both at once.
                if (down & BTN_MINUS != 0 && ed.minus_rescans()) || painter.gui.wants_rescan {
                    painter.gui.wants_rescan = false;
                    ed.set_save_files(scan_saves());
                    ed.set_arc_files(scan_quests());
                    ed.message = format!(
                        "再スキャン完了: セーブ {} 件 / クエスト {} 件",
                        ed.save_files.len(),
                        ed.arc_files.len()
                    );
                    need_draw = true;
                }

                // L / R, answered on the release. A tap is the sub-tab (or the
                // ten row step on a pick list); a hold offers the second job and
                // is not a tap at all. The two keys of the band answer on the
                // same frame - they used to answer on two different ones, [L] on
                // the press and [R] on the release.
                //
                // A pick list is the exception, exactly as it is for [R]: there
                // the shoulder is the ten-row step and there is no hold job at
                // all, so the press is answered at once and the auto-repeat
                // follows it.
                if down & BTN_L != 0 && l_tap.press() {
                    painter.gui.stop_fling();
                    if crate::ui::shoulder_moves(&ed) {
                        painter.gui.handle_pad_key(&mut ed, Key::L, cur);
                        l_repeat.press();
                        // The tap was this press: the release owes nothing.
                        l_tap.spend();
                    }
                    need_draw = true;
                }
                // [R]: the tap waits for the release, the hold is 詳細.
                //
                // It used to be answered on the press like [L], and the review
                // found what that costs: holding R to read the bytes behind a
                // screen also moved the sub-tab, so the screen changed under
                // the hold and 詳細 opened on the wrong one. The press only
                // remembers where it happened (hbedit::DetailTarget) now; the
                // release decides between the tap and the hold
                // (hbedit::ShoulderTap).
                //
                // A pick list is the exception: there [R] is the ten-row step
                // and there is no hold job at all, so the press is answered at
                // once and the auto-repeat follows it.
                if down & BTN_R != 0 && r_tap.press() {
                    painter.gui.stop_fling();
                    if crate::ui::shoulder_moves(&ed) {
                        painter.gui.handle_pad_key(&mut ed, Key::R, cur);
                        r_repeat.press();
                        // The tap was this press: the release owes nothing.
                        r_tap.spend();
                    } else {
                        r_target = Some(ed.detail_target(painter.gui.sets_tab));
                    }
                    need_draw = true;
                }
                // A held shoulder repeats where L / R mean movement. On every
                // other screen the shoulder is the sub-tab key, and repeating
                // that would walk the tab strip while the finger rests.
                if crate::ui::shoulder_moves(&ed) {
                    if cur & BTN_L != 0 {
                        if l_repeat.tick(true) {
                            painter.gui.handle_pad_key(&mut ed, Key::L, cur);
                            need_draw = true;
                        }
                    } else {
                        l_repeat.tick(false);
                    }
                    if cur & BTN_R != 0 {
                        if r_repeat.tick(true) {
                            painter.gui.handle_pad_key(&mut ed, Key::R, cur);
                            need_draw = true;
                        }
                    } else {
                        r_repeat.tick(false);
                    }
                } else {
                    // L: the whole-save undo, two holds. R: 詳細, which is a
                    // view rather than a destructive action, so the first hold
                    // opens it and the offer is taken back at once so holding
                    // longer cannot open it twice.
                    match l_tap.tick() {
                        mhnx_core::confirm::HoldAction::Arm => {
                            ed.message = alloc::format!(
                                "もう一度 [L] を {} 秒長押しすると元に戻します（取り消す操作: {}）",
                                mhnx_core::confirm::HOLD_CONFIRM_FRAMES / 60,
                                ed.undo_label().unwrap_or("-")
                            );
                            need_draw = true;
                        }
                        mhnx_core::confirm::HoldAction::Run => {
                            if ed.undo_last() {
                                need_draw = true;
                            }
                        }
                        mhnx_core::confirm::HoldAction::None => {}
                    }
                    // R: 詳細. The target is the state captured when the key
                    // went down, so a hold that started on 進行状況's 依頼 view
                    // opens the 依頼フラグ region even though the release frame
                    // is a second later.
                    match r_tap.tick() {
                        mhnx_core::confirm::HoldAction::Arm
                        | mhnx_core::confirm::HoldAction::Run => {
                            r_tap.clear_arm();
                            let target = r_target
                                .unwrap_or_else(|| ed.detail_target(painter.gui.sets_tab));
                            // 詳細 has nothing to show on the start screen, so
                            // the hold is the settings door of the updater there:
                            // it is the one way in when the start-up check is off
                            // or the banner was already put away.
                            if target.screen == mhnx_core::hbedit::Screen::SaveList {
                                painter.gui.update.open_dialog();
                            } else {
                                painter.gui.open_detail_at(&mut ed, target);
                            }
                            need_draw = true;
                        }
                        mhnx_core::confirm::HoldAction::None => {}
                    }
                }
                // The keys came up: the holds restart from zero next time, and
                // a press that never reached the hold was a tap - its job runs
                // now, on the screen that is showing (nothing else can change it
                // while L is held).
                if cur & BTN_L == 0 {
                    if l_tap.release() {
                        painter.gui.stop_fling();
                        painter.gui.handle_pad_key(&mut ed, Key::L, cur);
                        need_draw = true;
                    }
                }
                if cur & BTN_R == 0 {
                    // A press that never reached the hold was a tap, and the
                    // tap's job runs now - on the screen that is showing, which
                    // is the one the key went down on (nothing else can change
                    // it while R is held).
                    if r_tap.release() {
                        painter.gui.stop_fling();
                        painter.gui.handle_pad_key(&mut ed, Key::R, cur);
                        need_draw = true;
                    }
                    r_target = None;
                }

                if let Some(k) = key {
                    // Don't double-send Key::Z when ZR was already used for tab
                    // cycling, and L / R are answered above (press = tap, hold =
                    // the second job).
                    if k != Key::Z && k != Key::L && k != Key::R {
                        // A key press takes the list back from a coasting flick:
                        // the two must never move the cursor at the same time.
                        painter.gui.stop_fling();
                        // Some other key means the holds' offers are gone: they
                        // have to be asked for again rather than waiting for the
                        // next hold however long later.
                        l_tap.cancel();
                        // The pending [R] tap is not owed any more: the user
                        // pressed something else while it was still down.
                        r_tap.cancel();
                        r_target = None;
                        painter.gui.handle_pad_key(&mut ed, k, cur);
                        need_draw = true;
                    }
                }
            }

            // Touch screen input
            let mut touch_state = sys::HidTouchScreenState::ZERO;
            let n_states = sys::hidGetTouchScreenStates(&mut touch_state, 1);
            let (touching, tx, ty) = if n_states > 0 && touch_state.count > 0 {
                let t = touch_state.touches[0];
                (true, t.x as i32, t.y as i32)
            } else {
                (false, 0, 0)
            };
            if painter.gui.handle_touch(&mut ed, touching, tx, ty) {
                need_draw = true;
            }
            if painter.gui.wants_rescan {
                painter.gui.wants_rescan = false;
                ed.set_save_files(scan_saves());
                ed.set_arc_files(scan_quests());
                ed.message = format!(
                    "再スキャン完了: セーブ {} 件 / クエスト {} 件",
                    ed.save_files.len(),
                    ed.arc_files.len()
                );
                need_draw = true;
            }

            let before = ed.message.clone();
            let forced = service(&mut ed, &mut painter.gui);
            if forced || ed.message != before {
                need_draw = true;
            }

            if ed.quit {
                break;
            }

            // The new .nro is on the card and the loader has been told to start
            // it: the process has to leave for that to happen.
            if painter.gui.update.restart_pending {
                break;
            }

            if need_draw {
                painter.draw(&ed);
                need_draw = false;
            }

            sys::svcSleepThread(16_000_000);
        }

        sys::fsExit();
        if !using_canvas {
            sys::consoleExit(core::ptr::null_mut());
        }
    }

    0
}
