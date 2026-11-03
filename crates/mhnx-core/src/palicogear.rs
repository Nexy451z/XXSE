//! The palico equipment box.
//!
//! A character slot holds two equipment boxes that share the 36 byte record
//! shape of the hunter's box ([`crate::equip`]): 2000 records at slot + 0x62EE
//! and then 1000 more at slot + 0x17C2E. This is the second one.
//!
//! Where it starts is arithmetic that closes rather than a guess. 2000 records
//! of 36 bytes are 72000 = 0x17C2E bytes, so the hunter's box (0x62EE) ends
//! exactly where this one begins; this box is 1000 * 36 = 36000 = 0x208CE bytes
//! and ends exactly where the equipment set array starts
//! ([`crate::myset::OFF_EQUIP_SETS`]). Both boundaries were checked against a
//! real save: every record here that is in use carries a type of 22, 23 or 24
//! and nothing else, while the hunter's box next to it carries 1..=21 only, and
//! reading either one a record out of place produces types the game does not
//! write.
//!
//! The type lives in the low five bits of the first word, exactly as it does
//! for the hunter's box, and only three values belong here:
//!
//! * 22 = palico weapon
//! * 23 = palico head armour
//! * 24 = palico body armour
//!
//! A palico names the three pieces it wears by box index in the roster record
//! at +0x100 (weapon, head, body; 0xFFFF when nothing is worn) - see
//! [`crate::palico`]. In the sample save every one of those indices lands on a
//! record of the kind its position implies, which is the evidence that this
//! offset and this record size are right. That roster field is the only thing
//! in a character slot that points into this box, which is why [`clear`] asks it
//! before emptying a record.
//!
//! Nothing past +0x0C is written by anything in this module. In the sample save
//! all 72 used palico records have an all zero tail anyway - the palico box is
//! far quieter than the hunter's, where talismans keep skills there - but the
//! tail is still treated as type owned: [`set_level`], [`set_decoration`],
//! [`create`] and [`clear`] touch the first twelve bytes and stop there.

use alloc::vec::Vec;

use crate::equip::{self, Equipment};
use crate::equipops::EquipError;
use crate::save::Save;

/// Offset of the palico equipment box, relative to the slot base.
///
/// Equals 0x62EE + 2000 * 36, the end of the hunter's box.
pub const PALICO_EQUIP_OFFSET: usize = 0x17C2E;
/// Records in the box.
pub const PALICO_EQUIP_COUNT: usize = 1000;
/// Bytes per record, the same 36 the hunter's box uses.
pub const RECORD: usize = equip::EQUIP_RECORD;

/// Palico weapon.
pub const WEAPON: u8 = 22;
/// Palico head armour.
pub const HEAD: u8 = 23;
/// Palico body armour.
pub const BODY: u8 = 24;
/// The three kinds that belong in this box, in the order a palico wears them.
pub const KINDS: [u8; 3] = [WEAPON, HEAD, BODY];

/// Bytes at the head of a record that this module may write.
///
/// Type, level and transmog level in the first word, the id at +0x02, the
/// transmog id at +0x04 and three decoration ids from +0x06. Everything from
/// +0x0C is owned by the type and is never written here.
pub const WRITABLE: usize = 0x0C;

/// Highest level the record's five bit field can show.
pub const MAX_LEVEL: u16 = 32;

// The box has to fit inside a character slot, and it does: 0x17C2E + 36000 =
// 0x208CE, well inside the 0x11D088 bytes a slot holds on MHXX. Asserted at
// compile time so a wrong offset cannot silently slice into the next structure.
const _: () = assert!(
    PALICO_EQUIP_OFFSET + PALICO_EQUIP_COUNT * RECORD <= crate::save::SLOT_DATA_SIZE
);

/// Does this type belong in the palico box?
pub fn is_palico_kind(kind: u8) -> bool {
    matches!(kind, WEAPON | HEAD | BODY)
}

/// Which part of a worn list a kind belongs at.
///
/// 22 -> 0 (weapon), 23 -> 1 (head), 24 -> 2 (body).
pub fn part_of(kind: u8) -> Option<usize> {
    match kind {
        WEAPON => Some(0),
        HEAD => Some(1),
        BODY => Some(2),
        _ => None,
    }
}

fn record_range(save: &Save, slot: usize, index: usize) -> Option<core::ops::Range<usize>> {
    if index >= PALICO_EQUIP_COUNT {
        return None;
    }
    let base = save.slot_base(slot)? as usize;
    let at = base + PALICO_EQUIP_OFFSET + index * RECORD;
    if at.checked_add(RECORD)? > save.body().len() {
        return None;
    }
    Some(at..at + RECORD)
}

/// The whole box of one character, as stored.
///
/// The range is rebuilt from [`PALICO_EQUIP_OFFSET`] rather than taken from
/// [`Save::palico_equipment_box`]; the two agree, and the test suite says so, so
/// that a change to either one cannot go unnoticed.
pub fn box_bytes(save: &Save, slot: usize) -> Option<&[u8]> {
    let base = save.slot_base(slot)? as usize;
    let at = base + PALICO_EQUIP_OFFSET;
    save.body().get(at..at + PALICO_EQUIP_COUNT * RECORD)
}

/// One raw record, or None when the index or the slot is out of range.
pub fn record(save: &Save, slot: usize, index: usize) -> Option<&[u8]> {
    let r = record_range(save, slot, index)?;
    save.body().get(r)
}

fn record_mut<'a>(save: &'a mut Save, slot: usize, index: usize) -> Option<&'a mut [u8]> {
    let r = record_range(save, slot, index)?;
    save.body_mut().get_mut(r)
}

/// Decode the records that are in use.
///
/// A record counts as in use when it has a type: an all zero record is the empty
/// state the game leaves behind, and a record with data but no type is not
/// something the game would show as gear, so neither is returned. The `index`
/// in the result is the box index, the number a palico's worn list holds.
pub fn decode(save: &Save, slot: usize) -> Vec<Equipment> {
    let mut out = Vec::new();
    let Some(box_bytes) = box_bytes(save, slot) else {
        return out;
    };
    for index in 0..PALICO_EQUIP_COUNT {
        let off = index * RECORD;
        let Some(rec) = box_bytes.get(off..off + RECORD) else {
            break;
        };
        if equip::record_is_blank(rec) {
            continue;
        }
        if let Some(eq) = equip::decode_record(index, rec) {
            if eq.kind != 0 {
                out.push(eq);
            }
        }
    }
    out
}

/// Decode one record of the box, or None when it holds nothing.
pub fn decode_one(save: &Save, slot: usize, index: usize) -> Option<Equipment> {
    let rec = record(save, slot, index)?;
    if equip::record_is_blank(rec) {
        return None;
    }
    equip::decode_record(index, rec)
}

/// The first blank record that could hold a new piece of `kind`.
///
/// `kind` has to be one of [`KINDS`]; anything else returns None instead of
/// picking a slot, because a caller asking for a type the palico box cannot hold
/// is better told nothing than handed a row the game will not read.
///
/// The box is a flat array - the game does not reserve a range per kind, and in
/// the sample save weapons, heads and bodies are interleaved near the end - so
/// the kind does not move the answer. What the search accepts is exactly what
/// [`crate::equipops::create`] accepts for the hunter's box: a record with every
/// byte zero. A record whose type was cleared but whose other bytes were left
/// behind is not offered, so a write can never overwrite data this module does
/// not understand.
pub fn first_free(save: &Save, slot: usize, kind: u8) -> Option<usize> {
    if !is_palico_kind(kind) {
        return None;
    }
    let box_bytes = box_bytes(save, slot)?;
    (0..PALICO_EQUIP_COUNT).find(|i| {
        let off = i * RECORD;
        box_bytes
            .get(off..off + RECORD)
            .map(equip::record_is_blank)
            .unwrap_or(false)
    })
}

/// How many records hold something.
pub fn used(save: &Save, slot: usize) -> usize {
    decode(save, slot).len()
}

/// Which palico wears the record at `index`, if any.
///
/// Returns the roster index (0..84), not the box index. A record can be worn by
/// more than one palico and only the first is reported; [`worn_by_all`] has the
/// whole list. Unnamed roster records are searched too: a released palico can
/// still leave an index behind, and clearing a record it names would move a
/// value the game still holds.
pub fn worn_by(save: &Save, slot: usize, index: usize) -> Option<usize> {
    worn_by_all(save, slot, index).first().copied()
}

/// Every palico that wears the record at `index`.
pub fn worn_by_all(save: &Save, slot: usize, index: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if index > u16::MAX as usize {
        return out;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return out;
    };
    let want = index as u16;
    let body = save.body();
    for p in 0..crate::palico::SLOTS {
        let at =
            base + crate::palico::OFF_ROSTER + p * crate::palico::RECORD + crate::palico::EQUIP;
        for part in 0..crate::palico::EQUIP_SLOTS {
            let off = at + part * 2;
            let Some(bytes) = body.get(off..off + 2) else {
                return out;
            };
            if u16::from_le_bytes([bytes[0], bytes[1]]) == want {
                out.push(p);
                break;
            }
        }
    }
    out
}

/// Rewrite the level of one record.
///
/// The level is stored minus one in five bits, so it runs 1..=32. A level
/// outside that range is refused rather than clamped: the caller is asking for
/// something the field cannot hold, and silently writing 32 would hide it. A
/// blank record is refused too - a level with no type in front of it is not a
/// record the game can use.
pub fn set_level(save: &mut Save, slot: usize, index: usize, level: u16) -> bool {
    if level < 1 || level > MAX_LEVEL {
        return false;
    }
    let blank = match record(save, slot, index) {
        Some(rec) => equip::record_is_blank(rec),
        None => return false,
    };
    if blank {
        return false;
    }
    let Some(rec) = record_mut(save, slot, index) else {
        return false;
    };
    equip::set_level(rec, level)
}

/// Rewrite one of the three decoration sockets. Zero empties it.
///
/// A blank record is refused, and so is a socket past the third. Palico gear in
/// the sample save carries no decoration at all - every socket is zero in all 72
/// used records - but the field is there and the record shape is shared with the
/// hunter's box, so it is editable.
pub fn set_decoration(save: &mut Save, slot: usize, index: usize, socket: usize, id: u16) -> bool {
    if socket >= 3 {
        return false;
    }
    let blank = match record(save, slot, index) {
        Some(rec) => equip::record_is_blank(rec),
        None => return false,
    };
    if blank {
        return false;
    }
    let Some(rec) = record_mut(save, slot, index) else {
        return false;
    };
    equip::set_decoration(rec, socket, id)
}

/// Write a new piece of palico gear into the first blank record.
///
/// Returns the box index used, which is the number to put in a palico's worn
/// list. The type has to be one of [`KINDS`], the id may not be zero and the
/// level has to fit the field; anything else is [`EquipError::OutOfRange`], and a
/// box with no blank record left is [`EquipError::BoxFull`].
pub fn create(
    save: &mut Save,
    slot: usize,
    kind: u8,
    id: u16,
    level: u16,
) -> Result<usize, EquipError> {
    if !is_palico_kind(kind) || id == 0 || level < 1 || level > MAX_LEVEL {
        return Err(EquipError::OutOfRange);
    }
    if box_bytes(save, slot).is_none() {
        return Err(EquipError::OutOfRange);
    }
    let index = match first_free(save, slot, kind) {
        Some(i) => i,
        None => return Err(EquipError::BoxFull),
    };
    let Some(rec) = record_mut(save, slot, index) else {
        return Err(EquipError::OutOfRange);
    };
    rec[..WRITABLE].fill(0);
    rec[0..2].copy_from_slice(&(kind as u16).to_le_bytes());
    rec[2..4].copy_from_slice(&id.to_le_bytes());
    equip::set_level(rec, level);
    Ok(index)
}

/// Empty a record, refusing while a palico still wears it.
///
/// Returns false when the index or the slot is out of range, when the record is
/// already blank, or when [`worn_by`] still names it - that last case is the one
/// that matters: the worn list holds box indices and a dangling one is not
/// something this library can repair. Take the gear off the palico first, then
/// clear.
///
/// Only the first twelve bytes are zeroed. On the sample save the tail is
/// already zero everywhere, so the record reads as blank again and
/// [`first_free`] will offer it; a record with a tail this module does not
/// understand keeps that tail and is not offered by `first_free`, which is the
/// cautious half of the same rule.
pub fn clear(save: &mut Save, slot: usize, index: usize) -> bool {
    let blank = match record(save, slot, index) {
        Some(rec) => equip::record_is_blank(rec),
        None => return false,
    };
    if blank {
        return false;
    }
    if !worn_by_all(save, slot, index).is_empty() {
        return false;
    }
    let Some(rec) = record_mut(save, slot, index) else {
        return false;
    };
    rec[..WRITABLE].fill(0);
    true
}
