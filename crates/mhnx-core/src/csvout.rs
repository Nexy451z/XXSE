//! CSV export and import for the item box and the equipment box.
//!
//! The file is an RFC 4180 style table: a header line, a `,` between the
//! cells, `"` around any cell that holds a `,`, a `"` or a line break, and
//! `\r\n` at the end of every line.
//!
//! Nothing but ids is written. The game data tables are not shipped with this
//! crate, so a name column would be empty for everyone; an id is the part that
//! can be checked against the save and it survives a round trip unchanged.
//!
//! The two directions are deliberately not symmetric:
//!
//! * the item file lists all 2300 records, blanks included, because the box is
//!   written back from it in file order - a file holding only the used records
//!   could not reproduce the gaps between them;
//! * the equipment file lists the records that hold something, because the
//!   empty part of a 2000 record box is 2000 rows of nothing.
//!
//! A row is addressed by its position in the file, not by its first cell: the
//! `slot` / `index` cell is written for whoever reads the file (and to keep the
//! two exports self-describing), while [`parse_items`] returns just the pairs
//! the box is written from and [`apply_items`] places row *n* in record *n*.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::equip;
use crate::equipops;
use crate::itembox::{ItemSlot, MAX_COUNT, TOTAL_ITEM_SLOTS};
use crate::save::Save;

/// Header of an item CSV: box position, item id, stack count.
pub const ITEM_COLUMNS: [&str; 3] = ["slot", "id", "count"];
/// Header of an equipment CSV.
pub const EQUIP_COLUMNS: [&str; 8] = [
    "index", "kind", "id", "level", "transmog", "deco1", "deco2", "deco3",
];

/// Largest item id the 12 bit id field of a record holds.
///
/// [`crate::itembox::encode`] keeps the low 12 bits, so anything above this
/// would come back as a different item rather than fail. Refusing it here is
/// the only place the mistake can still be reported.
const MAX_ITEM_ID: i64 = 4095;
/// Largest equipment type the 5 bit kind field holds.
const MAX_KIND: i64 = 31;
/// The level field stores level - 1 in five bits, so it runs 1..=32.
const LEVEL_MIN: i64 = 1;
const LEVEL_MAX: i64 = 32;

/// Why a CSV could not be read.
///
/// Every variant carries the 1 based line the record starts on, so a file a
/// spreadsheet broke can be fixed without guessing which row is meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvError {
    /// A quoted cell runs to the end of the file without its closing quote.
    UnterminatedQuote { line: usize },
    /// A cell is empty or holds something that is not a number.
    NotANumber {
        line: usize,
        column: usize,
        value: String,
    },
    /// A number does not fit the field it is written to.
    OutOfRange {
        line: usize,
        column: usize,
        value: i64,
        min: i64,
        max: i64,
    },
    /// The row stops before a column that is needed.
    MissingColumn {
        line: usize,
        column: usize,
        name: &'static str,
    },
}

impl core::fmt::Display for CsvError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Shown in the interface, so written in its language like the crate's
        // other messages.
        match self {
            CsvError::UnterminatedQuote { line } => {
                write!(f, "{} 行目: 引用符が閉じていません", line)
            }
            CsvError::NotANumber {
                line,
                column,
                value,
            } => write!(
                f,
                "{} 行目の {} 列目が数値ではありません: {}",
                line, column, value
            ),
            CsvError::OutOfRange {
                line,
                column,
                value,
                min,
                max,
            } => write!(
                f,
                "{} 行目の {} 列目が範囲外です: {}（{}〜{}）",
                line, column, value, min, max
            ),
            CsvError::MissingColumn { line, column, name } => {
                write!(f, "{} 行目の {} 列目 ({}) がありません", line, column, name)
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CsvError {}

/// One record of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvRow {
    /// 1 based line the record starts on.
    ///
    /// A quoted cell may hold line breaks, so this is not the record number:
    /// it is the line a spreadsheet would point at.
    pub line: usize,
    /// The cells, with their quotes removed.
    pub fields: Vec<String>,
}

/// One equipment record, as a CSV row carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EquipmentRow {
    /// Box position (0..1999).
    pub index: usize,
    /// Equipment type (1..21 for the hunter's box, see
    /// [`crate::equip::type_name`]).
    pub kind: u8,
    /// Weapon, armour, talisman or decoration id.
    pub id: u16,
    /// Level, 1..=32.
    pub level: u16,
    /// Layered (transmog) id, 0 when the record has never been transmogged.
    pub transmog: u16,
    /// The three decoration sockets, 0 for an empty one.
    pub deco: [u16; 3],
}

/// What applying a file did.
///
/// Nothing is changed quietly: a count that had to be rounded is counted in
/// `clamped` and named in `messages`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyReport {
    /// Rows written to the save.
    pub applied: usize,
    /// Rows that were not written (out of range, or the record could not be
    /// written).
    pub skipped: usize,
    /// Rows whose value had to be pulled into range (an item count above 99, an
    /// equipment level outside 1..=32).
    pub clamped: usize,
    /// One line per row worth mentioning, in the language of the interface.
    pub messages: Vec<String>,
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// A position in the text that also knows which line it is on.
struct Cursor<'a> {
    text: &'a str,
    at: usize,
    line: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<char> {
        self.text.get(self.at..)?.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Consume the line break that ends a record.
    ///
    /// `\r\n` is what the writer emits; a lone `\r` or a lone `\n` is accepted
    /// too, because a spreadsheet that was told to use the platform line ending
    /// on an old Mac writes one.
    fn end_record(&mut self) {
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
    }
}

/// Read the text into records.
///
/// A line with nothing on it is not a record, and the header line is returned
/// like any other: what the first cell means is up to the caller that knows
/// which table the file holds.
pub fn parse_rows(csv: &str) -> Result<Vec<CsvRow>, CsvError> {
    // Excel writes a byte order mark when it saves UTF-8. It is not part of the
    // first header cell, and leaving it there made the header unrecognisable.
    let csv = csv.strip_prefix('\u{feff}').unwrap_or(csv);
    let mut cur = Cursor {
        text: csv,
        at: 0,
        line: 1,
    };
    let mut rows = Vec::new();
    while cur.peek().is_some() {
        let line = cur.line;
        let mut fields: Vec<String> = Vec::new();
        loop {
            let mut cell = String::new();
            if cur.peek() == Some('"') {
                cur.bump();
                loop {
                    match cur.bump() {
                        None => return Err(CsvError::UnterminatedQuote { line }),
                        Some('"') => {
                            if cur.peek() == Some('"') {
                                // A doubled quote is one quote in the cell.
                                cur.bump();
                                cell.push('"');
                            } else {
                                break;
                            }
                        }
                        Some(c) => cell.push(c),
                    }
                }
            } else {
                while let Some(c) = cur.peek() {
                    if c == ',' || c == '\r' || c == '\n' {
                        break;
                    }
                    cell.push(c);
                    cur.bump();
                }
            }
            fields.push(cell);
            match cur.peek() {
                Some(',') => {
                    cur.bump();
                }
                Some('\r') | Some('\n') => {
                    cur.end_record();
                    break;
                }
                None => break,
                Some(_) => {
                    // Text after a closing quote that no delimiter follows.
                    // Skip it so the record still ends where its line ends
                    // instead of swallowing the next record.
                    while let Some(c) = cur.peek() {
                        if c == ',' || c == '\r' || c == '\n' {
                            break;
                        }
                        cur.bump();
                    }
                    if cur.peek() == Some(',') {
                        cur.bump();
                    } else {
                        cur.end_record();
                        break;
                    }
                }
            }
        }
        if !(fields.len() == 1 && fields[0].is_empty()) {
            rows.push(CsvRow { line, fields });
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Quote a cell when it holds a character that would otherwise split the row.
fn push_cell(out: &mut String, cell: &str) {
    let needs_quotes = cell
        .chars()
        .any(|c| c == ',' || c == '"' || c == '\r' || c == '\n');
    if !needs_quotes {
        out.push_str(cell);
        return;
    }
    out.push('"');
    for c in cell.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
}

/// One CSV record, `\r\n` included.
///
/// Public because it is the other half of [`parse_rows`]: the two together are
/// what keeps a cell holding a comma, a quote or a line break intact.
pub fn csv_line(fields: &[&str]) -> String {
    let mut out = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_cell(&mut out, field);
    }
    out.push('\r');
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Cells to numbers
// ---------------------------------------------------------------------------

fn cell(row: &CsvRow, column: usize) -> &str {
    row.fields.get(column).map(|s| s.trim()).unwrap_or("")
}

/// A number out of one cell, with a message that names the line and the column.
fn number(row: &CsvRow, column: usize, name: &'static str) -> Result<i64, CsvError> {
    let raw = cell(row, column);
    if raw.is_empty() {
        return Err(CsvError::MissingColumn {
            line: row.line,
            // The caller passes the 0 based index; a reader counts from 1.
            column: column + 1,
            name,
        });
    }
    raw.parse::<i64>().map_err(|_| CsvError::NotANumber {
        line: row.line,
        column: column + 1,
        value: String::from(raw),
    })
}

fn ranged(
    row: &CsvRow,
    column: usize,
    name: &'static str,
    min: i64,
    max: i64,
) -> Result<u16, CsvError> {
    let value = number(row, column, name)?;
    if value < min || value > max {
        return Err(CsvError::OutOfRange {
            line: row.line,
            column: column + 1,
            value,
            min,
            max,
        });
    }
    Ok(value as u16)
}

/// Is this row the header line?
///
/// A row whose first cell is the name of the first column is skipped, wherever
/// it turns up: a header pasted in the middle of a file is not a broken row.
fn is_header(row: &CsvRow, first: &str) -> bool {
    cell(row, 0).eq_ignore_ascii_case(first)
}

// ---------------------------------------------------------------------------
// Item box
// ---------------------------------------------------------------------------

/// The item box of one character as CSV.
///
/// Every one of the 2300 records is written, blank ones as `slot,0,0`, so
/// [`apply_items`] can reproduce the box - including where its holes are - from
/// the file alone. A slot the save does not have yields the header and nothing
/// else.
pub fn items_to_csv(save: &Save, slot: usize) -> String {
    let mut out = csv_line(&ITEM_COLUMNS);
    let Some(items) = save.read_item_box(slot) else {
        return out;
    };
    for (index, item) in items.iter().enumerate() {
        out.push_str(&csv_line(&[
            &index.to_string(),
            &item.id.to_string(),
            &item.count.to_string(),
        ]));
    }
    out
}

/// Read an item file into the pairs the box is written from.
///
/// The `slot` cell is not needed: the rows are taken in file order and the
/// boxes this module writes have the position already in order. Extra columns
/// are ignored, and a header line is skipped.
pub fn parse_items(csv: &str) -> Result<Vec<(u16, u16)>, CsvError> {
    let mut out = Vec::new();
    for row in parse_rows(csv)? {
        if is_header(&row, ITEM_COLUMNS[0]) {
            continue;
        }
        let id = ranged(&row, 1, ITEM_COLUMNS[1], 0, MAX_ITEM_ID)?;
        let count = ranged(&row, 2, ITEM_COLUMNS[2], 0, u16::MAX as i64)?;
        out.push((id, count));
    }
    Ok(out)
}

/// Write a parsed item file back into a slot.
///
/// The box becomes the file: row *n* is record *n*, and every record the file
/// does not reach is left blank. A count above [`MAX_COUNT`] is pulled down to
/// it and the row is counted in [`ApplyReport::clamped`], never silently.
/// An all blank file is refused rather than wiping the box - that is what an
/// empty export of a broken slot would look like.
pub fn apply_items(save: &mut Save, slot: usize, rows: &[(u16, u16)]) -> ApplyReport {
    let mut report = ApplyReport::default();
    if rows.is_empty() {
        report.messages.push(String::from(
            "適用する行がありません（ボックスは変更していません）",
        ));
        return report;
    }
    if save.read_item_box(slot).is_none() {
        report.skipped = rows.len();
        report
            .messages
            .push(format!("枠 {} のアイテムボックスが読めません", slot + 1));
        return report;
    }

    let extra = rows.len().saturating_sub(TOTAL_ITEM_SLOTS);
    if extra > 0 {
        report.skipped += extra;
        report.messages.push(format!(
            "{} 行はボックスの {} 枠を超えるため無視しました",
            extra, TOTAL_ITEM_SLOTS
        ));
    }

    let mut slots = vec![ItemSlot::default(); TOTAL_ITEM_SLOTS];
    for (index, &(id, count)) in rows.iter().take(TOTAL_ITEM_SLOTS).enumerate() {
        if id as i64 > MAX_ITEM_ID {
            report.skipped += 1;
            report.messages.push(format!(
                "枠 {} のアイテムID {} は 12 ビットに入りません",
                index, id
            ));
            continue;
        }
        let mut count = count;
        if count > MAX_COUNT {
            report.clamped += 1;
            report.messages.push(format!(
                "枠 {} の個数 {} を {} に丸めました",
                index, count, MAX_COUNT
            ));
            count = MAX_COUNT;
        }
        slots[index] = ItemSlot {
            id,
            count: count as u8,
        };
        report.applied += 1;
    }

    if rows.len() < TOTAL_ITEM_SLOTS {
        report.messages.push(format!(
            "{} 行だけなので、残り {} 枠は空きになります",
            rows.len(),
            TOTAL_ITEM_SLOTS - rows.len()
        ));
    }

    if let Err(e) = save.write_item_box(slot, &slots) {
        report.messages.push(format!("書き込めませんでした: {}", e));
        report.skipped += report.applied;
        report.applied = 0;
    }
    report
}

// ---------------------------------------------------------------------------
// Equipment box
// ---------------------------------------------------------------------------

/// The equipment box of one character as CSV.
///
/// Only the records that hold something are written; the empty part of the box
/// is 2000 rows of nothing and the box is written by position, not by packing
/// the rows.
pub fn equipment_to_csv(save: &Save, slot: usize) -> String {
    let mut out = csv_line(&EQUIP_COLUMNS);
    let Some(box_bytes) = save.equipment_box(slot) else {
        return out;
    };
    for index in 0..equip::EQUIP_COUNT {
        let at = index * equip::EQUIP_RECORD;
        let Some(record) = box_bytes.get(at..at + equip::EQUIP_RECORD) else {
            break;
        };
        let Some(equipment) = equip::decode_record(index, record) else {
            break;
        };
        // `is_empty` is the type byte, which is what the equipment list of the
        // editor shows too. A record with a type is listed even if it is the
        // only byte set.
        if equipment.is_empty() {
            continue;
        }
        let deco = equip::Equipment::decorations(index, record);
        out.push_str(&csv_line(&[
            &index.to_string(),
            &equipment.kind.to_string(),
            &equipment.id.to_string(),
            &equipment.level.to_string(),
            &equipment.transmog.to_string(),
            &deco[0].to_string(),
            &deco[1].to_string(),
            &deco[2].to_string(),
        ]));
    }
    out
}

/// Read an equipment file.
///
/// All eight columns are needed: the level and the layered id are part of the
/// record, and a row that left them out would overwrite them with whatever the
/// default were. Extra columns are ignored and a header line is skipped.
pub fn parse_equipment(csv: &str) -> Result<Vec<EquipmentRow>, CsvError> {
    let mut out = Vec::new();
    for row in parse_rows(csv)? {
        if is_header(&row, EQUIP_COLUMNS[0]) {
            continue;
        }
        out.push(EquipmentRow {
            index: ranged(
                &row,
                0,
                EQUIP_COLUMNS[0],
                0,
                (equip::EQUIP_COUNT - 1) as i64,
            )? as usize,
            kind: ranged(&row, 1, EQUIP_COLUMNS[1], 0, MAX_KIND)? as u8,
            id: ranged(&row, 2, EQUIP_COLUMNS[2], 0, u16::MAX as i64)?,
            level: ranged(&row, 3, EQUIP_COLUMNS[3], LEVEL_MIN, LEVEL_MAX)?,
            transmog: ranged(&row, 4, EQUIP_COLUMNS[4], 0, u16::MAX as i64)?,
            deco: [
                ranged(&row, 5, EQUIP_COLUMNS[5], 0, u16::MAX as i64)?,
                ranged(&row, 6, EQUIP_COLUMNS[6], 0, u16::MAX as i64)?,
                ranged(&row, 7, EQUIP_COLUMNS[7], 0, u16::MAX as i64)?,
            ],
        });
    }
    Ok(out)
}

/// Write parsed equipment rows back into a slot.
///
/// A row for a record that already holds something goes through
/// [`equipops::replace`] when its type or id moved, and through the field
/// helpers of [`crate::equip`] otherwise, so the bytes below +0x0C (a
/// talisman's skills, above all) are left as they were. The layered id's source
/// level is not in the file, so the one the record carries is kept.
///
/// A row for a blank record is created with [`equipops::create`], which always
/// uses the first blank record: the file is written by position but a record
/// cannot be placed at an arbitrary blank index, so when that lands somewhere
/// else the message says where it went.
pub fn apply_equipment(save: &mut Save, slot: usize, rows: &[EquipmentRow]) -> ApplyReport {
    let mut report = ApplyReport::default();
    if rows.is_empty() {
        report.messages.push(String::from("適用する行がありません"));
        return report;
    }
    for row in rows {
        if row.index >= equip::EQUIP_COUNT {
            report.skipped += 1;
            report.messages.push(format!(
                "枠 {} は装備ボックスの外です（0〜{}）",
                row.index,
                equip::EQUIP_COUNT - 1
            ));
            continue;
        }
        if row.kind == 0 {
            // The export never writes one; a hand edit that did means "empty"
            // and wiping a record is not something a table import should do
            // without being asked.
            report.skipped += 1;
            report.messages.push(format!(
                "枠 {} は種類が空きなので書きませんでした",
                row.index
            ));
            continue;
        }
        let Some(current) = save.equipment_at(slot, row.index) else {
            report.skipped += 1;
            report
                .messages
                .push(format!("枠 {} の装備ボックスが読めません", row.index));
            continue;
        };
        let blank = save
            .equipment_box(slot)
            .map(|box_bytes| {
                let at = row.index * equip::EQUIP_RECORD;
                box_bytes
                    .get(at..at + equip::EQUIP_RECORD)
                    .map(equip::record_is_blank)
                    .unwrap_or(true)
            })
            .unwrap_or(true);

        let level = row.level.clamp(LEVEL_MIN as u16, LEVEL_MAX as u16);
        if level != row.level {
            report.clamped += 1;
            report.messages.push(format!(
                "枠 {} のレベル {} を {} に丸めました",
                row.index, row.level, level
            ));
        }

        let target = if blank {
            match equipops::create(save, slot, row.kind, row.id, level) {
                Ok(created) => {
                    if created != row.index {
                        report.messages.push(format!(
                            "枠 {} は空きだったため、#{} に作成しました",
                            row.index, created
                        ));
                    }
                    created
                }
                Err(e) => {
                    report.skipped += 1;
                    report
                        .messages
                        .push(format!("枠 {}: {}", row.index, e.text()));
                    continue;
                }
            }
        } else if current.kind != row.kind || current.id != row.id {
            match equipops::replace(save, slot, row.index, row.kind, row.id, level) {
                Ok(()) => row.index,
                Err(e) => {
                    report.skipped += 1;
                    report
                        .messages
                        .push(format!("枠 {}: {}", row.index, e.text()));
                    continue;
                }
            }
        } else {
            row.index
        };

        // The layer id keeps the source level the record already had: it is not
        // one of the columns, and inventing one would change a field the file
        // never described.
        let source_level = current.transmog_level;
        let mut written = false;
        if let Some(record) = save.equipment_record_mut(slot, target) {
            written = equip::set_level(record, level);
            written &= equip::set_transmog(record, row.transmog, source_level);
            for (socket, id) in row.deco.iter().enumerate() {
                written &= equip::set_decoration(record, socket, *id);
            }
        }
        if written {
            report.applied += 1;
        } else {
            report.skipped += 1;
            report
                .messages
                .push(format!("枠 {} に書き込めませんでした", target));
        }
    }
    report
}
