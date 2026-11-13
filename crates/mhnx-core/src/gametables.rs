//! The game's own tables: what an item looks like, and how rare a piece is.
//!
//! A save stores ids, not presentation. The icon an item draws, the colour the
//! game tints it with and the rarity behind an equipment icon all live in
//! `nativeNX/loc/arc/resident.arc`, in `table\*`. This module reads those
//! files and nothing else: it never touches the filesystem, so the same code
//! runs on the console after the caller has read the bytes off the card.
//!
//! Three files matter, and every offset below was measured against the shipped
//! Japanese MHXX build:
//!
//! ```text
//! table\itemData            u32 count at +4, 44 byte records from +8
//!   +0x04 type   +0x05 rarity - 1   +0x10 icon cell   +0x11 colour
//!   (2991 records, 8 + 2991 * 44 = 131612 = the file's size)
//!
//! table\armorSeriesData     u32 count at +4, 127 byte records from +8
//!   +6..+11 the parts a series has   +103 rarity - 1
//!   (1287 records, 8 + 1287 * 127 = 163457 = the file's size)
//!
//! table\weaponNNBaseData    u32 count at +4, stride = (size - 8) / count
//!   +9 max level   +10 levels past the limit   +11 rarity - 1
//! ```
//!
//! Rarity is stored minus one and runs 1..=11, which is exactly the number of
//! colours in [`crate::icons::RARITY_COLORS`].

use alloc::vec::Vec;

/// A table whose header is "u32 count at +4, records from +8".
fn decode(bytes: &[u8], stride: usize) -> Option<Vec<&[u8]>> {
    if stride == 0 || bytes.len() < 8 {
        return None;
    }
    let count = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let body = bytes.len() - 8;
    if count == 0 || count > 100_000 {
        return None;
    }
    // The shipped files divide exactly; a file that does not is not this table.
    if body != count * stride {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 8 + i * stride;
        out.push(&bytes[at..at + stride]);
    }
    Some(out)
}

/// What one item looks like on the box screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemLook {
    /// The icon cell on the game's sheet (see [`crate::icons::cell_for`]).
    pub icon: u8,
    /// The colour byte, already reduced the way the game reduces it.
    pub color: u8,
    /// The item's rarity, 1 based.
    pub rarity: u8,
    /// The item's own type byte, kept for anything that wants to group by it.
    pub kind: u8,
}

/// `table\itemData`.
#[derive(Debug, Clone, Default)]
pub struct ItemTable {
    looks: Vec<ItemLook>,
}

impl ItemTable {
    /// Read the table, or `None` when the bytes are not it.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let records = decode(bytes, 44)?;
        let looks = records
            .iter()
            .map(|r| ItemLook {
                kind: r[0x04],
                rarity: r[0x05].wrapping_add(1),
                icon: r[0x10],
                color: r[0x11] % 17,
            })
            .collect();
        Some(ItemTable { looks })
    }

    /// How many items the table holds.
    pub fn len(&self) -> usize {
        self.looks.len()
    }

    /// True when the table is empty.
    pub fn is_empty(&self) -> bool {
        self.looks.is_empty()
    }

    /// The look of one item id, if the table has it.
    pub fn look(&self, id: u16) -> Option<ItemLook> {
        self.looks.get(id as usize).copied()
    }
}

/// `table\armorSeriesData`.
#[derive(Debug, Clone, Default)]
pub struct ArmorTable {
    rarities: Vec<u8>,
}

impl ArmorTable {
    /// Read the table, or `None` when the bytes are not it.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let records = decode(bytes, 127)?;
        let rarities = records.iter().map(|r| r[103].wrapping_add(1)).collect();
        Some(ArmorTable { rarities })
    }

    /// The rarity of an armour piece.
    ///
    /// `kind` is the save's type byte: 1 head, 2 chest, 3 arms, 4 waist, 5 legs.
    /// A series that does not carry that part has no rarity to give.
    pub fn rarity_for(&self, bytes: &[u8], kind: u8, id: u16) -> Option<u8> {
        if !(1..=5).contains(&kind) {
            return None;
        }
        let part = (kind - 1) as usize;
        let at = 8 + id as usize * 127;
        if bytes.len() < at + 127 {
            return None;
        }
        // A series that does not have this part is not a piece of armour: the
        // game would not draw one either.
        if bytes[at + 6 + part] == 0 {
            return None;
        }
        self.rarities.get(id as usize).copied()
    }

    /// How many series the table holds.
    pub fn len(&self) -> usize {
        self.rarities.len()
    }

    /// True when the table is empty.
    pub fn is_empty(&self) -> bool {
        self.rarities.is_empty()
    }
}

/// One `table\weaponNNBaseData`, for one weapon class.
#[derive(Debug, Clone, Default)]
pub struct WeaponTable {
    rarities: Vec<u8>,
}

impl WeaponTable {
    /// Read the table, or `None` when the bytes are not it.
    ///
    /// The record size is not fixed across classes (33 and 34 both ship), so it
    /// comes from the count in the header rather than from a constant.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        let count = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        if count == 0 || (bytes.len() - 8) % count != 0 {
            return None;
        }
        let stride = (bytes.len() - 8) / count;
        if stride < 12 {
            return None;
        }
        let records = decode(bytes, stride)?;
        let rarities = records.iter().map(|r| r[11].wrapping_add(1)).collect();
        Some(WeaponTable { rarities })
    }

    /// The rarity of one weapon of this class.
    pub fn rarity(&self, id: u16) -> Option<u8> {
        self.rarities.get(id as usize).copied()
    }

    /// How many weapons the class holds.
    pub fn len(&self) -> usize {
        self.rarities.len()
    }

    /// True when the table is empty.
    pub fn is_empty(&self) -> bool {
        self.rarities.is_empty()
    }
}

/// The weapon class a box type names, or `None` for armour and talismans.
///
/// The box's type byte is 7 + the class, and class 5 has no table: the game has
/// no `weapon05BaseData`, which is why type 12 is the one the box never holds.
pub fn weapon_class(kind: u8) -> Option<u8> {
    match kind {
        7..=21 => Some(kind - 7),
        _ => None,
    }
}

/// Every table the editor can use, filled as far as the card had the files.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    /// `table\itemData`.
    pub items: ItemTable,
    /// `table\armorSeriesData`.
    pub armor: ArmorTable,
    /// The armour series file itself, needed for the part-present bytes.
    pub armor_bytes: Vec<u8>,
    /// `table\weaponNNBaseData`, indexed by class.
    pub weapons: [Option<WeaponTable>; 15],
}

impl Tables {
    /// The rarity the game would draw an equipment icon at, if it is knowable.
    ///
    /// A talisman is not here: its rarity is in the save itself, at +0x12, so
    /// the caller reads it from the record rather than from a table.
    pub fn equip_rarity(&self, kind: u8, id: u16) -> Option<u8> {
        if let Some(class) = weapon_class(kind) {
            return self.weapons.get(class as usize)?.as_ref()?.rarity(id);
        }
        if (1..=5).contains(&kind) {
            return self.armor.rarity_for(&self.armor_bytes, kind, id);
        }
        None
    }

    /// True when nothing at all was loaded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
            && self.armor.is_empty()
            && self.weapons.iter().all(|w| w.is_none())
    }
}
