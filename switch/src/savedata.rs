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
    /// The bytes are not an MHXX save.
    NotMhxx,
}

impl SaveError {
    /// A short Japanese description for the screen.
    pub fn text(self) -> &'static str {
        match self {
            SaveError::Account => "アカウントサービスの初期化に失敗しました",
            SaveError::NoUser => "ユーザーが選択されていません",
            SaveError::NotMounted(_) => "このゲームのセーブがまだありません（一度ゲームを起動してください）",
            SaveError::Io => "セーブの読み書きに失敗しました",
            SaveError::NotMhxx => "MHXX のセーブではありません",
        }
    }
}

/// A mounted save. Unmounting happens on drop so an early return cannot leak it.
pub struct Mount {
    mounted: bool,
}

impl Mount {
    /// Mount the MHXX save of the currently selected user.
    pub fn open() -> Result<Mount, SaveError> {
        unsafe {
            let rc = sys::accountInitialize(sys::ACCOUNT_SERVICE_TYPE_APPLICATION);
            if rc != 0 {
                return Err(SaveError::Account);
            }
            let mut uid = sys::AccountUid::default();
            let rc = sys::accountGetPreselectedUser(&mut uid);
            sys::accountExit();
            if rc != 0 {
                return Err(SaveError::NoUser);
            }
            let rc = sys::fsdevMountSaveData(DEVICE.as_ptr(), MHXX_TITLE_ID, uid);
            if rc != 0 {
                return Err(SaveError::NotMounted(rc));
            }
            Ok(Mount { mounted: true })
        }
    }

    /// Read the whole save file into a buffer.
    pub fn read(&self) -> Result<Vec<u8>, SaveError> {
        read_file(SAVE_FILE).ok_or(SaveError::Io)
    }

    /// Replace the save file's contents.
    ///
    /// The bytes are written to a temporary file first and only then moved
    /// into place, so an interrupted write cannot leave a half written save.
    pub fn write(&self, bytes: &[u8]) -> Result<(), SaveError> {
        write_file_atomic(SAVE_FILE, bytes).ok_or(SaveError::Io)
    }

    /// Push the written bytes to the console.
    pub fn commit(&self) -> Result<(), SaveError> {
        unsafe {
            let rc = sys::fsdevCommitDevice(DEVICE.as_ptr());
            if rc == 0 {
                Ok(())
            } else {
                Err(SaveError::Io)
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
    fn rename(from: *const c_char, to: *const c_char) -> i32;
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

/// Write a whole file through a temporary name, then rename it into place.
fn write_file_atomic(path: &CStr, bytes: &[u8]) -> Option<()> {
    // "save:/system.new" shares the device prefix with the target.
    let tmp = c"save:/system.new";
    unsafe {
        remove(tmp.as_ptr());
        let f = fopen(tmp.as_ptr(), c"wb".as_ptr());
        if f.is_null() {
            return None;
        }
        let n = fwrite(bytes.as_ptr() as *const c_void, 1, bytes.len(), f);
        let closed = fclose(f);
        if n != bytes.len() || closed != 0 {
            remove(tmp.as_ptr());
            return None;
        }
        if rename(tmp.as_ptr(), path.as_ptr()) != 0 {
            remove(tmp.as_ptr());
            return None;
        }
    }
    Some(())
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