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
use crate::save::Options;
use crate::itembox::ItemSlot;
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
    /// The 45 quest-equipment entries the DLC section carries.
    QuestEquip,
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

/// A text edit request (software keyboard).
///
/// index == usize::MAX means the character name of the current slot.
pub type TextRequest = (usize, String);

/// Sentinel id for "ask the player's name".
pub const TEXT_NAME: usize = usize::MAX;

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
/// Sentinel id for "ask for an item name to filter the picker by".
pub const TEXT_ITEM_FILTER: usize = usize::MAX - 1;
/// Sentinel id for "ask for a skill name to filter the picker by".
pub const TEXT_SKILL_FILTER: usize = usize::MAX - 2;
/// Sentinel id for "ask for a quest id or title to filter the list by".
pub const TEXT_QUEST_FILTER: usize = usize::MAX - 3;
/// Sentinel id for "ask what to call the converted save".
pub const TEXT_SAVE_NAME: usize = usize::MAX - 4;
/// Sentinel id for "ask which equipment to show".
pub const TEXT_EQUIP_FILTER: usize = usize::MAX - 5;

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
    /// Equipment list filter and cursor.
    pub equip_filter: String,
    /// Cursor over the filtered equipment list.
    pub equip_cursor: usize,
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
    mib_bytes: Vec<u8>,
    /// Working copy of the .gmd strings.
    gmd: Option<Gmd>,
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
            equip_filter: String::new(),
            equip_cursor: 0,
            item_pick_mode: 0,
            entry_item_row: 0,
            entry_row_cursor: 0,
            quest_filter: String::new(),
            skill_pick_filter: String::new(),
            skill_pick_cursor: 0,
            item_pick_filter: String::new(),
            item_pick_cursor: 0,
            item_pick_target: 0,
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
                self.message = String::from("text updated - press X to write the .arc");
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
                self.message = String::from("name updated - press A to write the save");
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
    /// The Switch layer uses Minus to rescan folders everywhere else.
    pub fn uses_minus(&self) -> bool {
        matches!(self.screen, Screen::ArcEntries | Screen::SaveQuests)
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

    fn character(&self) -> Option<Character> {
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
            Screen::QuestEquip => self.input_quest_equip(key),
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
                            "record {} - pick a new item",
                            self.entry_row_cursor
                        );
                    } else {
                        self.message = String::from("this entry has no editable records");
                    }
                }
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
            self.message = String::from("no charms yet - press Y to create one");
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
        self.charm_edit = Some(CharmEdit {
            index: seed.index,
            slots: seed.slots,
            skill1: seed.skill1,
            points1: seed.points1,
            skill2: seed.skill2,
            points2: seed.points2,
            field: 2,
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
            Key::Up => e.field = (e.field + 5) % 6,
            Key::Down => e.field = (e.field + 1) % 6,
            Key::Left | Key::Right => {
                let d: i64 = if key == Key::Right { 1 } else { -1 } * STEPS[self.step].min(10).max(1);
                match e.field {
                    0 => e.index = ((e.index as i64 + d).rem_euclid(CHARM_COUNT as i64)) as usize,
                    1 => e.slots = ((e.slots as i64 + d).rem_euclid(4)) as u8,
                    2 => e.skill1 = ((e.skill1 as i64 + d).rem_euclid(256)) as u8,
                    3 => e.points1 = (e.points1 as i64 + d).clamp(-127, 127) as i8,
                    4 => e.skill2 = ((e.skill2 as i64 + d).rem_euclid(256)) as u8,
                    5 => e.points2 = (e.points2 as i64 + d).clamp(-127, 127) as i8,
                    _ => {}
                }
            }
            // Y on a skill row opens the name picker: typing a number is
            // hopeless when there are 306 skills.
            Key::Y => {
                if e.field == 2 || e.field == 4 {
                    self.skill_pick_field = if e.field == 2 { 1 } else { 2 };
                    self.skill_pick_filter.clear();
                    self.skill_pick_cursor = 0;
                    self.charm_edit = Some(e);
                    self.screen = Screen::SkillPick;
                    self.message = String::from("pick a skill by name");
                } else {
                    self.message = String::from("move to a skill row first");
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
                    if s.set_charm(self.slot, &c) {
                        self.dirty = true;
                        self.message = format!(
                            "wrote charm {} - it copies a real talisman's rarity and unknown bytes",
                            e.index
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
                    String::from("Japanese Palico names")
                } else {
                    String::from("English Palico names")
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
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
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
                    self.message = String::from("this finding cannot be fixed automatically");
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
                        None => self.message = String::from("no .mib entry"),
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
                self.message = String::from("writing save...");
            }
        }
    }

    fn request_write_arc(&mut self) {
        self.store_entry();
        // A quest pulled out of the save has no path; A used to do nothing at
        // all here, which read as a broken button.
        if self.arc_path.is_none() {
            self.message = if self.save.is_some() {
                String::from("press X on the entry list to write this quest back into the save")
            } else {
                String::from("open a save, or use Y to save this .arc to a file")
            };
            return;
        }
        if let Some(p) = self.arc_path.clone() {
            if let Some(a) = &self.arc {
                match a.to_bytes() {
                    Ok(bytes) => {
                        self.pending_write = Some(Write::File { path: p, bytes });
                        self.message = String::from("writing .arc...");
                    }
                    Err(_) => self.message = String::from("repack failed"),
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
                self.message = String::from("quest editor");
            }
            Key::X => {
                self.screen = Screen::Options;
                self.cursor = 0;
                self.message = String::from("options");
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
                self.message = String::from("L/R switches the name language");
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
                self.message = String::from("quest editor");
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
            Key::L | Key::R => self.slot = (self.slot + 1) % 3,
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
                        items[idx].count = c.clamp(0, 127) as u8;
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
                    self.message = format!("box #{} - pick a new item", idx + 1);
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

    /// Item ids whose name matches the current filter, in id order.
    pub fn item_pick_matches(&self) -> Vec<u16> {
        let needle = self.item_pick_filter.trim().to_lowercase();
        #[cfg(feature = "gamedata")]
        {
            let mut out = Vec::new();
            for (id, name) in crate::gamedata::ITEM_NAME_LIST.iter().enumerate() {
                if id == 0 || id > 4095 {
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
            self.message = String::from("this table cannot be parsed");
            return;
        };
        if row >= t.records.len() {
            self.message = String::from("no such record");
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
            self.message = String::from("nothing to undo");
            return;
        };
        let slots = crate::itembox::decode(&raw);
        let slot = self.slot;
        if let Some(s) = &mut self.save {
            if s.write_item_box(slot, &slots).is_ok() {
                self.dirty = true;
                self.message = String::from("item box restored");
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
            self.message = String::from("open a save first");
            return;
        };
        if save.kind() == crate::save::SaveKind::Mhgu {
            self.message = String::from("MHGU is not converted here");
            return;
        }
        let Some(base) = self.save_path.clone() else {
            self.message = String::from("this save has no path");
            return;
        };
        let dir = split_dir(&base);
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
                        self.message = String::from("cannot build a Switch container");
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

    fn input_quest_equip(&mut self, key: Key) {
        let n = self.quest_equipment().len();
        match key {
            Key::Up => self.cursor = self.cursor.saturating_sub(1),
            Key::Down => {
                if n > 0 {
                    self.cursor = (self.cursor + 1).min(n - 1);
                }
            }
            Key::B => {
                self.screen = Screen::SaveQuests;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    fn input_equip(&mut self, key: Key) {
        let n = self.equipment().len();
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
            Key::X => {
                self.equip_filter.clear();
                self.equip_cursor = 0;
            }
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
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
                } else {
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
                    self.message = String::from("open a save first");
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
                self.message = String::from("save editor");
            }
            Key::Plus => self.quit = true,
            _ => {}
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
        self.quest_titles
            .iter()
            .find(|(q, _)| *q == id)
            .map(|(_, t)| t.as_str())
            .unwrap_or("")
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
                    _ => self.message = String::from("this entry type cannot be edited"),
                }
            }
            // Y shows the entry decoded, whatever its type. The six container
            // formats used to be unreadable on the console.
            Key::Y => {
                // The view renders self.entry, so the cursor has to be copied
                // across or every entry shows whichever one was opened last.
                self.entry = self.cursor;
                self.screen = Screen::EntryView;
                self.message = String::from("B goes back");
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
                    self.message = String::from("cannot read the quest id");
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
                    self.message = String::from("raw byte field - not editable here");
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
            self.message = String::from("open a quest first");
            return;
        };
        let id = a
            .by_ext("mib")
            .and_then(|e| Mib::from_bytes(&e.data).ok())
            .map(|m| m.quest_id())
            .unwrap_or(0);
        if id == 0 {
            self.message = String::from("cannot read the quest id");
            return;
        }
        let raw = match a.to_bytes() {
            Ok(b) => b,
            Err(_) => {
                self.message = String::from("repack failed");
                return;
            }
        };
        let Some(s) = &mut self.save else {
            self.message = String::from("open a save first");
            return;
        };
        match dlc::install_quest(s, id as i32, &raw) {
            Ok(slot) => {
                self.dirty = true;
                self.message = format!("installed quest {} into slot {}", id, slot);
            }
            Err(e) => self.message = format!("install failed: {}", e),
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
                push(&mut out, String::from("/switch/mhnxeditor/saves/"), false, false);
                for (i, f) in self.save_files.iter().enumerate() {
                    push(&mut out, format!("  {}", f), true, i == self.cursor);
                }
                if self.save_files.is_empty() {
                    push(&mut out, String::from("  (none)"), false, false);
                }
            }
            Screen::Slots => {
                push(&mut out, String::from("CHARACTER SLOT"), false, false);
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
            }
            Screen::EntryView => {
                use crate::entries::{Esl, ItemTable, Qdp, QuestLink, SemEntry};
                let entry = self
                    .arc
                    .as_ref()
                    .and_then(|a| a.entries.get(self.entry));
                match entry {
                    None => push(&mut out, String::from("no entry"), false, false),
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
                push(&mut out, String::from("QUEST PARAMETERS (.mib)"), false, false);
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
            Screen::QuestEquip => {
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
                push(
                    &mut out,
                    String::from("  Up/Down row  Y filter  X clear  B back"),
                    false,
                    false,
                );
            }
            Screen::Convert => {
                push(&mut out, String::from("CONVERT SAVE"), false, false);
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
                push(&mut out, String::from("CHARM EDIT"), false, false);
                let e = self.charm_edit.unwrap_or_default();
                let rows: [(alloc::string::String, alloc::string::String); 6] = [
                    (String::from("slot #"), format!("{}", e.index)),
                    (String::from("deco slots"), format!("{}", e.slots)),
                    (
                        String::from("skill 1 id"),
                        format!(
                            "{}  ({})",
                            e.skill1,
                            crate::skills::skill_name(e.skill1).unwrap_or("none")
                        ),
                    ),
                    (String::from("skill 1 points"), format!("{:+}", e.points1)),
                    (
                        String::from("skill 2 id"),
                        format!(
                            "{}  ({})",
                            e.skill2,
                            crate::skills::skill_name(e.skill2).unwrap_or("none")
                        ),
                    ),
                    (String::from("skill 2 points"), format!("{:+}", e.points2)),
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
                    for (label, sid, pts, second) in [
                        ("skill 1", e.skill1, e.points1, false),
                        ("skill 2", e.skill2, e.points2, true),
                    ] {
                        if sid == 0 {
                            continue;
                        }
                        let Some(r) = crate::charm::allowed_points(ty, sid, second) else {
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
                push(&mut out, String::from("OPTIONS"), false, false);
                let o = self
                    .save
                    .as_ref()
                    .map(|s| s.options())
                    .unwrap_or_else(Options::default);
                let rows = [
                    (String::from("brightness"), format!("{}", o.brightness)),
                    (
                        String::from("rumble"),
                        String::from(if o.rumble != 0 { "on" } else { "off" }),
                    ),
                    (
                        String::from("disable 3D"),
                        String::from(if o.disable_3d != 0 { "yes" } else { "no" }),
                    ),
                    (
                        String::from("circle pad pro"),
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