//! Raw FFI declarations for libnx (devkitA64) and newlib.
//!
//! Everything here is transcribed from the headers installed in
//! `$(DEVKITPRO)/libnx/include` - no bindgen run, no extra crates, so the
//! layouts stay reviewable.  Compile-time assertions guard the struct layouts
//! that would otherwise silently corrupt the stack.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_void};

/// libnx `Result`.
pub type Result = u32;
/// libnx `Handle`.
pub type Handle = u32;

// ---------------------------------------------------------------------------
// Structures (see libnx/include/switch/sf/service.h, services/fs.h, runtime/pad.h)
// ---------------------------------------------------------------------------

/// `Service` (switch/sf/service.h): { Handle session; u32 own_handle;
/// u32 object_id; u16 pointer_buffer_size; }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Service {
    pub session: Handle,
    pub own_handle: u32,
    pub object_id: u32,
    pub pointer_buffer_size: u16,
}

/// `FsFileSystem` (services/fs.h): { Service s; }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsFileSystem {
    pub s: Service,
}

/// `FsFile` (services/fs.h): { Service s; }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsFile {
    pub s: Service,
}

/// `FsDir` (services/fs.h): { Service s; }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsDir {
    pub s: Service,
}

const _: () = {
    use core::mem::size_of;
    assert!(size_of::<Service>() == 16);
    assert!(size_of::<FsFileSystem>() == 16);
    assert!(size_of::<FsFile>() == 16);
    assert!(size_of::<FsDir>() == 16);
};

impl Service {
    pub const ZERO: Self = Self { session: 0, own_handle: 0, object_id: 0, pointer_buffer_size: 0 };
}
impl FsFileSystem {
    pub const ZERO: Self = Self { s: Service::ZERO };
}
impl FsFile {
    pub const ZERO: Self = Self { s: Service::ZERO };
}
impl FsDir {
    pub const ZERO: Self = Self { s: Service::ZERO };
}

/// `FsDirectoryEntry` (services/fs.h):
/// { char name[FS_MAX_PATH=0x301]; u8 pad[3]; s8 type; u8 pad2[3]; s64 file_size; }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsDirectoryEntry {
    pub name: [c_char; 0x301],
    pub pad: [u8; 3],
    pub entry_type: i8,
    pub pad2: [u8; 3],
    pub file_size: i64,
}

const _: () = {
    assert!(core::mem::size_of::<FsDirectoryEntry>() == 784);
    assert!(core::mem::offset_of!(FsDirectoryEntry, entry_type) == 772);
    assert!(core::mem::offset_of!(FsDirectoryEntry, file_size) == 776);
};

impl FsDirectoryEntry {
    pub const ZERO: Self = Self {
        name: [0; 0x301],
        pad: [0; 3],
        entry_type: 0,
        pad2: [0; 3],
        file_size: 0,
    };

    /// Name of the entry as a lossy &str (the raw bytes are UTF-8 on the SD card).
    pub fn name_str(&self) -> &str {
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(self.name.as_ptr() as *const u8, self.name.len()) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        core::str::from_utf8(&bytes[..end]).unwrap_or("<non-utf8>")
    }

    pub fn is_empty(&self) -> bool {
        self.name[0] == 0
    }

    pub fn is_file(&self) -> bool {
        self.entry_type == FS_DIR_ENTRY_TYPE_FILE
    }
}

/// `HidAnalogStickState` (services/hid.h): { s32 x; s32 y; }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HidAnalogStickState {
    pub x: i32,
    pub y: i32,
}

/// `PadState` (runtime/pad.h).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PadState {
    pub id_mask: u8,
    pub active_id_mask: u8,
    pub read_handheld: bool,
    pub active_handheld: bool,
    pub style_set: u32,
    pub attributes: u32,
    pub buttons_cur: u64,
    pub buttons_old: u64,
    pub sticks: [HidAnalogStickState; 2],
    pub gc_triggers: [u32; 2],
}

const _: () = {
    assert!(core::mem::size_of::<PadState>() == 56);
    assert!(core::mem::offset_of!(PadState, buttons_cur) == 16);
    assert!(core::mem::offset_of!(PadState, buttons_old) == 24);
    assert!(core::mem::offset_of!(PadState, sticks) == 32);
};

impl PadState {
    pub const ZERO: Self = Self {
        id_mask: 0,
        active_id_mask: 0,
        read_handheld: false,
        active_handheld: false,
        style_set: 0,
        attributes: 0,
        buttons_cur: 0,
        buttons_old: 0,
        sticks: [HidAnalogStickState { x: 0, y: 0 }; 2],
        gc_triggers: [0; 2],
    };

    /// `padGetButtonsDown()` is an inline in pad.h: ~buttons_old & buttons_cur.
    pub fn buttons_down(&self) -> u64 {
        !self.buttons_old & self.buttons_cur
    }
}

/// Opaque `PrintConsole` (runtime/devices/console.h) - we always pass NULL,
/// which makes libnx use its built-in default console.
#[repr(C)]
pub struct PrintConsole {
    _opaque: [u8; 0],
}

// ---------------------------------------------------------------------------
// Constants (libnx enums)
// ---------------------------------------------------------------------------

/// `HidNpadStyleSet_NpadStandard`.
pub const HID_NPAD_STYLE_SET_NPAD_STANDARD: u32 = 0x1F;
/// `PAD_ANY_ID_MASK` (pad.h).
pub const PAD_ANY_ID_MASK: u64 = 0x1000_100FF;
pub const HID_NPAD_BUTTON_A: u64 = 1 << 0;
pub const HID_NPAD_BUTTON_B: u64 = 1 << 1;
pub const HID_NPAD_BUTTON_PLUS: u64 = 1 << 10;

/// `FsOpenMode_Read`.
pub const FS_OPEN_MODE_READ: u32 = 0x1;
/// `FsDirOpenMode_ReadDirs`.
pub const FS_DIR_OPEN_MODE_READ_DIRS: u32 = 0x1;
/// `FsDirOpenMode_ReadFiles`.
pub const FS_DIR_OPEN_MODE_READ_FILES: u32 = 0x2;
/// `FsDirEntryType_File`.
pub const FS_DIR_ENTRY_TYPE_FILE: i8 = 1;

// ---------------------------------------------------------------------------
// libnx / newlib functions
// ---------------------------------------------------------------------------

extern "C" {
    // --- runtime/devices/console.h -------------------------------------------------
    pub fn consoleInit(con: *mut PrintConsole) -> *mut PrintConsole;
    pub fn consoleExit(con: *mut PrintConsole);
    pub fn consoleUpdate(con: *mut PrintConsole);

    // --- newlib stdio / heap -------------------------------------------------------
    pub fn printf(fmt: *const c_char, ...) -> c_int;
    pub fn puts(s: *const c_char) -> c_int;
    pub fn consoleClear();
    pub fn malloc(size: usize) -> *mut c_void;
    pub fn free(ptr: *mut c_void);
    pub fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    pub fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    pub fn fclose(stream: *mut c_void) -> c_int;

    // --- services/hid.h + runtime/pad.h --------------------------------------------
    pub fn padConfigureInput(max_players: u32, style_set: u32);
    pub fn padInitializeWithMask(pad: *mut PadState, mask: u64);
    pub fn padUpdate(pad: *mut PadState);

    // --- services/applet.h ---------------------------------------------------------
    pub fn appletMainLoop() -> bool;

    // --- kernel/svc.h --------------------------------------------------------------
    pub fn svcSleepThread(nano: i64);

    // --- services/fs.h -------------------------------------------------------------
    pub fn fsInitialize() -> Result;
    pub fn fsExit();
    pub fn fsOpenSdCardFileSystem(out: *mut FsFileSystem) -> Result;
    pub fn fsFsGetFreeSpace(fs: *mut FsFileSystem, path: *const c_char, out: *mut i64) -> Result;
    pub fn fsFsGetTotalSpace(fs: *mut FsFileSystem, path: *const c_char, out: *mut i64) -> Result;
    pub fn fsFsOpenFile(fs: *mut FsFileSystem, path: *const c_char, mode: u32, out: *mut FsFile) -> Result;
    pub fn fsFileGetSize(f: *mut FsFile, out: *mut i64) -> Result;
    pub fn fsFileRead(
        f: *mut FsFile,
        off: i64,
        buf: *mut c_void,
        read_size: u64,
        option: u32,
        bytes_read: *mut u64,
    ) -> Result;
    pub fn fsFileClose(f: *mut FsFile);
    pub fn fsFsClose(fs: *mut FsFileSystem);
    pub fn fsFsOpenDirectory(fs: *mut FsFileSystem, path: *const c_char, mode: u32, out: *mut FsDir) -> Result;
    pub fn fsDirRead(d: *mut FsDir, total_entries: *mut i64, max_entries: usize, buf: *mut FsDirectoryEntry) -> Result;
    pub fn fsDirClose(d: *mut FsDir);
}

// ---------------------------------------------------------------------------
// Result helpers (libnx/include/switch/result.h)
// ---------------------------------------------------------------------------

pub const fn r_succeeded(res: Result) -> bool {
    res == 0
}
pub const fn r_module(res: Result) -> u32 {
    res & 0x1FF
}
pub const fn r_description(res: Result) -> u32 {
    (res >> 9) & 0x1FFF
}

/// `Result` wrapper so it can be printed with `{}`.
pub struct Res(pub Result);

impl core::fmt::Display for Res {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if r_succeeded(self.0) {
            write!(f, "0x00000000 (success)")
        } else {
            write!(
                f,
                "0x{:08X} (module {}, description {})",
                self.0,
                r_module(self.0),
                r_description(self.0)
            )
        }
    }
}
/// Print one line through libnx' console (UTF-8 safe: it is plain printf).
pub fn log_line(s: &str) {
    let mut b: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(s.len() + 1);
    b.extend_from_slice(s.as_bytes());
    b.push(0);
    unsafe { printf(c"%s\n".as_ptr(), b.as_ptr() as *const c_char) };
}
