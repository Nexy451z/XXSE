//! Reading and writing the console's own save data.
//!
//! The save lives in the console's NAND, not on the SD card, so it cannot be
//! opened with a path. libnx exposes it instead through fsdev: mounting it as
//! a device makes "save:/system" behave like any other file. Nothing written
//! reaches the console until fsdevCommitDevice is called, and the mount must
//! always be released with fsdevUnmountDevice or the save is left in a state
//! the system treats as still open.

use core::ffi::{c_char, c_void, CStr};

use crate::sys;

/// MHXX's title id, the base game rather than the update.
pub const MHXX_TITLE_ID: u64 = 0x0100_C380_0049_C000;
/// The device name the save is mounted under.
pub const DEVICE: &CStr = c"save";
/// The save file inside that device.
pub const SAVE_FILE: &CStr = c"save:/system";
/// The companion backup file inside that device.
pub const SAVE_BACKUP_FILE: &CStr = c"save:/system_backup";

/// Why a save operation did not work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    /// The account service would not initialise.
    Account,
    /// No user is signed in, so there is no save to mount.
    NoUser,
    /// The title has no save data on this console yet.
    NotMounted(u32),
    /// The file could not be opened or read.
    Io,
    /// Detailed I/O error with step name and libnx Result code.
    IoDetail(&'static str, u32),
    /// The bytes are not an MHXX save.
    NotMhxx,
}

impl SaveError {
    /// A description with the raw result code, so a failure can be told apart
    /// from the others without guessing: the numbers are what libnx returns.
    pub fn describe(self) -> alloc::string::String {
        use alloc::string::ToString;
        match self {
            SaveError::NotMounted(rc) => alloc::format!(
                "本体セーブを開けません（コード 0x{:08X}）。MHXXが裏で起動中の場合は一度ゲームを終了してください",
                rc
            ),
            SaveError::IoDetail(step, rc) => alloc::format!(
                "セーブの読み書きに失敗しました（{} / コード 0x{:08X}）",
                step,
                rc
            ),
            SaveError::Io => {
                let rc = unsafe { sys::fsdevGetLastResult() };
                if rc != 0 {
                    alloc::format!("セーブの読み書きに失敗しました（コード 0x{:08X}）", rc)
                } else {
                    alloc::string::String::from("セーブの読み書きに失敗しました")
                }
            }
            other => other.text().to_string(),
        }
    }

    /// A short Japanese description for the screen.
    pub fn text(self) -> &'static str {
        match self {
            SaveError::Account => "アカウントサービスの初期化に失敗しました",
            SaveError::NoUser => "ユーザーが選択されていません",
            SaveError::NotMounted(_) => "このゲームのセーブがまだありません（一度ゲームを起動してください）",
            SaveError::Io | SaveError::IoDetail(_, _) => "セーブの読み書きに失敗しました",
            SaveError::NotMhxx => "MHXX のセーブではありません",
        }
    }
}

/// A mounted save. Unmounting happens on drop so an early return cannot leak it.
pub struct Mount {
    mounted: bool,
}

impl Mount {
    /// Mount the MHXX save of the currently selected or active user.
    ///
    /// Supports both Title Override mode (`acc:u0`) and Applet / Album mode
    /// (`acc:u1` / `acc:su`), and falls back across preselected user, last
    /// opened user, auto-selected user, and all console user profiles so that
    /// the profile owning MHXX save data is found automatically.
    pub fn open() -> Result<Mount, SaveError> {
        unsafe {
            let service_types = [
                sys::ACCOUNT_SERVICE_TYPE_APPLICATION,
                sys::ACCOUNT_SERVICE_TYPE_SYSTEM,
                sys::ACCOUNT_SERVICE_TYPE_ADMINISTRATOR,
            ];
            let mut acc_ok = false;
            for &mode in &service_types {
                if sys::accountInitialize(mode) == 0 {
                    acc_ok = true;
                    break;
                }
            }
            if !acc_ok {
                return Err(SaveError::Account);
            }

            let mut candidates: alloc::vec::Vec<sys::AccountUid> = alloc::vec::Vec::new();
            let mut push_unique = |uid: sys::AccountUid| {
                if uid.is_valid() && !candidates.contains(&uid) {
                    candidates.push(uid);
                }
            };

            let mut uid = sys::AccountUid::default();
            if sys::accountGetPreselectedUser(&mut uid) == 0 {
                push_unique(uid);
            }
            uid = sys::AccountUid::default();
            if sys::accountGetLastOpenedUser(&mut uid) == 0 {
                push_unique(uid);
            }
            uid = sys::AccountUid::default();
            if sys::accountTrySelectUserWithoutInteraction(&mut uid, false) == 0 {
                push_unique(uid);
            }
            let mut all_uids = [sys::AccountUid::default(); 8];
            let mut total: i32 = 0;
            if sys::accountListAllUsers(all_uids.as_mut_ptr(), 8, &mut total) == 0 {
                let count = (total.max(0) as usize).min(all_uids.len());
                for &u in &all_uids[..count] {
                    push_unique(u);
                }
            }

            sys::accountExit();

            if candidates.is_empty() {
                return Err(SaveError::NoUser);
            }

            let mut last_rc = 0;
            for candidate in candidates {
                let rc = sys::fsdevMountSaveData(DEVICE.as_ptr(), MHXX_TITLE_ID, candidate);
                if rc == 0 {
                    return Ok(Mount { mounted: true });
                }
                last_rc = rc;
            }
            Err(SaveError::NotMounted(last_rc))
        }
    }

    /// Read the whole save file into a buffer.
    pub fn read(&self) -> Result<Vec<u8>, SaveError> {
        read_file(SAVE_FILE).ok_or(SaveError::Io)
    }

    /// Replace the save file's contents in-place.
    pub fn write(&self, bytes: &[u8]) -> Result<(), SaveError> {
        self.write_diff(bytes, None)
    }

    /// Write `save:/system` in-place (only writing modified 16 KB blocks when
    /// `old` is provided) and also keep `save:/system_backup` in sync if it
    /// exists on the console.
    pub fn write_diff(&self, bytes: &[u8], old: Option<&[u8]>) -> Result<(), SaveError> {
        unsafe {
            if remove(c"save:/system.new".as_ptr()) == 0 {
                let _ = sys::fsdevCommitDevice(DEVICE.as_ptr());
            }
        }
        write_save_in_place(SAVE_FILE, bytes, old)?;

        if bytes.len() > mhnx_core::save::SWITCH_HEADER_SIZE {
            if let Some(old_bak) = read_file(SAVE_BACKUP_FILE) {
                if old_bak.len() == bytes.len() {
                    let body = &bytes[mhnx_core::save::SWITCH_HEADER_SIZE..];
                    let hdr = mhnx_core::save::switch_header("system_backup", body.len());
                    let mut bak_bytes = alloc::vec::Vec::with_capacity(bytes.len());
                    bak_bytes.extend_from_slice(&hdr);
                    bak_bytes.extend_from_slice(body);
                    let _ = write_save_in_place(SAVE_BACKUP_FILE, &bak_bytes, Some(&old_bak));
                }
            }
        }
        Ok(())
    }

    /// Push the written bytes to the console.
    pub fn commit(&self) -> Result<(), SaveError> {
        unsafe {
            let rc = sys::fsdevCommitDevice(DEVICE.as_ptr());
            if rc == 0 {
                Ok(())
            } else {
                Err(SaveError::IoDetail("commit", rc))
            }
        }
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        if self.mounted {
            unsafe {
                sys::fsdevUnmountDevice(DEVICE.as_ptr());
            }
            self.mounted = false;
        }
    }
}

extern "C" {
    fn time(t: *mut i64) -> i64;
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    fn fclose(stream: *mut c_void) -> i32;
    fn fseek(stream: *mut c_void, off: i64, whence: i32) -> i32;
    fn ftell(stream: *mut c_void) -> i64;
    fn remove(path: *const c_char) -> i32;
}

const SEEK_SET: i32 = 0;
const SEEK_END: i32 = 2;

/// Read a whole file, or None when it cannot be opened.
pub fn read_file(path: &CStr) -> Option<Vec<u8>> {
    unsafe {
        let f = fopen(path.as_ptr(), c"rb".as_ptr());
        if f.is_null() {
            return None;
        }
        fseek(f, 0, SEEK_END);
        let len = ftell(f);
        if len < 0 {
            fclose(f);
            return None;
        }
        fseek(f, 0, SEEK_SET);
        let mut buf = alloc::vec![0u8; len as usize];
        let n = fread(buf.as_mut_ptr() as *mut c_void, 1, buf.len(), f);
        fclose(f);
        if n != buf.len() {
            return None;
        }
        Some(buf)
    }
}

/// Write a save file in-place (`r+b` with fallback to `wb`), committing every
/// 256 KB so the SaveData journal never overflows. When `old` has the same
/// length as `bytes`, only modified 16 KB blocks are written.
fn write_save_in_place(
    path: &CStr,
    bytes: &[u8],
    old: Option<&[u8]>,
) -> Result<(), SaveError> {
    const CHUNK: usize = 16 * 1024;
    const COMMIT_THRESHOLD: usize = 256 * 1024;

    unsafe {
        let can_diff = old.map(|o| o.len() == bytes.len()).unwrap_or(false);
        let mut f = fopen(path.as_ptr(), c"r+b".as_ptr());
        if f.is_null() {
            f = fopen(path.as_ptr(), c"wb".as_ptr());
        }
        if f.is_null() {
            let rc = sys::fsdevGetLastResult();
            return Err(SaveError::IoDetail("fopen", rc));
        }

        let mut uncommitted = 0usize;
        let mut any_written = false;
        let mut offset = 0usize;

        while offset < bytes.len() {
            let end = (offset + CHUNK).min(bytes.len());
            let slice = &bytes[offset..end];
            let needs_write = if can_diff {
                let old_slice = &old.unwrap()[offset..end];
                slice != old_slice
            } else {
                true
            };

            if needs_write {
                if fseek(f, offset as i64, SEEK_SET) != 0 {
                    let rc = sys::fsdevGetLastResult();
                    fclose(f);
                    return Err(SaveError::IoDetail("fseek", rc));
                }
                let n = fwrite(slice.as_ptr() as *const c_void, 1, slice.len(), f);
                if n != slice.len() {
                    let rc = sys::fsdevGetLastResult();
                    fclose(f);
                    return Err(SaveError::IoDetail("fwrite", rc));
                }
                any_written = true;
                uncommitted += slice.len();

                if uncommitted >= COMMIT_THRESHOLD {
                    if fclose(f) != 0 {
                        let rc = sys::fsdevGetLastResult();
                        return Err(SaveError::IoDetail("fclose", rc));
                    }
                    let rc = sys::fsdevCommitDevice(DEVICE.as_ptr());
                    if rc != 0 {
                        return Err(SaveError::IoDetail("commit", rc));
                    }
                    f = fopen(path.as_ptr(), c"r+b".as_ptr());
                    if f.is_null() {
                        let rc2 = sys::fsdevGetLastResult();
                        return Err(SaveError::IoDetail("reopen", rc2));
                    }
                    uncommitted = 0;
                }
            }
            offset = end;
        }

        if !any_written && !bytes.is_empty() {
            let hdr_len = bytes.len().min(36);
            if fseek(f, 0, SEEK_SET) == 0 {
                let _ = fwrite(bytes.as_ptr() as *const c_void, 1, hdr_len, f);
            }
        }

        if fclose(f) != 0 {
            let rc = sys::fsdevGetLastResult();
            return Err(SaveError::IoDetail("fclose", rc));
        }
        let rc = sys::fsdevCommitDevice(DEVICE.as_ptr());
        if rc != 0 {
            return Err(SaveError::IoDetail("commit", rc));
        }
    }
    Ok(())
}

/// A sortable stamp for backup file names.
pub fn stamp() -> alloc::string::String {
    let t = unsafe { time(core::ptr::null_mut()) };
    alloc::format!("{}", t)
}

/// Is this the length of an MHXX save in Switch form?
pub fn looks_like_save(bytes: &[u8]) -> bool {
    bytes.len() == 4_726_188
}

use alloc::vec::Vec;
