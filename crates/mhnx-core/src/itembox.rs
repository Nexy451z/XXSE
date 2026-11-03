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

impl ItemSlot {
    /// A record nothing is stored in.
    ///
    /// The id is the whole marker. The count of a blank record is not read by
    /// the game, and the box this tool opens carries a stale one in a few
    /// places, so it is deliberately not part of the test.
    pub fn is_blank(&self) -> bool {
        self.id == 0
    }
}

/// The largest stack one slot holds.
///
/// The count field is seven bits wide and a decode does produce 127 for a
/// record the game never wrote (the round trip test below does exactly that).
/// What the game itself stores is 99: the item box never shows more, and the
/// save checker flags a stack above it. Every operation in this module keeps
/// a stack at or below this number, so an edit made here always looks like
/// something the game could have produced.
pub const MAX_COUNT: u16 = 99;

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

// ---------------------------------------------------------------------------
// Item box operations
//
// Everything below is a pure function on a decoded box, so the same code runs
// in the GUI, on the console and in the tests. A "blank" record is one whose
// id is 0; its count is not meaningful and is never trusted.
// ---------------------------------------------------------------------------

/// How many of an item the box holds in total.
///
/// Stacks of the same id are added together, wherever they sit. Zero is the
/// blank marker rather than an item, so asking for it returns 0.
pub fn count_of(items: &[ItemSlot], id: u16) -> u32 {
    if id == 0 {
        return 0;
    }
    items
        .iter()
        .filter(|s| s.id == id)
        .map(|s| s.count as u32)
        .sum()
}

/// Add `count` of `id` to the box, returning how many actually fit.
///
/// Existing stacks are topped up first, then the remainder opens new stacks in
/// blank records, keeping the box order. A stack never grows past
/// [`MAX_COUNT`], so the answer is short of `count` when the box runs out of
/// room (no blank record left, or every stack of this id already full).
///
/// Id 0 is the blank marker, not an item, and adds nothing.
pub fn add(items: &mut [ItemSlot], id: u16, count: u16) -> u32 {
    if id == 0 || count == 0 {
        return 0;
    }
    let wanted = count as u32;
    let mut left = wanted;

    // Top up the stacks that are already there.
    for slot in items.iter_mut() {
        if left == 0 {
            break;
        }
        if slot.id == id && (slot.count as u32) < MAX_COUNT as u32 {
            let room = MAX_COUNT as u32 - slot.count as u32;
            let take = room.min(left);
            slot.count += take as u8;
            left -= take;
        }
    }

    // Then put what is left into blank records.
    for slot in items.iter_mut() {
        if left == 0 {
            break;
        }
        if slot.is_blank() {
            let take = left.min(MAX_COUNT as u32);
            *slot = ItemSlot {
                id,
                count: take as u8,
            };
            left -= take;
        }
    }

    wanted - left
}

/// Set every used record to `max` (at most [`MAX_COUNT`]).
///
/// Returns how many records actually changed: one that already held the
/// number is not counted, so the answer matches what the user will see move.
/// Blank records are left alone.
pub fn fill_all(items: &mut [ItemSlot], max: u16) -> usize {
    let target = max.min(MAX_COUNT) as u8;
    let mut changed = 0;
    for slot in items.iter_mut() {
        if !slot.is_blank() && slot.count != target {
            slot.count = target;
            changed += 1;
        }
    }
    changed
}

/// Merge stacks of the same item into one record each, returning how many
/// records were freed.
///
/// The first record of an id keeps its place and takes the sum of the others,
/// which are blanked. Because a stack cannot hold more than [`MAX_COUNT`],
/// an item the box holds more than 99 of keeps 99 in that record and the
/// overflow is dropped; that is why the box total can shrink. The book-keeping
/// the game does with the box does not allow a single stack above the cap, and
/// writing one would be flagged as tampering.
/// What a dedupe would do, without doing it.
///
/// Merging stacks cannot keep more than [`MAX_COUNT`] of one item, so an id
/// the box holds more than that of loses the excess. In the sample save 185 of
/// the 187 duplicated ids are over the cap, which makes this worth showing
/// before the merge rather than after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DedupePreview {
    /// Records that would be freed.
    pub freed: usize,
    /// Items that would be lost to the per-stack cap.
    pub lost: u32,
}

/// Work out what dedupe would free and what it would drop.
pub fn dedupe_preview(items: &[ItemSlot]) -> DedupePreview {
    // Run the real merge on a copy and compare the totals. Re-deriving the
    // arithmetic by hand is how the first version of this counted the same
    // duplicate once per member of the run instead of once.
    let total = |s: &[ItemSlot]| -> u32 {
        s.iter()
            .filter(|slot| !slot.is_blank())
            .map(|slot| slot.count as u32)
            .sum()
    };
    let before = total(items);
    let mut copy = items.to_vec();
    let freed = dedupe(&mut copy);
    let after = total(&copy);
    DedupePreview {
        freed,
        lost: before.saturating_sub(after),
    }
}

pub fn dedupe(items: &mut [ItemSlot]) -> usize {
    let mut freed = 0;
    for i in 0..items.len() {
        if items[i].is_blank() {
            continue;
        }
        let id = items[i].id;
        let mut sum = items[i].count as u32;
        for j in (i + 1)..items.len() {
            if items[j].id == id {
                sum += items[j].count as u32;
                items[j] = ItemSlot::default();
                freed += 1;
            }
        }
        items[i].count = sum.min(MAX_COUNT as u32) as u8;
    }
    freed
}

/// Move every used record to the front, keeping their order, and return how
/// many records changed place.
///
/// The records after the last used one are blanked, so a stale count in a
/// blank one does not survive. The set of items and their counts are
/// untouched: this only removes the holes, it never merges stacks.
pub fn compact(items: &mut [ItemSlot]) -> usize {
    let mut write = 0;
    let mut moved = 0;
    for read in 0..items.len() {
        if items[read].is_blank() {
            continue;
        }
        if write != read {
            items[write] = items[read];
            moved += 1;
        }
        write += 1;
    }
    for slot in items[write..].iter_mut() {
        *slot = ItemSlot::default();
    }
    moved
}

/// Pull counts above `max` (at most [`MAX_COUNT`]) back down, returning how
/// many records changed.
///
/// A stack above the cap is what a broken save or a hand edit leaves behind;
/// the game would show it but the checker flags it. Blank records are left
/// alone, because their count is not read.
pub fn clamp_counts(items: &mut [ItemSlot], max: u16) -> usize {
    let cap = max.min(MAX_COUNT) as u8;
    let mut changed = 0;
    for slot in items.iter_mut() {
        if !slot.is_blank() && slot.count > cap {
            slot.count = cap;
            changed += 1;
        }
    }
    changed
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

    /// A handful of records is enough to see the operations work; the real
    /// box is 2300 of them and the integration tests use that.
    fn boxed(items: &[(u16, u8)]) -> Vec<ItemSlot> {
        items
            .iter()
            .map(|&(id, count)| ItemSlot { id, count })
            .collect()
    }

    #[test]
    fn add_tops_up_the_existing_stack_before_opening_a_new_one() {
        let mut b = boxed(&[(5, 90), (0, 0), (7, 1)]);
        assert_eq!(add(&mut b, 5, 20), 20);
        assert_eq!(b[0], ItemSlot { id: 5, count: 99 });
        assert_eq!(b[1], ItemSlot { id: 5, count: 11 });
        assert_eq!(b[2], ItemSlot { id: 7, count: 1 });
        assert_eq!(count_of(&b, 5), 110);
    }

    #[test]
    fn add_stops_when_the_box_is_full() {
        let mut b = boxed(&[(1, 99), (2, 50)]);
        assert_eq!(add(&mut b, 9, 5), 0, "no blank record left");
        assert_eq!(add(&mut b, 2, 100), 49, "only the room in the stack");
        assert_eq!(count_of(&b, 2), 99);
    }

    #[test]
    fn add_treats_id_zero_as_blank() {
        let mut b = boxed(&[(1, 1)]);
        assert_eq!(add(&mut b, 0, 10), 0);
        assert_eq!(count_of(&b, 0), 0);
        assert_eq!(b[0], ItemSlot { id: 1, count: 1 });
    }

    #[test]
    fn dedupe_keeps_the_first_place_and_caps_at_the_limit() {
        let mut b = boxed(&[(4, 80), (0, 0), (4, 80), (5, 1)]);
        assert_eq!(dedupe(&mut b), 1);
        assert_eq!(b[0], ItemSlot { id: 4, count: 99 });
        assert!(b[1].is_blank());
        assert!(b[2].is_blank());
        assert_eq!(b[3], ItemSlot { id: 5, count: 1 });
    }

    #[test]
    fn fill_all_sets_used_records_and_counts_only_changes() {
        let mut b = boxed(&[(1, 99), (2, 3), (0, 7)]);
        assert_eq!(fill_all(&mut b, 200), 1);
        assert_eq!(b[0].count, 99);
        assert_eq!(b[1].count, 99);
        assert_eq!(b[2].count, 7, "a blank record is not filled");
        assert_eq!(fill_all(&mut b, 99), 0);
    }

    #[test]
    fn compact_keeps_the_order_and_blanks_the_tail() {
        let mut b = boxed(&[(0, 0), (3, 5), (0, 0), (1, 2), (0, 0)]);
        assert_eq!(compact(&mut b), 2);
        assert_eq!(b[0], ItemSlot { id: 3, count: 5 });
        assert_eq!(b[1], ItemSlot { id: 1, count: 2 });
        for slot in &b[2..] {
            assert!(slot.is_blank());
            assert_eq!(slot.count, 0);
        }
    }

    #[test]
    fn clamp_counts_leaves_blanks_and_the_limit_alone() {
        let mut b = boxed(&[(1, 127), (0, 127), (2, 99), (3, 40)]);
        assert_eq!(clamp_counts(&mut b, 99), 1);
        assert_eq!(b[0].count, 99);
        assert_eq!(b[1].count, 127, "a blank record is not touched");
        assert_eq!(b[2].count, 99);
        assert_eq!(b[3].count, 40);
        assert_eq!(clamp_counts(&mut b, 200), 0, "200 is capped to MAX_COUNT");
    }
}
