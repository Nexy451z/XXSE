//! Adding, replacing and clearing equipment records.
//!
//! The three operations that are not a plain field edit. Each one has to ask
//! the reference checker first: the seven worn items and forty saved sets hold
//! box indices, and clearing a record that one of them still names leaves a
//! dangling index that this tool cannot repair.

use alloc::vec::Vec;

use crate::equip;
use crate::equiprefs::{self, EquipRef};
use crate::save::Save;

/// Why an equipment change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquipError {
    /// The slot number is not 0..3 or the record is past the end of the box.
    OutOfRange,
    /// The record is already blank.
    AlreadyBlank,
    /// The record is still named by something. Carries where.
    Referenced(Vec<EquipRef>),
    /// No blank record is left in the box.
    BoxFull,
}

impl EquipError {
    /// One line for the user.
    pub fn text(&self) -> alloc::string::String {
        use alloc::format;
        match self {
            EquipError::OutOfRange => alloc::string::String::from("枠が範囲外です"),
            EquipError::AlreadyBlank => alloc::string::String::from("すでに空き枠です"),
            EquipError::Referenced(refs) => {
                let mut s = alloc::string::String::from("使用中のため削除できません: ");
                for (i, r) in refs.iter().take(3).enumerate() {
                    if i > 0 {
                        s += "、";
                    }
                    s += &r.text();
                }
                if refs.len() > 3 {
                    s += &format!(" ほか{}件", refs.len() - 3);
                }
                s
            }
            EquipError::BoxFull => alloc::string::String::from("空き枠がありません"),
        }
    }
}

/// Clear a record, refusing while anything still points at it.
///
/// `force` is for the caller that has shown the references and been told to go
/// ahead anyway.
pub fn clear(
    save: &mut Save,
    slot: usize,
    index: usize,
    force: bool,
) -> Result<(), EquipError> {
    let Some(box_bytes) = save.equipment_box(slot) else {
        return Err(EquipError::OutOfRange);
    };
    let o = index * equip::EQUIP_RECORD;
    let Some(rec) = box_bytes.get(o..o + equip::EQUIP_RECORD) else {
        return Err(EquipError::OutOfRange);
    };
    if equip::record_is_blank(rec) {
        return Err(EquipError::AlreadyBlank);
    }
    let refs = equiprefs::references(save, slot, index);
    if !refs.is_empty() && !force {
        return Err(EquipError::Referenced(refs));
    }
    let Some(target) = save.equipment_record_mut(slot, index) else {
        return Err(EquipError::OutOfRange);
    };
    equip::clear(target);
    Ok(())
}

/// Put a different item in an existing record.
///
/// The type and the id are what change; the level is kept unless a new one is
/// given, and the three decoration ids are emptied because they belonged to
/// the item that was there. The tail at +0x0C is left alone: it belongs to the
/// type, and a record can carry data its type does not explain.
pub fn replace(
    save: &mut Save,
    slot: usize,
    index: usize,
    kind: u8,
    id: u16,
    level: u16,
) -> Result<(), EquipError> {
    let Some(box_bytes) = save.equipment_box(slot) else {
        return Err(EquipError::OutOfRange);
    };
    let o = index * equip::EQUIP_RECORD;
    let Some(rec) = box_bytes.get(o..o + equip::EQUIP_RECORD) else {
        return Err(EquipError::OutOfRange);
    };
    if equip::record_is_blank(rec) {
        // Replacing a blank record is what create() is for.
        return Err(EquipError::AlreadyBlank);
    }
    let Some(target) = save.equipment_record_mut(slot, index) else {
        return Err(EquipError::OutOfRange);
    };
    let mut head = u16::from_le_bytes([target[0], target[1]]);
    head &= !0x1F;
    head |= (kind as u16) & 0x1F;
    target[0..2].copy_from_slice(&head.to_le_bytes());
    target[2..4].copy_from_slice(&id.to_le_bytes());
    for socket in 0..3 {
        equip::set_decoration(target, socket, 0);
    }
    equip::set_level(target, level);
    Ok(())
}

/// Write a new item into the first blank record.
///
/// Returns the index used. The tail is zeroed because the record was blank to
/// begin with, so there is nothing there to preserve.
pub fn create(
    save: &mut Save,
    slot: usize,
    kind: u8,
    id: u16,
    level: u16,
) -> Result<usize, EquipError> {
    let index = {
        let Some(box_bytes) = save.equipment_box(slot) else {
            return Err(EquipError::OutOfRange);
        };
        (0..equip::EQUIP_COUNT).find(|i| {
            let o = i * equip::EQUIP_RECORD;
            box_bytes
                .get(o..o + equip::EQUIP_RECORD)
                .map(equip::record_is_blank)
                .unwrap_or(false)
        })
    };
    let Some(index) = index else {
        return Err(EquipError::BoxFull);
    };
    let Some(target) = save.equipment_record_mut(slot, index) else {
        return Err(EquipError::OutOfRange);
    };
    target.fill(0);
    let head: u16 = (kind as u16) & 0x1F;
    target[0..2].copy_from_slice(&head.to_le_bytes());
    target[2..4].copy_from_slice(&id.to_le_bytes());
    equip::set_level(target, level);
    Ok(index)
}