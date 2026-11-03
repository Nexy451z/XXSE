//! Headless editor state machine.
//!
//! This module contains the whole editing flow - navigation, field edits,
//! item box edits and quest (.mib / .gmd) edits - with **no input/output at
//! all**. The Switch build feeds it button presses and renders the lines it
//! returns; the host tests drive it with synthetic key presses against the
//! real save files. That makes the on-device behaviour testable without
//! hardware.
//!
//! The I/O layer is expected to:
//!
//! 1. call Editor::set_save_files / Editor::set_arc_files after scanning the SD card
//! 2. poll Editor::take_request and answer it with Editor::open_save_bytes
//!    or Editor::open_arc_bytes
//! 3. poll Editor::take_write and write those bytes to disk
//! 4. poll Editor::take_text_request, show the software keyboard, then call
//!    Editor::apply_text (or Editor::apply_name) with the result

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::arc::{extension_for, Arc};
use crate::check::{self, Finding, Severity};
use crate::charm::{self, Charm, CHARM_COUNT};
use crate::dlc;
use crate::dlcats::{self, Lang};
use crate::equipops::{self, EquipError};
use crate::equiprefs;
use crate::save::Options;
use crate::itembox::{self, DedupePreview, ItemSlot, MAX_COUNT, TOTAL_ITEM_SLOTS};
use crate::quest::{Gmd, Mib};
use crate::questfields::{self, FieldKind};
use crate::save::{Character, Save, SaveKind};

/// Which screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Save files found on the SD card.
    SaveList,
    /// Character slot picker.
    Slots,
    /// Scalar character fields.
    Char,
    /// Item box.
    Items,
    /// Quest .arc files found on the SD card.
    ArcList,
    /// Download quests stored inside the open save.
    SaveQuests,
    /// Entries inside the open .arc.
    ArcEntries,
    /// .mib field editor.
    Mib,
    /// .gmd text viewer / editor.
    Gmd,
    /// Quest checker results.
    Check,
    /// Charm box (お守り).
    Charms,
    /// Charm editor form.
    CharmEdit,
    /// Decoded view of one .rem / .sup / .sem / .esl / .qdp / .qdl entry.
    EntryView,
    /// Skill name picker for the talisman editor.
    SkillPick,
    /// 3DS <-> Switch conversion for the open save.
    Convert,
    /// The equipment box of the current character.
    Equip,
    /// The equipment editor form (level, transmog, decorations).
    EquipEdit,
    /// Decoration name picker, opened from the equipment form with Y.
    JewelPick,
    /// The 45 quest-equipment entries the DLC section carries.
    QuestEquip,
    /// Monster hunt, capture and size records.
    Hunts,
    /// The guild card and the arena log.
    Guild,
    /// The palico roster.
    Roster,
    /// Item sets and equipment sets.
    Sets,
    /// Item name search, opened from the item box with R.
    ItemPick,
    /// Download Palicoes (配信オトモ).
    Palico,
    /// Global options.
    Options,
}

/// A button press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L,
    R,
    /// Minus on the Switch pad.
    Minus,
    Plus,
    /// ZR on the Switch pad, or the Z key in the PC build.
    Z,
}

/// Something the I/O layer has to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Read the save file at this index in the file list.
    ReadSave(usize),
    /// Read the archive at this index in the archive list.
    ReadArc(usize),
    /// Write quest arcs out of the save. An empty list means every quest.
    ///
    /// The platform layer performs the write because only it knows where to put
    /// the files (SD card on the Switch, a folder on the PC).
    ExtractQuests {
        /// Quest ids to write, or empty for all of them.
        ids: Vec<i32>,
    },
}

/// Everything up to and including the last path separator.
///
/// The console paths use '/', the PC ones can use '\', and this code runs on
/// both, so both are accepted. Returns "" when there is no separator.
/// Is this path one of the two files inside the console's save area?
pub fn is_console_save_path(path: &str) -> bool {
    path == CONSOLE_SAVE || path == "nand:/system_backup" || path.starts_with("nand:")
}

fn split_dir(path: &str) -> String {
    match path.rfind(['/', '\\']) {
        Some(i) => path[..=i].to_string(),
        None => String::new(),
    }
}

/// Data the I/O layer must write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// Write these bytes to this path.
    File { path: String, bytes: Vec<u8> },
}

/// One display line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The text to draw.
    pub text: String,
    /// Can the cursor land here?
    pub selectable: bool,
    /// Is the cursor here right now?
    pub cursor: bool,
}

/// The charm currently being edited on the CharmEdit screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CharmEdit {
    /// Charm slot index (0..1999).
    pub index: usize,
    /// Talisman grade (rarity 97..100).
    pub grade: crate::charm::TalismanGrade,
    /// Decoration slots.
    pub slots: u8,
    /// First skill id.
    pub skill1: u8,
    /// First skill points.
    pub points1: i8,
    /// Second skill id.
    pub skill2: u8,
    /// Second skill points.
    pub points2: i8,
    /// Which row the cursor is on.
    pub field: usize,
}

/// Rows on the equipment form when an existing record is edited.
///
/// The first seven are the rows the form has always had; 種別 and ID were
/// added at the end rather than at the top so the seven existing rows keep
/// their numbers (the on-screen highlight, the decoration range and the
/// console audit all step by row number).
pub const EQUIP_EDIT_ROWS: usize = 9;

/// Rows on the equipment form when a record is being created.
///
/// A new record has no level word, layer or sockets to preserve yet, so the
/// form asks for the three things `equipops::create` writes: type, id, level.
pub const EQUIP_NEW_ROWS: usize = 3;

/// The equipment record currently being edited on the EquipEdit screen.
///
/// Only the first twelve bytes of a record are ever rewritten: the level word
/// at +0x00, the id at +0x02, the layer id at +0x04 and the three decoration
/// ids at +0x06, +0x08 and +0x0A. Everything from +0x0C on (talisman skills,
/// the kinsect, the charge blade phials) is copied through untouched, because
/// this save holds weapon records that carry talisman-shaped data there and
/// are in use.
///
/// Changing the type or the id is not a plain field edit: it goes through
/// `equipops::replace`, which empties the three sockets because they belonged
/// to the item that was there. A record being created goes through
/// `equipops::create`, which writes the first blank row of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EquipEdit {
    /// Box slot index (0..1999). Unused while `new` is set.
    pub index: usize,
    /// Equipment type, 1..=21 (see `equip::type_name`).
    pub kind: u8,
    /// Weapon, armour or talisman id at +0x02.
    pub id: u16,
    /// Level, 1..=32 (stored minus one in five bits).
    pub level: u16,
    /// Layered (transmog) id at +0x04.
    pub transmog_id: u16,
    /// Level of the piece the layer came from, 1..=32.
    pub transmog_level: u16,
    /// Decoration ids at +0x06, +0x08 and +0x0A. Zero is an empty socket.
    pub decorations: [u16; 3],
    /// Which row the cursor is on.
    ///
    /// Editing: 0 枠 / 1 レベル / 2 着せ替えID / 3 着せ替え元Lv /
    /// 4..6 装飾品1..3 / 7 種別 / 8 ID.
    /// Creating: 0 種別 / 1 ID / 2 レベル.
    pub field: usize,
    /// Is the form making a new record rather than editing one?
    pub new: bool,
}

impl EquipEdit {
    /// How many rows the form shows in its current mode.
    pub fn rows(&self) -> usize {
        if self.new {
            EQUIP_NEW_ROWS
        } else {
            EQUIP_EDIT_ROWS
        }
    }

    /// Which decoration socket a row writes, if any.
    ///
    /// Only the editing form has sockets; a record that does not exist yet
    /// cannot carry a decoration id.
    pub fn decoration_socket(&self, field: usize) -> Option<usize> {
        if self.new {
            return None;
        }
        if (4..=6).contains(&field) {
            Some(field - 4)
        } else {
            None
        }
    }
}

/// Rows on the equipment set editor: 0 is the name, 1..=7 the seven parts.
pub const SET_EDIT_ROWS: usize = 1 + crate::myset::EQUIP_SET_PARTS;

/// The set editor's row that renames the set.
pub const SET_ROW_NAME: usize = 0;

/// The equipment set being edited on the Sets screen.
///
/// This is a **mode of the my-sets screen**, not a screen of its own: opening
/// it does not add a variant to `Screen` (and therefore does not have to be
/// taught to every match on it or to the console audit), and the list
/// underneath keeps its cursor - the console layer owns that one, because the
/// rows it shows are the records that are not empty.
///
/// `pick` is what [A] would write: one equipment box index per part, seeded
/// from the record when the editor opens. It is kept apart from the record on
/// purpose. `myset::set_part` writes the part **and** copies that record's
/// decoration ids into the set, so merely walking the rows must not write;
/// only [A] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetEdit {
    /// Index into the equipment set array (0..EQUIP_SETS).
    pub set: usize,
    /// Which row the cursor is on: 0 name, 1..=7 weapon..talisman.
    pub field: usize,
    /// Box index [A] would write, per part. `myset::NONE` is an empty part.
    pub pick: [u16; crate::myset::EQUIP_SET_PARTS],
}

impl Default for SetEdit {
    fn default() -> Self {
        SetEdit {
            set: 0,
            field: SET_ROW_NAME,
            // Nothing selected, not box record 0: 0 is a real equipment slot.
            pick: [crate::myset::NONE; crate::myset::EQUIP_SET_PARTS],
        }
    }
}

/// A text edit request (software keyboard).
///
/// index == usize::MAX means the character name of the current slot.
pub type TextRequest = (usize, String);

/// Sentinel id for "ask the player's name".
pub const TEXT_NAME: usize = usize::MAX;

/// Monster name for a hunt-record index, or a placeholder when unavailable.
pub fn monster_name(index: usize) -> &'static str {
    #[cfg(feature = "gamedata")]
    {
        crate::gamedata::MONSTER_HUNT_NAMES
            .get(index)
            .copied()
            .unwrap_or("?")
    }
    #[cfg(not(feature = "gamedata"))]
    {
        let _ = index;
        "?"
    }
}

/// Display name for an item id, or a placeholder when the table is absent.
#[cfg(feature = "gamedata")]
pub fn item_name_of(id: u16) -> alloc::string::String {
    crate::gamedata::ITEM_NAME_LIST
        .get(id as usize)
        .map(|s| alloc::string::ToString::to_string(*s))
        .unwrap_or_else(|| alloc::format!("(unknown {})", id))
}

/// Display name for an item id, or a placeholder when the table is absent.
#[cfg(not(feature = "gamedata"))]
pub fn item_name_of(id: u16) -> alloc::string::String {
    alloc::format!("#{}", id)
}

/// Every decoration id the name table knows, empty socket (0) excluded.
pub fn decoration_ids() -> Vec<u16> {
    #[cfg(feature = "gamedata")]
    {
        // JWL_I_DS[0] is 0 ("(None)"), which the picker offers separately.
        crate::gamedata::JWL_I_DS
            .iter()
            .filter(|id| **id > 0 && **id <= u16::MAX as i32)
            .map(|id| *id as u16)
            .collect()
    }
    #[cfg(not(feature = "gamedata"))]
    {
        Vec::new()
    }
}

/// Display name for a decoration id (2638 + k), or a placeholder.
///
/// JWL_I_DS says which ids are decorations rather than computing 2638 + k
/// (the table is not dense), and the Japanese item table - which carries the
/// same ids - gives the name the game shows. JWL_NAMES is the English list and
/// is only the fallback.
pub fn decoration_name_of(id: u16) -> alloc::string::String {
    if id == 0 {
        return String::from("なし");
    }
    #[cfg(feature = "gamedata")]
    {
        if let Some(i) = crate::gamedata::JWL_I_DS.iter().position(|v| *v == id as i32) {
            if let Some(name) = crate::gamedata::ITEM_NAME_LIST.get(id as usize) {
                if !name.is_empty() {
                    return alloc::string::ToString::to_string(*name);
                }
            }
            if let Some(name) = crate::gamedata::JWL_NAMES.get(i) {
                return alloc::string::ToString::to_string(*name);
            }
        }
    }
    alloc::format!("#{}", id)
}

/// Sentinel id for "ask for an item name to filter the picker by".
pub const TEXT_ITEM_FILTER: usize = usize::MAX - 1;
/// Sentinel id for "ask for a skill name to filter the picker by".
pub const TEXT_SKILL_FILTER: usize = usize::MAX - 2;
/// Sentinel id for "ask for a quest id or title to filter the list by".
pub const TEXT_QUEST_FILTER: usize = usize::MAX - 3;
/// Sentinel id for "ask what to call the converted save".
pub const TEXT_SAVE_NAME: usize = usize::MAX - 4;
/// Sentinel base for "ask for the name of palico N": N is added to this.
pub const TEXT_PALICO_NAME: usize = usize::MAX - 1_000;
/// Sentinel id for "ask for the guild card comment".
pub const TEXT_GUILD_COMMENT: usize = usize::MAX - 999;
/// Sentinel id for "ask which equipment to show".
pub const TEXT_EQUIP_FILTER: usize = usize::MAX - 5;
/// Sentinel id for "ask for a decoration name to filter the jewel picker by".
pub const TEXT_JEWEL_FILTER: usize = usize::MAX - 6;
/// Sentinel id for "ask for the name of the equipment set being edited".
///
/// It has to stay **below** `TEXT_PALICO_NAME`, because the console layer
/// reads one contiguous range as palico names ("TEXT_PALICO_NAME or above");
/// a sentinel above it would be answered as a palico that does not exist.
pub const TEXT_SET_NAME: usize = usize::MAX - 1_001;

/// How many terminal cells one character takes.
///
/// The wide scripts this editor prints - Japanese above all - take two cells,
/// everything else one. Used only to line the ASCII fallback console up; the
/// canvas measures real glyphs instead.
fn char_cells(c: char) -> usize {
    let u = c as u32;
    let wide = (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE6F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x3000..=0x303F).contains(&u);
    if wide {
        2
    } else {
        1
    }
}

/// Pad text to a number of console cells, cutting it when it does not fit.
///
/// Cells, not characters: padding "護石" to five columns with spaces would put
/// the next column two cells off, which is what the first version did.
fn pad_cells(s: &str, cells: usize) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = char_cells(ch);
        if used + w > cells {
            break;
        }
        out.push(ch);
        used += w;
    }
    while used < cells {
        out.push(' ');
        used += 1;
    }
    out
}

/// Cut text to a character count for the fixed-width console lines.
///
/// Counted in characters, not bytes: these lines feed either the Switch's
/// ASCII fallback console or the test reader, both of which count cells, and a
/// byte cut would split a three byte character in half.
fn clip_text(s: &str, max_chars: usize) -> alloc::string::String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: alloc::string::String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Add `step` to a value, staying in the u16 range and never going below zero.
fn clamp_u16(value: u16, step: i64) -> u16 {
    let v = value as i64 + step;
    v.clamp(0, u16::MAX as i64) as u16
}

/// Step the equipment type through 1..=21, the range a record can hold.
///
/// 12 is deliberately unused by the game (see `equip::type_name`), and it is
/// still one of the reachable values: the field is five bits wide and an
/// editor that silently skipped a number would be lying about the field.
fn cycle_equip_kind(kind: u8, d: i64) -> u8 {
    let cur = if (1..=21).contains(&kind) { kind as i64 } else { 1 };
    ((cur - 1 + d).rem_euclid(21) + 1) as u8
}

/// Step a five bit level field, which is stored minus one: 1..=32 wraps.
fn cycle_equip_level(level: u16, d: i64) -> u16 {
    let cur = level.clamp(1, 32) as i64;
    ((cur - 1 + d).rem_euclid(32) + 1) as u16
}

/// Label for the monster under the hunt cursor, for the status line.
fn monster_label(rows: &[(usize, u16, u16, u16, u16)], cursor: usize) -> usize {
    rows.get(cursor).map(|r| r.0).unwrap_or(0)
}
/// Quest id taken from an archive's .mib.
///
/// The id is the first field of the .mib, so an archive can be placed in the
/// right slot whatever its file name says.
pub fn quest_id_of_arc(bytes: &[u8]) -> Option<i32> {
    let arc = crate::arc::Arc::parse(bytes).ok()?;
    let e = arc
        .entries
        .iter()
        .find(|e| crate::arc::extension_for(e.type_hash) == Some("mib"))?;
    let b = e.data.get(0x0C..0x10)?;
    Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Pseudo path that stands for the console's own save, not a file on the SD card.
pub const CONSOLE_SAVE: &str = "nand:/system";

/// The editable character fields, in menu order.
pub const CHAR_FIELDS: &[(&str, &str)] = &[
    ("name", "名前"),
    ("funds", "所持金"),
    ("play_time", "プレイ時間(秒)"),
    ("hr", "ハンターランク"),
    ("hrp", "HRポイント"),
    ("academy", "アカデミー点数"),
    ("berna", "ベルナポイント"),
    ("kokoto", "ココットポイント"),
    ("pokke", "ポッケポイント"),
    ("yukumo", "ユクモポイント"),
    ("voice", "ボイス"),
    ("eye", "瞳の色"),
    ("cloth", "服"),
    ("gender", "性別"),
    ("hair", "髪型"),
    ("face", "顔"),
    ("feature", "特徴"),
];

/// Adjustment steps cycled with X.
pub const STEPS: [i64; 5] = [1, 10, 100, 1_000, 10_000];

/// Steps the hunt screen cycles through: counts are small, so 10000 is useless.
const HUNT_STEPS: [i64; 5] = [1, 10, 100, 1000, -1];

// ---------------------------------------------------------------------------
// Item box bulk operations
//
// The item screen shows one row per box record; these are the edits that touch
// every record at once. They live here (not in the console UI) so the Switch
// build, the PC build and the tests all run the same code, and so the two rows
// that can drop data can say what they would drop before they run.
// ---------------------------------------------------------------------------

/// The rows of the item box operation menu, opened with `-`.
///
/// The first four are the requested bulk edits, then the two rows that do not
/// fit that shape: adding through the picker and taking the last edit back.
/// Order is menu order; the constants below name the rows.
pub const ITEM_OPS: [(&str, &str); 6] = [
    (
        "すべて 99 に",
        "所持している全枠の個数を 99 にそろえます（空き枠はそのまま）",
    ),
    (
        "重複を整理",
        "同じアイテムの枠を1つにまとめます。99 を超えた分は失われます",
    ),
    (
        "空きを詰める",
        "空き枠を後ろへ寄せ、所持品を前から順に並べ直します",
    ),
    (
        "上限を丸める",
        "99 を超えている個数を 99 に戻します（空き枠は触りません）",
    ),
    (
        "アイテムを追加",
        "名前で選んで個数を決め、空き枠へまとめて追加します",
    ),
    (
        "直前の操作を元に戻す",
        "この画面での直前の変更を1つ取り消します",
    ),
];

/// "すべて 99 に" - itembox::fill_all.
pub const ITEM_OP_FILL: usize = 0;
/// "重複を整理" - itembox::dedupe, behind a preview and a second press.
pub const ITEM_OP_DEDUPE: usize = 1;
/// "空きを詰める" - itembox::compact.
pub const ITEM_OP_COMPACT: usize = 2;
/// "上限を丸める" - itembox::clamp_counts.
pub const ITEM_OP_CLAMP: usize = 3;
/// "アイテムを追加" - opens the picker in its two-stage add mode (mode 3).
pub const ITEM_OP_ADD: usize = 4;
/// "直前の操作を元に戻す" - pops Editor::item_undo.
pub const ITEM_OP_UNDO: usize = 5;

/// What the operation menu shows next to each row, read from the box once.
///
/// The console redraws this every frame, so the numbers are collected in one
/// pass instead of one decode per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemMenuStats {
    /// Used records below the cap: what "すべて 99 に" would raise.
    pub fill: usize,
    /// Blank records: what "空きを詰める" would close.
    pub holes: usize,
    /// Used records above the cap: what "上限を丸める" would pull down.
    pub over: usize,
    /// What "重複を整理" would free and drop.
    pub dedupe: DedupePreview,
}

impl ItemMenuStats {
    /// The short right-hand text for one menu row, empty when nothing to do.
    pub fn value(&self, op: usize) -> String {
        match op {
            ITEM_OP_FILL if self.fill > 0 => format!("{} 枠", self.fill),
            ITEM_OP_DEDUPE => format!("{} 枠 / {} 個", self.dedupe.freed, self.dedupe.lost),
            ITEM_OP_COMPACT if self.holes > 0 => format!("{} 枠", self.holes),
            ITEM_OP_CLAMP if self.over > 0 => format!("{} 枠", self.over),
            _ => String::new(),
        }
    }
}

/// How many items the box holds in total.
fn box_total(items: &[ItemSlot]) -> u32 {
    items
        .iter()
        .filter(|s| !s.is_blank())
        .map(|s| s.count as u32)
        .sum()
}

/// The editor.
pub struct Editor {
    /// Current screen.
    pub screen: Screen,
    /// Cursor row inside the current screen.
    pub cursor: usize,
    /// Selected character slot (0..2).
    pub slot: usize,
    /// Selected character field index.
    pub field: usize,
    /// Index into STEPS.
    pub step: usize,
    /// Show empty item slots?
    pub show_empty: bool,
    /// Selected .arc entry.
    pub entry: usize,
    /// Cursor inside the .mib field list.
    pub mib_cursor: usize,
    /// Cursor inside the .gmd string list.
    pub gmd_cursor: usize,
    /// Cursor inside the checker result list.
    pub check_cursor: usize,
    /// Problems found by the checker.
    pub findings: Vec<Finding>,
    /// Cursor inside the charm list.
    pub charm_cursor: usize,
    /// The charm being edited, if the editor form is open.
    pub charm_edit: Option<CharmEdit>,
    /// Cached quest titles: (quest id, title). Decoded once when the list
    /// opens, because inflating a .gmd for every visible row each frame is
    /// far too slow on the console.
    pub quest_titles: Vec<(i32, alloc::string::String)>,
    /// Raw item boxes for undo, newest last.
    ///
    /// 5463 bytes each, so the stack is deliberately shallow.
    pub item_undo: Vec<Vec<u8>>,
    /// Which talisman skill the picker writes: 1 or 2.
    pub skill_pick_field: u8,
    /// What the item picker writes: 0 = the item box, 1 = a record inside
    /// the .rem / .sup entry that is being viewed.
    pub item_pick_mode: u8,
    /// Which reward record the picker will write.
    pub entry_item_row: usize,
    /// Cursor over the records of the open reward table.
    pub entry_row_cursor: usize,
    /// File name chosen for the converted save, without path or extension.
    pub convert_name: String,
    /// Hunt record cursor and the working copies.
    pub hunt_cursor: usize,
    /// Which of the four numbers (slay, capture, smallest, largest) is being
    /// changed on the hunt screen.
    pub hunt_field: usize,
    /// Palico roster cursor.
    pub roster_cursor: usize,
    /// Which palico field (level, bond, experience) Left/Right changes.
    pub roster_field: usize,
    /// Which guild card field (rank, clock, comment) is being changed.
    pub guild_field: usize,
    /// Whether the quest-equipment screen is showing the items of one slot.
    pub qe_open: bool,
    /// Cursor inside that item list.
    pub qe_item_cursor: usize,
    /// The entry the item picker will write to when it is opened from there.
    pub qe_target: crate::dlc::QuestEquipItem,
    /// Equipment list filter and cursor.
    pub equip_filter: String,
    /// Cursor over the filtered equipment list.
    pub equip_cursor: usize,
    /// The equipment record being edited, if the form is open.
    pub equip_edit: Option<EquipEdit>,
    /// Box index of a record whose delete is waiting for its second press.
    ///
    /// A delete is never one press: the first press only says what would go
    /// and which worn slots / saved sets still point at it, and the second one
    /// performs it. Any cursor move drops the pending index so a press can
    /// never land on a record the user has stopped looking at.
    pub equip_delete_armed: Option<usize>,
    /// Decoration name filter used by the jewel picker.
    pub jewel_pick_filter: String,
    /// Cursor inside the filtered decoration list.
    pub jewel_pick_cursor: usize,
    /// Quest id / title filter for the save's quest list.
    pub quest_filter: String,
    /// Skill name filter used by the picker.
    pub skill_pick_filter: String,
    /// Cursor inside the filtered skill list.
    pub skill_pick_cursor: usize,
    /// Item name filter used by the picker.
    pub item_pick_filter: String,
    /// Cursor inside the filtered item list.
    pub item_pick_cursor: usize,
    /// Box index the picker will write to.
    pub item_pick_target: usize,
    /// Which stage of the bulk add the picker is in: 0 picks the item,
    /// 1 picks how many of it to add (only while item_pick_mode == 3).
    pub item_pick_stage: u8,
    /// The item the bulk add will put into the box.
    pub item_pick_add_id: u16,
    /// How many of it, chosen with Left / Right in stage 1.
    pub item_pick_add_count: u16,
    /// Is the item box operation menu open?
    pub item_menu_open: bool,
    /// Which row of that menu is highlighted.
    pub item_menu_cursor: usize,
    /// Has the dedupe preview been shown and its second press not arrived yet?
    pub item_dedupe_armed: bool,
    /// The equipment set being edited, if the set editor is open.
    ///
    /// A mode of the my-sets screen, like `item_menu_open` is a mode of the
    /// item screen. `None` means the screen shows the list.
    pub set_edit: Option<SetEdit>,
    /// Part whose [-] clear is waiting for its second press.
    ///
    /// A clear is never one press, for the same reason an equipment delete is
    /// not: the first press only says which part would empty, and any other
    /// key drops the pending one so a press cannot land on a part the user has
    /// stopped looking at.
    pub set_clear_armed: Option<usize>,
    /// What `myset::warnings` last said about the set on screen.
    ///
    /// Collected when the editor opens, after every write and on [X], never
    /// while drawing: `warnings` decodes the whole record and looks seven
    /// parts up in the equipment box, which is more than a redraw should do.
    pub set_warnings: Vec<String>,
    /// Cursor inside the save's own quest list.
    pub save_quest_cursor: usize,
    /// Cursor inside the download Palico catalogue.
    pub palico_cursor: usize,
    /// 0 = Japanese, 1 = English.
    pub palico_lang: u8,
    /// Cursor inside the options list.
    pub option_cursor: usize,
    /// Save files offered by the I/O layer.
    pub save_files: Vec<String>,
    /// Archive files offered by the I/O layer.
    pub arc_files: Vec<String>,
    /// The open save.
    pub save: Option<Save>,
    /// Path of the open save.
    pub save_path: Option<String>,
    /// The open archive.
    pub arc: Option<Arc>,
    /// Path of the open archive.
    pub arc_path: Option<String>,
    /// Set when the user asked to leave.
    pub quit: bool,
    /// Anything changed since the last write?
    pub dirty: bool,
    /// Status line.
    pub message: String,

    pending_request: Option<Request>,
    pending_write: Option<Write>,
    pending_text: Option<TextRequest>,
    /// Working copy of the .mib bytes.
    pub mib_bytes: Vec<u8>,
    /// Working copy of the .gmd strings.
    pub gmd: Option<Gmd>,
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

impl Editor {
    /// Create an empty editor.
    pub fn new() -> Self {
        Editor {
            screen: Screen::SaveList,
            cursor: 0,
            slot: 0,
            field: 0,
            step: 0,
            show_empty: false,
            entry: 0,
            mib_cursor: 0,
            gmd_cursor: 0,
            check_cursor: 0,
            findings: Vec::new(),
            charm_cursor: 0,
            charm_edit: None,
            quest_titles: Vec::new(),
            item_undo: Vec::new(),
            skill_pick_field: 1,
            convert_name: String::new(),
            hunt_cursor: 0,
            hunt_field: 0,
            roster_cursor: 0,
            roster_field: 0,
            guild_field: 0,
            qe_open: false,
            qe_item_cursor: 0,
            qe_target: crate::dlc::QuestEquipItem {
                record: 0,
                list: crate::dlc::QuestEquipList::Supply,
                index: 0,
                item: 0,
                count: 0,
            },
            equip_filter: String::new(),
            equip_cursor: 0,
            equip_edit: None,
            equip_delete_armed: None,
            jewel_pick_filter: String::new(),
            jewel_pick_cursor: 0,
            item_pick_mode: 0,
            entry_item_row: 0,
            entry_row_cursor: 0,
            quest_filter: String::new(),
            skill_pick_filter: String::new(),
            skill_pick_cursor: 0,
            item_pick_filter: String::new(),
            item_pick_cursor: 0,
            item_pick_target: 0,
            item_pick_stage: 0,
            item_pick_add_id: 0,
            item_pick_add_count: 1,
            item_menu_open: false,
            item_menu_cursor: 0,
            item_dedupe_armed: false,
            set_edit: None,
            set_clear_armed: None,
            set_warnings: Vec::new(),
            save_quest_cursor: 0,
            palico_cursor: 0,
            palico_lang: 0,
            option_cursor: 0,
            save_files: Vec::new(),
            arc_files: Vec::new(),
            save: None,
            save_path: None,
            arc: None,
            arc_path: None,
            quit: false,
            dirty: false,
            message: String::new(),
            pending_request: None,
            pending_write: None,
            pending_text: None,
            mib_bytes: Vec::new(),
            gmd: None,
        }
    }

    /// Replace the save file list.
    pub fn set_save_files(&mut self, files: Vec<String>) {
        self.save_files = files;
        if self.cursor >= self.save_files.len() {
            self.cursor = 0;
        }
    }

    /// Replace the archive file list.
    pub fn set_arc_files(&mut self, files: Vec<String>) {
        self.arc_files = files;
        if self.cursor >= self.arc_files.len() {
            self.cursor = 0;
        }
    }

    /// Take a pending I/O request.
    pub fn take_request(&mut self) -> Option<Request> {
        self.pending_request.take()
    }

    /// Take a pending write.
    pub fn take_write(&mut self) -> Option<Write> {
        self.pending_write.take()
    }

    /// Take a pending software-keyboard request.
    pub fn take_text_request(&mut self) -> Option<TextRequest> {
        self.pending_text.take()
    }

    /// The I/O layer answers a .gmd text request with this.
    pub fn apply_text(&mut self, index: usize, text: String) {
        if let Some(g) = &mut self.gmd {
            if index < g.strings.len() {
                g.strings[index] = text;
                self.dirty = true;
                self.message = String::from("テキストを更新しました（X で .arc に書き込み）");
            }
        }
    }

    /// The I/O layer answers a character-name request with this.
    pub fn apply_name(&mut self, text: String) {
        if let Some(mut c) = self.character() {
            c.name = text;
            c.slot = self.slot;
            if let Some(s) = &mut self.save {
                let _ = s.write_character(&c);
                self.dirty = true;
                self.message = String::from("名前を更新しました（A でセーブに書き込み）");
            }
        }
    }

    // ------------------------------------------------------------- loading

    /// Feed raw save bytes.
    pub fn open_save_bytes(&mut self, path: &str, bytes: &[u8]) {
        match Save::from_bytes(bytes) {
            Err(e) => self.message = format!("not a MHXX save: {}", e),
            Ok(s) => {
                self.slot = (0..3).find(|i| s.slot_used(*i)).unwrap_or(0);
                self.message = format!("loaded {} bytes from {}", bytes.len(), path);
                self.save = Some(s);
                self.save_path = Some(path.to_string());
                self.dirty = false;
                self.screen = Screen::Slots;
                self.cursor = 0;
            }
        }
    }

    /// Does the current screen consume the Minus button itself?
    ///
    /// The Switch layer uses Minus to rescan folders everywhere else. The
    /// equipment list uses it for the two-press delete and the item screen
    /// uses it for the bulk operation menu, so the rescan must not fire there
    /// (on the item screen it used to fire *and* undo, both at once).
    pub fn uses_minus(&self) -> bool {
        match self.screen {
            Screen::ArcEntries | Screen::SaveQuests | Screen::Equip | Screen::Items => true,
            // The set editor uses Minus for the two-press part clear. On the
            // list above it Minus still means "rescan".
            Screen::Sets => self.set_edit.is_some(),
            _ => false,
        }
    }

    /// The I/O layer answers an ExtractQuests request with this.
    pub fn apply_extracted(&mut self, written: usize, dir: &str) {
        self.message = if written == 0 {
            format!("nothing written to {} - no matching quest in this save", dir)
        } else {
            format!("wrote {} quest file(s) to {}", written, dir)
        };
    }

    /// Feed raw archive bytes.
    pub fn open_arc_bytes(&mut self, path: &str, bytes: &[u8]) {
        match Arc::parse(bytes) {
            Err(e) => self.message = format!("not an .arc: {}", e),
            Ok(a) => {
                let idx = a
                    .entries
                    .iter()
                    .position(|e| extension_for(e.type_hash) == Some("mib"))
                    .unwrap_or(0);
                self.message = format!("{} entries", a.entries.len());
                self.arc = Some(a);
                self.arc_path = Some(path.to_string());
                self.entry = idx;
                self.screen = Screen::ArcEntries;
                self.cursor = idx;
                self.load_entry();
            }
        }
    }

    /// Load the selected entry into the working copies.
    fn load_entry(&mut self) {
        self.mib_bytes.clear();
        self.gmd = None;
        let Some(a) = &self.arc else { return };
        let Some(e) = a.entries.get(self.entry) else { return };
        match extension_for(e.type_hash) {
            Some("mib") => {
                if let Ok(m) = Mib::from_bytes(&e.data) {
                    self.mib_bytes = m.raw.clone();
                }
            }
            Some("gmd") => {
                if let Ok(g) = Gmd::from_bytes(&e.data) {
                    self.gmd = Some(g);
                }
            }
            _ => {}
        }
        self.mib_cursor = 0;
        self.gmd_cursor = 0;
    }

    /// Push the working copies back into the archive.
    pub fn store_entry(&mut self) {
        let Some(a) = &mut self.arc else { return };
        let Some(e) = a.entries.get_mut(self.entry) else { return };
        if !self.mib_bytes.is_empty() && extension_for(e.type_hash) == Some("mib") {
            e.data = self.mib_bytes.clone();
        } else if let Some(g) = &self.gmd {
            e.data = g.to_bytes();
        }
    }

    /// The .mib working bytes (for tests).
    pub fn mib_bytes(&self) -> &[u8] {
        &self.mib_bytes
    }

    // ------------------------------------------------------------ get/set

    /// Read the currently selected character slot from the open save.
    pub fn character(&self) -> Option<Character> {
        self.save.as_ref()?.read_character(self.slot)
    }

    /// Numeric value of a character field.
    pub fn char_value(&self, field: usize) -> i64 {
        let Some(c) = self.character() else { return 0 };
        match field {
            1 => c.funds as i64,
            2 => c.play_time as i64,
            3 => c.hunter_rank as i64,
            4 => c.hr_points as i64,
            5 => c.academy_points as i64,
            6..=9 => c.village_points[field - 6] as i64,
            10 => c.voice as i64,
            11 => c.eye_color as i64,
            12 => c.clothing as i64,
            13 => c.gender as i64,
            14 => c.hairstyle as i64,
            15 => c.face as i64,
            16 => c.features as i64,
            _ => 0,
        }
    }

    /// Clamp a value to the range of the given field.
    pub fn clamp_char(field: usize, v: i64) -> i64 {
        match field {
            1 => v.clamp(0, 9_999_999),
            2 => v.clamp(0, 100_000_000),
            3 => v.clamp(0, 999),
            4 => v.clamp(0, 99_999_999),
            5 => v.clamp(0, 9_999_999),
            6..=9 => v.clamp(0, 9_999_999),
            10..=16 => v.clamp(0, 255),
            _ => v,
        }
    }

    /// Set a numeric character field.
    pub fn set_char_value(&mut self, field: usize, v: i64) {
        let Some(c) = self.character() else { return };
        let mut c = c;
        let v = Self::clamp_char(field, v);
        match field {
            1 => c.funds = v as i32,
            2 => c.play_time = v as i32,
            3 => c.hunter_rank = v as u16,
            4 => c.hr_points = v as i32,
            5 => c.academy_points = v as i32,
            6..=9 => c.village_points[field - 6] = v as i32,
            10 => c.voice = v as u8,
            11 => c.eye_color = v as u8,
            12 => c.clothing = v as u8,
            13 => c.gender = v as u8,
            14 => c.hairstyle = v as u8,
            15 => c.face = v as u8,
            16 => c.features = v as u8,
            _ => {}
        }
        c.slot = self.slot;
        if let Some(s) = &mut self.save {
            let _ = s.write_character(&c);
            self.dirty = true;
        }
    }

    /// Indices of the item slots currently visible.
    pub fn visible_items(&self) -> Vec<usize> {
        let Some(items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return Vec::new();
        };
        items
            .iter()
            .enumerate()
            .filter(|(_, s)| self.show_empty || s.id != 0)
            .map(|(i, _)| i)
            .collect()
    }

    // ------------------------------------------------------------- input

    /// Feed one button press.
    pub fn input(&mut self, key: Key) {
        match self.screen {
            Screen::SaveList => self.input_save_list(key),
            Screen::Slots => self.input_slots(key),
            Screen::Char => self.input_char(key),
            Screen::Items => self.input_items(key),
            Screen::ArcList => self.input_arc_list(key),
            Screen::SaveQuests => self.input_save_quests(key),
            Screen::ItemPick => self.input_item_pick(key),
            Screen::SkillPick => self.input_skill_pick(key),
            Screen::Convert => self.input_convert(key),
            Screen::Equip => self.input_equip(key),
            Screen::EquipEdit => self.input_equip_edit(key),
            Screen::JewelPick => self.input_jewel_pick(key),
            Screen::QuestEquip => self.input_quest_equip(key),
            Screen::Hunts => self.input_hunts(key),
            Screen::Guild => self.input_guild(key),
            Screen::Roster => self.input_roster(key),
            Screen::Sets => self.input_sets(key),
            Screen::EntryView => match key {
                Key::B => {
                    self.screen = Screen::ArcEntries;
                    self.cursor = self.entry;
                }
                Key::Up => {
                    self.entry_row_cursor = self.entry_row_cursor.saturating_sub(1);
                }
                Key::Down => {
                    self.entry_row_cursor += 1;
                }
                // The console could swap the item but not say how many of it
                // the quest hands out; the count was printed and fixed.
                Key::Left => self.adjust_entry_count(-1),
                Key::Right => self.adjust_entry_count(1),
                // Only the reward and supply tables hold editable records.
                Key::A => {
                    let ext = self
                        .arc
                        .as_ref()
                        .and_then(|a| a.entries.get(self.entry))
                        .and_then(|e| extension_for(e.type_hash));
                    if matches!(ext, Some("rem") | Some("sup")) {
                        self.entry_item_row = self.entry_row_cursor;
                        self.item_pick_mode = 1;
                        self.item_pick_cursor = 0;
                        self.item_pick_filter.clear();
                        self.screen = Screen::ItemPick;
                        self.message = alloc::format!(
                            "{} 行目のアイテムを選んでください",
                            self.entry_row_cursor
                        );
                    } else {
                        self.message = String::from("このエントリに編集できる項目はありません");
                    }
                }
                // X was advertised on this screen but had no arm, so pressing
                // it did nothing at all.
                Key::X => self.request_write_arc(),
                Key::Plus => self.quit = true,
                _ => {}
            },
            Screen::ArcEntries => self.input_arc_entries(key),
            Screen::Mib => self.input_mib(key),
            Screen::Gmd => self.input_gmd(key),
            Screen::Check => self.input_check(key),
            Screen::Charms => self.input_charms(key),
            Screen::CharmEdit => self.input_charm_edit(key),
            Screen::Palico => self.input_palico(key),
            Screen::Options => self.input_options(key),
        }
    }

    /// Open the charm list for the current slot.
    pub fn open_charms(&mut self) {
        self.charm_cursor = 0;
        self.charm_edit = None;
        self.screen = Screen::Charms;
        self.cursor = 0;
        if self.charms().is_empty() {
            self.message = String::from("お守りがありません（Y で作成）");
        }
    }

    /// The talismans of the current slot.
    pub fn charms(&self) -> Vec<Charm> {
        self.save.as_ref().and_then(|s| s.charms(self.slot)).unwrap_or_default()
    }

    /// Open the charm form, either on the selected charm or on a free slot.
    fn open_charm_editor(&mut self, fresh: bool) {
        let list = self.charms();
        // The box also holds weapons and armour. Never pick one of those: ask
        // the charm module for a record that is empty or already a talisman.
        let free = self
            .save
            .as_ref()
            .and_then(|s| s.equipment_box(self.slot))
            .and_then(charm::first_writable)
            .unwrap_or(CHARM_COUNT - 1);
        let base = if fresh {
            Charm {
                index: free,
                ..Default::default()
            }
        } else {
            list.get(self.cursor).copied().unwrap_or(Charm {
                index: free,
                ..Default::default()
            })
        };
        // A brand new talisman starts as a copy of a real one, skills included.
        // Inventing "Poison +1 at rarity 100" produced a record the game's
        // per-rarity skill ranges do not allow; copying keeps it legal until
        // the user actually changes a field.
        let seed = if fresh {
            self.save
                .as_ref()
                .and_then(|s| s.equipment_box(self.slot))
                .and_then(charm::template_from_box)
                .map(|t| Charm {
                    index: free,
                    slots: t[0x10],
                    skill1: t[0x0C],
                    points1: t[0x0E] as i8,
                    skill2: t[0x0D],
                    points2: t[0x0F] as i8,
                })
                .unwrap_or(Charm {
                    index: free,
                    ..Default::default()
                })
        } else {
            base
        };
        // New talismans start on the grade of the record they were seeded
        // from, so the console stops defaulting every one to rarity 100.
        let grade = self
            .save
            .as_ref()
            .and_then(|s| s.equipment_box(self.slot))
            .and_then(|b| {
                let o = seed.index * crate::charm::CHARM_RECORD;
                b.get(o..o + crate::charm::CHARM_RECORD)
                    .and_then(crate::charm::rarity_of_record)
            })
            .unwrap_or_default();
        self.charm_edit = Some(CharmEdit {
            index: seed.index,
            grade,
            slots: seed.slots,
            skill1: seed.skill1,
            points1: seed.points1,
            skill2: seed.skill2,
            points2: seed.points2,
            // Land on the first skill: the grade and slot rows are quicker to
            // reach than to pass through on every visit.
            field: 3,
        });
        self.screen = Screen::CharmEdit;
        self.cursor = 0;
    }

    fn input_charms(&mut self, key: Key) {
        let list = self.charms();
        let n = list.len();
        match key {
            Key::Up => self.move_cursor(n, -1),
            Key::Down => self.move_cursor(n, 1),
            Key::A => {
                if let Some(c) = list.get(self.cursor) {
                    let idx = c.index;
                    let slot = self.slot;
                    let ok = match &mut self.save {
                        Some(s) => s.clear_charm(slot, idx),
                        None => false,
                    };
                    if ok {
                        self.dirty = true;
                        if self.cursor > 0 && self.cursor >= n - 1 {
                            self.cursor = n.saturating_sub(2);
                        }
                        self.message = format!("deleted charm {}", idx);
                    } else {
                        self.message =
                            format!("could not delete {} - only a talisman can be removed", idx);
                    }
                }
            }
            // Y edits the charm under the cursor, L starts a brand new one in
            // the first free slot. The form itself can also move the slot
            // number, so both roads lead to the same place.
            Key::Y => self.open_charm_editor(false),
            Key::L => self.open_charm_editor(true),
            Key::X => self.request_write_save(),
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_charm_edit(&mut self, key: Key) {
        let Some(mut e) = self.charm_edit else {
            self.screen = Screen::Charms;
            return;
        };
        match key {
            Key::Up => e.field = (e.field + 6) % 7,
            Key::Down => e.field = (e.field + 1) % 7,
            Key::Left | Key::Right => {
                let d: i64 = if key == Key::Right { 1 } else { -1 } * STEPS[self.step].min(10).max(1);
                match e.field {
                    0 => e.index = ((e.index as i64 + d).rem_euclid(CHARM_COUNT as i64)) as usize,
                    // The grade decides the slot ceiling and the points
                    // ranges, so moving it re-clamps both straight away
                    // instead of writing a record the game would refuse.
                    1 => {
                        let all = crate::charm::TalismanGrade::ALL;
                        let at = all.iter().position(|g| *g == e.grade).unwrap_or(3);
                        let next = if d > 0 {
                            (at + 1) % all.len()
                        } else {
                            (at + all.len() - 1) % all.len()
                        };
                        e.grade = all[next];
                        e.slots = e.slots.min(e.grade.max_slots());
                        for (skill, pts, second) in [
                            (e.skill1, &mut e.points1, false),
                            (e.skill2, &mut e.points2, true),
                        ] {
                            if skill == 0 {
                                continue;
                            }
                            match crate::charm::allowed_points_rarity(e.grade.rarity(), skill, second) {
                                Some(r) if r.min == 0 && r.max == 0 => {
                                    *pts = 0;
                                }
                                Some(r) => *pts = (*pts).clamp(r.min, r.max),
                                None => {}
                            }
                        }
                    }
                    2 => {
                        let max = e.grade.max_slots() as i64;
                        e.slots = ((e.slots as i64 + d).rem_euclid(max + 1)) as u8;
                    }
                    3 => e.skill1 = ((e.skill1 as i64 + d).rem_euclid(256)) as u8,
                    4 => e.points1 = (e.points1 as i64 + d).clamp(-127, 127) as i8,
                    5 => e.skill2 = ((e.skill2 as i64 + d).rem_euclid(256)) as u8,
                    6 => e.points2 = (e.points2 as i64 + d).clamp(-127, 127) as i8,
                    _ => {}
                }
            }
            // Y on a skill row opens the name picker: typing a number is
            // hopeless when there are 306 skills.
            Key::Y => {
                if e.field == 3 || e.field == 5 {
                    self.skill_pick_field = if e.field == 3 { 1 } else { 2 };
                    self.skill_pick_filter.clear();
                    self.skill_pick_cursor = 0;
                    self.charm_edit = Some(e);
                    self.screen = Screen::SkillPick;
                    self.message = String::from("スキルを名前から選んでください");
                } else {
                    self.message = String::from("先にスキルの行へ移動してください");
                }
            }
            Key::X => self.step = (self.step + 1) % STEPS.len(),
            Key::A => {
                let c = Charm {
                    index: e.index,
                    slots: e.slots,
                    skill1: e.skill1,
                    points1: e.points1,
                    skill2: e.skill2,
                    points2: e.points2,
                };
                if let Some(s) = &mut self.save {
                    if s.set_charm_with_grade(self.slot, &c, e.grade) {
                        self.dirty = true;
                        self.message = format!(
                            "お守り {} を書き込みました（{}）",
                            e.index,
                            e.grade.name()
                        );
                    } else {
                        self.message = String::from(
                            "could not write here - the slot is a weapon or the box has no talisman to copy",
                        );
                    }
                }
                self.charm_edit = Some(e);
                self.screen = Screen::Charms;
                self.cursor = 0;
            }
            Key::B => {
                self.charm_edit = Some(e);
                self.screen = Screen::Charms;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
        self.charm_edit = Some(e);
    }

    fn input_palico(&mut self, key: Key) {
        let ids = dlcats::all_ids();
        let n = ids.len();
        match key {
            // palico_cursor is the highlighted row.
            Key::Up => {
                self.palico_cursor = (self.palico_cursor + n.saturating_sub(1)) % n.max(1);
                self.cursor = self.palico_cursor;
            }
            Key::Down => {
                self.palico_cursor = (self.palico_cursor + 1) % n.max(1);
                self.cursor = self.palico_cursor;
            }
            Key::L | Key::R => {
                self.palico_lang = 1 - self.palico_lang;
                self.message = if self.palico_lang == 0 {
                    String::from("オトモ名（日本語）")
                } else {
                    String::from("オトモ名（英語）")
                };
            }
            Key::A => {
                let id = match ids.get(self.palico_cursor) {
                    Some(v) => *v,
                    None => return,
                };
                let lang = if self.palico_lang == 0 { Lang::Jp } else { Lang::En };
                let installed = self
                    .save
                    .as_ref()
                    .map(|s| s.has_dlc_palico(id))
                    .unwrap_or(false);
                if let Some(s) = &mut self.save {
                    if installed {
                        if let Some(sl) = s.dlc_palicoes().iter().find(|c| c.id == id).map(|c| c.index) {
                            let name = s.dlc_palicoes().get(sl).map(|c| c.name.clone()).unwrap_or_default();
                            s.remove_dlc_palico(sl);
                            self.dirty = true;
                            self.message = format!("removed {}", name);
                        }
                    } else {
                        match s.install_dlc_palico(lang, id) {
                            Ok(slot) => {
                                let name = s.dlc_palicoes().get(slot).map(|c| c.name.clone()).unwrap_or_default();
                                self.dirty = true;
                                self.message = format!("added {} to slot {}", name, slot + 1);
                            }
                            Err(e) => self.message = format!("{}", e),
                        }
                    }
                }
            }
            Key::X => self.request_write_save(),
            Key::Y => self.install_all_dlc_palicoes(),
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Install all downloadable Palicoes of the current language (`palico_lang`)
    /// into free slots in the open save.
    pub fn install_all_dlc_palicoes(&mut self) {
        let lang = if self.palico_lang == 0 {
            Lang::Jp
        } else {
            Lang::En
        };
        let ids: &[u8] = match lang {
            Lang::Jp => dlcats::JP_IDS,
            Lang::En => dlcats::EN_IDS,
        };
        let Some(s) = &mut self.save else {
            self.message = String::from("先にセーブを開いてください");
            return;
        };
        let mut added = 0usize;
        for &id in ids {
            if !s.has_dlc_palico(id) && s.install_dlc_palico(lang, id).is_ok() {
                added += 1;
            }
        }
        if added > 0 {
            self.dirty = true;
            self.message = format!(
                "公式配信オトモを {} 匹一括導入しました！([X] またはヘッダーでセーブ保存)",
                added
            );
        } else {
            self.message = String::from("選択言語の配信オトモはすべて導入済み（または枠満杯）です");
        }
    }

    fn input_options(&mut self, key: Key) {
        match key {
            Key::Up => self.option_cursor = (self.option_cursor + 3) % 4,
            Key::Down => self.option_cursor = (self.option_cursor + 1) % 4,
            Key::Left | Key::Right => {
                let d: i64 = if key == Key::Right { 1 } else { -1 };
                let Some(s) = &mut self.save else { return };
                let mut o = s.options();
                match self.option_cursor {
                    // Real saves hold values well above 31, so only bound
                    // the byte. Clamping tighter silently rewrote the field.
                    0 => o.brightness = (o.brightness as i64 + d).clamp(0, 255) as u8,
                    1 => o.rumble = ((o.rumble as i64 + d).rem_euclid(2)) as u8,
                    2 => o.disable_3d = ((o.disable_3d as i64 + d).rem_euclid(2)) as u8,
                    3 => o.circle_pad_pro = ((o.circle_pad_pro as i64 + d).rem_euclid(2)) as u8,
                    _ => {}
                }
                s.set_options(&o);
                self.dirty = true;
            }
            Key::A => self.request_write_save(),
            Key::B => {
                self.screen = Screen::SaveList;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Run the quest checker on the open archive.
    pub fn run_check(&mut self) {
        // Same reason: flush the editor buffer into the archive first.
        self.store_entry();
        self.load_entry();
        self.findings = match &self.arc {
            Some(a) => check::check_quest(a),
            None => Vec::new(),
        };
        self.check_cursor = 0;
        self.screen = Screen::Check;
        self.cursor = 0;
    }

    fn input_check(&mut self, key: Key) {
        let n = self.findings.len();
        match key {
            // check_cursor is what the list highlights and what A acts on, so
            // it is the one that has to move.
            Key::Up => {
                self.check_cursor = (self.check_cursor + n.saturating_sub(1)) % n.max(1);
                self.cursor = self.check_cursor;
            }
            Key::Down => {
                self.check_cursor = (self.check_cursor + 1) % n.max(1);
                self.cursor = self.check_cursor;
            }
            Key::A => {
                let fixable = self
                    .findings
                    .get(self.check_cursor)
                    .map(|f| f.fixable)
                    .unwrap_or(false);
                if !fixable {
                    self.message = String::from("この指摘は自動で直せません");
                    return;
                }
                if let Some(a) = &mut self.arc {
                    match check::clear_one_player_only(a, 0x00) {
                        Some(before) => {
                            self.message = format!(
                                "participation condition 1: 0x{:02X} -> 0x00   (press X to write the .arc)",
                                before
                            );
                            self.dirty = true;
                            self.findings = check::check_quest(a);
                            // The fix edited the archive entry directly, so the
                            // .mib working copy must be refreshed or the next
                            // store_entry() would write the old bytes back.
                            self.load_entry();
                        }
                        None => self.message = String::from(".mib がありません"),
                    }
                }
            }
            Key::X => self.request_write_arc(),
            Key::B => {
                self.screen = Screen::ArcEntries;
                self.cursor = self.entry;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn move_cursor(&mut self, len: usize, delta: i64) {
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let n = len as i64;
        self.cursor = (((self.cursor as i64 + delta) % n + n) % n) as usize;
    }

    fn request_write_save(&mut self) {
        if let Some(p) = self.save_path.clone() {
            if let Some(s) = &self.save {
                self.pending_write = Some(Write::File {
                    path: p,
                    bytes: s.to_bytes(),
                });
                self.message = String::from("セーブに書き込み中...");
            }
        }
    }

    fn request_write_arc(&mut self) {
        self.store_entry();
        // A quest pulled out of the save has no path; A used to do nothing at
        // all here, which read as a broken button.
        if self.arc_path.is_none() {
            self.message = if self.save.is_some() {
                String::from("エントリ一覧で X を押すと、このクエストをセーブに書き戻します")
            } else {
                String::from("セーブを開くか、Y で .arc をファイルに保存してください")
            };
            return;
        }
        if let Some(p) = self.arc_path.clone() {
            if let Some(a) = &self.arc {
                match a.to_bytes() {
                    Ok(bytes) => {
                        self.pending_write = Some(Write::File { path: p, bytes });
                        self.message = String::from(".arc に書き込み中...");
                    }
                    Err(_) => self.message = String::from("再構築に失敗しました"),
                }
            }
        }
    }

    fn input_save_list(&mut self, key: Key) {
        match key {
            Key::Up => self.move_cursor(self.save_files.len(), -1),
            Key::Down => self.move_cursor(self.save_files.len(), 1),
            Key::A => {
                if !self.save_files.is_empty() {
                    self.pending_request = Some(Request::ReadSave(self.cursor));
                }
            }
            Key::Y => {
                self.screen = Screen::ArcList;
                self.cursor = 0;
                self.message = String::from("クエスト編集");
            }
            Key::X => {
                self.screen = Screen::Options;
                self.cursor = 0;
                self.message = String::from("設定");
            }
            // L converts between the 3DS and Switch containers.
            Key::L => {
                self.screen = Screen::Convert;
                self.message = String::from("3DS <-> Switch");
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_slots(&mut self, key: Key) {
        match key {
            // The list highlights self.cursor, so moving the slot has to move
            // the cursor with it or the highlight stays on slot 1.
            Key::Up => {
                self.slot = (self.slot + 2) % 3;
                self.cursor = self.slot;
            }
            Key::Down => {
                self.slot = (self.slot + 1) % 3;
                self.cursor = self.slot;
            }
            Key::L => self.open_charms(),
            Key::R => {
                self.screen = Screen::Palico;
                self.cursor = 0;
                self.message = String::from("L/R で名前の言語を切り替え");
            }
            Key::A => {
                self.screen = Screen::Char;
                self.cursor = 0;
                self.field = 0;
            }
            // - opens the equipment box: this screen had every other key
            // spoken for.
            Key::Minus => {
                self.screen = Screen::Equip;
                self.equip_cursor = 0;
            }
            Key::X => {
                self.screen = Screen::Items;
                self.cursor = 0;
            }
            Key::B => {
                self.screen = Screen::SaveList;
                self.cursor = 0;
            }
            Key::Y => {
                self.screen = Screen::ArcList;
                self.cursor = 0;
                self.message = String::from("クエスト編集");
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_char(&mut self, key: Key) {
        let n = CHAR_FIELDS.len();
        match key {
            Key::Up => {
                self.field = (self.field + n - 1) % n;
                self.cursor = self.field;
            }
            Key::Down => {
                self.field = (self.field + 1) % n;
                self.cursor = self.field;
            }
            Key::L => self.slot = (self.slot + 2) % 3,
            Key::R => self.slot = (self.slot + 1) % 3,
            // The slot screen has no spare key, so the record screens hang off
            // this one: - for hunt records and the guild card, plus palicos
            // from the equipment screen.
            Key::Minus => {
                self.screen = Screen::Hunts;
                self.hunt_cursor = 0;
            }
            Key::Y => self.screen = Screen::Sets,
            Key::X => self.step = (self.step + 1) % STEPS.len(),
            Key::Left | Key::Right => {
                if self.field == 0 {
                    if let Some(c) = self.character() {
                        self.pending_text = Some((usize::MAX, c.name));
                    }
                } else {
                    let d = if key == Key::Left { -1 } else { 1 } * STEPS[self.step];
                    let v = self.char_value(self.field);
                    self.set_char_value(self.field, v + d);
                }
            }
            Key::A => self.request_write_save(),
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_items(&mut self, key: Key) {
        // The bulk menu is a mode of this screen, not a screen of its own:
        // while it is open the list underneath keeps its place and every key
        // drives the menu instead.
        if self.item_menu_open {
            self.input_item_menu(key);
            return;
        }
        let vis = self.visible_items();
        let n = vis.len();
        match key {
            Key::Up => self.move_cursor(n, -1),
            Key::Down => self.move_cursor(n, 1),
            Key::Left | Key::Right => {
                if let Some(&idx) = vis.get(self.cursor) {
                    let step = if key == Key::Right {
                        STEPS[self.step]
                    } else {
                        -STEPS[self.step]
                    };
                    if let Some(mut items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) {
                        let c = items[idx].count as i64 + step;
                        // 99 is the ceiling the game itself uses (and the PC
                        // build has always clamped at); 127 could be stored
                        // but no item in this game is handed out above 99.
                        items[idx].count = c.clamp(0, MAX_COUNT as i64) as u8;
                        if let Some(s) = &mut self.save {
                            let _ = s.write_item_box(self.slot, &items);
                            self.dirty = true;
                        }
                    }
                }
            }
            Key::X => self.step = (self.step + 1) % STEPS.len(),
            Key::Y => {
                self.show_empty = !self.show_empty;
                self.cursor = 0;
            }
            // R opens the item picker for the slot under the cursor, so the
            // type can be changed by name instead of only the count.
            // Replacing an item is the destructive step here, so it can be
            // taken back with L.
            Key::L => self.undo_item(),
            Key::R => {
                if let Some(&idx) = vis.get(self.cursor) {
                    self.item_pick_mode = 0;
                    self.item_pick_target = idx;
                    self.item_pick_cursor = 0;
                    self.item_pick_filter.clear();
                    self.screen = Screen::ItemPick;
                    self.message = format!("枠 #{} のアイテムを選んでください", idx + 1);
                }
            }
            // The bulk menu hangs off Minus: X is the step key on this screen
            // and A writes the save, so this is the only key it had free.
            Key::Minus => self.open_item_menu(),
            Key::A => self.request_write_save(),
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    // ---------------------------------------------------- item bulk operations

    /// Open the item box operation menu (Minus on the item screen).
    ///
    /// The menu is a mode of the item screen instead of a screen of its own,
    /// so it needs no extra place in every match on Screen: the list
    /// underneath keeps its place and B or Minus goes back to it.
    pub fn open_item_menu(&mut self) {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return;
        }
        self.item_menu_open = true;
        if self.item_menu_cursor >= ITEM_OPS.len() {
            self.item_menu_cursor = 0;
        }
        self.item_dedupe_armed = false;
        self.message = self.item_op_hint();
    }

    /// Close the menu and go back to the item list.
    pub fn close_item_menu(&mut self) {
        self.item_menu_open = false;
        self.item_dedupe_armed = false;
        self.message = String::from("アイテム一覧に戻りました");
    }

    fn input_item_menu(&mut self, key: Key) {
        let n = ITEM_OPS.len();
        match key {
            // Moving off a row cancels a pending dedupe: the second press must
            // never land on a row the user has stopped looking at.
            Key::Up => {
                self.item_menu_cursor = (self.item_menu_cursor + n - 1) % n;
                self.item_dedupe_armed = false;
                self.message = self.item_op_hint();
            }
            Key::Down => {
                self.item_menu_cursor = (self.item_menu_cursor + 1) % n;
                self.item_dedupe_armed = false;
                self.message = self.item_op_hint();
            }
            Key::A | Key::Right => self.run_item_op(self.item_menu_cursor),
            Key::B | Key::Minus => self.close_item_menu(),
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// The label and the one-line explanation of a menu row.
    pub fn item_op_label(op: usize) -> (&'static str, &'static str) {
        ITEM_OPS.get(op).copied().unwrap_or(("", ""))
    }

    /// The numbers the menu shows, read from the box in one pass.
    pub fn item_menu_stats(&self) -> ItemMenuStats {
        let Some(items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return ItemMenuStats::default();
        };
        let mut stats = ItemMenuStats {
            holes: items.iter().filter(|s| s.is_blank()).count(),
            ..ItemMenuStats::default()
        };
        for slot in &items {
            if slot.is_blank() {
                continue;
            }
            if (slot.count as u16) < MAX_COUNT {
                stats.fill += 1;
            }
            if (slot.count as u16) > MAX_COUNT {
                stats.over += 1;
            }
        }
        stats.dedupe = itembox::dedupe_preview(&items);
        stats
    }

    /// One line about the highlighted row, shown in the footer.
    pub fn item_op_hint(&self) -> String {
        let (label, note) = Self::item_op_label(self.item_menu_cursor);
        if self.item_dedupe_armed && self.item_menu_cursor == ITEM_OP_DEDUPE {
            let p = self.item_menu_stats().dedupe;
            return format!(
                "{}: {} 枠解放 / {} 個消失します。もう一度 [A] で実行、[B] で中止",
                label, p.freed, p.lost
            );
        }
        format!("{}: {}", label, note)
    }

    /// Run one row of the operation menu.
    ///
    /// Every row that can drop data says what it would drop before it does
    /// it, and every row keeps the box it replaced in Editor::item_undo, so
    /// "直前の操作を元に戻す" (or L) can put the box back.
    pub fn run_item_op(&mut self, op: usize) {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return;
        }
        match op {
            ITEM_OP_FILL => self.item_op_fill(),
            ITEM_OP_DEDUPE => self.item_op_dedupe(),
            ITEM_OP_COMPACT => self.item_op_compact(),
            ITEM_OP_CLAMP => self.item_op_clamp(),
            ITEM_OP_ADD => self.open_item_add(),
            ITEM_OP_UNDO => self.undo_item(),
            _ => {}
        }
    }

    /// Write an edited box back and mark the save dirty.
    fn store_item_box(&mut self, items: &[ItemSlot]) {
        if let Some(s) = &mut self.save {
            if s.write_item_box(self.slot, items).is_ok() {
                self.dirty = true;
            }
        }
    }

    /// すべて 99 に: raise every used record to the cap.
    fn item_op_fill(&mut self) {
        if self.item_menu_stats().fill == 0 {
            self.message = String::from("所持している枠はすでにすべて 99 です");
            return;
        }
        let Some(mut items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return;
        };
        self.push_item_undo();
        let changed = itembox::fill_all(&mut items, MAX_COUNT);
        self.store_item_box(&items);
        self.message = format!("{} 枠を ×99 にしました（[A] でセーブ保存）", changed);
    }

    /// 重複を整理: preview first, run on the second press.
    ///
    /// The preview is not a courtesy. Merging stacks cannot keep more than the
    /// cap of one item, and on this save 185 of the 187 duplicated ids are
    /// already past it, so the merge drops items. Saying so before the press
    /// is the difference between a tool and a trap.
    fn item_op_dedupe(&mut self) {
        let Some(items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return;
        };
        let preview = itembox::dedupe_preview(&items);
        if preview.freed == 0 && preview.lost == 0 {
            self.item_dedupe_armed = false;
            self.message = String::from("重複はありません（まとめても枠は空きません）");
            return;
        }
        if !self.item_dedupe_armed {
            self.item_dedupe_armed = true;
            self.message = format!(
                "重複整理: {} 枠解放 / {} 個消失します。もう一度 [A] で実行、[B] で中止",
                preview.freed, preview.lost
            );
            return;
        }
        self.item_dedupe_armed = false;
        let before = box_total(&items);
        let mut items = items;
        self.push_item_undo();
        let freed = itembox::dedupe(&mut items);
        let lost = before.saturating_sub(box_total(&items));
        self.store_item_box(&items);
        self.message = format!("{}枠解放・{}個消失（重複を整理しました）", freed, lost);
    }

    /// 空きを詰める: move the used records to the front, in order.
    fn item_op_compact(&mut self) {
        let Some(mut items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return;
        };
        if items.iter().all(|s| !s.is_blank()) {
            self.message = String::from("空き枠はありません（すべて使用中）");
            return;
        }
        self.push_item_undo();
        let moved = itembox::compact(&mut items);
        let holes = items.iter().filter(|s| s.is_blank()).count();
        self.store_item_box(&items);
        self.message = format!("空きを詰めました（{} 枠移動、空きは {} 枠）", moved, holes);
    }

    /// 上限を丸める: pull every stack above the cap back down to it.
    fn item_op_clamp(&mut self) {
        let over = self.item_menu_stats().over;
        if over == 0 {
            self.message = String::from("99 を超えている個数はありません");
            return;
        }
        let Some(mut items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return;
        };
        self.push_item_undo();
        let changed = itembox::clamp_counts(&mut items, MAX_COUNT);
        self.store_item_box(&items);
        self.message = format!("99 を超えていた {} 枠を 99 に戻しました", changed);
    }

    /// Open the picker for the two-stage bulk add: item first, then count.
    fn open_item_add(&mut self) {
        self.item_pick_mode = 3;
        self.item_pick_stage = 0;
        self.item_pick_cursor = 0;
        self.item_pick_filter.clear();
        self.item_pick_add_id = 0;
        self.item_pick_add_count = 1;
        self.screen = Screen::ItemPick;
        self.message = String::from("追加するアイテムを選んでください（選んだあと [◀/▶] で個数）");
    }

    /// The footer line of the add stage: which item and how many.
    pub fn item_add_message(&self) -> String {
        format!(
            "{} を {} 個追加 — [◀/▶] ±1、[▲/▼] ±10、[A] 決定、[B] 選び直す",
            item_name_of(self.item_pick_add_id),
            self.item_pick_add_count
        )
    }

    /// Put the chosen number of the chosen item into the box.
    fn apply_item_add(&mut self) {
        let id = self.item_pick_add_id;
        if id == 0 {
            self.message = String::from("アイテムが選ばれていません");
            self.item_pick_stage = 0;
            return;
        }
        let want = self.item_pick_add_count.clamp(1, MAX_COUNT);
        let Some(mut items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return;
        };
        self.push_item_undo();
        let got = itembox::add(&mut items, id, want);
        if got == 0 {
            // Nothing fit: drop the undo entry that was pushed for a write
            // that is not going to happen.
            self.item_undo.pop();
            self.message = String::from("空き枠がなく、1 個も入りませんでした");
            self.item_pick_stage = 0;
            return;
        }
        self.store_item_box(&items);
        let name = item_name_of(id);
        let short = want as u32 - got;
        self.message = if short > 0 {
            format!(
                "{} を {} 個追加しました（{} 個は枠が足りず入りません）",
                name, got, short
            )
        } else {
            format!("{} を {} 個追加しました（[A] でセーブ保存）", name, got)
        };
        // Back to the menu, on the row that was just used, so adding another
        // stack is one press away.
        self.item_pick_stage = 0;
        self.screen = Screen::Items;
        self.item_menu_open = true;
        self.item_menu_cursor = ITEM_OP_ADD;
        self.item_dedupe_armed = false;
    }

    /// Item ids whose name matches the current filter, in id order.
    pub fn item_pick_matches(&self) -> Vec<u16> {
        let needle = self.item_pick_filter.trim().to_lowercase();
        #[cfg(feature = "gamedata")]
        {
            let mut out = Vec::new();
            for (id, name) in crate::gamedata::ITEM_NAME_LIST.iter().enumerate() {
                if id == 0 || id as usize > crate::gamedata::LAST_ITEM_ID {
                    continue;
                }
                if needle.is_empty() || name.to_lowercase().contains(&needle) {
                    out.push(id as u16);
                }
            }
            out
        }
        #[cfg(not(feature = "gamedata"))]
        {
            let _ = needle;
            Vec::new()
        }
    }

    /// The io layer answers a filter request with this.
    pub fn apply_item_filter(&mut self, text: String) {
        self.item_pick_filter = text;
        self.item_pick_cursor = 0;
    }

    /// Change how many of the item one reward record holds.
    ///
    /// The desktop build has had a drag value here for a while; this is the
    /// same edit for the console, where only the item was swappable.
    fn adjust_entry_count(&mut self, delta: i64) {
        let row = self.entry_row_cursor;
        let idx = self.entry;
        let ext = self
            .arc
            .as_ref()
            .and_then(|a| a.entries.get(idx))
            .and_then(|e| extension_for(e.type_hash));
        if !matches!(ext, Some("rem") | Some("sup")) {
            self.message = String::from("このエントリに編集できる項目はありません");
            return;
        }
        let Some(a) = &mut self.arc else {
            return;
        };
        let Some(e) = a.entries.get_mut(idx) else {
            return;
        };
        let Ok(mut t) = crate::entries::ItemTable::parse(&e.data) else {
            self.message = String::from("この表を読み取れません");
            return;
        };
        if row >= t.records.len() {
            self.message = String::from("その記録はありません");
            return;
        }
        if t.records[row].item == 0 {
            self.message = String::from("空の行です。先に A でアイテムを選んでください");
            return;
        }
        let next = (t.records[row].count as i64 + delta).clamp(0, 99) as u8;
        if next == t.records[row].count {
            return;
        }
        t.records[row].count = next;
        e.data = t.to_bytes();
        self.dirty = true;
        self.message = alloc::format!(
            "{} 行目の個数を {} にしました（X で .arc に書き込み）",
            row,
            next
        );
    }

    /// Write one item id into the reward record the picker is aimed at.
    fn store_reward_item(&mut self, id: u16) {
        let row = self.entry_item_row;
        let idx = self.entry;
        let Some(a) = &mut self.arc else {
            return;
        };
        let Some(e) = a.entries.get_mut(idx) else {
            return;
        };
        let Ok(mut t) = crate::entries::ItemTable::parse(&e.data) else {
            self.message = String::from("この表を読み取れません");
            return;
        };
        if row >= t.records.len() {
            self.message = String::from("その記録はありません");
            return;
        }
        t.records[row].item = id;
        if t.records[row].count == 0 {
            t.records[row].count = 1;
        }
        e.data = t.to_bytes();
        self.dirty = true;
        let name = item_name_of(id);
        self.message =
            alloc::format!("record {} -> {} ({})  (X writes the .arc)", row, name, id);
    }

    /// Remember the item box before replacing an entry.
    fn push_item_undo(&mut self) {
        let snapshot = self
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(self.slot))
            .map(|b| crate::itembox::encode(&b));
        if let Some(raw) = snapshot {
            self.item_undo.push(raw);
            if self.item_undo.len() > 16 {
                self.item_undo.remove(0);
            }
        }
    }

    /// Put the item box back the way it was.
    fn undo_item(&mut self) {
        let Some(raw) = self.item_undo.pop() else {
            self.message = String::from("取り消せる操作がありません");
            return;
        };
        let slots = crate::itembox::decode(&raw);
        let slot = self.slot;
        if let Some(s) = &mut self.save {
            if s.write_item_box(slot, &slots).is_ok() {
                self.dirty = true;
                self.message = String::from("アイテムボックスを元に戻しました");
            }
        }
    }

    /// Skill ids whose name matches the current filter, in id order.
    pub fn skill_pick_matches(&self) -> Vec<u8> {
        let needle = self.skill_pick_filter.trim().to_lowercase();
        #[cfg(feature = "gamedata")]
        {
            let mut out = Vec::new();
            for (i, name) in crate::skills::SKILL_NAMES.iter().enumerate() {
                let id = (i + 1) as u8;
                if needle.is_empty() || name.to_lowercase().contains(&needle) {
                    out.push(id);
                }
            }
            out
        }
        #[cfg(not(feature = "gamedata"))]
        {
            let _ = needle;
            Vec::new()
        }
    }

    /// The io layer answers a skill filter request with this.
    pub fn apply_skill_filter(&mut self, text: String) {
        self.skill_pick_filter = text;
        self.skill_pick_cursor = 0;
    }

    /// Build the converted save and hand the bytes to the io layer.
    fn convert_now(&mut self) {
        let Some(save) = &self.save else {
            self.message = String::from("先にセーブを開いてください");
            return;
        };
        if save.kind() == crate::save::SaveKind::Mhgu {
            self.message = String::from("MHGU はここでは変換できません");
            return;
        }
        let Some(base) = self.save_path.clone() else {
            self.message = String::from("このセーブにはパスがありません");
            return;
        };
        // The console save lives on a mounted save area, and that area holds
        // exactly the files the game made. A converted copy does not belong
        // next to it: "nand:/system" used to produce "nand:/system.3ds", which
        // then went straight back through the console writer and left the save
        // as a hybrid of a 3DS body and the old tail.
        let dir = if crate::hbedit::is_console_save_path(&base) {
            alloc::string::String::from("/switch/mhnxeditor/")
        } else {
            split_dir(&base)
        };
        let stem = if self.convert_name.trim().is_empty() {
            alloc::string::String::from("system")
        } else {
            // Keep it to one path segment: a stray slash would write elsewhere.
            self.convert_name.trim().replace('/', "_").replace('\\', "_")
        };
        let (path, bytes, what) = match save.kind() {
            crate::save::SaveKind::ThreeDs => {
                let name = if save.header().iter().any(|b| *b != 0) {
                    "system"
                } else {
                    "system"
                };
                match crate::save::Save::from_3ds_body(save.body(), name) {
                    Ok(sw) => (
                        alloc::format!("{}{}.switch", dir, stem),
                        sw.to_bytes(),
                        "Switch (36 byte header synthesised)",
                    ),
                    Err(_) => {
                        self.message = String::from("Switch 形式を作れません");
                        return;
                    }
                }
            }
            _ => (
                alloc::format!("{}{}.3ds", dir, stem),
                save.to_3ds_bytes(),
                "3DS (header stripped)",
            ),
        };
        self.pending_write = Some(Write::File {
            path: path.clone(),
            bytes,
        });
        self.message = alloc::format!("writing {} -> {}", what, path);
    }

    /// Equipment of the current slot, after the filter.
    pub fn equipment(&self) -> Vec<crate::equip::Equipment> {
        let Some(save) = &self.save else {
            return Vec::new();
        };
        let Some(box_) = save.equipment_box(self.slot) else {
            return Vec::new();
        };
        let needle = self.equip_filter.trim().to_lowercase();
        crate::equip::decode_box(&box_)
            .into_iter()
            .filter(|e| !e.is_empty())
            .filter(|e| {
                if needle.is_empty() {
                    return true;
                }
                crate::equip::name(e).to_lowercase().contains(&needle)
                    || crate::equip::type_name(e.kind)
                        .to_lowercase()
                        .contains(&needle)
            })
            .collect()
    }

    /// The DLC section's 45 quest-equipment entries.
    pub fn quest_equipment(&self) -> Vec<crate::dlc::QuestEquipment> {
        match &self.save {
            Some(s) => crate::dlc::quest_equipment(s),
            None => Vec::new(),
        }
    }

    /// The hunt tables as a flat list of monsters that have records.
    pub fn hunt_rows(&self) -> Vec<(usize, u16, u16, u16, u16)> {
        let Some(save) = &self.save else {
            return Vec::new();
        };
        let slay = save.hunt_counts(self.slot).unwrap_or_default();
        let cap = save.capture_counts(self.slot).unwrap_or_default();
        let size = save.monster_sizes(self.slot).unwrap_or_default();
        (0..slay.len())
            .map(|i| {
                let (a, b) = size.get(i).copied().unwrap_or((0, 0));
                (i, slay[i], cap.get(i).copied().unwrap_or(0), a, b)
            })
            .filter(|(_, s, c, _, _)| *s > 0 || *c > 0)
            .collect()
    }

    fn input_hunts(&mut self, key: Key) {
        let rows = self.hunt_rows();
        let n = rows.len();
        match key {
            Key::Up => self.hunt_cursor = self.hunt_cursor.saturating_sub(1),
            Key::Down => {
                if n > 0 {
                    self.hunt_cursor = (self.hunt_cursor + 1).min(n - 1);
                }
            }
            // X walks the four numbers of the highlighted monster.
            Key::X | Key::R => self.hunt_field = (self.hunt_field + 1) % 4,
            Key::L => self.hunt_field = (self.hunt_field + 3) % 4,
            Key::Y => self.step = (self.step + 1) % STEPS.len(),
            // Left/Right change the highlighted number.
            Key::Left | Key::Right => {
                let Some(&(monster, slay, cap, lo, hi)) = rows.get(self.hunt_cursor) else {
                    return;
                };
                let up = key == Key::Right;
                let step = if up {
                    HUNT_STEPS[self.step]
                } else {
                    -HUNT_STEPS[self.step]
                };
                let Some(save) = &mut self.save else { return };
                let slot = self.slot;
                let ok = match self.hunt_field {
                    // Slay and capture are counts, so they clamp at zero.
                    0 => save.set_hunt_count(slot, monster, clamp_u16(slay, step)),
                    1 => save.set_capture_count(slot, monster, clamp_u16(cap, step)),
                    // The size pair keeps its order: the game never stores a
                    // smallest above a largest.
                    _ => {
                        let (mut a, mut b) = (lo, hi);
                        if self.hunt_field == 2 {
                            a = clamp_u16(a, step);
                        } else {
                            b = clamp_u16(b, step);
                        }
                        if a > b {
                            core::mem::swap(&mut a, &mut b);
                        }
                        save.set_monster_size(slot, monster, a, b)
                    }
                };
                if ok {
                    self.dirty = true;
                }
            }
            Key::A => {
                if self.save.is_some() {
                    self.dirty = true;
                    self.message = alloc::format!(
                        "狩猟記録 {} 番の値を変更しました",
                        monster_label(&rows, self.hunt_cursor)
                    );
                } else {
                    self.message = String::from("先にセーブを開いてください");
                }
            }
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_guild(&mut self, key: Key) {
        // Field 0 is the rank, 1 is the card's own clock, 2 the comment.
        match key {
            Key::Up => self.guild_field = (self.guild_field + 2) % 3,
            Key::Down | Key::X => self.guild_field = (self.guild_field + 1) % 3,
            Key::Left | Key::Right => {
                let up = key == Key::Right;
                let slot = self.slot;
                let Some(save) = &mut self.save else { return };
                let ok = match self.guild_field {
                    0 => {
                        let cur = crate::guildcard::hunter_rank(save, slot).unwrap_or(0);
                        let v = if up {
                            (cur + 1).min(999)
                        } else {
                            cur.saturating_sub(1)
                        };
                        crate::guildcard::set_hunter_rank(save, slot, v)
                    }
                    1 => {
                        let cur = crate::guildcard::play_time(save, slot).unwrap_or(0);
                        let step = STEPS[self.step].max(1) as u32;
                        let v = if up {
                            cur.saturating_add(step)
                        } else {
                            cur.saturating_sub(step)
                        };
                        crate::guildcard::set_play_time(save, slot, v)
                    }
                    _ => false,
                };
                if ok {
                    self.dirty = true;
                }
            }
            // The comment is text, so it needs the keyboard.
            Key::A => {
                if self.guild_field == 2 {
                    if let Some(save) = &self.save {
                        let cur = crate::guildcard::comment(save, self.slot).unwrap_or_default();
                        self.pending_text = Some((TEXT_GUILD_COMMENT, cur));
                    }
                }
            }
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_roster(&mut self, key: Key) {
        let list = self.palicos();
        let n = list.len();
        match key {
            Key::Up => self.roster_cursor = self.roster_cursor.saturating_sub(1),
            Key::Down => {
                if n > 0 {
                    self.roster_cursor = (self.roster_cursor + 1).min(n - 1);
                }
            }
            // X walks the editable fields of the highlighted palico.
            Key::X | Key::R => self.roster_field = (self.roster_field + 1) % 3,
            Key::L => self.roster_field = (self.roster_field + 2) % 3,
            Key::Left | Key::Right => {
                let Some(p) = list.get(self.roster_cursor).cloned() else {
                    return;
                };
                let up = key == Key::Right;
                let slot = self.slot;
                let Some(save) = &mut self.save else { return };
                let ok = match self.roster_field {
                    0 => {
                        let lvl = if up {
                            (p.level + 1).min(99)
                        } else {
                            p.level.saturating_sub(1).max(1)
                        };
                        crate::palico::set_level(save, slot, p.index, lvl)
                    }
                    1 => {
                        let v = if up {
                            (p.enthusiasm as u16 + 1).min(100) as u8
                        } else {
                            p.enthusiasm.saturating_sub(1)
                        };
                        crate::palico::set_enthusiasm(save, slot, p.index, v)
                    }
                    _ => {
                        if up {
                            crate::palico::set_exp(save, slot, p.index, p.exp.saturating_add(1_000))
                        } else {
                            crate::palico::set_exp(save, slot, p.index, p.exp.saturating_sub(1_000))
                        }
                    }
                };
                if ok {
                    self.dirty = true;
                }
            }
            // A renames: the console has no text entry of its own, so this
            // asks the platform layer for the software keyboard.
            Key::A => {
                let Some(p) = list.get(self.roster_cursor) else {
                    return;
                };
                self.pending_text = Some((TEXT_PALICO_NAME + p.index, p.name.clone()));
            }
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Palicos the current character owns.
    pub fn palicos(&self) -> Vec<crate::palico::Palico> {
        match &self.save {
            Some(s) => crate::palico::used(s, self.slot),
            None => Vec::new(),
        }
    }
    fn input_quest_equip(&mut self, key: Key) {
        let n = self.quest_equipment().len();
        // Two levels: the slot list, and the items inside the chosen slot.
        let items = if self.qe_open {
            self.quest_equip_items().len()
        } else {
            0
        };
        match key {
            Key::Up => {
                if self.qe_open {
                    self.qe_item_cursor = self.qe_item_cursor.saturating_sub(1);
                } else {
                    self.cursor = self.cursor.saturating_sub(1);
                }
            }
            Key::Down => {
                if self.qe_open {
                    if items > 0 {
                        self.qe_item_cursor = (self.qe_item_cursor + 1).min(items - 1);
                    }
                } else if n > 0 {
                    self.cursor = (self.cursor + 1).min(n - 1);
                }
            }
            // Left and right change how many of the entry the hunt hands out.
            // The screen showed the count and nothing edited it.
            Key::Left | Key::Right => {
                if self.qe_open {
                    if let Some(t) = self.quest_equip_items().get(self.qe_item_cursor).copied() {
                        // The max button refused an empty entry; this path did
                        // not, so it could write a count onto nothing.
                        if t.item == 0 {
                            self.message =
                                String::from("空きスロットです。先に A でアイテムを選んでください");
                            return;
                        }
                        let slot_index = self.cursor;
                        let step = if key == Key::Right {
                            STEPS[self.step]
                        } else {
                            -STEPS[self.step]
                        };
                        let next = (t.count as i64 + step).clamp(0, 99) as u8;
                        if next != t.count {
                            if let Some(s) = &mut self.save {
                                if crate::dlc::set_quest_equipment_item(
                                    s,
                                    slot_index,
                                    t.record,
                                    t.list,
                                    t.index,
                                    t.item,
                                    next,
                                )
                                .is_ok()
                                {
                                    self.dirty = true;
                                    self.message = alloc::format!(
                                        "rec{} {}[{}] の個数を {} にしました",
                                        t.record,
                                        match t.list {
                                            crate::dlc::QuestEquipList::Supply => "支給品",
                                            crate::dlc::QuestEquipList::Ammo => "弾ポーチ",
                                        },
                                        t.index,
                                        next
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // A opens the slot, then asks for a new item for the chosen entry.
            Key::A => {
                if self.qe_open {
                    if let Some(entry) = self.quest_equip_items().get(self.qe_item_cursor).copied() {
                        self.item_pick_mode = 2;
                        self.qe_target = entry;
                        self.item_pick_cursor = 0;
                        self.item_pick_filter.clear();
                        self.screen = Screen::ItemPick;
                    }
                } else if n > 0 {
                    self.qe_open = true;
                    self.qe_item_cursor = 0;
                }
            }
            Key::B => {
                if self.qe_open {
                    self.qe_open = false;
                } else {
                    self.screen = Screen::SaveQuests;
                    self.cursor = 0;
                }
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Items inside the quest-equipment slot under the cursor.
    pub fn quest_equip_items(&self) -> Vec<crate::dlc::QuestEquipItem> {
        match &self.save {
            Some(s) => crate::dlc::quest_equipment_items(s, self.cursor),
            None => Vec::new(),
        }
    }

    // --------------------------------------------------- the set list screen
    //
    // The my-sets screen lists two unrelated arrays. Only the equipment sets
    // can be edited; the item sets are still list-only, and this module says so
    // rather than pretending.

    /// The equipment sets of the current slot, as stored.
    pub fn equip_sets(&self) -> Vec<crate::myset::EquipSet> {
        match &self.save {
            Some(s) => crate::myset::equip_sets(s, self.slot),
            None => Vec::new(),
        }
    }

    /// The item sets of the current slot, as stored.
    pub fn item_sets(&self) -> Vec<crate::myset::ItemSet> {
        match &self.save {
            Some(s) => crate::myset::item_sets(s, self.slot),
            None => Vec::new(),
        }
    }

    /// Is the set editor open? (A mode of the my-sets screen, not a screen.)
    pub fn set_editor_open(&self) -> bool {
        self.set_edit.is_some()
    }

    /// The equipment set the editor is looking at, re-read from the save.
    ///
    /// Re-read rather than cached, so the name and the parts on screen are the
    /// bytes that are really stored. Only the pending index of a part runs
    /// ahead of the record, and only until [A] writes it.
    pub fn editing_set(&self) -> Option<crate::myset::EquipSet> {
        let e = self.set_edit?;
        self.equip_sets().into_iter().nth(e.set)
    }

    /// Box index the given part row would write, or myset::NONE.
    pub fn set_pick(&self, part: usize) -> u16 {
        self.set_edit
            .as_ref()
            .and_then(|e| e.pick.get(part).copied())
            .unwrap_or(crate::myset::NONE)
    }

    /// The equipment record behind a part row's pending index.
    ///
    /// None when the part is empty, or when the box does not hold a record at
    /// that index any more (an edit of the box can blank one under it).
    pub fn set_pick_equipment(&self, part: usize) -> Option<crate::equip::Equipment> {
        let idx = self.set_pick(part);
        if idx == crate::myset::NONE {
            return None;
        }
        let save = self.save.as_ref()?;
        save.equipment_at(self.slot, idx as usize)
            .filter(|q| !q.is_empty())
    }

    /// "大剣 ボルジン" for a part row, "なし" for an empty one.
    pub fn set_part_label(&self, part: usize) -> String {
        match self.set_pick_equipment(part) {
            Some(q) => alloc::format!(
                "{} {}",
                crate::equip::type_name(q.kind),
                crate::equip::name(&q)
            ),
            None => {
                if self.set_pick(part) == crate::myset::NONE {
                    String::from("なし")
                } else {
                    alloc::format!("空き枠 #{}", self.set_pick(part))
                }
            }
        }
    }

    /// Open the set editor on one record of the equipment set array.
    ///
    /// The argument is an index into the forty record array, not a row of the
    /// list the console draws: that list skips the empty records, so the
    /// caller maps its row back to this index.
    pub fn open_set_editor(&mut self, set: usize) -> bool {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return false;
        }
        if set >= crate::myset::EQUIP_SETS {
            self.message = alloc::format!("セット {} は範囲外です", set + 1);
            return false;
        }
        let Some(rec) = self.equip_sets().into_iter().nth(set) else {
            self.message = String::from("このセットは読み取れません");
            return false;
        };
        let mut pick = [crate::myset::NONE; crate::myset::EQUIP_SET_PARTS];
        for (i, p) in pick.iter_mut().enumerate() {
            *p = rec.parts[i];
        }
        let used = rec
            .parts
            .iter()
            .filter(|p| **p != crate::myset::NONE)
            .count();
        let name = if rec.name.trim().is_empty() {
            String::from("(無題)")
        } else {
            rec.name.clone()
        };
        self.set_edit = Some(SetEdit {
            set,
            field: SET_ROW_NAME,
            pick,
        });
        self.set_clear_armed = None;
        let warned = self.refresh_set_warnings();
        self.message = alloc::format!(
            "セット #{:02} 「{}」を編集中 — {} 部位 / 警告 {} 件",
            set + 1,
            name,
            used,
            warned
        );
        true
    }

    /// Leave the editor and go back to the list.
    ///
    /// Only [A] ever writes a part, so leaving cannot lose an edit that was
    /// already made: the bytes are in the open save and reach the file with the
    /// save key pressed on the list.
    pub fn close_set_editor(&mut self) {
        self.set_edit = None;
        self.set_clear_armed = None;
        self.set_warnings.clear();
        self.message = String::from("マイセット一覧に戻りました（[X] でセーブ保存）");
    }

    /// Re-read myset::warnings for the set on screen.
    ///
    /// Returns how many there are. Called when the editor opens, after every
    /// write and on [X] - never from the renderer, because warnings decodes the
    /// whole record and looks seven parts up in the equipment box.
    pub fn refresh_set_warnings(&mut self) -> usize {
        let Some(e) = self.set_edit else {
            self.set_warnings.clear();
            return 0;
        };
        let slot = self.slot;
        let set = e.set;
        self.set_warnings = match &self.save {
            Some(s) => crate::myset::warnings(s, slot, set),
            None => Vec::new(),
        };
        self.set_warnings.len()
    }

    /// One line for the footer: what the highlighted row is and what [A] does.
    pub fn set_row_hint(&self) -> String {
        let Some(e) = self.set_edit else {
            return String::from("装備セットを選んで [A]");
        };
        if e.field == SET_ROW_NAME {
            let name = self.editing_set().map(|r| r.name).unwrap_or_default();
            return alloc::format!("名前「{}」— [A] で入力", name);
        }
        let part = (e.field - 1).min(crate::myset::EQUIP_SET_PARTS - 1);
        alloc::format!(
            "{} — [◀/▶] 装備BOX を巡回 / [A] 決定 / [-] 2回で外す",
            crate::myset::PART_NAMES[part]
        )
    }

    /// The my-sets list: A opens the editor, B leaves, everything else belongs
    /// to the console layer (it owns the row cursor and the tab).
    fn input_sets(&mut self, key: Key) {
        if self.set_edit.is_some() {
            self.input_set_edit(key);
            return;
        }
        match key {
            // The console layer opens the record its own cursor points at.
            // This arm is the headless door the PC build and the tests use, so
            // it takes the first set that holds something.
            Key::A => {
                if self.save.is_none() {
                    self.message = String::from("先にセーブを開いてください");
                    return;
                }
                match self.equip_sets().into_iter().find(|s| !s.is_empty()) {
                    Some(s) => {
                        self.open_set_editor(s.index);
                    }
                    None => {
                        self.message = String::from("編集できる装備セットがありません");
                    }
                }
            }
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_set_edit(&mut self, key: Key) {
        let Some(mut e) = self.set_edit else {
            return;
        };
        // Any key other than the confirmation itself means the user is no
        // longer looking at the part that was armed, so the pending clear is
        // dropped before the key does its own work.
        if key != Key::Minus {
            self.set_clear_armed = None;
        }
        match key {
            Key::Up => e.field = (e.field + SET_EDIT_ROWS - 1) % SET_EDIT_ROWS,
            Key::Down => e.field = (e.field + 1) % SET_EDIT_ROWS,
            // The name row has nothing to cycle: the two arrows open the
            // keyboard, the same way the character name row on the Char screen
            // does.
            Key::Left | Key::Right => {
                self.set_edit = Some(e);
                if e.field == SET_ROW_NAME {
                    self.ask_set_name(e.set);
                } else {
                    self.move_set_pick(e.field - 1, if key == Key::Right { 1 } else { -1 });
                }
                return;
            }
            Key::A => {
                self.set_edit = Some(e);
                if e.field == SET_ROW_NAME {
                    self.ask_set_name(e.set);
                } else {
                    self.commit_set_part();
                }
                return;
            }
            Key::Minus => {
                self.set_edit = Some(e);
                self.clear_set_part_under_cursor();
                return;
            }
            // Y reads the record back and says what looks wrong, leaving X
            // to save the way it does on every other screen. It repairs
            // nothing: the game loads every case warnings reports, so a silent
            // fix would change data the user did not ask about.
            Key::Y => {
                self.set_edit = Some(e);
                let n = self.refresh_set_warnings();
                self.message = if n == 0 {
                    String::from("警告はありません（部位と装備の種別は一致しています）")
                } else {
                    alloc::format!("警告 {} 件: {}", n, self.set_warnings.join(" / "))
                };
                return;
            }
            Key::B => {
                self.set_edit = Some(e);
                self.close_set_editor();
                return;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
        self.set_edit = Some(e);
    }

    /// Ask the software keyboard for the set's name.
    fn ask_set_name(&mut self, set: usize) {
        let current = self
            .equip_sets()
            .into_iter()
            .nth(set)
            .map(|r| r.name)
            .unwrap_or_default();
        self.pending_text = Some((TEXT_SET_NAME, current));
        self.message = String::from("セットの名前を入力してください（最大31B / 全角10文字）");
    }

    /// Walk the equipment box's used records for one part's pending index.
    ///
    /// Every used record is offered, not only the ones whose kind fits the
    /// part. myset::set_part deliberately does not check the kind - the sample
    /// save holds a set with a talisman in the head slot and the game loads it -
    /// so a picker that filtered them out could not reach the state the save
    /// already contains. The row shows the type name and [X] reports a mismatch.
    fn move_set_pick(&mut self, part: usize, d: i64) {
        if part >= crate::myset::EQUIP_SET_PARTS {
            return;
        }
        let ids = self.equip_record_indices();
        if ids.is_empty() {
            self.message = String::from("装備BOXが空です（先に装備BOXで装備を作ってください）");
            return;
        }
        let Some(mut e) = self.set_edit else {
            return;
        };
        let n = ids.len() as i64;
        let cur = e.pick[part];
        let next = match ids.iter().position(|i| *i as u16 == cur) {
            Some(p) => ((p as i64 + d).rem_euclid(n)) as usize,
            // The stored index is blank or outside the box: start from an end.
            None => {
                if d > 0 {
                    0
                } else {
                    (n - 1) as usize
                }
            }
        };
        e.pick[part] = ids[next] as u16;
        let field = e.field;
        self.set_edit = Some(e);
        self.message = alloc::format!(
            "[{}] {}（枠 #{:04} / 使用中 {} 枠中 {} 番目）— [A] で決定",
            field,
            self.set_part_label(part),
            e.pick[part],
            ids.len(),
            next + 1
        );
    }

    /// Write the part row's pending index into the set.
    fn commit_set_part(&mut self) {
        let Some(e) = self.set_edit else {
            return;
        };
        if e.field == SET_ROW_NAME {
            return;
        }
        let part = e.field - 1;
        if part >= crate::myset::EQUIP_SET_PARTS {
            return;
        }
        let idx = e.pick[part];
        if idx == crate::myset::NONE {
            self.message = alloc::format!(
                "{} は空です（[◀/▶] で装備を選んでください）",
                crate::myset::PART_NAMES[part]
            );
            return;
        }
        let slot = self.slot;
        let set = e.set;
        let label = self.set_part_label(part);
        let wrote = match &mut self.save {
            Some(s) => crate::myset::set_part(s, slot, set, part, idx),
            None => false,
        };
        if wrote {
            self.dirty = true;
            let n = self.refresh_set_warnings();
            self.message = alloc::format!(
                "{} に「{}」を入れました（枠 #{:04} / 装飾品の写しも同時に / 警告 {} 件）",
                crate::myset::PART_NAMES[part],
                label,
                idx,
                n
            );
        } else {
            self.message = alloc::format!(
                "枠 #{:04} には書き込めません（空の装備枠か、セーブが開かれていません）",
                idx
            );
        }
    }

    /// First half of the two-press clear: report, do not touch the record.
    ///
    /// The second half is the same key again with set_clear_armed naming the
    /// part. Nothing else in the set moves: myset::clear_part empties the index
    /// and its decoration copy only.
    fn clear_set_part_under_cursor(&mut self) {
        let Some(e) = self.set_edit else {
            return;
        };
        if e.field == SET_ROW_NAME {
            self.message = String::from("外せるのは部位の行だけです（名前は [A] で変更）");
            return;
        }
        let part = e.field - 1;
        if part >= crate::myset::EQUIP_SET_PARTS {
            return;
        }
        if e.pick[part] == crate::myset::NONE {
            self.set_clear_armed = None;
            self.message = alloc::format!("{} はすでに空です", crate::myset::PART_NAMES[part]);
            return;
        }
        if self.set_clear_armed != Some(part) {
            self.set_clear_armed = Some(part);
            let label = self.set_part_label(part);
            self.message = alloc::format!(
                "{} を外しますか？ もう一度 [-] で確定（いま「{}」）",
                crate::myset::PART_NAMES[part],
                label
            );
            return;
        }
        let slot = self.slot;
        let set = e.set;
        self.set_clear_armed = None;
        let wrote = match &mut self.save {
            Some(s) => crate::myset::clear_part(s, slot, set, part),
            None => false,
        };
        if wrote {
            self.dirty = true;
            if let Some(mut ed) = self.set_edit {
                ed.pick[part] = crate::myset::NONE;
                self.set_edit = Some(ed);
            }
            let n = self.refresh_set_warnings();
            self.message = alloc::format!(
                "{} を外しました（装飾品の写しもゼロにしました / 警告 {} 件）",
                crate::myset::PART_NAMES[part],
                n
            );
        } else {
            self.message = String::from("外せませんでした");
        }
    }

    /// The io layer answers a set-name request with this.
    pub fn apply_set_name(&mut self, text: String) {
        let Some(e) = self.set_edit else {
            return;
        };
        let slot = self.slot;
        let set = e.set;
        let wrote = match &mut self.save {
            Some(s) => crate::myset::set_name(s, slot, set, &text),
            None => false,
        };
        if wrote {
            self.dirty = true;
            let n = self.refresh_set_warnings();
            self.message = alloc::format!(
                "セット名を「{}」にしました（[B] で一覧 → [X] でセーブ保存 / 警告 {} 件）",
                text,
                n
            );
        } else {
            self.message = String::from("セット名を書き込めませんでした");
        }
    }

    fn input_equip(&mut self, key: Key) {
        let n = self.equipment().len();
        // Anything other than the confirmation itself means the user is no
        // longer looking at the record that was armed, so the pending delete
        // is dropped before the key does its own work.
        if key != Key::Minus {
            self.equip_delete_armed = None;
        }
        match key {
            Key::Up => self.equip_cursor = self.equip_cursor.saturating_sub(1),
            Key::Down => {
                if n > 0 {
                    self.equip_cursor = (self.equip_cursor + 1).min(n - 1);
                }
            }
            Key::Y => {
                self.pending_text = Some((TEXT_EQUIP_FILTER, self.equip_filter.clone()));
            }
            // A opens the form for the highlighted record.
            Key::A => self.open_equip_editor(),
            Key::X => {
                self.equip_filter.clear();
                self.equip_cursor = 0;
            }
            // Two presses: the first only names what would go, the second one
            // clears the record. The reference check is the same one
            // equipops::clear runs, asked first so the first press cannot
            // delete anything by surprise.
            Key::Minus => self.delete_equip_under_cursor(),
            // The new-record form. Plus is the one key this screen can take
            // from the global "quit" shortcut (the console layer routes it
            // here only on this screen).
            Key::Plus => self.open_equip_creator(),
            Key::L => {
                self.screen = Screen::Guild;
            }
            Key::R => {
                self.screen = Screen::Roster;
                self.roster_cursor = 0;
            }
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
            _ => {}
        }
    }

    /// First half of the two-press delete: report, do not touch the record.
    ///
    /// The second half is the same key again with `equip_delete_armed` set to
    /// the record under the cursor.
    fn delete_equip_under_cursor(&mut self) {
        let list = self.equipment();
        let Some(eq) = list.get(self.equip_cursor).copied() else {
            self.equip_delete_armed = None;
            self.message = String::from("削除する装備がありません");
            return;
        };
        let index = eq.index;
        let slot = self.slot;
        let second_press = self.equip_delete_armed == Some(index);

        if !second_press {
            let Some(save) = &self.save else {
                self.message = String::from("先にセーブを開いてください");
                return;
            };
            // The same question equipops::clear asks, asked before anything is
            // written. Calling clear(false) here would already have cleared an
            // unreferenced record, and a delete must never happen on the first
            // press.
            let refs = equiprefs::references(save, slot, index);
            let count = refs.len();
            self.equip_delete_armed = Some(index);
            self.message = if refs.is_empty() {
                format!("枠 #{} を削除しますか？ もう一度 [-] で削除確定", index)
            } else {
                format!(
                    "{}。もう一度 [-] で強制削除します（参照 {} 件）",
                    EquipError::Referenced(refs).text(),
                    count
                )
            };
            return;
        }

        // Second press: go ahead. `force` is what the armed state earned.
        let Some(save) = &mut self.save else {
            self.equip_delete_armed = None;
            self.message = String::from("先にセーブを開いてください");
            return;
        };
        match equipops::clear(save, slot, index, true) {
            Ok(()) => {
                self.dirty = true;
                self.equip_delete_armed = None;
                let list = self.equipment();
                let pos = list.iter().position(|q| q.index == index);
                self.equip_cursor =
                    pos.unwrap_or_else(|| self.equip_cursor.min(list.len().saturating_sub(1)));
                self.message = format!(
                    "枠 #{} を削除しました（{}）",
                    index,
                    crate::equip::type_name(eq.kind)
                );
            }
            Err(err) => {
                self.equip_delete_armed = None;
                self.message = err.text();
            }
        }
    }

    /// What the next [-] would do, for the screens and the tests.
    ///
    /// `None` when no delete is armed. Otherwise the box index, how many worn
    /// slots and saved sets still point at it, and the line naming them.
    pub fn equip_delete_pending(&self) -> Option<(usize, usize, alloc::string::String)> {
        let index = self.equip_delete_armed?;
        let save = self.save.as_ref()?;
        let refs = equiprefs::references(save, self.slot, index);
        let count = refs.len();
        let text = if refs.is_empty() {
            format!("枠 #{} を削除しますか？ もう一度 [-] で削除確定", index)
        } else {
            format!(
                "{}。もう一度 [-] で強制削除します（参照 {} 件）",
                EquipError::Referenced(refs).text(),
                count
            )
        };
        Some((index, count, text))
    }

    /// Open the form in its create mode (type, id, level; no record behind it
    /// yet, so no layer or sockets to show).
    fn open_equip_creator(&mut self) {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return;
        }
        self.equip_delete_armed = None;
        self.equip_edit = Some(EquipEdit {
            index: 0,
            kind: 1,
            id: 1,
            level: 1,
            transmog_id: 0,
            transmog_level: 1,
            decorations: [0; 3],
            field: 0,
            new: true,
        });
        self.screen = Screen::EquipEdit;
        self.message = String::from("新規装備: 種別・ID・レベルを決めて [A] で作成");
    }

    /// Box indices that already hold a record, in box order.
    ///
    /// The row that walks the box only ever moves between these: a blank
    /// record has nothing to edit, and `equipops::create` is what fills one.
    fn equip_record_indices(&self) -> Vec<usize> {
        let Some(save) = &self.save else {
            return Vec::new();
        };
        let Some(box_) = save.equipment_box(self.slot) else {
            return Vec::new();
        };
        (0..crate::equip::EQUIP_COUNT)
            .filter(|i| {
                let o = i * crate::equip::EQUIP_RECORD;
                box_.get(o..o + crate::equip::EQUIP_RECORD)
                    .map(|rec| !crate::equip::record_is_blank(rec))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Read one box record into the form, keeping the cursor row.
    ///
    /// Returns false when the slot is blank or the box cannot be read, which
    /// is what keeps a write from landing on a record that holds nothing.
    fn load_equip_form(&mut self, index: usize) -> bool {
        let Some(save) = &self.save else {
            return false;
        };
        let Some(box_) = save.equipment_box(self.slot) else {
            return false;
        };
        let o = index * crate::equip::EQUIP_RECORD;
        let Some(rec) = box_.get(o..o + crate::equip::EQUIP_RECORD) else {
            return false;
        };
        if crate::equip::record_is_blank(rec) {
            return false;
        }
        let Some(eq) = crate::equip::decode_record(index, rec) else {
            return false;
        };
        // Moving off the armed record is a cursor move like any other.
        self.equip_delete_armed = None;
        let field = self
            .equip_edit
            .map(|e| e.field)
            .unwrap_or(1)
            .min(EQUIP_EDIT_ROWS - 1);
        self.equip_edit = Some(EquipEdit {
            index,
            kind: eq.kind,
            id: eq.id,
            level: eq.level,
            transmog_id: eq.transmog,
            transmog_level: eq.transmog_level,
            decorations: crate::equip::Equipment::decorations(index, rec),
            field,
            new: false,
        });
        true
    }

    /// Open the equipment form on the record under the list cursor.
    fn open_equip_editor(&mut self) {
        let list = self.equipment();
        let Some(eq) = list.get(self.equip_cursor).copied() else {
            self.message = String::from("編集できる装備がありません");
            return;
        };
        if !self.load_equip_form(eq.index) {
            self.message = String::from("この枠は空なので編集できません");
            return;
        }
        self.screen = Screen::EquipEdit;
        self.message = alloc::format!(
            "{} を編集中（A で書き込み / B で戻る）",
            crate::equip::name(&eq)
        );
    }

    fn input_equip_edit(&mut self, key: Key) {
        let Some(mut e) = self.equip_edit else {
            self.screen = Screen::Equip;
            return;
        };
        let rows = e.rows();
        match key {
            Key::Up => e.field = (e.field + rows - 1) % rows,
            Key::Down => e.field = (e.field + 1) % rows,
            Key::Left | Key::Right => {
                let d: i64 = if key == Key::Right { 1 } else { -1 } * STEPS[self.step].min(10).max(1);
                if e.new {
                    // 種別 / ID / レベル: the three values create() writes.
                    match e.field {
                        0 => e.kind = cycle_equip_kind(e.kind, d),
                        1 => e.id = clamp_u16(e.id, d),
                        _ => e.level = cycle_equip_level(e.level, d),
                    }
                } else {
                    match e.field {
                        // The slot row walks the records that already hold
                        // something; the form never points at an empty slot.
                        0 => {
                            let ids = self.equip_record_indices();
                            let n = ids.len();
                            if n > 0 {
                                let pos = ids.iter().position(|i| *i == e.index).unwrap_or(0);
                                let next = if d > 0 { (pos + 1) % n } else { (pos + n - 1) % n };
                                if self.load_equip_form(ids[next]) {
                                    if let Some(reloaded) = self.equip_edit {
                                        e = reloaded;
                                    }
                                    e.field = 0;
                                    self.message = alloc::format!("枠 #{} を編集中", e.index);
                                }
                            }
                        }
                        // Level and layer level are five bit fields stored
                        // minus one, so 1..=32 wraps rather than clamping.
                        1 => e.level = cycle_equip_level(e.level, d),
                        2 => e.transmog_id = clamp_u16(e.transmog_id, d),
                        3 => e.transmog_level = cycle_equip_level(e.transmog_level, d),
                        // The decorations step through the ids the name table
                        // knows, so a socket never takes an id the game has no
                        // decoration for.
                        4..=6 => {
                            let ids = decoration_ids();
                            let s = e.field - 4;
                            if !ids.is_empty() {
                                let pos = ids.iter().position(|i| *i == e.decorations[s]).unwrap_or(0);
                                let n = ids.len() as i64;
                                let next = ((pos as i64 + d).rem_euclid(n)) as usize;
                                e.decorations[s] = ids[next];
                            }
                        }
                        // 種別 / ID. Changing either is what makes A replace
                        // the record instead of patching its fields.
                        7 => e.kind = cycle_equip_kind(e.kind, d),
                        _ => e.id = clamp_u16(e.id, d),
                    }
                }
            }
            Key::Y => {
                if e.decoration_socket(e.field).is_some() {
                    self.equip_edit = Some(e);
                    self.jewel_pick_filter.clear();
                    self.jewel_pick_cursor = 0;
                    self.screen = Screen::JewelPick;
                    self.message = String::from("装飾品を名前から選んでください");
                } else if e.new {
                    self.message =
                        String::from("装飾品は作成後に選べます（[A] で作成してから編集）");
                } else {
                    self.message = String::from("先に装飾品の行へ移動してください");
                }
            }
            Key::X => self.step = (self.step + 1) % STEPS.len(),
            Key::A => {
                if e.new {
                    self.write_new_equip(&e);
                } else {
                    self.write_equip_edit(&e);
                }
                return;
            }
            Key::B => {
                // The working copy is dropped: only A writes.
                self.equip_edit = None;
                self.screen = Screen::Equip;
                return;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
        self.equip_edit = Some(e);
    }

    /// Write the form's values for an existing record, replacing it when its
    /// type or id changed.
    fn write_equip_edit(&mut self, e: &EquipEdit) {
        let idx = e.index;
        let slot = self.slot;
        let current = match self.save.as_ref().and_then(|s| s.equipment_at(slot, idx)) {
            Some(q) => q,
            None => {
                self.message = String::from("この枠は空なので編集できません");
                return;
            }
        };
        let identity_changed = current.kind != e.kind || current.id != e.id;

        if identity_changed {
            // equipops::replace is the only writer that may move the type and
            // the id: it also empties the three sockets, because they belonged
            // to the item that was there. Writing them back afterwards from
            // the form would put old decorations on the new item.
            let Some(save) = &mut self.save else { return };
            let replaced = match equipops::replace(save, slot, idx, e.kind, e.id, e.level) {
                Ok(()) => true,
                Err(err) => {
                    self.message = err.text();
                    false
                }
            };
            if !replaced {
                return;
            }
            // The layer id at +0x04 is not part of replace's contract: it is a
            // field edit like the level, and the form still shows it.
            if let Some(rec) = save.equipment_record_mut(slot, idx) {
                crate::equip::set_transmog(rec, e.transmog_id, e.transmog_level);
            }
            self.dirty = true;
            let pos = self.equipment().iter().position(|q| q.index == idx).unwrap_or(0);
            self.equip_cursor = pos;
            self.message = alloc::format!(
                "枠 #{} を {} ID {} に差し替えました（装飾品は外れました / Lv{}）",
                idx,
                crate::equip::type_name(e.kind),
                e.id,
                e.level
            );
            self.equip_edit = None;
            self.screen = Screen::Equip;
            return;
        }

        let mut written = false;
        if let Some(save) = &mut self.save {
            if let Some(rec) = save.equipment_record_mut(slot, idx) {
                if !crate::equip::record_is_blank(rec) {
                    // +0x00..+0x0C only. The rest of the record - the
                    // talisman's skills above all - is left as it was,
                    // because those bytes are in use on this save.
                    let mut ok = crate::equip::set_level(rec, e.level);
                    ok &= crate::equip::set_transmog(rec, e.transmog_id, e.transmog_level);
                    for s in 0..3 {
                        ok &= crate::equip::set_decoration(rec, s, e.decorations[s]);
                    }
                    written = ok;
                }
            }
        }
        if written {
            self.dirty = true;
            let pos = self.equipment().iter().position(|q| q.index == idx).unwrap_or(0);
            self.equip_cursor = pos;
            self.message = alloc::format!(
                "{} 枠 #{} を書き込みました（Lv{}）",
                crate::equip::type_name(e.kind),
                idx,
                e.level
            );
        } else {
            self.message = String::from("書き込めませんでした（空き枠は編集できません）");
        }
        self.equip_edit = None;
        self.screen = Screen::Equip;
    }

    /// Create the form's values in the first blank record of the box.
    fn write_new_equip(&mut self, e: &EquipEdit) {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = String::from("先にセーブを開いてください");
            return;
        };
        match equipops::create(save, slot, e.kind, e.id, e.level) {
            Ok(index) => {
                self.dirty = true;
                self.equip_edit = None;
                self.equip_delete_armed = None;
                self.screen = Screen::Equip;
                // Select the row that was just written, so the list shows what
                // create() produced instead of leaving the cursor behind.
                let list = self.equipment();
                let pos = list.iter().position(|q| q.index == index);
                self.equip_cursor = pos.unwrap_or_else(|| self.equip_cursor.min(list.len().saturating_sub(1)));
                self.message = alloc::format!(
                    "枠 #{} に {} ID {} Lv{} を新規作成しました",
                    index,
                    crate::equip::type_name(e.kind),
                    e.id,
                    e.level
                );
            }
            Err(err) => self.message = err.text(),
        }
    }

    /// Decoration ids whose name matches the current filter.
    ///
    /// The empty socket is always first, so a decoration can be taken back out
    /// with the same picker that put it in.
    pub fn jewel_pick_matches(&self) -> Vec<u16> {
        let needle = self.jewel_pick_filter.trim().to_lowercase();
        let mut out: Vec<u16> = Vec::new();
        if needle.is_empty() || "なし".contains(needle.as_str()) {
            out.push(0);
        }
        #[cfg(feature = "gamedata")]
        {
            // Walk the two tables side by side instead of looking every id up:
            // this list is rebuilt on every frame the picker is open. Both the
            // Japanese and the English name answer the filter, because the row
            // shows the Japanese one while the table the reader may know from
            // the PC build is the English one.
            for (i, id) in crate::gamedata::JWL_I_DS.iter().enumerate() {
                if *id <= 0 || *id > u16::MAX as i32 {
                    continue;
                }
                let id = *id as u16;
                let jp = crate::gamedata::ITEM_NAME_LIST
                    .get(id as usize)
                    .copied()
                    .unwrap_or("");
                let en = crate::gamedata::JWL_NAMES.get(i).copied().unwrap_or("");
                if needle.is_empty()
                    || jp.to_lowercase().contains(&needle)
                    || en.to_lowercase().contains(&needle)
                {
                    out.push(id);
                }
            }
        }
        #[cfg(not(feature = "gamedata"))]
        {
            let _ = needle;
        }
        out
    }

    /// The io layer answers a decoration filter request with this.
    pub fn apply_jewel_filter(&mut self, text: String) {
        self.jewel_pick_filter = text;
        self.jewel_pick_cursor = 0;
    }

    fn input_jewel_pick(&mut self, key: Key) {
        let n = self.jewel_pick_matches().len();
        match key {
            Key::Up => {
                self.jewel_pick_cursor =
                    (self.jewel_pick_cursor + n.saturating_sub(1)) % n.max(1)
            }
            Key::Down => self.jewel_pick_cursor = (self.jewel_pick_cursor + 1) % n.max(1),
            Key::Left => self.jewel_pick_cursor = self.jewel_pick_cursor.saturating_sub(10),
            Key::Right => {
                if n > 0 {
                    self.jewel_pick_cursor = (self.jewel_pick_cursor + 10).min(n - 1);
                }
            }
            Key::Y => {
                self.pending_text = Some((TEXT_JEWEL_FILTER, self.jewel_pick_filter.clone()));
            }
            Key::X => {
                self.jewel_pick_filter.clear();
                self.jewel_pick_cursor = 0;
            }
            Key::A => {
                let matches = self.jewel_pick_matches();
                if let Some(&id) = matches.get(self.jewel_pick_cursor) {
                    if let Some(e) = &mut self.equip_edit {
                        if let Some(socket) = e.decoration_socket(e.field) {
                            e.decorations[socket] = id;
                        }
                    }
                    self.message = alloc::format!(
                        "{} を選びました（フォームに A で書き込み）",
                        decoration_name_of(id)
                    );
                }
                self.screen = Screen::EquipEdit;
            }
            Key::B => self.screen = Screen::EquipEdit,
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_convert(&mut self, key: Key) {
        match key {
            Key::A => self.convert_now(),
            // Name the output instead of taking the default.
            Key::Y => {
                self.pending_text = Some((TEXT_SAVE_NAME, self.convert_name.clone()));
            }
            Key::B => {
                self.screen = Screen::SaveList;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_skill_pick(&mut self, key: Key) {
        let n = self.skill_pick_matches().len();
        match key {
            Key::Up => {
                self.skill_pick_cursor = (self.skill_pick_cursor + n.saturating_sub(1)) % n.max(1)
            }
            Key::Down => self.skill_pick_cursor = (self.skill_pick_cursor + 1) % n.max(1),
            Key::Left => self.skill_pick_cursor = self.skill_pick_cursor.saturating_sub(10),
            Key::Right => {
                if n > 0 {
                    self.skill_pick_cursor = (self.skill_pick_cursor + 10).min(n - 1);
                }
            }
            Key::Y => {
                self.pending_text = Some((TEXT_SKILL_FILTER, self.skill_pick_filter.clone()));
            }
            Key::X => {
                self.skill_pick_filter.clear();
                self.skill_pick_cursor = 0;
            }
            Key::A => {
                let matches = self.skill_pick_matches();
                if let Some(&id) = matches.get(self.skill_pick_cursor) {
                    if let Some(e) = &mut self.charm_edit {
                        if self.skill_pick_field == 1 {
                            e.skill1 = id;
                        } else {
                            e.skill2 = id;
                        }
                    }
                    let name = crate::skills::skill_name(id).unwrap_or("?");
                    self.message =
                        alloc::format!("skill {} -> {} ({})", self.skill_pick_field, name, id);
                }
                self.screen = Screen::CharmEdit;
            }
            Key::B => self.screen = Screen::CharmEdit,
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_item_pick(&mut self, key: Key) {
        // Stage two of "アイテムを追加": the item is chosen, so Left / Right
        // now pick the count and A adds it. The list underneath is kept on
        // screen (it still names the item) but no longer moves.
        if self.item_pick_mode == 3 && self.item_pick_stage == 1 {
            match key {
                Key::Left => {
                    self.item_pick_add_count = self.item_pick_add_count.saturating_sub(1).max(1);
                    self.message = self.item_add_message();
                }
                Key::Right => {
                    self.item_pick_add_count = (self.item_pick_add_count + 1).min(MAX_COUNT);
                    self.message = self.item_add_message();
                }
                Key::Up => {
                    self.item_pick_add_count = (self.item_pick_add_count + 10).min(MAX_COUNT);
                    self.message = self.item_add_message();
                }
                Key::Down => {
                    self.item_pick_add_count = self.item_pick_add_count.saturating_sub(10).max(1);
                    self.message = self.item_add_message();
                }
                Key::A => self.apply_item_add(),
                Key::B => {
                    self.item_pick_stage = 0;
                    self.message = String::from("追加するアイテムを選び直してください");
                }
                Key::Plus => self.quit = true,
                _ => {}
            }
            return;
        }
        let n = self.item_pick_matches().len();
        match key {
            Key::Up => self.item_pick_cursor = (self.item_pick_cursor + n.saturating_sub(1)) % n.max(1),
            Key::Down => self.item_pick_cursor = (self.item_pick_cursor + 1) % n.max(1),
            Key::Left => self.item_pick_cursor = self.item_pick_cursor.saturating_sub(10),
            Key::Right => {
                if n > 0 {
                    self.item_pick_cursor = (self.item_pick_cursor + 10).min(n - 1);
                }
            }
            Key::Y => {
                self.pending_text = Some((TEXT_ITEM_FILTER, self.item_pick_filter.clone()));
            }
            Key::X => {
                self.item_pick_filter.clear();
                self.item_pick_cursor = 0;
            }
            Key::A => {
                let matches = self.item_pick_matches();
                if self.item_pick_mode == 1 {
                    if let Some(&id) = matches.get(self.item_pick_cursor) {
                        self.store_reward_item(id);
                    }
                    self.screen = Screen::EntryView;
                    return;
                }
                // Mode 2 writes into a quest-equipment entry instead of the
                // item box.
                if self.item_pick_mode == 2 {
                    if let Some(&id) = matches.get(self.item_pick_cursor) {
                        let t = self.qe_target;
                        let slot = self.slot;
                        let slot_index = self.cursor;
                        if let Some(s) = &mut self.save {
                            if crate::dlc::set_quest_equipment_item(
                                s,
                                slot_index,
                                t.record,
                                t.list,
                                t.index,
                                id,
                                t.count.max(1),
                            )
                            .is_ok()
                            {
                                self.dirty = true;
                                self.message = alloc::format!(
                                    "クエスト装備 rec{} {}[{}] -> {} ({})",
                                    t.record,
                                    match t.list {
                                        crate::dlc::QuestEquipList::Supply => "支給品",
                                        crate::dlc::QuestEquipList::Ammo => "弾",
                                    },
                                    t.index,
                                    item_name_of(id),
                                    id
                                );
                            } else {
                                self.message = String::from("書き込めませんでした");
                                let _ = slot;
                            }
                        }
                    }
                    self.screen = Screen::QuestEquip;
                    return;
                }
                // Mode 3 is the bulk add: A only chooses the item, the count
                // comes next and nothing is written yet.
                if self.item_pick_mode == 3 {
                    if let Some(&id) = matches.get(self.item_pick_cursor) {
                        self.item_pick_add_id = id;
                        self.item_pick_stage = 1;
                        if self.item_pick_add_count == 0 {
                            self.item_pick_add_count = 1;
                        }
                        self.message = self.item_add_message();
                    }
                    return;
                }
                if let Some(&id) = matches.get(self.item_pick_cursor) {
                    let slot = self.slot;
                    let box_i = self.item_pick_target;
                    if let Some(mut items) = self.save.as_ref().and_then(|s| s.read_item_box(slot)) {
                        if box_i < items.len() {
                            self.push_item_undo();
                            items[box_i].id = id;
                            if items[box_i].count == 0 {
                                items[box_i].count = 1;
                            }
                            if let Some(s) = &mut self.save {
                                let _ = s.write_item_box(slot, &items);
                                self.dirty = true;
                            }
                            let name = item_name_of(id);
                            self.message = format!("box #{} -> {} ({})", box_i + 1, name, id);
                        }
                    }
                    self.screen = Screen::Items;
                    self.cursor = 0;
                }
            }
            Key::B => {
                if self.item_pick_mode == 1 {
                    self.screen = Screen::EntryView;
                } else if self.item_pick_mode == 2 {
                    self.screen = Screen::QuestEquip;
                } else {
                    // Back to the box, and back into the menu the add started
                    // from when it came from there.
                    if self.item_pick_mode == 3 {
                        self.item_pick_stage = 0;
                        self.item_menu_open = true;
                        self.item_menu_cursor = ITEM_OP_ADD;
                    }
                    self.screen = Screen::Items;
                    self.cursor = 0;
                }
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_arc_list(&mut self, key: Key) {
        match key {
            Key::Up => self.move_cursor(self.arc_files.len(), -1),
            Key::Down => self.move_cursor(self.arc_files.len(), 1),
            Key::A => {
                if !self.arc_files.is_empty() {
                    self.pending_request = Some(Request::ReadArc(self.cursor));
                }
            }
            // X lists the quests that live inside the save instead of on disk.
            Key::X => {
                if self.save.is_none() {
                    self.message = String::from("先にセーブを開いてください");
                    return;
                }
                let n = self.save_quests().len();
                // Decode the titles once here rather than per frame.
                self.refresh_quest_titles();
                self.screen = Screen::SaveQuests;
                self.save_quest_cursor = self.save_quest_cursor.min(n.saturating_sub(1));
                self.cursor = 0;
                self.message = format!("{} quest(s) inside this save", n);
            }
            Key::Y => {
                self.screen = Screen::SaveList;
                self.cursor = 0;
                self.message = String::from("セーブ編集");
            }
            // Z installs the highlighted archive into the save: the one action
            // the console could not do at all before.
            Key::Z => self.install_selected_arc(),
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Install the highlighted .arc into the save's download quest slots.
    ///
    /// The id comes from the archive itself rather than its file name: every
    /// quest carries a .mib whose first field is the id.
    fn install_selected_arc(&mut self) {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return;
        }
        let Some(path) = self.arc_files.get(self.cursor).cloned() else {
            self.message = String::from("クエストが選ばれていません");
            return;
        };
        // The archive has to be open already: the console layer owns file
        // access, so installing straight from the list would need a second
        // round trip. Pressing A first is one extra button.
        let bytes = match (&self.arc, self.arc_path.as_deref()) {
            (Some(arc), Some(open)) if open == path => match arc.to_bytes() {
                Ok(b) => b,
                Err(_) => {
                    self.message = String::from("アーカイブを書き出せません");
                    return;
                }
            },
            _ => {
                self.message = String::from("先に A でクエストを開いてから Z を押してください");
                return;
            }
        };
        let Some(id) = quest_id_of_arc(&bytes) else {
            self.message = String::from("クエストIDが読めません（.mib がない）");
            return;
        };
        let Some(save) = &mut self.save else {
            return;
        };
        match crate::dlc::add_quest(save, id, &bytes) {
            Ok(slot) => {
                self.dirty = true;
                self.refresh_quest_titles();
                self.message = format!("クエスト {} を枠 {} に追加しました", id, slot);
            }
            Err(e) => self.message = format!("追加できません: {:?}", e),
        }
    }

    /// Decode every installed quest's title into the cache.
    pub fn refresh_quest_titles(&mut self) {
        self.quest_titles.clear();
        for q in self.save_quests() {
            if q.data.is_empty() {
                continue;
            }
            let title = crate::arc::Arc::parse(&q.data)
                .ok()
                .and_then(|a| {
                    a.by_ext("gmd")
                        .and_then(|e| crate::quest::Gmd::from_bytes(&e.data).ok())
                })
                .and_then(|g| g.title().map(alloc::string::ToString::to_string))
                .unwrap_or_default();
            self.quest_titles.push((q.quest_id, title));
        }
    }

    /// The save's quests after the filter, in slot order.
    pub fn filtered_quests(&self) -> Vec<crate::dlc::InstalledQuest> {
        let needle = self.quest_filter.trim().to_lowercase();
        self.save_quests()
            .into_iter()
            .filter(|q| {
                if needle.is_empty() {
                    return true;
                }
                format!("{}", q.quest_id).contains(&needle)
                    || self.quest_title(q.quest_id).to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// The io layer answers a save-name request with this.
    pub fn apply_convert_name(&mut self, text: String) {
        self.convert_name = text;
    }

    /// The io layer answers a quest filter request with this.
    pub fn apply_quest_filter(&mut self, text: String) {
        self.quest_filter = text;
        self.save_quest_cursor = 0;
    }

    /// Cached title for a quest id.
    pub fn quest_title(&self, id: i32) -> &str {
        if let Some((_, t)) = self
            .quest_titles
            .iter()
            .find(|(q, t)| *q == id && !t.is_empty())
        {
            return t.as_str();
        }
        crate::dlc_bundle::official_quest_title(id).unwrap_or("")
    }

    /// Install all 189 bundled official event & challenge quests (plus the 40
    /// challenge quest equipment loadouts and DLC section header flags) into
    /// the open save.
    #[cfg(feature = "gamedata")]
    pub fn install_official_dlc(&mut self) {
        self.install_dlc_category(true, false, "公式イベント(189件)");
    }

    /// Install bundled quests matching the specified categories into the open save.
    #[cfg(feature = "gamedata")]
    pub fn install_dlc_category(&mut self, official: bool, dead: bool, label: &str) {
        let Some(save) = &mut self.save else {
            self.message = String::from("先にセーブデータを開いてください");
            return;
        };
        match crate::dlc::install_dlc_quests_by_category(save, official, dead) {
            Ok((added, total)) => {
                self.dirty = true;
                self.refresh_quest_titles();
                let n = self.filtered_quests().len();
                self.save_quest_cursor = self.save_quest_cursor.min(n.saturating_sub(1));
                self.cursor = self.save_quest_cursor;
                if added > 0 {
                    self.message = format!("{} を {} 件追加しました（計 {} 件）", label, added, total);
                } else {
                    self.message = format!("{} はすべて導入済みです（計 {} 件）", label, total);
                }
            }
            Err(e) => {
                self.message = format!("{} の追加に失敗: {}", label, e);
            }
        }
    }

    /// Toggle one bundled quest (install into free slot or remove from save).
    #[cfg(feature = "gamedata")]
    pub fn toggle_bundled_quest(&mut self, quest_id: i32) {
        let Some(save) = &mut self.save else {
            self.message = String::from("先にセーブデータを開いてください");
            return;
        };
        let title = crate::dlc_bundle::official_quest_title(quest_id).unwrap_or("");
        match crate::dlc::toggle_bundled_quest(save, quest_id) {
            Ok((slot, true)) => {
                self.dirty = true;
                self.refresh_quest_titles();
                self.message = format!(
                    "クエスト {} 「{}」を枠 {:03} に導入しました！",
                    quest_id, title, slot
                );
            }
            Ok((slot, false)) => {
                self.dirty = true;
                self.refresh_quest_titles();
                self.message = format!(
                    "クエスト {} 「{}」（枠 {:03}）をセーブから削除しました",
                    quest_id, title, slot
                );
            }
            Err(e) => {
                self.message = format!("クエスト {} の切替に失敗しました: {}", quest_id, e);
            }
        }
    }

    /// Remove a quest from the open save by its quest ID.
    #[cfg(feature = "gamedata")]
    pub fn remove_save_quest(&mut self, quest_id: i32) {
        let title = self.quest_title(quest_id).to_string();
        let Some(save) = &mut self.save else {
            self.message = String::from("先にセーブデータを開いてください");
            return;
        };
        match crate::dlc::remove_quest_by_id(save, quest_id) {
            Ok(slot) => {
                self.dirty = true;
                self.refresh_quest_titles();
                let n = self.filtered_quests().len();
                self.save_quest_cursor = self.save_quest_cursor.min(n.saturating_sub(1));
                self.cursor = self.save_quest_cursor;
                self.message = format!(
                    "クエスト {} 「{}」（枠 {:03}）をセーブから削除しました",
                    quest_id, title, slot
                );
            }
            Err(e) => {
                self.message = format!("削除失敗: {}", e);
            }
        }
    }

    /// Open a quest either from the save (if installed) or directly from the bundled `.arc` catalogue.
    pub fn open_bundled_quest(&mut self, quest_id: i32) {
        if self.save_quests().iter().any(|q| q.quest_id == quest_id) {
            self.open_quest_from_save(quest_id);
            return;
        }
        let Some(meta) = crate::dlc_bundle::find_official_quest(quest_id) else {
            self.message = format!("クエスト {} が見つかりません", quest_id);
            return;
        };
        let data = crate::dlc_bundle::arc_bytes(meta);
        match Arc::parse(data) {
            Ok(a) => {
                let idx = a
                    .entries
                    .iter()
                    .position(|e| extension_for(e.type_hash) == Some("mib"))
                    .or_else(|| {
                        a.entries
                            .iter()
                            .position(|e| extension_for(e.type_hash) == Some("gmd"))
                    })
                    .unwrap_or(0);
                self.arc = Some(a);
                self.arc_path = None;
                self.entry = idx;
                self.cursor = idx;
                self.load_entry();
                self.screen = Screen::ArcEntries;
                self.message = format!(
                    "内蔵クエスト {} 「{}」を開きました（[X]でセーブへ導入できます）",
                    quest_id, meta.title
                );
            }
            Err(e) => self.message = format!("クエスト {} を開けません: {}", quest_id, e),
        }
    }

    /// The download quests stored in the open save, in slot order.
    pub fn save_quests(&self) -> Vec<dlc::InstalledQuest> {
        self.save.as_ref().map(dlc::list_quests).unwrap_or_default()
    }

    /// Load one of the save's own quests into the quest editor.
    ///
    /// The bytes are copied into memory, so nothing is written until the user
    /// asks for it; the id keeps them tied together so X writes back to the
    /// same slot.
    pub fn open_quest_from_save(&mut self, quest_id: i32) {
        let Some(q) = self.save_quests().into_iter().find(|q| q.quest_id == quest_id) else {
            self.message = format!("quest {} is not in this save", quest_id);
            return;
        };
        if q.data.is_empty() {
            self.message = format!("quest {} has no payload (size {})", quest_id, q.size);
            return;
        }
        match Arc::parse(&q.data) {
            Ok(a) => {
                let idx = a
                    .entries
                    .iter()
                    .position(|e| extension_for(e.type_hash) == Some("mib"))
                    .or_else(|| {
                        a.entries
                            .iter()
                            .position(|e| extension_for(e.type_hash) == Some("gmd"))
                    })
                    .unwrap_or(0);
                self.arc = Some(a);
                self.arc_path = None;
                self.entry = idx;
                // load_entry() answers for self.entry, so the list cursor has
                // to point at the same row or pressing A would re-open entry 0
                // and throw the .mib buffer away.
                self.cursor = idx;
                self.load_entry();
                self.screen = Screen::ArcEntries;
                self.message = format!("opened quest {} from the save ({} B)", quest_id, q.data.len());
            }
            Err(e) => self.message = format!("quest {} is not an .arc: {}", quest_id, e),
        }
    }

    fn input_save_quests(&mut self, key: Key) {
        let list = self.filtered_quests();
        let n = list.len();
        match key {
            Key::Up => {
                self.save_quest_cursor = (self.save_quest_cursor + n.saturating_sub(1)) % n.max(1);
                self.cursor = self.save_quest_cursor;
            }
            Key::Down => {
                self.save_quest_cursor = (self.save_quest_cursor + 1) % n.max(1);
                self.cursor = self.save_quest_cursor;
            }
            Key::A => {
                if let Some(q) = list.get(self.save_quest_cursor) {
                    let id = q.quest_id;
                    self.open_quest_from_save(id);
                }
            }
            Key::Minus => {
                if let Some(q) = list.get(self.save_quest_cursor) {
                    self.pending_request = Some(Request::ExtractQuests { ids: alloc::vec![q.quest_id] });
                }
            }
            Key::R => {
                self.pending_request = Some(Request::ExtractQuests { ids: Vec::new() });
            }
            // 175 quests at 16 rows a page needs a filter.
            Key::Y => {
                self.pending_text = Some((TEXT_QUEST_FILTER, self.quest_filter.clone()));
            }
            Key::X => {
                self.quest_filter.clear();
                self.save_quest_cursor = 0;
            }
            // L shows the quest-equipment entries behind the low mask bits.
            Key::L => {
                self.screen = Screen::QuestEquip;
                self.cursor = 0;
            }
            #[cfg(feature = "gamedata")]
            Key::Z => {
                self.install_official_dlc();
            }
            Key::B => {
                self.screen = Screen::ArcList;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_arc_entries(&mut self, key: Key) {
        let n = self.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0);
        match key {
            Key::Up => self.move_cursor(n, -1),
            Key::Down => self.move_cursor(n, 1),
            Key::A => {
                // Store the entry we are leaving first: the .mib and .gmd
                // editors keep their edits in mib_bytes/gmd, so loading a new
                // entry without this silently threw them away.
                self.store_entry();
                self.entry = self.cursor;
                self.load_entry();
                let kind = self
                    .arc
                    .as_ref()
                    .and_then(|a| a.entries.get(self.entry))
                    .and_then(|e| extension_for(e.type_hash));
                match kind {
                    Some("mib") => {
                        self.screen = Screen::Mib;
                        self.cursor = 0;
                    }
                    Some("gmd") => {
                        self.screen = Screen::Gmd;
                        self.cursor = 0;
                    }
                    _ => self.message = String::from("この種類のエントリは編集できません"),
                }
            }
            // Y shows the entry decoded, whatever its type. The six container
            // formats used to be unreadable on the console.
            Key::Y => {
                // The view renders self.entry, so the cursor has to be copied
                // across or every entry shows whichever one was opened last.
                self.entry = self.cursor;
                self.screen = Screen::EntryView;
                self.message = String::from("B で戻る");
            }
            Key::X => self.install_into_save(),
            Key::L => self.run_check(),
            // Pull the quest arc back out of the save as a file.
            Key::Minus => {
                let id = self
                    .arc
                    .as_ref()
                    .and_then(|a| a.by_ext("mib"))
                    .and_then(|m| m.data.get(0x0C..0x10))
                    .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .unwrap_or(0);
                if id == 0 {
                    self.message = String::from("クエストIDを読めません");
                } else {
                    self.pending_request = Some(Request::ExtractQuests { ids: alloc::vec![id] });
                }
            }
            Key::R => {
                self.pending_request = Some(Request::ExtractQuests { ids: Vec::new() });
            }
            Key::B => {
                self.screen = Screen::ArcList;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_mib(&mut self, key: Key) {
        let fields = questfields::MIB_FIELDS;
        let n = fields.len();
        match key {
            Key::Up => {
                self.mib_cursor = (self.mib_cursor + n - 1) % n;
                self.cursor = self.mib_cursor;
            }
            Key::Down => {
                self.mib_cursor = (self.mib_cursor + 1) % n;
                self.cursor = self.mib_cursor;
            }
            Key::X => self.step = (self.step + 1) % STEPS.len(),
            Key::Left | Key::Right => {
                let f = fields[self.mib_cursor];
                if f.kind == FieldKind::Bytes {
                    if f.key.starts_with("mob_slot[") && f.offset + 13 <= self.mib_bytes.len() {
                        let cur_id = u16::from_le_bytes([
                            self.mib_bytes[f.offset],
                            self.mib_bytes[f.offset + 1],
                        ]);
                        let mint = questfields::ENUM_MINT_;
                        let cur_idx = mint
                            .iter()
                            .position(|s| s.parse::<u16>().ok() == Some(cur_id))
                            .unwrap_or(0);
                        let len = (mint.len().saturating_sub(1)).max(1);
                        let next_idx = if key == Key::Right {
                            (cur_idx + 1) % len
                        } else {
                            (cur_idx + len - 1) % len
                        };
                        let next_id = mint
                            .get(next_idx)
                            .and_then(|s| s.parse::<u16>().ok())
                            .unwrap_or(0);
                        self.mib_bytes[f.offset..f.offset + 2]
                            .copy_from_slice(&next_id.to_le_bytes());
                        if next_id > 0 && self.mib_bytes[f.offset + 4] == 0 {
                            // Initialize default spawn count=1, hp=50, atk=50, def=50, size=100%, stamina=1
                            self.mib_bytes[f.offset + 4] = 1;
                            self.mib_bytes[f.offset + 5] = 50;
                            self.mib_bytes[f.offset + 6] = 50;
                            self.mib_bytes[f.offset + 7] = 50;
                            self.mib_bytes[f.offset + 9..f.offset + 11]
                                .copy_from_slice(&100u16.to_le_bytes());
                            self.mib_bytes[f.offset + 12] = 1;
                        } else if next_id == 0 {
                            self.mib_bytes[f.offset..f.offset + 13].fill(0);
                        }
                        self.dirty = true;
                        let mname = questfields::ENUM_MONS_
                            .get(next_idx)
                            .copied()
                            .unwrap_or("?");
                        self.message = format!("{} -> {} (ID: {})", f.ja, mname, next_id);
                        return;
                    }
                    self.message = String::from("生バイトの項目です（ここでは編集できません）");
                    return;
                }
                let cur = questfields::read_u32(&self.mib_bytes, f.offset, f.kind).unwrap_or(0);
                let next = if let Some(table) = f.enum_name.and_then(questfields::enum_table) {
                    let len = table.len() as i64;
                    if len == 0 {
                        cur
                    } else {
                        let d = if key == Key::Right { 1 } else { -1 };
                        (((cur as i64 + d) % len + len) % len) as u32
                    }
                } else {
                    let d = if key == Key::Right { 1 } else { -1 } * STEPS[self.step];
                    let width = match f.kind {
                        FieldKind::U8 | FieldKind::I8 => 8u32,
                        FieldKind::U16 | FieldKind::I16 => 16,
                        _ => 32,
                    };
                    let signed = matches!(f.kind, FieldKind::I8 | FieldKind::I16 | FieldKind::I32);
                    let v = if signed {
                        let shift = 32 - width;
                        ((cur << shift) as i32 >> shift) as i64 + d
                    } else {
                        cur as i64 + d
                    };
                    (v.max(0) as u64 & ((1u64 << width) - 1)) as u32
                };
                questfields::write_u32(&mut self.mib_bytes, f.offset, f.kind, next);
                self.dirty = true;
            }
            Key::A => self.request_write_arc(),
            Key::B => {
                self.screen = Screen::ArcEntries;
                self.cursor = self.entry;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_gmd(&mut self, key: Key) {
        let n = self.gmd.as_ref().map(|g| g.strings.len()).unwrap_or(0).max(1);
        match key {
            Key::Up => {
                self.gmd_cursor = (self.gmd_cursor + n - 1) % n;
                self.cursor = self.gmd_cursor;
            }
            Key::Down => {
                self.gmd_cursor = (self.gmd_cursor + 1) % n;
                self.cursor = self.gmd_cursor;
            }
            Key::A => {
                if let Some(g) = &self.gmd {
                    if let Some(s) = g.strings.get(self.gmd_cursor) {
                        self.pending_text = Some((self.gmd_cursor, s.clone()));
                    }
                }
            }
            Key::X => self.request_write_arc(),
            Key::B => {
                self.screen = Screen::ArcEntries;
                self.cursor = self.entry;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Install the currently open archive into the open save.
    pub fn install_into_save(&mut self) {
        // The .mib and .gmd editors keep their edits in mib_bytes/gmd, not in
        // the archive. Repacking without this wrote the archive as it was when
        // it was opened, so an edit followed by X silently saved nothing.
        self.store_entry();
        let Some(a) = &self.arc else {
            self.message = String::from("先にクエストを開いてください");
            return;
        };
        let id = a
            .by_ext("mib")
            .and_then(|e| Mib::from_bytes(&e.data).ok())
            .map(|m| m.quest_id())
            .unwrap_or(0);
        if id == 0 {
            self.message = String::from("クエストIDを読めません");
            return;
        }
        let raw = match a.to_bytes() {
            Ok(b) => b,
            Err(_) => {
                self.message = String::from("再構築に失敗しました");
                return;
            }
        };
        let Some(s) = &mut self.save else {
            self.message = String::from("先にセーブを開いてください");
            return;
        };
        // A quest the save already lists is overwritten in place; a new one
        // needs its catalogue bit as well, which is what add_quest does.
        let already = dlc::list_quests(s)
            .iter()
            .any(|q| q.quest_id == id as i32);
        let result = if already {
            dlc::install_quest(s, id as i32, &raw).map(|slot| (slot, false))
        } else {
            dlc::add_quest(s, id as i32, &raw).map(|slot| (slot, true))
        };
        match result {
            Ok((slot, added)) => {
                self.dirty = true;
                self.refresh_quest_titles();
                self.message = if added {
                    format!("クエスト {} を新規に枠 {} へ追加しました", id, slot)
                } else {
                    format!("クエスト {} を枠 {} に上書きしました", id, slot)
                };
            }
            Err(e) => self.message = format!("書き込み失敗: {:?}", e),
        }
    }

    // ------------------------------------------------------------- render

    /// Build the display lines for the current screen.
    pub fn lines(&self) -> Vec<Line> {
        let mut out: Vec<Line> = Vec::new();
        let push = |out: &mut Vec<Line>, text: String, selectable: bool, cursor: bool| {
            out.push(Line {
                text,
                selectable,
                cursor,
            });
        };

        match self.screen {
            Screen::SaveList => {
                push(&mut out, format!("SAVE FILES  ({} found)", self.save_files.len()), false, false);
                push(
                    &mut out,
                    String::from("本体のセーブ or /switch/mhnxeditor/saves/"),
                    false,
                    false,
                );
                for (i, f) in self.save_files.iter().enumerate() {
                    let label = if f == CONSOLE_SAVE {
                        String::from("  本体のセーブ (NAND) - 直接編集")
                    } else {
                        format!("  {}", f)
                    };
                    push(&mut out, label, true, i == self.cursor);
                }
                if self.save_files.is_empty() {
                    push(&mut out, String::from("  (none)"), false, false);
                }
            }
            Screen::Slots => {
                push(&mut out, String::from("キャラクター枠"), false, false);
                if let Some(s) = &self.save {
                    push(
                        &mut out,
                        format!(
                            "  type {}   body {} bytes",
                            match s.kind() {
                                SaveKind::ThreeDs => "3DS",
                                SaveKind::Switch => "Switch",
                                SaveKind::Mhgu => "MHGU",
                            },
                            s.body().len()
                        ),
                        false,
                        false,
                    );
                    for i in 0..3 {
                        let name = s
                            .read_character(i)
                            .map(|c| c.name)
                            .unwrap_or_default();
                        push(
                            &mut out,
                            format!(
                                "  slot {}: {}{}",
                                i + 1,
                                if name.is_empty() { "(empty)" } else { name.as_str() },
                                if s.slot_used(i) { "" } else { "  [unused]" }
                            ),
                            true,
                            i == self.cursor,
                        );
                    }
                }
                push(&mut out, String::from("  Up/Down slot   A char edit   X items"), false, false);
            }
            Screen::Char => {
                push(&mut out, format!("CHARACTER  slot {}", self.slot + 1), false, false);
                for (i, (key, ja)) in CHAR_FIELDS.iter().enumerate() {
                    let value = if i == 0 {
                        self.character().map(|c| c.name).unwrap_or_default()
                    } else {
                        format!("{}", self.char_value(i))
                    };
                    push(
                        &mut out,
                        format!("  {:<7} {}  {}", key, ja, value),
                        true,
                        i == self.field,
                    );
                }
                push(&mut out, format!("  step {}   (X)", STEPS[self.step]), false, false);
            }
            Screen::Items => {
                // The bulk menu replaces the list while it is open; the list
                // is one press away (B or Minus) and nothing below it moves.
                if self.item_menu_open {
                    let stats = self.item_menu_stats();
                    push(
                        &mut out,
                        format!(
                            "ITEM OPS  box {}  ({} used, {} free)",
                            self.slot + 1,
                            TOTAL_ITEM_SLOTS - stats.holes,
                            stats.holes
                        ),
                        false,
                        false,
                    );
                    for (i, (label, note)) in ITEM_OPS.iter().enumerate() {
                        let armed = i == ITEM_OP_DEDUPE && self.item_dedupe_armed;
                        push(
                            &mut out,
                            format!(
                                "  {:<12} {}{}",
                                label,
                                stats.value(i),
                                if armed { "  << press A again" } else { "" }
                            ),
                            true,
                            i == self.item_menu_cursor,
                        );
                        if i == self.item_menu_cursor {
                            push(&mut out, format!("      {}", note), false, false);
                        }
                    }
                    let p = stats.dedupe;
                    push(
                        &mut out,
                        format!(
                            "  重複整理の予定: {} 枠解放 / {} 個消失",
                            p.freed, p.lost
                        ),
                        false,
                        false,
                    );
                    push(
                        &mut out,
                        String::from("  Up/Down row  A run  - close"),
                        false,
                        false,
                    );
                    return out;
                }
                let vis = self.visible_items();
                push(
                    &mut out,
                    format!(
                        "ITEM BOX  {} shown{}",
                        vis.len(),
                        if self.show_empty { " (incl. empty)" } else { "" }
                    ),
                    false,
                    false,
                );
                if let Some(items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) {
                    let mut start = self.cursor.saturating_sub(8);
                    if start + 16 > vis.len() {
                        start = vis.len().saturating_sub(16);
                    }
                    for row in 0..16 {
                        if let Some(&idx) = vis.get(start + row) {
                            let s: ItemSlot = items[idx];
                            push(
                                &mut out,
                                format!("  #{:<5} id {:<5} x{}", idx + 1, s.id, s.count),
                                true,
                                self.cursor == start + row,
                            );
                        }
                    }
                    let cur = vis.get(self.cursor).copied().unwrap_or(0);
                    let s = items[cur];
                    push(
                        &mut out,
                        format!("  selected #{}, id {}, count {}", cur + 1, s.id, s.count),
                        false,
                        false,
                    );
                }
                push(
                    &mut out,
                    String::from(
                        "  Up/Down row  Left/Right count  - ops  R item  Y empty  L undo  A save",
                    ),
                    false,
                    false,
                );
            }
            Screen::EntryView => {
                use crate::entries::{Esl, ItemTable, Qdp, QuestLink, SemEntry};
                let entry = self
                    .arc
                    .as_ref()
                    .and_then(|a| a.entries.get(self.entry));
                match entry {
                    None => push(&mut out, String::from("エントリなし"), false, false),
                    Some(e) => {
                        let ext = extension_for(e.type_hash).unwrap_or("?");
                        push(
                            &mut out,
                            format!("{} ({} B)  .{}", e.file_name(), e.data.len(), ext),
                            false,
                            false,
                        );
                        match ext {
                            "rem" | "sup" => match ItemTable::parse(&e.data) {
                                Ok(t) => {
                                    push(
                                        &mut out,
                                        format!(
                                            "{} records of {} slots",
                                            t.records.len(),
                                            t.capacity
                                        ),
                                        false,
                                        false,
                                    );
                                    for (i, r) in t.records.iter().take(18).enumerate() {
                                        push(
                                            &mut out,
                                            format!(
                                                "  {:>2}  {:<6} x{:<3} slot {}  {}",
                                                i,
                                                r.item,
                                                r.count,
                                                r.slot,
                                                item_name_of(r.item)
                                            ),
                                            true,
                                            i == self.entry_row_cursor,
                                        );
                                    }
                                    push(
                                        &mut out,
                                        String::from(
                                            "  Up/Down row  A change item  X write .arc  B back",
                                        ),
                                        false,
                                        false,
                                    );
                                }
                                Err(_) => push(
                                    &mut out,
                                    String::from("  cannot parse"),
                                    false,
                                    false,
                                ),
                            },
                            "sem" => {
                                for (i, c) in e.data.chunks_exact(32).take(8).enumerate() {
                                    let s = SemEntry { raw: c.to_vec() };
                                    let (x, y, z, _) = s.position();
                                    push(
                                        &mut out,
                                        format!(
                                            "  {:>2}  x {:>8.1}  y {:>8.1}  z {:>8.1}  {}",
                                            i,
                                            x,
                                            y,
                                            z,
                                            if s.is_dummy() { "dummy" } else { "real" }
                                        ),
                                        false,
                                        false,
                                    );
                                }
                            }
                            "esl" => match Esl::parse(&e.data) {
                                Ok(l) => push(
                                    &mut out,
                                    format!("  waves {}", l.waves),
                                    false,
                                    false,
                                ),
                                Err(_) => push(
                                    &mut out,
                                    String::from("  cannot parse"),
                                    false,
                                    false,
                                ),
                            },
                            "qdp" => match Qdp::parse(&e.data) {
                                Ok(q) => {
                                    let hex: alloc::string::String =
                                        q.flags().iter().map(|b| alloc::format!("{:02X}", b)).collect();
                                    push(&mut out, format!("  flags {}", hex), false, false);
                                }
                                Err(_) => push(
                                    &mut out,
                                    String::from("  cannot parse"),
                                    false,
                                    false,
                                ),
                            },
                            "qdl" => match QuestLink::parse(&e.data) {
                                Ok(q) => {
                                    for r in q.refs.iter().take(18) {
                                        push(
                                            &mut out,
                                            format!(
                                                "  {:<4} {}",
                                                extension_for(r.type_hash).unwrap_or("?"),
                                                r.name
                                            ),
                                            false,
                                            false,
                                        );
                                    }
                                }
                                Err(_) => push(
                                    &mut out,
                                    String::from("  cannot parse"),
                                    false,
                                    false,
                                ),
                            },
                            "mib" | "gmd" => push(
                                &mut out,
                                String::from("  B, then A, to edit this one"),
                                false,
                                false,
                            ),
                            other => push(
                                &mut out,
                                format!("  .{} has no viewer yet", other),
                                false,
                                false,
                            ),
                        }
                    }
                }
            }
            Screen::ItemPick => {
                let matches = self.item_pick_matches();
                if self.item_pick_mode == 3 {
                    // The bulk add has two stages and the header says which
                    // one is showing, because the list looks the same in both.
                    let head = if self.item_pick_stage == 0 {
                        format!(
                            "ADD ITEM   1) 選ぶ   ({} match)",
                            matches.len()
                        )
                    } else {
                        format!(
                            "ADD ITEM   2) 個数   {} x{}",
                            item_name_of(self.item_pick_add_id),
                            self.item_pick_add_count
                        )
                    };
                    push(&mut out, head, false, false);
                    push(
                        &mut out,
                        format!(
                            "filter: {}   ({} match)",
                            if self.item_pick_filter.is_empty() {
                                "(all)"
                            } else {
                                &self.item_pick_filter
                            },
                            matches.len()
                        ),
                        false,
                        false,
                    );
                    let mut start = self.item_pick_cursor.saturating_sub(8);
                    if start + 14 > matches.len() {
                        start = matches.len().saturating_sub(14);
                    }
                    for (i, id) in matches.iter().enumerate().skip(start).take(14) {
                        push(
                            &mut out,
                            format!("  {:<6} {}", id, item_name_of(*id)),
                            true,
                            i == self.item_pick_cursor,
                        );
                    }
                    push(
                        &mut out,
                        if self.item_pick_stage == 0 {
                            String::from("  A choose  Y filter  X clear  Up/Down  Left/Right +-10  B back")
                        } else {
                            String::from("  Left/Right +-1  Up/Down +-10  A add  B choose again")
                        },
                        false,
                        false,
                    );
                    return out;
                }
                let cur = self.save
                    .as_ref()
                    .and_then(|s| s.read_item_box(self.slot))
                    .and_then(|b| b.get(self.item_pick_target).copied())
                    .unwrap_or_default();
                push(
                    &mut out,
                    format!(
                        "PICK ITEM   box #{}   now: {} ({})",
                        self.item_pick_target + 1,
                        item_name_of(cur.id),
                        cur.id
                    ),
                    false,
                    false,
                );
                push(
                    &mut out,
                    format!(
                        "filter: {}   ({} match)",
                        if self.item_pick_filter.is_empty() {
                            "(all)"
                        } else {
                            &self.item_pick_filter
                        },
                        matches.len()
                    ),
                    false,
                    false,
                );
                let mut start = self.item_pick_cursor.saturating_sub(8);
                if start + 16 > matches.len() {
                    start = matches.len().saturating_sub(16);
                }
                for (i, id) in matches.iter().enumerate().skip(start).take(16) {
                    push(
                        &mut out,
                        format!("  {:<6} {}", id, item_name_of(*id)),
                        true,
                        i == self.item_pick_cursor,
                    );
                }
                push(
                    &mut out,
                    String::from("  A choose  Y type filter  X clear  Up/Down  Left/Right +-10  B back"),
                    false,
                    false,
                );
            }
            Screen::SaveQuests => {
                let list = self.filtered_quests();
                let note = if self.quest_filter.trim().is_empty() {
                    alloc::string::String::new()
                } else {
                    alloc::format!(" of {}   filter: {}", self.quest_titles.len(), self.quest_filter)
                };
                push(
                    &mut out,
                    alloc::format!("QUESTS INSIDE THIS SAVE   {} shown{}", list.len(), note),
                    false,
                    false,
                );
                if list.is_empty() {
                    push(
                        &mut out,
                        String::from("  (none - this save has no download quests)"),
                        false,
                        false,
                    );
                }
                let mut start = self.save_quest_cursor.saturating_sub(8);
                if start + 16 > list.len() {
                    start = list.len().saturating_sub(16);
                }
                for (i, q) in list.iter().enumerate().skip(start).take(16) {
                    // Titles come from the cache filled when the screen opens,
                    // so showing sixteen of them costs nothing per frame.
                    let mut line = format!(
                        "  slot {:>3}  id {:<8} {:>5} B",
                        q.slot, q.quest_id, q.size
                    );
                    let title = self.quest_title(q.quest_id);
                    if !title.is_empty() {
                        line.push_str("  ");
                        line.push_str(title);
                    }
                    push(&mut out, line, true, i == self.save_quest_cursor);
                }
                push(
                    &mut out,
                    String::from("  A edit this quest  - extract this  R extract all  B back"),
                    false,
                    false,
                );
            }
            Screen::ArcList => {
                push(&mut out, format!("QUEST FILES  ({} found)", self.arc_files.len()), false, false);
                push(&mut out, String::from("/switch/mhnxeditor/quest/"), false, false);
                let mut start = self.cursor.saturating_sub(10);
                if start + 20 > self.arc_files.len() {
                    start = self.arc_files.len().saturating_sub(20);
                }
                for (i, f) in self.arc_files.iter().enumerate().skip(start).take(20) {
                    push(&mut out, format!("  {}", f), true, i == self.cursor);
                }
                if self.arc_files.is_empty() {
                    push(&mut out, String::from("  (none)"), false, false);
                }
            }
            Screen::ArcEntries => {
                push(
                    &mut out,
                    format!(
                        "QUEST {}  ({} entries)",
                        self.arc_path.clone().unwrap_or_default(),
                        self.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0)
                    ),
                    false,
                    false,
                );
                if let Some(a) = &self.arc {
                    let mut start = self.cursor.saturating_sub(8);
                    if start + 16 > a.entries.len() {
                        start = a.entries.len().saturating_sub(16);
                    }
                    for (i, e) in a.entries.iter().enumerate().skip(start).take(16) {
                        push(
                            &mut out,
                            format!("  {:<46} {} B", e.file_name(), e.data.len()),
                            true,
                            i == self.cursor,
                        );
                    }
                }
                push(
                    &mut out,
                    String::from("  A edit  X install  - extract this  R extract all  L check  B back"),
                    false,
                    false,
                );
            }
            Screen::Mib => {
                push(&mut out, String::from("クエスト条件 (.mib)"), false, false);
                let fields = questfields::MIB_FIELDS;
                let mut start = self.mib_cursor.saturating_sub(9);
                if start + 18 > fields.len() {
                    start = fields.len().saturating_sub(18);
                }
                for (i, f) in fields.iter().enumerate().skip(start).take(18) {
                    let value = match f.kind {
                        FieldKind::Bytes => {
                            let a = f.offset.min(self.mib_bytes.len());
                            let b = (f.offset + f.size).min(self.mib_bytes.len());
                            self.mib_bytes[a..b]
                                .iter()
                                .map(|x| format!("{:02X}", x))
                                .collect::<Vec<_>>()
                                .join(" ")
                        }
                        _ => {
                            let v = questfields::read_u32(&self.mib_bytes, f.offset, f.kind).unwrap_or(0);
                            match f.enum_name.and_then(questfields::enum_table) {
                                Some(t) => {
                                    format!("{} ({})", t.get(v as usize).copied().unwrap_or("?"), v)
                                }
                                None => format!("{}", v),
                            }
                        }
                    };
                    let key: String = f.key.chars().take(16).collect();
                    push(
                        &mut out,
                        format!("  {:02X} {:<16} {}", f.offset, key, value),
                        f.kind != FieldKind::Bytes,
                        i == self.mib_cursor,
                    );
                }
            }
            Screen::Gmd => {
                push(
                    &mut out,
                    format!(
                        "QUEST TEXT (.gmd)  {}",
                        self.gmd.as_ref().map(|g| g.key.clone()).unwrap_or_default()
                    ),
                    false,
                    false,
                );
                if let Some(g) = &self.gmd {
                    for (i, s) in g.strings.iter().enumerate() {
                        let label = Gmd::LABELS.get(i).copied().unwrap_or("other");
                        push(&mut out, format!("  {}: {}", label, s), true, i == self.gmd_cursor);
                    }
                }
                push(&mut out, String::from("  A edit text  X write .arc  B back"), false, false);
            }
            Screen::Check => {
                push(
                    &mut out,
                    format!("QUEST CHECK  {} finding(s)", self.findings.len()),
                    false,
                    false,
                );
                for (i, f) in self.findings.iter().enumerate() {
                    let tag = match f.severity {
                        Severity::Error => "ERROR",
                        Severity::Warn => "WARN ",
                        Severity::Info => "info ",
                    };
                    push(
                        &mut out,
                        format!("  {} {:<18} {}", tag, f.code, f.message),
                        f.fixable,
                        i == self.check_cursor,
                    );
                    if let Some((entry, off)) = &f.location {
                        let name = entry.rsplit('\\').next().unwrap_or(entry.as_str());
                        push(&mut out, format!("        {} +0x{:X}", name, off), false, false);
                    }
                }
                push(
                    &mut out,
                    String::from("  A fix the selected item   X write .arc   B back"),
                    false,
                    false,
                );
            }
            Screen::Charms => {
                let list = self.charms();
                push(
                    &mut out,
                    format!(
                        "CHARMS slot {}   {} used of {}",
                        self.slot + 1,
                        list.len(),
                        CHARM_COUNT
                    ),
                    false,
                    false,
                );
                if list.is_empty() {
                    push(&mut out, String::from("  (no charms)"), false, false);
                }
                let mut start = self.cursor.saturating_sub(8);
                if start + 16 > list.len() {
                    start = list.len().saturating_sub(16);
                }
                for (i, c) in list.iter().enumerate().skip(start).take(16) {
                    push(
                        &mut out,
                        format!("  #{:<5} {}", c.index, c.summary()),
                        true,
                        i == self.cursor,
                    );
                }
                push(
                    &mut out,
                    String::from("  A delete  Y edit  L new  X write save  B back"),
                    false,
                    false,
                );
            }
            Screen::Hunts => {
                let rows = self.hunt_rows();
                push(
                    &mut out,
                    alloc::format!("HUNT RECORDS   slot {}", self.slot + 1),
                    false,
                    false,
                );
                if rows.is_empty() {
                    push(&mut out, String::from("  (no records)"), false, false);
                }
                let mut start = self.hunt_cursor.saturating_sub(8);
                if start + 18 > rows.len() {
                    start = rows.len().saturating_sub(18);
                }
                // The four numbers are drawn as fields so it is obvious which
                // one Left/Right is about to change.
                let arrow = |f: usize| if self.hunt_field == f { "[" } else { " " };
                let arrow_end = |f: usize| if self.hunt_field == f { "]" } else { " " };
                for (i, (id, slay, cap, lo, hi)) in rows.iter().enumerate().skip(start).take(18) {
                    let size = if *lo == 0 && *hi == 0 {
                        alloc::format!("{}  -{}", arrow(2), arrow_end(3))
                    } else {
                        alloc::format!(
                            "{}{:>3}..{:>3}{}",
                            arrow(2),
                            lo,
                            hi,
                            arrow_end(3)
                        )
                    };
                    push(
                        &mut out,
                        alloc::format!(
                            "  {:>3}  {:<22} {}{:>4}{} {}{:>4}{}  {}",
                            id,
                            monster_name(*id),
                            arrow(0),
                            slay,
                            arrow_end(0),
                            arrow(1),
                            cap,
                            arrow_end(1),
                            size
                        ),
                        true,
                        i == self.hunt_cursor,
                    );
                }
                push(
                    &mut out,
                    alloc::format!(
                        "  変更中: {}   刻み {}",
                        ["討伐数", "捕獲数", "最小サイズ", "最大サイズ"][self.hunt_field.min(3)],
                        HUNT_STEPS[self.step]
                    ),
                    false,
                    false,
                );
            }
            Screen::Guild => {
                push(
                    &mut out,
                    alloc::format!("GUILD CARD   slot {}", self.slot + 1),
                    false,
                    false,
                );
                let Some(save) = &self.save else {
                    push(&mut out, String::from("  no save"), false, false);
                    return out;
                };
                let hr = crate::guildcard::hunter_rank(save, self.slot).unwrap_or(0);
                let time = crate::guildcard::play_time(save, self.slot).unwrap_or(0);
                let comment = crate::guildcard::comment(save, self.slot).unwrap_or_default();
                // The field being edited is bracketed, so Left/Right is
                // never a guess.
                let mk = |f: usize| if self.guild_field == f { "[" } else { " " };
                let cl = |f: usize| if self.guild_field == f { "]" } else { " " };
                push(
                    &mut out,
                    alloc::format!("  rank    : {}{:>3}{}", mk(0), hr, cl(0)),
                    false,
                    false,
                );
                push(
                    &mut out,
                    alloc::format!(
                        "  time    : {}{}:{:02}:{:02}{}  刻み {}",
                        mk(1),
                        time / 3600,
                        (time % 3600) / 60,
                        time % 60,
                        cl(1),
                        STEPS[self.step]
                    ),
                    false,
                    false,
                );
                push(
                    &mut out,
                    alloc::format!("  comment : {}{}{}", mk(2), comment, cl(2)),
                    false,
                    false,
                );
                push(&mut out, String::from("  ARENA"), false, false);
                let log = crate::guildcard::arena(save, self.slot);
                for a in 0..crate::guildcard::ARENA_COUNT {
                    let times: Vec<alloc::string::String> = log
                        .iter()
                        .filter(|e| e.arena == a && !e.is_empty())
                        .map(|e| {
                            let (m, s, cs) = e.as_clock();
                            alloc::format!("{}:{:02}.{:02}", m, s, cs)
                        })
                        .collect();
                    if times.is_empty() {
                        continue;
                    }
                    push(
                        &mut out,
                        alloc::format!("  {:>2} : {}", a, times.join("  ")),
                        false,
                        false,
                    );
                }
                push(&mut out, String::from("  B back"), false, false);
            }
            Screen::Sets => {
                // The editor is a mode of this screen, so it replaces the two
                // lists while it is open and gives them back on B. Nothing
                // below it moves, the same shape the item bulk menu uses.
                if let Some(e) = self.set_edit {
                    push(
                        &mut out,
                        alloc::format!("MY SETS   slot {}   EDIT", self.slot + 1),
                        false,
                        false,
                    );
                    let name = self.editing_set().map(|r| r.name).unwrap_or_default();
                    push(
                        &mut out,
                        alloc::format!("  SET #{:02}  {}", e.set + 1, clip_text(&name, 24)),
                        false,
                        false,
                    );
                    for row in 0..SET_EDIT_ROWS {
                        let (label, value) = if row == SET_ROW_NAME {
                            (String::from("名前"), clip_text(&name, 24))
                        } else {
                            let part = row - 1;
                            let armed = if self.set_clear_armed == Some(part) {
                                "  << もう一度 [-] で確定"
                            } else {
                                ""
                            };
                            (
                                alloc::format!("{}", crate::myset::PART_NAMES[part]),
                                alloc::format!(
                                    "{} 枠 #{:04}{}",
                                    pad_cells(&self.set_part_label(part), 30),
                                    self.set_pick(part),
                                    armed
                                ),
                            )
                        };
                        push(
                            &mut out,
                            alloc::format!("  {:>2} {} {}", row, pad_cells(&label, 6), value),
                            true,
                            row == e.field,
                        );
                    }
                    push(
                        &mut out,
                        alloc::format!("  警告 {} 件 ([X] で読み直す)", self.set_warnings.len()),
                        false,
                        false,
                    );
                    if self.set_warnings.is_empty() {
                        push(&mut out, String::from("    (なし)"), false, false);
                    } else {
                        for line in self.set_warnings.iter().take(4) {
                            push(
                                &mut out,
                                alloc::format!("    {}", clip_text(line, 60)),
                                false,
                                false,
                            );
                        }
                    }
                    push(
                        &mut out,
                        String::from(
                            "  Up/Down 行  Left/Right 装備  A 決定  - 外す×2  X 警告  B 一覧",
                        ),
                        false,
                        false,
                    );
                    return out;
                }
                push(
                    &mut out,
                    alloc::format!("MY SETS   slot {}", self.slot + 1),
                    false,
                    false,
                );
                let Some(save) = &self.save else {
                    return out;
                };
                push(&mut out, String::from(" ITEM SETS"), false, false);
                for set in crate::myset::item_sets(save, self.slot) {
                    if set.is_empty() {
                        continue;
                    }
                    let n = set.used_items().len();
                    push(
                        &mut out,
                        alloc::format!("  [{:>2}] {:<16} {} items", set.index, set.name, n),
                        false,
                        false,
                    );
                }
                push(&mut out, String::from(" EQUIPMENT SETS"), false, false);
                for set in crate::myset::equip_sets(save, self.slot) {
                    if set.is_empty() {
                        continue;
                    }
                    push(
                        &mut out,
                        alloc::format!("  [{:>2}] {}", set.index, set.name),
                        false,
                        false,
                    );
                }
                push(
                    &mut out,
                    String::from("  A 編集(装備セット)   B back"),
                    false,
                    false,
                );
            }
            Screen::Roster => {
                let list = self.palicos();
                push(
                    &mut out,
                    alloc::format!("PALICOS   slot {}   {} owned", self.slot + 1, list.len()),
                    false,
                    false,
                );
                let mut start = self.roster_cursor.saturating_sub(8);
                if start + 18 > list.len() {
                    start = list.len().saturating_sub(18);
                }
                let mark = |f: usize| if self.roster_field == f { "[" } else { " " };
                let close = |f: usize| if self.roster_field == f { "]" } else { " " };
                for (i, p) in list.iter().enumerate().skip(start).take(18) {
                    push(
                        &mut out,
                        alloc::format!(
                            "  {:>3}  {:<14} {}{:>3}{} {}exp {:>7}{}",
                            p.index,
                            p.name,
                            mark(0),
                            p.level,
                            close(0),
                            mark(1),
                            p.exp,
                            close(1)
                        ),
                        true,
                        i == self.roster_cursor,
                    );
                }
                push(
                    &mut out,
                    alloc::format!(
                        "  変更中: {}   A で名前変更",
                        ["レベル", "なつき度", "経験値"][self.roster_field.min(2)]
                    ),
                    false,
                    false,
                );
            }            Screen::QuestEquip => {
                let list = self.quest_equipment();
                let used = list.iter().filter(|e| e.occupied).count();
                push(
                    &mut out,
                    alloc::format!("QUEST EQUIPMENT   {} / {} used", used, list.len()),
                    false,
                    false,
                );
                push(
                    &mut out,
                    String::from("  The first four bytes of each entry are its quest id."),
                    false,
                    false,
                );
                let mut start = self.cursor.saturating_sub(8);
                if start + 16 > list.len() {
                    start = list.len().saturating_sub(16);
                }
                // With a slot opened, the items inside it replace the slot
                // list: the console has room for one level at a time.
                if self.qe_open {
                    let items = self.quest_equip_items();
                    push(
                        &mut out,
                        alloc::format!("  slot {} の中身  ({} 件)", self.cursor, items.len()),
                        false,
                        false,
                    );
                    let mut start = self.qe_item_cursor.saturating_sub(7);
                    if start + 15 > items.len() {
                        start = items.len().saturating_sub(15);
                    }
                    for (i, it) in items.iter().enumerate().skip(start).take(15) {
                        let kind = match it.list {
                            crate::dlc::QuestEquipList::Supply => "支給",
                            crate::dlc::QuestEquipList::Ammo => "弾  ",
                        };
                        push(
                            &mut out,
                            alloc::format!(
                                "  rec{} {}[{:>2}] {:>5} x{:<3} {}",
                                it.record,
                                kind,
                                it.index,
                                it.item,
                                it.count,
                                if it.item == 0 {
                                    alloc::string::String::from("-")
                                } else {
                                    item_name_of(it.item)
                                }
                            ),
                            true,
                            i == self.qe_item_cursor,
                        );
                    }
                    push(
                        &mut out,
                        String::from("  A アイテム変更   B 戻る"),
                        false,
                        false,
                    );
                } else {
                    for (i, e) in list.iter().enumerate().skip(start).take(16) {
                        let id = if e.occupied {
                            alloc::format!("{}", e.quest_id)
                        } else {
                            alloc::string::String::from("(empty)")
                        };
                        push(
                            &mut out,
                            alloc::format!("  slot {:>2}   quest {}", e.index, id),
                            true,
                            i == self.cursor,
                        );
                    }
                }
                push(
                    &mut out,
                    String::from("  Up/Down row   B back"),
                    false,
                    false,
                );
            }
            Screen::Equip => {
                let all = self.equipment();
                push(
                    &mut out,
                    alloc::format!(
                        "EQUIPMENT   slot {}   {} shown{}",
                        self.slot + 1,
                        all.len(),
                        if self.equip_filter.trim().is_empty() {
                            alloc::string::String::new()
                        } else {
                            alloc::format!("   filter: {}", self.equip_filter)
                        }
                    ),
                    false,
                    false,
                );
                if all.is_empty() {
                    push(&mut out, String::from("  (nothing matches)"), false, false);
                }
                let mut start = self.equip_cursor.saturating_sub(8);
                if start + 16 > all.len() {
                    start = all.len().saturating_sub(16);
                }
                for (i, e) in all.iter().enumerate().skip(start).take(16) {
                    push(
                        &mut out,
                        alloc::format!(
                            "  {:>4}  {:<12} Lv{:<3} {}",
                            e.index,
                            crate::equip::type_name(e.kind),
                            e.level,
                            crate::equip::name(e)
                        ),
                        true,
                        i == self.equip_cursor,
                    );
                }
                // What the key guide below cannot say in one line: a delete
                // is two presses, and the first one names what still points at
                // the record.
                if let Some((idx, refs, _)) = self.equip_delete_pending() {
                    push(
                        &mut out,
                        format!(
                            "  削除の確認中: 枠 #{} / 参照 {} 件 / もう一度 [-] で{}",
                            idx,
                            refs,
                            if refs == 0 { "削除" } else { "強制削除" }
                        ),
                        false,
                        false,
                    );
                } else {
                    push(
                        &mut out,
                        String::from("  [-] 2回で削除 / [+] 新規追加 / [A] 編集（種別・IDを変えると差し替え）"),
                        false,
                        false,
                    );
                }
                push(
                    &mut out,
                    String::from("  Up/Down row  A 編集  Y filter  X clear  B back"),
                    false,
                    false,
                );
            }
            Screen::EquipEdit => {
                let e = self.equip_edit.unwrap_or_default();
                if e.new {
                    // Three rows: the three values equipops::create writes.
                    push(&mut out, String::from("新規装備の作成"), false, false);
                    let rows = [
                        (
                            String::from("種別"),
                            format!("{}  {}", e.kind, crate::equip::type_name(e.kind)),
                        ),
                        (String::from("ID"), format!("{}", e.id)),
                        (String::from("レベル"), format!("{}", e.level)),
                    ];
                    for (i, (k, v)) in rows.iter().enumerate() {
                        push(&mut out, format!("  {:<14} {}", k, v), true, i == e.field);
                    }
                    push(
                        &mut out,
                        String::from("  Up/Down row  Left/Right value  A 作成  B 戻る"),
                        false,
                        false,
                    );
                } else {
                    push(&mut out, String::from("装備の編集"), false, false);
                    // Nine rows, the same shape as the talisman form, so the
                    // highlight and the value it edits stay on the same row.
                    // The type and the id sit at the bottom: moving them is
                    // what makes A replace the record.
                    let rows = [
                        (
                            String::from("枠"),
                            format!("{}  （{}）", e.index, crate::equip::type_name(e.kind)),
                        ),
                        (String::from("レベル"), format!("{}", e.level)),
                        (String::from("着せ替えID"), format!("{}", e.transmog_id)),
                        (
                            String::from("着せ替え元Lv"),
                            format!("{}", e.transmog_level),
                        ),
                        (
                            String::from("装飾品1"),
                            format!(
                                "{}  {}",
                                e.decorations[0],
                                decoration_name_of(e.decorations[0])
                            ),
                        ),
                        (
                            String::from("装飾品2"),
                            format!(
                                "{}  {}",
                                e.decorations[1],
                                decoration_name_of(e.decorations[1])
                            ),
                        ),
                        (
                            String::from("装飾品3"),
                            format!(
                                "{}  {}",
                                e.decorations[2],
                                decoration_name_of(e.decorations[2])
                            ),
                        ),
                        (
                            String::from("種別"),
                            format!("{}  {}", e.kind, crate::equip::type_name(e.kind)),
                        ),
                        (String::from("ID"), format!("{}", e.id)),
                    ];
                    for (i, (k, v)) in rows.iter().enumerate() {
                        push(&mut out, format!("  {:<14} {}", k, v), true, i == e.field);
                    }
                    push(
                        &mut out,
                        String::from("  Up/Down row  Left/Right value  Y 装飾品  X 刻み  A 反映/差し替え  B 戻る"),
                        false,
                        false,
                    );
                }
            }
            Screen::JewelPick => {
                let matches = self.jewel_pick_matches();
                push(
                    &mut out,
                    alloc::format!(
                        "PICK DECORATION   filter: {}   ({} match)",
                        if self.jewel_pick_filter.is_empty() {
                            "(all)"
                        } else {
                            &self.jewel_pick_filter
                        },
                        matches.len()
                    ),
                    false,
                    false,
                );
                let mut start = self.jewel_pick_cursor.saturating_sub(8);
                if start + 16 > matches.len() {
                    start = matches.len().saturating_sub(16);
                }
                for (i, id) in matches.iter().enumerate().skip(start).take(16) {
                    push(
                        &mut out,
                        alloc::format!("  {:<5} {}", id, decoration_name_of(*id)),
                        true,
                        i == self.jewel_pick_cursor,
                    );
                }
                push(
                    &mut out,
                    String::from(
                        "  A choose  Y type filter  X clear  Up/Down  Left/Right +-10  B back",
                    ),
                    false,
                    false,
                );
            }
            Screen::Convert => {
                push(&mut out, String::from("セーブ形式の変換"), false, false);
                match (&self.save, &self.save_path) {
                    (None, _) => push(&mut out, String::from("  no save open"), false, false),
                    (Some(s), p) => {
                        let kind = match s.kind() {
                            crate::save::SaveKind::ThreeDs => "3DS MHXX (no header)",
                            crate::save::SaveKind::Switch => "Switch MHXX (36 byte header)",
                            crate::save::SaveKind::Mhgu => "Switch MHGU",
                        };
                        push(&mut out, alloc::format!("  source : {}", kind), false, false);
                        let target = if s.kind() == crate::save::SaveKind::ThreeDs {
                            "Switch MHXX (header synthesised)"
                        } else {
                            "3DS MHXX (header stripped)"
                        };
                        push(&mut out, alloc::format!("  target : {}", target), false, false);
                        let ext = if s.kind() == crate::save::SaveKind::ThreeDs {
                            "switch"
                        } else {
                            "3ds"
                        };
                        let stem = if self.convert_name.trim().is_empty() {
                            alloc::string::String::from("system")
                        } else {
                            self.convert_name.trim().to_string()
                        };
                        let path = p
                            .clone()
                            .map(|v| alloc::format!("{}{}.{}", split_dir(&v), stem, ext))
                            .unwrap_or_else(|| String::from("(no path)"));
                        push(&mut out, alloc::format!("  writes : {}", path), false, false);
                        push(
                            &mut out,
                            alloc::format!("  name   : {}  (Y changes it)", stem),
                            false,
                            false,
                        );
                    }
                }
                push(
                    &mut out,
                    String::from("  A convert and write   Y name   B back"),
                    false,
                    false,
                );
            }
            Screen::SkillPick => {
                let matches = self.skill_pick_matches();
                push(
                    &mut out,
                    alloc::format!(
                        "PICK SKILL {}   filter: {}   ({} match)",
                        self.skill_pick_field,
                        if self.skill_pick_filter.is_empty() {
                            "(all)"
                        } else {
                            &self.skill_pick_filter
                        },
                        matches.len()
                    ),
                    false,
                    false,
                );
                let mut start = self.skill_pick_cursor.saturating_sub(8);
                if start + 16 > matches.len() {
                    start = matches.len().saturating_sub(16);
                }
                for (i, id) in matches.iter().enumerate().skip(start).take(16) {
                    push(
                        &mut out,
                        alloc::format!(
                            "  {:<4} {}",
                            id,
                            crate::skills::skill_name(*id).unwrap_or("?")
                        ),
                        true,
                        i == self.skill_pick_cursor,
                    );
                }
                push(
                    &mut out,
                    String::from("  A choose  Y type  X clear  Up/Down  Left/Right +-10  B back"),
                    false,
                    false,
                );
            }
            Screen::CharmEdit => {
                push(&mut out, String::from("お守りの編集"), false, false);
                let e = self.charm_edit.unwrap_or_default();
                // Seven rows, matching the state machine and the console's
                // drawn form. This text view was left at six when the grade
                // row was added, so the highlight sat one row above the value
                // it was editing.
                let rows: [(alloc::string::String, alloc::string::String); 7] = [
                    (String::from("枠"), format!("{}", e.index)),
                    (
                        String::from("レア度"),
                        format!("{}（最大 S{}）", e.grade.name(), e.grade.max_slots()),
                    ),
                    (String::from("装飾スロット"), format!("{}", e.slots)),
                    (
                        String::from("スキル1 ID"),
                        format!(
                            "{}  ({})",
                            e.skill1,
                            crate::skills::skill_name(e.skill1).unwrap_or("none")
                        ),
                    ),
                    (String::from("スキル1 ポイント"), format!("{:+}", e.points1)),
                    (
                        String::from("スキル2 ID"),
                        format!(
                            "{}  ({})",
                            e.skill2,
                            crate::skills::skill_name(e.skill2).unwrap_or("none")
                        ),
                    ),
                    (String::from("スキル2 ポイント"), format!("{:+}", e.points2)),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    push(
                        &mut out,
                        format!("  {:<16} {}", k, v),
                        true,
                        i == e.field,
                    );
                }
                // The published per-rarity range, same check the PC build
                // shows: it is the difference between a talisman the game
                // accepts and one it quietly rejects.
                #[cfg(feature = "gamedata")]
                {
                    let ty = self
                        .save
                        .as_ref()
                        .and_then(|s| s.equipment_box(self.slot))
                        .and_then(|b| {
                            let o = e.index * crate::charm::CHARM_RECORD + 2;
                            b.get(o).copied()
                        })
                        .unwrap_or(0);
                    if let Some(r) = crate::charm::rarity_of(ty) {
                        push(
                            &mut out,
                            format!("  type 0x{:02X}  rarity {}", ty, r),
                            false,
                            false,
                        );
                    }
                    // The form's own grade, not the stored record's: a skill
                    // picked with Y is not in the box until A is pressed.
                    push(
                        &mut out,
                        format!(
                            "  編集中のレア度 {} / スロット上限 {}",
                            e.grade.name(),
                            e.grade.max_slots()
                        ),
                        false,
                        false,
                    );
                    for (label, sid, pts, second) in [
                        ("skill 1", e.skill1, e.points1, false),
                        ("skill 2", e.skill2, e.points2, true),
                    ] {
                        if sid == 0 {
                            continue;
                        }
                        // From the form's grade, so the range shown matches
                        // what the console form is about to write.
                        let Some(r) =
                            crate::charm::allowed_points_rarity(e.grade.rarity(), sid, second)
                        else {
                            continue;
                        };
                        let name = crate::skills::skill_name(sid).unwrap_or("?");
                        let line = if r.min == 0 && r.max == 0 {
                            format!("  !! {} {} cannot go on this type", label, name)
                        } else if pts < r.min || pts > r.max {
                            format!(
                                "  !! {} {} is {:+}, allowed {:+}..{:+}",
                                label, name, pts, r.min, r.max
                            )
                        } else {
                            format!(
                                "     {} {} allowed {:+}..{:+}",
                                label, name, r.min, r.max
                            )
                        };
                        push(&mut out, line, false, false);
                    }
                }
                push(
                    &mut out,
                    String::from("  Up/Down row  Left/Right value  Y pick skill  X step  A write"),
                    false,
                    false,
                );
            }
            Screen::Palico => {
                let lang = if self.palico_lang == 0 { Lang::Jp } else { Lang::En };
                let installed: Vec<u8> = self
                    .save
                    .as_ref()
                    .map(|s| s.dlc_palicoes().iter().map(|c| c.id).collect())
                    .unwrap_or_default();
                let ids = dlcats::all_ids();
                push(
                    &mut out,
                    format!(
                        "DOWNLOAD PALICO   {}   {}/{} installed",
                        if self.palico_lang == 0 { "JP" } else { "EN" },
                        installed.iter().filter(|i| **i != 0).count(),
                        ids.len()
                    ),
                    false,
                    false,
                );
                let mut start = self.palico_cursor.saturating_sub(8);
                if start + 16 > ids.len() {
                    start = ids.len().saturating_sub(16);
                }
                for (i, id) in ids.iter().enumerate().skip(start).take(16) {
                    let name = dlcats::record(lang, *id)
                        .map(dlcats::record_name)
                        .unwrap_or_default();
                    let mark = if installed.contains(id) { "+" } else { " " };
                    push(
                        &mut out,
                        format!("  {} id 0x{:02X}  {}", mark, id, name),
                        true,
                        i == self.palico_cursor,
                    );
                }
                push(
                    &mut out,
                    String::from("  A add/remove   L R language   X write save   B back"),
                    false,
                    false,
                );
            }
            Screen::Options => {
                push(&mut out, String::from("設定"), false, false);
                let o = self
                    .save
                    .as_ref()
                    .map(|s| s.options())
                    .unwrap_or_else(Options::default);
                let rows = [
                    (String::from("明るさ"), format!("{}", o.brightness)),
                    (
                        String::from("振動"),
                        String::from(if o.rumble != 0 { "on" } else { "off" }),
                    ),
                    (
                        String::from("3D を無効化"),
                        String::from(if o.disable_3d != 0 { "yes" } else { "no" }),
                    ),
                    (
                        String::from("スライドパッドプロ"),
                        String::from(if o.circle_pad_pro != 0 { "on" } else { "off" }),
                    ),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    push(
                        &mut out,
                        format!("  {:<16} {}", k, v),
                        true,
                        i == self.option_cursor,
                    );
                }
                push(
                    &mut out,
                    String::from("  Up/Down row  Left/Right value  A write save  B back"),
                    false,
                    false,
                );
            }
        }
        out
    }
}