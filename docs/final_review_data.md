# Final review: data integrity of the charm / save / dlc / arc code

Save under test (read only, never written):
`C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`
4,726,188 B = 36 B header + 4,726,152 B body, SHA-256
`D06B72520E379F462D49098E55C270BA48B203C10A9DBF39A1F8EE7E410C112A` (unchanged after this review).

Method: my own Python parser over the whole file, my own Rust probe crate
(`C:\anyWorkspace\_final_review\probe`, outside the repo, `cargo run --offline`, 8 sections of
checks on in-memory copies), the project's own test suites, and the independent C# reference
editor (`nx/mhxx/save_editor/repos/MHXX-GUSaveEditor`) for the skill/rarity tables. No project
code was trusted for any measurement quoted as "measured".

---

## Verdict

The three record-level fixes are real and I could not break them:

* `update_charm` writes exactly five bytes and nothing else - verified over **all 650** talismans
  (identity write = 0 changed bytes; hostile write with skill/slots/points at their extremes =
  0 changed bytes outside `+0x0C..+0x10`).
* No path turns a weapon or a piece of armour into a talisman any more; record 0 of slot 0
  (`47 01 1a ..`, Great Sword) survives every charm entry point.
* `install_quest` no longer blanks the slot: re-installing a quest with its own bytes leaves the
  whole save byte-identical.
* The arc contiguity change is right for **every** archive that matters: all 175 files in
  `dist/quests_all` (and all 175 payloads inside the real save, which are byte-identical to them)
  are packed contiguously from `align32(12+80*n)`, and my independent repack of each of the 175 is
  byte-identical.

But four claims in the change list do not survive, and one created-taliman defect from the previous
review survives through the new code path:

1. `slot_field()` does **not** bound every slot accessor. `read_character`, `write_character`,
   `read_item_box` and `write_item_box` still slice blindly and **panic** on a corrupt slot base
   that `slot_base()` accepts. The GUI calls two of them while loading a save (`app.rs:152`,
   `:160`) and hbedit a third (`hbedit.rs:428`), both written as if they returned `Option` on a
   bad base.
2. `template_from_box` picks the **last** record of the modal class, which in slot 0 is record
   **#1994** - one of the 15 records in the whole save whose `+0x06..+0x09` are non-zero. Every
   talisman created in slot 0 therefore clones `8c 0a 97 0a` from another talisman.
3. A created talisman is still game-illegal by default: the template fixes the rarity to 100
   (Enduring) but the skills/points come from the form, and the reference table gives skill 1
   ("Poison") the range `0~0` at that rarity. Neither `create_charm` nor `update_charm` ever
   consults the rarity. 0 of the 580 rarity-100 talismans in this save uses skill 1 as its first
   skill.
4. `arc::tests::reparsing_and_repacking_is_byte_identical` cannot run: its path literal has lost
   every backslash and now contains a line break (`arc.rs:255-256`), so `src.exists()` is false
   and the test returns. If the literal is repaired to the file it names (the shipped
   `mhxx_quetool/quest/q1010165.arc`), the assertion `again == bytes` **fails**: the writer
   normalises that file from 2233 B to 2217 B.

And one more write path still reaches the equipment box: with a corrupt `body+0x0C`,
`dlc::uninstall_quest` (GUI button, `app.rs:545`) zeroes 5120 bytes of it (measured; see E6).

---

## CONFIRMED (my own measurements)

### 1. Talisman layout over all 650 talismans (188 + 231 + 231)

Parsed with my own Python script straight from the file; slot bases read from `body+0x10/0x14/0x18`
= `0x126474 / 0x244C34 / 0x3633F4`; box = `base+0x62EE`, 2000 x 36 B.

| field | measured over all 650 |
|---|---|
| `+0x00` | `0x06` in all 650 (level bits `(b1<<3)|(b0>>5)` = 0 in all 650) |
| `+0x01` | `0x00` in all 650 |
| `+0x02` | **not constant**: `0x0A` x580, `0x07` x46, `0x06` x20, `0x00` x2, `0x04` x2 |
| `+0x03..+0x05` | `0x00` in all 650 |
| `+0x06..+0x09` | **non-zero in exactly 15 of 650** (13 in slot 0, 1 in slot 1, 1 in slot 2) |
| `+0x0C..+0x0F` | skill ids and signed points (all 650 fit the reference range table, see below) |
| `+0x10` | 3 x590, 2 x18, 1 x10, 0 x32 |
| `+0x11` | `0x00` in all 650 |
| `+0x12` | **not constant**: 100 x580, 99 x68, 98 x2 |
| `+0x13` | `0x01` in all 650 |
| `+0x14..+0x23` | all zero in all 650 |

The pair `(+0x02, +0x12)` is linked, exactly five classes:
`(0x0A,100) x580, (0x07,99) x46, (0x06,99) x20, (0x00,99) x2, (0x04,98) x2`.
The reference table `TalismanRarity = {0,97,97,98,98,99,99,99,100,100,100}` reproduces every pair
except the two id-0 records (table says 97, save says 99).

The 15 records with non-zero `+0x06..+0x09` are **all in the modal `(0x0A,100)` class**:
slot 0 #1408 `8f0a`, #1415 `8f0a`, #1423 `d60a`, #1425 `d60a`, #1450 `8f0a`, #1459 `ce0a`,
#1478 `6d0a`, #1508 `d00a`, #1586 `d60a`, #1959 `8f0a`, #1967 `d60a`, #1972 `da0a`, #1994
`8c0a970a`; slot 1 #281 `ce0a`; slot 2 #281 `ce0a`. (`+0x07` and `+0x09` are always `0x0A` when
set, so the four bytes look like two little-endian u16s in the 0x0A6D..0x0ADA range, i.e. 2669..2778
- a serial/seed-like quantity, not something an editor can invent.)

Cross-check that the fields are read at the right offsets, independent of the editor: applying the
C# editor's per-rarity point tables (`TalismanMystery/Shining/Timeworn/EnduringSkill`) to all 650
records gives **0 violations**, and reading the points as i8 is required for that (6 records carry
`0xFF/0xFE/0xFD` at `+0x0F`). Read as u8 they are impossible.

### 2. `update_charm` cannot change `+0x02`, `+0x12`, `+0x13` or `+0x06..+0x09`

Probe section F, run over the real box of every slot:

* `update_charm(box, c)` with the decoded values of each of the 650 talismans:
  **0** records changed a single byte (identity write is a true no-op).
* `update_charm` with `slots=(slots+1)%4, skill1=255, skill2=254, points1=-128, points2=127`:
  **0** bytes changed anywhere in the box outside `+0x0C..+0x10`; the five owned bytes hold exactly
  the values asked for.

`create_charm` was checked the same way: the created record equals the template **byte for byte
outside `+0x0C..+0x10`**, i.e. `+0x02`, `+0x12`, `+0x13` and `+0x06..+0x09` do come from a real
record. `set_charm` refuses a weapon (`regression_review.rs` and probe), `first_writable` returns
`None` on a box that has neither a blank record nor a talisman, and the box-full case of the old
bug is gone.

### 3. `dlc` bounds and MHGU refusal

* `check_kind` is called by `bitmask`, `list_quests`, `equipment_catalogue_index`,
  `install_quest`, `uninstall_quest`, `find_quest` (and therefore by `installed_flag` and
  `installed_bit_count`): every public entry point refuses MHGU.
* Corrupt section pointers (`0`, `1`, `2`, `len-4`, `0x1000`, `0xFFFFFFF0`) and a payload whose
  size runs past the body: no panic in any of them (probe section B, dlc unit tests).
* A no-op `install_quest` leaves `to_bytes()` byte-identical (`regression_review.rs:163` and
  `quest_injection.rs`), including the slot id and the installed bit set.

### 4. `arc` contiguity

My own analyser over **all 175** `dist/quests_all/*.arc`:

* `offset[0] == align32(12 + 80*count)` in 175/175;
* every payload contiguous (`offset[i+1] == offset[i] + csize[i]`) in 175/175;
* `+0x48 == 0x4000_0000 | uncompressed_size` in every entry; no entry has bit 31 set;
* no trailing bytes after the last payload, no non-zero padding, no duplicate offsets, no
  name-padding anomalies;
* reconstructing the writer's output (same streams, recomputed directory) reproduces **all 175
  files byte for byte**.

The 175 quest payloads stored inside the real save are byte-identical to those 175 files and are
likewise contiguous, so the packing rule matches what the game itself wrote.

---

## ERRORS

### E1 (panic, reachable) - four slot accessors still slice blindly

`crates/mhnx-core/src/save.rs:347-390` (`read_character`, first slice at `:349`),
`:397-436` (`write_character`, first slice at `:406`), `:439-444` (`read_item_box`, `:442`),
`:447-454` (`write_item_box`, `:452`). None of them calls `slot_field`; `slot_base` (`:309-319`)
only rejects `0` and values `>= body.len()`.

Repro (probe section A, `C:\anyWorkspace\_final_review\probe`):

```text
set body[0x10..0x14] = body.len()-16   (passes slot_base)
read_character(0)  -> panic: range start index 4872437 out of range for slice of length 4726152   [save.rs:349]
write_character(0) -> panic: range start index 4872437 out of range for slice of length 4726152   [save.rs:406]
read_item_box(0)   -> panic: range start index 4726768 out of range for slice of length 4726152   [save.rs:442]
write_item_box(0)  -> panic: range start index 4726768 out of range for slice of length 4726152   [save.rs:452]
```

The same happens for `body.len()-1`, `body.len()-0x62EE`, `0x481BB4` and for all three slots.
The fix list of the previous review item 3 ("return None unless `base + OFF + SIZE <= body.len()`")
was applied to the accessors that already went through `slot_field` - `equipment_box`,
`palico_equipment_box`, `palico_data`, `guild_card`, `shoutouts`, `charms`, `export_slot`,
`import_slot` all behaved (None/Err) for all 8 corrupt bases x 3 slots - but not to these four.

Reachability: `app.rs:152` / `:160` (save loading), `app.rs:172` / `:177`, `hbedit.rs:428`,
`hbedit.rs:496`, `hbedit.rs:1308`, `cli main.rs:90/103/312/329/365`. All of them are written as if
the accessor returned `None` for a bad slot; instead the GUI and the TUI die with a panic on a save
whose slot base field is damaged but whose magic and size are intact.

### E2 - `template_from_box` clones one specific talisman's unknown bytes

`crates/mhnx-core/src/charm.rs:199-231`. The loop keys on `(rec[0x12], rec[0x02])` and replaces
`best` whenever a running count exceeds the previous best, so the winner is the **last record of the
modal class**, not a representative record. Measured (probe F, and a Python port of the function):

| slot | template record | `+0x06..+0x09` of the template |
|---|---|---|
| 0 | **#1994** | `8c 0a 97 0a` |
| 1 | #1980 | `00 00 00 00` |
| 2 | #1980 | `00 00 00 00` |

So every talisman created in slot 0 inherits `8c 0a 97 0a` from #1994 (probe: created record =
`06 00 0a 00 00 00 8c 0a 97 0a 00 00 01 c8 01 fe 03 00 64 01 00 ...`). This is exactly the class of
byte the previous review asked not to invent or lose, and it is silently copied by a picker whose
docstring says the modal pair makes the result representative. 565 of the 580 modal records have
`+0x03..+0x0B` all zero and would be a safe template; the picker never prefers them.

### E3 - a created talisman is still impossible in game, by default

`charm.rs:175-193` (create) and `:154-168` (update) never look at the record's rarity, and the
defaults are illegal for the rarity the template supplies:

* `hbedit.rs:566-574`: fresh charm = `skill1 = 1, points1 = 1`; `app.rs:116-117`:
  `charm_skill1: 1, charm_points1: 1`.
* Template rarity is 100 (Enduring). Reference table `GameConstants.cs:51`,
  `TalismanEnduringSkill[1] = "0~0:0~0"` - skill 1 may never carry a point at that rarity.
* Measured: **0 of the 580** rarity-100 talismans uses skill 1 as its first skill, while all 650
  real records satisfy the reference table (0 violations).
* The same applies to the tests' sample charm `skill2 = 200, points2 = -2`
  (`webapp_features.rs:100-107`, `regression_review.rs:108`): skill 200 is `0~0:0~0` at rarity 100
  in the reference table.
* Editing an existing rarity-99 record with the form's values has the same problem: the record keeps
  its own rarity but the values are unvalidated, so a legal Timeworn record can be turned into an
  out-of-table one without any warning.

The previous review raised this as "new bug 2"; the rewrite moved the fixed rarity from a constant
to a copied template, but the value validation is still absent, so the defect survives end to end.

### E4 - the only arc byte-identity test cannot run, and its target would fail

`crates/mhnx-core/src/arc.rs:253-266`:

```rust
let src = std::path::PathBuf::from(
    r"C:anyWorkspace
xmhxxsave_editormhxx_quetoolquestq1010165.arc",
);
if !src.exists() { return; }
```

The literal contains a real newline and no directory separators (verified byte for byte:
`b'            r"C:anyWorkspace'` / `b'xmhxxsave_editormhxx_quetoolquestq1010165.arc",'`), so
`exists()` is false and the test returns - `cargo test` prints it as `ok`. The path it names is
`.../save_editor/mhxx_quetool/quest/q1010165.arc`, and that file is the one archive out of 141
whose payload area starts at 1232 instead of `align32(dir_end)=1216`. Probe G:

```text
q1010165.arc (dist/quests_all, 2222 B): repack 2222 B, identical true
q1010165.arc (shipped 141 set, 2233 B): repack 2217 B, identical false, first diff at byte 88 (= entry 0 offset field)
```

So repair of the literal must point at the `dist/quests_all` copy, or the writer must preserve the
original first-payload offset for untouched archives.

### E5 - `to_bytes_with_level(level)` ignores `level` for untouched entries

`arc.rs:170-197`: an entry with `original = Some((packed, decoded))` and `decoded == data` is
written from `packed` regardless of `level`. Measured: `to_bytes_with_level(0)` and
`to_bytes_with_level(9)` produce **identical** output and both equal the original file for an
unmodified archive; for an edited archive they differ (2244 B vs 2223 B). This is a defensible
trade-off for byte stability, but the function documents itself as "serialise with an explicit zlib
compression level", and callers cannot force a recompression. Either honour the level when it
differs from the stored stream, or rename/document the exception.

### E6 - `uninstall_quest` / `install_quest` trust `body+0x0C` and can write into a character slot and into the equipment box

`dlc.rs:130-166` and `:236-291` bound the *offset arithmetic* but never check that the section
pointer lands somewhere sane. With `body[0x0C..0x10] = 0x12816E` (= equipment box - `0x45F4`),
`uninstall_quest(save, 0)` finds "slot 0" exactly on top of the equipment box and zeroes it
(probe I):

```text
equipment-box aim sec=0x12816e: uninstall ok=true,
record 0 before 47 01 1a 00 ...   after 00 00 00 ...   bytes of box zeroed in first 0x1400: 5120
```

With `sec = 0x121E80` (= slot 0 base - `0x45F4`) the same call zeroes 4605 non-zero bytes at the
start of character slot 0, and `list_quests` first reports 63 bogus quests. Both are reachable from
the GUI's quest-delete button (`app.rs:545`). This is the only remaining way to write into the
equipment box without going through the charm guard (see task 5 below); it needs a damaged
`body+0x0C`, but the module's own model knows the arrays must end at the first slot base, so the
check is one line.

### E7 - documentation still wrong in the places that were supposed to be fixed

* `save.rs:10`: "last 660 bytes - three localised 220 byte strings". Measured tail:
  `body.len() - (0x3633F4 + 0x11E7C0) = 0x1D4 = 468` bytes (all zero in this save).
* `save.rs:456` ("Decode the charm box"), `:478` ("Delete one charm from the charm box") still
  call the shared equipment box a charm box; `charm.rs` itself documents the truth.
* `arc.rs:16-19` documents the one archive with a different payload start as handled ("every entry
  stores its own offset"); the writer does not preserve that offset (E4).

---

## Remaining risks

1. **No skill/point validation anywhere** (`charm.rs`, GUI `app.rs:1878-1899`, hbedit
   `hbedit.rs:630-637`). `skill1/skill2` accept 0..=255, points -127..=127, and the record's
   rarity - which picks the legal range in the game - is never consulted. A user can build records
   that match no real talisman (E3) without any warning.
2. **`+0x06..+0x09` are still not understood** and are now propagated to new records (E2). 15 of
   650 real records carry them; nothing in the tree explains them.
3. **`slot_base` accepts any in-range value.** A base that points at the middle of the shared block
   makes `read_character`/`read_item_box` return plausible garbage, and `write_character` then
   writes that garbage back into the shared block. No check that a base is a multiple of the stride
   or that `base + stride <= body.len()`, and the three bases are not required to be ordered.
4. **MHGU**: `save.rs`'s slot-relative offsets (name, box, palico, guild card) are applied to MHGU
   saves without a kind check, while `dlc` refuses MHGU. The MHGU blank template's slot base is
   `0x18CC78` (not `0x126474`) and its slot is empty, so the MHGU equipment-box offset is unverified;
   a charm write on an MHGU save may hit the wrong region. Low confidence, untested for lack of data.
5. **`skills.rs` disagrees with the reference table for ids >= 192** (project: 192 `Readiness`,
   200 `Anti-Kushala`, 205 `Torso Up`; C#: 192 `Sheathed`, 200 `Anti-Steel`, 205 `Talisman+`).
   The save only uses ids 5..179, where the two agree apart from abbreviations, so nothing in this
   dump is affected - but a user picking id 200 is shown a different skill than the reference means.
6. **`refresh_header` (`save.rs:718`) still has no call site** (only the definition matches a
   whole-tree grep), so writing the save under the name `system_backup` keeps the CRC of
   `system`. Carried over from the previous review, unchanged.
7. **Silent skips**: all 9 tests in `webapp_features.rs` begin with `if !p.exists() { return; }`,
   so on any machine without this dump the file is green while testing nothing.
   `regression_review.rs` fixed this with a panic plus `MHNX_SKIP_REAL_SAVE` - the same pattern
   should be used everywhere.

---

## Vacuous / unrepresentative tests

|crate file:line|test / assertion|why it cannot fail|what would make it real|
|---|---|---|---|
|`regression_review.rs:71`|`the_equipment_box_is_shared_with_weapons_and_armour`: `assert_eq!(talismans, src.charms(0).unwrap().len())`|both sides run `is_talisman` over the same records - equal by definition|assert the measured counts: 188 talismans, 601 equipment, 1211 blanks in slot 0 (231 talismans / 540 equipment / 1229 blanks in slots 1-2)|
|`regression_review.rs:115-117`|`a_new_charm_record_matches_a_real_one` compares 7 offsets with slot-0 record #224|six of the seven bytes come from the template, and #224 is a modal-shape record that shares them; the test is slot-0-only and never looks at `+0x06..+0x09`, where the created record actually differs from #224|compare the created record against `template_from_box(box)` for **all 36** bytes (equal outside `0x0C..=0x10`, the user values inside), and require the template's `+0x06..+0x09` to be zero|
|`regression_review.rs:143`|`assert_eq!(src.options().brightness, src.body()[0x28])`|`options()` is defined as `body[0x28]`|drop it; the paragraph above (zero DLC pointer) is the real assertion|
|`webapp_features.rs:33-58`|`options_match_the_web_app_layout`|`rumble/disable_3d/circle_pad_pro <= 1` is true for the bytes 1/0/0 whatever their offsets; the rest is `options()` -> `set_options()` -> `options()`|assert the four measured values `(0x30, 1, 0, 0)` and the absolute write targets `0x28 / 0x29 / 0xB27E / 0xB27F`|
|`webapp_features.rs:126-147`|`gender_is_written_to_both_copies`|it writes `c.gender` to `base+0x2A` and `base+0x23B4B` and then reads those two addresses; a wrong address passes. The name comparison compares two reads of the same field|assert that exactly those two bytes changed (count the diff over the whole body) and that both held the same value before the write|
|`webapp_features.rs:68`|`assert_eq!(cats.len(), 50)`|`dlc_palicoes()` is a loop over `0..DLC_PALICO_COUNT`|assert the 7 installed indexes and ids (0/1/9/11/12/13/14 = 0x64,0x65,0x97,0x99..0x9C)|
|`webapp_features.rs:86`|`assert_eq!(0x62EE + CHARM_BOX_SIZE, 0x17C2E)`|two constants restated|assert the type histogram of the real box, or that the region starting at `0x62EE+72000` holds only types 22/23/24|
|`webapp_features.rs:236-250`|`download_palicoes_can_be_installed_and_removed` final round trip|the record is removed before `to_bytes()`, so serialisation is only checked for the removed state|do the `to_bytes()` round trip while the Palico is installed|
|`webapp_features.rs` (all 9)|`if !p.exists() { return; }`|on a machine without the dump every test passes having asserted nothing|use the `real_save()` helper of `regression_review.rs` (panic unless `MHNX_SKIP_REAL_SAVE` is set)|
|`arc.rs:253-266`|`reparsing_and_repacking_is_byte_identical`|path literal is corrupted (contains a newline, no separators), so it returns at `if !src.exists()`|fix the literal to `dist\\quests_all\\q1010165.arc`; then also assert the shipped 2233 B file's behaviour explicitly|

Everything else in the two files is a real check: the weapon-refusal test, the level-bits test, the
quest no-op and single-slot-diff tests, the slot offset/stride anchors, the Palico id/name anchors,
the export/import size checks and the `installed_palico_survives_a_save_round_trip` test all
measure the file or a real property.

Test status when run (`cargo test -p mhnx-core --offline`, 2026-10-19): lib 33/33,
regression_review 8/8, webapp_features 9/9, hbedit_flow 21/21, real_data 7/7, quest_injection 6/6,
check_quests 4/4. A probe file `crates/mhnx-core/tests/_final_probe.rs` (not present when this
review started, presumably another agent's leftover) currently fails (`p3_editor_flow_edit_then_install`)
and makes a plain `cargo test` stop at that binary; it is not part of this review and was left
untouched.

---

## Prioritised fixes

1. **Bound the four accessors (E1).** Route `read_character`, `write_character`, `read_item_box`,
   `write_item_box` through `slot_field`/`slot_field_mut` and return `None`/`Err` when the
   required range does not fit; add a probe test with `body[0x10..0x14] = body.len()-16` that calls
   every public accessor and asserts no panic.
2. **Make the template picker safe (E2).** Choose the first modal-pair record whose `+0x03..+0x0B`
   are zero (565 of 580 qualify), or copy only `+0x00, +0x02, +0x12, +0x13` into a zeroed record;
   assert in a test that two created talismans differ only in the owned bytes and that
   `+0x06..+0x09` are zero.
3. **Validate the values against the rarity (E3).** Ship the four per-rarity range tables (or at
   least the first-skill range) and refuse/clamp out-of-table combinations; change the fresh-charm
   default from `skill1=1, points1=1` to a legal pair for the template's rarity. Update the two
   tests that currently enshrine `skill2=200, points2=-2` at rarity 100.
4. **Repair the arc test and the level contract (E4, E5).** Fix the literal, point it at
   `dist/quests_all/q1010165.arc`, add a case for the shipped 2233 B file that documents the
   normalisation, and either honour `to_bytes_with_level`'s level or rename it.
5. **Sanity-check the section pointer before writing (E6).** Refuse `install_quest`/`uninstall_quest`
   unless `sec + 0x45F4 + 205*0x1400 + 45*0x800 <= first_slot_base` (or at least unless the whole
   array is inside the shared block and does not overlap a slot base).
6. **Turn the silent skips into hard failures** in `webapp_features.rs` (reuse
   `regression_review.rs::real_save`).
7. **Docs**: `save.rs:10` 660 -> 468; `save.rs:456/478` "equipment box"; `arc.rs:16-19` note the
   q1010165 exception; document `+0x06..+0x09` as unknown-but-copied.

---

## Reproducing this review

* Python parsers: `C:\anyWorkspace\_final_review\measure.py` (layout), `tmpl.py`
  (template selection), `arcs.py` / `arcs141.py` (contiguity), `legal.py` (range tables),
  `skills2.py` (skill table comparison). Run with the bundled interpreter.
* Rust probe crate: `C:\anyWorkspace\_final_review\probe` -`cargo run --offline` prints
  sections A..I (corrupt bases, corrupt DLC pointers, charm invariants, arc behaviour). It only ever
  works on in-memory copies; the real save's SHA-256 is unchanged.
* Project tests: `cd C:\anyWorkspace\nx\mhxx\mhnxeditor && cargo test -p mhnx-core --offline
  --test regression_review` (and the other five integration targets).
