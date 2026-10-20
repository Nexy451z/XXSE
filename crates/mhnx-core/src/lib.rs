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

pub mod arc;
pub mod charm;
pub mod check;
pub mod crc;
pub mod dlc;
pub mod dlcats;
pub mod equip;
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
        match self {
            Error::BadSize { got, want } => write!(f, "bad size: got {} want {}", got, want),
            Error::BadMagic { context } => write!(f, "bad magic in {}", context),
            Error::OutOfRange { context, value } => write!(f, "{}: value {} out of range", context, value),
            Error::Decompress => write!(f, "decompression failed"),
            Error::Parse(s) => write!(f, "parse error: {}", s),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}