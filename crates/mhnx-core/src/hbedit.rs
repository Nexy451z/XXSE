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
use crate::awards;
use crate::check::{self, Finding, Severity};
use crate::diffvalues::{diff_values, ValueChange};
use crate::charm::{self, Charm, CHARM_COUNT};
use crate::dlc;
use crate::dlcats::{self, Lang};
use crate::equipops::{self, EquipError};
use crate::equiprefs;
use crate::save::Options;
use crate::itembox::{self, DedupePreview, ItemSlot, MAX_COUNT, TOTAL_ITEM_SLOTS};
use crate::quest::{Gmd, Mib};
use crate::questfields::{self, FieldKind};
use crate::questprogress::{self, Progress};
use crate::requests;
use crate::romfstables::RequestRecord;
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
    /// CSV export and import of the item box and the equipment box.
    ///
    /// A screen of its own rather than a mode: the folder it lists has nothing
    /// to do with the record that is open, and the two keys that matter (write
    /// the boxes out, apply a file) are the whole screen.
    Csv,
    /// Per-slot progress: guild card awards, request flags and quest bits.
    ///
    /// One screen with three views (勲章 / 依頼 / クエスト) rather than three
    /// screens: they are the same question - what has this character done -
    /// about three bit maps that all sit inside the character slot, and they
    /// share the same two keys (a row, and one bit on that row).
    ///
    /// Every action here flips **one bit per press**. Nothing on this screen
    /// takes a second press to confirm, because nothing here can lose a record:
    /// an award or a quest bit is a single flag the game itself re-merges, so
    /// pressing the same key again is the undo.
    Progress,
}

/// The three views of [Screen::Progress], in the order L / R walks them.
pub const PROGRESS_AWARDS: usize = 0;
/// The request / event flag view.
pub const PROGRESS_REQUESTS: usize = 1;
/// The quest cleared / seen / failed view.
pub const PROGRESS_QUESTS: usize = 2;
/// How many views the progress screen has.
pub const PROGRESS_TABS: usize = 3;

/// Rows Left / Right jump on the progress screen.
///
/// One page of quests (1508 indices over this step is 31 pages), and a
/// comfortable stride over 132 awards or 380 request flags.
pub const PROGRESS_JUMP: usize = 50;

/// The two request flag bands, as `[start, end)` numbers into the flag map.
///
/// The game's own request table uses 300..555 and 1270..1395 (see
/// romfstables: every record owns an accept / complete pair, and the second
/// band starts at 1270). The other flags in the 1536 bit map belong to
/// other unlocks, so the request view shows these two bands and reports how
/// many bits are set outside them rather than pretending they are requests.
pub const REQUEST_BANDS: [(usize, usize); 2] = [(300, 555), (1270, 1395)];

/// Rows the request view lists: the two bands, end to end (380).
///
/// Written as the sum of the bands rather than as 380, so a band that moves
/// cannot leave the row count disagreeing with the list.
pub const REQUEST_ROWS: usize =
    (REQUEST_BANDS[0].1 - REQUEST_BANDS[0].0) + (REQUEST_BANDS[1].1 - REQUEST_BANDS[1].0);

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

/// The last component of a path: what the user calls the file.
fn file_name_of(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharmEdit {
    /// Charm slot index (0..1999).
    pub index: usize,
    /// Talisman kind: the byte at +0x02, 1 兵士 … 10 天, with 7 龍.
    ///
    /// This is the row the console cycles; the grade below follows it, because
    /// the game writes the rarity as part of the same pair (+0x12/+0x13 is the
    /// charm item id 353..356). Cycling the rarity instead was what left 龍の
    /// 護石 unreachable and renamed 天 to 英雄.
    pub kind: u8,
    /// Talisman grade (rarity 97..100), derived from [CharmEdit::kind].
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

impl Default for CharmEdit {
    fn default() -> Self {
        CharmEdit {
            index: 0,
            kind: 10, // 天の護石, the grade the sample saves are full of
            grade: crate::charm::TalismanGrade::Enduring,
            slots: 0,
            skill1: 0,
            points1: 0,
            skill2: 0,
            points2: 0,
            field: 0,
        }
    }
}

/// Rows on the talisman form.
///
/// Seven, and the state machine (Up / Down wrap with 7), the console's list
/// geometry and the host tests all read this number, so the highlight and the
/// row the form edits cannot drift apart.
pub const CHARM_EDIT_ROWS: usize = 7;

/// What `[A]` does on one row of the talisman form.
///
/// The console's `[A]` used to be one thing on every row - write the form into
/// the box - while the pad dispatch already did something else on the kind, the
/// slot and the two skill rows. The footer and the inspector button therefore
/// said `[A] BOXへ反映` next to a key that opened the skill list. The row's
/// meaning lives here now, and the label, the footer line and the key the
/// console hands to the state machine are all read from this value, so the
/// three cannot disagree again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharmAction {
    /// 装備BOX枠: write the form into the box slot it names.
    WriteToBox,
    /// 種類: cycle the kind, and the rarity that is stored with it.
    CycleKind,
    /// スロット数: cycle the slot count inside the kind's ceiling.
    CycleSlots,
    /// 第1スキル / 第2スキル: open the skill name picker.
    PickSkill,
    /// 第1スキル値 / 第2スキル値: raise the value by one step.
    RaisePoints,
}

impl CharmAction {
    /// The action `[A]` takes on a row, by row number.
    ///
    /// Row 0 is the one row that still writes: it names the box slot the form
    /// would go into, so `[A]` there is the copy itself. The other rows edit
    /// one field each, the way the arrow keys do.
    pub fn of_row(row: usize) -> Self {
        match row {
            1 => CharmAction::CycleKind,
            2 => CharmAction::CycleSlots,
            3 | 5 => CharmAction::PickSkill,
            4 | 6 => CharmAction::RaisePoints,
            _ => CharmAction::WriteToBox,
        }
    }

    /// The word the footer and the inspector `[A]` button print.
    pub fn label(self) -> &'static str {
        match self {
            CharmAction::WriteToBox => "BOXへ反映",
            CharmAction::CycleKind => "種類を切替",
            CharmAction::CycleSlots => "スロットを切替",
            CharmAction::PickSkill => "スキルを選ぶ",
            CharmAction::RaisePoints => "数値を増やす",
        }
    }

    /// The key the console sends to the state machine for this action.
    pub fn key(self) -> Key {
        match self {
            CharmAction::WriteToBox => Key::A,
            CharmAction::PickSkill => Key::Y,
            // The three that step a field use the arrow key the footer names,
            // so `[A]` and `[◀/▶]` cannot drift apart either.
            CharmAction::CycleKind | CharmAction::CycleSlots | CharmAction::RaisePoints => Key::Right,
        }
    }

}

impl CharmEdit {
    /// The row the cursor is on, clamped to the rows the form has.
    pub fn row(&self) -> usize {
        self.field.min(CHARM_EDIT_ROWS - 1)
    }

    /// What `[A]` does on the row the cursor is on.
    pub fn a_action(&self) -> CharmAction {
        CharmAction::of_row(self.row())
    }

    /// The label the footer and the inspector `[A]` button print.
    pub fn a_label(&self) -> &'static str {
        self.a_action().label()
    }

    /// The key the console sends when `[A]` is pressed on this row.
    pub fn a_key(&self) -> Key {
        self.a_action().key()
    }

    /// The footer line for this row: the `[A]` chip is the row's own word, the
    /// rest are the keys the form always has.
    pub fn footer(&self) -> String {
        // [X] is the save key on every non-picker screen (the console's global
        // X -> QuickSave), which on this form writes the record into the box and
        // then saves the file. It used to be advertised as 刻み here, which the
        // pad never sent the state machine.
        format!(
            "[A] {}  [▲/▼] 行  [◀/▶] 値  [Y] スキル行で選択  [X] 保存  [B] 戻る",
            self.a_label()
        )
    }

    /// The line the form prints beside its header: the measured ceiling of the
    /// kind on the form, 「R100 / スキル1+10 / スキル2+13 / S3」.
    ///
    /// It is drawn there rather than in a list column because the list draws
    /// only the title and the value of a row - the meta column is the
    /// inspector's, shown for the selected row alone.
    pub fn header_note(&self) -> String {
        match charm::kind_limits(self.kind) {
            Some(l) => format!("R{} / {}", l.rarity, l.compact()),
            None => format!(
                "レア度 {} / 最大 S{}",
                self.grade.rarity(),
                self.grade.max_slots()
            ),
        }
    }
}

/// One row of the drawn talisman form.
///
/// The console's list draws `title` on the left and `value` on the right and
/// never draws `meta`: that is the inspector's second line, and the inspector
/// only shows the row the cursor is on. The measured ceiling used to live in
/// the 種類 row's meta alone, which is why it was missing from the row itself;
/// the form prints it beside its header as well ([CharmEdit::header_note]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharmFormRow {
    /// The row number Up / Down counts.
    pub field: usize,
    /// Left column: the field's name.
    pub title: String,
    /// The inspector's second line (the list never draws this).
    pub meta: String,
    /// Right column: the value the row holds.
    pub value: String,
    /// The row badge ("#04").
    pub badge: String,
}

/// The seven rows of the drawn talisman form, in row order.
///
/// The console builds its list from this instead of assembling the strings
/// itself, so what the host tests read is what the screen draws.
pub fn charm_form_rows(e: &CharmEdit) -> [CharmFormRow; CHARM_EDIT_ROWS] {
    let s1 = crate::skills::skill_name(e.skill1).unwrap_or("なし");
    let s2 = crate::skills::skill_name(e.skill2).unwrap_or("なし");
    // The kind on the form carries the measured ceiling of its row of
    // charm::KIND_LIMITS. It is the tighter of the two answers, so the range
    // column reports it: 女王 / 王 / 龍 stop at +8 on the first skill even
    // though the rarity-99 table reaches +13 on the second.
    let limits = charm::kind_limits(e.kind);
    let range_of = |skill: u8, second: bool| -> String {
        if skill == 0 {
            return String::from("なし");
        }
        let cap = limits.and_then(|l| l.max_points(second));
        match charm::allowed_points_rarity(e.grade.rarity(), skill, second) {
            Some(r) if r.min == 0 && r.max == 0 => String::from("このレア度では不可"),
            Some(r) => match cap {
                Some(c) => format!("{} 〜 {}", r.min, r.max.min(c)),
                None => String::from("この種類は第2スキル不可"),
            },
            None => match cap {
                Some(c) => format!("上限 {}", c),
                None => String::from("—"),
            },
        }
    };
    [
        CharmFormRow {
            field: 0,
            title: String::from("装備BOX枠"),
            meta: String::from("対象スロット"),
            value: format!("#{:04}", e.index),
            badge: String::from("#01"),
        },
        CharmFormRow {
            field: 1,
            title: String::from("種類"),
            // The game's own name of the talisman is the value; the rarity and
            // the measured ceiling of this kind are the inspector's line.
            meta: match limits {
                Some(l) => format!("R{} / {}", l.rarity, l.compact()),
                None => format!(
                    "◀ ▶ で切替 / レア度 {} / 最大 S{}",
                    e.grade.rarity(),
                    e.grade.max_slots()
                ),
            },
            value: String::from(charm::kind_name(e.kind).unwrap_or("?")),
            badge: String::from("#02"),
        },
        CharmFormRow {
            field: 2,
            title: String::from("スロット数"),
            meta: format!(
                "0 〜 {}",
                limits
                    .map(|l| l.max_slots.min(e.grade.max_slots()))
                    .unwrap_or_else(|| e.grade.max_slots())
            ),
            value: format!("S{} ({})", e.slots, "◯".repeat(e.slots.min(3) as usize)),
            badge: String::from("#03"),
        },
        CharmFormRow {
            field: 3,
            title: String::from("第1スキル"),
            meta: format!("Skill ID: {}", e.skill1),
            value: s1.to_string(),
            badge: String::from("#04"),
        },
        CharmFormRow {
            field: 4,
            title: String::from("第1スキル値"),
            meta: range_of(e.skill1, false),
            value: format!("{:+}", e.points1),
            badge: String::from("#05"),
        },
        CharmFormRow {
            field: 5,
            title: String::from("第2スキル"),
            meta: format!("Skill ID: {}", e.skill2),
            value: s2.to_string(),
            badge: String::from("#06"),
        },
        CharmFormRow {
            field: 6,
            title: String::from("第2スキル値"),
            meta: range_of(e.skill2, true),
            value: format!("{:+}", e.points2),
            badge: String::from("#07"),
        },
    ]
}

/// The next kind when the kind row is turned, skipping row 0 ("装備なし").
fn step_kind(kind: u8, forward: bool) -> u8 {
    let top = crate::charm::TALISMAN_KIND_COUNT;
    let cur = if crate::charm::is_kind(kind) { kind } else { 10 };
    if forward {
        if cur >= top {
            1
        } else {
            cur + 1
        }
    } else if cur <= 1 {
        top
    } else {
        cur - 1
    }
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

/// Rows of the palico equipment panel: the weapon, the head and the body.
///
/// The order is the worn list's own (`palico::EQUIP` holds weapon, head, body),
/// which is also `palicogear::KINDS`, so row `n` edits part `n` of both.
pub const PALICO_EQUIP_ROWS: usize = crate::palico::EQUIP_SLOTS;

/// Names of the three parts, in worn-list order.
pub const PALICO_PART_NAMES: [&str; PALICO_EQUIP_ROWS] = ["武器", "頭", "胴"];

/// What a worn-list slot holds when the palico wears nothing there.
///
/// `palico::Palico::equip` documents 0xFFFF as the empty value and the game
/// writes it, so [-] clears a part back to the value the save itself uses
/// rather than to one made up here.
pub const PALICO_EQUIP_NONE: u16 = 0xFFFF;

/// Shown under the palico equipment rows.
///
/// `equip::name` answers "-" for the three palico kinds on purpose: the name
/// tables exist but their id mapping is not established, so the panel prints
/// the box index and the equip id instead of a name that could be wrong.
pub const PALICO_EQUIP_NAME_NOTE: &str = "オトモ装備の名前は未対応（装備IDと枠番号で表示します）";

/// The palico equipment panel, open as a mode of one of two screens.
///
/// This is a **mode**, not a variant of `Screen`: the roster screen and the
/// download-palico screen both keep their own list and cursor underneath, and
/// the panel is the same state whichever one opened it. `pick` is what [A]
/// would write and is kept apart from the roster on purpose - walking the rows
/// must not write anything, only [A] and the second [-] do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PalicoEquip {
    /// Roster record index (0..palico::SLOTS) being equipped.
    pub palico: usize,
    /// Row under the cursor: 0 weapon, 1 head, 2 body.
    pub field: usize,
    /// Box index [A] would write, per part.
    ///
    /// `PALICO_EQUIP_NONE` means the box holds no record of that kind, which is
    /// the only value that cannot be written.
    pub pick: [u16; PALICO_EQUIP_ROWS],
    /// Part whose [-] release is waiting for its second press.
    ///
    /// A release is never one press: the first only names what would go and
    /// any other key drops it, the rule the equipment list and the set editor
    /// already use.
    pub clear_armed: Option<usize>,
}

/// One decoded row of the palico equipment panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PalicoEquipRow {
    /// 0 weapon, 1 head, 2 body.
    pub part: usize,
    /// Box index the palico wears now, `PALICO_EQUIP_NONE` when nothing.
    pub worn: u16,
    /// Equip id of the worn record, 0 when nothing is worn.
    pub worn_id: u16,
    /// Box index [A] would write, `PALICO_EQUIP_NONE` when nothing is picked.
    pub pick: u16,
    /// Equip id of the picked record, 0 when nothing is picked.
    pub pick_id: u16,
}

impl PalicoEquipRow {
    /// "枠 7 / ID 431" for the record the palico wears now, "なし" for nothing.
    pub fn worn_text(&self) -> String {
        if self.worn == PALICO_EQUIP_NONE {
            String::from("なし")
        } else {
            format!("枠 {} / ID {}", self.worn, self.worn_id)
        }
    }

    /// The same shape for the record [A] would write.
    pub fn pick_text(&self) -> String {
        if self.pick == PALICO_EQUIP_NONE {
            String::from("候補なし")
        } else {
            format!("枠 {} / ID {}", self.pick, self.pick_id)
        }
    }
}

/// Name of one of the three slots a palico wears things in.
pub fn palico_part_name(part: usize) -> &'static str {
    PALICO_PART_NAMES.get(part).copied().unwrap_or("?")
}

/// Write one worn-list index into one roster record.
///
/// `palico::EQUIP` is the documented offset (+0x100) of the three u16 worn
/// list and `part` picks which of them, so only the two bytes of that part
/// move. The target range is bounded by the body's own length, so a bad slot
/// or a short body writes nothing instead of panicking.
fn write_palico_equip(save: &mut Save, slot: usize, palico: usize, part: usize, index: u16) -> bool {
    if palico >= crate::palico::SLOTS || part >= crate::palico::EQUIP_SLOTS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base
        + crate::palico::OFF_ROSTER
        + palico * crate::palico::RECORD
        + crate::palico::EQUIP
        + part * 2;
    let Some(dst) = save.body_mut().get_mut(at..at + 2) else {
        return false;
    };
    dst.copy_from_slice(&index.to_le_bytes());
    true
}

/// Rows on the palico skill panel's skill tab: the twelve learned skills a
/// record carries (palico::LEARNED_SKILL_SLOTS).
pub const PALICO_SKILL_ROWS: usize = crate::palico::LEARNED_SKILL_SLOTS;

/// Rows on its support move tab: the sixteen learned moves.
pub const PALICO_MOVE_ROWS: usize = crate::palico::LEARNED_MOVE_SLOTS;

/// The skill tab of the palico skill panel.
pub const PALICO_TAB_SKILLS: usize = 0;

/// The support move tab of the palico skill panel.
pub const PALICO_TAB_MOVES: usize = 1;

/// The id [A] starts a row on when its slot holds nothing.
///
/// Zero is "no skill" and the 57 / 96 fill is "not in the list", so neither can
/// be written back: id 1 is the first entry both name tables have.
pub const PALICO_PICK_FIRST: u8 = 1;

/// Name of one of the panel's two lists.
pub fn palico_skill_tab_name(tab: usize) -> &'static str {
    if tab == PALICO_TAB_MOVES {
        "サポート行動"
    } else {
        "スキル"
    }
}

/// Rows one tab shows: twelve skills, sixteen support moves.
pub fn palico_skill_tab_rows(tab: usize) -> usize {
    if tab == PALICO_TAB_MOVES {
        PALICO_MOVE_ROWS
    } else {
        PALICO_SKILL_ROWS
    }
}

/// Name of one id in one of the two learned lists.
pub fn palico_skill_id_name(tab: usize, id: u8) -> String {
    if tab == PALICO_TAB_MOVES {
        crate::palico::move_name(id)
    } else {
        crate::palico::skill_name(id)
    }
}

/// Highest id one of the two learned lists takes.
pub fn palico_skill_max_id(tab: usize) -> u8 {
    if tab == PALICO_TAB_MOVES {
        crate::palico::MAX_MOVE
    } else {
        crate::palico::MAX_SKILL
    }
}

/// The palico skill / support move panel, open as a mode of the roster screen.
///
/// The same shape [PalicoEquip] has: a **mode**, not a variant of `Screen`, so
/// the roster underneath keeps its own row and cursor, and nothing has to be
/// taught about a new screen. The tab picks which of the two learned lists is
/// on screen, the field the slot in it, and [PalicoSkill::picks] the id [A]
/// would write into each slot with palico::set_skill / palico::set_move.
/// Walking the rows and moving a candidate never writes: only [A] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PalicoSkill {
    /// Roster record index (0..palico::SLOTS) being edited.
    pub palico: usize,
    /// 0 = learned skills, 1 = learned support moves.
    pub tab: usize,
    /// Slot under the cursor within that list.
    pub field: usize,
    /// Id [A] would write, per slot. Both tabs share the buffer because only
    /// one of them is on screen at a time and switching tabs re-seeds it.
    pub picks: [u8; PALICO_MOVE_ROWS],
}

impl PalicoSkill {
    /// The id [A] would write into the slot under the cursor.
    pub fn pick(&self) -> u8 {
        self.picks
            .get(self.field)
            .copied()
            .unwrap_or(PALICO_PICK_FIRST)
    }
}

/// One slot of a learned list, decoded for the panel.
///
/// The counted flag is the record's own length byte: the game holds
/// [ids in use][taught but empty][57 / 96 fill] and only writes inside the
/// first two, which is the range palico::set_skill / palico::set_move accept.
/// The fill is listed rather than hidden so the panel shows the list the
/// record really carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PalicoSkillRow {
    /// Slot index, 0-based.
    pub index: usize,
    /// Id stored in the slot; 0, 57 or 96 when it holds nothing.
    pub current: u8,
    /// Id [A] would write here.
    pub pick: u8,
    /// Does the record's length byte count this slot?
    pub counted: bool,
    /// Name of the stored id, "-" when the slot holds nothing.
    pub current_name: String,
    /// Name of the candidate, "-" when it holds nothing.
    pub pick_name: String,
}

impl PalicoSkillRow {
    /// Does the slot hold nothing the panel can name?
    pub fn is_empty(&self) -> bool {
        self.current_name == "-"
    }

    /// "ID 5 / Health Up (S)" for the slot, or "空き" for nothing.
    pub fn current_text(&self) -> String {
        if self.is_empty() {
            String::from("空き")
        } else {
            format!("ID {} / {}", self.current, self.current_name)
        }
    }

    /// The same shape for the candidate.
    pub fn pick_text(&self) -> String {
        if self.pick_name == "-" {
            String::from("空き")
        } else {
            format!("ID {} / {}", self.pick, self.pick_name)
        }
    }
}

/// The candidate every row of a learned list starts on.
///
/// A row starts on the id it already holds, so [A] on an untouched row writes
/// that same id back and changes nothing; an empty or fill slot starts on the
/// first id, which is where a short walk with [◀/▶] begins.
fn palico_skill_picks(p: &crate::palico::Palico, tab: usize) -> [u8; PALICO_MOVE_ROWS] {
    let (list, empty) = if tab == PALICO_TAB_MOVES {
        (&p.learned_moves, crate::palico::MOVE_EMPTY)
    } else {
        (&p.learned_skills, crate::palico::SKILL_EMPTY)
    };
    let mut picks = [PALICO_PICK_FIRST; PALICO_MOVE_ROWS];
    for (k, v) in list.iter().take(PALICO_MOVE_ROWS).enumerate() {
        if *v != 0 && *v != empty {
            picks[k] = *v;
        }
    }
    picks
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

/// Rows on the item set form: one per entry the set holds.
pub const ITEM_SET_EDIT_ROWS: usize = crate::myset::ITEM_SET_ITEMS;

/// The item set being edited on the my-sets screen.
///
/// The twin of [SetEdit] for the other array the screen lists: the record
/// index, the row the cursor is on and the pending entry of each of the
/// thirty-two rows. Like SetEdit it is a **mode of the my-sets screen**, not a
/// screen of its own, and the pending entries are kept apart from the record so
/// that walking the rows writes nothing: only [A] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSetEdit {
    /// Index into the twenty-four record array (0..ITEM_SETS).
    pub set: usize,
    /// Which entry row the cursor is on (0..ITEM_SET_EDIT_ROWS).
    pub field: usize,
    /// The entry [A] would write, per row. Id 0 is an empty entry.
    pub pick: [crate::myset::SetItem; ITEM_SET_EDIT_ROWS],
}

impl Default for ItemSetEdit {
    fn default() -> Self {
        ItemSetEdit {
            set: 0,
            field: 0,
            pick: [crate::myset::SetItem { id: 0, count: 0 }; ITEM_SET_EDIT_ROWS],
        }
    }
}

/// A two-press operation on the item set form.
///
/// Both of them replace thirty-two entries at once - the set, or the pouch -
/// so neither is one press: the first press only says what would be lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSetOp {
    /// myset::copy_from_pouch: the pouch replaces the set.
    FromPouch,
    /// myset::apply_to_pouch: the set replaces the pouch.
    ToPouch,
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
/// Sentinel id for "ask for an arena time as `m:ss.cc`".
///
/// Below `TEXT_PALICO_NAME` for the reason TEXT_SET_NAME is: the console layer
/// reads one contiguous range as palico names, so a sentinel above it would be
/// answered as a palico that does not exist.
pub const TEXT_ARENA_TIME: usize = usize::MAX - 1_002;

// ---------------------------------------------------------------------------
// 闘技大会 (the arena log): the guild screen's second view
// ---------------------------------------------------------------------------
//
// The log is part of the guild card (guildcard::OFF_ARENA, 17 arenas of five
// ranked entries, four bytes each) and the card is one screen, so the arena
// log is a *view* of the guild screen and not a screen of its own: the sub-tab
// bar names the two views and L / R walk them, exactly the way the progress
// screen carries its three views. No offset is invented here - every row is an
// entry guildcard::arena already reads and guildcard::set_arena_time already
// writes, and the panel the PC build has on the ギルド tab is the same one.

/// View 0 of the guild screen: the card itself (rank, clock, comment).
pub const GUILD_TAB_CARD: usize = 0;
/// View 1 of the guild screen: the arena log.
pub const GUILD_TAB_ARENA: usize = 1;
/// Views the guild screen has.
pub const GUILD_TABS: usize = 2;

/// Arena rows the view lists: one per arena of the log.
pub const ARENA_ROWS: usize = crate::guildcard::ARENA_COUNT;
/// Rank columns the view lists: the five the card keeps, best first.
pub const ARENA_COLUMNS: usize = crate::guildcard::ARENA_RANKS;
/// Parts of one time, in the order Left / Right walks them.
pub const ARENA_PARTS: usize = 3;
/// The minutes part.
pub const ARENA_PART_MIN: usize = 0;
/// The seconds part.
pub const ARENA_PART_SEC: usize = 1;
/// The hundredths part.
pub const ARENA_PART_CS: usize = 2;

/// Names of the arena log's rows.
///
/// The log keeps seventeen arenas and MHXX's 闘技大会 has exactly seventeen
/// arena quests, so the row is named after the quest. The list is the one the
/// game's own quest list shows (ホロロホルル討伐 first, ババコンガ討伐 last);
/// the slot-to-quest order is **not** verified on hardware yet, which is why
/// every row also prints its number. `""` for a row past the end.
pub const ARENA_NAMES: [&str; ARENA_ROWS] = [
    "ホロロホルル",
    "フルフル",
    "ガララアジャラ",
    "リオレウス",
    "ケチャワチャ",
    "ガノトトス",
    "ブラキディオス",
    "ベリオロス",
    "イャンガルルガ",
    "ウラガンキン",
    "セルレギオス",
    "ドスマッカォ",
    "ゲリョス",
    "鳥竜種2種",
    "ボルボロス",
    "テツカブラ",
    "ババコンガ",
];

/// Quest name of one arena row, or "" past the end.
pub fn arena_name(arena: usize) -> &'static str {
    match ARENA_NAMES.get(arena) {
        Some(s) => s,
        None => "",
    }
}

/// Name of one part of a time.
pub fn arena_part_name(part: usize) -> &'static str {
    match part {
        ARENA_PART_MIN => "分",
        ARENA_PART_SEC => "秒",
        _ => "1/100",
    }
}

/// How a rank column is written on screen: 1位 .. 5位.
pub fn arena_rank_name(rank: usize) -> &'static str {
    match rank {
        0 => "1位",
        1 => "2位",
        2 => "3位",
        3 => "4位",
        4 => "5位",
        _ => "?",
    }
}

/// An arena time the way the game shows it: `m:ss.cc`.
pub fn arena_clock(centis: u16) -> alloc::string::String {
    let total = centis as u32;
    alloc::format!(
        "{}:{:02}.{:02}",
        total / 6000,
        (total / 100) % 60,
        total % 100
    )
}

/// Read a time typed into the arena editor.
///
/// `9:35.44` is minutes, seconds and hundredths; `35.44` is seconds and
/// hundredths; a bare number is the raw count of hundredths the save holds, so
/// a value copied out of another tool round-trips unchanged. `0` is the empty
/// entry, which is also how the game erases a record. Anything that does not
/// parse returns None, which leaves the entry as it was.
pub fn arena_clock_value(text: &str) -> Option<u16> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let (has_colon, minutes, rest) = match t.split_once(':') {
        Some((m, r)) => (true, m.trim().parse::<u64>().ok()?, r.trim()),
        None => (false, 0, t),
    };
    if !has_colon && !rest.contains('.') {
        return rest.parse::<u64>().ok().map(|v| v.min(u16::MAX as u64) as u16);
    }
    let (secs, frac) = match rest.split_once('.') {
        Some((s, f)) => (s.trim(), Some(f.trim())),
        None => (rest, None),
    };
    let secs: u64 = secs.parse().ok()?;
    let frac: u64 = match frac {
        None => 0,
        Some("") => return None,
        Some(f) => {
            // .4 is 40 hundredths and .456 is 45: the field has two digits.
            let mut f = String::from(f);
            f.truncate(2);
            while f.len() < 2 {
                f.push('0');
            }
            f.parse().ok()?
        }
    };
    let total = minutes * 6000 + secs * 100 + frac;
    Some(total.min(u16::MAX as u64) as u16)
}

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

/// The same padding, on the left: numbers line up under a right-aligned column.
///
/// The arena grid prints one column of times per rank, and a time changes width
/// with its minutes ("9:35.44" is seven cells, "10:55.35" eight), so the column
/// is padded, not counted on: every cell is exactly `cells` wide, which is what
/// keeps the five columns from walking into each other on the console.
fn pad_cells_left(s: &str, cells: usize) -> alloc::string::String {
    // Measure first: the padding is what the text does not fill, and a wide
    // character that would cross the edge is dropped rather than half drawn.
    let mut used = 0usize;
    for ch in s.chars() {
        let w = char_cells(ch);
        if used + w > cells {
            break;
        }
        used += w;
    }
    let mut out = alloc::string::String::new();
    for _ in used..cells {
        out.push(' ');
    }
    let mut room = cells - used;
    for ch in s.chars() {
        let w = char_cells(ch);
        if w > room {
            break;
        }
        out.push(ch);
        room -= w;
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

/// Where the CSV files live on the SD card.
///
/// One folder for everything the tool trades with a spreadsheet, beside the
/// saves and the quest arcs. The I/O layer creates it and lists it; a CSV is
/// never read from anywhere else, so a user can always find what was written.
pub const CSV_DIR: &str = "/switch/XXSE/csv";
/// File name stem of the item box export: items-<stamp>.csv.
pub const CSV_ITEMS_STEM: &str = "items";
/// File name stem of the equipment box export: equipment-<stamp>.csv.
pub const CSV_EQUIP_STEM: &str = "equipment";
/// The byte order mark a spreadsheet may put in front of the header.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// What a file in the CSV folder holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvKind {
    /// The item box: what crate::csvout::items_to_csv writes.
    Items,
    /// The equipment box: what crate::csvout::equipment_to_csv writes.
    Equipment,
}

impl CsvKind {
    /// The word the interface uses for it.
    pub fn label(self) -> &'static str {
        match self {
            CsvKind::Items => "アイテム",
            CsvKind::Equipment => "装備",
        }
    }
}

/// The kind a file name claims, or None when the name says nothing.
///
/// The exports are named from a stem, so the list can say what a file holds
/// before anything is read. A user who renamed one is not lost: the first line
/// of the file decides when it is applied, and this is only the fallback.
fn csv_kind_of_name(path: &str) -> Option<CsvKind> {
    let name = file_name_of(path).to_ascii_lowercase();
    if name.starts_with(CSV_ITEMS_STEM) {
        Some(CsvKind::Items)
    } else if name.starts_with(CSV_EQUIP_STEM) {
        Some(CsvKind::Equipment)
    } else {
        None
    }
}

/// The kind the first line of a file claims, or None.
///
/// The header is the only thing in a file that says which table it is, and
/// crate::csvout writes "slot,..." for the item box and "index,..." for the
/// equipment box. A byte order mark is skipped: a spreadsheet on Windows adds
/// one, and it would otherwise hide the first cell.
fn csv_kind_of_text(text: &str) -> Option<CsvKind> {
    let first = text.trim_start_matches(BYTE_ORDER_MARK).lines().next()?;
    let cell = first.split(',').next()?.trim().trim_matches('"');
    if cell.eq_ignore_ascii_case(crate::csvout::EQUIP_COLUMNS[0]) {
        Some(CsvKind::Equipment)
    } else if cell.eq_ignore_ascii_case(crate::csvout::ITEM_COLUMNS[0]) {
        Some(CsvKind::Items)
    } else {
        None
    }
}

/// "アイテム" / "装備" / "?" for a file in the folder.
pub fn csv_kind_label(path: &str) -> &'static str {
    match csv_kind_of_name(path) {
        Some(k) => k.label(),
        None => "?",
    }
}

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
/// An ApplyReport as the lines the panel shows.
///
/// The counts come first because they are the answer to "did it work"; the
/// per-row messages follow, five at most, each clipped to the panel's width.
fn csv_report_lines(report: &crate::csvout::ApplyReport) -> Vec<String> {
    let mut out = Vec::with_capacity(6);
    out.push(alloc::format!(
        "適用 {} 行 / スキップ {} 行 / 丸め {} 行",
        report.applied,
        report.skipped,
        report.clamped
    ));
    for m in report.messages.iter().take(5) {
        out.push(clip_text(m, 60));
    }
    out
}

fn box_total(items: &[ItemSlot]) -> u32 {
    items
        .iter()
        .filter(|s| !s.is_blank())
        .map(|s| s.count as u32)
        .sum()
}

// ---------------------------------------------------------------------------
// Whole save undo
//
// The item box has had its own sixteen step undo since the beginning, but that
// is one box. A step here is the parsed save itself, taken before an edit
// writes it, so every screen that writes the save can take its edit back the
// same way. The depth is four because memory is the thing this console does not
// have: one step is the 4,726,152 byte body plus the 36 byte Switch header
// (4,726,188 bytes, printed as 4.5MB), so four steps cost 18.0MB on top of the
// save that is open.
// ---------------------------------------------------------------------------

/// How many whole-save undo steps the editor keeps.
pub const UNDO_STEPS: usize = 4;

/// Bytes one undo step costs: the Switch header plus the body.
pub const UNDO_STEP_BYTES: usize = crate::save::SWITCH_SIZE;

/// Bytes as the screen prints them, in tenths of a MiB and without floats.
///
/// The arithmetic is integer on purpose: the console build has no libm, and a
/// number the user can check against save::SWITCH_SIZE is worth more than a
/// pretty one. 4,726,188 bytes prints as "4.5MB".
pub fn bytes_mb_text(bytes: usize) -> String {
    let tenths = (bytes as u64 * 10 + (1 << 19)) / (1 << 20);
    format!("{}.{}MB", tenths / 10, tenths % 10)
}

/// What one undo step costs, e.g. "4.5MB".
pub fn undo_step_text() -> String {
    bytes_mb_text(UNDO_STEP_BYTES)
}

/// What the whole stack can cost, e.g. "18.0MB".
pub fn undo_total_text() -> String {
    bytes_mb_text(UNDO_STEP_BYTES * UNDO_STEPS)
}

/// One step of the whole-save undo: the save as it was before an edit.
pub struct SaveUndo {
    /// What the edit was called, for the status line.
    pub label: String,
    /// The slot that was open, put back with the bytes.
    pub slot: usize,
    /// The save before the edit.
    pub save: Save,
}

// ---------------------------------------------------------------------------
// The save layout, for naming what a write changes
//
// Every boundary below is an existing constant from mhnx-core, so the names
// cannot drift away from the offsets the writers use. This table names the
// parts a writer knows (the box, the record, the card); the 183 rows of
// [crate::savemap] name the bytes inside them, and the review prefers whichever
// is narrower, so a change the table cannot name is printed with the map's own
// row instead of "その他". A row neither table covers is still "その他".
//
// The save map is plain table data (`&'static str` names, no once-cell), so it
// is compiled into both builds and the console names records exactly like the
// PC one. This table stays as the fallback for bytes the map does not cover.
// ---------------------------------------------------------------------------

/// One named part of the save.
struct Layout {
    /// Offset from the slot base (slot parts) or from the body start.
    start: usize,
    /// End offset, same base as start.
    end: usize,
    /// What the part holds.
    name: &'static str,
    /// Bytes per record when the part is an array, 0 when it is not.
    unit: usize,
    /// What one record is called, e.g. "枠".
    unit_name: &'static str,
}

/// The parts of one character slot, relative to its base.
const SLOT_LAYOUT: &[Layout] = &[
    Layout { start: 0x0000, end: 0x0080, name: "キャラクター情報", unit: 0, unit_name: "" },
    Layout {
        start: crate::save::OFF_ITEM_BOX,
        end: crate::myset::OFF_ITEM_SETS,
        name: "アイテムボックス",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::myset::OFF_ITEM_SETS,
        end: crate::myset::OFF_POUCH,
        name: "アイテムマイセット",
        unit: crate::myset::ITEM_SET_SIZE,
        unit_name: "セット",
    },
    Layout {
        start: crate::myset::OFF_POUCH,
        end: crate::save::OFF_HR_POINTS,
        name: "ポーチ",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::save::OFF_HR_POINTS,
        end: crate::questprogress::OFF_CLEARED,
        name: "所持金・HR ポイント",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::questprogress::OFF_CLEARED,
        end: crate::awards::MAP_OFF,
        name: "クエスト進行状況",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::awards::MAP_OFF,
        end: crate::awards::MAP_OFF + crate::awards::MAP_LEN,
        name: "勲章（解禁マップ）",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::awards::NOTICE_OFF,
        end: crate::awards::NOTICE_OFF + crate::awards::NOTICE_LEN,
        name: "勲章（通知）",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::save::OFF_MONSTER_HUNT,
        end: crate::save::OFF_MONSTER_SIZE + crate::save::MONSTER_SIZE_TABLE_SIZE,
        name: "狩猟記録",
        unit: 2,
        unit_name: "匹",
    },
    Layout {
        start: crate::save::OFF_EQUIPMENT_BOX,
        end: crate::save::OFF_EQUIPMENT_BOX + crate::save::EQUIPMENT_BOX_SIZE,
        name: "装備ボックス",
        unit: crate::save::EQUIPMENT_RECORD,
        unit_name: "枠",
    },
    Layout {
        start: crate::palicogear::PALICO_EQUIP_OFFSET,
        end: crate::palicogear::PALICO_EQUIP_OFFSET + crate::save::PALICO_EQUIPMENT_SIZE,
        name: "オトモ装備ボックス",
        unit: crate::save::EQUIPMENT_RECORD,
        unit_name: "枠",
    },
    Layout {
        start: crate::myset::OFF_EQUIP_SETS,
        end: crate::myset::OFF_EQUIP_SETS
            + crate::myset::EQUIP_SETS * crate::myset::EQUIP_SET_SIZE,
        name: "装備マイセット",
        unit: crate::myset::EQUIP_SET_SIZE,
        unit_name: "セット",
    },
    Layout {
        start: crate::save::OFF_NAME,
        end: crate::save::OFF_NAME + 32,
        name: "名前",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::palico::OFF_ROSTER,
        end: crate::palico::OFF_ROSTER + crate::palico::SLOTS * crate::palico::RECORD,
        name: "オトモ",
        unit: crate::palico::RECORD,
        unit_name: "匹",
    },
    Layout {
        start: crate::guildcard::OFF_GUILD_CARD,
        end: crate::guildcard::OFF_GUILD_CARD + crate::guildcard::GC_SIZE,
        name: "ギルドカード",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::awards::CARD_OFF,
        end: crate::awards::CARD_OFF + crate::awards::CARD_LEN,
        name: "勲章（カード側）",
        unit: 0,
        unit_name: "",
    },
    Layout {
        start: crate::guildcard::OFF_ARENA,
        end: crate::guildcard::OFF_ARENA
            + crate::guildcard::ARENA_COUNT
                * crate::guildcard::ARENA_RANKS
                * crate::guildcard::ARENA_ENTRY,
        name: "闘技大会の記録",
        unit: crate::guildcard::ARENA_ENTRY,
        unit_name: "件",
    },
    Layout {
        start: crate::guildcard::GC_CARDS_INFO,
        end: crate::guildcard::GC_INBOX,
        name: "受信カード一覧",
        unit: crate::guildcard::CARD_INFO_LEN,
        unit_name: "枚",
    },
    Layout {
        start: crate::guildcard::GC_INBOX_INFO,
        end: crate::guildcard::GC_INBOX_INFO
            + crate::guildcard::INBOX_LIST_LEN * crate::guildcard::INBOX_INFO_LEN,
        name: "受信箱の情報",
        unit: crate::guildcard::INBOX_INFO_LEN,
        unit_name: "枚",
    },
    Layout {
        start: crate::save::OFF_MANUAL_SHOUTOUTS,
        end: crate::save::OFF_AUTO_SHOUTOUTS,
        name: "定型文（手動）",
        unit: crate::save::SHOUTOUT_ENTRY,
        unit_name: "件",
    },
    Layout {
        start: crate::save::OFF_AUTO_SHOUTOUTS,
        end: crate::save::OFF_AUTO_SHOUTOUTS + crate::save::AUTO_SHOUTOUT_SIZE,
        name: "定型文（自動）",
        unit: crate::save::SHOUTOUT_ENTRY,
        unit_name: "件",
    },
];

/// The parts that sit in the body itself rather than in a character slot.
const BODY_LAYOUT: &[Layout] = &[
    Layout { start: 0x00, end: 0x04, name: "本体ヘッダ", unit: 0, unit_name: "" },
    Layout { start: 0x04, end: 0x07, name: "スロット使用フラグ", unit: 0, unit_name: "" },
    Layout { start: 0x0C, end: 0x10, name: "DLC セクションの位置", unit: 0, unit_name: "" },
    Layout { start: 0x10, end: 0x1C, name: "スロット基点", unit: 0, unit_name: "" },
    Layout {
        start: crate::save::OFF_BRIGHTNESS,
        end: crate::save::OFF_DLC_PALICO + 1,
        name: "本体オプションと DLC オトモ枠",
        unit: 0,
        unit_name: "",
    },
];

/// The download section, whose offset the save itself carries.
///
/// It holds the download palicoes, the quest payloads and the 45 quest-equipment
/// loadouts, so it is named for the section rather than for the quests alone:
/// a write that only touches a loadout is not a quest change.
const DLC_SECTION: Layout = Layout {
    start: 0,
    end: crate::dlc::EQUIP_ARRAY_OFFSET + crate::dlc::EQUIP_SLOTS * crate::dlc::EQUIP_SLOT_SIZE,
    name: "DLC セクション",
    unit: 0,
    unit_name: "",
};

/// One stretch of bytes a write changes, named by what lives there.
///
/// Two names travel with a stretch. [DiffRange::part] is the layout part, and
/// it is what [crate::diffvalues] gates its getters on, so a finer name cannot
/// change which values the review walks. [DiffRange::name] is what the screens
/// print, and it comes from the save map when the map has a row for these bytes:
/// "装備ボックス #24" instead of "装備ボックス +0x33C".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRange {
    /// Character slot this belongs to, None for the body-wide parts.
    pub slot: Option<usize>,
    /// The layout part these bytes belong to, e.g. "装備ボックス"; "その他"
    /// when the layout has no entry for them.
    pub part: &'static str,
    /// What the screens print, e.g. "装備ボックス #24": the save map's name for
    /// the bytes, with the record the change starts at.
    pub name: String,
    /// Distance from the start of the stretch [DiffRange::name] names.
    pub rel: usize,
    /// How many bytes differ inside the stretch.
    pub len: usize,
    /// Records the stretch touches, when the part has a known record size.
    pub records: Option<(usize, &'static str)>,
}

impl DiffRange {
    /// "スロット1 装備ボックス #24" - the place, without the size.
    pub fn head(&self) -> String {
        match self.slot {
            Some(i) => format!("スロット{} {}", i + 1, self.name),
            None => self.name.clone(),
        }
    }

    /// The line the review prints for this stretch, e.g.
    /// "スロット1 装備ボックス 3 バイト（1 枠）".
    ///
    /// The console wording: its review has no room for "+0x… 付近", so the
    /// offset stays out. The window wording is [`DiffRange::window_line`].
    pub fn line(&self) -> String {
        let mut out = format!("{} {} バイト", self.head(), self.len);
        if let Some((n, what)) = self.records {
            out.push_str(&format!("（{} {}）", n, what));
        }
        out
    }

    /// The same line as the desktop dialog prints it, with where inside the
    /// part the change starts: "スロット1 装備ボックス +0x3C 付近 3 バイト（1 枠）".
    pub fn window_line(&self) -> String {
        let mut out = format!("{} +0x{:X} 付近 {} バイト", self.head(), self.rel, self.len);
        if let Some((n, what)) = self.records {
            out.push_str(&format!("（{} {}）", n, what));
        }
        out
    }
}

/// What one write would change: the named stretches and a feature summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveDiff {
    /// How many bytes differ from the file on disk.
    pub changed: usize,
    /// Feature level lines, e.g. "スロット1: 名前、装備ボックス 1 枠".
    pub summary: Vec<String>,
    /// Value level lines, e.g. "装備ボックス #12 レベル: 1 → 3".
    ///
    /// One entry per field, read through an existing getter by
    /// [crate::diffvalues]. Readable values only: a byte nobody can decode
    /// stays in `ranges`.
    pub values: Vec<ValueChange>,
    /// The stretches themselves, in file order.
    pub ranges: Vec<DiffRange>,
}

/// How many ranges the review names before it says "ほか N 件".
pub const REVIEW_RANGES: usize = 8;

/// Which wording a review body is written in.
///
/// The console review and the desktop dialog print the same three sections;
/// what differs is the range line ("+0x… 付近" only fits the window) and what
/// an empty summary does. Keeping both in [`SaveDiff::review_lines`] is what
/// stops the two screens from drifting apart again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewWording {
    /// The console's two-press review.
    Console,
    /// The desktop review dialog and the snapshot comparison.
    Window,
}

impl SaveDiff {
    /// Is there anything to review at all?
    pub fn is_empty(&self) -> bool {
        self.changed == 0
    }

    /// The review body the console draws: [`SaveDiff::review_lines`] in
    /// [`ReviewWording::Console`].
    pub fn lines(&self, shown: usize) -> Vec<String> {
        self.review_lines(shown, ReviewWording::Console)
    }

    /// The review body the desktop review dialog and the snapshot comparison
    /// draw: the same assembly, in [`ReviewWording::Window`].
    ///
    /// Both windows call this instead of building the sections themselves, so
    /// the order (内容 → 値 → 範囲), the caps and the "ほか N 件" wording exist
    /// once. Only the range line and the empty-summary case differ, and the
    /// wording enum says how.
    pub fn window_lines(&self, shown: usize) -> Vec<String> {
        self.review_lines(shown, ReviewWording::Window)
    }

    /// The review body: what changes, then what values, then where. shown caps
    /// the ranges and [crate::diffvalues::VALUE_SHOWN] the values, so a
    /// difference in hundreds of places cannot push the buttons off screen;
    /// what is left over is counted rather than dropped silently.
    ///
    /// This is the one place the three sections are assembled. The console
    /// ([SaveDiff::lines]) and the desktop windows ([SaveDiff::window_lines])
    /// both come through here, which is what keeps the two reviews from saying
    /// the same thing in two slightly different ways.
    pub fn review_lines(&self, shown: usize, wording: ReviewWording) -> Vec<String> {
        let window = wording == ReviewWording::Window;
        let mut out: Vec<String> = Vec::new();
        // A window over a file that did not parse has no summary line at all,
        // and leaves the section out; the console keeps the heading and puts
        // the byte count under it.
        if window {
            if !self.summary.is_empty() {
                out.push(String::new());
                out.push(String::from("■ 変更される内容"));
                for line in &self.summary {
                    out.push(format!("・{}", line));
                }
            }
        } else {
            out.push(String::from("■ 変更される内容"));
            if self.summary.is_empty() {
                out.push(format!("・変更 {} バイト", self.changed));
            }
            for line in &self.summary {
                out.push(format!("・{}", line));
            }
        }
        if !self.values.is_empty() {
            out.push(String::new());
            out.push(String::from("■ 変更される値（旧 → 新）"));
            for value in self.values.iter().take(crate::diffvalues::VALUE_SHOWN) {
                out.push(format!("・{}", value.line()));
            }
            if self.values.len() > crate::diffvalues::VALUE_SHOWN {
                out.push(format!(
                    "・ほか {} 件（値は先頭 {} 件まで。残りは下の範囲に残ります）",
                    self.values.len() - crate::diffvalues::VALUE_SHOWN,
                    crate::diffvalues::VALUE_SHOWN
                ));
            }
        }
        out.push(String::new());
        out.push(String::from("■ 変更される範囲"));
        if self.ranges.is_empty() {
            out.push(String::from("・特定できる違いはありません"));
        }
        for range in self.ranges.iter().take(shown) {
            out.push(format!(
                "・{}",
                match wording {
                    ReviewWording::Console => range.line(),
                    ReviewWording::Window => range.window_line(),
                }
            ));
        }
        if self.ranges.len() > shown {
            out.push(format!("・ほか {} 件", self.ranges.len() - shown));
        }
        out
    }
}

/// The layout entry that contains body_off, preferring the narrowest.
///
/// Overlapping entries are resolved by span: the arena log and the award map
/// both sit inside the guild card block, and naming a change after the
/// narrowest piece is what tells the user which one moved.
fn layout_at(save: &Save, body_off: usize) -> Option<(Option<usize>, &'static Layout, usize)> {
    fn consider(
        best: &mut Option<(usize, Option<usize>, &'static Layout, usize)>,
        body_off: usize,
        slot: Option<usize>,
        layout: &'static Layout,
        start: usize,
    ) {
        let end = start + (layout.end - layout.start);
        if body_off >= start && body_off < end {
            let span = end - start;
            if best.map(|(s, _, _, _)| span < s).unwrap_or(true) {
                *best = Some((span, slot, layout, start));
            }
        }
    }
    let mut best: Option<(usize, Option<usize>, &'static Layout, usize)> = None;
    for layout in BODY_LAYOUT {
        consider(&mut best, body_off, None, layout, layout.start);
    }
    for slot in 0..crate::save::MAX_SLOTS {
        let Some(base) = save.slot_base(slot) else {
            continue;
        };
        for layout in SLOT_LAYOUT {
            consider(&mut best, body_off, Some(slot), layout, base as usize + layout.start);
        }
    }
    let dlc = save.dlc_section_offset() as usize;
    let dlc_end = dlc + DLC_SECTION.end;
    if dlc > 0 && dlc_end <= save.body().len() {
        consider(&mut best, body_off, None, &DLC_SECTION, dlc);
    }
    best.map(|(_, slot, layout, start)| (slot, layout, start))
}

/// One row of [crate::savemap], resolved against a particular save.
///
/// The map's rows hold the canonical MHXX offset; this is the offset the file
/// in hand actually has, read the same way [crate::savemap::offset_in] reads it
/// (so an imported character or a save with a moved download section is named
/// where its own pointers say).
struct MapRow {
    /// Character slot the row lives in, None for the body-wide rows.
    slot: Option<usize>,
    /// Body offset the row starts at in this save.
    start: usize,
    /// How many bytes it covers.
    len: usize,
    /// What the map calls it, e.g. "装備ボックス".
    name: &'static str,
    /// Bytes per record when the row is an array, 0 when it is not.
    record: usize,
}

impl MapRow {
    /// The row for these bytes, with #N when the row is an array.
    ///
    /// "#24" is the record the change starts in, counted from one the way the
    /// value lines count ("装備ボックス #24 レベル"): a reader looks the same
    /// number up in the box, so the two screens have to agree on it.
    fn name(&self, rel: usize, len: usize) -> String {
        let mut out = String::from(self.name);
        if self.record > 0 && len > 0 {
            let first = rel / self.record + 1;
            let last = (rel + len - 1) / self.record + 1;
            if first == last {
                out.push_str(&format!(" #{}", first));
            } else {
                out.push_str(&format!(" #{}〜#{}", first, last));
            }
        }
        out
    }
}

/// The save map row that covers `body_off`, narrowest first.
///
/// Which row that is when two of them cover the same bytes is decided by the
/// value, in [crate::savemap::row_at]: 装備ボックス and 護石 both span the
/// equipment box's 72,000 bytes, and the record's type byte (`charm`) says
/// which one a change belongs to. Everything here reads the row it hands back.
///
/// The map is plain data on both builds (its names are `&'static str`), so the
/// console review names records the same way the PC one does. There is no
/// `std` gate left in this path: an offset-only fallback on hardware was the
/// one place the two reviews disagreed.
fn map_row_at(save: &Save, body_off: usize) -> Option<MapRow> {
    use crate::savemap::{self, FieldKind};
    let index = savemap::row_at(save, body_off)?;
    let field = savemap::fields().get(index)?;
    Some(MapRow {
        slot: field.slot,
        start: savemap::offset_in(save, index)?,
        len: field.len,
        name: field.name,
        record: match field.kind {
            FieldKind::Record { size, .. } => size,
            _ => 0,
        },
    })
}

/// Where a body offset sits, as the review names it.
struct Region {
    /// Character slot the bytes belong to.
    slot: Option<usize>,
    /// The layout part, for the value walk's gates; "その他" when the layout
    /// has no entry for these bytes.
    part: &'static str,
    /// The save map row that names them, when the map has one.
    row: Option<MapRow>,
    /// The layout entry that covers them, when the layout has one.
    layout: Option<&'static Layout>,
    /// Body offset the printed name starts at.
    start: usize,
    /// Body offset it ends at.
    end: usize,
}

/// What names the bytes at `body_off`.
///
/// The save map is preferred: it is the table with 183 rows against this
/// module's 23, so it is the one that can say "装備ボックス #24" (and the one
/// that names the bytes between the layout's parts at all). The layout entry is
/// still fetched, because its name is the key the value walk gates on and the
/// fallback name when the map has no row.
///
/// The narrowest wins; on an equal span the map wins, because its rows are the
/// finer grain. `None` means neither table covers the byte, and the review
/// reports it as "その他".
fn region_at(save: &Save, body_off: usize) -> Option<Region> {
    let layout = layout_at(save, body_off);
    let part = layout.map(|(_, layout, _)| layout.name).unwrap_or("その他");
    let row = match (map_row_at(save, body_off), layout) {
        (Some(row), Some((_, layout, _))) => {
            // An equal span is a row that names the same bytes as the layout
            // entry: the map's row is the one that knows the record size.
            if row.len <= layout.end - layout.start {
                Some(row)
            } else {
                None
            }
        }
        (row, None) => row,
        (None, Some(_)) => None,
    };
    let (slot, start, end) = match (&row, layout) {
        (Some(row), _) => (row.slot, row.start, row.start + row.len),
        (None, Some((slot, layout, start))) => (slot, start, start + (layout.end - layout.start)),
        (None, None) => return None,
    };
    Some(Region {
        slot,
        part,
        row,
        layout: layout.map(|(_, layout, _)| layout),
        start,
        end,
    })
}

impl Region {
    /// What the screens call this stretch: the map's row and record when there
    /// is one, the layout's part otherwise.
    fn name(&self, rel: usize, len: usize) -> String {
        match &self.row {
            Some(row) => row.name(rel, len),
            None => String::from(match self.layout {
                Some(layout) => layout.name,
                None => self.part,
            }),
        }
    }
}

/// Where the named parts start, so an unnamed gap ends where the next begins.
fn region_starts(save: &Save) -> Vec<usize> {
    let mut starts: Vec<usize> = BODY_LAYOUT.iter().map(|l| l.start).collect();
    for slot in 0..crate::save::MAX_SLOTS {
        if let Some(base) = save.slot_base(slot) {
            starts.extend(SLOT_LAYOUT.iter().map(|l| base as usize + l.start));
        }
    }
    let dlc = save.dlc_section_offset() as usize;
    if dlc > 0 {
        starts.push(dlc);
    }
    // The map's rows start named stretches too: a gap between two parts ends at
    // whichever table names the byte that follows it.
    for index in 0..crate::savemap::fields().len() {
        if let Some(start) = crate::savemap::offset_in(save, index) {
            starts.push(start);
        }
    }
    starts
}

/// The differing stretches of two files, named with the save layout.
///
/// header_len is the Switch header the body does not contain. Differences
/// closer together than 32 bytes are reported as one stretch: a record that has
/// been rewritten is one line, not thirty.
pub fn diff_ranges(
    old: &[u8],
    new: &[u8],
    header_len: usize,
    save: &Save,
) -> (usize, Vec<DiffRange>) {
    const GAP: usize = 32;
    let common = old.len().min(new.len());
    let mut changed = 0usize;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    for i in 0..common {
        if old[i] == new[i] {
            continue;
        }
        changed += 1;
        open = match open {
            Some((s, e)) if i <= e + GAP => Some((s, i + 1)),
            Some((s, e)) => {
                runs.push((s, e));
                Some((i, i + 1))
            }
            None => Some((i, i + 1)),
        };
    }
    if let Some(run) = open {
        runs.push(run);
    }
    if old.len() != new.len() {
        let trailing = old.len().max(new.len());
        changed += trailing - common;
        match runs.last_mut() {
            Some((_, end)) if common <= *end + GAP => *end = trailing,
            _ => runs.push((common, trailing)),
        }
    }

    let mut out: Vec<DiffRange> = Vec::new();
    for (run_start, run_end) in runs {
        let mut at = run_start;
        while at < run_end {
            if at < header_len {
                let end = run_end.min(header_len);
                out.push(DiffRange {
                    slot: None,
                    part: "ファイルヘッダ",
                    name: String::from("ファイルヘッダ"),
                    rel: at,
                    len: end - at,
                    records: None,
                });
                at = end;
                continue;
            }
            let body = at - header_len;
            match region_at(save, body) {
                Some(region) => {
                    let end = region.end.min(body + (run_end - at));
                    let stop = (header_len + end).min(run_end);
                    let len = stop - at;
                    let rel = body.saturating_sub(region.start);
                    let records = match region.layout {
                        Some(layout) if layout.unit > 0 => {
                            Some(((len + layout.unit - 1) / layout.unit, layout.unit_name))
                        }
                        _ => None,
                    };
                    out.push(DiffRange {
                        slot: region.slot,
                        part: region.part,
                        name: region.name(rel, len),
                        rel,
                        len,
                        records,
                    });
                    at = stop;
                }
                None => {
                    let next = region_starts(save)
                        .into_iter()
                        .filter(|s| *s > body)
                        .min()
                        .unwrap_or(run_end - header_len);
                    let stop = (header_len + next).min(run_end);
                    out.push(DiffRange {
                        slot: None,
                        part: "その他",
                        name: String::from("その他"),
                        rel: body,
                        len: stop - at,
                        records: None,
                    });
                    at = stop;
                }
            }
        }
    }
    (changed, out)
}

/// Which character fields differ between the two saves.
fn character_fields_changed(old: &Save, new: &Save, slot: usize) -> Vec<&'static str> {
    let (Some(a), Some(b)) = (old.read_character(slot), new.read_character(slot)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if a.name != b.name || a.name_preview != b.name_preview {
        out.push("名前");
    }
    if a.play_time != b.play_time {
        out.push("プレイ時間");
    }
    if a.funds != b.funds {
        out.push("所持金");
    }
    if a.hunter_rank != b.hunter_rank {
        out.push("ハンターランク");
    }
    if a.hr_points != b.hr_points {
        out.push("HR ポイント");
    }
    if a.academy_points != b.academy_points {
        out.push("学園ポイント");
    }
    if a.village_points != b.village_points {
        out.push("村ポイント");
    }
    if a.voice != b.voice
        || a.eye_color != b.eye_color
        || a.clothing != b.clothing
        || a.gender != b.gender
        || a.hairstyle != b.hairstyle
        || a.face != b.face
        || a.features != b.features
        || a.armor_colors != b.armor_colors
        || a.skin_color != b.skin_color
        || a.hair_color != b.hair_color
        || a.features_color != b.features_color
        || a.clothing_color != b.clothing_color
    {
        out.push("外見");
    }
    out
}

/// How many download quests the write adds, removes or rewrites.
fn quests_changed(old: &Save, new: &Save) -> usize {
    let before = crate::dlc::list_quests(old);
    let after = crate::dlc::list_quests(new);
    let mut n = 0usize;
    for q in &after {
        match before.iter().find(|b| b.quest_id == q.quest_id) {
            Some(b) if b.data == q.data => {}
            _ => n += 1,
        }
    }
    n + before
        .iter()
        .filter(|b| !after.iter().any(|q| q.quest_id == b.quest_id))
        .count()
}

/// The feature level lines shown above the byte ranges.
///
/// One line per character slot naming what changed there, one for the body,
/// one for the download section: the example the audit asked for is
/// "スロット1: 装備ボックス 1 枠" and "DLC: クエスト 2 件".
pub fn diff_summary(ranges: &[DiffRange], old: &Save, new: &Save) -> Vec<String> {
    let mut per_slot: Vec<(usize, Vec<String>)> = Vec::new();
    let mut body: Vec<String> = Vec::new();
    let mut dlc: Vec<String> = Vec::new();
    for range in ranges {
        // The summary stays at the layout's grain ("装備ボックス 1 枠"): it is
        // the feature level line, and which record it was is what the range
        // section underneath already says. Bytes the layout cannot name keep
        // the save map's name, because that is the whole point of reading it.
        let part = match (range.slot, range.part) {
            (Some(slot), "キャラクター情報") => {
                let fields = character_fields_changed(old, new, slot);
                if fields.is_empty() {
                    String::from("キャラクター情報")
                } else {
                    fields.join("、")
                }
            }
            (Some(_), "その他") => range.name.clone(),
            (Some(_), part) => match range.records {
                Some((n, what)) => format!("{} {} {}", part, n, what),
                None => String::from(part),
            },
            (None, "DLC セクション") => {
                let n = quests_changed(old, new);
                if n > 0 {
                    format!("クエスト {} 件", n)
                } else {
                    String::from("クエスト")
                }
            }
            (None, "ファイルヘッダ") => String::from("ファイルヘッダ（名前のハッシュなど）"),
            (None, "その他") => range.name.clone(),
            (None, part) => String::from(part),
        };
        match range.slot {
            Some(slot) => match per_slot.iter_mut().find(|(s, _)| *s == slot) {
                Some((_, parts)) => {
                    if !parts.contains(&part) {
                        parts.push(part);
                    }
                }
                None => per_slot.push((slot, alloc::vec![part])),
            },
            None => {
                let list = if range.part == "DLC セクション" {
                    &mut dlc
                } else {
                    &mut body
                };
                if !list.contains(&part) {
                    list.push(part);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (slot, parts) in per_slot {
        out.push(format!("スロット{}: {}", slot + 1, parts.join("、")));
    }
    if !dlc.is_empty() {
        out.push(format!("DLC: {}", dlc.join("、")));
    }
    if !body.is_empty() {
        out.push(format!("本体: {}", body.join("、")));
    }
    out
}

/// Count the bytes two slices disagree on, without naming them.
fn count_changed(old: &[u8], new: &[u8]) -> usize {
    let common = old.len().min(new.len());
    let mut n = old.iter().take(common).zip(new.iter()).filter(|(a, b)| a != b).count();
    n += old.len().max(new.len()) - common;
    n
}

/// What one save write would change, as the review shows it.
///
/// Both sides are parsed when they parse, so the ranges can be named; when one
/// does not (a file that is not a save, a header that disagrees) the count of
/// differing bytes is still reported, under "セーブファイル".
pub fn diff_save(old: &[u8], new: &[u8]) -> SaveDiff {
    // The layout below is one format's; a 3DS body compared against a Switch
    // file would name every offset wrongly, so a kind change is reported as a
    // plain count instead of a guess.
    let parsed = match (Save::from_bytes(old), Save::from_bytes(new)) {
        (Ok(a), Ok(b)) if a.kind() == b.kind() => Some((a, b)),
        _ => None,
    };
    let Some((old_save, new_save)) = parsed else {
        let changed = count_changed(old, new);
        let mut diff = SaveDiff::default();
        if changed > 0 {
            diff.changed = changed;
            diff.summary = alloc::vec![format!("セーブ全体: {} バイト", changed)];
            diff.ranges = alloc::vec![DiffRange {
                slot: None,
                part: "セーブファイル",
                name: String::from("セーブファイル"),
                rel: 0,
                len: changed,
                records: None,
            }];
        }
        return diff;
    };
    let header_len = if new_save.kind().has_header() {
        crate::save::SWITCH_HEADER_SIZE
    } else {
        0
    };
    let (changed, ranges) = diff_ranges(old, new, header_len, &new_save);
    let (summary, values) = if changed > 0 {
        (
            diff_summary(&ranges, &old_save, &new_save),
            // Parsed once here, so the value walk costs the reads of the parts
            // the ranges named and no second copy of the save.
            diff_values(&ranges, &old_save, &new_save, None),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    SaveDiff {
        changed,
        summary,
        values,
        ranges,
    }
}

/// A save write waiting for the user to agree to what it changes.
///
/// The bytes themselves are not held here: Editor::confirm_save_review builds
/// them again from the save that is still open, so the review costs the name of
/// the changes and nothing else.
pub struct SaveReview {
    /// Lines ready to draw, headers included.
    pub lines: Vec<String>,
    /// The feature level lines, for tests and the size note.
    pub summary: Vec<String>,
    /// The value level lines, e.g. "スロット1 名前: ナナ → 差分テスト".
    ///
    /// The \`lines\` field already carries them as the "■ 変更される値" section;
    /// this is what the screen and the tests read them from.
    pub values: Vec<ValueChange>,
    /// The named stretches.
    pub ranges: Vec<DiffRange>,
    /// How many bytes differ from the file on disk.
    pub changed: usize,
    /// Size of what is on disk now.
    pub old_len: usize,
    /// Size of what the write would put there.
    pub new_len: usize,
    /// The file the bytes would land in.
    pub path: String,
}

impl SaveReview {
    /// The one line above the body: what changes and how big the file is.
    pub fn headline(&self) -> String {
        format!(
            "変更 {} バイト（{} バイト → {} バイト）",
            self.changed, self.old_len, self.new_len
        )
    }
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
    /// Which view of the guild screen is showing.
    ///
    /// GUILD_TAB_CARD is the card itself, GUILD_TAB_ARENA the arena log. A view,
    /// not a screen: both read the same card and the same slot.
    pub guild_tab: usize,
    /// Arena row under the cursor on the arena view (0..ARENA_ROWS).
    pub arena_row: usize,
    /// Rank column under the cursor (0 is 1位; 0..ARENA_COLUMNS).
    pub arena_rank: usize,
    /// Which part of the selected time the editor steps (分 / 秒 / 1/100).
    pub arena_part: usize,
    /// Is the time editor armed? [A] starts it, [A] and [B] end it.
    ///
    /// A mode, not a screen: while it is on, Left / Right walk the three parts
    /// and Up / Down change the one under the cursor. The rest of the view - the
    /// arena and the rank - stays put so the number being changed cannot move
    /// out from under the press.
    pub arena_edit: bool,
    /// Which view of the progress screen is showing.
    ///
    /// One of PROGRESS_AWARDS / PROGRESS_REQUESTS / PROGRESS_QUESTS. The three
    /// share the screen but not the cursor: switching away and back keeps the
    /// row the user was on, which is what a list of 1508 indices needs.
    pub progress_tab: usize,
    /// Award index under the cursor on the award view (0..AWARDS).
    pub award_cursor: usize,
    /// Row under the cursor on the request view, an index into the flag list
    /// the two request bands describe (0..380).
    pub request_cursor: usize,
    /// Row under the cursor on the quest view.
    ///
    /// A row, not an index: row 0 is quest progress index 1, because index 0
    /// lines up with the dummy quest and is never used.
    pub quest_cursor: usize,
    /// Award names from the user's own romfs, 132 entries, or empty.
    ///
    /// Game text, so this crate never ships it: a caller that has the dump
    /// hands the names over and the screen appends them, and a caller that
    /// has none (the console, which cannot read romfs) shows the number alone.
    pub romfs_awards: Vec<String>,
    /// Request names from the same dump, one per request record, or empty.
    pub romfs_requests: Vec<String>,
    /// The request table from the same dump, or empty.
    ///
    /// Carries the accept / complete flag numbers, which is what ties a flag
    /// in the map to a request in the table.
    pub romfs_records: Vec<RequestRecord>,
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
    /// The item set form, when it is open.
    ///
    /// The second mode of the my-sets screen. At most one of the two forms is
    /// open, so opening either closes the other.
    pub item_set_edit: Option<ItemSetEdit>,
    /// The two-press item set operation waiting for its second press.
    pub item_set_armed: Option<ItemSetOp>,
    /// What `myset::item_set_warnings` last said about the set on screen.
    ///
    /// Collected when the form opens and after every write, never while
    /// drawing: the check decodes the whole record.
    pub item_set_warnings: Vec<String>,
    /// Cursor inside the save's own quest list.
    pub save_quest_cursor: usize,
    /// Cursor inside the download Palico catalogue.
    pub palico_cursor: usize,
    /// 0 = Japanese, 1 = English.
    pub palico_lang: u8,
    /// The palico equipment panel, when it is open.
    ///
    /// A mode of the roster / download-palico screen, the same shape `set_edit`
    /// has on the my-sets screen: `None` means the screen shows its own list.
    pub palico_equip: Option<PalicoEquip>,
    /// The palico skill / support move panel, when it is open.
    ///
    /// The other mode of the roster screen, kept apart from the equipment one
    /// because the two edit different bytes of the same record: opening either
    /// closes the other, and both leave the list underneath alone.
    pub palico_skill: Option<PalicoSkill>,
    /// Cursor inside the options list.
    pub option_cursor: usize,
    /// CSV files offered by the I/O layer, in the folder's own order.
    ///
    /// Paths, not contents: a file is read only when the user applies it, and
    /// the console layer does the reading.
    pub csv_files: Vec<String>,
    /// Cursor inside that list.
    pub csv_cursor: usize,
    /// Row whose apply is waiting for its second [A].
    ///
    /// An apply cannot be undone - it replaces a whole box - so the first press
    /// only says what would happen. Any cursor move drops it, the same rule the
    /// equipment delete and the part clear follow.
    pub csv_armed: Option<usize>,
    /// What the last apply did, one line per fact, for the panel.
    pub csv_report: Vec<String>,
    /// The folder was asked for again and the I/O layer has not answered yet.
    pub wants_csv_rescan: bool,
    /// The user asked for an export; the I/O layer owns the clock.
    pub wants_csv_export: bool,
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
    /// Whole-save undo steps, newest last. At most [UNDO_STEPS].
    ///
    /// A step is the whole parsed save, so every screen that writes the save
    /// shares one stack. The oldest step is dropped first; four steps is what
    /// the console's heap affords (see [UNDO_STEPS]).
    pub undo: Vec<SaveUndo>,
    /// A console write waiting for its second press, with the review it shows.
    ///
    /// The review holds the names of the changes, not the bytes: the second
    /// press rebuilds them from the save that is still open.
    pub review: Option<SaveReview>,
    /// Status line.
    pub message: String,

    pending_request: Option<Request>,
    pending_write: Option<Write>,
    pending_text: Option<TextRequest>,
    /// The CSV file the I/O layer should read and hand back.
    pending_csv_read: Option<String>,
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
            guild_tab: GUILD_TAB_CARD,
            arena_row: 0,
            arena_rank: 0,
            arena_part: ARENA_PART_CS,
            arena_edit: false,
            progress_tab: 0,
            award_cursor: 0,
            request_cursor: 0,
            quest_cursor: 0,
            romfs_awards: Vec::new(),
            romfs_requests: Vec::new(),
            romfs_records: Vec::new(),
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
            item_set_edit: None,
            item_set_armed: None,
            item_set_warnings: Vec::new(),
            save_quest_cursor: 0,
            palico_cursor: 0,
            palico_lang: 0,
            palico_equip: None,
            palico_skill: None,
            option_cursor: 0,
            csv_files: Vec::new(),
            csv_cursor: 0,
            csv_armed: None,
            csv_report: Vec::new(),
            wants_csv_rescan: false,
            wants_csv_export: false,
            save_files: Vec::new(),
            arc_files: Vec::new(),
            save: None,
            save_path: None,
            arc: None,
            arc_path: None,
            quit: false,
            dirty: false,
            undo: Vec::new(),
            review: None,
            message: String::new(),
            pending_request: None,
            pending_write: None,
            pending_text: None,
            pending_csv_read: None,
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
        self.push_undo("名前");
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

    /// The guild card comment for the open slot, as one undo step.
    ///
    /// The console gets this text from its own keyboard, so the switch layer
    /// used to write the comment itself - which is why the comment was the one
    /// guild card edit the undo could not see. Going through the editor keeps
    /// the step and the message in the same place as every other write.
    pub fn set_guild_comment(&mut self, text: &str) -> bool {
        if self.save.is_none() {
            return false;
        }
        self.push_undo("ギルドカードのコメント");
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            return false;
        };
        if !crate::guildcard::set_comment(save, slot, text) {
            return false;
        }
        self.dirty = true;
        self.message = String::from("コメントを変更しました（[X] でセーブ保存）");
        true
    }

    // ------------------------------------------------------- arena log view

    /// Which view of the guild screen is showing.
    pub fn guild_view(&self) -> usize {
        if self.guild_tab < GUILD_TABS {
            self.guild_tab
        } else {
            GUILD_TAB_CARD
        }
    }

    /// Show one view of the guild screen.
    ///
    /// The time editor is a mode of the arena view, so a view change ends it;
    /// the cursors are clamped rather than cleared so a return to the arena
    /// view lands on the row the user was reading.
    pub fn set_guild_view(&mut self, view: usize) {
        self.guild_tab = view.min(GUILD_TABS - 1);
        self.arena_edit = false;
        self.arena_row = self.arena_row.min(ARENA_ROWS.saturating_sub(1));
        self.arena_rank = self.arena_rank.min(ARENA_COLUMNS.saturating_sub(1));
        self.message = if self.guild_tab == GUILD_TAB_ARENA {
            String::from("闘技大会: ▲▼ アリーナ / ◀▶ 順位 / [A] 編集 / [Y] 直接入力 / [X] 保存")
        } else {
            String::from("ギルドカード: ▼ 項目 / ◀▶ 増減 / [A] コメント / [-] 進行状況 / [X] 保存")
        };
    }

    /// The arena log, one row per arena and one column per rank.
    ///
    /// Read in one pass: guildcard::arena decodes all 85 entries of the slot, so
    /// a row-at-a-time caller would decode it 85 times per frame.
    pub fn arena_grid(&self) -> Vec<Vec<u16>> {
        let mut grid = alloc::vec![alloc::vec![0u16; ARENA_COLUMNS]; ARENA_ROWS];
        let Some(save) = &self.save else {
            return grid;
        };
        for e in crate::guildcard::arena(save, self.slot) {
            if let Some(row) = grid.get_mut(e.arena) {
                if let Some(cell) = row.get_mut(e.rank) {
                    *cell = e.centis;
                }
            }
        }
        grid
    }

    /// One stored time, 0 when the entry is empty or no save is open.
    pub fn arena_centis_at(&self, arena: usize, rank: usize) -> u16 {
        if arena >= ARENA_ROWS || rank >= ARENA_COLUMNS {
            return 0;
        }
        match &self.save {
            Some(save) => crate::guildcard::arena(save, self.slot)
                .into_iter()
                .find(|e| e.arena == arena && e.rank == rank)
                .map(|e| e.centis)
                .unwrap_or(0),
            None => 0,
        }
    }

    /// The arena and rank the view is on, clamped to what the log holds.
    pub fn arena_selected(&self) -> (usize, usize) {
        (
            self.arena_row.min(ARENA_ROWS.saturating_sub(1)),
            self.arena_rank.min(ARENA_COLUMNS.saturating_sub(1)),
        )
    }

    /// The time under the cursor, in hundredths.
    pub fn arena_centis(&self) -> u16 {
        let (a, r) = self.arena_selected();
        self.arena_centis_at(a, r)
    }

    /// The time under the cursor as minutes, seconds and hundredths.
    pub fn arena_parts(&self) -> (u16, u8, u8) {
        let total = self.arena_centis() as u32;
        (
            (total / 6000) as u16,
            ((total / 100) % 60) as u8,
            (total % 100) as u8,
        )
    }

    /// One part of the time under the cursor.
    pub fn arena_part_value(&self, part: usize) -> u16 {
        let (m, s, cs) = self.arena_parts();
        match part {
            ARENA_PART_MIN => m,
            ARENA_PART_SEC => s as u16,
            _ => cs as u16,
        }
    }

    /// Write one arena entry, as one undo step.
    ///
    /// Only the two time bytes move (guildcard::set_arena_time), and 0 empties
    /// the entry - the game's own "no record". A press that would write the
    /// value that is already there writes nothing and pushes nothing: holding
    /// Up at 0:00.00 must not fill the undo stack with identical saves.
    pub fn set_arena_time_at(&mut self, arena: usize, rank: usize, centis: u16) -> bool {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return false;
        }
        if arena >= ARENA_ROWS || rank >= ARENA_COLUMNS {
            self.message = alloc::format!("アリーナ {} / {} 位はありません", arena + 1, rank + 1);
            return false;
        }
        let current = self.arena_centis_at(arena, rank);
        if current == centis {
            self.message = alloc::format!(
                "{} {} はすでに {} です",
                arena_name(arena),
                arena_rank_name(rank),
                arena_clock(centis)
            );
            return false;
        }
        self.push_undo(alloc::format!("闘技大会 {}", arena + 1));
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            return false;
        };
        if !crate::guildcard::set_arena_time(save, slot, arena, rank, centis) {
            self.message = alloc::format!(
                "アリーナ {} の {} には書けません",
                arena + 1,
                arena_rank_name(rank)
            );
            return false;
        }
        self.dirty = true;
        self.message = alloc::format!(
            "{} {} を {} にしました（[X] でセーブ保存）",
            arena_name(arena),
            arena_rank_name(rank),
            arena_clock(centis)
        );
        true
    }

    /// Step one part of the time under the cursor by `delta`.
    pub fn adjust_arena_part(&mut self, delta: i64) -> bool {
        let (a, r) = self.arena_selected();
        let (m, s, cs) = self.arena_parts();
        let part = self.arena_part.min(ARENA_PARTS - 1);
        let (mut m, mut s, mut cs) = (m as i64, s as i64, cs as i64);
        match part {
            // 65,535 hundredths is 10:55.35, so ten minutes is the ceiling the
            // two stored bytes allow; the seconds a minute holds are 0..=59.
            ARENA_PART_MIN => m = (m + delta).clamp(0, (u16::MAX as i64) / 6000),
            ARENA_PART_SEC => s = (s + delta).clamp(0, 59),
            _ => cs = (cs + delta).clamp(0, 99),
        }
        let total = (m * 6000 + s * 100 + cs).clamp(0, u16::MAX as i64) as u16;
        let ok = self.set_arena_time_at(a, r, total);
        if !ok && self.arena_centis_at(a, r) == total {
            // Already at the end of the part's range: say so once, write nothing.
            self.message = alloc::format!(
                "{} はこれ以上 {} できません",
                arena_part_name(part),
                if delta > 0 { "増やせ" } else { "減らせ" }
            );
        }
        ok
    }

    /// Read a typed time into the entry under the cursor.
    pub fn apply_arena_time(&mut self, text: &str) -> bool {
        let (a, r) = self.arena_selected();
        match arena_clock_value(text) {
            Some(centis) => {
                let ok = self.set_arena_time_at(a, r, centis);
                if !ok && self.arena_centis_at(a, r) == centis {
                    self.message = alloc::format!(
                        "{} {} はすでに {} です",
                        arena_name(a),
                        arena_rank_name(r),
                        arena_clock(centis)
                    );
                }
                ok
            }
            None => {
                self.message =
                    String::from("m:ss.cc の形で入力してください（例 9:35.44 / 0:00.00 で消去）");
                false
            }
        }
    }

    /// Ask the I/O layer for the time under the cursor, as text.
    pub fn ask_arena_time(&mut self) -> bool {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return false;
        }
        let cur = arena_clock(self.arena_centis());
        self.pending_text = Some((TEXT_ARENA_TIME, cur));
        true
    }

    /// Rename one palico of the open slot, as one undo step.
    pub fn rename_palico(&mut self, which: usize, name: &str) -> bool {
        if self.save.is_none() {
            return false;
        }
        self.push_undo("オトモの名前");
        let slot = self.slot;
        let Some(save) = &mut self.save else {
            return false;
        };
        if !crate::palico::set_name(save, slot, which, name) {
            return false;
        }
        self.dirty = true;
        self.message = format!("オトモ {} の名前を変更しました", which);
        true
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
                // A step names the file and the slot it was taken in, so a
                // different file must not be able to reach it.
                self.undo.clear();
                self.review = None;
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
            // The equipment set editor uses Minus for the two-press part
            // clear and the item set editor for the two-press pouch write. On
            // the list above them Minus still means "rescan".
            Screen::Sets => self.set_editor_open(),
            // The CSV screen lists a folder, so Minus reads it again.
            Screen::Csv => true,
            // On the two palico screens Minus opens the equipment panel, so it
            // cannot also rescan there: one press would do both at once.
            Screen::Roster | Screen::Palico => true,
            // Minus on the guild card opens the progress screen for the same
            // reason: the rescan would fire on the same press.
            Screen::Guild => true,
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
        self.push_undo("キャラクター値");
        if let Some(s) = &mut self.save {
            let _ = s.write_character(&c);
            self.dirty = true;
        }
    }

    /// The one press "資金・P 一括MAX" of the console, as one undo step.
    ///
    /// The screen used to call set_char_value eleven times, which would have
    /// left four undo steps for one press and three of them naming the same ten
    /// fields. One press, one step, one label.
    pub fn maximize_character(&mut self) -> bool {
        let Some(mut c) = self.character() else {
            return false;
        };
        self.push_undo("資金・ポイント一括MAX");
        c.funds = 9_999_999;
        c.hunter_rank = 999;
        c.hr_points = 99_999_999;
        c.academy_points = 9_999_999;
        c.village_points = [9_999_999; 4];
        c.slot = self.slot;
        if let Some(s) = &mut self.save {
            let _ = s.write_character(&c);
            self.dirty = true;
        }
        self.message = String::from("所持金・HR・全ポイントを最大値にしました");
        true
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
            Screen::Progress => self.input_progress(key),
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
            Screen::Csv => self.input_csv(key),
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
        // The kind is what names the talisman, so it is read from the record
        // the form is seeded from rather than assumed: a 龍の護石 edited here
        // must stay a 龍の護石, and a fresh one must not silently become 天.
        let kind = self
            .save
            .as_ref()
            .and_then(|s| s.equipment_box(self.slot))
            .and_then(|b| {
                let o = seed.index * crate::charm::CHARM_RECORD;
                b.get(o..o + crate::charm::CHARM_RECORD)
                    .and_then(crate::charm::kind_of_record)
            })
            .unwrap_or_else(|| {
                crate::charm::canonical_kind(grade.rarity()).unwrap_or(10)
            });
        self.charm_edit = Some(CharmEdit {
            index: seed.index,
            kind,
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
                    self.push_undo("お守りの削除");
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
                        // The kind, not the rarity: the ten rows of
                        // amuletData_jpn.gmd are what the game names a talisman
                        // by, and the rarity is written with the kind. 龍 (7)
                        // is only reachable through this row.
                        e.kind = step_kind(e.kind, d > 0);
                        if let Some(g) = crate::charm::TalismanGrade::from_rarity(
                            crate::charm::kind_rarity(e.kind).unwrap_or(0),
                        ) {
                            e.grade = g;
                        }
                        // The kind carries its own measured ceiling (see
                        // charm::KIND_LIMITS): 兵士 / 闘士 stop at one slot
                        // and cannot hold a second skill at all, and 女王 /
                        // 王 / 龍 top out at +8 on the first skill. The form
                        // re-clamps against that before the box is touched,
                        // so the console never writes what the kind refuses.
                        let limits = crate::charm::kind_limits(e.kind);
                        let slot_cap = limits
                            .map(|l| l.max_slots.min(e.grade.max_slots()))
                            .unwrap_or_else(|| e.grade.max_slots());
                        e.slots = e.slots.min(slot_cap);
                        for (skill, pts, second) in [
                            (e.skill1, &mut e.points1, false),
                            (e.skill2, &mut e.points2, true),
                        ] {
                            if skill == 0 {
                                continue;
                            }
                            let cap = limits.and_then(|l| l.max_points(second));
                            match crate::charm::allowed_points_rarity(e.grade.rarity(), skill, second) {
                                Some(r) if r.min == 0 && r.max == 0 => {
                                    *pts = 0;
                                }
                                Some(r) => {
                                    let high = match cap {
                                        Some(c) => r.max.min(c),
                                        None => r.max,
                                    };
                                    *pts = (*pts).clamp(r.min, high);
                                }
                                // The table knows nothing about this id, so
                                // the form leaves the value alone (the same
                                // answer it gave before the kind ceilings).
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
                self.push_undo("お守りの書き込み");
                let kind_name = crate::charm::kind_name(e.kind).unwrap_or("?");
                if let Some(s) = &mut self.save {
                    // The kind is written with the skills, and its rarity comes
                    // along: the pair (+0x02, +0x12/+0x13) cannot disagree.
                    if s.set_charm_with_kind(self.slot, &c, e.kind) {
                        self.dirty = true;
                        self.message =
                            format!("お守り {} を書き込みました（{}）", e.index, kind_name);
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
        // Same rule as the roster: the panel owns every key while it is open.
        if self.palico_equip.is_some() {
            self.input_palico_equip(key);
            return;
        }
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
                self.push_undo("配信オトモ");
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
            // Minus opens the equipment panel for the palico the highlighted
            // download entry became in the roster. Y already means "install
            // all", L / R mean "language", A installs or removes and X saves,
            // so Minus is the one key this screen had left.
            Key::Minus => {
                if let Some(&id) = ids.get(self.palico_cursor) {
                    self.open_palico_equip_for_dlc(id);
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
        self.push_undo("配信オトモの一括導入");
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
                self.push_undo("設定");
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
            // R is the console's tab-forward key, and the tab after the
            // settings is the CSV screen, so one press lands on the same screen
            // either way (the pad takes L / R for the tab bar before the screen
            // sees them, exactly as L on the save list means the format tab).
            Key::R => self.open_csv(),
            Key::B => {
                self.screen = Screen::SaveList;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // CSV export / import
    // -----------------------------------------------------------------------
    //
    // The two boxes a player shares with a spreadsheet: the item box and the
    // equipment box. The files live in CSV_DIR and are named after a timestamp,
    // so an export never overwrites an earlier one, and an apply asks twice
    // because it replaces a whole box.
    //
    // Nothing here touches a file. The I/O layer lists the folder
    // (set_csv_files), owns the clock (export_csv takes it), writes what
    // export_csv returns and reads the file an apply asks for (take_csv_read,
    // answered with apply_csv_text). That is what keeps every rule above
    // testable on the host.

    /// Replace the CSV file list.
    ///
    /// Clamps the cursor and drops a pending apply that the new list no longer
    /// covers: an armed row belongs to the file it was armed on.
    pub fn set_csv_files(&mut self, files: Vec<String>) {
        self.csv_files = files;
        if self.csv_cursor >= self.csv_files.len() {
            self.csv_cursor = 0;
        }
        if self
            .csv_armed
            .map(|a| a >= self.csv_files.len())
            .unwrap_or(false)
        {
            self.csv_armed = None;
        }
    }

    /// Open the CSV screen and ask the I/O layer for the folder.
    pub fn open_csv(&mut self) {
        self.screen = Screen::Csv;
        self.cursor = 0;
        self.csv_cursor = 0;
        self.csv_armed = None;
        self.wants_csv_rescan = true;
        self.message = alloc::format!(
            "CSV 入出力: {}（[Y] で書き出し / [A] を 2 回で適用）",
            CSV_DIR
        );
    }

    /// Take the "read the folder again" request.
    pub fn take_csv_rescan(&mut self) -> bool {
        let w = self.wants_csv_rescan;
        self.wants_csv_rescan = false;
        w
    }

    /// Take the "write the two CSV files now" request.
    ///
    /// The clock belongs to the I/O layer, so it is the layer that collects the
    /// request and hands the timestamp back to Self::export_csv.
    pub fn take_csv_export(&mut self) -> bool {
        let w = self.wants_csv_export;
        self.wants_csv_export = false;
        w
    }

    /// The file an apply asked for, or None.
    ///
    /// The I/O layer reads it and answers with Self::apply_csv_text.
    pub fn take_csv_read(&mut self) -> Option<String> {
        self.pending_csv_read.take()
    }

    /// The CSV file under the cursor, if the folder has one.
    pub fn csv_selected(&self) -> Option<&str> {
        self.csv_files.get(self.csv_cursor).map(|s| s.as_str())
    }

    /// The CSV screen's keys.
    fn input_csv(&mut self, key: Key) {
        match key {
            Key::Up => {
                self.csv_cursor = self.csv_cursor.saturating_sub(1);
                self.csv_armed = None;
            }
            Key::Down => {
                if self.csv_cursor + 1 < self.csv_files.len() {
                    self.csv_cursor += 1;
                }
                self.csv_armed = None;
            }
            Key::A => self.csv_apply_press(),
            // Y writes both boxes out. The I/O layer supplies the clock, so the
            // request is left for it rather than stamped here.
            Key::Y => {
                self.csv_armed = None;
                self.wants_csv_export = true;
                self.message = String::from("CSV を書き出しています…");
            }
            Key::Minus => {
                self.csv_armed = None;
                self.wants_csv_rescan = true;
                self.message = String::from("CSV フォルダを読み直しています…");
            }
            Key::B => {
                self.csv_armed = None;
                self.screen = Screen::SaveList;
                self.cursor = 0;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// [A] on the CSV screen: the first press says what would happen, the
    /// second one asks the I/O layer for the file.
    fn csv_apply_press(&mut self) {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return;
        }
        let Some(path) = self.csv_selected().map(String::from) else {
            self.csv_armed = None;
            self.message =
                String::from("CSV がありません（[Y] でこのキャラの BOX を書き出せます）");
            return;
        };
        if self.csv_armed != Some(self.csv_cursor) {
            self.csv_armed = Some(self.csv_cursor);
            self.message = alloc::format!(
                "「{}」（{}）をキャラ {} に適用します。BOX の内容は置き換わります — もう一度 [A] で確定 / [B] で中止",
                file_name_of(&path),
                csv_kind_label(&path),
                self.slot + 1
            );
            return;
        }
        self.csv_armed = None;
        self.pending_csv_read = Some(path);
        self.message = String::from("CSV を読み込んでいます…");
    }

    /// Build both CSV files for the current character.
    ///
    /// Returns the (path, bytes) pairs the I/O layer has to write, in the order
    /// they are reported. Nothing is written here: only the layer knows where
    /// the SD card is. An empty result means nothing was built, and the message
    /// already says why - no clock, or no save open.
    ///
    /// The timestamp comes from the caller because this module has no clock.
    /// It names the files, so an export never overwrites an earlier one.
    pub fn export_csv(&mut self, unix: Option<u64>) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let Some(unix) = unix else {
            self.message = String::from("本体の時計から時刻を取得できないため書き出せません");
            return out;
        };
        let Some(save) = &self.save else {
            self.message = String::from("先にセーブを開いてください");
            return out;
        };
        let stamp = crate::snapshot::stamp(unix);
        let items_path = alloc::format!("{}/{}-{}.csv", CSV_DIR, CSV_ITEMS_STEM, stamp);
        let equip_path = alloc::format!("{}/{}-{}.csv", CSV_DIR, CSV_EQUIP_STEM, stamp);
        out.push((
            items_path.clone(),
            crate::csvout::items_to_csv(save, self.slot).into_bytes(),
        ));
        out.push((
            equip_path.clone(),
            crate::csvout::equipment_to_csv(save, self.slot).into_bytes(),
        ));
        out
    }

    /// The I/O layer wrote some of the files Self::export_csv handed it.
    ///
    /// Some of them, not all: a card that is full or write protected is
    /// reported as such instead of as a successful export.
    pub fn csv_exported(&mut self, written: usize, total: usize) {
        let line = if written == total {
            alloc::format!("{} ファイルを書き出しました（{}）", written, CSV_DIR)
        } else {
            alloc::format!(
                "{} / {} ファイルしか書き出せませんでした（SD カードの空き容量・保護を確認してください）",
                written,
                total
            )
        };
        self.csv_report = alloc::vec![line.clone()];
        // A file that landed changes the folder, so list it again.
        if written > 0 {
            self.wants_csv_rescan = true;
        }
        self.message = line;
    }

    /// The I/O layer hands back the text of a file an apply asked for.
    ///
    /// The header decides which table the file is, so a renamed export still
    /// applies; the file name is only the fallback for a file without one. The
    /// report is kept in csv_report for the panel and the save is marked dirty
    /// only when a row was really written.
    pub fn apply_csv_text(&mut self, path: &str, text: &str) -> bool {
        let name = file_name_of(path);
        // A spreadsheet on Windows puts a byte order mark in front of the
        // header. It is invisible on screen but it is not part of the first
        // cell, so the reader would take the header for a record and refuse
        // the whole file. Drop it once, here, for both the sniff and the parse.
        let text = text.trim_start_matches(BYTE_ORDER_MARK);
        let Some(kind) = csv_kind_of_text(text).or_else(|| csv_kind_of_name(path)) else {
            self.csv_report = alloc::vec![String::from(
                "先頭の行が items / equipment のどちらでもありません",
            )];
            self.message = alloc::format!("{} はアイテム/装備の CSV ではありません", name);
            return false;
        };
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return false;
        }
        let slot = self.slot;
        let mut report = None;
        if kind == CsvKind::Items {
            match crate::csvout::parse_items(text) {
                Ok(rows) => {
                    self.push_undo("CSV の適用");
                    if let Some(s) = &mut self.save {
                        report = Some(crate::csvout::apply_items(s, slot, &rows));
                    }
                }
                Err(e) => {
                    self.csv_report = alloc::vec![alloc::format!("{}", e)];
                    self.message = alloc::format!("{} を読めません: {}", name, e);
                    return false;
                }
            }
        } else {
            match crate::csvout::parse_equipment(text) {
                Ok(rows) => {
                    self.push_undo("CSV の適用");
                    if let Some(s) = &mut self.save {
                        report = Some(crate::csvout::apply_equipment(s, slot, &rows));
                    }
                }
                Err(e) => {
                    self.csv_report = alloc::vec![alloc::format!("{}", e)];
                    self.message = alloc::format!("{} を読めません: {}", name, e);
                    return false;
                }
            }
        }
        let Some(report) = report else {
            return false;
        };
        if report.applied > 0 {
            self.dirty = true;
        }
        let head = alloc::format!(
            "{}（{}）: {} 行適用 / {} 行スキップ / {} 行丸め",
            name,
            kind.label(),
            report.applied,
            report.skipped,
            report.clamped
        );
        self.csv_report = csv_report_lines(&report);
        self.message = head;
        true
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

    // ------------------------------------------------- whole save undo

    /// Keep the save as it is now, before an edit changes it.
    ///
    /// Called by every writer on the save, so a screen never has to grow its
    /// own undo. The oldest step is dropped first: four steps is what the heap
    /// affords (see [UNDO_STEPS]), and the newest step is the one a user reaches
    /// for.
    pub fn push_undo(&mut self, label: impl Into<String>) {
        let Some(save) = self.save.clone() else {
            return;
        };
        self.undo.push(SaveUndo {
            label: label.into(),
            slot: self.slot,
            save,
        });
        while self.undo.len() > UNDO_STEPS {
            self.undo.remove(0);
        }
    }

    /// How many steps are on the stack right now.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// What the next undo would take back, if anything.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|step| step.label.as_str())
    }

    /// The line the screen prints, e.g.
    /// "元に戻す: 3/4（1回 4.5MB を使用）".
    ///
    /// Both halves the audit asked for are here: how many steps are left of
    /// the four, and how much memory one of them costs.
    pub fn undo_line(&self) -> String {
        format!(
            "元に戻す: {}/{}（1回 {} を使用）",
            self.undo.len(),
            UNDO_STEPS,
            undo_step_text()
        )
    }

    /// "3/4" - the count alone, for the top bar.
    pub fn undo_count_text(&self) -> String {
        format!("{}/{}", self.undo.len(), UNDO_STEPS)
    }

    /// Put the whole save, and the slot it belonged to, back the way it was.
    ///
    /// Returns whether anything was taken back. Steps whose bytes match the
    /// save already in memory are skipped: a writer that pushed before an edit
    /// it then refused leaves one, and pressing undo on it must not look like a
    /// step that did nothing. The save is marked dirty, because the file on
    /// disk is no longer what this editor holds.
    pub fn undo_last(&mut self) -> bool {
        while let Some(step) = self.undo.pop() {
            let same = match &self.save {
                Some(current) => {
                    current.kind() == step.save.kind() && current.body() == step.save.body()
                }
                None => false,
            };
            if same {
                continue;
            }
            self.slot = step.slot.min(crate::save::MAX_SLOTS - 1);
            self.save = Some(step.save);
            self.dirty = true;
            self.after_undo();
            self.message = format!("元に戻しました: {}", step.label);
            return true;
        }
        self.message = String::from("戻せる操作がありません");
        false
    }

    /// Drop the state that named the bytes the undo just replaced.
    ///
    /// A form open on a record the restored save may not hold, or a per-tab
    /// stack whose entries describe the old boxes, would write the wrong bytes
    /// on the next press. Selections are clamped instead of cleared so the user
    /// stays where they were looking.
    fn after_undo(&mut self) {
        // The arena editor points at a time of the save that was just
        // replaced, so it is put away with the other forms.
        self.arena_edit = false;
        self.charm_edit = None;
        self.equip_edit = None;
        self.equip_delete_armed = None;
        self.set_edit = None;
        self.set_clear_armed = None;
        self.set_warnings.clear();
        self.item_set_edit = None;
        self.item_set_armed = None;
        self.item_set_warnings.clear();
        self.item_dedupe_armed = false;
        self.item_menu_open = false;
        self.csv_armed = None;
        self.review = None;
        self.item_undo.clear();
        let charms = self.charms().len();
        self.charm_cursor = self.charm_cursor.min(charms.saturating_sub(1));
        let equip = self.equipment().len();
        self.equip_cursor = self.equip_cursor.min(equip.saturating_sub(1));
        let palicos = self.palicos().len();
        self.roster_cursor = self.roster_cursor.min(palicos.saturating_sub(1));
        let quests = self.filtered_quests().len();
        self.save_quest_cursor = self.save_quest_cursor.min(quests.saturating_sub(1));
        self.cursor = self.cursor.min(self.lines_len_hint());
    }

    /// A cheap upper bound for the cursor on the current screen.
    ///
    /// Only used to keep a cursor inside the restored save; every screen clips
    /// its own list anyway, so being generous here costs nothing.
    fn lines_len_hint(&self) -> usize {
        match self.screen {
            Screen::Slots => 2,
            Screen::Char => CHAR_FIELDS.len().saturating_sub(1),
            _ => usize::MAX,
        }
    }

    // ---------------------------------------------------- pre-save review

    /// Is a save write waiting for its second press?
    pub fn review_open(&self) -> bool {
        self.review.is_some()
    }

    /// Compare a console write with what the console holds now.
    ///
    /// Called by the I/O layer after it reads the file that is about to be
    /// replaced and before it writes the new bytes. Returns true when the write
    /// differs from the file and must wait for a second press: the review is
    /// then on [Editor::review] and the message says so. A write that changes
    /// nothing returns false and is written at once, the way it always was.
    pub fn stage_save_review(&mut self, path: &str, old: &[u8], new: &[u8]) -> bool {
        let diff = diff_save(old, new);
        if diff.is_empty() {
            self.review = None;
            return false;
        }
        let lines = diff.lines(REVIEW_RANGES);
        self.message = format!(
            "変更 {} バイト。内容を確認して [A] でもう一度押すと保存します（[B] 中止）",
            diff.changed
        );
        self.review = Some(SaveReview {
            lines,
            summary: diff.summary,
            values: diff.values,
            ranges: diff.ranges,
            changed: diff.changed,
            old_len: old.len(),
            new_len: new.len(),
            path: String::from(path),
        });
        true
    }

    /// The second press: hand the write back so the I/O layer can perform it.
    ///
    /// The bytes are built again from the save that is still open, so the
    /// review never holds a second copy of the 4.5MB file.
    pub fn confirm_save_review(&mut self) -> Option<Write> {
        self.review.take()?;
        self.request_write_save();
        self.pending_write.take()
    }

    /// The B press: keep the edit in memory and write nothing.
    pub fn cancel_save_review(&mut self) {
        if self.review.take().is_some() {
            self.message = String::from("保存を中止しました（変更はこの画面に残っています）");
        }
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
                        self.push_undo("アイテム個数");
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
        // The whole-save stack records the same press: the box stack can only
        // take back the box, this one takes back everything together.
        self.push_undo("アイテムボックス");
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
                self.push_undo("狩猟記録");
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
        // The guild screen carries two views. The arena log is the second one,
        // and while it is showing it owns every key: the card's fields are not
        // on screen, so a press must not reach them.
        if self.guild_view() == GUILD_TAB_ARENA {
            return self.input_arena(key);
        }
        // Field 0 is the rank, 1 is the card's own clock, 2 the comment.
        match key {
            Key::Up => self.guild_field = (self.guild_field + 2) % 3,
            // [X] is the save key on every screen, so it is no longer a second
            // "next field" here (it used to be, and nothing said so).
            Key::Down => self.guild_field = (self.guild_field + 1) % 3,
            Key::Left | Key::Right => {
                let up = key == Key::Right;
                let slot = self.slot;
                self.push_undo("ギルドカード");
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
            // The card is the last record of this kind and the progress screen
            // continues it: awards, request flags and quest bits of the same
            // slot. [-] rather than a new screen-wide key, because A / Y / L /
            // R on the card already mean something.
            Key::Minus => self.open_progress(),
            Key::X => self.request_write_save(),
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// The arena view: 17 rows of five ranked times, one of them editable.
    ///
    /// Two modes, because the four directions have to mean two different things
    /// on one grid:
    ///
    /// * list mode - Up / Down walk the arenas, Left / Right walk the ranks
    ///   (1位 .. 5位), [A] arms the time editor, [Y] asks for the time as text.
    /// * edit mode - Left / Right walk the parts of the time (分 / 秒 / 1/100)
    ///   and Up / Down step the part under the cursor, [A] and [B] leave it.
    ///
    /// [X] saves in both, [-] opens the progress screen like the card view does.
    fn input_arena(&mut self, key: Key) {
        let (row, rank) = self.arena_selected();
        if self.arena_edit {
            match key {
                Key::Left => self.arena_part = (self.arena_part + ARENA_PARTS - 1) % ARENA_PARTS,
                Key::Right => self.arena_part = (self.arena_part + 1) % ARENA_PARTS,
                Key::Up => {
                    self.adjust_arena_part(1);
                }
                Key::Down => {
                    self.adjust_arena_part(-1);
                }
                Key::A | Key::B => {
                    self.arena_edit = false;
                    self.message = alloc::format!(
                        "{} {} は {}（[A] でまた編集できます）",
                        arena_name(row),
                        arena_rank_name(rank),
                        arena_clock(self.arena_centis())
                    );
                }
                Key::Y => {
                    self.ask_arena_time();
                }
                Key::X => self.request_write_save(),
                Key::Minus => {
                    // The editor is a mode of the view, and the progress screen
                    // is not it: leaving it armed would put a stepping cursor
                    // on a screen with no time to step.
                    self.arena_edit = false;
                    self.open_progress();
                }
                Key::Plus => self.quit = true,
                _ => {}
            }
            return;
        }
        match key {
            Key::Up => {
                self.arena_row = (row + ARENA_ROWS - 1) % ARENA_ROWS;
                self.describe_arena_row();
            }
            Key::Down => {
                self.arena_row = (row + 1) % ARENA_ROWS;
                self.describe_arena_row();
            }
            Key::Left => {
                self.arena_rank = (rank + ARENA_COLUMNS - 1) % ARENA_COLUMNS;
                self.describe_arena_row();
            }
            Key::Right => {
                self.arena_rank = (rank + 1) % ARENA_COLUMNS;
                self.describe_arena_row();
            }
            Key::A => {
                if self.save.is_none() {
                    self.message = String::from("先にセーブを開いてください");
                    return;
                }
                self.arena_edit = true;
                self.message = alloc::format!(
                    "編集中 {} {} — [◀▶] {} / [▲▼] 増減 / [A] 終了 / [Y] 直接入力{}",
                    arena_name(row),
                    arena_rank_name(rank),
                    arena_part_name(self.arena_part.min(ARENA_PARTS - 1)),
                    if self.arena_centis_at(row, rank) == 0 {
                        "（空き枠。0:00.00 のままなら無記録）"
                    } else {
                        ""
                    }
                );
            }
            Key::Y => {
                self.ask_arena_time();
            }
            Key::X => self.request_write_save(),
            Key::Minus => self.open_progress(),
            Key::B => {
                // Back to the card: the arena log is a view of this screen, so
                // one press goes one level up and the card's own B goes to the
                // slot list.
                self.set_guild_view(GUILD_TAB_CARD);
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Put the row and rank the cursor is on into the status line.
    fn describe_arena_row(&mut self) {
        let (row, rank) = self.arena_selected();
        self.message = alloc::format!(
            "{} ({}/{}) {} {} 編集: [A] / 直接入力: [Y] / 保存: [X]",
            arena_name(row),
            row + 1,
            ARENA_ROWS,
            arena_rank_name(rank),
            arena_clock(self.arena_centis_at(row, rank))
        );
    }


    // -----------------------------------------------------------------------
    // Progress screen: awards, request flags and quest bits
    //
    // Three bit maps that all live inside the character slot, asked the same
    // question: what has this character done? The screen is one screen with
    // three views so the cursor, the tab and the save target are shared - and
    // so the console's sub-tab bar (L / R) is the only navigation it needs.
    // -----------------------------------------------------------------------

    /// Open the progress screen for the current slot.
    pub fn open_progress(&mut self) {
        self.screen = Screen::Progress;
        // Every view clamps against its own length, so opening on an empty
        // save cannot leave a cursor past the end of a list.
        let row = self.progress_cursor();
        self.set_progress_cursor(row);
        self.message = if self.save.is_some() {
            String::from("A でビットを切り替え（2回押しは不要）  L / R でタブ切替")
        } else {
            String::from("先にセーブを開いてください")
        };
    }

    /// Which view of the progress screen is showing.
    pub fn progress_view(&self) -> usize {
        if self.progress_tab < PROGRESS_TABS {
            self.progress_tab
        } else {
            PROGRESS_AWARDS
        }
    }

    /// How many rows the showing view has.
    pub fn progress_len(&self) -> usize {
        match self.progress_view() {
            PROGRESS_AWARDS => awards::AWARDS,
            PROGRESS_REQUESTS => REQUEST_ROWS,
            // Index 0 is the unused dummy, so the rows are 1..=1508.
            _ => questprogress::BITS - 1,
        }
    }

    /// Row the cursor is on in the showing view.
    ///
    /// The three views keep their own cursor, so switching tabs and coming
    /// back lands on the row that was left - the alternative is losing the
    /// place in a list of 1508.
    pub fn progress_cursor(&self) -> usize {
        let last = self.progress_len().saturating_sub(1);
        let cur = match self.progress_view() {
            PROGRESS_AWARDS => self.award_cursor,
            PROGRESS_REQUESTS => self.request_cursor,
            _ => self.quest_cursor,
        };
        cur.min(last)
    }

    /// Put the cursor on a row, clipped to the showing view.
    pub fn set_progress_cursor(&mut self, row: usize) {
        let last = self.progress_len().saturating_sub(1);
        let row = row.min(last);
        match self.progress_view() {
            PROGRESS_AWARDS => self.award_cursor = row,
            PROGRESS_REQUESTS => self.request_cursor = row,
            _ => self.quest_cursor = row,
        }
    }

    /// Hand over the names and the request table from the user's own romfs.
    ///
    /// Nothing here reads a file and nothing here ships game text: the I/O
    /// layer parses the dump (romfstables) and this only keeps the result. The
    /// lengths are not required to match, so a table of the wrong size simply
    /// does not name every row instead of shifting the names by one.
    pub fn set_romfs_tables(
        &mut self,
        awards: Vec<String>,
        requests: Vec<String>,
        records: Vec<RequestRecord>,
    ) {
        self.romfs_awards = awards;
        self.romfs_requests = requests;
        self.romfs_records = records;
    }

    /// Name of one award, or "" when no table was supplied.
    pub fn award_name(&self, award: usize) -> &str {
        self.romfs_awards
            .get(award)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// (held, total) awards of the current slot.
    pub fn progress_awards(&self) -> (usize, usize) {
        let held = match &self.save {
            Some(save) => awards::count(save, self.slot),
            None => 0,
        };
        (held, awards::AWARDS)
    }

    /// Do the three copies of the award map hold the same bits?
    ///
    /// None when there is no character to compare. Some(false) means the save
    /// is already in the state the game repairs by OR, so the screen says so
    /// before a write picks a side - the card copy may be the stale one.
    pub fn progress_awards_agree(&self) -> Option<bool> {
        let save = self.save.as_ref()?;
        if !save.slot_used(self.slot) {
            return None;
        }
        Some(awards::copies_agree(save, self.slot))
    }

    /// The flag numbers the request view lists: the two request bands, end to
    /// end and in ascending order.
    pub fn request_flags(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(REQUEST_ROWS);
        for (start, end) in REQUEST_BANDS {
            out.extend(start..end);
        }
        out
    }

    /// The flag a request row stands for, without building the whole list.
    pub fn request_flag_at(&self, row: usize) -> Option<usize> {
        let mut left = row;
        for (start, end) in REQUEST_BANDS {
            let len = end - start;
            if left < len {
                return Some(start + left);
            }
            left -= len;
        }
        None
    }

    /// How many of the 1536 event flags are set.
    ///
    /// Not the number of requests: the flag space is shared with other
    /// unlocks, so the screen labels this as bits, not requests.
    pub fn progress_request_count(&self) -> usize {
        match &self.save {
            Some(save) => requests::count(save, self.slot),
            None => 0,
        }
    }

    /// How many of the request band flags are set.
    pub fn progress_request_band_count(&self) -> usize {
        let Some(save) = &self.save else {
            return 0;
        };
        self.request_flags()
            .into_iter()
            .filter(|&flag| requests::get(save, self.slot, flag) == Some(true))
            .count()
    }

    /// What a request flag number is, when the request table was supplied.
    ///
    /// Without the table (the console, which has no romfs) a flag number is
    /// only an event flag: the save does not say which request it opens.
    pub fn request_flag_label(&self, flag: usize) -> String {
        let Some(rec) = self
            .romfs_records
            .iter()
            .find(|r| r.accept_flag as usize == flag || r.complete_flag as usize == flag)
        else {
            return String::from("イベント");
        };
        let step = if rec.accept_flag as usize == flag {
            "受理"
        } else {
            "完了"
        };
        let name = self
            .romfs_requests
            .get(rec.index)
            .map(|s| s.as_str())
            .unwrap_or("");
        if name.is_empty() {
            alloc::format!("依頼 #{:03} {}", rec.index, step)
        } else {
            alloc::format!("依頼 #{:03} {} {}", rec.index, step, name)
        }
    }

    /// (cleared, seen, failed) counts of the current slot.
    pub fn progress_quest_counts(&self) -> (usize, usize, usize) {
        let Some(save) = &self.save else {
            return (0, 0, 0);
        };
        (
            questprogress::count(save, self.slot, Progress::Cleared),
            questprogress::count(save, self.slot, Progress::Seen),
            questprogress::count(save, self.slot, Progress::Failed),
        )
    }

    /// The quest progress index the cursor is on: the row plus one, because
    /// index 0 lines up with the unused dummy quest.
    pub fn progress_quest_index(&self) -> usize {
        self.quest_cursor.min(questprogress::BITS - 2) + 1
    }

    /// (page, pages) of the quest view, 1 based, in pages of PROGRESS_JUMP.
    pub fn progress_quest_page(&self) -> (usize, usize) {
        let total = questprogress::BITS - 1;
        (
            self.progress_cursor() / PROGRESS_JUMP + 1,
            total.div_ceil(PROGRESS_JUMP),
        )
    }

    /// (cleared, seen, failed) of one quest row.
    pub fn progress_quest_state(&self, row: usize) -> (bool, bool, bool) {
        let index = row + 1;
        let Some(save) = &self.save else {
            return (false, false, false);
        };
        (
            questprogress::get(save, self.slot, Progress::Cleared, index) == Some(true),
            questprogress::get(save, self.slot, Progress::Seen, index) == Some(true),
            questprogress::get(save, self.slot, Progress::Failed, index) == Some(true),
        )
    }

    /// How many quests are cleared without being seen: the invariant the core
    /// reports and deliberately does not repair.
    pub fn progress_clears_without_seen(&self) -> usize {
        match &self.save {
            Some(save) => questprogress::clears_without_seen(save, self.slot).len(),
            None => 0,
        }
    }

    fn input_progress(&mut self, key: Key) {
        match key {
            // The three views are this screen's sub-tabs, so L / R walk them
            // the same way every other sub-tab bar does.
            Key::L => {
                self.progress_tab = (self.progress_view() + PROGRESS_TABS - 1) % PROGRESS_TABS;
            }
            Key::R => {
                self.progress_tab = (self.progress_view() + 1) % PROGRESS_TABS;
            }
            // Y is the second bit of the quest view (seen). The other two views
            // have no second bit to give - an award or a request flag is one
            // bit - so there it cycles the view instead. The chips on screen
            // name whichever it is on the view in front of the user.
            Key::Y => {
                if self.progress_view() == PROGRESS_QUESTS {
                    self.progress_quest_seen();
                } else {
                    self.progress_tab = (self.progress_view() + 1) % PROGRESS_TABS;
                }
            }
            Key::Up => {
                let row = self.progress_cursor();
                self.set_progress_cursor(row.saturating_sub(1));
            }
            Key::Down => {
                let row = self.progress_cursor();
                self.set_progress_cursor(row.saturating_add(1));
            }
            Key::Left => {
                let row = self.progress_cursor();
                self.set_progress_cursor(row.saturating_sub(PROGRESS_JUMP));
            }
            Key::Right => {
                let row = self.progress_cursor();
                self.set_progress_cursor(row.saturating_add(PROGRESS_JUMP));
            }
            Key::A => self.progress_toggle(),
            Key::B => {
                self.screen = Screen::Guild;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
    }

    /// Flip the bit under the cursor. One press, because one press is the undo.
    fn progress_toggle(&mut self) {
        let slot = self.slot;
        let row = self.progress_cursor();
        self.push_undo("進行状況");
        match self.progress_view() {
            PROGRESS_AWARDS => {
                let Some(save) = &mut self.save else {
                    self.message = String::from("先にセーブを開いてください");
                    return;
                };
                let before = awards::has(save, slot, row).unwrap_or(false);
                if awards::set(save, slot, row, !before) {
                    self.dirty = true;
                    self.message = alloc::format!(
                        "勲章 {:03} を{}にしました（3つのコピーすべて）",
                        row + 1,
                        if before { "未取得" } else { "取得" }
                    );
                } else {
                    self.message =
                        String::from("勲章を書き換えられませんでした（この枠にキャラがいません）");
                }
            }
            PROGRESS_REQUESTS => {
                let Some(flag) = self.request_flag_at(row) else {
                    return;
                };
                // Named before the save is borrowed: the label reads the romfs
                // table, which lives in the same struct.
                let label = self.request_flag_label(flag);
                let Some(save) = &mut self.save else {
                    self.message = String::from("先にセーブを開いてください");
                    return;
                };
                let before = requests::get(save, slot, flag).unwrap_or(false);
                if requests::set(save, slot, flag, !before) {
                    self.dirty = true;
                    self.message = alloc::format!(
                        "フラグ {} を {} にしました（{}）",
                        flag,
                        if before { "OFF" } else { "ON" },
                        label
                    );
                } else {
                    self.message =
                        String::from("フラグを書き換えられませんでした（この枠にフラグ表がありません）");
                }
            }
            _ => {
                let index = row + 1;
                let Some(save) = &mut self.save else {
                    self.message = String::from("先にセーブを開いてください");
                    return;
                };
                match questprogress::get(save, slot, Progress::Cleared, index) {
                    // Taking the clear off leaves the seen bit alone: "seen but
                    // not cleared" is a state the game writes itself.
                    Some(true) => {
                        if questprogress::set(save, slot, Progress::Cleared, index, false) {
                            self.dirty = true;
                            self.message = alloc::format!(
                                "クエスト {} のクリアを取り消しました（既読はそのまま）",
                                index
                            );
                        }
                    }
                    Some(false) => {
                        if questprogress::set(save, slot, Progress::Cleared, index, true) {
                            self.dirty = true;
                            self.message = alloc::format!(
                                "クエスト {} をクリアにしました（既読も立ちます）",
                                index
                            );
                        }
                    }
                    None => {
                        self.message =
                            String::from("クエスト進捗を読めません（MHGU のセーブは未対応です）")
                    }
                }
            }
        }
    }

    /// Y on the quest view: the seen bit, which the core refuses to clear while
    /// the quest is still cleared.
    fn progress_quest_seen(&mut self) {
        let slot = self.slot;
        let index = self.progress_cursor() + 1;
        self.push_undo("クエスト既読");
        let Some(save) = &mut self.save else {
            self.message = String::from("先にセーブを開いてください");
            return;
        };
        match questprogress::get(save, slot, Progress::Seen, index) {
            Some(true) => {
                if questprogress::set(save, slot, Progress::Seen, index, false) {
                    self.dirty = true;
                    self.message = alloc::format!("クエスト {} の既読を取り消しました", index);
                } else {
                    self.message = alloc::format!(
                        "クエスト {} はクリア済みなので既読を外せません（先に A でクリアを取り消してください）",
                        index
                    );
                }
            }
            Some(false) => {
                if questprogress::set(save, slot, Progress::Seen, index, true) {
                    self.dirty = true;
                    self.message = alloc::format!("クエスト {} を既読にしました", index);
                }
            }
            None => {
                self.message = String::from("クエスト進捗を読めません（MHGU のセーブは未対応です）")
            }
        }
    }

    fn input_roster(&mut self, key: Key) {
        // The two panels are modes of this screen: while either is open every
        // key belongs to it and the list underneath cannot move.
        if self.palico_skill.is_some() {
            self.input_palico_skill(key);
            return;
        }
        if self.palico_equip.is_some() {
            self.input_palico_equip(key);
            return;
        }
        let list = self.palicos();
        let n = list.len();
        match key {
            Key::Up => self.roster_cursor = self.roster_cursor.saturating_sub(1),
            Key::Down => {
                if n > 0 {
                    self.roster_cursor = (self.roster_cursor + 1).min(n - 1);
                }
            }
            // R walks the editable fields of the highlighted palico. X used to
            // do the same, but on the pad X is the save key and never reached
            // this screen; keeping it as a second field key meant the chips had
            // to advertise X as something it did not do.
            Key::R => self.roster_field = (self.roster_field + 1) % 3,
            Key::X => self.request_write_save(),
            Key::L => self.roster_field = (self.roster_field + 2) % 3,
            Key::Left | Key::Right => {
                let Some(p) = list.get(self.roster_cursor).cloned() else {
                    return;
                };
                let up = key == Key::Right;
                let slot = self.slot;
                self.push_undo("オトモ");
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
            // Minus opens the equipment panel for the highlighted palico. The
            // Switch layer therefore stops rescanning on this screen; one
            // press cannot do both (see `uses_minus`).
            Key::Minus => {
                if let Some(p) = list.get(self.roster_cursor) {
                    let index = p.index;
                    self.open_palico_equip(index);
                }
            }
            // Plus opens the skill / support move panel for the highlighted
            // palico. The pad has no other free button on this screen and
            // Plus is already the screen-specific tool key on the equipment
            // list; quitting stays on the slot list one B away, and comes
            // back here while the panel itself is open.
            Key::Plus => {
                if let Some(p) = list.get(self.roster_cursor) {
                    let index = p.index;
                    self.open_palico_skill(index);
                }
            }
            Key::B => {
                self.screen = Screen::Slots;
                self.cursor = self.slot;
            }
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

    /// Is the palico equipment panel open?
    pub fn palico_equip_open(&self) -> bool {
        self.palico_equip.is_some()
    }

    /// The roster record the panel edits, as the save holds it now.
    pub fn palico_equip_target(&self) -> Option<crate::palico::Palico> {
        let state = self.palico_equip?;
        self.palicos().into_iter().find(|p| p.index == state.palico)
    }

    /// The three panel rows: what is worn now and what [A] would write.
    ///
    /// A worn index the box no longer holds keeps its number and reports ID 0
    /// rather than pretending nothing is worn: the roster names a record by
    /// index and showing the number is what lets the user go and look.
    pub fn palico_equip_rows(&self) -> Vec<PalicoEquipRow> {
        let mut out = Vec::with_capacity(PALICO_EQUIP_ROWS);
        let Some(state) = self.palico_equip else {
            return out;
        };
        let target = self.palico_equip_target();
        let save = self.save.as_ref();
        let id_of = |index: u16| -> u16 {
            match save {
                Some(save) if index != PALICO_EQUIP_NONE => {
                    crate::palicogear::decode_one(save, self.slot, index as usize)
                        .map(|e| e.id)
                        .unwrap_or(0)
                }
                _ => 0,
            }
        };
        for part in 0..PALICO_EQUIP_ROWS {
            let worn = target
                .as_ref()
                .map(|p| p.equip[part])
                .unwrap_or(PALICO_EQUIP_NONE);
            let pick = state.pick[part];
            out.push(PalicoEquipRow {
                part,
                worn,
                worn_id: id_of(worn),
                pick,
                pick_id: id_of(pick),
            });
        }
        out
    }

    /// The box records one part can pick from, in box order.
    ///
    /// Only records whose type is the one that part takes are offered: the box
    /// is a flat array and the sample save interleaves the three kinds, so the
    /// type decides membership, not a range of indices.
    pub fn palico_equip_candidates(&self, part: usize) -> Vec<u16> {
        let Some(kind) = crate::palicogear::KINDS.get(part).copied() else {
            return Vec::new();
        };
        let Some(save) = self.save.as_ref() else {
            return Vec::new();
        };
        crate::palicogear::decode(save, self.slot)
            .into_iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.index as u16)
            .collect()
    }

    /// Open the equipment panel for one roster record.
    pub fn open_palico_equip(&mut self, palico: usize) -> bool {
        if palico >= crate::palico::SLOTS {
            return false;
        }
        let Some(record) = self.palicos().into_iter().find(|p| p.index == palico) else {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        };
        // The skill panel is the other mode of this screen: one form at a
        // time, so opening this one puts that one away.
        self.palico_skill = None;
        let mut pick = [PALICO_EQUIP_NONE; PALICO_EQUIP_ROWS];
        for part in 0..PALICO_EQUIP_ROWS {
            // Start on what the palico wears, so [A] on an untouched row writes
            // the same index back; a part with nothing worn starts on the
            // first record of its kind, so the panel is usable without moving.
            if record.equip[part] != PALICO_EQUIP_NONE {
                pick[part] = record.equip[part];
            } else if let Some(first) = self.palico_equip_candidates(part).first().copied() {
                pick[part] = first;
            }
        }
        self.palico_equip = Some(PalicoEquip {
            palico,
            field: 0,
            pick,
            clear_armed: None,
        });
        self.message = format!(
            "{} の装備を割り当てます（[▲/▼] 部位  [◀/▶] 候補  [A] 装着  [-]×2 解除）",
            record.name
        );
        true
    }

    /// Open the panel for the download palico the list row names.
    ///
    /// A download record is the game's template for the roster entry it
    /// installs, and the roster copy carries the same name. That name field is
    /// the one link both documented layouts share, so a missing or ambiguous
    /// name is refused instead of writing an index into the wrong palico.
    pub fn open_palico_equip_for_dlc(&mut self, id: u8) -> bool {
        let Some(record) = self
            .save
            .as_ref()
            .and_then(|s| s.dlc_palicoes().into_iter().find(|c| c.id == id))
        else {
            self.message = format!("DLC ID {} はまだ導入されていません（[A] で導入）", id);
            return false;
        };
        let name = record.name;
        let matches: Vec<usize> = self
            .palicos()
            .into_iter()
            .filter(|p| p.name == name)
            .map(|p| p.index)
            .collect();
        match matches.len() {
            0 => {
                self.message = format!(
                    "「{}」はロスターに居ません（ゲーム内で受け取ると並びます）",
                    name
                );
                false
            }
            1 => self.open_palico_equip(matches[0]),
            _ => {
                self.message = format!(
                    "「{}」と同じ名前のオトモが {} 居るので、どれか決められません",
                    name,
                    matches.len()
                );
                false
            }
        }
    }

    /// Put the panel away. The screen keeps its list and its cursor.
    pub fn close_palico_equip(&mut self) {
        if self.palico_equip.take().is_some() {
            self.message = String::from("装備の割当を閉じました");
        }
    }

    /// Point one of a palico's three equipment slots at a record in its box.
    ///
    /// The record has to be in use and has to carry the kind that part takes:
    /// the sample save has every worn index landing on the right kind, and a
    /// value that breaks that is not something this editor should create. The
    /// record itself is read but never written - the roster names it by index.
    pub fn palico_equip_assign(&mut self, palico: usize, part: usize, index: u16) -> bool {
        let slot = self.slot;
        let part_name = palico_part_name(part);
        self.push_undo("オトモ装備");
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if part >= PALICO_EQUIP_ROWS {
            self.message = format!("{} はオトモの装備部位ではありません", part);
            return false;
        }
        // An empty roster record is not a palico: writing gear into it would
        // leave an index the game reads for a slot it never shows.
        let exists = crate::palico::roster(save, slot)
            .get(palico)
            .map(|p| !p.is_empty())
            .unwrap_or(false);
        if !exists {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        }
        let Some(eq) = crate::palicogear::decode_one(save, slot, index as usize) else {
            self.message = format!(
                "装備ボックスの枠 {} は空きなので、{}にできません",
                index, part_name
            );
            return false;
        };
        if eq.kind != crate::palicogear::KINDS[part] {
            self.message = format!(
                "枠 {} は{}なので、{}にはできません",
                index,
                crate::equip::type_name(eq.kind),
                part_name
            );
            return false;
        }
        if !write_palico_equip(save, slot, palico, part, index) {
            self.message = format!("枠 {} の{}を書き換えられませんでした", palico, part_name);
            return false;
        }
        self.dirty = true;
        self.message = format!(
            "枠 {} の{}をオトモ装備の枠 {} にしました（ボックスのレコードはそのままです）",
            palico, part_name, index
        );
        true
    }

    /// Take one piece off a palico by writing 0xFFFF into its roster slot.
    pub fn palico_equip_release(&mut self, palico: usize, part: usize) -> bool {
        let slot = self.slot;
        let part_name = palico_part_name(part);
        self.push_undo("オトモ装備の解除");
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        if part >= PALICO_EQUIP_ROWS {
            self.message = format!("{} はオトモの装備部位ではありません", part);
            return false;
        }
        let current = crate::palico::roster(save, slot)
            .get(palico)
            .map(|p| p.equip[part]);
        let Some(current) = current else {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        };
        if current == PALICO_EQUIP_NONE {
            self.message = format!("枠 {} の{}はすでに外れています", palico, part_name);
            return false;
        }
        if !write_palico_equip(save, slot, palico, part, PALICO_EQUIP_NONE) {
            self.message = format!("枠 {} の{}を外せませんでした", palico, part_name);
            return false;
        }
        self.dirty = true;
        self.message = format!("枠 {} の{}を外しました（0xFFFF）", palico, part_name);
        true
    }

    /// Keys of the palico equipment panel.
    ///
    /// [A] is the only key that writes a worn index and [-] is the only one
    /// that empties a part, twice. Every other key is a cursor move and drops a
    /// pending release first, so a press can never land on a part the user has
    /// stopped looking at.
    fn input_palico_equip(&mut self, key: Key) {
        let Some(mut state) = self.palico_equip else {
            return;
        };
        if key != Key::Minus {
            state.clear_armed = None;
        }
        let field = state.field.min(PALICO_EQUIP_ROWS - 1);
        match key {
            Key::Up => {
                state.field = (field + PALICO_EQUIP_ROWS - 1) % PALICO_EQUIP_ROWS;
                self.palico_equip = Some(state);
            }
            Key::Down => {
                state.field = (field + 1) % PALICO_EQUIP_ROWS;
                self.palico_equip = Some(state);
            }
            Key::Left | Key::Right => {
                self.palico_equip = Some(state);
                self.cycle_palico_pick(if key == Key::Right { 1 } else { -1 });
            }
            Key::A => {
                let pick = state.pick[field];
                let palico = state.palico;
                self.palico_equip = Some(state);
                if pick == PALICO_EQUIP_NONE {
                    self.message = format!(
                        "{}の候補がありません（装備BOXで作成してください）",
                        palico_part_name(field)
                    );
                } else {
                    self.palico_equip_assign(palico, field, pick);
                }
            }
            Key::Minus => {
                let palico = state.palico;
                self.palico_equip = Some(state);
                self.release_palico_part(palico, field);
            }
            Key::B => self.close_palico_equip(),
            Key::X => self.request_write_save(),
            // Y has no tool of its own here; it repeats the one line the panel
            // cannot say on a row (the names are not in the catalogue).
            Key::Y => {
                self.palico_equip = Some(state);
                self.message = String::from(PALICO_EQUIP_NAME_NOTE);
            }
            Key::Plus => self.quit = true,
            // L / R would jump to another sub-tab and Z is ZR on the pad; both
            // are swallowed while the panel owns the screen.
            Key::L | Key::R | Key::Z => {
                self.palico_equip = Some(state);
            }
        }
    }

    /// Move the row's candidate through the records of that part's kind.
    fn cycle_palico_pick(&mut self, delta: i64) {
        let Some(mut state) = self.palico_equip else {
            return;
        };
        let part = state.field.min(PALICO_EQUIP_ROWS - 1);
        let list = self.palico_equip_candidates(part);
        if list.is_empty() {
            self.message = format!(
                "{}の装備がBOXにありません（装備BOXで作成してください）",
                palico_part_name(part)
            );
            return;
        }
        let n = list.len() as i64;
        let next = match list.iter().position(|v| *v == state.pick[part]) {
            Some(pos) => (pos as i64 + delta).rem_euclid(n) as usize,
            // Nothing chosen yet: step in from the end the key points at.
            None => {
                if delta >= 0 {
                    0
                } else {
                    list.len() - 1
                }
            }
        };
        state.pick[part] = list[next];
        state.clear_armed = None;
        self.palico_equip = Some(state);
        self.message = format!("{}の候補: 枠 {} / ID {}", palico_part_name(part), list[next], self.palico_gear_id(list[next]));
    }

    /// First half of the two-press release: report, do not touch the record.
    fn release_palico_part(&mut self, palico: usize, part: usize) {
        let Some(mut state) = self.palico_equip else {
            return;
        };
        let worn = self
            .palico_equip_target()
            .map(|p| p.equip[part])
            .unwrap_or(PALICO_EQUIP_NONE);
        if worn == PALICO_EQUIP_NONE {
            state.clear_armed = None;
            self.palico_equip = Some(state);
            self.message = format!("{}はすでに外れています", palico_part_name(part));
            return;
        }
        if state.clear_armed != Some(part) {
            state.clear_armed = Some(part);
            self.palico_equip = Some(state);
            self.message = format!(
                "{}を外しますか？ もう一度 [-] で解除（枠 {} / オトモ枠 {}）",
                palico_part_name(part),
                worn,
                palico
            );
            return;
        }
        self.palico_equip = Some(state);
        self.palico_equip_release(palico, part);
    }

    /// Equip id of one palico box record, 0 when the index holds nothing.
    fn palico_gear_id(&self, index: u16) -> u16 {
        match self.save.as_ref() {
            Some(save) if index != PALICO_EQUIP_NONE => {
                crate::palicogear::decode_one(save, self.slot, index as usize)
                    .map(|e| e.id)
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    // ---------------------------------------------------------- palico skills

    /// Is the palico skill / support move panel open?
    pub fn palico_skill_open(&self) -> bool {
        self.palico_skill.is_some()
    }

    /// The roster record the panel edits, as the save holds it now.
    pub fn palico_skill_target(&self) -> Option<crate::palico::Palico> {
        let state = self.palico_skill?;
        self.palicos().into_iter().find(|p| p.index == state.palico)
    }

    /// The tab the panel is on: PALICO_TAB_SKILLS or PALICO_TAB_MOVES.
    pub fn palico_skill_tab(&self) -> usize {
        match self.palico_skill {
            Some(state) if state.tab == PALICO_TAB_MOVES => PALICO_TAB_MOVES,
            _ => PALICO_TAB_SKILLS,
        }
    }

    /// Rows the tab on screen lists: twelve skills or sixteen support moves.
    pub fn palico_skill_list_len(&self) -> usize {
        match self.palico_skill {
            Some(_) => palico_skill_tab_rows(self.palico_skill_tab()),
            None => 0,
        }
    }

    /// Slots the list on screen uses, out of the rows it shows.
    ///
    /// The first number is the record's own length byte: only those slots can be
    /// written, because the game never writes past it. The rest is the 57 / 96
    /// fill, listed rather than hidden so the panel shows the list the record
    /// really carries.
    pub fn palico_skill_slots(&self) -> (usize, usize) {
        if self.palico_skill.is_none() {
            return (0, 0);
        }
        let tab = self.palico_skill_tab();
        let rows = palico_skill_tab_rows(tab);
        let used = match self.palico_skill_target() {
            Some(p) if tab == PALICO_TAB_MOVES => p.learned_move_slots(),
            Some(p) => p.learned_skill_slots(),
            None => 0,
        };
        (used, rows)
    }

    /// "系統: ヒーリング   攻撃傾向: 3" for the palico on the panel.
    ///
    /// The support tendency has eight documented values. The attack tendency has
    /// no table in this project, so it is shown as the number the byte stores and
    /// a byte outside the range the game offers is called out rather than dressed
    /// up as a name.
    pub fn palico_skill_tendencies(&self) -> String {
        let Some(p) = self.palico_skill_target() else {
            return String::new();
        };
        let system = if crate::palico::type_of(&p).is_some() {
            String::from(p.system_name())
        } else {
            format!("?（{} は範囲外）", p.system)
        };
        let attack = match crate::palico::attack_of(&p) {
            Some(v) => format!("{}", v),
            None => format!("?（{} は範囲外）", p.attack),
        };
        format!("系統: {}   攻撃傾向: {}", system, attack)
    }

    /// Every slot of the list on screen, decoded.
    pub fn palico_skill_rows(&self) -> Vec<PalicoSkillRow> {
        let mut out = Vec::new();
        let Some(state) = self.palico_skill else {
            return out;
        };
        let Some(target) = self.palico_skill_target() else {
            return out;
        };
        let tab = if state.tab == PALICO_TAB_MOVES {
            PALICO_TAB_MOVES
        } else {
            PALICO_TAB_SKILLS
        };
        let moves = tab == PALICO_TAB_MOVES;
        let empty = if moves {
            crate::palico::MOVE_EMPTY
        } else {
            crate::palico::SKILL_EMPTY
        };
        let (list, used, rows) = if moves {
            (
                &target.learned_moves,
                target.learned_move_slots(),
                PALICO_MOVE_ROWS,
            )
        } else {
            (
                &target.learned_skills,
                target.learned_skill_slots(),
                PALICO_SKILL_ROWS,
            )
        };
        for index in 0..rows {
            let current = list.get(index).copied().unwrap_or(empty);
            let pick = state.picks.get(index).copied().unwrap_or(PALICO_PICK_FIRST);
            let named = current != 0 && current != empty;
            out.push(PalicoSkillRow {
                index,
                current,
                pick,
                counted: index < used,
                current_name: if named {
                    palico_skill_id_name(tab, current)
                } else {
                    String::from("-")
                },
                pick_name: if pick != 0 {
                    palico_skill_id_name(tab, pick)
                } else {
                    String::from("-")
                },
            });
        }
        out
    }

    /// The panel's warnings: the learned and equipped lists of this palico.
    ///
    /// Both lists are checked by the core, because the equipped lists hang off
    /// the learned ones: showing half of what is inconsistent about a record
    /// would be lying by omission.
    pub fn palico_skill_warnings(&self) -> Vec<String> {
        let Some(state) = self.palico_skill else {
            return Vec::new();
        };
        let Some(save) = self.save.as_ref() else {
            return Vec::new();
        };
        let mut out = crate::palico::skill_warnings(save, self.slot, state.palico);
        out.extend(crate::palico::move_warnings(
            save,
            self.slot,
            state.palico,
        ));
        out
    }

    /// Open the skill / support move panel for one roster record.
    ///
    /// The equipment panel is closed on the way in: the two are modes of the
    /// same screen and edit the same record, so leaving one open under the other
    /// would give the user two forms whose keys fight.
    pub fn open_palico_skill(&mut self, palico: usize) -> bool {
        if palico >= crate::palico::SLOTS {
            return false;
        }
        let Some(record) = self.palicos().into_iter().find(|p| p.index == palico) else {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        };
        self.palico_equip = None;
        self.palico_skill = Some(PalicoSkill {
            palico,
            tab: PALICO_TAB_SKILLS,
            field: 0,
            picks: palico_skill_picks(&record, PALICO_TAB_SKILLS),
        });
        self.message = format!(
            "{} のスキルとサポート行動（スキル {} 枠中 {}）",
            record.name,
            PALICO_SKILL_ROWS,
            record.learned_skill_slots()
        );
        true
    }

    /// Put the panel away. The roster keeps its own row and cursor.
    pub fn close_palico_skill(&mut self) {
        if self.palico_skill.take().is_some() {
            self.message = String::from("スキル編集を閉じました");
        }
    }

    /// Write one slot of the learned list on screen.
    ///
    /// This is crate::palico::set_skill / set_move and nothing else: the length
    /// byte stays where it is, a slot past it is refused, and a skill that was
    /// equipped is unequipped by the core when it leaves the list. Writing the id
    /// a slot already holds changes nothing and says so instead of marking the
    /// save dirty.
    pub fn palico_skill_assign(&mut self, palico: usize, index: usize, id: u8) -> bool {
        let slot = self.slot;
        let tab = self.palico_skill_tab();
        let list_name = palico_skill_tab_name(tab);
        let moves = tab == PALICO_TAB_MOVES;
        if index >= palico_skill_tab_rows(tab) {
            self.message = format!("{} の {} 番目は一覧の外です", list_name, index);
            return false;
        }
        let max = palico_skill_max_id(tab);
        if id == 0 || id > max {
            self.message = format!("{} の ID は 1..={} です", list_name, max);
            return false;
        }
        self.push_undo("オトモスキル");
        let Some(save) = self.save.as_mut() else {
            self.message = String::from("セーブが開かれていません");
            return false;
        };
        let Some(record) = crate::palico::roster(save, slot)
            .into_iter()
            .find(|p| p.index == palico)
        else {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        };
        if record.is_empty() {
            self.message = format!("枠 {} にオトモがいません", palico);
            return false;
        }
        let empty = if moves {
            crate::palico::MOVE_EMPTY
        } else {
            crate::palico::SKILL_EMPTY
        };
        let used = if moves {
            record.learned_move_slots()
        } else {
            record.learned_skill_slots()
        };
        let current = if moves {
            record.learned_moves.get(index).copied().unwrap_or(empty)
        } else {
            record.learned_skills.get(index).copied().unwrap_or(empty)
        };
        if current == id {
            self.message = format!(
                "{} の {} 番目はすでに {}（{}）です",
                list_name,
                index,
                id,
                palico_skill_id_name(tab, id)
            );
            return true;
        }
        let ok = if moves {
            crate::palico::set_move(save, slot, palico, index, id)
        } else {
            crate::palico::set_skill(save, slot, palico, index, id)
        };
        if !ok {
            self.message = format!(
                "{} の {} 番目には書けません（記録の長さ {}、ID は 1..={}）",
                list_name, index, used, max
            );
            return false;
        }
        self.dirty = true;
        self.message = format!(
            "{} の {} 番目を {}（{}）にしました",
            list_name,
            index,
            id,
            palico_skill_id_name(tab, id)
        );
        true
    }

    /// Keys of the skill / support move panel.
    ///
    /// [A] is the only key that writes a slot. [Y] swaps the two lists, [◀/▶]
    /// move the candidate by one and [L/R] by ten - the tables run to 93 and 56
    /// entries, so a key that steps ten is the difference between usable and not.
    /// Every other key is a cursor move or is swallowed while the panel owns the
    /// screen, so a press can never reach the roster underneath it.
    fn input_palico_skill(&mut self, key: Key) {
        let Some(mut state) = self.palico_skill else {
            return;
        };
        let tab = if state.tab == PALICO_TAB_MOVES {
            PALICO_TAB_MOVES
        } else {
            PALICO_TAB_SKILLS
        };
        let count = palico_skill_tab_rows(tab);
        let field = state.field.min(count - 1);
        state.tab = tab;
        state.field = field;
        match key {
            Key::Up => {
                state.field = (field + count - 1) % count;
                self.palico_skill = Some(state);
            }
            Key::Down => {
                state.field = (field + 1) % count;
                self.palico_skill = Some(state);
            }
            Key::Left | Key::Right => {
                self.palico_skill = Some(state);
                self.cycle_palico_skill_pick(if key == Key::Right { 1 } else { -1 });
            }
            Key::L | Key::R => {
                self.palico_skill = Some(state);
                self.cycle_palico_skill_pick(if key == Key::R { 10 } else { -10 });
            }
            Key::Y => {
                let next = if tab == PALICO_TAB_MOVES {
                    PALICO_TAB_SKILLS
                } else {
                    PALICO_TAB_MOVES
                };
                state.tab = next;
                state.field = 0;
                if let Some(p) = self.palico_skill_target() {
                    state.picks = palico_skill_picks(&p, next);
                }
                self.palico_skill = Some(state);
                self.message = format!("{} の一覧に切り替えました", palico_skill_tab_name(next));
            }
            Key::A => {
                let palico = state.palico;
                let pick = state.picks[field];
                self.palico_skill = Some(state);
                self.palico_skill_assign(palico, field, pick);
            }
            Key::B => self.close_palico_skill(),
            Key::X => {
                self.palico_skill = Some(state);
                self.request_write_save();
            }
            // Plus is the quit key again while the panel is open: on the list it
            // is what opens the panel, here it has no other job.
            Key::Plus => self.quit = true,
            // [-] is the equipment panel's key on the list under this one, so it
            // must not reach it while the skill panel is open; Z is ZR, which the
            // pad spends on the section bar and the PC build on the arc list.
            Key::Minus | Key::Z => {
                self.palico_skill = Some(state);
            }
        }
    }

    /// Move the cursor row's candidate by delta, wrapping inside the table.
    fn cycle_palico_skill_pick(&mut self, delta: i64) {
        let Some(mut state) = self.palico_skill else {
            return;
        };
        let tab = if state.tab == PALICO_TAB_MOVES {
            PALICO_TAB_MOVES
        } else {
            PALICO_TAB_SKILLS
        };
        let count = palico_skill_tab_rows(tab);
        let field = state.field.min(count - 1);
        let max = palico_skill_max_id(tab) as i64;
        let cur = (state.picks[field] as i64).clamp(1, max);
        let next = (cur - 1 + delta).rem_euclid(max) + 1;
        state.tab = tab;
        state.field = field;
        state.picks[field] = next as u8;
        self.palico_skill = Some(state);
        self.message = format!(
            "{} の {} 番目: 差し替え先 {}（{}）",
            palico_skill_tab_name(tab),
            field,
            next,
            palico_skill_id_name(tab, next as u8)
        );
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
                            self.push_undo("クエスト装備");
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

    /// Is one of the two set forms open? (A mode of the my-sets screen, not a
    /// screen.)
    ///
    /// Either form answers true: the console layer holds every key while one of
    /// them owns the panel, whichever tab of the list it was opened from.
    pub fn set_editor_open(&self) -> bool {
        self.set_edit.is_some() || self.item_set_edit.is_some()
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
        // One form at a time, for the same reason the item form says so.
        self.item_set_edit = None;
        self.item_set_armed = None;
        self.item_set_warnings.clear();
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
        if self.item_set_edit.is_some() {
            self.input_item_set_edit(key);
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
        self.push_undo("装備マイセット");
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
        self.push_undo("マイセットの部位解除");
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
        self.push_undo("マイセット名");
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

    // -----------------------------------------------------------------------
    // Item sets: the other half of the my-sets screen
    // -----------------------------------------------------------------------
    //
    // The item sets used to be list-only. This is their editor: the same shape
    // the equipment set form has - one row per entry, Left/Right walking the
    // candidates, [A] writing the row - plus the two operations a set exists
    // for: take the pouch in, put the set back out there.
    //
    // The pouch is not edited here (a screen of its own is the next step).
    // These two keys are the only place this form touches it, and both ask
    // twice, because both replace thirty-two entries at once.

    /// Is the item set form open?
    pub fn item_set_editor_open(&self) -> bool {
        self.item_set_edit.is_some()
    }

    /// The item set the form is looking at, re-read from the save.
    ///
    /// Re-read rather than cached, so the rows on screen are the bytes that are
    /// really stored. Only the pending entries of the form run ahead of the
    /// record, and only until [A] writes them.
    pub fn editing_item_set(&self) -> Option<crate::myset::ItemSet> {
        let e = self.item_set_edit?;
        self.item_sets().into_iter().nth(e.set)
    }

    /// The pending entry of one row of the item set form.
    pub fn item_set_pick(&self, row: usize) -> crate::myset::SetItem {
        self.item_set_edit
            .as_ref()
            .and_then(|e| e.pick.get(row).copied())
            .unwrap_or(crate::myset::SetItem { id: 0, count: 0 })
    }

    /// "回復薬 ×10" for a row of the item set form, "空き" for an empty one.
    pub fn item_set_row_label(&self, row: usize) -> String {
        let item = self.item_set_pick(row);
        if item.id == 0 {
            String::from("空き")
        } else {
            alloc::format!("{} ×{}", item_name_of(item.id), item.count)
        }
    }

    /// Open the item set form on one record of the item set array.
    ///
    /// The argument is an index into the twenty-four record array, not a row of
    /// the list the console draws: that list skips the empty records, so the
    /// caller maps its row back to this index.
    pub fn open_item_set_editor(&mut self, set: usize) -> bool {
        if self.save.is_none() {
            self.message = String::from("先にセーブを開いてください");
            return false;
        }
        if set >= crate::myset::ITEM_SETS {
            self.message = alloc::format!("アイテムセット {} は範囲外です", set + 1);
            return false;
        }
        let Some(rec) = self.item_sets().into_iter().nth(set) else {
            self.message = String::from("このアイテムセットは読み取れません");
            return false;
        };
        let mut pick = [crate::myset::SetItem { id: 0, count: 0 }; ITEM_SET_EDIT_ROWS];
        for (i, p) in pick.iter_mut().enumerate() {
            *p = rec.items.get(i).copied().unwrap_or(crate::myset::SetItem { id: 0, count: 0 });
        }
        let name = if rec.name.trim().is_empty() {
            String::from("(無題)")
        } else {
            rec.name.clone()
        };
        let used = rec.used_items().len();
        // One form at a time: the two editors belong to the same screen and both
        // hold a record, so opening one puts the other away.
        self.set_edit = None;
        self.set_clear_armed = None;
        self.set_warnings.clear();
        self.item_set_armed = None;
        self.item_set_edit = Some(ItemSetEdit {
            set,
            field: 0,
            pick,
        });
        let warned = self.refresh_item_set_warnings();
        self.message = alloc::format!(
            "アイテムセット #{:02} 「{}」を編集中 — {} 枠 / 警告 {} 件",
            set + 1,
            name,
            used,
            warned
        );
        true
    }

    /// Leave the item set form and go back to the list.
    ///
    /// Only [A] ever writes an entry, so leaving cannot lose an edit that was
    /// already made: the bytes are in the open save and reach the file with the
    /// save key pressed on the list.
    pub fn close_item_set_editor(&mut self) {
        self.item_set_edit = None;
        self.item_set_armed = None;
        self.item_set_warnings.clear();
        self.message = String::from("マイセット一覧に戻りました（[X] でセーブ保存）");
    }

    /// Re-read myset::item_set_warnings for the set on screen.
    ///
    /// Returns how many there are. Called when the form opens and after every
    /// write, never from the renderer.
    pub fn refresh_item_set_warnings(&mut self) -> usize {
        let Some(e) = self.item_set_edit else {
            self.item_set_warnings.clear();
            return 0;
        };
        let slot = self.slot;
        self.item_set_warnings = match &self.save {
            Some(s) => crate::myset::item_set_warnings(s, slot, e.set),
            None => Vec::new(),
        };
        self.item_set_warnings.len()
    }

    /// How many entries of the pouch hold something.
    fn pouch_used(&self) -> usize {
        match &self.save {
            Some(s) => crate::myset::pouch(s, self.slot)
                .iter()
                .filter(|i| i.id != 0)
                .count(),
            None => 0,
        }
    }

    /// The entries the item form's Left / Right walks.
    ///
    /// An empty entry comes first - a set the game wrote can hold holes and
    /// there is otherwise no way back to one - then every item the item box
    /// holds, in box order and without repeating an id. The box is where the
    /// character's items are; a set naming something the box does not hold
    /// could not be registered again.
    fn item_set_candidates(&self) -> Vec<crate::myset::SetItem> {
        let mut out = Vec::new();
        out.push(crate::myset::SetItem { id: 0, count: 0 });
        let Some(items) = self.save.as_ref().and_then(|s| s.read_item_box(self.slot)) else {
            return out;
        };
        for it in items.iter() {
            if it.id == 0 || it.count == 0 {
                continue;
            }
            if out.iter().any(|c| c.id == it.id) {
                continue;
            }
            out.push(crate::myset::SetItem {
                id: it.id,
                count: it.count as u16,
            });
        }
        out
    }

    /// Walk the candidates for one row of the item set form.
    fn move_item_set_pick(&mut self, row: usize, d: i64) {
        if row >= ITEM_SET_EDIT_ROWS {
            return;
        }
        let cands = self.item_set_candidates();
        if cands.len() <= 1 {
            self.message = String::from("アイテムBOXが空です（先にアイテムBOXへ入れてください）");
            return;
        }
        let Some(mut e) = self.item_set_edit else {
            return;
        };
        let n = cands.len() as i64;
        let cur = e.pick[row];
        let next = match cands.iter().position(|c| *c == cur) {
            Some(p) => ((p as i64 + d).rem_euclid(n)) as usize,
            // What the record holds is not in the box any more: start from an
            // end rather than pretending it is the first candidate.
            None => {
                if d > 0 {
                    0
                } else {
                    (n - 1) as usize
                }
            }
        };
        e.pick[row] = cands[next];
        self.item_set_edit = Some(e);
        let label = self.item_set_row_label(row);
        self.message = alloc::format!(
            "[枠 {:02}] {} — [A] で決定（候補 {} / {} 番目）",
            row + 1,
            label,
            cands.len(),
            next + 1
        );
    }

    /// Write the row's pending entry into the set.
    fn commit_item_set_row(&mut self) {
        let Some(e) = self.item_set_edit else {
            return;
        };
        if e.field >= ITEM_SET_EDIT_ROWS {
            return;
        }
        let row = e.field;
        let item = e.pick[row];
        let slot = self.slot;
        let set = e.set;
        self.push_undo("アイテムマイセット");
        let wrote = match &mut self.save {
            Some(s) => {
                if item.id == 0 {
                    crate::myset::clear_item(s, slot, set, row)
                } else {
                    crate::myset::set_item(s, slot, set, row, item.id, item.count)
                }
            }
            None => false,
        };
        if wrote {
            self.dirty = true;
            let label = self.item_set_row_label(row);
            let n = self.refresh_item_set_warnings();
            self.message = if item.id == 0 {
                alloc::format!("枠 {:02} を空けました（警告 {} 件）", row + 1, n)
            } else {
                alloc::format!(
                    "枠 {:02} に「{}」を入れました（警告 {} 件）",
                    row + 1,
                    label,
                    n
                )
            };
        } else {
            self.message = alloc::format!(
                "枠 {:02} には書き込めません（値が範囲外か、セーブが開かれていません）",
                row + 1
            );
        }
    }

    /// First or second press of [Y]: copy the pouch into the set.
    ///
    /// myset::copy_from_pouch rewrites all thirty-two entries, so the first
    /// press only says what would be replaced; the second one writes.
    fn item_set_from_pouch(&mut self) {
        let Some(e) = self.item_set_edit else {
            return;
        };
        if self.item_set_armed != Some(ItemSetOp::FromPouch) {
            self.item_set_armed = Some(ItemSetOp::FromPouch);
            let held = self
                .editing_item_set()
                .map(|r| r.used_items().len())
                .unwrap_or(0);
            self.message = alloc::format!(
                "ポーチの内容でセット #{:02} の 32 枠を置き換えます（いま {} 枠 → ポーチ {} 枠）。もう一度 [Y] で実行",
                e.set + 1,
                held,
                self.pouch_used()
            );
            return;
        }
        self.item_set_armed = None;
        let slot = self.slot;
        let set = e.set;
        let readable = match &self.save {
            Some(s) => !crate::myset::pouch(s, slot).is_empty(),
            None => false,
        };
        if !readable {
            self.message = String::from("ポーチを読めないため取り込めませんでした");
            return;
        }
        self.push_undo("ポーチからセットへ");
        let n = match &mut self.save {
            Some(s) => crate::myset::copy_from_pouch(s, slot, set),
            None => 0,
        };
        self.dirty = true;
        self.reload_item_set_picks(set);
        let w = self.refresh_item_set_warnings();
        self.message = alloc::format!("ポーチから {} 枠を取り込みました（警告 {} 件）", n, w);
    }

    /// First or second press of [-]: copy the set into the pouch.
    ///
    /// myset::apply_to_pouch replaces the pouch, so the first press names what
    /// would be lost; the second one writes. A record the game left unused is
    /// refused by the API itself, which is why a zero result is reported as a
    /// refusal rather than as success.
    fn item_set_to_pouch(&mut self) {
        let Some(e) = self.item_set_edit else {
            return;
        };
        if self.item_set_armed != Some(ItemSetOp::ToPouch) {
            self.item_set_armed = Some(ItemSetOp::ToPouch);
            let held = self
                .editing_item_set()
                .map(|r| r.used_items().len())
                .unwrap_or(0);
            self.message = alloc::format!(
                "ポーチをセット #{:02} の内容で置き換えます（ポーチ {} 枠 → {} 枠）。もう一度 [-] で実行",
                e.set + 1,
                self.pouch_used(),
                held
            );
            return;
        }
        self.item_set_armed = None;
        let slot = self.slot;
        let set = e.set;
        self.push_undo("セットをポーチへ");
        let n = match &mut self.save {
            Some(s) => crate::myset::apply_to_pouch(s, slot, set),
            None => 0,
        };
        if n == 0 {
            self.message = String::from(
                "適用できませんでした（セットが空か、セーブが開かれていません）",
            );
            return;
        }
        self.dirty = true;
        self.message =
            alloc::format!("ポーチをセットの {} 枠で置き換えました（[X] でセーブ保存）", n);
    }

    /// Re-seed the pending entries from the record, after a write that changed
    /// more than one row.
    fn reload_item_set_picks(&mut self, set: usize) {
        let Some(rec) = self.item_sets().into_iter().nth(set) else {
            return;
        };
        let Some(mut e) = self.item_set_edit else {
            return;
        };
        for (i, p) in e.pick.iter_mut().enumerate() {
            *p = rec.items.get(i).copied().unwrap_or(crate::myset::SetItem { id: 0, count: 0 });
        }
        if e.field >= ITEM_SET_EDIT_ROWS {
            e.field = ITEM_SET_EDIT_ROWS - 1;
        }
        self.item_set_edit = Some(e);
    }

    /// One line for the footer on the item set form.
    pub fn item_set_row_hint(&self) -> String {
        let Some(e) = self.item_set_edit else {
            return String::from("アイテムセットを選んで [A]");
        };
        alloc::format!(
            "枠 {:02} {} — [◀/▶] アイテムBOX を巡回 / [A] 決定 / [Y] ポーチから取込 / [-] 2回でポーチへ適用",
            e.field + 1,
            self.item_set_row_label(e.field)
        )
    }

    /// The item set form's keys.
    fn input_item_set_edit(&mut self, key: Key) {
        let Some(mut e) = self.item_set_edit else {
            return;
        };
        // Both two-press operations belong to the row the user is looking at,
        // so any other key drops the pending one before it does its own work.
        if !matches!(key, Key::Y | Key::Minus) {
            self.item_set_armed = None;
        }
        match key {
            Key::Up => e.field = (e.field + ITEM_SET_EDIT_ROWS - 1) % ITEM_SET_EDIT_ROWS,
            Key::Down => e.field = (e.field + 1) % ITEM_SET_EDIT_ROWS,
            Key::Left | Key::Right => {
                self.item_set_edit = Some(e);
                self.move_item_set_pick(e.field, if key == Key::Right { 1 } else { -1 });
                return;
            }
            Key::A => {
                self.item_set_edit = Some(e);
                self.commit_item_set_row();
                return;
            }
            Key::Y => {
                self.item_set_edit = Some(e);
                self.item_set_from_pouch();
                return;
            }
            Key::Minus => {
                self.item_set_edit = Some(e);
                self.item_set_to_pouch();
                return;
            }
            Key::B => {
                self.item_set_edit = Some(e);
                self.close_item_set_editor();
                return;
            }
            Key::Plus => self.quit = true,
            _ => {}
        }
        self.item_set_edit = Some(e);
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
        self.push_undo("装備の削除");
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
        self.push_undo("装備の編集");
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
        self.push_undo("装備の新規作成");
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
                        self.push_undo("クエスト装備");
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
        self.push_undo("クエストの追加");
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
        self.push_undo("DLC クエストの一括導入");
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
        self.push_undo("クエストの導入と削除");
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
        self.push_undo("クエストの削除");
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
        self.push_undo("クエストの書き込み");
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

        // The palico equipment panel is a mode of two screens, so it is drawn
        // before the screen match: while it is open it owns the output and the
        // list underneath keeps its own lines to itself.
        if let Some(state) = self.palico_equip {
            let (who, roster_no) = match self.palico_equip_target() {
                Some(p) => (p.name, p.index + 1),
                None => (String::from("?"), state.palico + 1),
            };
            push(
                &mut out,
                format!("オトモ装備 — {}（ロスター #{}）", who, roster_no),
                false,
                false,
            );
            for row in self.palico_equip_rows() {
                push(
                    &mut out,
                    format!(
                        "  {}  現在: {}  → {}",
                        palico_part_name(row.part),
                        row.worn_text(),
                        row.pick_text()
                    ),
                    true,
                    row.part == state.field,
                );
            }
            push(
                &mut out,
                format!("  {}", PALICO_EQUIP_NAME_NOTE),
                false,
                false,
            );
            push(
                &mut out,
                String::from("  Up/Down 部位  Left/Right 候補  A 装着  - x2 解除  X save  B back"),
                false,
                false,
            );
            return out;
        }

        // The skill / support move panel is the other mode of the roster
        // screen and owns the output while it is open, the same way.
        if let Some(state) = self.palico_skill {
            let tab = self.palico_skill_tab();
            let (used, rows) = self.palico_skill_slots();
            let (who, roster_no) = match self.palico_skill_target() {
                Some(p) => (p.name, p.index + 1),
                None => (String::from("?"), state.palico + 1),
            };
            push(
                &mut out,
                format!(
                    "オトモ{} — {}（ロスター #{}）",
                    palico_skill_tab_name(tab),
                    who,
                    roster_no
                ),
                false,
                false,
            );
            push(
                &mut out,
                format!("  {}", self.palico_skill_tendencies()),
                false,
                false,
            );
            push(
                &mut out,
                format!(
                    "  {} の修得枠 {} 使用 / {} 枠{}",
                    palico_skill_tab_name(tab),
                    used,
                    rows,
                    if used < rows { "" } else { "（いっぱい）" }
                ),
                false,
                false,
            );
            for row in self.palico_skill_rows() {
                // A slot past the length byte is the fill the game leaves there:
                // it is listed, and marked so nobody walks down to it to write.
                let mark = if row.counted { " " } else { "x" };
                let arrow = if row.index == state.field {
                    format!("  → {}", row.pick_text())
                } else {
                    String::new()
                };
                push(
                    &mut out,
                    format!("  {} {:>2}  {}{}", mark, row.index, row.current_text(), arrow),
                    true,
                    row.index == state.field,
                );
            }
            for warning in self.palico_skill_warnings().iter().take(2) {
                push(&mut out, format!("  !! {}", warning), false, false);
            }
            push(
                &mut out,
                String::from("  Up/Down 枠  Left/Right 候補  L/R ±10  Y タブ  A 差し替え"),
                false,
                false,
            );
            push(
                &mut out,
                String::from("  X save   B back   (x = 修得枠の外)"),
                false,
                false,
            );
            return out;
        }

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
                    // The game's name of the kind comes first: it is the label
                    // the box screen shows, and the one the report said was
                    // missing.
                    let kind = self
                        .save
                        .as_ref()
                        .and_then(|s| s.charm_kind_name(self.slot, c.index))
                        .unwrap_or("—");
                    push(
                        &mut out,
                        format!("  #{:<5} {:<12} {}", c.index, kind, c.summary()),
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
            Screen::Guild if self.guild_view() == GUILD_TAB_ARENA => {
                // The same seventeen rows the canvas draws: one row per arena,
                // one ten-cell column per rank, the cursor cell bracketed. Every
                // cell is padded to a fixed width so a "10:55.35" cannot push
                // the columns after it out of line.
                push(
                    &mut out,
                    alloc::format!("ARENA LOG   slot {}   view 2/2 闘技大会", self.slot + 1),
                    false,
                    false,
                );
                if self.save.is_none() {
                    push(&mut out, String::from("  no save"), false, false);
                    return out;
                }
                let grid = self.arena_grid();
                let (cur_row, cur_rank) = self.arena_selected();
                let mut head = alloc::format!(
                    "  {} {}",
                    pad_cells("#", 2),
                    pad_cells("名前", 20)
                );
                for r in 0..ARENA_COLUMNS {
                    head.push_str(&pad_cells_left(arena_rank_name(r), 10));
                }
                push(&mut out, head, false, false);
                for a in 0..ARENA_ROWS {
                    let mut line = alloc::format!(
                        "{} {} {}",
                        if a == cur_row { ">" } else { " " },
                        pad_cells(&alloc::format!("{:02}", a + 1), 2),
                        pad_cells(arena_name(a), 20)
                    );
                    for r in 0..ARENA_COLUMNS {
                        let centis = grid.get(a).and_then(|row| row.get(r)).copied().unwrap_or(0);
                        let text = if centis == 0 {
                            alloc::string::String::from("-")
                        } else {
                            arena_clock(centis)
                        };
                        let padded = pad_cells_left(&text, 8);
                        if a == cur_row && r == cur_rank {
                            line.push_str(&alloc::format!("[{}]", padded));
                        } else {
                            line.push_str(&alloc::format!(" {} ", padded));
                        }
                    }
                    push(&mut out, line, true, a == cur_row);
                }
                push(
                    &mut out,
                    alloc::format!(
                        "  {} {} : {}   ［{}］{}",
                        arena_name(cur_row),
                        arena_rank_name(cur_rank),
                        arena_clock(self.arena_centis()),
                        arena_part_name(self.arena_part.min(ARENA_PARTS - 1)),
                        if self.arena_edit { " 編集中" } else { "" }
                    ),
                    false,
                    false,
                );
                push(
                    &mut out,
                    String::from(
                        "  Up/Down アリーナ  Left/Right 順位  A 編集  Y 直接入力  X save  B カードへ",
                    ),
                    false,
                    false,
                );
                push(
                    &mut out,
                    String::from(
                        "  編集中: Left/Right 分・秒・1/100  Up/Down 増減  A 終了  Y 直接入力",
                    ),
                    false,
                    false,
                );
            }
            Screen::Guild => {
                push(
                    &mut out,
                    alloc::format!("GUILD CARD   slot {}   view 1/2", self.slot + 1),
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
                // The item set form, the twin of the equipment one above.
                if let Some(e) = self.item_set_edit {
                    push(
                        &mut out,
                        alloc::format!("MY SETS   slot {}   ITEM EDIT", self.slot + 1),
                        false,
                        false,
                    );
                    let name = self
                        .editing_item_set()
                        .map(|r| r.name)
                        .unwrap_or_default();
                    push(
                        &mut out,
                        alloc::format!("  SET #{:02}  {}", e.set + 1, clip_text(&name, 24)),
                        false,
                        false,
                    );
                    for row in 0..ITEM_SET_EDIT_ROWS {
                        let item = e.pick[row];
                        let label = if item.id == 0 {
                            String::from("(空き)")
                        } else {
                            alloc::format!("{} x{}", item_name_of(item.id), item.count)
                        };
                        // Kept short: this renderer is the 80 column
                        // console, and the row already holds a name and a
                        // count.
                        let armed = if self.item_set_armed == Some(ItemSetOp::FromPouch) {
                            "  << [Y] で取込確定"
                        } else if self.item_set_armed == Some(ItemSetOp::ToPouch) {
                            "  << [-] で適用確定"
                        } else {
                            ""
                        };
                        push(
                            &mut out,
                            alloc::format!("  {:>2} {}{}", row + 1, pad_cells(&label, 30), armed),
                            true,
                            row == e.field,
                        );
                    }
                    push(
                        &mut out,
                        alloc::format!("  警告 {} 件", self.item_set_warnings.len()),
                        false,
                        false,
                    );
                    if self.item_set_warnings.is_empty() {
                        push(&mut out, String::from("    (なし)"), false, false);
                    } else {
                        for line in self.item_set_warnings.iter().take(4) {
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
                            "  Up/Down 枠  Left/Right アイテム  A 決定  Y ポーチから取込×2  - ポーチへ適用×2  B 一覧",
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
                    String::from("  A 編集(アイテム/装備セット)   B back"),
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
                push(
                    &mut out,
                    String::from("  Y 項目切替   - 装備/解除(2回)   X save   B back"),
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
                        String::from("種類"),
                        match crate::charm::kind_limits(e.kind) {
                            // The measured ceiling of this kind, not the
                            // grade's: they agree, but the kind is what the
                            // user just turned and what the screen names.
                            Some(l) => format!(
                                "{}（レア度 {} / {}）",
                                l.name(),
                                l.rarity,
                                l.short()
                            ),
                            None => format!(
                                "{}（レア度 {} / 最大 S{}）",
                                crate::charm::kind_name(e.kind).unwrap_or("?"),
                                e.grade.rarity(),
                                e.grade.max_slots()
                            ),
                        },
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
                // The measured ceiling of the kind on the form. It is a
                // constant, so it is printed even when the gamedata tables are
                // compiled out; the per-skill ranges below need the tables.
                if let Some(l) = crate::charm::kind_limits(e.kind) {
                    push(&mut out, format!("  {}", l.summary()), false, false);
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
                        // The kind's own ceiling is the tighter of the two,
                        // so it is what the "allowed" text reports.
                        let high = crate::charm::kind_limits(e.kind)
                            .and_then(|l| l.max_points(second))
                            .map(|c| r.max.min(c))
                            .unwrap_or(r.max);
                        let line = if r.min == 0 && r.max == 0 {
                            format!("  !! {} {} cannot go on this type", label, name)
                        } else if pts < r.min || pts > high {
                            format!(
                                "  !! {} {} is {:+}, allowed {:+}..{:+}",
                                label, name, pts, r.min, high
                            )
                        } else {
                            format!(
                                "     {} {} allowed {:+}..{:+}",
                                label, name, r.min, high
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
                    String::from("  A add/remove   - 装備/解除(2回)   L R language   X write save   B back"),
                    false,
                    false,
                );
            }
            Screen::Csv => {
                push(
                    &mut out,
                    alloc::format!("CSV   slot {}   {}", self.slot + 1, CSV_DIR),
                    false,
                    false,
                );
                if self.csv_files.is_empty() {
                    push(
                        &mut out,
                        String::from("  (CSV ファイルがありません - Y で書き出します)"),
                        false,
                        false,
                    );
                }
                for (i, path) in self.csv_files.iter().enumerate() {
                    let armed = if self.csv_armed == Some(i) {
                        "  << [A] で適用確定"
                    } else {
                        ""
                    };
                    push(
                        &mut out,
                        alloc::format!(
                            "  [{:>2}] {}  {}  {}",
                            i,
                            pad_cells(csv_kind_label(path), 8),
                            pad_cells(&file_name_of(path), 34),
                            armed
                        ),
                        true,
                        i == self.csv_cursor,
                    );
                }
                if !self.csv_report.is_empty() {
                    push(&mut out, String::from(" 前回の結果"), false, false);
                    for line in self.csv_report.iter().take(3) {
                        push(
                            &mut out,
                            alloc::format!("    {}", clip_text(line, 64)),
                            false,
                            false,
                        );
                    }
                }
                push(
                    &mut out,
                    String::from("  Up/Down 行  A 適用(2回)  Y 書き出し  - 再読込  B back"),
                    false,
                    false,
                );
            }
            Screen::Progress => {
                let tab = self.progress_view();
                match tab {
                    PROGRESS_AWARDS => {
                        let (have, total) = self.progress_awards();
                        push(
                            &mut out,
                            alloc::format!("進捗 [勲章]  取得 {} / {}", have, total),
                            false,
                            false,
                        );
                    }
                    PROGRESS_REQUESTS => push(
                        &mut out,
                        alloc::format!(
                            "進捗 [依頼]  立っているビット {} / {}",
                            self.progress_request_count(),
                            requests::FLAGS
                        ),
                        false,
                        false,
                    ),
                    _ => {
                        let (cleared, seen, failed) = self.progress_quest_counts();
                        let (page, pages) = self.progress_quest_page();
                        push(
                            &mut out,
                            alloc::format!(
                                "進捗 [クエスト]  クリア {} / 既読 {} / 失敗 {}   p.{}/{}",
                                cleared,
                                seen,
                                failed,
                                page,
                                pages
                            ),
                            false,
                            false,
                        );
                    }
                }
                // A window of rows around the cursor: 1508 of anything does not
                // fit on the console, and the same window is what the canvas
                // build pages through.
                let visible = 14usize;
                let len = self.progress_len();
                let cur = self.progress_cursor();
                let start = if len <= visible {
                    0
                } else {
                    cur.saturating_sub(visible / 2).min(len - visible)
                };
                for row in start..(start + visible).min(len) {
                    let text = match tab {
                        PROGRESS_AWARDS => {
                            let held = match &self.save {
                                Some(save) => awards::has(save, self.slot, row).unwrap_or(false),
                                None => false,
                            };
                            let name = self.award_name(row);
                            if name.is_empty() {
                                alloc::format!(
                                    "  勲章 {:03}  {}",
                                    row + 1,
                                    if held { "取得" } else { "未取得" }
                                )
                            } else {
                                alloc::format!(
                                    "  勲章 {:03}  {:6}  {}",
                                    row + 1,
                                    if held { "取得" } else { "未取得" },
                                    name
                                )
                            }
                        }
                        PROGRESS_REQUESTS => {
                            let flag = self.request_flag_at(row).unwrap_or(0);
                            let on = match &self.save {
                                Some(save) => {
                                    requests::get(save, self.slot, flag).unwrap_or(false)
                                }
                                None => false,
                            };
                            alloc::format!(
                                "  フラグ {:04}  {}  {}",
                                flag,
                                if on { "ON " } else { "OFF" },
                                self.request_flag_label(flag)
                            )
                        }
                        _ => {
                            let (cleared, seen, failed) = self.progress_quest_state(row);
                            alloc::format!(
                                "  クエスト {:04}  {}  {}  {}",
                                row + 1,
                                if cleared { "クリア" } else { "------" },
                                if seen { "既読" } else { "----" },
                                if failed { "失敗" } else { "----" }
                            )
                        }
                    };
                    push(&mut out, clip_text(&text, 76), true, row == cur);
                }
                if start > 0 {
                    push(
                        &mut out,
                        alloc::format!("  … 上に {} 行", start),
                        false,
                        false,
                    );
                }
                // The facts a write needs, in the order they matter.
                match tab {
                    PROGRESS_AWARDS => {
                        if self.progress_awards_agree() == Some(false) {
                            push(
                                &mut out,
                                String::from(
                                    "  警告: 勲章の3つのコピーが一致しません（書き込みは3つ同時です）",
                                ),
                                false,
                                false,
                            );
                        }
                    }
                    PROGRESS_REQUESTS => {
                        let outside = self
                            .progress_request_count()
                            .saturating_sub(self.progress_request_band_count());
                        push(
                            &mut out,
                            alloc::format!(
                                "  この帯の外にも {} ビット立っています（依頼以外のイベント）",
                                outside
                            ),
                            false,
                            false,
                        );
                    }
                    _ => {
                        push(
                            &mut out,
                            String::from("  クリアを立てると既読も立ちます（Y は既読だけ）"),
                            false,
                            false,
                        );
                        let broken = self.progress_clears_without_seen();
                        if broken > 0 {
                            push(
                                &mut out,
                                alloc::format!(
                                    "  クリア済みなのに未読のクエストが {} 件あります（修正はしません）",
                                    broken
                                ),
                                false,
                                false,
                            );
                        }
                    }
                }
                let keys = match tab {
                    PROGRESS_AWARDS => {
                        "  Up/Down 行  Left/Right ±50  A 取得切替  L/R タブ  X 保存  B back"
                    }
                    PROGRESS_REQUESTS => {
                        "  Up/Down 行  Left/Right ±50  A フラグ切替  L/R タブ  X 保存  B back"
                    }
                    _ => {
                        "  Up/Down 行  Left/Right ±50  A クリア  Y 既読  L/R タブ  X 保存  B back"
                    }
                };
                push(&mut out, String::from(keys), false, false);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palico;
    use crate::palicogear;

    /// A blank Switch save with the given roster names, one record of each
    /// palico gear kind, and an editor that has it open.
    ///
    /// Nothing here comes from a real save: the box, the roster and the worn
    /// list are all written through the documented writers, so the test runs on
    /// any machine and still exercises the same bytes.
    fn editor_with_palico_gear(names: &[&str]) -> (Editor, [usize; 3]) {
        let mut save = crate::save::empty_switch_save();
        for (i, name) in names.iter().enumerate() {
            assert!(palico::set_name(&mut save, 0, i, name));
        }
        let weapon = palicogear::create(&mut save, 0, palicogear::WEAPON, 431, 1)
            .expect("a blank weapon record");
        let head =
            palicogear::create(&mut save, 0, palicogear::HEAD, 464, 1).expect("a blank head record");
        let body =
            palicogear::create(&mut save, 0, palicogear::BODY, 465, 1).expect("a blank body record");
        // A blank save holds zeros in the worn list, but the game writes
        // 0xFFFF for "nothing worn": with zeros there every palico would claim
        // to wear box record 0. Put the real empty value in all three slots.
        let base = save.slot_base(0).expect("slot base") as usize;
        for palico in 0..palico::SLOTS {
            for part in 0..palico::EQUIP_SLOTS {
                let at =
                    base + palico::OFF_ROSTER + palico * palico::RECORD + palico::EQUIP + part * 2;
                save.body_mut()[at..at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
            }
        }
        let mut ed = Editor::new();
        ed.open_save_bytes("C:/mhnx/test/system", &save.to_bytes());
        assert!(ed.save.is_some(), "{}", ed.message);
        (ed, [weapon, head, body])
    }

    #[test]
    fn the_roster_panel_shows_three_rows_and_offers_only_their_kind() {
        let (mut ed, gear) = editor_with_palico_gear(&["テスト"]);
        ed.screen = Screen::Roster;
        ed.roster_cursor = 0;
        ed.input(Key::Minus);
        assert!(ed.palico_equip_open(), "{}", ed.message);
        // A mode, not a screen: the list underneath is still the roster.
        assert_eq!(ed.screen, Screen::Roster);
        assert_eq!(ed.palico_equip.map(|s| s.palico), Some(0));

        let rows = ed.palico_equip_rows();
        assert_eq!(rows.len(), PALICO_EQUIP_ROWS);
        assert_eq!(PALICO_EQUIP_ROWS, palico::EQUIP_SLOTS);
        for (part, row) in rows.iter().enumerate() {
            assert_eq!(row.part, part);
            // Nothing is worn yet, and a part with nothing worn starts on the
            // first record of its kind so [A] is usable without moving.
            assert_eq!(row.worn, PALICO_EQUIP_NONE);
            assert_eq!(row.worn_text(), "なし");
            assert_eq!(row.pick as usize, gear[part]);
            assert_eq!(row.pick_id, [431u16, 464, 465][part]);

            // Only records of that kind are offered, and the box holds one.
            let candidates = ed.palico_equip_candidates(part);
            assert_eq!(candidates, alloc::vec![gear[part] as u16]);
            let save = ed.save.as_ref().expect("save");
            let eq = palicogear::decode_one(save, 0, gear[part]).expect("decodes");
            assert_eq!(eq.kind, palicogear::KINDS[part]);
        }

        // Walking the rows is not a write.
        assert!(!ed.dirty);
        ed.input(Key::Down);
        ed.input(Key::Down);
        ed.input(Key::Up);
        assert!(!ed.dirty);
        assert_eq!(ed.palico_equip.map(|s| s.field), Some(1));
    }

    #[test]
    fn assigning_a_row_writes_only_the_roster_slot() {
        let (mut ed, gear) = editor_with_palico_gear(&["テスト"]);
        ed.screen = Screen::Roster;
        ed.roster_cursor = 0;
        ed.input(Key::Minus);
        assert!(ed.palico_equip_open(), "{}", ed.message);
        let base = ed
            .save
            .as_ref()
            .and_then(|s| s.slot_base(0))
            .expect("slot base") as usize;
        let before = ed.save.as_ref().expect("save").body().to_vec();
        ed.dirty = false;

        // Row 0 is the weapon and already starts on the only weapon record.
        ed.input(Key::A);
        assert!(ed.dirty, "an assignment has to mark the save dirty");
        let roster = palico::roster(ed.save.as_ref().expect("save"), 0);
        assert_eq!(roster[0].equip[0], gear[0] as u16);
        assert_eq!(roster[0].equip[1], PALICO_EQUIP_NONE);
        assert_eq!(roster[0].equip[2], PALICO_EQUIP_NONE);

        // Exactly the two bytes of part 0 moved, and no box record was rewritten.
        let after = ed.save.as_ref().expect("save").body().to_vec();
        let at = base + palico::OFF_ROSTER + palico::EQUIP;
        let mut moved = 0usize;
        for (off, (a, b)) in before.iter().zip(after.iter()).enumerate() {
            if a != b {
                assert!(
                    off >= at && off < at + 2,
                    "byte 0x{:X} changed, outside the worn index",
                    off
                );
                moved += 1;
            }
        }
        assert_eq!(moved, 2);
        let eq = palicogear::decode_one(ed.save.as_ref().expect("save"), 0, gear[0])
            .expect("still there");
        assert_eq!((eq.kind, eq.id), (palicogear::WEAPON, 431));

        // A record of another kind is refused, and a refusal writes nothing.
        ed.dirty = false;
        assert!(!ed.palico_equip_assign(0, 0, gear[1] as u16), "{}", ed.message);
        assert!(ed.message.contains("なので"), "{}", ed.message);
        assert!(!ed.dirty);
        assert_eq!(
            palico::roster(ed.save.as_ref().expect("save"), 0)[0].equip[0],
            gear[0] as u16
        );

        // Past the end of the box, past the last part and past the last record
        // there is nothing to write.
        assert!(!ed.palico_equip_assign(0, 0, palicogear::PALICO_EQUIP_COUNT as u16));
        assert!(!ed.palico_equip_assign(0, PALICO_EQUIP_ROWS, gear[0] as u16));
        assert!(!ed.palico_equip_assign(palico::SLOTS, 0, gear[0] as u16));
        assert!(!ed.dirty);
    }

    #[test]
    fn releasing_a_part_takes_two_presses() {
        let (mut ed, gear) = editor_with_palico_gear(&["テスト"]);
        ed.screen = Screen::Roster;
        ed.roster_cursor = 0;
        ed.input(Key::Minus);
        assert!(ed.palico_equip_open(), "{}", ed.message);
        ed.input(Key::A);
        let weapon = gear[0] as u16;
        assert_eq!(
            palico::roster(ed.save.as_ref().expect("save"), 0)[0].equip[0],
            weapon
        );
        ed.dirty = false;

        // The first press only names what would go.
        ed.input(Key::Minus);
        assert!(!ed.dirty, "one press must not write");
        assert_eq!(ed.palico_equip.and_then(|s| s.clear_armed), Some(0));
        assert!(ed.message.contains("もう一度"), "{}", ed.message);
        assert_eq!(
            palico::roster(ed.save.as_ref().expect("save"), 0)[0].equip[0],
            weapon
        );

        // Any other key drops the pending release.
        ed.input(Key::Down);
        assert_eq!(ed.palico_equip.and_then(|s| s.clear_armed), None);
        assert!(!ed.dirty);

        // Two presses in a row clear the part to 0xFFFF.
        ed.input(Key::Up);
        ed.input(Key::Minus);
        ed.input(Key::Minus);
        assert!(ed.dirty, "{}", ed.message);
        assert_eq!(
            palico::roster(ed.save.as_ref().expect("save"), 0)[0].equip[0],
            PALICO_EQUIP_NONE
        );

        // A part that is already empty says so and writes nothing.
        ed.dirty = false;
        ed.input(Key::Down);
        ed.input(Key::Down);
        ed.input(Key::Minus);
        assert!(!ed.dirty);
        assert!(ed.message.contains("すでに外れています"), "{}", ed.message);
    }

    #[test]
    fn the_download_list_opens_the_roster_twin() {
        let mut save = crate::save::empty_switch_save();
        let ids = dlcats::all_ids();
        let id = *ids.first().expect("the catalogue has entries");
        assert!(
            save.install_dlc_palico(dlcats::Lang::Jp, id).is_ok(),
            "the blank save has a free download slot"
        );
        // The game copies the download record into the roster, so the name is
        // the one thing the two layouts share. Write that name into a roster
        // record and the panel must find it.
        let name = save
            .dlc_palicoes()
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| c.name)
            .expect("the record just installed");
        assert!(palico::set_name(&mut save, 0, 5, &name));

        let mut ed = Editor::new();
        ed.open_save_bytes("C:/mhnx/test/system", &save.to_bytes());
        ed.screen = Screen::Palico;
        ed.palico_cursor = ids.iter().position(|v| *v == id).expect("in the list");
        ed.input(Key::Minus);
        assert!(ed.palico_equip_open(), "{}", ed.message);
        assert_eq!(ed.palico_equip_target().map(|p| p.index), Some(5));

        // An entry that was never installed has no roster twin to write to.
        ed.close_palico_equip();
        let other = ids.iter().copied().find(|v| *v != id).expect("a second id");
        ed.palico_cursor = ids.iter().position(|v| *v == other).expect("pos");
        ed.input(Key::Minus);
        assert!(!ed.palico_equip_open());
        assert!(ed.message.contains("導入"), "{}", ed.message);
    }

    #[test]
    fn the_panel_names_the_record_and_says_the_names_are_missing() {
        let (mut ed, _gear) = editor_with_palico_gear(&["テスト"]);
        ed.screen = Screen::Roster;
        ed.roster_cursor = 0;
        ed.input(Key::Minus);
        assert!(ed.palico_equip_open(), "{}", ed.message);
        let rows = ed.palico_equip_rows();
        assert_eq!(rows[0].pick_text(), format!("枠 {} / ID 431", rows[0].pick));
        assert_eq!(rows[0].worn_text(), "なし");
        assert_eq!(palico_part_name(0), "武器");
        assert_eq!(palico_part_name(1), "頭");
        assert_eq!(palico_part_name(2), "胴");
        assert_eq!(palico_part_name(3), "?");

        // The console output carries the three rows, the note about the
        // missing names, and nothing else selectable.
        ed.screen = Screen::Roster;
        let lines = ed.lines();
        assert!(
            lines.iter().any(|l| l.text.contains(PALICO_EQUIP_NAME_NOTE)),
            "the note line is missing"
        );
        assert_eq!(
            lines.iter().filter(|l| l.selectable).count(),
            PALICO_EQUIP_ROWS
        );
        assert_eq!(lines.iter().filter(|l| l.cursor).count(), 1);

        // The panel's row count is the worn list's own number, not a new one.
        assert_eq!(PALICO_PART_NAMES.len(), palico::EQUIP_SLOTS);
        assert_eq!(palicogear::KINDS.len(), PALICO_EQUIP_ROWS);
    }

    /// A blank Switch save with one palico whose two learned lists are written
    /// the way the game writes them: [ids in use][taught but empty][57 / 96
    /// fill], with the length byte counting the first two.
    fn editor_with_learned_lists() -> Editor {
        let mut save = crate::save::empty_switch_save();
        assert!(palico::set_name(&mut save, 0, 0, "テスト"));
        let base = save.slot_base(0).expect("slot base") as usize;
        let rec = base + palico::OFF_ROSTER;
        let skills: [u8; 3] = [5, 46, 80];
        for i in 0..palico::LEARNED_SKILL_SLOTS {
            save.body_mut()[rec + palico::LEARNED_SKILLS + i] = if i < 3 {
                skills[i]
            } else if i < 5 {
                0
            } else {
                palico::SKILL_EMPTY
            };
        }
        save.body_mut()[rec + palico::SKILL_LEN] = 5;
        let moves: [u8; 3] = [3, 5, 9];
        for i in 0..palico::LEARNED_MOVE_SLOTS {
            save.body_mut()[rec + palico::LEARNED_MOVES + i] = if i < 3 {
                moves[i]
            } else if i < 6 {
                0
            } else {
                palico::MOVE_EMPTY
            };
        }
        save.body_mut()[rec + palico::MOVE_LEN] = 6;
        save.body_mut()[rec + palico::SYSTEM] = 4;
        save.body_mut()[rec + palico::ATTACK] = 3;
        let mut ed = Editor::new();
        ed.open_save_bytes("C:/mhnx/test/system", &save.to_bytes());
        assert!(ed.save.is_some(), "{}", ed.message);
        ed.screen = Screen::Roster;
        ed
    }

    #[test]
    fn the_skill_panel_lists_the_two_lists_and_writes_one_slot() {
        let mut ed = editor_with_learned_lists();
        ed.roster_cursor = 0;
        // Plus is the panel key on the roster screen.
        ed.input(Key::Plus);
        assert!(ed.palico_skill_open(), "{}", ed.message);
        // A mode, not a screen: the roster is still the screen underneath.
        assert_eq!(ed.screen, Screen::Roster);
        assert_eq!(ed.palico_skill_target().map(|p| p.index), Some(0));
        assert_eq!(ed.palico_skill_tab(), PALICO_TAB_SKILLS);

        let rows = ed.palico_skill_rows();
        assert_eq!(rows.len(), PALICO_SKILL_ROWS);
        assert_eq!(PALICO_SKILL_ROWS, palico::LEARNED_SKILL_SLOTS);
        assert_eq!(ed.palico_skill_slots(), (5, PALICO_SKILL_ROWS));
        assert_eq!(rows[0].current, 5);
        assert_eq!(rows[0].current_name, palico::skill_name(5));
        // Two slots the list counts but has not taught yet, then the fill.
        assert!(rows[3].counted && rows[3].is_empty(), "slot 3 is a taught-empty slot");
        assert!(!rows[5].counted, "slot 5 is past the stored length");
        assert_eq!(rows[5].current, palico::SKILL_EMPTY);
        // The candidate starts on the id the slot already holds, so [A] on an
        // untouched row writes the same id back.
        assert_eq!(rows[0].pick, 5);
        // Walking the rows is not a write.
        ed.dirty = false;
        ed.input(Key::Down);
        assert_eq!(ed.palico_skill.map(|s| s.field), Some(1));
        ed.input(Key::Up);
        assert!(!ed.dirty);

        // Right moves the candidate, and still nothing is written.
        ed.input(Key::Right);
        assert_eq!(ed.palico_skill.map(|s| s.pick()), Some(6));
        assert!(!ed.dirty, "moving the candidate writes nothing");

        // A writes the candidate, and only the one byte of that slot moves.
        let before = ed.save.as_ref().expect("save").body().to_vec();
        ed.input(Key::A);
        assert!(ed.dirty, "{}", ed.message);
        let base = ed
            .save
            .as_ref()
            .and_then(|s| s.slot_base(0))
            .expect("slot base") as usize;
        let at = base + palico::OFF_ROSTER + palico::LEARNED_SKILLS;
        let after = ed.save.as_ref().expect("save").body().to_vec();
        let moved: Vec<usize> = (0..before.len()).filter(|i| before[*i] != after[*i]).collect();
        assert_eq!(moved, alloc::vec![at]);
        assert_eq!(
            palico::learned_skills(&palico::roster(ed.save.as_ref().expect("save"), 0)[0]),
            alloc::vec![6, 46, 80]
        );

        // Y swaps to the sixteen slot support move list, and [A] writes there
        // through set_move.
        ed.input(Key::Y);
        assert_eq!(ed.palico_skill_tab(), PALICO_TAB_MOVES);
        let rows = ed.palico_skill_rows();
        assert_eq!(rows.len(), PALICO_MOVE_ROWS);
        assert_eq!(PALICO_MOVE_ROWS, palico::LEARNED_MOVE_SLOTS);
        assert_eq!(ed.palico_skill_slots(), (6, PALICO_MOVE_ROWS));
        assert_eq!(rows[0].current, 3);
        assert_eq!(rows[0].pick, 3);
        ed.input(Key::Right);
        ed.input(Key::A);
        assert_eq!(
            palico::support_moves(&palico::roster(ed.save.as_ref().expect("save"), 0)[0]),
            alloc::vec![4, 5, 9]
        );

        // The core owns the refusal: a slot past the stored length, an id the
        // table does not have, a slot past the array and a record that is not
        // a palico all leave the save alone.
        ed.dirty = false;
        let before = ed.save.as_ref().expect("save").body().to_vec();
        assert!(!ed.palico_skill_assign(0, 9, 6), "{}", ed.message);
        assert!(ed.message.contains("書けません"), "{}", ed.message);
        assert!(!ed.palico_skill_assign(0, 0, 0));
        assert!(!ed.palico_skill_assign(0, 0, palico::MAX_MOVE + 1));
        assert!(!ed.palico_skill_assign(0, PALICO_MOVE_ROWS, 1));
        assert!(!ed.palico_skill_assign(palico::SLOTS, 0, 1));
        assert!(!ed.dirty);
        assert_eq!(ed.save.as_ref().expect("save").body().to_vec(), before);

        // Writing the id a slot already holds is not a change, and says so.
        assert!(ed.palico_skill_assign(0, 1, 5));
        assert!(!ed.dirty);
        assert!(ed.message.contains("すでに"), "{}", ed.message);
    }

    #[test]
    fn the_skill_panel_says_the_tendencies_and_carries_the_warnings() {
        let mut ed = editor_with_learned_lists();
        ed.roster_cursor = 0;
        ed.input(Key::Plus);
        assert!(ed.palico_skill_open(), "{}", ed.message);
        // System 4 is ヒーリング and the attack byte is 3: both are on the panel.
        let tendencies = ed.palico_skill_tendencies();
        assert!(tendencies.contains("ヒーリング"), "{}", tendencies);
        assert!(tendencies.contains("攻撃傾向: 3"), "{}", tendencies);

        // Nothing is inconsistent yet, and the console output carries the line.
        assert!(ed.palico_skill_warnings().is_empty());
        let lines = ed.lines();
        assert!(
            lines.iter().any(|l| l.text.contains("ヒーリング")),
            "the tendency line is missing"
        );
        assert!(!lines.iter().any(|l| l.text.contains("!!")));
        // One row per slot of the list, one of them the cursor, and the header
        // names the record.
        assert_eq!(lines.iter().filter(|l| l.selectable).count(), PALICO_SKILL_ROWS);
        assert_eq!(lines.iter().filter(|l| l.cursor).count(), 1);
        assert!(lines[0].text.contains("ロスター #1"), "{}", lines[0].text);

        // Equipping a skill the record never learned is a state the sample save
        // itself holds: the panel shows the warning line instead of hiding it.
        {
            let base = ed
                .save
                .as_ref()
                .and_then(|s| s.slot_base(0))
                .expect("slot base") as usize;
            let at = base + palico::OFF_ROSTER + palico::SKILLS;
            ed.save.as_mut().expect("save").body_mut()[at] = 90;
        }
        let warnings = ed.palico_skill_warnings();
        assert_eq!(warnings.len(), 1, "{:?}", warnings);
        assert!(warnings[0].contains("装備スキル"), "{:?}", warnings);
        let lines = ed.lines();
        assert!(
            lines
                .iter()
                .any(|l| l.text.contains("!!") && l.text.contains("装備スキル")),
            "the warning line is missing"
        );
        assert_eq!(lines.iter().filter(|l| l.selectable).count(), PALICO_SKILL_ROWS);

        // A tendency byte the game does not offer is called out, not named.
        let base = ed
            .save
            .as_ref()
            .and_then(|s| s.slot_base(0))
            .expect("slot base") as usize;
        ed.save.as_mut().expect("save").body_mut()[base + palico::OFF_ROSTER + palico::ATTACK] = 9;
        assert!(ed.palico_skill_tendencies().contains("範囲外"));
    }

    #[test]
    fn closing_the_skill_panel_leaves_the_roster_where_it_was() {
        let mut ed = editor_with_learned_lists();
        ed.roster_cursor = 0;
        ed.input(Key::Plus);
        assert!(ed.palico_skill_open(), "{}", ed.message);
        ed.input(Key::Down);
        ed.input(Key::Down);
        assert_eq!(ed.palico_skill.map(|s| s.field), Some(2));

        // B closes the panel; the roster keeps its own row.
        ed.input(Key::B);
        assert!(!ed.palico_skill_open());
        assert_eq!(ed.screen, Screen::Roster);
        assert_eq!(ed.roster_cursor, 0);
        assert!(ed.message.contains("閉じました"), "{}", ed.message);

        // The two panels are modes of one screen: opening either closes the
        // other, so one form is never left under the second.
        ed.input(Key::Plus);
        assert!(ed.palico_skill_open());
        assert!(ed.open_palico_equip(0), "{}", ed.message);
        assert!(ed.palico_equip_open());
        assert!(!ed.palico_skill_open());
        assert!(ed.open_palico_skill(0), "{}", ed.message);
        assert!(ed.palico_skill_open());
        assert!(!ed.palico_equip_open());

        // An empty roster record and a record number past the roster are not a
        // palico, so there is nothing to list.
        assert!(!ed.open_palico_skill(palico::SLOTS));
        assert!(!ed.open_palico_skill(7));
        assert!(ed.message.contains("オトモがいません"), "{}", ed.message);
    }

    /// A blank Switch save whose item box holds these (id, count) pairs, with
    /// an editor that has it open on slot 0.
    ///
    /// Nothing comes from a real save: the box is written through the same
    /// writer the editor uses, and the rest of the body stays zero, so the test
    /// runs anywhere.
    fn editor_with_item_box(items: &[(u16, u8)]) -> Editor {
        let mut save = crate::save::empty_switch_save();
        let mut slots = alloc::vec![ItemSlot::default(); TOTAL_ITEM_SLOTS];
        for (i, &(id, count)) in items.iter().enumerate() {
            slots[i] = ItemSlot { id, count };
        }
        save.write_item_box(0, &slots).expect("item box");
        let mut ed = Editor::new();
        ed.open_save_bytes("C:/mhnx/test/system", &save.to_bytes());
        assert_eq!(ed.slot, 0);
        ed
    }

    #[test]
    fn an_item_set_is_edited_one_entry_at_a_time() {
        // Two items in the box: the form walks them, and every row it is not
        // asked to write keeps what the record has.
        let mut ed = editor_with_item_box(&[(1, 5), (2, 3)]);
        ed.screen = Screen::Sets;
        assert!(ed.open_item_set_editor(0), "{}", ed.message);
        assert!(ed.item_set_editor_open());
        // A mode of the my-sets screen, not a screen of its own.
        assert_eq!(ed.screen, Screen::Sets);
        assert_eq!(ed.item_set_edit.map(|e| e.field), Some(0));
        assert!(ed.set_editor_open(), "either form answers the same question");
        // The console renderer draws the form instead of the list.
        let shown = ed
            .lines()
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains("ITEM EDIT"), "{}", shown);
        // Thirty-two entry rows, one of them the cursor.
        assert_eq!(
            ed.lines().iter().filter(|l| l.selectable).count(),
            ITEM_SET_EDIT_ROWS
        );
        assert_eq!(ed.lines().iter().filter(|l| l.cursor).count(), 1);

        // Walking the rows is not a write.
        ed.input(Key::Down);
        ed.input(Key::Up);
        assert!(!ed.dirty);
        assert_eq!(ed.item_set_row_label(0), "空き");

        // The candidates are the empty entry and the box's items in box order,
        // so the first Right lands on the first item.
        ed.input(Key::Right);
        assert_eq!(ed.item_set_pick(0), crate::myset::SetItem { id: 1, count: 5 });
        assert!(!ed.dirty, "walking the candidates writes nothing");
        let stored = crate::myset::item_sets(ed.save.as_ref().expect("save"), 0)[0].clone();
        assert_eq!(stored.items[0].id, 0);

        // [A] writes that one entry.
        ed.input(Key::A);
        assert!(ed.dirty);
        let stored = crate::myset::item_sets(ed.save.as_ref().expect("save"), 0)[0].clone();
        assert_eq!(stored.items[0], crate::myset::SetItem { id: 1, count: 5 });
        assert!(stored.items[1..].iter().all(|i| i.id == 0), "nothing else moved");

        // The second row takes the second item: the pending entry starts on the
        // empty one, so two steps reach it.
        ed.input(Key::Down);
        ed.input(Key::Right);
        ed.input(Key::Right);
        assert_eq!(ed.item_set_pick(1), crate::myset::SetItem { id: 2, count: 3 });
        ed.input(Key::A);
        let stored = crate::myset::item_sets(ed.save.as_ref().expect("save"), 0)[0].clone();
        assert_eq!(stored.items[1], crate::myset::SetItem { id: 2, count: 3 });

        // Cycling past the last candidate comes back round to the empty one,
        // and [A] on it empties the row again.
        ed.input(Key::Up);
        ed.input(Key::Right);
        ed.input(Key::Right);
        assert_eq!(ed.item_set_pick(0), crate::myset::SetItem { id: 0, count: 0 });
        ed.input(Key::A);
        let stored = crate::myset::item_sets(ed.save.as_ref().expect("save"), 0)[0].clone();
        assert_eq!(stored.items[0].id, 0);
        assert_eq!(stored.items[1].id, 2, "the other row is untouched");

        // B gives the list back and drops the form.
        ed.input(Key::B);
        assert!(!ed.item_set_editor_open());
        assert_eq!(ed.screen, Screen::Sets);
        // The other form is not left open behind it.
        assert!(!ed.set_editor_open());
    }

    #[test]
    fn the_pouch_and_an_item_set_trade_places_two_presses_at_a_time() {
        let mut ed = editor_with_item_box(&[(1, 5)]);
        // A pouch of its own, so the two directions cannot look alike.
        {
            let save = ed.save.as_mut().expect("save");
            assert!(crate::myset::set_pouch_item(save, 0, 0, 11, 7));
            assert!(crate::myset::set_pouch_item(save, 0, 1, 12, 2));
        }
        ed.screen = Screen::Sets;
        assert!(ed.open_item_set_editor(0), "{}", ed.message);
        ed.dirty = false;
        let before = ed.save.as_ref().expect("save").body().to_vec();

        // One press only says what would be replaced.
        ed.input(Key::Y);
        assert_eq!(ed.item_set_armed, Some(ItemSetOp::FromPouch));
        assert!(!ed.dirty);
        assert_eq!(
            ed.save.as_ref().expect("save").body(),
            &before[..],
            "the first press must write nothing"
        );
        assert!(ed.message.contains("もう一度"), "{}", ed.message);

        // The second press takes the pouch in.
        ed.input(Key::Y);
        assert_eq!(ed.item_set_armed, None);
        assert!(ed.dirty);
        let stored = crate::myset::item_sets(ed.save.as_ref().expect("save"), 0)[0].clone();
        assert_eq!(stored.items[0], crate::myset::SetItem { id: 11, count: 7 });
        assert_eq!(stored.items[1], crate::myset::SetItem { id: 12, count: 2 });
        assert_eq!(stored.used_items().len(), 2);
        // The pending rows follow the record that was just written.
        assert_eq!(ed.item_set_pick(0), crate::myset::SetItem { id: 11, count: 7 });

        // Now the other direction. The pouch is emptied first so that a write
        // is visible, and one [-] press is not enough to replace it.
        {
            let save = ed.save.as_mut().expect("save");
            assert!(crate::myset::set_pouch_item(save, 0, 0, 0, 0));
            assert!(crate::myset::set_pouch_item(save, 0, 1, 0, 0));
        }
        ed.dirty = false;
        ed.input(Key::Minus);
        assert_eq!(ed.item_set_armed, Some(ItemSetOp::ToPouch));
        assert!(!ed.dirty);
        assert_eq!(crate::myset::pouch(ed.save.as_ref().expect("save"), 0)[0].id, 0);

        ed.input(Key::Minus);
        assert!(ed.dirty);
        let pouch = crate::myset::pouch(ed.save.as_ref().expect("save"), 0);
        assert_eq!(pouch[0], crate::myset::SetItem { id: 11, count: 7 });
        assert_eq!(pouch[1], crate::myset::SetItem { id: 12, count: 2 });
        // Applying a set is not a move: the record keeps what it had.
        let stored = crate::myset::item_sets(ed.save.as_ref().expect("save"), 0)[0].clone();
        assert_eq!(stored.items[0], crate::myset::SetItem { id: 11, count: 7 });

        // Any other key drops the pending operation, so the next [Y] starts the
        // question over instead of writing on a press the user did not see.
        ed.input(Key::Y);
        ed.input(Key::Down);
        assert_eq!(ed.item_set_armed, None);
    }

    #[test]
    fn csv_export_names_the_files_after_the_clock_and_applies_back() {
        let mut ed = editor_with_item_box(&[(1, 5), (2, 3)]);
        ed.open_csv();
        assert_eq!(ed.screen, Screen::Csv);
        assert!(ed.take_csv_rescan(), "opening the screen lists the folder");
        let shown = ed
            .lines()
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains(CSV_DIR), "{}", shown);

        // No clock: nothing is built, and the message says why.
        assert!(ed.export_csv(None).is_empty());
        assert!(ed.message.contains("時計"), "{}", ed.message);

        let unix = 1_700_000_000u64;
        let files = ed.export_csv(Some(unix));
        assert_eq!(files.len(), 2, "one file per box");
        let stamp = crate::snapshot::stamp(unix);
        assert_eq!(
            files[0].0,
            alloc::format!("{}/items-{}.csv", CSV_DIR, stamp)
        );
        assert_eq!(
            files[1].0,
            alloc::format!("{}/equipment-{}.csv", CSV_DIR, stamp)
        );
        let text = core::str::from_utf8(&files[0].1).expect("utf-8");
        // The name column was added after the first release of this export;
        // reading it back still uses only the first three columns.
        assert!(text.starts_with("slot,id,count,name\r\n"), "{}", ed.message);
        assert!(text.contains("\r\n0,1,5,"));
        assert!(text.contains("\r\n1,2,3,"));

        // The layer reports what it wrote, and the folder is listed again.
        ed.csv_exported(2, 2);
        assert!(ed.message.contains("2 ファイル"), "{}", ed.message);
        assert!(ed.take_csv_rescan());

        // Applying the file back to a second editor restores the box.
        let mut other = editor_with_item_box(&[]);
        other.open_csv();
        assert!(other.take_csv_rescan());
        other.set_csv_files(alloc::vec![files[0].0.clone()]);
        // The first press only arms the apply; nothing is read yet.
        other.input(Key::A);
        assert_eq!(other.csv_armed, Some(0));
        assert_eq!(other.take_csv_read(), None);
        assert!(other.message.contains("もう一度"), "{}", other.message);
        // The second press asks the I/O layer for the file.
        other.input(Key::A);
        assert_eq!(other.take_csv_read(), Some(files[0].0.clone()));
        assert_eq!(other.csv_armed, None);

        assert!(other.apply_csv_text(&files[0].0, text));
        assert!(other.dirty);
        let boxed = other.save.as_ref().expect("save").read_item_box(0).expect("box");
        assert_eq!((boxed[0].id, boxed[0].count), (1, 5));
        assert_eq!((boxed[1].id, boxed[1].count), (2, 3));
        assert_eq!((boxed[2].id, boxed[2].count), (0, 0));
        // The report the panel draws says how much was written.
        assert!(
            other.csv_report[0].contains("適用 2300 行"),
            "{:?}",
            other.csv_report
        );
        assert!(other.message.contains("適用"), "{}", other.message);
    }

    #[test]
    fn a_csv_that_is_not_one_is_refused_without_touching_the_box() {
        let mut ed = editor_with_item_box(&[(1, 5)]);
        ed.open_csv();
        ed.take_csv_rescan();
        ed.set_csv_files(alloc::vec![alloc::format!("{}/notes.csv", CSV_DIR)]);
        ed.input(Key::A);
        ed.input(Key::A);
        let path = ed.take_csv_read().expect("the second press asks for the file");

        // A file whose first line names neither table is refused whole.
        assert!(!ed.apply_csv_text(&path, "hello,world\r\n"));
        assert!(!ed.dirty);
        assert!(ed.message.contains("CSV ではありません"), "{}", ed.message);
        assert_eq!(
            ed.save.as_ref().expect("save").read_item_box(0).expect("box")[0].id,
            1
        );

        // A broken row names the line it is on and writes nothing either.
        let bad = "slot,id,count\r\n1,x,3\r\n";
        assert!(!ed.apply_csv_text(&alloc::format!("{}/items-1.csv", CSV_DIR), bad));
        assert!(!ed.dirty);
        assert!(ed.message.contains("2 行目"), "{}", ed.message);
        assert_eq!(
            ed.save.as_ref().expect("save").read_item_box(0).expect("box")[0].id,
            1
        );

        // A renamed export still applies: the header decides, not the name.
        let good = "slot,id,count\r\n0,4,9\r\n";
        assert!(ed.apply_csv_text(&alloc::format!("{}/renamed.csv", CSV_DIR), good));
        assert!(ed.dirty);
        assert_eq!(
            ed.save.as_ref().expect("save").read_item_box(0).expect("box")[0].id,
            4
        );

        // A byte order mark in front of the header is what a spreadsheet on
        // Windows writes; it is skipped rather than taken for a broken row.
        let bom = "\u{feff}slot,id,count\r\n0,7,1\r\n";
        assert!(ed.apply_csv_text(&alloc::format!("{}/excel.csv", CSV_DIR), bom));
        assert_eq!(
            ed.save.as_ref().expect("save").read_item_box(0).expect("box")[0].id,
            7
        );
    }

    /// An editor holding a blank Switch save whose slot 0 holds a character.
    ///
    /// Every writer on the progress screen refuses an unused slot, so the flag
    /// has to be set or the tests would be asserting refusals.
    fn editor_on_a_used_slot() -> Editor {
        let mut save = crate::save::empty_switch_save();
        save.set_slot_used(0, true);
        let mut ed = Editor::new();
        ed.open_save_bytes("C:/mhnx/test/system", &save.to_bytes());
        ed
    }

    #[test]
    fn one_press_flips_one_award_bit_in_all_three_copies() {
        let mut ed = editor_on_a_used_slot();
        ed.open_progress();
        assert_eq!(ed.screen, Screen::Progress);
        assert_eq!(ed.progress_view(), PROGRESS_AWARDS);
        assert_eq!(ed.progress_len(), awards::AWARDS);
        // Row 11 is the twelfth award, the one the header example names.
        ed.set_progress_cursor(11);
        assert_eq!(ed.progress_awards(), (0, awards::AWARDS));
        ed.input(Key::A);
        assert_eq!(ed.progress_awards(), (1, awards::AWARDS));
        assert!(ed.dirty);

        // All three copies hold it, so the OR the game repairs with has
        // nothing to undo - and copies_agree proves it bit for bit.
        assert_eq!(ed.progress_awards_agree(), Some(true));
        let save = ed.save.as_ref().expect("a save");
        assert_eq!(awards::has(save, 0, 11), Some(true));
        for (off, _) in awards::COPIES {
            let at = save.slot_base(0).expect("a slot") as usize + off;
            assert!(
                save.body()[at + 11 / 8] & (1 << (11 % 8)) != 0,
                "the copy at {:#X} was not written",
                off
            );
        }

        // One press again is the undo: no screen on this page asks twice.
        ed.input(Key::A);
        assert_eq!(ed.progress_awards(), (0, awards::AWARDS));
        assert_eq!(ed.progress_awards_agree(), Some(true));
        // The console renderer shows the count the player checks.
        assert_eq!(ed.lines().iter().filter(|l| l.cursor).count(), 1);
        let shown = ed
            .lines()
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("
");
        assert!(shown.contains("取得 0 / 132"), "{}", shown);
    }

    #[test]
    fn the_three_views_keep_their_own_cursor_and_move_by_a_page() {
        let mut ed = editor_on_a_used_slot();
        ed.open_progress();
        ed.set_progress_cursor(7);

        // R walks to the request view, which starts on its own first row.
        ed.input(Key::R);
        assert_eq!(ed.progress_view(), PROGRESS_REQUESTS);
        assert_eq!(ed.progress_len(), REQUEST_ROWS);
        assert_eq!(ed.progress_cursor(), 0, "each view keeps its own cursor");
        assert_eq!(ed.request_flag_at(0), Some(300));
        assert_eq!(ed.request_flag_at(REQUEST_ROWS - 1), Some(1394));
        assert_eq!(ed.request_flag_at(REQUEST_ROWS), None);
        ed.input(Key::Right);
        assert_eq!(ed.progress_cursor(), PROGRESS_JUMP);
        ed.input(Key::Left);
        assert_eq!(ed.progress_cursor(), 0, "Left at the top stays there");

        // R again: the quest view pages its 1508 indices by PROGRESS_JUMP.
        ed.input(Key::R);
        assert_eq!(ed.progress_view(), PROGRESS_QUESTS);
        assert_eq!(ed.progress_len(), 1508);
        assert_eq!(ed.progress_quest_page(), (1, 31));
        assert_eq!(ed.progress_quest_index(), 1);
        ed.input(Key::Right);
        assert_eq!(ed.progress_quest_page(), (2, 31));
        assert_eq!(ed.progress_quest_index(), 51);

        // Y on this view is the seen bit, not the view key.
        ed.input(Key::Y);
        assert_eq!(ed.progress_view(), PROGRESS_QUESTS, "Y is not the tab here");
        assert_eq!(ed.progress_quest_state(50), (false, true, false));

        // L back to the award view: the cursor is where it was left.
        ed.input(Key::L);
        ed.input(Key::L);
        assert_eq!(ed.progress_view(), PROGRESS_AWARDS);
        assert_eq!(ed.progress_cursor(), 7);

        // B leaves for the guild card, the record this screen continues.
        ed.input(Key::B);
        assert_eq!(ed.screen, Screen::Guild);
    }

    // ------------------------------------------------- 闘技大会 (arena log)

    /// An editor sitting on the arena view of the guild screen.
    fn editor_on_the_arena_view() -> Editor {
        let mut ed = editor_on_a_used_slot();
        ed.screen = Screen::Guild;
        ed.set_guild_view(GUILD_TAB_ARENA);
        ed
    }

    #[test]
    fn the_arena_view_lists_every_recorded_time_and_edits_the_one_under_the_cursor() {
        let mut ed = editor_on_the_arena_view();
        // Two recorded times, written through the function the PC build uses.
        let slot = ed.slot;
        {
            let save = ed.save.as_mut().expect("a save");
            assert!(crate::guildcard::set_arena_time(save, slot, 0, 0, 57_544));
            assert!(crate::guildcard::set_arena_time(save, slot, 10, 2, 2_348));
        }

        // The grid the view draws is the log itself: one row per arena, one
        // column per rank, and nothing invented.
        let grid = ed.arena_grid();
        assert_eq!(grid.len(), ARENA_ROWS);
        assert_eq!(ARENA_ROWS, crate::guildcard::ARENA_COUNT);
        assert_eq!(grid[0].len(), ARENA_COLUMNS);
        assert_eq!(ARENA_COLUMNS, crate::guildcard::ARENA_RANKS);
        assert_eq!(grid[0][0], 57_544);
        assert_eq!(grid[10][2], 2_348);
        assert_eq!(grid[1][0], 0, "an unrecorded entry stays 0");

        // Up / Down walk the arenas, Left / Right the ranks, and both wrap.
        ed.arena_row = 0;
        ed.arena_rank = 0;
        ed.input(Key::Up);
        assert_eq!(ed.arena_selected(), (ARENA_ROWS - 1, 0));
        ed.input(Key::Down);
        assert_eq!(ed.arena_selected(), (0, 0));
        ed.input(Key::Left);
        assert_eq!(ed.arena_selected(), (0, ARENA_COLUMNS - 1));
        ed.input(Key::Right);
        assert_eq!(ed.arena_selected(), (0, 0));
        // Moving the cursor writes nothing and records nothing.
        assert!(!ed.dirty);
        assert_eq!(ed.undo_depth(), 0);

        // [A] arms the editor; from then on Left / Right pick the part.
        ed.input(Key::A);
        assert!(ed.arena_edit, "{}", ed.message);
        assert_eq!(ed.arena_part, ARENA_PART_CS);
        ed.input(Key::Right);
        assert_eq!(ed.arena_part, ARENA_PART_MIN);
        ed.input(Key::Left);
        assert_eq!(ed.arena_part, ARENA_PART_CS);
        // The hundredths of 9:35.44 against 9:35.45, as one undo step.
        ed.input(Key::Up);
        assert_eq!(ed.arena_centis(), 57_545);
        assert!(ed.dirty);
        assert_eq!(ed.undo_depth(), 1);
        assert_eq!(ed.undo_label(), Some("闘技大会 1"));
        let save = ed.save.as_ref().expect("a save");
        let entry = crate::guildcard::arena(save, 0)
            .into_iter()
            .find(|e| e.arena == 0 && e.rank == 0)
            .expect("the entry");
        assert_eq!(entry.centis, 57_545);
        // The two bytes behind the time are not the editor's business.
        assert_eq!(entry.extra, [0, 0]);

        // The minutes step by a minute: 9:35.45 -> 10:35.45.
        ed.arena_part = ARENA_PART_MIN;
        ed.input(Key::Up);
        assert_eq!(ed.arena_centis(), 63_545);
        // The seconds step by a second.
        ed.arena_part = ARENA_PART_SEC;
        ed.input(Key::Down);
        assert_eq!(ed.arena_centis(), 63_445);

        // [A] leaves the editor and the cursor stays on the same cell.
        ed.input(Key::A);
        assert!(!ed.arena_edit);
        assert_eq!(ed.arena_selected(), (0, 0));

        // [X] is the save key here like everywhere else.
        ed.input(Key::X);
        assert!(ed.take_write().is_some(), "X must ask for the save to be written");
    }

    #[test]
    fn an_arena_time_typed_as_a_clock_lands_in_the_entry_under_the_cursor() {
        // The formatter and the parser are each other's inverse on everything
        // the two stored bytes can hold.
        assert_eq!(arena_clock(57_544), "9:35.44");
        assert_eq!(arena_clock(0), "0:00.00");
        assert_eq!(arena_clock(2_348), "0:23.48");
        assert_eq!(arena_clock(u16::MAX), "10:55.35");
        assert_eq!(arena_clock_value("9:35.44"), Some(57_544));
        assert_eq!(arena_clock_value(" 0:23.48 "), Some(2_348));
        assert_eq!(arena_clock_value("35.4"), Some(3_540));
        assert_eq!(arena_clock_value("9:35"), Some(57_500));
        assert_eq!(arena_clock_value("57544"), Some(57_544));
        assert_eq!(arena_clock_value(""), None);
        assert_eq!(arena_clock_value("abc"), None);
        assert_eq!(arena_clock_value("9:"), None);
        assert_eq!(arena_clock_value("999:00.00"), Some(u16::MAX));
        for c in [0u16, 1, 2_348, 57_544, 60_000, u16::MAX] {
            assert_eq!(arena_clock_value(&arena_clock(c)), Some(c), "{}", c);
        }

        let mut ed = editor_on_the_arena_view();
        ed.arena_row = 2;
        ed.arena_rank = 1;
        // [Y] asks the I/O layer for the cell as text, seeded with what it is.
        ed.input(Key::Y);
        assert_eq!(
            ed.take_text_request(),
            Some((TEXT_ARENA_TIME, String::from("0:00.00")))
        );
        // The answer comes back as typed text.
        assert!(ed.apply_arena_time("9:35.44"), "{}", ed.message);
        assert_eq!(ed.arena_centis(), 57_544);
        assert_eq!(ed.arena_rank, 1, "the typed value went into the cell");
        assert!(ed.dirty);
        assert!(ed.message.contains("9:35.44"), "{}", ed.message);

        // A text that is not a time leaves the entry alone and says the shape.
        ed.dirty = false;
        let depth = ed.undo_depth();
        assert!(!ed.apply_arena_time("9:xx"));
        assert!(ed.message.contains("m:ss.cc"), "{}", ed.message);
        assert!(!ed.dirty);
        assert_eq!(ed.undo_depth(), depth);

        // 0:00.00 is how the game stores "no record", so it erases one.
        assert!(ed.apply_arena_time("0:00.00"));
        assert_eq!(ed.arena_centis(), 0);
        assert!(ed.dirty);
        assert_eq!(ed.undo_depth(), depth + 1);
    }

    #[test]
    fn the_arena_view_undo_puts_the_time_back_and_the_stack_sees_it() {
        let mut ed = editor_on_the_arena_view();
        ed.arena_row = 3;
        ed.arena_rank = 0;
        assert!(ed.set_arena_time_at(3, 0, 12_345));
        assert_eq!(ed.arena_centis_at(3, 0), 12_345);
        assert_eq!(ed.undo_label(), Some("闘技大会 4"));

        assert!(ed.undo_last(), "{}", ed.message);
        assert_eq!(ed.arena_centis_at(3, 0), 0);
        assert!(ed.message.contains("元に戻しました: 闘技大会 4"), "{}", ed.message);
        assert!(!ed.arena_edit, "the undo leaves the editor on the cell");

        // A press that would write what is already there writes nothing, so a
        // held key cannot fill the four-step stack with identical saves.
        ed.arena_edit = true;
        ed.arena_part = ARENA_PART_CS;
        ed.input(Key::Down);
        assert_eq!(ed.undo_depth(), 0);
        assert_eq!(ed.arena_centis_at(3, 0), 0);
        assert!(ed.message.contains("これ以上"), "{}", ed.message);
    }

    #[test]
    fn the_guild_screen_carries_the_arena_log_as_its_second_view() {
        let mut ed = editor_on_a_used_slot();
        ed.screen = Screen::Guild;
        // A view of the same screen, not a screen of its own: the progress
        // screen's B still lands on Guild, and the slot base is the same.
        assert_eq!(ed.guild_view(), GUILD_TAB_CARD);
        ed.set_guild_view(GUILD_TAB_ARENA);
        assert_eq!(ed.guild_view(), GUILD_TAB_ARENA);
        assert_eq!(ed.screen, Screen::Guild);

        // The rows are the game's arena quests, and the console fallback prints
        // one fixed-width row per arena: five ten-cell columns after a 20-cell
        // name column, so 75 cells at most.
        assert_eq!(ARENA_NAMES.len(), ARENA_ROWS);
        assert_eq!(arena_name(0), "ホロロホルル");
        assert_eq!(arena_name(ARENA_ROWS - 1), "ババコンガ");
        assert_eq!(arena_name(ARENA_ROWS), "");
        assert_eq!(arena_rank_name(0), "1位");
        assert_eq!(arena_rank_name(4), "5位");
        assert_eq!(arena_part_name(ARENA_PART_MIN), "分");
        assert_eq!(arena_part_name(ARENA_PART_SEC), "秒");
        assert_eq!(arena_part_name(ARENA_PART_CS), "1/100");

        let lines = ed.lines();
        let text = lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("
");
        assert!(text.contains("ARENA LOG"), "{}", text);
        for name in ARENA_NAMES {
            assert!(text.contains(name), "{} is missing:\n{}", name, text);
        }
        // One selectable line per arena and exactly one cursor.
        assert_eq!(lines.iter().filter(|l| l.selectable).count(), ARENA_ROWS);
        assert_eq!(lines.iter().filter(|l| l.cursor).count(), 1);
        // Nothing runs past the console's eighty cells.
        for line in &lines {
            let cells: usize = line.text.chars().map(char_cells).sum();
            assert!(cells <= 80, "{} cells: {}", cells, line.text);
        }

        // B steps back one level at a time: the log -> the card -> the slots.
        ed.input(Key::B);
        assert_eq!(ed.guild_view(), GUILD_TAB_CARD);
        assert_eq!(ed.screen, Screen::Guild);
        // [X] is the save key on the card too, not a second "next field": the
        // field did not move and a write was asked for.
        let field = ed.guild_field;
        ed.input(Key::X);
        assert_eq!(ed.guild_field, field);
        assert!(
            ed.take_write().is_some(),
            "X on the card must ask for the save to be written"
        );
        ed.input(Key::B);
        assert_eq!(ed.screen, Screen::Slots);
    }

    #[test]
    fn clearing_a_quest_also_marks_it_seen_and_seen_cannot_go_first() {
        let mut ed = editor_on_a_used_slot();
        ed.open_progress();
        ed.progress_tab = PROGRESS_QUESTS;
        ed.set_progress_cursor(0);

        ed.input(Key::A);
        assert_eq!(ed.progress_quest_state(0), (true, true, false));
        assert!(ed.dirty);
        let save = ed.save.as_ref().expect("a save");
        assert_eq!(questprogress::get(save, 0, Progress::Cleared, 1), Some(true));
        assert_eq!(questprogress::get(save, 0, Progress::Seen, 1), Some(true));
        assert_eq!(ed.progress_quest_counts(), (1, 1, 0));

        // Y cannot take the seen bit off while the quest is still cleared: the
        // core refuses and the screen says why instead of doing nothing.
        ed.input(Key::Y);
        assert_eq!(ed.progress_quest_state(0), (true, true, false));
        assert!(ed.message.contains("クリア済み"), "{}", ed.message);

        // A again takes the clear off and leaves the seen bit, which is a state
        // the game itself writes.
        ed.input(Key::A);
        assert_eq!(ed.progress_quest_state(0), (false, true, false));
        assert_eq!(ed.progress_quest_counts(), (0, 1, 0));
        // 失敗 is read only, so nothing on this screen can set it.
        assert_eq!(ed.progress_quest_state(0).2, false);
    }

    #[test]
    fn the_request_view_lists_the_two_bands_and_writes_one_flag() {
        let mut ed = editor_on_a_used_slot();
        ed.open_progress();
        ed.progress_tab = PROGRESS_REQUESTS;
        assert_eq!(ed.progress_len(), REQUEST_ROWS);

        // The bands are contiguous and the second one starts at 1270.
        let flags = ed.request_flags();
        assert_eq!(flags.len(), 380);
        assert_eq!(flags[0], 300);
        assert_eq!(flags[254], 554, "the first band ends before 555");
        assert_eq!(flags[255], 1270);
        assert_eq!(flags[379], 1394);

        assert_eq!(ed.progress_request_count(), 0);
        ed.set_progress_cursor(0);
        ed.input(Key::A);
        assert_eq!(ed.progress_request_count(), 1);
        assert_eq!(ed.progress_request_band_count(), 1);
        let save = ed.save.as_ref().expect("a save");
        assert_eq!(requests::get(save, 0, 300), Some(true));
        // Without the romfs table a flag can only be called what it is.
        assert_eq!(ed.request_flag_label(300), "イベント");
        assert!(ed.message.contains("フラグ 300"), "{}", ed.message);

        ed.input(Key::A);
        assert_eq!(ed.progress_request_count(), 0);
        assert!(!ed
            .save
            .as_ref()
            .expect("a save")
            .body()
            .is_empty());
    }

    #[test]
    fn a_supplied_request_table_names_the_flag_it_owns() {
        let mut ed = editor_on_a_used_slot();
        ed.set_romfs_tables(
            alloc::vec![String::from("勲章の名前")],
            alloc::vec![String::from("村長の依頼")],
            alloc::vec![RequestRecord {
                index: 0,
                kind: 0,
                quest_id: 1_010_000,
                accept_flag: 300,
                complete_flag: 301,
            }],
        );
        assert_eq!(ed.award_name(0), "勲章の名前");
        assert_eq!(ed.award_name(1), "", "a short table names nothing else");
        assert_eq!(ed.request_flag_label(300), "依頼 #000 受理 村長の依頼");
        assert_eq!(ed.request_flag_label(301), "依頼 #000 完了 村長の依頼");
        assert_eq!(ed.request_flag_label(302), "イベント");
    }

    // ------------------------------------------------------- whole save undo

    #[test]
    fn the_undo_stack_keeps_four_steps_and_drops_the_oldest() {
        let mut ed = editor_on_a_used_slot();
        assert_eq!(ed.undo_depth(), 0);
        assert_eq!(ed.undo_label(), None);
        for i in 0..6 {
            ed.push_undo(format!("手順 {}", i));
        }
        assert_eq!(ed.undo_depth(), UNDO_STEPS);
        assert_eq!(UNDO_STEPS, 4);
        // The two oldest are gone; the newest is what the footer names.
        assert_eq!(ed.undo_label(), Some("手順 5"));
        assert_eq!(ed.undo_line(), "元に戻す: 4/4（1回 4.5MB を使用）");
        assert_eq!(ed.undo_count_text(), "4/4");
    }

    #[test]
    fn undo_puts_the_save_and_the_slot_back_and_marks_it_dirty() {
        let mut ed = editor_on_a_used_slot();
        ed.slot = 0;
        let funds = ed.char_value(1);
        ed.dirty = false;

        ed.set_char_value(1, 1234);
        assert_eq!(ed.char_value(1), 1234);
        assert!(ed.dirty);
        assert_eq!(ed.undo_depth(), 1);
        assert_eq!(ed.undo_label(), Some("キャラクター値"));

        ed.dirty = false;
        assert!(ed.undo_last(), "the step has to come back");
        assert_eq!(ed.char_value(1), funds);
        assert!(ed.dirty, "the file on disk no longer matches the editor");
        assert!(ed.message.contains("元に戻しました: キャラクター値"), "{}", ed.message);
        assert_eq!(ed.undo_depth(), 0);

        // Nothing left: the key says so instead of doing nothing quietly.
        assert!(!ed.undo_last());
        assert!(ed.message.contains("戻せる操作がありません"), "{}", ed.message);
    }

    #[test]
    fn every_major_screen_records_a_step_before_it_writes() {
        let mut ed = editor_on_a_used_slot();

        // キャラクター
        ed.set_char_value(1, 1234);
        assert_eq!(ed.undo_label(), Some("キャラクター値"));

        // ギルドカード
        assert!(ed.set_guild_comment("テスト"), "{}", ed.message);
        assert_eq!(ed.undo_label(), Some("ギルドカードのコメント"));

        // オトモ
        assert!(ed.rename_palico(0, "アイルー"), "{}", ed.message);
        assert_eq!(ed.undo_label(), Some("オトモの名前"));

        // 設定
        ed.screen = Screen::Options;
        ed.input(Key::Right);
        assert_eq!(ed.undo_label(), Some("設定"));

        // 装備
        ed.screen = Screen::Equip;
        ed.input(Key::Plus);
        ed.input(Key::A);
        assert_eq!(ed.undo_label(), Some("装備の新規作成"), "{}", ed.message);

        // アイテムボックス (一括操作)
        let mut items = itembox::decode(&alloc::vec![0u8; crate::itembox::ITEM_BOX_SIZE]);
        items[0] = ItemSlot { id: 1, count: 1 };
        ed.save
            .as_mut()
            .expect("save")
            .write_item_box(0, &items)
            .expect("the box takes a record");
        ed.screen = Screen::Items;
        ed.open_item_menu();
        ed.run_item_op(ITEM_OP_FILL);
        assert_eq!(ed.undo_label(), Some("アイテムボックス"), "{}", ed.message);

        // 進行状況
        ed.open_progress();
        ed.input(Key::A);
        assert_eq!(ed.undo_label(), Some("進行状況"), "{}", ed.message);

        // 護石: the form writes through set_charm_with_kind.
        ed.open_charms();
        ed.input(Key::L);
        ed.input(Key::A);
        assert_eq!(ed.undo_label(), Some("お守りの書き込み"), "{}", ed.message);

        // DLC
        ed.install_dlc_category(true, false, "公式");
        assert_eq!(
            ed.undo_label(),
            Some("DLC クエストの一括導入"),
            "{}",
            ed.message
        );

        // Four is the ceiling, and one press of a bulk MAX is one step.
        assert_eq!(ed.undo_depth(), UNDO_STEPS);
    }

    #[test]
    fn one_press_of_the_bulk_max_is_one_undo_step() {
        let mut ed = editor_on_a_used_slot();
        assert!(ed.maximize_character(), "{}", ed.message);
        assert_eq!(ed.undo_depth(), 1);
        assert_eq!(ed.undo_label(), Some("資金・ポイント一括MAX"));
        assert_eq!(ed.char_value(1), 9_999_999);
        assert_eq!(ed.char_value(3), 999);
    }

    #[test]
    fn the_undo_line_names_the_count_and_the_memory() {
        let mut ed = editor_on_a_used_slot();
        assert_eq!(ed.undo_line(), "元に戻す: 0/4（1回 4.5MB を使用）");
        ed.set_char_value(3, 5);
        let line = ed.undo_line();
        assert!(line.contains("1/4"), "{}", line);
        assert!(line.contains("4.5MB"), "{}", line);
        // The number is the constant, not a sentence that can drift from it.
        assert_eq!(UNDO_STEP_BYTES, crate::save::SWITCH_SIZE);
        assert_eq!(undo_step_text(), "4.5MB");
        assert_eq!(undo_total_text(), "18.0MB");
    }

    // ---------------------------------------------------- pre-save review

    #[test]
    fn the_review_names_the_equipment_box_it_would_change() {
        let save = crate::save::empty_switch_save();
        let base = save.slot_base(0).expect("slot base") as usize;
        let old = save.to_bytes();
        let mut new_save = save.clone();
        let at = base + crate::save::OFF_EQUIPMENT_BOX;
        new_save.body_mut()[at..at + 3].copy_from_slice(&[1, 2, 3]);
        let new = new_save.to_bytes();

        let diff = diff_save(&old, &new);
        assert_eq!(diff.changed, 3);
        assert_eq!(diff.ranges.len(), 1, "{:?}", diff.ranges);
        let line = diff.ranges[0].line();
        assert!(line.contains("スロット1 装備ボックス"), "{}", line);
        assert!(line.contains("3 バイト"), "{}", line);
        assert!(line.contains("1 枠"), "{}", line);
        let summary = diff.summary.join(" / ");
        assert!(summary.contains("スロット1: 装備ボックス 1 枠"), "{}", summary);

        // The same bytes are not a change, and the dialog is not shown for them.
        assert!(diff_save(&old, &old).is_empty());
    }

    #[cfg(feature = "gamedata")]
    fn quest_arc(id: u32) -> Vec<u8> {
        let mut mib = alloc::vec![0u8; crate::quest::MIB_SIZE];
        mib[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
        let a = Arc {
            version: crate::arc::ARC_VERSION,
            header8: 0xFFFF_FFFE,
            entries: alloc::vec![crate::arc::ArcEntry {
                name: String::from("questData\\questData_00"),
                type_hash: 0x1BBF_D18E,
                data: mib,
                original: None,
                original_offset: None,
            }],
        };
        a.to_bytes().expect("the archive rebuilds")
    }

    #[cfg(feature = "gamedata")]
    #[test]
    fn the_review_counts_the_quests_a_dlc_write_adds() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        let ids = [
            crate::gamedata::DLC_CATALOG[0],
            crate::gamedata::DLC_CATALOG[1],
        ];
        for id in ids {
            dlc::add_quest(&mut new, id, &quest_arc(id as u32)).expect("a free quest slot");
        }
        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        let summary = diff.summary.join(" / ");
        assert!(summary.contains("DLC: クエスト 2 件"), "{}", summary);
        // The part is the section, so the value walk still finds the quests;
        // the printed name is the save map's row - the quest slot itself.
        assert!(
            diff.ranges.iter().all(|r| r.part == "DLC セクション"),
            "{:?}",
            diff.ranges
        );
        assert!(
            diff.ranges
                .iter()
                .any(|r| r.name.starts_with("DLC クエスト（205 枠） #")),
            "{:?}",
            diff.ranges
        );
        assert_eq!(quests_changed(&old, &new), 2);
    }

    #[test]
    fn a_review_is_asked_for_only_when_the_bytes_differ() {
        let mut ed = editor_on_a_used_slot();
        let old = ed.save.as_ref().expect("save").to_bytes();

        // The same bytes: written at once, exactly the way it used to be.
        assert!(!ed.stage_save_review("nand:/system", &old, &old));
        assert!(!ed.review_open());

        // One byte different: the write waits for the second press.
        let mut changed = old.clone();
        changed[crate::save::SWITCH_HEADER_SIZE + 0x1000] ^= 0x01;
        assert!(ed.stage_save_review("nand:/system", &old, &changed));
        assert!(ed.review_open());
        let review = ed.review.as_ref().expect("a review");
        assert_eq!(review.changed, 1);
        assert!(review.headline().contains("変更 1 バイト"), "{}", review.headline());
        assert!(review.lines.iter().any(|l| l.contains("変更される内容")));
        assert!(review.lines.iter().any(|l| l.contains("変更される範囲")));
        assert!(ed.message.contains("もう一度"), "{}", ed.message);

        // The second press hands the write back, built from the open save.
        let write = ed.confirm_save_review().expect("the second press writes");
        match write {
            Write::File { path, bytes } => {
                assert_eq!(path, "C:/mhnx/test/system");
                assert_eq!(bytes, old);
            }
        }
        assert!(!ed.review_open());

        // B keeps the edit and writes nothing at all.
        assert!(ed.stage_save_review("nand:/system", &old, &changed));
        ed.cancel_save_review();
        assert!(!ed.review_open());
        assert!(ed.message.contains("中止"), "{}", ed.message);
        // Nothing was written: the save in memory is still the file on disk.
        assert_eq!(ed.save.as_ref().expect("save").to_bytes(), old);
    }

    #[test]
    fn the_console_review_carries_the_values_before_the_ranges() {
        let (mut ed, gear) = editor_with_palico_gear(&["テスト"]);
        let old = ed.save.as_ref().expect("save").to_bytes();
        // Put the weapon record on the first palico: the worn list is what
        // moves, so the review has to name the part and the box index.
        assert!(ed.palico_equip_assign(0, 0, gear[0] as u16), "{}", ed.message);
        let new = ed.save.as_ref().expect("save").to_bytes();
        assert_ne!(old, new);

        assert!(ed.stage_save_review("nand:/system", &old, &new));
        let review = ed.review.as_ref().expect("a review");
        let values = review
            .values
            .iter()
            .map(|value| value.line())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            values.contains("スロット1 オトモ #1 武器: なし → 枠 0 / オトモ武器 ID 431"),
            "{}",
            values
        );
        // The body the console draws has the section, above the ranges.
        let lines = review.lines.join("\n");
        assert!(lines.contains("■ 変更される値（旧 → 新）"), "{}", lines);
        assert!(lines.contains("オトモ #1 武器"), "{}", lines);
        let at = |needle: &str| {
            review
                .lines
                .iter()
                .position(|l| l == needle)
                .unwrap_or_else(|| panic!("no {:?} in\n{}", needle, lines))
        };
        assert!(at("■ 変更される値（旧 → 新）") < at("■ 変更される範囲"));

        // The second press still writes exactly the reviewed bytes.
        match ed.confirm_save_review().expect("the second press writes") {
            Write::File { bytes, .. } => assert_eq!(bytes, new),
        }
        assert!(!ed.review_open());
    }

    /// The review body is assembled once: the console (`lines`) and the
    /// desktop dialog (`window_lines`) come through the same function and
    /// differ only where a screen really needs it - the range offset, and
    /// what an empty summary does.
    #[test]
    fn the_window_and_the_console_read_the_same_review_body() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        let mut character = new.read_character(0).expect("a character record");
        character.name = String::from("差分テスト");
        character.funds = 123_456;
        new.write_character(&character).expect("the character writes");

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        assert!(!diff.is_empty());
        let console = diff.lines(REVIEW_RANGES);
        let window = diff.window_lines(REVIEW_RANGES);
        let heads = |lines: &[String]| {
            lines
                .iter()
                .filter(|line| line.starts_with('■'))
                .cloned()
                .collect::<Vec<String>>()
        };
        assert_eq!(heads(&console), heads(&window), "{:?}\n{:?}", console, window);
        // The console starts on the heading; the window leaves a blank line
        // above it, because the dialog already printed its own header there.
        assert_eq!(console.first().map(String::as_str), Some("■ 変更される内容"));
        assert_eq!(window.first().map(String::as_str), Some(""));
        // Every value line is the same word for word.
        let values = |lines: &[String]| {
            lines
                .iter()
                .filter(|line| line.contains(" → "))
                .cloned()
                .collect::<Vec<String>>()
        };
        assert_eq!(values(&console), values(&window));
        assert!(
            values(&console)
                .iter()
                .any(|line| line.contains("所持金: 0 z → 123456 z")),
            "{:?}",
            values(&console)
        );
        // The range line is the one wording difference. Take the offset out
        // of the window line and it is the console line exactly.
        let range = |lines: &[String]| {
            lines
                .iter()
                .find(|line| line.contains("名前") && line.contains("バイト"))
                .cloned()
                .expect("a 名前 range line")
        };
        let console_range = range(&console);
        let window_range = range(&window);
        assert!(!console_range.contains("+0x"), "{}", console_range);
        let at = window_range.find(" +0x").expect("the window names the offset");
        let end = at + window_range[at..].find(" 付近 ").expect("付近") + " 付近 ".len();
        assert_eq!(
            format!("{} {}", &window_range[..at], &window_range[end..]),
            console_range,
            "the same range in two wordings"
        );

        // A file that did not parse has no summary. The console keeps the
        // heading and puts the byte count under it; the window leaves the
        // section out, the way its dialogs always have.
        let bare = SaveDiff {
            changed: 3,
            summary: Vec::new(),
            values: Vec::new(),
            ranges: alloc::vec![DiffRange {
                slot: None,
                part: "セーブファイル",
                name: String::from("セーブファイル"),
                rel: 0,
                len: 3,
                records: None,
            }],
        };
        let console = bare.lines(REVIEW_RANGES);
        assert_eq!(console[0], "■ 変更される内容");
        assert_eq!(console[1], "・変更 3 バイト");
        let window = bare.window_lines(REVIEW_RANGES);
        assert!(
            !window.iter().any(|line| line == "■ 変更される内容"),
            "{:?}",
            window
        );
        assert_eq!(window[0], "");
        assert!(window
            .iter()
            .any(|line| line.contains("セーブファイル") && line.contains("3 バイト")));
    }
}
