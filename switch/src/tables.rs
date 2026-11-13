//! The game's tables, read from the SD card.
//!
//! Same rule as the icon sheet: the repository ships no game files, so the
//! caller drops the tables it wants next to the .nro and they are read once.
//!
//! ```text
//! /switch/XXSE/table/itemData.bin           item icons, colours and rarity
//! /switch/XXSE/table/armorSeriesData.bin    armour rarity
//! /switch/XXSE/table/weapon00BaseData.bin   weapon rarity, one file per class
//! ...
//! ```
//!
//! Every file is optional. A missing item table costs the item box its icons; a
//! missing weapon table costs that class its rarity colour. Nothing stops
//! working, and `status()` says what was found so the panel can tell the user.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;

use mhnx_core::gametables::{ArmorTable, ItemTable, Tables, WeaponTable};

use crate::sys;

/// The folder the tables are read from.
pub const DIR: &str = "/switch/XXSE/table";

static mut TABLES: Option<Tables> = None;
static mut TRIED: bool = false;

fn to_c(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let p = to_c(path);
    let m = to_c("rb");
    let f = unsafe { sys::fopen(p.as_ptr() as *const c_char, m.as_ptr() as *const c_char) };
    if f.is_null() {
        return None;
    }
    let mut out: Vec<u8> = Vec::new();
    let mut buf = alloc::vec![0u8; 64 * 1024];
    loop {
        let n = unsafe { sys::fread(buf.as_mut_ptr() as *mut core::ffi::c_void, 1, buf.len(), f) };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > 4 * 1024 * 1024 {
            unsafe { sys::fclose(f) };
            return None;
        }
    }
    unsafe { sys::fclose(f) };
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn load() -> Tables {
    let mut tables = Tables::default();
    if let Some(bytes) = read_file(&alloc::format!("{}/itemData.bin", DIR)) {
        if let Some(t) = ItemTable::parse(&bytes) {
            tables.items = t;
        }
    }
    if let Some(bytes) = read_file(&alloc::format!("{}/armorSeriesData.bin", DIR)) {
        if let Some(t) = ArmorTable::parse(&bytes) {
            tables.armor = t;
            tables.armor_bytes = bytes;
        }
    }
    for class in 0u8..15 {
        let path = alloc::format!("{}/weapon{:02}BaseData.bin", DIR, class);
        if let Some(bytes) = read_file(&path) {
            if let Some(t) = WeaponTable::parse(&bytes) {
                tables.weapons[class as usize] = Some(t);
            }
        }
    }
    tables
}

/// The tables, read on first use.
///
/// Always returns something: an empty set answers "no rarity known" to every
/// question, which is what the screen draws when the card has no tables.
pub fn get() -> &'static Tables {
    unsafe {
        if !TRIED {
            TRIED = true;
            TABLES = Some(load());
        }
        if TABLES.is_none() {
            TABLES = Some(Tables::default());
        }
        TABLES.as_ref().unwrap_or_else(|| unreachable!())
    }
}

/// Forget what was read and look again.
pub fn reload() -> bool {
    unsafe {
        TABLES = Some(load());
        TRIED = true;
        TABLES.as_ref().map(|t| !t.is_empty()).unwrap_or(false)
    }
}

/// One line naming what was and was not found, for the panel to print.
pub fn status() -> String {
    let t = get();
    let weapons = t.weapons.iter().filter(|w| w.is_some()).count();
    alloc::format!(
        "アイテム {} / 防具 {} / 武器 {} 種別",
        if t.items.is_empty() { "なし" } else { "あり" },
        if t.armor.is_empty() { "なし" } else { "あり" },
        weapons
    )
}
