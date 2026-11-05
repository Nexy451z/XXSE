//! Names that live in the user's romfs, not in this repository.
//!
//! The editor shows award (勲章) titles and village request (依頼) names. Those
//! are game text, so this crate never ships them: the caller hands over the
//! bytes of two files from its own MHXX dump and gets the names back.
//!
//! Nothing here reads the filesystem, and nothing here depends on a particular
//! directory layout. A missing or unreadable table therefore surfaces as an
//! empty list, which is the same answer the GUI already knows how to show.
//!
//! ## `.gmd` — the text container
//!
//! Verified against all 104 `.gmd` files in a retail Japanese MHXX dump, and
//! against the `.gmd` inside all 141 shipped quest archives.
//!
//! ```text
//!     0x00  "GMD\0"
//!     0x04  version word (0x0001_0302 in every shipped file)
//!     0x08  0
//!     0x0C  tag, constant inside one file
//!     0x10  0
//!     0x14  count, only in files that also carry the optional line table
//!     0x18  number of strings                    <- used
//!     0x1C  size of that optional line table (0 when absent)
//!     0x20  byte length of the string block      <- used
//!     0x24  key length                           <- used
//!     0x28  key bytes, then one NUL
//!     ...   optional 20 byte line records and their ASCII line names
//!     tail  `0x20` bytes: `0x18` NUL terminated UTF-8 strings
//! ```
//!
//! The string block is always the **tail** of the file, so `len - 0x20` is
//! where it starts. Splitting from `0x28 + key length + 1` instead — which is
//! enough for the quest `.gmd` of `quest.rs` — walks into the line table of
//! the tables in this module (`GC_Medal` carries 264 line records before its
//! text) and returns binary noise. Both formulas agree on every file that has
//! no line table, so the tail is used first and the after-the-key position is
//! the fallback for a header whose block length is unusable.
//!
//! ## `activityData.atd` — the request records
//!
//! ```text
//!     0x00  constant 00 00 A0 40
//!     0x04  u32 record count (184)
//!     0x08  records, 55 bytes each = (file size - 8) / count
//!             +0x00  u32 index, the record's own position
//!             +0x04  u8  kind: 0 on the 167 quest rows, 1 or 2 on the rows
//!                         that carry no quest, 3 on the closing sentinel
//!             +0x16  u32 quest this request points at (0 unless kind is 0)
//!             +0x33  u16 accept flag
//!             +0x35  u16 complete flag, always accept + 1
//! ```
//!
//! A record's kind and its quest id agree: the 17 rows with a zero quest id
//! are exactly the 16 kind 1/2 rows and the sentinel. Those rows still own a
//! flag pair, so a caller that only wants quests filters on the kind.
//!
//! Every record owns a consecutive flag pair: accept `N`, complete `N + 1`.
//! The numbers start at 300 and are **not** dense — the game skips values and
//! the second band starts at 1270 — so the pair has to be read from the record
//! rather than derived from its index. The last record is an all zero
//! sentinel, and it is the only one whose flags are both clear.
//!
//! ## `activityData_jpn.gmd` — the request names
//!
//! Five strings per record, in record order: name, reward, and three filler
//! slots ("dummy" in most rows). 184 x 5 = 920 strings. Only the first column
//! is a name, so `request_names` returns one string per record.
//!
//! ## `ot_skl_jpn.gmd` — the otomon skills (術)
//!
//! 192 strings: one name and one description per skill, 96 of each, in the
//! game's own skill-id order. Measured on a retail Japanese dump: all 96 names
//! are **identical, position for position**, to `gamedata::PALICO_SKILL_NAMES`
//! entries 0..96 (that table carries one extra "?" row at 96 whose skill the
//! file does not name). Every name is one line, none is empty, and the
//! descriptions wrap where the names do not, so the two columns cannot be
//! confused.
//!
//! The file is therefore a verified source for the **skill** names and not for
//! the 58 support actions: none of `gamedata::PALICO_SUPPORT_MOVES` is a whole
//! string of it. Eleven of them do occur *inside* the descriptions
//! ("笛上手の術" mentions 回復笛, "ビースト延長の術" mentions ビースト変化, ...),
//! which is why the column is read by stride and never by searching the file:
//! a search would hand back a support action as if it were a skill.
//!
//! ## `catSkillData_jpn.gmd` — the 66 cat skills
//!
//! 198 strings: a name and two note columns per row, 66 rows. The names are
//! unique and are the ネコの… set ("ネコの解体術【大】", "ネコの医療術", ...),
//! which is **not** the table the editor shows: `PALICO_SKILL_NAMES` holds the
//! 96 術 names above, and the only string the two lists have in common is the
//! "スキル無し" placeholder. The reader is kept so the table list shows the names
//! of this file; they are not adopted as an editor name column.
//!
//! ## `HN_MonsterListMsg_jpn.gmd` — the hunter's note monster list
//!
//! 220 strings: ten interface labels ("モンスターリスト", "狩猟数(捕獲数)",
//! "最大記録", "未登録", "%d(%d)", ...) followed by 105 pairs of description and
//! hunting tip, in the hunter's note order - the Deviants come first, so it is
//! not the save's hunt-record order either.
//!
//! **The file holds no monster names at all**, and neither does any other table
//! of the dump. Measured over all 388 `.gmd` files of a retail Japanese dump
//! (129 real names of the hunt record, the seven 不明 rows and "ROBOT" left out):
//!
//! * 73 of the 129 occur as a whole string somewhere, but no file holds more
//!   than 57 of them - `GC_Title_1_jpn` 57 (guild-card titles),
//!   `MonsterDemo_jpn` 43, `GC_background_jpn` 10, `OtMenuMsg_jpn` 2. A 137 row
//!   name table is not among them.
//! * 52 more occur only inside longer strings (item names such as
//!   "リオレイア希少種の鱗", this file's descriptions, ...), where taking them out
//!   again would be guessing which part is the name.
//! * 4 occur nowhere at all: 怒り喰らうイビルジョー, ミラボレアス【紅龍】,
//!   ミラボレアス【祖龍】, 猛り爆ぜるブラキディオス - the editor composes those
//!   from a base name and a variant word, and the dump does not spell them out.
//!
//! The only list of names is `MonsterDemo_jpn.gmd`: 96 strings in 48 pairs of an
//! alias and a name, covering 48 monsters, which is neither the 137 rows of the
//! save nor their order. The monster names therefore stay on the bundled table;
//! this parser exists to read the descriptions, not to replace the names with
//! something the dump does not say.

use alloc::string::String;
use alloc::vec::Vec;

/// Magic at the start of every `.gmd`.
pub const GMD_MAGIC: [u8; 4] = *b"GMD\0";
/// Size of the fixed `.gmd` header, before the key string.
pub const GMD_HEADER_SIZE: usize = 0x28;
/// Offset of the string count.
pub const GMD_COUNT_OFFSET: usize = 0x18;
/// Offset of the byte length of the string block.
pub const GMD_BLOCK_LEN_OFFSET: usize = 0x20;
/// Offset of the key length.
pub const GMD_KEY_LEN_OFFSET: usize = 0x24;

/// Number of awards the guild card medal table lists.
pub const AWARD_COUNT: usize = 132;
/// Strings in the medal table: 132 names followed by 132 descriptions.
pub const AWARD_STRINGS: usize = 2 * AWARD_COUNT;

/// Header of `activityData.atd`, before the first record.
pub const ATD_HEADER_SIZE: usize = 8;
/// Fixed size of one `activityData.atd` record.
pub const ATD_RECORD_SIZE: usize = 55;
/// Offset of the record count inside the `.atd` header.
pub const ATD_COUNT_OFFSET: usize = 4;
/// Offset of the record kind inside a record.
pub const ATD_KIND_OFFSET: usize = 0x04;
/// Offset of the quest id inside a record.
pub const ATD_QUEST_ID_OFFSET: usize = 0x16;
/// Offset of the accept flag inside a record.
pub const ATD_ACCEPT_OFFSET: usize = 0x33;
/// Offset of the complete flag inside a record.
pub const ATD_COMPLETE_OFFSET: usize = 0x35;

/// Strings per `activityData` record: name, reward, then three fillers.
pub const REQUEST_STRIDE: usize = 5;

/// Read a little endian `u32` without ever indexing out of bounds.
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let bytes = b.get(off..end)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Read a little endian `u16` without ever indexing out of bounds.
fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let bytes = b.get(off..end)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Where the string block starts and how many strings it holds.
///
/// `None` means the bytes are not a `.gmd` at all, or the header does not
/// describe a block that fits inside the buffer.
fn gmd_text_region(b: &[u8]) -> Option<(usize, usize)> {
    if b.len() < GMD_HEADER_SIZE || b[..4] != GMD_MAGIC {
        return None;
    }
    let count = u32_at(b, GMD_COUNT_OFFSET)? as usize;
    let block_len = u32_at(b, GMD_BLOCK_LEN_OFFSET)? as usize;
    let key_len = u32_at(b, GMD_KEY_LEN_OFFSET)? as usize;
    let after_key = GMD_HEADER_SIZE.checked_add(key_len)?.checked_add(1)?;
    if after_key > b.len() {
        return None;
    }
    // The block is the tail of the file, which is the only description that
    // survives the optional line table. Files without one also agree with the
    // after-the-key start, so that stays as the fallback for a header whose
    // length field is unusable.
    if block_len > 0 && block_len <= b.len() {
        let by_block = b.len() - block_len;
        if by_block >= after_key {
            return Some((by_block, count));
        }
    }
    Some((after_key, count))
}

/// Extract every string of a `.gmd` file, in file order.
///
/// Returns an empty list for anything that is not a NUL terminated UTF-8
/// `.gmd`; bytes that do not decode are replaced rather than rejected, so a
/// damaged table still shows up as text instead of vanishing.
pub fn gmd_strings(bytes: &[u8]) -> Vec<String> {
    let (start, count) = match gmd_text_region(bytes) {
        Some(region) => region,
        None => return Vec::new(),
    };
    if count == 0 {
        return Vec::new();
    }
    let text = &bytes[start..];
    // `count` comes straight out of the file. A string plus its terminator is
    // at least two bytes, so the text bounds how many can exist; reserving the
    // raw count would ask for gigabytes from a corrupt header.
    let mut out: Vec<String> = Vec::with_capacity(count.min(text.len() / 2 + 1));
    let mut cur: Vec<u8> = Vec::new();
    for &c in text {
        if c == 0 {
            out.push(String::from_utf8_lossy(&cur).into_owned());
            cur.clear();
            if out.len() == count {
                break;
            }
        } else {
            cur.push(c);
        }
    }
    // A block whose last terminator is missing still yields that string.
    if out.len() < count && !cur.is_empty() {
        out.push(String::from_utf8_lossy(&cur).into_owned());
    }
    out
}

/// The award names of the guild card medal table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AwardNames {
    /// 132 single line names, in table order, or empty when the file does not
    /// look like `GC_Medal`.
    pub names: Vec<String>,
}

/// Extract the 132 award names from `GC_Medal_jpn.gmd` (or any other
/// language's `GC_Medal`).
///
/// The table holds names first and descriptions second, 132 of each, so the
/// split is positional. A file with any other string count is treated as "no
/// names" rather than half parsed: showing the descriptions in place of the
/// names would be worse than showing none.
pub fn award_names(gmd: &[u8]) -> AwardNames {
    let mut strings = gmd_strings(gmd);
    if strings.len() != AWARD_STRINGS {
        return AwardNames { names: Vec::new() };
    }
    strings.truncate(AWARD_COUNT);
    AwardNames { names: strings }
}

/// One record of `activityData.atd`: a village request and its two flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestRecord {
    /// Position of the record in the table (0 based).
    ///
    /// The record repeats this number at +0x00; the stored value equals the
    /// position in every shipped record, and the position is what a caller
    /// needs to line the record up with `request_names`.
    pub index: usize,
    /// What the record describes (+0x04).
    ///
    /// `0` on the 167 rows that point at a quest, `1` or `2` on the 16 rows
    /// that do not, `3` on the closing sentinel. Filter on this before using
    /// `quest_id`: every row with a non-zero kind stores zero there.
    pub kind: u8,
    /// Quest this request points at (+0x16); zero unless `kind` is zero.
    pub quest_id: u32,
    /// Flag the game sets when the request is accepted (+0x33).
    pub accept_flag: u16,
    /// Flag the game sets when the request is completed (+0x35).
    pub complete_flag: u16,
}

/// Parse the request records of `activityData.atd`.
///
/// Returns an empty list when the header is missing or claims more records
/// than the buffer holds: a truncated file would otherwise produce records
/// made of whatever followed it.
pub fn request_records(atd: &[u8]) -> Vec<RequestRecord> {
    let count = match u32_at(atd, ATD_COUNT_OFFSET) {
        Some(count) => count as usize,
        None => return Vec::new(),
    };
    let needed = match count
        .checked_mul(ATD_RECORD_SIZE)
        .and_then(|n| n.checked_add(ATD_HEADER_SIZE))
    {
        Some(needed) if needed <= atd.len() => needed,
        _ => return Vec::new(),
    };
    let table = &atd[ATD_HEADER_SIZE..needed];
    let mut out = Vec::with_capacity(count);
    for (index, record) in table.chunks_exact(ATD_RECORD_SIZE).enumerate() {
        out.push(RequestRecord {
            index,
            kind: record.get(ATD_KIND_OFFSET).copied().unwrap_or(0),
            quest_id: u32_at(record, ATD_QUEST_ID_OFFSET).unwrap_or(0),
            accept_flag: u16_at(record, ATD_ACCEPT_OFFSET).unwrap_or(0),
            complete_flag: u16_at(record, ATD_COMPLETE_OFFSET).unwrap_or(0),
        });
    }
    out
}

/// Extract the request names from `activityData_jpn.gmd` (or any other
/// language's `activityData`).
///
/// Each record occupies `REQUEST_STRIDE` consecutive strings and only the
/// first one is the name. A block whose length is not a multiple of the stride
/// is refused, because every column would shift.
pub fn request_names(gmd: &[u8]) -> Vec<String> {
    let strings = gmd_strings(gmd);
    let mut rows = strings.chunks_exact(REQUEST_STRIDE);
    let names: Vec<String> = rows.by_ref().map(|row| row[0].clone()).collect();
    // A block whose length is not a multiple of the stride would shift every
    // column, so a ragged one is refused rather than partly read.
    if rows.remainder().is_empty() {
        names
    } else {
        Vec::new()
    }
}

/// Skills named by `ot_skl_jpn.gmd` (96 on a retail Japanese dump).
pub const OTOMO_SKILL_COUNT: usize = 96;
/// Strings per otomon skill: its name, then its description.
pub const OTOMO_SKILL_STRIDE: usize = 2;

/// The otomon skill names of `ot_skl_jpn.gmd` (or another language's
/// `ot_skl`), in the game's own skill-id order.
///
/// The file is a name and a description per skill, so the names are every
/// other string from the first. A file whose string count is not exactly
/// `OTOMO_SKILL_COUNT` skills is refused whole rather than half read: a table
/// with a different number of rows would put a description where a name
/// belongs, and every id past the first would silently shift.
///
/// Verified against a retail Japanese dump: the 96 names are identical, one for
/// one, to `gamedata::PALICO_SKILL_NAMES` 0..96. This is the **skill** table
/// (`体力強化の術【小】`, ...), not the 58 support actions: none of those is a
/// whole string of the file (eleven are mentioned inside the descriptions, which
/// is why the names must be read by stride and never by searching).
pub fn otomo_skill_names(gmd: &[u8]) -> Vec<String> {
    let strings = gmd_strings(gmd);
    if strings.len() != OTOMO_SKILL_COUNT * OTOMO_SKILL_STRIDE {
        return Vec::new();
    }
    strings.into_iter().step_by(OTOMO_SKILL_STRIDE).collect()
}

/// Skills named by `catSkillData_jpn.gmd` (66 on a retail Japanese dump).
pub const CAT_SKILL_COUNT: usize = 66;
/// Strings per cat skill: its name, then two note columns.
pub const CAT_SKILL_STRIDE: usize = 3;

/// The cat skill names of `catSkillData_jpn.gmd` (or another language's
/// `catSkillData`), in file order.
///
/// The names are the ネコの… set (`ネコの解体術【大】`, ...), which is not the
/// table the editor names palico skills with - see the module docs. They are
/// read here so the table list can show this file's rows, and a file whose
/// string count is not exactly `CAT_SKILL_COUNT` rows is refused whole.
pub fn cat_skill_names(gmd: &[u8]) -> Vec<String> {
    let strings = gmd_strings(gmd);
    if strings.len() != CAT_SKILL_COUNT * CAT_SKILL_STRIDE {
        return Vec::new();
    }
    strings.into_iter().step_by(CAT_SKILL_STRIDE).collect()
}

/// Interface labels `HN_MonsterListMsg_jpn.gmd` starts with.
pub const MONSTER_LIST_HEADER: usize = 10;
/// Monster entries that follow the labels: a description and a hunting tip each.
pub const MONSTER_LIST_ROWS: usize = 105;

/// One entry of the hunter's note monster list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterListRow {
    /// What the monster is, as the note's list shows it.
    pub description: String,
    /// How to hunt it.
    pub tip: String,
}

/// The monster list entries of `HN_MonsterListMsg_jpn.gmd` (or another
/// language's `HN_MonsterListMsg`), in the order the note lists them.
///
/// The file is **not** a name table: ten labels, then a description and a tip
/// per monster, in the note's own order (the Deviant entries come first, so it
/// is not the save's hunt-record order either). A file that is not exactly
/// `MONSTER_LIST_HEADER + MONSTER_LIST_ROWS * 2` strings is refused whole: a
/// shifted pair would put a tip where a description belongs.
pub fn monster_list_rows(gmd: &[u8]) -> Vec<MonsterListRow> {
    let strings = gmd_strings(gmd);
    if strings.len() != MONSTER_LIST_HEADER + MONSTER_LIST_ROWS * 2 {
        return Vec::new();
    }
    let mut out: Vec<MonsterListRow> = Vec::with_capacity(MONSTER_LIST_ROWS);
    for pair in strings[MONSTER_LIST_HEADER..].chunks_exact(2) {
        out.push(MonsterListRow {
            description: pair[0].clone(),
            tip: pair[1].clone(),
        });
    }
    out
}
