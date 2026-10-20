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
}