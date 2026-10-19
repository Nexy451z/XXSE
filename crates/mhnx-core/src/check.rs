//! Quest and save validation.
//!
//! The rules below come from the community knowledge collected around the
//! Quedixx workflow (the MHXX custom-quest thread). Almost every "my quest
//! does not show up / the game freezes / multiplayer is rejected" report maps
//! onto one of these checks, so the editor runs them and says what is wrong
//! instead of letting the user find out on the console.
//!
//! Every finding names the exact byte offset so it can be fixed by hand too.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::arc::{extension_for, Arc};
use crate::quest::{Gmd, Mib, Rem, Sup, REM_SIZE, SUP_SIZE};
use crate::save::Save;

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The quest will not work, or the console will freeze.
    Error,
    /// The quest works but something is missing or restricted.
    Warn,
    /// Worth knowing.
    Info,
}

/// One problem found in a quest or save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How bad it is.
    pub severity: Severity,
    /// Stable short code, handy for tests and for filtering.
    pub code: &'static str,
    /// Human readable explanation.
    pub message: String,
    /// Where the problem lives: (entry name, byte offset).
    pub location: Option<(String, usize)>,
    /// Can the editor repair this automatically?
    pub fixable: bool,
}

impl Finding {
    fn new(severity: Severity, code: &'static str, message: String) -> Self {
        Finding {
            severity,
            code,
            message,
            location: None,
            fixable: false,
        }
    }
    fn at(mut self, entry: &str, offset: usize) -> Self {
        self.location = Some((String::from(entry), offset));
        self
    }
    fn fixable(mut self) -> Self {
        self.fixable = true;
        self
    }
}

/// The value Quedixx force-writes into the first participation condition.
///
/// 0x2F is the index of "1 player only" in the san_ enum. Quedixx rewrites it
/// on every save, which is why a quest edited with it can never be played with
/// other people.
pub const ONE_PLAYER_ONLY: u8 = 0x2F;
/// Offset of the first participation condition inside a .mib.
pub const OFF_CONDITION1: usize = 0x1A;
/// Offset of the second participation condition inside a .mib.
pub const OFF_CONDITION2: usize = 0x1B;

/// Run every quest check.
pub fn check_quest(arc: &Arc) -> Vec<Finding> {
    let mut out = Vec::new();

    // ---- .mib ------------------------------------------------------------
    let mib_entry = arc.by_ext("mib");
    if mib_entry.is_none() {
        out.push(Finding::new(
            Severity::Error,
            "mib-missing",
            String::from("クエストパラメータ (.mib) がありません。この .arc はクエストとして使えません"),
        ));
    }
    if let Some(e) = mib_entry {
        match Mib::from_bytes(&e.data) {
            Err(_) => out.push(Finding::new(
                Severity::Error,
                "mib-size",
                format!(".mib のサイズが {} バイトではありません ({} バイト)", crate::quest::MIB_SIZE, e.data.len()),
            )),
            Ok(m) => {
                let cond1 = m.u8_at(OFF_CONDITION1).unwrap_or(0);
                if cond1 == ONE_PLAYER_ONLY {
                    out.push(
                        Finding::new(
                            Severity::Warn,
                            "one-player-only",
                            String::from(
                                "参加条件1 が「1人限定」(0x2F) です。このままではマルチプレイで受注できません。\
                                 Quedixx は保存のたびにここを 0x2F に書き戻すため、最後にこのツールで直してください",
                            ),
                        )
                        .at(&e.name, OFF_CONDITION1)
                        .fixable(),
                    );
                }
                let id = m.quest_id();
                let file_id = file_quest_id(&e.name);
                if let Some(fid) = file_id {
                    if fid != id {
                        out.push(
                            Finding::new(
                                Severity::Error,
                                "mib-id-mismatch",
                                format!(".mib のクエストID {} がファイル名の {} と一致しません", id, fid),
                            )
                            .at(&e.name, 0x0C),
                        );
                    }
                } else if id == 0 {
                    out.push(Finding::new(
                        Severity::Error,
                        "mib-id-zero",
                        String::from(".mib のクエストIDが 0 です"),
                    ));
                }
            }
        }
    }

    // ---- .gmd ------------------------------------------------------------
    if let Some(e) = arc.by_ext("gmd") {
        if let Ok(g) = Gmd::from_bytes(&e.data) {
            if g.strings.len() < 7 {
                out.push(Finding::new(
                    Severity::Warn,
                    "gmd-strings",
                    format!(".gmd の文章が {} 個しかありません (通常は 7 個)", g.strings.len()),
                ));
            }
        } else {
            out.push(Finding::new(
                Severity::Error,
                "gmd-broken",
                String::from(".gmd を読み取れません"),
            ));
        }
    } else {
        out.push(Finding::new(
            Severity::Warn,
            "gmd-missing",
            String::from("クエスト文章 (.gmd) がありません。クエスト名などが表示されません"),
        ));
    }

    // ---- reward files ----------------------------------------------------
    let rems: Vec<_> = arc
        .entries
        .iter()
        .filter(|e| extension_for(e.type_hash) == Some("rem"))
        .collect();
    let real_rems: Vec<_> = rems
        .iter()
        .filter(|e| {
            let n = e.name.rsplit('\\').next().unwrap_or("");
            n != "rem_000000"
        })
        .collect();

    if real_rems.is_empty() && !rems.is_empty() {
        out.push(Finding::new(
            Severity::Error,
            "reward-dummy-only",
            String::from(
                "報酬ファイルが rem_000000 (ダミー) しかありません。rem_000000 はゲームに読み込まれないため、\
                 報酬がまったく反映されません。他の rem_xxxxxx を編集してください",
            ),
        ));
    }
    for e in &rems {
        if e.data.len() != REM_SIZE {
            out.push(Finding::new(
                Severity::Error,
                "rem-size",
                format!("{} のサイズが {} バイトではありません", e.name, REM_SIZE),
            ));
            continue;
        }
        let name = e.name.rsplit('\\').next().unwrap_or("").to_string();
        if name == "rem_000000" {
            out.push(Finding::new(
                Severity::Info,
                "reward-dummy",
                String::from("rem_000000 はダミーファイルです。ゲームは読み込みません"),
            ));
            continue;
        }
        if let Ok(r) = Rem::from_bytes(&e.data) {
            check_rewards(&r, &e.name, &mut out);
        }
    }

    // ---- supply items ----------------------------------------------------
    if let Some(e) = arc.by_ext("sup") {
        if e.data.len() != SUP_SIZE {
            out.push(Finding::new(
                Severity::Error,
                "sup-size",
                format!("{} のサイズが {} バイトではありません", e.name, SUP_SIZE),
            ));
        } else if let Ok(s) = Sup::from_bytes(&e.data) {
            // NOTE: the community guide quotes "supplies cap at 10", but the
            // official quests routinely use larger stacks (the shipped set goes
            // up to 30), so this is only reported, never treated as an error.
            let used = s.used();
            if used.iter().any(|x| x.count > 99) {
                out.push(Finding::new(
                    Severity::Error,
                    "supply-too-many",
                    String::from("支給品の個数が 99 を超えています。公式クエストでも 30 程度までなので不自然です"),
                ));
            }
            out.push(Finding::new(
                Severity::Info,
                "supply-summary",
                format!(
                    "支給品 {} 種類 / 合計 {} 個",
                    used.len(),
                    s.total_count()
                ),
            ));
        }
    }

    // ---- quest link ------------------------------------------------------
    if arc.by_ext("qdl").is_none() {
        out.push(Finding::new(
            Severity::Error,
            "qdl-missing",
            String::from("questLink (.qdl) がありません。クエストが正しく読み込まれません"),
        ));
    }

    if out.is_empty() {
        out.push(Finding::new(
            Severity::Info,
            "ok",
            String::from("問題は見つかりませんでした"),
        ));
    }
    out
}

fn check_rewards(r: &Rem, entry: &str, out: &mut Vec<Finding>) {
    let slots = r.slots();
    let mut total = 0u32;
    for (i, s) in slots.iter().enumerate() {
        let off = crate::quest::REWARD_DATA_OFFSET + i * 4;
        // A record with probability 255 and no item is the usual terminator.
        if s.id == 0 && s.count == 0 && s.probability == 255 {
            continue;
        }
        if s.id == 0 && (s.count != 0 || s.probability != 0) {
            out.push(
                Finding::new(
                    Severity::Error,
                    "reward-dummy-item",
                    format!(
                        "報酬 {} 番がアイテムID 0 のまま個数/確率だけ設定されています。ダミーアイテムはフリーズの原因になります",
                        i + 1
                    ),
                )
                .at(entry, off),
            );
            continue;
        }
        if s.count > 99 {
            out.push(
                Finding::new(
                    Severity::Error,
                    "reward-too-many",
                    format!("報酬 {} 番の個数が {} です。報酬品は 99 個が上限です", i + 1, s.count),
                )
                .at(entry, off + 2),
            );
        }
        if s.probability > 100 {
            out.push(
                Finding::new(
                    Severity::Error,
                    "reward-probability",
                    format!("報酬 {} 番の確率が {} です。100 以下にしてください", i + 1, s.probability),
                )
                .at(entry, off + 3),
            );
        }
        total += s.count as u32;
    }
    if total > 99 * 4 {
        // Only warn well past anything the official quests do; a single
        // official quest already exceeds the "150 total" figure from the guide.
        out.push(
            Finding::new(
                Severity::Warn,
                "reward-total",
                format!("{} の報酬の合計が {} 個です。公式クエストの範囲を超えています", entry, total),
            )
            .at(entry, 0),
        );
    }
}

/// Pull the numeric quest id out of a name such as
/// "quest\questData\questData_1010165" or "..._1010165_jpn".
fn file_quest_id(name: &str) -> Option<u32> {
    let base = name.rsplit('\\').next().unwrap_or(name);
    let digits: String = base.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 7 {
        return None;
    }
    // Take the first run of exactly 7 digits.
    let bytes = digits.as_bytes();
    for i in 0..=bytes.len() - 7 {
        let s = &digits[i..i + 7];
        if let Ok(v) = s.parse::<u32>() {
            if (1_000_000..2_000_000).contains(&v) {
                return Some(v);
            }
        }
    }
    None
}

/// Set the first participation condition of the .mib.
///
/// Returns the value that was there before.
pub fn clear_one_player_only(arc: &mut Arc, replacement: u8) -> Option<u8> {
    let entry = arc.entries.iter_mut().find(|e| extension_for(e.type_hash) == Some("mib"))?;
    if entry.data.len() <= OFF_CONDITION1 {
        return None;
    }
    let before = entry.data[OFF_CONDITION1];
    entry.data[OFF_CONDITION1] = replacement;
    Some(before)
}

/// Run the save side checks.
pub fn check_save(save: &Save) -> Vec<Finding> {
    let mut out = Vec::new();
    if !(0..3).any(|i| save.slot_used(i)) {
        out.push(Finding::new(
            Severity::Warn,
            "no-slot",
            String::from("使用中のキャラクタースロットがありません"),
        ));
    }
    let installed = crate::dlc::list_quests(save);
    let mut seen = Vec::new();
    for q in &installed {
        if seen.contains(&q.quest_id) {
            out.push(Finding::new(
                Severity::Warn,
                "dlc-duplicate",
                format!("ダウンロードクエスト {} が重複して入っています", q.quest_id),
            ));
        }
        seen.push(q.quest_id);
    }
    if out.is_empty() {
        out.push(Finding::new(Severity::Info, "ok", String::from("問題は見つかりませんでした")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn quest_id_extraction() {
        assert_eq!(file_quest_id("quest\\questData\\questData_1010165"), Some(1_010_165));
        assert_eq!(file_quest_id("questData_1010165_jpn"), Some(1_010_165));
        assert_eq!(file_quest_id("b_m00em000_00"), None);
    }
}
