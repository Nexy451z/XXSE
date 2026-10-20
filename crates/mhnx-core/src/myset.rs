//! Item sets and equipment sets.
//!
//! Two unrelated arrays. Item sets live at slot + 0x17CF, twenty-four records
//! of 170 bytes: a 32 byte name, ten reserved bytes, then thirty-two pairs of
//! a u16 item id and a u16 count. 32 is the pouch size.
//!
//! Equipment sets live at slot + 0x208CE, forty records of 136 bytes: a 32 byte
//! name, ten reserved bytes, seven u16 indices into the equipment box (weapon,
//! head, body, arms, waist, legs, talisman), seven decoration triples, five
//! armour colours and sixteen bytes not yet understood.
//!
//! An unused record has the name '－－－' and an empty body.

use alloc::string::String;
use alloc::vec::Vec;

use crate::save::Save;

/// Offset of the item set array, relative to the slot base.
pub const OFF_ITEM_SETS: usize = 0x17CF;
/// Item set records.
pub const ITEM_SETS: usize = 24;
/// Bytes per item set.
pub const ITEM_SET_SIZE: usize = 170;
/// Items a set holds.
pub const ITEM_SET_ITEMS: usize = 32;
/// Offset of the item pairs.
pub const ITEM_SET_BODY: usize = 0x2A;

/// Offset of the equipment set array.
pub const OFF_EQUIP_SETS: usize = 0x208CE;
/// Equipment set records.
pub const EQUIP_SETS: usize = 40;
/// Bytes per equipment set.
pub const EQUIP_SET_SIZE: usize = 136;
/// Parts an equipment set names.
pub const EQUIP_SET_PARTS: usize = 7;
/// Offset of the part indices.
pub const EQUIP_SET_BODY: usize = 0x2A;
/// Offset of the armour colours.
pub const EQUIP_SET_COLOURS: usize = 0x64;
/// A slot holding nothing.
pub const NONE: u16 = 0xFFFF;
/// The name an unused record carries.
pub const EMPTY_NAME: &str = "－－－";

/// One entry of an item set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetItem {
    /// Item id, 0 when the entry is unused.
    pub id: u16,
    /// How many to carry.
    pub count: u16,
}

/// An item set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSet {
    /// Index in the array.
    pub index: usize,
    /// Name as stored.
    pub name: String,
    /// The thirty-two entries.
    pub items: Vec<SetItem>,
}

impl ItemSet {
    /// Is this slot unused?
    pub fn is_empty(&self) -> bool {
        self.name == EMPTY_NAME || self.name.is_empty()
            || self.items.iter().all(|i| i.id == 0)
    }

    /// The entries that hold something.
    pub fn used_items(&self) -> Vec<SetItem> {
        self.items.iter().copied().filter(|i| i.id != 0).collect()
    }
}

/// An equipment set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipSet {
    /// Index in the array.
    pub index: usize,
    /// Name as stored.
    pub name: String,
    /// Weapon, head, body, arms, waist, legs and talisman, as equipment box
    /// indices. 0xFFFF means the part holds nothing.
    pub parts: [u16; EQUIP_SET_PARTS],
    /// Five armour colours.
    pub colours: [[u8; 4]; 5],
}

impl EquipSet {
    /// Is this slot unused?
    pub fn is_empty(&self) -> bool {
        self.name == EMPTY_NAME || self.parts.iter().all(|p| *p == NONE)
    }
}

fn read_name(b: &[u8]) -> String {
    let raw = &b[..32.min(b.len())];
    let end = raw.iter().position(|x| *x == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

fn write_name(dst: &mut [u8], name: &str) {
    crate::write_name_field(dst, name);
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

/// Read the item sets of one character.
pub fn item_sets(save: &Save, slot: usize) -> Vec<ItemSet> {
    let mut out = Vec::new();
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return out;
    };
    for index in 0..ITEM_SETS {
        let at = base + OFF_ITEM_SETS + index * ITEM_SET_SIZE;
        let Some(rec) = save.body().get(at..at + ITEM_SET_SIZE) else {
            break;
        };
        let mut items = Vec::with_capacity(ITEM_SET_ITEMS);
        for i in 0..ITEM_SET_ITEMS {
            let o = ITEM_SET_BODY + i * 4;
            items.push(SetItem {
                id: u16_at(rec, o),
                count: u16_at(rec, o + 2),
            });
        }
        out.push(ItemSet {
            index,
            name: read_name(rec),
            items,
        });
    }
    out
}

/// Read the equipment sets of one character.
pub fn equip_sets(save: &Save, slot: usize) -> Vec<EquipSet> {
    let mut out = Vec::new();
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return out;
    };
    for index in 0..EQUIP_SETS {
        let at = base + OFF_EQUIP_SETS + index * EQUIP_SET_SIZE;
        let Some(rec) = save.body().get(at..at + EQUIP_SET_SIZE) else {
            break;
        };
        let mut parts = [NONE; EQUIP_SET_PARTS];
        for (i, p) in parts.iter_mut().enumerate() {
            *p = u16_at(rec, EQUIP_SET_BODY + i * 2);
        }
        let mut colours = [[0u8; 4]; 5];
        for (i, c) in colours.iter_mut().enumerate() {
            let o = EQUIP_SET_COLOURS + i * 4;
            c.copy_from_slice(&rec[o..o + 4]);
        }
        out.push(EquipSet {
            index,
            name: read_name(rec),
            parts,
            colours,
        });
    }
    out
}

/// Rename an item set.
pub fn set_item_set_name(save: &mut Save, slot: usize, index: usize, name: &str) -> bool {
    if index >= ITEM_SETS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ITEM_SETS + index * ITEM_SET_SIZE;
    let Some(dst) = save.body_mut().get_mut(at..at + 32) else {
        return false;
    };
    write_name(dst, name);
    true
}

/// Replace one entry of an item set.
pub fn set_item_set_entry(
    save: &mut Save,
    slot: usize,
    index: usize,
    entry: usize,
    id: u16,
    count: u16,
) -> bool {
    if index >= ITEM_SETS || entry >= ITEM_SET_ITEMS {
        return false;
    }
    if id as usize > crate::gamedata::LAST_ITEM_ID {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ITEM_SETS + index * ITEM_SET_SIZE + ITEM_SET_BODY + entry * 4;
    let Some(dst) = save.body_mut().get_mut(at..at + 4) else {
        return false;
    };
    dst[0..2].copy_from_slice(&id.to_le_bytes());
    dst[2..4].copy_from_slice(&count.to_le_bytes());
    true
}

/// Rename an equipment set.
pub fn set_equip_set_name(save: &mut Save, slot: usize, index: usize, name: &str) -> bool {
    if index >= EQUIP_SETS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_EQUIP_SETS + index * EQUIP_SET_SIZE;
    let Some(dst) = save.body_mut().get_mut(at..at + 32) else {
        return false;
    };
    write_name(dst, name);
    true
}