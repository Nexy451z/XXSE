# MHXX save layout review - adversarial verification

Save under test (read only, never written):
`C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`
(4,726,188 B = 36 B header + 4,726,152 B body; `0\system_backup`, `1\system`, `1\system_backup`)

Method: my own Python 3 scripts (`tools/_review1.py` ... `tools/_review8.py`, run with the bundled interpreter) plus one
Rust probe (`crates/mhnx-core/tests/_review_save_layout.rs`) that works on in-memory copies only.
None of the author's scripts were re-run. Cross-references are to independent implementations that
exist on this machine: the MHXX/MHGU Save Manager web app (`refs/SJ-site/MHGU/save-manager/scripts/main.js`,
the app the author says he copied from) and the C# `MHXXSwitchSaveEditor` (`save_editor/repos/MHXXSwitchSaveEditor`,
an independent editor for this exact file format).

---

## Verdict

The container level (claims 1, 2, 3, 8) is **correct and independently confirmed**, the character-slot
field map (claim 4) is **correct**, the download-Palico block (claim 6) is **correct**, and the charm
*field* offsets (claim 7) are **correct** - they agree with both independent implementations and produce
plausible charms. The download-quest section at body+0x0C is confirmed by a strong structural proof
(175 valid quests ending exactly on slot 0's base).

Four real defects: (1) the 72,000-byte region at slot+0x62EE is the shared **equipment box**, not a
"charm box" - 601 of its 789 used records in slot 0 are weapons/armour, and the editor's "create a new
charm" path therefore overwrites a weapon with a charm; (2) the record type test `byte0 == 0x06` misses
talismans whose level bits are set (type = byte0 & 0x1F, level = (byte1<<3)|(byte0>>5)); (3) a freshly
created talisman record is structurally incomplete (no equip id, rarity and tail bytes left 0 where all
real talismans have rarity 98..100 and tail 1); (4) the documented option values ("brightness 24",
"brightness 0..=31") are false for this file (0x30 = 48). Claim 5's *offsets* match the reference for
MHXX, but the reasoning about MHGU and the quoted values do not survive contact with the data.

---

## CONFIRMED claims (measured)

### Claim 1 - body+0x00 marker and slot flags: CONFIRMED
```
body+0x00 = c6 00 00 00           -> 0xC6 marker
body+0x04..0x06 = 01 01 01        -> three slots used (body+0x07 = 00)
```
All three characters exist ("ナナ" x3), so 01 01 01 is expected. The C# editor declares the same three
one-byte flags (`Data/Offsets.cs:6-8`: FIRST/SECOND/THIRD_CHAR_SLOT_USED = 0x04/0x05/0x06).

### Claim 2 - slot bases live in the save: CONFIRMED
```
body+0x10 = 0x00126474   body+0x14 = 0x00244C34   body+0x18 = 0x003633F4
stride    = 0x244C34-0x126474 = 0x11E7C0 (= 0x11D088 + 0x1738, exactly as documented)
```

### Claim 3 - body+0x0C = download-quest section offset: CONFIRMED (independently reproducible)
```
body+0x0C = 0x0000B280
0xB280 + 0x45F4 + 205*0x1400 + 45*0x800 = 0x126474  == slot 0 base
```
My own walk of the section (no library code) found **175 records** (the number stated for this save),
every one starting with `ARC\0`, quest ids 1010001..1120012, all unique, no malformed size, and the
installed bit set at 0xB280+0x4C (28 B) has popcount **175**.
Corroboration of the "MHGU is one byte later" remark: the blank MHGU template
(`save_editor/repos/MHGU-MHXX-Save-Converter-Script/Blank_Switch_Save/MHGU/system`) has
body+0x0C = **0xB281**, while both MHXX saves (real and blank) have 0xB280.

### Claim 4 - character slot layout: CONFIRMED (value by value)
```
slot 0 base 0x126474                slot 1 base 0x244C34        slot 2 base 0x3633F4
+0x00   32 B name preview "ナナ"      "ナナ"                      "ナナ"
+0x23B7D 32 B full name "ナナ"        "ナナ"                      "ナナ"      (identical to preview)
+0x20   u32 play time 2,409,628 s   2,363,516 s                 2,363,516 s
+0x2248B u32 play time (same value in all three slots)
+0x24   u32 funds 9,997,199         9,999,999                  9,999,999
+0x280F u32 funds (same value in all three slots)
+0x28   u16 HR 967                  999                        999
+0x2A   u8 gender 1 (female)        1                          1
+0x23B4B u8 gender 1 (agrees with +0x2A in all three slots)
+0x23B48 voice 10, +0x23B4C style 5, +0x23B4D hair 21, +0x23B4E face 2
```
Arithmetic: 0x62EE + 2000*36 = 0x62EE + 72,000 = **0x17C2E** (equals OFF_PALICO_EQUIPMENT), and
0x60CA + 548 (monster-size table) = 0x62EE, 0x278 + 5,463 (item box) = 0x17CF. The whole field map matches
the C# editor's `Data/Offsets.cs` (NAME 0x23B7D, PLAY_TIME 0x20/0x2248B, FUNDS 0x24/0x280F,
HR 0x28, VOICE/EYE/CLOTHING/GENDER 0x23B48..0x23B4B, PALICO 0x23BB6 = 84*324, EQUIPMENT_BOX 0x62EE,
PALICO_EQUIPMENT 0x17C2E). Note 0x23B4C (hunting style, value 5 here) is declared by the C# editor and is
**not modelled** by `Character` - an omission, not a corruption (write_character touches only its own fields).

### Claim 6 - download Palico block: CONFIRMED
```
50 records of 0x144 B from body+0x2A; id at +0x5F; name = first 32 B; block ends at 0x2A+50*0x144 = 0x3F72 < 0xB280
idx 0  id 0x64 ぽかぽか        idx 1  id 0x65 ミケゴールド    idx 9  id 0x97 ナナコＸＸ
idx 11 id 0x99 ジョイル－      idx 12 id 0x9a すえちょん      idx 13 id 0x9b とったん
idx 14 id 0x9c てちてち        -> 7 installed, exactly as stated for this save
```
The C#/SJ code uses the same base, stride and +0x5F id (main.js:463-467).

### Claim 7 - charm record fields: CONFIRMED (fields), see ERRORS for the type test
```
+0x0C skill1 (u8, 1-based), +0x0D skill2, +0x0E points1, +0x0F points2, +0x10 decoration slots
records with byte0 == 0x06:  slot 0 -> 188,  slot 1 -> 231,  slot 2 -> 231   (matches the stated contents)
first five of slot 0 (indices 224..228):
  #224 [3 slots] Crit Element +6 / Expert +10        raw 06 00 0a 00 .. 4a 46 06 0a 03 00 64 01
  #225 [3 slots] Crit Element +6 / Bomb Boost +10    raw .. 4a 5c 06 0a 03 ..
  #226 [3 slots] Crit Element +6 / Boltreaver X +3   raw .. 4a b1 06 03 03 ..
  #227 [3 slots] Chain Crit  +5 / Expert +10         raw .. 48 46 05 0a 03 ..
  #228 [3 slots] Chain Crit  +5 / Sharpness +7       raw .. 48 24 05 07 03 ..
```
Skill ids actually used span 5..179; **no** id > 306 anywhere in the 188/231/231 records (skills.rs has
exactly 306 entries), so the "id exceeds the table" failure mode does not occur here. The offsets match
`EditTalismanDialog.cs:19-31` (skill1/2 at +12/+13, level1/2 at +14/+15, slots at +16) and
main.js:338-350 - three independent implementations agree.

### Claim 8 - Switch container header: CONFIRMED, CRC recomputed by hand
```
+0x00..0x08 = 00*8      +0x08 = 04 00 00 00   +0x10 = 14 00 00 00
+0x14 = 74 ee b2 36     +0x18 = 0c 00 00 00   +0x1C = 88 1d 48 00 (=4,726,152)   +0x20 = 00*4
crc32("system")          = 0xC94D118B ; ^ 0xFFFFFFFF = 0x36B2EE74 == header +0x14
crc32("system_backup")   = 0x2F7E646D ; ^ 0xFFFFFFFF = 0xD0819B92 == header +0x14
0\system       vs 0\system_backup : 4 differing bytes, all at +0x14..+0x17
0\system       vs 1\system        : 0 differing bytes (the two files are byte-identical, headers included)
1\system_backup                   : header +0x14 = 92 9b 81 d0 (crc32 of "system_backup")
blank MHGU template               : +0x1C = 98 b8 4e 00 (=0x4EB898 = 0x4EB8BC-36)
```
`switch_header()` (save.rs:180-188) reproduces all 36 bytes exactly, for MHXX and for MHGU.

---

## ERRORS

### E1 (severe) - "+0x62EE charm box" is the shared equipment box; creating a charm destroys equipment
`crates/mhnx-core/src/charm.rs:3-5` ("Each character carries 2000 charm records ..."),
`crates/mhnx-core/src/save.rs:99-101, 452-455, 567-567`, `docs/save_manager_review.md:71-75`.

Measured, slot 0, box = body[0x126474+0x62EE .. +72,000):
```
789 used records = 188 talismans + 601 weapons/armour.  Type histogram (type = byte0 & 0x1F):
  1 Head 101, 2 Chest 101, 3 Arms 98, 4 Waist 95, 5 Legs 95, 6 Talisman 188, 7..20 weapons 101
record 0 = 47 01 1a 00 00 ...  -> type 7 "Great Sword" (GameConstants.cs:9 EquipmentTypes[7]),
                                 level (byte1<<3)|(byte0>>5) = 10, equip id = 0x001A = 26
```
The C# editor calls this `EQUIPMENT_BOX_OFFSET` with exactly 2000 x 36 (`Offsets.cs:82`).

Counterexample (executed in memory, `crates/mhnx-core/tests/_review_save_layout.rs`, test
`probe_charm_first_free_slot_is_a_weapon`):
```
charms decoded = 188, first index with no decoded charm = 0
record 0 before set_charm(0, Charm{index:0,..}) : 47 01 1a 00 00 00 00 00 00 00 00 00
record 0 after                                  : 06 01 1a 00 00 00 00 00 00 00 00 00
```
`hbedit.rs:515-520` picks that "first free slot" as `(0..2000).find(|i| !used.contains(i))`
(used = decoded talismans only) and `hbedit.rs:569` (key L) + `hbedit.rs:601-613` (key A) write there, so one
keypress replaces a Great Sword with a talisman. The GUI has the same exposure (`app.rs:1530` lets the
user type any index 0..=1999, `app.rs:1558-1572` writes it). No test notices: `hbedit_flow.rs:521-569`
does the same thing against the real save and only asserts that the charm count went up by one.

### E2 - the record-type test `== 0x06` misses real talismans
`crates/mhnx-core/src/charm.rs:88` (`if box_bytes[o] != CHARM_MARKER { continue; }`, marker 0x06 at `charm.rs:29`).

Byte 0 is a bitfield: type = `byte0 & 0x1F`, level = `(byte1 << 3) | (byte0 >> 5)`
(`MainForm.cs:338-339, 1651-1652`; `analysis/switch_save_format.json:559`). A talisman whose level bits are
non-zero has byte0 = 0x26/0x46/0x66/... and is silently skipped. Reproduced on an in-memory copy
(`probe_talisman_with_level_bits_is_invisible`): taking the real talisman at index 224 and setting byte0 = 0x26
makes `charms(0)` return 187 instead of 188.
In *this* save every talisman has byte0 = 0x06, byte1 = 0x00 (so the counts above are right), and no
non-talisman record has `byte0 & 0x1F == 6` - the bug is latent, not visible here. A laxer "looks valid"
test would be worse: byte0 = 0x26 with the same payload *is* a talisman.

### E3 - a charm written into a used equipment record is malformed
`crates/mhnx-core/src/charm.rs:104-116` writes only +0x00 (as 0x06), +0x0C, +0x0D, +0x0E, +0x0F, +0x10.
Consequences, both measured:
* writing over record 0 leaves byte1 = 0x01, so the new record is type 6 at **level 8**
  (`06 01 1a 00 ...`), not a clean talisman;
* a charm created in an empty slot is `06 00 00 00 ... 01 c8 06 fe 03 00 00 00 ...` - equip id 0,
  +0x12 (rarity) = 0, +0x13 = 0, whereas all 188/231 real talismans have +0x11 = 0, +0x12 = 98..100
  and +0x13 = 1 (`probe_new_charm_record_lacks_the_rarity_and_tail_bytes`).
The reference writes those bytes explicitly: `EditTalismanDialog.cs:41-47`
(`[+17] = 0; [+18] = TalismanRarity[equip id]; [+19] = 1;`, then zeroes +20..+35), and the game uses +0x12 to
pick the legal skill-point range (97..100 = Mystery/Shining/Timeworn/Enduring). Whether the game rejects
rarity-0 talismans cannot be tested here, but the record does not match any real one.

### E4 - documented option values and range are wrong
`crates/mhnx-core/src/save.rs:47-48` ("Screen brightness (0..=31)") and
`docs/save_manager_review.md:106, 157` ("明るさ24/振動1/3D0/CCP0").

Measured in `0\system`: body+0x28 = **0x30 (48)**, body+0x29 = 0x01,
body+0xB27E = 0x00, body+0xB27F = 0x00. 24 (0x18) is the value in the *blank* MHXX Switch template
(`Blank_Switch_Save/MHXX Switch Version/system`: body+0x28 = 0x18, body+0x29 = 0x01) and is exactly what the
web app hardcodes on export (`main.js:459-460`: `newData[0x28+off] = 0x18; newData[0x29+off] = 0x01;`). So the
"measured" values in the doc were taken from a converted/blank save, not from this save - and 48 is
outside the documented 0..=31 range. The live test already retreated from the claim
(`webapp_features.rs:41`: "Real saves hold brightness well above 31"), so doc and code now disagree.

### E5 - `docs/save_manager_review.md:108-109` states the wrong addresses
```
body+0x0C-2  3D 表示の無効化
body+0x0C-1  スライドパッドプロ
```
As written these are body+0x0A and body+0x0B (both 0x00 in this save) - a different place from the intended
"section offset minus 2/-1" (0xB27E/0xB27F). The parenthetical "DLC セクション位置から逆算" only hints at the
intent; the offset field is at body+0x0C, so the difference matters to anyone re-implementing.

### E6 - `options()` can panic (unsigned underflow)
`crates/mhnx-core/src/save.rs:481-482`:
```rust
disable_3d: self.body.get(dlc - 2).copied().unwrap_or(0),
```
`dlc` comes straight from the file (body+0x0C). With body+0x0C = 0 the debug build panics
(`attempt to subtract with overflow` at save.rs:481:39 - reproduced by
`probe_options_underflows_when_the_dlc_pointer_is_zero`); in release it wraps and silently yields 0.
`set_options` (save.rs:491) has the `dlc >= 2` guard - only the reader is unguarded.
`equipment_box()`/`palico_data()` etc. (save.rs:568-591) also slice without a bounds check, so a save
with a corrupt slot base can panic instead of returning None.

### E7 - the module doc's tail description is arithmetically impossible
`crates/mhnx-core/src/save.rs:10`: "last 660 bytes - three localised 220 byte strings".
Measured: slot 2 base + stride = 0x3633F4 + 0x11E7C0 = 0x481BB4, body length 0x481D88, so the tail is
**0x1D4 = 468 bytes**, not 660 (3 x 220 = 660 does not fit). In this save the 468 bytes are all 0x00,
so no string content could be observed at all.

### E8 (housekeeping) - dead code and a header that is never refreshed
* `enable_all(` appears exactly **once in the whole tree**: the definition at `crates/mhnx-core/src/dlc.rs:112`.
  **There are zero call sites** (CLI, GUI, hbedit, tests: none). The only other mention is
  `docs/quest_injection_spec.md:178`, explaining that `install_quest` no longer calls it because filling the
  28-byte set with 0xFF makes the game look for 224 quests. It is currently dead code.
* `refresh_header` (`save.rs:666-670`) also has no call sites: `to_bytes()` writes back whatever header
  the save was loaded with, so `app.rs:177` / `main.rs:130` will happily write `0\system` content to a file named
  `system_backup` (or any other name) with a stale +0x14 CRC and the game will not accept it.

---

## UNSUPPORTED / UNVERIFIABLE

* **Claim 5's premise - that the two option bytes track the section offset.** For MHXX it is right by
  construction (section 0xB280, so dlc-2/-1 = 0xB27E/0xB27F, which is where `main.js:161-164` reads them).
  For MHGU it contradicts the reference: the author's code reads 0xB27F/0xB280 (because the MHGU blank
  template's body+0x0C = 0xB281 - which I *did* confirm), while the web app reads 0xB27E/0xB27F for every
  kind. The blank MHGU file holds 00 00 01 at 0xB27E/0xB27F/0xB280, so the two readings disagree
  (author: disable_3d 0x00, circle_pad_pro 0x01; web app: 0x00, 0x00). "Circle pad pro = 1" on a Switch
  save is implausible, but without a real MHGU dump neither reading can be falsified here. No test covers
  it: `real_data.rs:208-235` checks only the blank MHGU *header*, never its options.
* **`save.rs:680` "3D disabled (1 on Switch, 0 on 3DS)".** The real Switch save holds 0x00 at that byte,
  so the stated expectation is simply not what the data shows. (It also cannot be checked without options
  being changed on hardware.)
* **Charm point signs.** The author reads +0x0E/+0x0F as i8; the C# editor reads them unsigned; the web app
  sign-corrects only the second. No value >= 0x80 occurs at either offset in the 188/231/231 talismans
  (max 0x0A and 0x0D), so this save cannot arbitrate - all three readings are consistent with the data.
* **`lib.rs:13-15`** ("The 36 byte header contains no content checksum") is true as written (only the
  *name* CRC and the body length are stored), but it is not a strong checksum claim to lean on either way.
* **`dlc.rs:3`** says the shared block "starts with a download-quest section"; the shared block actually
  starts at body+0x00 (Palico block at 0x2A) and the quest section is at 0xB280. Doc wording only.

---

## Vacuous / misleading tests

1. `crates/mhnx-core/tests/webapp_features.rs:33-58` `options_match_the_web_app_layout` - cannot fail.
   It asserts only `<= 1` on three bytes that are 0/1 and a round trip through the same accessors;
   no value is compared with anything external, so any offset whose bytes happen to be <= 1 passes.
   The name (and `docs/save_manager_review.md:157`) claims it pins 明るさ24/振動1/3D0/CCP0, which the real
   file contradicts (0x30/0x01/0x00/0x00).
2. `webapp_features.rs:78-121` `charm_box_is_at_the_equipment_offset` - despite the name, it never checks
   the box against the save's contents. Its only real assertion is
   `assert_eq!(0x62EE + charm::CHARM_BOX_SIZE, 0x17C2E)` - two constants restated - then it writes with
   `set_charm` and reads with `charms()`, i.e. the encoder tested against its own decoder. It would
   pass with 0x62EE pointing at arbitrary bytes (as long as the pointer stayed in range). It also silently
   overwrites record 0 (a Great Sword) and asserts nothing about it.
3. `webapp_features.rs:124-145` `gender_is_written_to_both_copies` - tautology: `write_character`
   writes `c.gender` to base+0x2A and base+0x23B4B, and the test reads exactly those two addresses. It
   cannot detect that either address is the wrong field; the closing name comparison is likewise
   self-referential.
4. `crates/mhnx-core/tests/hbedit_flow.rs:429-462` `options_screen_edits_and_saves` - self-consistency only
   (`options()` -> key press -> `options()`), never compared with a real value; `before - 1` would
   also underflow if brightness were 0.
5. `hbedit_flow.rs:521-569` `charm_screen_creates_and_deletes` - passes *because* of E1: it creates a charm
   in the first "free" slot of the real save (index 0 = Great Sword) and only asserts that the list grew.
   It is a regression test for the bug, not for correctness.
6. `crates/mhnx-core/tests/real_data.rs:208-235` `blank_templates_agree_with_computed_headers` - every case is
   `if !p.exists() { continue; }` and there is no `assert!(checked >= 1)`, so on a machine without those
   files the test passes having verified nothing (contrast `real_data.rs:143`, `:174`, `:205` and
   `check_quests.rs:83`, which do assert minimum counts).
7. Silent skip-on-missing-file: `webapp_features.rs` (all 8 tests), `real_data.rs:18-103`,
   `check_quests.rs:17-53` and `:91-103`, `quest_injection.rs` (all 5), `hbedit_flow.rs` (10 of 11)
   begin with `if !path.exists() { return; }` and report nothing. On a clean checkout the suite is green
   while testing almost none of this save.
8. `webapp_features.rs:60-75` `download_palicoes_are_found_in_the_shared_block` - the id/name assertions on
   `cats[0]`/`cats[1]` are a genuine anchor, but `assert_eq!(cats.len(), 50)` is tautological
   (the loop is `0..DLC_PALICO_COUNT`) and the comment "This save has two ... the rest are empty" is
   false: index 9 is id 0x97 ナナコＸＸ and 11..14 are 0x99..0x9C (7 records, measured).
9. `quest_injection.rs:112-135` `adding_a_new_quest_sets_only_its_own_bit` - `assert!(changed.len() <= 1)`
   accepts *zero* changes (the comment concedes this), so the behaviour in the test's name is not
   distinguished from "bit set was already right"; only the trailing `quest_bit` check bites.
10. `charm.rs:130-189` (in-module) and `save.rs:831-849` - encode/decode round trips and literal byte
    arrays: they pin the author's own constants, not the file. (`real_data.rs:86-103` does anchor the
    header against the real file, which is what makes the header claim trustworthy.)

---

## Prioritised fix list

1. **Stop writing charms into equipment slots.** Treat slot+0x62EE as 2000 equipment records: a "new charm"
   may only go into an all-zero record or one that is already type 6; if none exists, refuse (the box is
   full). Applies to `hbedit.rs:515-520`, `app.rs:1530`, and `save.rs::set_charm` (make it return false
   when the target record is occupied by a different type). Until this is fixed, "create charm" is a
   save-corrupting action on any real save.
2. **Make `encode_charm` write a complete record:** clear/keep the type+level bits properly (byte0 = 6,
   level bits of byte1 = 0), and set +0x02 equip id, +0x11 = 0, +0x12 rarity, +0x13 = 1, +0x14..+0x23 = 0,
   as `EditTalismanDialog.cs:41-47` does. Without +0x12/+0x13 the record matches no real talisman.
3. **Use the type bitfield** (`byte0 & 0x1F == 6`) instead of `byte0 == 0x06` in `charm.rs:88`, and add the
   arithmetic to the docs. Also fix the "charm box" wording everywhere (`charm.rs:3-5`, `save.rs:99-101`,
   `save.rs:567`, `app.rs:1502` "{} / 2000", `docs/save_manager_review.md:71-75`): it is the equipment
   box; slot 0 holds 188 talismans among 2000 records.
4. **Options:** correct the doc values to the measured ones (0x30/0x01/0x00/0x00), drop the 0..=31 range,
   guard `options()` with `dlc >= 2` like `set_options`, and settle the MHGU byte positions against a real
   MHGU dump before claiming the derivation is an improvement (today it is a one-byte shift from the
   reference with no test either way). Add bounds checks to the slot accessors.
5. **Make the tests able to fail:** replace the skip-returns with a shared guard that fails (or at least
   prints loudly) when the real files are missing; add content-anchored assertions (188/231/231 talismans;
   brightness == 0x30; slot 0 funds == 9,997,199; 175 quests) and one test that asserts creating a charm
   leaves every non-talisman record byte-identical. Fix the two false comments
   (`webapp_features.rs:69-74`, `real_data.rs:212`).
6. **Doc/robustness cleanups:** `save.rs:10` 660 -> 468 bytes; `docs/save_manager_review.md:108-109`
   `body+0x0C-2` -> `(DLC section offset) - 2`; either call `refresh_header` from the write paths or
   delete it; delete or wire up `dlc::enable_all`; consider modelling +0x23B4C (hunting style), which the
   C# editor exposes and this save stores (value 5).

---

## Artefacts produced by this review

* `tools/_review1.py` .. `tools/_review8.py` - the independent Python verification (headers, offsets,
  charm/quest decoding, blank-template comparison). All read-only.
* `crates/mhnx-core/tests/_review_save_layout.rs` - the Rust probe (4 tests, all green) that demonstrates
  E1, E2, E3 and E6 against in-memory copies of the real save. The real save files are never written.
