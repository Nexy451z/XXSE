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
//!
//! The item sets end at slot + 0x27BF, where the pouch begins: thirty-two more
//! (id, count) pairs, packed nineteen bits each into 76 bytes. A set is a
//! snapshot of the pouch, so the two are read and written with the same pair
//! type and the same codec as the item box. See `OFF_POUCH` for the evidence.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::save::{check_name, NameError, NameFit, NameUnit, Save};

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
/// Bytes the name field of a my-set record occupies.
///
/// Both arrays use the same 32 byte UTF-8 field in front of the ten reserved
/// bytes at +0x20, so a name may hold a line break the game draws on a second
/// line and fills at most [@@NAME_CAPACITY@@] of them.
pub const NAME_LEN: usize = 32;
/// Bytes of a my-set name the field can hold: one short, for the NUL.
pub const NAME_CAPACITY: usize = NAME_LEN - 1;

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

    /// The set name split into the lines the screen draws.
    pub fn name_lines(&self) -> Vec<&str> {
        name_lines(&self.name)
    }

    /// How many lines the set name takes on screen.
    pub fn name_line_count(&self) -> usize {
        name_line_count(&self.name)
    }

    /// What writing this name would do to the record's field.
    pub fn name_fit(&self) -> Result<NameFit, NameError> {
        name_check(&self.name)
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

    /// The set name split into the lines the screen draws.
    pub fn name_lines(&self) -> Vec<&str> {
        name_lines(&self.name)
    }

    /// How many lines the set name takes on screen.
    pub fn name_line_count(&self) -> usize {
        name_line_count(&self.name)
    }

    /// What writing this name would do to the record's field.
    pub fn name_fit(&self) -> Result<NameFit, NameError> {
        name_check(&self.name)
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
    let raw = &b[..NAME_LEN.min(b.len())];
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

/// Check a my-set name against a record's field, without writing.
///
/// Item sets and equipment sets share the 32 byte name field, so both go
/// through this. A line break is allowed; any other control character is
/// refused with [`NameError::Control`].
pub fn name_check(name: &str) -> Result<NameFit, NameError> {
    check_name(name, NAME_CAPACITY, NameUnit::Bytes)
}

/// The lines a my-set name holds, as the screen draws them.
pub fn name_lines(name: &str) -> Vec<&str> {
    crate::save::name_lines(name)
}

/// How many lines a my-set name holds.
pub fn name_line_count(name: &str) -> usize {
    crate::save::name_line_count(name)
}

/// Rename an item set, refusing control characters.
///
/// The checked form of [`set_item_set_name`]: nothing is written when the name
/// does not pass [`name_check`]. Line breaks are kept as they are and their
/// warnings (leading, trailing, doubled) come back with the fit.
pub fn set_item_set_name_checked(
    save: &mut Save,
    slot: usize,
    index: usize,
    name: &str,
) -> Result<NameFit, NameError> {
    let fit = name_check(name)?;
    if !set_item_set_name(save, slot, index, name) {
        return Err(NameError::NoField);
    }
    Ok(fit)
}

/// Rename an item set.
///
/// A line break is written as it stands; any other control character is
/// refused, so false means the record is untouched. [`set_item_set_name_checked`]
/// reports the same write with the warnings a caller may want to show.
pub fn set_item_set_name(save: &mut Save, slot: usize, index: usize, name: &str) -> bool {
    if index >= ITEM_SETS || name_check(name).is_err() {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ITEM_SETS + index * ITEM_SET_SIZE;
    let Some(dst) = save.body_mut().get_mut(at..at + NAME_LEN) else {
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

// ---------------------------------------------------------------------------
// Item set operations
//
// An item set is a snapshot of the pouch: the same thirty-two (id, count)
// pairs a character carries into a quest. The two arrays sit next to each
// other in the slot, sets first and the pouch second, so the pouch offset is
// fixed by arithmetic on constants that were already here rather than by a
// number taken from another editor. See [`OFF_POUCH`].
// ---------------------------------------------------------------------------

/// Offset of the pouch, relative to the slot base.
///
/// Three measurements in the sample save close on this byte:
///
/// * the item set array ends here: `0x17CF + 24 * 170 == 0x27BF`;
/// * the field is thirty-two nineteen bit records, 608 bits, 76 whole bytes;
/// * the next field this library knows, [`crate::save::OFF_HR_POINTS`], starts
///   at `0x280B`, the byte after `0x27BF + 76`.
///
/// Decoding those 76 bytes with the same nineteen bit codec the item box uses
/// yields a believable pouch: 23 used pairs, every id inside the item table and
/// every count at or below 99, and 22 of them identical, count included, to the
/// entries of one of the two item sets the save holds. A set is registered from
/// the pouch, so that agreement is the evidence for the offset.
pub const OFF_POUCH: usize = 0x27BF;
/// Slots in the pouch; the same number an item set holds.
pub const POUCH_SLOTS: usize = ITEM_SET_ITEMS;
/// Bytes the pouch takes: `32 * 19` bits, which is exactly 76 bytes.
pub const POUCH_SIZE: usize = POUCH_SLOTS * ITEM_BITS / 8;

/// Bits one pouch record takes.
const ITEM_BITS: usize = 19;
/// Bits of the id inside a pouch record; the count takes the rest.
const ITEM_ID_BITS: usize = 12;

/// Does a pair describe something the game could have written?
///
/// The id runs 0..=[`crate::gamedata::LAST_ITEM_ID`], the count stops at
/// [`crate::itembox::MAX_COUNT`], and an empty record (id 0) carries no count:
/// the count of an unused record is not read, so a set written here never
/// leaves a stray one behind.
fn pair_is_sane(id: u16, count: u16) -> bool {
    if count > crate::itembox::MAX_COUNT {
        return false;
    }
    if id as usize > crate::gamedata::LAST_ITEM_ID {
        return false;
    }
    id != 0 || count == 0
}

fn item_set_field(save: &Save, slot: usize, set: usize) -> Option<core::ops::Range<usize>> {
    if set >= ITEM_SETS {
        return None;
    }
    let base = save.slot_base(slot)? as usize;
    let at = base
        .checked_add(OFF_ITEM_SETS)?
        .checked_add(set.checked_mul(ITEM_SET_SIZE)?)?;
    if at.checked_add(ITEM_SET_SIZE)? > save.body().len() {
        return None;
    }
    Some(at..at + ITEM_SET_SIZE)
}

fn item_set_bytes(save: &Save, slot: usize, set: usize) -> Option<&[u8]> {
    save.body().get(item_set_field(save, slot, set)?)
}

fn item_set_bytes_mut<'a>(
    save: &'a mut Save,
    slot: usize,
    set: usize,
) -> Option<&'a mut [u8]> {
    let r = item_set_field(save, slot, set)?;
    save.body_mut().get_mut(r)
}

/// One entry of an item set record.
fn read_set_item(rec: &[u8], index: usize) -> SetItem {
    let o = ITEM_SET_BODY + index * 4;
    SetItem {
        id: u16_at(rec, o),
        count: u16_at(rec, o + 2),
    }
}

fn write_set_item(rec: &mut [u8], index: usize, item: SetItem) {
    let o = ITEM_SET_BODY + index * 4;
    rec[o..o + 2].copy_from_slice(&item.id.to_le_bytes());
    rec[o + 2..o + 4].copy_from_slice(&item.count.to_le_bytes());
}

/// Write one entry of an item set.
///
/// `index` runs 0..[`ITEM_SET_ITEMS`]. The write is refused, and nothing at all
/// is changed, when the set index, the entry index, the id or the count is out
/// of range; the count may not exceed [`crate::itembox::MAX_COUNT`] and an empty
/// entry (id 0) may not carry a count. Writing id 0 with count 0 clears the
/// entry, which is what [`clear_item`] does.
///
/// Only the four bytes of that one entry move; the name, the other entries and
/// the neighbouring records keep what they had.
pub fn set_item(
    save: &mut Save,
    slot: usize,
    set: usize,
    index: usize,
    id: u16,
    count: u16,
) -> bool {
    if index >= ITEM_SET_ITEMS || !pair_is_sane(id, count) {
        return false;
    }
    let Some(rec) = item_set_bytes_mut(save, slot, set) else {
        return false;
    };
    write_set_item(rec, index, SetItem { id, count });
    true
}

/// Empty one entry of an item set: the id and the count both become zero.
///
/// The rest of the record keeps what it had. An entry that was already empty
/// is left byte for byte as it was.
pub fn clear_item(save: &mut Save, slot: usize, set: usize, index: usize) -> bool {
    if index >= ITEM_SET_ITEMS {
        return false;
    }
    let Some(rec) = item_set_bytes_mut(save, slot, set) else {
        return false;
    };
    write_set_item(rec, index, SetItem { id: 0, count: 0 });
    true
}

fn pouch_field(save: &Save, slot: usize) -> Option<core::ops::Range<usize>> {
    let base = save.slot_base(slot)? as usize;
    let at = base.checked_add(OFF_POUCH)?;
    if at.checked_add(POUCH_SIZE)? > save.body().len() {
        return None;
    }
    Some(at..at + POUCH_SIZE)
}

fn pouch_bytes(save: &Save, slot: usize) -> Option<&[u8]> {
    save.body().get(pouch_field(save, slot)?)
}

fn pouch_bytes_mut<'a>(save: &'a mut Save, slot: usize) -> Option<&'a mut [u8]> {
    let r = pouch_field(save, slot)?;
    save.body_mut().get_mut(r)
}

/// One entry of the pouch field.
///
/// The bit address is `slot * 19` inside the 76 byte block, read low bit
/// first: the twelve id bits come first, then the seven count bits. This is the
/// same codec the item box uses; decoding the box with it reproduces
/// [`crate::itembox::decode`] byte for byte on the sample save, which is one of
/// the reasons the pouch is read this way.
fn read_pouch_item(raw: &[u8], index: usize) -> SetItem {
    let bit = index * ITEM_BITS;
    let mut id = 0u16;
    for k in 0..ITEM_ID_BITS {
        let b = bit + k;
        if (raw[b / 8] >> (b % 8)) & 1 == 1 {
            id |= 1u16 << k;
        }
    }
    let mut count = 0u16;
    for k in ITEM_ID_BITS..ITEM_BITS {
        let b = bit + k;
        if (raw[b / 8] >> (b % 8)) & 1 == 1 {
            count |= 1u16 << (k - ITEM_ID_BITS);
        }
    }
    SetItem { id, count }
}

fn write_pouch_item(raw: &mut [u8], index: usize, item: SetItem) {
    let bit = index * ITEM_BITS;
    let value = (item.id as u32) | ((item.count as u32) << ITEM_ID_BITS);
    for k in 0..ITEM_BITS {
        let b = bit + k;
        let mask = 1u8 << (b % 8);
        if (value >> k) & 1 == 1 {
            raw[b / 8] |= mask;
        } else {
            raw[b / 8] &= !mask;
        }
    }
}

/// What the pouch holds, in slot order.
///
/// The result is empty when the slot has no base or the field does not fit the
/// save body; otherwise it always has [`POUCH_SLOTS`] entries, with id 0 for an
/// empty one.
///
/// Decoding this field is a read of the save, not of an item set: what a set
/// holds is a snapshot taken when it was registered, and the character may have
/// changed the pouch since.
pub fn pouch(save: &Save, slot: usize) -> Vec<SetItem> {
    let mut out = Vec::with_capacity(POUCH_SLOTS);
    let Some(raw) = pouch_bytes(save, slot) else {
        return out;
    };
    for index in 0..POUCH_SLOTS {
        out.push(read_pouch_item(raw, index));
    }
    out
}

/// Write one pouch entry.
///
/// The same range checks as [`set_item`] apply. Only the bits of that one
/// nineteen bit record move; the rest of the 76 bytes, and the hunter rank
/// points that follow them, keep what they had.
pub fn set_pouch_item(
    save: &mut Save,
    slot: usize,
    index: usize,
    id: u16,
    count: u16,
) -> bool {
    if index >= POUCH_SLOTS || !pair_is_sane(id, count) {
        return false;
    }
    let Some(raw) = pouch_bytes_mut(save, slot) else {
        return false;
    };
    write_pouch_item(raw, index, SetItem { id, count });
    true
}

/// Copy the pouch into an item set, returning how many entries hold something.
///
/// All thirty-two entries are written, including the empty ones, so the set is
/// an exact snapshot of what the character carries. The set's name is not
/// touched. An out of range set index, or a slot whose pouch cannot be read,
/// returns 0 and writes nothing.
///
/// **An existing set is overwritten** and nothing here asks first: the caller
/// owns the confirmation.
pub fn copy_from_pouch(save: &mut Save, slot: usize, set: usize) -> usize {
    let items = pouch(save, slot);
    if items.len() != POUCH_SLOTS {
        return 0;
    }
    let Some(rec) = item_set_bytes_mut(save, slot, set) else {
        return 0;
    };
    let mut used = 0;
    for (index, item) in items.iter().enumerate() {
        write_set_item(rec, index, *item);
        if item.id != 0 {
            used += 1;
        }
    }
    used
}

/// Copy an item set into the pouch, returning how many entries hold something.
///
/// **The pouch is replaced**, and the caller owns the confirmation. Nothing
/// else in the slot moves: the set keeps what it had.
///
/// A record the game itself left unused - its name is [`EMPTY_NAME`] and every
/// entry is empty - is refused and the pouch keeps what it had, because loading
/// it would quietly empty what the character carries. A record that holds items
/// is applied even if it was never named: [`copy_from_pouch`] fills entries and
/// leaves the name as it found it, and the caller may apply that snapshot
/// before naming it.
pub fn apply_to_pouch(save: &mut Save, slot: usize, set: usize) -> usize {
    let items = {
        let Some(rec) = item_set_bytes(save, slot, set) else {
            return 0;
        };
        let mut items = [SetItem { id: 0, count: 0 }; POUCH_SLOTS];
        for (index, item) in items.iter_mut().enumerate() {
            *item = read_set_item(rec, index);
        }
        let unused = read_name(rec) == EMPTY_NAME
            && items.iter().all(|item| item.id == 0 && item.count == 0);
        if unused {
            return 0;
        }
        items
    };
    let used = items.iter().filter(|item| item.id != 0).count();
    let Some(raw) = pouch_bytes_mut(save, slot) else {
        return 0;
    };
    for (index, item) in items.iter().enumerate() {
        write_pouch_item(raw, index, *item);
    }
    used
}

/// Everything about an item set that looks wrong, as text for the user.
///
/// Only reports; none of this is repaired, because a save that came from the
/// game (or from a hand edit) can hold each of them:
///
/// * an empty entry (id 0) that still carries a count - the count of an unused
///   entry is not read, so only a broken write leaves one;
/// * the same item id in more than one entry;
/// * a count above [`crate::itembox::MAX_COUNT`];
/// * an id above the item table;
/// * a used entry whose count is zero;
/// * a record still named '－－－' that holds items.
///
/// A record the game left unused produces no warnings, and an out of range set
/// index produces none either.
pub fn item_set_warnings(save: &Save, slot: usize, set: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(rec) = item_set_bytes(save, slot, set) else {
        return out;
    };
    let mut items = [SetItem { id: 0, count: 0 }; POUCH_SLOTS];
    for (index, item) in items.iter_mut().enumerate() {
        *item = read_set_item(rec, index);
    }

    // The record the game never used, in the state the game leaves it.
    let untouched = items.iter().all(|item| item.id == 0 && item.count == 0);
    if read_name(rec) == EMPTY_NAME && untouched {
        return out;
    }
    if read_name(rec) == EMPTY_NAME {
        out.push(String::from("名前が '－－－' のまま中身が入っています"));
    }

    let mut seen: Vec<u16> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if item.id == 0 {
            if item.count != 0 {
                out.push(format!(
                    "{}: 空きなのに個数だけ入っています（{}）",
                    index, item.count
                ));
            }
            continue;
        }
        if item.count == 0 {
            out.push(format!("{}: アイテムはあるのに個数が 0 です", index));
        }
        if item.count > crate::itembox::MAX_COUNT {
            out.push(format!(
                "{}: 個数が上限 {} を超えています（{}）",
                index,
                crate::itembox::MAX_COUNT,
                item.count
            ));
        }
        if item.id as usize > crate::gamedata::LAST_ITEM_ID {
            out.push(format!(
                "{}: アイテムIDが範囲外です（{}）",
                index, item.id
            ));
        }
        if seen.contains(&item.id) {
            out.push(format!(
                "{}: 同じアイテムが重複しています（ID {}）",
                index, item.id
            ));
        } else {
            seen.push(item.id);
        }
    }
    out
}

/// Rename an equipment set, refusing control characters.
///
/// The checked form of [`set_equip_set_name`], with the same warnings as the
/// item set writer.
pub fn set_equip_set_name_checked(
    save: &mut Save,
    slot: usize,
    index: usize,
    name: &str,
) -> Result<NameFit, NameError> {
    let fit = name_check(name)?;
    if !set_equip_set_name(save, slot, index, name) {
        return Err(NameError::NoField);
    }
    Ok(fit)
}

/// Rename an equipment set.
///
/// Same rule as [`set_item_set_name`]: a line break goes in, every other
/// control character is refused, and false means nothing was written.
pub fn set_equip_set_name(save: &mut Save, slot: usize, index: usize, name: &str) -> bool {
    if index >= EQUIP_SETS || name_check(name).is_err() {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_EQUIP_SETS + index * EQUIP_SET_SIZE;
    let Some(dst) = save.body_mut().get_mut(at..at + NAME_LEN) else {
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

/// The checked form of [`set_name`].
pub fn set_name_checked(
    save: &mut Save,
    slot: usize,
    set: usize,
    name: &str,
) -> Result<NameFit, NameError> {
    set_equip_set_name_checked(save, slot, set, name)
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