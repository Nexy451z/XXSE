//! mhnxeditor - Nintendo Switch (.nro) entry point.
//!
//! `#![no_main]`: libnx' crt0 (switch_crt0.o inside libnx.a, pulled in because
//! switch.ld sets ENTRY(_start)) provides `_start` and calls
//! `main(argc, argv)` - which is exactly the `#[no_mangle] extern "C" fn main`
//! below.  The panic handler and the global allocator come from the
//! `mhnx_switch` library crate.

#![no_std]
#![no_main]
#![allow(dead_code)]

use core::ffi::{c_char, c_int};

#[no_mangle]
pub extern "C" fn main(argc: c_int, argv: *const *const c_char) -> c_int {
    mhnx_switch::app::run(argc, argv)
}
