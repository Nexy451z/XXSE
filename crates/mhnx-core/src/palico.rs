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
/// Enthusiasm, which the game calls tension.
pub const ENTHUSIASM: usize = 0x26;
/// Support tendency, eight kinds.
pub const SYSTEM: usize = 0x25;
/// Attack tendency.
pub const ATTACK: usize = 0x27;
/// Equipped support moves, eight slots.
pub const MOVES: usize = 0x28;
/// Equipped skills, eight slots.
pub const SKILLS: usize = 0x30;
/// Learned support moves, sixteen slots.
pub const LEARNED_MOVES: usize = 0x38;
/// Learned skills, twelve slots.
pub const LEARNED_SKILLS: usize = 0x48;
/// Empty entry in a learned move list.
pub const MOVE_EMPTY: u8 = 57;
/// Empty entry in a learned skill list, one past the skill table.
pub const SKILL_EMPTY: u8 = 96;
/// Equipment indices, weapon, head and body.
pub const EQUIP: usize = 0x100;
/// Equipment slots.
pub const EQUIP_SLOTS: usize = 3;

/// The eight support tendencies, in the order the game stores them.
pub const SYSTEMS: [&str; 8] = [
    "カリスマ",
    "ファイト",
    "ガード",
    "アシスト",
    "ヒーリング",
    "ボマー",
    "コレクト",
    "ビースト",
];

/// Name of a support tendency.
pub fn system_name(id: u8) -> &'static str {
    SYSTEMS.get(id as usize).copied().unwrap_or("?")
}

/// Name of an otomon skill, or a placeholder.
pub fn skill_name(id: u8) -> alloc::string::String {
    #[cfg(feature = "gamedata")]
    {
        crate::gamedata::PALICO_SKILL_NAMES
            .get(id as usize)
            .map(|s| alloc::string::ToString::to_string(*s))
            .unwrap_or_else(|| alloc::format!("#{}", id))
    }
    #[cfg(not(feature = "gamedata"))]
    {
        #[cfg(feature = "std")]
        {
            alloc::format!("#{}", id)
        }
        #[cfg(not(feature = "std"))]
        {
            alloc::format!("#{}", id)
        }
    }
}

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
    /// Support tendency.
    pub system: u8,
    /// Attack tendency.
    pub attack: u8,
    /// Equipped support moves.
    pub moves: Vec<u8>,
    /// Equipped skills.
    pub skills: Vec<u8>,
    /// Learned support moves.
    pub learned_moves: Vec<u8>,
    /// Learned skills.
    pub learned_skills: Vec<u8>,
    /// Equipment box indices, 0xFFFF when nothing is equipped.
    pub equip: [u16; EQUIP_SLOTS],
}

impl Palico {
    /// A record with no name is an empty slot.
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }
    /// Support tendency as an id.
    pub fn system(&self) -> u8 {
        self.system
    }

    /// Support tendency as a name.
    pub fn system_name(&self) -> &'static str {
        system_name(self.system)
    }

    /// Equipped skills, empty slots omitted.
    pub fn skills(&self) -> Vec<u8> {
        self.skills
            .iter()
            .copied()
            .filter(|v| *v != 0 && *v != SKILL_EMPTY)
            .collect()
    }

    /// Learned skills, empty slots omitted.
    pub fn learned_skills(&self) -> Vec<u8> {
        self.learned_skills
            .iter()
            .copied()
            .filter(|v| *v != 0 && *v != SKILL_EMPTY)
            .collect()
    }

    /// Equipped support moves, empty slots omitted.
    pub fn moves(&self) -> Vec<u8> {
        self.moves
            .iter()
            .copied()
            .filter(|v| *v != 0 && *v != MOVE_EMPTY)
            .collect()
    }

    /// Names of the equipped skills.
    pub fn skill_names(&self) -> Vec<String> {
        self.skills().into_iter().map(skill_name).collect()
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
            system: rec[SYSTEM],
            attack: rec[ATTACK],
            moves: rec[MOVES..MOVES + 8].to_vec(),
            skills: rec[SKILLS..SKILLS + 8].to_vec(),
            learned_moves: rec[LEARNED_MOVES..LEARNED_MOVES + 16].to_vec(),
            learned_skills: rec[LEARNED_SKILLS..LEARNED_SKILLS + 12].to_vec(),
            equip: [
                u16::from_le_bytes([rec[EQUIP], rec[EQUIP + 1]]),
                u16::from_le_bytes([rec[EQUIP + 2], rec[EQUIP + 3]]),
                u16::from_le_bytes([rec[EQUIP + 4], rec[EQUIP + 5]]),
            ],
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