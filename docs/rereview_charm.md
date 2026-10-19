# Re-review of the charm fix - adversarial verification

Save under test (read only, never written):
`C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`

Method: my own Node parser over the whole file (no project code), plus a throw-away Rust probe
(`crates/mhnx-core/tests/_rereview_charm_probe.rs`, 5 tests, all green, in-memory only, deleted after the run).
Cross-checks against two independent implementations on this machine:
`nx/mhxx/save_editor/repos/MHXXSwitchSaveEditor` (C#) and `nx/mhxx/refs/SJ-site/MHGU/save-manager/scripts/main.js` (web app).

---

## Verdict

**The equipment-destroying bug is fixed.** Every path that can write into slot+0x62EE is guarded, and I could
not construct any input that turns a weapon or a piece of armour into a talisman. The type mask, the i8
points, the writable-only picker and the brightness fix are confirmed on the real data.

**But the constants the fix is built on are wrong, and the fix still rewrites real talismans.**
`charm.rs` claims `+0x02 = 0x0A` and `+0x12 = 100` are the same in every talisman.
They are not: the box holds **five** distinct (class id, rarity) pairs, and `encode_charm` force-converts
all of them to one. Saving a talisman the user only looked at silently changes its class and rarity
(measured: slot 1 #209 `06 00 06 .. 63 01` becomes `06 00 0a .. 64 01`) - and those two bytes select
the legal skill-point range, so the written record can fall outside the game's own table.
`first_writable` also breaks its contract on a full box, and the equipment-box accessor still panics
on a bad slot base.

---

## FIXES CONFIRMED (my own measurements)

### Layout (claim 1) - confirmed, with corrections

Record byte 0 is a bitfield and the author's decode is right. Over **all 650 talismans**
(slot 0 = 188, slot 1 = 231, slot 2 = 231; the same counts the old code reported):

| field | measured |
|---|---|
| `+0x00` | `0x06` in all 650; level `(b1<<3)` OR `(b0>>5)` = 0 in all 650 |
| `+0x01` | `0x00` in all 650 |
| `+0x0C..+0x0F` | skill ids 5..179, points as **i8** (see below) |
| `+0x10` | 3 x590, 2 x18, 1 x10, 0 x32 |
| `+0x11` | `0x00` in all 650 |
| `+0x13` | `0x01` in all 650 |
| `+0x14..+0x23` | all zero in all 650 |
| `+0x02` | **NOT constant**: `0x0A` x580, `0x07` x46, `0x06` x20, `0x00` x2, `0x04` x2 |
| `+0x12` | **NOT constant**: `100` x580, `99` x68, `98` x2 |
| `+0x06..+0x09` | non-zero in **15** records (13 in slot 0; 1 each in slots 1 and 2) |

`+0x02` and `+0x12` are a **linked pair**, not independent constants:
(0x0A,100) x580, (0x07,99) x46, (0x06,99) x20, (0x00,99) x2, (0x04,98) x2. The C# editor derives the
second from the first (`EditTalismanDialog.cs:42`: `[+18] = TalismanRarity[+2]`, with
`TalismanRarity = {0,97,97,98,98,99,99,99,100,100,100}`), which reproduces every pair in this save except
the single id-0 record (table says 97, the save says 99 - one outlier, probably written by a tool).

So `charm.rs:20` (the same in every talisman) and `charm.rs:27` (100 in every talisman) are
false, and TALISMAN_02 / TALISMAN_RARITY are samples of one class, not constants.

### Points are signed - the author is RIGHT, the first review was WRONG (claim 1)

The first review stated that no value >= 0x80 occurs at either points offset in the 188/231/231 talismans.
That is false. Six records carry `0xFF/0xFE/0xFD` at `+0x0F`:

```
slot1 #209  06 00 06 .. 30 4b 04 ff 00 00 63 01   (s1=48 p1=4  s2=75  p2=-1)
slot1 #210  06 00 06 .. 25 53 04 fe 02 00 63 01   (s1=37 p1=4  s2=83  p2=-2)
slot1 #222  06 00 07 .. 2e 92 05 fd 02 00 63 01   (s1=46 p1=5  s2=146 p2=-3)
```

(the same three records exist in slot 2). Read as i8, **all 650 talismans fall inside the skill-point
range the reference gives for their rarity** (`GameConstants.cs:48-51`, e.g. Timeworn skill 75 = `-3~3`).
Read as u8, 253/254/255 points are impossible. `points1: i8` / `points2: i8` (`charm.rs:90,94`) is
the correct choice and is now data-backed.

### The box is the shared equipment box and the guard works (claim 1/2)

* Slot 0 box: types 1..20 = 601 records, type 6 = 188, all-zero = 1211, plus one type-21 record.
  So type < 21 = equipment is right (the doc's 7..20 weapon list misses the type-21 record that
  exists in slots 1/2 - harmless, it is simply not writable).
* `encode_charm` refuses a non-empty non-talisman record; `clear_charm` refuses anything that is not a
  talisman. Record 0 (`47 01 1a`) survives a refused `set_charm(0, ...)` byte for byte.
* Every write path is guarded: `charm::encode_charm` / `charm::clear_charm` have exactly one caller each
  (`save.rs:463`, `save.rs:470`), `equipment_box_mut` is called only from those two functions, and the
  CLI is read-only (`crates/mhnx-cli/src/main.rs:462-479`). The only other writers into that region are
  `import_slot` (`save.rs:630`, whole-slot replacement, bounds-checked at `:645`) and `replace_body`
  (`save.rs:665`, length checked). No unguarded path remains.

### 0x62EE + 72000 == 0x17C2E and the box really ends there (task 7)

Verified structurally, not by restating constants: the region ending at base+0x17C2E contains types 1..21
only, and the region starting exactly at base+0x17C2E contains **types 22, 23, 24 only** (58+7+7 used
records = Palico weapon/helmet/armour) plus blanks - i.e. the next block. The bytes straddling the
boundary are `... 00 00` then `16 00 01 00 ...` (Palico weapon, id 1). The preceding table also lines
up: 0x60CA + 548 = 0x62EE, and the monster-size table holds 156 plausible u16 values (max 160) in
548 bytes. The assertion `0x62EE + charm::CHARM_BOX_SIZE == 0x17C2E` is still present at
`webapp_features.rs:86` and `charm.rs:318-323` - both restate constants, but the claim behind them is confirmed.

### Brightness (claim 3) - confirmed

`save.rs:50-52` (doc + plain byte), `hbedit.rs:700` (`clamp(0,255)`), `app.rs:1395` (`0..=255`). The save
holds 0x30 = 48; no clamp to 31 survives anywhere.

### FIRST_SLOT_BASE / empty_switch_save() (claim 5) - confirmed

`save.rs:48,858-860` store 0x126474 + n*0x11E7C0 = 0x126474 / 0x244C34 / 0x3633F4, exactly what the real
file stores.

---

## REMAINING DATA-LOSS PATHS

No path that destroys equipment remains. Two paths still destroy data inside real talisman records,
and one still panics.

### R1 - editing an existing talisman rewrites its class, its rarity and four unmodelled bytes
`charm.rs:161-172` (encode), reached from `save.rs:461` (`set_charm`), from `hbedit.rs:613` (key A on the
charm form) and from `app.rs:1590-1604` (GUI write button).

Repro (probe B, executed): open the charm form on slot 1 #209 with the values it already has, press A
or click write. Result:

```
before  06 00 06 00 00 00 00 00 00 00 00 00 30 4b 04 ff 00 00 63 01 ...
after   06 00 0a 00 00 00 00 00 00 00 00 00 30 4b 04 ff 00 00 64 01 ...
differs at +0x02 (0x06 -> 0x0A) and +0x12 (99 -> 100); the decoded Charm is unchanged
```

The same happens to 70 records in slots 1/2 (68 x rarity 99, 2 x rarity 98). Rarity is not cosmetic:
`EditTalismanDialog.cs:61-78` switches the legal point range on it. Concrete case: slot 1 #222 has
`s2 = 146 (Stonefist), p2 = -3`, inside the Timeworn (99) range `-3~3` but outside the Enduring (100)
range `0~0` - so the editor manufactures an out-of-range record from a legal one. In addition the 15
records with non-zero `+0x06..+0x09` lose those bytes, because `encode_charm` zeroes all 36 bytes and the
decoder never models `+0x02..+0x0B`.

Not destroy-a-Great-Sword, but it is silent modification of data the user did not touch, and it is the
class of failure the first review's fix item 2 asked to close.

### R2 - first_writable returns a WEAPON when the box has no blank record
`charm.rs:203-212`. The fallback is `(0..CHARM_COUNT).find(|i| !used.contains(i))` where `used` is the
decoded *talisman* list - i.e. the exact old broken rule (first index with no decoded charm), which the
doc comment above it (else the last talisman) does not describe.

Repro (probe A, executed): fill all 2000 records with `47 01 1a` (Great Sword). `first_free` returns `None`,
`first_writable` returns `Some(0)`, and `is_writable(record 0)` is `false`. Consumers are
`hbedit.rs:517-522` (`unwrap_or(CHARM_COUNT-1)`) and `app.rs:1508-1512`, which then set the form's index to a
weapon. Nothing is destroyed today because `encode_charm` refuses - the last line of defence holds - but
the picker is one refactor away from being the bug again, and `first_writable_skips_equipment` covers
only the happy path.

### R3 - equipment_box() panics on a bad slot base (the first review's E6, unfixed here)
`save.rs:579-587` slices `body[base+0x62EE .. base+0x62EE+72000]` with no check; `slot_base` (`save.rs:309-319`)
only tests `v != 0 && v < body.len()`.

Repro (probe C, executed): set body+0x10 to `body.len()-16` (still passes `slot_base`) and call
`equipment_box(0)` -> panic (range end index out of range). In the GUI this is reached from
`app.rs:1511` / `:1582` and takes the whole app down (unsaved edits are lost); the same pattern
exists in `palico_equipment_box`, `palico_data`, `guild_card` and `shoutouts`. Only `options()` was
fixed for this class of problem.

---

## NEW BUGS

1. **charm.rs:20,27 document false constants** (the same in every talisman). The module's table says
   it was measured across all 188 of the sample save - it was measured on slot 0 only. Slots 1 and 2
   disprove it (35 of 231 each, measured above).
2. **The default charm the editor offers is out of range for the rarity it forces.** `encode_charm`
   always writes rarity 100 (Enduring) but leaves the skills/points the form holds.
   `hbedit.rs:537-540` and `app.rs:103-108` default to skill1 = 1 (Poison), points1 = 1. The reference
   table for rarity 100 gives skill 1 the first-skill range `0~0`, and **0 of the 580 real
   rarity-100 talismans in this save uses skill 1 as its first skill**. `SetMaxMinSkills` in the C#
   dialog would not let a user build this record. Whether the game rejects it cannot be tested here,
   but it matches no real talisman and no reference-legal combination. Same for the test charm with
   `skill2 = 200, points2 = -2` (probe E: range `0~0` at rarity 100).
3. **GUI delete ignores the result and then lies**: `app.rs:1543-1548` calls `s.clear_charm(slot, i)`,
   drops the bool, sets `dirty = true` and prints deleted #i unconditionally. Today `i` always comes
   from the decoded list so the call succeeds; a future refusal would report success without a write.
4. **The GUI write button is silent on refusal**: `app.rs:1590-1604` has no else branch; if `set_charm`
   returns false nothing is shown (the hbedit path at `hbedit.rs:616-618` does report it).
5. **The GUI warning at `app.rs:1581-1589` is only reachable through R2.** Normally the snap at
   `app.rs:1513-1520` silently moves the index to a writable record first, so the warning never appears;
   in the full-box case it appears but the write button does nothing (4).
6. **`app.rs:1516` / `:1583` index a `pub usize` field with `&b[i*36..(i+1)*36]`.** It does not panic
   today: the only writers are the DragValue with `range(0..=1999)` (`:1553`) and `= w` from
   `first_writable` (<= 1999), and `equipment_box` returns a full 72000-byte slice or panics (R3) -
   it never returns a short slice. So the bound lives in the *widget*, not in the code that indexes;
   a defensive `box.get(i*36..(i+1)*36)` would make the invariant local. (The panel is only reached
   inside `tab_extras`, which returns early when no save is loaded - `app.rs:1377-1380` - so the
   `.unwrap()`s at `:1515` / `:1582` cannot fire on a missing save.)
7. **hbedit refuses a delete with no message** (`hbedit.rs:556-564`: `if s.clear_charm(...) { ... }` with no
   else), leaving a stale status line. The cursor handling after a delete is correct (measured:
   cursor 187 of 188 -> 186 of 187; the empty list renders because no charm is indexed without `get`).
8. **Wording not fully cleaned up** (first review's fix item 3): `save.rs:456,460,467,578` and
   `hbedit.rs:55` still call slot+0x62EE the charm box, and `save.rs:578` says it both ways in one line.
   `save.rs:10` still claims the trailing region is 660 bytes (it is 468).
9. Pre-existing and untouched: `refresh_header` (`save.rs:682`) still has no call site, so
   `write_current` (`app.rs:174-185`) writes the loaded header to any chosen file name - the +0x14
   CRC still says system. `write_switch_pair` (`:188-208`) builds fresh headers and is fine.

---

## Vacuous / unrepresentative tests

1. `regression_review.rs:53`: `assert_eq!(talismans, src.charms(0).unwrap().len())` - both sides count
   `is_talisman` records; it cannot fail for any input. (The neighbouring `other > 100` / `empty > 100`
   are weak but real: 601 and 1211.)
2. `regression_review.rs:96` picks slot 0 record 224 as a real talisman. That record is
   `06 00 0a .. 03 00 64 01` - the modal rarity-100 shape - so the comparison at `:97` passes.
   **Slot 1's record 224 is `06 00 07 .. 02 00 63 01`; the same loop against `equipment_box(1)` fails at
   +0x02, +0x10 and +0x12** (probe E). The test pins the constants to one lucky record instead of
   asserting a property.
3. `webapp_features.rs:84-96` - the boundary assertion restates two constants
   (`charm::CHARM_BOX_SIZE` vs `save::OFF_PALICO_EQUIPMENT`), and the rest writes with `set_charm` and reads
   with `charms`. It would now fail if `first_writable` returned a weapon (an improvement) but still
   asserts nothing about the file's contents. `charm.rs:318-323` is the same constant check again.
4. `webapp_features.rs:74`: `assert_eq!(cats[2].name, "", "the rest are empty")` - the assertion holds
   for index 2, the trailing claim is false (indices 9 and 11..14 are used).
5. `regression_review.rs:131-139` brightness_is_not_clamped - `v > 31` is a real measurement (48),
   but `:125`'s `brightness == body[0x28]` is a tautology tacked onto an otherwise useful test.
6. `hbedit_flow.rs:521-569` now creates in a blank record (good) but still only asserts that the list
   length changed; it never checks the other 1999 records, and it does not touch delete-refusal or the
   cursor, so R1/R2/R7 stay invisible.

---

## Prioritised list

1. **Do not invent +0x02 / +0x12.** When rewriting an existing talisman keep the record's own class
   bytes (patch only +0x0C..+0x11, as `EditTalismanDialog.cs:36-47` does); when creating on a blank
   record pick a class from a table and write the matching rarity `{0,97,97,98,98,99,99,99,100,100,100}[id]`.
   Removes R1 and new bug 2.
2. **Fix `first_writable`** (`charm.rs:203-212`): drop the first-index-that-is-not-a-talisman
   fallback - return `None` when the box has no blank and no talisman, or the first record for which
   `is_writable` is true, and assert that in a test (R2).
3. **Bounds-check every slot accessor** (`save.rs:579-610`): return `None` unless
   `base + OFF + SIZE <= body.len()`, and have callers treat that as slot unavailable instead of
   panicking (R3).
4. **Preserve +0x06..+0x0B** when overwriting an existing record (copy the record, patch the modelled
   fields) and document what those bytes are; today they are silently cleared on 15 real talismans.
5. **Make the constants data-backed or delete them**: `charm.rs:16-30` should list the five measured
   (id, rarity) pairs and state that points are i8, citing the six negative records.
6. **Tests that can fail**: replace the tape measure at `regression_review.rs:53`; run the record-shape
   comparison over a set of real records (at least one rarity-99 and one rarity-98 record from slots
   1/2); assert the real counts (188/231/231 talismans, 1211/1229 blanks, 601 used equipment records in
   slot 0); add a test that an identity write is a no-op (it is not today).
7. **Report refusals to the user**: GUI `app.rs:1590` (missing else) and `:1545` (result ignored), hbedit
   delete at `hbedit.rs:557`.
8. **Wording/doc**: `save.rs:456,460,467,578`, `hbedit.rs:55`, `save.rs:10` (660 -> 468).
9. **Call `refresh_header`** from `write_current` (or refuse to write under a name whose CRC does not
   match), so a save written as `system_backup` is accepted by the game.

---

## Reproducing this review

* Everything above was measured with a throw-away Node parser run over the real save (read-only)
  and 5 Rust probes (probe_a .. probe_e) that only touch in-memory copies.
* Probe output of record: `first_writable(full box of weapons) = Some(0)`;
  `#209 differences (offset, before, after) = [(2, 6, 10), (18, 99, 100)]`;
  `equipment_box panicked = true`; `after delete cursor 186 n 187`;
  `slot 1 #224 = 06 00 07 .. 63 01`.
* The probe file was deleted after the run to keep the tree clean; recreate it from the recipes in
  R1/R2/R3 if these fixes are to be regression-tested.
