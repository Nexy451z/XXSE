//! Item box codec.
//!
//! The 2300 item slots are stored as a bit stream of 19 bit records
//! (7 bit count + 12 bit item id) inside 5463 bytes, and the byte order of
//! that block is reversed (Array.Reverse in the reference editor).
//! Records are emitted for slot index 2299 down to 0, and the stream carries
//! 4 leading padding bits.

use alloc::vec;
use alloc::vec::Vec;

/// Number of item slots in the box.
pub const TOTAL_ITEM_SLOTS: usize = 2300;
/// Raw byte length of the item box block.
pub const ITEM_BOX_SIZE: usize = 5463;
const BITS_PER_RECORD: usize = 19;
const COUNT_BITS: usize = 7;
const ID_BITS: usize = 12;

/// One item box entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemSlot {
    /// Item id (12 bit).
    pub id: u16,
    /// Stack count (7 bit).
    pub count: u8,
}

fn bits_to_u32(bits: &[u8]) -> u32 {
    let mut v = 0u32;
    for &b in bits {
        v = (v << 1) | (b as u32 & 1);
    }
    v
}

/// Decode the raw 5463 byte block into 2300 item slots.
pub fn decode(raw: &[u8]) -> Vec<ItemSlot> {
    let mut out = vec![ItemSlot::default(); TOTAL_ITEM_SLOTS];
    if raw.len() < ITEM_BOX_SIZE {
        return out;
    }
    let mut bits: Vec<u8> = Vec::with_capacity(ITEM_BOX_SIZE * 8);
    for &b in raw[..ITEM_BOX_SIZE].iter().rev() {
        for k in (0..8).rev() {
            bits.push((b >> k) & 1);
        }
    }
    let mut pos = 4usize;
    for idx in (0..TOTAL_ITEM_SLOTS).rev() {
        if pos + BITS_PER_RECORD > bits.len() {
            break;
        }
        let count = bits_to_u32(&bits[pos..pos + COUNT_BITS]) as u8;
        let id = bits_to_u32(&bits[pos + COUNT_BITS..pos + BITS_PER_RECORD]) as u16;
        out[idx] = ItemSlot { id, count };
        pos += BITS_PER_RECORD;
    }
    out
}

/// Encode 2300 item slots back into the raw 5463 byte block.
pub fn encode(slots: &[ItemSlot]) -> Vec<u8> {
    let mut bits: Vec<u8> = Vec::with_capacity(ITEM_BOX_SIZE * 8);
    bits.extend_from_slice(&[0u8; 4]);
    for idx in (0..TOTAL_ITEM_SLOTS).rev() {
        let s = slots.get(idx).copied().unwrap_or_default();
        for k in (0..COUNT_BITS).rev() {
            bits.push(((s.count as u32 >> k) & 1) as u8);
        }
        for k in (0..ID_BITS).rev() {
            bits.push(((s.id as u32 >> k) & 1) as u8);
        }
    }
    while bits.len() % 8 != 0 {
        bits.push(0);
    }
    let mut raw = vec![0u8; ITEM_BOX_SIZE];
    for (i, chunk) in bits.chunks(8).enumerate() {
        if i >= ITEM_BOX_SIZE {
            break;
        }
        raw[i] = bits_to_u32(chunk) as u8;
    }
    raw.reverse();
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut slots = vec![ItemSlot::default(); TOTAL_ITEM_SLOTS];
        slots[0] = ItemSlot { id: 1, count: 99 };
        slots[2299] = ItemSlot { id: 4095, count: 127 };
        slots[1000] = ItemSlot { id: 2638, count: 5 };
        let raw = encode(&slots);
        assert_eq!(raw.len(), ITEM_BOX_SIZE);
        let back = decode(&raw);
        assert_eq!(back[0], slots[0]);
        assert_eq!(back[2299], slots[2299]);
        assert_eq!(back[1000], slots[1000]);
    }
}
