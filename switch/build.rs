//! Turns the devkitPro installation into the link line that rustc hands to
//! devkitA64's gcc driver, and compiles the one C file of the crate.
//!
//! The paths cannot live in .cargo/config.toml because they depend on where
//! devkitPro is installed, so they are computed here (DEVKITPRO is provided by
//! .cargo/config.toml [env] as a default, or by build.ps1 / the user).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=tools/font_shim.c");
    println!("cargo::rerun-if-changed=tools/stb_truetype.h");
    println!("cargo::rerun-if-env-changed=DEVKITPRO");
    println!("cargo::rerun-if-env-changed=XXSE_UI_VARIANT");
    // switch_curl is set below when the portlib is installed; declaring it here
    // is what keeps the unexpected_cfgs lint quiet about it.
    println!("cargo::rustc-check-cfg=cfg(switch_curl)");

    let devkitpro = env::var("DEVKITPRO").unwrap_or_else(|_| "C:/devkitPro".to_string());
    let devkitpro = devkitpro.replace('\\', "/");
    let libnx = format!("{devkitpro}/libnx");
    let portlibs = format!("{devkitpro}/portlibs/switch");

    // tools/font_shim.c + tools/stb_truetype.h -> $OUT_DIR/libfont_shim.a.
    // Built for the same CPU as the Rust code below (see build_font_shim), so
    // it can simply join the link line.
    let font_shim = build_font_shim(&devkitpro);

    // The update check is the only part of this crate that needs a portlib, and
    // a machine without switch-curl must still be able to build the editor: the
    // socket compiles out and the banner simply never appears. build.ps1 prints
    // the same warning with the command that fixes it.
    // The archive appears (or disappears) when the portlib is installed or
    // removed, and that is exactly when this decision has to be made again:
    // without these two lines cargo keeps the previous answer and the link
    // fails on a -lcurl that is no longer there.
    // The directory is watched as well as the two archives: pacman installs and
    // removes files, which moves the directory's own timestamp even when the
    // archive is put back with the one it had.
    println!("cargo::rerun-if-changed={portlibs}/lib");
    println!("cargo::rerun-if-changed={portlibs}/lib/libcurl.a");
    println!("cargo::rerun-if-changed={portlibs}/lib/libz.a");

    let curl = Path::new(&format!("{portlibs}/lib/libcurl.a")).is_file()
        && Path::new(&format!("{portlibs}/lib/libz.a")).is_file();
    if curl {
        println!("cargo::rustc-cfg=switch_curl");
    } else {
        println!(
            "cargo::warning=switch-curl is not installed, so the update check is compiled out. \
             Install it with: C:/devkitPro/msys2/usr/bin/pacman.exe -S switch-curl"
        );
    }

    // Mirrors $(LDFLAGS) / $(LIBS) / $(LIBPATHS) of the devkitPro application
    // template Makefile one flag at a time.
    let mut args: Vec<String> = vec![
        // switch.specs injects: -T .../libnx/switch.ld, -pie, --no-dynamic-linker,
        // --gc-sections, -z text -z now -z nodynamic-undefined-weak,
        // -z pack-relative-relocs, --build-id=sha1, --nx-module-name and the
        // crti/crtbegin start files.
        format!("-specs={libnx}/switch.specs"),
        "-march=armv8-a+crc+crypto".to_string(),
        "-mtune=cortex-a57".to_string(),
        "-mtp=soft".to_string(),
        "-fPIE".to_string(),
        format!("-L{libnx}/lib"),
        format!("-L{portlibs}/lib"),
        // libnx, exactly like the Makefile template.
        "-lnx".to_string(),
    ];
    if curl {
        // switch-curl: the update check's only portlib. It is a static archive
        // that references libnx (sslCreateContext, socket*, svc*) and zlib, so
        // both are named again below inside the --start-group where the linker
        // rescan can resolve them.
        args.push("-lcurl".to_string());
    }
    // font_shim.c -> libfont_shim.a.  It sits after -lnx and before the
    // --start-group below so its newlib/libm references (malloc, memcpy,
    // sqrt, ...) are resolved by the group; it does not reference libnx.
    args.push(font_shim);
    // rustc passes -nodefaultlibs (this target has no C runtime "fallback"
    // crt objects), which also removes the libraries the gcc driver would
    // normally append.  These are the ones devkitA64 needs - taken
    // verbatim from a native link:
    //   collect2 ... -lnx --start-group -lgcc -lg -lc -lsysbase --end-group
    // -lm is part of that reference set but not of the template (a pure C
    // homebrew never needs it); tools/font_shim.c does, because
    // stb_truetype calls sqrt/pow/cos/acos/fmod in its SDF/GPOS code.
    // It is added *inside* the group so it is rescanned like libc.
    // (gcc still appends its own crtend.o/crtn.o afterwards.)
    args.push("-Wl,--start-group".to_string());
    if curl {
        args.push("-lz".to_string());
        args.push("-lnx".to_string());
    }
    for flag in ["-lgcc", "-lg", "-lc", "-lm", "-lsysbase"] {
        args.push(flag.to_string());
    }
    args.push("-Wl,--end-group".to_string());

    for a in args {
        println!("cargo::rustc-link-arg={a}");
    }
}

/// devkitPro tool, with the .exe suffix that the Windows installer uses.
fn dkp_tool(devkitpro: &str, name: &str) -> PathBuf {
    let dir = Path::new(devkitpro).join("devkitA64").join("bin");
    let with_exe = dir.join(format!("{name}.exe"));
    if with_exe.is_file() {
        with_exe
    } else {
        dir.join(name)
    }
}

/// Compiles tools/font_shim.c (which #includes tools/stb_truetype.h) into
/// $OUT_DIR/libfont_shim.a and returns the archive path as a link argument.
fn build_font_shim(devkitpro: &str) -> String {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let tools = manifest.join("tools");
    let src = tools.join("font_shim.c");
    let obj = out_dir.join("font_shim.o");
    let archive = out_dir.join("libfont_shim.a");

    if !src.is_file() {
        panic!("missing C shim: {}", src.display());
    }

    let gcc = dkp_tool(devkitpro, "aarch64-none-elf-gcc");
    if !gcc.is_file() {
        panic!(
            "devkitA64 gcc not found at {} (set DEVKITPRO to the devkitPro root)",
            gcc.display()
        );
    }

    // Same CPU/ABI flags as the Rust side.  -ffunction-sections lets
    // --gc-sections drop the stb_truetype code this application never calls
    // (SDF rasterisation and friends), which is also what keeps libm small.
    let status = Command::new(&gcc)
        .arg("-c")
        .args(["-O2", "-g0", "-fno-strict-aliasing"])
        .args(["-march=armv8-a+crc+crypto", "-mtune=cortex-a57", "-mtp=soft"])
        .args(["-fPIE", "-ffunction-sections", "-fdata-sections"])
        .arg("-I")
        .arg(&tools)
        .arg("-o")
        .arg(&obj)
        .arg(&src)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", gcc.display()));
    if !status.success() {
        panic!("compiling {} failed ({status})", src.display());
    }

    let ar = dkp_tool(devkitpro, "aarch64-none-elf-ar");
    if !ar.is_file() {
        panic!("devkitA64 ar not found at {}", ar.display());
    }
    let _ = std::fs::remove_file(&archive);
    let status = Command::new(&ar)
        .current_dir(&out_dir)
        .args(["rcs", "libfont_shim.a", "font_shim.o"])
        .status()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", ar.display()));
    if !status.success() {
        panic!("archiving {} failed ({status})", archive.display());
    }

    archive.display().to_string()
}