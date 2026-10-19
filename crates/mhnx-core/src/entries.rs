//! The remaining quest containers: .rem, .sup, .sem, .esl, .qdp, .qdl.
//!
//! Only .mib (parameters) and .gmd (text) were decoded before. The rest were
//! carried through as opaque bytes. The layouts below were measured on the real
//! quest arcs pulled out of a save, not guessed:
//!
//! - .rem (184 B) reward table
//!   - +0x00 f32  weight, always 1.0
//!   - +0x04 u32  1
//!   - +0x08 u16  slot count for the first group
//!   - +0x0A u16  slot count for the second group
//!   - +0x18      records of 4 bytes: { u16 item id, u8 count, u8 slot }
//!                terminated by 00 00 00 FF, zero padded
//! - .sup (168 B) supply box
//!   - +0x00 f32  weight, always 1.0
//!   - +0x04 u32  1
//!   - +0x08      records of 4 bytes, same shape, 40 of them
//! - .sem (32 B) one large-monster placement
//!   - +0x00 f32  weight
//!   - +0x04 u32  1
//!   - +0x08 u32  0
//!   - +0x0C u32  monster flag (0 on the dummy b_m00em000)
//!   - +0x10 f32 x, +0x14 y, +0x18 z, +0x1C w (spawn position / rotation)
//! - .esl (64 B) small-monster set list: magic "esl" + u32 count + payload
//! - .qdp (25 B) quest plus flags: same 8 byte header, rest is flags
//! - .qdl (308 B) the file list of the quest
//!   - +0x00 u32  0x434B0000
//!   - +0x04 u32  1
//!   - +0x08      records of 20 bytes: { u32 type hash, char[16] name }
//!                (308 - 8 = 300 = 15 * 20 exactly, and the 15 names match
//!                 the 15 entries of q1010165 one for one)

use alloc::string::String;
use alloc::vec::Vec;

use crate::Error;

/// One reward or supply record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemRecord {
    /// Item id (12 bit in practice, stored as u16).
    pub item: u16,
    /// Stack count.
    pub count: u8,
    /// Slot index the game shows it in.
    pub slot: u8,
}

impl ItemRecord {
    fn read(b: &[u8]) -> ItemRecord {
        ItemRecord {
            item: u16::from_le_bytes([b[0], b[1]]),
            count: b[2],
            slot: b[3],
        }
    }
    fn write(&self, b: &mut [u8]) {
        b[0..2].copy_from_slice(&self.item.to_le_bytes());
        b[2] = self.count;
        b[3] = self.slot;
    }
}

/// A .rem reward table or a .sup supply box.
///
/// Both are the same byte shape: an 8 byte header then 4 byte records. They are
/// kept as raw bytes apart from the records so everything else survives an edit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemTable {
    /// The original bytes, so untouched regions round trip exactly.
    pub raw: Vec<u8>,
    /// Offset of the first record.
    pub first: usize,
    /// How many records the container holds.
    pub capacity: usize,
    /// The records.
    pub records: Vec<ItemRecord>,
    /// Where each record sits inside the record array.
    ///
    /// A .sup is a fixed array of 40 where an empty entry does not mean the
    /// list has ended: 119 of the 316 real files hold items after a gap.
    /// Storing the original index keeps the edit aimed at the right bytes.
    pub positions: Vec<usize>,
    /// Did the file end its list with 00 00 00 FF?
    ///
    /// .rem does, .sup does not (it is a fixed array of 40 where zero means
    /// empty). Writing a terminator into a .sup changed bytes that were
    /// already correct.
    pub terminated: bool,
}

impl ItemTable {
    /// Parse a .rem or .sup payload.
    pub fn parse(raw: &[u8]) -> Result<ItemTable, Error> {
        // .rem starts its records at 0x18, .sup at 0x08.
        let first = if raw.len() == 184 { 0x18 } else { 8 };
        if raw.len() < first {
            return Err(Error::BadSize {
                got: raw.len(),
                want: first,
            });
        }
        let capacity = (raw.len() - first) / 4;
        let mut records = Vec::new();
        let mut positions = Vec::new();
        let mut terminated = false;
        // .rem is a list that ends; .sup is a fixed array, so an empty record
        // is just a free slot and the scan continues past it.
        let fixed = raw.len() != 184;
        for i in 0..capacity {
            let o = first + i * 4;
            let r = ItemRecord::read(&raw[o..o + 4]);
            // 00 00 00 FF ends the list; anything after it is padding.
            if r.item == 0 && r.count == 0 && r.slot == 0xFF {
                terminated = true;
                break;
            }
            if r.item == 0 && r.count == 0 && r.slot == 0 {
                if fixed {
                    continue;
                }
                break;
            }
            records.push(r);
            positions.push(i);
        }
        Ok(ItemTable {
            raw: raw.to_vec(),
            first,
            capacity,
            records,
            positions,
            terminated,
        })
    }

    /// Serialise back, keeping every byte the records do not cover.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.raw.clone();
        // Only write the records we actually hold. Iterating over capacity and
        // defaulting the missing ones to zero wiped real records that sit after
        // a gap - a .sup can hold two blocks separated by an empty run.
        for (i, r) in self.records.iter().enumerate() {
            let at = self.positions.get(i).copied().unwrap_or(i);
            let o = self.first + at * 4;
            if o + 4 > out.len() {
                break;
            }
            r.write(&mut out[o..o + 4]);
        }
        // Only re-terminate a list that was terminated to begin with.
        if self.terminated {
            let t = self.first + self.records.len() * 4;
            if t + 4 <= out.len() {
                out[t..t + 4].copy_from_slice(&[0, 0, 0, 0xFF]);
            }
        }
        out
    }
}

/// One small-monster wave (.esl) or placement (.sem).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SemEntry {
    /// Original bytes; the position fields are edited in place.
    pub raw: Vec<u8>,
}

impl SemEntry {
    /// Spawn position (x, y, z, w) read from +0x10.
    pub fn position(&self) -> (f32, f32, f32, f32) {
        let f = |o: usize| {
            f32::from_le_bytes([
                self.raw[o],
                self.raw[o + 1],
                self.raw[o + 2],
                self.raw[o + 3],
            ])
        };
        (f(0x10), f(0x14), f(0x18), f(0x1C))
    }
    /// Is this the b_m00em000 dummy slot the game ignores?
    pub fn is_dummy(&self) -> bool {
        self.raw.len() >= 0x10 && self.raw[0x0C..0x10] == [0, 0, 0, 0]
    }
}

/// Bytes per record in a .qdl: a u32 type hash plus a 16 byte name.
pub const QDL_RECORD: usize = 20;

/// One file reference inside a .qdl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QdlRef {
    /// Type hash of the referenced file.
    pub type_hash: u32,
    /// Base name, no extension.
    pub name: String,
}

/// A .qdl: the file list of a quest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestLink {
    /// Original bytes.
    pub raw: Vec<u8>,
    /// The referenced files.
    pub refs: Vec<QdlRef>,
}

impl QuestLink {
    /// Parse a .qdl payload.
    pub fn parse(raw: &[u8]) -> Result<QuestLink, Error> {
        if raw.len() < 8 {
            return Err(Error::BadSize {
                got: raw.len(),
                want: 8,
            });
        }
        let mut refs = Vec::new();
        let mut o = 8;
        // 20 byte records, not 16: the name field is 16 bytes wide. Reading it
        // as 12 put every record after the first out of step and produced
        // garbage like "pS%b_m15em0".
        while o + QDL_RECORD <= raw.len() {
            let type_hash =
                u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]);
            let name_bytes = &raw[o + 4..o + QDL_RECORD];
            let end = name_bytes.iter().position(|b| *b == 0).unwrap_or(16);
            if type_hash == 0 && end == 0 {
                break;
            }
            refs.push(QdlRef {
                type_hash,
                name: String::from_utf8_lossy(&name_bytes[..end]).into_owned(),
            });
            o += QDL_RECORD;
        }
        Ok(QuestLink {
            raw: raw.to_vec(),
            refs,
        })
    }
}

/// A .esl small-monster wave list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Esl {
    /// Original bytes.
    pub raw: Vec<u8>,
    /// The wave count stored at +0x04.
    pub waves: u32,
}

impl Esl {
    /// Parse an .esl payload.
    pub fn parse(raw: &[u8]) -> Result<Esl, Error> {
        if raw.len() < 8 {
            return Err(Error::BadSize {
                got: raw.len(),
                want: 8,
            });
        }
        if &raw[0..4] != b"esl\0" {
            return Err(Error::BadMagic { context: "esl" });
        }
        Ok(Esl {
            waves: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            raw: raw.to_vec(),
        })
    }
}

/// A .qdp quest-plus flag block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Qdp {
    /// Original bytes.
    pub raw: Vec<u8>,
}

impl Qdp {
    /// Parse a .qdp payload.
    pub fn parse(raw: &[u8]) -> Result<Qdp, Error> {
        if raw.len() < 8 {
            return Err(Error::BadSize {
                got: raw.len(),
                want: 8,
            });
        }
        Ok(Qdp { raw: raw.to_vec() })
    }
    /// The flag bytes after the 8 byte header.
    pub fn flags(&self) -> &[u8] {
        &self.raw[8..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_rem() -> Vec<u8> {
        // quest/test/rem/rem_620114 from the sample quest.
        let mut v = alloc::vec![0u8; 184];
        v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
        v[4..8].copy_from_slice(&1u32.to_le_bytes());
        v[8] = 1;
        v[9] = 0x0E;
        v[24..28].copy_from_slice(&[0x63, 0x01, 0x01, 0x05]);
        v[28..32].copy_from_slice(&[0x55, 0x03, 0x01, 0x08]);
        v[32..36].copy_from_slice(&[0x00, 0x00, 0x00, 0xFF]);
        v
    }

    #[test]
    fn rem_records_decode() {
        let t = ItemTable::parse(&real_rem()).unwrap();
        assert_eq!(t.first, 0x18);
        assert_eq!(t.records.len(), 2);
        assert_eq!(t.records[0].item, 0x0163);
        assert_eq!(t.records[0].count, 1);
        assert_eq!(t.records[0].slot, 5);
        assert_eq!(t.records[1].item, 0x0355);
        assert_eq!(t.records[1].slot, 8);
    }

    #[test]
    fn rem_round_trips_then_edits_one_record() {
        let raw = real_rem();
        let t = ItemTable::parse(&raw).unwrap();
        assert_eq!(t.to_bytes(), raw, "an untouched table must not move");

        let mut t2 = t.clone();
        t2.records[0].item = 42;
        t2.records[0].count = 9;
        let out = t2.to_bytes();
        assert_eq!(&out[24..28], &[42, 0, 9, 5]);
        // Everything outside the record is untouched.
        let mut expect = raw.clone();
        expect[24..28].copy_from_slice(&[42, 0, 9, 5]);
        assert_eq!(out, expect);
    }

    #[test]
    fn sup_uses_an_eight_byte_header() {
        let mut raw = alloc::vec![0u8; 168];
        raw[0..4].copy_from_slice(&1.0f32.to_le_bytes());
        raw[4..8].copy_from_slice(&1u32.to_le_bytes());
        // b4 06 03 00 -> item 0x06b4, count 3
        raw[8..12].copy_from_slice(&[0xB4, 0x06, 0x03, 0x00]);
        let t = ItemTable::parse(&raw).unwrap();
        assert_eq!(t.first, 8);
        assert_eq!(t.records[0].item, 0x06B4);
        assert_eq!(t.records[0].count, 3);
        assert_eq!(t.to_bytes(), raw);
    }

    #[test]
    fn qdl_lists_the_files() {
        // 00 00 4b 43 01 00 00 00 then 16 byte records.
        let mut raw = alloc::vec![0u8; 308];
        raw[0] = 0x00;
        raw[1] = 0x00;
        raw[2] = 0x4B;
        raw[3] = 0x43;
        raw[4..8].copy_from_slice(&1u32.to_le_bytes());
        raw[8..12].copy_from_slice(&0x2553_701Du32.to_le_bytes());
        raw[12..16].copy_from_slice(b"b_m1");
        raw[16..20].copy_from_slice(b"5em0");
        raw[20..24].copy_from_slice(b"47_0");
        raw[28..32].copy_from_slice(&0x2553_701Du32.to_le_bytes());
        raw[32..36].copy_from_slice(b"b_m0");
        let q = QuestLink::parse(&raw).unwrap();
        assert_eq!(q.refs.len(), 2, "20 byte stride");
        assert_eq!(q.refs[0].type_hash, 0x2553_701D);
        assert_eq!(q.refs[0].name, "b_m15em047_0");
        assert_eq!(q.refs[1].name, "b_m0");
    }

    #[test]
    fn sem_position_decodes() {
        let mut raw = alloc::vec![0u8; 32];
        raw[0..4].copy_from_slice(&1.0f32.to_le_bytes());
        raw[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes());
        raw[0x10..0x14].copy_from_slice(&180.0f32.to_le_bytes());
        raw[0x14..0x18].copy_from_slice(&23.0f32.to_le_bytes());
        raw[0x18..0x1C].copy_from_slice(&(-150.0f32).to_le_bytes());
        let s = SemEntry { raw };
        assert_eq!(s.position(), (180.0, 23.0, -150.0, 0.0));
        assert!(!s.is_dummy());
    }

    #[test]
    fn esl_needs_its_magic() {
        let mut raw = alloc::vec![0u8; 64];
        raw[0..4].copy_from_slice(b"esl\0");
        raw[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(Esl::parse(&raw).unwrap().waves, 2);
        raw[0] = b'X';
        assert!(Esl::parse(&raw).is_err());
    }
}
