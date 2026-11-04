//! The words the interface says, in one place, so another language can be
//! added without touching the code that draws them.
//!
//! Nothing here is game data and nothing here is translated from the game: it
//! is the editor's own wording (buttons, column headings, hover text). The
//! catalogue ships with the Japanese text and, where one has been written, the
//! English text. A key with no translation is not an error: [text] falls back
//! to Japanese, so a half translated interface still reads as an interface.
//!
//! ## Why the table is a flat static list
//!
//! The crate is no_std + alloc, so there is no lazy-loaded file to read at run
//! time and no catalogue parser to carry: the words are compiled in. A
//! translation added later is a line in the table KEYS is generated from, and
//! the strings test checks the table itself (every key once, Japanese complete,
//! English either written or empty on purpose).
//!
//! ## Adding a language
//!
//! 1. a variant in [Lang] (Ja, En today),
//! 2. one column per key in the catalogue! table,
//! 3. the arms in [Lang::code] / [Lang::label] / [Lang::romfs_tag],
//! 4. one arm in [catalog].
//!
//! Nothing else in this crate has to change, and no call site changes: the call
//! site passes the language it was given and the fallback keeps it truthful.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{Display, Write as _};

/// The languages the interface can speak.
///
/// The order is the order the 設定 window lists them in, and [Lang::Ja] is the
/// fallback for every key in every other language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {
    /// Japanese: the original wording, and the text every key is checked
    /// against.
    Ja,
    /// English.
    En,
}

/// Every language, in the order the chooser lists them.
pub const LANGS: &[Lang] = &[Lang::Ja, Lang::En];

impl Lang {
    /// The language a missing or unreadable setting falls back to.
    pub const DEFAULT: Lang = Lang::Ja;

    /// The two letter code the settings file stores.
    pub const fn code(self) -> &'static str {
        match self {
            Lang::Ja => "ja",
            Lang::En => "en",
        }
    }

    /// The three letter tag a table file of the game carries (_jpn, _eng).
    ///
    /// The same language as [Lang::code] but in the spelling the dump uses, so
    /// a table can be chosen by the language that is on screen.
    pub const fn romfs_tag(self) -> &'static str {
        match self {
            Lang::Ja => "jpn",
            Lang::En => "eng",
        }
    }

    /// The language's own name, for the chooser: a language is never listed in
    /// a language the user has to already read.
    pub const fn label(self) -> &'static str {
        match self {
            Lang::Ja => "日本語",
            Lang::En => "English",
        }
    }

    /// Read a stored code back.
    ///
    /// Accepts the two and three letter spellings and ignores case, because the
    /// value comes from a settings file a person can edit. None means "not a
    /// language this build speaks", which the caller turns into [Lang::DEFAULT]
    /// rather than an error: a settings file from a newer build must not stop
    /// the editor from opening.
    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim();
        for lang in LANGS {
            if code.eq_ignore_ascii_case(lang.code()) || code.eq_ignore_ascii_case(lang.romfs_tag()) {
                return Some(*lang);
            }
        }
        if code.eq_ignore_ascii_case("jp") {
            return Some(Lang::Ja);
        }
        None
    }
}

impl Default for Lang {
    fn default() -> Self {
        Lang::DEFAULT
    }
}

/// What [text] returns for a key the catalogue does not hold.
///
/// A wrong key is a bug in the call site, not missing translation, so it is
/// shown rather than hidden: the same shape as a placeholder that had no
/// argument. It is deliberately short and not a sentence, so it cannot be
/// mistaken for the text that was meant.
pub const MISSING: &str = "!key";

/// One key and its text in one language.
struct Entry {
    key: &'static str,
    text: &'static str,
}

/// A catalogue in one language, with the fallback applied.
///
/// Constructed by [catalog]; it borrows the compiled table, so it is Copy and
/// costs nothing to hand around.
#[derive(Clone, Copy)]
pub struct Catalog {
    lang: Lang,
    entries: &'static [Entry],
}

impl Catalog {
    /// Which language this catalogue speaks.
    pub const fn lang(&self) -> Lang {
        self.lang
    }

    /// How many keys the catalogue holds.
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the catalogue holds no key at all.
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The raw text of a key in this language, empty when it is not translated.
    ///
    /// None is a key that does not exist; an empty string is a key that exists
    /// but has no text here yet. [Catalog::text] is what the interface calls.
    pub fn get(&self, key: &str) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.text)
    }

    /// The text to draw for this key: this language, else Japanese, else
    /// [MISSING].
    ///
    /// Falling back to Japanese inside one language is what makes a partial
    /// translation usable: the lines that were written are English and the rest
    /// are the wording the tool has always had, in one consistent screen rather
    /// than blank rows or raw keys.
    pub fn text(&self, key: &str) -> &'static str {
        if let Some(text) = self.get(key) {
            if !text.is_empty() {
                return text;
            }
        }
        if self.lang != Lang::Ja {
            if let Some(text) = catalog(Lang::Ja).get(key) {
                if !text.is_empty() {
                    return text;
                }
            }
        }
        MISSING
    }

    /// Every key this language has no text for, in table order.
    ///
    /// The list is what makes "not translated yet" a visible state instead of a
    /// silent one; a test reads it so the English column cannot quietly lose a
    /// line, and the 設定 window could print the count.
    pub fn untranslated(&self) -> Vec<&'static str> {
        self.entries
            .iter()
            .filter(|entry| entry.text.is_empty())
            .map(|entry| entry.key)
            .collect()
    }

    /// Whether this language has its own text for a key.
    pub fn is_translated(&self, key: &str) -> bool {
        self.get(key).map(|text| !text.is_empty()).unwrap_or(false)
    }
}

/// The catalogue for a language.
///
/// Total: every language has a table, so a caller never has to handle "this
/// language has no words".
pub const fn catalog(lang: Lang) -> Catalog {
    let entries = match lang {
        Lang::Ja => JA,
        Lang::En => EN,
    };
    Catalog { lang, entries }
}

/// The text to draw for a key in a language, with the Japanese fallback
/// applied.
///
/// This is the one function a drawing call needs; see [Catalog::text] for what
/// happens to an untranslated or unknown key. The lookup is a linear scan of a
/// table this size on purpose: a key inserted anywhere then cannot break the
/// lookup the way a sorted table would.
pub fn text(lang: Lang, key: &str) -> &'static str {
    catalog(lang).text(key)
}

/// Whether the catalogue holds a key.
pub fn has_key(key: &str) -> bool {
    KEYS.contains(&key)
}

/// The text of a key with its placeholders filled in.
///
/// Two spellings are accepted, the same two the reference editor's catalogue
/// uses: a bare pair of braces takes the next argument in order, and a braced
/// number takes the argument at that index, so a translation may put the
/// arguments in another order. A placeholder with no argument is left standing
/// (never dropped, never a panic): a missing argument has to be visible,
/// because a silently missing number is worse than a literal placeholder.
pub fn textf(lang: Lang, key: &str, args: &[&dyn Display]) -> String {
    let template = text(lang, key);
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    // The next argument a bare placeholder will take.
    let mut next = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            // Copy one whole character: slicing at a byte that is not a char
            // boundary would panic on the first Japanese letter.
            let ch = template[i..].chars().next().unwrap_or('{');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let Some(rel) = template[i + 1..].find('}') else {
            // An opening brace with no closing one is text, not a placeholder.
            out.push('{');
            i += 1;
            continue;
        };
        let end = i + 1 + rel;
        let inside = &template[i + 1..end];
        let index = if inside.is_empty() {
            let taken = next;
            next += 1;
            Some(taken)
        } else if inside.bytes().all(|b| b.is_ascii_digit()) {
            inside.parse::<usize>().ok()
        } else {
            // A named placeholder is not a spelling this catalogue uses.
            None
        };
        match index.and_then(|n| args.get(n)) {
            Some(arg) => {
                // Writing into a String cannot fail.
                let _ = write!(out, "{}", arg);
            }
            None => out.push_str(&template[i..=end]),
        }
        i = end + 1;
    }
    out
}

/// One entry per key: the key, the Japanese text, the English text.
///
/// English may be empty, and an empty line is what "not translated yet" looks
/// like: [text] then shows the Japanese. This is the only table in the crate,
/// so [KEYS] and both languages cannot drift apart.
macro_rules! catalogue {
    ( $( $key:literal, $ja:literal, $en:literal ; )* ) => {
        const JA: &[Entry] = &[ $( Entry { key: $key, text: $ja } ),* ];
        const EN: &[Entry] = &[ $( Entry { key: $key, text: $en } ),* ];

        /// Every key of the catalogue, in table order.
        ///
        /// The list is generated from the same table the text comes from, so it
        /// cannot name a key that has no entry, and a test can walk it to prove
        /// the picture is complete.
        pub const KEYS: &[&str] = &[ $( $key ),* ];
    };
}

// The item tab, keyed first as the shape the rest of the interface will
// follow. The remaining tabs still hold their text inline; moving them is the
// next step, and doing it one tab at a time is what keeps the change
// reviewable.
catalogue! {
    "tab.items", "アイテム", "Items";

    "items.empty", "セーブを開くとアイテムボックスを表示します", "Open a save to show the item box";
    "items.filter", "絞り込み", "Filter";
    "items.show_empty", "空きスロットも表示", "Show empty slots";
    "items.undo", "元に戻す", "Undo";
    "items.undo_hint", "直前のアイテム変更を取り消します", "Undo the last item change";
    "items.bulk", "まとめて追加", "Bulk add";
    "items.bulk_hint", "アイテム名と個数を決めて、空き枠へまとめて入れます", "Pick an item and a count, then fill the empty slots with it";
    "items.fill_all", "全部99に", "All to 99";
    "items.fill_all_hint", "使っている枠の個数をすべて 99 にします（確認あり）", "Set every used slot to 99 (asks first)";
    "items.dedupe", "重複を整理", "Merge duplicates";
    "items.dedupe_hint", "同じアイテムの 2 枠目以降を 1 枠にまとめます。99 を超える分は失います（確認あり）", "Merge a second stack of the same item into the first; anything past 99 is lost (asks first)";
    "items.compact", "空きを詰める", "Compact";
    "items.compact_hint", "空き枠を消して前へ詰めます。並び順が変わります（確認あり）", "Drop the empty slots and move the rest up; the order changes (asks first)";
    "items.clamp", "上限超えを丸める", "Cap at 99";
    "items.clamp_hint", "99 を超えた個数を 99 に戻します（確認あり）", "Bring any count above 99 back to 99 (asks first)";
    "items.add", "アイテムを追加", "Add item";
    "items.add_hint", "空きスロットに回復薬を入れ、選択状態にします", "Put a potion in the first empty slot and select it";
    "items.csv", "CSV", "CSV";
    "items.csv_hint", "アイテムボックスを表として持ち出し、その表から書き戻せます（ID と個数だけの 3 列）", "Take the box out as a table and write it back; three columns, id and count only";
    "items.csv_export", "CSV 書き出し", "Export CSV";
    "items.csv_export_hint", "このキャラのアイテムボックス 2300 枠を CSV にします（空き枠も 1 行ずつ）", "Write this character's 2300 box slots to a CSV, one row each, empty ones included";
    "items.csv_import", "CSV 読み込み", "Import CSV";
    "items.csv_import_hint", "CSV の内容をこのキャラのボックスに書き戻します（内容を確認してから適用・確認あり）", "Write the CSV back into this character's box, after showing what it changes (asks first)";
    "items.csv_export_title", "アイテムボックスの CSV 書き出し", "Export the item box as CSV";
    "items.csv_import_title", "アイテムボックスの CSV 読み込み", "Import the item box from CSV";
    // Left untranslated on purpose: it is the visible proof that a key with no
    // English line falls back to Japanese, and the GUI test reads this very
    // sentence out of an English frame.
    "items.csv_note", "UTF-8 の CSV を読みます。適用の前に、何行が入るかを確認できます", "";
    "items.count", "{0} 件", "{0} items";
    "items.col_box", "箱 #", "Box #";
    "items.col_id", "ID", "ID";
    "items.col_name", "名前", "Name";
    "items.col_count", "個数", "Qty";
    "items.bulk_search_hint", "アイテム名で検索", "Search by item name";
    "items.bulk_none", "未選択", "Not chosen";
    "items.bulk_add", "追加する", "Add";
    "items.bulk_add_hint", "入りきらなかった数はメッセージに出ます", "Whatever does not fit is reported in the message line";
    "items.bulk_close", "閉じる", "Close";
    "items.bulk_guide", "名前で絞り込むか、ID を直接入力してください", "Narrow by name, or type an id";
    "items.no_match", "一致するアイテムがありません", "No item matches";
    "items.detail_guide", "行をクリックすると、ここで種類と個数を変更できます", "Click a row to change its kind and count here";
    "items.detail_selected", "選択中: 箱 #{0}", "Selected: box #{0}";
    "items.detail_id", "ID ", "ID ";
    "items.detail_count", "個数 ", "Qty ";
    "items.detail_clear", "空にする", "Empty";
    "items.detail_pick", "種類を変更", "Change kind";
    "items.detail_clear_filter", "クリア", "Clear";
    "items.detail_keep_filter", "選んだあとも絞り込みを残す", "Keep the filter after picking";
    "items.detail_keep_filter_hint", "同じ名前のアイテムを続けて複数の枠へ入れやすくなります", "Makes it easy to fill several slots with items of the same name";
    "items.detail_changed", "箱 #{0} を {1} に変更しました", "Box #{0} changed to {1}";
    "items.undo_done", "アイテムボックスを元に戻しました", "The item box was put back";
    "items.undo_none", "戻せる変更がありません", "Nothing to undo";
    "items.no_open", "先にセーブを開いてください", "Open a save first";
    "items.box_full", "アイテムボックスに空きがありません", "The item box has no empty slot";
    "items.added", "{0} を箱 {1} に追加しました", "Added {0} to box {1}";
    "items.fill_all_none", "すべての枠が 99 個です（変わりません）", "Every slot already holds 99 (nothing changes)";
    "items.dedupe_none", "重複した枠はありません（変わりません）", "No duplicate stack (nothing changes)";
    "items.compact_none", "空き枠は先頭に詰まっています（変わりません）", "The used slots are already at the front (nothing changes)";
    "items.clamp_none", "99 を超えた個数はありません（変わりません）", "No count is above 99 (nothing changes)";
    "items.count_required", "アイテムと個数を指定してください", "Choose an item and a count";
    "items.added_count", "{0} を {1} 個追加しました", "Added {1} of {0}";
    "items.added_none", "{0} は入りませんでした（{1} 個は入りきりませんでした）", "{0} did not fit ({1} left over)";
    "items.added_part", "{0} を {1} 個追加しました（{2} 個は入りきりませんでした）", "Added {1} of {0} ({2} left over)";
    "items.fill_all_done", "{0} 枠を 99 個にしました（元に戻せます）", "Set {0} slots to 99 (undo is available)";
    "items.dedupe_done", "{0} 枠を解放しました（失ったアイテムはありません）／元に戻せます", "Freed {0} slots; no item was lost (undo is available)";
    "items.dedupe_done_lost", "{0} 枠を解放し、{1} 個を失いました／元に戻せます", "Freed {0} slots and lost {1} items (undo is available)";
    "items.compact_done", "{0} 枠を前に詰めました（元に戻せます）", "Moved {0} slots to the front (undo is available)";
    "items.clamp_done", "{0} 枠の個数を 99 に丸めました（元に戻せます）", "Capped {0} slots at 99 (undo is available)";
    "items.undo_label", "アイテムボックス", "Item box";

    // The confirmation dialogs of the same tab. They are separate keys from the
    // buttons that open them: a button is a label and a dialog is a sentence
    // with the numbers in it, and a translation may word the two apart. The
    // audit found these five dialogs still spoken in Japanese inside an English
    // frame (AUDIT8 §4 N1), which is why they live here now.
    "confirm.fill_all_title", "全部 99 個にします", "Set every used slot to 99";
    "confirm.fill_all_body", "使っている {0} 枠の個数を、すべて 99 個にします。\n今の個数は失われます（この操作は元に戻せます）。", "Set all {0} used slots to 99.\nThe counts they hold now are lost (this can be undone).";
    "confirm.fill_all_yes", "99 個にする", "Set to 99";

    "confirm.dedupe_title", "重複を整理します", "Merge duplicates";
    "confirm.dedupe_body", "同じアイテムの 2 枠目以降を 1 枠にまとめます。", "Merge a second stack of the same item into the first.";
    "confirm.dedupe_line", "{0} 枠を解放、{1} 個を失います", "Frees {0} slots and loses {1} items";
    "confirm.dedupe_lost", "1 枠に 99 個までしか入らないため、あふれた分は失われます。", "A slot holds at most 99, so anything past that is lost.";
    "confirm.dedupe_undo", "この操作は元に戻せます。", "This can be undone.";
    "confirm.dedupe_yes", "整理する", "Merge";

    "confirm.compact_title", "空きを詰めます", "Compact the box";
    "confirm.compact_body", "空き枠を消して、使っている {0} 枠を前へ詰めます。\n順序が変わります（アイテムと個数はそのままです）。\nこの操作は元に戻せます。", "Drop the empty slots and move the {0} used ones to the front.\nThe order changes (the items and counts do not).\nThis can be undone.";
    "confirm.compact_yes", "詰める", "Compact";

    "confirm.clamp_title", "個数を 99 に丸めます", "Cap the counts at 99";
    "confirm.clamp_body", "99 を超えている {0} 枠の個数を 99 に戻します。\n超えた分は失われます（この操作は元に戻せます）。", "Bring the {0} slots above 99 back to 99.\nWhat is above 99 is lost (this can be undone).";
    "confirm.clamp_yes", "丸める", "Cap";

    "confirm.csv_title", "CSV を読み込みます", "Import the CSV";
    "confirm.csv_yes", "適用する", "Apply";
    "confirm.csv_none", "読み込んだ CSV がありません", "No CSV has been read yet";
    "confirm.csv_body", "{0} を {1} に書き込みます。\n{2}（{3} 行）\n\n", "{0} is written into {1}.\n{2} ({3} rows)\n\n";
    "confirm.csv_report", "書き込み {0} 行 / 対象外 {1} 行 / 丸め {2} 行", "{0} rows applied / {1} skipped / {2} clamped";
    "confirm.csv_more", "・ほか {0} 件", "・{0} more";
    "confirm.csv_note", "「適用する」でセーブデータに反映します（ファイルへは「上書き保存」で書き込みます）。", "Apply changes the save in the editor; Save writes it to the file.";
    "confirm.csv_kind_items", "アイテムボックスの CSV", "the item box CSV";
    "confirm.csv_kind_equip", "装備ボックスの CSV", "the equipment box CSV";
    "confirm.csv_target_items", "枠 {0} のアイテムボックス", "the item box of slot {0}";
    "confirm.csv_target_equip", "枠 {0} の装備ボックス", "the equipment box of slot {0}";
    "confirm.csv_written", "{0} を書き出しました: {1}（{2} 行）", "Wrote {0}: {1} ({2} rows)";
    "confirm.csv_write_failed", "CSV を書き出せません: {0}（{1}）", "The CSV could not be written: {0} ({1})";
    "confirm.csv_applied", "{0} を適用しました: {1}", "Applied {0}: {1}";
    "confirm.csv_failed", "CSV を適用できませんでした: {0}", "The CSV could not be applied: {0}";
    "confirm.csv_undo", "CSV（{0}）の適用", "Applied CSV ({0})";

    // What the character tab says about the two colours the game rewrites on
    // save (AUDIT8 §4 N2), and the line the MHGU save keeps above the tabs
    // (AUDIT8 §4 N8).
    "save.color_overwritten", "ゲームが保存時に上書きします（編集しても反映されません）", "The game overwrites this when it saves (an edit here will not show)";
    "save.mhgu_banner", "このセーブは MHGU です。MHXX の値として表示しています。保存はできません。", "This save is MHGU. It is shown as MHXX values; it cannot be saved.";

    "settings.language", "言語", "Language";
    "settings.language_hint", "いまはアイテムタブ・確認ダイアログ・キャラタブの注記が切り替わります。訳の無い文は日本語のまま出ます。", "The item tab, its confirmation dialogs and the character tab's notes follow this choice so far; a line with no translation yet stays in Japanese.";
    "settings.language_saved", "言語を {0} にしました（次回も同じ言語で開きます）", "Language set to {0}; the next run opens with it";
    "settings.language_memory", "言語を {0} にしました（設定を保存できないため、この起動のあいだだけです）", "Language set to {0}; it could not be saved, so this run only";
    "settings.language_failed", "言語を {0} にしました（保存できません: {1}）", "Language set to {0}, but it was not saved: {1}";
}
