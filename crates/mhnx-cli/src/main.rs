//! mhnx - command line access to MHXX save and quest data.
//!
//! Useful for preparing an SD card, for scripting, and for checking a save
//! without starting the GUI.

use std::path::PathBuf;
use std::process::ExitCode;

use mhnx_core::arc::Arc;
use mhnx_core::dlc;
use mhnx_core::quest::{Gmd, Mib};
use mhnx_core::save::{self, Save, SaveKind};

fn usage() {
    println!(
        r#"mhnx - MHXX save / quest tool

USAGE:
  mhnx info <file>                     show what a file is (save or .arc)
  mhnx save-info <system>              detailed character dump
  mhnx to-switch <3ds-system> <dir>    write <dir>/system and <dir>/system_backup
  mhnx to-3ds <switch-system> <out>    strip the 36 byte container header
  mhnx arc-list <file.arc>             list the entries in a quest archive
  mhnx arc-mib <file.arc>              dump the .mib fields of a quest
  mhnx arc-gmd <file.arc>              dump the quest text
  mhnx arc-repack <in.arc> <out.arc>   decompress and recompress an archive
  mhnx install <system> <file.arc>     install a quest into the save's DLC slots
  mhnx dlc-list <system>               list installed download quests
  mhnx dlc-extract <system> <outdir> [id...]   write the quest arcs out of the save
  mhnx dlc-export <system> <outdir>    same, but every quest in the save
  mhnx check <file.arc>                validate a quest (rewards, conditions, ids)
  mhnx check-save <system>             validate a save
  mhnx fix-oneplayer <in.arc> <out.arc>  clear the forced "1 player only" flag
  mhnx options <system>                show the global options
  mhnx palicoes <system>               list the 50 download Palicoes
  mhnx charms <system> [slot]          list the charms of a character (slot 1-3)
  mhnx skills                          list every skill name and its id
  mhnx slot-export <system> <slot> <out>   write one character slot (.saveslot)
  mhnx slot-import <system> <slot> <in>    read one character slot back
  mhnx items <system> [slot]           item box summary with item names
  mhnx cats                            list the downloadable Palicoes and their ids
  mhnx cat-add <system> <jp|en> <id>   install a download Palico
  mhnx cat-del <system> <index>        remove a download Palico from a slot
"#
    );
}

fn read(p: &str) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| format!("{}: {}", p, e))
}

fn kind_name(k: SaveKind) -> &'static str {
    match k {
        SaveKind::ThreeDs => "3DS MHXX",
        SaveKind::Switch => "Switch MHXX",
        SaveKind::Mhgu => "Switch MHGU",
    }
}

fn cmd_info(path: &str) -> Result<(), String> {
    let b = read(path)?;
    if b.len() >= 4 && &b[0..4] == b"ARC\0" {
        let a = Arc::parse(&b).map_err(|e| e.to_string())?;
        println!("quest archive {} ({} bytes, version {})", path, b.len(), a.version);
        println!("{} entries", a.entries.len());
        return Ok(());
    }
    let s = Save::from_bytes(&b).map_err(|e| e.to_string())?;
    println!("{}: {}", path, kind_name(s.kind()));
    println!("  file size  {}", b.len());
    println!("  body size  {}", s.body().len());
    println!("  slot bases 0x{:X} 0x{:X} 0x{:X}", s.slot_bases()[0], s.slot_bases()[1], s.slot_bases()[2]);
    println!("  dlc section 0x{:X}", s.dlc_section_offset());
    if s.kind().has_header() {
        println!(
            "  header +0x14 0x{:08X} (expected 0x{:08X} for 'system')",
            u32::from_le_bytes([s.header()[0x14], s.header()[0x15], s.header()[0x16], s.header()[0x17]]),
            mhnx_core::name_hash(b"system")
        );
    }
    Ok(())
}

fn cmd_save_info(path: &str) -> Result<(), String> {
    let b = read(path)?;
    let s = Save::from_bytes(&b).map_err(|e| e.to_string())?;
    println!("{}: {} ({} bytes)", path, kind_name(s.kind()), b.len());
    for i in 0..3 {
        let used = s.slot_used(i);
        let c = s.read_character(i).ok_or("no slot base")?;
        println!(
            "\nslot {} {}",
            i + 1,
            if used { "[used]" } else { "[unused]" }
        );
        println!("  name        {}", c.name);
        println!("  play time   {} ({}:{:02}:{:02})", c.play_time, c.play_time / 3600, (c.play_time % 3600) / 60, c.play_time % 60);
        println!("  zenny       {}", c.funds);
        println!("  HR          {}  (HRP {})", c.hunter_rank, c.hr_points);
        println!("  academy     {}", c.academy_points);
        println!("  villages    {:?}", c.village_points);
        println!("  look        voice {} eye {} cloth {} gender {} hair {} face {} feature {}", c.voice, c.eye_color, c.clothing, c.gender, c.hairstyle, c.face, c.features);
        let items = s.read_item_box(i).ok_or("no item box")?;
        let used_items = items.iter().filter(|x| x.id != 0).count();
        println!("  item box    {} non-empty slots", used_items);
    }
    Ok(())
}

fn cmd_to_switch(src: &str, dir: &str) -> Result<(), String> {
    let b = read(src)?;
    let s = Save::from_bytes(&b).map_err(|e| e.to_string())?;
    if s.kind() != SaveKind::ThreeDs {
        return Err(format!("{} is already a Switch save", src));
    }
    let d = PathBuf::from(dir);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let body = s.body();
    for name in ["system", "system_backup"] {
        let mut hdr = save::switch_header(name, body.len()).to_vec();
        hdr.extend_from_slice(body);
        let out = d.join(name);
        std::fs::write(&out, &hdr).map_err(|e| e.to_string())?;
        println!("wrote {} ({} bytes)", out.display(), hdr.len());
    }
    Ok(())
}

fn cmd_to_3ds(src: &str, out: &str) -> Result<(), String> {
    let b = read(src)?;
    let s = Save::from_bytes(&b).map_err(|e| e.to_string())?;
    std::fs::write(out, s.to_3ds_bytes()).map_err(|e| e.to_string())?;
    println!("wrote {} ({} bytes)", out, s.body().len());
    Ok(())
}

fn cmd_arc_list(path: &str) -> Result<(), String> {
    let a = Arc::parse(&read(path)?).map_err(|e| e.to_string())?;
    println!("{}: {} entries, version {}", path, a.entries.len(), a.version);
    for (i, e) in a.entries.iter().enumerate() {
        println!(
            "  {:>3}  {:<50} {:>8} B  hash 0x{:08X}",
            i,
            e.file_name(),
            e.data.len(),
            e.type_hash
        );
    }
    Ok(())
}

fn cmd_arc_mib(path: &str) -> Result<(), String> {
    let a = Arc::parse(&read(path)?).map_err(|e| e.to_string())?;
    let m = Mib::from_bytes(&a.by_ext("mib").ok_or("no .mib entry")?.data).map_err(|e| e.to_string())?;
    println!("quest id {} ({})", m.quest_id(), mhnx_core::quest::decode_quest_id(m.quest_id()).describe());
    for f in mhnx_core::questfields::MIB_FIELDS {
        let value = match f.kind {
            mhnx_core::questfields::FieldKind::Bytes => m.raw[f.offset..(f.offset + f.size).min(m.raw.len())]
                .iter()
                .map(|x| format!("{:02X}", x))
                .collect::<Vec<_>>()
                .join(" "),
            _ => {
                let v = mhnx_core::questfields::read_u32(&m.raw, f.offset, f.kind).unwrap_or(0);
                match f.enum_name.and_then(mhnx_core::questfields::enum_table) {
                    Some(t) => format!("{} ({})", t.get(v as usize).copied().unwrap_or("?"), v),
                    None => format!("{}", v),
                }
            }
        };
        println!("  0x{:02X} {:<18} {:<24} {}", f.offset, f.key, value, f.ja);
    }
    Ok(())
}

fn cmd_arc_gmd(path: &str) -> Result<(), String> {
    let a = Arc::parse(&read(path)?).map_err(|e| e.to_string())?;
    let g = Gmd::from_bytes(&a.by_ext("gmd").ok_or("no .gmd entry")?.data).map_err(|e| e.to_string())?;
    println!("key {}", g.key);
    for (i, s) in g.strings.iter().enumerate() {
        println!("  {} : {}", Gmd::LABELS.get(i).copied().unwrap_or("other"), s);
    }
    Ok(())
}

fn cmd_arc_repack(src: &str, out: &str) -> Result<(), String> {
    let a = Arc::parse(&read(src)?).map_err(|e| e.to_string())?;
    let bytes = a.to_bytes().map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| e.to_string())?;
    println!("wrote {} ({} bytes, was {})", out, bytes.len(), std::fs::metadata(src).map(|m| m.len()).unwrap_or(0));
    // Sanity: the output must parse back to the same content.
    let back = Arc::parse(&bytes).map_err(|e| e.to_string())?;
    for (x, y) in a.entries.iter().zip(back.entries.iter()) {
        if x.data != y.data || x.name != y.name {
            return Err(format!("round trip mismatch on {}", x.name));
        }
    }
    println!("round trip verified for {} entries", back.entries.len());
    Ok(())
}

fn cmd_install(save_path: &str, arc_path: &str) -> Result<(), String> {
    let mut s = Save::from_bytes(&read(save_path)?).map_err(|e| e.to_string())?;
    let raw = read(arc_path)?;
    let a = Arc::parse(&raw).map_err(|e| e.to_string())?;
    let id = a
        .by_ext("mib")
        .ok_or("archive has no .mib")?
        .data
        .get(0x0C..0x10)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or("short .mib")?;
    let slot = dlc::install_quest(&mut s, id as i32, &raw).map_err(|e| e.to_string())?;
    let out = s.to_bytes();
    std::fs::write(save_path, &out).map_err(|e| e.to_string())?;
    println!("installed quest {} into slot {} of {}", id, slot, save_path);
    println!("NOTE: write system_backup too - the game keeps the pair in sync");
    Ok(())
}

fn severity_tag(s: mhnx_core::check::Severity) -> &'static str {
    use mhnx_core::check::Severity::*;
    match s {
        Error => "ERROR",
        Warn => "WARN ",
        Info => "info ",
    }
}

/// Show the guild card and the arena records of one character.
fn cmd_guild_card(path: &str, slot: Option<usize>) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let slot = match slot {
        Some(n) if (1..=3).contains(&n) => n - 1,
        Some(_) => return Err(String::from("slot must be 1, 2 or 3")),
        None => 0,
    };
    let hr = mhnx_core::guildcard::hunter_rank(&s, slot).unwrap_or(0);
    let time = mhnx_core::guildcard::play_time(&s, slot).unwrap_or(0);
    let id = mhnx_core::guildcard::card_id(&s, slot).unwrap_or([0; 8]);
    let comment = mhnx_core::guildcard::comment(&s, slot).unwrap_or_default();
    println!("slot {} guild card", slot + 1);
    println!("  hunter rank : {}", hr);
    println!("  card time   : {}s ({}:{:02}:{:02})", time, time / 3600, (time % 3600) / 60, time % 60);
    println!("  card id     : {}", id.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(""));
    println!("  comment     : {}", comment);

    let log = mhnx_core::guildcard::arena(&s, slot);
    let mut any = false;
    for a in 0..mhnx_core::guildcard::ARENA_COUNT {
        let rows: Vec<_> = log.iter().filter(|e| e.arena == a && !e.is_empty()).collect();
        if rows.is_empty() {
            continue;
        }
        any = true;
        print!("  arena {:>2} :", a);
        for e in rows {
            let (m, sec, cs) = e.as_clock();
            print!("  {}:{:02}.{:02}", m, sec, cs);
        }
        println!();
    }
    if !any {
        println!("  arena       : no records");
    }
    Ok(())
}

/// Show the monster hunt, capture and size records of one character.
/// The slot argument is 1, 2 or 3, matching every other command here.
fn cmd_hunts(path: &str, wanted: Option<usize>) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let slot = match wanted {
        Some(n) => {
            if !(1..=3).contains(&n) {
                return Err(String::from("slot must be 1, 2 or 3"));
            }
            n - 1
        }
        None => 0,
    };
    let Some(hunt) = s.hunt_counts(slot) else {
        return Err(format!("slot {} has no hunt table", slot + 1));
    };
    let cap = s.capture_counts(slot).unwrap_or_default();
    let size = s.monster_sizes(slot).unwrap_or_default();
    let names = &mhnx_core::gamedata::MONSTER_HUNT_NAMES;
    println!("slot {}: {} monsters", slot + 1, hunt.len());
    println!(
        "{:>4}  {:<26} {:>6} {:>6} {:>10}",
        "id", "monster", "slay", "capture", "size"
    );
    for i in 0..hunt.len() {
        if hunt[i] == 0 && cap.get(i).copied().unwrap_or(0) == 0 {
            continue;
        }
        let sz = size
            .get(i)
            .map(|(a, b)| {
                if *a == 0 && *b == 0 {
                    String::from("-")
                } else {
                    format!("{}..{}", a, b)
                }
            })
            .unwrap_or_default();
        println!(
            "{:>4}  {:<26} {:>6} {:>6} {:>10}",
            i,
            names.get(i).copied().unwrap_or("?"),
            hunt[i],
            cap.get(i).copied().unwrap_or(0),
            sz
        );
    }
    Ok(())
}

/// List the supply and ammo items inside a quest-equipment slot.
fn cmd_equip_items(save_path: &str, slot: usize) -> Result<(), String> {
    let s = Save::from_bytes(&read(save_path)?).map_err(|e| e.to_string())?;
    let entries = dlc::quest_equipment(&s);
    let Some(e) = entries.get(slot) else {
        return Err(format!("slot {} does not exist", slot));
    };
    if !e.occupied {
        return Err(format!("slot {} is empty", slot));
    }
    println!("slot {} -> quest {}", slot, e.quest_id);
    for it in dlc::quest_equipment_items(&s, slot) {
        if it.item == 0 {
            continue;
        }
        println!(
            "  rec {} {}[{:>2}]  item {:>5} x{}",
            it.record,
            match it.list {
                dlc::QuestEquipList::Supply => "supply",
                dlc::QuestEquipList::Ammo => "ammo  ",
            },
            it.index,
            it.item,
            it.count
        );
    }
    Ok(())
}

/// Rewrite one supply or ammo entry of a quest-equipment slot.
fn cmd_equip_set(
    save_path: &str,
    slot: usize,
    record: usize,
    list: &str,
    index: usize,
    item: u16,
    count: u8,
    out: &str,
) -> Result<(), String> {
    let mut s = Save::from_bytes(&read(save_path)?).map_err(|e| e.to_string())?;
    let which = match list {
        "supply" => dlc::QuestEquipList::Supply,
        "ammo" => dlc::QuestEquipList::Ammo,
        other => return Err(format!("unknown list {}", other)),
    };
    dlc::set_quest_equipment_item(&mut s, slot, record, which, index, item, count)
        .map_err(|e| e.to_string())?;
    std::fs::write(out, s.to_bytes()).map_err(|e| e.to_string())?;
    println!(
        "slot {} rec {} {}[{}] = item {} x{}, wrote {}",
        slot, record, list, index, item, count, out
    );
    Ok(())
}

/// Add a quest the save does not list into a free slot.
fn cmd_add_quest(save_path: &str, arc_path: &str, out: &str) -> Result<(), String> {
    let mut s = Save::from_bytes(&read(save_path)?).map_err(|e| e.to_string())?;
    let raw = read(arc_path)?;
    let a = Arc::parse(&raw).map_err(|e| e.to_string())?;
    let id = a
        .by_ext("mib")
        .and_then(|e| e.data.get(0x0C..0x10))
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or("archive has no .mib")? as i32;
    let slot = dlc::add_quest(&mut s, id, &raw).map_err(|e| e.to_string())?;
    let bit = dlc::bit_for_quest(id).unwrap_or(0);
    std::fs::write(out, s.to_bytes()).map_err(|e| e.to_string())?;
    println!("added quest {} to slot {} (mask bit {}), wrote {}", id, slot, bit, out);
    println!("the input save was not modified");
    Ok(())
}

/// Dump the strings of a standalone .gmd file.
fn cmd_gmd_dump(path: &str, limit: usize) -> Result<(), String> {
    let g = mhnx_core::quest::Gmd::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    println!("{}: key {:?}, {} string(s)", path, g.key, g.strings.len());
    for (i, s) in g.strings.iter().enumerate().take(limit) {
        let one = s.replace('\n', " / ");
        println!("  {:>4}  {}", i, one);
    }
    Ok(())
}

/// List the 45 quest-equipment entries and which quest each belongs to.
fn cmd_equip_list(path: &str) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let entries = dlc::quest_equipment(&s);
    let mut used = 0usize;
    println!("{} quest-equipment entries", entries.len());
    for e in &entries {
        if e.occupied {
            used += 1;
            println!("  [{:>2}] quest {}", e.index, e.quest_id);
        } else {
            println!("  [{:>2}] empty", e.index);
        }
    }
    println!("{}/{} used", used, entries.len());
    Ok(())
}

/// Dump one quest-equipment entry, or all of them, to files.
fn cmd_equip_export(path: &str, dir: &str, only: Option<usize>) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut written = 0usize;
    for i in 0..dlc::EQUIP_SLOTS {
        if let Some(want) = only {
            if want != i {
                continue;
            }
        }
        let Some(bytes) = dlc::quest_equipment_bytes(&s, i) else {
            continue;
        };
        if bytes.iter().all(|b| *b == 0) {
            continue;
        }
        let id = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let out = format!("{}/equip_{:02}_{}.bin", dir, i, id);
        std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
        println!("wrote {}", out);
        written += 1;
    }
    println!("{} file(s)", written);
    Ok(())
}

/// Report every talisman whose skills or points the game would reject.
fn cmd_charm_check(path: &str) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let mut total = 0usize;
    let mut bad = 0usize;
    for slot in 0..3 {
        let Some(box_) = s.equipment_box(slot) else { continue };
        for i in 0..mhnx_core::charm::CHARM_COUNT {
            let o = i * mhnx_core::charm::CHARM_RECORD;
            let Some(rec) = box_.get(o..o + mhnx_core::charm::CHARM_RECORD) else {
                continue;
            };
            if !mhnx_core::charm::is_talisman(rec) {
                continue;
            }
            total += 1;
            let ty = rec[0x02];
            if ty == 0 {
                println!("slot {} #{}: type 0, which the game does not define", slot + 1, i);
                bad += 1;
                continue;
            }
            for (label, sid, pts, second) in [
                ("skill 1", rec[0x0C], rec[0x0E] as i8, false),
                ("skill 2", rec[0x0D], rec[0x0F] as i8, true),
            ] {
                if sid == 0 {
                    continue;
                }
                let Some(r) = mhnx_core::charm::allowed_points(ty, sid, second) else {
                    continue;
                };
                let name = mhnx_core::skills::skill_name(sid).unwrap_or("?");
                if r.min == 0 && r.max == 0 {
                    println!(
                        "slot {} #{}: {} {} cannot go on type 0x{:02X}",
                        slot + 1,
                        i,
                        label,
                        name,
                        ty
                    );
                    bad += 1;
                } else if pts < r.min || pts > r.max {
                    println!(
                        "slot {} #{}: {} {} is {:+} but the range is {:+}..{:+}",
                        slot + 1,
                        i,
                        label,
                        name,
                        pts,
                        r.min,
                        r.max
                    );
                    bad += 1;
                }
            }
        }
    }
    println!("{} talismans checked, {} problem(s)", total, bad);
    Ok(())
}

fn cmd_check(path: &str) -> Result<(), String> {
    let a = Arc::parse(&read(path)?).map_err(|e| e.to_string())?;
    let findings = mhnx_core::check::check_quest(&a);
    println!("{}: {} finding(s)", path, findings.len());
    for f in &findings {
        let loc = match &f.location {
            Some((entry, off)) => format!("  [{} +0x{:X}]", entry.rsplit('\\').next().unwrap_or(entry), off),
            None => String::new(),
        };
        println!("  {} {:<20} {}{}", severity_tag(f.severity), f.code, f.message, loc);
        if f.fixable {
            println!("        -> mhnx fix-oneplayer <in> <out> で直せます");
        }
    }
    Ok(())
}

fn cmd_check_save(path: &str) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    for f in mhnx_core::check::check_save(&s) {
        println!("  {} {:<20} {}", severity_tag(f.severity), f.code, f.message);
    }
    let dlc = dlc::list_quests(&s);
    if !dlc.is_empty() {
        println!("  ({} download quest(s) installed)", dlc.len());
    }
    Ok(())
}

fn cmd_fix_oneplayer(src: &str, out: &str) -> Result<(), String> {
    let mut a = Arc::parse(&read(src)?).map_err(|e| e.to_string())?;
    match mhnx_core::check::clear_one_player_only(&mut a, 0x00) {
        None => Err(String::from("no .mib entry in this archive")),
        Some(before) => {
            if before != mhnx_core::check::ONE_PLAYER_ONLY {
                println!("note: the flag was 0x{:02X}, not the usual 0x2F - cleared anyway", before);
            }
            let bytes = a.to_bytes().map_err(|e| e.to_string())?;
            std::fs::write(out, &bytes).map_err(|e| e.to_string())?;
            println!(
                "cleared the 1-player-only flag (0x{:02X} -> 0x00) and wrote {}",
                before, out
            );
            println!("remember: do not reopen this file in Quedixx, it would set it back");
            // Re-check.
            let back = Arc::parse(&bytes).map_err(|e| e.to_string())?;
            let left = mhnx_core::check::check_quest(&back)
                .into_iter()
                .filter(|f| f.code == "one-player-only")
                .count();
            if left == 0 {
                println!("verified: no one-player-only finding remains");
            }
            Ok(())
        }
    }
}

/// Accept both 100 and 0x64.
fn parse_id(s: &str) -> Result<u8, String> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).map_err(|_| format!("bad id: {}", s))
    } else {
        s.parse::<u8>().map_err(|_| format!("bad id: {}", s))
    }
}

fn cmd_skills() -> Result<(), String> {
    println!("{} skills", mhnx_core::skills::SKILL_NAMES.len());
    for (i, n) in mhnx_core::skills::SKILL_NAMES.iter().enumerate() {
        println!("  {:>3}  {}", i + 1, n);
    }
    Ok(())
}

fn cmd_slot_export(save: &str, slot: usize, out: &str) -> Result<(), String> {
    if !(1..=3).contains(&slot) {
        return Err(String::from("slot must be 1, 2 or 3"));
    }
    let s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let data = s.export_slot(slot - 1).ok_or("slot has no base offset")?;
    let name = s
        .read_character(slot - 1)
        .map(|c| c.name)
        .unwrap_or_default();
    std::fs::write(out, &data).map_err(|e| e.to_string())?;
    println!("wrote {} ({} bytes) for character {}", out, data.len(), name);
    println!("NOTE: this file contains the whole slot - keep it private");
    Ok(())
}

fn cmd_slot_import(save: &str, slot: usize, inp: &str) -> Result<(), String> {
    if !(1..=3).contains(&slot) {
        return Err(String::from("slot must be 1, 2 or 3"));
    }
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let data = read(inp)?;
    s.import_slot(slot - 1, &data).map_err(|e| e.to_string())?;
    let name = s
        .read_character(slot - 1)
        .map(|c| c.name)
        .unwrap_or_default();
    let out = s.to_bytes();
    std::fs::write(save, &out).map_err(|e| e.to_string())?;
    println!(
        "imported {} bytes into slot {} (character {}) of {}",
        data.len(),
        slot,
        name,
        save
    );
    println!("NOTE: write system_backup too - the game keeps the pair in sync");
    Ok(())
}

/// Item name from the shipped table, falling back to the raw id.
fn item_name(id: u16) -> &'static str {
    mhnx_core::gamedata::ITEM_NAME_LIST
        .get(id as usize)
        .copied()
        .unwrap_or("?")
}

fn cmd_items(path: &str, slot: Option<usize>) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let slots: Vec<usize> = match slot {
        Some(n) => {
            if !(1..=3).contains(&n) {
                return Err(String::from("slot must be 1, 2 or 3"));
            }
            vec![n - 1]
        }
        None => (0..3).filter(|i| s.slot_used(*i)).collect(),
    };
    for i in slots {
        let items = s.read_item_box(i).ok_or("no item box")?;
        let used = items.iter().filter(|x| x.id != 0).count();
        let total: u32 = items.iter().map(|x| x.count as u32).sum();
        let kinds = {
            let mut v: Vec<u16> = items.iter().filter(|x| x.id != 0).map(|x| x.id).collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        println!(
            "slot {}: {} / {} slots used, {} items of {} kinds",
            i + 1,
            used,
            mhnx_core::itembox::TOTAL_ITEM_SLOTS,
            total,
            kinds
        );
        let mut v: Vec<_> = items.iter().filter(|x| x.id != 0).collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then(a.id.cmp(&b.id)));
        for it in v.iter().take(10) {
            println!("   #{:<5} id {:<5} x{:<4} {}", it.id, it.id, it.count, item_name(it.id));
        }
    }
    Ok(())
}

fn cmd_cats() -> Result<(), String> {
    use mhnx_core::dlcats::{all_ids, record, record_name, Lang};
    let ids = all_ids();
    println!("{} downloadable Palicoes", ids.len());
    for id in ids {
        let en = record(Lang::En, id).map(record_name);
        let jp = record(Lang::Jp, id).map(record_name);
        println!(
            "  id 0x{:02X}  EN: {:<16} JP: {}",
            id,
            en.unwrap_or_default(),
            jp.unwrap_or_default()
        );
    }
    Ok(())
}

fn cmd_cat_add(save: &str, lang: &str, id: u8) -> Result<(), String> {
    use mhnx_core::dlcats::Lang;
    let lang = match lang {
        "jp" | "JP" => Lang::Jp,
        "en" | "EN" => Lang::En,
        other => return Err(format!("language must be jp or en, got {}", other)),
    };
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = s.install_dlc_palico(lang, id).map_err(|e| e.to_string())?;
    std::fs::write(save, s.to_bytes()).map_err(|e| e.to_string())?;
    let name = s.dlc_palicoes().get(slot).map(|c| c.name.clone()).unwrap_or_default();
    println!("installed {} (id 0x{:02X}) into slot {}", name, id, slot + 1);
    println!("NOTE: write system_backup too");
    Ok(())
}

fn cmd_cat_del(save: &str, index: usize) -> Result<(), String> {
    if !(1..=50).contains(&index) {
        return Err(String::from("index must be 1..50"));
    }
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let name = s
        .dlc_palicoes()
        .get(index - 1)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    if !s.remove_dlc_palico(index - 1) {
        return Err(String::from("could not clear the slot"));
    }
    std::fs::write(save, s.to_bytes()).map_err(|e| e.to_string())?;
    println!("removed slot {} ({})", index, name);
    Ok(())
}

fn cmd_options(path: &str) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let o = s.options();
    println!("options for {}", path);
    println!("  brightness      {}", o.brightness);
    println!("  rumble          {}", o.rumble);
    println!("  disable 3d      {}", o.disable_3d);
    println!("  circle pad pro  {}", o.circle_pad_pro);
    Ok(())
}

fn cmd_palicoes(path: &str) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let cats = s.dlc_palicoes();
    let used = cats.iter().filter(|c| c.id != 0 || !c.name.is_empty()).count();
    println!("download Palicoes: {} / {} slots used", used, cats.len());
    for c in cats.iter().filter(|c| c.id != 0 || !c.name.is_empty()) {
        println!("  #{:<3} id 0x{:02X} ({:<3}) {}", c.index + 1, c.id, c.id, c.name);
    }
    Ok(())
}

fn cmd_charms(path: &str, slot: Option<usize>) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let slots: Vec<usize> = match slot {
        Some(n) => {
            if !(1..=3).contains(&n) {
                return Err(String::from("slot must be 1, 2 or 3"));
            }
            vec![n - 1]
        }
        None => (0..3).filter(|i| s.slot_used(*i)).collect(),
    };
    if slots.is_empty() {
        println!("no character slots in use");
        return Ok(());
    }
    for i in slots {
        let list = s.charms(i).ok_or("no such slot")?;
        println!("slot {}: {} charm(s) of {}", i + 1, list.len(), mhnx_core::charm::CHARM_COUNT);
        for c in &list {
            println!("  #{:<5} {}", c.index, c.summary());
        }
    }
    Ok(())
}

/// Write the quest arcs stored in a save back out as files.
///
/// With no ids every quest is written. Otherwise only the ones named, so a
/// single quest can be pulled out for editing. The payload is copied byte for
/// byte; nothing is re-packed, because the save already holds a finished arc.
fn cmd_dlc_extract(save_path: &str, outdir: &str, ids: &[String]) -> Result<(), String> {
    let s = Save::from_bytes(&read(save_path)?).map_err(|e| e.to_string())?;
    let list = dlc::list_quests(&s);
    if list.is_empty() {
        return Err(String::from(
            "this save has no download quests (or it is an MHGU save)",
        ));
    }

    let wanted: Option<Vec<i32>> = if ids.is_empty() {
        None
    } else {
        let mut v = Vec::new();
        for raw in ids {
            let id = parse_id32(raw)?;
            v.push(id);
        }
        Some(v)
    };

    let dir = std::path::Path::new(outdir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    let mut written = 0usize;
    let mut missing: Vec<i32> = wanted.clone().unwrap_or_default();
    for q in &list {
        if let Some(w) = &wanted {
            if !w.contains(&q.quest_id) {
                continue;
            }
            if let Some(i) = missing.iter().position(|x| *x == q.quest_id) {
                missing.remove(i);
            }
        }
        // A corrupt record reports a size but no payload; skip it loudly.
        if q.data.is_empty() {
            println!("  id {} slot {}: no payload (bad size {})", q.quest_id, q.slot, q.size);
            continue;
        }
        let name = format!("q{:07}.arc", q.quest_id);
        let path = dir.join(&name);
        match std::fs::write(&path, &q.data) {
            Ok(_) => {
                let mut title = String::new();
                if let Ok(a) = Arc::parse(&q.data) {
                    if let Some(g) = a.by_ext("gmd").and_then(|e| Gmd::from_bytes(&e.data).ok()) {
                        title = g.title().unwrap_or("").to_string();
                    }
                }
                println!(
                    "  slot {:>3}  {}  {:>6} B  {}",
                    q.slot, name, q.data.len(), title
                );
                written += 1;
            }
            Err(e) => println!("  id {}: {}", q.quest_id, e),
        }
    }

    println!("wrote {} file(s) to {}", written, outdir);
    if !missing.is_empty() {
        return Err(format!(
            "not in this save: {}",
            missing.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

/// Parse an id written either as 1010165 or 0x1010165.
fn parse_id32(s: &str) -> Result<i32, String> {
    let v = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16)
    } else {
        s.parse::<i64>()
    };
    match v {
        Ok(n) if n > 0 && n <= i32::MAX as i64 => Ok(n as i32),
        _ => Err(format!("bad quest id: {}", s)),
    }
}

fn cmd_dlc_list(path: &str) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let list = dlc::list_quests(&s);
    println!("{} installed download quest(s)", list.len());
    for q in &list {
        print!("  slot {:>3}  id {:<8} {:>6} B", q.slot, q.quest_id, q.size);
        if let Ok(a) = Arc::parse(&q.data) {
            if let Some(g) = a.by_ext("gmd").and_then(|e| Gmd::from_bytes(&e.data).ok()) {
                print!("  {}", g.title().unwrap_or(""));
            }
        }
        println!();
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        return ExitCode::SUCCESS;
    }
    let r = match args[0].as_str() {
        "info" if args.len() == 2 => cmd_info(&args[1]),
        "save-info" if args.len() == 2 => cmd_save_info(&args[1]),
        "to-switch" if args.len() == 3 => cmd_to_switch(&args[1], &args[2]),
        "to-3ds" if args.len() == 3 => cmd_to_3ds(&args[1], &args[2]),
        "arc-list" if args.len() == 2 => cmd_arc_list(&args[1]),
        "arc-mib" if args.len() == 2 => cmd_arc_mib(&args[1]),
        "arc-gmd" if args.len() == 2 => cmd_arc_gmd(&args[1]),
        "arc-repack" if args.len() == 3 => cmd_arc_repack(&args[1], &args[2]),
        "install" if args.len() == 3 => cmd_install(&args[1], &args[2]),
        "dlc-list" if args.len() == 2 => cmd_dlc_list(&args[1]),
        "dlc-extract" if args.len() >= 3 => cmd_dlc_extract(&args[1], &args[2], &args[3..]),
        "dlc-export" if args.len() == 3 => cmd_dlc_extract(&args[1], &args[2], &[]),
        "check" if args.len() == 2 => cmd_check(&args[1]),
        "check-save" if args.len() == 2 => cmd_check_save(&args[1]),
        "fix-oneplayer" if args.len() == 3 => cmd_fix_oneplayer(&args[1], &args[2]),
        "options" if args.len() == 2 => cmd_options(&args[1]),
        "palicoes" if args.len() == 2 => cmd_palicoes(&args[1]),
        "charm-check" if args.len() == 2 => cmd_charm_check(&args[1]),
        "add-quest" if args.len() == 4 => cmd_add_quest(&args[1], &args[2], &args[3]),
        "guild-card" if args.len() == 2 => cmd_guild_card(&args[1], None),
        "guild-card" if args.len() == 3 => cmd_guild_card(&args[1], args[2].parse().ok()),
        "hunts" if args.len() == 2 => cmd_hunts(&args[1], None),
        "hunts" if args.len() == 3 => cmd_hunts(&args[1], args[2].parse().ok()),
        "equip-items" if args.len() == 3 => {
            cmd_equip_items(&args[1], args[2].parse().unwrap_or(0))
        }
        "equip-set" if args.len() == 9 => cmd_equip_set(
            &args[1],
            args[2].parse().unwrap_or(0),
            args[3].parse().unwrap_or(0),
            &args[4],
            args[5].parse().unwrap_or(0),
            args[6].parse().unwrap_or(0),
            args[7].parse().unwrap_or(1),
            &args[8],
        ),
        "gmd-dump" if args.len() == 2 => cmd_gmd_dump(&args[1], 40),
        "gmd-dump" if args.len() == 3 => {
            cmd_gmd_dump(&args[1], args[2].parse().unwrap_or(40))
        }
        "equip-list" if args.len() == 2 => cmd_equip_list(&args[1]),
        "equip-export" if args.len() == 3 => cmd_equip_export(&args[1], &args[2], None),
        "equip-export" if args.len() == 4 => {
            cmd_equip_export(&args[1], &args[2], args[3].parse().ok())
        }
        "charms" if args.len() == 2 => cmd_charms(&args[1], None),
        "charms" if args.len() == 3 => cmd_charms(&args[1], args[2].parse().ok()),
        "skills" if args.len() == 1 => cmd_skills(),
        "slot-export" if args.len() == 4 => {
            cmd_slot_export(&args[1], args[2].parse().unwrap_or(0), &args[3])
        }
        "slot-import" if args.len() == 4 => {
            cmd_slot_import(&args[1], args[2].parse().unwrap_or(0), &args[3])
        }
        "items" if args.len() == 2 => cmd_items(&args[1], None),
        "items" if args.len() == 3 => cmd_items(&args[1], args[2].parse().ok()),
        "cats" if args.len() == 1 => cmd_cats(),
        "cat-add" if args.len() == 4 => match parse_id(&args[3]) {
            Ok(id) => cmd_cat_add(&args[1], &args[2], id),
            Err(e) => Err(e),
        },
        "cat-del" if args.len() == 3 => cmd_cat_del(&args[1], args[2].parse().unwrap_or(0)),
        other => {
            eprintln!("unknown or incomplete command: {}\n", other);
            usage();
            return ExitCode::FAILURE;
        }
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {}", e);
            ExitCode::FAILURE
        }
    }
}