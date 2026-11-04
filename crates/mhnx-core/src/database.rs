//! The game's own tables: what a romfs dump holds, listed and read on demand.
//!
//! The editor names things with text that ships inside the game - award and
//! request names, item, skill, armour and weapon names, the hunter's notes.
//! None of it is in this repository and none of it ever will be: this module is
//! the reader, and the only thing it knows about the data is *where* in a file
//! an entry starts.
//!
//! Nothing here touches a filesystem and nothing here caches anything. The
//! caller walks its own dump, hands over the bytes of each file it found, and
//! gets a [`Table`] back when the bytes are a table this module can read. That
//! keeps the same promise [`crate::romfstables`] makes - a missing dump is an
//! empty list, not a failure - and keeps `no_std`: the console build compiles
//! this module and never calls it.
//!
//! ## Which tables can be read
//!
//! Two containers, both already parsed elsewhere in the crate:
//!
//! * `.gmd` - the text container, read by [`crate::romfstables::gmd_strings`].
//!   Every one of the 104 `.gmd` files in a retail Japanese MHXX dump decodes
//!   completely, so every `.gmd` is listed. What differs is how the strings
//!   line up with the entries, and that is [`Layout`].
//! * `.atd` - `activityData.atd`, read by
//!   [`crate::romfstables::request_records`]. Its 184 records are numbers, not
//!   text, so a row is written out for each one.
//!
//! The other 214 files in `nativeNX/table` (`.ots`, `.ntd`, `.w00d`, `.skd`,
//! `.amskl`, ...) are binary tables this crate has no parser for. They are
//! **not** listed: a row that cannot be read is worse than a row that is not
//! there, because the list would claim coverage it does not have. The caller
//! reports how many files it skipped instead.
//!
//! ## The layouts, measured on a retail Japanese dump
//!
//! The first number is the strings in the file, the second the entries the
//! layout below gives it.
//!
//! `text
//! GC_Medal_jpn.gmd          264 -> 132     names, then the same count of descriptions
//! GC_Title_1_jpn.gmd       2618 -> 1309    ditto (1309 words of a guild-card title)
//! GC_Title_2_jpn.gmd        122 -> 121     linking words, then one spare string
//! activityData_jpn.gmd      920 -> 184     five columns per record, the first is the name
//! activityData.atd           -- -> 184     binary records
//! itemData_jpn.gmd         5982 -> 2991    name, description, name, description, ...
//! skillData_jpn.gmd         656 -> 328     ditto
//! skillTypeData_jpn.gmd     412 -> 206     ditto
//! armorSeriesData_jpn.gmd 12870 -> 6435    ten per series: five names, five descriptions
//! weapon00MsgData_jpn.gmd   822 -> 137     six per weapon: three forms, each name + text
//! otArmorData_jpn.gmd      2108 -> 1054    four per series: head, body, head text, body text
//! otWeaponData_jpn.gmd     1018 -> 509     name, description, ...
//! catSkillData_jpn.gmd      198 -> 66      three per skill: name, note, note
//! ot_skl_jpn.gmd            192 -> 96      name, description, ...
//! amuletData_jpn.gmd         22 -> 11      name, description, ...
//! hunterArtsData_jpn.gmd    764 -> 191     four per art: name twice, description twice
//! `
//!
//! Three of them are checked against numbers this crate already relies on:
//! `itemData` gives exactly `gamedata::LAST_ITEM_ID + 1` items, `GC_Medal` the
//! 132 awards of [`crate::romfstables::AWARD_COUNT`], and
//! `activityData_jpn.gmd` the 184 names [`crate::romfstables::request_names`]
//! returns. The tests assert all three, so a layout that drifts is caught
//! rather than merely shown.
//!
//! Age, quality and completeness of the *text* are the game's business: a table
//! that names its entries "dummy", "Invalid Message" or "未登録" is shown with
//! those strings. This module does not filter them out, because the row numbers
//! are what a caller looks things up by.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::romfstables;

/// How many rows the preview under the list shows.
pub const PREVIEW: usize = 50;

/// Extension of the text container, without the dot.
pub const GMD_EXT: &str = "gmd";
/// Extension of the request record table, without the dot.
pub const ATD_EXT: &str = "atd";

/// How the strings of a table line up with the entries it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Every string is one entry: a plain name or message list.
    Each,
    /// Names first, descriptions second; the split is exactly half way.
    FirstHalf,
    /// Only the first `n` strings name something, the rest is padding.
    First(usize),
    /// One name every `stride` strings, at `offset` inside each group.
    Stride {
        /// Strings per entry.
        stride: usize,
        /// Which of them is the name (0 based).
        offset: usize,
    },
    /// The first `take` strings of every `group` strings.
    Groups {
        /// Strings per series.
        group: usize,
        /// How many at the front of the series are names.
        take: usize,
    },
    /// Binary request records, not strings at all.
    Records,
}

impl Layout {
    /// The layout in the language of the interface, for the 形式 column.
    pub fn describe(self) -> String {
        match self {
            Layout::Each => String::from("全文字列"),
            Layout::FirstHalf => String::from("前半が名前"),
            Layout::First(n) => format!("先頭 {}", n),
            Layout::Stride {
                stride: 1,
                offset: 0,
            } => String::from("全文字列"),
            Layout::Stride {
                stride: 2,
                offset: 0,
            } => String::from("2 列ごと"),
            Layout::Stride {
                stride: 2,
                offset: 1,
            } => String::from("2 列の 2 番目"),
            Layout::Stride { stride, offset } => format!("{} 列の {} 番目", stride, offset + 1),
            Layout::Groups { group, take } => format!("{} 列の先頭 {}", group, take),
            Layout::Records => String::from("レコード"),
        }
    }
}

/// One table this crate knows how to read, named the way the game names it.
///
/// The catalogue is a list of **file names and layouts**, not of data: it says
/// which file holds which kind of text, and nothing about what the text is.
/// Adding a row here does not add game data to the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSpec {
    /// Extension without the dot, lower case.
    pub ext: &'static str,
    /// Lower case start of the file stem.
    pub prefix: &'static str,
    /// Lower case text the stem must also contain, or "" for nothing.
    pub needle: &'static str,
    /// What the table holds, in the language of the interface.
    pub title: &'static str,
    /// Where the names sit in the strings.
    pub layout: Layout,
}

impl TableSpec {
    /// Whether a file name is this spec's table.
    fn matches(&self, stem_lower: &str, ext: &str) -> bool {
        self.ext == ext
            && stem_lower.starts_with(self.prefix)
            && (self.needle.is_empty() || stem_lower.contains(self.needle))
    }
}

/// The tables this crate can read, in the order the list shows them.
///
/// The order is the order of a guild card and then the game's own menus: what a
/// hunter is, what they carry, what they wear, what they hunt, what they are
/// called. A file that is not in here is still listed, under its own name and
/// with every string as a row - the catalogue only adds the label and the
/// layout.
pub static TABLES: &[TableSpec] = &[
    // ---- the guild card, the requests, the titles
    TableSpec {
        ext: GMD_EXT,
        prefix: "gc_medal",
        needle: "",
        title: "勲章名",
        layout: Layout::FirstHalf,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "gc_title_1",
        needle: "",
        title: "称号（語）",
        layout: Layout::FirstHalf,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "gc_title_2",
        needle: "",
        title: "称号（接続語）",
        layout: Layout::First(121),
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "activitydata",
        needle: "",
        title: "依頼名",
        layout: Layout::Stride {
            stride: 5,
            offset: 0,
        },
    },
    TableSpec {
        ext: ATD_EXT,
        prefix: "activitydata",
        needle: "",
        title: "依頼レコード",
        layout: Layout::Records,
    },
    // ---- what a hunter carries
    TableSpec {
        ext: GMD_EXT,
        prefix: "itemdata",
        needle: "",
        title: "アイテム名",
        layout: Layout::Stride {
            stride: 2,
            offset: 0,
        },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "itemcategorytypedata",
        needle: "",
        title: "アイテムの分類",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "skilltypedata",
        needle: "",
        title: "スキル系統名",
        layout: Layout::Stride {
            stride: 2,
            offset: 0,
        },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "skilldata",
        needle: "",
        title: "スキル名",
        layout: Layout::Stride {
            stride: 2,
            offset: 0,
        },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "amuletdata",
        needle: "",
        title: "護石名",
        layout: Layout::Stride {
            stride: 2,
            offset: 0,
        },
    },
    // ---- what a hunter wears
    TableSpec {
        ext: GMD_EXT,
        prefix: "armorseriesdata",
        needle: "",
        title: "防具名",
        layout: Layout::Groups { group: 10, take: 5 },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "weapon",
        needle: "msgdata",
        title: "武器名",
        layout: Layout::Groups { group: 6, take: 1 },
    },
    // ---- what a hunter hunts
    TableSpec {
        ext: GMD_EXT,
        prefix: "hn_monsterlistmsg",
        needle: "",
        title: "モンスターリスト",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "monsterdemo",
        needle: "",
        title: "モンスター名（デモ）",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "insectdata",
        needle: "",
        title: "猟虫の名前",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "insectabirity",
        needle: "",
        title: "猟虫の能力",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "insectessenceskill",
        needle: "",
        title: "猟虫のエキス強化",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "hunterartsdata",
        needle: "",
        title: "狩技名",
        layout: Layout::Stride {
            stride: 4,
            offset: 0,
        },
    },
    // ---- the palicoes
    TableSpec {
        ext: GMD_EXT,
        prefix: "otweapondata",
        needle: "",
        title: "オトモ武器名",
        layout: Layout::Stride {
            stride: 2,
            offset: 0,
        },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "otarmordata",
        needle: "",
        title: "オトモ防具名",
        layout: Layout::Groups { group: 4, take: 2 },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "ot_skl",
        needle: "",
        title: "オトモスキル（術）",
        layout: Layout::Stride {
            stride: 2,
            offset: 0,
        },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "catskilldata",
        needle: "",
        title: "オトモスキル",
        layout: Layout::Groups { group: 3, take: 1 },
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "otname",
        needle: "",
        title: "オトモ名",
        layout: Layout::Each,
    },
    // ---- places, people, side quests
    TableSpec {
        ext: GMD_EXT,
        prefix: "npcname",
        needle: "",
        title: "NPC 名",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "mapname",
        needle: "",
        title: "マップ名",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "guestquestdata",
        needle: "",
        title: "ゲストクエスト名",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "otodokesetlist",
        needle: "",
        title: "オトモへのお届けセット",
        layout: Layout::Each,
    },
    TableSpec {
        ext: GMD_EXT,
        prefix: "otodokeconditionlist",
        needle: "",
        title: "お届けの条件",
        layout: Layout::Each,
    },
];

/// One table, read into memory.
///
/// The rows are every entry of the file in file order, not just the preview:
/// the caller filters them (a search has to reach the whole table) and shows
/// the first [`PREVIEW`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The file it came from, as the caller named it.
    pub file: String,
    /// What it holds: the catalogue's label, or the file stem when unknown.
    pub title: String,
    /// How the strings line up with the entries.
    pub layout: Layout,
    /// Size of the file on disk.
    pub bytes: usize,
    /// Strings decoded from the file (0 for a record table).
    pub strings: usize,
    /// Every entry, in file order.
    pub rows: Vec<String>,
}

impl Table {
    /// How many entries the table holds.
    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// The rows the preview shows: the first [`PREVIEW`] of them.
    pub fn preview(&self) -> &[String] {
        &self.rows[..self.rows.len().min(PREVIEW)]
    }

    /// The rows the needle picks out, with the position each has in the file.
    pub fn matching(&self, needle: &str) -> Vec<(usize, &str)> {
        filter(&self.rows, needle)
    }

    /// Whether this table answers the needle at all.
    ///
    /// A table answers when its own name or file matches, or when one of its
    /// rows does - so typing a name the list does not show still finds the
    /// table that holds it.
    pub fn answered_by(&self, needle: &str) -> bool {
        let needle = normalise(needle);
        if needle.is_empty() {
            return true;
        }
        self.title.to_ascii_lowercase().contains(&needle)
            || self.file.to_ascii_lowercase().contains(&needle)
            || self
                .rows
                .iter()
                .any(|row| row.to_ascii_lowercase().contains(&needle))
    }
}

/// The needle with the edges trimmed and ASCII letters folded.
///
/// Japanese has no case, so the fold only has to reach the Latin text a table
/// holds ("Invalid Message", "dummy", the names of a localisation).
pub fn normalise(needle: &str) -> String {
    needle.trim().to_ascii_lowercase()
}

/// The rows the needle picks out, with their positions in the file.
///
/// An empty needle matches every row, so the caller does not need a branch.
pub fn filter<'a>(rows: &'a [String], needle: &str) -> Vec<(usize, &'a str)> {
    let needle = normalise(needle);
    let mut out = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if needle.is_empty() || row.to_ascii_lowercase().contains(&needle) {
            out.push((index, row.as_str()));
        }
    }
    out
}

/// One line for the status bar: how many tables and how many entries.
pub fn summary(tables: &[Table]) -> String {
    let entries: usize = tables.iter().map(Table::count).sum();
    format!("{} テーブル / {} 件", tables.len(), entries)
}

/// The stem of a file name: everything before the last dot.
fn stem_of(file: &str) -> &str {
    match file.rfind('.') {
        Some(at) => &file[..at],
        None => file,
    }
}

/// The extension of a file name, lower case and without the dot.
fn extension_of(file: &str) -> Option<String> {
    let at = file.rfind('.')?;
    let ext = &file[at + 1..];
    if ext.is_empty() {
        None
    } else {
        Some(ext.to_ascii_lowercase())
    }
}

/// The catalogue entry for a file name, when one names it.
pub fn spec_for(file: &str) -> Option<&'static TableSpec> {
    let stem = stem_of(file).to_ascii_lowercase();
    let ext = extension_of(file)?;
    TABLES.iter().find(|spec| spec.matches(&stem, &ext))
}

/// Whether this crate can read a file with that name.
pub fn can_read(file: &str) -> bool {
    match extension_of(file) {
        Some(ext) => ext == GMD_EXT || ext == ATD_EXT,
        None => false,
    }
}

/// Turn the decoded strings of a table into its entries.
fn rows_from(strings: Vec<String>, layout: Layout) -> Vec<String> {
    match layout {
        Layout::Each | Layout::Records => strings,
        Layout::FirstHalf => {
            let half = strings.len() / 2;
            strings.into_iter().take(half).collect()
        }
        Layout::First(n) => strings.into_iter().take(n).collect(),
        Layout::Stride { stride, offset } => {
            if stride <= 1 {
                strings.into_iter().skip(offset).collect()
            } else {
                strings.into_iter().skip(offset).step_by(stride).collect()
            }
        }
        // A spec whose group or take is zero describes no entry; refusing is
        // better than dividing by zero or reading the whole table as names.
        Layout::Groups { group, take } if group == 0 || take == 0 => Vec::new(),
        Layout::Groups { group, take } => strings
            .chunks(group)
            .flat_map(|series| series.iter().take(take).cloned())
            .collect(),
    }
}

/// One row per request record: the flag pair the game keeps for it.
fn record_rows(records: &[romfstables::RequestRecord]) -> Vec<String> {
    records
        .iter()
        .map(|r| {
            format!(
                "受注 {} / 完了 {} · クエスト {}",
                r.accept_flag, r.complete_flag, r.quest_id
            )
        })
        .collect()
}

/// Read one table out of its bytes.
///
/// `file` only names the file: the language, the folder and the way it was
/// found are the caller's business. `None` means the bytes are not a table this
/// crate can read - a truncated file, another container, or nothing at all -
/// and the caller is expected to count it as skipped rather than to show it
/// empty.
pub fn read(file: &str, bytes: &[u8]) -> Option<Table> {
    let ext = extension_of(file)?;
    let spec = spec_for(file);
    let layout = match spec {
        Some(spec) => spec.layout,
        None => Layout::Each,
    };
    let title = match spec {
        Some(spec) => String::from(spec.title),
        None => String::from(stem_of(file)),
    };
    let (rows, strings) = match ext.as_str() {
        GMD_EXT => {
            let decoded = romfstables::gmd_strings(bytes);
            if decoded.is_empty() {
                return None;
            }
            let strings = decoded.len();
            (rows_from(decoded, layout), strings)
        }
        ATD_EXT => {
            let records = romfstables::request_records(bytes);
            if records.is_empty() {
                return None;
            }
            (record_rows(&records), 0)
        }
        _ => return None,
    };
    if rows.is_empty() {
        return None;
    }
    Some(Table {
        file: file.to_string(),
        title,
        layout,
        bytes: bytes.len(),
        strings,
        rows,
    })
}
