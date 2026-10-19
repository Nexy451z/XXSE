# Re-review: the quest-injection fix (dlc.rs / save.rs / tests)

Re-reviewed revision (sha256, first 16 hex):

| file | bytes | sha256 | mtime (JST) |
|---|---|---|---|
| crates/mhnx-core/src/dlc.rs | 13411 | `d9e878b78322ac1c` | 2026-10-19 15:42:22 |
| crates/mhnx-core/src/save.rs | 32918 | `c2b2f2eb4721c32f` | 2026-10-19 16:12:14 (charm work only) |
| crates/mhnx-core/tests/quest_injection.rs | 6182 | `55a0e9953f14dac3` | 2026-10-19 15:45:54 |
| crates/mhnx-core/tests/regression_review.rs | 5659 | `c465ecb38505e465` | 2026-10-19 15:46:57 |

Evidence: the read-only real save
`C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system` (never written),
the blank MHXX / MHGU / 3DS templates of MHGU-MHXX-Save-Converter-Script, the reference homebrew
`refs/mhx_data_manager/source/quest.c`, my own read-only parsers, and a probe crate at
`C:\anyWorkspace\nx\mhxx\_rereview\probe` that links mhnx-core directly. All probing was in memory.

---

## Verdict

**The central fix is real: I could not break the "no mask write" claim itself.** `set_quest_bit` / `quest_bit` /
`enable_all` / `clear_all` are gone, nothing in the repository writes the 28 bytes at `section+0x4C` any more,
and the catalogue semantics in the module doc are correct. I re-derived the decisive facts independently:

* "bit i = slot i" is dead: of the 175 records, **0** sit in a slot whose bit is set, while the chance
  expectation for that count is 141.7 (the "140 matches" quoted in the spec is exactly chance).
* The 45-entry equipment catalogue at `section+0x1049F4` is an independent witness: every non-empty entry k
  (40 of them, k = 0..24 and 26..40) has bit k set, the five empty entries (k = 25, 41..44) have bit k clear —
  **45/45, zero mismatches** — and the challenge records sit in slots 160..199, unrelated to k
  (equipment k = 0 is id 1020001 in slot 180).

The follow-through, though, is incomplete and it introduced one real inconsistency:

1. `list_quests` **still panics** on a crafted/corrupt section pointer (dlc.rs:155), and the new regression
   test meant to cover it passes only because it leaves the id field zero.
2. `install_quest`'s new precondition — "the record exists, therefore its catalogue bit is set" — is **never
   checked**. I produced a concrete state (mask cleared, records left in place: exactly what the reference's own
   `clear()` writes) in which the tool reports success for an edit the same reference's enumeration cannot see.
   The challenge half of that check is derivable from the save today; the event half is not.
3. `uninstall_quest` + the new refusal form a **one-way door**: delete a quest and the tool can never put it
   back, while the bit stays set and now advertises a quest whose payload is gone.
4. Docs, CLI usage and GUI text still describe the removed free-slot behaviour.

---

## FIXES CONFIRMED

**C1 (claim 1) — the writers are gone.** Grep over every `*.rs` in `crates/` and `switch/` finds no
`set_quest_bit` / `quest_bit` / `enable_all` / `clear_all` and no write to `BITMASK_OFFSET`. dlc.rs now only
*reads* the set (bitmask dlc.rs:98, installed_bit_count dlc.rs:114); every body write is inside one slot
(dlc.rs:214, :216, :217, :229). `switch/src` reaches the save only through `hbedit::Editor`, so it inherits the
same path. The only other occurrences of `sec+0x4C` in the tree are read-only analysis scripts in `tools/`.

**C2 (claim 2) — install_quest only replaces an existing id and never writes the set.** Probe on the real save:
`install_quest(1010165, ...) -> Ok(118)`, and the 28 mask bytes are byte-identical before and after.
dlc.rs:209-211 refuses an id `find_quest` cannot find; `install_quest(9990001, ...)` returns
`Err(Parse("that quest is not installed; only an installed quest can be replaced"))` on both the real save and a
blank template, with the body unchanged.

**C3 (claim 3) — MHGU is refused, a legitimate 3DS save is NOT.** `check_kind` (dlc.rs:88-95) matches only
`SaveKind::Mhgu`. Probe: blank MHGU template (5,159,100 B) ->
`Err(Parse("download quests are not supported for MHGU saves"))`; the real save stripped of its 36-byte header
(classifies as `ThreeDs`) -> `list_quests` = 175, `install_quest(1010165) = Ok(118)`. The 3DS/Switch sharing of
geometry is not an assumption: the third-party blank 3DS template (Blank_3DS_Save/system, 4,726,152 B) and the
blank Switch template (.../MHXX Switch Version/system, 4,726,188 B) both carry `body+0x0C = 0xB280` and slot
bases `0x126474 / 0x244C34 / 0x3633F4`. So check_kind does not reject a legitimate MHXX 3DS save.

**C4 (claim 4) — slot_extent exists and both write paths use it.** dlc.rs:162-184 checks `slot < QUEST_SLOTS`,
uses `checked_add`, and requires `base + QUEST_SLOT_SIZE <= body.len()`. `install_quest` (:212) and
`uninstall_quest` (:228) both go through it; on my crafted short file both return `Err(OutOfRange)` instead of
panicking. The read paths do not use it — see N1.

**C5 (claim 5) — no silent clamp.** dlc.rs:141-149 returns the record with `data: Vec::new()` when `size` is
outside `0..=0x13F8`, preserving the value of `size`. Verified with `size = i32::MAX`. (The struct's implicit
invariant `data.len() == size` is now broken by design — see I7.)

**C6 (claim 6) — uninstall_quest leaves the set alone.** Probe on the real save, slot 0 (id 1010005): set bits
175 -> 175, `find_quest` -> `None`, record zeroed. The *consequence* is a new problem — see N4.

**C7 (claim 7) — the module doc's catalogue section is correct** (dlc.rs:19-40), including the warning that a
slot number cannot address a bit. Two stale lines remain in the same header (dlc.rs:7 says "7 x u32,
0xFFFFFFFF = enabled"; dlc.rs:12-13 still calls the endpoint identity "how this layout was confirmed").

**C8 — nothing else in the repository writes the set** (grep over `*.rs`, `*.py`, `*.ps1`, `*.c`).

---

## NEW BUGS

### N1 — `list_quests` still panics on a corrupt section pointer (dlc.rs:155)

dlc.rs:131 only checks `base + 8 > body.len()`; the payload slice `body[base + 8 .. base + 8 + n]` is unchecked,
and `n` may be as large as `QUEST_MAX_PAYLOAD` (0x13F8).

Concrete input — a 4,726,188-byte Switch file with `body+0x0C = 0x47C39C`, slot 0 id = `1010165`,
size = `0x13F8`:

~~~
base      = 0x47C39C + 0x45F4   = 0x480990   (body offset)
base + 8  = 0x480998 <= body len 0x481D88    -> the guard passes
payload   = 0x480998 .. 0x481D90 > 0x481D88  -> panic
thread 'main' panicked at crates\mhnx-core\src\dlc.rs:155:23:
range end index 4726160 out of range for slice of length 4726152
~~~

Blast radius: `check::check_save` (check.rs:362) calls `list_quests`, so `mhnx check-save` **crashes** instead of
printing findings, and the GUI crashes on the Dlc tab (app.rs:1276). The write paths are safe on this input
(`install_quest` / `uninstall_quest` -> `Err(OutOfRange)`), so this is a read-path crash, not corruption.

### N2 — the regression test for N1 passes vacuously (dlc.rs:339-346)

`a_corrupt_section_offset_does_not_panic` uses **the same 0x47C39C pointer** but builds on `empty_switch_save()`,
so the whole body is zero and `id == 0` makes the loop `continue` before the slice. Executed counter-test: the
same pointer with `body[base..base+4] = 1010165` and `body[base+4..base+8] = 0x13F8` panics at dlc.rs:155. The
test therefore certifies a fix it does not exercise. (The sibling `a_corrupt_size_does_not_panic`, dlc.rs:324-336,
is fine because it seeds a real id first — copy that pattern.)

### N3 — "a record exists => its bit is set" is an unchecked premise (dlc.rs:209-211)

The module doc (dlc.rs:33-35) and the spec (§3.3 / §5.1) assert that a replaced quest's bit "is already set".
Nothing verifies it; `install_quest` never looks at the mask.

Reachable counter-state: the reference's own delete-all-quests writes zeroes over the mask and leaves every
record in place (quest.c:225-239, `INSTALLED_OFFSET 0x34` for MHX; the MHXX analogue is the 28 bytes at 0x4C).
Zeroing those 28 bytes on a copy of the real save gives **175 records, 0 set bits** — a save the tool still calls
"175 installed quests".

Executed rigorously on a case where the catalogue is *known* (challenge quests, via the equipment array):
equipment entry k = 0 holds id 1020001, so that id's catalogue bit is bit 0. Clear bit 0, then:

~~~
[clear-bit] equipment k=0 id=1020001 -> install_quest = Ok(180)
[clear-bit] bit 0 still clear? true  (mask now fe vs before ff)
[clear-bit] list_quests reports it as installed: true
~~~

And the same result on the full cleared state (all 28 mask bytes zeroed, 175 records left untouched):

~~~
[cleared-mask] records=175 bits=0
[cleared-mask] install_quest(1011002) -> Ok(10) ; bits after = 0
~~~

What the other side sees (reference quest.c): `extract()` (quest.c:117-146) exports a record only when
`quest_info[0] != 0` and the id appears in `installed_ids`, which come from the bits via `get_ids()`
(quest.c:68-81). With the bit clear the record is never enumerated, i.e. the tool reports success on an edit that
— by the reference's own model, the model this fix adopts — the game cannot see. `inject()` (quest.c:179-182)
matches by id *and* writes the bit for the catalogue entry afterwards (quest.c:216-217): the reference never
relies on "the bit is already set".

I am not claiming the game's menu behaves exactly like `extract` (no disassembly was done here), but the fix's
whole justification is that model, and that model calls this state broken while the code calls it installed.

Concretely fixable today for the 40 challenge ids: the equipment array at `section+0x1049F4` maps id -> k (the id
appears at +0 and +8 of each 0x800 record) and bit k is the installed flag. For event ids no catalogue exists in
the workspace — see R1.

### N4 — uninstall is a one-way door (dlc.rs:226-231 with dlc.rs:209-211)

`uninstall_quest(save, 0)` zeroes id 1010005, leaves 175 set bits, and `find_quest(save, 1010005)` ->
`None`. Because `install_quest` now requires `find_quest` to succeed, the same tool can never put the quest back:
the user is left with a save whose mask advertises a quest whose payload is zeroed, and no supported way to
restore it (short of a backup or another tool). Executed:

~~~
[uninstall]   slot 0 id 1010005 zeroed; bits 175 -> 175 ; find_quest = None
[one-way]     after uninstall(slot 10): find=None
              reinstall=Err(Parse("that quest is not installed; only an installed quest can be replaced"))
~~~

The two halves of the fix (claims 2 and 6) are individually defensible but combine into this.

The GUI exposes it as a plain delete button per row (app.rs:1285) whose success message is
"スロット {} を削除しました" (app.rs:392); the CLI has no uninstall. The doc comment (dlc.rs:222-225) is honest
("treat this as a diagnostic rather than a clean uninstall"), but nothing the user sees repeats it. For a
challenge quest it also leaves the quest's equipment entry (id + data at `section+0x1049F4 + k*0x800`) pointing
at a quest that no longer exists.

### N5 — `bitmask` / `installed_bit_count` ignore check_kind (dlc.rs:98, dlc.rs:114)

The module doc (dlc.rs:85-87) says "Everything that writes refuses to touch an MHGU save ... Reading returns
empty for the same reason", but only `list_quests` / `find_quest` are guarded. Probe on the blank MHGU template:
`bitmask() -> Some([0; 28])`, `installed_bit_count() -> 0` — read at MHGU offsets and reported as an answer.
Low impact (diagnostics only), but it contradicts the new doc.

---

## INCOMPLETE FIXES

* **I1 — E6 (panics) is only half fixed.** `slot_extent` protects the two writers; the readers keep the weak
  `base + 8 > len` guard. See N1/N2. This is the highest-value remaining item: the earlier review's fix #4 asked
  for `base + 8 + n <= len` by name, and `check-save` / the GUI crash on the input.
* **I2 — E9 (the tautology) survives.** `arrays_end_at_first_slot` (dlc.rs:269-281) reads `s.slot_base(0)` from a
  save that `empty_switch_save` (save.rs:859-860) filled with the literal `FIRST_SLOT_BASE` (save.rs:48). Both
  sides come from the same constant, so the comment "Read the slot base from the save instead of hard coding both
  sides, so the two numbers actually have to agree" is not true. It now checks that two files' literals agree
  (it would catch `QUEST_SLOTS = 206`), which is worth keeping, but it is not evidence. The only check against
  reality is quest_injection.rs:46-53, and that test is skippable via `MHNX_SKIP_REAL_SAVE=1`.
* **I3 — the docs are only partly updated.**
  * quest_injection_spec.md:88 still presents `section+0x34` as the live MHX-compatible 140-bit mask. My own
    reading of the real save: 20 bytes, 112 bits set, and all 112 land on occupied slots (chance 95.6), so it is
    slot-correlated — but its meaning is still unestablished; the earlier review's U2 stands.
  * quest_injection_spec.md:241 (§6 step 3) still says "同じ ID があればそのスロットを上書き、**無ければ空きスロットへ**" —
    the opposite of the new behaviour.
  * quest_injection_spec.md:257-274 (§7) lists six tests, five of which no longer exist:
    `the_installed_bit_set_lines_up_with_the_records`, `replacing_a_quest_keeps_the_slot_and_the_payload`,
    `adding_a_new_quest_sets_only_its_own_bit`, `installing_touches_only_its_own_bit`,
    `installing_an_existing_id_replaces_in_place`.
  * dlc.rs:7 and dlc.rs:12-13 (see C7).
* **I4 — E5's second half ("reject ids whose bit is not already set") is not done** — see N3; the challenge half
  is implementable today.
* **I5 — regression_review.rs still passes vacuously.** `real_save()` (regression_review.rs:12-15) is
  `std::fs::read(p).ok().map(...)`; with no file present all six tests return early and pass — contradicting both
  the file's own header and the policy adopted in quest_injection.rs:15-29. (Its E1–E4 charm tests are another
  reviewer's area; only its `options` / brightness tests belong to this fix.)
* **I6 — duplicate ids (old M5) are still unhandled.** `find_quest` (dlc.rs:240-249) returns the first match, so a
  duplicated id keeps a stale second payload; `check_save` only warns (check.rs:364-373). This matters more now
  that replace-in-place is the only write path.
* **I7 — the new "empty data" contract is undocumented on the type.** `InstalledQuest { size, data }` can now
  return `data.len() != size` (e.g. `size: i32::MAX, data: []`), while real_data.rs:51-52 still asserts
  `q.size > 0 && q.data.len() == q.size`. A corrupt save would fail that test with a misleading message; the
  field docs (dlc.rs:74-76) should say so.
* **I8 — the user-facing contract is not explained.** All three callers do surface the error
  (main.rs:211 -> "error: ..." + FAILURE exit via main.rs:548-554; app.rs:378 log line; hbedit.rs:1110
  "install failed: ..."), so the user is *told*, not left guessing. But the CLI usage still says
  "install a quest into the save's DLC slots" (main.rs:27), the GUI heading still says
  "ダウンロードクエストのインストール ... そのままセーブに書き込みます" (app.rs:1210-1212), and the failure text
  ("that quest is not installed; only an installed quest can be replaced") never says why nothing can be added or
  what to do. On a save holding no quests (the four blank MHXX saves in `save_backups/20261019_150128/`) a folder
  install of the 141 shipped `.arc` files fails for **all 141**, and the summary line is "0 件をインストールしました".
  On the real save, 6 of those 141 ids are absent (1010002, 1010116, 1010117, 1010169, 1011045, 1110113) and are
  refused. Also `install_dlc` sets `self.dirty = true` (app.rs:383) even when nothing was installed, so the GUI
  offers to save an unchanged file.

---

## REMAINING RISKS

* **R1 — the event half of the catalogue is unverified, and the MHX table cannot substitute for it.** On the real
  save: 135 event records, 135 bits in 64..214, bits 45..63 all clear, and the sorted record->bit pairing is
  monotone with drops exactly at ids missing from the save (consistent with a catalogue ordered by id). That is
  consistency, not proof, and no event catalogue exists in the workspace. The obvious candidate is refuted:
  quest.c:35-48 lists MHX's 120 event ids; 86 of the 135 records appear in it and all 86 satisfy
  "bit = 64 + MHX index", but 49 records are *not* in the MHX list while only 40 catalogue positions remain after
  the MHX 120 (the last set bit is 214 = 64+150, so >=151 entries), so MHXX's event catalogue is a different
  list. Until that table exists, `install_quest` cannot verify the bit for an event quest, and the N3 state is
  undetectable for those 135 records.
* **R2 — `tools/_saverestore.py:6`** copies `.../1/system` over `.../0/system` (the live Ryujinx save path) with no
  confirmation and no backup. I did not run it; flagged because it is one keystroke from the real save and this
  fix does not touch it.
* **R3 — the GUI installs by file name, not by the .mib id** (app.rs:363-371): it derives the id from `q<id>.arc`
  and never parses the payload, while the CLI (main.rs:204-210) and hbedit (hbedit.rs:1085-1089) read the .mib.
  A renamed or mismatched file writes id X with a payload whose own id is Y — the freeze cause the spec itself
  documents ("ID を変更した時"). Pre-existing, but the fix makes id correctness the entire safety argument. A file
  whose stem does not parse is dropped with no log line.
* **R4 — MHGU geometry is still unknown** (and correctly refused). From the blank MHGU template: body 5,159,064 B,
  `body+0x0C = 0xB281`, slot bases `0x18CC78 / 0x2AC53C / 0x3CBE00` (stride `0x11F8C4`), and
  `[sec, sec+0x126474)` is entirely zero, so nothing can be measured from it. The MHXX endpoint identity evaluates
  to `0x126475` there, i.e. **419,843 bytes short** of the real `slot_base(0) = 0x18CC78`, so the MHXX layout
  demonstrably is not MHGU's. The space from `sec` to `slot_base(0)` is `0x1819F7` (1,578,487 B) versus
  `0x761F4` (483,828 B) for the MHXX layout; 205 x 0x1400 + 45 x 0x800 leaves 436,727 B unexplained, while
  205 x 0x1C00 + 45 x 0x800 (the reference's "large" record, used for the localized releases) needs 1,561,600 B
  and would fit with 16,887 B to spare. **Both are arithmetically possible; I am not claiming 0x1C00 is right** —
  only that MHGU cannot be processed with MHXX constants, which the fix now enforces.
* **R5 — `FIRST_SLOT_BASE = 0x126474` is right for MHXX.** The real save and both blank MHXX templates (Switch and
  3DS) store `0x126474 / 0x244C34 / 0x3633F4`, and `empty_switch_save` now reproduces that, so the endpoint
  identity `0xB280 + 0x45F4 + 205*0x1400 + 45*0x800 == slot_base(0) == 0x126474` holds for the synthetic save
  (executed). For MHGU it must be `0x18CC78` (blank template); no code hard-codes it, which is correct since the
  geometry differs anyway.
* **R6 — the new real-save policy works, but "green" can still mean "nothing ran".** Executed:
  `MHNX_SKIP_REAL_SAVE=1 cargo test -p mhnx-core --test quest_injection` -> 6 passed (all return early);
  `MHNX_REAL_SAVE=<bogus> cargo test ... --test quest_injection` -> 6 failed loudly with "no save at ... - point
  MHNX_REAL_SAVE at one, or set MHNX_SKIP_REAL_SAVE=1". That is exactly claim 5's new contract. But
  `regression_review.rs` has no such guard (I5), so on a machine without the dump it still reports 6 passes.
* **R7 — the crate did not compile for a few minutes in the middle of this review** because another agent was
  editing `charm.rs` / `hbedit.rs` concurrently (`cargo build -p mhnx-core` -> `unresolved imports crate::charm::Charm`).
  It compiled again afterwards and every check above was executed against the pinned revisions in the table:
  `cargo test -p mhnx-core` (green), the probe crate (N1/C2/C3/C5/C6/N3/N4/R5), the two env-var test runs (R6).
  Note that `save.rs` grew from 32,466 to 32,918 bytes at 16:12 during that edit; I re-read every `save.rs` line
  this review cites (`options` 481-496, `empty_switch_save` 845-863, `FIRST_SLOT_BASE` 48) after it and they are
  unchanged.

---

## PRIORITISED LIST

1. **(blocking) dlc.rs:155 — bound the payload read** (`base + 8 + n <= body.len()`, else take the empty-data
   branch), then make dlc.rs:339-346 actually reproduce N1 (nonzero id + `size = 0x13F8` at the crafted pointer)
   so the regression is real. This is the earlier review's fix #4, still open on the read path, and it takes down
   `mhnx check-save` and the GUI.
2. **(high) Decide the "bit already set" precondition (N3).** Minimum: for ids found in the equipment catalogue
   (`section+0x1049F4`) verify bit k is set and return a distinct error if it is not; for event ids, warn
   explicitly that the premise is unverified or require an explicit force flag. Stop claiming in the doc that the
   bit "is already set" unconditionally.
3. **(high) Close the one-way door (N4).** `uninstall_quest` should refuse (or need a force flag) when the id
   still has a set bit, because the state it creates cannot be undone by this tool; and the GUI's delete button
   and the message at app.rs:392 must say the game may still list the entry.
4. **(high) Make the tests honest**: fix dlc.rs:339-346 (N2), gate regression_review.rs:12-15 on the same env var
   as quest_injection.rs (I5), and either delete dlc.rs:269-281 or relabel it "constants agree with each other",
   keeping the real-save check as the only evidence (I2).
5. **(medium) Update the text that contradicts the code**: spec §3.1 line 88, §6 line 241, §7 lines 257-274;
   main.rs:27; app.rs:1210-1212; dlc.rs:7, :12-13, :85-87.
6. **(medium) Gate `bitmask` / `installed_bit_count` on check_kind (N5)** and document the new
   `data.len() != size` contract on `InstalledQuest` (I7).
7. **(low) Duplicate ids (I6)**: `install_quest` should refuse or replace every occurrence instead of silently
   leaving a stale twin.
8. **(low) Do not set `dirty` in `install_dlc` when every file was refused (app.rs:383)**; report the refusal count
   in the summary line, not only in the log.

## Reproduction

* `C:\anyWorkspace\nx\mhxx\_rereview\probe` — `cargo run` prints the panic (N1), the clear-bit acceptance (N3),
  the uninstall result (C6/N4), the MHGU refusal and 3DS acceptance (C3) and the endpoint identity (R5). It reads
  the real save and only ever mutates in-memory copies.
* The numeric tables above came from one-off read-only parsers run inline; they open the real save with
  `readFileSync` and never write it.
* `cargo test -p mhnx-core` was green at 16:02 JST (27 unit + 6 quest_injection + 6 regression_review + 12
  hbedit_flow + 7 real_data + 9 webapp_features + 5 charm probe + 4 check_quests) — consistent with N2: the suite
  passes while the panic is still reachable.
