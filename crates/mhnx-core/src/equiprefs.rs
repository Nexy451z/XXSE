//! Which parts of the save point at an equipment record.
//!
//! The box is addressed by index from two places: the seven items currently
//! worn, and forty saved sets of seven. Clearing or replacing a record without
//! looking here leaves a dangling index behind, so this is the check that has
//! to pass before either is allowed.
//!
//! Both offsets were confirmed against the sample save by reading the indices
//! out and checking that each one lands on a record of the part it names: the
//! head index points at kind 1, the body at kind 2, and so on.

use alloc::vec::Vec;

use crate::save::Save;

/// The seven worn items, as u16 box indices. Relative to the slot base.
pub const WORN_OFFSET: usize = 0x23B39;
/// Parts the worn list names, in the order the game stores them.
pub const WORN_PARTS: usize = 7;
/// A slot holding nothing.
pub const NONE: u16 = 0xFFFF;

/// One place that names an equipment record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipRef {
    /// Currently worn, by part index 0..7 (weapon, head, body, arms, waist,
    /// legs, talisman).
    Worn {
        /// Part index.
        part: usize,
    },
    /// Named by a saved equipment set.
    Set {
        /// Set index 0..40.
        set: usize,
        /// Part index inside the set.
        part: usize,
    },
}

impl EquipRef {
    /// Short label for the user.
    pub fn text(self) -> alloc::string::String {
        use alloc::format;
        const PARTS: [&str; WORN_PARTS] = ["武器", "頭", "胴", "腕", "腰", "脚", "護石"];
        match self {
            EquipRef::Worn { part } => {
                format!("装備中（{}）", PARTS.get(part).copied().unwrap_or("?"))
            }
            EquipRef::Set { set, part } => format!(
                "マイセット {}（{}）",
                set + 1,
                PARTS.get(part).copied().unwrap_or("?")
            ),
        }
    }
}

fn read_u16(body: &[u8], at: usize) -> Option<u16> {
    let b = body.get(at..at + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// Everything that names `box_index` in this character's slot.
pub fn references(save: &Save, slot: usize, box_index: usize) -> Vec<EquipRef> {
    let mut out = Vec::new();
    if box_index > u16::MAX as usize {
        return out;
    }
    let want = box_index as u16;
    let Some(base) = save.slot_base(slot) else {
        return out;
    };
    let base = base as usize;
    let body = save.body();

    for part in 0..WORN_PARTS {
        let at = base + WORN_OFFSET + part * 2;
        if read_u16(body, at) == Some(want) {
            out.push(EquipRef::Worn { part });
        }
    }

    for set in 0..crate::myset::EQUIP_SETS {
        let set_at = base
            + crate::myset::OFF_EQUIP_SETS
            + set * crate::myset::EQUIP_SET_SIZE
            + crate::myset::EQUIP_SET_BODY;
        for part in 0..crate::myset::EQUIP_SET_PARTS {
            if read_u16(body, set_at + part * 2) == Some(want) {
                out.push(EquipRef::Set { set, part });
            }
        }
    }
    out
}

/// True when nothing points at this record and it is safe to clear.
pub fn is_unreferenced(save: &Save, slot: usize, box_index: usize) -> bool {
    references(save, slot, box_index).is_empty()
}