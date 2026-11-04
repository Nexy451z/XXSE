//! The text catalogue: the table itself, and the fallback that makes a partial
//! translation safe.
//!
//! These tests are the contract the rest of the interface leans on. A drawing
//! call passes a key and a language and gets a string back; what is checked
//! here is that the table behind that call cannot be half built (a key twice, a
//! key with no Japanese, a key that is only in the list), and that a missing
//! English line reads as Japanese rather than as a blank or a raw key.

use mhnx_core::strings::{catalog, has_key, text, textf, Catalog, Lang, KEYS, LANGS, MISSING};

/// A key whose English line is deliberately empty, so the fallback has
/// something real to fall back from. It is named in the catalogue's comment.
const UNTRANSLATED: &str = "items.csv_note";

/// Every key is listed once, and the catalogue holds exactly those keys in both
/// languages.
///
/// The list and the tables come from one macro, so this pins the macro itself:
/// a duplicate or a dropped column would show up here rather than as one
/// language quietly missing a line.
#[test]
fn every_key_appears_once_and_both_languages_hold_it() {
    let mut seen = std::collections::BTreeSet::new();
    for key in KEYS {
        assert!(seen.insert(*key), "the key {} is listed twice", key);
        assert!(!key.is_empty());
    }
    assert_eq!(seen.len(), KEYS.len());

    for lang in LANGS {
        let cat = catalog(*lang);
        assert_eq!(cat.len(), KEYS.len(), "{:?} has a different number of entries", lang);
        assert_eq!(cat.lang(), *lang);
        for key in KEYS {
            assert!(cat.get(key).is_some(), "{:?} has no entry for {}", lang, key);
        }
    }
    // The list is generated from the table, so a key in it is a key the
    // catalogue can answer.
    for key in KEYS {
        assert!(has_key(key), "{} is listed but not in the catalogue", key);
    }
    assert!(!has_key("no.such.key"));
    assert!(!Catalog::is_empty(&catalog(Lang::Ja)));
}

/// Japanese is the fallback target, so it cannot be the language with a hole in
/// it: every key must say something in Japanese.
#[test]
fn the_japanese_catalogue_is_complete() {
    let ja = catalog(Lang::Ja);
    assert!(
        ja.untranslated().is_empty(),
        "these keys have no Japanese text: {:?}",
        ja.untranslated()
    );
    for key in KEYS {
        assert!(!text(Lang::Ja, key).is_empty(), "{} is empty", key);
        assert_ne!(text(Lang::Ja, key), MISSING, "{} is missing from Japanese", key);
    }
}

/// A key with no English line falls back to the Japanese text; a key that has
/// one answers in English. This is the whole point of the catalogue: a
/// translation can be added one line at a time and nothing breaks while it is
/// half done.
#[test]
fn an_untranslated_key_falls_back_to_japanese() {
    let en = catalog(Lang::En);
    assert!(
        !en.is_translated(UNTRANSLATED),
        "{} is meant to be untranslated; the fallback test needs it",
        UNTRANSLATED
    );
    assert!(en.untranslated().contains(&UNTRANSLATED));
    assert_eq!(en.get(UNTRANSLATED), Some(""));
    // The empty English line is the Japanese one, character for character.
    assert_eq!(text(Lang::En, UNTRANSLATED), text(Lang::Ja, UNTRANSLATED));
    assert!(text(Lang::En, UNTRANSLATED).contains("CSV"));

    // A translated key really is English, and it is not the Japanese text.
    assert!(en.is_translated("items.fill_all"));
    assert_eq!(text(Lang::En, "items.fill_all"), "All to 99");
    assert_eq!(text(Lang::Ja, "items.fill_all"), "全部99に");
    assert_ne!(text(Lang::En, "items.fill_all"), text(Lang::Ja, "items.fill_all"));

    // Japanese is the default, and asking for Japanese twice is stable.
    assert_eq!(Lang::default(), Lang::Ja);
    assert_eq!(Lang::DEFAULT, Lang::Ja);
    assert_eq!(text(Lang::Ja, "items.filter"), "絞り込み");
}

/// A key the catalogue does not hold is shown as the marker, in every language,
/// rather than as an empty line or the key itself: a wrong key is a bug, and a
/// bug that draws nothing is the one nobody notices.
#[test]
fn a_key_that_is_not_in_the_catalogue_says_so() {
    for lang in LANGS {
        assert_eq!(text(*lang, "items.no_such_key"), MISSING);
        assert!(!catalog(*lang).is_translated("items.no_such_key"));
        assert_eq!(catalog(*lang).get("items.no_such_key"), None);
    }
    // The marker is not a word any language of the catalogue returns for a real
    // key, so seeing it always means a call site is wrong.
    for key in KEYS {
        for lang in LANGS {
            assert_ne!(text(*lang, key), MISSING, "{:?} {} is empty", lang, key);
        }
    }
}

/// Placeholders: the indexed spelling, the bare one that takes the next
/// argument, and the case where an argument is missing.
#[test]
fn textf_fills_both_spellings_and_keeps_a_placeholder_it_cannot_fill() {
    // {0} from the catalogue.
    assert_eq!(textf(Lang::Ja, "items.count", &[&3]), "3 件");
    assert_eq!(textf(Lang::En, "items.count", &[&3]), "3 items");
    // Two indexed arguments, in the order the item and the slot are named.
    assert_eq!(
        textf(Lang::Ja, "items.added", &[&"回復薬", &7]),
        "回復薬 を箱 7 に追加しました"
    );
    assert_eq!(
        textf(Lang::En, "items.added", &[&"Potion", &7]),
        "Added Potion to box 7"
    );
    // Two indexed arguments in the other order, as a translation may write
    // them: the template decides, not the call.
    assert_eq!(
        textf(Lang::En, "items.detail_changed", &[&2, &"Potion"]),
        "Box #2 changed to Potion"
    );
    assert_eq!(
        textf(Lang::Ja, "items.detail_changed", &[&2, &"Potion"]),
        "箱 #2 を Potion に変更しました"
    );
    // A bare placeholder takes the next argument, in order.
    assert_eq!(
        textf(Lang::Ja, "items.detail_selected", &[&12]),
        "選択中: 箱 #12"
    );
    assert_eq!(
        textf(Lang::En, "items.detail_selected", &[&12]),
        "Selected: box #12"
    );
    // No argument for the placeholder: it is left standing, not dropped and not
    // a panic.
    assert_eq!(textf(Lang::Ja, "items.count", &[]), "{0} 件");
    assert_eq!(textf(Lang::En, "items.added", &[&"Potion"]), "Added Potion to box {1}");
    // A key with no placeholders ignores the arguments it was given.
    assert_eq!(textf(Lang::Ja, "items.filter", &[&1]), "絞り込み");
    // And an unknown key is the marker, unchanged.
    assert_eq!(textf(Lang::Ja, "items.no_such_key", &[&1]), MISSING);
}

/// textf walks the text character by character, so a Japanese sentence with no
/// placeholders comes back byte for byte: the walk cannot split a multi-byte
/// character, and there is nothing to fill in.
#[test]
fn textf_returns_japanese_text_unchanged() {
    for key in KEYS {
        let ja = text(Lang::Ja, key);
        assert_eq!(textf(Lang::Ja, key, &[]), ja, "{} was changed by textf", key);
    }
    // The space after the label is part of the catalogue, not the call site.
    assert_eq!(textf(Lang::Ja, "items.detail_id", &[]), "ID ");
    assert_eq!(textf(Lang::En, "items.detail_id", &[]), "ID ");
}

/// The language codes round trip and the dump tag is the one the game uses, so
/// a settings line and a table file name mean the same language.
#[test]
fn the_language_codes_and_the_table_tags_agree() {
    for lang in LANGS {
        assert_eq!(Lang::from_code(lang.code()), Some(*lang));
        assert_eq!(Lang::from_code(lang.romfs_tag()), Some(*lang));
        assert!(!lang.label().is_empty());
        // The tag the dump writes is the three letter spelling, not the two.
        assert_eq!(lang.romfs_tag().len(), 3);
        assert_eq!(
            Lang::from_code(lang.code().to_uppercase().as_str()),
            Some(*lang)
        );
    }
    assert_eq!(Lang::from_code(" jpn "), Some(Lang::Ja));
    assert_eq!(Lang::from_code("ENG"), Some(Lang::En));
    assert_eq!(Lang::from_code("jp"), Some(Lang::Ja));
    // Something this build does not speak is None, not a panic and not a wrong
    // language: the caller decides to use the default.
    assert_eq!(Lang::from_code("fr"), None);
    assert_eq!(Lang::from_code(""), None);
    // The two languages do not share a tag.
    assert_ne!(Lang::Ja.romfs_tag(), Lang::En.romfs_tag());
    assert_eq!(LANGS.len(), 2);
}

/// The catalogue as a whole: at least one table's worth of keys, no empty
/// Japanese line, and the only untranslated English line is the one that was
/// left empty on purpose.
#[test]
fn the_english_column_only_misses_what_it_says_it_misses() {
    assert!(KEYS.len() >= 40, "only {} keys", KEYS.len());
    let en = catalog(Lang::En);
    assert_eq!(
        en.untranslated(),
        [UNTRANSLATED],
        "the English column gained or lost an untranslated line"
    );
    // untranslated() is table order, so it is the keys in KEYS order.
    let expected: Vec<&str> = KEYS
        .iter()
        .copied()
        .filter(|key| !en.is_translated(key))
        .collect();
    assert_eq!(en.untranslated(), expected);
    assert!(en.untranslated().len() < KEYS.len(), "nothing is translated");
    assert_eq!(catalog(Lang::En).len(), catalog(Lang::Ja).len());
}
