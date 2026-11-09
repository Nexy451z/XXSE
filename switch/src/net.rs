//! Raw FFI for the two libraries the update check needs: libnx' bsd:u socket
//! driver and the devkitPro portlib switch-curl (libcurl 7.69.1).
//!
//! Nothing here is a wrapper with policy: the declarations are transcribed from
//! curl/curl.h and switch/runtime/devices/socket.h, and the constants are the
//! enum values from the same headers, written out so that this file can be read
//! against them. The policy (timeouts, what to do with an answer) lives in
//! crate::update.
//!
//! ## TLS
//!
//! switch-curl is built --with-libnx --with-default-ssl-backend=libnx (see
//! devkitPro's switch/curl PKGBUILD). Its TLS is the console's own ssl service
//! (libnx sslCreateContext / sslConnectionDoHandshake), *not* a PEM file, and
//! the verify mask the service starts with is SslVerifyOption_PeerCa |
//! SslVerifyOption_HostName - curl's CURLOPT_SSL_VERIFYPEER / VERIFYHOST are
//! mapped straight onto those bits. So the chain is checked against the
//! console's built-in CA store and the host name is checked, with no cacert.pem
//! on the card and no CURLOPT_CAINFO: a CA file would be read by nothing.
//! This module sets neither VERIFYPEER nor VERIFYHOST, so both keep curl's
//! defaults (1 and 2), i.e. verification is ON.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_long, c_void};

// ---------------------------------------------------------------------------
// libnx: runtime/devices/socket.h
// ---------------------------------------------------------------------------

/// SocketInitConfig (runtime/devices/socket.h). Only ever passed as NULL, which
/// selects libnx' default configuration, so the fields are not modelled: the
/// pointer stays opaque here.
#[repr(C)]
pub struct SocketInitConfig {
    _opaque: [u8; 0],
}

extern "C" {
    /// The real entry point. socketInitializeDefault() in the header is a
    /// static inline wrapper around this with NULL, so it has no symbol of its
    /// own and cannot be called from Rust.
    pub fn socketInitialize(config: *const SocketInitConfig) -> u32;
    pub fn socketExit();
}

/// The header's socketInitializeDefault().
///
/// BsdServiceType_Auto (the default): bsd:s first, bsd:u if that fails, which
/// is what official software does. Sessions are per-thread; the update worker
/// initialises its own and exits it when it is done.
pub fn socket_initialize_default() -> u32 {
    unsafe { socketInitialize(core::ptr::null()) }
}

// ---------------------------------------------------------------------------
// libcurl: curl/curl.h
// ---------------------------------------------------------------------------

/// CURLcode. 0 is CURLE_OK.
pub type CURLcode = c_int;
/// Opaque CURL handle.
pub type CURL = c_void;
/// Opaque curl_slist (the header list).
#[repr(C)]
pub struct curl_slist {
    pub data: *mut c_char,
    pub next: *mut curl_slist,
}

/// CURLE_OK.
pub const CURLE_OK: CURLcode = 0;
/// The window the state machine reports on: a timeout, a refused connection and
/// a name that does not resolve are three different sentences.
pub const CURLE_COULDNT_RESOLVE_HOST: CURLcode = 6;
pub const CURLE_COULDNT_CONNECT: CURLcode = 7;
pub const CURLE_WRITE_ERROR: CURLcode = 23;
pub const CURLE_OPERATION_TIMEDOUT: CURLcode = 28;
pub const CURLE_SSL_CONNECT_ERROR: CURLcode = 35;
pub const CURLE_ABORTED_BY_CALLBACK: CURLcode = 42;
pub const CURLE_SSL_CACERT: CURLcode = 60;

/// CURL_GLOBAL_DEFAULT.
pub const CURL_GLOBAL_DEFAULT: c_long = 3;

// The option values, as CURLOPT(<name>, CURLOPTTYPE_x, n) expands in 7.69.1:
// LONG = 0, OBJECTPOINT = STRINGPOINT = SLISTPOINT = 10000,
// FUNCTIONPOINT = 20000.
pub const CURLOPT_WRITEDATA: c_int = 10001;
pub const CURLOPT_URL: c_int = 10002;
pub const CURLOPT_WRITEFUNCTION: c_int = 20011;
pub const CURLOPT_USERAGENT: c_int = 10018;
pub const CURLOPT_HTTPHEADER: c_int = 10023;
pub const CURLOPT_ACCEPT_ENCODING: c_int = 10102;
pub const CURLOPT_NOSIGNAL: c_int = 99;
pub const CURLOPT_NOPROGRESS: c_int = 43;
pub const CURLOPT_FAILONERROR: c_int = 45;
pub const CURLOPT_FOLLOWLOCATION: c_int = 52;
pub const CURLOPT_MAXREDIRS: c_int = 68;
pub const CURLOPT_CONNECTTIMEOUT_MS: c_int = 156;
pub const CURLOPT_TIMEOUT_MS: c_int = 155;
pub const CURLOPT_XFERINFOFUNCTION: c_int = 20219;
/// CURLOPT_XFERINFODATA (an alias of CURLOPT_PROGRESSDATA in curl's header).
pub const CURLOPT_XFERINFODATA: c_int = 10057;
/// Kept as a named constant even though this build ignores it (see the module
/// comment): a reader looking for what the updater does with a CA file should
/// find the answer here rather than find nothing.
pub const CURLOPT_CAINFO: c_int = 10065;

/// CURLINFO_RESPONSE_CODE (CURLINFO_LONG + 2).
pub const CURLINFO_RESPONSE_CODE: c_int = 0x200002;

/// curl's write callback. Returning anything but the byte count aborts.
pub type WriteFn = unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize;
/// curl's progress callback. Returning non-zero aborts the transfer, which is
/// how the download's キャンセル button stops it.
pub type XferInfoFn =
    unsafe extern "C" fn(*mut c_void, i64, i64, i64, i64) -> c_int;

extern "C" {
    pub fn curl_global_init(flags: c_long) -> CURLcode;
    pub fn curl_global_cleanup();
    pub fn curl_easy_init() -> *mut CURL;
    pub fn curl_easy_cleanup(curl: *mut CURL);
    pub fn curl_easy_setopt(curl: *mut CURL, option: c_int, ...) -> CURLcode;
    pub fn curl_easy_perform(curl: *mut CURL) -> CURLcode;
    pub fn curl_easy_getinfo(curl: *mut CURL, info: c_int, ...) -> CURLcode;
    pub fn curl_easy_strerror(code: CURLcode) -> *const c_char;
    pub fn curl_version() -> *const c_char;

    pub fn curl_slist_append(list: *mut curl_slist, s: *const c_char) -> *mut curl_slist;
    pub fn curl_slist_free_all(list: *mut curl_slist);
}

/// One sentence per failure the user can actually do something about.
pub fn describe(code: CURLcode) -> &'static str {
    match code {
        CURLE_OK => "成功",
        CURLE_COULDNT_RESOLVE_HOST => "名前が引けません（DNS / ネット未接続）",
        CURLE_COULDNT_CONNECT => "接続できません（ネット未接続）",
        CURLE_WRITE_ERROR => "書き込みに失敗しました（空き容量・カード）",
        CURLE_OPERATION_TIMEDOUT => "時間切れ（3秒）",
        CURLE_SSL_CONNECT_ERROR => "TLS の接続に失敗しました（時計・本体の状態）",
        CURLE_SSL_CACERT => "証明書を確認できませんでした",
        CURLE_ABORTED_BY_CALLBACK => "中止しました",
        _ => "通信に失敗しました",
    }
}
