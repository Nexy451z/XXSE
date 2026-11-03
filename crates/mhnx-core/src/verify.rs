//! Proving that a write did what it claimed.
//!
//! Every path that saves a file used to trust `write`'s return value. If the
//! bytes that landed on disk were not the bytes that were meant, nothing said
//! so: the next launch opened a save that had silently changed. These helpers
//! are the comparison, with the filesystem left to the caller so the same code
//! runs in the console build.

use alloc::vec::Vec;

/// What a round trip found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoundTrip {
    /// Offsets where the two byte strings differ, with (offset, in, out).
    pub differing: Vec<(usize, u8, u8)>,
    /// True when the lengths differ at all; the rest cannot be compared then.
    pub length_change: Option<(usize, usize)>,
}

impl RoundTrip {
    /// Nothing changed and nothing was lost.
    pub fn is_identical(&self) -> bool {
        self.length_change.is_none() && self.differing.is_empty()
    }

    /// A one line summary for a status bar.
    pub fn summary(&self) -> alloc::string::String {
        use alloc::format;
        match self.length_change {
            Some((a, b)) => format!("長さが {} → {} に変わりました", a, b),
            None if self.differing.is_empty() => alloc::string::String::from("書き込み後も一致"),
            None => format!("{} バイトが一致しません", self.differing.len()),
        }
    }

    /// The first few differences, for a message.
    pub fn examples(&self, limit: usize) -> alloc::string::String {
        use alloc::format;
        let mut s = alloc::string::String::new();
        for (i, (at, want, got)) in self.differing.iter().take(limit).enumerate() {
            if i > 0 {
                s += ", ";
            }
            s += &format!("0x{:X}: {:02X}→{:02X}", at, want, got);
        }
        s
    }
}

/// Compare what was meant to be written with what came back.
pub fn compare(written: &[u8], read_back: &[u8]) -> RoundTrip {
    let mut out = RoundTrip::default();
    if written.len() != read_back.len() {
        out.length_change = Some((written.len(), read_back.len()));
        return out;
    }
    for (i, (a, b)) in written.iter().zip(read_back.iter()).enumerate() {
        if a != b {
            out.differing.push((i, *a, *b));
        }
    }
    out
}

/// A structural check that does not need the original bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finding {
    /// The length is not one this tool understands.
    UnexpectedLength(usize),
    /// The header says the body is a different size than it is.
    HeaderLengthMismatch { header: u32, body: usize },
    /// The header's name hash does not match the file name it was given.
    HeaderNameHash,
}

impl Finding {
    /// Message for the user.
    pub fn text(self) -> alloc::string::String {
        use alloc::format;
        match self {
            Finding::UnexpectedLength(n) => format!("長さが想定外です: {} バイト", n),
            Finding::HeaderLengthMismatch { header, body } => {
                format!("ヘッダの本体長 {} が実際の {} と違います", header, body)
            }
            Finding::HeaderNameHash => alloc::string::String::from("ヘッダの名前ハッシュが不正です"),
        }
    }
}

/// Check the name hash in a Switch header against the name it was given.
///
/// A bare 3DS body has no header to check and returns nothing. This is the one
/// header field that ties the file to its name, so a file copied over another
/// one (system over system_backup, say) is caught here and nowhere else.
pub fn check_header_name(bytes: &[u8], file_name: &str) -> Vec<Finding> {
    use crate::crc::name_hash;
    use crate::save::{SWITCH_HEADER_SIZE, SWITCH_SIZE};
    let mut out = Vec::new();
    if bytes.len() != SWITCH_SIZE {
        return out;
    }
    let header = &bytes[..SWITCH_HEADER_SIZE];
    let stored = u32::from_le_bytes([header[0x14], header[0x15], header[0x16], header[0x17]]);
    if stored != name_hash(file_name.as_bytes()) {
        out.push(Finding::HeaderNameHash);
    }
    out
}

/// Everything that can be checked without the original bytes.
pub fn check_all(bytes: &[u8], file_name: &str) -> Vec<Finding> {
    let mut out = check_shape(bytes);
    out.extend(check_header_name(bytes, file_name));
    out
}

/// Check the shape of a container without modifying it.
///
/// Only checks that need no knowledge of the file's name are done here; the
/// name hash needs the file name and belongs to the caller.
pub fn check_shape(bytes: &[u8]) -> Vec<Finding> {
    let mut out = Vec::new();
    use crate::save::{BODY_SIZE, SWITCH_HEADER_SIZE, SWITCH_SIZE};
    if bytes.len() == BODY_SIZE {
        return out; // a bare 3DS body is always the right shape
    }
    if bytes.len() != SWITCH_SIZE {
        out.push(Finding::UnexpectedLength(bytes.len()));
        return out;
    }
    let header = &bytes[..SWITCH_HEADER_SIZE];
    let body_len = u32::from_le_bytes([header[0x1C], header[0x1D], header[0x1E], header[0x1F]]);
    let actual = bytes.len() - SWITCH_HEADER_SIZE;
    if body_len as usize != actual {
        out.push(Finding::HeaderLengthMismatch {
            header: body_len,
            body: actual,
        });
    }
    out
}