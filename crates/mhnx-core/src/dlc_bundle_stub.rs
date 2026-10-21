//! Official DLC quest bundle - **empty stub**.
//!
//! This file ships with the repository and contains no game data. It exists so
//! the crate builds and every consumer keeps a stable API. The real bundle is
//! produced from your own copy of the game by:
//!
//! ```text
//! python tools/gen_official_dlc.py
//! ```
//!
//! That writes `dlc_bundle_gen.rs` and `official_dlc.bin` next to this file,
//! both of which are git-ignored, and `build.rs` then compiles the generated
//! module instead of this one. The practical effect of the stub is that the
//! tool works normally but has no built-in list of official quests to import.

/// Raw bundle of the compiled quest archives. Empty in the stub.
pub static OFFICIAL_DLC_BLOB: &[u8] = &[];

/// Category of a bundled quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DlcQuestCategory {
    /// Playable event and challenge quests.
    Official = 0,
    /// Unused / placeholder archives that no one can play.
    Dead = 1,
}

/// Metadata for one bundled downloadable quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficialQuestMeta {
    /// Quest ID.
    pub quest_id: i32,
    /// Category.
    pub category: DlcQuestCategory,
    /// Default slot index, or 255 for none.
    pub default_slot: u8,
    /// Byte offset inside the blob.
    pub offset: u32,
    /// Byte length of the archive.
    pub len: u16,
    /// Japanese title, or an empty string.
    pub title: &'static str,
}

/// Metadata for one bundled equipment record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficialEquipMeta {
    /// Slot index.
    pub slot: u8,
    /// Associated challenge quest ID.
    pub quest_id: i32,
    /// Byte offset inside the blob.
    pub offset: u32,
    /// Byte length.
    pub len: u16,
}

/// Metadata for one header slice of the DLC section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficialHeaderSlice {
    /// Offset relative to the DLC section start.
    pub rel_offset: u16,
    /// Byte offset inside the blob.
    pub offset: u32,
    /// Byte length.
    pub len: u16,
}

/// No quests: the real table comes from the generated module.
pub static OFFICIAL_QUESTS: &[OfficialQuestMeta] = &[];
/// No equipment.
pub static OFFICIAL_EQUIPS: &[OfficialEquipMeta] = &[];
/// No header slices.
pub static OFFICIAL_HEADER_SLICES: &[OfficialHeaderSlice] = &[];

/// Bytes of one quest archive.
pub fn arc_bytes(_meta: &OfficialQuestMeta) -> &'static [u8] {
    &[]
}

/// Bytes of one equipment record.
pub fn equip_bytes(_meta: &OfficialEquipMeta) -> &'static [u8] {
    &[]
}

/// Bytes of one header slice.
pub fn header_slice_bytes(_meta: &OfficialHeaderSlice) -> &'static [u8] {
    &[]
}

/// Look a quest up by ID.
pub fn find_official_quest(quest_id: i32) -> Option<&'static OfficialQuestMeta> {
    OFFICIAL_QUESTS.iter().find(|q| q.quest_id == quest_id)
}

/// Japanese title of a quest.
pub fn official_quest_title(quest_id: i32) -> Option<&'static str> {
    find_official_quest(quest_id).map(|q| q.title)
}

/// Is the real bundle compiled in?
pub const AVAILABLE: bool = false;
