//! The player's palico roster.
//!
//! Eighty-four records of 324 bytes at slot + 0x23BB6. Each starts with a
//! UTF-8 name, then the experience at +0x20, a level byte at +0x24 that the
//! game shows as value + 1, and the enthusiasm byte at +0x26.
//!
//! The sample character has 17 of the 84 filled.

use alloc::string::String;
use alloc::vec::Vec;

use crate::save::Save;

/// Offset of the palico roster, relative to the slot base.
pub const OFF_ROSTER: usize = 0x23BB6;
/// Bytes per record.
pub const RECORD: usize = 324;
/// Records in the roster.
pub const SLOTS: usize = 84;
/// Name field, UTF-8, NUL padded.
pub const NAME: usize = 0x00;
/// Name bytes available.
pub const NAME_LEN: usize = 32;
/// Experience.
pub const EXP: usize = 0x20;
/// Level, shown as this plus one.
pub const LEVEL: usize = 0x24;
/// Enthusiasm (なつき度).
pub const ENTHUSIASM: usize = 0x26;

/// One palico.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palico {
    /// Record index in the roster.
    pub index: usize,
    /// Name as stored.
    pub name: String,
    /// Level as the game shows it.
    pub level: u16,
    /// Experience points.
    pub exp: u32,
    /// Enthusiasm.
    pub enthusiasm: u8,
}

impl Palico {
    /// A record with no name is an empty slot.
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }
}

/// Read the roster of one character.
pub fn roster(save: &Save, slot: usize) -> Vec<Palico> {
    let mut out = Vec::new();
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return out;
    };
    for index in 0..SLOTS {
        let at = base + OFF_ROSTER + index * RECORD;
        let Some(rec) = save.body().get(at..at + RECORD) else {
            break;
        };
        let raw = &rec[NAME..NAME + NAME_LEN];
        let end = raw.iter().position(|b| *b == 0).unwrap_or(NAME_LEN);
        let name = String::from_utf8_lossy(&raw[..end]).into_owned();
        out.push(Palico {
            index,
            name,
            level: rec[LEVEL] as u16 + 1,
            exp: u32::from_le_bytes([rec[EXP], rec[EXP + 1], rec[EXP + 2], rec[EXP + 3]]),
            enthusiasm: rec[ENTHUSIASM],
        });
    }
    out
}

/// The palicos that actually exist.
pub fn used(save: &Save, slot: usize) -> Vec<Palico> {
    roster(save, slot)
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect()
}

/// Rename a palico. The name field is fixed width and NUL padded.
pub fn set_name(save: &mut Save, slot: usize, index: usize, name: &str) -> bool {
    if index >= SLOTS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + NAME;
    let Some(dst) = save.body_mut().get_mut(at..at + NAME_LEN) else {
        return false;
    };
    dst.fill(0);
    let bytes = name.as_bytes();
    let n = bytes.len().min(NAME_LEN - 1);
    dst[..n].copy_from_slice(&bytes[..n]);
    true
}

/// Set a palico's experience.
pub fn set_exp(save: &mut Save, slot: usize, index: usize, exp: u32) -> bool {
    if index >= SLOTS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + EXP;
    let Some(dst) = save.body_mut().get_mut(at..at + 4) else {
        return false;
    };
    dst.copy_from_slice(&exp.to_le_bytes());
    true
}

/// Set a palico's level, stored as one less than the number shown.
pub fn set_level(save: &mut Save, slot: usize, index: usize, level: u16) -> bool {
    if index >= SLOTS || !(1..=99).contains(&level) {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + LEVEL;
    let Some(dst) = save.body_mut().get_mut(at) else {
        return false;
    };
    *dst = (level - 1) as u8;
    true
}

/// Set a palico's enthusiasm.
pub fn set_enthusiasm(save: &mut Save, slot: usize, index: usize, value: u8) -> bool {
    if index >= SLOTS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + ENTHUSIASM;
    let Some(dst) = save.body_mut().get_mut(at) else {
        return false;
    };
    *dst = value;
    true
}