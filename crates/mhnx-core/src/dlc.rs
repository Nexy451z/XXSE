//! Download-quest storage inside the save body.
//!
//! The shared block of the save starts with a download-quest section whose
//! offset is stored at body[0x0C]. Relative to that section offset:
//!
//! ```text
//!     +0x004C   28 bytes   installed bit set (7 x u32, 0xFFFFFFFF = enabled)
//!     +0x45F4   205 slots of 0x1400 bytes   quest archives (raw .arc bytes)
//!     +0x1049F4  45 slots of 0x0800 bytes   quest equipment (q<id>.equ)
//! ```
//!
//! The quest array plus the equipment array end **exactly** at the first
//! character slot base 0x126474, which is how this layout was confirmed.
//!
//! Each quest slot is  { s32 quest_id; s32 size; u8 raw_arc[size]; }
//! with quest_id == 0 marking a free slot. Only size is authoritative: slots
//! keep stale bytes from earlier quests after the payload ends.
//!
//! # The installed bit set is not indexed by slot
//!
//! The 28 byte set at section + 0x4C looks tempting as "bit i = slot i" but
//! that is provably wrong: the real save has bits 205..214 set while the quest
//! array only has slots 0..204, and bit 25 clear while slot 25 holds a quest.
//! Counting matches is not evidence either: the sample save currently holds
//! 175 set bits for 175 records, but the same file had 166 when this note was
//! first written and the quest set has grown since. The totals line up because
//! one bit means one installed quest, not because bit i maps to slot i.
//!
//! The set is indexed by **catalogue entry**: mask bits 0..40 line up exactly
//! with the 45 entry quest-equipment catalogue, and the remaining 135 set bits
//! pair with the 135 event records. So a bit identifies a *quest*, not a
//! position, and a slot index cannot be turned into a bit index without the
//! catalogue.
//!
//! Consequence for editing: **replacing a quest that is already installed
//! needs no mask write at all**. Its catalogue bit is already set, and the
//! slot keeps both its id and its bit. This module never writes the set.
//!
//! Only replacing an installed quest is supported, because that is the only
//! operation whose effect on the bit set is known to be a no-op. Adding a quest
//! to a free slot would need its catalogue bit, which cannot be derived from
//! the slot number, so install_quest refuses instead of guessing.

use alloc::vec::Vec;

use crate::save::Save;
use crate::Error;

/// One item entry inside a quest-equipment loadout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestEquipItem {
    /// Which of the five loadouts (0..4). Entry 4 is the ranged one.
    pub record: usize,
    /// Which of the two lists: the 24 slot supply box, or the 8 slot ammo list.
    pub list: QuestEquipList,
    /// Index inside that list.
    pub index: usize,
    /// Item id, 0 when the slot is unused.
    pub item: u16,
    /// Stack count.
    pub count: u8,
}

/// The two item lists a loadout carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestEquipList {
    /// The 24 slot supply box at body + 0x53.
    Supply,
    /// The 8 slot ammo or bottle list at body + 0x9B.
    Ammo,
}

impl QuestEquipList {
    /// Byte offset of the list inside a loadout body.
    fn offset(self) -> usize {
        match self {
            QuestEquipList::Supply => 0x53,
            QuestEquipList::Ammo => 0x9B,
        }
    }
    /// Entries in the list.
    pub fn len(self) -> usize {
        match self {
            QuestEquipList::Supply => 24,
            QuestEquipList::Ammo => 8,
        }
    }
}

/// Loadout header: the id and size come first, then 0x1D bytes, then five
/// blocks of a 3 byte header and a 179 byte body.
const LOADOUT_HEADER: usize = 0x1D;
const LOADOUT_BLOCK: usize = 182;
const LOADOUT_BLOCK_HEADER: usize = 3;

fn loadout_block(data: &[u8], record: usize) -> Option<usize> {
    if record >= 5 {
        return None;
    }
    let at = LOADOUT_HEADER + LOADOUT_BLOCK * record + LOADOUT_BLOCK_HEADER;
    if at + 0xA3 > data.len() {
        return None;
    }
    Some(at)
}

/// Every item entry of one quest-equipment slot.
pub fn quest_equipment_items(save: &Save, slot: usize) -> Vec<QuestEquipItem> {
    let mut out = Vec::new();
    let Some(bytes) = quest_equipment_bytes(save, slot) else {
        return out;
    };
    let data = &bytes[8..];
    for record in 0..5 {
        let Some(body) = loadout_block(data, record) else {
            continue;
        };
        for list in [QuestEquipList::Supply, QuestEquipList::Ammo] {
            for index in 0..list.len() {
                let at = body + list.offset() + index * 3;
                let Some(b) = data.get(at..at + 3) else {
                    continue;
                };
                out.push(QuestEquipItem {
                    record,
                    list,
                    index,
                    item: u16::from_le_bytes([b[0], b[1]]),
                    count: b[2],
                });
            }
        }
    }
    out
}

/// Rewrite one item entry of a quest-equipment slot.
///
/// Only the three bytes of that entry change: the loadout length, the block
/// headers, the trailer and the equipment mask are all left alone, which is
/// what makes this safe to do without understanding the rest of the record.
pub fn set_quest_equipment_item(
    save: &mut Save,
    slot: usize,
    record: usize,
    list: QuestEquipList,
    index: usize,
    item: u16,
    count: u8,
) -> Result<(), Error> {
    if index >= list.len() {
        return Err(Error::OutOfRange {
            context: "item index",
            value: index as u32,
        });
    }
    if item as usize > crate::gamedata::LAST_ITEM_ID {
        return Err(Error::OutOfRange {
            context: "item id past the item table",
            value: item as u32,
        });
    }
    let sec = section_start(save).ok_or(Error::OutOfRange {
        context: "save has no DLC section",
        value: 0,
    })?;
    let base = sec + EQUIP_ARRAY_OFFSET + slot * EQUIP_SLOT_SIZE;
    let at = {
        let body = save.body().get(base + 8..base + EQUIP_SLOT_SIZE).ok_or(
            Error::OutOfRange {
                context: "quest equipment slot",
                value: base as u32,
            },
        )?;
        let off = loadout_block(body, record).ok_or(Error::OutOfRange {
            context: "loadout record",
            value: record as u32,
        })?;
        base + 8 + off + list.offset() + index * 3
    };
    let dst = save.body_mut().get_mut(at..at + 3).ok_or(Error::OutOfRange {
        context: "item entry",
        value: at as u32,
    })?;
    dst[0..2].copy_from_slice(&item.to_le_bytes());
    dst[2] = count;
    Ok(())
}
/// One of the 45 quest-equipment entries the DLC section carries.
///
/// Every non-empty entry starts with the quest id it belongs to, which is what
/// makes the low mask bits readable: bit i is set exactly when entry i holds
/// data, and entry i names the quest. Verified against the sample save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestEquipment {
    /// Index into the 45 entry array.
    pub index: usize,
    /// Quest id from the first four bytes, or 0 when the entry is empty.
    pub quest_id: i32,
    /// Does the entry hold any data at all?
    pub occupied: bool,
}

/// Mask bit for a catalogue position.
///
/// The mask is two fields side by side: the 45 equipment entries sit at bits
/// 0..44, and the 160 event entries start at bit 64 because the game rounds the
/// equipment array up to two 32 bit words. Positions 41..44 are placeholders in
/// the catalogue and never carry a bit.
pub fn bit_for_position(pos: usize) -> Option<usize> {
    if pos > 204 {
        return None;
    }
    if (41..=44).contains(&pos) {
        return None;
    }
    Some(if pos < 45 { pos } else { 64 + (pos - 45) })
}

/// Catalogue position for a quest id, if the catalogue lists it.
#[cfg(feature = "gamedata")]
pub fn position_for_quest(quest_id: i32) -> Option<usize> {
    crate::gamedata::DLC_CATALOG
        .iter()
        .position(|q| *q == quest_id)
}

/// Mask bit for a quest id, if the catalogue lists it.
#[cfg(feature = "gamedata")]
pub fn bit_for_quest(quest_id: i32) -> Option<usize> {
    bit_for_position(position_for_quest(quest_id)?)
}

/// Is the installed mask bit set for this quest?
pub fn is_installed(save: &Save, quest_id: i32) -> bool {
    let Some(bit) = bit_for_quest(quest_id) else {
        return false;
    };
    let Some(sec) = section_start(save) else {
        return false;
    };
    save.body()
        .get(sec + BITMASK_OFFSET + (bit >> 3))
        .map(|b| (b >> (bit & 7)) & 1 == 1)
        .unwrap_or(false)
}

/// Set or clear one installed-mask bit.
///
/// This is the operation the tool has always refused to do, because a wrong bit
/// means a quest the game lists but cannot load, or one it will not list at all.
/// The catalogue makes the bit computable. Whether the game accepts a quest
/// placed in a slot the catalogue never filled is still unverified, so the
/// callers keep this behind an explicit opt in.
pub fn set_installed(save: &mut Save, quest_id: i32, on: bool) -> Result<(), Error> {
    // The module says every writer refuses an MHGU save; this one did not.
    check_kind(save)?;
    let bit = bit_for_quest(quest_id).ok_or(Error::OutOfRange {
        context: "quest id is not in the DLC catalogue",
        value: quest_id as u32,
    })?;
    let sec = section_start(save).ok_or(Error::OutOfRange {
        context: "save has no DLC section",
        value: 0,
    })?;
    let off = sec + BITMASK_OFFSET + (bit >> 3);
    let byte = save.body_mut().get_mut(off).ok_or(Error::OutOfRange {
        context: "mask byte",
        value: off as u32,
    })?;
    if on {
        *byte |= 1 << (bit & 7);
    } else {
        *byte &= !(1 << (bit & 7));
    }
    Ok(())
}

/// Read the quest-equipment array.
pub fn quest_equipment(save: &Save) -> Vec<QuestEquipment> {
    let mut out = Vec::with_capacity(EQUIP_SLOTS);
    let Some(sec) = section_start(save) else {
        return out;
    };
    let body = save.body();
    for i in 0..EQUIP_SLOTS {
        let off = sec + EQUIP_ARRAY_OFFSET + i * EQUIP_SLOT_SIZE;
        let Some(rec) = body.get(off..off + EQUIP_SLOT_SIZE) else {
            break;
        };
        let occupied = rec.iter().any(|b| *b != 0);
        let quest_id = if occupied {
            i32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]])
        } else {
            0
        };
        out.push(QuestEquipment {
            index: i,
            quest_id,
            occupied,
        });
    }
    out
}

/// The raw bytes of one quest-equipment entry.
pub fn quest_equipment_bytes(save: &Save, index: usize) -> Option<&[u8]> {
    if index >= EQUIP_SLOTS {
        return None;
    }
    let sec = section_start(save)?;
    let off = sec + EQUIP_ARRAY_OFFSET + index * EQUIP_SLOT_SIZE;
    save.body().get(off..off + EQUIP_SLOT_SIZE)
}

/// Number of downloadable quest slots.
pub const QUEST_SLOTS: usize = 205;
/// Size of one quest slot.
pub const QUEST_SLOT_SIZE: usize = 0x1400;
/// Maximum .arc payload that fits in a quest slot.
pub const QUEST_MAX_PAYLOAD: usize = QUEST_SLOT_SIZE - 8;
/// Number of quest equipment slots.
pub const EQUIP_SLOTS: usize = 45;
/// Size of one quest equipment slot.
pub const EQUIP_SLOT_SIZE: usize = 0x800;
/// Offset of the quest array, relative to the section offset.
pub const QUEST_ARRAY_OFFSET: usize = 0x45F4;
/// Offset of the equipment array, relative to the section offset.
pub const EQUIP_ARRAY_OFFSET: usize = 0x1049F4;
/// Offset of the installed bit set, relative to the section offset.
pub const BITMASK_OFFSET: usize = 0x4C;
/// Length of the installed bit set in bytes.
pub const BITMASK_LEN: usize = 28;

/// One occupied download-quest slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledQuest {
    /// Index of the slot (0..204).
    pub slot: usize,
    /// Quest id stored in the slot.
    pub quest_id: i32,
    /// Payload length in bytes.
    pub size: i32,
    /// Raw .arc payload.
    pub data: Vec<u8>,
}

fn read_i32(b: &[u8], off: usize) -> i32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as i32
}

/// Validate that the download-quest section really lies in front of the
/// character slots.
///
/// The section offset comes out of the save, so a corrupt one can point the
/// quest array at the equipment box or at a character slot. Requiring the whole
/// section - quest array plus equipment - to end at or before the first
/// character slot rejects every such save. On a real MHXX save the two are
/// exactly equal (0xB280 + 0x45F4 + 205*0x1400 + 45*0x800 = 0x126474).
fn section_start(save: &Save) -> Option<usize> {
    let sec = save.dlc_section_offset() as usize;
    let end = sec
        .checked_add(QUEST_ARRAY_OFFSET)?
        .checked_add(QUEST_SLOTS * QUEST_SLOT_SIZE)?
        .checked_add(EQUIP_SLOTS * EQUIP_SLOT_SIZE)?;
    let first_slot = save.slot_base(0)? as usize;
    if end > first_slot || end > save.body().len() {
        return None;
    }
    Some(sec)
}

/// MHGU moved the download-quest section, so the offsets below are MHXX only.
///
/// Everything that writes refuses to touch an MHGU save rather than apply
/// offsets taken from the wrong game. Reading returns empty for the same
/// reason: a wrong offset would report plausible nonsense.
fn check_kind(save: &Save) -> Result<(), Error> {
    match save.kind() {
        crate::save::SaveKind::Mhgu => {
            Err(Error::Parse("download quests are not supported for MHGU saves"))
        }
        _ => Ok(()),
    }
}

/// Read the installed bit set.
pub fn bitmask(save: &Save) -> Option<[u8; BITMASK_LEN]> {
    if check_kind(save).is_err() {
        return None;
    }
    let sec = section_start(save)?;
    let off = sec + BITMASK_OFFSET;
    if off + BITMASK_LEN > save.body().len() {
        return None;
    }
    let mut out = [0u8; BITMASK_LEN];
    out.copy_from_slice(&save.body()[off..off + BITMASK_LEN]);
    Some(out)
}

/// How many bits of the installed set are on.
///
/// Diagnostics only. There is deliberately no "set the bit for slot N" helper:
/// the set is catalogue indexed, so a slot number cannot address a bit. See the
/// module documentation.
pub fn installed_bit_count(save: &Save) -> usize {
    match bitmask(save) {
        Some(m) => m.iter().map(|b| b.count_ones() as usize).sum(),
        None => 0,
    }
}

/// List every occupied quest slot.
pub fn list_quests(save: &Save) -> Vec<InstalledQuest> {
    let mut out = Vec::new();
    if check_kind(save).is_err() {
        return out;
    }
    let Some(sec) = section_start(save) else {
        return out;
    };
    let body = save.body();
    for i in 0..QUEST_SLOTS {
        let base = sec + QUEST_ARRAY_OFFSET + i * QUEST_SLOT_SIZE;
        if base + 8 > body.len() {
            break;
        }
        // A corrupt size must never reach a slice index. Only size bytes are
        // authoritative: the rest of the slot may hold stale quest data.
        let id = read_i32(body, base);
        let size = read_i32(body, base + 4);
        if id == 0 {
            continue;
        }
        if !(0..=QUEST_MAX_PAYLOAD as i32).contains(&size) {
            out.push(InstalledQuest {
                slot: i,
                quest_id: id,
                size,
                data: Vec::new(),
            });
            continue;
        }
        // The slot may run past the body when the section pointer is corrupt,
        // so bound the read rather than trusting the slot extent.
        let n = size as usize;
        let end = (base + 8).saturating_add(n).min(body.len());
        let data = body.get(base + 8..end).map(|s| s.to_vec()).unwrap_or_default();
        out.push(InstalledQuest {
            slot: i,
            quest_id: id,
            size,
            data,
        });
    }
    out
}

/// Bounds check for one quest slot.
fn slot_extent(save: &Save, slot: usize) -> Result<usize, Error> {
    if slot >= QUEST_SLOTS {
        return Err(Error::OutOfRange {
            context: "quest slot",
            value: slot as u32,
        });
    }
    let sec = section_start(save).ok_or(Error::OutOfRange {
        context: "the download-quest section does not fit before the character slots",
        value: save.dlc_section_offset(),
    })?;
    let base = sec
        .checked_add(QUEST_ARRAY_OFFSET)
        .and_then(|v| v.checked_add(slot * QUEST_SLOT_SIZE))
        .ok_or(Error::OutOfRange {
            context: "quest slot offset",
            value: slot as u32,
        })?;
    if base + QUEST_SLOT_SIZE > save.body().len() {
        return Err(Error::OutOfRange {
            context: "quest slot offset",
            value: base as u32,
        });
    }
    Ok(base)
}

/// Which equipment-catalogue entry holds a quest id, if any.
///
/// The 45 entries at section + 0x1049F4 are the challenge quests and their
/// equipment data; entry k is the quest whose installed bit is bit k. Returns
/// None for anything that is not in that table.
pub fn equipment_catalogue_index(save: &Save, quest_id: i32) -> Option<usize> {
    if check_kind(save).is_err() || quest_id <= 0 {
        return None;
    }
    let sec = section_start(save)?;
    let body = save.body();
    for k in 0..EQUIP_SLOTS {
        let base = sec.checked_add(EQUIP_ARRAY_OFFSET)?.checked_add(k * EQUIP_SLOT_SIZE)?;
        if base + 4 > body.len() {
            return None;
        }
        if read_i32(body, base) == quest_id {
            return Some(k);
        }
    }
    None
}

/// Is a quest's installed flag set, when that can be determined?
///
/// Returns None when the quest is not in the equipment table, which is the
/// case for every event quest: their half of the catalogue cannot be
/// reconstructed from one save.
pub fn installed_flag(save: &Save, quest_id: i32) -> Option<bool> {
    let k = equipment_catalogue_index(save, quest_id)?;
    let m = bitmask(save)?;
    Some((m[k / 8] >> (k % 8)) & 1 == 1)
}

/// Replace the payload of a quest that is already installed.
///
/// This is the only supported way to write a quest, and it is the edit the
/// community guide describes by hand: the slot keeps its id and its catalogue
/// bit, so nothing the game indexes by quest can go stale. The installed bit
/// set is left exactly as it was.
///
/// Fails if the save does not already hold that quest, because adding one
/// would need its catalogue bit and a slot number cannot address that set.
/// Install a quest into a slot the save does not already use.
///
/// This is the operation the tool has always refused: without the catalogue a
/// quest id could not be turned into a mask bit, so a new entry would be a
/// quest the game either lists but cannot load, or never lists at all. The
/// catalogue is now embedded, so the bit is computable and both halves can be
/// written together.
///
/// Still unverified on hardware: whether the game accepts a quest written
/// into a slot no catalogue entry has ever filled. Callers should treat this
/// as an experiment and keep a backup.
pub fn add_quest(save: &mut Save, quest_id: i32, arc_bytes: &[u8]) -> Result<usize, Error> {
    check_kind(save)?;
    if arc_bytes.len() > QUEST_MAX_PAYLOAD {
        return Err(Error::OutOfRange {
            context: "quest payload larger than a slot",
            value: arc_bytes.len() as u32,
        });
    }
    if bit_for_quest(quest_id).is_none() {
        return Err(Error::OutOfRange {
            context: "quest id is not in the DLC catalogue",
            value: quest_id as u32,
        });
    }
    if list_quests(save).iter().any(|q| q.quest_id == quest_id) {
        return Err(Error::OutOfRange {
            context: "quest is already installed",
            value: quest_id as u32,
        });
    }
    let sec = section_start(save).ok_or(Error::OutOfRange {
        context: "save has no DLC section",
        value: 0,
    })?;
    // The first slot whose size is zero is free: a used slot always has a
    // non-zero length.
    let empty = (0..QUEST_SLOTS).find(|i| {
        let base = sec + QUEST_ARRAY_OFFSET + i * QUEST_SLOT_SIZE;
        save.body()
            .get(base + 4..base + 8)
            .map(|b| read_i32(b, 0))
            .unwrap_or(-1)
            == 0
    });
    let Some(slot) = empty else {
        return Err(Error::OutOfRange {
            context: "no free quest slot",
            value: QUEST_SLOTS as u32,
        });
    };
    let base = sec + QUEST_ARRAY_OFFSET + slot * QUEST_SLOT_SIZE;
    let rec = save
        .body_mut()
        .get_mut(base..base + QUEST_SLOT_SIZE)
        .ok_or(Error::OutOfRange {
            context: "quest slot",
            value: base as u32,
        })?;
    rec.fill(0);
    rec[0..4].copy_from_slice(&(quest_id as u32).to_le_bytes());
    rec[4..8].copy_from_slice(&(arc_bytes.len() as u32).to_le_bytes());
    rec[8..8 + arc_bytes.len()].copy_from_slice(arc_bytes);
    set_installed(save, quest_id, true)?;
    Ok(slot)
}

pub fn install_quest(save: &mut Save, quest_id: i32, arc_bytes: &[u8]) -> Result<usize, Error> {
    check_kind(save)?;
    if quest_id <= 0 {
        return Err(Error::OutOfRange {
            context: "quest id must be positive",
            value: quest_id as u32,
        });
    }
    // The payload has to be a real archive. This used to be an "if let Ok",
    // so anything at all was accepted: 777 bytes of 0xAB returned Ok, replaced
    // an installed quest and left a slot that no longer parses. The game would
    // then drop or crash on that entry.
    let arc = crate::arc::Arc::parse(arc_bytes).map_err(|_| {
        Error::Parse("the payload is not a quest archive, so it was not installed")
    })?;
    // The .arc carries its own quest id in the .mib. Installing a payload whose
    // id disagrees with the slot would leave the save claiming one quest and
    // holding another, which the game indexes by id, so refuse it.
    if let Some(mib) = arc
        .by_ext("mib")
        .and_then(|e| crate::quest::Mib::from_bytes(&e.data).ok())
    {
        let payload_id = mib.quest_id();
        if payload_id != 0 && payload_id != quest_id as u32 {
            return Err(Error::Parse(
                "the archive's own quest id does not match the id being installed",
            ));
        }
    } else {
        return Err(Error::Parse(
            "the archive has no readable .mib, so its quest id cannot be checked",
        ));
    }
    if arc_bytes.len() > QUEST_MAX_PAYLOAD {
        return Err(Error::OutOfRange {
            context: "quest payload too large for a slot",
            value: arc_bytes.len() as u32,
        });
    }
    // The whole edit rests on the record's catalogue bit already being set: the
    // game lists a quest from that set, not from the record. For the challenge
    // quests the mapping is derivable (bit k belongs to equipment entry k), so
    // check it there. The event half of the catalogue is not recoverable from a
    // single save, so those cannot be checked and are documented as such.
    if let Some(k) = equipment_catalogue_index(save, quest_id) {
        let on = bitmask(save)
            .map(|m| (m[k / 8] >> (k % 8)) & 1 == 1)
            .unwrap_or(true);
        if !on {
            return Err(Error::Parse(
                "that quest's installed flag is clear, so writing it would be invisible to the game",
            ));
        }
    }
    let slot = find_quest(save, quest_id).ok_or(Error::Parse(
        "this save does not have that quest yet. Only a quest the save already lists can be          updated: the game decides what to show from a catalogue indexed flag, and a free slot          number cannot address that flag. Install the quest once by other means, then use this          tool to change its contents.",
    ))?;
    let base = slot_extent(save, slot)?;
    let body = save.body_mut();
    // Do not blank the slot first. Real saves leave the tail of a slot full of
    // whatever a previously larger quest put there and only the size field is
    // authoritative, so clearing it would rewrite bytes the game itself keeps.
    // Leaving it alone also makes re-saving an untouched quest a true no-op:
    // the whole save comes back byte for byte identical.
    body[base..base + 4].copy_from_slice(&quest_id.to_le_bytes());
    body[base + 4..base + 8].copy_from_slice(&(arc_bytes.len() as i32).to_le_bytes());
    body[base + 8..base + 8 + arc_bytes.len()].copy_from_slice(arc_bytes);
    Ok(slot)
}

/// Zero one quest slot.
///
/// The record disappears but the catalogue bit cannot be cleared, because a
/// slot number does not address that set. The game may therefore still list an
/// empty entry, so treat this as a diagnostic rather than a clean uninstall.
pub fn uninstall_quest(save: &mut Save, slot: usize) -> Result<(), Error> {
    check_kind(save)?;
    let base = slot_extent(save, slot)?;
    save.body_mut()[base..base + QUEST_SLOT_SIZE].fill(0);
    Ok(())
}

/// Find the slot holding a quest id.
pub fn find_quest(save: &Save, quest_id: i32) -> Option<usize> {
    if check_kind(save).is_err() || quest_id <= 0 {
        return None;
    }
    let sec = section_start(save)?;
    let body = save.body();
    for i in 0..QUEST_SLOTS {
        let base = sec + QUEST_ARRAY_OFFSET + i * QUEST_SLOT_SIZE;
        if base + 8 > body.len() {
            break;
        }
        if read_i32(body, base) == quest_id {
            return Some(i);
        }
    }
    None
}

/// Install all 175 bundled official event & challenge quests (plus the 40
/// challenge quest equipment loadouts and DLC section bonus header flags)
/// into `save`.
///
/// Safe on both brand-new saves (that have never received any DLC) and saves
/// that already have some or all quests installed:
/// - Any quest already present in `save` with a valid payload size is kept as-is
///   (preserving user edits), while ensuring its catalogue bitmask bit is set.
/// - Any missing official quest is placed into its original default slot if free,
///   or the first available free slot otherwise, with its bitmask bit set.
/// - Any empty challenge quest equipment slot (0..40) is populated with its
///   official fixed loadout data.
/// - Any zeroed bytes in the DLC bonus header area (+0x0000..+0x45F4 outside
///   the quest bitmask) are populated with the official DLC unlock data.
///
/// Returns `Ok((newly_added_quests, total_installed_official_quests))`.
#[cfg(feature = "gamedata")]
pub fn install_official_dlc_quests(save: &mut Save) -> Result<(usize, usize), Error> {
    install_dlc_quests_by_category(save, true, false)
}

/// Install bundled quests filtered by category (`Official`, `Dead`),
/// plus the 40 challenge quest equipment loadouts and DLC section header flags.
///
/// Returns `Ok((newly_added_quests, total_installed_in_selected_categories))`.
#[cfg(feature = "gamedata")]
pub fn install_dlc_quests_by_category(
    save: &mut Save,
    include_official: bool,
    include_dead: bool,
) -> Result<(usize, usize), Error> {
    use crate::dlc_bundle::DlcQuestCategory;
    check_kind(save)?;
    let sec = section_start(save).ok_or(Error::OutOfRange {
        context: "save has no DLC section",
        value: 0,
    })?;

    // 1. Populate non-zero DLC section header slices without overwriting existing non-zero bytes
    for h in crate::dlc_bundle::OFFICIAL_HEADER_SLICES.iter() {
        let start = sec + h.rel_offset as usize;
        let src = crate::dlc_bundle::header_slice_bytes(h);
        if let Some(dst) = save.body_mut().get_mut(start..start + src.len()) {
            for (d, &s) in dst.iter_mut().zip(src.iter()) {
                if *d == 0 {
                    *d = s;
                }
            }
        }
    }

    // 2. Populate the 40 challenge quest equipment records if empty
    for eq in crate::dlc_bundle::OFFICIAL_EQUIPS.iter() {
        let slot = eq.slot as usize;
        if slot >= EQUIP_SLOTS {
            continue;
        }
        let base = sec + EQUIP_ARRAY_OFFSET + slot * EQUIP_SLOT_SIZE;
        let is_empty = save
            .body()
            .get(base..base + 8)
            .map(|b| read_i32(b, 0) == 0 || read_i32(b, 4) <= 0)
            .unwrap_or(false);
        if is_empty {
            let src = crate::dlc_bundle::equip_bytes(eq);
            if let Some(dst) = save.body_mut().get_mut(base..base + EQUIP_SLOT_SIZE) {
                dst.fill(0);
                let copy_len = src.len().min(EQUIP_SLOT_SIZE);
                dst[..copy_len].copy_from_slice(&src[..copy_len]);
            }
        }
        let _ = set_installed(save, eq.quest_id, true);
    }

    // 3. Install matching bundled quests
    let mut added = 0usize;
    let mut total_present = 0usize;

    for q in crate::dlc_bundle::OFFICIAL_QUESTS.iter() {
        let matches_cat = match q.category {
            DlcQuestCategory::Official => include_official,
            DlcQuestCategory::Dead => include_dead,
        };
        if !matches_cat {
            continue;
        }

        let arc_data = crate::dlc_bundle::arc_bytes(q);
        if let Some(existing_slot) = find_quest(save, q.quest_id) {
            let base = sec + QUEST_ARRAY_OFFSET + existing_slot * QUEST_SLOT_SIZE;
            let cur_size = save
                .body()
                .get(base + 4..base + 8)
                .map(|b| read_i32(b, 0))
                .unwrap_or(0);
            if cur_size <= 0 || cur_size as usize > QUEST_MAX_PAYLOAD {
                if let Some(dst) = save.body_mut().get_mut(base..base + QUEST_SLOT_SIZE) {
                    dst.fill(0);
                    dst[0..4].copy_from_slice(&q.quest_id.to_le_bytes());
                    dst[4..8].copy_from_slice(&(arc_data.len() as i32).to_le_bytes());
                    dst[8..8 + arc_data.len()].copy_from_slice(arc_data);
                }
                added += 1;
            }
            let _ = set_installed(save, q.quest_id, true);
            total_present += 1;
            continue;
        }

        let def_slot = q.default_slot as usize;
        let slot_is_free = |s: &Save, idx: usize| -> bool {
            if idx >= QUEST_SLOTS {
                return false;
            }
            let b = sec + QUEST_ARRAY_OFFSET + idx * QUEST_SLOT_SIZE;
            s.body()
                .get(b..b + 8)
                .map(|slice| read_i32(slice, 0) == 0 && read_i32(slice, 4) == 0)
                .unwrap_or(false)
        };

        let target_slot = if slot_is_free(save, def_slot) {
            Some(def_slot)
        } else {
            (0..QUEST_SLOTS).find(|&idx| slot_is_free(save, idx))
        };

        if let Some(slot) = target_slot {
            let base = sec + QUEST_ARRAY_OFFSET + slot * QUEST_SLOT_SIZE;
            if let Some(dst) = save.body_mut().get_mut(base..base + QUEST_SLOT_SIZE) {
                dst.fill(0);
                dst[0..4].copy_from_slice(&q.quest_id.to_le_bytes());
                dst[4..8].copy_from_slice(&(arc_data.len() as i32).to_le_bytes());
                dst[8..8 + arc_data.len()].copy_from_slice(arc_data);
            }
            let _ = set_installed(save, q.quest_id, true);
            added += 1;
            total_present += 1;
        }
    }

    Ok((added, total_present))
}

/// Remove a quest from `save` by its `quest_id`, clearing both its slot and its
/// catalogue bitmask bit (if present in `DLC_CATALOG`).
#[cfg(feature = "gamedata")]
pub fn remove_quest_by_id(save: &mut Save, quest_id: i32) -> Result<usize, Error> {
    check_kind(save)?;
    let slot = find_quest(save, quest_id).ok_or(Error::Parse("quest is not in save"))?;
    uninstall_quest(save, slot)?;
    let _ = set_installed(save, quest_id, false);
    Ok(slot)
}

/// Toggle a bundled quest in `save`:
/// - If already installed, removes it and returns `Ok((slot, false))`.
/// - If not installed, installs it into a free slot (and initializes DLC headers/equips)
///   and returns `Ok((slot, true))`.
#[cfg(feature = "gamedata")]
pub fn toggle_bundled_quest(save: &mut Save, quest_id: i32) -> Result<(usize, bool), Error> {
    check_kind(save)?;
    if let Some(slot) = find_quest(save, quest_id) {
        uninstall_quest(save, slot)?;
        let _ = set_installed(save, quest_id, false);
        return Ok((slot, false));
    }
    let meta = crate::dlc_bundle::find_official_quest(quest_id)
        .ok_or(Error::Parse("quest id not in bundled catalogue"))?;
    let sec = section_start(save).ok_or(Error::OutOfRange {
        context: "save has no DLC section",
        value: 0,
    })?;

    // Ensure DLC header slices and any matching challenge quest equipment are present
    for h in crate::dlc_bundle::OFFICIAL_HEADER_SLICES.iter() {
        let start = sec + h.rel_offset as usize;
        let src = crate::dlc_bundle::header_slice_bytes(h);
        if let Some(dst) = save.body_mut().get_mut(start..start + src.len()) {
            for (d, &s) in dst.iter_mut().zip(src.iter()) {
                if *d == 0 {
                    *d = s;
                }
            }
        }
    }
    for eq in crate::dlc_bundle::OFFICIAL_EQUIPS.iter() {
        if eq.quest_id == quest_id {
            let slot = eq.slot as usize;
            if slot < EQUIP_SLOTS {
                let base = sec + EQUIP_ARRAY_OFFSET + slot * EQUIP_SLOT_SIZE;
                let src = crate::dlc_bundle::equip_bytes(eq);
                if let Some(dst) = save.body_mut().get_mut(base..base + EQUIP_SLOT_SIZE) {
                    dst.fill(0);
                    let copy_len = src.len().min(EQUIP_SLOT_SIZE);
                    dst[..copy_len].copy_from_slice(&src[..copy_len]);
                }
            }
        }
    }

    let slot_is_free = |s: &Save, idx: usize| -> bool {
        if idx >= QUEST_SLOTS {
            return false;
        }
        let b = sec + QUEST_ARRAY_OFFSET + idx * QUEST_SLOT_SIZE;
        s.body()
            .get(b..b + 8)
            .map(|slice| read_i32(slice, 0) == 0 && read_i32(slice, 4) == 0)
            .unwrap_or(false)
    };
    let def_slot = meta.default_slot as usize;
    let target_slot = if slot_is_free(save, def_slot) {
        Some(def_slot)
    } else {
        (0..QUEST_SLOTS).find(|&idx| slot_is_free(save, idx))
    };
    let slot = target_slot.ok_or(Error::OutOfRange {
        context: "no free quest slot (max 205)",
        value: QUEST_SLOTS as u32,
    })?;

    let arc_data = crate::dlc_bundle::arc_bytes(meta);
    let base = sec + QUEST_ARRAY_OFFSET + slot * QUEST_SLOT_SIZE;
    if let Some(dst) = save.body_mut().get_mut(base..base + QUEST_SLOT_SIZE) {
        dst.fill(0);
        dst[0..4].copy_from_slice(&quest_id.to_le_bytes());
        dst[4..8].copy_from_slice(&(arc_data.len() as i32).to_le_bytes());
        dst[8..8 + arc_data.len()].copy_from_slice(arc_data);
    }
    let _ = set_installed(save, quest_id, true);
    Ok((slot, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::{empty_switch_save, Save, SaveKind};

    /// Put a record straight into a slot, the way the game's own downloader
    /// would. Tests need this because install_quest deliberately refuses to
    /// create a quest that is not already in the save.
    fn seed(save: &mut Save, slot: usize, id: i32, data: &[u8]) {
        let base = slot_extent(save, slot).unwrap();
        let body = save.body_mut();
        body[base..base + QUEST_SLOT_SIZE].fill(0);
        body[base..base + 4].copy_from_slice(&id.to_le_bytes());
        body[base + 4..base + 8].copy_from_slice(&(data.len() as i32).to_le_bytes());
        body[base + 8..base + 8 + data.len()].copy_from_slice(data);
    }

    #[test]
    fn arrays_end_at_first_slot() {
        // Read the slot base from the save instead of hard coding both sides,
        // so the two numbers actually have to agree.
        let s = empty_switch_save();
        let sec = s.dlc_section_offset() as usize;
        assert_eq!(
            sec + QUEST_ARRAY_OFFSET
                + QUEST_SLOTS * QUEST_SLOT_SIZE
                + EQUIP_SLOTS * EQUIP_SLOT_SIZE,
            s.slot_base(0).unwrap() as usize
        );
    }

    /// A payload that really is a quest archive: one .mib with the given id.
    /// install_quest now refuses anything else, so the tests need a real one.
    fn arc_for(id: u32) -> alloc::vec::Vec<u8> {
        let mut mib = alloc::vec![0u8; 249];
        mib[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
        let a = crate::arc::Arc {
            version: crate::arc::ARC_VERSION,
            header8: 0xFFFF_FFFE,
            entries: alloc::vec![crate::arc::ArcEntry {
                name: alloc::string::String::from("questData\\questData_x"),
                type_hash: 0x1BBF_D18E,
                data: mib,
                original: None,
                original_offset: None,
            }],
        };
        a.to_bytes().unwrap()
    }

    #[test]
    fn a_payload_that_is_not_an_archive_is_refused() {
        let mut s = empty_switch_save();
        seed(&mut s, 3, 1010165, &arc_for(1010165));
        let before = s.body().to_vec();
        // 777 bytes of 0xAB used to return Ok and destroy the installed quest.
        assert!(install_quest(&mut s, 1010165, &alloc::vec![0xABu8; 777]).is_err());
        assert!(install_quest(&mut s, 1010165, &[1u8; 64]).is_err());
        // A truncated archive must be refused too.
        let mut cut = arc_for(1010165);
        cut.truncate(cut.len() / 2);
        assert!(install_quest(&mut s, 1010165, &cut).is_err());
        assert_eq!(s.body(), &before[..], "a refused install must not write");
    }

    #[test]
    fn replacing_keeps_the_slot_the_id_and_the_mask() {
        let mut s = empty_switch_save();
        seed(&mut s, 3, 1010165, &arc_for(1010165));
        let mask_before = bitmask(&s).unwrap();

        let slot = install_quest(&mut s, 1010165, &arc_for(1010165)).unwrap();
        assert_eq!(slot, 3, "must reuse the slot that holds the quest");

        let list = list_quests(&s);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].quest_id, 1010165);
        assert_eq!(list[0].data, arc_for(1010165), "the payload must round trip");

        // The installed set is catalogue indexed, so a slot number cannot
        // address it: a replace must leave it byte for byte alone.
        assert_eq!(bitmask(&s).unwrap(), mask_before);
    }

    #[test]
    fn a_quest_that_is_not_installed_is_refused() {
        let mut s = empty_switch_save();
        let before = s.body().to_vec();
        assert!(install_quest(&mut s, 1010165, &[1u8; 64]).is_err());
        assert_eq!(s.body(), &before[..], "a refused install must not write");
        assert!(list_quests(&s).is_empty());
    }

    #[test]
    fn bad_input_is_rejected() {
        let mut s = empty_switch_save();
        let before = s.body().to_vec();
        assert!(install_quest(&mut s, 0, &[1u8; 64]).is_err(), "id 0");
        assert!(install_quest(&mut s, -5, &[1u8; 64]).is_err(), "negative id");
        assert!(
            install_quest(&mut s, 1010165, &alloc::vec![0u8; QUEST_MAX_PAYLOAD + 1]).is_err(),
            "oversized payload"
        );
        assert_eq!(s.body(), &before[..], "a refused install must not write");
    }

    #[test]
    fn a_corrupt_size_does_not_panic() {
        let mut s = empty_switch_save();
        seed(&mut s, 0, 1010165, &[1u8; 16]);
        let base = slot_extent(&s, 0).unwrap();
        s.body_mut()[base + 4..base + 8].copy_from_slice(&i32::MAX.to_le_bytes());

        let list = list_quests(&s);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].size, i32::MAX);
        assert!(list[0].data.is_empty(), "a bad size yields no payload");
        assert_eq!(find_quest(&s, 1010165), Some(0));
    }

    /// A section pointer that puts the record header inside the body but the
    /// payload outside it. An earlier version sliced blindly and panicked with
    /// "range end out of range for slice of length ...", which
    /// check::check_save reaches.
    #[test]
    fn a_payload_running_past_the_body_does_not_panic() {
        let mut s = empty_switch_save();
        let len = s.body().len();
        // Slot 0's header lands in the last 8 bytes of the body.
        let sec = len - 8 - QUEST_ARRAY_OFFSET;
        s.body_mut()[0x0C..0x10].copy_from_slice(&(sec as u32).to_le_bytes());
        let base = sec + QUEST_ARRAY_OFFSET;
        s.body_mut()[base..base + 4].copy_from_slice(&1010165i32.to_le_bytes());
        s.body_mut()[base + 4..base + 8].copy_from_slice(&0x13F8i32.to_le_bytes());

        // The section guard rejects a pointer that would put the array past
        // the character slots, so this reads as "no quests" rather than
        // panicking or reporting a record whose payload cannot be read.
        let list = list_quests(&s);
        assert!(list.is_empty(), "a section past the slots must be refused");
        assert!(uninstall_quest(&mut s, 0).is_err());
        assert!(install_quest(&mut s, 1010165, &[1u8; 64]).is_err());
    }

    #[test]
    fn a_corrupt_section_offset_does_not_panic() {
        let mut s = empty_switch_save();
        s.body_mut()[0x0C..0x10].copy_from_slice(&0x47C39Cu32.to_le_bytes());
        // The reviewer's pointer lands inside the body but nowhere near the
        // character slots, so the scan finds nothing and the writers refuse.
        assert!(list_quests(&s).is_empty());
        assert_eq!(find_quest(&s, 1010165), None);
        assert!(uninstall_quest(&mut s, 0).is_err());
        assert!(install_quest(&mut s, 1010165, &[1u8; 64]).is_err());
    }

    #[test]
    fn the_equipment_table_maps_a_challenge_quest_to_its_bit() {
        let mut s = empty_switch_save();
        let sec = s.dlc_section_offset() as usize;
        // Put a challenge quest id into equipment entry 3.
        let base = sec + EQUIP_ARRAY_OFFSET + 3 * EQUIP_SLOT_SIZE;
        s.body_mut()[base..base + 4].copy_from_slice(&1020004i32.to_le_bytes());
        assert_eq!(equipment_catalogue_index(&s, 1020004), Some(3));
        assert_eq!(equipment_catalogue_index(&s, 1010165), None, "event quests are not there");
        // Entry 3's bit is clear in a blank save, so the write is refused.
        assert_eq!(installed_flag(&s, 1020004), Some(false));
        seed(&mut s, 0, 1020004, &arc_for(1020004));
        assert!(install_quest(&mut s, 1020004, &arc_for(1020004)).is_err());
        // Set bit 3 and the same write goes through.
        set_test_bit(&mut s, 3, true);
        assert_eq!(installed_flag(&s, 1020004), Some(true));
        assert!(install_quest(&mut s, 1020004, &arc_for(1020004)).is_ok());
    }

    /// Test-only bit poke: the library itself deliberately never writes the set.
    fn set_test_bit(save: &mut Save, bit: usize, on: bool) {
        let sec = save.dlc_section_offset() as usize;
        let off = sec + BITMASK_OFFSET + bit / 8;
        let m = 1u8 << (bit % 8);
        if on {
            save.body_mut()[off] |= m;
        } else {
            save.body_mut()[off] &= !m;
        }
    }

    /// A section pointer that lands on the equipment box used to let
    /// uninstall_quest zero 5120 bytes of someone's gear. The guard must refuse
    /// any section that does not end at or before the first character slot.
    #[test]
    fn a_section_pointer_inside_the_character_area_is_refused() {
        let mut s = empty_switch_save();
        let first = s.slot_base(0).unwrap();
        // Past the first character slot, the way a corrupt save can be.
        let bad = first + 0x2000;
        s.body_mut()[0x0C..0x10].copy_from_slice(&bad.to_le_bytes());

        assert!(list_quests(&s).is_empty(), "no quests may be read");
        assert!(bitmask(&s).is_none());
        assert_eq!(find_quest(&s, 1010165), None);
        assert!(uninstall_quest(&mut s, 0).is_err(), "must refuse to write");
        assert!(install_quest(&mut s, 1010165, &[1u8; 64]).is_err());

        // Only the four bytes we poked may differ from a fresh template.
        let before = empty_switch_save();
        let diff = (0..before.body().len())
            .filter(|i| before.body()[*i] != s.body()[*i])
            .count();
        assert!(diff <= 4, "a refused operation must not write ({} bytes)", diff);
    }

    #[test]
    fn slot_extent_rejects_out_of_range_slots() {
        let s = empty_switch_save();
        assert!(slot_extent(&s, QUEST_SLOTS).is_err());
        assert!(slot_extent(&s, usize::MAX).is_err());
        assert!(slot_extent(&s, 0).is_ok());
    }

    #[test]
    fn the_empty_template_is_mhxx() {
        let s = empty_switch_save();
        assert!(check_kind(&s).is_ok());
        assert!(matches!(s.kind(), SaveKind::Switch | SaveKind::ThreeDs));
    }

    #[test]
    #[cfg(feature = "gamedata")]
    fn official_dlc_bundle_installs_all_189_quests_and_40_equips_into_empty_save() {
        let mut s = empty_switch_save();
        assert_eq!(list_quests(&s).len(), 0);
        assert_eq!(installed_bit_count(&s), 0);

        let (added, total) = install_official_dlc_quests(&mut s).expect("install official dlc");
        assert_eq!(added, 189);
        assert_eq!(total, 189);
        assert_eq!(list_quests(&s).len(), 189);
        assert_eq!(installed_bit_count(&s), 189);

        let eqs = quest_equipment(&s);
        let occupied_eqs = eqs.iter().filter(|e| e.occupied).count();
        assert_eq!(occupied_eqs, 40);

        // Calling a second time is idempotent and adds 0 new quests
        let (added2, total2) = install_official_dlc_quests(&mut s).expect("second install");
        assert_eq!(added2, 0);
        assert_eq!(total2, 189);
    }
}