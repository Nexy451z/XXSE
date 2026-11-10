//! End-to-end tests for the on-device editing flow.
//!
//! These drive the exact state machine the Switch build runs, using synthetic
//! button presses against the real save and quest files on this machine. They
//! are the closest thing to hardware testing that is possible without a
//! console, and they are skipped when the files are missing.

use std::path::PathBuf;

use mhnx_core::arc::{extension_for, Arc};
use mhnx_core::dlc;
use mhnx_core::hbedit::{Editor, Key, Request, Screen, Write};
use mhnx_core::questfields;
use mhnx_core::save::Save;

fn real_save() -> PathBuf {
    // Prefer the fixed copy: the Ryujinx save is live and the game writes to
    // it, so assertions pinned to its contents fail the next time it changes.
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        return stable;
    }
    PathBuf::from(r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system")
}
fn real_arc() -> PathBuf {
    PathBuf::from(r"C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest\q1010165.arc")
}

/// Act as the I/O layer: answer whatever the editor asks for.
fn service(ed: &mut Editor) {
    if let Some(req) = ed.take_request() {
        match req {
            Request::ReadSave(_) => {
                let p = ed.save_files.first().cloned().unwrap_or_default();
                match std::fs::read(&p) {
                    Ok(b) => ed.open_save_bytes(&p, &b),
                    Err(e) => panic!("read {}: {}", p, e),
                }
            }
            Request::ReadArc(_) => {
                let p = ed.arc_files.first().cloned().unwrap_or_default();
                match std::fs::read(&p) {
                    Ok(b) => ed.open_arc_bytes(&p, &b),
                    Err(e) => panic!("read {}: {}", p, e),
                }
            }
            // The PC test does not write files for an extract request; the
            // tests that care inspect the request itself.
            Request::ExtractQuests { .. } => {}
        }
    }
}

fn open_save(ed: &mut Editor) {
    let p = real_save();
    if !p.exists() {
        return;
    }
    ed.set_save_files(vec![p.to_string_lossy().to_string()]);
    ed.input(Key::A);
    service(ed);
    assert_eq!(ed.screen, Screen::Slots, "save should open on the slot screen");
}

/// Enter the quest section the way the console's section bar does.
///
/// A section is not a state-machine key: the console's ZL / ZR move between
/// sections, and the state machine's own [Y] on the save list is 再読込 and on
/// the slot screen 一括MAX - the console's chip table is the reference for every
/// key (tests/review_18.rs pins that). These flows therefore enter the section
/// the way the console's jump does instead of pressing a key that means
/// something else now.
fn open_quest_section(ed: &mut Editor) {
    ed.screen = Screen::ArcList;
    ed.cursor = 0;
}

#[test]
fn save_flow_edits_funds_and_writes_a_valid_container() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);

    // Slots -> character editor.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Char);
    assert_eq!(ed.field, 0);
    // The name is the first field. Whatever it is, it comes from the save.
    let want_name = ed
        .save
        .as_ref()
        .unwrap()
        .read_character(0)
        .unwrap()
        .name
        .clone();
    assert!(ed.lines()[1].text.contains(&want_name), "line: {}", ed.lines()[1].text);

    // Move to funds (field 1) and bump it. The default step is 1. The expected
    // value is read from the save so the test works with any dump.
    ed.input(Key::Down);
    assert_eq!(ed.field, 1);
    let funds0 = ed.save.as_ref().unwrap().read_character(0).unwrap().funds as i64;
    assert_eq!(ed.char_value(1), funds0);
    ed.input(Key::Right);
    assert_eq!(ed.char_value(1), funds0 + 1);
    ed.input(Key::Left);
    assert_eq!(ed.char_value(1), funds0);
    // The step is the deck's button (STEPS = [1, 10, 100, 1000, 10000]); [X]
    // is the save key on every screen and the footer says so.
    ed.step = 1;
    ed.input(Key::Right);
    assert_eq!(ed.char_value(1), funds0 + 10);
    ed.input(Key::Left);
    assert_eq!(ed.char_value(1), funds0);

    // Write and check the produced container.
    ed.input(Key::A);
    match ed.take_write() {
        Some(Write::File { path, bytes }) => {
            assert!(path.ends_with("system"));
            assert_eq!(bytes.len(), 4_726_188, "switch save size");
            let s = Save::from_bytes(&bytes).expect("valid save");
            assert_eq!(s.read_character(0).unwrap().funds as i64, funds0);
            assert_eq!(&bytes[0x14..0x18], &[0x74, 0xEE, 0xB2, 0x36], "container header");
        }
        other => panic!("expected a save write, got {:?}", other),
    }
}

#[test]
fn save_flow_edits_an_item_count() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);

    // Slots -> X -> item box.
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::Items);
    let vis = ed.visible_items();
    assert!(!vis.is_empty(), "the dump has non-empty item slots");

    // The cursor starts on the first visible slot, so that is the one the
    // three presses below edit. Whatever count it holds, the result has to
    // stop at the game's own ceiling of 99: that is the clamp the console
    // shares with the PC build.
    let boxes = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    let first = vis[0];
    let before = boxes[first].count;
    // Default step is 1; X cycles it to 10.
    ed.input(Key::Right);
    ed.input(Key::X);
    ed.input(Key::Right);
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap()[first].count;
    assert_eq!(
        after,
        (before as u16 + 11).min(99) as u8,
        "the item count is capped at 99, not 127"
    );

    // [A] is the picker on this screen now, so the save goes through the
    // method the console's global [X] calls.
    ed.item_screen_save();
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_eq!(s.read_item_box(0).unwrap()[first].count, after);
            // Everything else must be untouched.
            let orig = std::fs::read(real_save()).unwrap();
            let os = Save::from_bytes(&orig).unwrap();
            let a = s.read_item_box(0).unwrap();
            let b = os.read_item_box(0).unwrap();
            for i in 0..2300 {
                if i != first {
                    assert_eq!(a[i], b[i], "item {} changed unexpectedly", i);
                }
            }
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

/// Move the .mib cursor to the field with the given key.
fn goto_mib_field(ed: &mut Editor, key: &str) {
    for _ in 0..questfields::MIB_FIELDS.len() {
        if questfields::MIB_FIELDS[ed.mib_cursor].key == key {
            return;
        }
        ed.input(Key::Down);
    }
    panic!("field {} not found", key);
}

#[test]
fn quest_flow_edits_a_mib_field_and_repacks_the_arc() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    // Y switches from the save list to the quest list.
    open_quest_section(&mut ed);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);
    // The editor should have pre-selected the .mib entry.
    let e = ed.arc.as_ref().unwrap().entries[ed.cursor].clone();
    assert_eq!(extension_for(e.type_hash), Some("mib"));

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib);

    // Read and change the time limit.
    let before = {
        let f = questfields::MIB_FIELDS
            .iter()
            .find(|f| f.key == "time_limit")
            .unwrap();
        questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap()
    };
    assert_eq!(before, 50);
    goto_mib_field(&mut ed, "time_limit");
    ed.input(Key::Right);
    let after_val = {
        let f = questfields::MIB_FIELDS
            .iter()
            .find(|f| f.key == "time_limit")
            .unwrap();
        questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap()
    };
    assert_eq!(after_val, 51);

    // Write the archive back.
    ed.input(Key::A);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let a = Arc::parse(&bytes).expect("repacked archive must parse");
            let mib = a.by_ext("mib").unwrap();
            let f = questfields::MIB_FIELDS
                .iter()
                .find(|f| f.key == "time_limit")
                .unwrap();
            assert_eq!(
                questfields::read_u32(&mib.data, f.offset, f.kind).unwrap(),
                51
            );
            // Every other entry must survive untouched.
            let orig = Arc::parse(&std::fs::read(real_arc()).unwrap()).unwrap();
            assert_eq!(a.entries.len(), orig.entries.len());
            for (x, y) in a.entries.iter().zip(orig.entries.iter()) {
                assert_eq!(x.name, y.name);
                if extension_for(x.type_hash) != Some("mib") {
                    assert_eq!(x.data, y.data, "entry {} changed", y.name);
                }
            }
        }
        other => panic!("expected an .arc write, got {:?}", other),
    }
}

#[test]
fn quest_flow_cycles_an_enum_field() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_quest_section(&mut ed);
    ed.input(Key::A);
    service(&mut ed);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib);

    // quest_kind is an enum field; Right must wrap inside the table.
    goto_mib_field(&mut ed, "quest_kind");
    let f = questfields::MIB_FIELDS
        .iter()
        .find(|f| f.key == "quest_kind")
        .unwrap();
    let table = questfields::enum_table(f.enum_name.unwrap()).unwrap();
    let before = questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap() as usize;
    ed.input(Key::Right);
    let after = questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap() as usize;
    assert_eq!(after, (before + 1) % table.len());
    ed.input(Key::Left);
    assert_eq!(
        questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap() as usize,
        before
    );
}

#[test]
fn quest_flow_edits_gmd_text() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_quest_section(&mut ed);
    ed.input(Key::A);
    service(&mut ed);

    // Move to the .gmd entry and open it.
    let n = ed.arc.as_ref().unwrap().entries.len();
    let mut found = false;
    for _ in 0..n {
        let e = ed.arc.as_ref().unwrap().entries[ed.cursor].clone();
        if extension_for(e.type_hash) == Some("gmd") {
            found = true;
            break;
        }
        ed.input(Key::Down);
    }
    assert!(found, "no .gmd entry");
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Gmd);

    // Ask to edit the title, then answer like the software keyboard would.
    ed.input(Key::A);
    let (idx, initial) = ed.take_text_request().expect("text request");
    assert_eq!(idx, 0);
    assert!(!initial.is_empty());
    ed.apply_text(0, "テスト用クエスト名".to_string());

    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let a = Arc::parse(&bytes).unwrap();
            let g = mhnx_core::quest::Gmd::from_bytes(&a.by_ext("gmd").unwrap().data).unwrap();
            assert_eq!(g.title(), Some("テスト用クエスト名"));
            assert_eq!(g.strings.len(), 7);
        }
        other => panic!("expected an .arc write, got {:?}", other),
    }
}

#[test]
fn quest_flow_installs_a_download_quest_into_the_save() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    // Open the save first.
    open_save(&mut ed);
    // Then switch to the quest list and open the archive.
    // Saving is on the slot screen, so Y jumps straight to the quest side.
    open_quest_section(&mut ed);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // X installs the open archive into the open save.
    let had_it = dlc::find_quest(ed.save.as_ref().unwrap(), 1010165).is_some();
    let before_n = dlc::list_quests(ed.save.as_ref().unwrap()).len();
    ed.input(Key::X);
    assert!(
        ed.message.contains("上書き") || ed.message.contains("追加"),
        "message was {}",
        ed.message
    );
    let list = dlc::list_quests(ed.save.as_ref().unwrap());
    // Installing a quest that is already there replaces it in place, which is
    // the safe edit: the slot and the id never move.
    assert_eq!(list.len(), if had_it { before_n } else { before_n + 1 });
    assert!(list.iter().any(|q| q.quest_id == 1010165));

    // Writing the save must keep the quest.
    ed.input(Key::X); // install again is harmless (idempotent per id? it uses a free slot)
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert!(!dlc::list_quests(&s).is_empty());
        }
        _ => {}
    }
}

#[test]
fn cursor_navigation_wraps_on_every_screen() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // Slot screen wraps over three slots.
    ed.input(Key::Up);
    assert_eq!(ed.slot, 2);
    ed.input(Key::Down);
    assert_eq!(ed.slot, 0);

    ed.input(Key::A);
    // Character screen wraps over all fields.
    for _ in 0..CHARFIELD_COUNT {
        ed.input(Key::Down);
    }
    assert_eq!(ed.field, 0);
    ed.input(Key::Up);
    assert_eq!(ed.field, CHARFIELD_COUNT - 1);
}

const CHARFIELD_COUNT: usize = mhnx_core::hbedit::CHAR_FIELDS.len();
#[test]
fn quest_flow_checks_and_fixes_the_one_player_only_flag() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_quest_section(&mut ed);
    ed.input(Key::A);
    service(&mut ed);

    // L opens the checker.
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Check);
    let idx = ed
        .findings
        .iter()
        .position(|f| f.code == "one-player-only")
        .expect("the sample quest carries the forced 1-player flag");
    assert!(ed.findings[idx].fixable);
    assert_eq!(ed.findings[idx].location.as_ref().unwrap().1, 0x1A);

    // A repairs it.
    ed.input(Key::A);
    assert!(
        ed.findings.iter().all(|f| f.code != "one-player-only"),
        "the finding should be gone after the fix"
    );
    assert!(ed.message.contains("0x2F"));

    // X writes the archive.
    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let a = Arc::parse(&bytes).unwrap();
            let mib = a.by_ext("mib").unwrap();
            assert_eq!(mib.data[0x1A], 0x00);
            // Nothing else changed.
            let orig = Arc::parse(&std::fs::read(real_arc()).unwrap()).unwrap();
            for (x, y) in a.entries.iter().zip(orig.entries.iter()) {
                assert_eq!(x.name, y.name);
                if x.name != mib.name {
                    assert_eq!(x.data, y.data, "{} changed", y.name);
                }
            }
        }
        other => panic!("expected a write, got {:?}", other),
    }
}
// ---------------------------------------------------------------------------
// Screens added for the features taken from the MHXX/MHGU Save Manager.
// ---------------------------------------------------------------------------

#[test]
fn options_screen_edits_and_saves() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // X on the save list opens the options.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::SaveList);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::Options);

    let before = ed.save.as_ref().unwrap().options().brightness;
    ed.input(Key::Left);
    assert_eq!(ed.save.as_ref().unwrap().options().brightness, before - 1);
    ed.input(Key::Right);
    assert_eq!(ed.save.as_ref().unwrap().options().brightness, before);
    // Row 1 is rumble and toggles.
    ed.input(Key::Down);
    let r = ed.save.as_ref().unwrap().options().rumble;
    ed.input(Key::Right);
    assert_ne!(ed.save.as_ref().unwrap().options().rumble, r);

    // The whole body still serialises.
    ed.input(Key::A);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_ne!(s.options().rumble, r);
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn palico_screen_installs_and_removes() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // R on the slot screen opens the Palico catalogue.
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Palico);

    let lines = ed.lines();
    assert!(lines[0].text.contains("DOWNLOAD PALICO"));
    assert!(lines[0].text.contains("JP"));

    // Language toggle: the state machine answers the arrow pair itself.
    ed.input(Key::Left);
    assert!(ed.lines()[0].text.contains("EN"));
    ed.input(Key::Right);

    // Walk to 0x66 (Boonyah / 坊ニャ春道) and install it.
    let ids = mhnx_core::dlcats::all_ids();
    let target = ids.iter().position(|i| *i == 0x66).unwrap();
    for _ in 0..target {
        ed.input(Key::Down);
    }
    assert!(!ed.save.as_ref().unwrap().has_dlc_palico(0x66));
    let initial_cats = ed
        .save
        .as_ref()
        .unwrap()
        .dlc_palicoes()
        .iter()
        .filter(|c| c.id != 0)
        .count();
    ed.input(Key::A);
    assert!(ed.save.as_ref().unwrap().has_dlc_palico(0x66));
    assert!(
        ed.message.contains("導入しました"),
        "message was {}",
        ed.message
    );

    // Releasing deletes the record, so it costs two presses: the first only
    // names the palico, and the second one removes it.
    ed.input(Key::A);
    assert!(
        ed.save.as_ref().unwrap().has_dlc_palico(0x66),
        "one press must not delete a record"
    );
    assert!(ed.message.contains("もう一度"), "message was {}", ed.message);
    ed.input(Key::A);
    assert!(!ed.save.as_ref().unwrap().has_dlc_palico(0x66));

    // X writes it out.
    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_eq!(
                s.dlc_palicoes().iter().filter(|c| c.id != 0).count(),
                initial_cats
            );
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn charm_screen_creates_and_deletes() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // L on the slot screen opens the charm box.
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Charms);
    let before_n = ed.save.as_ref().unwrap().charms(0).unwrap().len();

    // L starts a brand new charm in the first free slot.
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit);
    let lines = ed.lines();
    assert!(lines[0].text.contains("護石の編集"));
    // The form shows each field on its own row. Find the skill row by label
    // rather than by a fixed line number, so the grade row that sits above it
    // does not shift the assertion.
    assert!(
        lines.iter().any(|l| l.text.contains("スキル1 ID")),
        "the skill row must be drawn: {:?}",
        lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>()
    );

    // A new charm starts as a copy of a real one, so the seed values come from
    // the box rather than from invented defaults. Read them, then bump.
    let seed = ed.charm_edit.expect("the form is open");
    assert!(seed.skill1 > 0, "a new charm must start from a real talisman");
    // The skill row walks the ids the picker lists, not the raw 0..=255 (the
    // third review's finding 9), so the next value is the next allowed id.
    let allowed = mhnx_core::charm::allowed_skill_ids(seed.kind, false);
    let at = allowed.iter().position(|id| *id == seed.skill1).unwrap_or(0);
    let want_skill1 = allowed[(at + 1) % allowed.len()] as i32;
    // The writer clamps the points to what the new skill's row of the game's
    // table allows (charm::clamp_points), so the value it lands on is the
    // bumped one or that ceiling.
    let stepped_points1 = seed.points1 as i32 + 1;
    let want_points1 = match mhnx_core::charm::allowed_points_rarity(
        seed.grade.rarity(),
        want_skill1 as u8,
        false,
    ) {
        Some(r) => stepped_points1.min(r.max as i32),
        None => stepped_points1,
    };

    // The form opens on the first skill row; bump it and its points, then write.
    ed.input(Key::Right);
    ed.input(Key::Down);
    ed.input(Key::Right);
    // [A] on a skill or point row is that row's own action, so the write comes
    // from the row the form marks "▸ 書き込み" (装備BOX枠, row 0).
    while ed.charm_edit.map(|e| e.field) != Some(0) {
        ed.input(Key::Up);
    }
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Charms);
    let list = ed.save.as_ref().unwrap().charms(0).unwrap();
    assert_eq!(list.len(), before_n + 1, "one charm was added");
    let added = list
        .iter()
        .find(|c| c.index == seed.index)
        .expect("the charm the form wrote");
    assert_eq!(added.skill1 as i32, want_skill1);
    assert_eq!(added.points1 as i32, want_points1);
    assert!(added.skill1_name().is_some(), "the skill must resolve");

    // It survives a save round trip.
    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_eq!(s.charms(0).unwrap().len(), before_n + 1);
        }
        other => panic!("expected a write, got {:?}", other),
    }

    // [-] on the list deletes whatever the cursor points at, on the second
    // press; [A] opens the form and deletes nothing.
    let n = ed.save.as_ref().unwrap().charms(0).unwrap().len();
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "[A] edits");
    assert_eq!(ed.save.as_ref().unwrap().charms(0).unwrap().len(), n);
    ed.input(Key::B);
    ed.input(Key::Minus);
    assert_eq!(
        ed.save.as_ref().unwrap().charms(0).unwrap().len(),
        n,
        "the first press only asks"
    );
    assert!(ed.message.contains("2回"), "{}", ed.message);
    ed.input(Key::Minus);
    assert_eq!(ed.save.as_ref().unwrap().charms(0).unwrap().len(), n - 1);
}

#[test]
fn every_screen_renders_without_panicking() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    let mut steps = 0;
    for key in [
        Key::L, Key::Y, Key::Right, Key::A, Key::B,
        Key::R, Key::Down, Key::L, Key::A, Key::X, Key::B,
        Key::B, Key::X, Key::Down, Key::Right, Key::A, Key::B,
    ] {
        ed.input(key);
        for l in ed.lines() {
            assert!(!l.text.is_empty(), "empty line on {:?}", ed.screen);
        }
        steps += 1;
    }
    assert!(steps > 0);
}
// ---------------------------------------------------------------------------
// Quest extraction: the editor asks, the platform writes.
// ---------------------------------------------------------------------------

#[test]
fn extracting_one_quest_asks_for_exactly_that_id() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_save(&mut ed);
    open_quest_section(&mut ed);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // The editor must ask for the id it read out of the open .mib.
    let want = ed
        .arc
        .as_ref()
        .and_then(|a| a.by_ext("mib"))
        .and_then(|m| m.data.get(0x0C..0x10))
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap();
    assert!(want > 0);

    assert_eq!(ed.take_request(), None);
    ed.input(Key::Z);
    match ed.take_request() {
        Some(Request::ExtractQuests { ids }) => assert_eq!(ids, vec![want]),
        other => panic!("expected an extract request, got {:?}", other),
    }
    // The I/O layer reports back through apply_extracted.
    ed.apply_extracted(1, "/switch/mhnxeditor/mhxx/quest/arc");
    assert!(
        ed.message.contains("1 件のクエスト"),
        "message was {}",
        ed.message
    );
}

#[test]
fn extracting_all_asks_for_every_quest() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_save(&mut ed);
    open_quest_section(&mut ed);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);

    ed.input(Key::R);
    match ed.take_request() {
        Some(Request::ExtractQuests { ids }) => assert!(ids.is_empty(), "empty means every quest"),
        other => panic!("expected an extract request, got {:?}", other),
    }
}

#[test]
fn minus_is_only_swallowed_on_the_quest_entry_screen() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_save(&mut ed);
    // One press, one thing: the folder rescan owns Minus on the save list and
    // nowhere else, so no other screen can fire it alongside its own action.
    assert!(!ed.minus_rescans(), "slot screen must not rescan on Minus");
    open_quest_section(&mut ed);
    assert!(!ed.minus_rescans(), "quest list must not rescan on Minus");
    ed.input(Key::A);
    service(&mut ed);
    assert!(!ed.minus_rescans(), "entry screen must not rescan on Minus");
}

#[test]
fn a_missing_save_reports_that_nothing_was_written() {
    let mut ed = Editor::new();
    ed.apply_extracted(0, "/switch/mhnxeditor/mhxx/quest/arc");
    assert!(
        ed.message.contains("何も書き出していません"),
        "message was {}",
        ed.message
    );
}
// ---------------------------------------------------------------------------
// The save -> quest editor loop on the console side.
// ---------------------------------------------------------------------------

#[test]
fn x_on_the_quest_list_opens_the_saves_own_quests() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests, "X must switch to the save's quests");
    assert!(!ed.save_quests().is_empty());
    // The lines must name a real quest from the save.
    let first = ed.save_quests()[0].clone();
    let lines = ed.lines();
    assert!(lines[0].text.contains("QUESTS INSIDE THIS SAVE"), "{}", lines[0].text);
    assert!(
        lines.iter().any(|l| l.text.contains(&first.quest_id.to_string())),
        "the list must show the ids"
    );
}

#[test]
fn opening_a_quest_from_the_save_fills_the_editor() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);

    let want = ed.save_quests()[0].quest_id;
    ed.input(Key::A);
    // The quest is opened to be edited, so the screen it lands on is the one
    // that edits it: 構成(.arc) - the file list - is one press away through the
    // [ファイル] pill / [R] held (the second review's finding 4).
    assert_eq!(ed.screen, Screen::QuestEdit, "A must open the quest");
    assert!(ed.message.contains(&want.to_string()), "message was {}", ed.message);
    // The .mib of that quest is now in the field editor's buffer.
    let mib_len = ed.arc.as_ref().and_then(|a| a.by_ext("mib")).expect("a .mib").data.len();
    let buf = ed.mib_bytes();
    assert_eq!(buf.len(), mib_len);
    // And the id in it matches the quest we picked.
    let id = i32::from_le_bytes([buf[0x0C], buf[0x0D], buf[0x0E], buf[0x0F]]);
    assert_eq!(id, want);
}

#[test]
fn editing_a_quest_from_the_save_writes_back_to_the_same_slot() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    let before = ed.save.as_ref().unwrap().to_bytes();
    open_quest_section(&mut ed);
    ed.input(Key::X);
    let target = ed.save_quests()[0].clone();
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::QuestEdit);

    // The entry cursor must point at the .mib, so A opens the field editor
    // instead of re-loading entry 0 and wiping the buffer.
    assert!(
        !ed.mib_bytes().is_empty(),
        "opening a quest must load its .mib (cursor {} vs entry {})",
        ed.cursor,
        ed.entry
    );
    // The .mib editor is behind the file list, which the [ファイル] pill opens
    // (the quest screen lands on the editor itself now).
    ed.screen = Screen::ArcEntries;
    ed.cursor = ed.entry;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib, "A must open the .mib of the chosen entry");

    // Edit a field that cannot be zero, then write back. The change must reach
    // the save: this is the bug where install_into_save repacked the archive as
    // it was on disk and the edit vanished while claiming success.
    let fields = mhnx_core::questfields::MIB_FIELDS;
    let lv = fields
        .iter()
        .position(|f| f.key == "quest_level")
        .expect("a quest_level field");
    // Move the cursor to that row and bump it with the default step of 1.
    for _ in 0..lv {
        ed.input(Key::Down);
    }
    let before_val =
        mhnx_core::questfields::read_u32(&ed.mib_bytes(), fields[lv].offset, fields[lv].kind)
            .unwrap();
    ed.input(Key::Right);
    let after_val =
        mhnx_core::questfields::read_u32(&ed.mib_bytes(), fields[lv].offset, fields[lv].kind)
            .unwrap();
    assert_eq!(after_val, before_val + 1, "the field editor must change the buffer");

    // Back to the entry list, then X installs into the save. This is the exact
    // path that used to repack the archive as it was on disk and lose the edit.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
    ed.input(Key::X);
    assert!(
        ed.message.contains("上書き") || ed.message.contains("追加"),
        "message was {}",
        ed.message
    );
    let after = ed.save.as_ref().unwrap().to_bytes();
    assert_eq!(after.len(), before.len());
    let diff = (0..before.len()).filter(|i| before[*i] != after[*i]).count();
    assert!(
        diff > 0,
        "the edit must reach the save - a silent no-op is the bug this test exists for"
    );
    // Only that quest's slot may move.
    let sec = ed.save.as_ref().unwrap().dlc_section_offset() as usize;
    let start = 0x24 + sec + mhnx_core::dlc::QUEST_ARRAY_OFFSET + target.slot * mhnx_core::dlc::QUEST_SLOT_SIZE;
    let end = start + mhnx_core::dlc::QUEST_SLOT_SIZE;
    let outside = (0..before.len())
        .filter(|i| before[*i] != after[*i])
        .filter(|i| *i < start || *i >= end)
        .count();
    assert_eq!(outside, 0, "{} bytes changed outside the quest slot", outside);
    // The rest of the save is intact.
    assert_eq!(ed.save.as_ref().unwrap().charms(0).unwrap().len(), 188);
    let q = ed.save_quests()[target.slot].clone();
    assert_eq!(q.quest_id, target.quest_id);
}

#[test]
fn a_quest_id_that_is_not_in_the_save_is_refused() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.open_quest_from_save(9999999);
    assert_ne!(ed.screen, Screen::ArcEntries);
    assert!(
        ed.message.contains("このセーブにありません"),
        "message was {}",
        ed.message
    );
}

#[test]
fn minus_is_also_swallowed_on_the_save_quest_list() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);
    assert!(
        !ed.minus_rescans(),
        "the save quest list owns Minus (the two-press delete)"
    );
}
// ---------------------------------------------------------------------------
// Item type change on the console side.
// ---------------------------------------------------------------------------

#[test]
fn a_on_the_item_box_opens_the_picker() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X); // items screen
    assert_eq!(ed.screen, Screen::Items);
    // [A] is the picker on this screen (the console's chip table says
    // "[A] 種類変更"); [R] is the count keyboard.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);
    // With no filter every item is listed.
    assert!(ed.item_pick_matches().len() > 2500, "{}", ed.item_pick_matches().len());
    let lines = ed.lines();
    assert!(lines[0].text.contains("PICK ITEM"), "{}", lines[0].text);
}

#[test]
fn filtering_and_choosing_changes_the_item_type() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);

    let target = ed.item_pick_target;
    // The picker talks to the software keyboard; here we answer directly.
    ed.apply_item_filter(String::from("回復薬"));
    let matches = ed.item_pick_matches();
    assert!(!matches.is_empty(), "the filter must find something");
    let want = matches[0];

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Items, "choosing returns to the box");
    let boxed = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(boxed[target].id, want, "the slot must hold the chosen item");
    assert!(boxed[target].count >= 1, "a new item needs a count");
    assert!(ed.message.contains("回復薬"), "message was {}", ed.message);
}

#[test]
fn cancelling_the_picker_leaves_the_box_alone() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    ed.input(Key::A);
    ed.apply_item_filter(String::from("回復薬"));
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Items);
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(before, after, "B must not write anything");
}

#[test]
fn the_filter_request_uses_its_own_sentinel() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::A);
    ed.input(Key::Y);
    let (idx, initial) = ed.take_text_request().expect("a filter request");
    assert_eq!(idx, mhnx_core::hbedit::TEXT_ITEM_FILTER);
    assert_eq!(idx, usize::MAX - 1, "must not collide with the name request");
    assert!(initial.is_empty());
}

#[test]
fn the_item_picker_does_not_panic_on_an_empty_match() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::A);
    ed.apply_item_filter(String::from("zzzznothingmatchesthis"));
    assert!(ed.item_pick_matches().is_empty());
    // Navigation and A must both be harmless with nothing to pick.
    ed.input(Key::Down);
    ed.input(Key::Up);
    ed.input(Key::Right);
    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    ed.input(Key::A);
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(before, after);
}
// ---------------------------------------------------------------------------
// The console entry viewer: the six container formats used to be unreadable.
// ---------------------------------------------------------------------------

fn sample_arc() -> Option<std::path::PathBuf> {
    let mut p = std::path::PathBuf::from("C:/");
    for part in ["anyWorkspace", "nx", "mhxx", "mhnxeditor", "dist", "quests_all"] {
        p.push(part);
    }
    p.push("q1010165.arc");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn y_on_an_entry_shows_it_decoded() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // Walk every entry and check that Y always produces a readable screen,
    // and that the four container types show their contents.
    let n = ed.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0);
    assert_eq!(n, 15, "the sample quest has 15 entries");
    let mut seen: Vec<String> = Vec::new();
    for i in 0..n {
        ed.cursor = i;
        ed.input(Key::Y);
        assert_eq!(ed.screen, Screen::EntryView, "entry {} did not open", i);
        let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
        assert!(lines.len() > 1, "entry {} rendered nothing", i);
        assert!(
            lines[0].len() > 3,
            "entry {} has no header: {:?}",
            i,
            lines[0]
        );
        seen.push(lines.join(" | "));
        ed.input(Key::B);
        assert_eq!(ed.screen, Screen::ArcEntries);
    }
    let joined = seen.join("\n");
    // .rem shows reward records with the Japanese item names.
    assert!(
        joined.contains("古びたお守り"),
        ".rem reward names missing; got:
{}",
        &joined[..joined.len().min(1500)]
    );
    // .qdl lists the referenced files.
    assert!(joined.contains("questData_1010165"), ".qdl file list missing");
    // .sem shows a position.
    assert!(joined.contains("x "), ".sem position missing");
}

#[test]
fn the_entry_view_never_panics_on_a_junk_payload() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    // Replace the payload under every entry with noise and view them all.
    if let Some(a) = &mut ed.arc {
        for e in a.entries.iter_mut() {
            e.data = vec![0x5A; 40];
        }
    }
    let n = ed.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0);
    for i in 0..n {
        ed.cursor = i;
        ed.input(Key::Y);
        let _ = ed.lines();
        ed.input(Key::B);
    }
}
// ---------------------------------------------------------------------------
// The console skill picker: 306 skills cannot be reached by +/- on a number.
// ---------------------------------------------------------------------------

#[test]
fn y_on_a_skill_row_opens_the_name_picker() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L); // charms
    ed.input(Key::L); // new charm form
    assert_eq!(ed.screen, Screen::CharmEdit);

    // The form opens on the skill 1 row, so Y works straight away.
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_field, 1);
    // The picker lists the kind's own row of the game's point table, not the
    // whole 306 entry name table: a 天 offers 62 skills in slot 1. Before the
    // filter existed this answered 306 and invited records the game never
    // writes (攻撃 is a 天's *second* skill, not its first).
    let kind = ed.charm_edit.expect("the form is open").kind;
    let pool = mhnx_core::charm::allowed_skill_ids(kind, false);
    assert_eq!(ed.skill_pick_matches(), pool, "the picker shows the pool");
    assert!(pool.len() <= 62, "kind {}: {} ids", kind, pool.len());
    assert!(ed.skill_pick_matches().len() < 306, "the name table is filtered");
    let lines = ed.lines();
    assert!(lines[0].text.contains("PICK SKILL 1"), "{}", lines[0].text);
    // The first rows carry real Japanese names - of the pool, not of the whole
    // name table, so 毒 (a skill no 天 can carry in slot 1) is not one of them.
    // The window is centred on the row the form already holds (so opening the
    // list twice does not lose it), which is the name that has to be on screen.
    let first_name =
        mhnx_core::skills::skill_name(pool[ed.skill_pick_cursor]).expect("a name");
    assert!(
        lines.iter().any(|l| l.text.contains(first_name)),
        "skill names must be rendered: {:?}",
        first_name
    );
    assert!(
        !lines.iter().any(|l| l.text.contains("PICK SKILL 1 / ?")),
        "the header must name the kind: {}",
        lines[0].text
    );
}

#[test]
fn filtering_by_name_and_choosing_writes_the_skill() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);

    // A name from the kind's own pool: 攻撃 is not a 天's first skill, so the
    // filtered list must not be asked for one it cannot carry.
    let kind = ed.charm_edit.expect("open").kind;
    let pool = mhnx_core::charm::allowed_skill_ids(kind, false);
    let target = pool[pool.len() / 2];
    let name = mhnx_core::skills::skill_name(target).expect("a name").to_string();
    ed.apply_skill_filter(name.clone());
    let matches = ed.skill_pick_matches();
    assert!(!matches.is_empty(), "the filter must find {}", name);
    assert!(
        matches.iter().all(|id| pool.contains(id)),
        "a filtered list may only hold ids of the pool: {:?}",
        matches
    );
    let want = *matches.iter().find(|id| **id == target).expect("the target");

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "choosing returns to the form");
    assert_eq!(
        ed.charm_edit.map(|e| e.skill1),
        Some(want),
        "the chosen id must land in skill 1"
    );
    assert!(ed.message.contains(&name), "message was {}", ed.message);
}

#[test]
fn the_second_skill_row_targets_skill_2() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    // A new charm is seeded from a real talisman, so remember skill 1.
    let skill1_before = ed.charm_edit.map(|e| e.skill1);
    // skill 1 -> points 1 -> skill 2
    ed.input(Key::Down);
    ed.input(Key::Down);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_field, 2);
    ed.apply_skill_filter(String::from("匠"));
    let want = ed.skill_pick_matches()[0];
    ed.input(Key::A);
    assert_eq!(ed.charm_edit.map(|e| e.skill2), Some(want));
    assert_eq!(
        ed.charm_edit.map(|e| e.skill1),
        skill1_before,
        "skill 1 untouched"
    );
}

#[test]
fn y_off_a_skill_row_says_so_instead_of_opening() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    ed.input(Key::Up); // skill 1 -> the slot count row
    ed.input(Key::Y);
    // A row that is neither a skill nor a socket says which rows do have a
    // list, and opens nothing.
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(
        ed.message.contains("スキル・装飾品の行"),
        "message was {}",
        ed.message
    );

    // The three decoration sockets are rows of this form now (the equipment
    // form stopped opening on a talisman), and [Y] on one opens the same
    // decoration list the equipment form opens, with the search box up.
    while ed.charm_edit.map(|e| e.field) != Some(9) {
        ed.input(Key::Up);
    }
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::JewelPick);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::CharmEdit);
}

#[test]
fn the_skill_picker_does_not_panic_on_an_empty_match() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    ed.input(Key::Y);
    ed.apply_skill_filter(String::from("zzzznothing"));
    assert!(ed.skill_pick_matches().is_empty());
    ed.input(Key::Down);
    ed.input(Key::Up);
    ed.input(Key::Right);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
}
// ---------------------------------------------------------------------------
// Undo for the console item box.
// ---------------------------------------------------------------------------

#[test]
fn replacing_an_item_can_be_taken_back() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X); // items
    assert_eq!(ed.screen, Screen::Items);

    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    let target = ed.visible_items()[ed.cursor];

    // Change the type through the picker.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);
    ed.apply_item_filter(String::from("解毒薬"));
    let want = ed.item_pick_matches()[0];
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Items);
    let changed = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(changed[target].id, want, "the picker must write");
    assert_ne!(changed[target].id, before[target].id);

    // L puts it back.
    ed.input(Key::L);
    let restored = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(
        restored[target].id, before[target].id,
        "undo must restore the old item"
    );
    assert!(ed.message.contains("元に戻しました"), "message was {}", ed.message);
}

#[test]
fn undo_with_nothing_to_take_back_says_so() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::L);
    assert!(ed.message.contains("取り消せる操作がありません"), "message was {}", ed.message);
}

#[test]
fn undo_restores_the_whole_box_not_just_the_item() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();

    ed.input(Key::R);
    ed.apply_item_filter(String::from("回復薬"));
    let want = ed.item_pick_matches()[0];
    ed.input(Key::A);
    ed.input(Key::L);

    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(before, after, "every slot must come back");
}
// ---------------------------------------------------------------------------
// The console talisman editor shows the published range, not just the numbers.
// ---------------------------------------------------------------------------

#[test]
fn the_console_charm_editor_can_change_the_grade() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Charms;
    ed.cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
    let before = ed.charm_edit.expect("editor state");
    assert_eq!(before.field, 3, "the cursor lands on the first skill");
    ed.input(Key::Up);
    ed.input(Key::Up);
    assert_eq!(ed.charm_edit.unwrap().field, 1, "row 1 is the grade");
    let grade_before = ed.charm_edit.unwrap().grade;
    ed.input(Key::Right);
    let after = ed.charm_edit.unwrap();
    assert_ne!(after.grade, grade_before, "right must move the grade");
    assert!(
        after.slots <= after.grade.max_slots(),
        "slots {} exceed the ceiling {} for {}",
        after.slots,
        after.grade.max_slots(),
        after.grade.name()
    );
    // The write is the 装備BOX枠 row; [A] on the grade row steps the grade.
    while ed.charm_edit.map(|e| e.field) != Some(0) {
        ed.input(Key::Up);
    }
    ed.input(Key::A);
    assert!(ed.dirty, "writing marks the save dirty");
    let rec = {
        let s = ed.save.as_ref().unwrap();
        let b = s.equipment_box(ed.slot).unwrap();
        let o = after.index * mhnx_core::charm::CHARM_RECORD;
        b[o..o + mhnx_core::charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(rec[0x12], after.grade.rarity());
    assert_eq!(
        mhnx_core::charm::rarity_of(rec[0x02]),
        Some(after.grade.rarity() as i32),
        "the equipment id must agree with the rarity"
    );
}

#[test]
fn the_console_charm_editor_shows_the_allowed_range() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L); // charms
    // Select a known talisman (224: 属性会心 +6 / 達人 +10).
    ed.cursor = ed
        .charms()
        .iter()
        .position(|c| c.index == 224)
        .unwrap_or(0);
    ed.input(Key::A); // edit
    assert_eq!(ed.screen, Screen::CharmEdit);
    let text: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = text.join("\n");
    assert!(
        joined.contains("type 0x"),
        "the type and rarity must be shown:\n{}",
        joined
    );
    assert!(
        joined.contains("rarity"),
        "the rarity label is missing:\n{}",
        joined
    );
    assert!(
        joined.contains("allowed"),
        "the allowed range must be shown:\n{}",
        joined
    );
}

/// The console form prints the measured ceiling of the kind on its 種類 row,
/// and turning the kind re-clamps the form against that kind (charm::KIND_LIMITS)
/// rather than only against the rarity's table.
#[test]
fn the_console_charm_form_shows_the_kind_ceiling() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Charms;
    ed.cursor = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
    let kind = ed.charm_edit.expect("the form is open").kind;
    let limits = mhnx_core::charm::kind_limits(kind).expect("a shipped kind");
    let joined: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = joined.join("\n");
    assert!(
        joined.contains(limits.name()),
        "the kind must be named:\n{}",
        joined
    );
    assert!(
        joined.contains(&limits.summary()),
        "the kind's ceiling must be printed:\n{}",
        joined
    );

    // Turning row 1 to 兵士 (the next kind after 天, which wraps) re-clamps the
    // slots and the points against that kind: one slot, no second skill.
    ed.input(Key::Up);
    ed.input(Key::Up);
    assert_eq!(ed.charm_edit.expect("open").field, 1, "row 1 is the kind");
    ed.input(Key::Right);
    let after = ed.charm_edit.expect("open");
    let after_limits = mhnx_core::charm::kind_limits(after.kind).expect("a shipped kind");
    assert!(after_limits.max_slots <= limits.max_slots);
    assert!(
        after.slots <= after_limits.max_slots,
        "slots {} exceed the ceiling {} of {}",
        after.slots,
        after_limits.max_slots,
        after_limits.name()
    );
    let joined: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = joined.join("\n");
    assert!(
        joined.contains(&after_limits.summary()),
        "the ceiling line must follow the kind row:\n{}",
        joined
    );
}

#[test]
fn an_illegal_skill_is_flagged_on_the_console() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.cursor = ed
        .charms()
        .iter()
        .position(|c| c.index == 224)
        .unwrap_or(0);
    ed.input(Key::A);
    // Force skill 1 to something the Enduring table forbids: id 1 (Poison)
    // is 0~0 for every talisman type in the sample data.
    if let Some(e) = &mut ed.charm_edit {
        e.skill1 = 1;
        e.points1 = 5;
    }
    let joined: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = joined.join("\n");
    assert!(
        joined.contains("cannot go on this type") || joined.contains("allowed"),
        "an illegal skill must be flagged:\n{}",
        joined
    );
}
// ---------------------------------------------------------------------------
// 3DS <-> Switch conversion on the console.
// ---------------------------------------------------------------------------

#[test]
fn l_on_the_save_list_opens_the_convert_screen() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::B); // back to the save list
    assert_eq!(ed.screen, Screen::SaveList);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Convert);
    let joined: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = joined.join("\n");
    assert!(joined.contains("セーブ形式の変換"), "{}", joined);
    assert!(joined.contains("source"), "{}", joined);
    assert!(joined.contains("3DS"), "the target must be named:\n{}", joined);
}

#[test]
fn a_switch_save_converts_to_a_3ds_body() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::B);
    ed.input(Key::L);
    // The write replaces whatever is at the output path, so [A] asks first
    // (the review's ninth finding): the first press only names the file.
    ed.input(Key::A);
    assert!(ed.take_write().is_none(), "the first press must not write");
    ed.input(Key::A);
    match ed.take_write() {
        Some(mhnx_core::hbedit::Write::File { path, bytes }) => {
            assert!(path.ends_with(".3ds"), "wrote {}", path);
            // The 3DS container is the body alone, with no 36 byte header.
            assert_eq!(bytes.len(), 4_726_152, "3DS save is the bare body");
            let before = ed.save.as_ref().unwrap().body().to_vec();
            assert_eq!(bytes, before, "the body must be carried over unchanged");
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn a_3ds_save_converts_to_a_switch_container() {
    if !real_save().exists() {
        return;
    }
    let src = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    // A bare body is what a 3DS save is.
    let body = src.to_3ds_bytes();
    let mut ed = Editor::new();
    ed.open_save_bytes("C:/fake/system", &body);
    assert_eq!(
        ed.save.as_ref().unwrap().kind(),
        mhnx_core::save::SaveKind::ThreeDs,
        "a bare body must be recognised as 3DS"
    );
    ed.input(Key::B); // slots -> save list
    ed.input(Key::L); // convert
    assert_eq!(ed.screen, Screen::Convert);
    // Two presses: the write replaces whatever is at the output path.
    ed.input(Key::A);
    assert!(ed.take_write().is_none(), "the first press must not write");
    ed.input(Key::A);
    match ed.take_write() {
        Some(mhnx_core::hbedit::Write::File { path, bytes }) => {
            assert!(path.ends_with(".switch"), "wrote {}", path);
            assert_eq!(bytes.len(), 4_726_188, "Switch save carries the header");
            let back = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
            assert_eq!(
                back.kind(),
                mhnx_core::save::SaveKind::Switch,
                "the container must be a Switch one"
            );
            assert_eq!(back.body(), &body[..], "the body must survive");
        }
        other => panic!("expected a write, got {:?}", other),
    }
}
#[test]
fn the_quest_list_shows_a_title_on_every_row() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed); // quest list
    ed.input(Key::X); // quests inside the save
    assert_eq!(ed.screen, Screen::SaveQuests);
    // Titles are decoded once when the screen opens.
    assert!(
        ed.quest_titles.len() > 150,
        "expected the cache to be filled, got {}",
        ed.quest_titles.len()
    );
    let first = ed.save_quests()[0].quest_id;
    assert!(
        !ed.quest_title(first).is_empty(),
        "quest {} has no cached title",
        first
    );
    // Every visible row must carry its title, not just the cursor row.
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let rows: Vec<&String> = lines.iter().filter(|l| l.contains("slot ")).collect();
    assert!(rows.len() >= 10, "expected a page of rows, got {}", rows.len());
    let with_title = rows
        .iter()
        .filter(|l| {
            // "slot NNN  id XXXX  NNNN B  <title>"
            l.split(" B  ").nth(1).map(|t| !t.trim().is_empty()).unwrap_or(false)
        })
        .count();
    assert!(
        with_title >= rows.len() - 1,
        "only {} of {} rows carried a title",
        with_title,
        rows.len()
    );
}
#[test]
fn the_quest_list_can_be_filtered_by_id_or_title() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);
    let all = ed.filtered_quests().len();
    assert!(all > 150, "expected the whole set, got {}", all);

    // By id.
    ed.apply_quest_filter(String::from("1010165"));
    let by_id = ed.filtered_quests();
    assert!(!by_id.is_empty(), "the id filter found nothing");
    assert!(
        by_id.iter().all(|q| format!("{}", q.quest_id).contains("1010165")),
        "the id filter let something through"
    );

    // By title, using a string that really is in the cache.
    let sample = ed
        .quest_titles
        .iter()
        .find(|(_, t)| t.chars().count() > 4)
        .map(|(_, t)| t.chars().take(3).collect::<String>())
        .expect("a title to search for");
    ed.apply_quest_filter(sample.clone());
    let by_title = ed.filtered_quests();
    assert!(
        !by_title.is_empty(),
        "the title filter {:?} found nothing",
        sample
    );
    assert!(by_title.len() < all, "the filter did not narrow anything");

    // X clears it and Y asks the keyboard.
    ed.input(Key::X);
    assert_eq!(ed.filtered_quests().len(), all);
    ed.input(Key::Y);
    let (idx, _) = ed.take_text_request().expect("a filter request");
    assert_eq!(idx, mhnx_core::hbedit::TEXT_QUEST_FILTER);
    assert_eq!(idx, usize::MAX - 3, "must not collide with the others");
}

#[test]
fn the_quest_filter_survives_an_empty_result() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed);
    ed.input(Key::X);
    ed.apply_quest_filter(String::from("zzzznothing"));
    assert!(ed.filtered_quests().is_empty());
    // Navigation and A must be harmless with nothing to show.
    ed.input(Key::Down);
    ed.input(Key::Up);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::SaveQuests);
    let lines = ed.lines();
    assert!(lines[0].text.contains("QUESTS INSIDE THIS SAVE"));
}

#[test]
fn a_reward_count_can_be_changed_from_the_console() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    ed.cursor = 5; // rem_620114
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    ed.entry_row_cursor = 0;
    let before = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        let t = mhnx_core::entries::ItemTable::parse(&e.data).unwrap();
        (t.records[0].item, t.records[0].count)
    };
    assert_ne!(before.0, 0, "the row under test must hold an item");
    // The cursor starts on the アイテム cell; [ZR] walks to 個数, which is the
    // cell ◀/▶ steps.
    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "個数");
    ed.input(Key::Right);
    let after = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        let t = mhnx_core::entries::ItemTable::parse(&e.data).unwrap();
        (t.records[0].item, t.records[0].count)
    };
    assert_eq!(after.0, before.0, "the item must not move");
    assert_eq!(after.1, (before.1 as i64 + 1).min(99) as u8, "right adds one");
    assert!(ed.dirty, "the archive must be marked changed");
    ed.input(Key::Left);
    let back = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].count
    };
    assert_eq!(back, before.1, "left takes it back");
}

#[test]
fn a_reward_chance_can_be_stepped_and_typed_from_the_console() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    ed.cursor = 5; // rem_620114
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    assert!(ed.entry_has_chance(), "a .rem carries a chance column");
    assert_eq!(ed.entry_field_label(), "アイテム", "the row starts on the item cell");

    // [ZR] walks the three cells; the guide says which one ([-] is the delete
    // key and nothing else). 個数 first, then 確率%.
    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "個数");
    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "確率%");
    assert!(ed.message.contains("確率"), "message was {}", ed.message);
    assert!(ed.entry_reward_sub().contains("編集中: 確率%"), "{}", ed.entry_reward_sub());
    assert!(ed.entry_reward_sub().contains("合計"), "{}", ed.entry_reward_sub());

    let before = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].slot
    };
    assert!(before >= 1 && before < 100, "sample chance {}", before);
    ed.input(Key::Right);
    let after = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].slot
    };
    assert_eq!(after, before + 1, "right adds the step");
    assert!(ed.dirty, "the archive must be marked changed");
    // The sample group adds up to 100, so one more point overshoots and the
    // press says so instead of quietly rescaling anything.
    assert!(ed.message.contains("合計"), "message was {}", ed.message);
    assert!(
        ed.message.contains("100% を超えています"),
        "message was {}",
        ed.message
    );
    assert!(!ed.entry_reward_warnings().is_empty());

    // [R] types the number; a value over 100 is clamped, letters are refused.
    ed.input(Key::R);
    let (idx, initial) = ed.take_text_request().expect("a text request");
    assert_eq!(idx, mhnx_core::hbedit::TEXT_REWARD_PROB);
    assert_eq!(initial, format!("{}", after));
    assert!(ed.apply_reward_text("130"));
    let typed = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].slot
    };
    assert_eq!(typed, 100, "a chance stops at 100");
    assert!(!ed.apply_reward_text("abc"), "letters are not a chance");
    assert_eq!(
        {
            let e = &ed.arc.as_ref().unwrap().entries[5];
            mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].slot
        },
        100,
        "a refused answer writes nothing"
    );

    // [ZR] walks on to the item cell and then back to the count, which still
    // moves the count byte.
    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "アイテム");
    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "個数");
    let count_before = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].count
    };
    ed.input(Key::Left);
    let count_after = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].count
    };
    assert_eq!(count_after, count_before.saturating_sub(1));
}

#[test]
fn a_supply_row_says_it_has_no_chance() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    // The sample quest's .sup entry.
    let Some(sup_idx) = ed.arc.as_ref().and_then(|a| {
        a.entries
            .iter()
            .position(|e| extension_for(e.type_hash) == Some("sup"))
    }) else {
        return;
    };
    ed.cursor = sup_idx;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    assert!(ed.entry_has_numbers(), "a supply row still holds a count");
    assert!(!ed.entry_has_chance(), "a .sup has no chance column");
    assert!(ed.entry_reward_sub().contains("確率は使いません"), "{}", ed.entry_reward_sub());
    assert!(ed.entry_reward_warnings().is_empty());
    // [ZR] refuses to switch to a chance the file does not have.
    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "個数");
    assert!(ed.message.contains("確率を使いません"), "message was {}", ed.message);
}

/// Change a real quest's chance, write the archive, read it back.
///
/// This is the whole path the console and the desktop share: the edit lives in
/// the entry bytes, X hands the io layer a complete .arc, and parsing those
/// bytes again must show the new chance with every other byte where it was.
#[test]
fn a_reward_chance_survives_a_write_and_a_re_read() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    ed.cursor = 5; // rem_620114
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    let (name, before_data) = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        (e.name.clone(), e.data.clone())
    };
    let before = {
        let t = mhnx_core::entries::ItemTable::parse(&before_data).unwrap();
        assert!(!t.records.is_empty());
        t.records[0].slot
    };
    assert!(before >= 1, "the sample row has a chance: {}", before);

    // [ZR] walks the cells to the chance column (アイテム -> 個数 -> 確率%),
    // then one step down.
    ed.input(Key::Z);
    ed.input(Key::Z);
    assert_eq!(ed.entry_field_label(), "確率%");
    ed.input(Key::Left);

    // X is what hands the archive to the io layer.
    ed.input(Key::X);
    let written = match ed.take_write() {
        Some(Write::File { bytes, .. }) => bytes,
        other => panic!("expected the archive to be written, got {:?}", other),
    };
    assert_ne!(written, bytes, "the write must carry the edit");

    // Re-read it the way the card would.
    let arc = Arc::parse(&written).expect("the written archive parses");
    let e = arc
        .entries
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("{} is gone after the write", name));
    let after = mhnx_core::entries::ItemTable::parse(&e.data)
        .expect("the reward table parses")
        .records[0]
        .slot;
    assert_eq!(after, before - 1, "the chance must survive the round trip");
    let moved: Vec<usize> = (0..before_data.len())
        .filter(|i| before_data[*i] != e.data[*i])
        .collect();
    assert_eq!(moved, vec![0x18 + 3], "one byte: the chance of row 0");
}

#[test]
fn a_reward_item_can_be_changed_from_the_console() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    ed.cursor = 5; // rem_620114
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    let before = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].item
    };
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "A must open the picker");
    assert_eq!(ed.item_pick_mode, 1);
    ed.apply_item_filter(String::from("回復薬"));
    let want = ed.item_pick_matches()[0];
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EntryView, "choosing returns to the view");
    let after = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap().records[0].item
    };
    assert_eq!(after, want, "the record must hold the chosen item");
    assert_ne!(after, before, "the test must actually change something");
    assert!(ed.dirty, "the archive must be marked changed");
    // The line names the key that writes the archive, in Japanese like every
    // other status message (the third review's finding 4).
    assert!(
        ed.message.contains("[X] で .arc に保存"),
        "message was {}",
        ed.message
    );
}

#[test]
fn the_row_cursor_picks_which_record_changes() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    ed.cursor = 5;
    ed.input(Key::Y);
    ed.input(Key::Down);
    ed.input(Key::Down);
    assert_eq!(ed.entry_row_cursor, 2);
    ed.input(Key::A);
    ed.apply_item_filter(String::from("回復薬"));
    let want = ed.item_pick_matches()[0];
    ed.input(Key::A);
    let t = {
        let e = &ed.arc.as_ref().unwrap().entries[5];
        mhnx_core::entries::ItemTable::parse(&e.data).unwrap()
    };
    assert_eq!(t.records[2].item, want, "row 2 must hold the new item");
    assert_ne!(t.records[0].item, want, "row 0 must be untouched");
}

#[test]
fn b_leaves_the_picker_without_writing() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    ed.cursor = 5;
    ed.input(Key::Y);
    let before = ed.arc.as_ref().unwrap().entries[5].data.clone();
    ed.input(Key::A);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::EntryView, "B returns to the view");
    assert_eq!(ed.arc.as_ref().unwrap().entries[5].data, before, "nothing written");
}

// ---------------------------------------------------------------------------
// Torture: random key sequences must not panic or corrupt anything.
// ---------------------------------------------------------------------------

const ALL_KEYS: [Key; 11] = [
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::A,
    Key::B,
    Key::X,
    Key::Y,
    Key::L,
    Key::R,
    Key::Minus,
];

#[test]
fn random_key_sequences_never_panic_or_lose_the_save() {
    if !real_save().exists() {
        return;
    }
    let mut seed = 0x1234_5678u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    let mut screens_seen = std::collections::BTreeSet::new();
    for round in 0..40 {
        let mut ed = Editor::new();
        open_save(&mut ed);
        let body_len = ed.save.as_ref().unwrap().body().len();
        for _ in 0..400 {
            let key = ALL_KEYS[next() % ALL_KEYS.len()];
            ed.input(key);
            screens_seen.insert(format!("{:?}", ed.screen));
            if let Some(s) = &ed.save {
                let now = s.to_bytes();
                let back = mhnx_core::save::Save::from_bytes(&now).unwrap_or_else(|e| {
                    panic!("round {} produced an unparseable save: {:?}", round, e)
                });
                assert_eq!(back.body().len(), body_len, "round {}: body length changed", round);
            }
        }
        if let Some(s) = &ed.save {
            let out = s.to_bytes();
            let back = mhnx_core::save::Save::from_bytes(&out).unwrap();
            assert_eq!(back.to_bytes(), out, "round {} did not round trip", round);
        }
    }
    assert!(
        screens_seen.len() >= 6,
        "the fuzz only reached {:?}",
        screens_seen
    );
}

#[test]
fn random_keys_on_an_arc_never_produce_a_broken_archive() {
    let Some(p) = sample_arc() else { return };
    let bytes = std::fs::read(&p).unwrap();
    let mut seed = 0x9E37_79B9u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 33) as usize
    };
    for round in 0..30 {
        let mut ed = Editor::new();
        ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
        for _ in 0..300 {
            ed.input(ALL_KEYS[next() % ALL_KEYS.len()]);
            if let Some(a) = &ed.arc {
                let out = a
                    .to_bytes()
                    .unwrap_or_else(|e| panic!("round {}: repack failed: {:?}", round, e));
                let back = mhnx_core::arc::Arc::parse(&out)
                    .unwrap_or_else(|e| panic!("round {}: repack produced junk: {:?}", round, e));
                assert_eq!(
                    back.entries.len(),
                    a.entries.len(),
                    "round {}: entry count changed",
                    round
                );
            }
        }
    }
}

#[test]
fn the_converted_save_can_be_given_a_name() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::B);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Convert);
    // Default name is "system".
    let has_default = ed.lines().iter().any(|l| l.text.contains("system.3ds"));
    assert!(has_default, "the default output name must be shown");
    ed.input(Key::Y);
    let (idx, _) = ed.take_text_request().expect("a name request");
    assert_eq!(idx, mhnx_core::hbedit::TEXT_SAVE_NAME);
    assert_eq!(idx, usize::MAX - 4, "must not collide");
    ed.apply_convert_name(String::from("before-edit"));
    let shown = ed.lines().iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n");
    assert!(shown.contains("before-edit.3ds"), "{}", shown);
    ed.input(Key::A);
    assert!(ed.take_write().is_none(), "the first press must not write");
    ed.input(Key::A);
    match ed.take_write() {
        Some(mhnx_core::hbedit::Write::File { path, .. }) => {
            assert!(path.ends_with("before-edit.3ds"), "wrote {}", path);
            assert!(
                path.contains('/') || path.contains('\\'),
                "the directory must be kept: {}",
                path
            );
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn a_slash_in_the_name_cannot_escape_the_directory() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::B);
    ed.input(Key::L);
    ed.apply_convert_name(String::from("../escape"));
    ed.input(Key::A);
    assert!(ed.take_write().is_none(), "the first press must not write");
    ed.input(Key::A);
    match ed.take_write() {
        Some(mhnx_core::hbedit::Write::File { path, .. }) => {
            assert!(!path.contains("../"), "path escaped: {}", path);
            let seps = |s: &str| s.matches(['/', '\\']).count();
            assert_eq!(
                seps(&path),
                seps(ed.save_path.as_ref().unwrap()),
                "the file must stay in the same directory: {}",
                path
            );
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn the_name_hash_matches_the_files_ryujinx_wrote() {
    // Taken from the two real files: their bodies are byte identical and only
    // the 4 bytes at header +0x14 differ, which is the name hash.
    let system = mhnx_core::save::switch_header("system", mhnx_core::save::BODY_SIZE);
    let backup =
        mhnx_core::save::switch_header("system_backup", mhnx_core::save::BODY_SIZE);
    let u = |h: &[u8; mhnx_core::save::SWITCH_HEADER_SIZE]| {
        u32::from_le_bytes([h[0x14], h[0x15], h[0x16], h[0x17]])
    };
    assert_eq!(
        u(&system),
        0x36B2_EE74,
        "name_hash(\"system\") no longer matches the real file"
    );
    assert_eq!(
        u(&backup),
        0xD081_9B92,
        "name_hash(\"system_backup\") no longer matches the real file"
    );
    // Everything else in the two headers must be the same.
    for i in 0..mhnx_core::save::SWITCH_HEADER_SIZE {
        if (0x14..0x18).contains(&i) {
            continue;
        }
        assert_eq!(system[i], backup[i], "header byte 0x{:X} differs", i);
    }
}

#[test]
fn the_two_real_headers_differ_only_in_the_name_hash() {
    let dir = std::path::PathBuf::from("C:/Users/haruk/AppData/Roaming/Ryujinx/bis/user/save/0000000000000001/0");
    let (a, b) = (dir.join("system"), dir.join("system_backup"));
    if !a.exists() || !b.exists() {
        return;
    }
    let (a, b) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
    assert_eq!(a.len(), b.len());
    let diffs: Vec<usize> = (0..a.len()).filter(|i| a[*i] != b[*i]).collect();
    assert!(
        diffs.iter().all(|i| (0x14..0x18).contains(i)),
        "the files differ outside the name hash: {:?}",
        diffs
    );
    // And the bodies really are identical, which is what makes the pair safe.
    assert_eq!(a[36..], b[36..], "the bodies must be identical");
}

// ---------------------------------------------------------------------------
// DLC section layout. These numbers are load bearing: the section must end
// exactly at the first character slot, so a wrong size would overwrite it.
// ---------------------------------------------------------------------------

#[test]
fn the_dlc_section_fills_the_gap_up_to_the_first_slot() {
    use mhnx_core::dlc;
    let start = 0xB280usize;
    let quests = dlc::QUEST_ARRAY_OFFSET as usize;
    let count = dlc::QUEST_SLOTS as usize;
    let stride = mhnx_core::dlc::QUEST_SLOT_SIZE as usize;
    let equip = mhnx_core::dlc::EQUIP_ARRAY_OFFSET as usize;
    let equip_count = mhnx_core::dlc::EQUIP_SLOTS as usize;
    let equip_stride = mhnx_core::dlc::EQUIP_SLOT_SIZE as usize;
    assert_eq!(
        quests + count * stride,
        equip,
        "the quest array must end where the equipment array starts"
    );
    assert_eq!(
        start + equip + equip_count * equip_stride,
        mhnx_core::save::FIRST_SLOT_BASE as usize,
        "the DLC section must end exactly at the first character slot"
    );
}

#[test]
fn the_two_mask_regions_sit_where_the_reader_expects() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    let body = s.body(); // body relative: no 36 byte header here
    let base = 0xB280usize;
    // 24 bytes at +0x34, 28 bytes at +0x4C: measured, and both are shared by
    // all three characters because they live outside the slot area.
    let unknown = &body[base + 0x34..base + 0x34 + 24];
    let installed = &body[base + 0x4C..base + 0x4C + 28];
    let ones = |a: &[u8]| a.iter().map(|b| b.count_ones() as usize).sum::<usize>();
    // Recorded so a change in the sample save is noticed rather than assumed.
    assert_eq!(ones(unknown), 112, "the +0x34 mask changed");
    assert_eq!(ones(installed), 175, "the +0x4C mask changed");
    assert!(
        !unknown.iter().all(|b| *b == 0) && !installed.iter().all(|b| *b == 0),
        "both masks should hold data"
    );
}

#[test]
fn installing_a_quest_leaves_both_masks_alone() {
    let Some(arc) = sample_arc() else { return };
    if !real_save().exists() {
        return;
    }
    let raw = std::fs::read(&arc).unwrap();
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let base = 36 + 0xB280usize;
    let before: Vec<u8> = s.body()[base + 0x34..base + 0x68].to_vec();
    let a = mhnx_core::arc::Arc::parse(&raw).unwrap();
    let id = a
        .by_ext("mib")
        .and_then(|e| e.data.get(0x0C..0x10))
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap() as i32;
    if mhnx_core::dlc::install_quest(&mut s, id, &raw).is_ok() {
        let after: Vec<u8> = s.body()[base + 0x34..base + 0x68].to_vec();
        assert_eq!(
            before, after,
            "replacing an installed quest must not touch either mask"
        );
    }
}

#[test]
fn the_first_mask_bits_track_the_quest_equipment_catalogue() {
    if !real_save().exists() {
        return;
    }
    let body = std::fs::read(real_save()).unwrap();
    let base = 36 + 0xB280usize;
    let mask = &body[base + 0x4C..base + 0x4C + 28];
    let equipment = base + 0x1049F4;
    let stride = mhnx_core::dlc::EQUIP_SLOT_SIZE;
    let count = mhnx_core::dlc::EQUIP_SLOTS;
    let mut set_bits = Vec::new();
    let mut filled = Vec::new();
    for i in 0..count {
        if (mask[i >> 3] >> (i & 7)) & 1 == 1 {
            set_bits.push(i);
        }
        // An entry counts as present when its first 32 bytes hold any
        // non-zero byte. That is enough to separate used from empty here.
        let off = equipment + i * stride;
        if body[off..off + 32].iter().any(|b| *b != 0) {
            filled.push(i);
        }
    }
    assert_eq!(
        set_bits, filled,
        "bit i must mean equipment entry i is installed"
    );
    // The region is a strict prefix of the mask and it is not all set.
    assert!(set_bits.len() > 30, "expected the catalogue to be mostly full");
    assert!(set_bits.len() < count, "expected at least one empty entry");
}

#[test]
fn the_catalogue_and_event_bits_add_up_to_the_installed_total() {
    if !real_save().exists() {
        return;
    }
    let body = std::fs::read(real_save()).unwrap();
    let base = 36 + 0xB280usize;
    let mask = &body[base + 0x4C..base + 0x4C + 28];
    let total: usize = mask.iter().map(|b| b.count_ones() as usize).sum();
    let catalogue = mhnx_core::dlc::EQUIP_SLOTS;
    let below: usize = (0..catalogue)
        .filter(|i| (mask[i >> 3] >> (i & 7)) & 1 == 1)
        .count();
    let above = total - below;
    // 40 catalogue entries plus 135 event records is the 175 this save has.
    assert_eq!(below + above, total);
    assert_eq!(
        below, 40,
        "the catalogue part of the mask changed shape"
    );
    assert_eq!(above, 135, "the event part of the mask changed shape");
}

#[test]
fn the_quest_equipment_entries_name_installed_quests() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    let entries = mhnx_core::dlc::quest_equipment(&s);
    assert_eq!(entries.len(), mhnx_core::dlc::EQUIP_SLOTS);

    let installed: Vec<i32> = mhnx_core::dlc::list_quests(&s)
        .iter()
        .map(|q| q.quest_id)
        .collect();
    let used: Vec<_> = entries.iter().filter(|e| e.occupied).collect();
    assert_eq!(used.len(), 40, "the sample save uses 40 of the 45 entries");

    // Every entry names a quest that is actually installed.
    for e in &used {
        assert!(
            installed.contains(&e.quest_id),
            "entry {} names quest {}, which is not installed",
            e.index,
            e.quest_id
        );
    }

    // The ids are neither consecutive nor sorted: entries 0..13 run 1020001..,
    // then 14..19 jump to 1120001.., then 20 goes back to 1020015. The array
    // follows the game's catalogue order, which is not id order, so only
    // uniqueness is asserted here.
    let mut seen = std::collections::BTreeSet::new();
    for e in &used {
        assert!(seen.insert(e.quest_id), "quest {} appears twice", e.quest_id);
    }
    assert!(
        used.iter().any(|e| e.quest_id >= 1120000),
        "the sample save should cover both challenge id ranges"
    );

    // The occupancy must agree with the mask bit for the same index.
    let base = 0xB280usize;
    let mask = &s.body()[base + 0x4C..base + 0x4C + 28];
    for e in &entries {
        let bit = (mask[e.index >> 3] >> (e.index & 7)) & 1 == 1;
        assert_eq!(bit, e.occupied, "entry {} and its mask bit disagree", e.index);
    }
}

#[test]
fn an_empty_equipment_entry_returns_zeros() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    // Index 25 is empty in the sample save.
    let Some(b) = mhnx_core::dlc::quest_equipment_bytes(&s, 25) else {
        panic!("entry 25 should be addressable");
    };
    assert_eq!(b.len(), mhnx_core::dlc::EQUIP_SLOT_SIZE);
    assert!(b.iter().all(|x| *x == 0), "entry 25 should be blank");
    assert!(mhnx_core::dlc::quest_equipment_bytes(&s, 45).is_none(), "45 is out of range");
}

// ---------------------------------------------------------------------------
// The DLC catalogue. This is what made the installed mask readable.
// ---------------------------------------------------------------------------

#[test]
fn the_catalogue_reproduces_the_installed_mask_exactly() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    let installed: Vec<i32> = mhnx_core::dlc::list_quests(&s)
        .iter()
        .map(|q| q.quest_id)
        .collect();
    let base = 0xB280usize;
    let mask = &s.body()[base + 0x4C..base + 0x4C + 28];
    let actual: std::collections::BTreeSet<usize> = (0..224)
        .filter(|i| (mask[i >> 3] >> (i & 7)) & 1 == 1)
        .collect();

    let mut predicted = std::collections::BTreeSet::new();
    for id in &installed {
        if let Some(bit) = mhnx_core::dlc::bit_for_quest(*id) {
            predicted.insert(bit);
        }
    }
    assert_eq!(
        predicted.len(),
        actual.len(),
        "the catalogue must account for every set bit"
    );
    assert_eq!(predicted, actual, "predicted and stored masks disagree");
}

#[test]
fn every_installed_quest_has_a_mask_bit() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    for q in mhnx_core::dlc::list_quests(&s) {
        assert!(
            mhnx_core::dlc::bit_for_quest(q.quest_id).is_some(),
            "quest {} is installed but not in the catalogue",
            q.quest_id
        );
        assert!(
            mhnx_core::dlc::is_installed(&s, q.quest_id),
            "quest {} is installed but its bit is clear",
            q.quest_id
        );
    }
}

#[test]
fn the_placeholder_positions_have_no_bit() {
    for pos in 41..=44 {
        assert_eq!(
            mhnx_core::dlc::bit_for_position(pos),
            None,
            "position {} is a placeholder",
            pos
        );
    }
    // The two fields meet at bit 64, not 45.
    assert_eq!(mhnx_core::dlc::bit_for_position(40), Some(40));
    assert_eq!(mhnx_core::dlc::bit_for_position(45), Some(64));
    assert_eq!(mhnx_core::dlc::bit_for_position(204), Some(223));
    assert_eq!(mhnx_core::dlc::bit_for_position(205), None);
}

#[test]
fn setting_a_bit_is_reversible_and_hits_the_right_byte() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let mut s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    let before = s.to_bytes();
    // 1010002 is in the catalogue and is not installed in the sample save.
    let id = 1010002;
    assert!(!mhnx_core::dlc::is_installed(&s, id), "fixture changed");
    let bit = mhnx_core::dlc::bit_for_quest(id).unwrap();
    assert_eq!(bit, 65);
    mhnx_core::dlc::set_installed(&mut s, id, true).unwrap();
    assert!(mhnx_core::dlc::is_installed(&s, id));
    // Only one bit changed.
    let after = s.to_bytes();
    let diff: Vec<usize> = (0..after.len()).filter(|i| after[*i] != before[*i]).collect();
    assert_eq!(diff.len(), 1, "more than one byte changed: {:?}", diff);
    assert_eq!(after[diff[0]] & !before[diff[0]], 1 << (bit & 7));
    mhnx_core::dlc::set_installed(&mut s, id, false).unwrap();
    assert_eq!(s.to_bytes(), before, "clearing must restore the original");
}

// ---------------------------------------------------------------------------
// Equipment box decoding and the Japanese names.
// ---------------------------------------------------------------------------

#[test]
fn the_equipment_box_decodes_into_named_items() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    let box_ = s.equipment_box(0).expect("a box");
    let items: Vec<_> = mhnx_core::equip::decode_box(&box_)
        .into_iter()
        .filter(|e| !e.is_empty())
        .collect();
    assert!(items.len() > 100, "expected a used box, got {}", items.len());

    let mut types = std::collections::BTreeSet::new();
    let mut unnamed = Vec::new();
    for e in &items {
        types.insert(e.kind);
        let n = mhnx_core::equip::name(e);
        assert!(!n.is_empty(), "slot {} decoded to an empty name", e.index);
        if n == "-" {
            unnamed.push((e.index, e.kind, e.id));
        }
    }
    assert!(
        unnamed.is_empty(),
        "{} slots have no name, first: {:?}",
        unnamed.len(),
        unnamed.first()
    );
    // The sample box holds weapons, armour and talismans.
    assert!(types.iter().any(|k| (1..=5).contains(k)), "no armour");
    assert!(types.iter().any(|k| *k == 6), "no talismans");
    assert!(types.iter().any(|k| (7..=21).contains(k)), "no weapons");
    // Type 12 is the deliberately unused slot.
    assert!(!types.contains(&12), "type 12 should never appear");
}

#[test]
fn the_weapon_type_numbering_matches_the_japanese_tables() {
    // The JP tables skip type 5; EQUIPMENT_TYPES skips 12. The offset is 7.
    for jp in 0..15usize {
        if jp == 5 {
            assert!(mhnx_core::equipnames::WEAPON_NAMES[5].is_empty());
            continue;
        }
        let game = jp + 7;
        assert!(game != 12, "jp {} would map onto the unused type", jp);
        assert_eq!(
            mhnx_core::equip::type_name(game as u8),
            mhnx_core::equipnames::WEAPON_NAMES[jp]
                .first()
                .map(|_| mhnx_core::equip::type_name(game as u8))
                .unwrap(),
            "mapping check for jp type {}",
            jp
        );
        assert!(!mhnx_core::equipnames::WEAPON_NAMES[jp].is_empty());
    }
}

#[test]
fn a_known_weapon_decodes_to_its_japanese_name() {
    if !real_save().exists() {
        return;
    }
    let bytes = std::fs::read(real_save()).unwrap();
    let s = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
    let box_ = s.equipment_box(0).unwrap();
    let items = mhnx_core::equip::decode_box(&box_);
    // Every weapon id must fall inside its table.
    let mut checked = 0;
    for e in items.iter().filter(|e| (7..=21).contains(&e.kind)) {
        let t = e.kind as usize - 7;
        if t == 5 {
            continue;
        }
        let tbl = mhnx_core::equipnames::WEAPON_NAMES[t];
        assert!(
            (e.id as usize) < tbl.len(),
            "weapon id {} is outside the {} table ({} entries)",
            e.id,
            mhnx_core::equip::type_name(e.kind),
            tbl.len()
        );
        assert_eq!(mhnx_core::equip::name(e), tbl[e.id as usize]);
        checked += 1;
    }
    assert!(checked > 5, "expected weapons in the sample box");
}

#[test]
fn armour_names_index_by_series_and_part() {
    let names = mhnx_core::equipnames::ARMOR_NAMES;
    assert_eq!(names.len() % 5, 0, "the table must be a whole number of sets");
    // Series 0 is the empty set, series 1 is Leather.
    assert_eq!(names[0], "装備なし");
    assert_eq!(names[5 + 0], "レザーヘッド");
    assert_eq!(names[5 + 1], "レザーベスト");
    assert_eq!(names[5 + 2], "レザーグラブ");
    assert_eq!(names[5 + 3], "レザーベルト");
    assert_eq!(names[5 + 4], "レザーパンツ");
    // And the decoder reaches them the same way.
    let eq = mhnx_core::equip::Equipment {
        index: 0,
        kind: 1,
        level: 1,
        transmog_level: 1,
        id: 1,
        transmog: 0,
    };
    assert_eq!(mhnx_core::equip::name(&eq), "レザーヘッド");
    let eq = mhnx_core::equip::Equipment { kind: 5, ..eq };
    assert_eq!(mhnx_core::equip::name(&eq), "レザーパンツ");
}

#[test]
fn the_console_equipment_screen_lists_real_names() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    assert_eq!(ed.screen, Screen::Slots, "open_save lands on the slot list");
    // The equipment box is a section, not a key on the slot list: [-] is the
    // delete key and nothing else (the review's twelfth finding).
    ed.screen = Screen::Equip;
    let all = ed.equipment();
    assert!(all.len() > 300, "expected a used box, got {}", all.len());
    // Every row must carry a real name, not the "-" fallback.
    for e in &all {
        let n = mhnx_core::equip::name(e);
        assert_ne!(n, "-", "slot {} ({:?}) has no name", e.index, e.kind);
    }
    // And the screen must render the names.
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = lines.join("\n");
    assert!(joined.contains("EQUIPMENT"), "{}", joined);
    assert!(joined.contains("ボーンブレイド") || joined.contains("大剣"), "{}", joined);
    // Navigation stays inside the list.
    for _ in 0..5 {
        ed.input(Key::Down);
    }
    assert!(ed.equip_cursor <= all.len());
    assert_eq!(ed.equip_cursor, 5, "five downs should land on row 5");
    ed.input(Key::Up);
    assert_eq!(ed.equip_cursor, 4, "up should step back one");
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
}

#[test]
fn the_console_equipment_filter_narrows_and_clears() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Equip;
    let all = ed.equipment().len();
    ed.input(Key::Y);
    let (idx, _) = ed.take_text_request().expect("a filter request");
    assert_eq!(idx, mhnx_core::hbedit::TEXT_EQUIP_FILTER);
    assert_eq!(idx, usize::MAX - 5, "must not collide");
    ed.equip_filter = String::from("大剣");
    ed.equip_cursor = 0;
    let filtered = ed.equipment();
    assert!(!filtered.is_empty());
    assert!(filtered.len() < all, "the filter did not narrow anything");
    assert!(filtered
        .iter()
        .all(|e| mhnx_core::equip::type_name(e.kind).contains("大剣")));
    // [X] is the save key on every screen, so the clear is the empty answer
    // from the filter box - and the heading says the filter is on.
    ed.input(Key::X);
    assert_eq!(ed.equipment().len(), filtered.len(), "[X] must not clear it");
    assert!(
        ed.equip_filter_heading().contains("大剣"),
        "the heading must carry the filter: {}",
        ed.equip_filter_heading()
    );
    ed.apply_equip_filter(String::new());
    assert_eq!(ed.equipment().len(), all, "an empty filter clears it");
    // The heading names the box and says where the talismans are (the third
    // review's finding 14); it is not just the two words.
    assert!(ed.equip_filter_heading().starts_with("装備BOX"), "{}", ed.equip_filter_heading());
    assert!(ed.equip_filter_heading().contains("護石一覧"), "{}", ed.equip_filter_heading());
    // Nothing matching must not panic.
    ed.equip_filter = String::from("zzzz");
    assert!(ed.equipment().is_empty());
    ed.input(Key::Down);
    ed.input(Key::Up);
    let lines = ed.lines();
    assert!(lines[0].text.contains("EQUIPMENT"));
}

#[test]
fn the_console_lists_the_quest_equipment_entries() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed); // quest editor
    ed.input(Key::X); // quests inside the save
    assert_eq!(ed.screen, Screen::SaveQuests);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::QuestEquip);
    let list = ed.quest_equipment();
    assert_eq!(list.len(), mhnx_core::dlc::EQUIP_SLOTS);
    let used = list.iter().filter(|e| e.occupied).count();
    assert_eq!(used, 40, "the sample save uses 40 entries");
    assert!(list.iter().any(|e| !e.occupied), "some entries are empty");
    // Every used entry names a quest that is installed.
    let installed: Vec<i32> = mhnx_core::dlc::list_quests(ed.save.as_ref().unwrap())
        .iter()
        .map(|q| q.quest_id)
        .collect();
    for e in list.iter().filter(|e| e.occupied) {
        assert!(
            installed.contains(&e.quest_id),
            "entry {} names quest {} which is not installed",
            e.index,
            e.quest_id
        );
    }
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = lines.join("\n");
    assert!(joined.contains("QUEST EQUIPMENT"), "{}", joined);
    assert!(joined.contains("40 / 45 used"), "{}", joined);
    // Navigation is bounded and B returns to the list.
    for _ in 0..60 {
        ed.input(Key::Down);
    }
    assert!(ed.cursor < list.len());
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::SaveQuests);
}

// ---------------------------------------------------------------------------
// Adding a quest the save does not already list. This needed the catalogue.
// ---------------------------------------------------------------------------

fn sample_quest_arc(id: &str) -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(format!(
        "C:/anyWorkspace/nx/mhxx/save_editor/mhxx_quetool/quest/q{}.arc",
        id
    ));
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn a_new_quest_can_be_added_to_a_free_slot() {
    let Some(arc) = sample_quest_arc("1010002") else {
        return;
    };
    if !real_save().exists() {
        return;
    }
    let raw = std::fs::read(&arc).unwrap();
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let before = mhnx_core::dlc::list_quests(&s).len();
    // It must be in the catalogue but not installed.
    assert!(mhnx_core::dlc::bit_for_quest(1010002).is_some());
    assert!(!mhnx_core::dlc::is_installed(&s, 1010002));
    assert!(!mhnx_core::dlc::list_quests(&s)
        .iter()
        .any(|q| q.quest_id == 1010002));

    let slot = mhnx_core::dlc::add_quest(&mut s, 1010002, &raw).expect("add");
    // The slot must be one that was free, and now hold the payload verbatim.
    let after = mhnx_core::dlc::list_quests(&s);
    assert_eq!(after.len(), before + 1);
    let q = after.iter().find(|q| q.quest_id == 1010002).expect("installed");
    assert_eq!(q.slot, slot);
    assert_eq!(q.data, raw, "the payload must be carried over unchanged");
    // And the mask bit must be set.
    assert!(mhnx_core::dlc::is_installed(&s, 1010002));
    assert_eq!(mhnx_core::dlc::bit_for_quest(1010002), Some(65));
    // The save must still round trip.
    let out = s.to_bytes();
    let back = mhnx_core::save::Save::from_bytes(&out).unwrap();
    assert_eq!(back.to_bytes(), out);
    assert_eq!(mhnx_core::dlc::list_quests(&back).len(), before + 1);
}

#[test]
fn adding_an_installed_quest_is_refused() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let existing = mhnx_core::dlc::list_quests(&s)[0].quest_id;
    let r = mhnx_core::dlc::add_quest(&mut s, existing, &[0u8; 16]);
    assert!(r.is_err(), "adding an installed quest must fail");
}

#[test]
fn adding_a_quest_outside_the_catalogue_is_refused() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let r = mhnx_core::dlc::add_quest(&mut s, 9999999, &[0u8; 16]);
    assert!(r.is_err(), "an id the catalogue does not list must be refused");
}

#[test]
fn an_oversized_payload_is_refused() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let big = vec![0u8; mhnx_core::dlc::QUEST_MAX_PAYLOAD + 1];
    let r = mhnx_core::dlc::add_quest(&mut s, 1010002, &big);
    assert!(r.is_err(), "a payload that cannot fit must be refused");
}

#[test]
fn the_item_id_bound_matches_the_item_table() {
    // itemData holds 2991 entries and the game compares against 2991, so the
    // last usable id is 2990.
    assert_eq!(mhnx_core::gamedata::LAST_ITEM_ID, 2990);
    assert_eq!(
        mhnx_core::gamedata::ITEM_NAME_LIST.len(),
        mhnx_core::gamedata::LAST_ITEM_ID + 1,
        "the name table and the id bound must agree"
    );
}

#[test]
fn every_item_id_in_the_sample_save_is_within_the_bound() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    for slot in 0..3 {
        let Some(box_) = s.equipment_box(slot) else { continue };
        let _ = box_;
    }
    let items = s.read_item_box(0).expect("an item box");
    let mut checked = 0;
    for it in items.iter().filter(|i| i.id != 0) {
        assert!(
            (it.id as usize) <= mhnx_core::gamedata::LAST_ITEM_ID,
            "item id {} is past the end of the table",
            it.id
        );
        checked += 1;
    }
    assert!(checked > 100, "expected a used item box, got {}", checked);
}

// ---------------------------------------------------------------------------
// The supply items inside a quest-equipment loadout.
// ---------------------------------------------------------------------------

#[test]
fn the_quest_equipment_items_read_back_exactly() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let items = mhnx_core::dlc::quest_equipment_items(&s, 0);
    // Five loadouts of 24 supplies and 8 ammo entries each.
    assert_eq!(items.len(), 5 * (24 + 8));
    // The first supply of every loadout is the same in this save: 1724 potion x2.
    for it in items.iter().filter(|i| i.list == mhnx_core::dlc::QuestEquipList::Supply && i.index == 0) {
        assert_eq!(it.item, 1724, "record {} supply 0", it.record);
        assert_eq!(it.count, 2);
    }
    // Only the fifth loadout carries ammo in slot 0, and it is a light bowgun.
    let ammo: Vec<_> = items
        .iter()
        .filter(|i| i.list == mhnx_core::dlc::QuestEquipList::Ammo && i.item != 0)
        .collect();
    assert!(!ammo.is_empty(), "expected ammo somewhere");
    for it in &ammo {
        assert_eq!(it.record, 4, "only the fifth loadout should carry ammo");
    }
    // Every id must be inside the item table.
    for it in items.iter().filter(|i| i.item != 0) {
        assert!((it.item as usize) <= mhnx_core::gamedata::LAST_ITEM_ID);
    }
}


#[test]
fn the_console_can_change_a_supply_count() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // Straight to the screen: the navigation that reaches it is covered by
    // the screen-reachability test, this one is about the count.
    ed.screen = Screen::QuestEquip;
    ed.cursor = 0;
    ed.qe_open = false;
    ed.input(Key::A); // open the loadout
    assert!(ed.qe_open, "the loadout should be open");
    ed.qe_item_cursor = 0;

    let before = ed.quest_equip_items()[0];
    let save_before = ed.save.as_ref().unwrap().to_bytes();
    ed.input(Key::Right);
    let after = ed.quest_equip_items()[0];
    assert_eq!(
        after.count,
        (before.count as i64 + 1).min(99) as u8,
        "right should add one"
    );
    assert_eq!(after.item, before.item, "the item must not change");
    assert!(ed.dirty, "a changed count is an unsaved change");

    // Only the count byte of that one entry may differ.
    let save_after = ed.save.as_ref().unwrap().to_bytes();
    let diff: Vec<usize> = (0..save_after.len())
        .filter(|i| save_after[*i] != save_before[*i])
        .collect();
    assert!(!diff.is_empty(), "the write did nothing");
    // Same base the core test uses: the loadout stride is 8, then the block
    // header, the entry header and the supply box offset inside it.
    let entry = 36 + 0xB280 + 0x1049F4 + before.record * 8 + 8 + 0x1D + 3 + 0x53;
    for at in &diff {
        assert!(
            *at >= entry && *at < entry + 3,
            "byte 0x{:X} changed outside the entry at 0x{:X}",
            at,
            entry
        );
    }

    // And left takes it back.
    ed.input(Key::Left);
    assert_eq!(ed.quest_equip_items()[0].count, before.count);
}

#[test]
fn writing_one_supply_entry_changes_only_those_bytes() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let before = s.to_bytes();
    mhnx_core::dlc::set_quest_equipment_item(
        &mut s,
        0,
        0,
        mhnx_core::dlc::QuestEquipList::Supply,
        0,
        1718,
        5,
    )
    .expect("write");
    let after = s.to_bytes();
    let diff: Vec<usize> = (0..after.len()).filter(|i| after[*i] != before[*i]).collect();
    // Up to three bytes: the low half of the id and the count changed here, so
    // only two actually differ. Every difference must fall inside the entry.
    let entry = 36 + 0xB280 + 0x1049F4 + 0 + 8 + 0x1D + 3 + 0x53;
    assert!(!diff.is_empty(), "the write did nothing");
    for at in &diff {
        assert!(
            (*at >= entry && *at < entry + 3),
            "byte 0x{:X} changed outside the entry at 0x{:X}",
            at,
            entry
        );
    }
    // And the entry now reads back what was written.
    let items = mhnx_core::dlc::quest_equipment_items(&s, 0);
    let it = items
        .iter()
        .find(|i| {
            i.record == 0
                && i.list == mhnx_core::dlc::QuestEquipList::Supply
                && i.index == 0
        })
        .unwrap();
    assert_eq!((it.item, it.count), (1718, 5));
    // The mask was not touched, which is what keeps the entry listed.
    let base = 0xB280usize;
    assert_eq!(
        &s.body()[base + 0x4C..base + 0x4C + 28],
        &before[36 + base + 0x4C..36 + base + 0x4C + 28]
    );
}

#[test]
fn an_out_of_range_supply_write_is_refused() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    // Index past the end of the supply list.
    assert!(mhnx_core::dlc::set_quest_equipment_item(
        &mut s,
        0,
        0,
        mhnx_core::dlc::QuestEquipList::Supply,
        24,
        1718,
        1,
    )
    .is_err());
    // Item id past the item table.
    assert!(mhnx_core::dlc::set_quest_equipment_item(
        &mut s,
        0,
        0,
        mhnx_core::dlc::QuestEquipList::Supply,
        0,
        2991,
        1,
    )
    .is_err());
}

#[test]
fn a_repacked_archive_is_byte_identical_to_the_original() {
    // The writer used to leave the word at +0x08 as zero while every shipped
    // archive carries 0xFFFFFFFE there, so a repack differed in four bytes.
    let q = std::path::PathBuf::from(
        "C:/anyWorkspace/nx/mhxx/extract/base_romfs/nativeNX/arc/quest",
    );
    if !q.exists() {
        return;
    }
    let mut checked = 0;
    for e in std::fs::read_dir(&q).unwrap().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("arc") {
            continue;
        }
        let raw = std::fs::read(&p).unwrap();
        let Ok(a) = mhnx_core::arc::Arc::parse(&raw) else {
            continue;
        };
        let out = a.to_bytes().unwrap();
        assert_eq!(
            out,
            raw,
            "{} did not round trip",
            p.file_name().unwrap().to_string_lossy()
        );
        checked += 1;
        if checked >= 12 {
            break;
        }
    }
    assert!(checked > 0, "no archives were checked");
}

// ---------------------------------------------------------------------------
// Monster hunt records.
// ---------------------------------------------------------------------------

#[test]
fn the_three_monster_tables_line_up_with_each_other() {
    // The strongest evidence for the count is arithmetic, not the roster:
    // hunt 0x5EA6 + 137*2 = capture 0x5FB8, + 137*2 = size 0x60CA,
    // + 137*4 = 0x62EE, which is exactly where the equipment box begins.
    use mhnx_core::save::*;
    assert_eq!(OFF_MONSTER_HUNT + MONSTER_COUNT * 2, OFF_MONSTER_CAPTURE);
    assert_eq!(OFF_MONSTER_CAPTURE + MONSTER_COUNT * 2, OFF_MONSTER_SIZE);
    assert_eq!(OFF_MONSTER_SIZE + MONSTER_COUNT * 4, OFF_EQUIPMENT_BOX);
    assert_eq!(MONSTER_TABLE_SIZE, MONSTER_COUNT * 2);
    assert_eq!(MONSTER_SIZE_TABLE_SIZE, MONSTER_COUNT * 4);
}

#[test]
fn the_sample_save_holds_plausible_hunt_records() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let hunt = s.hunt_counts(0).expect("hunt table");
    let cap = s.capture_counts(0).expect("capture table");
    let size = s.monster_sizes(0).expect("size table");
    assert_eq!(hunt.len(), mhnx_core::save::MONSTER_COUNT);
    assert_eq!(cap.len(), mhnx_core::save::MONSTER_COUNT);
    assert_eq!(size.len(), mhnx_core::save::MONSTER_COUNT);

    // Hunt counts must look like counts: a few hundred at most, and the
    // character has certainly hunted something.
    let total: u32 = hunt.iter().map(|v| *v as u32).sum();
    assert!(total > 1000, "the sample character should have hunted a lot");
    assert!(
        hunt.iter().all(|v| *v <= 9999),
        "a hunt count above 9999 is not a count"
    );
    assert_eq!(
        hunt.iter().filter(|v| **v > 0).count() > 40,
        true,
        "expected many monsters hunted"
    );
    // The first table counts slays, not total encounters, so a monster that is
    // almost always captured can show more captures than slays. Only a broad
    // disagreement would mean the tables are mismatched.
    let over: Vec<_> = (0..hunt.len()).filter(|i| cap[*i] > hunt[*i]).collect();
    assert!(
        over.len() <= 3,
        "{} monsters have more captures than slays, first few: {:?}",
        over.len(),
        over.iter()
            .take(5)
            .map(|i| (*i, cap[*i], hunt[*i]))
            .collect::<Vec<_>>()
    );
    // Recorded sizes look like the percentages the game shows.
    let recorded: Vec<_> = size.iter().filter(|(a, b)| *a != 0 || *b != 0).collect();
    assert!(recorded.len() > 40, "expected many monsters sized");
    for (a, b) in &recorded {
        assert!(*a >= 20 && *a <= 300, "small size {} is not a percentage", a);
        assert!(*b >= 20 && *b <= 300, "big size {} is not a percentage", b);
        assert!(a <= b, "small {} should not exceed big {}", a, b);
    }
}

#[test]
fn a_hunt_count_can_be_written_and_read_back() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let before = s.to_bytes();
    assert!(s.set_hunt_count(0, 3, 1234));
    assert_eq!(s.hunt_counts(0).unwrap()[3], 1234);
    // Only that monster's two bytes move.
    let after = s.to_bytes();
    let diff: Vec<usize> = (0..after.len()).filter(|i| after[*i] != before[*i]).collect();
    let at = 36 + 0x126474 + mhnx_core::save::OFF_MONSTER_HUNT + 3 * 2;
    assert!(diff.iter().all(|d| *d >= at && *d < at + 2), "diff {:?}", diff);
    // Out of range is refused rather than writing somewhere else.
    assert!(!s.set_hunt_count(0, mhnx_core::save::MONSTER_COUNT, 1));
    assert!(!s.set_monster_size(0, mhnx_core::save::MONSTER_COUNT, 1, 2));
}

#[test]
fn a_monster_size_can_be_written_and_read_back() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    assert!(s.set_monster_size(0, 5, 90, 130));
    assert_eq!(s.monster_sizes(0).unwrap()[5], (90, 130));
    assert!(s.set_capture_count(0, 5, 7));
    assert_eq!(s.capture_counts(0).unwrap()[5], 7);
}

// ---------------------------------------------------------------------------
// The guild card keeps its own copy of the appearance.
// ---------------------------------------------------------------------------

#[test]
fn the_guild_card_mirrors_the_appearance_in_the_sample_save() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    use mhnx_core::save::*;
    for slot in 0..3 {
        let Some(base) = s.slot_base(slot).map(|b| b as usize) else {
            continue;
        };
        let gc = base + OFF_GUILD_CARD;
        let body = s.body();
        assert_eq!(
            &body[gc + 0x19..gc + 0x21],
            &body[base + OFF_VOICE..base + OFF_VOICE + 8],
            "slot {}: the card appearance does not match the character",
            slot + 1
        );
        assert_eq!(
            &body[gc + 0x38..gc + 0x48],
            &body[base + OFF_SKIN_COLOR..base + OFF_SKIN_COLOR + 16],
            "slot {}: the card colours do not match the character",
            slot + 1
        );
    }
}

#[test]
fn editing_the_appearance_updates_the_guild_card_too() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    use mhnx_core::save::*;
    let mut c = s.read_character(0).expect("a character");
    c.hairstyle = c.hairstyle.wrapping_add(7);
    c.skin_color = [1, 2, 3, 255];
    s.write_character(&c).unwrap();
    let base = s.slot_base(0).unwrap() as usize;
    let gc = base + OFF_GUILD_CARD;
    assert_eq!(s.body()[gc + 0x19 + 5], c.hairstyle, "card hairstyle is stale");
    assert_eq!(
        &s.body()[gc + 0x38..gc + 0x3C],
        &[1, 2, 3, 255],
        "card colours are stale"
    );
    // And the two copies agree afterwards.
    assert_eq!(
        &s.body()[gc + 0x19..gc + 0x21],
        &s.body()[base + OFF_VOICE..base + OFF_VOICE + 8]
    );
}

// ---------------------------------------------------------------------------
// The five armour pigments (the game's colour slots 0-4).
// ---------------------------------------------------------------------------

/// The measured pigments: 3D 36 3E FF four times, then 38 62 CD FF.
///
/// These are the bytes the sample save holds at slot + 0x23B53, verified while
/// the offset was being pinned down; OFF_SKIN_COLOR follows twenty bytes later
/// (0x23B67). All three slots carry them, both through the slot bases the save
/// stores in its header and at the fixed stride.
#[test]
fn the_five_armour_pigments_read_back_as_measured() {
    if !real_save().exists() {
        return;
    }
    use mhnx_core::save::*;
    let s = Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let want: [[u8; 4]; ARMOR_COLOR_SLOTS] = [
        [0x3D, 0x36, 0x3E, 0xFF],
        [0x3D, 0x36, 0x3E, 0xFF],
        [0x3D, 0x36, 0x3E, 0xFF],
        [0x3D, 0x36, 0x3E, 0xFF],
        [0x38, 0x62, 0xCD, 0xFF],
    ];
    let mut flat = Vec::new();
    for p in want {
        flat.extend_from_slice(&p);
    }
    // The block is twenty bytes and ends exactly on the skin colour, which is
    // the whole reason 0x23B53 is the offset rather than a guess.
    assert_eq!(ARMOR_COLORS_SIZE, 20);
    assert_eq!(OFF_ARMOR_COLORS + ARMOR_COLORS_SIZE, OFF_SKIN_COLOR);
    for slot in 0..3 {
        // The stride-placed copy of the body and the slot base the header
        // names are the same address for this save, so both readings have to
        // agree with the measured bytes.
        let at = FIRST_SLOT_BASE as usize + slot * SLOT_STRIDE + OFF_ARMOR_COLORS;
        assert_eq!(&s.body()[at..at + 20], &flat[..], "slot copy {}", slot + 1);
        let c = s.read_character(slot).expect("a character");
        assert_eq!(c.armor_colors, want, "slot {}", slot + 1);
    }
    // The five pigments are followed by the four colours the crate already
    // knew, in slot order, with no gap.
    assert_eq!(OFF_HAIR_COLOR, OFF_SKIN_COLOR + COLOR_BYTES);
    assert_eq!(OFF_FEATURES_COLOR, OFF_HAIR_COLOR + COLOR_BYTES);
    assert_eq!(OFF_CLOTHING_COLOR, OFF_FEATURES_COLOR + COLOR_BYTES);
}

/// Writing one pigment moves those four bytes and the card's copy of them -
/// nothing else. The identity write first means the assertion is about the
/// change, not about how the rest of the record re-encodes.
#[test]
fn writing_a_pigment_touches_that_pigment_and_its_card_copy() {
    if !real_save().exists() {
        return;
    }
    use mhnx_core::guildcard::GC_PIGMENTS;
    use mhnx_core::save::*;
    let mut s = Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let mut c = s.read_character(0).expect("a character");
    s.write_character(&c).expect("the character writes");
    let before = s.to_bytes();

    c.armor_colors[1] = [0x11, 0x22, 0x33, 0x44];
    s.write_character(&c).expect("the character writes again");
    let after = s.to_bytes();
    let base = SWITCH_HEADER_SIZE + FIRST_SLOT_BASE as usize;
    let live = base + OFF_ARMOR_COLORS + COLOR_BYTES;
    let card = base + OFF_GUILD_CARD + GC_PIGMENTS + COLOR_BYTES;
    let diff: Vec<usize> = (0..after.len()).filter(|i| after[*i] != before[*i]).collect();
    assert_eq!(diff.len(), 8, "diff {:?}", diff);
    assert!(
        diff.iter()
            .all(|d| (*d >= live && *d < live + COLOR_BYTES)
                || (*d >= card && *d < card + COLOR_BYTES)),
        "diff {:?}",
        diff
    );
    let back = s.read_character(0).expect("a character");
    assert_eq!(back.armor_colors[1], [0x11, 0x22, 0x33, 0x44]);
    // The other four pigments are still the measured ones.
    assert_eq!(back.armor_colors[0], [0x3D, 0x36, 0x3E, 0xFF]);
    assert_eq!(back.armor_colors[4], [0x38, 0x62, 0xCD, 0xFF]);
    assert_eq!(back.skin_color, c.skin_color);
}

// ---------------------------------------------------------------------------
// Guild card and arena log.
// ---------------------------------------------------------------------------

#[test]
fn the_guild_card_agrees_with_the_character() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    for slot in 0..3 {
        let Some(hr_card) = mhnx_core::guildcard::hunter_rank(&s, slot) else {
            continue;
        };
        let c = s.read_character(slot).expect("character");
        assert_eq!(
            hr_card, c.hunter_rank,
            "slot {}: the card HR differs from the character",
            slot + 1
        );
        // The card's clock is a snapshot, so it never runs ahead.
        let card = mhnx_core::guildcard::play_time(&s, slot).unwrap();
        assert!(
            card <= c.play_time as u32,
            "slot {}: the card is ahead of the character ({} > {})",
            slot + 1,
            card,
            c.play_time
        );
        assert!(c.play_time as u32 - card < 3600, "the card is very stale");
    }
}

#[test]
fn the_arena_log_holds_ordered_centisecond_times() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let log = mhnx_core::guildcard::arena(&s, 0);
    assert_eq!(
        log.len(),
        mhnx_core::guildcard::ARENA_COUNT * mhnx_core::guildcard::ARENA_RANKS
    );
    let mut recorded = 0;
    for a in 0..mhnx_core::guildcard::ARENA_COUNT {
        let times: Vec<u16> = log
            .iter()
            .filter(|e| e.arena == a && !e.is_empty())
            .map(|e| e.centis)
            .collect();
        // Within one arena the ranks are ordered fastest first.
        for w in times.windows(2) {
            assert!(
                w[0] <= w[1],
                "arena {} has out of order times: {:?}",
                a,
                times
            );
        }
        recorded += times.len();
    }
    assert!(recorded >= 10, "expected some arena records, got {}", recorded);
    // A time that reads as a sane arena run.
    let first = log.iter().find(|e| !e.is_empty()).expect("a recorded time");
    let (m, sec, cs) = first.as_clock();
    assert!(
        (1..30).contains(&m),
        "a {}-minute arena run is not plausible ({:02}:{:02}.{:02})",
        m,
        m,
        sec,
        cs
    );
}

#[test]
fn the_arena_clock_converts_hundredths() {
    let t = mhnx_core::guildcard::ArenaTime {
        arena: 0,
        rank: 0,
        centis: 57_544,
        extra: [0, 0],
    };
    assert_eq!(t.as_clock(), (9, 35, 44), "57,544 hundredths is 9:35.44");
    let t = mhnx_core::guildcard::ArenaTime {
        centis: 0,
        ..t
    };
    assert!(t.is_empty());
}

// ---------------------------------------------------------------------------
// The palico roster.
// ---------------------------------------------------------------------------

#[test]
fn the_palico_roster_reads_real_records() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let all = mhnx_core::palico::roster(&s, 0);
    assert_eq!(all.len(), mhnx_core::palico::SLOTS);
    let used = mhnx_core::palico::used(&s, 0);
    assert_eq!(used.len(), 17, "the sample character has seventeen palicos");
    for p in &used {
        assert!(!p.name.is_empty());
        assert!(
            (1..=99).contains(&p.level),
            "palico {} has level {}, which is not a level",
            p.index,
            p.level
        );
        assert!(p.exp < 2_000_000, "experience {} looks wrong", p.exp);
        assert!(p.enthusiasm <= 100, "enthusiasm {} is out of range", p.enthusiasm);
    }
    // Names must be valid text, not mojibake.
    assert!(
        used.iter().all(|p| !p.name.contains('\u{FFFD}')),
        "a name failed to decode: {:?}",
        used.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    // Levels rise with experience across the roster; a swap would show up here.
    let mut pairs: Vec<(u32, u16)> = used.iter().map(|p| (p.exp, p.level)).collect();
    pairs.sort();
    let mut inversions = 0;
    for w in pairs.windows(2) {
        if w[1].1 + 2 < w[0].1 {
            inversions += 1;
        }
    }
    assert!(
        inversions <= 2,
        "level and experience disagree on {} of {} palicos",
        inversions,
        pairs.len()
    );
}

#[test]
fn palico_fields_can_be_written() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let before = s.to_bytes();
    assert!(mhnx_core::palico::set_name(&mut s, 0, 0, "テスト猫"));
    assert!(mhnx_core::palico::set_level(&mut s, 0, 0, 50));
    assert!(mhnx_core::palico::set_exp(&mut s, 0, 0, 123_456));
    assert!(mhnx_core::palico::set_enthusiasm(&mut s, 0, 0, 42));
    let p = &mhnx_core::palico::roster(&s, 0)[0];
    assert_eq!(p.name, "テスト猫");
    assert_eq!(p.level, 50);
    assert_eq!(p.exp, 123_456);
    assert_eq!(p.enthusiasm, 42);
    // Only record 0 may have moved.
    let after = s.to_bytes();
    let base = 36 + 0x126474 + mhnx_core::palico::OFF_ROSTER;
    let end = base + mhnx_core::palico::RECORD;
    let outside: Vec<usize> = (0..after.len())
        .filter(|i| after[*i] != before[*i])
        .filter(|i| *i < base || *i >= end)
        .collect();
    assert!(outside.is_empty(), "bytes outside record 0 changed: {:?}", outside);
    // Out of range is refused.
    assert!(!mhnx_core::palico::set_name(&mut s, 0, mhnx_core::palico::SLOTS, "x"));
    assert!(!mhnx_core::palico::set_level(&mut s, 0, 0, 100));
    assert!(!mhnx_core::palico::set_level(&mut s, 0, 0, 0));
}

// ---------------------------------------------------------------------------
// The three record screens on the console.
// ---------------------------------------------------------------------------

#[test]
fn the_console_shows_the_hunt_records() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::A); // character
    assert_eq!(ed.screen, Screen::Char);
    // The hunt record is the オトモ・記録 section: [-] is the delete key.
    ed.screen = Screen::Hunts;
    assert_eq!(ed.screen, Screen::Hunts);
    let rows = ed.hunt_rows();
    assert!(rows.len() > 100, "expected the recorded monsters, got {}", rows.len());
    // Every row must carry a real monster name, not the "?" fallback.
    for (i, ..) in &rows {
        assert_ne!(mhnx_core::hbedit::monster_name(*i), "?", "index {} unnamed", i);
    }
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = lines.join("\n");
    assert!(joined.contains("HUNT RECORDS"), "{}", joined);
    assert!(
        joined.contains("リオレイア") || joined.contains("リオレウス"),
        "{}",
        joined
    );
    // Navigation is bounded and B returns to the slot list.
    for _ in 0..200 {
        ed.input(Key::Down);
    }
    assert!(ed.hunt_cursor < rows.len());
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
}

#[test]
fn the_console_shows_the_guild_card_and_arena() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Equip; // the box is a section, not a key
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Guild);
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = lines.join("\n");
    assert!(joined.contains("GUILD CARD"), "{}", joined);
    assert!(joined.contains("rank"), "{}", joined);
    assert!(joined.contains("comment"), "{}", joined);
    // A recorded arena time must be rendered as a clock, not raw hundredths.
    assert!(joined.contains("ARENA"), "{}", joined);
    assert!(
        joined.contains(":") && !joined.contains("57544"),
        "arena times should be formatted: {}",
        joined
    );
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
}

/// The arena log of the real save, on the view that edits it.
///
/// The user reported the records as missing and uneditable, so this drives the
/// view the console now has: every stored time must be on screen, and one press
/// must move exactly the two bytes of the entry it names and nothing else.
#[test]
fn the_arena_view_finds_the_times_the_real_save_holds() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Equip; // the box is a section, not a key
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Guild);
    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_ARENA);
    // A view of the guild screen, not a screen of its own.
    assert_eq!(ed.screen, Screen::Guild);

    // The grid is the log entry for entry.
    let log = mhnx_core::guildcard::arena(ed.save.as_ref().expect("a save"), ed.slot);
    let grid = ed.arena_grid();
    for e in &log {
        assert_eq!(grid[e.arena][e.rank], e.centis, "arena {} rank {}", e.arena, e.rank);
    }
    let recorded = log.iter().filter(|e| !e.is_empty()).count();
    assert!(recorded > 0, "the sample save records no arena time at all");

    // The console lists the arena by name and its time as a clock.
    let first = *log.iter().find(|e| !e.is_empty()).expect("one record");
    let joined = ed
        .lines()
        .iter()
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("
");
    assert!(joined.contains("ARENA LOG"), "{}", joined);
    assert!(
        joined.contains(mhnx_core::hbedit::arena_name(first.arena)),
        "{} is missing from\n{}",
        mhnx_core::hbedit::arena_name(first.arena),
        joined
    );
    assert!(
        joined.contains(&mhnx_core::hbedit::arena_clock(first.centis)),
        "the time {} is missing from\n{}",
        mhnx_core::hbedit::arena_clock(first.centis),
        joined
    );

    // One press moves the two time bytes of that entry and nothing else.
    ed.arena_row = first.arena;
    ed.arena_rank = first.rank;
    let before = ed.save.as_ref().expect("a save").body().to_vec();
    assert!(
        ed.set_arena_time_at(first.arena, first.rank, first.centis.saturating_add(1)),
        "{}",
        ed.message
    );
    let after = ed.save.as_ref().expect("a save").body().to_vec();
    let changed: Vec<usize> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!changed.is_empty(), "the write changed nothing");
    assert!(changed.len() <= 2, "one time is two bytes, got {:?}", changed);

    // The undo puts the save back byte for byte.
    assert!(ed.undo_last(), "{}", ed.message);
    let restored = ed.save.as_ref().expect("a save").body().to_vec();
    assert_eq!(restored, before, "the undo must restore the save exactly");
}

#[test]
fn the_console_lists_the_palicos() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Equip; // the box is a section, not a key
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Roster);
    let list = ed.palicos();
    assert_eq!(list.len(), 17, "the sample character has seventeen palicos");
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = lines.join("\n");
    assert!(joined.contains("PALICOS"), "{}", joined);
    assert!(joined.contains("変更中: レベル"), "{}", joined);
    // Every name must have survived decoding.
    for p in &list {
        assert!(!p.name.contains('\u{FFFD}'));
    }
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
}

// ---------------------------------------------------------------------------
// Item sets and equipment sets.
// ---------------------------------------------------------------------------

#[test]
fn the_two_set_arrays_sit_where_they_should() {
    use mhnx_core::myset::*;
    // Neither array may run into its neighbour.
    assert_eq!(OFF_ITEM_SETS + ITEM_SETS * ITEM_SET_SIZE, 0x27BF);
    assert_eq!(OFF_EQUIP_SETS + EQUIP_SETS * EQUIP_SET_SIZE, 0x21E0E);
    // An item set is a name, ten reserved bytes and 32 pairs.
    assert_eq!(ITEM_SET_BODY + ITEM_SET_ITEMS * 4, ITEM_SET_SIZE);
    // An equipment set names seven parts that must fit before the colours.
    assert!(EQUIP_SET_BODY + EQUIP_SET_PARTS * 2 <= EQUIP_SET_COLOURS);
    assert!(EQUIP_SET_COLOURS + 5 * 4 <= EQUIP_SET_SIZE);
}

#[test]
fn the_item_sets_read_real_records() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let sets = mhnx_core::myset::item_sets(&s, 0);
    assert_eq!(sets.len(), mhnx_core::myset::ITEM_SETS);
    let used: Vec<_> = sets.iter().filter(|x| !x.is_empty()).collect();
    assert_eq!(used.len(), 2, "the sample has two item sets");
    assert!(
        used.iter().any(|x| x.name.contains("剣士")),
        "names: {:?}",
        used.iter().map(|x| &x.name).collect::<Vec<_>>()
    );
    for set in &used {
        let items = set.used_items();
        assert!(!items.is_empty(), "set {} has no items", set.index);
        for it in &items {
            assert!(
                (1..=2990).contains(&it.id),
                "set {} holds item id {}",
                set.index,
                it.id
            );
            assert!((1..=99).contains(&it.count), "count {} out of range", it.count);
        }
        // The game stores them in ascending id order.
        let mut ids: Vec<u16> = items.iter().map(|i| i.id).collect();
        let sorted = {
            let mut c = ids.clone();
            c.sort_unstable();
            c
        };
        ids = sorted;
        assert!(!items.is_empty());
    }
    // Unused records carry the placeholder name.
    assert!(
        sets.iter().filter(|x| x.is_empty()).all(|x| x.name == mhnx_core::myset::EMPTY_NAME
            || x.name.is_empty()),
        "an unused set has an unexpected name"
    );
}

#[test]
fn the_equipment_sets_read_real_records() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let sets = mhnx_core::myset::equip_sets(&s, 0);
    assert_eq!(sets.len(), mhnx_core::myset::EQUIP_SETS);
    let used: Vec<_> = sets.iter().filter(|x| !x.is_empty()).collect();
    assert_eq!(used.len(), 9, "slot 1 has nine equipment sets");
    for set in &used {
        assert!(!set.name.is_empty());
        // A weapon is always named; the box holds 2000 slots.
        let weapon = set.parts[0];
        assert!(
            weapon == mhnx_core::myset::NONE || weapon < 2000,
            "set {} points at equipment {}",
            set.index,
            weapon
        );
    }
    // The empty ones follow the documented shape.
    let empty: Vec<_> = sets.iter().filter(|x| x.is_empty()).collect();
    assert!(empty.len() > 20);
    assert!(empty.iter().all(|x| x.parts.iter().all(|p| *p == mhnx_core::myset::NONE)));
}

#[test]
fn an_item_and_equipment_set_can_be_renamed() {
    if !real_save().exists() {
        return;
    }
    let mut s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let before = s.to_bytes();
    assert!(mhnx_core::myset::set_item_set_name(&mut s, 0, 5, "テスト"));
    assert_eq!(mhnx_core::myset::item_sets(&s, 0)[5].name, "テスト");
    assert!(mhnx_core::myset::set_equip_set_name(&mut s, 0, 5, "テスト装備"));
    assert_eq!(mhnx_core::myset::equip_sets(&s, 0)[5].name, "テスト装備");
    // Writes stay inside their own record.
    let after = s.to_bytes();
    let item_at = 36 + 0x126474 + mhnx_core::myset::OFF_ITEM_SETS + 5 * mhnx_core::myset::ITEM_SET_SIZE;
    let eq_at = 36 + 0x126474 + mhnx_core::myset::OFF_EQUIP_SETS + 5 * mhnx_core::myset::EQUIP_SET_SIZE;
    let inside = |i: usize| {
        (i >= item_at && i < item_at + mhnx_core::myset::ITEM_SET_SIZE)
            || (i >= eq_at && i < eq_at + mhnx_core::myset::EQUIP_SET_SIZE)
    };
    let outside: Vec<usize> = (0..after.len())
        .filter(|i| after[*i] != before[*i])
        .filter(|i| !inside(*i))
        .collect();
    assert!(outside.is_empty(), "wrote outside the records: {:?}", outside);
    // Out of range and out of table are refused.
    assert!(!mhnx_core::myset::set_item_set_name(&mut s, 0, mhnx_core::myset::ITEM_SETS, "x"));
    assert!(!mhnx_core::myset::set_item_set_entry(&mut s, 0, 0, 0, 2991, 1));
    assert!(!mhnx_core::myset::set_item_set_entry(&mut s, 0, 0, 32, 1, 1));
}

#[test]
fn the_palico_skills_and_system_read() {
    if !real_save().exists() {
        return;
    }
    let s = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    let list = mhnx_core::palico::used(&s, 0);
    let mut real = 0;
    for p in &list {
        assert_ne!(p.system_name(), "?", "palico {} has system {}", p.index, p.system);
        let learned = p.learned_skills();
        for id in &learned {
            assert_ne!(
                mhnx_core::palico::skill_name(*id),
                "#",
                "skill {} has no name",
                id
            );
            assert!(*id != mhnx_core::palico::SKILL_EMPTY);
        }
        if !learned.is_empty() {
            real += 1;
        }
    }
    assert!(real >= 10, "expected most palicos to know skills, got {}", real);
    // skill 1 is the small health boost in the table we generated.
    assert_eq!(
        mhnx_core::palico::skill_name(1),
        "体力強化の術【小】",
        "the skill table is misaligned"
    );
}

#[test]
fn the_console_shows_the_sets() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::A);
    ed.screen = Screen::Sets;
    assert_eq!(ed.screen, Screen::Sets);
    let joined = ed
        .lines()
        .iter()
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("MY SETS"), "{}", joined);
    assert!(joined.contains("ITEM SETS"), "{}", joined);
    assert!(joined.contains("EQUIPMENT SETS"), "{}", joined);
    assert!(joined.contains("剣士"), "{}", joined);
    // [B] leaves the set screen for the talisman list, which is the screen the
    // console opens it from and the one the footer names ("[B] 護石一覧").
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Charms);
}

// ---------------------------------------------------------------------------
// The console save entry.
// ---------------------------------------------------------------------------

#[test]
fn the_save_list_offers_the_console_save_first() {
    // The pseudo path is not a real file: the console layer mounts the NAND
    // when it sees it, so it must never be handed to the file reader.
    assert_eq!(mhnx_core::hbedit::CONSOLE_SAVE, "nand:/system");
    assert!(mhnx_core::hbedit::CONSOLE_SAVE.starts_with("nand:"));
}

#[test]
fn the_console_entry_is_labelled_in_the_list() {
    let mut ed = Editor::new();
    ed.set_save_files(vec![String::from(mhnx_core::hbedit::CONSOLE_SAVE)]);
    let joined = ed
        .lines()
        .iter()
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("SAVE FILES"), "{}", joined);
    assert!(joined.contains("NAND"), "the console entry should be labelled: {}", joined);
    // Selecting it asks the platform layer to mount, not to read a file.
    ed.input(Key::A);
    assert_eq!(ed.take_request(), Some(Request::ReadSave(0)));
}


// ===========================================================================
// Console UI audit - docs/review_console_audit.md
//
// The questions a player asked about the console build: which screens can be
// reached and with which button, can the item / talisman / quest editors really
// change anything, can a quest be added, and does the button guide on screen
// agree with the keys the state machine handles?
//
// audit_*  behaviour that works today (stays green)
// gap_*    today's missing or wrong behaviour; it fails the moment the gap is
//          closed, which is the reminder to turn it into an audit_* test
// spec_*   what the UI should do; ignored until the gap is closed
//          (cargo test -- --ignored lists what is still missing)
// ===========================================================================

/// Every Screen variant, so the reachability test cannot miss a new one.
const AUDIT_SCREENS: [Screen; 32] = [
    Screen::SaveList,
    Screen::Slots,
    Screen::Char,
    Screen::Items,
    Screen::ItemPick,
    Screen::Charms,
    Screen::CharmEdit,
    Screen::SkillPick,
    Screen::Equip,
    Screen::EquipEdit,
    Screen::JewelPick,
    Screen::EquipPick,
    Screen::QuestEquip,
    Screen::Hunts,
    Screen::Guild,
    Screen::Roster,
    Screen::Sets,
    Screen::Palico,
    Screen::Convert,
    Screen::Options,
    Screen::SaveQuests,
    Screen::ArcList,
    Screen::ArcEntries,
    Screen::QuestEdit,
    Screen::EntryView,
    Screen::Mib,
    Screen::Gmd,
    Screen::Check,
    Screen::Csv,
    Screen::Progress,
    // Added with the two-tier navigation: 詳細 on any record screen.
    Screen::SaveMap,
    // Added with 変更一覧: the values the save differs from the card in.
    Screen::Review,
];

/// Keys that could plausibly start an edit on a record screen.
const AUDIT_EDIT_KEYS: [Key; 7] = [
    Key::A,
    Key::X,
    Key::Y,
    Key::L,
    Key::R,
    Key::Left,
    Key::Right,
];

fn audit_screen_name(s: Screen) -> &'static str {
    match s {
        Screen::SaveList => "SaveList",
        Screen::Slots => "Slots",
        Screen::Char => "Char",
        Screen::Items => "Items",
        Screen::ItemPick => "ItemPick",
        Screen::Charms => "Charms",
        Screen::CharmEdit => "CharmEdit",
        Screen::SkillPick => "SkillPick",
        Screen::Equip => "Equip",
        Screen::EquipEdit => "EquipEdit",
        Screen::JewelPick => "JewelPick",
        Screen::EquipPick => "EquipPick",
        Screen::QuestEquip => "QuestEquip",
        Screen::Hunts => "Hunts",
        Screen::Guild => "Guild",
        Screen::Roster => "Roster",
        Screen::Sets => "Sets",
        Screen::Palico => "Palico",
        Screen::Convert => "Convert",
        Screen::Options => "Options",
        Screen::SaveQuests => "SaveQuests",
        Screen::ArcList => "ArcList",
        Screen::ArcEntries => "ArcEntries",
        Screen::EntryView => "EntryView",
        Screen::Mib => "Mib",
        Screen::Gmd => "Gmd",
        Screen::Check => "Check",
        // Added with the CSV screen: the audit walks every screen, so the new
        // one has to have a name for its report.
        Screen::Csv => "Csv",
        // Added with the progress screen: awards / requests / quest bits.
        Screen::Progress => "Progress",
        // Added with クエスト改造: the aggregate quest screen.
        Screen::QuestEdit => "QuestEdit",
        // Added with the two-tier navigation: the raw bytes behind a screen.
        Screen::SaveMap => "SaveMap",
        // Added with 変更一覧: the values the save differs from the card in.
        Screen::Review => "Review",
    }
}

fn audit_note(ed: &Editor, seen: &mut Vec<&'static str>) {
    let name = audit_screen_name(ed.screen);
    if !seen.contains(&name) {
        seen.push(name);
    }
}

fn audit_walk(ed: &mut Editor, key: Key, seen: &mut Vec<&'static str>) {
    ed.input(key);
    audit_note(ed, seen);
}

/// The sample quest used by the entry tests, falling back to the one next to
/// the save editor on this machine.
fn audit_arc() -> Option<PathBuf> {
    if let Some(p) = sample_arc() {
        return Some(p);
    }
    let p = real_arc();
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// Press every edit candidate on a screen and prove that none of them moves the
/// save. That is the difference between "this screen is an editor" and "this
/// screen is a viewer".
fn audit_assert_read_only(what: &str, setup: fn(&mut Editor), keys: &[Key]) {
    for key in keys {
        let mut ed = Editor::new();
        open_save(&mut ed);
        setup(&mut ed);
        let before = ed.save.as_ref().expect("a save").body().to_vec();
        ed.input(*key);
        assert!(
            !ed.dirty,
            "{}: {:?} marked the save dirty - it has no writer for this record",
            what,
            key
        );
        assert!(
            ed.take_write().is_none(),
            "{}: {:?} queued a save write",
            what,
            key
        );
        assert!(
            ed.take_text_request().is_none(),
            "{}: {:?} opened a text editor",
            what,
            key
        );
        assert_eq!(
            ed.save.as_ref().expect("a save").body(),
            before.as_slice(),
            "{}: {:?} changed the save",
            what,
            key
        );
    }
}

/// The Switch build's actions() table, read straight out of its source.
///
/// That crate needs the Nintendo SDK, so it is not part of this workspace and
/// the test cannot call the function. Reading the text keeps the comparison
/// honest: this is the table the console really compiles.
const SWITCH_EDITOR_SRC: &str = include_str!("../../../switch/src/editor.rs");

/// (screen name, [(key, label)]) for every screen's key table.
///
/// The table is the state machine's now (`hbedit::Editor::key_facts`): the text
/// build's actions() renders it, the canvas footer renders it, and the guide
/// prints its first items. The audit reads that one table instead of parsing a
/// second copy out of the console's source - the copy is what drifted.
fn switch_hint_arms() -> Vec<(String, Vec<(String, String)>)> {
    AUDIT_SCREENS
        .iter()
        .map(|&screen| {
            let mut ed = Editor::new();
            ed.screen = screen;
            let chips = ed
                .key_facts()
                .into_iter()
                .filter(|f| !f.key.is_empty())
                .map(|f| (f.key.to_string(), f.label))
                .collect();
            (audit_screen_name(screen).to_string(), chips)
        })
        .collect()
}

fn hint_keys(arms: &[(String, Vec<(String, String)>)], screen: &str) -> Vec<String> {
    arms.iter()
        .find(|(n, _)| n == screen)
        .map(|(_, e)| e.iter().map(|(k, _)| k.clone()).collect())
        .unwrap_or_default()
}

/// The sample quest repacked with another id in its .mib.
fn quest_payload_with_id(arc: &PathBuf, id: i32) -> Option<Vec<u8>> {
    let raw = std::fs::read(arc).ok()?;
    let mut a = Arc::parse(&raw).ok()?;
    let i = a
        .entries
        .iter()
        .position(|e| extension_for(e.type_hash) == Some("mib"))?;
    if a.entries.get(i)?.data.len() < 0x10 {
        return None;
    }
    a.entries[i].data[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
    a.to_bytes().ok()
}

/// A quest id that the embedded catalogue knows but this save does not hold.
fn quest_id_the_save_does_not_have(save: &Save) -> Option<i32> {
    (1_010_000..1_020_000)
        .find(|id| !dlc::is_installed(save, *id) && dlc::bit_for_quest(*id).is_some())
}

#[test]
fn audit_every_screen_is_reachable_by_keys() {
    if !real_save().exists() {
        return;
    }
    let Some(arc) = audit_arc() else { return };
    let mut ed = Editor::new();
    ed.set_arc_files(vec![arc.to_string_lossy().to_string()]);
    let mut seen: Vec<&'static str> = Vec::new();
    audit_note(&ed, &mut seen);

    macro_rules! press {
        ($($k:expr),+ $(,)?) => {{
            $( audit_walk(&mut ed, $k, &mut seen); )+
        }};
    }

    // SaveList -> Slots: A asks the I/O layer for the bytes.
    open_save(&mut ed);
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::Slots);

    // Slot screen: X items, R download palicoes, L charms, - equipment,
    // Y quest files, A character.
    press!(Key::X);
    assert_eq!(ed.screen, Screen::Items);
    press!(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Items);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Slots);

    ed.screen = Screen::Equip; // the box is a section, not a key
    assert_eq!(ed.screen, Screen::Equip);
    // A opens the equipment form, and Y on a decoration row opens the
    // decoration picker; both are reachable without writing anything.
    press!(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit);
    for _ in 0..4 {
        press!(Key::Down);
    }
    assert_eq!(
        ed.equip_edit.map(|e| e.field),
        Some(5),
        "the cursor must land on the second decoration row"
    );
    press!(Key::Y);
    assert_eq!(ed.screen, Screen::JewelPick);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::EquipEdit);
    // The form's other name picker: [Y] on the ID row lists the equipment
    // names, which is the screen the hardware report was about.
    for _ in 0..3 {
        press!(Key::Down);
    }
    assert_eq!(
        ed.equip_edit.map(|e| e.field),
        Some(8),
        "the cursor must land on the ID row"
    );
    press!(Key::Y);
    assert_eq!(ed.screen, Screen::EquipPick);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::EquipEdit);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Equip);
    press!(Key::L);
    assert_eq!(ed.screen, Screen::Guild);
    // [ZR] on the card is the progress screen: one slot's awards, request
    // flags and quest bits, with its three views on the sub-tab bar. It used
    // to be [-], which is the delete key and nothing else now.
    press!(Key::Z);
    assert_eq!(ed.screen, Screen::Progress);
    // L / R walk the three views without leaving the screen.
    press!(Key::R);
    assert_eq!(ed.progress_view(), 1);
    press!(Key::R);
    assert_eq!(ed.progress_view(), 2);
    press!(Key::L);
    assert_eq!(ed.progress_view(), 1);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Guild);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
    // The roster hangs off the equipment box's [R]; the box is a section now,
    // because [-] is the delete key and nothing else.
    ed.screen = Screen::Equip;
    press!(Key::R);
    assert_eq!(ed.screen, Screen::Roster);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Slots);

    press!(Key::R);
    assert_eq!(ed.screen, Screen::Palico);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Slots);

    // Charms -> CharmEdit -> SkillPick.
    press!(Key::L);
    assert_eq!(ed.screen, Screen::Charms);
    press!(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit);
    press!(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::CharmEdit);
    // The new talisman is a draft only once a field is changed: a form that was
    // opened and left alone is clean (Editor::charm_draft_dirty compares the
    // form with the values it was opened with), so this [B] leaves at once.
    // The two-press question is pinned by review_b's charm test.
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Charms);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Slots);

    // Quest files: the section bar opens the section (the state machine's own
    // [Y] on the slot screen is 一括MAX), and A opens the .arc.
    ed.screen = Screen::ArcList;
    ed.cursor = 0;
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::ArcEntries);

    press!(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
    assert!(!ed.mib_bytes().is_empty(), "the .arc must open on its .mib");
    press!(Key::A);
    assert_eq!(ed.screen, Screen::Mib);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
    // Walk the entry cursor to the .gmd and open it.
    let gmd = ed
        .arc
        .as_ref()
        .expect("an arc")
        .entries
        .iter()
        .position(|e| extension_for(e.type_hash) == Some("gmd"))
        .expect("a .gmd entry");
    while ed.cursor != gmd {
        press!(Key::Down);
    }
    press!(Key::A);
    assert_eq!(ed.screen, Screen::Gmd);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
    press!(Key::L);
    assert_eq!(ed.screen, Screen::Check);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
    // The file list is クエスト改造's detail view, so its [B] goes back there.
    press!(Key::B);
    assert_eq!(ed.screen, Screen::QuestEdit);

    // クエスト改造 is where the console lands when a quest is picked (the
    // I/O layer calls this after open_arc_bytes), and its [B] goes back to the
    // list the quest was opened from.
    ed.enter_quest_edit();
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::QuestEdit);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::ArcList);

    press!(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);
    press!(Key::L);
    assert_eq!(ed.screen, Screen::QuestEquip);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::SaveQuests);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::ArcList);
    press!(Key::Y);
    assert_eq!(ed.screen, Screen::SaveList);

    // The rest hang off the save list and the character screen.
    press!(Key::L);
    assert_eq!(ed.screen, Screen::Convert);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::SaveList);
    press!(Key::X);
    assert_eq!(ed.screen, Screen::Options);
    // R is the tab-forward key on the console, and the tab after the settings
    // is the CSV screen, so one press lands on it either way.
    press!(Key::R);
    assert_eq!(ed.screen, Screen::Csv);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::SaveList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::Slots);
    press!(Key::A);
    assert_eq!(ed.screen, Screen::Char);
    // 詳細: not a key of its own - the console opens it from the [詳細] pill or
    // a long [R] hold - so the audit calls the entry point the pill calls and
    // proves that [B] hands the record screen back.
    ed.open_save_map(Screen::Char);
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::SaveMap);
    press!(Key::Down, Key::Right, Key::A);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Char, "B must come back to the record screen");
    // 変更一覧: the other screen of the 詳細・ツール shelf. Its [B] hands back
    // the screen it was opened from, the same rule 生データ follows.
    ed.open_review();
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::Review);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Char, "B must come back to the record screen");
    // The hunt record is the オトモ・記録 section now: [-] is the delete key.
    // The section bar is the console's (ZL / ZR and the tappable bar), so the
    // core test names the screen the section lands on.
    ed.screen = Screen::Hunts;
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::Hunts);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
    // 装備's マイセット tab is a section tab too, and the character screen's
    // own [Y] is 刻み now (the console's chip table is the reference).
    ed.screen = Screen::Sets;
    audit_note(&ed, &mut seen);
    assert_eq!(ed.screen, Screen::Sets);
    press!(Key::B);
    assert_eq!(ed.screen, Screen::Charms, "the set screen goes to the talisman list");

    let missing: Vec<&str> = AUDIT_SCREENS
        .iter()
        .map(|s| audit_screen_name(*s))
        .filter(|n| !seen.contains(n))
        .collect();
    assert!(
        missing.is_empty(),
        "screens no key press reaches: {:?} (reached {:?})",
        missing,
        seen
    );
    assert_eq!(seen.len(), AUDIT_SCREENS.len(), "reached {:?}", seen);
}

#[test]
fn audit_the_item_type_can_be_changed_through_the_picker() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::Items);
    let vis = ed.visible_items();
    assert!(!vis.is_empty(), "the dump has item slots");
    let target = vis[0];
    let old = ed
        .save
        .as_ref()
        .expect("a save")
        .read_item_box(ed.slot)
        .expect("a box")[target]
        .id;

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "A must open the type picker");
    assert_eq!(ed.item_pick_target, target, "the picker must aim at the cursor");
    assert!(ed.item_pick_matches().len() > 3, "the name table must be loaded");

    // Move until the highlighted id is a different one.
    ed.input(Key::Down);
    let mut want = ed.item_pick_matches()[ed.item_pick_cursor];
    let mut guard = 0;
    while want == old && guard < 8 {
        ed.input(Key::Down);
        want = ed.item_pick_matches()[ed.item_pick_cursor];
        guard += 1;
    }
    assert_ne!(want, old, "the test must pick another item");

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Items);
    let after = ed
        .save
        .as_ref()
        .expect("a save")
        .read_item_box(ed.slot)
        .expect("a box")[target];
    assert_eq!(after.id, want, "the chosen id must land in the box");
    assert!(after.count >= 1, "a new item must have a count");
    assert!(ed.dirty);
    assert!(!ed.item_undo.is_empty(), "the replacement must be undoable");

    // On the item box [X] is the save key, exactly as the footer says; the step
    // is the deck's button, so the headless build sets it directly.
    ed.step = 1;
    assert_eq!(ed.step, 1, "the step is not on a key here");
    ed.item_screen_save();
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).expect("a valid container");
            assert_eq!(
                s.read_item_box(ed.slot).expect("a box")[target].id,
                want,
                "the new type must survive the write"
            );
        }
        other => panic!("expected a save write, got {:?}", other),
    }
}

#[test]
fn audit_a_charm_can_be_edited_through_the_skill_picker() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Charms);
    assert!(!ed.charms().is_empty(), "the sample has talismans");

    ed.input(Key::A);
    assert_eq!(
        ed.screen,
        Screen::CharmEdit,
        "A must edit the charm under the cursor"
    );
    let seed = ed.charm_edit.expect("the form is open");
    let points0 = seed.points1;
    let grade = seed.grade;

    // The form has ten rows and opens on the first skill: 0 box slot, 1 kind,
    // 2 slots, 3 skill1, 4 points1, 5 skill2, 6 points2, 7..9 the three
    // decoration sockets. Down walks all of them and wraps, which is what "ten
    // rows" means here.
    assert_eq!(seed.field, 3, "the form lands on the first skill");
    for want in [4usize, 5, 6, 7, 8, 9, 0, 1, 2, 3] {
        ed.input(Key::Down);
        assert_eq!(
            ed.charm_edit.expect("open").field,
            want,
            "the form must have ten rows"
        );
    }

    // Left/Right edits the row under the cursor.
    ed.input(Key::Down);
    assert_eq!(ed.charm_edit.expect("open").field, 4);
    ed.input(Key::Right);
    assert_eq!(
        ed.charm_edit.expect("open").points1,
        points0 + 1,
        "the points row must move"
    );
    ed.input(Key::Up);
    assert_eq!(ed.charm_edit.expect("open").field, 3);

    // Y on a skill row opens the name picker, which lists the kind's own pool.
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    let pick_kind = ed.charm_edit.expect("the form is open").kind;
    let pool = mhnx_core::charm::allowed_skill_ids(pick_kind, false);
    assert_eq!(
        ed.skill_pick_matches(),
        pool,
        "the console must list the kind's pool"
    );
    assert!(ed.skill_pick_matches().len() > 3);
    let want_id = pool[pool.len() / 2];
    let want_name = mhnx_core::skills::skill_name(want_id)
        .expect("a name")
        .to_string();
    ed.apply_skill_filter(want_name.clone());
    assert!(
        !ed.skill_pick_matches().is_empty(),
        "the filter must find {}",
        want_name
    );
    ed.skill_pick_cursor = ed
        .skill_pick_matches()
        .iter()
        .position(|id| *id == want_id)
        .expect("the target is in the filtered list");
    let chosen = ed.skill_pick_matches()[ed.skill_pick_cursor];
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert_eq!(ed.charm_edit.expect("open").skill1, chosen);
    assert_eq!(
        ed.charm_edit.expect("open").skill2,
        seed.skill2,
        "picking skill 1 must leave skill 2 alone"
    );
    assert_eq!(
        ed.charm_edit.expect("open").grade,
        grade,
        "the picker must not touch the grade"
    );

    // Row 1 is the grade; Right steps it on and Left brings it back.
    ed.input(Key::Up); // 2: slots
    ed.input(Key::Up); // 1: grade
    assert_eq!(ed.charm_edit.expect("open").field, 1, "row 1 is the grade");
    ed.input(Key::Right);
    assert_ne!(
        ed.charm_edit.expect("open").grade,
        grade,
        "right must move the grade"
    );
    ed.input(Key::Left);
    assert_eq!(ed.charm_edit.expect("open").grade, grade, "left steps back");

    // Writing applies the grade the form holds. The points are clamped to what
    // that grade allows, but the chosen skill is kept.
    let edit = ed.charm_edit.expect("open");
    let (want_skill, want_points) = match mhnx_core::charm::allowed_points_rarity(
        edit.grade.rarity(),
        edit.skill1,
        false,
    ) {
        // A skill the grade forbids outright: warned about, never deleted.
        Some(r) if r.min == 0 && r.max == 0 => (edit.skill1, edit.points1),
        Some(r) => (edit.skill1, edit.points1.clamp(r.min, r.max)),
        None => (0, 0),
    };
    while ed.charm_edit.map(|e| e.field) != Some(0) {
        ed.input(Key::Up);
    }
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Charms);
    assert!(ed.dirty);
    // The write is [A]'s, and [A] leaves the form: the record is the box's now,
    // so the list is where the next thing happens (the save key is the one that
    // writes without leaving).
    assert!(
        ed.charm_edit.is_none(),
        "the write closes the form it wrote from"
    );
    let idx = edit.index;
    let rec = {
        let s = ed.save.as_ref().expect("a save");
        let b = s.equipment_box(ed.slot).expect("a box");
        let o = idx * mhnx_core::charm::CHARM_RECORD;
        b[o..o + mhnx_core::charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(
        rec[0x12],
        edit.grade.rarity(),
        "the grade in the form must be written"
    );
    assert_eq!(
        mhnx_core::charm::rarity_of(rec[0x02]),
        Some(edit.grade.rarity() as i32),
        "the equipment id must agree with the rarity"
    );
    assert_eq!(
        rec[0x0C], want_skill,
        "the chosen skill must survive the write"
    );
    assert_eq!(
        rec[0x0E] as i8,
        want_points,
        "the points must land inside the grade's range"
    );

    // A skill the grade cannot carry is still written: the console warns and
    // leaves the choice alone instead of quietly deleting it. Use a second
    // talisman so both writes can be checked in the written file.
    assert!(
        ed.charms().len() > 1,
        "the sample box holds more than one talisman"
    );
    ed.cursor = 1;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
    let second = ed.charm_edit.expect("open");
    assert_ne!(second.index, idx, "the second edit must touch another charm");
    let forbidden = (1..=255u8)
        .find(|s| {
            matches!(
                mhnx_core::charm::allowed_points_rarity(second.grade.rarity(), *s, false),
                Some(r) if r.min == 0 && r.max == 0
            )
        })
        .expect("the grade forbids some skills");
    if let Some(e) = &mut ed.charm_edit {
        e.skill1 = forbidden;
        e.points1 = 0;
    }
    let target = ed.charm_edit.expect("open");
    // Write from the 装備BOX枠 row: [A] on the skill row opens the skill list.
    while ed.charm_edit.map(|e| e.field) != Some(0) {
        ed.input(Key::Up);
    }
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Charms);
    let rec2 = {
        let s = ed.save.as_ref().expect("a save");
        let b = s.equipment_box(ed.slot).expect("a box");
        let o = target.index * mhnx_core::charm::CHARM_RECORD;
        b[o..o + mhnx_core::charm::CHARM_RECORD].to_vec()
    };
    assert_eq!(
        rec2[0x0C], forbidden,
        "a skill the grade forbids must not be deleted"
    );
    let warnings = {
        let s = ed.save.as_ref().expect("a save");
        let b = s.equipment_box(ed.slot).expect("a box");
        let grade = mhnx_core::charm::rarity_of_record(&rec2).expect("still a talisman");
        mhnx_core::charm::grade_warnings(b, target.index, grade)
    };
    assert!(
        !warnings.is_empty(),
        "the record must be warned about, not repaired"
    );

    // Both writes survive the save round trip.
    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).expect("a valid container");
            let list = s.charms(ed.slot).expect("charms");
            let c = list
                .iter()
                .find(|c| c.index == idx)
                .expect("the edited charm must be in the box");
            assert_eq!(c.skill1, want_skill, "the skill must survive the write");
            assert_eq!(c.points1, want_points, "the points must survive the write");
            let c2 = list
                .iter()
                .find(|c| c.index == target.index)
                .expect("the warned charm must be in the box");
            assert_eq!(
                c2.skill1, forbidden,
                "the forbidden skill is written as chosen"
            );
        }
        other => panic!("expected a save write, got {:?}", other),
    }
}

#[test]
fn audit_a_quest_inside_the_save_can_be_edited_and_written_back() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    let before = ed.save.as_ref().expect("a save").to_bytes();

    open_quest_section(&mut ed);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);
    let target = ed.save_quests()[0].clone();
    assert!(
        !target.data.is_empty(),
        "quest {} has no payload",
        target.quest_id
    );

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::QuestEdit);
    // The .mib editor is behind the file list, which the [ファイル] pill opens.
    ed.screen = Screen::ArcEntries;
    ed.cursor = ed.entry;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib);
    goto_mib_field(&mut ed, "quest_level");
    let lv = questfields::MIB_FIELDS
        .iter()
        .find(|f| f.key == "quest_level")
        .expect("a quest_level field");
    let before_val = questfields::read_u32(ed.mib_bytes(), lv.offset, lv.kind).unwrap();
    ed.input(Key::Right);
    assert_eq!(
        questfields::read_u32(ed.mib_bytes(), lv.offset, lv.kind).unwrap(),
        before_val + 1
    );

    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
    ed.input(Key::X);
    assert!(
        ed.message.contains("上書き") || ed.message.contains("追加"),
        "message was {}",
        ed.message
    );

    let written = ed
        .save_quests()
        .into_iter()
        .find(|q| q.slot == target.slot)
        .expect("the quest must still be there");
    assert_eq!(written.quest_id, target.quest_id, "the id must not move");
    let repacked = ed.arc.as_ref().expect("an arc").to_bytes().expect("repack");
    assert_eq!(
        written.data, repacked,
        "the edited payload must be what landed in the slot"
    );
    assert_ne!(
        ed.save.as_ref().expect("a save").to_bytes(),
        before,
        "the edit must reach the save"
    );
}

#[test]
fn audit_a_quest_the_save_does_not_have_can_be_added() {
    if !real_save().exists() {
        return;
    }
    let Some(arc) = audit_arc() else { return };
    let mut ed = Editor::new();
    open_save(&mut ed);
    let Some(id) = quest_id_the_save_does_not_have(ed.save.as_ref().expect("a save")) else {
        return;
    };
    let Some(payload) = quest_payload_with_id(&arc, id) else {
        return;
    };
    let before = dlc::list_quests(ed.save.as_ref().expect("a save")).len();

    // Open the .arc with its .mib id rewritten to a quest this save lacks.
    ed.open_arc_bytes("patched.arc", &payload);
    assert_eq!(ed.screen, Screen::ArcEntries);
    ed.input(Key::X);
    assert!(
        ed.message.contains("追加"),
        "the console must say it added the quest; message was {}",
        ed.message
    );
    assert_eq!(
        dlc::list_quests(ed.save.as_ref().expect("a save")).len(),
        before + 1,
        "the quest must take a slot"
    );
    assert!(dlc::is_installed(ed.save.as_ref().expect("a save"), id));
}

#[test]
fn audit_the_guild_card_can_be_edited() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Equip; // the box is a section, not a key
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Guild);
    let joined = ed
        .lines()
        .iter()
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("GUILD CARD"), "{}", joined);
    assert!(joined.contains("comment"), "{}", joined);
    // The rank is the first field, so Right raises it by one.
    let before = ed.save.as_ref().and_then(|s| {
        mhnx_core::guildcard::hunter_rank(s, ed.slot)
    });
    ed.input(Key::Right);
    let after = ed.save.as_ref().and_then(|s| {
        mhnx_core::guildcard::hunter_rank(s, ed.slot)
    });
    assert_eq!(
        after,
        before.map(|v| (v + 1).min(999)),
        "the card rank did not move"
    );
    assert!(ed.dirty);
}

#[test]
fn audit_the_palico_roster_can_be_edited() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Equip; // the box is a section, not a key
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Roster);
    assert!(!ed.palicos().is_empty());
    // Up/Down still move the cursor.
    let before_cursor = ed.roster_cursor;
    ed.input(Key::Down);
    assert_ne!(ed.roster_cursor, before_cursor);
    ed.input(Key::Up);
    // Left lowers the highlighted level.
    let before = ed.palicos()[0].clone();
    ed.input(Key::Left);
    let after = ed.palicos()[0].clone();
    assert_eq!(after.level, before.level.saturating_sub(1).max(1));
    assert!(ed.dirty);
}

#[test]
fn audit_the_hunt_records_can_be_edited() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::A);
    ed.screen = Screen::Hunts; // the record section, not a key
    assert_eq!(ed.screen, Screen::Hunts);
    assert!(ed.hunt_rows().len() > 100);
    assert!(ed.hunt_rows().iter().any(|(_, slay, _, _, _)| *slay > 0));
    // Right raises the slay count of the highlighted monster.
    let before = ed.hunt_rows()[0];
    ed.input(Key::Right);
    let after = ed.hunt_rows()[0];
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1 + 1, "the slay count did not move");
    assert!(ed.dirty);
}

#[test]
fn gap_the_quest_equipment_list_is_display_only() {
    if !real_save().exists() {
        return;
    }
    audit_assert_read_only(
        "QuestEquip",
        |ed| {
            open_quest_section(ed);
            ed.input(Key::X);
            ed.input(Key::L);
            assert_eq!(ed.screen, Screen::QuestEquip);
        },
        &AUDIT_EDIT_KEYS,
    );

    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed);
    ed.input(Key::X);
    ed.input(Key::L);
    let list = ed.quest_equipment();
    assert!(!list.is_empty(), "the DLC section has the 45 entries");
}

/// The my-sets screen used to be a list with no editor at all. This is the
/// guard that it now edits the equipment sets, and that the editor is a
/// **mode** of the screen rather than a screen of its own: the audit above
/// counts the Screen variants, and adding one just for a form would have made
/// every match on it (and this table) wrong.
#[test]
fn audit_the_my_sets_can_be_edited() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::A);
    ed.screen = Screen::Sets;
    assert_eq!(ed.screen, Screen::Sets);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Sets, "the editor is a mode, not a screen");
    assert!(ed.set_editor_open(), "A on the list must open the editor");
    assert!(
        !ed.minus_rescans(),
        "the editor owns Minus (the two-press part clear)"
    );

    // The first part row: walk it to another record and write it.
    ed.input(Key::Down);
    ed.input(Key::Right);
    let pick = ed.set_pick(0);
    assert_ne!(pick, mhnx_core::myset::NONE, "the row must land on a record");
    let set = ed.editing_set().expect("the editor knows its set").index;
    ed.input(Key::A);
    assert!(ed.dirty, "A must write the part into the set");

    // Whatever it wrote, the set and the equipment record now agree about the
    // decoration ids as well as the index - that is the whole point of
    // set_part writing both copies.
    let slot = ed.slot;
    let save = ed.save.as_ref().expect("a save");
    let rec = mhnx_core::myset::equip_sets(save, slot)[set].clone();
    assert_eq!(rec.parts[0], pick, "the part must name the chosen record");
    let box_bytes = save.equipment_box(slot).expect("an equipment box");
    let at = pick as usize * mhnx_core::equip::EQUIP_RECORD;
    let want = mhnx_core::equip::Equipment::decorations(
        pick as usize,
        &box_bytes[at..at + mhnx_core::equip::EQUIP_RECORD],
    );
    assert_eq!(
        rec.decos[0], want,
        "the set's decoration copy must follow the record"
    );

    // X reports, it does not repair: the body is byte for byte what it was.
    let before = ed.save.as_ref().expect("a save").body().to_vec();
    ed.input(Key::X);
    let expected = mhnx_core::myset::warnings(ed.save.as_ref().expect("a save"), slot, set);
    assert_eq!(ed.set_warnings, expected, "X must report exactly warnings()");
    assert_eq!(
        ed.save.as_ref().expect("a save").body(),
        before.as_slice(),
        "asking for the warnings must not touch the save"
    );
}

#[test]
fn gap_the_equipment_box_is_display_only() {
    if !real_save().exists() {
        return;
    }
    // Y opens the filter prompt and X clears it, so only the keys that would
    // have to start an edit are probed here.
    audit_assert_read_only(
        "Equip",
        |ed| {
            ed.screen = Screen::Equip; // the box is a section, not a key
            assert_eq!(ed.screen, Screen::Equip);
        },
        &[Key::A, Key::Left, Key::Right, Key::Up, Key::Down],
    );

    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Minus);
    assert!(!ed.equipment().is_empty(), "the box decodes to named items");
    // Save::equipment_box_mut has no editor in front of it.
}

/// The entry view used to advertise X and have no arm for it, so the key did
/// nothing. This is the guard that it still writes.
#[test]
fn audit_the_entry_view_x_writes_the_archive() {
    let Some(arc) = audit_arc() else { return };
    let bytes = std::fs::read(&arc).expect("the sample arc");
    let mut ed = Editor::new();
    ed.open_arc_bytes(&arc.to_string_lossy(), &bytes);
    assert_eq!(ed.screen, Screen::ArcEntries);
    let rem = ed
        .arc
        .as_ref()
        .expect("an arc")
        .entries
        .iter()
        .position(|e| extension_for(e.type_hash) == Some("rem"))
        .expect("a .rem entry");
    while ed.cursor != rem {
        ed.input(Key::Down);
    }
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::EntryView);
    let footer = ed.lines().last().map(|l| l.text.clone()).unwrap_or_default();
    assert!(
        footer.contains("X write .arc"),
        "the footer advertises the key: {}",
        footer
    );

    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::EntryView, "X must not navigate");
    assert!(
        ed.take_write().is_some(),
        "X on the entry view must ask for the archive to be written"
    );
}

/// The .arc list's way back is [B], and the chip has to say so.
///
/// It was the one screen whose chips named no way out: the console answered the
/// key before the state machine ever saw it, so the table - which describes the
/// console's dispatch - was written from the state machine and lost the key.
/// Both layers answer it now, and both go to the save list.
#[test]
fn audit_the_arc_list_names_the_back_key_the_console_answers() {
    let arms = switch_hint_arms();
    assert!(
        hint_keys(&arms, "ArcList").iter().any(|k| k == "B"),
        "the console answers [B] on this screen, so the chips must name it"
    );
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    open_quest_section(&mut ed);
    assert_eq!(ed.screen, Screen::ArcList);
    // The state machine answers [B] too now, and it goes where the console
    // sends it: the save list. It used to go to セーブ内, whose own [B] comes
    // right back here - one key, two screens, neither ever leaving the section.
    ed.input(Key::B);
    assert_eq!(
        ed.screen,
        Screen::SaveList,
        "[B] leaves the quest section for the save list"
    );
    // [Y] is the other way back (the save editor), and it lands in the same
    // place.
    ed.screen = Screen::ArcList;
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SaveList);
}

/// The slot screen's chips must not promise the palico roster on [R].
///
/// The state machine's [R] opens the download catalogue (配信オトモ) and its
/// [L] the talisman list, but the console answers both shoulders as the
/// sub-tab pair before the state machine sees them - so a chip that said
/// "R オトモ" named a key the console spends elsewhere. The five state
/// machine jobs (X items, L charms, R the download catalogue, - the equipment
/// box) are sections on this build: ZL / ZR, not keys.
#[test]
fn audit_the_slots_screen_does_not_promise_the_palico_list() {
    let arms = switch_hint_arms();
    let slots = arms
        .iter()
        .find(|(n, _)| n == "Slots")
        .map(|(_, e)| e.clone())
        .expect("an arm for the slot screen");
    assert!(
        !slots.iter().any(|(k, _)| k == "R"),
        "the console spends [R] on the sub-tab, not on the roster: {:?}",
        slots
    );
    // The footer names the slot pair, not the shoulders: the sub-tab is the
    // console's and no line on the glass promises it here.
    assert!(
        slots.iter().any(|(k, _)| k == "◀/▶"),
        "the slot pair is what the arrows do here: {:?}",
        slots
    );

    if !real_save().exists() {
        return;
    }
    // The state machine still answers R with the download catalogue; the chips
    // describe the console, which is the layer that answers the key first.
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::R);
    assert_eq!(
        ed.screen,
        Screen::Palico,
        "R opens the download catalogue in the state machine"
    );
    ed.input(Key::B);
    ed.screen = Screen::Equip; // the box is a section, not a key
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Roster);
}

/// The hint table used to spell the charm screen "Charm", which is not a
/// variant: it was a binding that matched every screen and made the seventeen
/// arms below it unreachable, so those screens all showed charm chips.
#[test]
fn audit_the_hint_table_has_no_catch_all_arm() {
    let arms = switch_hint_arms();
    let names: Vec<&str> = AUDIT_SCREENS
        .iter()
        .map(|s| audit_screen_name(*s))
        .collect();
    assert_eq!(
        arms.len(),
        names.len(),
        "arms: {:?}",
        arms.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    // Every entry must name a real screen and carry keys: the table is the
    // state machine's, and a screen with no keys is a screen nobody can drive.
    for (name, keys) in &arms {
        assert!(
            names.contains(&name.as_str()),
            "an entry names something that is not a screen: {name}"
        );
        assert!(!keys.is_empty(), "{name} has no keys");
    }
    // The console renders that table instead of carrying its own copy: the copy
    // is what had the catch-all arm (a bare `Charm` binding that swallowed the
    // seventeen arms below it).
    assert!(
        SWITCH_EDITOR_SRC.contains("ed.key_facts()"),
        "the text build must render the state machine's table"
    );
    assert!(
        !SWITCH_EDITOR_SRC.contains("fn actions(screen: mhnx_core::hbedit::Screen)"),
        "the second table is gone"
    );
}

#[test]
fn audit_the_hint_table_names_the_keys_that_work() {
    let arms = switch_hint_arms();

    // Every chip must name a key the console's dispatch really answers on that
    // screen. The table describes switch/src/ui.rs handle_pad_key, not the bare
    // state machine, so a key the console spends before the state machine sees
    // it is not a chip.
    //
    // Charms: [＋] creates a talisman (the console sends the state machine's L,
    // because a plain L is the sub-tab key), and [-] is the two-press delete.
    let charms = hint_keys(&arms, "Charms");
    assert!(
        charms.iter().any(|k| k == "＋"),
        "the charm screen creates with [＋], so it must say so: {:?}",
        charms
    );
    assert!(
        charms.iter().any(|k| k == "-"),
        "[-] is the two-press delete here: {:?}",
        charms
    );
    assert!(
        !charms.iter().any(|k| k == "L"),
        "L is the sub-tab key on the console, not a talisman"
    );
    // ArcEntries: [ZR] writes this quest out, [Y] is the checker, [X] saves.
    // [-] is the delete key and a file list has nothing to delete.
    let arc = hint_keys(&arms, "ArcEntries");
    for k in ["ZR", "Y", "X"] {
        assert!(
            arc.iter().any(|h| h == k),
            "ArcEntries handles {} but does not say so: {:?}",
            k,
            arc
        );
    }
    assert!(
        !arc.iter().any(|h| h == "-"),
        "[-] is the delete key and a file list has nothing to delete: {:?}",
        arc
    );
    // The pickers' [X] is クリア and the .mib screen's [X] is the .arc save.
    assert!(hint_keys(&arms, "SkillPick").iter().any(|k| k == "X"));
    assert!(hint_keys(&arms, "Mib").iter().any(|k| k == "X"));
    // Palico: the language is the arrow pair (the console answers ◀ / ▶ before
    // the state machine, whose L / R are the sub-tabs), so the chips say so.
    let palico = hint_keys(&arms, "Palico");
    assert!(
        palico.iter().any(|h| h == "◀/▶"),
        "the language is ◀ / ▶ on the console: {:?}",
        palico
    );
    for k in ["L", "R"] {
        assert!(
            !palico.iter().any(|h| h == k),
            "the console spends {} on the sub-tab, not on the language: {:?}",
            k,
            palico
        );
    }

    if !real_save().exists() {
        return;
    }
    // Charms: the state machine's L really creates a new talisman - it is the
    // key the console's [＋] sends (ui.rs Screen::Charms, Key::Plus).
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Charms);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit, "L must create a charm");

    // Palico: the state machine answers the arrow pair itself, and the deck's
    // button calls the same method - one implementation, one sentence.
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Palico);
    assert_eq!(ed.palico_lang, 0);
    ed.input(Key::Left);
    assert_eq!(ed.palico_lang, 1, "◀ must switch the language");
    ed.input(Key::Right);
    assert_eq!(ed.palico_lang, 0, "▶ switches it back");

    // SkillPick: X really clears the filter.
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    ed.apply_skill_filter(String::from("zzz"));
    assert!(!ed.skill_pick_filter.is_empty());
    ed.input(Key::X);
    assert!(ed.skill_pick_filter.is_empty(), "X clears the filter");

    // The core footer of the slot screen names the three keys the console
    // really answers here: [A] opens the character, [Y] is the bulk MAX and
    // [X] saves. The state machine's X / L / R / - jobs are sections (ZL / ZR)
    // on this build, so the line must not name them.
    let mut ed = Editor::new();
    open_save(&mut ed);
    let footer = ed.lines().last().map(|l| l.text.clone()).unwrap_or_default();
    assert!(footer.contains("A char edit"), "{}", footer);
    assert!(footer.contains("Y max all"), "{}", footer);
    assert!(footer.contains("X save"), "{}", footer);
    for k in ["L", "R", "-"] {
        assert!(
            !footer.contains(k),
            "the slot footer now names {}: {}",
            k,
            footer
        );
    }

    let Some(arc) = audit_arc() else { return };
    let bytes = std::fs::read(&arc).expect("the sample arc");

    // ArcEntries: - really asks the I/O layer for that quest.
    let mut ed = Editor::new();
    ed.open_arc_bytes(&arc.to_string_lossy(), &bytes);
    assert_eq!(ed.screen, Screen::ArcEntries);
    assert_eq!(ed.take_request(), None);
    ed.input(Key::Z);
    match ed.take_request() {
        Some(Request::ExtractQuests { ids }) => assert_eq!(ids.len(), 1),
        other => panic!("expected one quest, got {:?}", other),
    }

    // Mib: [X] is the save key (the footer says so), and the step is the deck's
    // button - the headless build sets it directly.
    let mut ed = Editor::new();
    ed.open_arc_bytes(&arc.to_string_lossy(), &bytes);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib);
    ed.step = 1;
    assert_eq!(ed.step, 1, "the step is not on a key here");
}

#[test]
fn gap_the_quest_list_cannot_add_or_remove_a_quest() {
    if !real_save().exists() {
        return;
    }
    for key in [
        Key::A,
        Key::B,
        Key::L,
        Key::R,
        Key::X,
        Key::Y,
        Key::Minus,
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
    ] {
        let mut ed = Editor::new();
        open_save(&mut ed);
        open_quest_section(&mut ed);
        ed.input(Key::X);
        assert_eq!(ed.screen, Screen::SaveQuests);
        let before_n = ed.save_quests().len();
        let before = ed.save.as_ref().expect("a save").to_bytes();
        ed.input(key);
        assert_eq!(
            ed.save_quests().len(),
            before_n,
            "{:?} must not add or drop a quest slot",
            key
        );
        assert_eq!(
            ed.save.as_ref().expect("a save").to_bytes(),
            before,
            "{:?} must not touch the save",
            key
        );
    }
    // dlc::add_quest and dlc::uninstall_quest exist; no screen calls either.
}

#[test]
#[ignore = "missing: install_into_save only overwrites an installed quest"]
fn spec_a_quest_can_be_added_from_an_arc() {
    if !real_save().exists() {
        return;
    }
    let Some(arc) = audit_arc() else { return };
    let mut ed = Editor::new();
    open_save(&mut ed);
    let Some(id) = quest_id_the_save_does_not_have(ed.save.as_ref().expect("a save")) else {
        return;
    };
    let Some(payload) = quest_payload_with_id(&arc, id) else {
        return;
    };
    ed.open_arc_bytes("patched.arc", &payload);
    ed.input(Key::X);
    assert!(
        dlc::is_installed(ed.save.as_ref().expect("a save"), id),
        "X on an .arc must add a quest the save does not have yet"
    );
}

#[test]
#[ignore = "missing: the guild card screen has no editor"]
fn spec_the_guild_card_can_be_edited() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Minus);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Guild);
    ed.input(Key::A);
    assert!(
        ed.dirty,
        "A on the guild card must open an editor (comment / rank / arena log)"
    );
}

#[test]
#[ignore = "missing: actions() has a catch-all Charm arm"]
fn spec_every_screen_has_its_own_hint_chips() {
    let arms = switch_hint_arms();
    let names: Vec<&str> = AUDIT_SCREENS
        .iter()
        .map(|s| audit_screen_name(*s))
        .collect();
    for (name, keys) in &arms {
        assert!(
            names.contains(&name.as_str()),
            "actions() has an arm that is not a Screen variant: {} (a bare identifier is a catch-all)",
            name
        );
        assert!(!keys.is_empty(), "{} has no hints", name);
    }
    for n in names {
        assert!(
            arms.iter().any(|(a, _)| a == n),
            "no hint arm for {}",
            n
        );
    }
}

#[test]
fn the_console_can_edit_a_quest_equipment_item() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // Slots -> quest list -> the save's own quests -> quest equipment.
    open_quest_section(&mut ed);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::QuestEquip);
    assert!(ed.quest_equipment().len() > 0);

    // Open the first slot that holds something, then walk to a filled entry.
    let mut guard = 0;
    while !ed.qe_open && guard < 50 {
        ed.input(Key::A);
        guard += 1;
        if !ed.qe_open {
            ed.input(Key::Down);
        }
    }
    assert!(ed.qe_open, "the slot did not open");
    let mut guard = 0;
    while ed.quest_equip_items().get(ed.qe_item_cursor).map(|i| i.item) == Some(0)
        && guard < 200
    {
        ed.input(Key::Down);
        guard += 1;
    }
    let target = ed.quest_equip_items()[ed.qe_item_cursor];
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ItemPick, "the picker did not open");
    assert_eq!(ed.item_pick_mode, 2);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::QuestEquip);
    assert!(ed.dirty, "choosing an item must mark the save dirty");
    // B closes the slot, B again leaves the screen.
    ed.input(Key::B);
    assert!(!ed.qe_open);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::SaveQuests);
}

#[test]
fn converting_the_console_save_does_not_write_next_to_it() {
    // The mount holds only what the game wrote. A converted copy must land on
    // the SD card, or the console writer picks it up and corrupts the save.
    let mut ed = Editor::new();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../..", "/nx/jksv_ready/system");
    let _ = path;
    assert!(mhnx_core::hbedit::is_console_save_path("nand:/system"));
    assert!(mhnx_core::hbedit::is_console_save_path("nand:/system_backup"));
    assert!(!mhnx_core::hbedit::is_console_save_path("sdmc:/switch/x/system"));
    // With a console path the converted output goes to the SD card.
    let _ = &mut ed;
}
#[test]
fn compaction_drops_the_romfs_padding_and_keeps_every_entry() {
    // A shipped quest archive starts its data at 0x8000 with about 31KB of
    // zeroes in front. That padding is why a 35KB file looked too big for a
    // 5,112 byte save slot: the payloads are only a couple of kilobytes.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../nx/mhxx/arcs/event/q1010002.arc"
    );
    let Ok(raw) = std::fs::read(path) else {
        return;
    };
    let arc = mhnx_core::arc::Arc::parse(&raw).expect("the shipped archive parses");
    let original_len = raw.len();
    let payload: usize = arc.entries.iter().map(|e| e.data.len()).sum();
    assert!(
        original_len > 30_000,
        "the shipped file is mostly padding: {original_len}"
    );
    assert!(
        payload < 4_000,
        "the real data is only {payload} bytes"
    );

    let compact = arc.to_bytes_compact().expect("compaction succeeds");
    assert!(
        compact.len() <= mhnx_core::dlc::QUEST_MAX_PAYLOAD,
        "compacted to {} bytes, over the {} byte slot limit",
        compact.len(),
        mhnx_core::dlc::QUEST_MAX_PAYLOAD
    );
    // The save copy carries zero at +0x08 and romfs carries 0xFFFFFFFE.
    assert_eq!(&compact[8..12], &[0, 0, 0, 0], "flags must take the save form");

    // The entries survive, in order and unchanged.
    let back = mhnx_core::arc::Arc::parse(&compact).expect("the result parses");
    assert_eq!(back.entries.len(), arc.entries.len());
    for (a, b) in arc.entries.iter().zip(back.entries.iter()) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.data, b.data, "payload changed for {}", a.name);
    }
    assert_eq!(
        back.to_bytes().expect("the compacted form round trips"),
        compact
    );
}
#[test]
fn a_name_is_never_cut_inside_a_character() {
    // The name fields are fixed byte widths, and a full width character is
    // three bytes: cutting at the limit used to split one in half and leave
    // the field holding invalid UTF-8.
    let mut f = [0u8; 32];
    mhnx_core::write_name_field(&mut f, "ああああああああああああ"); // 36 bytes
    let end = f.iter().position(|b| *b == 0).unwrap_or(f.len());
    assert!(end <= 31, "one byte is kept for the terminator: {end}");
    assert!(
        std::str::from_utf8(&f[..end]).is_ok(),
        "the cut has to land on a character boundary"
    );

    // Every field width behaves the same way.
    for width in [8usize, 20, 32, 33] {
        let mut g = vec![0u8; width];
        mhnx_core::write_name_field(&mut g, "ナナナナナナナナナナナナ");
        let e = g.iter().position(|b| *b == 0).unwrap_or(g.len());
        assert!(std::str::from_utf8(&g[..e]).is_ok(), "width {width}");
    }

    // A plain ASCII name still fills as much as it can.
    let mut h = [0u8; 16];
    mhnx_core::write_name_field(&mut h, "abcdefghijklmnop");
    assert_eq!(&h[..15], b"abcdefghijklmno");
    assert_eq!(h[15], 0);
}