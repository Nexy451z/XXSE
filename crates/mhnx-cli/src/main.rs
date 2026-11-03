//! mhnx - command line access to MHXX save and quest data.
//!
//! Useful for preparing an SD card, for scripting, and for checking a save
//! without starting the GUI.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mhnx_core::arc::Arc;
use mhnx_core::csvout::{self, ApplyReport};
use mhnx_core::dlc;
use mhnx_core::quest::{Gmd, Mib};
use mhnx_core::safety::{self, Fingerprint};
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
  mhnx install <system> <file.arc> [--force]     install a quest into the save's DLC slots
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
  mhnx slot-import <system> <slot> <in> [--force]   read one character slot back
  mhnx items <system> [slot]           item box summary with item names
  mhnx cats                            list the downloadable Palicoes and their ids
  mhnx cat-add <system> <jp|en> <id> [--force]   install a download Palico
  mhnx cat-del <system> <index> [--force]        remove a download Palico from a slot
  mhnx add-quest <system> <in.arc> <out>   add a quest the save does not list yet
  mhnx arc-compact <in.arc> <out.arc>  drop the romfs padding so it fits a slot
  mhnx hunts <system> [slot]           hunt records: slays, captures, sizes
  mhnx guild-card <system> [slot]      guild card: rank, clock, comment, arena
  mhnx mysets <system> [slot]          item sets and equipment sets
  mhnx equip <system> [slot]           equipment box summary
  mhnx quests <system>                 every download quest in the save
  mhnx mib-fields                      every .mib field with its offset and name
  mhnx cat                            list one downloadable Palico catalogue record
  mhnx gmd-dump <file.gmd>             dump the strings of a standalone .gmd
  mhnx arc-gmd-entry <arc> <index>     dump one .gmd entry by index
  mhnx verify <system>                 compare the file with its own round trip, read only
  mhnx snapshot <system>               keep a copy of the save in snapshots/, newest 10 kept
  mhnx snapshots <system>              list the kept snapshots, newest first
  mhnx restore <system> <name> [--force]   put a snapshot back, snapshotting the current save first
  mhnx csv-items <save> <out.csv> [--slot N]        write the item box as CSV
  mhnx csv-equipment <save> <out.csv> [--slot N]    write the equipment box as CSV
  mhnx import-items <save> <in.csv> [--slot N] [--force]   read a CSV back into the item box
  mhnx import-equipment <save> <in.csv> [--slot N] [--force]  read a CSV back into the equipment box
  mhnx pouch <save> [--slot N]         show the 32 pouch slots
  mhnx sets <save> [--slot N]          item sets and equipment sets, summarised

Commands that rewrite a save in place keep a copy next to it as <file>.bak.
Snapshots live in <save's directory>/snapshots/ and are named
<stem>-YYYYMMDD-HHMMSS.sav.

Before any of them writes, two checks run that the reference editor also runs:
  * no emulator is running (yuzu, suyu, Sudachi, Citron, Torzu, Nuzu, Ryujinx,
    Ryubing, Kenji-NX, Pomelo, Citra, Lime3DS, Azahar, Panda3DS, and the forks
    that carry one of those names);
  * the save still has the size, the timestamp and the contents it had when the
    command read it.
A command that fails either check writes nothing and says which process is
running, or what changed and when. --force on the commands above continues
anyway; it never gets past a read-only or a missing file.

The CSV commands and the set commands take --slot N to choose the character:
0, 1 or 2, defaulting to 0, and a slot the save has no character in is an
error rather than an empty report. import-items replaces the whole item box
with the file - row n is record n - so a file that stops early empties the
records after it; when that would drop an item the save holds now the command
stops, lists what would go, and waits for --force. Both imports go through the
same keep-a-copy-and-compare write as every other command here, so <file>.bak
is still left behind and a write that does not read back is undone.

verify never writes the save: it parses the bytes, writes them back in memory
and compares. It exits 0 when the bytes are identical, 1 when they differ.
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

fn cmd_install(save_path: &str, arc_path: &str, force: bool) -> Result<(), String> {
    let loaded = loaded_fingerprint(save_path);
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
    write_in_place(save_path, &out, loaded.as_ref(), force)?;
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

/// Parse a number the user typed, refusing anything that is not one.
///
/// These arguments used to fall back to zero, so a typo silently wrote to slot
/// zero (or item zero) and reported success.
fn num<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, String> {
    text.parse::<T>()
        .map_err(|_| format!("{} must be a number, got {:?}", what, text))
}

/// The file's state at the moment a command read it, when it could be taken.
///
/// Taken before the bytes are read: a change that lands during the read then
/// shows up as a difference at write time instead of being missed. A save whose
/// state cannot be recorded is one the check below cannot clear, so the note
/// printed here is the reason a later run needs --force.
fn loaded_fingerprint(path: &str) -> Option<Fingerprint> {
    match safety::fingerprint(Path::new(path)) {
        Ok(fingerprint) => Some(fingerprint),
        Err(e) => {
            eprintln!("note: {} の読み込み時の状態を記録できません: {}", path, e);
            None
        }
    }
}

/// What the checks could see, kept apart from the decision so the decision can
/// be tested without a filesystem or a running emulator.
struct Observed {
    /// The file as it is now; the sentence to show when it cannot be read.
    current: Result<Fingerprint, String>,
    /// Emulator processes running now; the sentence to show when the process
    /// table cannot be read at all.
    emulators: Result<Vec<String>, String>,
    /// The file is marked read-only, so the write would be refused by the OS.
    read_only: bool,
}

/// Look at the file and at the process table, without judging anything yet.
fn observe(path: &str) -> Observed {
    let target = Path::new(path);
    let current = match safety::fingerprint(target) {
        Ok(fingerprint) => Ok(fingerprint),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("書き込み先が見つかりません: {} ({})", path, e))
        }
        Err(e) => Err(format!("書き込み先の状態を確認できません: {} ({})", path, e)),
    };
    let emulators =
        safety::try_running_emulators().map_err(|e| format!("プロセスの一覧を読めません: {}", e));
    Observed {
        current,
        emulators,
        read_only: safety::is_read_only(target),
    }
}

/// Refuse the write, or let it through, from what was observed.
///
/// There are two classes of reason. --force may override the first: an emulator
/// is running, the save changed after the command read it, or the checks could
/// not look at all. It may not override the second: a read-only file or one
/// that is not there cannot be written correctly, so forcing those would only
/// produce a half-written save.
fn decide(
    path: &str,
    loaded: Option<&Fingerprint>,
    observed: &Observed,
    force: bool,
) -> Result<(), String> {
    let mut soft: Vec<String> = Vec::new();
    let mut hard: Vec<String> = Vec::new();

    match &observed.emulators {
        Err(e) => soft.push(format!("エミュレータが動いていないか確認できません: {}", e)),
        Ok(names) if !names.is_empty() => soft.push(format!(
            "エミュレータが起動しています: {}（開いたままのセーブに書き込むと、ゲーム側の保存で消えます）",
            names.join(", ")
        )),
        Ok(_) => {}
    }

    match &observed.current {
        Err(text) => hard.push(text.clone()),
        Ok(current) => match loaded {
            None => soft.push(format!(
                "読み込み時の状態を記録できていないため、{} が読み込んだ後に変わっていないか確認できません",
                path
            )),
            Some(loaded) if safety::is_stale(loaded, current) => {
                soft.push(format!("{} は読み込んだ後に変わっています:", path));
                soft.extend(safety::changes(loaded, current));
                soft.push(String::from(
                    "別のエディタかエミュレータが書き込んだ可能性があります",
                ));
            }
            Some(_) => {}
        },
    }

    if observed.read_only {
        hard.push(format!(
            "ファイルが読み取り専用です: {}（プロパティで読み取り専用を外してください）",
            path
        ));
    }

    if !hard.is_empty() {
        return Err(blocked(path, &hard, false));
    }
    if !soft.is_empty() {
        if !force {
            return Err(blocked(path, &soft, true));
        }
        // Say which check was overridden, so the run does not look as if it
        // passed them.
        eprintln!(
            "warning: 安全確認を --force で飛ばして書き込みます: {}",
            path
        );
        for line in &soft {
            eprintln!("  {}", line);
        }
    }
    Ok(())
}

/// The sentence a refused write ends with.
fn blocked(path: &str, reasons: &[String], forceable: bool) -> String {
    let mut message = format!("書き込みませんでした: {}", path);
    for reason in reasons {
        message.push_str("\n  ");
        message.push_str(reason);
    }
    if forceable {
        message.push_str(
            "\n理由を解消してからやり直してください（承知のうえで続けるなら --force を付けてください）。",
        );
    } else {
        message.push_str("\n--force を付けても通りません。先にこの状態を直してください。");
    }
    message
}

/// Everything that has to hold before a save is replaced in place.
fn check_write_allowed(
    path: &str,
    loaded: Option<&Fingerprint>,
    force: bool,
) -> Result<(), String> {
    let observed = observe(path);
    decide(path, loaded, &observed, force)
}

/// Write a save back over the file it came from, keeping a copy first.
///
/// These commands take the save path and nothing else, so they rewrite the
/// user's file in place. A tool that can do that without leaving a copy is one
/// bad run away from losing a save; the check in front of the write is about
/// the same loss from the other side, where something else is holding the file
/// or has already changed it.
fn write_in_place(
    path: &str,
    bytes: &[u8],
    loaded: Option<&Fingerprint>,
    force: bool,
) -> Result<(), String> {
    check_write_allowed(path, loaded, force)?;
    replace_save(path, bytes)
}

/// The write itself, for a caller that already ran the checks - the restore
/// command, which has to refuse before its snapshot is taken.
fn replace_save(path: &str, bytes: &[u8]) -> Result<(), String> {
    let bak = format!("{}.bak", path);
    if let Ok(old) = std::fs::read(path) {
        std::fs::write(&bak, &old).map_err(|e| format!("cannot keep a copy at {}: {}", bak, e))?;
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;

    // Read the file back and compare it with what was meant to be written. A
    // save that silently lost bytes on the way to the card is the one failure
    // the user cannot see, so it has to be caught here and undone.
    let back = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            let detail = restore_from_bak(path, &bak);
            return Err(format!(
                "書き込み後の読み戻しに失敗しました: {}: {}{}",
                path, e, detail
            ));
        }
    };
    let trip = mhnx_core::verify::compare(bytes, &back);
    if !trip.is_identical() {
        eprintln!("書き込み後の照合に失敗しました: {}", path);
        if let Some((want, got)) = trip.length_change {
            eprintln!("  長さが {} → {} に変わりました", want, got);
        }
        if !trip.differing.is_empty() {
            eprintln!("  {} バイトが一致しません", trip.differing.len());
            eprintln!("  先頭の差分: {}", trip.examples(8));
        }
        let detail = restore_from_bak(path, &bak);
        return Err(format!(
            "書き込み後の照合に失敗しました: {}（{}）{}",
            path,
            trip.summary(),
            detail
        ));
    }
    println!("kept a copy at {}", bak);
    Ok(())
}

/// Put the pre-write copy back after a failed verification.
///
/// Returns the sentence to append to the error: if the copy itself is gone the
/// caller must say so, because the file on disk is then the damaged one.
fn restore_from_bak(path: &str, bak: &str) -> String {
    match std::fs::read(bak) {
        Ok(old) => match std::fs::write(path, &old) {
            Ok(()) => format!(" / {} を {} から戻しました", path, bak),
            Err(e) => format!(" / {} から戻せませんでした: {}", bak, e),
        },
        Err(e) => format!(" / 控え {} を読めないため戻せません: {}", bak, e),
    }
}

/// Do two paths name the same file on disk?
fn same_file(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Strip the romfs padding so the archive fits a save slot.
fn cmd_arc_compact(src: &str, out: &str) -> Result<(), String> {
    let bytes = read(src)?;
    let arc = Arc::parse(&bytes).map_err(|e| e.to_string())?;
    let compact = arc.to_bytes_compact().map_err(|e| e.to_string())?;
    std::fs::write(out, &compact).map_err(|e| e.to_string())?;
    println!(
        "{} ({} B) -> {} ({} B)",
        src,
        bytes.len(),
        out,
        compact.len()
    );
    Ok(())
}

/// Show the item sets and equipment sets of one character.
fn cmd_mysets(path: &str, slot: Option<usize>) -> Result<(), String> {
    let s = Save::from_bytes(&read(path)?).map_err(|e| e.to_string())?;
    let slot = match slot {
        Some(n) if (1..=3).contains(&n) => n - 1,
        Some(_) => return Err(String::from("slot must be 1, 2 or 3")),
        None => 0,
    };
    println!("slot {} item sets", slot + 1);
    for set in mhnx_core::myset::item_sets(&s, slot) {
        if set.is_empty() {
            continue;
        }
        let items: Vec<String> = set
            .used_items()
            .iter()
            .map(|i| format!("{} x{}", mhnx_core::hbedit::item_name_of(i.id), i.count))
            .collect();
        println!("  [{}] {}", set.index, set.name);
        println!("      {}", items.join(", "));
    }
    println!("slot {} equipment sets", slot + 1);
    for set in mhnx_core::myset::equip_sets(&s, slot) {
        if set.is_empty() {
            continue;
        }
        let parts: Vec<String> = set
            .parts
            .iter()
            .map(|p| {
                if *p == mhnx_core::myset::NONE {
                    String::from("-")
                } else {
                    format!("{}", p)
                }
            })
            .collect();
        println!("  [{}] {:<16} {}", set.index, set.name, parts.join(" "));
    }
    println!("slot {} palicos", slot + 1);
    for p in mhnx_core::palico::used(&s, slot) {
        println!(
            "  {:>3} {:<14} Lv{:<3} {:<12} bond {:<3} skills: {}",
            p.index,
            p.name,
            p.level,
            p.system_name(),
            p.enthusiasm,
            p.learned_skills()
                .iter()
                .map(|id| mhnx_core::palico::skill_name(*id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
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
    // Reading the input and writing the result to the same path silently
    // modified the source while the message below claimed it had not been
    // touched. Compare the resolved files so a differing suffix cannot slip by.
    if same_file(save_path, out) {
        return Err(format!(
            "out must be a different file from the input save ({}); this command never writes the input",
            save_path
        ));
    }
    let mut s = Save::from_bytes(&read(save_path)?).map_err(|e| e.to_string())?;
    let raw = read(arc_path)?;
    let a = Arc::parse(&raw).map_err(|e| e.to_string())?;
    let id = a
        .by_ext("mib")
        .and_then(|e| e.data.get(0x0C..0x10))
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or("archive has no .mib")? as i32;
    if s.kind() == mhnx_core::save::SaveKind::Mhgu {
        return Err(String::from(
            "this is an MHGU save; download quests are not supported for it",
        ));
    }
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

fn cmd_slot_import(save: &str, slot: usize, inp: &str, force: bool) -> Result<(), String> {
    if !(1..=3).contains(&slot) {
        return Err(String::from("slot must be 1, 2 or 3"));
    }
    let loaded = loaded_fingerprint(save);
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let data = read(inp)?;
    s.import_slot(slot - 1, &data).map_err(|e| e.to_string())?;
    let name = s
        .read_character(slot - 1)
        .map(|c| c.name)
        .unwrap_or_default();
    let out = s.to_bytes();
    write_in_place(save, &out, loaded.as_ref(), force)?;
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

fn cmd_cat_add(save: &str, lang: &str, id: u8, force: bool) -> Result<(), String> {
    use mhnx_core::dlcats::Lang;
    let lang = match lang {
        "jp" | "JP" => Lang::Jp,
        "en" | "EN" => Lang::En,
        other => return Err(format!("language must be jp or en, got {}", other)),
    };
    let loaded = loaded_fingerprint(save);
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = s.install_dlc_palico(lang, id).map_err(|e| e.to_string())?;
    write_in_place(save, &s.to_bytes(), loaded.as_ref(), force)?;
    let name = s.dlc_palicoes().get(slot).map(|c| c.name.clone()).unwrap_or_default();
    println!("installed {} (id 0x{:02X}) into slot {}", name, id, slot + 1);
    println!("NOTE: write system_backup too");
    Ok(())
}

fn cmd_cat_del(save: &str, index: usize, force: bool) -> Result<(), String> {
    if !(1..=50).contains(&index) {
        return Err(String::from("index must be 1..50"));
    }
    let loaded = loaded_fingerprint(save);
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let name = s
        .dlc_palicoes()
        .get(index - 1)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    if !s.remove_dlc_palico(index - 1) {
        return Err(String::from("could not clear the slot"));
    }
    write_in_place(save, &s.to_bytes(), loaded.as_ref(), force)?;
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

/// Prove that this build reads the save and writes the same bytes back.
///
/// Nothing is written: the file is parsed, serialised in memory and the two
/// byte strings are compared. A difference means the file as it sits on the
/// card is not something this tool can reproduce, which is what a user needs
/// to know before letting any other command rewrite it.
///
/// Returns true when the round trip was identical, so main can exit 1 when it
/// was not; a difference is a finding, not an error message.
fn cmd_verify(path: &str) -> Result<bool, String> {
    let on_disk = read(path)?;
    // The shape check runs on the raw bytes, so it still reports something for
    // a file this build cannot parse at all.
    let shape = mhnx_core::verify::check_shape(&on_disk);
    let parsed = Save::from_bytes(&on_disk);
    match &parsed {
        Ok(s) => println!(
            "{}: {} ({} bytes)",
            path,
            kind_name(s.kind()),
            on_disk.len()
        ),
        Err(_) => println!("{}: {} bytes", path, on_disk.len()),
    }
    if shape.is_empty() {
        println!("  shape        OK");
    } else {
        println!("  shape        {} finding(s)", shape.len());
        for f in &shape {
            println!("               {}", f.text());
        }
        println!("               warning: 形が想定と違います。一致していてもゲームが読めない可能性があります");
    }

    let s = parsed.map_err(|e| format!("{} をセーブとして読めません: {}", path, e))?;
    let written = s.to_bytes();
    let trip = mhnx_core::verify::compare(&on_disk, &written);

    if trip.is_identical() {
        println!("  round trip   {} bytes back, identical", written.len());
        println!("identical");
        Ok(true)
    } else {
        if let Some((want, got)) = trip.length_change {
            println!("  round trip   長さが {} → {} に変わりました", want, got);
        }
        if !trip.differing.is_empty() {
            println!(
                "  round trip   {} バイトが一致しません",
                trip.differing.len()
            );
            println!("  先頭の差分: {}", trip.examples(10));
        }
        println!("different: the file on disk is not what this build writes back");
        Ok(false)
    }
}

/// Snapshots kept per save. Enough to step back through a few edits.
const SNAPSHOT_KEEP: usize = 10;

/// Where the snapshots of a save live: the snapshots directory beside it.
fn snapshot_dir(save: &str) -> PathBuf {
    match PathBuf::from(save).parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("snapshots"),
        _ => PathBuf::from("snapshots"),
    }
}

/// The part of the file name the snapshots are named after.
fn snapshot_stem(save: &str) -> String {
    PathBuf::from(save)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| String::from("save"))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The file names in a snapshot directory. A missing directory is empty, not
/// an error: nothing has been snapshotted yet.
fn snapshot_names(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(format!("{}: {}", dir.display(), e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {}", dir.display(), e))?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        out.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(out)
}

/// Write one snapshot of a save and drop the oldest beyond SNAPSHOT_KEEP.
///
/// The protect argument names a snapshot that must not be rotated away;
/// restoring from an old snapshot would otherwise be able to delete the file
/// being restored.
fn write_snapshot(save: &str, protect: Option<&str>) -> Result<(PathBuf, Vec<String>), String> {
    let bytes = std::fs::read(save).map_err(|e| format!("{}: {}", save, e))?;
    let dir = snapshot_dir(save);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let stem = snapshot_stem(save);
    let prefix = format!("{}-", stem);

    let mut names = snapshot_names(&dir)?;
    // Two snapshots in the same second would collide; step the stamp forward
    // instead of overwriting the one just taken.
    let mut unix = now_unix();
    let mut name = mhnx_core::snapshot::name(&stem, unix);
    while names.iter().any(|n| *n == name) || dir.join(&name).exists() {
        unix += 1;
        name = mhnx_core::snapshot::name(&stem, unix);
    }
    let out = dir.join(&name);
    std::fs::write(&out, &bytes).map_err(|e| format!("{}: {}", out.display(), e))?;

    names.push(name);
    if let Some(p) = protect {
        names.retain(|n| n != p);
    }
    let mut dropped = Vec::new();
    for i in mhnx_core::snapshot::older_than(&names, &prefix, SNAPSHOT_KEEP) {
        let victim = dir.join(&names[i]);
        match std::fs::remove_file(&victim) {
            Ok(()) => dropped.push(names[i].clone()),
            // Rotation failing must not hide a snapshot that was written.
            Err(e) => println!("could not drop {}: {}", victim.display(), e),
        }
    }
    Ok((out, dropped))
}

/// Say which snapshots the rotation removed, after the new one was announced.
fn report_dropped(dropped: &[String]) {
    for n in dropped {
        println!("dropped the oldest snapshot {}", n);
    }
}

/// The YYYYMMDD-HHMMSS part of a snapshot name, as YYYY-MM-DD HH:MM:SS.
fn snapshot_stamp(name: &str) -> Option<String> {
    let body = name.strip_suffix(".sav")?;
    let tail = body.get(body.len().checked_sub(15)?..)?;
    let b = tail.as_bytes();
    if b.len() != 15 || b[8] != b'-' {
        return None;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 8 || c.is_ascii_digit())
    {
        return None;
    }
    Some(format!(
        "{}-{}-{} {}:{}:{}",
        &tail[0..4],
        &tail[4..6],
        &tail[6..8],
        &tail[9..11],
        &tail[11..13],
        &tail[13..15]
    ))
}

fn cmd_snapshot(save: &str) -> Result<(), String> {
    let (out, dropped) = write_snapshot(save, None)?;
    let kept = snapshot_names(&snapshot_dir(save))?
        .iter()
        .filter(|n| n.starts_with(&format!("{}-", snapshot_stem(save))) && n.ends_with(".sav"))
        .count();
    println!("saved snapshot {}", out.display());
    report_dropped(&dropped);
    println!("{} snapshot(s) kept (limit {})", kept, SNAPSHOT_KEEP);
    Ok(())
}

fn cmd_snapshots(save: &str) -> Result<(), String> {
    let dir = snapshot_dir(save);
    let prefix = format!("{}-", snapshot_stem(save));
    let mut names: Vec<String> = snapshot_names(&dir)?
        .into_iter()
        .filter(|n| n.starts_with(&prefix) && n.ends_with(".sav"))
        .collect();
    if names.is_empty() {
        println!("no snapshots of {} yet ({})", save, dir.display());
        println!("take one with: mhnx snapshot {}", save);
        return Ok(());
    }
    // The stamp is fixed width, so plain order is chronological order.
    names.sort();
    println!(
        "{} snapshot(s) of {} in {}",
        names.len(),
        save,
        dir.display()
    );
    for n in names.iter().rev() {
        let p = dir.join(n);
        let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let when = snapshot_stamp(n).unwrap_or_else(|| String::from("-"));
        println!("  {:<34} {:>10} B  {}", n, size, when);
    }
    println!("put one back with: mhnx restore {} <name>", save);
    Ok(())
}

/// Put a snapshot back over the save, after snapshotting what is there now.
fn cmd_restore(save: &str, name: &str, force: bool) -> Result<(), String> {
    let loaded = loaded_fingerprint(save);
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || !name.ends_with(".sav")
    {
        return Err(format!(
            "snapshot name must be a plain <stem>-YYYYMMDD-HHMMSS.sav name, got {:?}",
            name
        ));
    }
    let dir = snapshot_dir(save);
    let src = dir.join(name);
    let wanted = std::fs::read(&src).map_err(|e| {
        format!(
            "{}: {} (mhnx snapshots {} lists them)",
            src.display(),
            e,
            save
        )
    })?;
    // A file that no longer parses would turn a bad restore into a dead save.
    let restored = Save::from_bytes(&wanted)
        .map_err(|e| format!("{} はセーブとして読めません: {}", src.display(), e))?;

    // Restoring a 3DS body over a Switch container (or the reverse) would
    // change the size under the game; refuse instead of guessing.
    let current = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    if current.kind() != restored.kind() {
        return Err(format!(
            "{} は {} ですが、{} は {} です",
            save,
            kind_name(current.kind()),
            name,
            kind_name(restored.kind())
        ));
    }

    // Refuse before the snapshot is taken: a refused restore must not rotate
    // the snapshots of the save away.
    check_write_allowed(save, loaded.as_ref(), force)?;
    let (made, dropped) = write_snapshot(save, Some(name))?;
    println!("現在の内容を {} に残しました", made.display());
    report_dropped(&dropped);
    replace_save(save, &wanted)?;
    println!(
        "restored {} ({} bytes, {}) from {}",
        save,
        wanted.len(),
        kind_name(restored.kind()),
        name
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// CSV, pouch and set commands
// ---------------------------------------------------------------------------

/// The options the CSV and set commands share.
struct Flags {
    /// Character slot to work on, 0..=2. The core counts slots from zero, and
    /// so does --slot; the messages name the slot one higher, like the rest of
    /// this tool, so the two never look the same by accident.
    slot: usize,
    /// The arguments that are not options, in the order they were given.
    positional: Vec<String>,
}

/// Split --slot N out of the argument list.
///
/// --force is taken off before this runs: every command that rewrites a save
/// in place accepts it and the others reject it, so here it would only be an
/// unknown option.
///
/// An unknown option is refused instead of skipped: these commands rewrite a
/// save, so a mistyped `--slto 2` must not quietly work on slot 0.
fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut flags = Flags {
        slot: 0,
        positional: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--slot" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| String::from("--slot needs a number (0, 1 or 2)"))?;
            flags.slot = num(value, "--slot")?;
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--slot=") {
            flags.slot = num(value, "--slot")?;
            i += 1;
        } else if arg.starts_with("--") {
            return Err(format!(
                "unknown option {} (these commands take --slot N)",
                arg
            ));
        } else {
            flags.positional.push(String::from(arg));
            i += 1;
        }
    }
    if flags.slot >= save::MAX_SLOTS {
        return Err(format!("--slot must be 0, 1 or 2, got {}", flags.slot));
    }
    Ok(flags)
}

/// The usage line of one of the commands below, for a wrong argument count.
fn command_usage(name: &str) -> &'static str {
    match name {
        "csv-items" => "mhnx csv-items <save> <out.csv> [--slot N]",
        "csv-equipment" => "mhnx csv-equipment <save> <out.csv> [--slot N]",
        "import-items" => "mhnx import-items <save> <in.csv> [--slot N] [--force]",
        "import-equipment" => "mhnx import-equipment <save> <in.csv> [--slot N] [--force]",
        "pouch" => "mhnx pouch <save> [--slot N]",
        "sets" => "mhnx sets <save> [--slot N]",
        _ => "mhnx",
    }
}

/// Refuse a slot the save has no character in, and hand it back.
///
/// Reading a slot that does not exist would produce an empty table, which
/// reads like "this character has nothing" instead of "there is no character
/// here". Every command below goes through this first.
fn checked_slot(s: &Save, slot: usize) -> Result<usize, String> {
    if slot >= save::MAX_SLOTS {
        return Err(format!("--slot must be 0, 1 or 2, got {}", slot));
    }
    if s.slot_base(slot).is_none() {
        return Err(format!(
            "slot {} is not in use: the save has no character there",
            slot + 1
        ));
    }
    Ok(slot)
}

/// `slot 2 (name)`, the way these commands name a character.
fn slot_label(s: &Save, slot: usize) -> String {
    let name = s.read_character(slot).map(|c| c.name).unwrap_or_default();
    if name.is_empty() {
        format!("slot {}", slot + 1)
    } else {
        format!("slot {} ({})", slot + 1, name)
    }
}

/// Read a UTF-8 text file, naming the path when it is not text.
///
/// A spreadsheet on a Japanese Windows may still save in Shift_JIS; saying so
/// is more use than the byte offset the parser would report.
fn read_text(path: &str) -> Result<String, String> {
    let bytes = read(path)?;
    String::from_utf8(bytes).map_err(|_| format!("{}: not a UTF-8 text file", path))
}

/// Refuse to write a CSV over the save it was read from.
fn csv_out_checked(save: &str, out: &str) -> Result<(), String> {
    if same_file(save, out) {
        return Err(format!(
            "out must be a different file from the save ({}); a CSV is not a save",
            save
        ));
    }
    Ok(())
}

/// Say what an import did, one line per thing worth mentioning.
fn print_report(report: &ApplyReport) {
    println!(
        "  applied {}, skipped {}, clamped {}",
        report.applied, report.skipped, report.clamped
    );
    for line in &report.messages {
        println!("  {}", line);
    }
}

/// Write one character's item box out as CSV.
fn cmd_csv_items(save: &str, out: &str, slot: usize) -> Result<(), String> {
    csv_out_checked(save, out)?;
    let s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = checked_slot(&s, slot)?;
    let items = s
        .read_item_box(slot)
        .ok_or_else(|| format!("slot {} has no item box", slot + 1))?;
    let csv = csvout::items_to_csv(&s, slot);
    std::fs::write(out, csv.as_bytes()).map_err(|e| format!("{}: {}", out, e))?;
    let used = items.iter().filter(|i| i.id != 0).count();
    println!("wrote {} for {}", out, slot_label(&s, slot));
    println!(
        "  {} data rows (header included: {} lines), {} of the {} records used",
        items.len(),
        items.len() + 1,
        used,
        mhnx_core::itembox::TOTAL_ITEM_SLOTS
    );
    Ok(())
}

/// Write one character's equipment box out as CSV.
fn cmd_csv_equipment(save: &str, out: &str, slot: usize) -> Result<(), String> {
    csv_out_checked(save, out)?;
    let s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = checked_slot(&s, slot)?;
    if s.equipment_box(slot).is_none() {
        return Err(format!("slot {} has no equipment box", slot + 1));
    }
    let csv = csvout::equipment_to_csv(&s, slot);
    std::fs::write(out, csv.as_bytes()).map_err(|e| format!("{}: {}", out, e))?;
    let rows = csv.lines().count().saturating_sub(1);
    println!("wrote {} for {}", out, slot_label(&s, slot));
    println!(
        "  {} data rows (records that hold something) out of {}",
        rows,
        mhnx_core::equip::EQUIP_COUNT
    );
    Ok(())
}

/// Apply an item CSV to one character's box.
///
/// The box becomes the file - row n is record n - so a file that stops early
/// empties the records after it. When that would drop something the save holds
/// now, the command lists what would go and stops; `--force` is the only way
/// past it. Nothing is written before that decision either way.
fn cmd_import_items(save: &str, inp: &str, slot: usize, force: bool) -> Result<(), String> {
    let loaded = loaded_fingerprint(save);
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = checked_slot(&s, slot)?;
    let current = s
        .read_item_box(slot)
        .ok_or_else(|| format!("slot {} has no item box", slot + 1))?;
    let text = read_text(inp)?;
    let rows = csvout::parse_items(&text).map_err(|e| format!("{}: {}", inp, e))?;
    if rows.is_empty() {
        return Err(format!(
            "{} holds no item rows; the save was not written",
            inp
        ));
    }
    if !force {
        let lost: Vec<usize> = current
            .iter()
            .enumerate()
            .filter(|(i, item)| item.id != 0 && rows.get(*i).map(|r| r.0).unwrap_or(0) == 0)
            .map(|(i, _)| i)
            .collect();
        if !lost.is_empty() {
            println!(
                "this import would empty {} record(s) that hold something now:",
                lost.len()
            );
            for i in lost.iter().take(10) {
                println!(
                    "  record {}: id {} x{} {}",
                    i,
                    current[*i].id,
                    current[*i].count,
                    item_name(current[*i].id)
                );
            }
            if lost.len() > 10 {
                println!("  ... and {} more", lost.len() - 10);
            }
            return Err(format!(
                "refusing to drop {} record(s); run again with --force to replace the box anyway (the save was not written)",
                lost.len()
            ));
        }
    }
    let report = csvout::apply_items(&mut s, slot, &rows);
    print_report(&report);
    if report.applied == 0 {
        return Err(String::from(
            "no row was applied, so the save was not written",
        ));
    }
    let out = s.to_bytes();
    let label = slot_label(&s, slot);
    write_in_place(save, &out, loaded.as_ref(), force)?;
    println!("{} applied to {} in {}", inp, label, save);
    println!("NOTE: write system_backup too - the game keeps the pair in sync");
    Ok(())
}

/// Apply an equipment CSV to one character's box.
///
/// A row names the record it belongs to, so a record the file does not mention
/// keeps what it had; nothing is emptied here. A row that would move a record
/// other sets point at is refused by the core and reported, not forced.
fn cmd_import_equipment(save: &str, inp: &str, slot: usize, force: bool) -> Result<(), String> {
    let loaded = loaded_fingerprint(save);
    let mut s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = checked_slot(&s, slot)?;
    if s.equipment_box(slot).is_none() {
        return Err(format!("slot {} has no equipment box", slot + 1));
    }
    let text = read_text(inp)?;
    let rows = csvout::parse_equipment(&text).map_err(|e| format!("{}: {}", inp, e))?;
    if rows.is_empty() {
        return Err(format!(
            "{} holds no equipment rows; the save was not written",
            inp
        ));
    }
    let report = csvout::apply_equipment(&mut s, slot, &rows);
    print_report(&report);
    if report.applied == 0 {
        return Err(String::from(
            "no row was applied, so the save was not written",
        ));
    }
    let out = s.to_bytes();
    let label = slot_label(&s, slot);
    write_in_place(save, &out, loaded.as_ref(), force)?;
    println!("{} applied to {} in {}", inp, label, save);
    println!("NOTE: write system_backup too - the game keeps the pair in sync");
    Ok(())
}

/// Show the thirty-two pouch slots of one character.
fn cmd_pouch(save: &str, slot: usize) -> Result<(), String> {
    let s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = checked_slot(&s, slot)?;
    let items = mhnx_core::myset::pouch(&s, slot);
    if items.is_empty() {
        return Err(format!("slot {} has no readable pouch", slot + 1));
    }
    let used = items.iter().filter(|i| i.id != 0).count();
    println!(
        "{} pouch: {} of {} slots used",
        slot_label(&s, slot),
        used,
        items.len()
    );
    for (index, item) in items.iter().enumerate() {
        if item.id == 0 {
            println!("  #{:<3} empty", index);
            continue;
        }
        println!(
            "  #{:<3} id {:<5} x{:<3} {}",
            index,
            item.id,
            item.count,
            mhnx_core::hbedit::item_name_of(item.id)
        );
    }
    Ok(())
}

/// Summarise the item sets and the equipment sets of one character.
///
/// The full contents are what `mysets` is for; this is the count and the
/// names, plus a warning count so a broken record is visible without dumping
/// every entry.
fn cmd_sets(save: &str, slot: usize) -> Result<(), String> {
    let s = Save::from_bytes(&read(save)?).map_err(|e| e.to_string())?;
    let slot = checked_slot(&s, slot)?;
    println!("{}", slot_label(&s, slot));

    let item_sets = mhnx_core::myset::item_sets(&s, slot);
    if item_sets.is_empty() {
        return Err(format!("slot {} has no readable item sets", slot + 1));
    }
    let used = item_sets.iter().filter(|set| !set.is_empty()).count();
    println!(
        "  item sets: {} of {} in use",
        used,
        mhnx_core::myset::ITEM_SETS
    );
    for set in item_sets.iter().filter(|set| !set.is_empty()) {
        let warnings = mhnx_core::myset::item_set_warnings(&s, slot, set.index);
        println!(
            "    [{:>2}] {:<16} {:>2}/{} entries{}",
            set.index,
            set.name,
            set.used_items().len(),
            mhnx_core::myset::ITEM_SET_ITEMS,
            warning_tag(warnings.len())
        );
    }

    let equip_sets = mhnx_core::myset::equip_sets(&s, slot);
    let used = equip_sets.iter().filter(|set| !set.is_empty()).count();
    println!(
        "  equipment sets: {} of {} in use",
        used,
        mhnx_core::myset::EQUIP_SETS
    );
    for set in equip_sets.iter().filter(|set| !set.is_empty()) {
        let parts: Vec<String> = set
            .parts
            .iter()
            .enumerate()
            .map(|(part, index)| {
                let name = mhnx_core::myset::PART_NAMES[part];
                if *index == mhnx_core::myset::NONE {
                    format!("{}-", name)
                } else {
                    format!("{}#{}", name, index)
                }
            })
            .collect();
        let warnings = mhnx_core::myset::warnings(&s, slot, set.index);
        println!(
            "    [{:>2}] {:<16} {}{}",
            set.index,
            set.name,
            parts.join(" "),
            warning_tag(warnings.len())
        );
    }
    Ok(())
}

/// `  (3 warning(s))`, or nothing when there are none.
fn warning_tag(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!("  ({} warning(s))", count)
    }
}

/// The commands that can overwrite the save they were given.
///
/// The rest refuse --force: a flag that silently does nothing is worse than one
/// that is rejected.
fn rewrites_in_place(command: &str) -> bool {
    matches!(
        command,
        "install"
            | "slot-import"
            | "cat-add"
            | "cat-del"
            | "restore"
            | "import-items"
            | "import-equipment"
    )
}

fn main() -> ExitCode {
    // --force is taken off before the command is looked at rather than parsed
    // by each one: every command that rewrites a save in place accepts it, and
    // the ones that do not must refuse it instead of quietly ignoring it.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|arg| arg == "--force");
    args.retain(|arg| arg != "--force");
    if args.is_empty() {
        usage();
        return ExitCode::SUCCESS;
    }
    if force && !rewrites_in_place(&args[0]) {
        eprintln!(
            "error: --force is only for the commands that rewrite a save in place: \
             install, slot-import, cat-add, cat-del, restore, import-items, import-equipment"
        );
        return ExitCode::FAILURE;
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
        "install" if args.len() == 3 => cmd_install(&args[1], &args[2], force),
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
        "arc-compact" if args.len() == 3 => cmd_arc_compact(&args[1], &args[2]),
        "mysets" if args.len() == 2 => cmd_mysets(&args[1], None),
        "mysets" if args.len() == 3 => cmd_mysets(&args[1], args[2].parse().ok()),
        "guild-card" if args.len() == 2 => cmd_guild_card(&args[1], None),
        "guild-card" if args.len() == 3 => cmd_guild_card(&args[1], args[2].parse().ok()),
        "hunts" if args.len() == 2 => cmd_hunts(&args[1], None),
        "hunts" if args.len() == 3 => cmd_hunts(&args[1], args[2].parse().ok()),
        "equip-items" if args.len() == 3 => (|| {
            cmd_equip_items(&args[1], num(&args[2], "slot")?)
        })(),
        "equip-set" if args.len() == 9 => (|| {
            cmd_equip_set(
                &args[1],
                num(&args[2], "slot")?,
                num(&args[3], "record")?,
                &args[4],
                num(&args[5], "index")?,
                num(&args[6], "item")?,
                num(&args[7], "count")?,
                &args[8],
            )
        })(),
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
        "slot-export" if args.len() == 4 => (|| {
            cmd_slot_export(&args[1], num(&args[2], "slot")?, &args[3])
        })(),
        "slot-import" if args.len() == 4 => (|| {
            cmd_slot_import(&args[1], num(&args[2], "slot")?, &args[3], force)
        })(),
        "items" if args.len() == 2 => cmd_items(&args[1], None),
        "items" if args.len() == 3 => cmd_items(&args[1], args[2].parse().ok()),
        "cats" if args.len() == 1 => cmd_cats(),
        "cat-add" if args.len() == 4 => match parse_id(&args[3]) {
            Ok(id) => cmd_cat_add(&args[1], &args[2], id, force),
            Err(e) => Err(e),
        },
        "cat-del" if args.len() == 3 => (|| cmd_cat_del(&args[1], num(&args[2], "index")?, force))(),
        "verify" if args.len() == 2 => match cmd_verify(&args[1]) {
            Ok(true) => Ok(()),
            // A difference is a finding, so say it once and exit 1.
            Ok(false) => return ExitCode::from(1),
            Err(e) => Err(e),
        },
        "snapshot" if args.len() == 2 => cmd_snapshot(&args[1]),
        "snapshots" if args.len() == 2 => cmd_snapshots(&args[1]),
        "restore" if args.len() == 3 => cmd_restore(&args[1], &args[2], force),
        "csv-items" | "csv-equipment" | "import-items" | "import-equipment" | "pouch"
        | "sets" => {
            let name = args[0].as_str();
            (|| {
                let flags = parse_flags(&args[1..])?;
                let count = flags.positional.len();
                let expected = match name {
                    "csv-items" | "csv-equipment" | "import-items" | "import-equipment" => 2,
                    _ => 1,
                };
                let wrong = || {
                    Err(format!(
                        "usage: {}\n({} argument(s) given, {} expected)",
                        command_usage(name),
                        count,
                        expected
                    ))
                };
                match name {
                    "csv-items" if count == 2 => cmd_csv_items(
                        &flags.positional[0],
                        &flags.positional[1],
                        flags.slot,
                    ),
                    "csv-equipment" if count == 2 => cmd_csv_equipment(
                        &flags.positional[0],
                        &flags.positional[1],
                        flags.slot,
                    ),
                    "import-items" if count == 2 => cmd_import_items(
                        &flags.positional[0],
                        &flags.positional[1],
                        flags.slot,
                        force,
                    ),
                    "import-equipment" if count == 2 => cmd_import_equipment(
                        &flags.positional[0],
                        &flags.positional[1],
                        flags.slot,
                        force,
                    ),
                    "pouch" if count == 1 => cmd_pouch(&flags.positional[0], flags.slot),
                    "sets" if count == 1 => cmd_sets(&flags.positional[0], flags.slot),
                    _ => wrong(),
                }
            })()
        }
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
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// A fingerprint with a fixed timestamp, for the comparisons.
    fn at(size: u64, secs: u64, hash: u64) -> Fingerprint {
        Fingerprint {
            size,
            modified: Some(UNIX_EPOCH + Duration::from_secs(secs)),
            hash,
        }
    }

    /// What a clean look at an unchanged file produces.
    fn clean(current: Fingerprint) -> Observed {
        Observed {
            current: Ok(current),
            emulators: Ok(Vec::new()),
            read_only: false,
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "mhnx_cli_{}_{}_{}.sav",
            std::process::id(),
            tag,
            now
        ))
    }

    #[test]
    fn an_unchanged_save_with_no_emulator_is_allowed() {
        let fingerprint = at(100, 1_000, 7);
        assert!(decide("save", Some(&fingerprint), &clean(fingerprint.clone()), false).is_ok());
    }

    #[test]
    fn a_save_that_changed_after_the_read_needs_force() {
        let loaded = at(100, 1_000, 7);
        let observed = clean(at(100, 1_001, 7));
        let error = decide("save", Some(&loaded), &observed, false).unwrap_err();
        assert!(error.contains("変わっています"), "{}", error);
        assert!(error.contains("--force"), "{}", error);
        assert!(decide("save", Some(&loaded), &observed, true).is_ok());
    }

    #[test]
    fn a_running_emulator_is_named_and_needs_force() {
        let fingerprint = at(100, 1_000, 7);
        let observed = Observed {
            current: Ok(fingerprint.clone()),
            emulators: Ok(vec![String::from("Ryujinx.exe")]),
            read_only: false,
        };
        let error = decide("save", Some(&fingerprint), &observed, false).unwrap_err();
        assert!(error.contains("Ryujinx.exe"), "{}", error);
        assert!(decide("save", Some(&fingerprint), &observed, true).is_ok());
    }

    #[test]
    fn a_process_table_that_cannot_be_read_needs_force() {
        let fingerprint = at(100, 1_000, 7);
        let observed = Observed {
            current: Ok(fingerprint.clone()),
            emulators: Err(String::from("Access is denied")),
            read_only: false,
        };
        let error = decide("save", Some(&fingerprint), &observed, false).unwrap_err();
        assert!(error.contains("確認できません"), "{}", error);
        assert!(decide("save", Some(&fingerprint), &observed, true).is_ok());
    }

    #[test]
    fn a_read_only_file_is_refused_even_with_force() {
        let fingerprint = at(100, 1_000, 7);
        let observed = Observed {
            current: Ok(fingerprint.clone()),
            emulators: Ok(Vec::new()),
            read_only: true,
        };
        let error = decide("save", Some(&fingerprint), &observed, true).unwrap_err();
        assert!(error.contains("読み取り専用"), "{}", error);
    }

    #[test]
    fn a_missing_target_is_refused_even_with_force() {
        let observed = Observed {
            current: Err(String::from("書き込み先が見つかりません: gone.sav")),
            emulators: Ok(Vec::new()),
            read_only: false,
        };
        let error = decide("gone.sav", None, &observed, true).unwrap_err();
        assert!(error.contains("見つかりません"), "{}", error);
    }

    #[test]
    fn a_write_without_a_loaded_fingerprint_needs_force() {
        let observed = clean(at(100, 1_000, 7));
        let error = decide("save", None, &observed, false).unwrap_err();
        assert!(error.contains("記録できていない"), "{}", error);
        assert!(decide("save", None, &observed, true).is_ok());
    }

    #[test]
    fn observe_names_a_file_that_is_not_there() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        let text = path.to_str().expect("the temp path is text");
        let observed = observe(text);
        match observed.current {
            Err(message) => assert!(message.contains("見つかりません"), "{}", message),
            Ok(_) => panic!("a file that is not there must not have a fingerprint"),
        }
        assert!(!observed.read_only);
    }

    #[test]
    fn observe_sees_the_read_only_mark() {
        let path = temp_path("readonly");
        std::fs::write(&path, b"save").expect("the temp file is writable");
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).expect("the mark can be set");

        let observed = observe(path.to_str().expect("the temp path is text"));
        assert!(observed.read_only, "the read-only mark must be seen");
        assert!(observed.current.is_ok());

        // Put the mark back so the file can be removed.
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(&path, permissions).expect("the mark can be cleared");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_real_file_that_changed_after_the_read_is_refused() {
        // The fingerprint is taken, the file is edited by "something else",
        // and the check runs on the pair: this is the whole pre-write check on
        // a real file, without a hand-built Observed.
        let path = temp_path("stale_check");
        std::fs::write(&path, b"aaaaa").expect("the temp file is writable");
        let text = path
            .to_str()
            .expect("the temp path is text")
            .to_string();
        let loaded = loaded_fingerprint(&text).expect("the temp file fingerprints");
        std::fs::write(&path, b"bbbbb").expect("the temp file is writable");
        let error = check_write_allowed(&text, Some(&loaded), false).unwrap_err();
        assert!(error.contains("変わっています"), "{}", error);
        assert!(error.contains("--force"), "{}", error);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_real_read_only_file_is_refused_by_the_whole_check() {
        let path = temp_path("readonly_check");
        std::fs::write(&path, b"save").expect("the temp file is writable");
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).expect("the mark can be set");
        let text = path
            .to_str()
            .expect("the temp path is text")
            .to_string();
        let loaded = loaded_fingerprint(&text);
        // --force does not clear a read-only file, so this holds on any machine.
        let error = check_write_allowed(&text, loaded.as_ref(), true).unwrap_err();
        assert!(error.contains("読み取り専用"), "{}", error);

        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(&path, permissions).expect("the mark can be cleared");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_is_refused_by_the_whole_check() {
        let path = temp_path("missing_check");
        let _ = std::fs::remove_file(&path);
        let text = path
            .to_str()
            .expect("the temp path is text")
            .to_string();
        for force in [false, true] {
            let error = check_write_allowed(&text, None, force).unwrap_err();
            assert!(error.contains("見つかりません"), "{}", error);
        }
    }

    #[test]
    fn force_gets_a_real_file_past_the_soft_checks() {
        // This machine may have an emulator open, so what can be checked about
        // the real path is that --force clears the soft reasons; the hard ones
        // are covered above.
        let path = temp_path("force");
        std::fs::write(&path, b"save").expect("the temp file is writable");
        let text = path
            .to_str()
            .expect("the temp path is text")
            .to_string();
        let loaded = loaded_fingerprint(&text).expect("the temp file fingerprints");
        check_write_allowed(&text, Some(&loaded), true)
            .expect("--force must get past the soft checks");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_write_itself_keeps_a_copy_and_verifies_the_bytes() {
        let path = temp_path("replace");
        std::fs::write(&path, b"old").expect("the temp file is writable");
        let text = path
            .to_str()
            .expect("the temp path is text")
            .to_string();
        let loaded = loaded_fingerprint(&text);
        write_in_place(&text, b"new bytes", loaded.as_ref(), true)
            .expect("the write goes through");
        assert_eq!(
            std::fs::read(&path).expect("the file reads"),
            b"new bytes"
        );
        let bak = format!("{}.bak", text);
        assert_eq!(
            std::fs::read(&bak).expect("the copy is kept"),
            b"old"
        );
        let _ = std::fs::remove_file(&bak);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn only_the_commands_that_rewrite_a_save_take_force() {
        for name in [
            "install",
            "slot-import",
            "cat-add",
            "cat-del",
            "restore",
            "import-items",
            "import-equipment",
        ] {
            assert!(rewrites_in_place(name), "{} rewrites the save", name);
        }
        for name in [
            "csv-items",
            "csv-equipment",
            "pouch",
            "sets",
            "info",
            "verify",
            "snapshot",
            "add-quest",
            "equip-set",
        ] {
            assert!(!rewrites_in_place(name), "{} must refuse --force", name);
        }
    }
}
