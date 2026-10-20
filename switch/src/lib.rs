//! `mhnx_switch` - Switch (libnx) support crate for MHNX Editor.
//!
//! The crate is written as `no_std` + `alloc` so the exact same sources can be
//! compiled either as the standalone `.nro` binary (`src/main.rs`) or as a
//! staticlib that is linked by a devkitA64 Makefile / gcc command line.
//!
//! The two global lang items (panic handler, global allocator) live here so
//! that *both* the binary and the staticlib are self-contained; the binary
//! crate root therefore only defines libnx' `main`.

#![no_std]
#![allow(dead_code)]

extern crate alloc;

pub mod app;
pub mod editor;
pub mod savedata;
pub mod swkbd;
pub mod sys;
pub mod text;
pub mod ui;

/// Backed by devkitA64's newlib heap, which is what libnx itself uses.
struct LibcAlloc;

unsafe impl core::alloc::GlobalAlloc for LibcAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        sys::malloc(layout.size()) as *mut u8
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: core::alloc::Layout) {
        sys::free(ptr as *mut core::ffi::c_void)
    }
}

#[global_allocator]
static GLOBAL_ALLOC: LibcAlloc = LibcAlloc;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    app::print_panic(info)
}

/// Entry point for the staticlib route: a C `main()` can simply `return
/// mhnx_main();` instead of re-implementing the program.
#[no_mangle]
pub extern "C" fn mhnx_main() -> i32 {
    app::run(0, core::ptr::null())
}