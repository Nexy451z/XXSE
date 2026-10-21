//! Application logic shared by the `.nro` binary and the staticlib route.

use alloc::borrow::Cow;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::{c_char, c_void, CStr};
use core::panic::PanicInfo;

use crate::sys::{
    self, FsDir, FsDirectoryEntry, FsFile, FsFileSystem, PadState, Res, FS_DIR_OPEN_MODE_READ_DIRS,
    FS_DIR_OPEN_MODE_READ_FILES, FS_OPEN_MODE_READ, HID_NPAD_BUTTON_A, HID_NPAD_BUTTON_B,
    HID_NPAD_BUTTON_PLUS, HID_NPAD_STYLE_SET_NPAD_STANDARD, PAD_ANY_ID_MASK,
};

/// File the SD card demo reads. The SD root is the fsdev root, i.e. "/".
const DEMO_FILE: &CStr = c"/switch/XXSE/hello.txt";
/// Directory the SD card demo lists.
const DEMO_DIR: &CStr = c"/switch/XXSE";

/// Print one line through libnx' console (printf -> newlib -> libnx console).
pub fn log(args: core::fmt::Arguments<'_>) {
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let _ = core::fmt::write(&mut FmtToBytes(&mut buf), args);
    buf.push(b'\n');
    buf.push(0);
    unsafe { sys::printf(c"%s".as_ptr(), buf.as_ptr() as *const c_char) };
}

/// Adapter that lets `core::fmt` write straight into a byte buffer.
struct FmtToBytes<'a>(&'a mut Vec<u8>);

impl core::fmt::Write for FmtToBytes<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// Used by the `#[panic_handler]` in lib.rs.
pub fn print_panic(info: &PanicInfo) -> ! {
    log(format_args!("*** PANIC ***"));
    if let Some(loc) = info.location() {
        log(format_args!("  at {}:{}:{}", loc.file(), loc.line(), loc.column()));
    }
    log(format_args!("  {}", info.message()));
    loop {
        unsafe { sys::consoleUpdate(core::ptr::null_mut()) };
        unsafe { sys::svcSleepThread(1_000_000_000) };
    }
}

/// Program entry point (called by libnx crt0 through `main`).
pub fn run(argc: i32, argv: *const *const c_char) -> i32 {
    // The real editor lives in crate::editor; the SD probe below stays
    // available for diagnostics (see probe_sdcard).
    let _ = (argc, argv);
    return crate::editor::run();
    #[allow(unreachable_code)]
    {
    unsafe { sys::consoleInit(core::ptr::null_mut()) };

    banner(argc, argv);
    probe_sdcard();

    log(format_args!(""));
    log(format_args!("Press A to probe again, + or B to exit."));
    unsafe { sys::consoleUpdate(core::ptr::null_mut()) };

    unsafe {
        sys::padConfigureInput(1, HID_NPAD_STYLE_SET_NPAD_STANDARD);
        let mut pad = PadState::ZERO;
        sys::padInitializeWithMask(&mut pad, PAD_ANY_ID_MASK);

        while sys::appletMainLoop() {
            sys::padUpdate(&mut pad);
            let down = pad.buttons_down();

            if down & (HID_NPAD_BUTTON_PLUS | HID_NPAD_BUTTON_B) != 0 {
                log(format_args!("exit requested (buttons 0x{:X})", down));
                break;
            }
            if down & HID_NPAD_BUTTON_A != 0 {
                probe_sdcard();
            }

            sys::consoleUpdate(core::ptr::null_mut());
            sys::svcSleepThread(16_000_000); // ~60 fps
        }

        sys::consoleExit(core::ptr::null_mut());
    }

    0
    }
}

fn banner(argc: i32, argv: *const *const c_char) {
    log(format_args!("XXSE - MHXX Save Editor (Nintendo Switch)"));
    log(format_args!("mhnx-switch {} / {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
    log(format_args!("--------------------------------------------------"));

    if argv.is_null() {
        return;
    }
    for i in 0..argc.clamp(0, 8) {
        let p = unsafe { *argv.offset(i as isize) };
        if p.is_null() {
            break;
        }
        let s: Cow<'_, str> = unsafe { CStr::from_ptr(p) }.to_string_lossy();
        log(format_args!("argv[{i}] = {s}"));
    }
    log(format_args!("--------------------------------------------------"));
}

/// Shows that the SD card is reachable both through libnx' raw fs API and
/// through newlib stdio (libnx wires stdio to the `sdmc` device via fsdev).
fn probe_sdcard() {
    log(format_args!(""));
    log(format_args!("=== SD card probe ==="));

    let rc = unsafe { sys::fsInitialize() };
    log(format_args!("fsInitialize()                -> {}", Res(rc)));

    let mut fs = FsFileSystem::ZERO;
    let rc = unsafe { sys::fsOpenSdCardFileSystem(&mut fs) };
    log(format_args!("fsOpenSdCardFileSystem()      -> {}", Res(rc)));
    if !sys::r_succeeded(rc) {
        log(format_args!("  !! SD card not available (no card inserted?)"));
        return;
    }

    let (mut free, mut total) = (0i64, 0i64);
    let rc_free = unsafe { sys::fsFsGetFreeSpace(&mut fs, c"/".as_ptr(), &mut free) };
    let rc_total = unsafe { sys::fsFsGetTotalSpace(&mut fs, c"/".as_ptr(), &mut total) };
    log(format_args!(
        "fsFsGetFreeSpace(\"/\")        -> {} = {} MiB",
        Res(rc_free),
        free / (1024 * 1024)
    ));
    log(format_args!(
        "fsFsGetTotalSpace(\"/\")       -> {} = {} MiB",
        Res(rc_total),
        total / (1024 * 1024)
    ));

    list_directory(&mut fs);

    let mut file = FsFile::ZERO;
    let rc = unsafe { sys::fsFsOpenFile(&mut fs, DEMO_FILE.as_ptr(), FS_OPEN_MODE_READ, &mut file) };
    log(format_args!("fsFsOpenFile(DEMO_FILE)       -> {}", Res(rc)));
    if sys::r_succeeded(rc) {
        let mut size = 0i64;
        let _ = unsafe { sys::fsFileGetSize(&mut file, &mut size) };
        let mut buf = vec![0u8; 256];
        let mut read = 0u64;
        let rc = unsafe {
            sys::fsFileRead(
                &mut file,
                0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u64,
                0,
                &mut read,
            )
        };
        log(format_args!("fsFileGetSize()               -> {} bytes", size));
        log(format_args!("fsFileRead()                  -> {} ({} bytes)", Res(rc), read));
        if sys::r_succeeded(rc) && read > 0 {
            let text = core::str::from_utf8(&buf[..read as usize]).unwrap_or("<non-utf8>");
            for line in text.lines() {
                log(format_args!("  | {}", line));
            }
        }
        unsafe { sys::fsFileClose(&mut file) };
    }

    // Second route: plain stdio - this is what the editor will use on the SD card.
    let f = unsafe { sys::fopen(DEMO_FILE.as_ptr(), c"rb".as_ptr()) };
    if f.is_null() {
        log(format_args!("fopen(DEMO_FILE)              -> NULL (file missing?)"));
    } else {
        let mut buf = [0u8; 256];
        let n = unsafe { sys::fread(buf.as_mut_ptr() as *mut c_void, 1, buf.len(), f) };
        unsafe { sys::fclose(f) };
        log(format_args!("fopen()+fread() via newlib    -> {} bytes", n));
        if n > 0 {
            let text = core::str::from_utf8(&buf[..n]).unwrap_or("<non-utf8>");
            for line in text.lines() {
                log(format_args!("  | {}", line));
            }
        }
    }

    unsafe { sys::fsFsClose(&mut fs) };
    log(format_args!("=== SD card probe done ==="));
    log(format_args!(""));
}

/// Directory iteration through the raw libnx fs API.
fn list_directory(fs: &mut FsFileSystem) {
    let mut dir = FsDir::ZERO;
    let rc = unsafe {
        sys::fsFsOpenDirectory(
            fs,
            DEMO_DIR.as_ptr(),
            FS_DIR_OPEN_MODE_READ_DIRS | FS_DIR_OPEN_MODE_READ_FILES,
            &mut dir,
        )
    };
    log(format_args!("fsFsOpenDirectory(DEMO_DIR)   -> {}", Res(rc)));
    if !sys::r_succeeded(rc) {
        return;
    }

    let mut entries = [FsDirectoryEntry::ZERO; 16];
    let mut total_entries = 0i64;
    let rc = unsafe { sys::fsDirRead(&mut dir, &mut total_entries, entries.len(), entries.as_mut_ptr()) };
    log(format_args!("fsDirRead()                   -> {} (total {})", Res(rc), total_entries));
    if sys::r_succeeded(rc) {
        for e in entries.iter() {
            if e.is_empty() {
                continue;
            }
            log(format_args!(
                "   {} {:>10}  {}",
                if e.is_file() { "F" } else { "D" },
                e.file_size,
                e.name_str()
            ));
        }
    }
    unsafe { sys::fsDirClose(&mut dir) };
}