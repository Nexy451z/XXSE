//! MHXX quest archive (.arc) reader / writer.
//!
//! Container layout (all little endian):
//!
//! - offset 0x00  u8[4]  magic "ARC" + NUL
//! - offset 0x04  u16    version (observed 0x11 = 17 in all 141 files)
//! - offset 0x06  u16    entry count
//! - offset 0x08  u32    zero
//! - then entry_count records of 80 bytes:
//!   - +0x00  u8[64]  name (backslash separated, NO extension), NUL padded
//!   - +0x40  u32     type hash  (identifies the extension / payload type)
//!   - +0x44  u32     compressed size (bytes stored at offset)
//!   - +0x48  u32     bit30 set | uncompressed size (low 30 bits)
//!   - +0x4C  u32     offset of the compressed payload from the file start
//!
//! The payload area starts at align32(12 + 80 * entry_count), but the payloads
//! inside it are packed back to back with no padding. Measured on the shipped
//! archives: q1010165 has 15 entries at offsets 1216, 1237, 1280, 1301, ...
//! whose gaps are exactly the previous compressed sizes.
//!
//! Each payload is a zlib (RFC 1950) stream.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::Error;

/// Archive magic.
pub const ARC_MAGIC: [u8; 4] = *b"ARC\0";
/// Archive version seen in every shipped file.
pub const ARC_VERSION: u16 = 0x11;
/// Size of one directory entry.
pub const ENTRY_SIZE: usize = 80;
/// Size of the file header.
pub const HEADER_SIZE: usize = 12;

/// Known payload type hashes.
pub const TYPE_HASHES: &[(u32, &str)] = &[
    (0x2553_701D, "sem"),
    (0x32CA_92F8, "esl"),
    (0x5B3C_302D, "rem"),
    (0x5453_9AEE, "sup"),
    (0x26BE_C21C, "qdp"),
    (0x00E0_BB1C, "qdl"),
    // Verified against quest_open/q1010165_...: the 249 byte fixed record is
    // .mib (0x1BBFD18E) and the text container is .gmd (0x242BB29A).
    (0x1BBF_D18E, "mib"),
    (0x242B_B29A, "gmd"),
];

/// Extension for a type hash, if known.
pub fn extension_for(type_hash: u32) -> Option<&'static str> {
    TYPE_HASHES.iter().find(|(h, _)| *h == type_hash).map(|(_, e)| *e)
}

/// One file inside an archive.
///
/// Equality ignores the original compressed stream, so a parsed entry compares
/// equal to the same entry built from scratch.
#[derive(Debug, Clone, Default)]
pub struct ArcEntry {
    /// Backslash separated path (no extension).
    pub name: String,
    /// Payload type hash.
    pub type_hash: u32,
    /// Uncompressed payload.
    pub data: Vec<u8>,
    /// The compressed bytes exactly as they appeared in the file, plus the
    /// bytes they decode to.
    ///
    /// zlib output is not canonical: recompressing the same data gives a
    /// different, usually larger, stream. Keeping the original means an entry
    /// nobody edited is written back byte for byte, so opening a quest,
    /// changing one field and writing it back only changes that field instead
    /// of reshaping the whole archive.
    pub original: Option<(Vec<u8>, Vec<u8>)>,
    /// Where that stream started in the file.
    ///
    /// Not every archive starts its first payload at align32(directory end):
    /// the shipped q1010165 begins 16 bytes later. Each entry stores its own
    /// offset, so the original gaps are remembered and reproduced when nothing
    /// moved.
    pub original_offset: Option<u32>,
}

impl PartialEq for ArcEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.type_hash == other.type_hash && self.data == other.data
    }
}
impl Eq for ArcEntry {}

impl ArcEntry {
    /// The original file name including the extension inferred from the hash.
    pub fn file_name(&self) -> String {
        match extension_for(self.type_hash) {
            Some(ext) => alloc::format!("{}.{}", self.name.replace('\\', "/"), ext),
            None => self.name.clone(),
        }
    }
}

/// A parsed quest archive.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Arc {
    /// Format version.
    pub version: u16,
    /// The word at +0x08, which the header documentation used to call zero.
    ///
    /// Every shipped archive carries 0xFFFFFFFE there. The writer used to
    /// leave the slot zeroed, so a repack differed from the original in those
    /// four bytes alone: harmless in testing, but not a faithful copy.
    pub header8: u32,
    /// Entries, in file order.
    pub entries: Vec<ArcEntry>,
}

fn align32(v: usize) -> usize {
    (v + 31) & !31
}

/// Smallest power-of-two style bound check used by the reader.
fn read_u32(b: &[u8], off: usize) -> Result<u32, Error> {
    if off + 4 > b.len() {
        return Err(Error::OutOfRange {
            context: "arc read",
            value: off as u32,
        });
    }
    Ok(u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]))
}
impl Arc {
    /// Parse an archive from raw bytes.
    pub fn parse(bytes: &[u8]) -> Result<Arc, Error> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::BadSize {
                got: bytes.len(),
                want: HEADER_SIZE,
            });
        }
        if bytes[0..4] != ARC_MAGIC {
            return Err(Error::BadMagic { context: "arc" });
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        let header8 = read_u32(bytes, 8).unwrap_or(0xFFFF_FFFE);
        let count = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let dir_end = HEADER_SIZE + ENTRY_SIZE * count;
        if bytes.len() < dir_end {
            return Err(Error::BadSize {
                got: bytes.len(),
                want: dir_end,
            });
        }
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let e = HEADER_SIZE + ENTRY_SIZE * i;
            let raw_name = &bytes[e..e + 64];
            let end = raw_name.iter().position(|&c| c == 0).unwrap_or(64);
            let name = String::from_utf8_lossy(&raw_name[..end]).into_owned();
            let type_hash = read_u32(bytes, e + 0x40)?;
            let csize = read_u32(bytes, e + 0x44)? as usize;
            let usize_flags = read_u32(bytes, e + 0x48)?;
            let usize_ = (usize_flags & 0x3FFF_FFFF) as usize;
            let offset = read_u32(bytes, e + 0x4C)? as usize;
            if offset.checked_add(csize).map(|v| v > bytes.len()).unwrap_or(true) {
                return Err(Error::OutOfRange {
                    context: "arc entry offset",
                    value: offset as u32,
                });
            }
            let payload = &bytes[offset..offset + csize];
            let data = miniz_oxide::inflate::decompress_to_vec_zlib(payload)
                .map_err(|_| Error::Decompress)?;
            if usize_ != 0 && data.len() != usize_ {
                return Err(Error::Parse("arc entry uncompressed size mismatch"));
            }
            entries.push(ArcEntry {
                name,
                type_hash,
                original: Some((payload.to_vec(), data.clone())),
                original_offset: Some(offset as u32),
                data,
            });
        }
        Ok(Arc {
            version,
            header8,
            entries,
        })
    }

    /// Serialise back to raw bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.to_bytes_with_level(6)
    }

    /// Serialise with an explicit zlib compression level (0..=10).
    ///
    /// The level only applies to entries this object has to compress. An entry
    /// that still matches what was parsed is written with the original stream,
    /// so re-saving an untouched archive is byte identical whatever level is
    /// asked for. Parse the bytes again if a real re-compress is wanted.
    pub fn to_bytes_with_level(&self, level: u8) -> Result<Vec<u8>, Error> {
        self.write(level, true)
    }

    /// Write the archive with the payloads packed from the directory's end.
    ///
    /// The quest archives in romfs start their data at 0x8000 and leave about
    /// 31KB of zeroes in front of it. That padding is what made a shipped
    /// quest look far too big for a save slot: the payloads themselves are
    /// only one to three kilobytes. Dropping it is not a re-compression, so
    /// the streams are kept exactly as they are and only their offsets move.
    pub fn to_bytes_compact(&self) -> Result<Vec<u8>, Error> {
        let mut out = self.write(6, false)?;
        // The archives in romfs carry 0xFFFFFFFE at +0x08 and the ones the game
        // keeps in a save carry zero: 60 of each checked, no exceptions. A
        // compacted archive is meant to go into a save, so it takes the save
        // form rather than the value that came from romfs.
        if out.len() >= 12 {
            out[8..12].fill(0);
        }
        Ok(out)
    }

    fn write(&self, level: u8, keep_gaps: bool) -> Result<Vec<u8>, Error> {
        let count = self.entries.len();
        if count > u16::MAX as usize {
            return Err(Error::OutOfRange {
                context: "arc entry count",
                value: count as u32,
            });
        }
        let data_start = align32(HEADER_SIZE + ENTRY_SIZE * count);
        let mut payloads: Vec<Vec<u8>> = Vec::with_capacity(count);
        let mut offsets: Vec<usize> = Vec::with_capacity(count);
        let mut cursor = data_start;
        for e in &self.entries {
            // Reuse the stream the game wrote whenever the payload still
            // matches, so an untouched entry costs nothing and an edited one
            // only moves its own bytes.
            let unchanged = matches!(&e.original, Some((_, decoded)) if *decoded == e.data);
            if unchanged && keep_gaps {
                // Honour any gap the original file had before this payload.
                if let Some(off) = e.original_offset {
                    if (off as usize) > cursor {
                        cursor = off as usize;
                    }
                }
            }
            let c = match &e.original {
                Some((packed, _)) if unchanged => packed.clone(),
                _ => miniz_oxide::deflate::compress_to_vec_zlib(&e.data, level),
            };
            offsets.push(cursor);
            cursor += c.len();
            payloads.push(c);
        }
        let total = cursor;
        let mut out = vec![0u8; total];
        out[0..4].copy_from_slice(&ARC_MAGIC);
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6..8].copy_from_slice(&(count as u16).to_le_bytes());
        out[8..12].copy_from_slice(&self.header8.to_le_bytes());
        for (i, e) in self.entries.iter().enumerate() {
            let base = HEADER_SIZE + ENTRY_SIZE * i;
            let nb = e.name.as_bytes();
            let n = nb.len().min(63);
            out[base..base + n].copy_from_slice(&nb[..n]);
            out[base + 0x40..base + 0x44].copy_from_slice(&e.type_hash.to_le_bytes());
            out[base + 0x44..base + 0x48].copy_from_slice(&(payloads[i].len() as u32).to_le_bytes());
            out[base + 0x48..base + 0x4C].copy_from_slice(
                &(0x4000_0000u32 | (e.data.len() as u32 & 0x3FFF_FFFF)).to_le_bytes(),
            );
            out[base + 0x4C..base + 0x50].copy_from_slice(&(offsets[i] as u32).to_le_bytes());
            out[offsets[i]..offsets[i] + payloads[i].len()].copy_from_slice(&payloads[i]);
        }
        Ok(out)
    }

    /// Find an entry whose name ends with the given path fragment.
    pub fn find(&self, needle: &str) -> Option<&ArcEntry> {
        self.entries.iter().find(|e| e.name.ends_with(needle))
    }
    /// Mutable lookup.
    pub fn find_mut(&mut self, needle: &str) -> Option<&mut ArcEntry> {
        self.entries.iter_mut().find(|e| e.name.ends_with(needle))
    }
    /// Entry by extension.
    pub fn by_ext(&self, ext: &str) -> Option<&ArcEntry> {
        self.entries
            .iter()
            .find(|e| extension_for(e.type_hash) == Some(ext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build paths from parts: a Windows path inside a raw string is too easy
    /// to mangle, and a mangled one makes the test skip silently.
    fn sample_quest() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from("C:/");
        for part in ["anyWorkspace", "nx", "mhxx", "save_editor", "mhxx_quetool", "quest"] {
            p.push(part);
        }
        p.push("q1010165.arc");
        p
    }

    /// zlib output is not canonical, so this only works because the writer
    /// reuses the stream a parsed entry remembers. Repacking used to add 210
    /// bytes of inter-payload padding and recompress everything.
    #[test]
    fn reparsing_and_repacking_is_byte_identical() {
        let src = sample_quest();
        assert!(src.exists(), "sample quest missing at {}", src.display());
        let bytes = std::fs::read(&src).unwrap();
        let a = Arc::parse(&bytes).unwrap();
        let again = a.to_bytes().unwrap();
        assert_eq!(again.len(), bytes.len(), "repack changed the file size");
        assert_eq!(again, bytes, "repack changed the bytes");

        // An edited entry moves, but only its own bytes.
        let mut b = Arc::parse(&bytes).unwrap();
        let mib = b
            .entries
            .iter()
            .position(|e| extension_for(e.type_hash) == Some("mib"))
            .unwrap();
        b.entries[mib].data[0x14] ^= 0xFF;
        let edited = b.to_bytes().unwrap();
        assert_ne!(edited, bytes);
        assert!(
            edited.len() <= bytes.len() + 256,
            "an edit must not reshape the archive ({} -> {})",
            bytes.len(),
            edited.len()
        );
    }

    #[test]
    fn round_trip_synthetic() {
        let a = Arc {
            version: ARC_VERSION,
            header8: 0xFFFF_FFFE,
            entries: vec![
                ArcEntry {
                    name: String::from("questData\\questData_1010001"),
                    type_hash: 0x1BBF_D18E,
                    data: vec![0xAB; 249],
                    original: None,
                    original_offset: None,
                },
                ArcEntry {
                    name: String::from("questData\\questData_1010001_jpn"),
                    type_hash: 0x242B_B29A,
                    data: b"GMD\0hello\0world\0".to_vec(),
                    original: None,
                    original_offset: None,
                },
            ],
        };
        let bytes = a.to_bytes().unwrap();
        // The payload area starts at align32(12 + 80 * entries) and the first
        // entry's offset field is at HEADER_SIZE + 0x4C.
        let p = HEADER_SIZE + 0x4C;
        let first_off =
            u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as usize;
        assert_eq!(first_off, align32(HEADER_SIZE + ENTRY_SIZE * 2));
        let back = Arc::parse(&bytes).unwrap();
        assert_eq!(back.entries.len(), 2);
        assert_eq!(back.entries[0].data, a.entries[0].data);
        assert_eq!(back.entries[1].data, a.entries[1].data);
        assert_eq!(
            back.entries[1].file_name(),
            "questData/questData_1010001_jpn.gmd"
        );
    }

    /// Payloads are packed with no per-entry padding; only the first one sits
    /// on the 32 byte boundary.
    #[test]
    fn payloads_are_contiguous() {
        let a = Arc {
            version: ARC_VERSION,
            header8: 0xFFFF_FFFE,
            entries: (0..3)
                .map(|i| ArcEntry {
                    name: alloc::format!("e{}", i),
                    type_hash: 0x2553_701D,
                    data: alloc::vec![i as u8; 20],
                    original: None,
                    original_offset: None,
                })
                .collect(),
        };
        let bytes = a.to_bytes().unwrap();
        let mut offs = Vec::new();
        for i in 0..3 {
            let p = HEADER_SIZE + ENTRY_SIZE * i + 0x4C;
            offs.push(u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]));
        }
        let first = align32(HEADER_SIZE + ENTRY_SIZE * 3) as u32;
        assert_eq!(offs[0], first);
        let c0 = u32::from_le_bytes([
            bytes[HEADER_SIZE + 0x44],
            bytes[HEADER_SIZE + 0x45],
            bytes[HEADER_SIZE + 0x46],
            bytes[HEADER_SIZE + 0x47],
        ]);
        assert_eq!(offs[1], offs[0] + c0, "no padding between payloads");
    }
}