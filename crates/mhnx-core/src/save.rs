//! MHXX save-data container (3DS and Switch).
//!
//! Layout (all offsets below are **body** offsets):
//!
//! ```text
//!     [0x000000, 0x126474)   shared block  1_205_364 B  (download-quest section)
//!     [0x126474, 0x244C34)   character slot 1   0x11E7C0 B
//!     [0x244C34, 0x3633F4)   character slot 2   0x11E7C0 B
//!     [0x3633F4, 0x481BB4)   character slot 3   0x11E7C0 B
//!     last 468 bytes         padding / localised tail (0x481D88-0x481BB4)
//! ```
//!
//! The slot base offsets are stored in the file itself at body[0x10],
//! body[0x14] and body[0x18].

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::crc::name_hash;
use crate::charm;
use crate::dlcats;
use crate::itembox::{self, ItemSlot, ITEM_BOX_SIZE};
use crate::Error;

/// Size of the 3DS save body.
pub const BODY_SIZE: usize = 4_726_152;
/// Size of the Switch header that precedes the body.
pub const SWITCH_HEADER_SIZE: usize = 36;
/// Size of a Switch save file.
pub const SWITCH_SIZE: usize = BODY_SIZE + SWITCH_HEADER_SIZE;
/// Size of a MHGU (English) Switch save file.
pub const MHGU_SIZE: usize = BODY_SIZE + 432_948;
/// Distance between character slot bases (MHXX).
pub const SLOT_STRIDE: usize = 0x11E7C0;
/// Distance between character slot bases (MHGU) = 1_177_796 bytes.
pub const MHGU_SLOT_STRIDE: usize = 0x11F8C4;

/// Number of character slots.
pub const MAX_SLOTS: usize = 3;

// ---- body header -----------------------------------------------------------
const OFF_BODY_MARKER: usize = 0x00;
const OFF_SLOT_USED: usize = 0x04;
const OFF_SLOT_BASE: [usize; 3] = [0x10, 0x14, 0x18];
/// Body offset of the first character slot. The three slots follow at
/// SLOT_STRIDE intervals; the download-quest section sits in front of them.
pub const FIRST_SLOT_BASE: u32 = 0x126474;
const OFF_DLC_SECTION: usize = 0x0C;
/// Screen brightness. Real saves hold values well above 31 (48 in the sample
/// dump), so this is a plain byte and must never be clamped to 0..=31.
pub const OFF_BRIGHTNESS: usize = 0x28;
/// Rumble on/off.
pub const OFF_RUMBLE: usize = 0x29;
/// First download Palico record in the shared block.
pub const OFF_DLC_PALICO: usize = 0x2A;
/// Number of download Palicoes.
pub const DLC_PALICO_COUNT: usize = 50;
/// Size of one download Palico record.
pub const DLC_PALICO_SIZE: usize = 0x144;
/// Offset of the Palico id inside a download Palico record.
pub const DLC_PALICO_ID: usize = 0x5F;

// ---- character field offsets (relative to a slot base) ---------------------
/// Character name, 32 bytes UTF-8 (in-game copy).
pub const OFF_NAME: usize = 0x23B7D;
/// Character name as shown on the save/load screen.
pub const OFF_NAME_PREVIEW: usize = 0x000;
/// Play time in seconds, in-game copy.
pub const OFF_PLAY_TIME: usize = 0x2248B;
/// Play time in seconds, save screen copy.
pub const OFF_PLAY_TIME_PREVIEW: usize = 0x20;
/// Zenny, in-game copy.
pub const OFF_FUNDS: usize = 0x280F;
/// Zenny, save screen copy.
pub const OFF_FUNDS_PREVIEW: usize = 0x24;
/// Hunter rank (u16).
pub const OFF_HUNTER_RANK: usize = 0x28;
/// Hunter rank points.
pub const OFF_HR_POINTS: usize = 0x280B;
/// Academy points.
pub const OFF_ACADEMY_POINTS: usize = 0x2817;
/// Bherna / Kokoto / Pokke / Yukumo village points.
pub const OFF_VILLAGE_POINTS: [usize; 4] = [0x281B, 0x281F, 0x2823, 0x2827];
/// Face / appearance byte fields.
pub const OFF_VOICE: usize = 0x23B48;
pub const OFF_EYE_COLOR: usize = 0x23B49;
pub const OFF_CLOTHING: usize = 0x23B4A;
pub const OFF_GENDER: usize = 0x23B4B;
/// Gender copy the character select screen shows. Must be kept in sync with
/// OFF_GENDER or the slot list shows the old value.
pub const OFF_GENDER_PREVIEW: usize = 0x2A;
pub const OFF_HAIRSTYLE: usize = 0x23B4D;
pub const OFF_FACE: usize = 0x23B4E;
pub const OFF_FEATURES: usize = 0x23B4F;
/// RGBA colours.
pub const OFF_SKIN_COLOR: usize = 0x23B67;
pub const OFF_HAIR_COLOR: usize = 0x23B6B;
pub const OFF_FEATURES_COLOR: usize = 0x23B6F;
pub const OFF_CLOTHING_COLOR: usize = 0x23B73;
/// Item box (2300 x 19 bit records inside 5463 bytes).
pub const OFF_ITEM_BOX: usize = 0x0278;
/// Equipment box: 2000 records of 36 bytes.
pub const OFF_EQUIPMENT_BOX: usize = 0x62EE;
pub const EQUIPMENT_BOX_SIZE: usize = 72_000;
/// Size of the character part of a slot, without the shoutout footer.
///
/// The reference save manager calls this SLOT_SIZE and exports files of this
/// size; stride = SLOT_DATA_SIZE + footer (0x1738 on MHXX, 0x283C on MHGU).
pub const SLOT_DATA_SIZE: usize = 0x11D088;
pub const EQUIPMENT_RECORD: usize = 36;
pub const TOTAL_EQUIP_SLOTS: usize = 2000;
/// Palico equipment box: 1000 records of 36 bytes.
pub const OFF_PALICO_EQUIPMENT: usize = 0x17C2E;
pub const PALICO_EQUIPMENT_SIZE: usize = 36_000;
pub const TOTAL_PALICO_EQUIP: usize = 1000;
/// Monster hunt / capture / size tables.
pub const OFF_MONSTER_HUNT: usize = 0x5EA6;
pub const OFF_MONSTER_CAPTURE: usize = 0x5FB8;
pub const OFF_MONSTER_SIZE: usize = 0x60CA;
pub const MONSTER_TABLE_SIZE: usize = 274;
pub const MONSTER_SIZE_TABLE_SIZE: usize = 548;

/// Number of monsters the hunt records cover.
///
/// The three tables sit back to back and the last one ends exactly where the
/// equipment box starts: 0x5EA6 + 137*2 = 0x5FB8, + 137*2 = 0x60CA,
/// + 137*4 = 0x62EE. That arithmetic is the evidence for the count, not a
/// guess from the monster roster.
pub const MONSTER_COUNT: usize = 137;
/// Palico roster: 84 records of 324 bytes.
pub const OFF_PALICO: usize = 0x23BB6;
pub const PALICO_SIZE: usize = 27_216;
pub const PALICO_RECORD: usize = 324;
pub const TOTAL_PALICO: usize = 84;
/// Guild card.
pub const OFF_GUILD_CARD: usize = 0xC71BD;
pub const GUILD_CARD_SIZE: usize = 4_986;
pub const OFF_GUILD_CARD_ID: usize = 0xC7A6D;
pub const OFF_ARENA_LOG: usize = 0xC83E1;
pub const ARENA_LOG_SIZE: usize = 342;
/// Shoutouts.
pub const OFF_MANUAL_SHOUTOUTS: usize = 0x11D629;
pub const MANUAL_SHOUTOUT_SIZE: usize = 2_880;
pub const TOTAL_MANUAL_SHOUTOUTS: usize = 48;
pub const OFF_AUTO_SHOUTOUTS: usize = 0x11E169;
pub const AUTO_SHOUTOUT_SIZE: usize = 1_620;
pub const TOTAL_AUTO_SHOUTOUTS: usize = 27;
/// Length of one shoutout entry in MHXX.
pub const SHOUTOUT_ENTRY: usize = 60;
/// Length of one shoutout entry in MHGU (English) - 60 bytes + 44 bytes padding.
pub const MHGU_SHOUTOUT_ENTRY: usize = 104;

/// Which platform / version a save file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    /// 3DS MHXX (no container header).
    ThreeDs,
    /// Switch MHXX (JP) - 36 byte header.
    Switch,
    /// Switch MHGU (EN) - 36 byte header, larger body.
    Mhgu,
}

impl SaveKind {
    /// Does this kind carry the 36 byte Switch container header?
    pub fn has_header(self) -> bool {
        !matches!(self, SaveKind::ThreeDs)
    }
    /// Body length for this kind.
    pub fn body_size(self) -> usize {
        match self {
            SaveKind::Mhgu => MHGU_SIZE - SWITCH_HEADER_SIZE,
            _ => BODY_SIZE,
        }
    }
    /// Slot stride for this kind.
    pub fn stride(self) -> usize {
        match self {
            SaveKind::Mhgu => MHGU_SLOT_STRIDE,
            _ => SLOT_STRIDE,
        }
    }
}

/// Build the 36 byte Switch container header for a given file name and body.
///
/// Verified byte-exact against:
/// * the locally dumped Ryujinx save (system and system_backup)
/// * the blank MHXX Switch save shipped by the MHGU-MHXX save converter
/// * the blank MHGU save shipped by the same project
pub fn switch_header(name: &str, body_size: usize) -> [u8; SWITCH_HEADER_SIZE] {
    let mut h = [0u8; SWITCH_HEADER_SIZE];
    h[0x08..0x0C].copy_from_slice(&4u32.to_le_bytes());
    h[0x10..0x14].copy_from_slice(&0x14u32.to_le_bytes());
    h[0x14..0x18].copy_from_slice(&name_hash(name.as_bytes()).to_le_bytes());
    h[0x18..0x1C].copy_from_slice(&0x0Cu32.to_le_bytes());
    h[0x1C..0x20].copy_from_slice(&(body_size as u32).to_le_bytes());
    h
}

/// A parsed MHXX save file.
#[derive(Debug, Clone)]
pub struct Save {
    kind: SaveKind,
    header: [u8; SWITCH_HEADER_SIZE],
    body: Vec<u8>,
}

impl Save {
    /// Parse a save file, auto-detecting 3DS / Switch / MHGU by size.
    pub fn from_bytes(data: &[u8]) -> Result<Save, Error> {
        let kind = match data.len() {
            BODY_SIZE => SaveKind::ThreeDs,
            SWITCH_SIZE => SaveKind::Switch,
            MHGU_SIZE => SaveKind::Mhgu,
            got => {
                return Err(Error::BadSize {
                    got,
                    want: SWITCH_SIZE,
                })
            }
        };
        let mut header = [0u8; SWITCH_HEADER_SIZE];
        let body = if kind.has_header() {
            header.copy_from_slice(&data[..SWITCH_HEADER_SIZE]);
            data[SWITCH_HEADER_SIZE..].to_vec()
        } else {
            data.to_vec()
        };
        if body[OFF_BODY_MARKER] != 0xC6 {
            return Err(Error::BadMagic {
                context: "save body (expected 0xC6)",
            });
        }
        Ok(Save { kind, header, body })
    }

    /// Serialise back to bytes (header included for Switch kinds).
    pub fn to_bytes(&self) -> Vec<u8> {
        if self.kind.has_header() {
            let mut out = Vec::with_capacity(SWITCH_HEADER_SIZE + self.body.len());
            out.extend_from_slice(&self.header);
            out.extend_from_slice(&self.body);
            out
        } else {
            self.body.clone()
        }
    }

    /// Export just the 3DS-format body.
    pub fn to_3ds_bytes(&self) -> Vec<u8> {
        self.body.clone()
    }

    /// Build a Switch save from a 3DS body, synthesising the header.
    ///
    /// No save dump from a real console is required for this.
    pub fn from_3ds_body(body: &[u8], name: &str) -> Result<Save, Error> {
        if body.len() != BODY_SIZE {
            return Err(Error::BadSize {
                got: body.len(),
                want: BODY_SIZE,
            });
        }
        Ok(Save {
            kind: SaveKind::Switch,
            header: switch_header(name, BODY_SIZE),
            body: body.to_vec(),
        })
    }

    /// Which platform this save came from.
    pub fn kind(&self) -> SaveKind {
        self.kind
    }
    /// Raw 36 byte header (zeroed for 3DS saves).
    pub fn header(&self) -> &[u8; SWITCH_HEADER_SIZE] {
        &self.header
    }
    /// The save body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    /// Mutable access to the save body - for advanced / raw editing.
    pub fn body_mut(&mut self) -> &mut [u8] {
        &mut self.body
    }
    /// Slot stride of this save kind.
    pub fn stride(&self) -> usize {
        self.kind.stride()
    }

    fn u32_at(&self, off: usize) -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.body[off..off + 4]);
        u32::from_le_bytes(b)
    }
    fn set_u32_at(&mut self, off: usize, v: u32) {
        self.body[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn u16_at(&self, off: usize) -> u16 {
        let mut b = [0u8; 2];
        b.copy_from_slice(&self.body[off..off + 2]);
        u16::from_le_bytes(b)
    }
    fn set_u16_at(&mut self, off: usize, v: u16) {
        self.body[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }

    /// Offset of the download-quest section inside the body.
    pub fn dlc_section_offset(&self) -> u32 {
        self.u32_at(OFF_DLC_SECTION)
    }

    /// Base offset of character slot (0..2) inside the body.
    ///
    /// A base is only accepted when the whole slot fits in the body. The value
    /// comes out of the save, so a corrupt one (the body length minus 16, say)
    /// used to be accepted and made every accessor slice past the end. Bounding
    /// it here means read_character, the item box, the equipment box and the
    /// rest are all safe without repeating the check.
    pub fn slot_base(&self, slot: usize) -> Option<u32> {
        if slot >= MAX_SLOTS {
            return None;
        }
        let v = self.u32_at(OFF_SLOT_BASE[slot]);
        if v == 0 {
            return None;
        }
        let start = v as usize;
        let end = start.checked_add(self.kind.stride())?;
        if end > self.body.len() {
            return None;
        }
        Some(v)
    }

    /// All three slot bases.
    pub fn slot_bases(&self) -> [u32; 3] {
        [
            self.u32_at(OFF_SLOT_BASE[0]),
            self.u32_at(OFF_SLOT_BASE[1]),
            self.u32_at(OFF_SLOT_BASE[2]),
        ]
    }

    /// Is this slot marked as used?
    pub fn slot_used(&self, slot: usize) -> bool {
        slot < MAX_SLOTS && self.body[OFF_SLOT_USED + slot] != 0
    }

    /// Mark a slot as used / unused.
    pub fn set_slot_used(&mut self, slot: usize, used: bool) {
        if slot < MAX_SLOTS {
            self.body[OFF_SLOT_USED + slot] = used as u8;
        }
    }

    fn read_i32(&self, off: usize) -> i32 {
        self.u32_at(off) as i32
    }

    /// Read the editable scalar fields of a character slot.
    pub fn read_character(&self, slot: usize) -> Option<Character> {
        let base = self.slot_base(slot)? as usize;
        let mut name = String::from_utf8_lossy(&self.body[base + OFF_NAME..base + OFF_NAME + 32]).into_owned();
        if let Some(z) = name.find('\0') {
            name.truncate(z);
        }
        let mut name_preview =
            String::from_utf8_lossy(&self.body[base + OFF_NAME_PREVIEW..base + OFF_NAME_PREVIEW + 32]).into_owned();
        if let Some(z) = name_preview.find('\0') {
            name_preview.truncate(z);
        }
        let col = |off: usize| -> [u8; 4] {
            let mut a = [0u8; 4];
            a.copy_from_slice(&self.body[base + off..base + off + 4]);
            a
        };
        let mut c = Character {
            slot,
            base: base as u32,
            name,
            name_preview,
            play_time: self.read_i32(base + OFF_PLAY_TIME),
            funds: self.read_i32(base + OFF_FUNDS),
            hunter_rank: self.u16_at(base + OFF_HUNTER_RANK),
            hr_points: self.read_i32(base + OFF_HR_POINTS),
            academy_points: self.read_i32(base + OFF_ACADEMY_POINTS),
            village_points: [0; 4],
            voice: self.body[base + OFF_VOICE],
            eye_color: self.body[base + OFF_EYE_COLOR],
            clothing: self.body[base + OFF_CLOTHING],
            gender: self.body[base + OFF_GENDER],
            hairstyle: self.body[base + OFF_HAIRSTYLE],
            face: self.body[base + OFF_FACE],
            features: self.body[base + OFF_FEATURES],
            skin_color: col(OFF_SKIN_COLOR),
            hair_color: col(OFF_HAIR_COLOR),
            features_color: col(OFF_FEATURES_COLOR),
            clothing_color: col(OFF_CLOTHING_COLOR),
        };
        for (i, off) in OFF_VILLAGE_POINTS.iter().enumerate() {
            c.village_points[i] = self.read_i32(base + off);
        }
        Some(c)
    }

    /// Write the editable scalar fields back into a slot.
    ///
    /// Values the game keeps in two places (name, play time, funds) are
    /// written to **both** copies so the save screen and the in-game values
    /// stay consistent.
    pub fn write_character(&mut self, c: &Character) -> Result<(), Error> {
        let base = self
            .slot_base(c.slot)
            .ok_or(Error::Parse("character slot has no base offset"))? as usize;

        let mut nb = [0u8; 32];
        let raw = c.name.as_bytes();
        let n = raw.len().min(31);
        nb[..n].copy_from_slice(&raw[..n]);
        self.body[base + OFF_NAME..base + OFF_NAME + 32].copy_from_slice(&nb);
        self.body[base + OFF_NAME_PREVIEW..base + OFF_NAME_PREVIEW + 32].copy_from_slice(&nb);

        self.set_u32_at(base + OFF_PLAY_TIME, c.play_time as u32);
        self.set_u32_at(base + OFF_PLAY_TIME_PREVIEW, c.play_time as u32);
        self.set_u32_at(base + OFF_FUNDS, c.funds as u32);
        self.set_u32_at(base + OFF_FUNDS_PREVIEW, c.funds as u32);
        self.set_u16_at(base + OFF_HUNTER_RANK, c.hunter_rank);
        self.set_u32_at(base + OFF_HR_POINTS, c.hr_points as u32);
        self.set_u32_at(base + OFF_ACADEMY_POINTS, c.academy_points as u32);
        for (i, off) in OFF_VILLAGE_POINTS.iter().enumerate() {
            self.set_u32_at(base + off, c.village_points[i] as u32);
        }
        self.body[base + OFF_VOICE] = c.voice;
        self.body[base + OFF_EYE_COLOR] = c.eye_color;
        self.body[base + OFF_CLOTHING] = c.clothing;
        self.body[base + OFF_GENDER] = c.gender;
        self.body[base + OFF_GENDER_PREVIEW] = c.gender;
        self.body[base + OFF_HAIRSTYLE] = c.hairstyle;
        self.body[base + OFF_FACE] = c.face;
        self.body[base + OFF_FEATURES] = c.features;
        for (off, v) in [
            (OFF_SKIN_COLOR, c.skin_color),
            (OFF_HAIR_COLOR, c.hair_color),
            (OFF_FEATURES_COLOR, c.features_color),
            (OFF_CLOTHING_COLOR, c.clothing_color),
        ] {
            self.body[base + off..base + off + 4].copy_from_slice(&v);
        }
        self.sync_guild_card_appearance(c.slot);
        Ok(())
    }

    /// Copy the appearance bytes into the guild card's own copy.
    ///
    /// The card carries a second copy of the same eight bytes at
    /// OFF_GUILD_CARD + 0x19 and of the four colours at +0x38. In the sample
    /// save both copies are byte for byte identical for every character, so
    /// changing only the live one would leave the card describing the old
    /// hunter.
    pub fn sync_guild_card_appearance(&mut self, slot: usize) -> bool {
        let Some(base) = self.slot_base(slot).map(|b| b as usize) else {
            return false;
        };
        let gc = base + OFF_GUILD_CARD;
        let src = base + OFF_VOICE;
        let (Some(dst), Some(from)) = (
            self.body.get(gc + 0x19..gc + 0x21),
            self.body.get(src..src + 8).map(|s| s.to_vec()),
        ) else {
            return false;
        };
        let _ = dst;
        self.body[gc + 0x19..gc + 0x21].copy_from_slice(&from);
        let colors = self.body[base + OFF_SKIN_COLOR..base + OFF_SKIN_COLOR + 16].to_vec();
        self.body[gc + 0x38..gc + 0x48].copy_from_slice(&colors);
        true
    }

    /// Decode the item box of a slot.
    pub fn read_item_box(&self, slot: usize) -> Option<Vec<ItemSlot>> {
        let base = self.slot_base(slot)? as usize;
        Some(itembox::decode(
            &self.body[base + OFF_ITEM_BOX..base + OFF_ITEM_BOX + ITEM_BOX_SIZE],
        ))
    }

    /// Encode an item box back into a slot.
    pub fn write_item_box(&mut self, slot: usize, slots: &[ItemSlot]) -> Result<(), Error> {
        let base = self
            .slot_base(slot)
            .ok_or(Error::Parse("character slot has no base offset"))? as usize;
        let raw = itembox::encode(slots);
        self.body[base + OFF_ITEM_BOX..base + OFF_ITEM_BOX + ITEM_BOX_SIZE].copy_from_slice(&raw);
        Ok(())
    }

    /// Hunt counts, one per monster, in the order the save stores them.
    pub fn hunt_counts(&self, slot: usize) -> Option<Vec<u16>> {
        let r = self.slot_field(slot, OFF_MONSTER_HUNT, MONSTER_TABLE_SIZE)?;
        let b = self.body.get(r)?;
        Some(
            (0..MONSTER_COUNT)
                .map(|i| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]))
                .collect(),
        )
    }

    /// Capture counts, one per monster.
    pub fn capture_counts(&self, slot: usize) -> Option<Vec<u16>> {
        let r = self.slot_field(slot, OFF_MONSTER_CAPTURE, MONSTER_TABLE_SIZE)?;
        let b = self.body.get(r)?;
        Some(
            (0..MONSTER_COUNT)
                .map(|i| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]))
                .collect(),
        )
    }

    /// Smallest and largest size recorded per monster, as the game shows them.
    pub fn monster_sizes(&self, slot: usize) -> Option<Vec<(u16, u16)>> {
        let r = self.slot_field(slot, OFF_MONSTER_SIZE, MONSTER_SIZE_TABLE_SIZE)?;
        let b = self.body.get(r)?;
        Some(
            (0..MONSTER_COUNT)
                .map(|i| {
                    let o = i * 4;
                    (
                        u16::from_le_bytes([b[o], b[o + 1]]),
                        u16::from_le_bytes([b[o + 2], b[o + 3]]),
                    )
                })
                .collect(),
        )
    }

    /// Set one monster's hunt count.
    pub fn set_hunt_count(&mut self, slot: usize, monster: usize, value: u16) -> bool {
        self.set_monster_u16(slot, OFF_MONSTER_HUNT, monster, value)
    }

    /// Set one monster's capture count.
    pub fn set_capture_count(&mut self, slot: usize, monster: usize, value: u16) -> bool {
        self.set_monster_u16(slot, OFF_MONSTER_CAPTURE, monster, value)
    }

    /// Set one monster's smallest and largest recorded size.
    pub fn set_monster_size(
        &mut self,
        slot: usize,
        monster: usize,
        small: u16,
        big: u16,
    ) -> bool {
        if monster >= MONSTER_COUNT {
            return false;
        }
        let Some(r) = self.slot_field(slot, OFF_MONSTER_SIZE, MONSTER_SIZE_TABLE_SIZE) else {
            return false;
        };
        let o = r.start + monster * 4;
        let Some(b) = self.body.get_mut(o..o + 4) else {
            return false;
        };
        b[0..2].copy_from_slice(&small.to_le_bytes());
        b[2..4].copy_from_slice(&big.to_le_bytes());
        true
    }

    fn set_monster_u16(&mut self, slot: usize, table: usize, monster: usize, value: u16) -> bool {
        if monster >= MONSTER_COUNT {
            return false;
        }
        let Some(r) = self.slot_field(slot, table, MONSTER_TABLE_SIZE) else {
            return false;
        };
        let o = r.start + monster * 2;
        let Some(b) = self.body.get_mut(o..o + 2) else {
            return false;
        };
        b.copy_from_slice(&value.to_le_bytes());
        true
    }
    /// Decode the talismans (お守り) held in a slot's equipment box.
    ///
    /// The box also holds weapons and armour; see the charm module.
    pub fn charms(&self, slot: usize) -> Option<Vec<charm::Charm>> {
        Some(charm::decode_box(self.equipment_box(slot)?))
    }
    /// Write one talisman into the slot's equipment box.
    ///
    /// An existing talisman has only its five owned fields changed. A blank
    /// record is filled by copying a talisman the game wrote, because the
    /// rarity byte and its partner at +0x02 cannot be invented. Weapons and
    /// armour are never touched either way.
    pub fn set_charm(&mut self, slot: usize, c: &charm::Charm) -> bool {
        let Some(b) = self.equipment_box_mut(slot) else {
            return false;
        };
        if charm::update_charm(b, c) {
            return true;
        }
        let Some(template) = charm::template_from_box(b) else {
            return false;
        };
        charm::create_charm(b, c.index, &template, c)
    }
    /// Delete one talisman from the slot's equipment box.
    pub fn clear_charm(&mut self, slot: usize, index: usize) -> bool {
        match self.equipment_box_mut(slot) {
            Some(b) => charm::clear_charm(b, index),
            None => false,
        }
    }

    /// Global options stored outside the character slots.
    ///
    /// The last two live just before the download-quest section, so their
    /// position is derived from the section offset rather than hard coded -
    /// MHGU stores the section one byte later than MHXX.
    pub fn options(&self) -> Options {
        let dlc = self.dlc_section_offset() as usize;
        // A corrupt section offset of 0 or 1 must not underflow.
        let before = |n: usize| -> u8 {
            dlc.checked_sub(n)
                .and_then(|o| self.body.get(o))
                .copied()
                .unwrap_or(0)
        };
        Options {
            brightness: self.body[OFF_BRIGHTNESS],
            rumble: self.body[OFF_RUMBLE],
            disable_3d: before(2),
            circle_pad_pro: before(1),
        }
    }

    /// Store the global options.
    pub fn set_options(&mut self, o: &Options) {
        let dlc = self.dlc_section_offset() as usize;
        self.body[OFF_BRIGHTNESS] = o.brightness;
        self.body[OFF_RUMBLE] = o.rumble;
        if dlc >= 2 && dlc <= self.body.len() {
            self.body[dlc - 2] = o.disable_3d;
            self.body[dlc - 1] = o.circle_pad_pro;
        }
    }

    /// The 50 download Palicoes (オトモ) that follow the game's free updates.
    ///
    /// They live at a fixed offset in the shared block, not inside a character
    /// slot: 50 records of 0x144 bytes, the Palico id at +0x5F and the name at
    /// the start of the record.
    pub fn dlc_palicoes(&self) -> Vec<DlcPalico> {
        use alloc::string::String;
        let mut out = Vec::new();
        for i in 0..DLC_PALICO_COUNT {
            let o = OFF_DLC_PALICO + i * DLC_PALICO_SIZE;
            if o + DLC_PALICO_SIZE > self.body.len() {
                break;
            }
            let rec = &self.body[o..o + DLC_PALICO_SIZE];
            let raw = &rec[..32];
            let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
            let name = String::from_utf8_lossy(&raw[..end]).into_owned();
            out.push(DlcPalico {
                index: i,
                id: rec[DLC_PALICO_ID],
                name,
            });
        }
        out
    }

    /// Put one download Palico record back (raw 0x144 bytes).
    pub fn set_dlc_palico(&mut self, index: usize, record: &[u8]) -> bool {
        if index >= DLC_PALICO_COUNT || record.len() != DLC_PALICO_SIZE {
            return false;
        }
        let o = OFF_DLC_PALICO + index * DLC_PALICO_SIZE;
        if o + DLC_PALICO_SIZE > self.body.len() {
            return false;
        }
        self.body[o..o + DLC_PALICO_SIZE].copy_from_slice(record);
        true
    }

    /// Is this download Palico already installed?
    pub fn has_dlc_palico(&self, id: u8) -> bool {
        self.dlc_palicoes().iter().any(|c| c.id == id)
    }

    /// Install a download Palico into the first free slot.
    ///
    /// Returns the slot index it landed in. The record is copied verbatim from
    /// the released data, exactly like the reference save manager does.
    pub fn install_dlc_palico(&mut self, lang: dlcats::Lang, id: u8) -> Result<usize, Error> {
        let rec = dlcats::record(lang, id).ok_or(Error::Parse("no such download Palico"))?;
        if self.has_dlc_palico(id) {
            return Err(Error::Parse("that download Palico is already installed"));
        }
        let slot = self
            .dlc_palicoes()
            .into_iter()
            .find(|c| c.id == 0)
            .map(|c| c.index)
            .ok_or(Error::Parse("no free download Palico slot"))?;
        if !self.set_dlc_palico(slot, rec) {
            return Err(Error::Parse("could not write the Palico record"));
        }
        Ok(slot)
    }

    /// Clear one download Palico slot.
    pub fn remove_dlc_palico(&mut self, index: usize) -> bool {
        self.set_dlc_palico(index, &[0u8; DLC_PALICO_SIZE])
    }

    /// Byte range of a slot-relative field, or None when it does not fit.
    ///
    /// The slot base comes out of the save file, so a corrupt one (say the body
    /// length minus 16) used to slice past the end and panic. Every accessor
    /// goes through here so a bad base returns None instead.
    fn slot_field(&self, slot: usize, off: usize, len: usize) -> Option<core::ops::Range<usize>> {
        let base = self.slot_base(slot)? as usize;
        let start = base.checked_add(off)?;
        let end = start.checked_add(len)?;
        if end > self.body.len() {
            return None;
        }
        Some(start..end)
    }

    fn slot_field_mut(
        &mut self,
        slot: usize,
        off: usize,
        len: usize,
    ) -> Option<core::ops::Range<usize>> {
        self.slot_field(slot, off, len)
    }

    /// Raw equipment box (2000 x 36 bytes). Holds weapons, armour and
    /// talismans together - see the charm module.
    pub fn equipment_box(&self, slot: usize) -> Option<&[u8]> {
        let r = self.slot_field(slot, OFF_EQUIPMENT_BOX, EQUIPMENT_BOX_SIZE)?;
        self.body.get(r)
    }
    /// Mutable raw equipment box.
    pub fn equipment_box_mut(&mut self, slot: usize) -> Option<&mut [u8]> {
        let r = self.slot_field_mut(slot, OFF_EQUIPMENT_BOX, EQUIPMENT_BOX_SIZE)?;
        self.body.get_mut(r)
    }
    /// Raw palico equipment box.
    pub fn palico_equipment_box(&self, slot: usize) -> Option<&[u8]> {
        let r = self.slot_field(slot, OFF_PALICO_EQUIPMENT, PALICO_EQUIPMENT_SIZE)?;
        self.body.get(r)
    }
    /// Raw palico roster.
    pub fn palico_data(&self, slot: usize) -> Option<&[u8]> {
        let r = self.slot_field(slot, OFF_PALICO, PALICO_SIZE)?;
        self.body.get(r)
    }
    /// Raw guild card.
    pub fn guild_card(&self, slot: usize) -> Option<&[u8]> {
        let r = self.slot_field(slot, OFF_GUILD_CARD, GUILD_CARD_SIZE)?;
        self.body.get(r)
    }
    /// Raw shoutout blocks: (manual, automatic).
    pub fn shoutouts(&self, slot: usize) -> Option<(&[u8], &[u8])> {
        let a = self.slot_field(slot, OFF_MANUAL_SHOUTOUTS, MANUAL_SHOUTOUT_SIZE)?;
        let b = self.slot_field(slot, OFF_AUTO_SHOUTOUTS, AUTO_SHOUTOUT_SIZE)?;
        let a = self.body.get(a)?;
        let b = self.body.get(b)?;
        Some((a, b))
    }

    /// One whole character slot, exactly as stored in the file.
    ///
    /// This is the unit the reference save manager exchanges as a .saveslot
    /// file: the character data plus the shoutout footer, i.e. the stride
    /// between two slot bases.
    pub fn export_slot(&self, slot: usize) -> Option<Vec<u8>> {
        let start = self.slot_base(slot)? as usize;
        let end = start + self.slot_stride();
        if end > self.body.len() {
            return None;
        }
        Some(self.body[start..end].to_vec())
    }

    /// Put a character slot back.
    ///
    /// Accepts either a full stride (0x11E7C0 on MHXX, 0x11F8C4 on MHGU) or
    /// just the character part (0x11D088) that some tools export.
    pub fn import_slot(&mut self, slot: usize, data: &[u8]) -> Result<(), Error> {
        let start = self
            .slot_base(slot)
            .ok_or(Error::Parse("character slot has no base offset"))? as usize;
        let stride = self.slot_stride();
        let n = match data.len() {
            l if l == stride => l,
            l if l == SLOT_DATA_SIZE => l,
            _ => {
                return Err(Error::BadSize {
                    got: data.len(),
                    want: stride,
                })
            }
        };
        if start + n > self.body.len() {
            return Err(Error::BadSize {
                got: data.len(),
                want: stride,
            });
        }
        self.body[start..start + n].copy_from_slice(&data[..n]);
        self.set_slot_used(slot, true);
        Ok(())
    }

    /// Number of bytes between two character slots.
    pub fn slot_stride(&self) -> usize {
        match (self.slot_base(1), self.slot_base(0)) {
            (Some(a), Some(b)) if a > b => (a - b) as usize,
            _ => SLOT_STRIDE,
        }
    }

    /// Overwrite the whole body (used when importing a 3DS save).
    pub fn replace_body(&mut self, body: &[u8]) -> Result<(), Error> {
        if body.len() != self.kind.body_size() {
            return Err(Error::BadSize {
                got: body.len(),
                want: self.kind.body_size(),
            });
        }
        self.body = body.to_vec();
        Ok(())
    }

    /// Re-synthesise the container header for a new file name.
    ///
    /// Only meaningful for Switch kinds. Call this after renaming a save: the
    /// header carries a CRC of the file name at +0x14, so writing a save out
    /// under a name other than the one it was read under would leave a stale
    /// checksum. to_bytes() writes back whatever is in self.header.
    pub fn refresh_header(&mut self, name: &str) {
        if self.kind.has_header() {
            self.header = switch_header(name, self.body.len());
        }
    }

    /// The file name this save's header was built for, if any.
    ///
    /// Returns the name whose CRC sits at header +0x14 when it matches one of
    /// the two names the game uses, so callers can warn about a mismatch.
    pub fn header_name(&self) -> Option<&'static str> {
        if !self.kind.has_header() {
            return None;
        }
        for name in ["system", "system_backup"] {
            if switch_header(name, self.body.len())[0x14..0x18] == self.header[0x14..0x18] {
                return Some(name);
            }
        }
        None
    }
}

/// Global options stored outside the character slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Screen brightness.
    pub brightness: u8,
    /// Rumble enabled.
    pub rumble: u8,
    /// 3D disabled (1 on Switch, 0 on 3DS).
    pub disable_3d: u8,
    /// Circle Pad Pro enabled.
    pub circle_pad_pro: u8,
}

/// One download Palico from the shared block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DlcPalico {
    /// Record index (0..49).
    pub index: usize,
    /// Palico id, used to match the released data tables.
    pub id: u8,
    /// Display name.
    pub name: alloc::string::String,
}

/// The editable scalar fields of one character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    /// Slot index (0..2).
    pub slot: usize,
    /// Body offset of this slot.
    pub base: u32,
    /// Character name (in-game).
    pub name: String,
    /// Character name as stored for the save screen.
    pub name_preview: String,
    /// Play time in seconds.
    pub play_time: i32,
    /// Zenny.
    pub funds: i32,
    /// Hunter rank.
    pub hunter_rank: u16,
    /// Hunter rank points.
    pub hr_points: i32,
    /// Academy points.
    pub academy_points: i32,
    /// Bherna / Kokoto / Pokke / Yukumo points.
    pub village_points: [i32; 4],
    /// Voice id.
    pub voice: u8,
    /// Eye colour id.
    pub eye_color: u8,
    /// Clothing id.
    pub clothing: u8,
    /// Gender (0 = male, 1 = female).
    pub gender: u8,
    /// Hairstyle id.
    pub hairstyle: u8,
    /// Face id.
    pub face: u8,
    /// Features id.
    pub features: u8,
    /// Skin colour RGBA.
    pub skin_color: [u8; 4],
    /// Hair colour RGBA.
    pub hair_color: [u8; 4],
    /// Features colour RGBA.
    pub features_color: [u8; 4],
    /// Clothing colour RGBA.
    pub clothing_color: [u8; 4],
}

impl Default for Character {
    fn default() -> Self {
        Character {
            slot: 0,
            base: 0,
            name: String::new(),
            name_preview: String::new(),
            play_time: 0,
            funds: 0,
            hunter_rank: 0,
            hr_points: 0,
            academy_points: 0,
            village_points: [0; 4],
            voice: 0,
            eye_color: 0,
            clothing: 0,
            gender: 0,
            hairstyle: 0,
            face: 0,
            features: 0,
            skin_color: [0; 4],
            hair_color: [0; 4],
            features_color: [0; 4],
            clothing_color: [0; 4],
        }
    }
}

/// The blank character template (equivalent to the reference editor's
/// embedded CleanSave.txt, 0x11E7C0 bytes).
#[derive(Debug, Clone)]
pub struct BlankCharacter(pub Vec<u8>);

impl BlankCharacter {
    /// Load from raw bytes and validate the size.
    pub fn from_bytes(b: &[u8]) -> Result<Self, Error> {
        if b.len() != SLOT_STRIDE {
            return Err(Error::BadSize {
                got: b.len(),
                want: SLOT_STRIDE,
            });
        }
        Ok(BlankCharacter(b.to_vec()))
    }
    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Erase a character slot by copying the blank template over it.
pub fn erase_slot(save: &mut Save, slot: usize, blank: &BlankCharacter) -> Result<(), Error> {
    let base = save
        .slot_base(slot)
        .ok_or(Error::Parse("character slot has no base offset"))? as usize;
    let n = blank.0.len();
    if base + n > save.body.len() {
        return Err(Error::OutOfRange {
            context: "erase_slot",
            value: base as u32,
        });
    }
    save.body[base..base + n].copy_from_slice(&blank.0);
    save.set_slot_used(slot, false);
    Ok(())
}

/// A minimal, well-formed empty Switch save (useful for tests).
pub fn empty_switch_save() -> Save {
    let mut s = Save {
        kind: SaveKind::Switch,
        header: switch_header("system", BODY_SIZE),
        body: vec![0u8; BODY_SIZE],
    };
    s.body[OFF_BODY_MARKER] = 0xC6;
    s.set_u32_at(0x08, 0x1C);
    s.set_u32_at(OFF_DLC_SECTION, 0xB280);
    // Slot bases are absolute body offsets, not multiples of the stride: the
    // download-quest section sits in front of the first character. A real MHXX
    // save stores 0x126474 / 0x244C34 / 0x3633F4, i.e. FIRST_SLOT_BASE + n *
    // SLOT_STRIDE. Using n * SLOT_STRIDE put the first slot 0x11E7C0 bytes too
    // early, on top of the quest array.
    for (i, off) in OFF_SLOT_BASE.iter().enumerate() {
        s.set_u32_at(*off, FIRST_SLOT_BASE + (SLOT_STRIDE * i) as u32);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_matches_reference_bytes() {
        let h = switch_header("system", BODY_SIZE);
        assert_eq!(
            h,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0, 0x74, 0xEE, 0xB2, 0x36, 12, 0, 0, 0,
                0x88, 0x1D, 0x48, 0, 0, 0, 0, 0
            ]
        );
        let b = switch_header("system_backup", BODY_SIZE);
        assert_eq!(&b[0x14..0x18], &[0x92, 0x9B, 0x81, 0xD0]);
    }

    #[test]
    fn mhgu_header_matches() {
        let h = switch_header("system", MHGU_SIZE - SWITCH_HEADER_SIZE);
        assert_eq!(&h[0x1C..0x20], &[0x98, 0xB8, 0x4E, 0x00]);
    }
}
