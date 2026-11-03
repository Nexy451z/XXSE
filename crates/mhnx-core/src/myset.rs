//! Item sets and equipment sets.
//!
//! Two unrelated arrays. Item sets live at slot + 0x17CF, twenty-four records
//! of 170 bytes: a 32 byte name, ten reserved bytes, then thirty-two pairs of
//! a u16 item id and a u16 count. 32 is the pouch size.
//!
//! Equipment sets live at slot + 0x208CE, forty records of 136 bytes: a 32 byte
//! name, ten reserved bytes, seven u16 indices into the equipment box (weapon,
//! head, body, arms, waist, legs, talisman), seven decoration triples, two
//! unclaimed bytes, five armour colours and sixteen bytes that hold the style
//! and the hunter arts.
//!
//! The decoration triples at +0x38 are a second copy of what sits in the
//! equipment records the parts point at (record +0x06, +0x08, +0x0A). The game
//! keeps both places in step when a set is registered; this module writes both
//! in one go (`set_part`) because writing only the index leaves the set
//! describing decorations the equipment does not have. The sample save already
//! disagrees in places, which is what `warnings` reports.
//!
//! The last sixteen bytes are not fully understood. Five are always zero, five
//! hold 01 in a record the game has never used, +0x82 holds the hunting style
//! and +0x83..+0x86 the three hunter art ids: for the set that mirrors the
//! equipment the character is wearing those last four agree with the style and
//! arts the character carries at +0x23B4C and +0x2C. The remaining two bytes
//! are zero everywhere in the sample. See `EquipSet::tail`.
//!
//! An unused record has the name '－－－', seven NONE parts and the tail
//! pattern in `EMPTY_TAIL`.

use alloc::format;
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
/// Offset of the decoration copy, three u16 per part.
pub const EQUIP_SET_DECOS: usize = 0x38;
/// Bytes one part's decoration copy takes.
pub const EQUIP_SET_DECO_SIZE: usize = 6;
/// Offset of the armour colours.
pub const EQUIP_SET_COLOURS: usize = 0x64;
/// Offset of the sixteen bytes after the colours.
pub const EQUIP_SET_TAIL: usize = 0x78;
/// Bytes in the tail.
pub const EQUIP_SET_TAIL_SIZE: usize = 16;
/// Offset of the hunting style inside the tail.
pub const EQUIP_SET_STYLE: usize = 0x82;
/// Offset of the three hunter art ids inside the tail.
pub const EQUIP_SET_ARTS: usize = 0x83;
/// Offset of the two bytes after the arts inside the tail.
pub const EQUIP_SET_SP: usize = 0x86;
/// A slot holding nothing.
pub const NONE: u16 = 0xFFFF;
/// The name an unused record carries.
pub const EMPTY_NAME: &str = "－－－";
/// Part names, in the order a set stores them.
pub const PART_NAMES: [&str; EQUIP_SET_PARTS] = ["武器", "頭", "胴", "腕", "腰", "脚", "護石"];
/// The tail an unused record carries on a character the game has used.
///
/// Verified byte for byte in three real saves (the stable dump, a Ryujinx dump
/// and a backup): every record the game has never filled holds exactly these
/// bytes. What the five ones mean is not known; a character the game has never
/// used has an all zero record instead. Writing this pattern is what makes an
/// emptied set look like one the game itself left alone.
pub const EMPTY_TAIL: [u8; EQUIP_SET_TAIL_SIZE] = [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 6, 0, 0, 0, 0, 0];

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
    /// The decoration ids the set carries for each part, three sockets each.
    ///
    /// This is the copy at +0x38, not a read of the equipment box: a set that
    /// was registered long ago can name decorations the box record no longer
    /// has, and that is exactly what `warnings` reports.
    pub decos: [[u16; 3]; EQUIP_SET_PARTS],
    /// Five armour colours, RGBA, in part order head, body, arms, waist, legs.
    pub colours: [[u8; 4]; 5],
    /// The sixteen bytes at +0x78, as stored.
    ///
    /// Split as the record appears to lay them out: `modes` (five bytes),
    /// `own` (five), `style` (one) and `arts` (three) plus two bytes nobody
    /// has explained. Only the style and the arts are backed by evidence; see
    /// the accessors.
    pub tail: [u8; EQUIP_SET_TAIL_SIZE],
}

impl EquipSet {
    /// Is this slot unused?
    pub fn is_empty(&self) -> bool {
        self.name == EMPTY_NAME || self.parts.iter().all(|p| *p == NONE)
    }

    /// A slice of the tail, copied out.
    fn tail_slice<const N: usize>(&self, start: usize) -> [u8; N] {
        let mut out = [0u8; N];
        out.copy_from_slice(&self.tail[start..start + N]);
        out
    }

    /// The five bytes at +0x78..+0x7D.
    ///
    /// Always zero in the sample save, used or not. The name is the one the
    /// field layout another reverse-engineering attempt read into the record;
    /// nothing here depends on it.
    pub fn modes(&self) -> [u8; 5] {
        self.tail_slice(0)
    }

    /// The five bytes at +0x7D..+0x82.
    ///
    /// Unknown. The game writes 01 in all five for a record it has never used
    /// and 00 in every record that holds equipment, which is the whole of what
    /// is known about them.
    pub fn own(&self) -> [u8; 5] {
        self.tail_slice(5)
    }

    /// The hunting style id at +0x82.
    ///
    /// A record the game left unused carries 06, which is not a style the
    /// game accepts; for a registered set this is the style to load with it.
    /// The set that mirrors the worn equipment agrees with the character's own
    /// style byte at +0x23B4C, which is the evidence for this offset.
    pub fn style(&self) -> u8 {
        self.tail[EQUIP_SET_STYLE - EQUIP_SET_TAIL]
    }

    /// The three hunter art ids at +0x83..+0x86, in slot order.
    ///
    /// One byte each, unlike the u16 copies the character carries. The set
    /// that mirrors the worn equipment agrees with the character's arts.
    pub fn arts(&self) -> [u8; 3] {
        let at = EQUIP_SET_ARTS - EQUIP_SET_TAIL;
        [self.tail[at], self.tail[at + 1], self.tail[at + 2]]
    }

    /// The two bytes at +0x86..+0x88, unexplained and zero in the sample.
    pub fn sp(&self) -> [u8; 2] {
        self.tail_slice(EQUIP_SET_SP - EQUIP_SET_TAIL)
    }

    /// The decoration copy of one part, when the part exists.
    pub fn decos_of(&self, part: usize) -> Option<[u16; 3]> {
        self.decos.get(part).copied()
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

/// Decode one 136 byte record.
fn decode_set(index: usize, rec: &[u8]) -> EquipSet {
    let mut parts = [NONE; EQUIP_SET_PARTS];
    for (i, p) in parts.iter_mut().enumerate() {
        *p = u16_at(rec, EQUIP_SET_BODY + i * 2);
    }
    let mut decos = [[0u16; 3]; EQUIP_SET_PARTS];
    for (part, sockets) in decos.iter_mut().enumerate() {
        let o = EQUIP_SET_DECOS + part * EQUIP_SET_DECO_SIZE;
        for (socket, id) in sockets.iter_mut().enumerate() {
            *id = u16_at(rec, o + socket * 2);
        }
    }
    let mut colours = [[0u8; 4]; 5];
    for (i, c) in colours.iter_mut().enumerate() {
        let o = EQUIP_SET_COLOURS + i * 4;
        c.copy_from_slice(&rec[o..o + 4]);
    }
    let mut tail = [0u8; EQUIP_SET_TAIL_SIZE];
    tail.copy_from_slice(&rec[EQUIP_SET_TAIL..EQUIP_SET_TAIL + EQUIP_SET_TAIL_SIZE]);
    EquipSet {
        index,
        name: read_name(rec),
        parts,
        decos,
        colours,
        tail,
    }
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
        out.push(decode_set(index, rec));
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

/// Rename an equipment set.
///
/// The same write as [`set_equip_set_name`], under the shorter name the set
/// API uses. The name goes through [`crate::write_name_field`], which cuts a
/// long or multi-byte name on a character boundary and leaves a terminator, so
/// a name a user typed never ends up split in half.
pub fn set_name(save: &mut Save, slot: usize, set: usize, name: &str) -> bool {
    set_equip_set_name(save, slot, set, name)
}

// ---------------------------------------------------------------------------
// Equipment set operations
//
// Every write locates one record first, so a refused edit cannot leave half a
// part behind. A part moves in two places at once: the index at +0x2A and the
// decoration copy at +0x38. Writing only the index is what makes a set and the
// equipment box disagree about decorations.
// ---------------------------------------------------------------------------

fn record_range(save: &Save, slot: usize, set: usize) -> Option<core::ops::Range<usize>> {
    if set >= EQUIP_SETS {
        return None;
    }
    let base = save.slot_base(slot)? as usize;
    let at = base + OFF_EQUIP_SETS + set * EQUIP_SET_SIZE;
    if at.checked_add(EQUIP_SET_SIZE)? > save.body().len() {
        return None;
    }
    Some(at..at + EQUIP_SET_SIZE)
}

fn record(save: &Save, slot: usize, set: usize) -> Option<&[u8]> {
    let r = record_range(save, slot, set)?;
    save.body().get(r)
}

fn record_mut<'a>(save: &'a mut Save, slot: usize, set: usize) -> Option<&'a mut [u8]> {
    let r = record_range(save, slot, set)?;
    save.body_mut().get_mut(r)
}

/// The decoration ids of one equipment box record.
fn equipment_decos(save: &Save, slot: usize, index: usize) -> Option<[u16; 3]> {
    let box_bytes = save.equipment_box(slot)?;
    let o = index.checked_mul(crate::equip::EQUIP_RECORD)?;
    let rec = box_bytes.get(o..o + crate::equip::EQUIP_RECORD)?;
    Some(crate::equip::Equipment::decorations(index, rec))
}

/// Does an equipment kind belong in this part?
///
/// The weapon slot takes any weapon (7..=21; 12 is the hole the game leaves in
/// its own numbering), parts 1..=5 take the armour kind with the same number,
/// and the talisman slot takes kind 6.
fn kind_fits(part: usize, kind: u8) -> bool {
    match part {
        0 => matches!(kind, 7..=21) && kind != 12,
        1..=5 => kind as usize == part,
        6 => kind == 6,
        _ => false,
    }
}

/// Point one part at an equipment box record and copy its decorations.
///
/// `part` runs 0..7 in the order the record stores them (weapon, head, body,
/// arms, waist, legs, talisman). Passing [`NONE`] clears the part, exactly like
/// [`clear_part`]; an index outside the box, or one that points at an empty
/// record, is refused, because a set that names nothing the box can resolve is
/// not something the game writes.
///
/// The equipment kind is deliberately **not** checked. The sample save holds a
/// set with a talisman in the head slot and the game loads it, so a mismatch is
/// reported by [`warnings`] rather than refused here. What is copied is the
/// decoration ids at record +0x06..+0x0C, which is what makes the two copies
/// agree again even when they disagreed before.
pub fn set_part(save: &mut Save, slot: usize, set: usize, part: usize, box_index: u16) -> bool {
    if set >= EQUIP_SETS || part >= EQUIP_SET_PARTS {
        return false;
    }
    if box_index == NONE {
        return clear_part(save, slot, set, part);
    }
    if box_index as usize >= crate::save::TOTAL_EQUIP_SLOTS {
        return false;
    }
    let Some(eq) = save.equipment_at(slot, box_index as usize) else {
        return false;
    };
    if eq.is_empty() {
        return false;
    }
    let decos = equipment_decos(save, slot, box_index as usize).unwrap_or([0; 3]);
    let Some(rec) = record_mut(save, slot, set) else {
        return false;
    };
    let at = EQUIP_SET_BODY + part * 2;
    rec[at..at + 2].copy_from_slice(&box_index.to_le_bytes());
    let dat = EQUIP_SET_DECOS + part * EQUIP_SET_DECO_SIZE;
    for (socket, id) in decos.iter().enumerate() {
        let o = dat + socket * 2;
        rec[o..o + 2].copy_from_slice(&id.to_le_bytes());
    }
    true
}

/// Empty one part: the index becomes [`NONE`] and its decoration copy zero.
///
/// Nothing else in the record moves, so the other six parts keep the equipment
/// and decorations they name.
pub fn clear_part(save: &mut Save, slot: usize, set: usize, part: usize) -> bool {
    if set >= EQUIP_SETS || part >= EQUIP_SET_PARTS {
        return false;
    }
    let Some(rec) = record_mut(save, slot, set) else {
        return false;
    };
    let at = EQUIP_SET_BODY + part * 2;
    rec[at..at + 2].copy_from_slice(&NONE.to_le_bytes());
    let dat = EQUIP_SET_DECOS + part * EQUIP_SET_DECO_SIZE;
    rec[dat..dat + EQUIP_SET_DECO_SIZE].fill(0);
    true
}

/// Set one armour part's colour.
///
/// `part` uses the same numbering as the rest of the module, so 1..=5 (head,
/// body, arms, waist, legs); the weapon and the talisman have no colour of
/// their own and are refused. The four bytes go in as stored; the game writes
/// alpha 0xFF and this module leaves whatever the caller passes alone.
pub fn set_colour(save: &mut Save, slot: usize, set: usize, part: usize, colour: [u8; 4]) -> bool {
    if set >= EQUIP_SETS || !(1..=5).contains(&part) {
        return false;
    }
    let Some(rec) = record_mut(save, slot, set) else {
        return false;
    };
    let at = EQUIP_SET_COLOURS + (part - 1) * 4;
    rec[at..at + 4].copy_from_slice(&colour);
    true
}

/// Empty an equipment set, the way the game leaves a record it never used.
///
/// The name becomes '－－－', the parts [`NONE`], the decoration copies and
/// colours zero, and the tail [`EMPTY_TAIL`]; a record emptied this way is byte
/// for byte one of the records the game itself leaves behind. **The set's
/// contents are gone after this** and nothing here asks first: the caller owns
/// the confirmation.
pub fn clear_set(save: &mut Save, slot: usize, set: usize) -> bool {
    let Some(rec) = record_mut(save, slot, set) else {
        return false;
    };
    rec.fill(0);
    write_name(&mut rec[..32], EMPTY_NAME);
    for part in 0..EQUIP_SET_PARTS {
        let o = EQUIP_SET_BODY + part * 2;
        rec[o..o + 2].copy_from_slice(&NONE.to_le_bytes());
    }
    rec[EQUIP_SET_TAIL..EQUIP_SET_TAIL + EQUIP_SET_TAIL_SIZE].copy_from_slice(&EMPTY_TAIL);
    true
}

/// Everything about a set that looks wrong, as text for the user.
///
/// Only reports; none of this is repaired, because the game accepts all of it
/// and the real save already contains examples of each:
///
/// * a part that names an equipment kind the part cannot wear (the sample has
///   a talisman in the head slot);
/// * a part that names an empty equipment record or something outside the box;
/// * a decoration copy that no longer agrees with the equipment record it
///   belongs to;
/// * decorations left behind on a part that holds nothing.
///
/// A record the game left unused produces no warnings, and an out of range set
/// index produces none either.
pub fn warnings(save: &Save, slot: usize, set: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(rec) = record(save, slot, set) else {
        return out;
    };
    let parsed = decode_set(set, rec);
    if parsed.name == EMPTY_NAME && parsed.parts.iter().any(|p| *p != NONE) {
        out.push(String::from("名前が '－－－' のまま中身が入っています"));
    }
    for part in 0..EQUIP_SET_PARTS {
        let name = PART_NAMES[part];
        let index = parsed.parts[part];
        if index == NONE {
            if parsed.decos[part].iter().any(|d| *d != 0) {
                out.push(format!("{}: 装備が無いのに装飾品が残っています", name));
            }
            continue;
        }
        if index as usize >= crate::save::TOTAL_EQUIP_SLOTS {
            out.push(format!("{}: 装備枠の外を指しています（{}）", name, index));
            continue;
        }
        let Some(eq) = save.equipment_at(slot, index as usize) else {
            out.push(format!("{}: 装備を読み取れません（{}）", name, index));
            continue;
        };
        if eq.is_empty() {
            out.push(format!("{}: 空の装備枠を指しています（{}）", name, index));
        } else if !kind_fits(part, eq.kind) {
            out.push(format!(
                "{}: {}（種別 {}）が入っています",
                name,
                crate::equip::type_name(eq.kind),
                eq.kind
            ));
        }
        if let Some(box_decos) = equipment_decos(save, slot, index as usize) {
            if parsed.decos[part] != box_decos {
                out.push(format!(
                    "{}: 装飾品が装備と一致しません（セット {:?} / 装備 {:?}）",
                    name, parsed.decos[part], box_decos
                ));
            }
        }
    }
    out
}