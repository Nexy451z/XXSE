//! Software keyboard (swkbd) input for the Switch build.
//!
//! Public surface (fixed, see the task/tests that call it):
//!
//!     swkbd::input_text(header, initial, max_len) -> Option<String>
//!     swkbd::is_available()                       -> bool
//!
//! `input_text` shows the system keyboard (the `swkbd` LibraryApplet) and returns the
//! text the user entered as a UTF-8 `String`.  It returns `None` when
//!
//!   * the user pressed Cancel (libnx reports `LibnxError_LibAppletBadExit`),
//!   * the applet could not be launched at all - typically because the process
//!     is *not* running in application mode (hbmenu launched from the album runs
//!     as `AppletType_LibraryApplet`, which cannot create library applets),
//!   * the config/buffer allocation failed, or the returned bytes were not UTF-8.
//!
//! The caller is expected to fall back to some ASCII input path when this
//! returns `None`.
//!
//! Everything below is a hand transcription of libnx' headers - no bindgen, no
//! extra crates, exactly like `sys.rs`.  The layout of every struct that crosses
//! the FFI boundary is pinned by `const` assertions whose expected values were
//! measured with devkitA64 gcc for this exact header version; see
//! `docs/switch_swkbd.md` for the probe, the raw numbers and the limits.

#![allow(non_camel_case_types)]

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_void};

// ---------------------------------------------------------------------------
// libnx scalar types / enum values
// ---------------------------------------------------------------------------

/// libnx `Result` (`switch/result.h`).  0 == success (`R_SUCCEEDED(r)`).
///
/// Deliberately local instead of `crate::sys::Result` so this module stays a
/// self-contained wrapper that only needs libnx' symbols.
type Result = u32;

/// `SwkbdType_Normal` (`applets/swkbd.h:21`).
///
/// 0 is the *standard* keyboard: it follows the system language and offers the
/// globe key, i.e. it is the type that gives Japanese kana/flick input plus
/// kanji conversion on a Japanese console.  (`SwkbdType_QWERTY` = 2, which is
/// what `swkbdConfigMakePresetDefault` sets, locks the user to the Latin QWERTY
/// layout, so it is overridden below.  `SwkbdType_All` = 8 would expose every
/// language keyboard instead of only the system one.)
const SWKBD_TYPE_NORMAL: u32 = 0;

/// `AppletType_Application` (`services/applet.h:26`).
const APPLET_TYPE_APPLICATION: i32 = 0;
/// `AppletType_SystemApplication` (`services/applet.h:30`).
const APPLET_TYPE_SYSTEM_APPLICATION: i32 = 4;

/// Hard cap for the requested input length, in UTF-16 code units.
///
/// The swkbd applet returns at most 2004 bytes (= 1002 u16 units) of UTF-16 and
/// libnx' own `swkbdConfigSetInitialText` converts at most 249 units, so 248 is
/// the largest value that is guaranteed to round-trip through `swkbdShow`
/// without truncation (`docs/switch_swkbd.md`).
const MAX_TEXT_UNITS: usize = 248;

/// `SwkbdArgCommon::headerText` holds 65 u16 (`applets/swkbd.h:131`); libnx'
/// `swkbdConfigSetHeaderText` converts at most 64 units into it, so `header` is
/// pre-truncated to 64 UTF-16 code units here (see `c_string` below - overlong
/// input must never reach libnx).
const MAX_HEADER_UNITS: usize = 64;

/// `SwkbdArgCommon::guideText` holds 257 u16 (`applets/swkbd.h:133`);
/// `swkbdConfigSetGuideText` converts at most 256 units.
const MAX_GUIDE_UNITS: usize = 256;

/// `swkbdConfigSetInitialText` converts at most 249 units into the work buffer
/// (`applets/swkbd.h:439`, disassembly in the doc).
const MAX_INITIAL_UNITS: usize = 249;

// ---------------------------------------------------------------------------
// Structures (switch/applets/swkbd.h) - field-for-field mirrors
// ---------------------------------------------------------------------------

/// `SwkbdArgCommon` (`applets/swkbd.h:122-147`).  Size 980 (0x3D4).
#[repr(C)]
pub struct SwkbdArgCommon {
    /// `SwkbdType type;` - C `enum` => 4-byte `u32` here.
    pub type_: u32,
    /// `u16 okButtonText[18/2];`
    pub ok_button_text: [u16; 9],
    /// `u16 leftButtonText;`
    pub left_button_text: u16,
    /// `u16 rightButtonText;`
    pub right_button_text: u16,
    /// `u8 dicFlag;`
    pub dic_flag: u8,
    /// `u8 pad_x1b;`
    pub pad_x1b: u8,
    /// `u32 keySetDisableBitmask;`
    pub key_set_disable_bitmask: u32,
    /// `u32 initialCursorPos;`
    pub initial_cursor_pos: u32,
    /// `u16 headerText[130/2];`
    pub header_text: [u16; 65],
    /// `u16 subText[258/2];`
    pub sub_text: [u16; 129],
    /// `u16 guideText[514/2];`
    pub guide_text: [u16; 257],
    /// `u16 pad_x3aa;`
    pub pad_x3aa: u16,
    /// `u32 stringLenMax;`
    pub string_len_max: u32,
    /// `u32 stringLenMin;`
    pub string_len_min: u32,
    /// `u32 passwordFlag;`
    pub password_flag: u32,
    /// `SwkbdTextDrawType textDrawType;` - `enum` => `u32`.
    pub text_draw_type: u32,
    /// `u16 returnButtonFlag;`
    pub return_button_flag: u16,
    /// `u8 blurBackground;`
    pub blur_background: u8,
    /// `u8 pad_x3bf;`
    pub pad_x3bf: u8,
    /// `u32 initialStringOffset;`
    pub initial_string_offset: u32,
    /// `u32 initialStringSize;`
    pub initial_string_size: u32,
    /// `u32 userDicOffset;`
    pub user_dic_offset: u32,
    /// `s32 userDicEntries;`
    pub user_dic_entries: i32,
    /// `u8 textCheckFlag;`
    pub text_check_flag: u8,
}

/// `SwkbdArgV0` (`applets/swkbd.h:149-153`).  Size 1000 (0x3E8).
#[repr(C)]
pub struct SwkbdArgV0 {
    /// `SwkbdArgCommon arg;`
    pub arg: SwkbdArgCommon,
    /// `u8 pad_x3d1[7];`
    pub pad_x3d1: [u8; 7],
    /// `SwkbdTextCheckCb textCheckCb;`
    ///
    /// A function pointer, never set by us (the preset clears it and we never
    /// install a callback).  Kept as `*mut c_void` so the struct stays
    /// `unsafe`-free to zero-initialise.
    pub text_check_cb: *mut c_void,
}

/// `SwkbdArgV7` (`applets/swkbd.h:156-159`).  Size 1032 (0x408).
#[repr(C)]
pub struct SwkbdArgV7 {
    /// `SwkbdArgV0 arg;`
    pub arg: SwkbdArgV0,
    /// `u32 textGrouping[8];`
    pub text_grouping: [u32; 8],
}

/// `SwkbdCustomizedDictionarySet` (`applets/swkbd.h:114-119`, `NX_PACKED`).
///
/// Packed in C (alignment 1), hence `packed` here; only its size matters for
/// `SwkbdConfig`, its fields are never read or written by this module.
#[repr(C, packed)]
pub struct SwkbdCustomizedDictionarySet {
    /// `void* buffer;`
    pub buffer: *mut c_void,
    /// `u32 buffer_size;`
    pub buffer_size: u32,
    /// `u64 entries[0x18];`
    pub entries: [u64; 0x18],
    /// `u16 total_entries;`
    pub total_entries: u16,
}

/// `SwkbdConfig` (`applets/swkbd.h:174-187`).  Size 1264 (0x4F0), align 8.
///
/// Handed to `swkbdCreate` / `swkbdShow` / `swkbdClose` by pointer only; those
/// functions read and write the fields above (their concrete offsets are pinned
/// by the assertions below).
#[repr(C)]
pub struct SwkbdConfig {
    /// `SwkbdArgV7 arg;`
    pub arg: SwkbdArgV7,
    /// `u8* workbuf;` - allocated by `swkbdCreate`, released by `swkbdClose`.
    pub workbuf: *mut c_void,
    /// `size_t workbuf_size;`
    pub workbuf_size: usize,
    /// `s32 max_dictwords;`
    pub max_dictwords: i32,
    /// `SwkbdCustomizedDictionarySet customizedDictionarySet;`
    pub customized_dictionary_set: SwkbdCustomizedDictionarySet,
    /// `u8 unkFlag;`
    pub unk_flag: u8,
    /// `u8 trigger;`
    pub trigger: u8,
    /// `u32 version;` - set by `swkbdCreate` / `_swkbdInitVersion`, never by us.
    pub version: u32,
}

// ---------------------------------------------------------------------------
// Layout verification
//
// Every expected value below is a *measurement*, not a calculation: a probe
// translation unit (docs/switch_swkbd.md, "measured values") instantiated one
// global per sizeof/offsetof, was compiled for aarch64-none-elf against the
// same headers build.ps1 links against, and the symbols were read back with
// nm --print-size.  If a future libnx changes any of them, this file stops
// compiling instead of corrupting memory.
// ---------------------------------------------------------------------------

const _: () = {
    use core::mem::{align_of, offset_of, size_of};

    // --- SwkbdArgCommon: sizeof 0x3D4, align 4 -----------------------------
    assert!(size_of::<SwkbdArgCommon>() == 980);
    assert!(align_of::<SwkbdArgCommon>() == 4);
    assert!(offset_of!(SwkbdArgCommon, type_) == 0x00);
    assert!(offset_of!(SwkbdArgCommon, key_set_disable_bitmask) == 0x1C);
    assert!(offset_of!(SwkbdArgCommon, initial_cursor_pos) == 0x20);
    assert!(offset_of!(SwkbdArgCommon, header_text) == 0x24);
    assert!(offset_of!(SwkbdArgCommon, sub_text) == 0xA6);
    assert!(offset_of!(SwkbdArgCommon, guide_text) == 0x1A8);
    assert!(offset_of!(SwkbdArgCommon, string_len_max) == 0x3AC);
    assert!(offset_of!(SwkbdArgCommon, string_len_min) == 0x3B0);
    assert!(offset_of!(SwkbdArgCommon, text_draw_type) == 0x3B8);
    assert!(offset_of!(SwkbdArgCommon, return_button_flag) == 0x3BC);
    assert!(offset_of!(SwkbdArgCommon, blur_background) == 0x3BE);
    assert!(offset_of!(SwkbdArgCommon, initial_string_offset) == 0x3C0);
    assert!(offset_of!(SwkbdArgCommon, initial_string_size) == 0x3C4);
    assert!(offset_of!(SwkbdArgCommon, text_check_flag) == 0x3D0);

    // --- SwkbdArgV0: sizeof 0x3E8, align 8 ---------------------------------
    assert!(size_of::<SwkbdArgV0>() == 1000);
    assert!(align_of::<SwkbdArgV0>() == 8);
    assert!(offset_of!(SwkbdArgV0, pad_x3d1) == 0x3D4);
    assert!(offset_of!(SwkbdArgV0, text_check_cb) == 0x3E0);

    // --- SwkbdArgV7: sizeof 0x408, align 8 ---------------------------------
    assert!(size_of::<SwkbdArgV7>() == 1032);
    assert!(align_of::<SwkbdArgV7>() == 8);
    assert!(offset_of!(SwkbdArgV7, text_grouping) == 0x3E8);

    // --- SwkbdCustomizedDictionarySet: sizeof 0xCE, align 1 (packed) -------
    assert!(size_of::<SwkbdCustomizedDictionarySet>() == 206);
    assert!(align_of::<SwkbdCustomizedDictionarySet>() == 1);

    // --- SwkbdConfig: sizeof 0x4F0, align 8 --------------------------------
    assert!(size_of::<SwkbdConfig>() == 1264);
    assert!(align_of::<SwkbdConfig>() == 8);
    assert!(offset_of!(SwkbdConfig, workbuf) == 0x408);
    assert!(offset_of!(SwkbdConfig, workbuf_size) == 0x410);
    assert!(offset_of!(SwkbdConfig, max_dictwords) == 0x418);
    assert!(offset_of!(SwkbdConfig, customized_dictionary_set) == 0x41C);
    assert!(offset_of!(SwkbdConfig, unk_flag) == 0x4EA);
    assert!(offset_of!(SwkbdConfig, trigger) == 0x4EB);
    assert!(offset_of!(SwkbdConfig, version) == 0x4EC);
};

impl SwkbdConfig {
    /// All-zero config.  `swkbdCreate` memsets the struct itself, but starting
    /// from zeroes keeps `workbuf` a NULL pointer even if `swkbdCreate` fails
    /// before it stored anything.
    fn zeroed() -> Self {
        // Safe: every field is an integer, an array of integers or a raw
        // pointer, all of which accept the all-zero bit pattern.
        unsafe { core::mem::zeroed() }
    }

    /// Equivalent of the `static inline` `swkbdConfigSetType` (`swkbd.h:469`).
    fn set_type(&mut self, type_: u32) {
        self.arg.arg.arg.type_ = type_;
    }

    /// Equivalent of the `static inline` `swkbdConfigSetStringLenMax`
    /// (`swkbd.h:505`).
    fn set_string_len_max(&mut self, len: u32) {
        self.arg.arg.arg.string_len_max = len;
    }
}

// ---------------------------------------------------------------------------
// libnx entry points
//
// Only the functions that are actually exported by libnx.a are declared here:
// the other `swkbdConfigSet*` helpers in the header are `static inline` and
// therefore have no symbol (their field writes are done through the mirrors
// above).  Verified with "aarch64-none-elf-nm libnx.a".
// ---------------------------------------------------------------------------

#[allow(non_snake_case)]
extern "C" {
    /// `applets/swkbd.h:349`.  Allocates `c->workbuf` (`max_dictwords` = 0 -> no
    /// user dictionary).  A failed Result leaves `c` all zeroes, so there is
    /// nothing to free in that case.
    fn swkbdCreate(c: *mut SwkbdConfig, max_dictwords: i32) -> Result;

    /// `applets/swkbd.h:355`.  Frees `c->workbuf` and zeroes `c`.
    fn swkbdClose(c: *mut SwkbdConfig);

    /// `applets/swkbd.h:363`.  Resets the args to the "Default" preset
    /// (QWERTY, cursor at end, Return enabled, background blurred).
    fn swkbdConfigMakePresetDefault(c: *mut SwkbdConfig);

    /// `applets/swkbd.h:416`.  `str` is UTF-8, NUL-terminated; NULL is a no-op.
    fn swkbdConfigSetHeaderText(c: *mut SwkbdConfig, str: *const c_char);

    /// `applets/swkbd.h:432`.  Only displayed while the cursor is at position 0.
    fn swkbdConfigSetGuideText(c: *mut SwkbdConfig, str: *const c_char);

    /// `applets/swkbd.h:439`.  The UTF-16 copy goes into `c->workbuf` at offset 20.
    fn swkbdConfigSetInitialText(c: *mut SwkbdConfig, str: *const c_char);

    /// `applets/swkbd.h:587`.  Runs the applet to completion and writes a UTF-8,
    /// NUL-terminated string into `out_string` (`out_string_size` counts the
    /// terminator; NULL/0 are rejected with `LibnxError_BadInput`).
    fn swkbdShow(c: *mut SwkbdConfig, out_string: *mut c_char, out_string_size: usize) -> Result;

    /// `services/applet.h:387`.  Reads `__nx_applet_type` set up by libnx'
    /// `appletInitialize`, so it is safe to call at any time.
    fn appletGetAppletType() -> i32;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// libnx' `R_SUCCEEDED(res)`.
const fn succeeded(res: Result) -> bool {
    res == 0
}

/// UTF-8 -> NUL-terminated bytes, truncated on a character boundary so that the
/// result never exceeds `max_units` UTF-16 code units.
///
/// Truncating *here* matters: libnx' `utf8_to_utf16` returns the number of code
/// units the input *would* produce (`runtime/util/utf.h:57`) and the swkbd config
/// setters then NUL-terminate at that index, so handing them an overlong string
/// would write past the fixed-size `headerText` / `guideText` arrays.  Counting
/// UTF-16 code units (not bytes and not `char`s) matches how swkbd itself
/// counts characters.
fn c_string(s: &str, max_units: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(s.len().min(max_units * 4) + 1);
    let mut units = 0usize;
    for ch in s.chars() {
        let n = ch.len_utf16();
        if units + n > max_units {
            break;
        }
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        units += n;
    }
    out.push(0); // libnx expects a NUL-terminated (and non-NULL) C string
    out
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Can the software keyboard be used at all?
///
/// The swkbd applet is a *library* applet, and only `AppletType_Application` /
/// `AppletType_SystemApplication` own the `ILibraryAppletCreator` needed to launch
/// one (`services/applet.h:1174`).  Homebrew started from the album runs as
/// `AppletType_LibraryApplet`, where `swkbdShow` cannot succeed; a homebrew menu
/// (hbmenu) started from there normally runs as an application, which is the
/// supported case.
///
/// Handy on its own as well, because `input_text` blocks the calling thread until
/// the applet exits: a caller that only needs the ASCII fallback can test this
/// first.
pub fn is_available() -> bool {
    matches!(
        unsafe { appletGetAppletType() },
        APPLET_TYPE_APPLICATION | APPLET_TYPE_SYSTEM_APPLICATION
    )
}

/// Show the system software keyboard and return the entered text.
///
/// * `header`  - text shown above the input field (truncated to 64 characters).
/// * `initial` - pre-filled text, cursor placed after it (truncated to the
///   requested length).
/// * `max_len` - maximum number of characters the user may enter, passed to
///   the applet as `stringLenMax`; 0 means "as many as supported".
///
/// Returns `Some(text)` (possibly empty - the user may confirm an empty string)
/// on success and `None` on cancel/error.  Japanese and any other UTF-8 input
/// works: swkbd uses UTF-16 internally and libnx converts both ways, so the
/// returned `String` is plain UTF-8.
///
/// This blocks until the applet is finished (the applet draws itself, no
/// `appletMainLoop` call is needed while it runs), so it must be called from the
/// main thread / the UI loop, not from an applet hook or a menu callback.
pub fn input_text(header: &str, initial: &str, max_len: usize) -> Option<String> {
    // A library applet cannot be launched outside application mode; bail out
    // before touching libnx' applet machinery.
    if !is_available() {
        return None;
    }

    // 0 -> "no explicit request": use the largest length that survives the
    // round-trip through swkbdShow.
    let chars = if max_len == 0 { MAX_TEXT_UNITS } else { max_len.min(MAX_TEXT_UNITS) };

    let hdr = c_string(header, MAX_HEADER_UNITS);
    let initial = c_string(initial, chars.min(MAX_INITIAL_UNITS));

    let mut cfg = SwkbdConfig::zeroed();

    // 1. Create (allocates cfg.workbuf, which swkbdClose has to release).
    let rc = unsafe { swkbdCreate(&mut cfg, 0) };
    if !succeeded(rc) {
        // Nothing was allocated and no applet was started -> nothing to close.
        return None;
    }

    // 2. Configure.  The preset clears textCheckCb, so libnx never calls back
    //    into us (a TextCheck callback would receive a pointer into the output
    //    buffer, and a String is built from it only after swkbdShow returns).
    unsafe { swkbdConfigMakePresetDefault(&mut cfg) };
    cfg.set_type(SWKBD_TYPE_NORMAL); // Japanese kana/flick input + globe key
    cfg.set_string_len_max(chars as u32);
    unsafe {
        swkbdConfigSetHeaderText(&mut cfg, hdr.as_ptr() as *const c_char);
        swkbdConfigSetGuideText(&mut cfg, c"".as_ptr());
        // Needs the work buffer from swkbdCreate, hence the order.
        swkbdConfigSetInitialText(&mut cfg, initial.as_ptr() as *const c_char);
    }

    // 3. Output buffer: worst case 4 UTF-8 bytes per UTF-16 code unit (BMP
    //    characters need 3, a surrogate pair needs 2 per unit), plus the
    //    terminator libnx writes.  libnx passes len-1 to utf16_to_utf8, so the
    //    buffer must be strictly larger than the encoded text.
    let mut out: Vec<u8> = Vec::with_capacity(chars * 4 + 16);
    out.resize(chars * 4 + 16, 0);

    // 4. Run the applet, then always release the config - there is no early
    //    return between these two calls, so the work buffer cannot leak.
    let rc = unsafe { swkbdShow(&mut cfg, out.as_mut_ptr() as *mut c_char, out.len()) };
    unsafe { swkbdClose(&mut cfg) };
    if !succeeded(rc) {
        return None;
    }

    // 5. libnx NUL-terminates the (UTF-8) result; cut there and hand it out.
    let end = out.iter().position(|&b| b == 0).unwrap_or(out.len());
    core::str::from_utf8(&out[..end]).ok().map(String::from)
}
