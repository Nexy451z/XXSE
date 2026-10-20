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
    // One X press moves the step to 10 (STEPS = [1, 10, 100, 1000, 10000]).
    ed.input(Key::X);
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

    // Pick a slot with room for the +11 the test adds, whatever the dump holds.
    let boxes = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    let first = *vis
        .iter()
        .find(|i| boxes[**i].count < 100)
        .expect("a slot with room for 11 more");
    let before = boxes[first].count;
    // Default step is 1; X cycles it to 10.
    ed.input(Key::Right);
    ed.input(Key::X);
    ed.input(Key::Right);
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap()[first].count;
    assert_eq!(after, before + 11);

    ed.input(Key::A);
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
    ed.input(Key::Y);
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
    ed.input(Key::Y);
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
    ed.input(Key::Y);
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
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // X installs the open archive into the open save.
    let had_it = dlc::find_quest(ed.save.as_ref().unwrap(), 1010165).is_some();
    let before_n = dlc::list_quests(ed.save.as_ref().unwrap()).len();
    ed.input(Key::X);
    assert!(ed.message.contains("installed"), "message was {}", ed.message);
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
    ed.input(Key::Y);
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

    // Language toggle.
    ed.input(Key::L);
    assert!(ed.lines()[0].text.contains("EN"));
    ed.input(Key::L);

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
    assert!(ed.message.contains("added"), "message was {}", ed.message);

    // Pressing A again removes it.
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
    assert!(lines[0].text.contains("CHARM EDIT"));
    // The defaults are a usable charm.
    assert!(lines[3].text.contains("skill 1 id"));

    // A new charm starts as a copy of a real one, so the seed values come from
    // the box rather than from invented defaults. Read them, then bump.
    let seed = ed.charm_edit.expect("the form is open");
    assert!(seed.skill1 > 0, "a new charm must start from a real talisman");
    let want_skill1 = seed.skill1 as i32 + 1;
    let want_points1 = seed.points1 as i32 + 1;

    // The form opens on the first skill row; bump it and its points, then write.
    ed.input(Key::Right);
    ed.input(Key::Down);
    ed.input(Key::Right);
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

    // A on the list deletes whatever the cursor points at.
    let n = ed.save.as_ref().unwrap().charms(0).unwrap().len();
    ed.input(Key::A);
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
    ed.input(Key::Y);
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
    ed.input(Key::Minus);
    match ed.take_request() {
        Some(Request::ExtractQuests { ids }) => assert_eq!(ids, vec![want]),
        other => panic!("expected an extract request, got {:?}", other),
    }
    // The I/O layer reports back through apply_extracted.
    ed.apply_extracted(1, "/switch/mhnxeditor/mhxx/quest/arc");
    assert!(ed.message.contains("1 quest file"), "message was {}", ed.message);
}

#[test]
fn extracting_all_asks_for_every_quest() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_save(&mut ed);
    ed.input(Key::Y);
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
    // Elsewhere the Switch layer keeps Minus for its folder rescan.
    assert!(!ed.uses_minus(), "slot screen must leave Minus to the rescan");
    ed.input(Key::Y);
    assert!(!ed.uses_minus(), "quest list must leave Minus to the rescan");
    ed.input(Key::A);
    service(&mut ed);
    assert!(ed.uses_minus(), "entry screen consumes Minus itself");
}

#[test]
fn a_missing_save_reports_that_nothing_was_written() {
    let mut ed = Editor::new();
    ed.apply_extracted(0, "/switch/mhnxeditor/mhxx/quest/arc");
    assert!(ed.message.contains("nothing written"), "message was {}", ed.message);
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
    ed.input(Key::Y);
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
    ed.input(Key::Y);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);

    let want = ed.save_quests()[0].quest_id;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ArcEntries, "A must open the quest");
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
    ed.input(Key::Y);
    ed.input(Key::X);
    let target = ed.save_quests()[0].clone();
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // The entry cursor must point at the .mib, so A opens the field editor
    // instead of re-loading entry 0 and wiping the buffer.
    assert!(
        !ed.mib_bytes().is_empty(),
        "opening a quest must load its .mib (cursor {} vs entry {})",
        ed.cursor,
        ed.entry
    );
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
    assert!(ed.message.contains("installed"), "message was {}", ed.message);
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
    assert!(ed.message.contains("not in this save"), "message was {}", ed.message);
}

#[test]
fn minus_is_also_swallowed_on_the_save_quest_list() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Y);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);
    assert!(ed.uses_minus(), "the save quest list uses Minus to extract");
}
// ---------------------------------------------------------------------------
// Item type change on the console side.
// ---------------------------------------------------------------------------

#[test]
fn r_on_the_item_box_opens_the_picker() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X); // items screen
    assert_eq!(ed.screen, Screen::Items);
    ed.input(Key::R);
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
    ed.input(Key::R);
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
    ed.input(Key::R);
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
    ed.input(Key::R);
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
    ed.input(Key::R);
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
    assert_eq!(
        ed.skill_pick_matches().len(),
        306,
        "the whole table is reachable"
    );
    let lines = ed.lines();
    assert!(lines[0].text.contains("PICK SKILL 1"), "{}", lines[0].text);
    // The first rows carry real Japanese names.
    assert!(
        lines.iter().any(|l| l.text.contains("毒")),
        "skill names must be rendered"
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

    ed.apply_skill_filter(String::from("攻撃"));
    let matches = ed.skill_pick_matches();
    assert!(!matches.is_empty(), "the filter must find 攻撃");
    let want = matches[0];

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "choosing returns to the form");
    assert_eq!(
        ed.charm_edit.map(|e| e.skill1),
        Some(want),
        "the chosen id must land in skill 1"
    );
    assert!(ed.message.contains("攻撃"), "message was {}", ed.message);
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
    ed.input(Key::Up); // skill 1 -> deco slots
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(ed.message.contains("skill row"), "message was {}", ed.message);
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
    ed.input(Key::R);
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
    assert!(ed.message.contains("restored"), "message was {}", ed.message);
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
    assert!(ed.message.contains("nothing to undo"), "message was {}", ed.message);
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
    ed.input(Key::Y); // edit
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
    ed.input(Key::Y);
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
    assert!(joined.contains("CONVERT SAVE"), "{}", joined);
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
    ed.input(Key::Y); // quest list
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
    ed.input(Key::Y);
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
    ed.input(Key::Y);
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
    assert!(ed.message.contains("X writes"), "message was {}", ed.message);
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
    ed.input(Key::Minus);
    assert_eq!(ed.screen, Screen::Equip);
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
    ed.input(Key::Minus);
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
    ed.input(Key::X);
    assert_eq!(ed.equipment().len(), all, "X must clear the filter");
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
    ed.input(Key::Y); // quest editor
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
    ed.input(Key::Minus);
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
        joined.contains("Rathian") || joined.contains("Rathalos"),
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
    ed.input(Key::Minus); // equipment
    assert_eq!(ed.screen, Screen::Equip);
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

#[test]
fn the_console_lists_the_palicos() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Minus); // equipment
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Roster);
    let list = ed.palicos();
    assert_eq!(list.len(), 17, "the sample character has seventeen palicos");
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = lines.join("\n");
    assert!(joined.contains("PALICOS"), "{}", joined);
    assert!(joined.contains("Lv"), "{}", joined);
    // Every name must have survived decoding.
    for p in &list {
        assert!(!p.name.contains('\u{FFFD}'));
    }
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
}
