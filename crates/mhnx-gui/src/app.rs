//! Application state and UI.

use std::path::PathBuf;

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use mhnx_core::arc::{extension_for, Arc};
use mhnx_core::awards;
use mhnx_core::check::{self, Finding, Severity};
use mhnx_core::csvout::{self, ApplyReport, EquipmentRow};
use mhnx_core::dlc;
use mhnx_core::equipops::{self, EquipError};
use mhnx_core::equiprefs::EquipRef;
use mhnx_core::gamedata::LAST_ITEM_ID;
use mhnx_core::itembox::{self, DedupePreview, ItemSlot, MAX_COUNT};
use mhnx_core::quest::{Gmd, Mib};
use mhnx_core::questfields::FieldKind;
use mhnx_core::questprogress::{self, Progress};
use mhnx_core::requests;
use mhnx_core::romfstables::{self, RequestRecord};
use mhnx_core::save::{Character, Save, SaveKind, BODY_SIZE, SWITCH_SIZE};
use mhnx_core::snapshot;
use mhnx_core::verify;

/// Which of the two boxes a CSV file belongs to.
///
/// The two files are not interchangeable: the item file lists all 2300 records
/// and the equipment file only the used ones, and the parsers read different
/// columns. Keeping the kind in one place is what lets the dialog, the writer
/// and the reader say which box they mean without repeating the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvKind {
    /// The 2300 record item box.
    Items,
    /// The 2000 record equipment box.
    Equipment,
}

impl CsvKind {
    /// What the interface calls this file.
    pub fn label(self) -> &'static str {
        match self {
            CsvKind::Items => "アイテムボックスの CSV",
            CsvKind::Equipment => "装備ボックスの CSV",
        }
    }

    /// The part of the file name the export dialog starts from.
    pub fn stem(self) -> &'static str {
        match self {
            CsvKind::Items => "items",
            CsvKind::Equipment => "equip",
        }
    }

    /// Which box the file describes, for the confirmation line.
    pub fn target(self, slot: usize) -> String {
        match self {
            CsvKind::Items => format!("枠 {} のアイテムボックス", slot + 1),
            CsvKind::Equipment => format!("枠 {} の装備ボックス", slot + 1),
        }
    }
}

/// What a parsed CSV file holds, in file order.
///
/// The two shapes are kept apart rather than behind one boxed trait object: the
/// apply functions take a slice of their own row type, and a file that was
/// parsed as the wrong one would otherwise reach a writer that cannot read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvRows {
    /// One (id, count) pair per record, from csvout::parse_items.
    Items(Vec<(u16, u16)>),
    /// One decoded record per row, from csvout::parse_equipment.
    Equipment(Vec<EquipmentRow>),
}

impl CsvRows {
    /// How many rows the file holds.
    pub fn len(&self) -> usize {
        match self {
            CsvRows::Items(rows) => rows.len(),
            CsvRows::Equipment(rows) => rows.len(),
        }
    }

    /// Whether the file held no row at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A CSV file that was read and is waiting for the user to agree to it.
///
/// The rows are applied to a copy of the save the moment the file is read, and
/// the report of that dry run is what the dialog shows. Nothing of the file
/// reaches the open save until the dialog is answered; see csv_commit.
pub struct CsvPending {
    /// Which box the file describes.
    pub kind: CsvKind,
    /// Where it was read from, for the dialog and the status line.
    pub path: PathBuf,
    /// The rows, in file order.
    pub rows: CsvRows,
    /// What applying them to a copy of the save reported.
    pub report: ApplyReport,
}

/// Which page is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Save,
    Items,
    Quest,
    Dlc,
    Charm,
    Equip,
    Hunts,
    Guild,
    /// 勲章・依頼・クエストの進行状況.
    Progress,
    Palico,
    Sets,
    Extras,
    About,
}

/// The whole application.
/// Destructive actions that have to be confirmed before they happen.
#[derive(Default, PartialEq)]
pub enum Confirm {
    /// Nothing is waiting.
    #[default]
    None,
    /// Quit with unwritten edits.
    Quit,
    /// Open this save, dropping the unwritten edits.
    DiscardForOpen(PathBuf),
    /// Empty the DLC section.
    ClearDlc,
    /// Delete one charm.
    Charm(usize),
    /// Clear one equipment record. The first ask, before the reference check
    /// has run: the dialog cannot say yet whether anything points at it.
    EquipClear(usize),
    /// Clear it although these still name it. The second ask, shown only after
    /// clear() came back with the references it refused to break.
    EquipClearReferenced {
        /// Box index of the record.
        index: usize,
        /// Every place that still names it.
        refs: Vec<EquipRef>,
    },
    /// Put a different item in one record, emptying its decorations.
    EquipReplace {
        /// Box index of the record.
        index: usize,
        /// New type.
        kind: u8,
        /// New item id.
        id: u16,
        /// Level to write.
        level: u16,
    },
    /// Let a request's 完了 flag go. The dialog asks whether 受理 goes with it:
    /// the pair is consecutive flags, and clearing only one of them leaves a
    /// state the game never writes on its own.
    RequestIncomplete {
        /// Position in the activityData record table.
        record: usize,
    },
    /// Raise every used record to the cap. The count is what the box measured
    /// when the button was pressed, so the dialog can say what it will touch.
    ItemFillAll {
        /// Records whose count will change.
        changed: usize,
    },
    /// Merge duplicate stacks. The numbers come from dedupe_preview, measured
    /// before the dialog is shown: a merge that takes items with it has to say
    /// so before it runs.
    ItemDedupe {
        /// Records that will be freed.
        freed: usize,
        /// Items dropped because a stack stops at the cap.
        lost: u32,
    },
    /// Move the used records to the front, which reorders them.
    ItemCompact {
        /// Records that will change place.
        moved: usize,
    },
    /// Pull stacks above the cap back down.
    ItemClamp {
        /// Records whose count will change.
        changed: usize,
    },
    /// Empty one saved equipment set. The contents are gone after this, and
    /// nothing below asks a second time: this dialog is the ask.
    SetClear(usize),
    /// Write a parsed CSV file into a box, after the dialog showed what the
    /// file would do. The rows themselves wait in
    /// [MhnxEditor::csv_pending]: a whole item box is 2300 of them and the
    /// dialog only needs the numbers.
    CsvApply,
    /// Copy the pouch into this item set, replacing what the set holds.
    SetFromPouch(usize),
    /// Replace the pouch with this item set. Nothing of the pouch the
    /// character carries now survives except the copy kept for 元に戻す.
    SetToPouch(usize),
}

/// Two-step confirmation for the destructive charm delete.
#[derive(Default)]
pub struct CharmConfirm {
    /// The charm waiting for a second click.
    pub delete: Option<usize>,
}

/// One file in the `snapshots` folder next to the open save.
///
/// The list is read from disk rather than rebuilt every frame: the folder can
/// hold ten 4.7 MB files and a frame loop should not stat them sixty times a
/// second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotEntry {
    /// File name inside `snapshots/`, e.g. `system-20261104-012812.sav`.
    pub name: String,
    /// Size on disk, for the list.
    pub size: u64,
}

/// What the 進行状況 tab is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProgressSection {
    /// The 132 guild card awards.
    #[default]
    Awards,
    /// The 184 village requests.
    Requests,
    /// Quest progress, indices 1..=1508.
    Quests,
}

impl ProgressSection {
    /// The label on the switcher.
    pub fn label(self) -> &'static str {
        match self {
            ProgressSection::Awards => "勲章",
            ProgressSection::Requests => "依頼",
            ProgressSection::Quests => "クエスト",
        }
    }
}

/// One step of the 進行状況 undo stack.
///
/// All three sections change bits inside fixed, known ranges of a character
/// slot, so a step is kept as the bytes those ranges held before the change.
/// Putting them back restores the exact bits, including the ones next to the
/// bit that was clicked, which a single toggle must not touch.
pub struct ProgressUndo {
    /// What the step changed, for the status line and the tooltip.
    pub label: String,
    /// Character slot the bytes came from.
    pub slot: usize,
    /// Offset relative to the slot base, and the bytes that were there.
    pub regions: Vec<(usize, Vec<u8>)>,
}

/// The three tables of a romfs folder, as found on disk.
///
/// The paths are kept before they are read so the report can name the folder
/// that answered: a user with two dumps needs to know which one is in use.
struct RomfsTables {
    /// Folder the files were found in.
    dir: PathBuf,
    /// `GC_Medal*.gmd`, the 132 award names (and their descriptions).
    award: Option<PathBuf>,
    /// `activityData*.gmd`, the request names.
    names: Option<PathBuf>,
    /// `activityData.atd`, the request records with their two flags.
    atd: Option<PathBuf>,
}

/// What one 読み込む found, before it is copied into the editor.
struct RomfsLoad {
    /// Folder the tables came from.
    dir: PathBuf,
    /// The line under the path field.
    report: String,
    /// 132 names, empty when the medal table could not be read.
    award_names: Vec<String>,
    /// 184 names, empty when the activity table could not be read.
    request_names: Vec<String>,
    /// 184 records, empty when the `.atd` could not be read.
    request_records: Vec<RequestRecord>,
}

pub struct MhnxEditor {
    pub tab: Tab,
    pub status: String,
    pub message: String,

    // ---- save ----
    pub save_path: Option<PathBuf>,
    pub save: Option<Save>,
    pub dirty: bool,
    pub slot: usize,
    pub edit: Character,
    pub items: Vec<ItemSlot>,
    pub item_filter: String,
    pub item_selected: Option<usize>,
    /// Which charm the delete button is waiting for a second click on.
    pub charm_confirm: CharmConfirm,
    /// Row of the item table to scroll into view once.
    pub scroll_to_item: Option<usize>,
    /// Grade a new talisman is created at, and the one the selector last hit.
    pub charm_new_grade: mhnx_core::charm::TalismanGrade,
    /// Quest-equipment slot whose supply and ammo lists are on screen.
    pub qe_slot: Option<usize>,
    /// Supply or ammo entry the item picker is choosing for:
    /// (slot, 0 supply / 1 ammo, record, entry). The record matters: the five
    /// loadouts all have an entry 0, so without it the picker wrote to the
    /// first loadout whichever row was clicked.
    pub qe_pick: Option<(usize, u8, usize, usize)>,
    /// A destructive action waiting for the user to say yes.
    pub confirm: Confirm,
    /// Set when the user has just agreed to drop unsaved edits.
    pub discard_ok: bool,
    /// The download quest slot waiting for the delete confirmation.
    pub pending_dlc_slot: Option<usize>,
    pub item_show_empty: bool,
    /// Whether the bulk-add strip under the item buttons is open.
    pub item_bulk_open: bool,
    /// Search text of the bulk-add picker.
    pub item_bulk_filter: String,
    /// Item the bulk-add strip will insert, 0 = nothing picked yet.
    pub item_bulk_id: u16,
    /// How many of it the bulk-add strip will insert.
    pub item_bulk_count: u16,

    // ---- quest ----
    pub arc_path: Option<PathBuf>,
    pub arc: Option<Arc>,
    pub arc_selected: usize,
    pub arc_filter: String,
    /// Quest id the open archive was pulled out of the save with, if any.
    ///
    /// When this is set the quest tab can write straight back into the save,
    /// so the whole edit loop happens without touching a file on disk.
    pub arc_from_save: Option<i32>,
    pub mib: Option<Mib>,
    /// Editable copy of the 249 MIB bytes.
    pub mib_bytes: Vec<u8>,
    pub gmd: Option<Gmd>,
    /// Problems found by the quest checker.
    pub findings: Vec<Finding>,
    /// Download Palico installer: 0 = Japanese, 1 = English.
    pub cat_lang: usize,
    /// Download Palico installer: index into the id list.
    pub cat_id: usize,
    /// Free-text filter used by the item picker.
    pub item_pick_filter: String,
    /// Free-text filter for the installed quest list (id or title).
    pub dlc_filter: String,
    /// Output folder for the quest extractor.
    pub dlc_outdir: String,
    /// Snapshots for 元に戻す, newest last. Bounded so a long session cannot
    /// grow without limit; one item box is 2242 entries.
    pub item_undo: Vec<Vec<ItemSlot>>,
    /// Keep the picker filter after choosing, so several slots can be filled
    /// from one search.
    /// Hunt record working copies for the current slot.
    pub hunt_slay: Vec<u16>,
    pub hunt_capture: Vec<u16>,
    pub hunt_size: Vec<(u16, u16)>,
    /// Filter for the hunt tab.
    pub hunt_filter: String,
    /// Filter for the equipment tab.
    pub equip_filter: String,
    /// Box index of the equipment row the editor on the right is showing.
    pub equip_selected: Option<usize>,
    /// Show the empty slots next to the used ones. Off by default: a save has
    /// 2000 records and only a few hundred of them hold anything.
    pub equip_show_empty: bool,
    /// Index of the saved equipment set the panel on the right is editing.
    pub set_selected: Option<usize>,
    /// Index of the item set the マイセット tab's editor is showing.
    pub item_set_selected: Option<usize>,
    /// The pouch as it was before ポーチへ適用, so the one action that
    /// replaces all 32 entries can be taken back. Cleared once it is used,
    /// and never written to disk.
    pub pouch_undo: Option<Vec<mhnx_core::myset::SetItem>>,
    /// A CSV file that was read and is waiting for the user to agree to it.
    pub csv_pending: Option<CsvPending>,
    /// Search text of the seven equipment pickers in the set editor, one per
    /// part: each picker narrows the box on its own.
    pub set_part_search: [String; mhnx_core::myset::EQUIP_SET_PARTS],
    /// Roster index of the otomo whose three gear slots the panel is editing.
    pub palico_selected: Option<usize>,
    /// Box index of the otomo gear record the editor on the right is showing.
    pub palico_gear_selected: Option<usize>,
    /// Search text of the otomo gear list.
    pub palico_gear_filter: String,
    /// Show the blank records of the otomo gear box too. Off by default: the
    /// box holds a thousand records and the sample character uses 72.
    pub palico_gear_show_empty: bool,
    /// Search text of the three otomo decoration pickers, one per socket.
    pub palico_deco_search: [String; 3],
    /// Search text of the three decoration pickers, one per socket.
    pub equip_deco_search: [String; 3],
    /// Type, id and level the "空き枠に作成" row is set to.
    pub equip_new_kind: u8,
    pub equip_new_id: u16,
    pub equip_new_level: u16,
    /// Type, id and level the replace row in the editor is set to, and which
    /// record they were seeded from: the row shows the record it will replace,
    /// not whatever was left there by the last record that was looked at.
    pub equip_rep_for: Option<usize>,
    pub equip_rep_kind: u8,
    pub equip_rep_id: u16,
    pub equip_rep_level: u16,
    pub item_pick_keep_filter: bool,
    /// Row selected inside the .rem / .sup reward table.
    pub entry_item_sel: Option<usize>,
    /// Free-text filter for the talisman list.
    pub charm_filter: String,
    /// Search text inside the two skill pickers.
    pub skill_search1: String,
    pub skill_search2: String,
    /// Charm editor fields.
    pub charm_index: usize,
    pub charm_skill1: u8,
    pub charm_points1: i8,
    pub charm_skill2: u8,
    pub charm_points2: i8,
    pub charm_slots: u8,

    // ---- snapshots ----
    /// Snapshots next to the open save, newest first.
    pub snapshots: Vec<SnapshotEntry>,

    // ---- dlc ----
    pub dlc_files: Vec<PathBuf>,
    pub dlc_log: Vec<String>,

    // ---- progress (進行状況) ----
    /// romfs folder the user typed or picked. Nothing is read from it until
    /// 読み込む is pressed.
    pub romfs_path: String,
    /// What the last 読み込む found, shown under the path field.
    pub romfs_status: String,
    /// Folder the loaded names came from; None when nothing is loaded.
    pub romfs_dir: Option<PathBuf>,
    /// The 132 award names, empty when the medal table was not read.
    pub award_names: Vec<String>,
    /// The 184 request names, empty when the activity table was not read.
    pub request_names: Vec<String>,
    /// The 184 request records (accept / complete flag numbers).
    pub request_records: Vec<RequestRecord>,
    /// Which of the three sections is open.
    pub progress_section: ProgressSection,
    /// Undo steps of the three sections, newest last.
    pub progress_undo: Vec<ProgressUndo>,
    /// Text filter of the request list.
    pub request_filter: String,
    /// Text filter of the quest list.
    pub quest_filter: String,
    /// Show only quests that are not cleared.
    pub quest_missing_only: bool,
    /// Set once the launch-time hooks this file owns have been applied, so a
    /// later frame cannot run them again.
    pub launch_env_done: bool,

    // ---- headless screenshot / smoke test mode ----
    /// When set, the app renders a few frames, saves a PNG there and exits.
    pub screenshot_path: Option<PathBuf>,
    pub screenshot_frames: u32,
    pub screenshot_requested: bool,
}

impl Default for MhnxEditor {
    fn default() -> Self {
        MhnxEditor {
            tab: Tab::Save,
            status: String::new(),
            message: String::new(),
            save_path: None,
            save: None,
            dirty: false,
            slot: 0,
            edit: Character::default(),
            items: Vec::new(),
            item_filter: String::new(),
            item_selected: None,
        charm_confirm: CharmConfirm::default(),
        scroll_to_item: None,
        charm_new_grade: mhnx_core::charm::TalismanGrade::Enduring,
        qe_slot: None,
        qe_pick: None,
        confirm: Confirm::default(),
        discard_ok: false,
        pending_dlc_slot: None,
            item_show_empty: false,
            item_bulk_open: false,
            item_bulk_filter: String::new(),
            item_bulk_id: 0,
            item_bulk_count: 10,
            arc_path: None,
            arc: None,
            arc_selected: 0,
            arc_filter: String::new(),
            arc_from_save: None,
            mib: None,
            mib_bytes: Vec::new(),
            gmd: None,
            findings: Vec::new(),
            cat_lang: 0,
            cat_id: 0,
            item_pick_filter: String::new(),
            dlc_filter: String::new(),
            dlc_outdir: String::new(),
            item_undo: Vec::new(),
            hunt_slay: Vec::new(),
            hunt_capture: Vec::new(),
            hunt_size: Vec::new(),
            hunt_filter: String::new(),
            equip_filter: String::new(),
            equip_selected: None,
            equip_show_empty: false,
            set_selected: None,
            item_set_selected: None,
            pouch_undo: None,
            csv_pending: None,
            set_part_search: [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ],
            equip_deco_search: [String::new(), String::new(), String::new()],
            palico_selected: None,
            palico_gear_selected: None,
            palico_gear_filter: String::new(),
            palico_gear_show_empty: false,
            palico_deco_search: [String::new(), String::new(), String::new()],
            equip_new_kind: 7,
            equip_new_id: 1,
            equip_new_level: 1,
            equip_rep_for: None,
            equip_rep_kind: 7,
            equip_rep_id: 1,
            equip_rep_level: 1,
            item_pick_keep_filter: true,
            entry_item_sel: None,
            charm_filter: String::new(),
            skill_search1: String::new(),
            skill_search2: String::new(),
            charm_index: 0,
            charm_skill1: 1,
            charm_points1: 1,
            charm_skill2: 0,
            charm_points2: 0,
            charm_slots: 0,
            snapshots: Vec::new(),
            dlc_files: Vec::new(),
            dlc_log: Vec::new(),
            romfs_path: String::new(),
            romfs_status: String::new(),
            romfs_dir: None,
            award_names: Vec::new(),
            request_names: Vec::new(),
            request_records: Vec::new(),
            progress_section: ProgressSection::default(),
            progress_undo: Vec::new(),
            request_filter: String::new(),
            quest_filter: String::new(),
            quest_missing_only: false,
            launch_env_done: false,
            screenshot_path: None,
            screenshot_frames: 0,
            screenshot_requested: false,
        }
    }
}

impl MhnxEditor {
    // ---------------------------------------------------------------- save
    pub fn open_save(&mut self, path: PathBuf) {
        // Loading another file replaced whatever was in the working copies,
        // and nothing had been written yet. Ask once, and remember the answer
        // for the very next attempt so the caller can simply try again.
        if self.dirty && Some(&path) != self.save_path.as_ref() && !self.discard_ok {
            self.confirm = Confirm::DiscardForOpen(path);
            return;
        }
        self.discard_ok = false;
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| {
            Save::from_bytes(&b).map_err(|e| e.to_string())
        }) {
            Ok(s) => {
                // MHGU is a different game: the slot stride and the section
                // layout differ, so every offset this tool edits is wrong for
                // it. Only the DLC tab used to say so, and it said so after the
                // work was done. Refuse the write outright instead.
                let mhgu = s.kind() == SaveKind::Mhgu;
                self.save = Some(s);
                self.save_path = Some(path.clone());
                self.dirty = false;
                self.message = if mhgu {
                    format!(
                        "読み込みましたが MHGU のセーブです: {}（このツールは MHXX 用なので保存できません）",
                        path.display()
                    )
                } else {
                    format!("読み込みました: {}", path.display())
                };
                self.reload_from_save();
                self.refresh_snapshots();
            }
            Err(e) => self.message = format!("読み込み失敗: {}", e),
        }
    }

    /// Reload the working copies (character + item box) from the save.
    pub fn reload_from_save(&mut self) {
        self.items.clear();
        self.item_selected = None;
        // The undo snapshots are whole item boxes from the slot that was open.
        // Keeping them across a slot change let one press of undo write the
        // previous character's 186 items into the new character's box.
        self.item_undo.clear();
        // The 進行状況 undo holds bytes taken from the slot that was open, for
        // the same reason: putting them back after a slot switch would write
        // one character's awards into another's.
        self.progress_undo.clear();
        // The same for the set editor: the selection points into a character's
        // own forty records, so it cannot survive a slot change.
        self.set_selected = None;
        for s in self.set_part_search.iter_mut() {
            s.clear();
        }
        // The item set editor and the pouch undo are the same: both name a
        // record of this slot's own section.
        self.item_set_selected = None;
        self.pouch_undo = None;
        // A staged CSV file is a list of rows for the box that was open, and
        // applying it after a slot switch would write it into another box.
        self.csv_pending = None;
        // The otomo panel names a roster record and a row of this slot's own
        // gear box, so neither number means anything in another slot.
        self.palico_selected = None;
        self.palico_gear_selected = None;
        let Some(s) = &self.save else { return };
        if let Some(c) = s.read_character(self.slot) {
            self.edit = c;
        } else {
            self.edit = Character {
                slot: self.slot,
                ..Default::default()
            };
        }
        if let Some(items) = s.read_item_box(self.slot) {
            self.items = items;
        }
        self.hunt_slay = s.hunt_counts(self.slot).unwrap_or_default();
        self.hunt_capture = s.capture_counts(self.slot).unwrap_or_default();
        self.hunt_size = s.monster_sizes(self.slot).unwrap_or_default();
    }

    /// Write the hunt tables back, one monster at a time.
    fn apply_hunts(&mut self) {
        let Some(s) = &mut self.save else { return };
        let slot = self.slot;
        for i in 0..self.hunt_slay.len() {
            s.set_hunt_count(slot, i, self.hunt_slay[i]);
            if i < self.hunt_capture.len() {
                s.set_capture_count(slot, i, self.hunt_capture[i]);
            }
            if i < self.hunt_size.len() {
                let (a, b) = self.hunt_size[i];
                // The game keeps the pair ordered; writing it the other way
                // round would be a value it never produces.
                let (a, b) = if a > b { (b, a) } else { (a, b) };
                self.hunt_size[i] = (a, b);
                s.set_monster_size(slot, i, a, b);
            }
        }
        self.dirty = true;
        self.message = "狩猟記録を適用しました（まだファイルには書いていません）".to_owned();
    }

    /// Push the working copies back into the save.
    pub fn apply_to_save(&mut self) {
        let Some(s) = &mut self.save else { return };
        self.edit.slot = self.slot;
        if !s.slot_used(self.slot) {
            s.set_slot_used(self.slot, true);
        }
        if let Err(e) = s.write_character(&self.edit) {
            self.message = format!("適用失敗: {}", e);
            return;
        }
        if !self.items.is_empty() {
            if let Err(e) = s.write_item_box(self.slot, &self.items) {
                self.message = format!("アイテム適用失敗: {}", e);
                return;
            }
        }
        self.dirty = true;
        self.message = "編集内容を保存データに反映しました (まだファイルには書いていません)".to_owned();
    }

    /// Copy the file that is about to be overwritten to <name>.bak.
    ///
    /// Nothing used to keep the previous contents: one bad write and the save
    /// was gone. Returns the backup path when one was made.
    fn backup_before_write(path: &std::path::Path) -> Option<PathBuf> {
        if !path.exists() {
            return None;
        }
        let bak = path.with_extension(
            path.extension()
                .map(|e| format!("{}.bak", e.to_string_lossy()))
                .unwrap_or_else(|| String::from("bak")),
        );
        match std::fs::copy(path, &bak) {
            Ok(_) => Some(bak),
            Err(_) => None,
        }
    }

    // ------------------------------------------------------------ snapshots

    /// Folder the automatic snapshots live in: next to the save itself.
    fn snapshot_dir(path: &std::path::Path) -> PathBuf {
        path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("snapshots")
    }

    /// The save's name without its extension, the stem snapshots are named by.
    fn snapshot_stem(path: &std::path::Path) -> String {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_owned())
            .unwrap_or_else(|| String::from("save"))
    }

    /// Seconds since the epoch; 0 if the clock is before 1970.
    fn unix_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Keep `bytes` as a snapshot of `stem` next to `path`.
    ///
    /// Returns the file name that was created. Only the newest ten survive:
    /// the single `.bak` generation was the whole safety net, and the second
    /// save already overwrote it.
    fn push_snapshot(
        path: &std::path::Path,
        stem: &str,
        bytes: &[u8],
    ) -> std::io::Result<String> {
        let dir = Self::snapshot_dir(path);
        std::fs::create_dir_all(&dir)?;
        let mut unix = Self::unix_now();
        let mut name = snapshot::name(stem, unix);
        // Two saves inside the same second would overwrite the earlier
        // snapshot. The stamp is fixed width, so stepping the second forward
        // keeps the list in the right order.
        let mut tries = 0;
        while dir.join(&name).exists() && tries < 120 {
            unix += 1;
            name = snapshot::name(stem, unix);
            tries += 1;
        }
        std::fs::write(dir.join(&name), bytes)?;
        let existing: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_str().map(|n| n.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let prefix = format!("{}-", stem);
        for i in snapshot::older_than(&existing, &prefix, 10) {
            // A snapshot that will not delete is not worth a dialog; it only
            // means one file more than the limit is kept.
            let _ = std::fs::remove_file(dir.join(&existing[i]));
        }
        Ok(name)
    }

    /// What the status bar says about the snapshot a save just made.
    ///
    /// A snapshot that could not be written is reported rather than dropped:
    /// the save itself is fine, but the extra safety net is not there.
    fn snapshot_note(made: std::io::Result<String>) -> String {
        match made {
            Ok(name) => format!("（スナップショット: {}）", name),
            Err(e) => format!("（スナップショットを作れませんでした: {}）", e),
        }
    }

    /// Re-read the snapshot folder next to the open save, newest first.
    pub fn refresh_snapshots(&mut self) {
        self.snapshots.clear();
        let Some(path) = self.save_path.clone() else {
            return;
        };
        let dir = Self::snapshot_dir(&path);
        let prefix = format!("{}-", Self::snapshot_stem(&path));
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut found: Vec<SnapshotEntry> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_owned();
                if !name.starts_with(&prefix) || !name.ends_with(".sav") {
                    return None;
                }
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                Some(SnapshotEntry { name, size })
            })
            .collect();
        // The stamp is fixed width inside the name, so a descending name sort
        // is newest first.
        found.sort_by(|a, b| b.name.cmp(&a.name));
        self.snapshots = found;
    }

    // ---------------------------------------------------- writing, verified

    /// Put a file back the way it was after a write that did not land.
    ///
    /// `trip` is the comparison that failed, or `None` when reading the file
    /// back failed at all. The message always contains 「照合に失敗」 so the
    /// bottom bar can draw it in red.
    fn undo_bad_write(
        path: &std::path::Path,
        backup: &Option<PathBuf>,
        trip: Option<&verify::RoundTrip>,
        why: &str,
    ) -> String {
        let detail = match trip {
            Some(t) => match t.length_change {
                Some((a, b)) => format!("長さが {} → {} バイトに変わりました", a, b),
                None => format!("差分 {} 件: {}", t.differing.len(), t.examples(3)),
            },
            None => why.to_owned(),
        };
        match backup {
            Some(b) => match std::fs::copy(b, path) {
                Ok(_) => format!(
                    "照合に失敗したので戻しました（{}）。{} から復元しました",
                    detail,
                    b.display()
                ),
                Err(e) => format!(
                    "照合に失敗し、{} からの復元にも失敗しました（{}）。{} は壊れたままの可能性があります: {}",
                    b.display(),
                    detail,
                    path.display(),
                    e
                ),
            },
            None => format!(
                "照合に失敗しました（{}）。戻せる控え (.bak) がないため、{} は今の状態のままです",
                detail,
                path.display()
            ),
        }
    }

    /// Write `bytes`, read the file back and compare the two.
    ///
    /// `fs::write` only reports that the call did not fail; it says nothing
    /// about what the file now holds. On a mismatch the file is put back from
    /// the `.bak` copy taken just before the write. Returns the backup path
    /// (when one was made) and a success line, or the failure message.
    fn write_and_verify(
        path: &std::path::Path,
        bytes: &[u8],
    ) -> std::result::Result<(Option<PathBuf>, String), String> {
        let backup = Self::backup_before_write(path);
        if let Err(e) = std::fs::write(path, bytes) {
            // A failed write may still have truncated the file, so put the
            // copy back before saying anything else.
            let put_back = match &backup {
                Some(b) => match std::fs::copy(b, path) {
                    Ok(_) => format!("（元の控え {} から戻しました）", b.display()),
                    Err(e2) => format!(
                        "（{} からの復元にも失敗しました: {}）",
                        b.display(),
                        e2
                    ),
                },
                None => String::new(),
            };
            return Err(format!(
                "書き出し失敗: {}: {}{}",
                path.display(),
                e,
                put_back
            ));
        }
        let read_back = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                return Err(Self::undo_bad_write(
                    path,
                    &backup,
                    None,
                    &format!("読み戻しに失敗: {}", e),
                ))
            }
        };
        let trip = verify::compare(bytes, &read_back);
        if trip.is_identical() {
            Ok((backup, format!("照合OK: {} バイト一致", bytes.len())))
        } else {
            Err(Self::undo_bad_write(path, &backup, Some(&trip), ""))
        }
    }

    pub fn write_current(&mut self, path: PathBuf) {
        // See open_save: the offsets are MHXX's, so writing an MHGU save back
        // would corrupt it.
        if matches!(&self.save, Some(s) if s.kind() == SaveKind::Mhgu) {
            self.message =
                "MHGU のセーブは保存できません（このツールのオフセットは MHXX 用です）".to_owned();
            return;
        }
        self.apply_to_save();
        // The Switch header carries a CRC of the file name at +0x14, so a save
        // written out under another name kept the checksum of the name it was
        // read under. Saving over the same file is still a no-op: rebuilding
        // the header for the name it already had produces the same bytes.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(s) = &mut self.save {
                s.refresh_header(name);
            }
        }
        // The hunt tables are edited in their own buffers, which used to reach
        // the save only if the user pressed the apply button first: writing
        // straight from the table silently dropped the changes.
        self.apply_hunts();
        let Some(bytes) = self.save.as_ref().map(|s| s.to_bytes()) else {
            return;
        };
        match Self::write_and_verify(&path, &bytes) {
            Ok((backup, check)) => {
                self.dirty = false;
                self.save_path = Some(path.clone());
                // The snapshot is the very bytes that were just proven to be
                // on disk, so every entry in the list is a save that happened.
                let stamped = Self::push_snapshot(&path, &Self::snapshot_stem(&path), &bytes);
                self.refresh_snapshots();
                let mut m = format!("書き出しました: {}", path.display());
                m += &format!("（{}）", check);
                if let Some(b) = backup {
                    m += &format!("（元のファイルは {} に退避）", b.display());
                }
                m += &Self::snapshot_note(stamped);
                self.message = m;
            }
            Err(e) => {
                // Nothing landed, so the edits are still only in memory.
                self.dirty = true;
                self.message = e;
            }
        }
    }

    /// Write a Switch save as both system and system_backup into a directory.
    pub fn write_switch_pair(&mut self, dir: PathBuf) {
        self.apply_to_save();
        // The hunt tables live in their own buffers; without this the export
        // silently wrote the values from disk instead of the ones on screen.
        self.apply_hunts();
        let Some(s) = &self.save else { return };
        if s.kind() == SaveKind::ThreeDs {
            self.message = "3DS セーブのままです。先に Switch 形式へ変換してください".to_owned();
            return;
        }
        let body = s.body().to_vec();
        let sys = mhnx_core::save::switch_header("system", body.len());
        let bak = mhnx_core::save::switch_header("system_backup", body.len());
        let mut a = sys.to_vec();
        a.extend_from_slice(&body);
        let mut b = bak.to_vec();
        b.extend_from_slice(&body);
        // This pair is what the console actually loads, so replacing it
        // deserves the same copy the single file overwrite keeps. Two files go
        // out, two come back.
        let r1 = Self::write_and_verify(&dir.join("system"), &a);
        let r2 = Self::write_and_verify(&dir.join("system_backup"), &b);
        match (r1, r2) {
            (Ok((saved_a, _)), Ok((saved_b, _))) => {
                self.dirty = false;
                // system_backup carries the same body as system, so one
                // snapshot of the pair is enough; a second copy would double
                // the disk use for identical bytes.
                let stamped = Self::push_snapshot(&dir.join("system"), "system", &a);
                self.refresh_snapshots();
                let kept = saved_a.iter().chain(saved_b.iter()).count();
                let mut m = if kept > 0 {
                    format!(
                        "system と system_backup を書き出しました（元のファイルは {} 件退避）: {}",
                        kept,
                        dir.display()
                    )
                } else {
                    format!("system と system_backup を書き出しました: {}", dir.display())
                };
                m += &format!("（照合OK: 2 ファイル {} バイト一致）", a.len());
                let note = stamped.map(|s| format!("{}/snapshots/{}", dir.display(), s));
                m += &Self::snapshot_note(note);
                self.message = m;
            }
            (e1, e2) => {
                // At least one of the two did not land; say which and why.
                self.dirty = true;
                let mut parts: Vec<String> = Vec::new();
                if let Err(e) = e1 {
                    parts.push(format!("system: {}", e));
                }
                if let Err(e) = e2 {
                    parts.push(format!("system_backup: {}", e));
                }
                self.message = parts.join(" / ");
            }
        }
    }

    /// Export the body as a 3DS save.
    pub fn export_3ds(&mut self, path: PathBuf) {
        self.apply_to_save();
        self.apply_hunts();
        let Some(bytes) = self.save.as_ref().map(|s| s.to_3ds_bytes()) else {
            return;
        };
        match Self::write_and_verify(&path, &bytes) {
            Ok((backup, check)) => {
                self.dirty = false;
                let mut m = format!("3DS 形式で書き出しました: {}", path.display());
                m += &format!("（{}）", check);
                if let Some(b) = backup {
                    m += &format!("（元のファイルは {} に退避）", b.display());
                }
                self.message = m;
            }
            Err(e) => {
                self.dirty = true;
                self.message = e;
            }
        }
    }

    /// Put a snapshot back over the save file.
    ///
    /// Whatever is there now goes into the list first, so the restore itself
    /// can be undone from the same panel.
    pub fn restore_snapshot(&mut self, name: String) {
        let Some(path) = self.save_path.clone() else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        let src = Self::snapshot_dir(&path).join(&name);
        let bytes = match std::fs::read(&src) {
            Ok(b) => b,
            Err(e) => {
                self.message = format!("スナップショットを読めません: {}: {}", name, e);
                return;
            }
        };
        // Never write something the save parser refuses: a truncated copy would
        // otherwise replace a working save with a file nothing can open.
        if let Err(e) = Save::from_bytes(&bytes) {
            self.message = format!(
                "スナップショット {} はセーブとして読めないので戻しません: {}",
                name, e
            );
            return;
        }
        // The current contents are kept before they are replaced.
        let mut note = String::new();
        if let Ok(current) = std::fs::read(&path) {
            if current != bytes {
                let stem = Self::snapshot_stem(&path);
                note = match Self::push_snapshot(&path, &stem, &current) {
                    Ok(s) => format!("（戻す前の内容は {} に退避）", s),
                    Err(e) => format!("（戻す前の内容を控えられませんでした: {}）", e),
                };
            }
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            self.message = format!("スナップショットから戻せません: {}{}", e, note);
            return;
        }
        let read_back = std::fs::read(&path).unwrap_or_default();
        let trip = verify::compare(&bytes, &read_back);
        if !trip.is_identical() {
            let detail = match trip.length_change {
                Some((a, b)) => format!("長さが {} → {} バイトに変わりました", a, b),
                None => format!("差分 {} 件: {}", trip.differing.len(), trip.examples(3)),
            };
            self.message = format!(
                "スナップショットの照合に失敗しました（{}）。{} は書き換わっている可能性があります{}",
                detail,
                path.display(),
                note
            );
            return;
        }
        if let Ok(s) = Save::from_bytes(&bytes) {
            self.save = Some(s);
        }
        self.dirty = false;
        self.reload_from_save();
        self.refresh_snapshots();
        self.message = format!("スナップショット {} の時点に戻しました{}", name, note);
    }

    /// Import a 3DS body into the currently open Switch save.
    pub fn import_3ds_body(&mut self, path: PathBuf) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.message = format!("読み込み失敗: {}", e);
                return;
            }
        };
        let Some(s) = &mut self.save else {
            self.message = "先に Switch セーブを開いてください".to_owned();
            return;
        };
        match s.replace_body(&bytes) {
            Ok(_) => {
                self.dirty = true;
                self.message = "3DS の本体データを取り込みました".to_owned();
                self.reload_from_save();
            }
            Err(e) => self.message = format!("取り込み失敗: {}", e),
        }
    }

    /// Turn the current 3DS save into a Switch save (header is synthesised).
    pub fn convert_to_switch(&mut self) {
        let Some(s) = &self.save else { return };
        if s.kind() != SaveKind::ThreeDs {
            self.message = "すでに Switch 形式です".to_owned();
            return;
        }
        match Save::from_3ds_body(s.body(), "system") {
            Ok(ns) => {
                self.save = Some(ns);
                self.dirty = true;
                self.message = "Switch 形式に変換しました (ヘッダは計算で生成)".to_owned();
            }
            Err(e) => self.message = format!("変換失敗: {}", e),
        }
    }

    // --------------------------------------------------------------- quest
    pub fn open_arc(&mut self, path: PathBuf) {
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| Arc::parse(&b).map_err(|e| e.to_string()))
        {
            Ok(a) => {
                // Prefer the quest parameter record, then the text container.
                let idx = a
                    .entries
                    .iter()
                    .position(|e| extension_for(e.type_hash) == Some("mib"))
                    .or_else(|| a.entries.iter().position(|e| extension_for(e.type_hash) == Some("gmd")))
                    .unwrap_or(0);
                self.arc = Some(a);
                self.arc_path = Some(path.clone());
                self.arc_from_save = None;
                self.load_arc_entry(idx);
                self.run_quest_check();
                self.message = format!("クエストを読み込みました: {}", path.display());
            }
            Err(e) => self.message = format!("読み込み失敗: {}", e),
        }
    }

    /// Open one of the save's own download quests in the quest editor.
    ///
    /// The arc is copied out of the save into memory, so the editor works on a
    /// copy and nothing is written until the user asks for it. Returns true
    /// when the archive opened.
    pub fn open_arc_from_save(&mut self, quest_id: i32) -> bool {
        let Some(save) = &self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return false;
        };
        let Some(q) = dlc::list_quests(save)
            .into_iter()
            .find(|q| q.quest_id == quest_id)
        else {
            self.message = format!("ID {} のクエストがこのセーブにありません", quest_id);
            return false;
        };
        if q.data.is_empty() {
            self.message = format!("ID {} のデータが壊れています（サイズ {}）", quest_id, q.size);
            return false;
        }
        match Arc::parse(&q.data) {
            Ok(a) => {
                let idx = a
                    .entries
                    .iter()
                    .position(|e| extension_for(e.type_hash) == Some("mib"))
                    .or_else(|| a.entries.iter().position(|e| extension_for(e.type_hash) == Some("gmd")))
                    .unwrap_or(0);
                self.arc = Some(a);
                // No path: this arc lives in the save, not on disk.
                self.arc_path = None;
                self.arc_from_save = Some(quest_id);
                self.load_arc_entry(idx);
                self.run_quest_check();
                self.tab = Tab::Quest;
                self.message = format!(
                    "セーブのクエスト {} を開きました（{} バイト）",
                    quest_id,
                    q.data.len()
                );
                true
            }
            Err(e) => {
                self.message = format!("ID {} は .arc として読めません: {}", quest_id, e);
                false
            }
        }
    }

    /// Install the archive that is open into a free quest slot.
    ///
    /// Unlike write_arc_to_save this does not need the quest to be in the
    /// save already: add_quest writes the payload and sets the catalogue bit
    /// together, which is what makes a brand new quest appear in game.
    pub fn add_open_arc_to_save(&mut self) {
        // The .mib and .gmd editors keep their edits in separate buffers, so
        // they have to be flushed into the archive before it is rebuilt, or
        // the quest is added with the values it had on disk.
        let selected = self.arc_selected;
        self.store_arc_entry(selected);
        let Some(arc) = &self.arc else {
            self.message = "先に .arc を開いてください".to_owned();
            return;
        };
        let bytes = match arc.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                self.message = format!("再構築失敗: {}", e);
                return;
            }
        };
        let Some(id) = mhnx_core::hbedit::quest_id_of_arc(&bytes) else {
            self.message = "クエストIDが読めません（.mib がない）".to_owned();
            return;
        };
        let Some(save) = &mut self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        match dlc::add_quest(save, id, &bytes) {
            Ok(slot) => {
                self.dirty = true;
                self.message = format!(
                    "クエスト {} を枠 {} に追加しました（{} バイト）",
                    id,
                    slot,
                    bytes.len()
                );
            }
            Err(e) => self.message = format!("追加できません: {}", e),
        }
    }
    /// Rebuild the open archive and write it back into the save.
    ///
    /// Only works for an archive that came out of the save, and only for a
    /// quest the save already holds: adding one would need its catalogue flag,
    /// which a slot number cannot address.
    pub fn write_arc_to_save(&mut self) {
        let Some(id) = self.arc_from_save else {
            self.message = "セーブから開いたクエストではありません".to_owned();
            return;
        };
        self.store_arc_entry(self.arc_selected);
        let bytes = match self.arc.as_ref().map(|a| a.to_bytes()) {
            Some(Ok(b)) => b,
            Some(Err(e)) => {
                self.message = format!("再構築失敗: {}", e);
                return;
            }
            None => return,
        };
        let Some(save) = &mut self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        match dlc::install_quest(save, id, &bytes) {
            Ok(slot) => {
                self.dirty = true;
                self.message = format!(
                    "クエスト {} を枠 {} に書き戻しました（{} バイト）",
                    id,
                    slot,
                    bytes.len()
                );
            }
            Err(e) => self.message = format!("書き戻し失敗: {}", e),
        }
    }

    /// Switch to another entry of the archive that is already open.
    ///
    /// The buffer of the entry being left is flushed first: mib_bytes and gmd
    /// hold the live edits, so switching without this threw away whatever was
    /// typed in the previous entry. Callers must not assign arc_selected
    /// themselves, or this sees no change and skips the flush.
    pub fn select_arc_entry(&mut self, index: usize) {
        let prev = self.arc_selected;
        if self.arc.is_some() && prev != index && prev < self.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0) {
            self.store_arc_entry(prev);
        }
        self.load_arc_entry(index);
    }

    /// Point the editors at an entry of a freshly opened archive, discarding
    /// whatever the previous archive had in the buffers.
    pub fn load_arc_entry(&mut self, index: usize) {
        self.arc_selected = index;
        self.mib = None;
        self.gmd = None;
        self.mib_bytes.clear();
        let Some(a) = &self.arc else { return };
        let Some(e) = a.entries.get(index) else { return };
        match extension_for(e.type_hash) {
            Some("mib") => {
                if let Ok(m) = Mib::from_bytes(&e.data) {
                    self.mib_bytes = m.raw.clone();
                    self.mib = Some(m);
                }
            }
            Some("gmd") => {
                if let Ok(g) = Gmd::from_bytes(&e.data) {
                    self.gmd = Some(g);
                }
            }
            _ => {}
        }
    }

    pub fn store_arc_entry(&mut self, index: usize) {
        if let Some(m) = &mut self.mib {
            if self.mib_bytes.len() == m.raw.len() {
                m.raw.copy_from_slice(&self.mib_bytes);
            }
        }
        let mib_new = self.mib.as_ref().map(|m| m.raw.clone());
        let gmd_new = self.gmd.as_ref().map(|g| g.to_bytes());
        let Some(a) = &mut self.arc else { return };
        let Some(e) = a.entries.get_mut(index) else { return };
        if let Some(d) = mib_new {
            e.data = d;
        }
        if let Some(d) = gmd_new {
            e.data = d;
        }
    }

    pub fn save_arc(&mut self, path: PathBuf) {
        self.store_arc_entry(self.arc_selected);
        let Some(a) = &self.arc else { return };
        match a.to_bytes() {
            Ok(bytes) => match std::fs::write(&path, &bytes) {
                Ok(_) => self.message = format!(".arc を書き出しました: {}", path.display()),
                Err(e) => self.message = format!("書き出し失敗: {}", e),
            },
            Err(e) => self.message = format!("再構築失敗: {}", e),
        }
    }

    /// Collect every .arc in a folder.
    pub fn scan_quest_dir(&mut self, dir: PathBuf) {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("arc") {
                    v.push(p);
                }
            }
        }
        v.sort();
        self.message = format!("{} 件の .arc を検出しました", v.len());
        self.dlc_files = v;
    }

    /// Install the selected .arc files into the open save.
    pub fn install_dlc(&mut self) {
        let files = self.dlc_files.clone();
        let Some(save) = &mut self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        self.dlc_log.clear();
        let mut ok = 0usize;
        for f in &files {
            let id = match f
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix('q'))
                .and_then(|s| s.parse::<i32>().ok())
            {
                Some(v) => v,
                None => continue,
            };
            match std::fs::read(f) {
                Ok(bytes) => match dlc::install_quest(save, id, &bytes) {
                    Ok(slot) => {
                        self.dlc_log.push(format!("q{} -> スロット {}", id, slot));
                        ok += 1;
                    }
                    Err(e) => self.dlc_log.push(format!("q{}: {}", id, e)),
                },
                Err(e) => self.dlc_log.push(format!("q{}: {}", id, e)),
            }
        }
        self.dirty = true;
        self.message = format!("{} 件をインストールしました", ok);
    }

    /// Write quest arcs from the save back out as files.
    ///
    /// With an empty id list every quest is written. The payload is copied byte
    /// for byte: the save already holds a finished arc, so nothing is re-packed.
    /// Returns the number of files written.
    pub fn extract_dlc(&mut self, outdir: &str, ids: &[i32]) -> usize {
        let Some(save) = &self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return 0;
        };
        let list = dlc::list_quests(save);
        if list.is_empty() {
            self.message = "このセーブに配信クエストはありません".to_owned();
            return 0;
        }
        // An empty path is not a directory: create_dir_all("") returns Ok and
        // the loop below then wrote every quest into the working directory.
        if outdir.trim().is_empty() {
            self.message = "出力先を指定してください（空のままだと書き出しません）".to_owned();
            return 0;
        }
        let dir = std::path::Path::new(outdir);
        if let Err(e) = std::fs::create_dir_all(dir) {
            self.message = format!("出力先を作れません: {}", e);
            return 0;
        }
        let mut written = 0usize;
        let mut missing: Vec<i32> = ids.to_vec();
        for q in &list {
            if !ids.is_empty() && !ids.contains(&q.quest_id) {
                continue;
            }
            if let Some(i) = missing.iter().position(|x| *x == q.quest_id) {
                missing.remove(i);
            }
            if q.data.is_empty() {
                continue;
            }
            let path = dir.join(format!("q{:07}.arc", q.quest_id));
            if std::fs::write(&path, &q.data).is_ok() {
                written += 1;
            }
        }
        self.message = if missing.is_empty() {
            format!("{} 件を {} に書き出しました", written, outdir)
        } else {
            format!(
                "{} 件を書き出しました。見つからない ID: {}",
                written,
                missing.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
            )
        };
        written
    }

    /// Zero one quest slot.
    ///
    /// This is a one-way door: the record disappears but the installed flag
    /// lives in a catalogue indexed set that a slot number cannot address, so
    /// the flag stays set and the tool can never put the quest back. The UI
    /// asks for confirmation and says so.
    pub fn remove_dlc(&mut self, slot: usize) {
        let Some(save) = &mut self.save else { return };
        match dlc::uninstall_quest(save, slot) {
            Ok(_) => {
                self.dirty = true;
                self.message = format!(
                    "スロット {} を空にしました（元に戻せません。ゲーム側には空の項目が残る可能性があります）",
                    slot
                );
            }
            Err(e) => self.message = format!("削除失敗: {}", e),
        }
    }
}

// ------------------------------------------------------------------ drawing

impl eframe::App for MhnxEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The hooks main.rs does not know about are resolved here; see
        // apply_launch_env.
        self.apply_launch_env();
        // Closing the window with unwritten edits used to lose them silently.
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.discard_ok {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Confirm::Quit;
        }
        self.confirm_dialog(ctx);
        self.top_bar(ctx);
        // Tabs live in their own bar: laying them out inside the side panel
        // made the panel grow to fit the row, so it covered more than half the
        // window and pushed controls off the bottom of every tab.
        self.tab_bar(ctx);
        self.side_panel(ctx);
        self.bottom_bar(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            // These tabs stack several sections and could grow past the
            // window with no way to reach the last one. The ones that already
            // scroll their own lists are left alone so they do not nest.
            // Dlc is in the list for the same reason: the supply editor grew
            // under the equipment list and pushed the quest list itself below
            // the panel, where a clipped list cannot be reached at all.
            let outer_scroll =
                matches!(self.tab, Tab::Sets | Tab::Extras | Tab::Guild | Tab::Dlc);
            if outer_scroll {
                egui::ScrollArea::vertical()
                    .id_salt("outer")
                    .show(ui, |ui| self.draw_tab(ui));
            } else {
                self.draw_tab(ui);
            }
        });

        self.drive_screenshot(ctx);
    }
}

impl MhnxEditor {
    /// One place that says which tab draws what.
    fn draw_tab(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            Tab::Save => self.tab_save(ui),
            Tab::Items => self.tab_items(ui),
            Tab::Quest => self.tab_quest(ui),
            Tab::Dlc => self.tab_dlc(ui),
            Tab::Charm => self.tab_charm(ui),
            Tab::Equip => self.tab_equip(ui),
            Tab::Hunts => self.tab_hunts(ui),
            Tab::Guild => self.tab_guild(ui),
            Tab::Progress => self.tab_progress(ui),
            Tab::Palico => self.tab_palico_detail(ui),
            Tab::Sets => self.tab_sets(ui),
            Tab::Extras => self.tab_extras(ui),
            Tab::About => self.tab_about(ui),
        }
    }
}

impl MhnxEditor {
    /// Headless verification hook.
    ///
    /// When a screenshot path is configured the app renders a few frames,
    /// asks egui for a framebuffer copy, writes it as a PNG and quits. This
    /// gives an automated smoke test for the whole UI: if any tab panics the
    /// PNG is never produced.
    fn drive_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot_path.clone() else {
            return;
        };
        self.screenshot_frames += 1;
        if self.screenshot_frames >= 3 && !self.screenshot_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.screenshot_requested = true;
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let (w, h) = (img.size[0] as u32, img.size[1] as u32);
            let raw: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
            match image::RgbaImage::from_raw(w, h, raw) {
                Some(buf) => match buf.save(&path) {
                    Ok(_) => eprintln!("screenshot written to {}", path.display()),
                    Err(e) => eprintln!("screenshot save failed: {}", e),
                },
                None => eprintln!("screenshot buffer mismatch"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl MhnxEditor {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("XXSE");
                ui.separator();
                if ui.button("セーブを開く").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("MHXX save (system)", &["system", "sav", "*"])
                        .set_title("system ファイルを選択")
                        .pick_file()
                    {
                        self.open_save(p);
                    }
                }
                let has_save = self.save.is_some();
                if ui.add_enabled(has_save, egui::Button::new("上書き保存")).clicked() {
                    if let Some(p) = self.save_path.clone() {
                        self.write_current(p);
                    }
                }
                if ui.add_enabled(has_save, egui::Button::new("名前を付けて保存")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_file_name("system")
                        .set_title("保存先")
                        .save_file()
                    {
                        self.write_current(p);
                    }
                }
                if ui.add_enabled(has_save, egui::Button::new("Switch 用 (system 2 ファイル)")).clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_title("出力フォルダ").pick_folder() {
                        self.write_switch_pair(d);
                    }
                }
                if ui.add_enabled(has_save, egui::Button::new("3DS 形式で書き出す")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_file_name("system")
                        .set_title("3DS 形式の保存先")
                        .save_file()
                    {
                        self.export_3ds(p);
                    }
                }
                ui.separator();
                if ui.button(".arc を開く").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("quest archive", &["arc"])
                        .set_title("クエスト .arc を選択")
                        .pick_file()
                    {
                        self.open_arc(p);
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("3DS <-> Switch 変換").strong());
                let is_3ds = self.save.as_ref().map(|s| s.kind() == SaveKind::ThreeDs).unwrap_or(false);
                let is_switch = self
                    .save
                    .as_ref()
                    .map(|s| s.kind() != SaveKind::ThreeDs)
                    .unwrap_or(false);

                if ui
                    .add_enabled(is_3ds, egui::Button::new("3DS から Switch へ変換 (ヘッダを合成)"))
                    .on_hover_text(
                        "36 バイトのヘッダは CRC32(ファイル名) から計算できるため、実機で作った空セーブは不要です",
                    )
                    .clicked()
                {
                    self.convert_to_switch();
                }
                if ui
                    .add_enabled(
                        is_switch,
                        egui::Button::new("Switch セーブに 3DS の本体を取り込む"),
                    )
                    .on_hover_text("ヘッダはそのままに、本体 4,726,152 バイトだけを差し替えます")
                    .clicked()
                {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("3DS save (system)", &["system", "sav", "*"])
                        .set_title("3DS の system を選択")
                        .pick_file()
                    {
                        self.import_3ds_body(p);
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new("ヒント: 編集後は「Switch 用 (system 2 ファイル)」で system と system_backup を出力できます")
                        .weak()
                        .small(),
                );
            });
        });
    }

    /// The tab bar, across the top of the window.
    fn tab_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (tab, label) in [
                    (Tab::Save, "キャラ"),
                    (Tab::Items, "アイテム"),
                    (Tab::Quest, "クエスト"),
                    (Tab::Dlc, "DLC"),
                    (Tab::Charm, "お守り"),
                    (Tab::Equip, "装備"),
                    (Tab::Hunts, "狩猟記録"),
                    (Tab::Guild, "ギルド"),
                    (Tab::Progress, "進行状況"),
                    (Tab::Palico, "オトモ"),
                    (Tab::Sets, "マイセット"),
                    (Tab::Extras, "設定/オトモ"),
                    (Tab::About, "情報"),
                ] {
                    // A save has to be open before a tab can be anything but a
                    // dead end: eleven of them were just a line of text.
                    let enabled = self.save.is_some() || matches!(tab, Tab::Save | Tab::About);
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.selectable_value(&mut self.tab, tab, label);
                    });
                }
                if self.save.is_none() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new("セーブを開くと他のタブが使えます")
                            .weak()
                            .small(),
                    );
                }
            });
        });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side")
            .resizable(true)
            .default_width(320.0)
            .max_width(420.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(4.0);

                    ui.label(egui::RichText::new("セーブ情報").strong());
                    let mut pending_slot: Option<usize> = None;
                    match &self.save {
                        None => {
                            ui.label("まだセーブを開いていません");
                            ui.label("Switch: system / system_backup (4,726,188 B)");
                            ui.label("3DS: system (4,726,152 B)");
                        }
                        Some(s) => {
                            let kind = match s.kind() {
                                SaveKind::ThreeDs => "3DS (MHXX)",
                                SaveKind::Switch => "Switch (MHXX)",
                                SaveKind::Mhgu => "Switch (MHGU)",
                            };
                            egui::Grid::new("saveinfo").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
                                ui.label("形式");
                                ui.label(kind);
                                ui.end_row();
                                ui.label("ファイルサイズ");
                                // The whole 4.7MB save used to be serialised
                                // just to print its length, every frame.
                                ui.label(format!(
                                    "{} B",
                                    if s.kind().has_header() {
                                        mhnx_core::save::SWITCH_HEADER_SIZE + s.body().len()
                                    } else {
                                        s.body().len()
                                    }
                                ));
                                ui.end_row();
                                ui.label("本体サイズ");
                                ui.label(format!("{} B", s.body().len()));
                                ui.end_row();
                                ui.label("DLC セクション");
                                ui.label(format!("0x{:X}", s.dlc_section_offset()));
                                ui.end_row();
                                for (i, b) in s.slot_bases().iter().enumerate() {
                                    ui.label(format!("スロット{} 基点", i + 1));
                                    ui.label(format!("0x{:X} ({})", b, b));
                                    ui.end_row();
                                }
                                ui.label("ヘッダ +0x14");
                                ui.label(format!("0x{:08X}", u32::from_le_bytes([
                                    s.header()[0x14], s.header()[0x15], s.header()[0x16], s.header()[0x17]
                                ])));
                                ui.end_row();
                            });
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("保存スロット").strong());
                            for i in 0..3 {
                                let used = s.slot_used(i);
                                let name = s
                                    .read_character(i)
                                    .map(|c| c.name)
                                    .unwrap_or_default();
                                let label = format!(
                                    "{}: {}{}",
                                    i + 1,
                                    if name.is_empty() { "(空)" } else { name.as_str() },
                                    if used { "" } else { " [未使用]" }
                                );
                                if ui.selectable_label(self.slot == i, label).clicked() {
                                    pending_slot = Some(i);
                                }
                            }
                        }
                    }
                    if let Some(i) = pending_slot {
                        // Commit the edits first: reload_from_save() rebuilds
                        // the character, item box and talisman buffers from the
                        // save, so anything not applied yet was silently lost.
                        // The hunt tables live in their own buffers and were
                        // lost exactly this way: the save stayed clean while
                        // the window showed "unsaved".
                        if self.dirty {
                            self.apply_to_save();
                        }
                        self.apply_hunts();
                        self.slot = i;
                        self.reload_from_save();
                        self.message = format!("スロット {} に切り替えました", i + 1);
                    }

                    ui.separator();
                    ui.label(egui::RichText::new("クエスト情報").strong());
                    match &self.arc {
                        None => {
                            ui.label("まだ .arc を開いていません");
                        }
                        Some(a) => {
                            if let Some(p) = &self.arc_path {
                                ui.label(p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                            }
                            egui::Grid::new("arcinfo").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
                                ui.label("バージョン");
                                ui.label(format!("{} (0x{:X})", a.version, a.version));
                                ui.end_row();
                                ui.label("エントリ数");
                                ui.label(format!(
                                    "{} 件（表示中 {} 件）",
                                    a.entries.len(),
                                    a.entries.len()
                                ));
                                ui.end_row();
                                ui.label("合計 (展開後)");
                                ui.label(format!("{} B", a.entries.iter().map(|e| e.data.len()).sum::<usize>()));
                                ui.end_row();
                            });
                        }
                    }

                    ui.separator();
                    self.snapshots_panel(ui);
                });
            });
    }

    /// The snapshot section of the side panel.
    ///
    /// The list is read from the folder once (see refresh_snapshots) instead of
    /// every frame: the entries are 4.7 MB files and the panel is redrawn
    /// continuously.
    fn snapshots_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("スナップショット").strong());
        let Some(path) = self.save_path.clone() else {
            ui.label("セーブを開くと、保存のたびにここへ控えが残ります");
            return;
        };
        let dir = Self::snapshot_dir(&path);
        // The full path is wider than the panel, so it lives in the tooltip.
        ui.label(
            egui::RichText::new("snapshots/（最新 10 件 / 時刻は UTC）")
                .weak()
                .small(),
        )
        .on_hover_text(dir.display().to_string());
        if ui.small_button("一覧を更新").clicked() {
            self.refresh_snapshots();
        }
        if self.snapshots.is_empty() {
            ui.label("まだありません（保存すると 1 件できます）");
            return;
        }
        // A copy: the buttons below call back into self.
        let list: Vec<(String, u64)> = self
            .snapshots
            .iter()
            .map(|s| (s.name.clone(), s.size))
            .collect();
        let mut restore: Option<String> = None;
        for (name, size) in &list {
            ui.horizontal(|ui| {
                ui.label(Self::snapshot_stamp(name))
                    .on_hover_text(name.as_str());
                ui.label(egui::RichText::new(Self::human_size(*size)).weak().small());
                if ui
                    .small_button("この時点に戻す")
                    .on_hover_text("いまの内容は自動でスナップショットに退避してから戻します")
                    .clicked()
                {
                    restore = Some(name.clone());
                }
            });
        }
        if let Some(n) = restore {
            self.restore_snapshot(n);
        }
    }

    /// The timestamp part of a snapshot name (`system-20261104-012812.sav`).
    fn snapshot_stamp(name: &str) -> &str {
        let stem = name.strip_suffix(".sav").unwrap_or(name);
        let at = stem.len().saturating_sub(15);
        if at > 0 && stem.is_char_boundary(at) {
            &stem[at..]
        } else {
            stem
        }
    }

    /// A size for the snapshot list.
    fn human_size(n: u64) -> String {
        if n >= 1024 * 1024 {
            format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.0} KB", n as f64 / 1024.0)
        }
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.dirty {
                    ui.label(egui::RichText::new("● 未保存").color(egui::Color32::from_rgb(240, 180, 60)));
                } else {
                    ui.label("○ 変更なし");
                }
                ui.separator();
                // A write that did not survive the round trip is the one
                // message that must not look like an ordinary note.
                let text = egui::RichText::new(self.message.clone());
                let text = if Self::message_is_failure(&self.message) {
                    text.color(egui::Color32::from_rgb(229, 72, 77))
                } else {
                    text
                };
                ui.label(text);
            });
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(self.status.clone()).weak().small());
            }
        });
    }

    /// Whether a status message reports a failed round trip.
    ///
    /// Only the messages that say a written file did not read back the way it
    /// was meant are drawn in red; the wording is fixed by undo_bad_write, so
    /// this cannot drift out of step with it silently.
    fn message_is_failure(message: &str) -> bool {
        message.contains("照合に失敗")
    }

    /// A button that destroys something.
    ///
    /// They used to look exactly like every other button, so the one action
    /// that cannot be undone sat in the same row as the harmless ones.
    fn danger_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
        let fill = egui::Color32::from_rgb(92, 52, 56);
        let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 142, 142));
        ui.add(
            egui::Button::new(egui::RichText::new(label).color(egui::Color32::from_rgb(240, 190, 190)))
                .fill(fill)
                .stroke(stroke),
        )
    }

    fn tab_save(&mut self, ui: &mut egui::Ui) {
        if self.save.is_none() {
            ui.centered_and_justified(|ui| ui.label("左上の「セーブを開く」から system ファイルを選んでください"));
            return;
        }
        ui.heading(format!("キャラクター スロット {}", self.slot + 1));
        ui.add_space(4.0);
        // Every widget below writes into self.edit, which is only pushed into
        // the save by apply_to_save. Nothing here used to mark the save dirty,
        // so the window said "no changes", a slot switch rebuilt self.edit from
        // the save and the edits disappeared without a word.
        let before = self.edit.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("char").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("名前");
                ui.text_edit_singleline(&mut self.edit.name);
                ui.end_row();

                ui.label("プレイ時間 (秒)");
                let mut secs = self.edit.play_time;
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut secs).speed(60).range(0..=100_000_000));
                    ui.label(format!("= {}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60));
                });
                self.edit.play_time = secs;
                ui.end_row();

                ui.label("所持金 (z)");
                ui.add(egui::DragValue::new(&mut self.edit.funds).speed(1000).range(0..=9_999_999));
                ui.end_row();

                ui.label("ハンターランク");
                ui.add(egui::DragValue::new(&mut self.edit.hunter_rank).speed(1).range(0..=999));
                ui.end_row();

                ui.label("HR ポイント");
                ui.add(egui::DragValue::new(&mut self.edit.hr_points).speed(1000).range(0..=99_999_999));
                ui.end_row();

                ui.label("アカデミー点数");
                ui.add(egui::DragValue::new(&mut self.edit.academy_points).speed(100).range(0..=9_999_999));
                ui.end_row();

                for (i, name) in ["ベルナ", "ココット", "ポッケ", "ユクモ"].iter().enumerate() {
                    ui.label(format!("{} ポイント", name));
                    ui.add(egui::DragValue::new(&mut self.edit.village_points[i]).speed(100).range(0..=9_999_999));
                    ui.end_row();
                }
            });

            ui.separator();
            ui.label(egui::RichText::new("外見").strong());
            egui::Grid::new("look").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("性別");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.edit.gender, 0, "男性");
                    ui.selectable_value(&mut self.edit.gender, 1, "女性");
                });
                ui.end_row();
                for (label, v, max) in [
                    ("ボイス", &mut self.edit.voice, 40u8),
                    ("瞳の色", &mut self.edit.eye_color, 40),
                    ("服", &mut self.edit.clothing, 40),
                    ("髪型", &mut self.edit.hairstyle, 60),
                    ("顔", &mut self.edit.face, 40),
                    ("特徴", &mut self.edit.features, 40),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(v).speed(1).range(0..=max));
                    ui.end_row();
                }
            });

            ui.separator();
            ui.label(egui::RichText::new("カラー (RGBA)").strong());
            for (label, color) in [
                ("肌", &mut self.edit.skin_color),
                ("髪", &mut self.edit.hair_color),
                ("特徴", &mut self.edit.features_color),
                ("服", &mut self.edit.clothing_color),
            ] {
                ui.horizontal(|ui| {
                    ui.label(format!("{:<4}", label));
                    let mut c = egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        *color = [c.r(), c.g(), c.b(), c.a()];
                    }
                    ui.label(format!(
                        "R{} G{} B{} A{}",
                        color[0], color[1], color[2], color[3]
                    ));
                });
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("編集内容を反映 (メモリのみ)").clicked() {
                    self.apply_to_save();
                }
                if ui.button("このスロットを使う/使わない 切替").clicked() {
                    let used = self.save.as_ref().map(|s| s.slot_used(self.slot)).unwrap_or(false);
                    if let Some(s) = &mut self.save {
                        s.set_slot_used(self.slot, !used);
                        self.dirty = true;
                    }
                }
            });
        });
        if self.edit != before {
            self.dirty = true;
        }
    }

    /// Delete one download quest, after the dialog has said yes.
    pub fn clear_dlc(&mut self) {
        let Some(slot) = self.pending_dlc_slot.take() else {
            return;
        };
        if let Some(s) = &mut self.save {
            match dlc::uninstall_quest(s, slot) {
                Ok(()) => {
                    self.dirty = true;
                    self.message =
                        "空にしました（導入フラグは消せないため、ゲーム側には残ります）".to_owned();
                }
                Err(e) => self.message = format!("空にできません: {}", e),
            }
        }
    }

    /// Delete one charm, after the dialog has said yes.
    pub fn delete_charm(&mut self, index: usize) {
        if let Some(s) = &mut self.save {
            if s.clear_charm(self.slot, index) {
                self.dirty = true;
                self.message = format!("お守り {} を削除しました", index);
            }
        }
    }

    /// A modal that asks before something irreversible happens.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        // A dedupe that takes items with it is the one dialog that has to look
        // different: the line saying what is lost is drawn in the warning colour.
        let mut warn = false;
        let (title, body, yes) = match &self.confirm {
            Confirm::None => return,
            Confirm::Quit => (
                "未保存の変更があります",
                "保存せずに終了しますか？".to_string(),
                "保存せず終了",
            ),
            Confirm::DiscardForOpen(p) => (
                "未保存の変更があります",
                format!("保存せずに別のセーブを開きますか？\n{}", p.display()),
                "開く",
            ),
            Confirm::ClearDlc => (
                "クエストを削除します",
                match self.pending_dlc_slot {
                    Some(s) => format!(
                        "枠 {} のクエストを削除します。元に戻せません。
（ゲーム側には空の項目が残ることがあります）",
                        s
                    ),
                    None => "クエストを削除します。元に戻せません。".to_string(),
                },
                "削除する",
            ),
            Confirm::Charm(n) => (
                "お守りを削除します",
                format!("枠 {} のお守りを削除します。元に戻せません。", n),
                "削除する",
            ),
            Confirm::EquipClear(n) => (
                "装備を削除します",
                format!(
                    "枠 {} の装備を削除します。元に戻せません。\n\
                     装備中やマイセットがこの枠を指していると、その参照は空の枠を指したまま残ります。\n\
                     指している場合は、この次の確認で一覧を出してもう一度聞きます。",
                    n
                ),
                "削除する",
            ),
            Confirm::EquipClearReferenced { index, refs } => {
                let mut body =
                    format!("枠 {} の装備は、まだ次の場所から参照されています。\n", index);
                for r in refs.iter() {
                    body += &format!("・{}\n", r.text());
                }
                body += "\nそれでも削除すると、その参照は空の枠を指したまま残ります。\n\
                         このツールでは直せないので、ゲーム側で装備し直す必要があります。";
                ("装備はまだ参照されています", body, "それでも削除")
            }
            Confirm::EquipReplace {
                index,
                kind,
                id,
                level,
            } => (
                "装備を差し替えます",
                format!(
                    "枠 {} を「{}」ID {}（Lv{}）に差し替えます。\n\
                     装飾品 3 枠は空になります（前の装備のものなので残しません）。\n\
                     +0x0C 以降の持ち物はそのまま残ります。",
                    index,
                    mhnx_core::equip::type_name(*kind),
                    id,
                    level
                ),
                "差し替える",
            ),
            Confirm::RequestIncomplete { record } => {
                let title = self.request_title(*record);
                (
                    "依頼の「完了」を外します",
                    format!(
                        "依頼「{}」の完了フラグを外します。\n\
                         受理フラグも一緒に外しますか？\n\
                         「完了だけ外す」を選ぶと、受理はそのまま残ります。",
                        title
                    ),
                    "受理も外す",
                )
            }
            Confirm::ItemFillAll { changed } => (
                "全部 99 個にします",
                format!(
                    "使っている {} 枠の個数を、すべて 99 個にします。\n\
                     今の個数は失われます（この操作は元に戻せます）。",
                    changed
                ),
                "99 個にする",
            ),
            Confirm::ItemDedupe { freed, lost } => {
                let (line, warning) = Self::dedupe_confirm_text(DedupePreview {
                    freed: *freed,
                    lost: *lost,
                });
                warn = warning;
                let mut body = String::from("同じアイテムの 2 枠目以降を 1 枠にまとめます。\n");
                body += &line;
                body += "\n";
                if *lost > 0 {
                    body += "1 枠に 99 個までしか入らないため、あふれた分は失われます。\n";
                }
                body += "この操作は元に戻せます。";
                ("重複を整理します", body, "整理する")
            }
            Confirm::ItemCompact { moved } => (
                "空きを詰めます",
                format!(
                    "空き枠を消して、使っている {} 枠を前へ詰めます。\n\
                     順序が変わります（アイテムと個数はそのままです）。\n\
                     この操作は元に戻せます。",
                    moved
                ),
                "詰める",
            ),
            Confirm::ItemClamp { changed } => (
                "個数を 99 に丸めます",
                format!(
                    "99 を超えている {} 枠の個数を 99 に戻します。\n\
                     超えた分は失われます（この操作は元に戻せます）。",
                    changed
                ),
                "丸める",
            ),
            Confirm::SetToPouch(n) => {
                // The name is read now, so the dialog says which set it means.
                let name = self
                    .save
                    .as_ref()
                    .and_then(|s| {
                        mhnx_core::myset::item_sets(s, self.slot)
                            .into_iter()
                            .find(|set| set.index == *n)
                    })
                    .map(|set| set.name)
                    .unwrap_or_default();
                (
                    "ポーチを置き換えます",
                    format!(
                        "セット {}「{}」の 32 枠を、今のポーチに写します。\n\
                         今のポーチは置き換わります。\n\
                         ポーチはクエストに持ち込む 32 枠そのものです。\n\
                         置き換える前のポーチは控えてあり、「元に戻す」で戻せます。",
                        n, name
                    ),
                    "置き換える",
                )
            }
            Confirm::SetFromPouch(n) => (
                "セットにポーチを取り込みます",
                format!(
                    "今のポーチ 32 枠を、セット {} に写します。\n\
                     セットの中身は置き換わります（名前と装備はそのままです）。\n\
                     この操作は元に戻せません。",
                    n
                ),
                "取り込む",
            ),
            Confirm::CsvApply => {
                // Anything the file could not place is drawn in the warning
                // colour: the row count alone would hide it.
                let report = self.csv_pending.as_ref().map(|p| &p.report);
                warn = report
                    .map(|r| r.skipped > 0 || r.clamped > 0)
                    .unwrap_or(false);
                ("CSV を読み込みます", self.csv_confirm_text(), "適用する")
            }
            Confirm::SetClear(n) => {
                // The name is read now, so the dialog says which set it means
                // rather than only a number.
                let name = self
                    .save
                    .as_ref()
                    .and_then(|s| {
                        mhnx_core::myset::equip_sets(s, self.slot)
                            .into_iter()
                            .find(|set| set.index == *n)
                    })
                    .map(|set| set.name)
                    .unwrap_or_default();
                (
                    "装備セットを空にします",
                    format!(
                        "セット {}「{}」を空にします。\n\
                         名前は '－－－' になり、7 部位・装飾品の写し・防具の色が消えます。\n\
                         ゲームが未使用に残す並びに書き換えます。元に戻せません。",
                        n, name
                    ),
                    "空にする",
                )
            }
        };
        let mut answer: Option<bool> = None;
        // The request pair has two ways out, so the dialog offers both rather
        // than deciding for the user.
        let incomplete = matches!(self.confirm, Confirm::RequestIncomplete { .. });
        let mut complete_only = false;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if warn {
                    // A merge that takes items with it must not look like the
                    // harmless tidy-up the other dialogs are.
                    ui.label(
                        egui::RichText::new(body.as_str())
                            .color(egui::Color32::from_rgb(229, 72, 77)),
                    );
                } else {
                    ui.label(body.clone());
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    // The safer of the two answers sits first: taking the accept
                    // bit with it is one more thing to lose.
                    if incomplete && ui.button("完了だけ外す").clicked() {
                        answer = Some(true);
                        complete_only = true;
                    }
                    if ui.button(yes).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("やめる").clicked() {
                        answer = Some(false);
                    }
                });
            });
        match answer {
            Some(true) => {
                let what = core::mem::replace(&mut self.confirm, Confirm::None);
                self.discard_ok = true;
                match what {
                    Confirm::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Confirm::DiscardForOpen(p) => self.open_save(p),
                    Confirm::ClearDlc => self.clear_dlc(),
                    Confirm::Charm(n) => self.delete_charm(n),
                    // The first "yes" only starts the check: a record that is
                    // still named by something comes back as a second dialog.
                    Confirm::EquipClear(n) => self.equip_delete_checked(n),
                    Confirm::EquipClearReferenced { index, .. } => {
                        let _ = self.equip_delete(index, true);
                    }
                    Confirm::EquipReplace {
                        index,
                        kind,
                        id,
                        level,
                    } => {
                        self.equip_replace(index, kind, id, level);
                    }
                    // The two answers differ in one bit: 受理 follows 完了
                    // only when the user said so.
                    Confirm::RequestIncomplete { record } => {
                        self.set_request_complete(record, false, !complete_only);
                    }
                    Confirm::ItemFillAll { .. } => {
                        self.item_fill_all();
                    }
                    Confirm::ItemDedupe { .. } => {
                        self.item_dedupe();
                    }
                    Confirm::ItemCompact { .. } => {
                        self.item_compact();
                    }
                    Confirm::ItemClamp { .. } => {
                        self.item_clamp();
                    }
                    Confirm::SetClear(n) => {
                        self.set_clear(n);
                    }
                    Confirm::SetFromPouch(n) => {
                        self.item_set_from_pouch(n);
                    }
                    Confirm::SetToPouch(n) => {
                        self.item_set_to_pouch(n);
                    }
                    Confirm::CsvApply => {
                        self.csv_commit();
                    }
                    Confirm::None => {}
                }
            }
            Some(false) => {
                // A file that was turned down is dropped, so nothing is left
                // waiting to be applied by a later answer.
                if matches!(self.confirm, Confirm::CsvApply) {
                    self.csv_pending = None;
                }
                self.confirm = Confirm::None;
            }
            None => {}
        }
    }
    fn tab_items(&mut self, ui: &mut egui::Ui) {
        if self.items.is_empty() {
            ui.centered_and_justified(|ui| ui.label("セーブを開くとアイテムボックスを表示します"));
            return;
        }
        // Wrapped, because the row is wider than the content area at the
        // default window size and the last buttons fell off the right edge.
        ui.horizontal_wrapped(|ui| {
            ui.label("絞り込み");
            ui.text_edit_singleline(&mut self.item_filter);
            ui.checkbox(&mut self.item_show_empty, "空きスロットも表示");
            if ui
                .add_enabled(!self.item_undo.is_empty(), egui::Button::new("元に戻す"))
                .on_hover_text("直前のアイテム変更を取り消します")
                .clicked()
            {
                self.undo_item();
            }
        });

        // The bulk operations. Every one that can change what the box holds
        // only opens the confirmation here; dedupe in particular has to say how
        // many items it will drop before it drops them.
        ui.horizontal_wrapped(|ui| {
            // The toggle keeps its pressed look while the strip is open; the
            // triangle glyphs some fonts lack are not needed for that.
            let open = self.item_bulk_open;
            if ui
                .add(egui::Button::new("まとめて追加").selected(open))
                .on_hover_text("アイテム名と個数を決めて、空き枠へまとめて入れます")
                .clicked()
            {
                self.item_bulk_open = !self.item_bulk_open;
            }
            // These three are not destructive on their own - each one asks
            // first - so they read as normal buttons. Colouring a row of them
            // red made every bulk action look like a delete.
            if ui.button("全部99に")
                .on_hover_text("使っている枠の個数をすべて 99 にします（確認あり）")
                .clicked()
            {
                self.ask_fill_all();
            }
            if ui.button("重複を整理")
                .on_hover_text("同じアイテムの 2 枠目以降を 1 枠にまとめます。99 を超える分は失います（確認あり）")
                .clicked()
            {
                self.ask_dedupe();
            }
            if ui
                .button("空きを詰める")
                .on_hover_text("空き枠を消して前へ詰めます。並び順が変わります（確認あり）")
                .clicked()
            {
                self.ask_compact();
            }
            if ui.button("上限超えを丸める")
                .on_hover_text("99 を超えた個数を 99 に戻します（確認あり）")
                .clicked()
            {
                self.ask_clamp();
            }
            // Adding is the first thing anyone wants from an item editor, so
            // it gets its own button instead of "turn on empty slots and
            // scroll to the end".
            let can_add = !self.items.is_empty() && self.items.iter().any(|s| s.id == 0);
            if ui
                .add_enabled(can_add, egui::Button::new("アイテムを追加"))
                .on_hover_text("空きスロットに回復薬を入れ、選択状態にします")
                .clicked()
            {
                self.add_item_to_box(9);
            }
        });
        // CSV: the box as a table, and a table back into the box. Both go
        // through the same rfd file dialogs the rest of the window uses, so
        // there is no new dependency and no second way to pick a file.
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("CSV").strong()).on_hover_text(
                "アイテムボックスを表として持ち出し、その表から書き戻せます（ID と個数だけの 3 列）",
            );
            if ui
                .button("CSV 書き出し")
                .on_hover_text("このキャラのアイテムボックス 2300 枠を CSV にします（空き枠も 1 行ずつ）")
                .clicked()
            {
                let name = self.csv_suggested_name(CsvKind::Items);
                if let Some(p) = rfd::FileDialog::new()
                    .set_file_name(name)
                    .add_filter("CSV", &["csv"])
                    .set_title("アイテムボックスの CSV 書き出し")
                    .save_file()
                {
                    self.csv_write(CsvKind::Items, &p);
                }
            }
            if ui
                .button("CSV 読み込み")
                .on_hover_text("CSV の内容をこのキャラのボックスに書き戻します（内容を確認してから適用・確認あり）")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_title("アイテムボックスの CSV 読み込み")
                    .pick_file()
                {
                    self.csv_stage(CsvKind::Items, p);
                }
            }
            ui.label(
                egui::RichText::new("UTF-8 の CSV を読みます。適用の前に、何行が入るかを確認できます")
                    .weak()
                    .small(),
            );
        });
        if self.item_bulk_open {
            self.item_bulk_panel(ui);
        }

        // The detail panel sits above the table: the table fills whatever is
        // left, so anything below it ends up off the bottom of the window.
        self.item_detail_panel(ui);
        ui.separator();

        let filter = self.item_filter.to_lowercase();
        let show_empty = self.item_show_empty;
        let rows: Vec<usize> = (0..self.items.len())
            .filter(|&i| {
                let s = self.items[i];
                if !show_empty && s.id == 0 {
                    return false;
                }
                if filter.is_empty() {
                    return true;
                }
                let name = item_name(s.id).to_lowercase();
                name.contains(&filter) || format!("{}", s.id).contains(&filter)
            })
            .collect();

        ui.label(format!("{} 件", rows.len()));
        let mut selected = self.item_selected;
        // Scroll once to a row that was just added: the new row sits at the
        // end of two thousand, so without this nothing appears to happen.
        let scroll_target = self
            .scroll_to_item
            .take()
            .and_then(|t| rows.iter().position(|i| *i == t));
        let mut table = TableBuilder::new(ui)
            .striped(true)
            // Without this only the 20px box-number cell is clickable, which
            // nobody discovers.
            .sense(egui::Sense::click())
            .column(Column::exact(70.0))
            .column(Column::exact(80.0))
            .column(Column::remainder())
            .column(Column::exact(90.0));
        if let Some(pos) = scroll_target {
            table = table.scroll_to_row(pos, Some(egui::Align::Center));
        }
        table
            .header(22.0, |mut h| {
                h.col(|ui| {
                    ui.strong("箱 #");
                });
                h.col(|ui| {
                    ui.strong("ID");
                });
                h.col(|ui| {
                    ui.strong("名前");
                });
                h.col(|ui| {
                    ui.strong("個数");
                });
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let idx = rows[row.index()];
                    let (id, count) = (self.items[idx].id, self.items[idx].count);
                    row.col(|ui| {
                        if ui
                            .selectable_label(selected == Some(idx), format!("{}", idx + 1))
                            .clicked()
                        {
                            selected = Some(idx);
                        }
                    });
                    row.col(|ui| {
                        ui.label(format!("{}", id));
                    });
                    row.col(|ui| {
                        ui.label(item_name(id));
                    });
                    row.col(|ui| {
                        let mut c = count;
                        let r = ui.add(egui::DragValue::new(&mut c).range(0..=99));
                        // One snapshot per drag, not per pixel.
                        if r.drag_started() || (r.changed() && !r.dragged()) {
                            self.push_item_undo();
                        }
                        if r.changed() {
                            self.items[idx].count = c;
                            self.apply_to_save();
                        }
                    });
                    if row.response().clicked() {
                        selected = Some(idx);
                    }
                });
            });
        self.item_selected = selected;
    }

    /// The selected slot's editor, drawn above the table so the list can never
    /// push it off the screen.
    /// Remember the box before a change so it can be undone.
    fn push_item_undo(&mut self) {
        self.item_undo.push(self.items.clone());
        if self.item_undo.len() > 32 {
            self.item_undo.remove(0);
        }
    }

    /// Put the box back the way it was.
    fn undo_item(&mut self) {
        if let Some(prev) = self.item_undo.pop() {
            self.items = prev;
            self.apply_to_save();
            self.message = String::from("アイテムボックスを元に戻しました");
        } else {
            self.message = String::from("戻せる変更がありません");
        }
    }

    /// Put an item into the first empty box slot and select it for editing.
    ///
    /// Without this, adding an item meant turning on the empty slots, scrolling
    /// to the end of 2242 rows and typing an id: the box is append only in
    /// spirit, so the tool should do that for the user.
    pub fn add_item_to_box(&mut self, id: u16) {
        if self.items.is_empty() {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        }
        let slot = match self.items.iter().position(|s| s.id == 0) {
            Some(i) => i,
            None => {
                self.message = "アイテムボックスに空きがありません".to_owned();
                return;
            }
        };
        self.push_item_undo();
        self.items[slot] = ItemSlot { id, count: 1 };
        self.item_selected = Some(slot);
        self.item_show_empty = true;
        // The new row is at the end of 2300, so without this the user sees the
        // list jump nowhere and has no idea the item was added.
        self.scroll_to_item = Some(slot);
        self.apply_to_save();
        self.message = format!("{} を箱 {} に追加しました", item_name(id), slot + 1);
    }

    // ------------------------------------------------------- bulk operations

    /// Ask before setting every used record to the cap.
    pub fn ask_fill_all(&mut self) {
        if self.items.is_empty() {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        }
        let changed = self
            .items
            .iter()
            .filter(|s| !s.is_blank() && s.count != MAX_COUNT as u8)
            .count();
        if changed == 0 {
            self.message = "すべての枠が 99 個です（変わりません）".to_owned();
            return;
        }
        self.confirm = Confirm::ItemFillAll { changed };
    }

    /// Ask before merging stacks, saying what the merge would cost.
    pub fn ask_dedupe(&mut self) {
        if self.items.is_empty() {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        }
        let preview = itembox::dedupe_preview(&self.items);
        if preview.freed == 0 {
            self.message = "重複した枠はありません（変わりません）".to_owned();
            return;
        }
        self.confirm = Confirm::ItemDedupe {
            freed: preview.freed,
            lost: preview.lost,
        };
    }

    /// Ask before moving the used records to the front.
    pub fn ask_compact(&mut self) {
        if self.items.is_empty() {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        }
        // Counted on a copy: compact() is destructive and the number is what
        // the dialog needs before the user has agreed to anything.
        let mut copy = self.items.clone();
        let moved = itembox::compact(&mut copy);
        if moved == 0 {
            self.message = "空き枠は先頭に詰まっています（変わりません）".to_owned();
            return;
        }
        self.confirm = Confirm::ItemCompact { moved };
    }

    /// Ask before pulling counts over the cap back down.
    pub fn ask_clamp(&mut self) {
        if self.items.is_empty() {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        }
        let changed = self
            .items
            .iter()
            .filter(|s| !s.is_blank() && s.count > MAX_COUNT as u8)
            .count();
        if changed == 0 {
            self.message = "99 を超えた個数はありません（変わりません）".to_owned();
            return;
        }
        self.confirm = Confirm::ItemClamp { changed };
    }

    /// The line the dedupe confirmation puts above its buttons.
    ///
    /// The flag says whether that line has to be drawn in the warning colour:
    /// a merge that frees slots but takes items with it is not the harmless
    /// tidy-up the button looks like.
    fn dedupe_confirm_text(preview: DedupePreview) -> (String, bool) {
        (
            format!(
                "{} 枠を解放、{} 個を失います",
                preview.freed, preview.lost
            ),
            preview.lost > 0,
        )
    }

    /// What the status bar says after an add that did not fully fit.
    fn bulk_add_message(name: &str, wanted: u16, added: u32) -> String {
        let left = (wanted as u32).saturating_sub(added);
        if left == 0 {
            format!("{} を {} 個追加しました", name, added)
        } else if added == 0 {
            format!(
                "{} は入りませんでした（{} 個は入りきりませんでした）",
                name, left
            )
        } else {
            format!(
                "{} を {} 個追加しました（{} 個は入りきりませんでした）",
                name, added, left
            )
        }
    }

    /// Add a whole stack in one go, and report what did not fit.
    ///
    /// Returns how many actually landed. A request that fits nowhere leaves
    /// the box, the undo list and the dirty flag alone.
    pub fn add_items_bulk(&mut self, id: u16, count: u16) -> u32 {
        if self.items.is_empty() {
            self.message = "先にセーブを開いてください".to_owned();
            return 0;
        }
        if id == 0 || count == 0 {
            self.message = "アイテムと個数を指定してください".to_owned();
            return 0;
        }
        // A trial run on a copy: an add that fits nothing must not leave an
        // undo step behind or mark a save dirty for a change that never
        // happened.
        let mut trial = self.items.clone();
        let added = itembox::add(&mut trial, id, count);
        if added == 0 {
            self.message = Self::bulk_add_message(&item_name(id), count, 0);
            return 0;
        }
        self.push_item_undo();
        self.items = trial;
        self.item_show_empty = true;
        if let Some(slot) = self.items.iter().position(|s| s.id == id) {
            self.item_selected = Some(slot);
            self.scroll_to_item = Some(slot);
        }
        self.apply_to_save();
        self.message = Self::bulk_add_message(&item_name(id), count, added);
        added
    }

    /// Set every used record to the cap, after the dialog said yes.
    pub fn item_fill_all(&mut self) -> usize {
        if self.items.is_empty() {
            return 0;
        }
        self.push_item_undo();
        let changed = itembox::fill_all(&mut self.items, MAX_COUNT);
        self.apply_to_save();
        self.message = format!("{} 枠を 99 個にしました（元に戻せます）", changed);
        changed
    }

    /// Merge duplicate stacks, after the dialog said yes.
    pub fn item_dedupe(&mut self) -> DedupePreview {
        if self.items.is_empty() {
            return DedupePreview::default();
        }
        let preview = itembox::dedupe_preview(&self.items);
        self.push_item_undo();
        let freed = itembox::dedupe(&mut self.items);
        self.apply_to_save();
        self.message = if preview.lost == 0 {
            format!(
                "{} 枠を解放しました（失ったアイテムはありません）／元に戻せます",
                freed
            )
        } else {
            format!(
                "{} 枠を解放し、{} 個を失いました／元に戻せます",
                freed, preview.lost
            )
        };
        DedupePreview {
            freed,
            lost: preview.lost,
        }
    }

    /// Move the used records to the front, after the dialog said yes.
    pub fn item_compact(&mut self) -> usize {
        if self.items.is_empty() {
            return 0;
        }
        self.push_item_undo();
        let moved = itembox::compact(&mut self.items);
        self.item_selected = None;
        self.apply_to_save();
        self.message = format!("{} 枠を前に詰めました（元に戻せます）", moved);
        moved
    }

    /// Pull counts over the cap back down, after the dialog said yes.
    pub fn item_clamp(&mut self) -> usize {
        if self.items.is_empty() {
            return 0;
        }
        self.push_item_undo();
        let changed = itembox::clamp_counts(&mut self.items, MAX_COUNT);
        self.apply_to_save();
        self.message = format!("{} 枠の個数を 99 に丸めました（元に戻せます）", changed);
        changed
    }

    /// One row plus a picker, shown under the bulk buttons.
    fn item_bulk_panel(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("まとめて追加").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.item_bulk_filter)
                        .hint_text("アイテム名で検索")
                        .desired_width(200.0),
                );
                ui.label("ID");
                ui.add(
                    egui::DragValue::new(&mut self.item_bulk_id)
                        .range(0..=(LAST_ITEM_ID as i32))
                        .prefix("#"),
                );
                ui.label("個数");
                ui.add(
                    egui::DragValue::new(&mut self.item_bulk_count)
                        .speed(1.0)
                        .range(1..=MAX_COUNT),
                );
                let picked = if self.item_bulk_id == 0 {
                    String::from("未選択")
                } else {
                    item_name(self.item_bulk_id)
                };
                ui.label(picked);
                if ui
                    .add_enabled(self.item_bulk_id != 0, egui::Button::new("追加する"))
                    .on_hover_text("入りきらなかった数はメッセージに出ます")
                    .clicked()
                {
                    let (id, count) = (self.item_bulk_id, self.item_bulk_count);
                    self.add_items_bulk(id, count);
                }
                if ui.small_button("閉じる").clicked() {
                    self.item_bulk_open = false;
                }
            });
            let needle = self.item_bulk_filter.trim().to_lowercase();
            if needle.is_empty() {
                ui.label(
                    egui::RichText::new("名前で絞り込むか、ID を直接入力してください")
                        .weak()
                        .small(),
                );
                return;
            }
            let mut hits: Vec<(u16, &'static str)> = Vec::new();
            for (id, name) in mhnx_core::gamedata::ITEM_NAME_LIST.iter().enumerate() {
                if id == 0 || id > LAST_ITEM_ID {
                    continue;
                }
                if name.to_lowercase().contains(&needle) {
                    hits.push((id as u16, name));
                }
                if hits.len() >= 40 {
                    break;
                }
            }
            if hits.is_empty() {
                ui.label(egui::RichText::new("一致するアイテムがありません").weak());
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("itembulk")
                .max_height(120.0)
                .min_scrolled_height(120.0)
                .show(ui, |ui| {
                    for (id, name) in &hits {
                        if ui
                            .selectable_label(
                                self.item_bulk_id == *id,
                                format!("{:<6} {}", id, name),
                            )
                            .clicked()
                        {
                            self.item_bulk_id = *id;
                            self.item_show_empty = true;
                        }
                    }
                });
        });
    }
    fn item_detail_panel(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.item_selected else {
            ui.label(egui::RichText::new("行をクリックすると、ここで種類と個数を変更できます").weak());
            return;
        };
        if i >= self.items.len() {
            return;
        }
        // Everything below edits self.items; apply_to_save() pushes that into
        // the save. Forgetting it made the edits look like they did nothing.
        let mut touched = false;
        ui.horizontal(|ui| {
            ui.label(format!("選択中: 箱 #{} ", i + 1));
            let mut id = self.items[i].id;
            if ui
                .add(egui::DragValue::new(&mut id).range(0..=(LAST_ITEM_ID as i32)).prefix("ID "))
                .changed()
            {
                self.items[i].id = id;
                touched = true;
            }
            let mut c = self.items[i].count;
            if ui
                .add(egui::DragValue::new(&mut c).range(0..=99).prefix("個数 "))
                .changed()
            {
                self.items[i].count = c;
                touched = true;
            }
            if Self::danger_button(ui, "空にする").clicked() {
                self.push_item_undo();
                self.items[i] = ItemSlot::default();
                touched = true;
            }
        });

        // Pick a different item by name instead of typing a number.
        ui.horizontal(|ui| {
            ui.label("種類を変更");
            ui.add(
                egui::TextEdit::singleline(&mut self.item_pick_filter)
                    .hint_text("アイテム名で検索")
                    .desired_width(200.0),
            );
            if ui.small_button("クリア").clicked() {
                self.item_pick_filter.clear();
            }
            ui.checkbox(&mut self.item_pick_keep_filter, "選んだあとも絞り込みを残す")
                .on_hover_text("同じ名前のアイテムを続けて複数の枠へ入れやすくなります");
        });
        let needle = self.item_pick_filter.trim().to_lowercase();
        if needle.is_empty() {
            if touched {
                self.apply_to_save();
            }
            return;
        }
        let mut hits: Vec<(u16, &'static str)> = Vec::new();
        for (id, name) in mhnx_core::gamedata::ITEM_NAME_LIST.iter().enumerate() {
            if id == 0 || id > LAST_ITEM_ID {
                continue;
            }
            if name.to_lowercase().contains(&needle) {
                hits.push((id as u16, name));
            }
            // No cut-off: a search that stopped at forty hid matches for any
            // broad term, and the list below scrolls anyway.
            if hits.len() >= 400 {
                break;
            }
        }
        if hits.is_empty() {
            ui.label(egui::RichText::new("一致するアイテムがありません").weak());
            if touched {
                self.apply_to_save();
            }
            return;
        }
        // Fixed height: a block that grows with the hit count moved the table
        // under the pointer while typing.
        egui::ScrollArea::vertical()
            .id_salt("itempick")
            .max_height(160.0)
            .min_scrolled_height(160.0)
            .show(ui, |ui| {
                for (id, name) in &hits {
                    if ui
                        .selectable_label(self.items[i].id == *id, format!("{:<6} {}", id, name))
                        .clicked()
                    {
                        self.push_item_undo();
                        self.items[i].id = *id;
                        if self.items[i].count == 0 {
                            self.items[i].count = 1;
                        }
                        if !self.item_pick_keep_filter {
                            self.item_pick_filter.clear();
                        }
                        touched = true;
                    }
                }
            });
        if touched {
            self.apply_to_save();
            self.message = format!("箱 #{} を {} に変更しました", i + 1, item_name(self.items[i].id));
        }
    }

    fn tab_quest(&mut self, ui: &mut egui::Ui) {
        // The 249-byte hex grid and both 反映 buttons sit below the field
        // list; without a scroll they were off the bottom of the window.
        egui::ScrollArea::vertical()
            .id_salt("questtab")
            .show(ui, |ui| self.quest_tab_body(ui));
    }

    fn quest_tab_body(&mut self, ui: &mut egui::Ui) {
        if self.arc.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label("「.arc を開く」か、DLC タブのクエスト一覧で「編集」を押してください")
            });
            return;
        }
        // An archive pulled out of the save can go straight back into it.
        match self.arc_from_save {
            Some(id) => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("セーブ内のクエスト {} を編集中", id))
                            .strong()
                            .color(egui::Color32::from_rgb(0x6F, 0xC0, 0x6F)),
                    );
                    if ui
                        .button("セーブに書き戻す")
                        .on_hover_text("編集内容を再構築して、同じ ID の枠へ書き戻します")
                        .clicked()
                    {
                        self.write_arc_to_save();
                    }
                    if ui.small_button("切り離す").on_hover_text("セーブとの関連を切る").clicked() {
                        self.arc_from_save = None;
                    }
                });
            }
            None => {
                ui.label(
                    egui::RichText::new("ディスク上の .arc を編集中")
                        .weak()
                        .small(),
                );
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("絞り込み");
            ui.text_edit_singleline(&mut self.arc_filter);
            if ui.button("選択中の .arc を名前を付けて保存").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("quest archive", &["arc"])
                    .set_file_name("quest.arc")
                    .save_file()
                {
                    self.save_arc(p);
                }
            }
        });

        let filter = self.arc_filter.to_lowercase();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("エントリ").strong());
            ui.label(format!(
                "{} / {}",
                self.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0),
                self.arc_selected + 1
            ));
        });

        let entries: Vec<(usize, String, usize)> = self
            .arc
            .as_ref()
            .map(|a| {
                a.entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (i, e.file_name(), e.data.len()))
                    .collect()
            })
            .unwrap_or_default();
        let mut pending_arc: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("arclist")
            .max_height(180.0)
            .show(ui, |ui| {
                for (i, name, len) in &entries {
                    if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let label = format!("{:>3}  {:<52} {:>7} B", i, name, len);
                    if ui.selectable_label(self.arc_selected == *i, label).clicked() {
                        pending_arc = Some(*i);
                    }
                }
            });
        if let Some(i) = pending_arc {
            // No pre-assignment: select_arc_entry has to see the old index so
            // it can flush the edits made in the entry being left.
            self.select_arc_entry(i);
        }

        ui.separator();
        self.quest_check_panel(ui);
        ui.separator();
        if self.mib.is_some() {
            ui.label(egui::RichText::new("クエストパラメータ (.mib, 249 バイト固定)").strong());
            let qid = self.mib.as_ref().map(|m| m.quest_id()).unwrap_or(0);
            let parts = mhnx_core::quest::decode_quest_id(qid);
            ui.label(format!(
                "クエスト ID: {}  ({} / {} / #{})",
                qid,
                match parts.kind {
                    1 => "通常",
                    11 => "オトモ",
                    _ => "不明",
                },
                match parts.rank {
                    0 => "下位",
                    1 => "上位",
                    2 => "G級/訓練",
                    _ => "不明",
                },
                parts.serial
            ));
            ui.separator();

            ui.label(egui::RichText::new("フィールド (Quedixx 対応表)").strong());
            ui.label(
                egui::RichText::new("緑 = 確定 / 黄 = 推定。右端は Quedixx のコントロール名です。")
                    .weak()
                    .small(),
            );
            let mut new_id: Option<u32> = None;
            egui::ScrollArea::vertical()
                .id_salt("mibfields")
                .max_height(420.0)
                .show(ui, |ui| {
                    egui::Grid::new("mibf").num_columns(4).spacing([10.0, 3.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new("off").strong().monospace());
                        ui.label(egui::RichText::new("項目").strong());
                        ui.label(egui::RichText::new("値").strong());
                        ui.label(egui::RichText::new("Quedixx").strong());
                        ui.end_row();
                        for f in mhnx_core::questfields::MIB_FIELDS {
                            let ok = f.confidence == "confirmed";
                            let color = if ok {
                                egui::Color32::from_rgb(120, 220, 120)
                            } else {
                                egui::Color32::from_rgb(235, 190, 90)
                            };
                            ui.label(
                                egui::RichText::new(format!("{:02X}", f.offset))
                                    .monospace()
                                    .color(color),
                            );
                            ui.label(egui::RichText::new(f.ja).color(color));
                            let cur = mhnx_core::questfields::read_u32(
                                &self.mib_bytes, f.offset, f.kind,
                            )
                            .unwrap_or(0);
                            let table = f
                                .enum_name
                                .and_then(mhnx_core::questfields::enum_table);
                            if let Some(t) = table {
                                let shown = t.get(cur as usize).copied().unwrap_or("?");
                                egui::ComboBox::from_id_salt(("mibf", f.offset))
                                    .selected_text(format!("{} ({})", shown, cur))
                                    .width(220.0)
                                    .show_ui(ui, |ui| {
                                        for (i, name) in t.iter().enumerate() {
                                            if ui
                                                .selectable_label(
                                                    cur as usize == i,
                                                    format!("{} ({})", name, i),
                                                )
                                                .clicked()
                                            {
                                                mhnx_core::questfields::write_u32(
                                                    &mut self.mib_bytes,
                                                    f.offset,
                                                    f.kind,
                                                    i as u32,
                                                );
                                                if f.key == "quest_id" {
                                                    new_id = Some(i as u32);
                                                }
                                            }
                                        }
                                    });
                            } else if f.kind == FieldKind::Bytes {
                                // Byte blobs (the five monster slots and the
                                // rest) are not numbers: a 64 bit shift used to
                                // panic here in a debug build and render ten
                                // controls that could never write anything.
                                let mut s = String::new();
                                for b in self.mib_bytes.iter().skip(f.offset).take(f.size) {
                                    s.push_str(&format!("{:02X}", b));
                                }
                                ui.label(egui::RichText::new(s).monospace());
                                ui.label(
                                    egui::RichText::new("生バイト（下の 16 進表で編集）")
                                        .small()
                                        .weak(),
                                );
                                ui.end_row();
                                continue;
                            } else {
                                let width = ((f.size.min(8)) * 8) as u32;
                                let signed = matches!(
                                    f.kind,
                                    FieldKind::I8 | FieldKind::I16 | FieldKind::I32
                                );
                                let mut v: i64 = if signed {
                                    let shift = 64 - width;
                                    ((cur as u64) << shift) as i64 >> shift
                                } else {
                                    cur as i64
                                };
                                let lo = if signed { -(1i64 << (width - 1)) } else { 0 };
                                let hi = if signed {
                                    (1i64 << (width - 1)) - 1
                                } else {
                                    (1i64 << width) - 1
                                };
                                if width > 0
                                    && ui
                                        .add(
                                            egui::DragValue::new(&mut v)
                                                .range(lo..=hi)
                                                .speed(1),
                                        )
                                        .changed()
                                {
                                    mhnx_core::questfields::write_u32(
                                        &mut self.mib_bytes,
                                        f.offset,
                                        f.kind,
                                        (v as u64 & ((1u64 << width) - 1)) as u32,
                                    );
                                    if f.key == "quest_id" {
                                        new_id = Some(v as u32);
                                    }
                                }
                            }
                            ui.label(
                                egui::RichText::new(if f.ui.is_empty() { "-" } else { f.ui })
                                    .small()
                                    .weak(),
                            );
                            ui.end_row();
                        }
                    });
                });
            if let Some(id) = new_id {
                if let Some(m) = &mut self.mib {
                    m.set_quest_id(id);
                }
            }

            // A field edit only reaches mib_bytes; the archive still holds the
            // old payload until it is stored. Marking the save dirty without
            // storing it made the window say "unsaved" and then clear that
            // flag on save, because the saved bytes never carried the edit.
            if let Some(m) = &self.mib {
                if m.raw != self.mib_bytes {
                    self.dirty = true;
                }
            }

            // The button belongs next to the fields it commits: it used to sit
            // under the hex grid, which put it off the bottom of the window.
            if ui.button(".mib の変更を .arc に反映").clicked() {
                self.store_arc_entry(self.arc_selected);
                self.message = "反映しました".to_owned();
            }

            ui.add_space(6.0);
            ui.label(egui::RichText::new("全 249 バイト (16 進)").strong());
            ui.label(
                egui::RichText::new(
                    "未確定の領域は生バイトで直接編集できます。変更後は下の反映ボタンを押してください。",
                )
                .weak()
                .small(),
            );
            let n = self.mib_bytes.len();
            egui::ScrollArea::vertical()
                .id_salt("mibbytes")
                .max_height(300.0)
                .show(ui, |ui| {
                    egui::Grid::new("mibhex")
                        .num_columns(18)
                        .spacing([4.0, 2.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("off").strong().monospace());
                            for c in 0..16 {
                                ui.label(egui::RichText::new(format!("{:X}", c)).strong().monospace());
                            }
                            ui.label(egui::RichText::new("ascii").strong().monospace());
                            ui.end_row();
                            for base in (0..n).step_by(16) {
                                ui.label(
                                    egui::RichText::new(format!("{:04X}", base))
                                        .monospace()
                                        .weak(),
                                );
                                for c in 0..16 {
                                    let o = base + c;
                                    if o >= n {
                                        ui.label("");
                                        continue;
                                    }
                                    let mut b = self.mib_bytes[o] as i32;
                                    if ui
                                        .add(egui::DragValue::new(&mut b).range(0..=255).speed(1))
                                        .changed()
                                    {
                                        self.mib_bytes[o] = b as u8;
                                        // Editing the buffer is a real change:
                                        // without this the window still said
                                        // "no changes" and closing lost it.
                                        self.dirty = true;
                                    }
                                }
                                let txt: String = (0..16)
                                    .filter_map(|c| self.mib_bytes.get(base + c))
                                    .map(|b| {
                                        if *b >= 32 && *b < 127 {
                                            *b as char
                                        } else {
                                            '.'
                                        }
                                    })
                                    .collect();
                                ui.label(egui::RichText::new(txt).monospace().weak());
                                ui.end_row();
                            }
                        });
                });
        } else if self.gmd.is_some() {
            ui.label(egui::RichText::new("クエストテキスト (.gmd)").strong());
            let key = self.gmd.as_ref().map(|g| g.key.clone()).unwrap_or_default();
            ui.label(format!("キー: {}", key));
            ui.add_space(4.0);
            let labels = mhnx_core::quest::Gmd::LABELS;
            let n = self.gmd.as_ref().map(|g| g.strings.len()).unwrap_or(0);
            egui::ScrollArea::vertical()
                .id_salt("gmd")
                .max_height(360.0)
                .show(ui, |ui| {
                    if let Some(g) = &mut self.gmd {
                        for i in 0..n {
                            ui.label(labels.get(i).copied().unwrap_or("その他"));
                            let mut s = g.strings[i].clone();
                            if ui.text_edit_multiline(&mut s).changed() {
                                g.strings[i] = s;
                            }
                            ui.add_space(4.0);
                        }
                    }
                });
            if ui.button(".gmd の変更を .arc に反映").clicked() {
                self.store_arc_entry(self.arc_selected);
                self.message = "反映しました".to_owned();
            }
        } else {
            self.entry_panel(ui);
        }
    }

    /// Editors for .rem / .sup / .sem / .esl / .qdp / .qdl.
///
/// Every table here can be written back: seeds, counts, slots and the items
/// themselves. It said "read-only" long after that stopped being true.
    ///
    /// The layouts come from measuring real quests; the .rem and .sup record
    /// lists are editable, the rest are shown as they are.
    fn entry_panel(&mut self, ui: &mut egui::Ui) {
        use mhnx_core::entries::{Esl, ItemTable, Qdp, QuestLink, SemEntry};

        let Some(arc) = &self.arc else {
            return;
        };
        let Some(e) = arc.entries.get(self.arc_selected) else {
            return;
        };
        let ext = extension_for(e.type_hash).unwrap_or("?");
        let data = e.data.clone();
        ui.label(
            egui::RichText::new(format!(
                "{} ({} バイト, 型 {:#010x})",
                e.file_name(),
                data.len(),
                e.type_hash
            ))
            .strong(),
        );
        ui.add_space(4.0);

        match ext {
            "rem" | "sup" => {
                let mut table = match ItemTable::parse(&data) {
                    Ok(t) => t,
                    Err(err) => {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            format!("解析できません: {}", err),
                        );
                        return;
                    }
                };
                ui.label(format!(
                    "アイテム {} 件 / 収容 {} 件（{}）",
                    table.records.len(),
                    table.capacity,
                    if ext == "sup" { "支給品" } else { "報酬" }
                ));
                let mut changed = false;
                // Drop a selection left over from a bigger table: it matched
                // no row here, so neither the highlight nor the picker showed
                // and the table looked read-only again.
                let mut sel = self.entry_item_sel.filter(|i| *i < table.records.len());
                egui::ScrollArea::vertical()
                    .id_salt("itemtable")
                    .max_height(320.0)
                    .show(ui, |ui| {
                        egui::Grid::new("itab")
                            .striped(true)
                            .num_columns(4)
                            .show(ui, |ui| {
                                for h in ["#", "個数", "スロット", "アイテム名"] {
                                    ui.strong(h);
                                }
                                ui.end_row();
                                for (i, r) in table.records.iter_mut().enumerate() {
                                    if ui
                                        .selectable_label(
                                            sel == Some(i),
                                            format!("{}", i),
                                        )
                                        .clicked()
                                    {
                                        sel = Some(i);
                                    }
                                    let mut c = r.count as i32;
                                    if ui
                                        .add(egui::DragValue::new(&mut c).range(0..=99))
                                        .changed()
                                    {
                                        r.count = c as u8;
                                        changed = true;
                                    }
                                    let mut s = r.slot as i32;
                                    if ui
                                        .add(egui::DragValue::new(&mut s).range(0..=255))
                                        .changed()
                                    {
                                        r.slot = s as u8;
                                        changed = true;
                                    }
                                    // The name is the thing people click to
                                    // mean "change this one", so it selects the
                                    // row that the picker below acts on.
                                    if ui
                                        .selectable_label(
                                            sel == Some(i),
                                            format!("{:<6} {}", r.item, item_name(r.item)),
                                        )
                                        .clicked()
                                    {
                                        sel = Some(i);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                // Changing the item itself is the point of a reward table;
                // only the count and the slot were editable before.
                //
                // This whole block used to hang off "a row is selected", and
                // nothing said so, so the table looked like it could only edit
                // counts. It now always says what it does.
                self.entry_item_sel = sel;
                ui.add_space(4.0);
                if sel.is_none() {
                    ui.label(
                        egui::RichText::new(
                            "アイテムを差し替えるには、上の一覧でアイテム名を押してください",
                        )
                        .weak()
                        .small(),
                    );
                }
                if let Some(i) = sel {
                    if i < table.records.len() {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} 行目のアイテムを変更",
                                i
                            ));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.item_pick_filter)
                                    .hint_text("アイテム名で検索")
                                    .desired_width(180.0),
                            );
                            if ui.small_button("クリア").clicked() {
                                self.item_pick_filter.clear();
                            }
                        });
                        let needle = self.item_pick_filter.trim().to_lowercase();
                        if !needle.is_empty() {
                            let mut hits: Vec<(u16, &'static str)> = Vec::new();
                            for (id, name) in
                                mhnx_core::gamedata::ITEM_NAME_LIST.iter().enumerate()
                            {
                                if id == 0 || id > LAST_ITEM_ID {
                                    continue;
                                }
                                if name.to_lowercase().contains(&needle) {
                                    hits.push((id as u16, name));
                                }
                                if hits.len() >= 400 {
                                    break;
                                }
                            }
                            if hits.is_empty() {
                                ui.label(
                                    egui::RichText::new("その名前のアイテムはありません")
                                        .weak()
                                        .small(),
                                );
                            }
                            egui::ScrollArea::vertical()
                                .id_salt("entryitempick")
                                .max_height(140.0)
                                .min_scrolled_height(140.0)
                                .show(ui, |ui| {
                                    for (id, name) in &hits {
                                        if ui
                                            .selectable_label(
                                                table.records[i].item == *id,
                                                format!("{:<6} {}", id, name),
                                            )
                                            .clicked()
                                        {
                                            table.records[i].item = *id;
                                            if table.records[i].count == 0 {
                                                table.records[i].count = 1;
                                            }
                                            changed = true;
                                            self.item_pick_filter.clear();
                                        }
                                    }
                                });
                        }
                    }
                }
                if changed {
                    if let Some(a) = &mut self.arc {
                        if let Some(e) = a.entries.get_mut(self.arc_selected) {
                            e.data = table.to_bytes();
                        }
                    }
                    self.message =
                        format!("{} を更新しました（反映ボタンで .arc へ）", ext);
                }
                ui.add_space(4.0);
                if ui
                    .button(format!(".{} の変更を .arc に反映", ext))
                    .clicked()
                {
                    self.store_arc_entry(self.arc_selected);
                    self.message = "反映しました".to_owned();
                }
            }
            "sem" => {
                ui.label("モンスター配置 (.sem, 32 バイト単位)");
                egui::Grid::new("semg")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        for h in ["#", "位置 (x, y, z)", "有効"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for (i, c) in data.chunks_exact(32).enumerate() {
                            let s = SemEntry { raw: c.to_vec() };
                            let (x, y, z, _) = s.position();
                            ui.label(format!("{}", i));
                            ui.label(format!("{:.1}, {:.1}, {:.1}", x, y, z));
                            ui.label(if s.is_dummy() { "ダミー" } else { "実体" });
                            ui.end_row();
                        }
                    });
            }
            "esl" => match Esl::parse(&data) {
                Ok(l) => {
                    ui.label(format!("小モンスター出現 (.esl) / ウェーブ数 {}", l.waves));
                    ui.label(
                        egui::RichText::new("中身は未解読です（サイズはクエストにより可変）")
                            .weak(),
                    );
                }
                Err(err) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("解析できません: {}", err));
                }
            },
            "qdp" => match Qdp::parse(&data) {
                Ok(q) => {
                    ui.label("クエスト追加フラグ (.qdp)");
                    let hex: String = q.flags().iter().map(|b| format!("{:02X}", b)).collect();
                    ui.label(format!("ヘッダ後のフラグ: {}", hex));
                }
                Err(err) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("解析できません: {}", err));
                }
            },
            "qdl" => match QuestLink::parse(&data) {
                Ok(q) => {
                    ui.label(format!("クエスト構成ファイル一覧 (.qdl) / {} 件", q.refs.len()));
                    egui::Grid::new("qdlg")
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for h in ["型", "名前"] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            for r in &q.refs {
                                ui.label(extension_for(r.type_hash).unwrap_or("?"));
                                ui.label(&r.name);
                                ui.end_row();
                            }
                        });
                }
                Err(err) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("解析できません: {}", err));
                }
            },
            other => {
                ui.label(format!("このエントリ型 (.{}) は未対応です", other));
            }
        }
    }

    /// Refresh the checker findings for the open archive.
    pub fn run_quest_check(&mut self) {
        self.findings = match &self.arc {
            Some(a) => check::check_quest(a),
            None => Vec::new(),
        };
    }

    /// Run the quest checker on the open archive and list what it found.
    fn quest_check_panel(&mut self, ui: &mut egui::Ui) {
        if ui.button("このクエストをチェック").clicked() {
            self.findings = match &self.arc {
                Some(a) => check::check_quest(a),
                None => Vec::new(),
            };
            self.message = format!("{} 件の指摘", self.findings.len());
        }
        // Cloned so the fix below can take &mut self while the list is shown.
        let findings = self.findings.clone();
        let mut want_fix = false;
        for f in &findings {
            let (color, tag) = match f.severity {
                Severity::Error => (egui::Color32::from_rgb(235, 120, 120), "エラー"),
                Severity::Warn => (egui::Color32::from_rgb(235, 190, 90), "注意"),
                Severity::Info => (egui::Color32::from_rgb(150, 190, 235), "情報"),
            };
            let at = match &f.location {
                Some((name, off)) => format!(" ({} +0x{:X})", name, off),
                None => String::new(),
            };
            let mut fix_this = false;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("[{}] {}{}", tag, f.message, at)).color(color),
                );
                // README promised a one-click fix for the 1人限定 problem and
                // only the console ever had it.
                if f.fixable && ui.small_button("修正").clicked() {
                    fix_this = true;
                }
            });
            if fix_this {
                want_fix = true;
            }
        }
        if want_fix {
            {
                let before = self.arc.as_mut().and_then(|a| {
                    check::clear_one_player_only(a, 0x00)
                });
                match before {
                    Some(old) => {
                        // The fix edits the archive entry itself. Storing the
                        // working buffer here put the pre-fix byte straight
                        // back over it, so the fix undid itself: read the
                        // entry back instead of writing the buffer out.
                        self.load_arc_entry(self.arc_selected);
                        self.run_quest_check();
                        self.message = format!(
                            "参加条件1 を 0x{:02X} から 0x00 に直しました（.arc へ反映してください）",
                            old
                        );
                    }
                    None => {
                        self.message = String::from("この指摘は自動で直せません");
                    }
                }
            }
        }
    }

    /// The title carried by one quest payload.
    fn title_of(data: &[u8]) -> String {
        mhnx_core::arc::Arc::parse(data)
            .ok()
            .and_then(|a| {
                a.by_ext("gmd")
                    .and_then(|e| mhnx_core::quest::Gmd::from_bytes(&e.data).ok())
            })
            .and_then(|g| g.title().map(|t| t.to_string()))
            .unwrap_or_default()
    }

    /// Titles for a list of quests, built in one pass.
    ///
    /// This used to be called per row and each call re-listed and re-parsed
    /// every quest in the save, so a 175 quest save cost thousands of parses a
    /// frame.
    fn quest_titles(quests: &[dlc::InstalledQuest]) -> std::collections::HashMap<i32, String> {
        quests
            .iter()
            .map(|q| (q.quest_id, Self::title_of(&q.data)))
            .collect()
    }

    /// マイセット: the item sets, then the forty saved equipment sets.
    ///
    /// The item sets are a plain list with a name field. The equipment sets get
    /// a list and an editor, because the core module already knows how to move
    /// a part: the index at +0x2A and the decoration copy at +0x38 have to
    /// change together, and writing only the index is what leaves a set
    /// describing decorations its equipment does not have.
    fn tab_sets(&mut self, ui: &mut egui::Ui) {
        ui.heading("マイセット");
        if self.save.is_none() {
            ui.label("セーブを開くと、アイテムセットと装備セットを表示します");
            return;
        }
        // The width this tab may use, taken before anything wide is drawn: the
        // outer scroll area takes its content width from the widest child, and
        // one long line of items used to stretch it until the equipment list
        // and its editor were pushed off the right edge.
        let mut width = ui.available_width();
        if !width.is_finite() || width <= 0.0 {
            width = 900.0;
        }
        ui.label(egui::RichText::new("アイテムセット").strong());
        self.item_set_tab(ui, width);
        ui.separator();
        ui.label(egui::RichText::new("ポーチ").strong());
        self.pouch_panel(ui, width);
        ui.separator();
        ui.label(egui::RichText::new("装備セット").strong());
        self.equip_set_tab(ui, width);
    }

    /// The twenty-four item sets: a list on the left, one editor on the
    /// right.
    ///
    /// The list used to be a column of names with a line of item names under
    /// each; it now edits as well, because a set is only useful if its
    /// thirty-two entries can be changed. The height is fixed for the same
    /// reason the equipment sets use one: this tab already sits in the outer
    /// scroll area.
    fn item_set_tab(&mut self, ui: &mut egui::Ui, width: f32) {
        let slot = self.slot;
        let Some(sets) = self
            .save
            .as_ref()
            .map(|s| mhnx_core::myset::item_sets(s, slot))
        else {
            return;
        };
        // One pass over the twenty-four records, so the list itself says which
        // ones have something to report.
        let warn_counts: Vec<usize> = self
            .save
            .as_ref()
            .map(|s| {
                sets.iter()
                    .map(|set| mhnx_core::myset::item_set_warnings(s, slot, set.index).len())
                    .collect()
            })
            .unwrap_or_default();
        // The same yellow the equipment set editor uses, so the two read as one
        // signal.
        let warn_colour = egui::Color32::from_rgb(240, 199, 94);
        // Land on something editable the first time the tab is opened: an empty
        // panel on the right looks like the tab has no editor at all.
        if !self.item_set_selected.is_some_and(|i| i < sets.len()) {
            self.item_set_selected = sets
                .iter()
                .find(|s| !s.is_empty())
                .map(|s| s.index)
                .or(Some(0));
        }
        const PANEL_W: f32 = 460.0;
        const BLOCK_H: f32 = 300.0;
        let mut pick: Option<usize> = None;
        let selected = self.item_set_selected;
        ui.allocate_ui_with_layout(
            egui::vec2(width, BLOCK_H),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.horizontal_top(|ui| {
                    let table_w = (width - PANEL_W - 16.0).max(250.0);
                    ui.vertical(|ui| {
                        ui.set_width(table_w);
                        ui.set_height(ui.available_height());
                        TableBuilder::new(ui)
                            .id_salt("itemsets")
                            .striped(true)
                            .sense(egui::Sense::click())
                            .column(Column::exact(36.0))
                            .column(Column::remainder())
                            .column(Column::exact(52.0))
                            .column(Column::exact(44.0))
                            .header(22.0, |mut h| {
                                for t in ["枠", "名前", "中身", "注意"] {
                                    h.col(|ui| {
                                        ui.strong(t);
                                    });
                                }
                            })
                            .body(|body| {
                                body.rows(20.0, sets.len(), |mut row| {
                                    let set = &sets[row.index()];
                                    row.set_selected(selected == Some(set.index));
                                    row.col(|ui| {
                                        ui.label(format!("{}", set.index));
                                    });
                                    row.col(|ui| {
                                        if set.name.trim().is_empty()
                                            || set.name == mhnx_core::myset::EMPTY_NAME
                                        {
                                            ui.label(egui::RichText::new("（名前なし）").weak());
                                        } else {
                                            ui.label(set.name.clone());
                                        }
                                    });
                                    row.col(|ui| {
                                        let n = set.items.iter().filter(|i| i.id != 0).count();
                                        if n == 0 {
                                            ui.label(egui::RichText::new("0").weak());
                                        } else {
                                            ui.label(format!("{}", n));
                                        }
                                    });
                                    row.col(|ui| {
                                        let n = warn_counts.get(set.index).copied().unwrap_or(0);
                                        if n == 0 {
                                            ui.label(egui::RichText::new("—").weak());
                                        } else {
                                            ui.label(
                                                egui::RichText::new(format!("{}", n))
                                                    .color(warn_colour),
                                            );
                                        }
                                    });
                                    if row.response().clicked() {
                                        pick = Some(set.index);
                                    }
                                });
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(PANEL_W);
                        ui.set_max_width(PANEL_W + 60.0);
                        egui::ScrollArea::vertical()
                            .id_salt("itemsetedit")
                            .max_height(BLOCK_H - 8.0)
                            .show(ui, |ui| match pick.or(selected) {
                                Some(index) => self.item_set_edit_panel(ui, index, &sets),
                                None => {
                                    ui.label(
                                        egui::RichText::new(
                                            "行をクリックすると、ここで編集できます",
                                        )
                                        .weak(),
                                    );
                                }
                            });
                    });
                });
            },
        );
        if let Some(p) = pick {
            self.item_set_selected = Some(p);
        }
    }

    /// The editor for one item set: the name, the pouch buttons, the warnings
    /// and the thirty-two entries.
    ///
    /// Every write goes through myset: set_item for one entry, clear_item for
    /// emptying one, copy_from_pouch and apply_to_pouch for the two whole-pouch
    /// moves. Nothing here touches the 170 byte record by hand.
    fn item_set_edit_panel(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        sets: &[mhnx_core::myset::ItemSet],
    ) {
        let Some(set) = sets.iter().find(|s| s.index == index).cloned() else {
            ui.label("そのセットは読めません");
            return;
        };
        let slot = self.slot;
        // Reported, not repaired: a save that came from the game can hold each
        // of these. See myset::item_set_warnings.
        let warns = self
            .save
            .as_ref()
            .map(|s| mhnx_core::myset::item_set_warnings(s, slot, index))
            .unwrap_or_default();
        let warn_colour = egui::Color32::from_rgb(240, 199, 94);
        ui.label(egui::RichText::new(format!("セット {} の編集", index)).strong());
        ui.horizontal(|ui| {
            ui.label("名前");
            let mut name = set.name.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(230.0)
                        .hint_text("セット名"),
                )
                .changed()
            {
                self.item_set_name_write(index, &name);
            }
        });
        ui.add_space(4.0);
        if warns.is_empty() {
            ui.label(egui::RichText::new("警告はありません").weak().small());
        } else {
            ui.label(
                egui::RichText::new(format!(
                    "警告 {} 件（保存はできます。直すかどうかは任せます）",
                    warns.len()
                ))
                .color(warn_colour)
                .strong(),
            );
            for w in &warns {
                ui.label(egui::RichText::new(format!("・{}", w)).color(warn_colour));
            }
        }
        ui.add_space(2.0);
        // Both of these write over something - the set one way and the pouch
        // the other - so both only arm the confirmation dialog.
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("ポーチから取り込む")
                .on_hover_text(
                    "今のポーチ 32 枠をこのセットに写します（セットの中身は置き換わります・確認あり）",
                )
                .clicked()
            {
                self.confirm = Confirm::SetFromPouch(index);
            }
            if ui
                .button("ポーチへ適用")
                .on_hover_text(
                    "このセットの 32 枠をポーチに写します（今のポーチは置き換わります・確認あり）",
                )
                .clicked()
            {
                self.confirm = Confirm::SetToPouch(index);
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new("中身（32 枠）").strong())
            .on_hover_text("ID 0 は空きです。個数は 99 まで。ID を 0 にすると空きに戻ります。");
        let mut action: Option<(usize, u16, u16)> = None;
        egui::ScrollArea::vertical()
            .id_salt(("itemsetitems", index))
            .max_height(180.0)
            .show(ui, |ui| {
                egui::Grid::new(("itemsetgrid", index))
                    .num_columns(5)
                    .spacing([6.0, 2.0])
                    .show(ui, |ui| {
                        for t in ["#", "ID", "名前", "個数", ""] {
                            ui.label(egui::RichText::new(t).weak().small());
                        }
                        ui.end_row();
                        for entry in 0..set.items.len() {
                            let item = set.items[entry];
                            ui.label(format!("{}", entry));
                            let mut id = item.id;
                            let mut count = item.count;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut id)
                                        .range(0..=LAST_ITEM_ID as u16)
                                        .speed(1.0),
                                )
                                .changed()
                            {
                                action = Some((entry, id, if id == 0 { 0 } else { count }));
                            }
                            if id == 0 {
                                ui.label(egui::RichText::new("（空き）").weak());
                            } else {
                                ui.label(item_name(id));
                            }
                            if ui
                                .add(egui::DragValue::new(&mut count).range(0..=MAX_COUNT))
                                .changed()
                                && id != 0
                            {
                                action = Some((entry, id, count));
                            }
                            if ui
                                .add_enabled(id != 0, egui::Button::new("解除"))
                                .on_hover_text("この枠を空きに戻します（ID と個数が 0 になります）")
                                .clicked()
                            {
                                action = Some((entry, 0, 0));
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some((entry, id, count)) = action {
            self.item_set_entry_write(index, entry, id, count);
        }
    }

    /// The pouch: the same thirty-two entries an item set holds, as the
    /// character carries them into a quest.
    ///
    /// Editing here writes the pouch; the set side can write it too (ポーチへ
    /// 適用), which is why the confirmation there keeps a copy. Both roads end
    /// in the same 76 bytes at OFF_POUCH.
    fn pouch_panel(&mut self, ui: &mut egui::Ui, width: f32) {
        let slot = self.slot;
        let Some(entries) = self
            .save
            .as_ref()
            .map(|s| mhnx_core::myset::pouch(s, slot))
        else {
            return;
        };
        if entries.len() != mhnx_core::myset::POUCH_SLOTS {
            ui.label(
                egui::RichText::new(format!(
                    "枠 {} のポーチは読めません（32 枠ぶんの領域がありません）",
                    slot + 1
                ))
                .color(egui::Color32::from_rgb(229, 72, 77)),
            );
            return;
        }
        let used = entries.iter().filter(|i| i.id != 0).count();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} / {} 枠を使用中",
                used,
                mhnx_core::myset::POUCH_SLOTS
            ));
            if ui
                .add_enabled(self.pouch_undo.is_some(), egui::Button::new("元に戻す"))
                .on_hover_text("ポーチへ適用 の直前に控えたポーチへ戻します")
                .clicked()
            {
                self.pouch_restore();
            }
            ui.label(
                egui::RichText::new(
                    "ここはクエストに持ち込む 32 枠そのものです。「ポーチへ適用」はこの欄を丸ごと置き換えます",
                )
                .weak()
                .small(),
            );
        });
        let mut action: Option<(usize, u16, u16)> = None;
        ui.set_max_width(width);
        egui::ScrollArea::vertical()
            .id_salt("pouchedit")
            .max_height(165.0)
            .show(ui, |ui| {
                egui::Grid::new("pouchgrid")
                    .num_columns(5)
                    .spacing([6.0, 2.0])
                    .show(ui, |ui| {
                        for t in ["#", "ID", "名前", "個数", ""] {
                            ui.label(egui::RichText::new(t).weak().small());
                        }
                        ui.end_row();
                        for (entry, item) in entries.iter().enumerate() {
                            ui.label(format!("{}", entry));
                            let mut id = item.id;
                            let mut count = item.count;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut id)
                                        .range(0..=LAST_ITEM_ID as u16)
                                        .speed(1.0),
                                )
                                .changed()
                            {
                                action = Some((entry, id, if id == 0 { 0 } else { count }));
                            }
                            if id == 0 {
                                ui.label(egui::RichText::new("（空き）").weak());
                            } else {
                                ui.label(item_name(id));
                            }
                            if ui
                                .add(egui::DragValue::new(&mut count).range(0..=MAX_COUNT))
                                .changed()
                                && id != 0
                            {
                                action = Some((entry, id, count));
                            }
                            if ui
                                .add_enabled(id != 0, egui::Button::new("解除"))
                                .on_hover_text("この枠を空きに戻します")
                                .clicked()
                            {
                                action = Some((entry, 0, 0));
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some((entry, id, count)) = action {
            self.pouch_entry_write(entry, id, count);
        }
    }

    /// The forty saved equipment sets: a list on the left, one editor on the
    /// right. The height is fixed rather than taken from the frame, because
    /// this tab already sits inside the outer scroll area and an unbounded
    /// table there would run to the length of the whole list.
    fn equip_set_tab(&mut self, ui: &mut egui::Ui, width: f32) {
        let slot = self.slot;
        let Some(sets) = self
            .save
            .as_ref()
            .map(|s| mhnx_core::myset::equip_sets(s, slot))
        else {
            return;
        };
        // Copied out once per frame, as the equipment tab does it: the part
        // pickers read the box by name and the warnings compare the set's
        // decoration copy against it, and holding &self.save while the editor
        // writes would lock what it is writing.
        let boxv: Vec<u8> = self
            .save
            .as_ref()
            .and_then(|s| s.equipment_box(slot))
            .map(|b| b.to_vec())
            .unwrap_or_default();
        let used: Vec<mhnx_core::equip::Equipment> = mhnx_core::equip::decode_box(&boxv)
            .into_iter()
            .filter(|e| !e.is_empty())
            .collect();
        // One pass over the forty records, so the list itself says which sets
        // have something to report rather than leaving it to the editor.
        let warn_counts: Vec<usize> = self
            .save
            .as_ref()
            .map(|s| {
                sets.iter()
                    .map(|set| mhnx_core::myset::warnings(s, slot, set.index).len())
                    .collect()
            })
            .unwrap_or_default();
        // The same yellow the editor uses, so the two read as one signal.
        let warn_colour = egui::Color32::from_rgb(240, 199, 94);
        // Land on something editable the first time the tab is opened: an empty
        // panel on the right looks like the tab has no editor at all.
        if !self.set_selected.is_some_and(|i| i < sets.len()) {
            self.set_selected = sets
                .iter()
                .find(|s| !s.is_empty())
                .map(|s| s.index)
                .or(Some(0));
        }
        const PANEL_W: f32 = 430.0;
        const BLOCK_H: f32 = 430.0;
        let mut pick: Option<usize> = None;
        ui.allocate_ui_with_layout(
            egui::vec2(width, BLOCK_H),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.horizontal_top(|ui| {
                    let table_w = (width - PANEL_W - 16.0).max(260.0);
                    ui.vertical(|ui| {
                        ui.set_width(table_w);
                        ui.set_height(ui.available_height());
                        TableBuilder::new(ui)
                            .id_salt("equipsets")
                            .striped(true)
                            .sense(egui::Sense::click())
                            .column(Column::exact(36.0))
                            .column(Column::remainder())
                            .column(Column::exact(46.0))
                            .column(Column::exact(40.0))
                            .header(22.0, |mut h| {
                                for t in ["枠", "名前", "部位", "注意"] {
                                    h.col(|ui| {
                                        ui.strong(t);
                                    });
                                }
                            })
                            .body(|body| {
                                body.rows(20.0, sets.len(), |mut row| {
                                    let set = &sets[row.index()];
                                    row.set_selected(pick.or(self.set_selected) == Some(set.index));
                                    row.col(|ui| {
                                        ui.label(format!("{}", set.index));
                                    });
                                    row.col(|ui| {
                                        if set.is_empty() {
                                            ui.label(egui::RichText::new("（未使用）").weak());
                                        } else if set.name.trim().is_empty() {
                                            ui.label(egui::RichText::new("（名前なし）").weak());
                                        } else {
                                            ui.label(set.name.clone());
                                        }
                                    });
                                    row.col(|ui| {
                                        let filled = set
                                            .parts
                                            .iter()
                                            .filter(|p| **p != mhnx_core::myset::NONE)
                                            .count();
                                        let text = format!(
                                            "{}/{}",
                                            filled,
                                            mhnx_core::myset::EQUIP_SET_PARTS
                                        );
                                        if filled == 0 {
                                            ui.label(egui::RichText::new(text).weak());
                                        } else {
                                            ui.label(text);
                                        }
                                    });
                                    row.col(|ui| {
                                        let n = warn_counts.get(set.index).copied().unwrap_or(0);
                                        if n == 0 {
                                            ui.label(egui::RichText::new("—").weak());
                                        } else {
                                            ui.label(
                                                egui::RichText::new(format!("{}", n))
                                                    .color(warn_colour),
                                            );
                                        }
                                    });
                                    if row.response().clicked() {
                                        pick = Some(set.index);
                                    }
                                });
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(PANEL_W);
                        ui.set_max_width(PANEL_W + 60.0);
                        egui::ScrollArea::vertical()
                            .id_salt("setedit")
                            .max_height(BLOCK_H - 8.0)
                            .show(ui, |ui| match pick.or(self.set_selected) {
                                Some(index) => self.set_edit_panel(ui, index, &sets, &used),
                                None => {
                                    ui.label(
                                        egui::RichText::new("行をクリックすると、ここで編集できます")
                                            .weak(),
                                    );
                                }
                            });
                    });
                });
            },
        );
        if let Some(p) = pick {
            self.set_selected = Some(p);
        }
    }

    /// The editor for one saved equipment set.
    ///
    /// Every write goes through the myset helpers: a part moves the index and
    /// the decoration copy in one call, a colour through set_colour, the name
    /// through set_name, and the whole record through clear_set. Nothing here
    /// touches the 136 bytes by hand.
    fn set_edit_panel(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        sets: &[mhnx_core::myset::EquipSet],
        used: &[mhnx_core::equip::Equipment],
    ) {
        let Some(set) = sets.iter().find(|s| s.index == index).cloned() else {
            ui.label("そのセットは読めません");
            return;
        };
        let slot = self.slot;
        // Reported, not repaired: the game loads every one of these, and the
        // real save holds examples of each. See myset::warnings.
        let warns = self
            .save
            .as_ref()
            .map(|s| mhnx_core::myset::warnings(s, slot, index))
            .unwrap_or_default();
        let warn_colour = egui::Color32::from_rgb(240, 199, 94);
        ui.label(egui::RichText::new(format!("セット {} の編集", index)).strong());
        ui.horizontal(|ui| {
            ui.label("名前");
            let mut name = set.name.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(250.0)
                        .hint_text("セット名"),
                )
                .changed()
            {
                self.set_name_write(index, &name);
            }
        });
        // The warnings sit directly under the name, where they are seen before
        // the seven pickers rather than after scrolling past them.
        ui.add_space(4.0);
        if warns.is_empty() {
            ui.label(egui::RichText::new("警告はありません").weak().small());
        } else {
            ui.label(
                egui::RichText::new(format!(
                    "警告 {} 件（保存はできます。直すかどうかは任せます）",
                    warns.len()
                ))
                .color(warn_colour)
                .strong(),
            );
            for w in &warns {
                ui.label(egui::RichText::new(format!("・{}", w)).color(warn_colour));
            }
        }
        ui.add_space(2.0);
        ui.label(egui::RichText::new("部位").strong())
            .on_hover_text(
                "部位と種別が合わなくても書き込めます（実データに頭へ護石が入ったセットがあります）。\n気になる点は下の警告に出ます。",
            );
        let mut action: Option<(usize, u16)> = None;
        for part in 0..mhnx_core::myset::EQUIP_SET_PARTS {
            let current = set.parts[part];
            let found = used.iter().find(|e| e.index as u16 == current);
            let shown = match (found, current) {
                (Some(e), _) => format!(
                    "{} {} 枠{}",
                    mhnx_core::equip::type_name(e.kind),
                    mhnx_core::equip::name(e),
                    current
                ),
                (None, mhnx_core::myset::NONE) => String::from("（なし）"),
                (None, idx) => format!("枠 {} （読めません）", idx),
            };
            ui.horizontal(|ui| {
                ui.add_sized(
                    [40.0, 20.0],
                    egui::Label::new(mhnx_core::myset::PART_NAMES[part]),
                );
                let mut picked: Option<u16> = None;
                egui::ComboBox::from_id_salt(("setpart", part))
                    .selected_text(shown)
                    .width(250.0)
                    .show_ui(ui, |ui| {
                        let search = &mut self.set_part_search[part];
                        ui.add(
                            egui::TextEdit::singleline(search)
                                .hint_text("装備名・種別・枠番号で絞り込み")
                                .desired_width(250.0),
                        );
                        let needle = search.trim().to_lowercase();
                        egui::ScrollArea::vertical()
                            .id_salt(("setpartlist", part))
                            .max_height(260.0)
                            .show(ui, |ui| {
                                for e in used.iter() {
                                    if !needle.is_empty()
                                        && !mhnx_core::equip::name(e)
                                            .to_lowercase()
                                            .contains(&needle)
                                        && !mhnx_core::equip::type_name(e.kind)
                                            .to_lowercase()
                                            .contains(&needle)
                                        && !format!("{}", e.index).contains(&needle)
                                    {
                                        continue;
                                    }
                                    let label = format!(
                                        "枠 {} {} {} Lv{}",
                                        e.index,
                                        mhnx_core::equip::type_name(e.kind),
                                        mhnx_core::equip::name(e),
                                        e.level
                                    );
                                    if ui
                                        .selectable_label(current == e.index as u16, label)
                                        .clicked()
                                    {
                                        picked = Some(e.index as u16);
                                    }
                                }
                            });
                    });
                if ui
                    .add_enabled(
                        current != mhnx_core::myset::NONE,
                        egui::Button::new("解除"),
                    )
                    .on_hover_text("この部位を空にします（装飾品の写しも 0 になります）")
                    .clicked()
                {
                    picked = Some(mhnx_core::myset::NONE);
                }
                if let Some(p) = picked {
                    action = Some((part, p));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.add_space(44.0);
                let detail = match (found, current) {
                    (Some(e), _) => format!(
                        "{}（種別 {}）Lv{}",
                        mhnx_core::equip::name(e),
                        e.kind,
                        e.level
                    ),
                    (None, mhnx_core::myset::NONE) => String::from("装備なし"),
                    (None, idx) => format!("枠 {} は装備ボックスにありません", idx),
                };
                ui.label(egui::RichText::new(detail).weak().small());
                // The set's own copy at +0x38, which is what a mismatch in the
                // warnings below is measured against. Shown only when it holds
                // something: seven lines of "—・—・—" is noise, and the empty
                // case is what the 解除 button beside it already says.
                let d = set.decos_of(part).unwrap_or([0; 3]);
                if d.iter().any(|x| *x != 0) {
                    ui.label(
                        egui::RichText::new(format!(
                            "セット側の装飾品 {}・{}・{}",
                            deco_label(d[0]),
                            deco_label(d[1]),
                            deco_label(d[2])
                        ))
                        .weak()
                        .small(),
                    );
                }
            });
            ui.add_space(1.0);
        }
        if let Some((part, box_index)) = action {
            if box_index == mhnx_core::myset::NONE {
                self.set_part_clear(index, part);
            } else {
                self.set_part_choose(index, part, box_index);
            }
            self.set_part_search[part].clear();
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new("防具の色").strong());
        let mut colour_action: Option<(usize, [u8; 4])> = None;
        egui::Grid::new(("setcolour", index))
            .num_columns(2)
            .spacing([6.0, 3.0])
            .show(ui, |ui| {
                for part in 1..=5usize {
                    ui.label(mhnx_core::myset::PART_NAMES[part]);
                    // The game writes alpha 255. The swatch is shown at full
                    // alpha so a colour left at zero is still visible, and the
                    // write puts the alpha back the way the game leaves it.
                    let c = set.colours[part - 1];
                    let mut rgb = [c[0], c[1], c[2]];
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        colour_action = Some((part, [rgb[0], rgb[1], rgb[2], 0xFF]));
                    }
                    ui.end_row();
                }
            });
        if let Some((part, colour)) = colour_action {
            self.set_colour_write(index, part, colour);
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new("スタイル・狩技（読み取り専用）").strong());
        let arts = set.arts();
        egui::Grid::new(("settail", index))
            .num_columns(2)
            .spacing([6.0, 3.0])
            .show(ui, |ui| {
                ui.label("スタイル");
                ui.label(format!("{}", set.style()));
                ui.end_row();
                ui.label("狩技");
                ui.label(format!("{} / {} / {}", arts[0], arts[1], arts[2]));
                ui.end_row();
            });
        ui.label(
            egui::RichText::new(
                "この 4 バイトは読むだけで、書き換えません（対応が確かめられていないため）。",
            )
            .weak()
            .small(),
        );
        ui.add_space(8.0);
        ui.separator();
        let already_empty = set.name == mhnx_core::myset::EMPTY_NAME
            && set
                .parts
                .iter()
                .all(|p| *p == mhnx_core::myset::NONE);
        if ui
            .add_enabled(!already_empty, egui::Button::new("このセットを空にする"))
            .on_hover_text("名前・部位・色・装飾品を消し、ゲームが未使用に残す並びにします（確認あり）")
            .clicked()
        {
            self.confirm = Confirm::SetClear(index);
        }
    }

    /// Rename a saved equipment set.
    pub fn set_name_write(&mut self, index: usize, name: &str) -> bool {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            return false;
        };
        let ok = mhnx_core::myset::set_name(save, slot, index, name);
        if ok {
            self.dirty = true;
        }
        ok
    }

    /// Point one part of a saved set at an equipment box record.
    ///
    /// Goes through myset::set_part, which writes the index at +0x2A and the
    /// decoration copy at +0x38 together. The equipment kind is not checked,
    /// because the real save holds a set with a talisman in the head slot and
    /// the game loads it; the warnings list says so instead.
    pub fn set_part_choose(&mut self, set: usize, part: usize, box_index: u16) -> bool {
        let slot = self.slot;
        let name = mhnx_core::myset::PART_NAMES
            .get(part)
            .copied()
            .unwrap_or("?");
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if mhnx_core::myset::set_part(save, slot, set, part, box_index) {
            self.dirty = true;
            self.message = format!(
                "セット {} の{}を枠 {} にしました（装飾品の写しも装備側から写しました）",
                set, name, box_index
            );
            true
        } else {
            self.message = format!(
                "セット {} の{}を枠 {} にできませんでした（空の枠か、装備ボックスの外を指しています）",
                set, name, box_index
            );
            false
        }
    }

    /// Empty one part of a saved set: the index goes to NONE and its decoration
    /// copy to zero in the same write.
    pub fn set_part_clear(&mut self, set: usize, part: usize) -> bool {
        let slot = self.slot;
        let name = mhnx_core::myset::PART_NAMES
            .get(part)
            .copied()
            .unwrap_or("?");
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if mhnx_core::myset::clear_part(save, slot, set, part) {
            self.dirty = true;
            self.message = format!(
                "セット {} の{}を解除しました（装飾品の写しも 0 にしました）",
                set, name
            );
            true
        } else {
            self.message = format!("セット {} の{}を解除できませんでした", set, name);
            false
        }
    }

    /// Write one armour colour of a saved set. Parts 1..=5 only: the weapon and
    /// the talisman have no colour of their own.
    pub fn set_colour_write(&mut self, set: usize, part: usize, colour: [u8; 4]) -> bool {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if mhnx_core::myset::set_colour(save, slot, set, part, colour) {
            self.dirty = true;
            let name = mhnx_core::myset::PART_NAMES
                .get(part)
                .copied()
                .unwrap_or("?");
            self.message = format!("セット {} の{}の色を変えました", set, name);
            true
        } else {
            self.message = format!(
                "セット {} の色を書き換えられませんでした（色を持つのは頭・胴・腕・腰・脚だけです）",
                set
            );
            false
        }
    }

    /// Empty a saved set, the way the game leaves a record it never used.
    /// Destructive, and the caller owns the confirmation.
    pub fn set_clear(&mut self, index: usize) -> bool {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if mhnx_core::myset::clear_set(save, slot, index) {
            self.dirty = true;
            self.message = format!(
                "セット {} を空にしました（「上書き保存」で書き出します）",
                index
            );
            true
        } else {
            self.message = format!("セット {} は空にできませんでした", index);
            false
        }
    }

    // ------------------------------------------------------------------ CSV

    /// The file name the export dialog starts from: box, slot number.
    pub fn csv_suggested_name(&self, kind: CsvKind) -> String {
        format!("{}-slot{}.csv", kind.stem(), self.slot + 1)
    }

    /// The item box of the open slot as CSV, taken from what the screen shows.
    ///
    /// The working copy is written into a *copy* of the save first. An export
    /// must not mark the save dirty, and it must not write the box on screen
    /// into the real save just to get at its text either.
    pub fn csv_items_text(&self) -> Option<String> {
        let save = self.save.as_ref()?;
        let slot = self.slot;
        let mut copy = save.clone();
        if !self.items.is_empty() {
            let _ = copy.write_item_box(slot, &self.items);
        }
        Some(csvout::items_to_csv(&copy, slot))
    }

    /// The equipment box of the open slot as CSV.
    ///
    /// The equipment tab writes into the save as it goes, so there is no
    /// working copy to fold in first.
    pub fn csv_equipment_text(&self) -> Option<String> {
        let save = self.save.as_ref()?;
        Some(csvout::equipment_to_csv(save, self.slot))
    }

    /// Write one of the two boxes to `path`.
    ///
    /// Returns whether the file was written; the status line says where it
    /// went, or why it could not be written.
    pub fn csv_write(&mut self, kind: CsvKind, path: &std::path::Path) -> bool {
        let text = match kind {
            CsvKind::Items => self.csv_items_text(),
            CsvKind::Equipment => self.csv_equipment_text(),
        };
        let Some(text) = text else {
            self.message = String::from("先にセーブを開いてください");
            return false;
        };
        match std::fs::write(path, text.as_bytes()) {
            Ok(()) => {
                // lines() counts what a spreadsheet of the box would show,
                // header included; the header is not a record.
                let rows = text.lines().count().saturating_sub(1);
                self.message = format!(
                    "{} を書き出しました: {}（{} 行）",
                    kind.label(),
                    path.display(),
                    rows
                );
                true
            }
            Err(e) => {
                self.message = format!("CSV を書き出せません: {}（{}）", path.display(), e);
                false
            }
        }
    }

    /// Read a CSV file and put what it would do in front of the user.
    ///
    /// Nothing is written here: the rows are applied to a copy of the save and
    /// the report of that dry run is what the confirmation dialog shows. The
    /// rows wait in csv_pending until csv_commit.
    pub fn csv_stage(&mut self, kind: CsvKind, path: PathBuf) -> bool {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return false;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.message = format!("CSV を読めません: {}（{}）", path.display(), e);
                return false;
            }
        };
        // Excel writes UTF-8 or the system code page. The parser wants UTF-8,
        // and a file in another code page would be read as mojibake, so it is
        // refused rather than half applied.
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => {
                self.message =
                    String::from("CSV は UTF-8 で保存してください（文字コードを判別できません）");
                return false;
            }
        };
        let rows = match kind {
            CsvKind::Items => match csvout::parse_items(&text) {
                Ok(rows) => CsvRows::Items(rows),
                Err(e) => {
                    // The message of CsvError already carries the line and
                    // the column, so it is shown as it is.
                    self.message = format!("CSV を読めません: {}", e);
                    return false;
                }
            },
            CsvKind::Equipment => match csvout::parse_equipment(&text) {
                Ok(rows) => CsvRows::Equipment(rows),
                Err(e) => {
                    self.message = format!("CSV を読めません: {}", e);
                    return false;
                }
            },
        };
        if rows.is_empty() {
            self.message = format!("{} には適用できる行がありません", path.display());
            return false;
        }
        let slot = self.slot;
        let Some(save) = self.save.as_ref() else {
            return false;
        };
        // A dry run: the report the dialog shows is what the file would do,
        // measured without touching the save that is open.
        let mut trial = save.clone();
        let report = match &rows {
            CsvRows::Items(rows) => csvout::apply_items(&mut trial, slot, rows),
            CsvRows::Equipment(rows) => csvout::apply_equipment(&mut trial, slot, rows),
        };
        self.message = format!(
            "{} を読みました（{} 行）。内容を確認して、適用するか決めてください",
            path.display(),
            rows.len()
        );
        self.csv_pending = Some(CsvPending {
            kind,
            path,
            rows,
            report,
        });
        self.confirm = Confirm::CsvApply;
        true
    }

    /// The numbers one apply report has to show.
    fn report_line(report: &ApplyReport) -> String {
        format!(
            "書き込み {} 行 / 対象外 {} 行 / 丸め {} 行",
            report.applied, report.skipped, report.clamped
        )
    }

    /// The body of the CSV confirmation dialog.
    ///
    /// Every number and every message of the ApplyReport is listed: a row the
    /// file has but the box will not take must not disappear without a word.
    pub fn csv_confirm_text(&self) -> String {
        let Some(pending) = &self.csv_pending else {
            return String::from("読み込んだ CSV がありません");
        };
        let mut body = format!(
            "{} を {} に書き込みます。\n{}（{} 行）\n\n",
            pending.path.display(),
            pending.kind.target(self.slot),
            Self::report_line(&pending.report),
            pending.rows.len()
        );
        // Bounded: a file that trips every row would otherwise make a window
        // taller than the screen.
        const SHOWN: usize = 8;
        for m in pending.report.messages.iter().take(SHOWN) {
            body += &format!("・{}\n", m);
        }
        if pending.report.messages.len() > SHOWN {
            body += &format!("・ほか {} 件\n", pending.report.messages.len() - SHOWN);
        }
        body += "\n「適用する」でセーブデータに反映します（ファイルへは「上書き保存」で書き込みます）。";
        body
    }

    /// Apply the staged CSV file, after the dialog said yes.
    ///
    /// The report that comes back is the one the real save produced, not the
    /// preview: both are computed the same way, and this one is what happened.
    pub fn csv_commit(&mut self) -> Option<ApplyReport> {
        let pending = self.csv_pending.take()?;
        let slot = self.slot;
        let items_file = matches!(pending.kind, CsvKind::Items);
        if items_file {
            // The item tab's 元に戻す has to be able to take the file back.
            self.push_item_undo();
        }
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("先にセーブを開いてください");
            return None;
        };
        let report = match &pending.rows {
            CsvRows::Items(rows) => csvout::apply_items(save, slot, rows),
            CsvRows::Equipment(rows) => csvout::apply_equipment(save, slot, rows),
        };
        if items_file {
            if let Some(items) = save.read_item_box(slot) {
                self.items = items;
            }
        }
        if report.applied == 0 {
            self.message = format!(
                "CSV を適用できませんでした: {}",
                Self::report_line(&report)
            );
        } else {
            self.dirty = true;
            self.message = format!(
                "{} を適用しました: {}",
                pending.kind.label(),
                Self::report_line(&report)
            );
        }
        Some(report)
    }

    // -------------------------------------------------- item sets / pouch

    /// Rename one of the twenty-four item sets.
    pub fn item_set_name_write(&mut self, index: usize, name: &str) -> bool {
        let slot = self.slot;
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        let ok = mhnx_core::myset::set_item_set_name(save, slot, index, name);
        if ok {
            self.dirty = true;
        } else {
            self.message = format!("セット {} の名前を書き換えられませんでした", index);
        }
        ok
    }

    /// Write one of the thirty-two entries of an item set.
    ///
    /// An id of 0 means "empty", and the core refuses an empty entry that still
    /// carries a count, so clearing goes through clear_item and drops both.
    pub fn item_set_entry_write(
        &mut self,
        set: usize,
        entry: usize,
        id: u16,
        count: u16,
    ) -> bool {
        let slot = self.slot;
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        let ok = if id == 0 {
            mhnx_core::myset::clear_item(save, slot, set, entry)
        } else {
            mhnx_core::myset::set_item(save, slot, set, entry, id, count)
        };
        if ok {
            self.dirty = true;
            self.message = if id == 0 {
                format!("セット {} の {} 番目を空にしました", set, entry)
            } else {
                format!(
                    "セット {} の {} 番目を {} x{} にしました",
                    set,
                    entry,
                    item_name(id),
                    count
                )
            };
        } else {
            self.message = format!(
                "セット {} の {} 番目を書き換えられませんでした（ID {} / 個数 {}）",
                set, entry, id, count
            );
        }
        ok
    }

    /// Copy the pouch into an item set. Destructive: the caller asks first.
    ///
    /// Returns how many entries hold something; 0 means nothing was written,
    /// which is either an empty pouch or a pouch that cannot be read, and the
    /// status line says which.
    pub fn item_set_from_pouch(&mut self, set: usize) -> usize {
        let slot = self.slot;
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return 0;
        };
        let carried = mhnx_core::myset::pouch(save, slot);
        if carried.len() != mhnx_core::myset::POUCH_SLOTS {
            self.message = format!("枠 {} のポーチが読めません", slot + 1);
            return 0;
        }
        let used = mhnx_core::myset::copy_from_pouch(save, slot, set);
        self.dirty = true;
        self.message = if used == 0 {
            // All thirty-two entries are written either way, so an empty
            // pouch really does empty the set.
            format!(
                "ポーチが空だったので、セット {} を空にしました（名前はそのままです）",
                set
            )
        } else {
            format!(
                "ポーチの {} 個をセット {} に取り込みました（空き {} 枠）",
                used,
                set,
                mhnx_core::myset::POUCH_SLOTS - used
            )
        };
        used
    }

    /// Replace the pouch with an item set.
    ///
    /// Destructive and confirmed by the caller. What is about to be replaced
    /// is kept, because nothing else in the save can reconstruct it; the one
    /// press of 元に戻す puts it back.
    pub fn item_set_to_pouch(&mut self, set: usize) -> usize {
        let slot = self.slot;
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return 0;
        };
        let before = mhnx_core::myset::pouch(save, slot);
        if before.len() != mhnx_core::myset::POUCH_SLOTS {
            self.message = format!("枠 {} のポーチが読めません", slot + 1);
            return 0;
        }
        // A set that holds nothing must not reach the pouch: applying it would
        // empty the 32 slots the character carries into a quest, and the core
        // only refuses the narrower case of a record still named '－－－'.
        let holds = mhnx_core::myset::item_sets(save, slot)
            .into_iter()
            .find(|s| s.index == set)
            .map(|s| s.items.iter().any(|i| i.id != 0))
            .unwrap_or(false);
        if !holds {
            self.message = format!(
                "セット {} は中身が空なので、ポーチへは適用しませんでした（ポーチはそのままです）",
                set
            );
            return 0;
        }
        let used = mhnx_core::myset::apply_to_pouch(save, slot, set);
        if used == 0 {
            // Refused by the core, or the 76 bytes could not be written.
            self.message = format!(
                "セット {} をポーチへ適用できませんでした（ポーチはそのままです）",
                set
            );
            return 0;
        }
        self.pouch_undo = Some(before);
        self.dirty = true;
        self.message = format!(
            "ポーチをセット {} の内容 {} 個に置き換えました（「元に戻す」で戻せます）",
            set, used
        );
        used
    }

    /// Put the pouch back the way it was before ポーチへ適用.
    pub fn pouch_restore(&mut self) -> bool {
        let Some(before) = self.pouch_undo.clone() else {
            self.message = String::from("戻せるポーチの控えがありません");
            return false;
        };
        let slot = self.slot;
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        let mut bad = 0usize;
        for (index, item) in before.iter().enumerate() {
            if !mhnx_core::myset::set_pouch_item(save, slot, index, item.id, item.count) {
                bad += 1;
            }
        }
        if bad > 0 {
            self.message = format!("ポーチを戻せませんでした（{} 枠）", bad);
            return false;
        }
        self.pouch_undo = None;
        self.dirty = true;
        self.message = String::from("ポーチを ポーチへ適用 の前に戻しました");
        true
    }

    /// Write one pouch entry.
    ///
    /// The pouch is the same thirty-two (id, count) pairs an item set holds,
    /// coded into 76 bytes; the core owns the bit layout.
    pub fn pouch_entry_write(&mut self, entry: usize, id: u16, count: u16) -> bool {
        let slot = self.slot;
        // The core refuses an empty entry that carries a count; dropping the
        // count here is what makes "set the id to 0" mean "clear this one".
        let (id, count) = if id == 0 { (0, 0) } else { (id, count) };
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        let ok = mhnx_core::myset::set_pouch_item(save, slot, entry, id, count);
        if ok {
            self.dirty = true;
            self.message = if id == 0 {
                format!("ポーチの {} 番目を空にしました", entry)
            } else {
                format!(
                    "ポーチの {} 番目を {} x{} にしました",
                    entry,
                    item_name(id),
                    count
                )
            };
        } else {
            self.message = format!(
                "ポーチの {} 番目を書き換えられませんでした（ID {} / 個数 {}）",
                entry, id, count
            );
        }
        ok
    }

    fn tab_palico_detail(&mut self, ui: &mut egui::Ui) {
        ui.heading("オトモ");
        let Some(save) = &self.save else {
            ui.label("セーブを開くと、オトモの一覧を表示します");
            return;
        };
        // Copied out of the save once per frame, the way the equipment tab does
        // it: the two lists and the editor all read these bytes, and holding
        // &self.save across them would lock the fields the writes touch.
        let slot = self.slot;
        let list = mhnx_core::palico::used(save, slot);
        let gear = mhnx_core::palicogear::decode(save, slot);
        let gear_bytes = mhnx_core::palicogear::box_bytes(save, slot)
            .map(|b| b.to_vec())
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} 匹 / オトモ装備 {} 件（{} 枠中）",
                list.len(),
                gear.len(),
                mhnx_core::palicogear::PALICO_EQUIP_COUNT
            ));
            // The name tables for this box have never been mapped, so every
            // row names its record by id. Saying so once beats a "-" column.
            ui.label(
                egui::RichText::new("オトモ装備の名前は未対応です。装備は ID で表示します。")
                    .weak()
                    .small(),
            );
        });
        if list.is_empty() {
            ui.label("このスロットにはオトモがいません");
            return;
        }
        // Land on the first palico the first time the tab is opened: an empty
        // gear panel below the list looks like the tab has no editor at all.
        if !list.iter().any(|p| Some(p.index) == self.palico_selected) {
            self.palico_selected = Some(list[0].index);
        }
        let mut changed = false;
        // Bounded so the two panels below stay on screen. A share of what is
        // left rather than a fixed number: the window can be as short as 600
        // points, and a fixed 172 would push the gear editor off the bottom.
        let roster_h = (ui.available_height() * 0.34).clamp(120.0, 180.0);
        egui::ScrollArea::vertical()
            .id_salt("palicolist")
            .max_height(roster_h)
            .show(ui, |ui| {
                // Six headers and six cells per row: the grid used to declare
                // four columns, which squeezed every column and left the tail
                // of each row off the right edge.
                egui::Grid::new("palicogrid")
                    .num_columns(6)
                    .striped(true)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        for h in ["枠", "名前", "レベル", "なつき", "系統", "修得スキル"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for p in &list {
                            // The frame number is the selector: the gear panel
                            // below edits whichever palico is picked here.
                            if ui
                                .selectable_label(
                                    self.palico_selected == Some(p.index),
                                    format!("{}", p.index),
                                )
                                .clicked()
                            {
                                self.palico_selected = Some(p.index);
                            }
                            let mut name = p.name.clone();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut name)
                                        .desired_width(140.0),
                                )
                                .changed()
                            {
                                if let Some(s) = &mut self.save {
                                    mhnx_core::palico::set_name(s, self.slot, p.index, &name);
                                }
                                changed = true;
                            }
                            let mut lvl = p.level as i32;
                            if ui
                                .add(egui::DragValue::new(&mut lvl).range(1..=99))
                                .changed()
                            {
                                if let Some(s) = &mut self.save {
                                    mhnx_core::palico::set_level(
                                        s,
                                        self.slot,
                                        p.index,
                                        lvl as u16,
                                    );
                                }
                                changed = true;
                            }
                            let mut ent = p.enthusiasm as i32;
                            if ui
                                .add(egui::DragValue::new(&mut ent).range(0..=100))
                                .changed()
                            {
                                if let Some(s) = &mut self.save {
                                    mhnx_core::palico::set_enthusiasm(
                                        s,
                                        self.slot,
                                        p.index,
                                        ent as u8,
                                    );
                                }
                                changed = true;
                            }
                            ui.label(p.system_name());
                            let names = p
                                .learned_skills()
                                .iter()
                                .map(|id| mhnx_core::palico::skill_name(*id))
                                .collect::<Vec<_>>()
                                .join(", ");
                            // A palico can know eight skills by name and the
                            // row would otherwise run past the right edge.
                            ui.add(egui::Label::new(names).truncate());
                            ui.end_row();
                        }
                    });
            });
        if changed {
            self.dirty = true;
            self.message = "オトモを更新しました（まだファイルには書いていません）".to_owned();
        }
        ui.separator();
        let palico = self.palico_selected.unwrap_or(list[0].index);
        // Gear assignment on the left, the box it points into on the right.
        // The box list takes the rest of the height.
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(470.0);
                self.palico_equip_panel(ui, palico, &gear, &gear_bytes);
            });
            ui.separator();
            ui.vertical(|ui| {
                self.palico_gear_panel(ui, &gear, &gear_bytes);
            });
        });
    }

    /// Name of one of the three slots a palico wears things in.
    fn palico_part_name(part: usize) -> &'static str {
        match part {
            0 => "武器",
            1 => "頭",
            2 => "胴",
            _ => "?",
        }
    }

    /// "枠 4 / ID 431" for a worn box index, "なし" for the empty value.
    fn palico_gear_label(index: u16, gear: &[mhnx_core::equip::Equipment]) -> String {
        if index == u16::MAX {
            return String::from("なし");
        }
        match gear.iter().find(|e| e.index == index as usize) {
            Some(e) => format!("枠 {} / ID {}", e.index, e.id),
            // The roster can name a record the box no longer holds. Showing
            // the number beats pretending nothing is worn.
            None => format!("枠 {} / 不明", index),
        }
    }

    /// The three gear slots of one palico.
    ///
    /// Picking a row writes its box index into the roster record at +0x100;
    /// 解除 writes 0xFFFF. The box record itself is never touched: the roster
    /// names a row, and the game lets two palicos name the same one.
    fn palico_equip_panel(
        &mut self,
        ui: &mut egui::Ui,
        palico: usize,
        gear: &[mhnx_core::equip::Equipment],
        gear_bytes: &[u8],
    ) {
        let worn = self.palico_equip_of(palico).unwrap_or([u16::MAX; 3]);
        ui.label(egui::RichText::new(format!("枠 {} の装備の割当", palico)).strong());
        let mut assign: Option<(usize, u16)> = None;
        let mut release: Option<usize> = None;
        for part in 0..mhnx_core::palico::EQUIP_SLOTS {
            let want = mhnx_core::palicogear::KINDS[part];
            let current = worn[part];
            ui.horizontal_wrapped(|ui| {
                ui.label(Self::palico_part_name(part));
                let shown = Self::palico_gear_label(current, gear);
                egui::ComboBox::from_id_salt(("palicopart", palico, part))
                    .selected_text(shown)
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(current == u16::MAX, "なし（外す）")
                            .clicked()
                            && current != u16::MAX
                        {
                            release = Some(part);
                        }
                        // Only the records of the kind this part takes: the
                        // real save has all 21 worn indices landing on the
                        // kind their position implies, and a file that says
                        // otherwise is not something to write back.
                        for e in gear.iter().filter(|e| e.kind == want) {
                            let label = format!("枠 {} / ID {}", e.index, e.id);
                            if ui
                                .selectable_label(current == e.index as u16, label)
                                .clicked()
                            {
                                assign = Some((part, e.index as u16));
                            }
                        }
                    });
                if current != u16::MAX && ui.small_button("解除").clicked() {
                    release = Some(part);
                }
            });
        }
        if let Some((part, index)) = assign {
            self.palico_equip_assign(palico, part, index);
        } else if let Some(part) = release {
            self.palico_equip_release(palico, part);
        }
        ui.label(
            egui::RichText::new(
                "書き換えるのはオトモの記録（+0x100 の 3×u16）だけで、オトモ装備ボックスのレコードはそのままです。",
            )
            .weak()
            .small(),
        );
        ui.add_space(4.0);
        ui.separator();
        self.palico_gear_editor(ui, gear, gear_bytes);
    }

    /// Level and decoration editor for the selected otomo gear record.
    fn palico_gear_editor(
        &mut self,
        ui: &mut egui::Ui,
        gear: &[mhnx_core::equip::Equipment],
        gear_bytes: &[u8],
    ) {
        // Land on the first record so the panel is useful without a click.
        if self
            .palico_gear_selected
            .map_or(true, |i| !gear.iter().any(|e| e.index == i))
        {
            self.palico_gear_selected = gear.first().map(|e| e.index);
        }
        let Some(eq) = self
            .palico_gear_selected
            .and_then(|i| gear.iter().find(|e| e.index == i).copied())
        else {
            ui.label(egui::RichText::new("編集できるオトモ装備がありません").weak());
            return;
        };
        ui.label(egui::RichText::new(format!("装備 {} の編集", eq.index)).strong());
        ui.label(format!(
            "{} / ID {}（名前は未対応）",
            mhnx_core::equip::type_name(eq.kind),
            eq.id
        ));
        let mut level = eq.level;
        ui.horizontal(|ui| {
            ui.label("レベル");
            if ui
                .add(
                    egui::DragValue::new(&mut level)
                        .range(1..=mhnx_core::palicogear::MAX_LEVEL),
                )
                .changed()
            {
                self.palico_gear_level(eq.index, level);
            }
            ui.label(egui::RichText::new("1〜32").weak().small());
        });
        let at = eq.index * mhnx_core::palicogear::RECORD;
        let decos = gear_bytes
            .get(at..at + mhnx_core::palicogear::RECORD)
            .map(|rec| mhnx_core::equip::Equipment::decorations(eq.index, rec))
            .unwrap_or([0; 3]);
        let mut deco_action: Option<(usize, u16)> = None;
        for socket in 0..3 {
            ui.horizontal(|ui| {
                ui.label(format!("{}枠目", socket + 1));
                if let Some(id) = Self::deco_combo(
                    ui,
                    "palico",
                    socket,
                    decos[socket],
                    &mut self.palico_deco_search[socket],
                ) {
                    deco_action = Some((socket, id));
                }
            });
        }
        if let Some((socket, id)) = deco_action {
            self.palico_gear_decoration(eq.index, socket, id);
        }
        ui.label(
            egui::RichText::new(
                "先頭 12 バイト（Lv・装飾品）だけを書き換え、+0x0C 以降は保持します。",
            )
            .weak()
            .small(),
        );
    }

    /// The used otomo gear records, and the row the editor is showing.
    fn palico_gear_panel(
        &mut self,
        ui: &mut egui::Ui,
        gear: &[mhnx_core::equip::Equipment],
        gear_bytes: &[u8],
    ) {
        // Pinned before anything is drawn. set_height() unions the min_rect
        // with the cursor, so calling it after the filter row would pull the
        // cursor back to the top of the column and the table would paint over
        // the row above it.
        ui.set_height(ui.available_height());
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("オトモ装備ボックス").strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.palico_gear_filter)
                    .hint_text("種類・ID・枠番号")
                    .desired_width(140.0),
            );
            if ui.small_button("クリア").clicked() {
                self.palico_gear_filter.clear();
            }
            ui.checkbox(&mut self.palico_gear_show_empty, "空き枠を表示");
        });
        let needle = self.palico_gear_filter.trim().to_lowercase();
        let show_empty = self.palico_gear_show_empty;
        let mut rows: Vec<mhnx_core::equip::Equipment> = if show_empty {
            let Some(save) = &self.save else {
                return;
            };
            (0..mhnx_core::palicogear::PALICO_EQUIP_COUNT)
                .map(|i| {
                    mhnx_core::palicogear::decode_one(save, self.slot, i).unwrap_or(
                        mhnx_core::equip::Equipment {
                            index: i,
                            kind: 0,
                            level: 1,
                            transmog_level: 1,
                            id: 0,
                            transmog: 0,
                        },
                    )
                })
                .collect()
        } else {
            gear.to_vec()
        };
        rows.retain(|e| {
            needle.is_empty()
                || mhnx_core::equip::type_name(e.kind)
                    .to_lowercase()
                    .contains(&needle)
                || format!("{}", e.id).contains(&needle)
                || format!("{}", e.index).contains(&needle)
        });
        if self
            .palico_gear_selected
            .map_or(true, |i| !rows.iter().any(|e| e.index == i))
        {
            self.palico_gear_selected = rows.iter().find(|e| e.kind != 0).map(|e| e.index);
        }
        ui.label(
            egui::RichText::new(format!("{} 件を表示", rows.len()))
                .weak()
                .small(),
        );
        let selected = self.palico_gear_selected;
        let mut pick: Option<usize> = None;
        TableBuilder::new(ui)
            .id_salt("palicogearlist")
            .striped(true)
            .sense(egui::Sense::click())
            .column(Column::exact(46.0))
            .column(Column::exact(96.0))
            .column(Column::exact(70.0))
            .column(Column::exact(40.0))
            .column(Column::remainder())
            .header(22.0, |mut h| {
                for t in ["枠", "種類", "ID", "Lv", "装飾品"] {
                    h.col(|ui| {
                        ui.strong(t);
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let e = rows[row.index()];
                    row.set_selected(selected == Some(e.index));
                    row.col(|ui| {
                        ui.label(format!("{}", e.index));
                    });
                    row.col(|ui| {
                        ui.label(mhnx_core::equip::type_name(e.kind));
                    });
                    row.col(|ui| {
                        ui.label(format!("{}", e.id));
                    });
                    row.col(|ui| {
                        ui.label(format!("{}", e.level));
                    });
                    row.col(|ui| {
                        let at = e.index * mhnx_core::palicogear::RECORD;
                        let d = gear_bytes
                            .get(at..at + mhnx_core::palicogear::RECORD)
                            .map(|rec| {
                                mhnx_core::equip::Equipment::decorations(e.index, rec)
                            })
                            .unwrap_or([0; 3]);
                        let text = format!(
                            "{}・{}・{}",
                            deco_label(d[0]),
                            deco_label(d[1]),
                            deco_label(d[2])
                        );
                        ui.label(text.clone()).on_hover_text(text);
                    });
                    if row.response().clicked() {
                        pick = Some(e.index);
                    }
                });
            });
        if let Some(p) = pick {
            self.palico_gear_selected = Some(p);
        }
    }

    /// The three box indices one palico wears, weapon / head / body.
    pub fn palico_equip_of(&self, palico: usize) -> Option<[u16; 3]> {
        let save = self.save.as_ref()?;
        mhnx_core::palico::roster(save, self.slot)
            .get(palico)
            .map(|p| p.equip)
    }

    /// Point one of a palico's three equipment slots at a record in its box.
    ///
    /// The record has to be in use and has to carry the kind that part takes:
    /// the sample save has every worn index landing on the right kind, and a
    /// value that breaks that is not something this editor should create. The
    /// record itself is read but never written - the roster names it by index.
    pub fn palico_equip_assign(&mut self, palico: usize, part: usize, box_index: u16) -> bool {
        let slot = self.slot;
        let part_name = Self::palico_part_name(part);
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if part >= mhnx_core::palico::EQUIP_SLOTS {
            self.message = format!("{} はオトモの装備部位ではありません", part);
            return false;
        }
        // An empty roster record is not a palico: writing gear into it would
        // leave an index the game reads for a slot it never shows.
        let exists = mhnx_core::palico::roster(save, slot)
            .get(palico)
            .map(|p| !p.is_empty())
            .unwrap_or(false);
        if !exists {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        }
        let Some(eq) = mhnx_core::palicogear::decode_one(save, slot, box_index as usize) else {
            self.message = format!(
                "装備ボックスの枠 {} は空きなので、{}にできません",
                box_index, part_name
            );
            return false;
        };
        let want = mhnx_core::palicogear::KINDS[part];
        if eq.kind != want {
            self.message = format!(
                "枠 {} は{}なので、{}にはできません",
                box_index,
                mhnx_core::equip::type_name(eq.kind),
                part_name
            );
            return false;
        }
        if !Self::write_palico_equip(save, slot, palico, part, box_index) {
            self.message = format!("枠 {} の{}を書き換えられませんでした", palico, part_name);
            return false;
        }
        self.dirty = true;
        self.message = format!(
            "枠 {} の{}をオトモ装備の枠 {} にしました（ボックスのレコードはそのままです）",
            palico, part_name, box_index
        );
        true
    }

    /// Take one piece off a palico by writing 0xFFFF into its roster slot.
    pub fn palico_equip_release(&mut self, palico: usize, part: usize) -> bool {
        let slot = self.slot;
        let part_name = Self::palico_part_name(part);
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if part >= mhnx_core::palico::EQUIP_SLOTS {
            self.message = format!("{} はオトモの装備部位ではありません", part);
            return false;
        }
        let current = mhnx_core::palico::roster(save, slot)
            .get(palico)
            .map(|p| p.equip[part]);
        let Some(current) = current else {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        };
        if current == u16::MAX {
            self.message = format!("枠 {} の{}はすでに外れています", palico, part_name);
            return false;
        }
        if !Self::write_palico_equip(save, slot, palico, part, u16::MAX) {
            self.message = format!("枠 {} の{}を外せませんでした", palico, part_name);
            return false;
        }
        self.dirty = true;
        self.message = format!("枠 {} の{}を外しました（0xFFFF）", palico, part_name);
        true
    }

    /// Write one box index into the roster record at +0x100.
    ///
    /// Only the two bytes of the one part move. The offset is assembled from
    /// the constants in palico, which is where the roster layout is stated.
    fn write_palico_equip(
        save: &mut Save,
        slot: usize,
        palico: usize,
        part: usize,
        index: u16,
    ) -> bool {
        if palico >= mhnx_core::palico::SLOTS || part >= mhnx_core::palico::EQUIP_SLOTS {
            return false;
        }
        let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
            return false;
        };
        let at = base
            + mhnx_core::palico::OFF_ROSTER
            + palico * mhnx_core::palico::RECORD
            + mhnx_core::palico::EQUIP
            + part * 2;
        let Some(dst) = save.body_mut().get_mut(at..at + 2) else {
            return false;
        };
        dst.copy_from_slice(&index.to_le_bytes());
        true
    }

    /// Rewrite the level of one otomo gear record.
    pub fn palico_gear_level(&mut self, index: usize, level: u16) -> bool {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if mhnx_core::palicogear::set_level(save, slot, index, level) {
            self.dirty = true;
            self.message = format!("オトモ装備の枠 {} をレベル {} にしました", index, level);
            true
        } else {
            self.message = format!(
                "枠 {} のレベルを {} にできませんでした（1〜32、空き枠は不可）",
                index,
                level
            );
            false
        }
    }

    /// Rewrite one decoration socket of an otomo gear record.
    pub fn palico_gear_decoration(&mut self, index: usize, socket: usize, id: u16) -> bool {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if mhnx_core::palicogear::set_decoration(save, slot, index, socket, id) {
            self.dirty = true;
            self.message = format!(
                "オトモ装備の枠 {} の {} 枠目を {} にしました",
                index,
                socket + 1,
                deco_label(id)
            );
            true
        } else {
            self.message = format!(
                "枠 {} の {} 枠目の装飾品を書き換えられませんでした",
                index,
                socket + 1
            );
            false
        }
    }

    fn tab_hunts(&mut self, ui: &mut egui::Ui) {
        ui.heading("狩猟記録");
        if self.save.is_none() {
            ui.label("セーブを開くと、モンスターごとの討伐数と狩猟サイズを一覧できます");
            return;
        }
        if self.hunt_slay.is_empty() {
            ui.label("このスロットには狩猟記録がありません");
            return;
        }
        let names = mhnx_core::gamedata::MONSTER_HUNT_NAMES;
        ui.horizontal(|ui| {
            let recorded = self.hunt_slay.iter().filter(|v| **v > 0).count();
            ui.label(format!("{} 体 / {} 体中記録あり", recorded, self.hunt_slay.len()));
            ui.label("絞り込み");
            ui.add(
                egui::TextEdit::singleline(&mut self.hunt_filter)
                    .hint_text("モンスター名")
                    .desired_width(160.0),
            );
            if ui.small_button("クリア").clicked() {
                self.hunt_filter.clear();
            }
            if ui.button("変更を反映").clicked() {
                self.apply_hunts();
            }
        });
        let needle = self.hunt_filter.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("huntlist")
            .show(ui, |ui| {
                egui::Grid::new("huntgrid")
                    .num_columns(5)
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["id", "モンスター", "討伐", "捕獲", "サイズ"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        let mut changed = false;
                        for i in 0..self.hunt_slay.len() {
                            let name = names.get(i).copied().unwrap_or("?");
                            if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                                continue;
                            }
                            ui.label(format!("{}", i));
                            ui.label(name);
                            let mut slay = self.hunt_slay[i] as i32;
                            if ui
                                .add(egui::DragValue::new(&mut slay).range(0..=9999))
                                .changed()
                            {
                                self.hunt_slay[i] = slay as u16;
                                changed = true;
                            }
                            if i < self.hunt_capture.len() {
                                let mut cap = self.hunt_capture[i] as i32;
                                if ui
                                    .add(egui::DragValue::new(&mut cap).range(0..=9999))
                                    .changed()
                                {
                                    self.hunt_capture[i] = cap as u16;
                                    changed = true;
                                }
                            } else {
                                ui.label("-");
                            }
                            if i < self.hunt_size.len() {
                                let (a, b) = self.hunt_size[i];
                                let (mut lo, mut hi) = (a as i32, b as i32);
                                ui.horizontal(|ui| {
                                    let c1 = ui
                                        .add(egui::DragValue::new(&mut lo).range(0..=300))
                                        .changed();
                                    ui.label("..");
                                    let c2 = ui
                                        .add(egui::DragValue::new(&mut hi).range(0..=300))
                                        .changed();
                                    if c1 || c2 {
                                        self.hunt_size[i] = (lo as u16, hi as u16);
                                        changed = true;
                                    }
                                });
                            } else {
                                ui.label("-");
                            }
                            ui.end_row();
                        }
                        if changed {
                            self.dirty = true;
                        }
                    });
            });
    }

    fn tab_guild(&mut self, ui: &mut egui::Ui) {
        ui.heading("ギルドカード");
        // Everything below is copied out first: the edit controls need a
        // mutable borrow of the save, which cannot coexist with reading it.
        let (hr, time, id, comment, log) = {
            let Some(save) = &self.save else {
                ui.label("セーブを開くと、ギルドカードと闘技大会の記録を表示します");
                return;
            };
            let slot = self.slot;
            (
                mhnx_core::guildcard::hunter_rank(save, slot).unwrap_or(0),
                mhnx_core::guildcard::play_time(save, slot).unwrap_or(0),
                mhnx_core::guildcard::card_id(save, slot).unwrap_or([0; 8]),
                mhnx_core::guildcard::comment(save, slot).unwrap_or_default(),
                mhnx_core::guildcard::arena(save, slot),
            )
        };
        let slot = self.slot;
        // The card is editable: rank, its own clock and the comment are all
        // plain fields, so they can be nudged here like any other number.
        let mut new_hr = hr;
        let mut new_time = time;
        let mut new_comment = comment.clone();
        let mut changed = false;
        egui::Grid::new("gcinfo").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
            ui.label("ハンターランク");
            if ui
                .add(egui::DragValue::new(&mut new_hr).range(0..=999))
                .changed()
            {
                changed = true;
            }
            ui.end_row();
            ui.label("カード側プレイ時間(秒)");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut new_time)
                            .speed(60)
                            .range(0..=100_000_000u32),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.label(format!(
                    "= {}:{:02}:{:02}",
                    new_time / 3600,
                    (new_time % 3600) / 60,
                    new_time % 60
                ));
            });
            ui.end_row();
            ui.label("カードID");
            ui.label(id.iter().map(|b| format!("{:02X}", b)).collect::<String>());
            ui.end_row();
            ui.label("コメント");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut new_comment)
                        .desired_width(360.0)
                        .char_limit(mhnx_core::guildcard::GC_COMMENT_CHARS),
                )
                .changed()
            {
                changed = true;
            }
            ui.end_row();
        });
        if changed {
            if let Some(save) = &mut self.save {
                if new_hr != hr {
                    mhnx_core::guildcard::set_hunter_rank(save, slot, new_hr);
                }
                if new_time != time {
                    mhnx_core::guildcard::set_play_time(save, slot, new_time);
                }
                if new_comment != comment {
                    mhnx_core::guildcard::set_comment(save, slot, &new_comment);
                }
            }
            self.dirty = true;
            self.message = "ギルドカードを更新しました（まだファイルには書いていません）".to_owned();
        }
        ui.separator();
        ui.separator();
        ui.label(egui::RichText::new("闘技大会の記録（タイムは 1/100 秒）").strong());
        if log.iter().all(|e| e.is_empty()) {
            ui.label("記録はありません");
            return;
        }
        egui::Grid::new("arenagrid").num_columns(2).striped(true).show(ui, |ui| {
            ui.strong("アリーナ");
            ui.strong("ベストタイム");
            ui.end_row();
            for a in 0..mhnx_core::guildcard::ARENA_COUNT {
                let rows: Vec<_> = log.iter().filter(|e| e.arena == a && !e.is_empty()).collect();
                if rows.is_empty() {
                    continue;
                }
                ui.label(format!("{}", a));
                let text = rows
                    .iter()
                    .map(|e| {
                        let (m, s, cs) = e.as_clock();
                        format!("{}:{:02}.{:02}", m, s, cs)
                    })
                    .collect::<Vec<_>>()
                    .join("   ");
                ui.label(text);
                ui.end_row();
            }
        });
    }
    /// The equipment box: the list on the left, the editor for the selected
    /// record on the right.
    ///
    /// Records are edited, created, replaced and cleared. Both of the
    /// operations that move a reference go through equipops, which asks the
    /// reference checker first: deleting a named record comes back with the
    /// list of what still points at it, and the dialog shows that list before
    /// it offers the forced delete.
    fn tab_equip(&mut self, ui: &mut egui::Ui) {
        ui.heading("装備ボックス");
        if self.save.is_none() {
            ui.label("セーブを開くと、装備 2000 枠を一覧・編集できます");
            return;
        }
        // Copied out of the save once per frame, the way the talisman tab does
        // it: the table and the editor both read these 72,000 bytes, and
        // holding &self.save across them would lock the fields the editor
        // writes while it draws.
        let boxv: Vec<u8> = self
            .save
            .as_ref()
            .and_then(|s| s.equipment_box(self.slot))
            .map(|b| b.to_vec())
            .unwrap_or_default();
        if boxv.len() < mhnx_core::equip::EQUIP_RECORD {
            ui.label("このスロットには装備ボックスがありません");
            return;
        }
        let all = mhnx_core::equip::decode_box(&boxv);
        let used = all.iter().filter(|e| !e.is_empty()).count();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("使用中 {} / {} 枠", used, all.len()));
            ui.label("絞り込み");
            ui.add(
                egui::TextEdit::singleline(&mut self.equip_filter)
                    .hint_text("名前・種類・枠番号")
                    .desired_width(160.0),
            );
            if ui.small_button("クリア").clicked() {
                self.equip_filter.clear();
            }
            ui.checkbox(&mut self.equip_show_empty, "空き枠を表示")
                .on_hover_text("既定では使用中の枠だけを一覧します");
            ui.separator();
            // The three operations that move a reference exist now; they all
            // pass through a dialog, so the line says that rather than 未対応.
            ui.label(
                egui::RichText::new("作成・差し替え・削除も可（削除と差し替えは確認あり）")
                    .weak()
                    .small(),
            );
        });
        // New records: type, id and level, then one button. The row lives above
        // the list so it can be reached without first finding an empty slot,
        // and the box decides which blank record is used.
        let mut create = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("新規作成").strong());
            Self::equip_kind_combo(ui, "equipnewkind", &mut self.equip_new_kind);
            ui.label("ID");
            ui.add(
                egui::DragValue::new(&mut self.equip_new_id)
                    .range(0..=0xFFFF)
                    .speed(1.0),
            );
            ui.label("Lv");
            ui.add(egui::DragValue::new(&mut self.equip_new_level).range(1..=32));
            create = ui
                .button("空き枠に作成")
                .on_hover_text("最初の空きレコードに書き込み、その行を選びます")
                .clicked();
        });
        if create {
            self.equip_create(self.equip_new_kind, self.equip_new_id, self.equip_new_level);
        }
        // The same two CSV actions the item tab has, for the equipment box.
        // The export writes the records that hold something; the import is
        // shown before it is applied, like the item one.
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("CSV").strong()).on_hover_text(
                "装備ボックスを表として持ち出し、その表から書き戻せます（使用中の枠だけを 8 列で）",
            );
            if ui
                .button("CSV 書き出し")
                .on_hover_text("このキャラの装備ボックスの使用中の枠を CSV にします")
                .clicked()
            {
                let name = self.csv_suggested_name(CsvKind::Equipment);
                if let Some(p) = rfd::FileDialog::new()
                    .set_file_name(name)
                    .add_filter("CSV", &["csv"])
                    .set_title("装備ボックスの CSV 書き出し")
                    .save_file()
                {
                    self.csv_write(CsvKind::Equipment, &p);
                }
            }
            if ui
                .button("CSV 読み込み")
                .on_hover_text("CSV の内容を装備ボックスに書き戻します（内容を確認してから適用・確認あり）")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_title("装備ボックスの CSV 読み込み")
                    .pick_file()
                {
                    self.csv_stage(CsvKind::Equipment, p);
                }
            }
        });
        let needle = self.equip_filter.trim().to_lowercase();
        let show_empty = self.equip_show_empty;
        let rows: Vec<_> = all
            .iter()
            .filter(|e| show_empty || !e.is_empty())
            .filter(|e| {
                needle.is_empty()
                    || mhnx_core::equip::name(e).to_lowercase().contains(&needle)
                    || mhnx_core::equip::type_name(e.kind)
                        .to_lowercase()
                        .contains(&needle)
                    || format!("{}", e.index).contains(&needle)
            })
            .collect();
        ui.label(
            egui::RichText::new(format!("{} 件を表示", rows.len()))
                .weak()
                .small(),
        );
        // Land on something editable the first time the tab is opened: an
        // empty panel on the right looks like the tab has no editor at all.
        if self.equip_selected.is_none() {
            self.equip_selected = rows.iter().find(|e| !e.is_empty()).map(|e| e.index);
        }
        // The editor needs about this much; the list gets what is left.
        const PANEL_W: f32 = 360.0;
        let mut pick: Option<usize> = None;
        ui.horizontal_top(|ui| {
            let table_w = (ui.available_width() - PANEL_W - 14.0).max(420.0);
            ui.vertical(|ui| {
                ui.set_width(table_w);
                ui.set_height(ui.available_height());
                // Only the rows on screen are built: a save can hold two
                // thousand entries and a plain grid laid every one of them out
                // each frame.
                TableBuilder::new(ui)
                    .id_salt("equiplist")
                    .striped(true)
                    // Clicking anywhere on the row selects it, not just on the
                    // 46 point box number.
                    .sense(egui::Sense::click())
                    .column(Column::exact(46.0))
                    .column(Column::exact(70.0))
                    .column(Column::remainder())
                    .column(Column::exact(40.0))
                    .column(Column::exact(132.0))
                    .header(22.0, |mut h| {
                        for t in ["枠", "種類", "名前", "Lv", "装飾品"] {
                            h.col(|ui| {
                                ui.strong(t);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(20.0, rows.len(), |mut row| {
                            let e = rows[row.index()];
                            row.set_selected(pick.or(self.equip_selected) == Some(e.index));
                            row.col(|ui| {
                                ui.label(format!("{}", e.index));
                            });
                            row.col(|ui| {
                                ui.label(mhnx_core::equip::type_name(e.kind));
                            });
                            row.col(|ui| {
                                ui.label(mhnx_core::equip::name(e));
                            });
                            row.col(|ui| {
                                ui.label(format!("{}", e.level));
                            });
                            row.col(|ui| {
                                let d =
                                    mhnx_core::equip::Equipment::decorations(e.index, &boxv);
                                let text = format!(
                                    "{}・{}・{}",
                                    deco_label(d[0]),
                                    deco_label(d[1]),
                                    deco_label(d[2])
                                );
                                // The cell is narrower than three decoration
                                // names, so the full text lives in the tooltip.
                                ui.label(text.clone()).on_hover_text(text);
                            });
                            if row.response().clicked() {
                                pick = Some(e.index);
                            }
                        });
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(PANEL_W);
                ui.set_max_width(PANEL_W + 80.0);
                match pick.or(self.equip_selected) {
                    Some(index) => self.equip_edit_panel(ui, index, &boxv),
                    None => {
                        ui.label(
                            egui::RichText::new("行をクリックすると、ここで編集できます").weak(),
                        );
                    }
                }
            });
        });
        if let Some(p) = pick {
            self.equip_selected = Some(p);
        }
    }

    /// The editor for one equipment record, drawn to the right of the list.
    ///
    /// It works on a copy of the record and hands it back only when a widget
    /// moved, so a frame with no click writes nothing. Everything it changes
    /// goes through the helpers in equip, which own the first 12 bytes and
    /// never touch +0x0C.. - the part the record's type owns.
    fn equip_edit_panel(&mut self, ui: &mut egui::Ui, index: usize, box_bytes: &[u8]) {
        let off = index * mhnx_core::equip::EQUIP_RECORD;
        let Some(rec0) = box_bytes.get(off..off + mhnx_core::equip::EQUIP_RECORD) else {
            ui.label("その枠は読めません");
            return;
        };
        let mut rec = [0u8; mhnx_core::equip::EQUIP_RECORD];
        rec.copy_from_slice(rec0);
        let Some(eq) = mhnx_core::equip::decode_record(index, &rec) else {
            return;
        };
        ui.label(egui::RichText::new(format!("枠 {} の編集", index)).strong());
        if eq.kind == 0 {
            // A new record always lands in the first blank slot, which is not
            // necessarily this one, so the editor points at the row above
            // instead of offering a button that would fill a different index.
            ui.label(
                egui::RichText::new("空き枠です。上の「新規作成」→「空き枠に作成」で作れます。")
                    .weak()
                    .small(),
            );
            return;
        }
        egui::Grid::new("equipinfo")
            .num_columns(2)
            .spacing([8.0, 3.0])
            .show(ui, |ui| {
                ui.label("種別");
                ui.label(mhnx_core::equip::type_name(eq.kind));
                ui.end_row();
                ui.label("装備名");
                ui.label(mhnx_core::equip::name(&eq));
                ui.end_row();
                ui.label("装備ID");
                ui.label(format!("{} (0x{:04X})", eq.id, eq.id));
                ui.end_row();
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("レベル");
            let mut level = eq.level;
            if ui
                .add(egui::DragValue::new(&mut level).range(1..=32))
                .changed()
            {
                mhnx_core::equip::set_level(&mut rec, level);
            }
            ui.label(egui::RichText::new("1〜32").weak().small());
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new("着せ替え").strong());
        let mut remove = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("ID");
            let mut id = eq.transmog;
            if ui
                .add(egui::DragValue::new(&mut id).range(0..=0xFFFF).speed(1.0))
                .changed()
            {
                let lv = mhnx_core::equip::decode_record(index, &rec)
                    .map(|e| e.transmog_level)
                    .unwrap_or(1);
                mhnx_core::equip::set_transmog(&mut rec, id, lv);
            }
            ui.label("元レベル");
            let mut tl = eq.transmog_level;
            if ui
                .add(egui::DragValue::new(&mut tl).range(1..=32))
                .changed()
            {
                let cur = mhnx_core::equip::decode_record(index, &rec)
                    .map(|e| e.transmog)
                    .unwrap_or(0);
                mhnx_core::equip::set_transmog(&mut rec, cur, tl);
            }
            remove = ui
                .small_button("解除")
                .on_hover_text("着せ替えID を 0 に戻します")
                .clicked();
        });
        if remove {
            mhnx_core::equip::set_transmog(&mut rec, 0, 1);
        }
        // Read back, so the line describes the record as it is now rather than
        // as it was when the frame started.
        let now = mhnx_core::equip::decode_record(index, &rec).unwrap_or(eq);
        let look = if now.transmog == 0 {
            String::from("なし（ID 0 で解除）")
        } else {
            transmog_label(&now)
        };
        ui.label(
            egui::RichText::new(format!("見た目: {}", look))
                .weak()
                .small(),
        );
        ui.add_space(4.0);
        ui.label(egui::RichText::new("装飾品").strong());
        let decos = mhnx_core::equip::Equipment::decorations(index, &rec);
        for socket in 0..3 {
            ui.horizontal(|ui| {
                ui.label(format!("{}枠目", socket + 1));
                if let Some(id) = Self::deco_combo(
                    ui,
                    "equip",
                    socket,
                    decos[socket],
                    &mut self.equip_deco_search[socket],
                ) {
                    mhnx_core::equip::set_decoration(&mut rec, socket, id);
                }
            });
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "先頭 12 バイト（種別・Lv・ID・着せ替え・装飾品）だけを書き換えます。+0x0C 以降の持ち物は保持します。",
            )
            .weak()
            .small(),
        );
        ui.separator();
        // Seeded from the record the first time it is drawn, so the row starts
        // out as "this very item" and only differs once the user changes it.
        // Without the seeding, switching rows would leave the previous
        // record's id in the box and one click would write it here.
        if self.equip_rep_for != Some(index) {
            self.equip_rep_for = Some(index);
            self.equip_rep_kind = eq.kind;
            self.equip_rep_id = eq.id;
            self.equip_rep_level = eq.level;
        }
        ui.label(egui::RichText::new("差し替え").strong());
        ui.horizontal_wrapped(|ui| {
            Self::equip_kind_combo(ui, "equiprepkind", &mut self.equip_rep_kind);
            ui.label("ID");
            ui.add(
                egui::DragValue::new(&mut self.equip_rep_id)
                    .range(0..=0xFFFF)
                    .speed(1.0),
            );
            ui.label("Lv");
            ui.add(egui::DragValue::new(&mut self.equip_rep_level).range(1..=32));
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("差し替える")
                .on_hover_text("種別と ID を入れ替え、装飾品 3 枠を空にします")
                .clicked()
            {
                self.confirm = Confirm::EquipReplace {
                    index,
                    kind: self.equip_rep_kind,
                    id: self.equip_rep_id,
                    level: self.equip_rep_level,
                };
            }
            ui.label(
                egui::RichText::new("装飾品は空になります")
                    .weak()
                    .small(),
            );
        });
        if Self::danger_button(ui, "この枠を削除")
            .on_hover_text("確認ダイアログを挟んでから消します")
            .clicked()
        {
            self.confirm = Confirm::EquipClear(index);
        }
        if rec[..] != rec0[..] {
            if self.store_equipment_record(index, &rec) {
                self.message =
                    format!("装備 {} を編集しました（「上書き保存」で書き出します）", index);
            } else {
                self.message = String::from("装備を書き込めませんでした");
            }
        }
    }

    /// Write one edited 36 byte record back into the open save.
    ///
    /// The caller is expected to have changed it through the helpers in equip,
    /// which only own the first 12 bytes; the record is then copied back
    /// wholesale because it came from the same place.
    fn store_equipment_record(&mut self, index: usize, rec: &[u8]) -> bool {
        if rec.len() < mhnx_core::equip::EQUIP_RECORD {
            return false;
        }
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            return false;
        };
        let Some(dst) = save.equipment_record_mut(slot, index) else {
            return false;
        };
        dst[..mhnx_core::equip::EQUIP_RECORD]
            .copy_from_slice(&rec[..mhnx_core::equip::EQUIP_RECORD]);
        self.dirty = true;
        true
    }

    /// Write a new item into the first blank record and select that row.
    ///
    /// The index is whatever the box gave back: the operation goes through
    /// equipops::create, which owns the search for a blank record and returns
    /// the one it filled.
    pub fn equip_create(&mut self, kind: u8, id: u16, level: u16) -> bool {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        match equipops::create(save, slot, kind, id, level) {
            Ok(index) => {
                self.dirty = true;
                self.equip_selected = Some(index);
                self.message = format!(
                    "装備を作成しました（枠 {}・{} ID {}・Lv{}）",
                    index,
                    mhnx_core::equip::type_name(kind),
                    id,
                    level
                );
                true
            }
            Err(e) => {
                self.message = e.text();
                false
            }
        }
    }

    /// Put a different item in an existing record.
    pub fn equip_replace(&mut self, index: usize, kind: u8, id: u16, level: u16) -> bool {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        match equipops::replace(save, slot, index, kind, id, level) {
            Ok(()) => {
                self.dirty = true;
                self.message = format!(
                    "装備 {} を「{}」ID {} に差し替えました（装飾品は空にしました）",
                    index,
                    mhnx_core::equip::type_name(kind),
                    id
                );
                true
            }
            Err(e) => {
                self.message = e.text();
                false
            }
        }
    }

    /// Clear one record. With force = false this refuses while anything still
    /// names the record, and hands those references back in the error.
    pub fn equip_delete(&mut self, index: usize, force: bool) -> Result<(), EquipError> {
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            return Err(EquipError::OutOfRange);
        };
        let out = equipops::clear(save, slot, index, force);
        match &out {
            Ok(()) => {
                self.dirty = true;
                if self.equip_selected == Some(index) {
                    self.equip_selected = None;
                }
                self.message =
                    format!("装備 {} を削除しました（「上書き保存」で書き出します）", index);
            }
            Err(e) => self.message = e.text(),
        }
        out
    }

    /// The first "yes" of the delete dialog.
    ///
    /// It tries the unforced clear, and when the record is still named by
    /// something it turns the same dialog into the second ask, which lists
    /// those references and offers the forced delete as a separate button.
    pub fn equip_delete_checked(&mut self, index: usize) {
        if let Err(EquipError::Referenced(refs)) = self.equip_delete(index, false) {
            self.confirm = Confirm::EquipClearReferenced { index, refs };
        }
    }

    /// Pick one of the 21 hunter equipment types.
    fn equip_kind_combo(ui: &mut egui::Ui, salt: &str, kind: &mut u8) {
        egui::ComboBox::from_id_salt(salt)
            .selected_text(mhnx_core::equip::type_name(*kind))
            .width(150.0)
            .show_ui(ui, |ui| {
                for k in 1..=21u8 {
                    ui.selectable_value(kind, k, mhnx_core::equip::type_name(k));
                }
            });
    }

    /// Searchable decoration picker: item ids 2638 + k, named from gamedata.
    ///
    /// Returns the id that was picked, Some(0) for "take it out", and None when
    /// nothing was clicked this frame.
    #[cfg(feature = "gamedata")]
    fn deco_combo(
        ui: &mut egui::Ui,
        salt: &'static str,
        socket: usize,
        current: u16,
        search: &mut String,
    ) -> Option<u16> {
        let mut picked: Option<u16> = None;
        let shown = if current == 0 {
            String::from("なし")
        } else {
            format!("{} ({})", deco_name(current), current)
        };
        egui::ComboBox::from_id_salt((salt, socket))
            .selected_text(shown)
            .width(230.0)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(search)
                        .hint_text("装飾品名で検索")
                        .desired_width(220.0),
                );
                if ui.small_button("クリア").clicked() {
                    search.clear();
                }
                if current != 0 && ui.small_button("外す").clicked() {
                    picked = Some(0);
                }
                let needle = search.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt((salt, socket))
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for (i, name) in mhnx_core::gamedata::JWL_NAMES.iter().enumerate() {
                            let id =
                                mhnx_core::gamedata::JWL_I_DS.get(i).copied().unwrap_or(0) as u16;
                            if !needle.is_empty()
                                && !name.to_lowercase().contains(&needle)
                                && !format!("{}", id).contains(&needle)
                            {
                                continue;
                            }
                            let label = if id == 0 {
                                String::from("なし (0)")
                            } else {
                                format!("{:<4} {}", id, name)
                            };
                            if ui.selectable_label(current == id, label).clicked() {
                                picked = Some(id);
                            }
                        }
                    });
            });
        if picked.is_some() {
            search.clear();
        }
        picked
    }

    /// Without the name tables there is nothing to search, so the fallback is
    /// the raw id.
    #[cfg(not(feature = "gamedata"))]
    fn deco_combo(
        ui: &mut egui::Ui,
        _salt: &'static str,
        socket: usize,
        current: u16,
        _search: &mut String,
    ) -> Option<u16> {
        let _ = socket;
        let mut id = current;
        if ui
            .add(egui::DragValue::new(&mut id).range(0..=0xFFFF).speed(1.0))
            .changed()
        {
            Some(id)
        } else {
            None
        }
    }

    fn tab_dlc(&mut self, ui: &mut egui::Ui) {
        ui.heading("ダウンロードクエスト");
        let Some(save) = &self.save else {
            ui.label("セーブを開くと、導入済みのクエストを一覧できます");
            return;
        };
        if save.kind() == SaveKind::Mhgu {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "MHGU はセクション配置が違うため、このツールでは扱いません",
            );
            return;
        }
        let quests = dlc::list_quests(save);
        let titles = Self::quest_titles(&quests);
        let equipment = dlc::quest_equipment(save);
        let used = equipment.iter().filter(|e| e.occupied).count();
        // Land on a real loadout instead of an empty panel: the supply editor
        // is the reason most people open this tab.
        if self.qe_slot.is_none() {
            self.qe_slot = equipment
                .iter()
                .find(|e| e.occupied)
                .map(|e| e.index)
                .or_else(|| equipment.first().map(|e| e.index));
        }
        ui.horizontal(|ui| {
            ui.label(format!("導入済み {} 件", quests.len()));
            ui.label("絞り込み");
            ui.text_edit_singleline(&mut self.dlc_filter);
            // Adding a quest needs an archive, so the button only appears when
            // one is open; the id is read from its .mib rather than its name.
            let can_add = self.arc.is_some();
            if ui
                .add_enabled(can_add, egui::Button::new("開いている .arc を追加"))
                .on_hover_text("いま開いているクエストを、空き枠へ新規に導入します")
                .clicked()
            {
                self.add_open_arc_to_save();
            }
        });
        // The 45 quest-equipment entries were only readable once the low mask
        // bits were proven to index this array directly.
        egui::CollapsingHeader::new(format!(
            "クエスト装備 {} / {} 枠",
            used,
            equipment.len()
        ))
        .id_salt("questequip")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "各枠の先頭4バイトがクエストIDです。並びはゲーム側のカタログ順で、ID順ではありません。",
                )
                .weak()
                .small(),
            );
            egui::ScrollArea::vertical()
                .id_salt("questequiplist")
                .max_height(220.0)
                .show(ui, |ui| {
                    egui::Grid::new("questequipgrid")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("枠");
                            ui.strong("クエストID");
                            ui.strong("タイトル");
                            ui.end_row();
                            for e in &equipment {
                                // The slot used to be a plain label, and the
                                // supply box inside it could not be reached
                                // from this side at all: only the CLI could
                                // rewrite those entries.
                                if ui
                                    .selectable_label(
                                        self.qe_slot == Some(e.index),
                                        format!("{}", e.index),
                                    )
                                    .clicked()
                                {
                                    self.qe_slot = Some(e.index);
                                    self.qe_pick = None;
                                }
                                if e.occupied {
                                    ui.label(format!("{}", e.quest_id));
                                    ui.label(
                                        titles
                                            .get(&e.quest_id)
                                            .cloned()
                                            .unwrap_or_default(),
                                    );
                                } else {
                                    ui.weak("—");
                                    ui.weak("空き");
                                }
                                ui.end_row();
                            }
                        });
                });
        });
        // Supply and ammo inside the chosen loadout. The core has had
        // set_quest_equipment_item since the CLI gained quest-equip-set; the
        // window never called it, so an event quest's supply box could only
        // be edited from a terminal.
        // Collapsed by default: open, it takes about 300px and pushes the
        // quest list below the window, which is the thing most people come to
        // this tab for. The title says where to look.
        egui::CollapsingHeader::new(match self.qe_slot {
            Some(s) => format!("枠 {} の支給品・弾ポーチ（個数とアイテムを変更）", s),
            None => "支給品・弾ポーチ（個数とアイテムを変更）".to_owned(),
        })
        .id_salt("qesupply")
        .default_open(false)
        .show(ui, |ui| {
            let Some(slot) = self.qe_slot else {
                ui.label(
                    egui::RichText::new("上の一覧で枠を選ぶと、その中身を編集できます")
                        .weak()
                        .small(),
                );
                return;
            };
            let items = self
                .save
                .as_ref()
                .map(|s| dlc::quest_equipment_items(s, slot))
                .unwrap_or_default();
            ui.label(
                egui::RichText::new(
                    "個数はその場で書き換わります。アイテム名を押すと差し替えられます。",
                )
                .weak()
                .small(),
            );
            // Writes are collected and applied after the widgets are drawn,
            // because the table above borrows the save to read.
            let mut write: Option<(dlc::QuestEquipList, usize, usize, u16, u8)> = None;
            egui::ScrollArea::vertical()
                .id_salt("qesupplylist")
                .max_height(190.0)
                .show(ui, |ui| {
                    egui::Grid::new("qesupplygrid")
                        .striped(true)
                        .num_columns(5)
                        .show(ui, |ui| {
                            ui.strong("種別");
                            ui.strong("レコード");
                            ui.strong("位置");
                            ui.strong("アイテム");
                            ui.strong("個数");
                            ui.end_row();
                            for it in &items {
                                let code: u8 = if matches!(
                                    it.list,
                                    dlc::QuestEquipList::Supply
                                ) {
                                    0
                                } else {
                                    1
                                };
                                ui.label(if code == 0 { "支給品" } else { "弾ポーチ" });
                                ui.label(format!("{}", it.record));
                                ui.label(format!("{}", it.index));
                                let picked = matches!(
                                    self.qe_pick,
                                    Some((ps, pl, pr, pi))
                                        if ps == slot && pl == code && pr == it.record && pi == it.index
                                );
                                if ui
                                    .selectable_label(
                                        picked,
                                        format!("{:<5} {}", it.item, item_name(it.item)),
                                    )
                                    .clicked()
                                {
                                    self.qe_pick = if picked {
                                        None
                                    } else {
                                        Some((slot, code, it.record, it.index))
                                    };
                                }
                                let mut c = it.count as i32;
                                if ui
                                    .add(egui::DragValue::new(&mut c).range(0..=99))
                                    .changed()
                                {
                                    write = Some((it.list, it.index, it.record, it.item, c as u8));
                                }
                                ui.end_row();
                            }
                        });
                });
            if let Some((list, index, record, item, count)) = write {
                if let Some(save) = &mut self.save {
                    match dlc::set_quest_equipment_item(
                        save, slot, record, list, index, item, count,
                    ) {
                        Ok(()) => {
                            self.dirty = true;
                            self.message = format!("枠 {} の {} を x{} にしました", slot, item_name(item), count);
                        }
                        Err(e) => self.message = format!("書き換えできません: {}", e),
                    }
                }
            }
            // Item picker for the entry that was clicked.
            if let Some((ps, pl, pr, pi)) = self.qe_pick {
                if ps == slot {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(format!("レコード {} 位置 {} のアイテムを差し替え", pr, pi));
                        ui.text_edit_singleline(&mut self.item_pick_filter);
                    });
                    let needle = self.item_pick_filter.trim().to_lowercase();
                    if !needle.is_empty() {
                        let target = items
                            .iter()
                            .find(|i| {
                                let c2: u8 = if matches!(i.list, dlc::QuestEquipList::Supply) { 0 } else { 1 };
                                // Match the record too: every loadout has an
                                // entry 0, so index alone always hit record 0.
                                c2 == pl && i.record == pr && i.index == pi
                            })
                            .copied();
                        let mut hit: Option<u16> = None;
                        egui::ScrollArea::vertical()
                            .id_salt("qepicklist")
                            .max_height(120.0)
                            .show(ui, |ui| {
                                let mut shown = 0;
                                for (id, name) in
                                    mhnx_core::gamedata::ITEM_NAME_LIST.iter().enumerate()
                                {
                                    if id == 0 || id > LAST_ITEM_ID {
                                        continue;
                                    }
                                    if !name.to_lowercase().contains(&needle) {
                                        continue;
                                    }
                                    if ui
                                        .selectable_label(false, format!("{:<5} {}", id, name))
                                        .clicked()
                                    {
                                        hit = Some(id as u16);
                                    }
                                    shown += 1;
                                    if shown >= 30 {
                                        break;
                                    }
                                }
                            });
                        if let (Some(id), Some(t)) = (hit, target) {
                            if let Some(save) = &mut self.save {
                                let count = if t.count == 0 { 1 } else { t.count };
                                match dlc::set_quest_equipment_item(
                                    save, slot, t.record, t.list, t.index, id, count,
                                ) {
                                    Ok(()) => {
                                        self.dirty = true;
                                        self.message = format!("位置 {} を {} にしました", pi, item_name(id));
                                        self.qe_pick = None;
                                    }
                                    Err(e) => self.message = format!("書き換えできません: {}", e),
                                }
                            }
                        }
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("抽出先");
            ui.text_edit_singleline(&mut self.dlc_outdir);
            if ui.button("ここへ全件抽出").clicked() {
                let dir = self.dlc_outdir.clone();
                let n = self.extract_dlc(&dir, &[]);
                self.message = format!("{} 件を {} へ書き出しました", n, dir);
            }
        });
        ui.separator();

        let needle = self.dlc_filter.trim().to_lowercase();
        let mut open_edit: Option<i32> = None;
        let mut extract_one: Option<i32> = None;
        let mut wipe: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("dlclist")
            .max_height(420.0)
            .show(ui, |ui| {
                egui::Grid::new("dlcg")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for h in ["枠", "クエスト ID", "サイズ", "タイトル", ""] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for q in &quests {
                            if q.data.is_empty() {
                                continue;
                            }
                            // The .gmd inside the archive carries the
                            // title; decoding it here is what makes the filter
                            // able to match on one at all.
                            let title = Arc::parse(&q.data)
                                .ok()
                                .and_then(|a| {
                                    a.by_ext("gmd")
                                        .and_then(|e| Gmd::from_bytes(&e.data).ok())
                                })
                                .and_then(|g| g.title().map(|t| t.to_owned()))
                                .unwrap_or_default();
                            if !needle.is_empty()
                                && !format!("{}", q.quest_id).contains(&needle)
                                && !title.to_lowercase().contains(&needle)
                            {
                                continue;
                            }
                            ui.label(format!("{}", q.slot));
                            ui.label(format!("{}", q.quest_id));
                            ui.label(format!("{} B", q.size));
                            ui.label(title);
                            if ui.small_button("編集").clicked() {
                                open_edit = Some(q.quest_id);
                            }
                            if ui.small_button("抽出").clicked() {
                                extract_one = Some(q.quest_id);
                            }
                            if Self::danger_button(ui, "空にする").clicked() {
                                wipe = Some(q.slot);
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(id) = open_edit {
            self.open_arc_from_save(id);
            self.tab = Tab::Quest;
        }
        if let Some(id) = extract_one {
            let dir = self.dlc_outdir.clone();
            let n = self.extract_dlc(&dir, &[id]);
            self.message = format!("{} 件を書き出しました", n);
        }
        if let Some(slot) = wipe {
            // Deleting a quest is not undoable, so it asks first.
            self.confirm = Confirm::ClearDlc;
            self.pending_dlc_slot = Some(slot);
        }
    }

    /// Searchable skill picker: 306 Japanese names, index = id - 1.
    fn skill_combo(
        ui: &mut egui::Ui,
        id_salt: &str,
        current: &mut u8,
        search: &mut String,
    ) -> bool {
        let shown = mhnx_core::skills::skill_name(*current)
            .map(|s| format!("{} ({})", s, current))
            .unwrap_or_else(|| format!("なし ({})", current));
        let mut picked: Option<u8> = None;
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(shown)
            .width(260.0)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(search)
                        .hint_text("スキル名で検索")
                        .desired_width(240.0),
                );
                if ui.small_button("クリア").clicked() {
                    search.clear();
                }
                if *current != 0 && ui.small_button("なし に戻す").clicked() {
                    picked = Some(0);
                }
                let needle = search.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt(id_salt)
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for (i, name) in mhnx_core::skills::SKILL_NAMES.iter().enumerate() {
                            let id = (i + 1) as u8;
                            if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                                continue;
                            }
                            if ui
                                .selectable_label(*current == id, format!("{:<4} {}", id, name))
                                .clicked()
                            {
                                picked = Some(id);
                            }
                        }
                    });
            });
        match picked {
            Some(id) => {
                *current = id;
                search.clear();
                true
            }
            None => false,
        }
    }

    /// Talisman editing: the list on the left, the editor on the right.
    fn tab_charm(&mut self, ui: &mut egui::Ui) {
        let Some(save) = &self.save else {
            ui.centered_and_justified(|ui| ui.label("セーブを開いてください"));
            return;
        };
        let charms = save.charms(self.slot).unwrap_or_default();
        // Copied out of the save so the editor closures can hold &mut self.
        // Mutable because a grade switch rewrites the record and the warning
        // line below has to see that write in the same frame.
        let mut charm_box: Vec<u8> = save
            .equipment_box(self.slot)
            .map(|b| b.to_vec())
            .unwrap_or_default();
        let total = charms.len();
        ui.horizontal(|ui| {
            ui.heading("お守り");
            ui.label(format!(
                "スロット {} / {} 個（装備ボックス 2000 枠のうち）",
                self.slot + 1,
                total
            ));
            ui.label("絞り込み");
            ui.add(
                egui::TextEdit::singleline(&mut self.charm_filter)
                    .hint_text("スキル名・番号")
                    .desired_width(180.0),
            );
        });

        let needle = self.charm_filter.trim().to_lowercase();
        let shown: Vec<_> = charms
            .iter()
            .filter(|c| {
                if needle.is_empty() {
                    return true;
                }
                c.summary().to_lowercase().contains(&needle)
                    || format!("{}", c.index).contains(&needle)
            })
            .collect();
        ui.label(format!("{} 件表示", shown.len()));

        let mut pick: Option<usize> = None;
        let mut drop_idx: Option<usize> = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("charmlist")
                    .max_height(460.0)
                    .min_scrolled_height(460.0)
                    .show(ui, |ui| {
                        for c in &shown {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.charm_index == c.index,
                                        format!("{:<6} {}", c.index, c.summary()),
                                    )
                                    .clicked()
                                {
                                    pick = Some(c.index);
                                }
                                // Deleting an entry is destructive and one
                                // click used to be enough, so it asks first.
                                let armed = self.charm_confirm.delete == Some(c.index);
                                let label = if armed { "削除する?" } else { "削除" };
                                if ui.small_button(label).clicked() {
                                    if armed {
                                        drop_idx = Some(c.index);
                                        self.charm_confirm.delete = None;
                                    } else {
                                        self.charm_confirm.delete = Some(c.index);
                                    }
                                }
                            });
                        }
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(360.0);
                ui.label(egui::RichText::new("選択中のお守り").strong());
                // Everything this editor offers is bounded by the grade of the
                // record on screen: how many decoration slots may be picked and
                // how far each skill's points may go. A talisman has a grade of
                // its own; an empty slot borrows the one a new talisman would be
                // made at, so the fields already say what is going to be legal;
                // anything else (a weapon or a piece of armour, which cannot be
                // written from this tab) keeps the wide open fields, because its
                // bytes are not skills at all. The box is copied once per frame,
                // so a grade click lands on the next frame's copy.
                let rec_at = self.charm_index * mhnx_core::charm::CHARM_RECORD;
                let cur_rec = charm_box.get(rec_at..rec_at + mhnx_core::charm::CHARM_RECORD);
                let mut cur_grade = cur_rec.and_then(mhnx_core::charm::rarity_of_record);
                let bound = match cur_grade {
                    Some(g) => Some(g),
                    None if cur_rec.map_or(false, mhnx_core::charm::is_empty_record) => {
                        Some(self.charm_new_grade)
                    }
                    None => None,
                };
                let max_slots = bound.map(|g| g.max_slots()).unwrap_or(3);
                ui.horizontal(|ui| {
                    ui.label("枠");
                    ui.add(egui::DragValue::new(&mut self.charm_index).range(0..=1999));
                    ui.label(egui::RichText::new("装飾スロット").weak());
                    // Only the grades' own maximum is offered: 1 / 2 / 3 / 3.
                    // A record can still hold more than its grade allows (the
                    // game forbids writing it, not reading it), which is what
                    // grade_warnings below reports.
                    for n in 0..=max_slots {
                        if ui
                            .selectable_label(self.charm_slots == n, "●".repeat(n as usize))
                            .clicked()
                        {
                            self.charm_slots = n;
                        }
                    }
                    if max_slots < 3 {
                        ui.label(
                            egui::RichText::new(format!("最大 {} 個", max_slots))
                                .weak()
                                .small(),
                        );
                    }
                });
                ui.add_space(4.0);
                // Wrapped, not a plain row: the skill combo is 260pt wide, so
                // a single row pushed the points field off the right edge.
                ui.horizontal_wrapped(|ui| {
                    ui.label("スキル1");
                    Self::skill_combo(ui, "sk1", &mut self.charm_skill1, &mut self.skill_search1);
                    ui.label("ポイント");
                    let allowed = Self::charm_allowed(bound, self.charm_skill1, false);
                    Self::charm_points_field(ui, allowed, &mut self.charm_points1);
                });
                // Read after the row, so switching the skill in the combo above
                // is explained on the same frame.
                Self::charm_points_note(
                    ui,
                    Self::charm_allowed(bound, self.charm_skill1, false),
                    self.charm_skill1,
                    "スキル1",
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label("スキル2");
                    Self::skill_combo(ui, "sk2", &mut self.charm_skill2, &mut self.skill_search2);
                    ui.label("ポイント");
                    let allowed = Self::charm_allowed(bound, self.charm_skill2, true);
                    Self::charm_points_field(ui, allowed, &mut self.charm_points2);
                });
                Self::charm_points_note(
                    ui,
                    Self::charm_allowed(bound, self.charm_skill2, true),
                    self.charm_skill2,
                    "スキル2",
                );
                // The published per-rarity range. The indexing was resolved
                // from the reference editor: the byte at +0x02 is the talisman
                // type, TALISMAN_RARITY[type] the rarity, and the table is
                // indexed by skill id - 1. Verified against all 650 talismans
                // in the sample save.
                if !charm_box.is_empty() {
                    // The editor used to build every talisman from the most
                    // common record in the box, which is always rarity 100, so
                    // the lower grades could not be made at all. The grade is
                    // now something you choose.
                    let mut apply_grade: Option<mhnx_core::charm::TalismanGrade> = None;
                    ui.horizontal_wrapped(|ui| {
                        ui.label("レア度");
                        for g in mhnx_core::charm::TalismanGrade::ALL {
                            let is_cur = cur_grade == Some(g);
                            let resp = ui
                                .add(egui::Button::new(g.name()).selected(is_cur))
                                .on_hover_text(format!(
                                    "装飾スロットは最大 {} 個",
                                    g.max_slots()
                                ));
                            if resp.clicked() && !is_cur {
                                apply_grade = Some(g);
                            }
                        }
                    });
                    if let Some(g) = apply_grade {
                        let mut ok = false;
                        if let Some(s) = &mut self.save {
                            if let Some(b) = s.equipment_box_mut(self.slot) {
                                ok = mhnx_core::charm::set_grade(b, self.charm_index, g);
                                if ok {
                                    mhnx_core::charm::clamp_points(b, self.charm_index, g);
                                    mhnx_core::charm::clamp_slots(b, self.charm_index, g);
                                }
                            }
                        }
                        if ok {
                            self.dirty = true;
                            self.charm_new_grade = g;
                            // The switch is already written, so the bounds and
                            // the warning line below must describe the record as
                            // it is now rather than as it was at frame start.
                            cur_grade = Some(g);
                            // Reflect what the clamps did to the record.
                            self.charm_sync_from_record();
                            charm_box = self
                                .save
                                .as_ref()
                                .and_then(|s| s.equipment_box(self.slot))
                                .map(|b| b.to_vec())
                                .unwrap_or_default();
                            self.message = format!(
                                "レア度を {} に変更しました（許容範囲とスロットを調整）",
                                g.name()
                            );
                        } else {
                            self.message = format!("{} に変更できませんでした", g.name());
                        }
                    }
                    // Everything about this record the game would not have
                    // written for its grade: slots above the maximum, points
                    // outside the grade's table, a skill the grade cannot
                    // carry. The core owns that knowledge, so the UI only
                    // prints what it returns, one line each, in the same
                    // yellow the other out-of-range notes use.
                    for warning in mhnx_core::charm::grade_warnings(
                        &charm_box,
                        self.charm_index,
                        cur_grade.unwrap_or(self.charm_new_grade),
                    ) {
                        ui.colored_label(egui::Color32::from_rgb(235, 190, 90), warning);
                    }
                    let ty = charm_box.get(rec_at + 2).copied().unwrap_or(0);
                    let rarity = mhnx_core::charm::rarity_of(ty);
                    ui.label(
                        egui::RichText::new(format!(
                            "種別 0x{:02X} / レア度 {}",
                            ty,
                            rarity.map(|r| r.to_string()).unwrap_or_else(|| "-".into())
                        ))
                        .weak()
                        .small(),
                    );
                    for (label, sid, pts, second) in [
                        ("スキル1", self.charm_skill1, self.charm_points1, false),
                        ("スキル2", self.charm_skill2, self.charm_points2, true),
                    ] {
                        if sid == 0 {
                            continue;
                        }
                        let Some(r) = mhnx_core::charm::allowed_points(ty, sid, second) else {
                            continue;
                        };
                        let name = mhnx_core::skills::skill_name(sid).unwrap_or("?");
                        if r.min == 0 && r.max == 0 {
                            ui.colored_label(
                                egui::Color32::from_rgb(235, 120, 120),
                                format!("{} {} はこの種別では付けられません（0〜0）", label, name),
                            );
                        } else if pts < r.min || pts > r.max {
                            ui.colored_label(
                                egui::Color32::from_rgb(235, 190, 90),
                                format!(
                                    "{} {} は {:+} ですが、この種別の許容範囲は {:+}〜{:+}",
                                    label, name, pts, r.min, r.max
                                ),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {} 許容範囲 {:+}〜{:+}",
                                    label, name, r.min, r.max
                                ))
                                .weak()
                                .small(),
                            );
                        }
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("適用").clicked() {
                        let c = mhnx_core::charm::Charm {
                            index: self.charm_index,
                            slots: self.charm_slots,
                            skill1: self.charm_skill1,
                            points1: self.charm_points1,
                            skill2: self.charm_skill2,
                            points2: self.charm_points2,
                        };
                        // The grade is part of what this button writes: on an
                        // empty slot it picks the real record the talisman is
                        // seeded from, and on an existing one it is applied with
                        // the five edited fields, so what is on screen is what
                        // lands in the box.
                        let grade = cur_grade.unwrap_or(self.charm_new_grade);
                        let written = match &mut self.save {
                            Some(s) => s.set_charm_with_grade(self.slot, &c, grade),
                            None => false,
                        };
                        if written {
                            self.dirty = true;
                            self.charm_new_grade = grade;
                            // The call clamps the points to the grade, so read
                            // the record back instead of still showing a value
                            // it no longer holds.
                            self.charm_sync_from_record();
                            self.message =
                                format!("お守り {} を更新しました（{}）", c.index, grade.name());
                        } else {
                            self.message =
                                String::from("書き込めません（武器・防具の枠は保護されています）");
                        }
                    }
                    // Find the first free box slot instead of making the user
                    // type one: charm::first_free already knows where it is.
                    if ui
                        .button("空き枠に新規作成")
                        .on_hover_text("最初の空きスロットにお守りを作ります")
                        .clicked()
                    {
                        let free = match &self.save {
                            Some(s) => s
                                .equipment_box(self.slot)
                                .and_then(|b| mhnx_core::charm::first_free(&b)),
                            None => None,
                        };
                        let Some(free) = free else {
                            self.message = String::from("空き枠がありません");
                            return;
                        };
                        self.charm_index = free;
                        let new = mhnx_core::charm::Charm {
                            index: self.charm_index,
                            slots: 0,
                            skill1: 0,
                            points1: 0,
                            skill2: 0,
                            points2: 0,
                        };
                        let grade = self.charm_new_grade;
                        if let Some(s) = &mut self.save {
                            if s.set_charm_with_grade(self.slot, &new, grade) {
                                self.dirty = true;
                                self.message = format!(
                                    "空き枠 {} に {} を作りました",
                                    self.charm_index,
                                    grade.name()
                                );
                            } else {
                                // set_charm_with_grade seeds a new record from
                                // a real one of the requested grade, so this is
                                // about the grade, not about the slot: the slot
                                // just came back from first_free.
                                self.message = format!(
                                    "このレア度では作成できません（{} のレコードが装備ボックスにありません）",
                                    grade.name()
                                );
                            }
                        }
                    }
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "装飾スロットは ● の数、ポイントは符号付きです。レア度を変えると、その範囲外のポイントとスロットは自動で調整されます。新規作成は最後に選んだレア度で作られます。",
                    )
                    .weak()
                    .small(),
                );
            });
        });

        if let Some(i) = pick {
            if let Some(c) = charms.iter().find(|c| c.index == i) {
                self.charm_index = c.index;
                self.charm_slots = c.slots;
                self.charm_skill1 = c.skill1;
                self.charm_points1 = c.points1;
                self.charm_skill2 = c.skill2;
                self.charm_points2 = c.points2;
            }
        }
        if let Some(i) = drop_idx {
            if let Some(s) = &mut self.save {
                if s.clear_charm(self.slot, i) {
                    self.dirty = true;
                    self.message = format!("お守り {} を削除しました", i);
                }
            }
        }
    }

    /// Pull the five fields this editor owns back out of the record in the box.
    ///
    /// The core clamps what it writes, so after an apply or a grade switch the
    /// record is the truth and the fields have to follow it, not the reverse.
    fn charm_sync_from_record(&mut self) {
        let Some(b) = self.save.as_ref().and_then(|s| s.equipment_box(self.slot)) else {
            return;
        };
        let o = self.charm_index * mhnx_core::charm::CHARM_RECORD;
        if o + mhnx_core::charm::CHARM_RECORD > b.len() {
            return;
        }
        self.charm_slots = b[o + 0x10];
        self.charm_skill1 = b[o + 0x0C];
        self.charm_points1 = b[o + 0x0E] as i8;
        self.charm_skill2 = b[o + 0x0D];
        self.charm_points2 = b[o + 0x0F] as i8;
    }

    /// Legal points for one skill at the grade in effect, if there is one.
    ///
    /// The range comes from the grade's own table rather than from the type
    /// byte, because the grade is what this editor lets the user switch. None
    /// means "no bound to apply": no grade, no skill picked, or a skill the
    /// tables say nothing about.
    fn charm_allowed(
        grade: Option<mhnx_core::charm::TalismanGrade>,
        skill: u8,
        second: bool,
    ) -> Option<mhnx_core::charm::AllowedPoints> {
        match grade {
            Some(g) if skill != 0 => {
                mhnx_core::charm::allowed_points_rarity(g.rarity(), skill, second)
            }
            _ => None,
        }
    }

    /// One talisman points field, limited to what the grade allows.
    ///
    /// A 0〜0 range means the grade carries no such skill at all: the field is
    /// disabled and the note below the row explains it. The value is not
    /// cleared, so a value typed for another grade survives (the core leaves
    /// the record alone for the same reason and warns instead).
    fn charm_points_field(
        ui: &mut egui::Ui,
        allowed: Option<mhnx_core::charm::AllowedPoints>,
        points: &mut i8,
    ) {
        match allowed {
            Some(r) if r.min == 0 && r.max == 0 => {
                ui.add_enabled(false, Self::charm_points_drag(points, None));
            }
            _ => {
                ui.add(Self::charm_points_drag(points, allowed));
            }
        }
    }

    /// The "cannot be placed at this grade" note, on a line of its own.
    ///
    /// Inline it was squeezed beside the field and clipped by the panel edge;
    /// a full width row keeps the whole sentence readable.
    fn charm_points_note(
        ui: &mut egui::Ui,
        allowed: Option<mhnx_core::charm::AllowedPoints>,
        skill: u8,
        row: &str,
    ) {
        let Some(r) = allowed else {
            return;
        };
        if r.min != 0 || r.max != 0 {
            return;
        }
        let name = mhnx_core::skills::skill_name(skill).unwrap_or("?");
        ui.colored_label(
            egui::Color32::from_rgb(235, 190, 90),
            format!("{} {} はこのレア度では付けられません", row, name),
        );
    }

    /// A points widget that prints the sign itself.
    ///
    /// prefix("+") alone reads "+-7" as soon as the range reaches below zero,
    /// which the grade tables do, so the sign is formatted here. The value is
    /// not clamped merely by being displayed: a record can hold something its
    /// grade would not allow, and that has to stay visible until the user edits
    /// it or presses 適用.
    fn charm_points_drag(
        points: &mut i8,
        allowed: Option<mhnx_core::charm::AllowedPoints>,
    ) -> egui::DragValue<'_> {
        let (low, high) = match allowed {
            Some(r) if r.min <= r.max => (r.min, r.max),
            Some(r) => (r.max, r.min),
            None => (-128, 127),
        };
        egui::DragValue::new(points)
            .range(low..=high)
            .clamp_existing_to_range(false)
            .custom_formatter(|n, _| {
                let v = n as i8;
                if v > 0 {
                    format!("+{}", v)
                } else {
                    format!("{}", v)
                }
            })
            .custom_parser(|s| {
                let t = s.trim();
                let negative = t.starts_with('-');
                let digits = t.trim_start_matches(|c: char| c == '+' || c == '-');
                if digits.is_empty() {
                    return None;
                }
                digits
                    .parse::<f64>()
                    .ok()
                    .map(|v| if negative { -v } else { v })
            })
    }

    fn tab_extras(&mut self, ui: &mut egui::Ui) {
        if self.save.is_none() {
            ui.label("セーブを開いてください");
            return;
        }
        // Everything is gathered up front and applied at the end: the buttons
        // need &mut self.save while the widgets borrow it for reading.
        // Only the few values the tab actually shows are copied out; cloning
        // the whole 4.7MB save here cost about a millisecond every frame.
        let mut set_opts: Option<mhnx_core::save::Options> = None;
        let mut add_cat: Option<(usize, usize)> = None;
        let mut drop_cat: Option<usize> = None;
        let mut drop_charm: Option<usize> = None;
        let mut write_charm = false;
        let mut want_export = false;
        let mut want_import = false;

        ui.heading("本体オプション");
        let o = self.save.as_ref().unwrap().options();
        ui.horizontal(|ui| {
            ui.label("明るさ");
            let mut b = o.brightness as i32;
            if ui.add(egui::Slider::new(&mut b, 0..=255)).changed() {
                let mut n = o;
                n.brightness = b as u8;
                set_opts = Some(n);
            }
        });
        let mut rumble = o.rumble != 0;
        if ui.checkbox(&mut rumble, "振動").changed() {
            let mut n = o;
            n.rumble = if rumble { 1 } else { 0 };
            set_opts = Some(n);
        }

        ui.separator();
        ui.heading("配信オトモ");
        let cats: Vec<_> = self
            .save
            .as_ref()
            .map(|s| s.dlc_palicoes())
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c.id != 0)
            .collect();
        ui.label(format!("{} / 50 枠 使用中", cats.len()));
        // Bounded: a scroll area that grows pushes everything below off screen.
        egui::ScrollArea::vertical()
            .id_salt("palicolist")
            .max_height(140.0)
            .show(ui, |ui| {
                for c in &cats {
                    ui.horizontal(|ui| {
                        ui.label(format!("#{:<3} id 0x{:02X} {}", c.index, c.id, c.name));
                        if ui.small_button("外す").clicked() {
                            drop_cat = Some(c.index);
                        }
                    });
                }
            });
        ui.horizontal(|ui| {
            ui.label("日本語版");
            egui::ComboBox::from_id_salt("catlang")
                .selected_text(if self.cat_lang == 0 { "日本語版" } else { "英語版" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cat_lang, 0, "日本語版");
                    ui.selectable_value(&mut self.cat_lang, 1, "英語版");
                });
            let lang = if self.cat_lang == 0 {
                mhnx_core::dlcats::Lang::Jp
            } else {
                mhnx_core::dlcats::Lang::En
            };
            let ids = mhnx_core::dlcats::all_ids();
            // Asked once per frame instead of once per combo entry.
            let used: Vec<bool> = match &self.save {
                Some(s) => ids.iter().map(|id| s.has_dlc_palico(*id)).collect(),
                None => Vec::new(),
            };
            let shown = ids
                .get(self.cat_id)
                .and_then(|id| mhnx_core::dlcats::record(lang, *id))
                .map(mhnx_core::dlcats::record_name)
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("catid")
                .selected_text(shown)
                .width(240.0)
                .show_ui(ui, |ui| {
                    for (i, id) in ids.iter().enumerate() {
                        let name = mhnx_core::dlcats::record(lang, *id)
                            .map(mhnx_core::dlcats::record_name)
                            .unwrap_or_default();
                        let used = used.get(i).copied().unwrap_or(false);
                        ui.selectable_value(
                            &mut self.cat_id,
                            i,
                            if used {
                                format!("{} (導入済み)", name)
                            } else {
                                name
                            },
                        );
                    }
                });
            if ui.button("追加").clicked() {
                add_cat = Some((self.cat_lang, self.cat_id));
            }
        });

        ui.separator();
        ui.heading("お守り");
        let charms = self
            .save
            .as_ref()
            .and_then(|s| s.charms(self.slot))
            .unwrap_or_default();
        ui.label(format!("スロット {}: {} 個のお守り", self.slot + 1, charms.len()));
        egui::ScrollArea::vertical()
            .id_salt("charms")
            .max_height(150.0)
            .show(ui, |ui| {
                for c in &charms {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                self.charm_index == c.index,
                                format!("#{:<5} {}", c.index, c.summary()),
                            )
                            .clicked()
                        {
                            self.charm_index = c.index;
                            self.charm_slots = c.slots;
                            self.charm_skill1 = c.skill1;
                            self.charm_points1 = c.points1;
                            self.charm_skill2 = c.skill2;
                            self.charm_points2 = c.points2;
                        }
                        if ui.small_button("削除").clicked() {
                            drop_charm = Some(c.index);
                        }
                    });
                }
            });
        ui.horizontal(|ui| {
            ui.label("枠");
            ui.add(egui::DragValue::new(&mut self.charm_index).range(0..=1999));
            ui.label("装飾スロット");
            ui.add(egui::DragValue::new(&mut self.charm_slots).range(0..=3));
        });
        ui.horizontal(|ui| {
            ui.label("スキル1");
            ui.add(egui::DragValue::new(&mut self.charm_skill1).range(0..=255));
            let mut p = self.charm_points1 as i32;
            if ui.add(egui::DragValue::new(&mut p).range(-128..=127)).changed() {
                self.charm_points1 = p as i8;
            }
            ui.label("スキル2");
            ui.add(egui::DragValue::new(&mut self.charm_skill2).range(0..=255));
            let mut p2 = self.charm_points2 as i32;
            if ui.add(egui::DragValue::new(&mut p2).range(-128..=127)).changed() {
                self.charm_points2 = p2 as i8;
            }
        });
        if ui.button("この内容で書き込む").clicked() {
            write_charm = true;
        }

        ui.separator();
        ui.heading("キャラクタをまるごと入出力");
        ui.label(
            egui::RichText::new("1 スロット分（キャラクタ本体 + アイテム）を .saveslot として出し入れします")
                .weak()
                .small(),
        );
        ui.horizontal(|ui| {
            if ui.button("このスロットを書き出す").clicked() {
                want_export = true;
            }
            if ui.button("このスロットに読み込む").clicked() {
                want_import = true;
            }
        });

        // ---- apply ----
        if let Some(o) = set_opts {
            if let Some(s) = &mut self.save {
                s.set_options(&o);
                self.dirty = true;
                self.message = String::from("オプションを更新しました");
            }
        }
        if let Some((lang, idx)) = add_cat {
            let ls = if lang == 0 {
                mhnx_core::dlcats::Lang::Jp
            } else {
                mhnx_core::dlcats::Lang::En
            };
            let ids = mhnx_core::dlcats::all_ids();
            if let Some(id) = ids.get(idx).copied() {
                if let Some(s) = &mut self.save {
                    match s.install_dlc_palico(ls, id) {
                        Ok(slot) => {
                            self.dirty = true;
                            self.message = format!("オトモ 0x{:02X} を枠 {} に追加しました", id, slot);
                        }
                        Err(e) => self.message = format!("追加できません: {}", e),
                    }
                }
            }
        }
        if let Some(i) = drop_cat {
            if let Some(s) = &mut self.save {
                if s.remove_dlc_palico(i) {
                    self.dirty = true;
                    self.message = format!("枠 {} を外しました", i);
                }
            }
        }
        if let Some(i) = drop_charm {
            if let Some(s) = &mut self.save {
                if s.clear_charm(self.slot, i) {
                    self.dirty = true;
                    self.message = format!("お守り {} を削除しました", i);
                }
            }
        }
        if write_charm {
            let c = mhnx_core::charm::Charm {
                index: self.charm_index,
                slots: self.charm_slots,
                skill1: self.charm_skill1,
                points1: self.charm_points1,
                skill2: self.charm_skill2,
                points2: self.charm_points2,
            };
            if let Some(s) = &mut self.save {
                if s.set_charm(self.slot, &c) {
                    self.dirty = true;
                    self.message = format!("お守り {} を更新しました", c.index);
                } else {
                    self.message =
                        String::from("書き込めません（武器・防具の枠は保護されています）");
                }
            }
        }
        if want_export {
            if let Some(p) = rfd::FileDialog::new()
                .set_file_name(format!("slot{}.saveslot", self.slot + 1))
                .save_file()
            {
                if let Some(bytes) = self.save.as_ref().and_then(|s| s.export_slot(self.slot)) {
                    match std::fs::write(&p, &bytes) {
                        Ok(()) => self.message = format!("書き出しました: {}", p.display()),
                        Err(e) => self.message = format!("書き出せません: {}", e),
                    }
                }
            }
        }
        if want_import {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("save slot", &["saveslot"])
                .pick_file()
            {
                match std::fs::read(&p) {
                    Ok(bytes) => {
                        let mut err = None;
                        if let Some(s) = &mut self.save {
                            if let Err(e) = s.import_slot(self.slot, &bytes) {
                                err = Some(format!("{}", e));
                            }
                        }
                        match err {
                            Some(e) => self.message = e,
                            None => {
                                self.dirty = true;
                                self.reload_from_save();
                                self.message = format!("読み込みました: {}", p.display());
                            }
                        }
                    }
                    Err(e) => self.message = format!("読めません: {}", e),
                }
            }
        }
    }

    fn tab_about(&mut self, ui: &mut egui::Ui) {
        ui.heading("XXSE");
        ui.label("Monster Hunter XX / MHGU のセーブとクエストを編集するツールです。");
        ui.add_space(6.0);
        ui.label("・セーブは 3DS / Switch の両形式に対応");
        ui.label("・ダウンロードクエストの一覧・差し替え・抽出");
        ui.label("・.arc 内の .mib / .gmd / .rem / .sup / .sem / .esl / .qdp / .qdl を表示");
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("実機での動作は未検証です。必ずセーブを控えてから使ってください。")
                .weak(),
        );
    }
}

// ----------------------------------------------------- progress (進行状況)

impl MhnxEditor {
    /// Resolve the launch-time hooks that main.rs does not know about.
    ///
    /// main.rs turns MHNXEDITOR_OPEN_TAB into a Tab before the window exists,
    /// and its list predates this tab; the section and the romfs folder have no
    /// list at all. Both are read here, once, so an ordinary run reads no
    /// environment and the screenshot harness can reach every section without
    /// a human click.
    fn apply_launch_env(&mut self) {
        if self.launch_env_done {
            return;
        }
        self.launch_env_done = true;
        if let Ok(t) = std::env::var("MHNXEDITOR_OPEN_TAB") {
            if t.trim().eq_ignore_ascii_case("progress") {
                self.tab = Tab::Progress;
            }
        }
        if let Ok(s) = std::env::var("MHNXEDITOR_PROGRESS_SECTION") {
            self.progress_section = match s.trim() {
                "requests" | "request" | "依頼" => ProgressSection::Requests,
                "quests" | "quest" | "クエスト" => ProgressSection::Quests,
                _ => ProgressSection::Awards,
            };
        }
        if let Ok(p) = std::env::var("MHNXEDITOR_ROMFS") {
            // The line the window opened with (the font, or the file that was
            // opened) says more than this report until the user asks for it.
            let opened_with = self.message.clone();
            self.romfs_path = p;
            self.load_romfs();
            if !opened_with.is_empty() {
                self.message = opened_with;
            }
        }
    }

    /// Find the three game tables under the folder the user named.
    ///
    /// The retail layout puts them in nativeNX/table; the other spellings cover
    /// a folder that was picked one level too high (the romfs root) or too low
    /// (the table folder itself). A folder that holds only some of them is used
    /// for what it does hold: the medal table alone is still worth reading.
    fn find_romfs_tables(root: &std::path::Path) -> Option<RomfsTables> {
        // A file that was picked instead of a folder still names the folder its
        // neighbours live in.
        let root: PathBuf = if root.is_file() {
            root.parent()?.to_path_buf()
        } else {
            root.to_path_buf()
        };
        let mut best: Option<RomfsTables> = None;
        let mut best_score = 0usize;
        for dir in [
            root.clone(),
            root.join("romfs"),
            root.join("nativeNX").join("table"),
            root.join("romfs").join("nativeNX").join("table"),
            root.join("table"),
        ] {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut awards: Vec<PathBuf> = Vec::new();
            let mut names: Vec<PathBuf> = Vec::new();
            let mut atd: Vec<PathBuf> = Vec::new();
            for entry in rd.flatten() {
                let path = entry.path();
                let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let lower = file.to_ascii_lowercase();
                if lower.starts_with("gc_medal") && lower.ends_with(".gmd") {
                    awards.push(path);
                } else if lower.starts_with("activitydata") && lower.ends_with(".gmd") {
                    names.push(path);
                } else if lower.starts_with("activitydata") && lower.ends_with(".atd") {
                    atd.push(path);
                }
            }
            let score = (!awards.is_empty()) as usize
                + (!names.is_empty()) as usize
                + (!atd.is_empty()) as usize;
            if score > best_score {
                best_score = score;
                best = Some(RomfsTables {
                    dir: dir.clone(),
                    award: Self::prefer_japanese(awards),
                    names: Self::prefer_japanese(names),
                    atd: Self::prefer_japanese(atd),
                });
            }
        }
        best
    }

    /// The Japanese table when a folder holds several languages, else the first.
    ///
    /// The interface is Japanese, and so is the dump every offset in this tool
    /// was checked against: a folder holding GC_Medal_jpn.gmd next to
    /// GC_Medal_eng.gmd must not show the English names.
    fn prefer_japanese(mut files: Vec<PathBuf>) -> Option<PathBuf> {
        files.sort();
        let japanese = files
            .iter()
            .find(|p| p.to_string_lossy().to_ascii_lowercase().contains("jpn"))
            .cloned();
        japanese.or_else(|| files.into_iter().next())
    }


    /// Read the tables under root, or None when nothing there looks like romfs.
    fn read_romfs(root: &std::path::Path) -> Option<RomfsLoad> {
        let tables = Self::find_romfs_tables(root)?;
        let award_names = tables
            .award
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| romfstables::award_names(&b).names)
            .unwrap_or_default();
        let request_names = tables
            .names
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| romfstables::request_names(&b))
            .unwrap_or_default();
        let request_records = tables
            .atd
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| romfstables::request_records(&b))
            .unwrap_or_default();
        if award_names.is_empty() && request_names.is_empty() && request_records.is_empty() {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        parts.push(if award_names.is_empty() {
            String::from("勲章名 0（GC_Medal*.gmd が読めません）")
        } else {
            format!("勲章名 {}", award_names.len())
        });
        parts.push(if request_names.is_empty() {
            String::from("依頼名 0（activityData*.gmd が読めません）")
        } else {
            format!("依頼名 {}", request_names.len())
        });
        parts.push(if request_records.is_empty() {
            String::from("依頼レコード 0（activityData.atd が読めません）")
        } else {
            format!("依頼レコード {}", request_records.len())
        });
        Some(RomfsLoad {
            report: format!(
                "{} から読み込みました: {}",
                tables.dir.display(),
                parts.join(" / ")
            ),
            dir: tables.dir,
            award_names,
            request_names,
            request_records,
        })
    }

    /// Read the three tables out of the romfs folder that is in the field.
    ///
    /// None of this is data of this repository's: the names come from the
    /// user's own dump, are held in memory only, and are never written into a
    /// save or any other file.
    pub fn load_romfs(&mut self) {
        let raw = self.romfs_path.trim().to_owned();
        if raw.is_empty() {
            self.message = "romfs のフォルダを指定してください".to_owned();
            return;
        }
        match Self::read_romfs(std::path::Path::new(&raw)) {
            Some(load) => {
                self.romfs_dir = Some(load.dir);
                self.romfs_status = load.report.clone();
                self.award_names = load.award_names;
                self.request_names = load.request_names;
                self.request_records = load.request_records;
                self.message = load.report;
            }
            None => {
                // The names of another folder must not stay on screen under
                // this path: the tab would show one dump's text beside another
                // dump's flag numbers.
                self.romfs_dir = None;
                self.award_names.clear();
                self.request_names.clear();
                self.request_records.clear();
                self.romfs_status = format!(
                    "{} に romfs の table が見つかりません（nativeNX/table を探しました）",
                    raw
                );
                self.message = self.romfs_status.clone();
            }
        }
    }

    /// Drop the names that were read, keeping the path so it can be retried.
    pub fn clear_romfs(&mut self) {
        self.award_names.clear();
        self.request_names.clear();
        self.request_records.clear();
        self.romfs_dir = None;
        self.romfs_status = String::from("romfs を読み込んでいません");
        self.message = "romfs の読み込みを解除しました".to_owned();
    }

    /// One award bit, false when the slot holds no character.
    fn award_on(&self, award: usize) -> bool {
        self.save
            .as_ref()
            .and_then(|s| awards::has(s, self.slot, award))
            .unwrap_or(false)
    }

    /// One quest progress bit.
    fn quest_on(&self, index: usize, kind: Progress) -> bool {
        self.save
            .as_ref()
            .and_then(|s| questprogress::get(s, self.slot, kind, index))
            .unwrap_or(false)
    }

    /// The 受理 and 完了 bits of one request record.
    fn request_on(&self, record: usize) -> (bool, bool) {
        let (Some(r), Some(save)) = (self.request_records.get(record), self.save.as_ref()) else {
            return (false, false);
        };
        (
            requests::get(save, self.slot, r.accept_flag as usize).unwrap_or(false),
            requests::get(save, self.slot, r.complete_flag as usize).unwrap_or(false),
        )
    }

    /// The name shown for a request: the table's, or its number.
    fn request_title(&self, record: usize) -> String {
        match self.request_names.get(record).map(|n| n.trim()) {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => match self.request_records.get(record) {
                Some(r) => format!("依頼 {}", r.index + 1),
                None => format!("依頼 {}", record + 1),
            },
        }
    }

    /// The accept or complete flag number of one request record.
    fn request_flag(&self, record: usize, complete: bool) -> Option<usize> {
        let r = self.request_records.get(record)?;
        Some(if complete {
            r.complete_flag as usize
        } else {
            r.accept_flag as usize
        })
    }

    /// An award label short enough for a grid cell.
    ///
    /// Without a medal table the number is all there is, so it carries the
    /// label on its own; with one, the number stays in front of the name so a
    /// cell can still be matched against a guide.
    fn award_label(&self, award: usize) -> String {
        match self.award_names.get(award).map(|n| n.trim()) {
            Some(n) if !n.is_empty() => format!("{:03} {}", award + 1, Self::short_name(n)),
            _ => format!("勲章 {:03}", award + 1),
        }
    }

    /// Cut a table name down to the cell width, keeping the start of it.
    fn short_name(name: &str) -> String {
        let mut out: String = name.chars().take(8).collect();
        if name.chars().count() > 8 {
            out.push('…');
        }
        out
    }


    /// Keep the bytes of the given regions before they are changed.
    fn progress_snapshot(&self, label: String, regions: &[(usize, usize)]) -> Option<ProgressUndo> {
        let save = self.save.as_ref()?;
        let slot = self.slot;
        let base = save.slot_base(slot)? as usize;
        let mut kept = Vec::with_capacity(regions.len());
        for (off, len) in regions {
            let start = base.checked_add(*off)?;
            let end = start.checked_add(*len)?;
            kept.push((*off, save.body().get(start..end)?.to_vec()));
        }
        Some(ProgressUndo {
            label,
            slot,
            regions: kept,
        })
    }

    /// Remember one step, dropping the oldest once the stack is full.
    fn push_progress_undo(&mut self, step: ProgressUndo) {
        self.progress_undo.push(step);
        if self.progress_undo.len() > 64 {
            self.progress_undo.remove(0);
        }
    }

    /// Put the bytes of the newest step back.
    pub fn undo_progress(&mut self) {
        let Some(step) = self.progress_undo.pop() else {
            self.message = "戻せる変更がありません".to_owned();
            return;
        };
        let mut restored = 0usize;
        if let Some(save) = &mut self.save {
            if let Some(base) = save.slot_base(step.slot).map(|b| b as usize) {
                for (off, bytes) in &step.regions {
                    let Some(start) = base.checked_add(*off) else {
                        continue;
                    };
                    let Some(end) = start.checked_add(bytes.len()) else {
                        continue;
                    };
                    if let Some(dst) = save.body_mut().get_mut(start..end) {
                        dst.copy_from_slice(bytes);
                        restored += 1;
                    }
                }
            }
        }
        if restored == step.regions.len() && restored > 0 {
            self.dirty = true;
            self.message = format!("元に戻しました: {}", step.label);
        } else {
            self.message = "戻せませんでした（スロットの位置が変わりました）".to_owned();
        }
    }

    /// Grant or revoke one award and mark the save dirty.
    ///
    /// awards::set writes all three copies - one copy alone is undone by the
    /// game - so the bytes of all three are kept first and 元に戻す puts the
    /// whole set back.
    pub fn set_award(&mut self, award: usize, on: bool) {
        let (label, done) = if on {
            (
                format!("勲章 {} を取得", award + 1),
                format!("勲章 {} を取得しました", award + 1),
            )
        } else {
            (
                format!("勲章 {} を未取得に戻す", award + 1),
                format!("勲章 {} を未取得に戻しました", award + 1),
            )
        };
        let Some(step) = self.progress_snapshot(label, &awards::COPIES) else {
            self.message = "先にセーブを開いてください（未使用のスロットは編集できません）".to_owned();
            return;
        };
        let slot = self.slot;
        let ok = match &mut self.save {
            Some(save) => awards::set(save, slot, award, on),
            None => false,
        };
        if !ok {
            self.message = format!("勲章 {} は変更できませんでした", award + 1);
            return;
        }
        self.push_progress_undo(step);
        self.dirty = true;
        self.message = format!("{}（まだファイルには書いていません）", done);
    }

    /// Set or clear one request's 受理 flag.
    pub fn set_request_accept(&mut self, record: usize, on: bool) {
        let Some(flag) = self.request_flag(record, false) else {
            self.message = "依頼の表を読めていません（romfs を指定してください）".to_owned();
            return;
        };
        let title = self.request_title(record);
        let (label, done) = if on {
            (
                format!("依頼「{}」を受理", title),
                format!("依頼「{}」を受理にしました", title),
            )
        } else {
            (
                format!("依頼「{}」の受理を外す", title),
                format!("依頼「{}」の受理を外しました", title),
            )
        };
        let Some(step) =
            self.progress_snapshot(label, &[(requests::OFF_FLAGS, requests::FLAG_BYTES)])
        else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        let slot = self.slot;
        let ok = match &mut self.save {
            Some(save) => requests::set(save, slot, flag, on),
            None => false,
        };
        if !ok {
            self.message = "受理フラグを変更できませんでした".to_owned();
            return;
        }
        self.push_progress_undo(step);
        self.dirty = true;
        self.message = format!("{}（まだファイルには書いていません）", done);
    }

    /// Set or clear one request's 完了 flag.
    ///
    /// Completing always accepts first: the game only completes a request it
    /// has accepted, and the pair the other way round is a state it never
    /// writes. Clearing leaves 受理 alone unless the caller says otherwise; the
    /// dialog is the only caller that does, because only the user can tell
    /// "accepted, not done" from "not accepted at all".
    pub fn set_request_complete(&mut self, record: usize, on: bool, also_accept: bool) {
        let (Some(complete), Some(accept)) = (
            self.request_flag(record, true),
            self.request_flag(record, false),
        ) else {
            self.message = "依頼の表を読めていません（romfs を指定してください）".to_owned();
            return;
        };
        let title = self.request_title(record);
        let (label, done) = if on {
            (
                format!("依頼「{}」を完了", title),
                format!("依頼「{}」を完了にしました（受理も立てました）", title),
            )
        } else if also_accept {
            (
                format!("依頼「{}」の完了と受理を外す", title),
                format!("依頼「{}」の完了と受理を外しました", title),
            )
        } else {
            (
                format!("依頼「{}」の完了を外す", title),
                format!("依頼「{}」の完了を外しました（受理はそのままです）", title),
            )
        };
        let Some(step) =
            self.progress_snapshot(label, &[(requests::OFF_FLAGS, requests::FLAG_BYTES)])
        else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        // 完了 implies 受理, so the accept bit is written first when the pair is
        // being set and last when it is being cleared: a half written pair is
        // then always the harmless one.
        let mut ok = true;
        if on {
            ok &= requests::set(save, slot, accept, true);
            ok &= requests::set(save, slot, complete, true);
        } else {
            ok &= requests::set(save, slot, complete, false);
            if also_accept {
                ok &= requests::set(save, slot, accept, false);
            }
        }
        if !ok {
            self.message = "完了フラグを変更できませんでした".to_owned();
            return;
        }
        self.push_progress_undo(step);
        self.dirty = true;
        self.message = format!("{}（まだファイルには書いていません）", done);
    }

    /// Set or clear one quest progress bit.
    ///
    /// questprogress keeps the invariant that a cleared quest is also seen, so
    /// setting クリア writes 既読 too; both maps are in the undo, otherwise one
    /// press would put back half the change. 失敗 is read only and every call
    /// for it is refused, by the core and by the tab, which offers no box.
    pub fn set_quest(&mut self, index: usize, kind: Progress, on: bool) {
        let word = match kind {
            Progress::Cleared => "クリア",
            Progress::Seen => "既読",
            Progress::Failed => "失敗",
        };
        let label = format!(
            "クエスト {} の{}を{}",
            index,
            word,
            if on { "立てる" } else { "外す" }
        );
        let Some(step) = self.progress_snapshot(
            label,
            &[
                (questprogress::OFF_CLEARED, questprogress::MAP_SIZE),
                (questprogress::OFF_SEEN, questprogress::MAP_SIZE),
            ],
        ) else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        let slot = self.slot;
        let ok = match &mut self.save {
            Some(save) => questprogress::set(save, slot, kind, index, on),
            None => false,
        };
        if !ok {
            self.message = match (kind, on) {
                (Progress::Failed, _) => "失敗は読み取り専用です".to_owned(),
                (Progress::Seen, false) => {
                    "クリア済みなので既読は外せません（クリアを先に外してください）".to_owned()
                }
                _ => format!("クエスト {} は変更できませんでした", index),
            };
            return;
        }
        self.push_progress_undo(step);
        self.dirty = true;
        self.message = match (kind, on) {
            (Progress::Cleared, true) => {
                format!("クエスト {} をクリアにしました（既読も立てました）", index)
            }
            (_, true) => format!("クエスト {} を{}にしました", index, word),
            _ => format!("クエスト {} の{}を外しました", index, word),
        };
    }


    // -------------------------------------------------------------- drawing

    /// The 進行状況 tab: romfs on top, then one of the three sections.
    fn tab_progress(&mut self, ui: &mut egui::Ui) {
        ui.heading("進行状況");
        let Some(save) = &self.save else {
            ui.label("セーブを開くと、勲章・依頼・クエストの進行状況を一覧・編集できます");
            return;
        };
        // An MHGU slot is 0x1104 bytes longer and every offset here was read
        // off MHXX, so nothing in this tab describes an MHGU save. Refusing is
        // the only honest answer; the core answers None / false for the same
        // reason.
        if save.kind() == SaveKind::Mhgu {
            ui.label(
                egui::RichText::new(
                    "このセーブは MHGU です。勲章・依頼・クエストの位置は MHXX のセーブで確認したものなので、このタブでは表示も編集もしません。",
                )
                .color(warning_color()),
            );
            return;
        }
        self.progress_romfs_bar(ui);
        ui.separator();
        self.progress_section_bar(ui);
        ui.separator();
        match self.progress_section {
            ProgressSection::Awards => self.progress_awards(ui),
            ProgressSection::Requests => self.progress_requests(ui),
            ProgressSection::Quests => self.progress_quests(ui),
        }
    }

    /// The romfs row: the path, a folder picker, and 読み込む.
    fn progress_romfs_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("romfs");
            ui.add(
                egui::TextEdit::singleline(&mut self.romfs_path)
                    .hint_text(r"C:\...\romfs（nativeNX/table があるフォルダ）")
                    .desired_width(380.0),
            );
            if ui
                .button("フォルダを選ぶ")
                .on_hover_text("romfs のフォルダを選ぶと、そのまま読み込みます")
                .clicked()
            {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("romfs のフォルダを選択")
                    .pick_folder()
                {
                    self.romfs_path = dir.display().to_string();
                    self.load_romfs();
                }
            }
            if ui.button("読み込む").clicked() {
                self.load_romfs();
            }
            let loaded = !self.award_names.is_empty() || !self.request_records.is_empty();
            if loaded
                && ui
                    .small_button("解除")
                    .on_hover_text("読み込んだ名前を捨てます（セーブは変わりません）")
                    .clicked()
            {
                self.clear_romfs();
            }
        });
        let status = if self.romfs_status.is_empty() {
            String::from(
                "romfs にはゲームのテキストしかありません。名前はメモリ上だけで使い、セーブにもファイルにも書きません。",
            )
        } else {
            // The report already names the folder it came from, so the line is
            // not repeated here.
            self.romfs_status.clone()
        };
        ui.label(egui::RichText::new(status).weak().small());
    }

    /// The three-way switcher and the undo button.
    fn progress_section_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for section in [
                ProgressSection::Awards,
                ProgressSection::Requests,
                ProgressSection::Quests,
            ] {
                ui.selectable_value(&mut self.progress_section, section, section.label());
            }
            ui.separator();
            let last = self.progress_undo.last().map(|u| u.label.clone());
            let button = ui.add_enabled(last.is_some(), egui::Button::new("元に戻す"));
            let button = match &last {
                Some(label) => button.on_hover_text(format!("{} を取り消します", label)),
                None => button.on_hover_text("このタブで変更すると、ここから取り消せます"),
            };
            if button.clicked() {
                self.undo_progress();
            }
            if let Some(label) = last {
                ui.label(
                    egui::RichText::new(format!("最後の変更: {}", label))
                        .weak()
                        .small(),
                );
            }
        });
    }

    /// The 132 award cells.
    fn progress_awards(&mut self, ui: &mut egui::Ui) {
        let slot = self.slot;
        let (total, used, got, agree) = {
            let Some(save) = &self.save else {
                return;
            };
            (
                awards::AWARDS,
                save.slot_used(slot),
                awards::count(save, slot),
                awards::copies_agree(save, slot),
            )
        };
        if !used {
            ui.label("このスロットは未使用です。キャラクターが居ないスロットには勲章もありません。");
            return;
        }
        let states: Vec<bool> = (0..total).map(|a| self.award_on(a)).collect();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("取得 {} / {}", got, total)).strong());
            ui.label(egui::RichText::new(format!("未取得 {}", total - got)).weak());
        });
        if !agree {
            // The game merges the three copies with an OR, so a copy that
            // disagrees means the card - the one has() reads - can lag behind
            // the map screen. Saying so is the point.
            ui.label(
                egui::RichText::new(
                    "注意: 3 つの写し（カード / マップ / 通知）が一致しません。取得の表示はカード側の写しで、ゲームは 3 つを OR で統合するため、実機と食い違うことがあります。",
                )
                .color(warning_color()),
            );
        }
        ui.label(
            egui::RichText::new("押すと取得 / 未取得が入れ替わります（3 つの写しすべてに書きます）")
                .weak()
                .small(),
        );
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("awardgrid")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for a in 0..total {
                        let on = states[a];
                        let label = self.award_label(a);
                        let full = self
                            .award_names
                            .get(a)
                            .map(|n| n.trim().to_owned())
                            .unwrap_or_default();
                        let text = egui::RichText::new(label).color(award_text(on));
                        let response = ui.add_sized(
                            [170.0, 26.0],
                            egui::Button::new(text)
                                .fill(award_fill(on))
                                .stroke(egui::Stroke::new(1.0, award_stroke(on))),
                        );
                        let tip = if full.is_empty() {
                            format!("勲章 {}（{}）", a + 1, if on { "取得済み" } else { "未取得" })
                        } else {
                            format!(
                                "{}\n{}",
                                full,
                                if on {
                                    "取得済み - 押すと未取得に戻します"
                                } else {
                                    "未取得 - 押すと取得にします"
                                }
                            )
                        };
                        if response.on_hover_text(tip).clicked() {
                            self.set_award(a, !on);
                        }
                    }
                });
            });
    }


    /// The request list, or the raw flags when there is no activity table.
    fn progress_requests(&mut self, ui: &mut egui::Ui) {
        if self.request_records.is_empty() {
            self.progress_raw_flags(ui);
            return;
        }
        let accepted = (0..self.request_records.len())
            .filter(|r| self.request_on(*r).0)
            .count();
        let completed = (0..self.request_records.len())
            .filter(|r| self.request_on(*r).1)
            .count();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "受理 {} / 完了 {}（{} 件）",
                    accepted,
                    completed,
                    self.request_records.len()
                ))
                .strong(),
            );
            ui.separator();
            ui.label("絞り込み");
            ui.add(
                egui::TextEdit::singleline(&mut self.request_filter)
                    .hint_text("依頼名")
                    .desired_width(160.0),
            );
            if ui.small_button("クリア").clicked() {
                self.request_filter.clear();
            }
        });
        ui.label(
            egui::RichText::new(
                "受理と完了は activityData.atd の +0x33 / +0x35 のフラグです。完了を立てると受理も立ちます。完了を外すときは、受理も外すかを聞きます。",
            )
            .weak()
            .small(),
        );
        let needle = self.request_filter.trim().to_lowercase();
        let rows: Vec<usize> = (0..self.request_records.len())
            .filter(|i| {
                needle.is_empty()
                    || self.request_title(*i).to_lowercase().contains(&needle)
                    || format!("{}", i + 1).contains(&needle)
            })
            .collect();
        let mut accept_clicked: Option<(usize, bool)> = None;
        let mut complete_clicked: Option<(usize, bool)> = None;
        TableBuilder::new(ui)
            .striped(true)
            .id_salt("requestlist")
            .column(Column::exact(44.0))
            .column(Column::remainder())
            .column(Column::exact(74.0))
            .column(Column::exact(52.0))
            .column(Column::exact(52.0))
            .header(22.0, |mut h| {
                h.col(|ui| {
                    ui.strong("#");
                });
                h.col(|ui| {
                    ui.strong("依頼");
                });
                h.col(|ui| {
                    ui.strong("クエストID");
                });
                h.col(|ui| {
                    ui.strong("受理");
                });
                h.col(|ui| {
                    ui.strong("完了");
                });
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let record = rows[row.index()];
                    let Some(r) = self.request_records.get(record).copied() else {
                        return;
                    };
                    let (accept_on, complete_on) = self.request_on(record);
                    row.col(|ui| {
                        ui.label(format!("{}", record + 1));
                    });
                    row.col(|ui| {
                        ui.label(self.request_title(record)).on_hover_text(format!(
                            "record {} / kind {} / 受理フラグ {} / 完了フラグ {}",
                            record, r.kind, r.accept_flag, r.complete_flag
                        ));
                    });
                    row.col(|ui| {
                        if r.kind == 0 {
                            ui.label(format!("{}", r.quest_id));
                        } else {
                            ui.label("—").on_hover_text("クエストを持たない行です");
                        }
                    });
                    row.col(|ui| {
                        let mut on = accept_on;
                        if ui.checkbox(&mut on, "").changed() {
                            accept_clicked = Some((record, on));
                        }
                    });
                    row.col(|ui| {
                        let mut on = complete_on;
                        if ui.checkbox(&mut on, "").changed() {
                            complete_clicked = Some((record, on));
                        }
                    });
                });
            });
        if let Some((record, on)) = accept_clicked {
            self.set_request_accept(record, on);
        }
        if let Some((record, on)) = complete_clicked {
            if on {
                // 完了 cannot stand without 受理; the pair is written together.
                self.set_request_complete(record, true, false);
            } else {
                // Clearing is the one that has to ask: the accept bit may or
                // may not be meant to go with it.
                self.confirm = Confirm::RequestIncomplete { record };
            }
        }
    }

    /// What the request section shows without an activity table: flag numbers.
    ///
    /// The flags are read only here. 444 of the 742 bits set in the verified
    /// save belong to events that are not requests at all, and without the
    /// table there is no way to tell which of them this tool may write.
    fn progress_raw_flags(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("romfs を指定してください").strong());
        ui.label(
            "上の欄に romfs のフォルダ（nativeNX/table）を入れて「読み込む」を押すと、依頼名と、受理・完了のフラグ番号が読めます。",
        );
        ui.label(
            egui::RichText::new(
                "いまは生のフラグ番号だけを表示しています。どの番号が受理でどれが完了かは、ゲームのテーブルにしか書かれていません。",
            )
            .weak()
            .small(),
        );
        ui.add_space(4.0);
        let slot = self.slot;
        let set = self
            .save
            .as_ref()
            .map(|s| requests::count(s, slot))
            .unwrap_or(0);
        ui.label(format!(
            "セット済み {} / {} ビット（依頼以外のイベントも同じ領域にあります）",
            set,
            requests::FLAGS
        ));
        let total = requests::FLAGS;
        egui::ScrollArea::vertical()
            .id_salt("rawflags")
            .show_rows(ui, 18.0, total.div_ceil(8), |ui, range| {
                for line in range {
                    ui.horizontal(|ui| {
                        for column in 0..8 {
                            let flag = line * 8 + column;
                            if flag >= total {
                                break;
                            }
                            let on = self
                                .save
                                .as_ref()
                                .and_then(|s| requests::get(s, slot, flag))
                                .unwrap_or(false);
                            let text = if on {
                                egui::RichText::new(format!("{:04} ●", flag))
                                    .monospace()
                                    .color(egui::Color32::from_rgb(226, 240, 228))
                            } else {
                                egui::RichText::new(format!("{:04} ·", flag))
                                    .monospace()
                                    .weak()
                            };
                            ui.add_sized([82.0, 16.0], egui::Label::new(text));
                        }
                    });
                }
            });
    }

    /// Quest progress, indices 1..=1508.
    fn progress_quests(&mut self, ui: &mut egui::Ui) {
        let slot = self.slot;
        let (cleared, seen, failed, broken) = {
            let Some(save) = &self.save else {
                return;
            };
            (
                questprogress::count(save, slot, Progress::Cleared),
                questprogress::count(save, slot, Progress::Seen),
                questprogress::count(save, slot, Progress::Failed),
                questprogress::clears_without_seen(save, slot),
            )
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "クリア {} / 既読 {} / 失敗 {}",
                    cleared, seen, failed
                ))
                .strong(),
            );
            ui.label(
                egui::RichText::new(format!("索引 1..{}", questprogress::BITS - 1))
                    .weak()
                    .small(),
            );
        });
        ui.label(
            egui::RichText::new(
                "クリアを立てると既読も立ちます。既読を外せるのは、クリアしていないクエストだけです。失敗は読み取り専用です。",
            )
            .weak()
            .small(),
        );
        if !broken.is_empty() {
            // The save already violates the invariant in three places. The tool
            // does not guess which side is wrong; it names the rows instead.
            let list = broken
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            ui.label(
                egui::RichText::new(format!(
                    "注意: 既読なしでクリアになっている索引が {} 件あります（{}）。データの状態をそのまま表示しています。",
                    broken.len(),
                    list
                ))
                .color(warning_color()),
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("絞り込み");
            ui.add(
                egui::TextEdit::singleline(&mut self.quest_filter)
                    .hint_text("索引")
                    .desired_width(80.0),
            );
            ui.checkbox(&mut self.quest_missing_only, "未クリアのみ");
            if ui.small_button("クリア").clicked() {
                self.quest_filter.clear();
            }
        });
        let needle = self.quest_filter.trim().to_lowercase();
        let rows: Vec<usize> = (1..questprogress::BITS)
            .filter(|i| {
                (needle.is_empty() || i.to_string().contains(&needle))
                    && (!self.quest_missing_only || !self.quest_on(*i, Progress::Cleared))
            })
            .collect();
        let mut quest_clicked: Option<(usize, Progress, bool)> = None;
        TableBuilder::new(ui)
            .striped(true)
            .id_salt("questprogress")
            .column(Column::exact(64.0))
            .column(Column::exact(64.0))
            .column(Column::exact(64.0))
            .column(Column::exact(64.0))
            .column(Column::remainder())
            .header(22.0, |mut h| {
                h.col(|ui| {
                    ui.strong("索引");
                });
                h.col(|ui| {
                    ui.strong("クリア");
                });
                h.col(|ui| {
                    ui.strong("既読");
                });
                h.col(|ui| {
                    ui.strong("失敗");
                });
                h.col(|ui| {
                    ui.strong("状態");
                });
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let index = rows[row.index()];
                    let is_cleared = self.quest_on(index, Progress::Cleared);
                    let is_seen = self.quest_on(index, Progress::Seen);
                    let is_failed = self.quest_on(index, Progress::Failed);
                    row.col(|ui| {
                        ui.label(format!("{}", index));
                    });
                    row.col(|ui| {
                        let mut on = is_cleared;
                        if ui.checkbox(&mut on, "").changed() {
                            quest_clicked = Some((index, Progress::Cleared, on));
                        }
                    });
                    row.col(|ui| {
                        let mut on = is_seen;
                        // A cleared quest must stay seen; the core refuses the
                        // clear, so the box says why instead of doing nothing.
                        let locked = is_cleared && is_seen;
                        let response = ui.add_enabled(!locked, egui::Checkbox::new(&mut on, ""));
                        let response = if locked {
                            response.on_hover_text(
                                "クリア済みなので既読は外せません（先にクリアを外してください）",
                            )
                        } else {
                            response
                        };
                        if response.changed() {
                            quest_clicked = Some((index, Progress::Seen, on));
                        }
                    });
                    row.col(|ui| {
                        // 失敗 has no established meaning (58 of its 72 bits are
                        // also cleared), so it is read only: a disabled box
                        // shows the bit without offering to change it.
                        let mut on = is_failed;
                        ui.add_enabled(false, egui::Checkbox::new(&mut on, ""));
                    });
                    row.col(|ui| {
                        if is_cleared && !is_seen {
                            ui.label(
                                egui::RichText::new("既読なしでクリア").color(warning_color()),
                            )
                            .on_hover_text(
                                "データの状態をそのまま表示しています（自動では直しません）",
                            );
                        }
                    });
                });
            });
        if let Some((index, kind, on)) = quest_clicked {
            self.set_quest(index, kind, on);
        }
    }
}

#[cfg(test)]
mod tests {
    //! The write paths are the part of this file that can destroy a save, so
    //! they are checked without a window: everything here goes straight at the
    //! filesystem helpers and the editor state, never at egui.

    use super::*;

    /// A scratch folder that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let mut p = std::env::temp_dir();
            p.push(format!("mhnx-gui-{}-{}", tag, std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("scratch folder");
            Scratch(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A Switch save with the body marker the parser checks, written out.
    fn write_a_save(dir: &std::path::Path, name: &str) -> PathBuf {
        let mut body = vec![0u8; BODY_SIZE];
        body[0] = 0xC6;
        let save = Save::from_3ds_body(&body, name).expect("a save");
        let path = dir.join(name);
        std::fs::write(&path, save.to_bytes()).expect("write the fixture");
        path
    }

    /// The same, with the three slot bases filled in: without them every
    /// character-slot accessor returns None and the equipment box is empty.
    fn write_a_save_with_slots(dir: &std::path::Path, name: &str) -> PathBuf {
        let save = mhnx_core::save::empty_switch_save();
        let path = dir.join(name);
        std::fs::write(&path, save.to_bytes()).expect("write the fixture");
        path
    }

    #[test]
    fn stamp_and_size_are_read_out_of_the_name() {
        assert_eq!(
            MhnxEditor::snapshot_stamp("system-20261104-012812.sav"),
            "20261104-012812"
        );
        assert_eq!(
            MhnxEditor::snapshot_stamp("system_backup-20261104-012812.sav"),
            "20261104-012812"
        );
        assert_eq!(MhnxEditor::human_size(4_726_188), "4.51 MB");
        assert_eq!(MhnxEditor::human_size(2048), "2 KB");
    }

    #[test]
    fn only_the_newest_ten_snapshots_survive() {
        let dir = Scratch::new("rotate");
        let save = dir.path().join("system");
        std::fs::write(&save, b"x").expect("save");
        let snaps = MhnxEditor::snapshot_dir(&save);
        std::fs::create_dir_all(&snaps).expect("folder");
        // A different stem in the same folder must not be rotated away.
        std::fs::write(snaps.join("system_backup-20260101-000000.sav"), b"keep").expect("other");
        for _ in 0..13 {
            MhnxEditor::push_snapshot(&save, "system", b"body").expect("snapshot");
        }
        let names: Vec<String> = std::fs::read_dir(&snaps)
            .expect("folder")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names.iter().filter(|n| n.starts_with("system-")).count(),
            10
        );
        assert!(names.iter().any(|n| n == "system_backup-20260101-000000.sav"));
    }

    #[test]
    fn a_write_is_compared_with_what_came_back() {
        let dir = Scratch::new("verify");
        let path = dir.path().join("system");
        std::fs::write(&path, b"old").expect("old");
        let (backup, line) = MhnxEditor::write_and_verify(&path, b"new").expect("a match");
        assert!(line.contains("照合OK"), "{}", line);
        assert_eq!(std::fs::read(&path).expect("read"), b"new");
        let backup = backup.expect("the replaced file is kept");
        assert_eq!(std::fs::read(backup).expect("bak"), b"old");
    }

    #[test]
    fn a_failed_comparison_puts_the_backup_back() {
        let dir = Scratch::new("undo");
        let path = dir.path().join("system");
        let bak = dir.path().join("system.bak");
        std::fs::write(&path, b"bad").expect("bad");
        std::fs::write(&bak, b"old").expect("bak");
        let trip = verify::compare(b"want", b"bad!");
        assert!(!trip.is_identical());
        let msg = MhnxEditor::undo_bad_write(&path, &Some(bak), Some(&trip), "");
        assert!(msg.starts_with("照合に失敗したので戻しました"), "{}", msg);
        assert!(msg.contains("差分 3 件"), "{}", msg);
        assert!(MhnxEditor::message_is_failure(&msg));
        assert_eq!(std::fs::read(&path).expect("read"), b"old");
    }

    #[test]
    fn a_snapshot_can_be_put_back_and_what_it_replaced_is_kept() {
        let dir = Scratch::new("restore");
        let path = write_a_save(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path.clone());
        assert!(app.save.is_some(), "{}", app.message);
        let original = std::fs::read(&path).expect("read");
        let mut older = original.clone();
        older[SWITCH_SIZE - 1] ^= 0xFF;
        let snaps = MhnxEditor::snapshot_dir(&path);
        std::fs::create_dir_all(&snaps).expect("folder");
        let name = snapshot::name("system", 1_760_000_000);
        std::fs::write(snaps.join(&name), &older).expect("snapshot");
        app.restore_snapshot(name.clone());
        assert_eq!(std::fs::read(&path).expect("read"), older);
        assert!(app.message.contains("の時点に戻しました"), "{}", app.message);
        app.refresh_snapshots();
        let kept = app
            .snapshots
            .iter()
            .find(|s| s.name != name)
            .expect("the replaced contents are kept");
        assert_eq!(
            std::fs::read(snaps.join(&kept.name)).expect("read"),
            original
        );
        assert!(!app.dirty);
    }

    #[test]
    fn a_snapshot_that_is_not_a_save_is_refused() {
        let dir = Scratch::new("refuse");
        let path = write_a_save(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path.clone());
        let before = std::fs::read(&path).expect("read");
        let snaps = MhnxEditor::snapshot_dir(&path);
        std::fs::create_dir_all(&snaps).expect("folder");
        let name = snapshot::name("system", 1_760_000_001);
        std::fs::write(snaps.join(&name), b"not a save").expect("snapshot");
        app.restore_snapshot(name);
        assert!(app.message.contains("読めないので戻しません"), "{}", app.message);
        assert_eq!(std::fs::read(&path).expect("read"), before);
    }

    #[test]
    fn the_switch_pair_is_written_and_verified() {
        let dir = Scratch::new("pair");
        let path = write_a_save(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).expect("out");
        app.write_switch_pair(out.clone());
        let a = std::fs::read(out.join("system")).expect("system");
        let b = std::fs::read(out.join("system_backup")).expect("system_backup");
        assert_eq!(a.len(), SWITCH_SIZE);
        assert_eq!(b.len(), SWITCH_SIZE);
        assert!(app.message.contains("照合OK"), "{}", app.message);
        assert!(!app.dirty);
        let snaps: Vec<_> = std::fs::read_dir(MhnxEditor::snapshot_dir(&out.join("system")))
            .expect("snapshot folder")
            .flatten()
            .collect();
        assert_eq!(snaps.len(), 1);
    }

    #[test]
    fn the_3ds_export_is_written_and_verified() {
        let dir = Scratch::new("export");
        let path = write_a_save(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        let out = dir.path().join("system.3ds");
        app.export_3ds(out.clone());
        // The export applies the working copies first, so the file has to be
        // compared against what the save holds after that, not before.
        let want = app.save.as_ref().expect("save").to_3ds_bytes();
        let got = std::fs::read(&out).expect("read");
        assert_eq!(got.len(), BODY_SIZE);
        assert!(got == want, "the exported body is not the save's body");
        assert!(
            app.message.starts_with("3DS 形式で書き出しました"),
            "{}",
            app.message
        );
        assert!(app.message.contains("照合OK"), "{}", app.message);
    }

    #[test]
    fn the_equipment_editor_writes_only_the_bytes_it_owns() {
        let dir = Scratch::new("equip");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);

        // A weapon carrying a talisman-shaped tail. The sample save holds ten
        // of these and they are worn, so the editor must leave +0x0C.. alone.
        let mut rec = [0u8; mhnx_core::equip::EQUIP_RECORD];
        rec[0] = 7; // 大剣
        rec[2] = 40; // 装備ID
        for (i, b) in rec[0x0C..].iter_mut().enumerate() {
            *b = (i as u8) + 1;
        }
        let tail: Vec<u8> = rec[0x0C..].to_vec();
        {
            let dst = app
                .save
                .as_mut()
                .and_then(|s| s.equipment_record_mut(0, 5))
                .expect("record 5");
            dst.copy_from_slice(&rec);
        }

        // What the panel hands back after a level, a transmog and one socket.
        let mut edited = rec;
        assert!(mhnx_core::equip::set_level(&mut edited, 12));
        assert!(mhnx_core::equip::set_transmog(&mut edited, 300, 4));
        assert!(mhnx_core::equip::set_decoration(&mut edited, 1, 2640));
        app.equip_selected = Some(5);
        assert!(app.store_equipment_record(5, &edited));
        assert!(app.dirty, "an edit has to mark the save dirty");

        let save = app.save.as_ref().expect("save");
        let eq = save.equipment_at(0, 5).expect("record");
        assert_eq!(eq.kind, 7);
        assert_eq!(eq.id, 40);
        assert_eq!(eq.level, 12);
        assert_eq!(eq.transmog, 300);
        assert_eq!(eq.transmog_level, 4);
        let box_bytes = save.equipment_box(0).expect("box");
        let at = 5 * mhnx_core::equip::EQUIP_RECORD;
        let got = &box_bytes[at..at + mhnx_core::equip::EQUIP_RECORD];
        assert_eq!(mhnx_core::equip::Equipment::decorations(5, got), [0, 2640, 0]);
        assert_eq!(&got[0x0C..], &tail[..], "the tail is not the editor's to touch");
    }

    #[test]
    fn the_equipment_editor_refuses_what_it_cannot_write() {
        let mut app = MhnxEditor::default();
        // Nothing is open yet.
        assert!(!app.store_equipment_record(0, &[0u8; mhnx_core::equip::EQUIP_RECORD]));
        assert!(!app.dirty);

        let dir = Scratch::new("equiprefuse");
        let path = write_a_save_with_slots(dir.path(), "system");
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        // Past the end of the box, and a slice that is not a record.
        assert!(!app.store_equipment_record(2000, &[0u8; mhnx_core::equip::EQUIP_RECORD]));
        assert!(!app.store_equipment_record(0, &[0u8; 3]));
        assert!(!app.dirty, "a refused write must not make the save look edited");
    }

    #[test]
    fn a_new_record_lands_in_a_blank_slot_and_is_selected() {
        let dir = Scratch::new("equipnew");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);

        assert!(app.equip_create(9, 123, 7));
        assert!(app.dirty, "creating a record has to mark the save dirty");
        let index = app.equip_selected.expect("the created row is selected");
        let eq = app
            .save
            .as_ref()
            .expect("save")
            .equipment_at(0, index)
            .expect("record");
        assert_eq!((eq.kind, eq.id, eq.level), (9, 123, 7));
        assert!(app.message.contains("作成しました"), "{}", app.message);

        // The next one takes the next blank slot, not the same one again.
        assert!(app.equip_create(1, 5, 1));
        let second = app.equip_selected.expect("selected");
        assert!(second > index, "{} then {}", index, second);
        let used = app
            .save
            .as_ref()
            .and_then(|s| s.equipment_box(0))
            .map(|b| mhnx_core::equip::decode_box(b))
            .map(|all| all.iter().filter(|e| !e.is_empty()).count())
            .unwrap_or(0);
        assert_eq!(used, 2);
    }

    #[test]
    fn deleting_a_worn_record_is_refused_until_it_is_forced() {
        let dir = Scratch::new("equipdel");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        // Written straight in: a created record lands in slot 0, and this
        // fixture leaves every My Set pointing at index 0, so the reference
        // list would be every set rather than the one worn item.
        let index = 9;
        let mut rec = [0u8; mhnx_core::equip::EQUIP_RECORD];
        rec[0] = 7; // 大剣
        rec[2] = 40;
        app.save
            .as_mut()
            .and_then(|s| s.equipment_record_mut(0, index))
            .expect("record 9")
            .copy_from_slice(&rec);
        app.equip_selected = Some(index);
        app.dirty = false;

        // Wear it: part 0 of the seven worn indices is the weapon.
        let base = app
            .save
            .as_ref()
            .and_then(|s| s.slot_base(0))
            .expect("slot base") as usize;
        let at = base + mhnx_core::equiprefs::WORN_OFFSET;
        app.save.as_mut().expect("save").body_mut()[at..at + 2]
            .copy_from_slice(&(index as u16).to_le_bytes());

        match app.equip_delete(index, false) {
            Err(EquipError::Referenced(refs)) => {
                assert_eq!(refs, vec![EquipRef::Worn { part: 0 }]);
            }
            other => panic!("a worn record has to be refused, got {:?}", other),
        }
        assert!(!app.dirty, "a refused delete must not dirty the save");
        let still_there = app
            .save
            .as_ref()
            .and_then(|s| s.equipment_at(0, index))
            .expect("record");
        assert_eq!(still_there.kind, 7);

        // The dialog turns the refusal into the second ask, with the list.
        app.equip_delete_checked(index);
        assert!(
            matches!(app.confirm, Confirm::EquipClearReferenced { index: i, .. } if i == index),
            "the second dialog has to list the references"
        );
        app.confirm = Confirm::None;

        assert!(app.equip_delete(index, true).is_ok());
        assert!(app.dirty);
        let gone = app
            .save
            .as_ref()
            .and_then(|s| s.equipment_at(0, index))
            .expect("record");
        assert!(gone.is_empty(), "the record is zeroed");
        assert_eq!(app.equip_selected, None, "the deleted row is no longer shown");
    }

    #[test]
    fn replacing_an_item_empties_its_decorations_and_keeps_the_tail() {
        let dir = Scratch::new("equipreplace");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);

        let mut rec = [0u8; mhnx_core::equip::EQUIP_RECORD];
        rec[0] = 7; // 大剣
        rec[2] = 40;
        let tail: Vec<u8> = (0..mhnx_core::equip::EQUIP_RECORD - 0x0C)
            .map(|i| (i as u8) + 1)
            .collect();
        rec[0x0C..].copy_from_slice(&tail);
        assert!(mhnx_core::equip::set_decoration(&mut rec, 0, 2638));
        app.save
            .as_mut()
            .and_then(|s| s.equipment_record_mut(0, 9))
            .expect("record 9")
            .copy_from_slice(&rec);
        app.dirty = false;

        assert!(app.equip_replace(9, 13, 77, 3));
        assert!(app.dirty);
        let eq = app
            .save
            .as_ref()
            .and_then(|s| s.equipment_at(0, 9))
            .expect("record");
        assert_eq!((eq.kind, eq.id, eq.level), (13, 77, 3));
        let box_bytes = app.save.as_ref().and_then(|s| s.equipment_box(0)).expect("box");
        let at = 9 * mhnx_core::equip::EQUIP_RECORD;
        let got = &box_bytes[at..at + mhnx_core::equip::EQUIP_RECORD];
        assert_eq!(
            mhnx_core::equip::Equipment::decorations(9, got),
            [0, 0, 0],
            "the old decorations belonged to the item that was replaced"
        );
        assert_eq!(&got[0x0C..], &tail[..], "the tail is not the replace's to touch");

        // A blank record is not something replace may fill: create does that.
        assert!(!app.equip_replace(10, 7, 1, 1));
        assert!(app.message.contains("すでに空き枠"), "{}", app.message);
    }

    #[test]
    fn a_decoration_is_named_by_its_item_id() {
        assert_eq!(deco_label(0), "—");
        #[cfg(feature = "gamedata")]
        {
            assert_eq!(deco_name(2638), "Antidote Jwl 1");
            assert_eq!(deco_name(2640), "Paralysis Jwl 1");
            // An id the table does not know still reads back as something.
            assert_eq!(deco_name(65535), "#65535");
        }
    }

    /// Three stacks of one item: the merge frees two records and drops
    /// everything over the 99 cap, which the dialog has to say out loud.
    #[test]
    fn the_dedupe_dialog_says_what_the_merge_would_cost() {
        let dir = Scratch::new("dedupeask");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        let mut items = vec![ItemSlot::default(); 8];
        items[0] = ItemSlot { id: 5, count: 99 };
        items[1] = ItemSlot { id: 5, count: 99 };
        items[2] = ItemSlot { id: 5, count: 52 };
        app.items = items;

        // The button only arms the dialog; the box is untouched until it is
        // answered.
        app.ask_dedupe();
        assert!(
            matches!(app.confirm, Confirm::ItemDedupe { freed: 2, lost: 151 }),
            "the preview has to count both the freed records and the lost items"
        );
        assert!(!app.dirty, "asking must not write anything");

        let (line, warn) = MhnxEditor::dedupe_confirm_text(DedupePreview {
            freed: 2,
            lost: 151,
        });
        assert_eq!(line, "2 枠を解放、151 個を失います");
        assert!(
            warn,
            "a merge that loses items is drawn in the warning colour"
        );
        let (line, warn) = MhnxEditor::dedupe_confirm_text(DedupePreview { freed: 3, lost: 0 });
        assert_eq!(line, "3 枠を解放、0 個を失います");
        assert!(!warn, "a merge that loses nothing needs no warning colour");

        // Answering yes merges the three stacks into one capped one.
        assert_eq!(
            app.item_dedupe(),
            DedupePreview {
                freed: 2,
                lost: 151
            }
        );
        assert_eq!(app.items[0], ItemSlot { id: 5, count: 99 });
        assert!(app.items[1].is_blank());
        assert!(app.dirty);
        assert!(app.message.contains("151 個を失いました"), "{}", app.message);
        let stored = app
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(0))
            .expect("box");
        assert_eq!(itembox::count_of(&stored, 5), 99);
        // One undo puts all three stacks back.
        app.undo_item();
        assert_eq!(itembox::count_of(&app.items, 5), 250);
    }

    /// A stack that is nearly full takes only the room it has left, and a box
    /// with no blank record has to report the number it could not store.
    #[test]
    fn adding_more_than_fits_reports_the_leftover() {
        let dir = Scratch::new("bulkadd");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);

        // No blank record: only the room in the existing stack is usable.
        app.items = vec![
            ItemSlot { id: 1, count: 99 },
            ItemSlot { id: 2, count: 50 },
        ];
        app.dirty = false;
        assert_eq!(app.add_items_bulk(2, 100), 49);
        assert_eq!(app.items[1], ItemSlot { id: 2, count: 99 });
        assert!(app.message.contains("49 個追加"), "{}", app.message);
        assert!(
            app.message.contains("51 個は入りきりませんでした"),
            "{}",
            app.message
        );
        assert!(app.dirty, "a real add has to mark the save dirty");
        // The save holds what apply_to_save wrote, not just the working copy.
        let stored = app
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(0))
            .expect("box");
        assert_eq!(itembox::count_of(&stored, 2), 99);

        // Nothing fits: the box, the undo list and the dirty flag stay put,
        // and the number that did not fit is still said.
        let before = app.items.clone();
        let undo_before = app.item_undo.len();
        app.dirty = false;
        assert_eq!(app.add_items_bulk(9, 5), 0);
        assert!(
            app.message.contains("5 個は入りきりませんでした"),
            "{}",
            app.message
        );
        assert_eq!(app.items, before);
        assert_eq!(
            app.item_undo.len(),
            undo_before,
            "an add that fits nothing needs no undo step"
        );
        assert!(
            !app.dirty,
            "an add that fits nothing must not dirty the save"
        );
    }

    /// Every bulk button only arms the dialog: the box changes when the dialog
    /// is answered, and then the change reaches the save.
    #[test]
    fn the_bulk_buttons_ask_before_they_touch_the_box() {
        let dir = Scratch::new("bulkask");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        let mut items = vec![ItemSlot::default(); 6];
        items[0] = ItemSlot { id: 1, count: 5 };
        items[2] = ItemSlot { id: 2, count: 7 };
        items[4] = ItemSlot { id: 1, count: 3 };
        app.items = items.clone();
        app.dirty = false;

        // 全部99に asks first.
        app.ask_fill_all();
        assert!(matches!(app.confirm, Confirm::ItemFillAll { changed: 3 }));
        assert_eq!(app.items, items, "asking must not change the box");
        assert!(!app.dirty, "asking must not write anything");
        app.confirm = Confirm::None;
        assert_eq!(app.item_fill_all(), 3);
        assert!(app.dirty);
        assert!(app.message.contains("3 枠"), "{}", app.message);

        // 空きを詰める asks first, then moves the used rows to the front.
        app.items = items.clone();
        app.dirty = false;
        app.ask_compact();
        assert!(matches!(app.confirm, Confirm::ItemCompact { moved: 2 }));
        assert_eq!(app.items, items);
        app.confirm = Confirm::None;
        assert_eq!(app.item_compact(), 2);
        let stored = app
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(0))
            .expect("box");
        assert_eq!(stored[0], ItemSlot { id: 1, count: 5 });
        assert_eq!(stored[1], ItemSlot { id: 2, count: 7 });
        assert_eq!(stored[2], ItemSlot { id: 1, count: 3 });
        assert!(stored[3].is_blank());

        // 上限超えを丸める asks first too.
        app.items = items.clone();
        app.items[1] = ItemSlot { id: 3, count: 127 };
        app.dirty = false;
        app.ask_clamp();
        assert!(matches!(app.confirm, Confirm::ItemClamp { changed: 1 }));
        app.confirm = Confirm::None;
        assert_eq!(app.item_clamp(), 1);
        assert_eq!(app.items[1], ItemSlot { id: 3, count: 99 });
        assert!(app.dirty);
    }

    /// Put one equipment record in a box, the way the equipment tab writes one.
    fn put_record(app: &mut MhnxEditor, index: usize, kind: u8, id: u16, deco0: u16) {
        let mut rec = [0u8; mhnx_core::equip::EQUIP_RECORD];
        rec[0] = kind;
        rec[2..4].copy_from_slice(&id.to_le_bytes());
        rec[0x06..0x08].copy_from_slice(&deco0.to_le_bytes());
        let dst = app
            .save
            .as_mut()
            .and_then(|s| s.equipment_record_mut(0, index))
            .expect("equipment record");
        dst.copy_from_slice(&rec);
    }

    /// One saved equipment set, read back out of the open save.
    fn set_of(app: &MhnxEditor, index: usize) -> mhnx_core::myset::EquipSet {
        app.save
            .as_ref()
            .and_then(|s| {
                mhnx_core::myset::equip_sets(s, 0)
                    .into_iter()
                    .find(|set| set.index == index)
            })
            .expect("the set")
    }

    /// A part goes in through set_part, which moves the index at +0x2A and the
    /// decoration copy at +0x38 together: writing only the index is what leaves
    /// a set describing decorations its equipment does not have.
    #[test]
    fn a_set_part_is_written_with_its_decoration_copy() {
        let dir = Scratch::new("setpart");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        put_record(&mut app, 5, 1, 10, 2640);

        // Start from a record in the shape the game leaves an unused one in.
        // The empty fixture writes zeros here, and a zero part names box slot 0
        // rather than NONE, which would be a warning before the first write.
        let set = 2usize;
        assert!(app.set_clear(set), "{}", app.message);
        // A record still called '－－－' with a part filled is its own warning,
        // so the test names the set the way a user would.
        assert!(app.set_name_write(set, "検証セット"), "{}", app.message);
        app.dirty = false;

        assert!(app.set_part_choose(set, 1, 5), "{}", app.message);
        assert!(app.dirty, "a part write has to mark the save dirty");
        let got = set_of(&app, set);
        assert_eq!(got.parts[1], 5);
        assert_eq!(
            got.decos_of(1),
            Some([2640, 0, 0]),
            "the decoration copy follows the equipment record"
        );
        let warns = mhnx_core::myset::warnings(app.save.as_ref().expect("save"), 0, set);
        assert!(
            warns.is_empty(),
            "a part written through set_part must not warn: {:?}",
            warns
        );

        // 解除 puts the index back to NONE and the copy to zero.
        assert!(app.set_part_clear(set, 1), "{}", app.message);
        let got = set_of(&app, set);
        assert_eq!(got.parts[1], mhnx_core::myset::NONE);
        assert_eq!(got.decos_of(1), Some([0, 0, 0]));
        assert!(app.message.contains("解除しました"), "{}", app.message);

        // A box index that holds nothing cannot be named, so the write is
        // refused and says so rather than writing a dangling index.
        app.dirty = false;
        assert!(!app.set_part_choose(set, 1, 1999));
        assert!(app.message.contains("できませんでした"), "{}", app.message);
        assert!(!app.dirty, "a refused write must not dirty the save");
    }

    /// The sample save holds a set with a talisman in the head slot and the game
    /// loads it, so a kind that does not fit the part is written and reported,
    /// never refused.
    #[test]
    fn a_set_part_of_the_wrong_kind_is_written_and_reported() {
        let dir = Scratch::new("setwarn");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        // Kind 6 is a talisman, in the head part.
        put_record(&mut app, 6, 6, 1, 0);

        assert!(app.set_part_choose(1, 1, 6), "{}", app.message);
        let got = set_of(&app, 1);
        assert_eq!(got.parts[1], 6, "the mismatch is stored, not refused");

        let warns = mhnx_core::myset::warnings(app.save.as_ref().expect("save"), 0, 1);
        assert!(
            warns.iter().any(|w| w.contains("護石") && w.contains("頭")),
            "{:?}",
            warns
        );
    }

    /// Name, colour and the whole-record clear all go through the myset
    /// writers, and an emptied record is byte for byte the one the game leaves.
    #[test]
    fn a_set_name_colour_and_clear_go_through_the_core_writers() {
        let dir = Scratch::new("setwrite");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        put_record(&mut app, 3, 2, 7, 0);
        app.dirty = false;

        assert!(app.set_name_write(0, "テスト"), "{}", app.message);
        assert_eq!(set_of(&app, 0).name, "テスト");
        assert!(app.dirty, "a rename has to mark the save dirty");

        assert!(app.set_colour_write(0, 2, [10, 20, 30, 0xFF]), "{}", app.message);
        assert_eq!(set_of(&app, 0).colours[1], [10, 20, 30, 0xFF]);
        // The weapon and the talisman carry no colour of their own.
        assert!(!app.set_colour_write(0, 0, [9, 9, 9, 0xFF]));
        assert!(!app.set_colour_write(0, 6, [9, 9, 9, 0xFF]));
        assert!(app.message.contains("色"), "{}", app.message);
        assert_eq!(
            set_of(&app, 0).colours[1],
            [10, 20, 30, 0xFF],
            "a refused colour must not land in another part"
        );

        // The part, the name and the colour all go through one clear.
        assert!(app.set_part_choose(0, 2, 3), "{}", app.message);
        assert!(app.set_clear(0), "{}", app.message);
        let got = set_of(&app, 0);
        assert_eq!(got.name, mhnx_core::myset::EMPTY_NAME);
        assert_eq!(
            got.parts,
            [mhnx_core::myset::NONE; mhnx_core::myset::EQUIP_SET_PARTS]
        );
        assert_eq!(got.tail, mhnx_core::myset::EMPTY_TAIL);
        assert_eq!(got.colours, [[0u8; 4]; 5]);
        assert!(got.is_empty());
        assert!(mhnx_core::myset::warnings(app.save.as_ref().expect("save"), 0, 0).is_empty());
        assert!(app.message.contains("空にしました"), "{}", app.message);

        // The editor's selection points into one character's records and cannot
        // survive a reload.
        app.set_selected = Some(7);
        app.set_part_search[0].push_str("頭");
        app.reload_from_save();
        assert_eq!(app.set_selected, None);
        assert!(app.set_part_search.iter().all(|s| s.is_empty()));
    }

    /// The fixture writes zeros into the roster, but the game writes 0xFFFF
    /// for "nothing worn": with zeros there, every unnamed record would claim
    /// to wear box row 0. Put the real empty value in all three slots.
    fn clear_worn(app: &mut MhnxEditor) {
        let slot = app.slot;
        let Some(save) = app.save.as_mut() else {
            return;
        };
        let base = save.slot_base(slot).expect("slot base") as usize;
        for palico in 0..mhnx_core::palico::SLOTS {
            for part in 0..mhnx_core::palico::EQUIP_SLOTS {
                let at = base
                    + mhnx_core::palico::OFF_ROSTER
                    + palico * mhnx_core::palico::RECORD
                    + mhnx_core::palico::EQUIP
                    + part * 2;
                save.body_mut()[at..at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
            }
        }
    }

    /// A palico names the box row it wears in its own record at +0x100: the
    /// two bytes of one part move, and the record they name does not.
    #[test]
    fn assigning_palico_gear_writes_only_the_roster_slot() {
        let dir = Scratch::new("palicogear");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        clear_worn(&mut app);

        // One named palico and two pieces of gear of different kinds.
        let (weapon, head) = {
            let save = app.save.as_mut().expect("save");
            assert!(mhnx_core::palico::set_name(save, 0, 0, "テスト"));
            let w = mhnx_core::palicogear::create(save, 0, mhnx_core::palicogear::WEAPON, 431, 1)
                .expect("a blank weapon record");
            let h = mhnx_core::palicogear::create(save, 0, mhnx_core::palicogear::HEAD, 464, 1)
                .expect("a blank head record");
            (w, h)
        };
        let base = app
            .save
            .as_ref()
            .and_then(|s| s.slot_base(0))
            .expect("slot base") as usize;
        let box_before = app
            .save
            .as_ref()
            .and_then(|s| mhnx_core::palicogear::record(s, 0, weapon))
            .expect("the weapon record")
            .to_vec();
        let body_before = app.save.as_ref().expect("save").body().to_vec();
        app.dirty = false;

        assert!(app.palico_equip_assign(0, 0, weapon as u16), "{}", app.message);
        assert!(app.dirty, "an assignment has to mark the save dirty");
        assert_eq!(
            app.palico_equip_of(0).expect("a palico"),
            [weapon as u16, u16::MAX, u16::MAX]
        );

        // The record the palico names is untouched...
        assert_eq!(
            app.save
                .as_ref()
                .and_then(|s| mhnx_core::palicogear::record(s, 0, weapon))
                .expect("the weapon record"),
            &box_before[..]
        );
        // ...and exactly the two bytes of part 0 in the roster moved.
        let at = base + mhnx_core::palico::OFF_ROSTER + mhnx_core::palico::EQUIP;
        let body_after = app.save.as_ref().expect("save").body().to_vec();
        for (off, (a, b)) in body_before.iter().zip(body_after.iter()).enumerate() {
            if a != b {
                assert!(
                    off >= at && off < at + 2,
                    "byte 0x{:X} changed, outside the worn index",
                    off
                );
            }
        }

        // A record of another kind, a blank box row and a roster record with
        // no name are all refused, and a refusal writes nothing.
        app.dirty = false;
        assert!(!app.palico_equip_assign(0, 1, weapon as u16));
        assert!(app.message.contains("なので"), "{}", app.message);
        assert!(!app.dirty, "a refused assignment must not dirty the save");
        assert_eq!(app.palico_equip_of(0).expect("a palico")[1], u16::MAX);
        assert!(!app.palico_equip_assign(0, 2, 999));
        assert!(!app.palico_equip_assign(1, 0, weapon as u16));
        assert!(!app.palico_equip_assign(0, 3, weapon as u16));

        // 解除 writes 0xFFFF, and a second press has nothing to do.
        assert!(app.palico_equip_assign(0, 1, head as u16), "{}", app.message);
        assert_eq!(
            app.palico_equip_of(0).expect("a palico"),
            [weapon as u16, head as u16, u16::MAX]
        );
        assert!(app.palico_equip_release(0, 0), "{}", app.message);
        assert_eq!(
            app.palico_equip_of(0).expect("a palico"),
            [u16::MAX, head as u16, u16::MAX]
        );
        app.dirty = false;
        assert!(!app.palico_equip_release(0, 0));
        assert!(!app.dirty);
    }

    /// The box's own level and decoration writers are reached through the
    /// panel, and a value the fields cannot hold is refused without writing.
    #[test]
    fn palico_gear_level_and_decoration_are_edited_in_place() {
        let dir = Scratch::new("palicogearlevel");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);

        let index = {
            let save = app.save.as_mut().expect("save");
            assert!(mhnx_core::palico::set_name(save, 0, 0, "テスト"));
            mhnx_core::palicogear::create(save, 0, mhnx_core::palicogear::HEAD, 464, 1)
                .expect("a blank head record")
        };
        assert!(app.palico_equip_assign(0, 1, index as u16), "{}", app.message);
        let before = app
            .save
            .as_ref()
            .and_then(|s| mhnx_core::palicogear::record(s, 0, index))
            .expect("the record")
            .to_vec();
        app.dirty = false;

        assert!(app.palico_gear_level(index, 7), "{}", app.message);
        assert!(app.dirty, "a level edit has to mark the save dirty");
        let eq = app
            .save
            .as_ref()
            .and_then(|s| mhnx_core::palicogear::decode_one(s, 0, index))
            .expect("still decodes");
        assert_eq!(
            (eq.kind, eq.id, eq.level),
            (mhnx_core::palicogear::HEAD, 464, 7)
        );
        let after = app
            .save
            .as_ref()
            .and_then(|s| mhnx_core::palicogear::record(s, 0, index))
            .expect("the record")
            .to_vec();
        assert_eq!(
            &after[mhnx_core::palicogear::WRITABLE..],
            &before[mhnx_core::palicogear::WRITABLE..],
            "the tail is type owned"
        );

        // A level the five bit field cannot show is refused, not clamped.
        app.dirty = false;
        assert!(!app.palico_gear_level(index, 0));
        assert!(!app.palico_gear_level(index, 33));
        assert!(!app.dirty, "a refused level must not dirty the save");

        // One decoration socket moves; the other two and the tail do not.
        assert!(app.palico_gear_decoration(index, 1, 2639), "{}", app.message);
        let after_deco = app
            .save
            .as_ref()
            .and_then(|s| mhnx_core::palicogear::record(s, 0, index))
            .expect("the record")
            .to_vec();
        assert_eq!(
            mhnx_core::equip::Equipment::decorations(0, &after_deco),
            [0, 2639, 0]
        );
        assert_eq!(&after_deco[..0x08], &after[..0x08]);
        assert_eq!(&after_deco[0x0A..], &after[0x0A..]);

        // A fourth socket and a blank record have nothing to write.
        app.dirty = false;
        assert!(!app.palico_gear_decoration(index, 3, 1));
        assert!(!app.palico_gear_decoration(999, 0, 1));
        assert!(!app.dirty);

        // Nothing open: both writers refuse rather than panic.
        let mut fresh = MhnxEditor::default();
        assert!(!fresh.palico_gear_level(0, 2));
        assert!(!fresh.palico_gear_decoration(0, 0, 1));
    }

    /// Two palicos can name the same box row, because the roster stores an
    /// index and the box record is never copied or rewritten.
    #[test]
    fn two_palicos_can_wear_the_same_gear_row() {
        let dir = Scratch::new("palicoshare");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        clear_worn(&mut app);

        let weapon = {
            let save = app.save.as_mut().expect("save");
            assert!(mhnx_core::palico::set_name(save, 0, 0, "テスト"));
            assert!(mhnx_core::palico::set_name(save, 0, 1, "テスト2"));
            mhnx_core::palicogear::create(save, 0, mhnx_core::palicogear::WEAPON, 431, 2)
                .expect("a blank weapon record")
        };
        let record_before = app
            .save
            .as_ref()
            .and_then(|s| mhnx_core::palicogear::record(s, 0, weapon))
            .expect("the weapon record")
            .to_vec();

        assert!(app.palico_equip_assign(0, 0, weapon as u16), "{}", app.message);
        assert!(app.palico_equip_assign(1, 0, weapon as u16), "{}", app.message);
        assert_eq!(
            app.palico_equip_of(1).expect("a palico")[0],
            weapon as u16
        );
        let worn = {
            let save = app.save.as_ref().expect("save");
            mhnx_core::palicogear::worn_by_all(save, 0, weapon)
        };
        assert_eq!(worn, vec![0, 1], "both palicos name the one row");
        assert_eq!(
            app.save
                .as_ref()
                .and_then(|s| mhnx_core::palicogear::record(s, 0, weapon))
                .expect("the weapon record"),
            &record_before[..],
            "the shared record is not rewritten"
        );

        // Past the end of the box there is no record to point at.
        app.dirty = false;
        assert!(!app.palico_equip_assign(
            0,
            0,
            mhnx_core::palicogear::PALICO_EQUIP_COUNT as u16
        ));
        assert!(!app.dirty);
    }

    // ------------------------------------------------------------ csv tests

    /// One item set, read back out of the open save.
    fn item_set_of(app: &MhnxEditor, index: usize) -> mhnx_core::myset::ItemSet {
        app.save
            .as_ref()
            .and_then(|s| {
                mhnx_core::myset::item_sets(s, 0)
                    .into_iter()
                    .find(|set| set.index == index)
            })
            .expect("the set")
    }

    /// The pouch of slot 0, read back out of the open save.
    fn pouch_of(app: &MhnxEditor) -> Vec<mhnx_core::myset::SetItem> {
        app.save
            .as_ref()
            .map(|s| mhnx_core::myset::pouch(s, 0))
            .unwrap_or_default()
    }

    /// The item box makes the trip out as a table and back in through the same
    /// three columns; the import only lands when the dialog is answered.
    #[test]
    fn the_item_box_goes_out_as_csv_and_comes_back() {
        let dir = Scratch::new("csvitems");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        let total = mhnx_core::itembox::TOTAL_ITEM_SLOTS;
        let mut items = vec![ItemSlot::default(); total];
        items[0] = ItemSlot { id: 5, count: 7 };
        items[2] = ItemSlot { id: 12, count: 3 };
        app.items = items;
        app.dirty = false;

        let csv = dir.path().join("items.csv");
        assert!(app.csv_write(CsvKind::Items, &csv), "{}", app.message);
        assert!(app.message.contains("書き出しました"), "{}", app.message);
        assert!(!app.dirty, "an export must not dirty the save");
        let text = std::fs::read_to_string(&csv).expect("the file");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "slot,id,count");
        // Every one of the 2300 records travels, blank ones included: the box
        // is written back by position, so the holes have to come along.
        assert_eq!(lines.len(), total + 1);
        assert_eq!(lines[1], "0,5,7");
        assert_eq!(lines[2], "1,0,0");
        assert_eq!(lines[3], "2,12,3");

        // Reading it back only arms the dialog: the rows are measured on a copy
        // of the save and the save itself is untouched.
        assert!(app.csv_stage(CsvKind::Items, csv.clone()), "{}", app.message);
        assert!(matches!(app.confirm, Confirm::CsvApply));
        {
            let pending = app.csv_pending.as_ref().expect("the staged file");
            assert_eq!(pending.rows.len(), total);
            assert_eq!(pending.report.applied, total);
            assert_eq!(pending.report.skipped, 0);
            assert_eq!(pending.report.clamped, 0);
        }
        let body = app.csv_confirm_text();
        assert!(body.contains("書き込み 2300 行"), "{}", body);
        assert!(body.contains("アイテムボックス"), "{}", body);
        let stored = app
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(0))
            .expect("box");
        assert!(stored[0].is_blank(), "the dry run wrote nothing");

        // Something else on screen, so 元に戻す has something to put back.
        let mut other = vec![ItemSlot::default(); total];
        other[1] = ItemSlot { id: 9, count: 1 };
        app.items = other;

        let report = app.csv_commit().expect("the report");
        assert_eq!(report.applied, total);
        assert!(app.dirty, "an import is a change");
        assert!(app.message.contains("適用しました"), "{}", app.message);
        let stored = app
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(0))
            .expect("box");
        assert_eq!(stored[0], ItemSlot { id: 5, count: 7 });
        assert_eq!(stored[2], ItemSlot { id: 12, count: 3 });
        // The working copy the item tab reads was rebuilt from the save.
        assert_eq!(app.items[0], ItemSlot { id: 5, count: 7 });
        assert!(app.items[1].is_blank());
        // One undo puts the box that was on screen before the import back.
        app.undo_item();
        assert_eq!(app.items[1], ItemSlot { id: 9, count: 1 });
        // Nothing is staged after the import, so a second commit does nothing.
        assert!(app.csv_commit().is_none());
    }

    /// A file the parser cannot read is reported with the line and the column
    /// it names, and nothing is staged or written.
    #[test]
    fn a_broken_csv_file_names_the_line_and_changes_nothing() {
        let dir = Scratch::new("csvbad");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        let before = app
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(0))
            .expect("box");
        let csv = dir.path().join("broken.csv");
        std::fs::write(&csv, "slot,id,count\r\n0,5,7\r\n1,abc,3\r\n").expect("the file");
        app.dirty = false;

        assert!(!app.csv_stage(CsvKind::Items, csv));
        // The wording of CsvError is shown as it is: it names the line and the
        // column a spreadsheet would point at.
        assert!(app.message.contains("3 行目"), "{}", app.message);
        assert!(app.message.contains("2 列目"), "{}", app.message);
        assert!(app.message.contains("abc"), "{}", app.message);
        assert!(app.csv_pending.is_none(), "a broken file is not staged");
        assert!(matches!(app.confirm, Confirm::None));
        assert!(!app.dirty);
        let after = app
            .save
            .as_ref()
            .and_then(|s| s.read_item_box(0))
            .expect("box");
        assert_eq!(before, after);
        assert!(app.csv_commit().is_none());

        // A file that is not there at all says so, too.
        assert!(!app.csv_stage(CsvKind::Items, dir.path().join("missing.csv")));
        assert!(app.message.contains("読めません"), "{}", app.message);
    }

    /// The equipment box goes out as the eight columns of csvout, and a hand
    /// edited row goes back in through the same dialog as the item box.
    #[test]
    fn the_equipment_box_goes_out_as_csv_and_comes_back() {
        let dir = Scratch::new("csvequip");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);

        let mut rec = [0u8; mhnx_core::equip::EQUIP_RECORD];
        rec[0] = 1; // a weapon type the equipment box knows
        assert!(mhnx_core::equip::set_level(&mut rec, 3));
        rec[2..4].copy_from_slice(&10u16.to_le_bytes());
        app.save
            .as_mut()
            .and_then(|s| s.equipment_record_mut(0, 5))
            .expect("record 5")
            .copy_from_slice(&rec);

        let csv = dir.path().join("equip.csv");
        assert!(app.csv_write(CsvKind::Equipment, &csv), "{}", app.message);
        let text = std::fs::read_to_string(&csv).expect("the file");
        assert_eq!(
            text.lines().next(),
            Some("index,kind,id,level,transmog,deco1,deco2,deco3")
        );
        assert!(text.contains("5,1,10,3,0,0,0,0\r\n"), "{:?}", text);
        // Only the records that hold something are listed.
        assert_eq!(text.lines().count(), 2);

        // Hand edit the row, the way a spreadsheet would.
        let edited = text.replace("5,1,10,3,0,0,0,0", "5,1,11,5,0,0,0,0");
        std::fs::write(&csv, edited).expect("the edited file");
        let before = app
            .save
            .as_ref()
            .and_then(|s| s.equipment_at(0, 5))
            .expect("record");
        assert_eq!((before.kind, before.id, before.level), (1, 10, 3));

        assert!(app.csv_stage(CsvKind::Equipment, csv), "{}", app.message);
        assert!(matches!(app.confirm, Confirm::CsvApply));
        assert_eq!(app.csv_pending.as_ref().expect("staged").rows.len(), 1);
        let after = app
            .save
            .as_ref()
            .and_then(|s| s.equipment_at(0, 5))
            .expect("record");
        assert_eq!(
            (after.kind, after.id, after.level),
            (1, 10, 3),
            "nothing is written before the dialog is answered"
        );

        let report = app.csv_commit().expect("the report");
        assert_eq!(report.applied, 1);
        let got = app
            .save
            .as_ref()
            .and_then(|s| s.equipment_at(0, 5))
            .expect("record");
        assert_eq!((got.kind, got.id, got.level), (1, 11, 5));
        assert!(app.dirty);
    }

    /// The item set editor writes one entry at a time, and the id 0 it uses
    /// for "empty" drops the count rather than leaving a stray one behind.
    #[test]
    fn an_item_set_entry_is_written_and_cleared() {
        let dir = Scratch::new("itemsetentry");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        app.dirty = false;

        assert!(app.item_set_entry_write(3, 0, 5, 7), "{}", app.message);
        assert!(app.dirty);
        let set = item_set_of(&app, 3);
        assert_eq!(set.items[0], mhnx_core::myset::SetItem { id: 5, count: 7 });

        // id 0 means empty: the count goes with it.
        assert!(app.item_set_entry_write(3, 0, 0, 7), "{}", app.message);
        let set = item_set_of(&app, 3);
        assert_eq!(set.items[0], mhnx_core::myset::SetItem { id: 0, count: 0 });

        // A name is written the way the equipment set editor writes one.
        assert!(app.item_set_name_write(3, "旅支度"), "{}", app.message);
        assert_eq!(item_set_of(&app, 3).name, "旅支度");

        // Out of range, a record the core refuses: nothing moves.
        app.dirty = false;
        assert!(!app.item_set_entry_write(3, 0, 4000, 1));
        assert!(!app.item_set_entry_write(
            3,
            0,
            5,
            mhnx_core::itembox::MAX_COUNT + 1
        ));
        assert!(!app.item_set_entry_write(mhnx_core::myset::ITEM_SETS, 0, 5, 1));
        assert!(!app.dirty);

        // The yellow line the editor draws is this list of reports.
        assert!(app.item_set_entry_write(3, 0, 5, 7), "{}", app.message);
        let clean =
            mhnx_core::myset::item_set_warnings(app.save.as_ref().expect("save"), 0, 3);
        assert!(clean.is_empty(), "{:?}", clean);
        // The same item in a second entry is what the warning list reports.
        assert!(app.item_set_entry_write(3, 1, 5, 1), "{}", app.message);
        let warns =
            mhnx_core::myset::item_set_warnings(app.save.as_ref().expect("save"), 0, 3);
        assert_eq!(warns.len(), 1, "{:?}", warns);
        assert!(warns[0].contains("重複"), "{:?}", warns);
    }

    /// The pouch is written, read back, copied into a set and replaced by it;
    /// what ポーチへ適用 replaces is kept so 元に戻す can put it back.
    #[test]
    fn the_pouch_can_be_written_read_back_and_restored() {
        let dir = Scratch::new("pouchedit");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        assert_eq!(pouch_of(&app).len(), mhnx_core::myset::POUCH_SLOTS);

        assert!(app.pouch_entry_write(0, 5, 7), "{}", app.message);
        assert!(app.pouch_entry_write(1, 9, 2), "{}", app.message);
        let pouch = pouch_of(&app);
        assert_eq!(pouch[0], mhnx_core::myset::SetItem { id: 5, count: 7 });
        assert_eq!(pouch[1], mhnx_core::myset::SetItem { id: 9, count: 2 });

        // id 0 clears the entry instead of writing a stray count.
        assert!(app.pouch_entry_write(1, 0, 50), "{}", app.message);
        assert_eq!(
            pouch_of(&app)[1],
            mhnx_core::myset::SetItem { id: 0, count: 0 }
        );
        // A count past the cap and an entry past the end are refused.
        app.dirty = false;
        assert!(!app.pouch_entry_write(2, 5, mhnx_core::itembox::MAX_COUNT + 1));
        assert!(!app.pouch_entry_write(mhnx_core::myset::POUCH_SLOTS, 5, 1));
        assert!(!app.dirty);

        // ポーチから取り込む copies all 32 entries, the empty ones included.
        assert_eq!(app.item_set_from_pouch(3), 1);
        let set = item_set_of(&app, 3);
        assert_eq!(set.items[0], mhnx_core::myset::SetItem { id: 5, count: 7 });
        assert_eq!(
            set.items[31],
            mhnx_core::myset::SetItem { id: 0, count: 0 }
        );

        // Something the set does not hold, so replacing it is visible.
        assert!(app.pouch_entry_write(5, 77, 1), "{}", app.message);
        assert_eq!(app.item_set_to_pouch(3), 1);
        let pouch = pouch_of(&app);
        assert_eq!(pouch[0], mhnx_core::myset::SetItem { id: 5, count: 7 });
        assert_eq!(pouch[5], mhnx_core::myset::SetItem { id: 0, count: 0 });
        assert!(app.pouch_undo.is_some(), "the replaced pouch is kept");

        // 元に戻す puts the pouch that was there before back.
        assert!(app.pouch_restore(), "{}", app.message);
        assert_eq!(
            pouch_of(&app)[5],
            mhnx_core::myset::SetItem { id: 77, count: 1 }
        );
        assert!(app.pouch_undo.is_none());
        assert!(!app.pouch_restore(), "a second undo has nothing to put back");

        // A set that holds nothing must not empty the pouch.
        let before = pouch_of(&app);
        assert_eq!(app.item_set_to_pouch(20), 0);
        assert!(app.message.contains("空"), "{}", app.message);
        assert_eq!(pouch_of(&app), before);
    }

    /// A .gmd file the way the game writes one: the header, the key, then the
    /// NUL terminated strings as the tail of the file.
    fn gmd_bytes(strings: &[String]) -> Vec<u8> {
        let key = b"test";
        let mut block = Vec::new();
        for s in strings {
            block.extend_from_slice(s.as_bytes());
            block.push(0);
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"GMD\0");
        out.extend_from_slice(&0x0001_0302u32.to_le_bytes()); // version
        out.extend_from_slice(&0u32.to_le_bytes()); // 0x08
        out.extend_from_slice(&0u32.to_le_bytes()); // 0x0C tag
        out.extend_from_slice(&0u32.to_le_bytes()); // 0x10
        out.extend_from_slice(&0u32.to_le_bytes()); // 0x14
        out.extend_from_slice(&(strings.len() as u32).to_le_bytes()); // 0x18 count
        out.extend_from_slice(&0u32.to_le_bytes()); // 0x1C line table
        out.extend_from_slice(&(block.len() as u32).to_le_bytes()); // 0x20 block length
        out.extend_from_slice(&(key.len() as u32).to_le_bytes()); // 0x24 key length
        out.extend_from_slice(key);
        out.push(0);
        assert_eq!(out.len(), 0x28 + key.len() + 1);
        out.extend_from_slice(&block);
        out
    }

    /// The 264 strings of a medal table: 132 names, then 132 descriptions.
    fn medal_strings() -> Vec<String> {
        let mut v: Vec<String> = (0..132).map(|i| format!("勲章{}", i + 1)).collect();
        v.extend((0..132).map(|i| format!("説明{}", i + 1)));
        v
    }

    /// An activityData.atd holding the records a test needs. Every record owns
    /// a consecutive flag pair, the way the real table does.
    fn atd_bytes(records: &[(u8, u32, u16)]) -> Vec<u8> {
        let mut out = vec![0u8; 8];
        out[2] = 0xA0;
        out[3] = 0x40;
        out[4..8].copy_from_slice(&(records.len() as u32).to_le_bytes());
        for (i, (kind, quest, accept)) in records.iter().enumerate() {
            let mut rec = [0u8; 55];
            rec[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            rec[0x04] = *kind;
            rec[0x16..0x1A].copy_from_slice(&quest.to_le_bytes());
            rec[0x33..0x35].copy_from_slice(&accept.to_le_bytes());
            rec[0x35..0x37].copy_from_slice(&(accept + 1).to_le_bytes());
            out.extend_from_slice(&rec);
        }
        out
    }

    #[test]
    fn the_romfs_tables_are_found_and_parsed() {
        let dir = Scratch::new("romfs");
        let table = dir.path().join("nativeNX").join("table");
        std::fs::create_dir_all(&table).expect("table folder");
        // Both languages, so the choice between them is checked as well.
        std::fs::write(table.join("GC_Medal_jpn.gmd"), gmd_bytes(&medal_strings())).expect("medal");
        let english: Vec<String> = (0..264).map(|i| format!("Award{}", i + 1)).collect();
        std::fs::write(table.join("GC_Medal_eng.gmd"), gmd_bytes(&english)).expect("english");
        let mut names: Vec<String> = Vec::new();
        for i in 0..184 {
            names.push(format!("依頼{}", i + 1));
            for _ in 0..4 {
                names.push(String::from("dummy"));
            }
        }
        std::fs::write(table.join("activityData_jpn.gmd"), gmd_bytes(&names)).expect("names");
        let records: Vec<(u8, u32, u16)> = (0..184)
            .map(|i| (if i == 183 { 3 } else { 0 }, i as u32 + 1, 300 + i as u16 * 2))
            .collect();
        std::fs::write(table.join("activityData.atd"), atd_bytes(&records)).expect("atd");

        let mut app = MhnxEditor::default();
        app.romfs_path = dir.path().display().to_string();
        app.load_romfs();

        assert_eq!(app.award_names.len(), 132);
        assert_eq!(app.request_names.len(), 184);
        assert_eq!(app.request_records.len(), 184);
        assert_eq!(app.award_names[0], "勲章1", "the Japanese table is the one to use");
        assert_eq!(app.request_names[0], "依頼1");
        assert_eq!(app.request_records[0].accept_flag, 300);
        assert_eq!(app.request_records[0].complete_flag, 301);
        assert_eq!(app.request_records[183].kind, 3);
        assert!(app.romfs_dir.is_some());
        assert!(app.message.contains("勲章名 132"), "{}", app.message);
        assert!(app.message.contains("依頼レコード 184"), "{}", app.message);
    }

    #[test]
    fn a_folder_without_the_tables_drops_the_names_it_had() {
        let dir = Scratch::new("romfsgone");
        let table = dir.path().join("nativeNX").join("table");
        std::fs::create_dir_all(&table).expect("table folder");
        std::fs::write(table.join("GC_Medal_jpn.gmd"), gmd_bytes(&medal_strings())).expect("medal");
        let mut app = MhnxEditor::default();
        app.romfs_path = dir.path().display().to_string();
        app.load_romfs();
        assert_eq!(app.award_names.len(), 132);
        assert!(app.romfs_dir.is_some());

        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).expect("empty folder");
        app.romfs_path = empty.display().to_string();
        app.load_romfs();
        assert!(
            app.award_names.is_empty(),
            "the previous folder's names must not stay on screen"
        );
        assert!(app.request_names.is_empty());
        assert!(app.request_records.is_empty());
        assert!(app.romfs_dir.is_none());
        assert!(app.message.contains("見つかりません"), "{}", app.message);
    }

    #[test]
    fn an_award_toggle_is_taken_back_by_one_press() {
        let dir = Scratch::new("awardundo");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        app.save.as_mut().expect("save").set_slot_used(0, true);
        let before = app.save.as_ref().expect("save").body().to_vec();
        assert_eq!(awards::count(app.save.as_ref().expect("save"), 0), 0);

        app.set_award(4, true);
        assert!(app.dirty, "a toggle has to mark the save dirty");
        assert_eq!(
            awards::has(app.save.as_ref().expect("save"), 0, 4),
            Some(true)
        );
        assert!(
            awards::copies_agree(app.save.as_ref().expect("save"), 0),
            "all three copies must be written together"
        );
        assert_eq!(awards::count(app.save.as_ref().expect("save"), 0), 1);
        assert_eq!(app.progress_undo.len(), 1);

        app.undo_progress();
        assert_eq!(app.save.as_ref().expect("save").body(), &before[..]);
        assert_eq!(awards::count(app.save.as_ref().expect("save"), 0), 0);
        assert!(app.progress_undo.is_empty());

        // A step belongs to the slot it was measured in; reloading drops it.
        app.set_award(5, true);
        assert_eq!(app.progress_undo.len(), 1);
        app.slot = 1;
        app.reload_from_save();
        assert!(app.progress_undo.is_empty());
    }

    #[test]
    fn a_request_pairs_its_two_flags_and_one_undo_step_restores_both() {
        let dir = Scratch::new("request");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        app.save.as_mut().expect("save").set_slot_used(0, true);
        app.request_records = vec![RequestRecord {
            index: 0,
            kind: 0,
            quest_id: 4242,
            accept_flag: 300,
            complete_flag: 301,
        }];
        app.request_names = vec![String::from("テストの依頼")];
        let before = app.save.as_ref().expect("save").body().to_vec();

        // 完了 is only ever written together with 受理.
        app.set_request_complete(0, true, false);
        assert!(app.dirty);
        assert_eq!(app.request_on(0), (true, true));
        assert!(app.message.contains("受理も立てました"), "{}", app.message);

        // 完了だけ外す leaves the accept bit alone.
        app.set_request_complete(0, false, false);
        assert_eq!(app.request_on(0), (true, false));

        // 受理も外す takes both.
        app.set_request_complete(0, false, true);
        assert_eq!(app.request_on(0), (false, false));

        assert_eq!(app.progress_undo.len(), 3);
        app.undo_progress();
        app.undo_progress();
        app.undo_progress();
        assert_eq!(app.save.as_ref().expect("save").body(), &before[..]);
        assert_eq!(app.request_on(0), (false, false));
    }

    #[test]
    fn clearing_a_quest_also_marks_it_seen_and_failure_stays_read_only() {
        let dir = Scratch::new("quest");
        let path = write_a_save_with_slots(dir.path(), "system");
        let mut app = MhnxEditor::default();
        app.open_save(path);
        assert!(app.save.is_some(), "{}", app.message);
        let before = app.save.as_ref().expect("save").body().to_vec();

        app.set_quest(821, Progress::Cleared, true);
        assert!(app.dirty);
        assert!(app.quest_on(821, Progress::Cleared));
        assert!(app.quest_on(821, Progress::Seen), "クリア implies 既読");
        assert_eq!(
            questprogress::count(app.save.as_ref().expect("save"), 0, Progress::Cleared),
            1
        );

        // The invariant is refused by the core, so the 既読 of a cleared quest
        // stays set and the message says why.
        app.set_quest(821, Progress::Seen, false);
        assert!(app.quest_on(821, Progress::Seen));
        assert!(app.message.contains("クリア済み"), "{}", app.message);
        assert_eq!(app.progress_undo.len(), 1, "a refused write is not a step");

        // 失敗 is read only.
        app.set_quest(821, Progress::Failed, true);
        assert!(!app.quest_on(821, Progress::Failed));
        assert!(app.message.contains("読み取り専用"), "{}", app.message);

        app.undo_progress();
        assert_eq!(app.save.as_ref().expect("save").body(), &before[..]);
        assert!(!app.quest_on(821, Progress::Cleared));
        assert!(!app.quest_on(821, Progress::Seen));
    }
}



/// The colour a warning about the data itself is drawn in.
///
/// It is the amber the 未保存 marker uses: red is kept for a write that did not
/// come back the way it was meant to.
fn warning_color() -> egui::Color32 {
    egui::Color32::from_rgb(240, 180, 60)
}

/// Fill of an award cell. A held award is a muted green, a missing one is
/// almost the panel's own colour, so the grid reads as a picture of 132 bits.
///
/// Measured as WCAG contrast against the text each one carries: 6.4 to 1 on
/// the green and 5.9 to 1 on the dark fill. The dark fill is only 1.2 to 1
/// against the panel, which is why every cell also has an outline.
fn award_fill(on: bool) -> egui::Color32 {
    if on {
        egui::Color32::from_rgb(58, 92, 64)
    } else {
        egui::Color32::from_rgb(40, 40, 44)
    }
}

/// The text colour of an award cell, paired with award_fill.
fn award_text(on: bool) -> egui::Color32 {
    if on {
        egui::Color32::from_rgb(226, 240, 228)
    } else {
        egui::Color32::from_rgb(163, 163, 170)
    }
}

/// The outline of an award cell, so the dark half of the grid is still a grid.
fn award_stroke(on: bool) -> egui::Color32 {
    if on {
        egui::Color32::from_rgb(122, 152, 128)
    } else {
        egui::Color32::from_rgb(66, 66, 74)
    }
}

/// Name shown for a decoration socket: a dash when it is empty.
fn deco_label(id: u16) -> String {
    if id == 0 {
        String::from("—")
    } else {
        deco_name(id)
    }
}

/// Decoration name looked up in the generated game data tables.
fn deco_name(id: u16) -> String {
    #[cfg(feature = "gamedata")]
    {
        let ids = mhnx_core::gamedata::JWL_I_DS;
        let names = mhnx_core::gamedata::JWL_NAMES;
        match ids.iter().position(|v| *v as u16 == id) {
            Some(i) => names
                .get(i)
                .map(|s| (*s).to_owned())
                .unwrap_or_else(|| format!("#{}", id)),
            None => format!("#{}", id),
        }
    }
    #[cfg(not(feature = "gamedata"))]
    {
        format!("#{}", id)
    }
}

/// Readable "name Lv n" for the piece a record is transmogged from.
fn transmog_label(e: &mhnx_core::equip::Equipment) -> String {
    if e.transmog == 0 {
        return String::from("—");
    }
    let mut look = *e;
    look.id = e.transmog;
    format!("{} Lv{}", mhnx_core::equip::name(&look), e.transmog_level)
}

/// Item name looked up in the generated game data tables.
fn item_name(id: u16) -> String {
    #[cfg(feature = "gamedata")]
    {
        mhnx_core::gamedata::ITEM_NAME_LIST
            .get(id as usize)
            .map(|s| (*s).to_owned())
            .unwrap_or_else(|| format!("(unknown {})", id))
    }
    #[cfg(not(feature = "gamedata"))]
    {
        format!("#{}", id)
    }
}
