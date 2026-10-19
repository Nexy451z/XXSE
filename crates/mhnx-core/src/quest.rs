//! Quest payload formats found inside a quest .arc.
//!
//! Sizes and headers below were verified against the 141 shipped quest
//! archives in mhxx_quetool/quest and the extracted sample in quest_open.

use alloc::string::String;
use alloc::vec::Vec;

use crate::Error;

/// Fixed size of a .mib record.
pub const MIB_SIZE: usize = 249;
/// Offset of the quest id inside a .mib record.
pub const MIB_QUEST_ID_OFFSET: usize = 0x0C;

/// A .mib file: fixed 249 byte quest parameter record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mib {
    /// Raw bytes.
    pub raw: Vec<u8>,
}

impl Mib {
    /// Wrap raw bytes, validating the size.
    pub fn from_bytes(b: &[u8]) -> Result<Self, Error> {
        if b.len() != MIB_SIZE {
            return Err(Error::BadSize {
                got: b.len(),
                want: MIB_SIZE,
            });
        }
        Ok(Mib { raw: b.to_vec() })
    }
    /// The quest id stored at offset 0x0C.
    pub fn quest_id(&self) -> u32 {
        u32::from_le_bytes([
            self.raw[MIB_QUEST_ID_OFFSET],
            self.raw[MIB_QUEST_ID_OFFSET + 1],
            self.raw[MIB_QUEST_ID_OFFSET + 2],
            self.raw[MIB_QUEST_ID_OFFSET + 3],
        ])
    }
    /// Set the quest id field.
    pub fn set_quest_id(&mut self, id: u32) {
        self.raw[MIB_QUEST_ID_OFFSET..MIB_QUEST_ID_OFFSET + 4].copy_from_slice(&id.to_le_bytes());
    }
    /// Read a little endian u32 at an arbitrary offset.
    pub fn u32_at(&self, off: usize) -> Option<u32> {
        if off + 4 > self.raw.len() {
            return None;
        }
        Some(u32::from_le_bytes([
            self.raw[off],
            self.raw[off + 1],
            self.raw[off + 2],
            self.raw[off + 3],
        ]))
    }
    /// Write a little endian u32 at an arbitrary offset.
    pub fn set_u32_at(&mut self, off: usize, v: u32) -> bool {
        if off + 4 > self.raw.len() {
            return false;
        }
        self.raw[off..off + 4].copy_from_slice(&v.to_le_bytes());
        true
    }
    /// Read a single byte.
    pub fn u8_at(&self, off: usize) -> Option<u8> {
        self.raw.get(off).copied()
    }
    /// Write a single byte.
    pub fn set_u8_at(&mut self, off: usize, v: u8) -> bool {
        match self.raw.get_mut(off) {
            Some(s) => {
                *s = v;
                true
            }
            None => false,
        }
    }
}

/// A .gmd text file.
///
/// Container layout (verified against the 141 shipped quests):
///
/// ```text
///     +0x00  u8[4]  "GMD\0"
///     +0x04  u32    version, always 0x00010302
///     +0x08  u32    0
///     +0x0C  u32    constant 0x432F44B3 (NOT a hash of the key)
///     +0x10  u32    0
///     +0x14  u32    0
///     +0x18  u32    number of strings in the text block
///     +0x1C  u32    0
///     +0x20  u32    size of the text block in bytes
///     +0x24  u32    length of the key string (without the NUL)
///     +0x28  ...    key string + NUL  (e.g. "questData_1010165")
///     ...           text block: count NUL terminated UTF-8 strings
/// ```
///
/// The quest text order is: title, client, request, objective, fail condition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Gmd {
    /// Format version word (0x00010302 in every shipped file).
    pub version: u32,
    /// The constant stored at +0x0C.
    pub tag: u32,
    /// Resource key, e.g. "questData_1010165".
    pub key: String,
    /// The decoded strings, in file order.
    pub strings: Vec<String>,
}

/// GMD magic.
pub const GMD_MAGIC: [u8; 4] = *b"GMD\0";
/// Version word seen in every shipped .gmd.
pub const GMD_VERSION: u32 = 0x0001_0302;
/// Constant stored at +0x0C.
pub const GMD_TAG: u32 = 0x432F_44B3;
/// Size of the fixed part of the header, before the key string.
pub const GMD_HEADER_SIZE: usize = 0x28;

fn read_u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

impl Gmd {
    /// Parse a .gmd payload.
    pub fn from_bytes(b: &[u8]) -> Result<Self, Error> {
        if b.len() < GMD_HEADER_SIZE {
            return Err(Error::BadSize {
                got: b.len(),
                want: GMD_HEADER_SIZE,
            });
        }
        if b[0..4] != GMD_MAGIC {
            return Err(Error::BadMagic { context: "gmd" });
        }
        let version = read_u32_at(b, 0x04);
        let tag = read_u32_at(b, 0x0C);
        let count = read_u32_at(b, 0x18) as usize;
        let key_len = read_u32_at(b, 0x24) as usize;
        let key_end = GMD_HEADER_SIZE + key_len;
        if key_end + 1 > b.len() {
            return Err(Error::OutOfRange {
                context: "gmd key length",
                value: key_len as u32,
            });
        }
        let key = String::from_utf8_lossy(&b[GMD_HEADER_SIZE..key_end]).into_owned();
        let text = &b[key_end + 1..];
        // count comes straight out of the file. Reserving it unfiltered asked
        // for ~96 GiB from a 64 byte payload; it only survived here because
        // Windows overcommits, and a 32-bit or console build would abort.
        // No string can be longer than the text that is actually present.
        let mut strings = Vec::with_capacity(count.min(text.len() + 1));
        let mut cur: Vec<u8> = Vec::new();
        for &c in text {
            if c == 0 {
                strings.push(String::from_utf8_lossy(&cur).into_owned());
                cur.clear();
                if strings.len() == count {
                    break;
                }
            } else {
                cur.push(c);
            }
        }
        if strings.len() < count && !cur.is_empty() {
            strings.push(String::from_utf8_lossy(&cur).into_owned());
        }
        Ok(Gmd {
            version,
            tag,
            key,
            strings,
        })
    }

    /// Serialise back to a .gmd payload.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text: Vec<u8> = Vec::new();
        for s in &self.strings {
            text.extend_from_slice(s.as_bytes());
            text.push(0);
        }
        let mut out = Vec::with_capacity(GMD_HEADER_SIZE + self.key.len() + 1 + text.len());
        out.extend_from_slice(&GMD_MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&self.tag.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.key.len() as u32).to_le_bytes());
        out.extend_from_slice(self.key.as_bytes());
        out.push(0);
        out.extend_from_slice(&text);
        out
    }

    /// Index of every known quest text line, in file order.
    pub const LABELS: [&'static str; 7] = [
        "クエスト名",
        "依頼主",
        "依頼内容",
        "主なモンスター",
        "成功条件",
        "失敗条件",
        "サブ成功条件",
    ];

    /// Quest name (string 0).
    pub fn title(&self) -> Option<&str> {
        self.strings.first().map(|s| s.as_str())
    }
    /// Requesting client (string 1).
    pub fn client(&self) -> Option<&str> {
        self.strings.get(1).map(|s| s.as_str())
    }
    /// Request text (string 2).
    pub fn request(&self) -> Option<&str> {
        self.strings.get(2).map(|s| s.as_str())
    }
    /// Main monster (string 3).
    pub fn main_monster(&self) -> Option<&str> {
        self.strings.get(3).map(|s| s.as_str())
    }
    /// Success condition (string 4).
    pub fn success(&self) -> Option<&str> {
        self.strings.get(4).map(|s| s.as_str())
    }
    /// Fail condition (string 5).
    pub fn fail_condition(&self) -> Option<&str> {
        self.strings.get(5).map(|s| s.as_str())
    }
    /// Sub-quest success condition (string 6).
    pub fn sub_success(&self) -> Option<&str> {
        self.strings.get(6).map(|s| s.as_str())
    }
}

/// Size of a .rem (reward table) payload.
pub const REM_SIZE: usize = 184;
/// Size of a .sup (supply item) payload.
pub const SUP_SIZE: usize = 168;
/// Number of reward / supply records.
pub const REWARD_SLOTS: usize = 40;
/// Offset of the first reward record.
pub const REWARD_DATA_OFFSET: usize = 24;
/// Offset of the first supply record.
pub const SUP_DATA_OFFSET: usize = 8;

/// One reward or supply entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RewardSlot {
    /// Item id.
    pub id: u16,
    /// Stack count.
    pub count: u8,
    /// Probability (rewards) or unknown (supplies).
    pub probability: u8,
}

/// A .rem reward table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rem {
    /// Raw bytes.
    pub raw: Vec<u8>,
}

/// A .sup supply item table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sup {
    /// Raw bytes.
    pub raw: Vec<u8>,
}

macro_rules! table_impl {
    ($ty:ident, $size:ident, $offset:ident) => {
        impl $ty {
            /// Wrap raw bytes, validating the size.
            pub fn from_bytes(b: &[u8]) -> Result<Self, Error> {
                if b.len() != $size {
                    return Err(Error::BadSize {
                        got: b.len(),
                        want: $size,
                    });
                }
                Ok($ty { raw: b.to_vec() })
            }
            /// All 40 records.
            pub fn slots(&self) -> [RewardSlot; REWARD_SLOTS] {
                let mut out = [RewardSlot::default(); REWARD_SLOTS];
                for (i, s) in out.iter_mut().enumerate() {
                    let o = $offset + i * 4;
                    s.id = u16::from_le_bytes([self.raw[o], self.raw[o + 1]]);
                    s.count = self.raw[o + 2];
                    s.probability = self.raw[o + 3];
                }
                out
            }
            /// Records that actually carry an item.
            pub fn used(&self) -> Vec<RewardSlot> {
                self.slots().into_iter().filter(|s| s.id != 0).collect()
            }
            /// Total number of items handed out.
            pub fn total_count(&self) -> u32 {
                self.used().iter().map(|s| s.count as u32).sum()
            }
            /// Set one record.
            pub fn set_slot(&mut self, index: usize, slot: RewardSlot) -> bool {
                if index >= REWARD_SLOTS {
                    return false;
                }
                let o = $offset + index * 4;
                self.raw[o..o + 2].copy_from_slice(&slot.id.to_le_bytes());
                self.raw[o + 2] = slot.count;
                self.raw[o + 3] = slot.probability;
                true
            }
            /// The raw bytes.
            pub fn as_bytes(&self) -> &[u8] {
                &self.raw
            }
        }
    };
}

table_impl!(Rem, REM_SIZE, REWARD_DATA_OFFSET);
table_impl!(Sup, SUP_SIZE, SUP_DATA_OFFSET);

/// A fixed or variable length payload we do not fully model yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPayload {
    /// Raw bytes.
    pub raw: Vec<u8>,
}

impl RawPayload {
    /// Wrap raw bytes.
    pub fn new(b: &[u8]) -> Self {
        RawPayload { raw: b.to_vec() }
    }
    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.raw
    }
}

/// Decode a quest id into its parts.
///
/// The 7 digit id is 1 | kind(2) | rank(1) | serial(3), e.g. 1010165.
pub fn decode_quest_id(id: u32) -> QuestIdParts {
    QuestIdParts {
        raw: id,
        kind: ((id / 10_000) % 100) as u8,
        rank: ((id / 1_000) % 10) as u8,
        serial: (id % 1_000) as u16,
    }
}

/// Decoded quest id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestIdParts {
    /// The raw 7 digit id.
    pub raw: u32,
    /// 01 = normal quest, 11 = palico (nyan) quest.
    pub kind: u8,
    /// 0 = low rank, 1 = high rank, 2 = G rank / training.
    pub rank: u8,
    /// Serial number inside the group.
    pub serial: u16,
}

impl QuestIdParts {
    /// Build an id from its parts.
    pub fn build(kind: u8, rank: u8, serial: u16) -> u32 {
        1_000_000 + (kind as u32) * 10_000 + (rank as u32) * 1_000 + serial as u32
    }
    /// Human readable summary.
    pub fn describe(&self) -> String {
        let kind = match self.kind {
            1 => "normal",
            11 => "palico",
            _ => "unknown",
        };
        let rank = match self.rank {
            0 => "low",
            1 => "high",
            2 => "G / training",
            _ => "unknown",
        };
        alloc::format!("{} / {} / #{}", kind, rank, self.serial)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmd_round_trip() {
        let g = Gmd {
            version: GMD_VERSION,
            tag: GMD_TAG,
            key: String::from("questData_1010001"),
            strings: alloc::vec![
                String::from("Test Quest"),
                String::from("Guildmaster"),
                String::from("Hunt 2 Great Maccao"),
                String::from("Great Maccao x2"),
                String::from("Hunt 2 Great Maccao"),
                String::from("Cart 3 times"),
                String::from("Deliver 5 Kelbi Horns"),
            ],
        };
        let b = g.to_bytes();
        assert_eq!(&b[0..4], &GMD_MAGIC);
        let text_len: usize = g.strings.iter().map(|s| s.len() + 1).sum();
        assert_eq!(b.len(), GMD_HEADER_SIZE + g.key.len() + 1 + text_len);
        let back = Gmd::from_bytes(&b).unwrap();
        assert_eq!(back, g);
        assert_eq!(back.title(), Some("Test Quest"));
        assert_eq!(back.client(), Some("Guildmaster"));
        assert_eq!(back.main_monster(), Some("Great Maccao x2"));
        assert_eq!(back.success(), Some("Hunt 2 Great Maccao"));
    }

    #[test]
    fn quest_id_parts() {
        let p = decode_quest_id(1010165);
        assert_eq!(p.kind, 1);
        assert_eq!(p.rank, 0);
        assert_eq!(p.serial, 165);
        assert_eq!(QuestIdParts::build(1, 0, 165), 1010165);
        assert_eq!(decode_quest_id(1111007).kind, 11);
        assert_eq!(decode_quest_id(1111007).rank, 1);
    }
}