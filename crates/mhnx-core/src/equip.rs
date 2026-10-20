//! Equipment box records: decoding and Japanese names.
//!
//! A record is 36 bytes. The reference editor reads it like this
//! (MainForm.cs:336): the first two bytes hold the type and the level packed
//! into 16 bits, little endian, with the type in the top five bits and the
//! level in the low eleven. The id is a u16 at +0x02 and the transmog id a u16
//! at +0x04.
//!
//! Types come from EQUIPMENT_TYPES. 1..5 are the armour parts, 6 is the
//! talisman, and the weapons run 7..21 with 12 deliberately unused: the JP
//! weapon tables have their own numbering where 5 is the gap, and
//! type_id + 7 maps one to the other exactly.

use crate::charm::CHARM_RECORD;

/// Bytes per equipment record.
pub const EQUIP_RECORD: usize = CHARM_RECORD;
/// Records in the box.
pub const EQUIP_COUNT: usize = 2000;

/// One decoded equipment slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Equipment {
    /// Slot index in the box.
    pub index: usize,
    /// EQUIPMENT_TYPES index.
    pub kind: u8,
    /// Level, low 11 bits of the first word.
    pub level: u16,
    /// Weapon or armour id.
    pub id: u16,
    /// Transmog (layered) id.
    pub transmog: u16,
}

impl Equipment {
    /// Is the slot empty?
    pub fn is_empty(&self) -> bool {
        self.kind == 0
    }
}

/// Decode one 36 byte record.
pub fn decode_record(index: usize, rec: &[u8]) -> Option<Equipment> {
    if rec.len() < EQUIP_RECORD {
        return None;
    }
    // The reference builds a 16 bit value from the first two bytes and reads
    // the type from its low five bits and the level from the rest. That is the
    // same field the talisman reader uses, so both agree.
    let head = u16::from_le_bytes([rec[0], rec[1]]);
    Some(Equipment {
        index,
        kind: (head & 0x1F) as u8,
        level: head >> 5,
        id: u16::from_le_bytes([rec[2], rec[3]]),
        transmog: u16::from_le_bytes([rec[4], rec[5]]),
    })
}

/// Name of the equipment type, in Japanese.
pub fn type_name(kind: u8) -> &'static str {
    match kind {
        0 => "空き",
        1 => "頭",
        2 => "胴",
        3 => "腕",
        4 => "腰",
        5 => "脚",
        6 => "護石",
        7 => "大剣",
        8 => "片手剣",
        9 => "ハンマー",
        10 => "ランス",
        11 => "ヘビィボウガン",
        12 => "(未使用)",
        13 => "ライトボウガン",
        14 => "太刀",
        15 => "スラッシュアックス",
        16 => "ガンランス",
        17 => "弓",
        18 => "双剣",
        19 => "狩猟笛",
        20 => "操虫棍",
        21 => "チャージアックス",
        _ => "?",
    }
}

/// The Japanese name for a decoded slot.
#[cfg(feature = "gamedata")]
pub fn name(eq: &Equipment) -> &'static str {
    use crate::equipnames::{ARMOR_NAMES, WEAPON_NAMES};
    match eq.kind {
        1..=5 => {
            let part = (eq.kind - 1) as usize;
            ARMOR_NAMES
                .get(eq.id as usize * 5 + part)
                .copied()
                .unwrap_or("-")
        }
        7..=21 => {
            let t = match eq.kind as usize - 7 {
                5 => return "(未使用)",
                n => n,
            };
            WEAPON_NAMES
                .get(t)
                .and_then(|tbl| tbl.get(eq.id as usize))
                .copied()
                .unwrap_or("-")
        }
        6 => crate::gamedata::EQUIP_TALISMAN_NAMES
            .get(eq.id as usize)
            .copied()
            .unwrap_or("-"),
        _ => "-",
    }
}

/// The Japanese name for a decoded slot, when names are not available.
#[cfg(not(feature = "gamedata"))]
pub fn name(_eq: &Equipment) -> &'static str {
    "-"
}

/// Decode every slot of an equipment box.
pub fn decode_box(box_bytes: &[u8]) -> Vec<Equipment> {
    let mut out = Vec::new();
    for i in 0..EQUIP_COUNT {
        let off = i * EQUIP_RECORD;
        let Some(rec) = box_bytes.get(off..off + EQUIP_RECORD) else {
            break;
        };
        if let Some(eq) = decode_record(i, rec) {
            out.push(eq);
        }
    }
    out
}