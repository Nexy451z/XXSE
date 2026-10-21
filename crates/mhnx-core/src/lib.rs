//! # mhnx-core
//!
//! Monster Hunter XX (3DS / Switch) save-data and quest-data library.
//!
//! The crate is no_std + alloc compatible, so the exact same code runs in the
//! PC GUI (mhnxeditor.exe) and on the Nintendo Switch homebrew build
//! (mhnxeditor.nro).
//!
//! ## Verified facts this library is built on
//!
//! * A Switch system file is a **36 byte header + the 3DS save body**.
//!   4726188 = 36 + 4726152.
//! * The 36 byte header contains **no content checksum**. Offset 0x14 is
//!   crc32(file_name) XOR 0xFFFFFFFF (verified byte-exact against 6 real
//!   headers). Offset 0x1C is the body length.
//! * The body layout is identical on 3DS and Switch:
//!   [0, 0x126474) = shared block (the download-quest section lives here),
//!   then three character slots of 0x11E7C0 bytes each.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

/// Copy a name into a fixed width, NUL padded field.
///
/// The field is bytes, not characters, so the cut has to land on a character
/// boundary: a three byte character at the end used to be split in half and
/// every reader of the field then saw invalid UTF-8. The game itself writes
/// one byte short of the field to keep a terminator.
pub fn write_name_field(dst: &mut [u8], name: &str) {
    dst.fill(0);
    let room = dst.len().saturating_sub(1);
    let mut end = name.len().min(room);
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    dst[..end].copy_from_slice(&name.as_bytes()[..end]);
}

pub mod arc;
pub mod charm;
pub mod check;
pub mod crc;
pub mod dlc;
pub mod dlc_bundle;
pub mod dlcats;
pub mod equip;
pub mod guildcard;
pub mod myset;
pub mod palico;
pub mod equipnames;
pub mod entries;
pub mod hbedit;
pub mod itembox;
pub mod quest;
pub mod questfields;
pub mod save;
pub mod skills;

#[cfg(feature = "gamedata")]
pub mod gamedata;

pub use crc::{crc32, name_hash};
pub use save::{Save, SaveKind, BODY_SIZE, MHGU_SIZE, SLOT_STRIDE, SWITCH_HEADER_SIZE, SWITCH_SIZE};

/// Crate wide result alias.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The buffer is too small / too large for the target format.
    BadSize { got: usize, want: usize },
    /// A magic number or version field did not match.
    BadMagic { context: &'static str },
    /// The container declares something that is out of range.
    OutOfRange { context: &'static str, value: u32 },
    /// A compressed stream could not be decoded.
    Decompress,
    /// Generic parse failure with context.
    Parse(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // These reach the screen through the GUI status bar and the console
        // message line, so they are written in the language of the interface.
        match self {
            Error::BadSize { got, want } => {
                write!(f, "サイズが違います: {} バイト（期待 {} バイト）", got, want)
            }
            Error::BadMagic { context } => {
                write!(f, "形式が違います: {}", japanese_context(context))
            }
            Error::OutOfRange { context, value } => {
                write!(f, "{} が範囲外です: {}", japanese_context(context), value)
            }
            Error::Decompress => write!(f, "展開に失敗しました"),
            Error::Parse(s) => write!(f, "読み取りに失敗しました: {}", s),
        }
    }
}

/// The contexts written by this crate, in the language of the interface.
///
/// An unknown one is passed through unchanged rather than hidden: a wrong
/// description is worse than an untranslated one.
fn japanese_context(context: &'static str) -> &'static str {
    match context {
        "arc magic" => "クエストファイル (.arc)",
        "arc entry count" => "エントリ数",
        "arc name" => "エントリ名",
        "save size" => "セーブのサイズ",
        "switch header" => "Switch のヘッダ",
        "quest payload larger than a slot" => "クエストが枠に入りません",
        "quest id is not in the DLC catalogue" => "配信クエストの一覧に無いID",
        "DLC section" => "DLC 領域",
        "quest slot" => "クエストの枠",
        "character slot" => "キャラクター枠",
        "item index" => "アイテムの位置",
        "item id" => "アイテムID",
        "charm index" => "お守りの枠",
        "palico index" => "オトモの枠",
        other => other,
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}