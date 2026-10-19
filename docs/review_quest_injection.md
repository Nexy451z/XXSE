# Review: MHXX quest injection (docs/quest_injection_spec.md + crates/mhnx-core/src/dlc.rs)

Reviewed against the read-only real save
`C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`
(4,726,188 bytes = 36-byte header + 4,726,152-byte body), the blank MHXX/MHGU templates in
`C:\anyWorkspace\nx\mhxx\save_editor\repos\MHGU-MHXX-Save-Converter-Script\Blank_Switch_Save\`,
and the reference homebrew in `C:\anyWorkspace\nx\mhxx\refs\mhx_data_manager\`.
All numbers below were produced by my own parsers (`C:\anyWorkspace\nx\mhxx\_review\*.py` and a
Rust probe crate in `C:\anyWorkspace\nx\mhxx\_review\rustprobe\`); the author's scripts were not reused.
The real save was never modified.

---

## Verdict

The **geometry is right**, the **bit-set semantics are wrong**. Offsets, sizes and the record format
(checkable, and now independently cross-checked) hold up; but "bit i belongs to slot i" is refuted by
the save itself. The 28 bytes at `section+0x4C` are a **catalogue-indexed** set (bit k = "DLC catalogue
entry k is installed"), not a per-slot occupancy map. Every mask write in `dlc.rs`
(`quest_bit`, `set_quest_bit`, and therefore `install_quest`/`uninstall_quest`) targets the wrong bit and
can corrupt another quest's flag. The tests cannot catch this because four of the five real-save tests
encode the same wrong model, one is tautological, one is vacuous in this save, and all five silently
pass when the hard-coded save path is absent. Two genuine panics (no bounds check) exist in
`dlc.rs` on short/corrupt sections.

---

## CONFIRMED

1. **Container / section (claim 4 inputs).** file 4,726,188 = 0x24 + 4,726,152; `body[0] = 0xC6`;
   `body+0x0C = 0xB280` (file-relative 0xB2A4); slot bases at `body+0x10/0x14/0x18` =
   `0x126474 / 0x244C34 / 0x3633F4`, stride `0x11E7C0`. The blank MHXX template has the *same*
   `0xB280 / 0x126474`, so this is not a property of one dump. **CONFIRMED.**

2. **Claim 1 — quest array base and stride.** 175 records were found at exactly
   `section + 0x45F4 + i*0x1400` for `i = 0..199`; all have unique ids in 1010001..1120012, all payloads
   start with `41 52 43 00` ("ARC\0"), sizes are 1733..3453 (max 0xD7D), all ≤ 0x13F8.
   Slots 135..159 and 200..204 (30 slots) are all-zero with id 0. A wrong stride cannot produce 175
   self-consistent records, so **stride 0x1400 is confirmed by observation**; the count 205 is confirmed
   indirectly (see 4).

3. **Claim 3 — record format.** `{s32 id; s32 size; u8 data[size]}`, `id == 0` ⇒ free, and free slots are
   byte-for-byte zero. **CONFIRMED.** Caveat: the bytes *after* the payload inside a slot are **not**
   zero — all 175 slots carry stale tail bytes (e.g. slot 0 at `section+0x45F4+8+1821` = `29 bb 7b 5d`),
   so "a slot is id+size+payload+zeros" is false; only `size` is authoritative.

4. **Claim 4 — the endpoint.** `0xB280 + 0x45F4 + 205*0x1400 + 45*0x800 = 0x126474` = `slot_base(0)`
   exactly, on both MHXX saves. **CONFIRMED as an identity for MHXX.**
   Endpoint arithmetic *alone* does not fix the decomposition (`5q + 2e = 1115` in 0x400 units also
   admits 223/0, 221/5, ..., 205/45), but the equipment region settles it: at `section+0x1049F4` there are
   records every 0x800 bytes whose first u32 is 1020001..1020014, 1120001..1120006, 1020015..1020019,
   `0`, 1020021..1020029, 1120007..1120012, 0,0,0,0 — i.e. 45 slots of 0x800 (41 occupied), which forces
   `q = 205`. This is the *evidence for* the layout, and it is missing from the spec and from `dlc.rs`.

5. **Claim 5 — the reference targets MHX.** `quest.c:25-28` (0x1400/0x1C00/0x34/0x45D4), loops of 140
   (`quest.c:69,75,117,173`), `common.c:24-28` (only MHX JPN 0x1554 / MHGen EUR 0x185B / MHGen USA 0x1870).
   **CONFIRMED.**

6. **Claim 7's raw numbers.** 175 records; 166 bits set in the 0..204 window; 140 slots agree with
   "bit i = slot i" (35 records without their own bit, 26 bits without a record). All three reproduce
   exactly. (The *conclusion* drawn from them is wrong — see ERRORS 1.)

7. **`enable_all()` is dead.** Grep over `crates/*/src/*.rs` and `switch/src/*.rs`: `enable_all` and
   `clear_all` appear only at their definitions (`dlc.rs:112`, `dlc.rs:121`). No caller anywhere, no raw
   0xFF write to 0x4C..0x67 anywhere else. The "we stopped enabling everything" story is true in code —
   the replacement logic is just aimed at the wrong bits.

---

## ERRORS

### E1. `dlc.rs:73-75, 76-106` — "bit i belongs to slot i" is false; the mask is catalogue-indexed

The 28 bytes at `section+0x4C` are `ff ff ff fd ff 01 00 00 fd ff e7 df 77 fb ff 7f fd ef b7 f7 ff bf ff ff bf 7f 7f 00`.

**Direct proof against the slot model:** bit 25 is **clear**, yet slot 25 holds id **1011019**
(`section+0x45F4+25*0x1400`). Bits 205..214 are **set**, yet a 205-slot array has no slot 205..214.
Recompute the agreement as a null model: with 175 records in 205 slots and 166 set bits, the number of
accidental coincidences is hypergeometric with mean `166*175/205 = 141.7`, and
`P(X ≥ 140) = 0.87`. **The observed "140 matches" is exactly chance** — it is not evidence *for* the slot
model, and the spec presents it as if it were ("最も整合的です", spec:136).

**Positive proof of the catalogue model:** the equipment array at `section+0x1049F4` writes, for each
*challenge-quest catalogue entry* k, the id it belongs to (slot k of the array). Reading the 0x4C mask
with **bit k = catalogue entry k** gives a **perfect 41/41 fit**:

| k | id from equipment array | installed? | mask bit k |
|---|---|---|---|
| 0..24 | 1020001..1020014, 1120001..1120006, 1020015..1020019 | yes | 1 (all) |
| 25 | (empty slot ⇒ id absent from the save, the 1020001..1020029 gap = 1020020) | no | **0** |
| 26..40 | 1020021..1020029, 1120007..1120012 | yes | 1 (all) |
| 41..44 | (empty) | no | 0 |

Zero mismatches. The 40 challenge records are exactly catalogue entries {0..24, 26..40}. The remaining
**135 set bits (bits 64..214) number exactly the 135 event records**, and pairing the 135 ids with the
135 bits in order gives a *piecewise-constant* `bit − id` (e.g. bit 64=1010001, 66=1010003, 67=1010004,
68=1010005, 69=1010101...; the last bits 205..214 = 1110111, 1110112, 1111001..1111007), i.e. the bits
run in catalogue order over contiguous id ranges. Bits 45..63 are all clear.

This is the *same* structure the reference homebrew documents for MHX: `quest.c:83-94` (`get_bits`)
puts the challenge quests in the first u32 (bits 0..19 used) and the events from the second u32
(bits 32..151) — a catalogue index, never a slot index. In MHXX the challenge catalogue grew to 45
entries, so the challenge region occupies two u32 (bits 0..63, 45 used) and the events start at bit 64
(160 entries, bits 64..223 ⇒ 64+160 = 224 = 28 bytes). The total mask is thus 7 u32 = 224 bits, and the
save's **175 set bits biject onto the 175 records exactly** — the mask is *not* "out of step".

Corroborating coincidence that should have raised doubt: the mask has exactly 175 set bits for 175
records (a ~3 % coincidence under the author's model); under the catalogue model it is expected.

### E2. `docs/quest_injection_spec.md:137` — the reference is misread

> 「ビット i はスロット i に対応する…(MHX Data Manager の get_ids() も、ビットの位置とスロットの位置を同じ添字で扱っています)」

`quest.c:68-81` `get_ids()` maps **bit i → the i-th entry of `challenge_quest_ids` / `event_quest_ids`**,
i.e. a *catalogue id*, not a slot. `quest.c:179-182` then matches a record to a slot purely by
**id equality** (`file_ids[j] == quest_info[0]`); a slot index never touches a bit index. So the cited
source contradicts the claim rather than supporting it.

### E3. `dlc.rs:181-190` — replace-in-place flips a bit that belongs to a *different* quest

Concrete, on the real save (in-memory copy, via the probe):
`install_quest(s, 1011019, payload)` finds 1011019 in slot 25, rewrites the slot correctly, and then calls
`set_quest_bit(s, 25, true)`, turning on **bit 25 = challenge catalogue entry 25** (the id at
`section+0x1049F4+25*0x800` is 0, and no slot holds it). The save now advertises an installed challenge
quest whose payload does not exist — precisely the class of corruption the design is trying to avoid.
Nothing about the replaced quest needed a mask write: its bit was already set.

### E4. `dlc.rs:191-201` — "install into the first free slot" cannot work

On the real save the first free slot is **135**, and bit 135 is already set (it belongs to an installed
event quest). Probe output: `install(9990001) -> slot 135`, **mask bits changed = []**. The new quest gets
no catalogue bit (9990001 is not in the DLC catalogue at all, so it can never be enumerated by the game),
while `set_quest_bit(..., 135, true)` is a no-op here and, in a save where bit 135 happened to be clear,
would instead mark *another* quest as installed. Either way the branch is unsupported and unsafe.

### E5. `dlc.rs:158-202` — `quest_id == 0` is accepted

`install_quest(s, 0, b"ARC\0zero")` returns `Ok(slot 135)`: `find_quest(s, 0)` matches the first *free*
slot, the slot is zeroed, id 0 and a payload are written, and bit 135 is set. Record count stays 175
(id 0 = free) and the payload is silently discarded. `hbedit.rs:1088-1091` guards against `id == 0`, but
`mhnx-cli` `cmd_install` (`crates/mhnx-cli/src/main.rs:204-211`) does **not** — it takes whatever u32 the
`.mib` has at 0x0C and casts it with `as i32`. `dlc.rs` should reject it itself.

### E6. `dlc.rs:184` and `dlc.rs:149` — panics instead of `Err`

Both slice without checking the **whole slot / whole payload**:

* `dlc.rs:136` only checks `base + 8 > body.len()` (or `break`), but `dlc.rs:149` reads
  `body[base+8 .. base+8+n]` with `n` up to 0x13F8.
* `install_quest`'s replace path (`dlc.rs:184`) fills `base..base+0x1400` with **no** size check, while the
  new-slot path is protected by `dlc.rs:170` and `uninstall_quest` by `dlc.rs:215`.

Reproduced with a synthetic Switch file whose `body+0x0C = 0x47C39C` (body is 0x481D88):
`list_quests` → **panic at dlc.rs:149**, `install_quest(same id)` → **panic at dlc.rs:184**
("range end index 4726160 out of range for slice of length 4726152"), while
`uninstall_quest(0)` → `Err(OutOfRange)` and `install_quest(new id)` → `Err(Parse("no free download-quest slot"))`.
A corrupt or foreign section offset therefore crashes the library — and `mhnx check-save`, which calls
`list_quests` (`check.rs:362`), crashes instead of reporting a finding.

### E7. `dlc.rs:144` — silent payload clamp

`size.clamp(0, QUEST_MAX_PAYLOAD as i32)` means a corrupt record with `size = i32::MAX` yields
`InstalledQuest { size: 2147483647, data.len(): 5112 }` (probe) — the struct's own invariant
`data.len() == size` is broken silently. `tests/quest_injection.rs:49` asserts that invariant, so any
save that trips the clamp would make the test fail with a misleading message (or, better, the library
should reject rather than truncate).

### E8. `docs/quest_injection_spec.md:88-89` — the "MHX array at 0x45D4" does not exist here

`section+0x45D4 .. section+0x45F4` is **32 zero bytes** in the real save *and* in the blank MHXX template,
and an MHX-geometry walk (140 slots of 0x1400 from 0x45D4) finds **0** plausible records. So:
no second array, no stale MHX data to be confused by. What *is* non-zero is the MHX-format **mask** region
at `section+0x34` (see U2) — the spec's table (spec:88-89) presents both as if they were live structures.

### E9. `dlc.rs:12-13, 291-299` — "confirmed by the endpoint" is not a confirmation, and the unit test is a tautology

`dlc.rs:291-299` asserts `0xB280 + 0x45F4 + 205*0x1400 + 45*0x800 == 0x126474`; the "save" it builds is
synthetic (`empty_switch_save()` writes that very 0xB280) — the assertion is pure constant arithmetic and
cannot fail. The real-save test `quest_injection.rs:29-32` has the same tautology. The claim is only
*evidence* when read against the save's own `slot_base(0)`, which the tests never check.

---

## UNSUPPORTED / UNCERTAIN

### U1. Claim 6 is not "uncertain" — it is refuted
Spec:136-137 says the slot mapping is "最も整合的", spec:130-131 blames a cheat tool for a mask that is
"ずれたまま". Both are superseded by E1: the mask agrees perfectly with the records once indexed by
catalogue entry, and the "140 matches" is the chance expectation. The save is not corrupt.

### U2. `section+0x34` is *not* established as the MHX-compatible mask
It is 20 non-zero bytes (112 bits set: bits 0..46, bit 55, bits 56-58, bits 64..124), and it does **not**
decode as the reference's MHX catalogue mask: only 88 of the 106 MHX-catalogue ids it would claim are
present (chance = 84.8), and `bit[0] = 0xFFFFFFFF` would assert all 20 challenge quests installed.
It *is* slot-correlated in a way I cannot explain: all 112 set bits land on slots that hold a record
(chance expectation 94.5; `P = 2.7e-16`), and 23 occupied slots in 0..159 are unmarked
(47-54, 59-63, 125-134) — the signature of a stale per-slot mask, or of a second, different meaning.
**No writer in this repo touches 0x34, which is the right call; but the doc's description of it as the
MHX-format mask is unsupported and should be marked unknown.**

### U3. The event half of the catalogue is inferred, not read
I verified the *alignment* (135 bits ↔ 135 event records; contiguous id runs → contiguous bits; the ids my
in-order pairing assigns to bits 64..214 are all present in the save). I did **not** verify the actual
160-entry event catalogue or the individual bit↔id mapping: the catalogue table itself is not stored in
the save in a form I could read (the ids of installed quests never appear as u32 anywhere in
`section+0x68 .. section+0x45F4`). Treat the event mapping as "structure known, table unknown".

### U4. `0x1C00` is unreachable in MHXX-JP — but it is a per-*game* flag, not a per-record one
`quest.c:249-253,268-272` passes `large = 0` for MHX JPN and `1` for EUR/USA, and only the stride and the
`size+8` limit change (`quest.c:191`). In the MHXX Switch save the stride is provably 0x1400 (175
records + the endpoint), so a 0x1C00 record cannot be stored: 0x1C00 > 0x1400 would spill into the next
slot. `QUEST_MAX_PAYLOAD = 0x13F8` is correct for MHXX and `install_quest` does enforce it
(0x13F8 ⇒ Ok, 0x13F9 ⇒ `OutOfRange`). What is *missing* is that the library never checks which game it is
looking at (see M1).

### U5. Claim 4's "ends exactly at the first character slot" is not a law
On the blank **MHGU** template the same arithmetic gives `0x126475` while `slot_base(0) = 0x18CC78`.
The identity is a property of the MHXX layout, and must not be used as a validation rule for other kinds.

---

## MISSING CASES

### M1. MHGU / 3DS saves are accepted and then parsed with MHXX constants
`Save::from_bytes` accepts `SaveKind::Mhgu` (5,159,100 bytes; `save.rs:33`) and `SaveKind::ThreeDs`
(4,726,152 bytes, no header), but every constant in `dlc.rs` is MHXX-specific and nothing checks
`save.kind()`. Measured on the blank MHGU template: `body+0x0C = 0xB281` (one byte later than MHXX — the
`save.rs:475` comment is right) and `slot_base(0) = 0x18CC78`, so the MHXX endpoint identity fails there.
I could **not** determine MHGU's real quest geometry (the only MHGU file available is all-zero), so I
cannot say which of base/stride/count/equipment-size differs; the space from the section start to
`slot_base(0)` is 0x1815F7 = 1,578,487 bytes, enough for the MHXX geometry (1,159,668), so the MHXX
numbers are not obviously impossible — they are simply **unverified** for MHGU. Given that the reference
uses a *different* (larger) slot size exactly for the localized releases, writing to an MHGU save with
MHXX constants is a live risk of corrupting character/shared data. `dlc.rs` should refuse
(`Error::Parse`) unless `kind() == Switch`, or be given per-kind geometry.

### M2. "Install a new quest id into a free slot" is unsupported
See E4. Since the game enumerates the *catalogue*, a free-slot install of an id the catalogue does not
contain produces a record that is invisible, plus a wrong bit. The only supported operation the evidence
supports is *replace an existing id in place* (spec §4 is right about this); the code should not silently
offer the other path, or should at least require the id to be an existing catalogue member whose bit is
already set.

### M3. The region between the mask and the array is unexamined and adjacent to a write
`section+0x68 .. section+0x45F4` is 0x458C = 17,804 bytes with 1,128 non-zero bytes, starting with
`8a 41 00 00` immediately at `section+0x68` and continuing with 0x68-strided records holding UTF-8
Japanese text (e.g. `MHXX` at `section+0xAFC`, `ハチミツセット` at 0xB00, palico-style names such as
`ナナコ８８` / `笑顔の店員` at `section+0x1DA0`). Neither `dlc.rs` nor the spec mentions it. Two
consequences: (a) the 28-byte mask length is bounded **exactly** by the start of that data at 0x68 — a
"nice round" 32-byte (8 x u32) mask write, which `enable_all()` narrowly avoids, would clobber it;
(b) if 0x4C..0x67 is ever the wrong window, everything downstream is silently corrupt.

### M4. Stale bytes inside a slot / non-zero tails
All 175 slots keep non-zero bytes after the payload. Any future "compare slots" or "is this slot empty"
logic must use `id == 0` (which is reliable) rather than zero-scanning.

### M5. Duplicate ids
`find_quest` returns the first match; `check_save` (`check.rs:362-373`) warns about duplicates, but
`install_quest` would rewrite only the first occurrence and leave the second stale with a full payload.

---

## PRIORITISED FIXES

1. **(blocking) Stop writing the 0x4C mask from a slot index.** Delete/replace `set_quest_bit` and
   `quest_bit` (`dlc.rs:76-106`). An in-place replace needs **no** mask write at all — the bit for an
   already-installed quest is already correct. `enable_all`/`clear_all` (`dlc.rs:112-127`) should be
   deleted rather than kept as public API.
2. **(blocking) Remove or gate the free-slot install path** (`dlc.rs:191-201`) until a catalogue
   (id → catalogue index) exists; without it `install_quest` cannot make a quest visible and can only
   corrupt another. If a catalogue is added, it must be derived from evidence (the 45-entry challenge
   table in `section+0x1049F4` is directly readable; the event table is not, so a checked-in table with a
   test that re-verifies it against a real save is required).
3. **(blocking) Reject `quest_id <= 0`, and reject ids whose bit is not already set** — or at minimum
   refuse to write a mask bit other than one that is already set. Fix the CLI as well
   (`crates/mhnx-cli/src/main.rs:204-211` casts a u32 to i32 and accepts 0).
4. **(high) Fix the bounds checks**: check `base + QUEST_SLOT_SIZE <= len` in the replace path
   (`dlc.rs:181-190`, before line 184) and `base + 8 + n <= len` in `list_quests` (`dlc.rs:149`); return
   `Error::OutOfRange` instead of panicking; make `list_quests` return `Result` or stop early. Add a
   regression test with `body+0x0C` near the body end.
5. **(high) Guard on `save.kind()`** in `dlc.rs` (M1): refuse non-Switch kinds, or add per-kind geometry
   once an MHGU save is available to measure.
6. **(high) Fix the tests** (`crates/mhnx-core/tests/quest_injection.rs`): remove the silent
   `if !p.exists() { return; }` early returns (fail or `#[ignore]` instead), make the save path
   `MHNX_TEST_SAVE`-overridable, replace the tautology at lines 29-32 with
   `assert_eq!(s.slot_base(0), Some(0x126474))`, and rewrite
   `the_installed_bit_set_lines_up_with_the_records` — under the correct model the correct assertions are
   "the 40 challenge bits equal the equipment-array catalogue's installed entries" and "the number of set
   bits equals the number of records". `adding_a_new_quest_sets_only_its_own_bit` (lines 111-135) currently
   passes with `changed.len() == 0` and should be replaced by a test that fails if a bit outside the
   touched quest's catalogue entry changes; `dlc.rs:291-299` should assert against a real save.
7. **(medium) Document the truth**: update `docs/quest_injection_spec.md` §3.3 (catalogue semantics,
   the 64/160 bit split, the 45-entry challenge table as the evidence) and the `dlc.rs` header comment
   (`dlc.rs:3-16`, including the misleading "7 x u32, 0xFFFFFFFF = enabled"), and mark `section+0x34` as
   unknown (U2).
8. **(medium) Drop or reject the silent size clamp** (`dlc.rs:144`) and document the stale tail bytes.

---

## What I could not verify (stated explicitly)

* The game's own use of the mask — no disassembly of the Switch binary was done here; the catalogue model
  is inferred from save-internal structure (the equipment array is an independent witness, which is what
  makes it decisive).
* The full event catalogue (160 entries) and therefore the exact id behind bits 205..214.
* MHGU's quest geometry (only an all-zero MHGU template exists in the workspace).
* The identity of equipment entry 25 (inferred to be 1020020 from the 1020001..1020029 gap).
* The meaning of the `section+0x34` 20-byte region.

## Reproduction

* `C:\anyWorkspace\nx\mhxx\_review\p1_geometry.py`, `p2_walk.py`, `p3_regions.py`, `p4_hyp.py`,
  `p5_catalogue.py`, `p6_counterexamples.py`, `p7_gap.py`, `p8_records.py`, `p9_34.py`, `p10_blank.py`
  — read-only parsers.
* `C:\anyWorkspace\nx\mhxx\_review\rustprobe\` — `cargo run` reproduces the panics, the `id = 0`
  acceptance, the slot-135 mask no-op, the bit-25 flip and the silent clamp against `mhnx-core` itself.
* `cargo test -p mhnx-core --test quest_injection` → 5 passed (all five pass even though E1-E4 hold).
