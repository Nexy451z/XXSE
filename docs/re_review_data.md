# Data-integrity review: can mhnxeditor corrupt a save?

> Note: while this pass ran, `crates/mhnx-core/src/hbedit.rs`, `switch/Cargo.toml` and
> `switch/src/editor.rs` were modified by other reviewers (palico/check cursor, swkbd limits). This
> report's own numbers were taken against the tree at that moment; code line numbers cited for those
> three files were re-checked at the end and are only used for the write-path section.

Reviewer: adversarial data-integrity pass over `crates/mhnx-core` (save / arc / entries / charm / dlc),
driven by a standalone probe crate at `C:\anyWorkspace\nx\mhxx\probe` (`cargo run --offline`,
path dependency on `crates/mhnx-core`, no repo file modified). All real data was opened read-only.

Inputs used for every number below:

* save `C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`
  (4,726,188 B = 36 B header + 4,726,152 B body, Switch MHXX, 3 slots, 175 quests, 650 talismans,
  7 download Palicoes). `system_backup` (same size, body byte-identical) used only for header checks.
* 316 quest archives: 175 in `dist/quests_all`, 141 in
  `C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest`.

---

## Verdict

**I could not make the library, the CLI or the GUI corrupt the *save container* through any normal
path, and I could not make any public function panic.** The save/arc/charm writers are byte-exact on
untouched data, and every write path I could reach is confined to the record it names.

Three defects break that story, none of them in the container writers:

1. **`dlc::install_quest` accepts a payload that is not an .arc** and overwrites an installed quest
   with it (id validation is skipped exactly when the payload is broken). The save still loads; the
   quest is gone and its installed bit stays set.
2. **`.sup` is not parsed as the fixed 40-record array it is documented as.** 119 of 316 real
   supply containers hold a second block of real items after an empty run; those items are invisible
   and uneditable in the GUI (they are preserved, so no destruction).
3. **`Gmd::from_bytes` sizes a `Vec` from an unvalidated u32** in the file (96 GiB request from a
   64-byte input, measured). Survives only because Windows overcommits; a 32-bit/alloc-limited
   target (the Switch NRO shares this code) aborts.

Everything else I tried was clean, including 300 mutated saves and 2,500 mutated .arc files with
zero panics.

---

## CONFIRMED (own measurements)

### 1. Save round trip and no-op writes

* `Save::from_bytes(&raw)` -> `to_bytes()` == `raw` **exactly** for the real save (4,726,188 B).
  Kind detected as `Switch`, slot bases `0x126474 / 0x244C34 / 0x3633F4`, `slot_stride()` = `0x11E7C0`.
* The real 36-byte header is **byte-identical** to `switch_header("system", BODY_SIZE)`
  (`00…04 00 00 00 00 00 00 00 14 00 00 00 74 ee b2 36 0c 00 00 00 88 1d 48 00 …`);
  `system_backup`'s header matches `switch_header("system_backup", …)` and differs from the
  `system` one only at 0x14..0x18. `header_name()` returns `Some("system")` / `Some("system_backup")`.
* Read -> write through the public API changes **0 bytes** on this save:

  | call | changed bytes |
  |---|---|
  | `write_character(read_character())` x3 slots | 0 |
  | `write_item_box(read_item_box())` x3 slots | 0 |
  | `set_options(options())` | 0 |
  | `set_dlc_palico(i, existing record)` x50 | 0 |
  | GUI-equivalent `apply_to_save()` (both boxes + character, all 3 slots) | 0 |

  So opening the save and pressing 上書き保存 without editing is byte-identical — the reconstruction
  did not introduce a rewrite-on-save bug.
* No-op quest write: for all **175** quests, `dist/quests_all/q<id>.arc` is byte-identical to the
  payload in the save and its length equals the slot's stored `size`. `install_quest(save, id, same
  payload)` produced **0 differing bytes in 175/175** cases (max changed = 0).
* Edited quest: all 175 quests, one byte flipped at `.mib+0x14`, re-serialised, installed.
  **0/175** writes produced any changed byte outside
  `[36 + 0xB280 + 0x45F4 + slot*0x1400, +0x1400)`. Max changed bytes in a whole 4.7 MB save = **154**;
  the changed span inside the slot is `[+0x004, +0xD94]` worst case. Highest occupied slot = 199.
* Slot headroom: largest installed payload 3,453 B vs `QUEST_MAX_PAYLOAD` 5,112 B
  (`QUEST_SLOT_SIZE-8`); a 1-byte .mib edit re-compresses the arc **8..23 bytes larger** (all 175 grow),
  i.e. an edited quest still fits. An oversized payload is refused before any byte is written.

### 2. arc

* `Arc::parse` -> `to_bytes()` is **byte identical for all 316 files** (4,309 entries):
  identical length, identical bytes, no trailing data. 316/316 re-parse after re-serialisation too.
* Layout facts I re-measured rather than trusted: all flag words are `0x4xxxxxxx` (bit 30 set) and
  the low 30 bits always equal the decoded length (4,309/4,309); no duplicate payload offsets;
  no non-zero bytes in any inter-payload gap; no entry name is 64 B (so the 63-byte write cap is not
  hit); `first payload offset - align32(12+80*n)` is 0 for 315 files and 16 for one (`q1010165`),
  and the 16-byte gap is zero-filled and reproduced exactly; trailing bytes after the last payload
  are 0 in all 316.
* **Only some entries edited**: editing one entry re-compresses only that entry, later unchanged
  entries keep their original stream and simply move; earlier entries and the directory stay put
  (growth is entirely after the edited payload).
* **Can an edited entry overwrite another entry's payload? No.** Payload offsets are emitted
  monotonically from a single cursor, so intervals cannot overlap. Measured: editing entry 0 of every
  file -> 316/316 re-parse with all payloads preserved and **0 overlapping intervals**; editing each
  entry in turn (3 files, 43 edits) -> 0 failures; growing one entry to 200,000 incompressible bytes ->
  file 3,034 -> 203,060 B, re-parse OK, entry 0 and all later entries byte-preserved, no overlap.
  (Such a payload is refused for the save: `install_quest` checks 5,112 B first.)

### 3. entries.rs containers

* `ItemTable` (the only one with a writer): `parse` -> `to_bytes` identity on **1,776/1,776** real
  entries (1,460 `.rem`, 316 `.sup`).
* **Edit isolation: 12,919 single-field edits** (`count` and `item` of every parsed record of every
  entry) each changed only the bytes of the record they named — 0 leaks. Appending a record to a
  `.rem` writes only the new record and moves the `00 00 00 FF` terminator (40/40 checked,
  everything before it identical).
* **`.rem` terminator claim: confirmed with one nuance.** 1,223 of 1,460 `.rem` end with
  `00 00 00 FF` right after the last record. The remaining **237 are empty**: first record is
  `00 00 00 00`, 0 records parsed, all bytes after +0x18 are zero — i.e. no `.rem` with records
  lacks the terminator, and no `.rem` uses a 4-zero terminator.
* **`.sup` is a fixed array (confirmed) and nothing terminates it**: 0/316 contain a
  `00 00 00 FF` terminator; each is exactly 168 B = 8 + 40*4. See ERROR 3 for what the parser does
  with it.
* `.sem` (791), `.esl` (315+), `.qdp` (316), `.qdl` (316) have **no writer at all**
  (`SemEntry`, `Esl`, `Qdp`, `QuestLink` expose `raw` only; the GUI renders them read-only and
  `ArcEntry.data` is passed through untouched), so "identity" is vacuous for them rather than
  verified: there is no code path that could change them.
* Sizes are stable in all real data: `rem` 184 (1,460/1,460), `sup` 168 (316/316), `mib` 249,
  `qdl` 308, `qdp` 25, `gmd` and `esl` variable. `Gmd` parse->`to_bytes` identity on 316/316.

### 4. charm

* 650 talismans (188 / 231 / 231), each verified individually:
  `update_charm` changed **exactly +0x0C, +0x0D, +0x0E, +0x0F, +0x10 and nothing else** in
  650/650 cases (byte diff over the whole 72,000-byte box, mapped back to the record), and the five
  values read back correctly (points signed: `-3` -> `0xFD`).
* `create_charm` into the first free record: the created record **equals the template outside
  0x0C..0x10** (3/3 slots), nothing outside that 36-byte record changed, and the five fields hold the
  requested values.
* **No path touches a weapon or armour record**: 1,300 calls of `update_charm`/`clear_charm` on
  weapon/armour indices were all refused with 0 bytes changed; `Save::set_charm` on a weapon index
  returns `false` with 0 bytes changed in the whole save. `first_writable` returns `None` for a box
  with no blank and no talisman.
* The documented byte statistics are right: `+0x00`=6 (650/650), `+0x11`=0 (650/650), `+0x13`=1
  (650/650), `+0x12` in {98:2, 99:68, 100:580}, `+0x02` in {0:2, 4:2, 6:20, 7:46, 10:580},
  `+0x06..+0x09` non-zero in 15. The modal template picked is rarity 100 / `+0x02`=0x0A with a clean
  tail, as intended.

### 5. bounds

Battery of ~20 public entry points (`read/write_character`, item box, `charms`/`set_charm`/
`clear_charm`, all raw boxes, `export_slot`/`import_slot` with `[]` and `0x11E7C0`/`0x11D088`,
`options`/`set_options`, all palico calls, `slot_base`/`slot_used`, all `dlc` read/write calls,
`check::check_save`, `to_bytes`) executed on:

* body+`0x0C` (dlc section) in {0, 1, 2, 0x4C, 0xB280, 0x126474, 0x470000, 0x47C39C, 0xFFFFFFFF};
* all three slot bases in {0, 1, 0x126474, body-16, body-stride, body-stride+1, 0xFFFFFFFF,
  0x240000, 0x3633F4};
* quest `size` in {5112, 5120, 8192, i32::MAX, -1, -5120} on every slot;
* 300 randomly mutated saves (1..8 bytes mutated in the header region or anywhere);
* 2,500 mutated/garbage .arc files (parse, re-serialise, every entry container parser,
  `check_quest`).

**Total panics: 0.** `slot_base` (`save.rs:315-329`) rejects any base whose full stride does not fit,
which is what makes the accessors safe; `section_start` (`dlc.rs:91-102`) rejects a section that does
not end at or before slot 0, which is what keeps `uninstall_quest` from zeroing a character.

`cargo test --workspace` exits 0.

### 6. write paths (grep `body_mut` and every writer)

`body_mut()` is only called from `dlc::install_quest`, `dlc::uninstall_quest` and tests. All save
mutations go through `Save` methods, and `to_bytes()` is byte-exact, so no write path can perturb
bytes it does not touch (verified end-to-end: 0 changed bytes for every no-op path above).

Disk writes without an explicit user action: **none in normal use.**

* GUI: every `std::fs::write` sits behind a button (`app.rs:192` 保存, `:217/:218` Switch pair,
  `:229` 3DS export, `:440` .arc export, `:2018` .saveslot export). `apply_to_save()` mutates only
  the in-memory save; the UI says so ("まだファイルには書いていません").
* The one unconditional writer is the headless test hook in `crates/mhnx-gui/src/main.rs:111-121`:
  `MHNXEDITOR_WRITE_BACK` / `MHNXEDITOR_SAVE_OUT` write the save without asking. Test-only, but it is
  a real write path reachable by environment variable.
* CLI: writes are per-subcommand and always to the path the user named (`main.rs:215, 333, 417, 437`).
* Switch: `switch/src/editor.rs:448-469` (line numbers as of this pass; the file was being edited
  concurrently by another reviewer) writes only on a queued `Write::File` request; it then also
  regenerates `system_backup` next to `.../system`. No save-on-quit.
* `apply_to_save()` has one side effect worth knowing: it calls `set_slot_used(slot, true)`
  (`app.rs:172-174`), so pressing 保存 after merely selecting an unused slot marks it used.

---

## ERRORS

### E1 (high) — a broken payload silently destroys an installed quest: `crates/mhnx-core/src/dlc.rs:273`

```rust
if let Ok(arc) = crate::arc::Arc::parse(arc_bytes) {
    if let Some(mib) = arc.by_ext("mib").and_then(|e| crate::quest::Mib::from_bytes(&e.data).ok()) {
        let payload_id = mib.quest_id();
        if payload_id != 0 && payload_id != quest_id as u32 { return Err(...); }
    }
}
```

The id/consistency check is inside `Ok`. When the payload is **not** a parseable .arc the whole check
is skipped and the write proceeds. Measured on a copy of the real save:

* `install_quest(&mut save, 1010005, &[0xAB; 777])` -> `Ok(slot 0)`, 779 bytes changed,
  slot `size`=777, `Arc::parse` of the stored payload = `Err`. Quest 1010005 no longer exists in any
  usable form; the installed bit set is untouched, so the game still lists an entry it cannot load.
* Truncated arc (first 910 B of the real 1,821 B file) -> `Ok(slot 0)`, only **2 bytes** changed (the
  `size` field), the rest of the old payload left behind; the slot now claims 910 B and no longer
  parses.

Reachable in the GUI: `install_dlc` (`crates/mhnx-gui/src/app.rs:465-496`) identifies the quest by
**file name** (`q<id>.arc`) and never parses the file, so any truncated/corrupt .arc dropped into the
folder overwrites the matching installed quest. `mhnx install` is protected only because it parses
first (`crates/mhnx-cli/src/main.rs:205`).

Repro:

```rust
let mut s = Save::from_bytes(&std::fs::read(SAVE)?)?;
assert!(dlc::install_quest(&mut s, 1010005, &[0xAB; 777]).is_ok());   // no .arc at all
let q = dlc::list_quests(&s).into_iter().find(|q| q.quest_id == 1010005).unwrap();
assert_eq!(q.size, 777);
assert!(Arc::parse(&q.data).is_err());                                 // payload is gone
```

### E2 (high) — `.gmd` string count is an unvalidated allocation: `crates/mhnx-core/src/quest.rs:142,153`

```rust
let count = read_u32_at(b, 0x18) as usize;   // straight from the file
...
let mut strings = Vec::with_capacity(count);
```

`key_len` is range-checked; `count` is not. A 64-byte `.gmd` with `+0x18 = 0xFFFFFFF0` makes the
library ask for 4,294,967,280 * 24 B ~= **96 GiB**. Measured: on this x86-64 Windows host the call
returned `Ok(22)` because Windows overcommits; on a 32-bit/alloc-limited target (the Switch NRO
builds this same crate, `switch/build.ps1`) `with_capacity` fails and the process aborts. Reached
from the GUI by opening any `.arc` (`open_arc` -> `run_quest_check` -> `check_quest` ->
`Gmd::from_bytes`), i.e. from a downloaded file.

Repro (`probe/src/bin/gmdoom.rs`):

```rust
let mut b = vec![0u8; 0x40];
b[0..4].copy_from_slice(b"GMD\0");
b[0x18..0x1C].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
b[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());  // key len
b[0x28] = b'a'; b[0x29] = 0;
let _ = Gmd::from_bytes(&b);                          // requests ~96 GiB
```

### E3 (medium, functional) — `.sup` is documented as 40 records but only a prefix is parsed: `crates/mhnx-core/src/entries.rs:100-112`

`parse` breaks at the first all-zero record. Real supply containers hold **two blocks**: measured
119 of 316 `.sup` entries contain a non-zero record after an empty run, e.g. `q1010001.arc`
`quest\supp\supp_1010001` parses 22 records and then has **8 more real supply items at indices
32..39** (`06b4 x3 s4`, `06b6 x3`, `06b8 x2`, `0046 x2`, `0065 x30`, `006a x10`, `00d0 x10`,
`00cf x10`). The GUI therefore shows "アイテム 22 件 / 収容 40 件" and those 8 items can neither be
seen nor edited.

No corruption: `to_bytes` only rewrites the parsed prefix and `raw` keeps everything else
(verified: 316/316 identity), so the hidden block survives a save. But the container description in
`entries.rs:14-17` ("records of 4 bytes, same shape, 40 of them") and the comment at
`entries.rs:122-128` imply this was handled; it was only half-handled (no wipe, still unreadable).

Repro: parse `dist/quests_all/q1010001.arc`, take the `sup` entry, `ItemTable::parse(...).records.len()
== 22` while `capacity == 40` and records 32..39 are non-zero.

### E4 (low, documentation) — the `.rem` header fields at `+0x08`/`+0x0A` are mis-described: `crates/mhnx-core/src/entries.rs:10-11`

The doc says "`+0x08` u16 slot count for the first group, `+0x0A` u16 slot count for the second
group". Read as u16 the value is 0x0901 (2305) for `rem_610001`, 0x0303 (771) for `rem_610002`,
0x0A01 for `rem_611001` — while the record capacity is 40. Bytewise `+0x08` takes
{0,1,2,3,5,8,15,35,36,37,45,48} over the 1,460 files and `+0x09` takes 0..9; `+0x0A..+0x18` is
non-zero in 99 of 1,460. Nothing in the library writes these bytes, so this cannot corrupt anything —
but the line is wrong and must not be used as the basis for an editor.

### E5 (low, latent round-trip violations on crafted input) — `crates/mhnx-core/src/arc.rs:168,234,238`

Three write-side normalisations that are invisible on real data but make `parse -> to_bytes` lossy
for crafted archives:

* `arc.rs:234` writes at most 63 name bytes and never writes the 64th (a 64-byte name comes back
  with its last byte dropped). Measured 0/4,309 real entries have a 64-byte name.
* `arc.rs:168` skips the size check when the stored length word is 0 (`if usize_ != 0 && ...`);
  `arc.rs:238-240` then always writes `0x4000_0000 | len`, so a file with the low 30 bits zero is not
  reproduced. Measured 0/4,309 mismatches.
* Payload intervals are assumed unique; two entries pointing at one payload would be written twice
  and the file would grow. Measured 0/316 files with duplicate offsets.

---

## Residual risks (verified statements, no exploit found)

* **`system_backup` goes stale.** The real dump is a pair (`system`, `system_backup`, bodies
  identical). 上書き保存 (`app.rs:642-646`), `mhnx install`, `cat-add`, `cat-del`,
  `slot-import` write **one** file and leave the other at the old state. The tool has a correct
  writer for the pair (GUI `write_switch_pair`, Switch port), but no default backup and no warning
  beyond a console NOTE. If the console/Ryujinx falls back to the backup, the edit is lost or, worse,
  the pair disagrees.
* **No pre-write backup anywhere.** First write overwrites the user's only copy in place.
* **Unsaved edits are discarded silently.** `reload_from_save` on slot switch (`app.rs:795-804`) and
  on save close wipes `self.edit` / `self.items` with no prompt; there is no dirty check on exit
  (`dirty` is only a bottom-bar indicator). Loss of the user's work, not of save bytes.
* **Character names can be cut mid-UTF-8** (`save.rs:412-417`, `min(31)`): a Japanese name of 11
  characters is 33 bytes, so 31 bytes = 10 characters + the first byte of the 11th. The save then
  holds invalid UTF-8 in a 32-byte field (the library reads it back lossily). I cannot verify what
  the game does with that; it is not a bounds/safety issue. Also note the 32-byte field can hold at
  most 10 Japanese characters, which the GUI does not communicate.
* **`install_dlc_palico` decides "free" from the id byte alone** (`save.rs:584-589`). Measured: 43 of
  the 50 records have id 0 but are **not** blank — each has 25 non-zero bytes, identical
  (`0x100..0x144` = `ff*14, 00*15, (ff 00 00 00)*9, 00, ff, ff` shape). All 43 share the same tail, so
  it looks like a fixed empty-slot pattern and overwriting it is presumably correct, but the test is
  "id == 0", not "record is empty". `remove_dlc_palico` does zero the whole record, so the tool
  creates both shapes of "free".
* **`ItemTable::parse` dispatches on length, not type** (`entries.rs:90`):
  `if raw.len() == 184 { 0x18 } else { 8 }`. Fine for all real data (rem=184, sup=168) but a .rem of
  any other size is silently parsed as a .sup (records from +0x08 instead of +0x18) and writing it
  back would corrupt the header region. `check.rs` warns about non-184 .rem but does not prevent it.
* **Item box silently masks out-of-range values**: `itembox::encode` keeps 7 bits of `count` and
  12 of `id`; encode(id 5000, count 200) decodes as id 904, count 72 (measured). The GUI's ranges
  (0..=4095 / 0..=127) keep this unreachable from the UI; the library API does not.
* **MHGU and 3DS saves are unverified.** I have no MHGU or 3DS dump. Writing is refused for MHGU
  quests (`dlc.rs:109-116`) but every slot offset in `save.rs` is MHXX-verified only, and
  `MHGU_SLOT_STRIDE` is untested here. `Save::from_bytes` for a 3DS body was exercised only through
  the synthetic `empty_switch_save()` template.
* **`install_quest` is not atomic** but cannot fail after its first write: the size check, catalogue
  check and slot lookup all happen before `body_mut()`. Verified by construction and by the
  `a_refused_install_must_not_write` unit test.

---

## Prioritised fixes

1. **`dlc.rs:273` — refuse a payload that does not parse.** Change
   `if let Ok(arc) = Arc::parse(arc_bytes)` to bind the error and return it (and require a `.mib`
   whose id matches). One-line blast radius, removes the only path I found that silently destroys
   an installed quest.
2. **`quest.rs:142` — bound the `.gmd` string count.** Clamp `count` to the number of NUL-separated
   strings that can possibly fit (`text.len() + 1`), or just `Vec::new()`. Removes a 96 GiB
   allocation driven by a downloaded file.
3. **`entries.rs:100-112` — parse all `capacity` records of a `.sup`** (keeping a sparse/empty flag,
   or at least not truncating `records` at the first empty record) so the second block is editable.
   No risk in the writer: writing only the records that are held is already correct.
4. **`entries.rs:90` — dispatch on the declared type** (`parse_rem`/`parse_sup` from
   `extension_for(type_hash)`) instead of on the payload length.
5. **Save the pair, and back up first.** Before overwriting, copy the target to `<name>.bak` and write
   `system` + `system_backup` together (the GUI already has `write_switch_pair`; reuse it).
6. **Prompt when `dirty` on slot switch / save close** (`app.rs:795-804`).
7. **`arc.rs:234`** — NUL-terminate a 63-byte name or refuse names >= 63 bytes; preserve a clear
   bit 30 in the length word instead of forcing `0x4000_0000`.

---

## What I did not verify

* Real 3DS and MHGU saves (none available): every layout number is MHXX-Switch only.
* What the game itself does with an edited quest, a 31-byte-cut name, an invalid charm skill id, or
  a slot whose 8-bit-clean stale tail differs from the new payload.
* The Switch NRO build (no devkitPro toolchain run here); E2's 32-bit abort is inferred from
  `Vec::with_capacity` semantics, not measured on hardware.
* `crates/mhnx-gui/src/app.rs` beyond the write paths listed above (layout/behaviour of the
  reconstructed UI is out of scope for this pass).
