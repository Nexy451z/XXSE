//! The guild card and the arena records.
//!
//! The card is a 4,986 byte block at slot + 0xC71BD that mirrors part of the
//! character: its appearance bytes at +0x19 are identical to the character's
//! own copy at slot + 0x23B48 in every slot of the sample save, and its hunter
//! rank matches. It also keeps its own play time, which lags the character by
//! a few seconds because it was written earlier.
//!
//! The arena log sits at slot + 0xC83E1, which is card + 0x1224: seventeen
//! arenas of five ranked entries, four bytes each. The first two bytes are the
//! best time in hundredths of a second, so 57,544 is 9:35.44.

/// Offset of the guild card, relative to the slot base.
pub const OFF_GUILD_CARD: usize = 0xC71BD;
/// Size of the guild card block.
pub const GUILD_CARD_SIZE: usize = 4986;
/// Hunter rank as the card records it.
pub const GC_HR: usize = 0x16;
/// The card's copy of the appearance, eight bytes.
pub const GC_APPEARANCE: usize = 0x19;
/// The card's copy of the four colours.
pub const GC_COLOURS: usize = 0x38;
/// Card play time, in seconds.
pub const GC_PLAY_TIME: usize = 0x914;
/// Card ID, eight bytes.
pub const GC_ID: usize = 0x8B0;
/// Profile comment, UTF-16, 28 characters.
pub const GC_COMMENT: usize = 0x878;
/// Characters in the comment.
pub const GC_COMMENT_CHARS: usize = 28;

/// Offset of the arena log, relative to the slot base.
pub const OFF_ARENA: usize = 0xC83E1;
/// Arenas recorded.
pub const ARENA_COUNT: usize = 17;
/// Ranked entries kept per arena.
pub const ARENA_RANKS: usize = 5;
/// Bytes per entry.
pub const ARENA_ENTRY: usize = 4;

/// One arena result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArenaTime {
    /// Arena index, 0..16.
    pub arena: usize,
    /// Rank slot, 0 is the best.
    pub rank: usize,
    /// Best time in hundredths of a second, 0 when not recorded.
    pub centis: u16,
    /// The two trailing bytes, not yet understood.
    pub extra: [u8; 2],
}

impl ArenaTime {
    /// The time as minutes, seconds and hundredths.
    pub fn as_clock(&self) -> (u16, u8, u8) {
        let total = self.centis as u32;
        (
            (total / 6000) as u16,
            ((total / 100) % 60) as u8,
            (total % 100) as u8,
        )
    }

    /// Is this slot empty?
    pub fn is_empty(&self) -> bool {
        self.centis == 0
    }
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

/// Read the arena log of one character.
pub fn arena(save: &Save, slot: usize) -> Vec<ArenaTime> {
    let mut out = Vec::new();
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return out;
    };
    let at = base + OFF_ARENA;
    for arena in 0..ARENA_COUNT {
        for rank in 0..ARENA_RANKS {
            let o = at + (arena * ARENA_RANKS + rank) * ARENA_ENTRY;
            let Some(e) = save.body().get(o..o + ARENA_ENTRY) else {
                return out;
            };
            out.push(ArenaTime {
                arena,
                rank,
                centis: u16_at(e, 0),
                extra: [e[2], e[3]],
            });
        }
    }
    out
}

/// The hunter rank stored on the card.
pub fn hunter_rank(save: &Save, slot: usize) -> Option<u16> {
    let base = save.slot_base(slot)? as usize;
    let b = save.body().get(base + OFF_GUILD_CARD + GC_HR..base + OFF_GUILD_CARD + GC_HR + 2)?;
    Some(u16_at(b, 0))
}

/// The card's own play time in seconds.
pub fn play_time(save: &Save, slot: usize) -> Option<u32> {
    let base = save.slot_base(slot)? as usize;
    let o = base + OFF_GUILD_CARD + GC_PLAY_TIME;
    let b = save.body().get(o..o + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The card ID, eight raw bytes.
pub fn card_id(save: &Save, slot: usize) -> Option<[u8; 8]> {
    let base = save.slot_base(slot)? as usize;
    let o = base + OFF_GUILD_CARD + GC_ID;
    let b = save.body().get(o..o + 8)?;
    let mut out = [0u8; 8];
    out.copy_from_slice(b);
    Some(out)
}

/// The profile comment, decoded from UTF-16.
pub fn comment(save: &Save, slot: usize) -> Option<String> {
    let base = save.slot_base(slot)? as usize;
    let o = base + OFF_GUILD_CARD + GC_COMMENT;
    let b = save.body().get(o..o + GC_COMMENT_CHARS * 2)?;
    let mut s = String::new();
    for i in 0..GC_COMMENT_CHARS {
        let c = u16_at(b, i * 2);
        if c == 0 {
            break;
        }
        s.push(char::from_u32(c as u32).unwrap_or('?'));
    }
    Some(s)
}

use alloc::string::String;
use alloc::vec::Vec;

use crate::save::Save;
/// Rewrite the card's hunter rank.
pub fn set_hunter_rank(save: &mut Save, slot: usize, rank: u16) -> bool {
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_HR;
    let Some(b) = save.body_mut().get_mut(o..o + 2) else {
        return false;
    };
    b.copy_from_slice(&rank.to_le_bytes());
    true
}

/// Rewrite the card's own play time, in seconds.
pub fn set_play_time(save: &mut Save, slot: usize, seconds: u32) -> bool {
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_PLAY_TIME;
    let Some(b) = save.body_mut().get_mut(o..o + 4) else {
        return false;
    };
    b.copy_from_slice(&seconds.to_le_bytes());
    true
}

/// Rewrite the profile comment. Longer text is truncated to the field.
pub fn set_comment(save: &mut Save, slot: usize, text: &str) -> bool {
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_COMMENT;
    let Some(dst) = save.body_mut().get_mut(o..o + GC_COMMENT_CHARS * 2) else {
        return false;
    };
    dst.fill(0);
    for (i, ch) in text.chars().take(GC_COMMENT_CHARS).enumerate() {
        let v = ch as u32;
        // Anything outside the basic plane cannot fit in a single unit.
        let unit = if v <= 0xFFFF { v as u16 } else { 0x30FB };
        dst[i * 2..i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    true
}

/// Rewrite one arena entry's time, in hundredths of a second.
pub fn set_arena_time(
    save: &mut Save,
    slot: usize,
    arena: usize,
    rank: usize,
    centis: u16,
) -> bool {
    if arena >= ARENA_COUNT || rank >= ARENA_RANKS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_ARENA + (arena * ARENA_RANKS + rank) * ARENA_ENTRY;
    let Some(dst) = save.body_mut().get_mut(o..o + 2) else {
        return false;
    };
    dst.copy_from_slice(&centis.to_le_bytes());
    true
}