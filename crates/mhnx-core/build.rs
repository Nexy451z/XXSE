//! Keeps the official DLC bundle optional.
//!
//! The repository ships no game data. When the generated files are present
//! (see tools/gen_official_dlc.py) this switches the crate to them; otherwise
//! the empty stub is compiled and the tool simply has no built-in quest list.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/dlc_bundle_gen.rs");
    println!("cargo:rerun-if-changed=src/official_dlc.bin");

    let generated = Path::new("src/dlc_bundle_gen.rs").exists();
    let blob = Path::new("src/official_dlc.bin").exists();

    // The blob has to exist because the generated module includes it from
    // OUT_DIR. A copy is always placed there so the include never fails.
    let out = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let dest = Path::new(&out).join("official_dlc.bin");
    if blob {
        std::fs::copy("src/official_dlc.bin", &dest).expect("copy the bundle into OUT_DIR");
    } else {
        std::fs::write(&dest, b"").expect("write an empty bundle into OUT_DIR");
    }

    if generated && blob {
        println!("cargo:rustc-cfg=official_bundle");
    } else {
        println!("cargo:warning=official DLC bundle not generated: building without it");
        println!("cargo:warning=run: python tools/gen_official_dlc.py (needs your own game files)");
    }
}
