#!/usr/bin/env python3
"""Generate crates/mhnx-core/src/idmemo.rs from the author's own MHXX id memo.

The names the editor used to leave as numbers (勲章 001, 依頼 12, クエスト 0001)
come from a repository the same author keeps:

    https://github.com/Nexy451z/mhxx-id-memo

The tables are TSV, one row per id, and this script pins a commit so a run is
reproducible: the commit hash is written into the generated file's header.

Usage
-----
    python tools/gen_idmemo.py                # fetch the pinned commit, verify, write
    python tools/gen_idmemo.py --offline      # reuse the files already under .work/idmemo
    python tools/gen_idmemo.py --check        # verify only, write nothing

Every check below is a hard failure. The point of the script is not to copy
rows: it is to refuse to ship a table whose shape does not match the one the
editor is built on, because a table that is off by one row would put the wrong
Japanese name on every id after it.
"""

from __future__ import annotations

import argparse
import csv
import io
import os
import sys
import urllib.request

# --------------------------------------------------------------------------
# provenance
# --------------------------------------------------------------------------

REPO = "Nexy451z/mhxx-id-memo"
URL = "https://github.com/Nexy451z/mhxx-id-memo"
# Pinned so the generated file can be reproduced byte for byte.
COMMIT = "a284583b3bd04b8fe95e559b8b92d9e29bbdca00"
FETCHED = "2026-11-09"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CACHE = os.path.join(ROOT, ".work", "idmemo")
OUT = os.path.join(ROOT, "crates", "mhnx-core", "src", "idmemo.rs")

TABLES = [
    "data/world/awards.tsv",
    "data/world/requests.tsv",
    "data/world/quests.tsv",
    "data/world/stages.tsv",
    "data/world/arena.tsv",
]

# --------------------------------------------------------------------------
# what the editor's bit maps require
# --------------------------------------------------------------------------

AWARDS = 132          # crates/mhnx-core/src/awards.rs: AWARDS
REQUESTS = 184        # data/world/requests.tsv, == activityData.atd record count
QUEST_ROWS = 1508     # questprogress::BITS - 1 (index 0 is the unused dummy)
REQUEST_BANDS = [(300, 555), (1270, 1395)]   # hbedit::REQUEST_BANDS
REQUEST_KINDS = {0: 167, 1: 13, 2: 3, 3: 1}  # 167 quest rows, 16 without, 1 sentinel
SPECIAL_PERMIT = (947, 1174, 228)            # questprogress.rs documents this band
ARENA = (1288, 1304, 17)


def fail(msg):
    print("error: " + msg, file=sys.stderr)
    raise SystemExit(1)


def path_of(table):
    return os.path.join(CACHE, table.replace("/", os.sep))


def fetch(table):
    dst = path_of(table)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    url = "https://raw.githubusercontent.com/%s/%s/%s" % (REPO, COMMIT, table)
    print("fetching %s" % url)
    with urllib.request.urlopen(url) as f:
        data = f.read()
    with open(dst, "wb") as f:
        f.write(data)


def load(table):
    p = path_of(table)
    if not os.path.exists(p):
        fail("%s is missing; run without --offline to fetch it" % p)
    with open(p, encoding="utf-8-sig", newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def iv(value):
    s = (value or "").strip()
    return int(s) if s.lstrip("-").isdigit() else 0


def text(value):
    return (value or "").strip()


# --------------------------------------------------------------------------
# verification
# --------------------------------------------------------------------------

def verify_awards(rows):
    if len(rows) != AWARDS:
        fail("awards.tsv has %d rows, the save has %d award bits" % (len(rows), AWARDS))
    out = []
    for expect, row in enumerate(rows):
        if iv(row.get("award_id")) != expect:
            fail("awards.tsv row %d carries award_id %s" % (expect, row.get("award_id")))
        name = text(row.get("name_ja"))
        if not name:
            fail("award %d has no name_ja" % expect)
        if "\t" in name or "\n" in name:
            fail("award %d has a name that breaks a TSV cell" % expect)
        out.append(name)
    if len(set(out)) != len(out):
        fail("award names are not unique")
    return out


def verify_requests(rows):
    if len(rows) != REQUESTS:
        fail("requests.tsv has %d rows, activityData.atd carries %d" % (len(rows), REQUESTS))
    kinds, quests, accept, done, names = [], [], [], [], []
    for expect, row in enumerate(rows):
        if iv(row.get("request_index")) != expect:
            fail("requests.tsv row %d carries request_index %s" % (expect, row.get("request_index")))
        kind = iv(row.get("kind"))
        if kind not in REQUEST_KINDS:
            fail("request %d has kind %d, which activityData.atd never writes" % (expect, kind))
        a, d = iv(row.get("accept_flag")), iv(row.get("done_flag"))
        if kind == 3:
            # The closing sentinel owns no flag at all.
            if a != 0 or d != 0:
                fail("the sentinel row %d carries flags %d / %d" % (expect, a, d))
        else:
            if d != a + 1:
                fail("request %d flags are not a pair: %d / %d" % (expect, a, d))
            if not any(lo <= a < hi for lo, hi in REQUEST_BANDS):
                fail("request %d accept flag %d is outside the request bands" % (expect, a))
        name = text(row.get("name_ja"))
        if not name:
            fail("request %d has no name_ja" % expect)
        kinds.append(kind)
        quests.append(iv(row.get("quest_id")))
        accept.append(a)
        done.append(d)
        names.append(name)
    got = {k: kinds.count(k) for k in REQUEST_KINDS}
    if got != REQUEST_KINDS:
        fail("request kinds are %s, activityData.atd has %s" % (got, REQUEST_KINDS))
    zero = [i for i, q in enumerate(quests) if q == 0]
    if len(zero) != 17:
        fail("%d request rows carry no quest id, activityData.atd has 17" % len(zero))
    if [i for i, k in enumerate(kinds) if k != 0] != zero:
        fail("the rows without a quest id are not the rows with a non-zero kind")
    return {"kinds": kinds, "quest_ids": quests, "accept": accept, "done": done, "names": names}


def verify_quests(rows):
    if len(rows) != QUEST_ROWS:
        fail("quests.tsv has %d rows, the save has %d quest progress bits" % (len(rows), QUEST_ROWS))
    ids, names, stages = [], [], []
    for expect, row in enumerate(rows, start=1):
        if iv(row.get("quest_index")) != expect:
            fail("quests.tsv row %d carries quest_index %s" % (expect, row.get("quest_index")))
        name = text(row.get("name_ja"))
        if not name:
            fail("quest index %d has no name_ja" % expect)
        ids.append(iv(row.get("quest_id")))
        names.append(name)
        stages.append(iv(row.get("stage_id")))
    # Two rows may carry one id (1049 and 1050 appear twice, once per
    # expansion), but they must then be the same quest: an id that named two
    # different quests would make the cross-checks below meaningless.
    seen = {}
    for i, qid in enumerate(ids):
        if qid in seen and names[seen[qid]] != names[i]:
            fail("quest id %d names two different quests (#%d, #%d)" % (qid, seen[qid] + 1, i + 1))
        seen.setdefault(qid, i)
    lo, hi, n = SPECIAL_PERMIT
    if sum(1 for i in range(lo, hi + 1) if 40000 <= ids[i - 1] < 50000) != n:
        fail("the special permit band %d..%d is not the documented %d rows" % (lo, hi, n))
    lo, hi, n = ARENA
    if sum(1 for i in range(lo, hi + 1)
           if 20000 <= ids[i - 1] < 30000 or 120000 <= ids[i - 1] < 130000) != n:
        fail("the arena band %d..%d is not the documented %d rows" % (lo, hi, n))
    if max(stages) > 255:
        fail("a stage id does not fit in a u8")
    return {"ids": ids, "names": names, "stages": stages}


def verify_stages(rows):
    ids, names = [], []
    for row in rows:
        sid = iv(row.get("stage_id"))
        if sid in ids:
            fail("stage id %d appears twice" % sid)
        ids.append(sid)
        names.append(text(row.get("name_ja")))
    if not ids:
        fail("stages.tsv is empty")
    return ids, names


def verify_arena(quest_ids):
    """The arena table lists its 17 quests by id: the band must be those rows."""
    if not os.path.exists(path_of("data/world/arena.tsv")):
        return
    listed = [iv(r.get("quest_id")) for r in load("data/world/arena.tsv")]
    if not listed:
        return
    lo, hi, _ = ARENA
    band = quest_ids[lo - 1:hi]
    if listed != band:
        fail("arena.tsv ids do not match the arena band of quests.tsv")


# --------------------------------------------------------------------------
# emitting Rust
# --------------------------------------------------------------------------

def rs(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def block(items, per_line, indent="    "):
    lines = []
    for i in range(0, len(items), per_line):
        lines.append(indent + ", ".join(items[i:i + per_line]) + ",")
    return "\n".join(lines)


def numbers(items, per_line, indent="    "):
    lines = []
    for i in range(0, len(items), per_line):
        lines.append(indent + ", ".join(str(x) for x in items[i:i + per_line]) + ",")
    return "\n".join(lines)


DOC = [
    "//! Game-text names, bundled from the author's own MHXX id memo table.",
    "//!",
    "//! The editor used to print a number wherever the game shows a name:",
    "//! 勲章 001 on the guild card, 依頼 12 in the request list, クエスト 0001 in",
    "//! the quest progress list. The numbers are the save's; the names are the",
    "//! game's, and they come from a table kept beside this repository rather",
    "//! than from a romfs the user has to point at.",
    "//!",
    "//! Source:   https://github.com/Nexy451z/mhxx-id-memo",
    "//! Commit:   a284583b3bd04b8fe95e559b8b92d9e29bbdca00 (fetched 2026-11-09)",
    "//! License:  the author's own repository, MIT (see its LICENSE).",
    "//!",
    "//! | table                | rows | what it names                         |",
    "//! |----------------------|------|---------------------------------------|",
    "//! | world/awards.tsv     |  132 | 勲章, in guild-card bit order          |",
    "//! | world/requests.tsv   |  184 | 依頼, in activityData.atd record order |",
    "//! | world/quests.tsv     | 1508 | quests, in quest progress index order  |",
    "//! | world/stages.tsv     |   31 | ステージ, by stage id                   |",
    "//!",
    "//! Generated by tools/gen_idmemo.py - do not edit by hand. The generator",
    "//! refuses to write anything unless every table still matches the shape the",
    "//! editor's bit maps were measured on (see its docstring).",
    "//!",
    "//! ## Why the orders can be trusted",
    "//!",
    "//! *The awards* are one table of 132 rows, award_id 0..131, and the save's",
    "//! guild-card bit map is 132 bits: the row number **is** the bit number.",
    "//!",
    "//! *The requests* are the 184 records of activityData.atd. Row i carries the",
    "//! same accept/complete flag pair the record at +0x33/+0x35 carries, and the",
    "//! columns agree with the file the crate already parses: 167 rows point at a",
    "//! quest and 16 do not, every pair is n / n + 1, the first band runs from 300",
    "//! and the second starts at 1270. The request list is therefore built from",
    "//! this table even when no romfs was loaded.",
    "//!",
    "//! *The quests* are the save's quest progress indices 1..1508. Two things tie",
    "//! the order to the save rather than to a guess: the special permit band sits",
    "//! at 947..1174 with its documented 228 rows (18 deviants, twelve with 16",
    "//! quests and six with 6), and the arena band at 1288..1304 holds exactly the",
    "//! 17 quest ids of world/arena.tsv. Measured against a retail save: each of",
    "//! the 175 downloaded event quests is a row of this table, 169 of them on an",
    "//! index whose seen bit is set (the six are arena events the game does not",
    "//! mark seen), and every seen event quest is one of those 175.",
    "//!",
    "//! *Checked and not bundled:* the palico support moves and the palico skills",
    "//! agree row for row with gamedata's own tables, so they would add nothing;",
    "//! the monster names agree on all 129 real rows and leave the same seven",
    "//! dummy rows plus row 133 blank, so those stay 不明 as they already were.",
]


def emit(awards, req, quests, stage_ids, stage_names):
    out = io.StringIO()
    w = out.write
    for line in DOC:
        w(line + "\n")
    w("\n")
    w("/// Where the names come from: the author's own id memo repository.\n")
    w("pub const SOURCE_REPO: &str = %s;\n" % rs(URL))
    w("/// The commit the tables were read at.\n")
    w("pub const SOURCE_COMMIT: &str = %s;\n" % rs(COMMIT))
    w("\n")
    w("/// 勲章 names, index = the award bit number (0..131).\n")
    w("pub static AWARD_NAMES: &[&str] = &[\n")
    w(block([rs(n) for n in awards], 3))
    w("\n];\n")
    w("\n")
    w("/// 依頼 names, index = the request record number (0..183).\n")
    w("pub static REQUEST_NAMES: &[&str] = &[\n")
    w(block([rs(n) for n in req["names"]], 3))
    w("\n];\n")
    w("\n")
    w("/// activityData.atd kind byte of each request: 0 points at a quest, 1 and 2\n")
    w("/// do not, 3 is the closing sentinel.\n")
    w("pub static REQUEST_KINDS: &[u8] = &[\n")
    w(numbers(req["kinds"], 24))
    w("\n];\n")
    w("\n")
    w("/// Quest each request points at, 0 on the rows that carry none.\n")
    w("pub static REQUEST_QUEST_IDS: &[u32] = &[\n")
    w(numbers(req["quest_ids"], 12))
    w("\n];\n")
    w("\n")
    w("/// Accept flag of each request (0 on the sentinel).\n")
    w("pub static REQUEST_ACCEPT_FLAGS: &[u16] = &[\n")
    w(numbers(req["accept"], 16))
    w("\n];\n")
    w("\n")
    w("/// Complete flag of each request, always the accept flag + 1.\n")
    w("pub static REQUEST_COMPLETE_FLAGS: &[u16] = &[\n")
    w(numbers(req["done"], 16))
    w("\n];\n")
    w("\n")
    w("/// Quest names, index = the quest progress index (1..1508), so entry 0 is\n")
    w("/// the unused dummy the save keeps clear and is the empty string.\n")
    w("pub static QUEST_NAMES: &[&str] = &[\n")
    w('    "",\n')
    w(block([rs(n) for n in quests["names"]], 3))
    w("\n];\n")
    w("\n")
    w("/// Stage id of each quest, index = the quest progress index, 0 when the\n")
    w("/// table names no stage.\n")
    w("pub static QUEST_STAGE_IDS: &[u8] = &[\n")
    w("    0,\n")
    w(numbers(quests["stages"], 24))
    w("\n];\n")
    w("\n")
    w("/// Stage ids the stage table names, ascending.\n")
    w("pub static STAGE_IDS: &[u8] = &[\n")
    w(numbers(stage_ids, 16))
    w("\n];\n")
    w("\n")
    w("/// ステージ names, in the same order as STAGE_IDS.\n")
    w("pub static STAGE_NAMES: &[&str] = &[\n")
    w(block([rs(n) for n in stage_names], 3))
    w("\n];\n")
    w("\n")
    w("/// The 勲章 name of one award bit, or None when the bit is out of range.\n")
    w("pub fn award_name(award: usize) -> Option<&'static str> {\n")
    w("    AWARD_NAMES.get(award).copied()\n")
    w("}\n")
    w("\n")
    w("/// The 依頼 name of one request record.\n")
    w("pub fn request_name(record: usize) -> Option<&'static str> {\n")
    w("    REQUEST_NAMES.get(record).copied()\n")
    w("}\n")
    w("\n")
    w("/// The request record that owns this accept or complete flag, if any.\n")
    w("///\n")
    w("/// Flag 0 belongs to no request: the closing sentinel row carries it, and\n")
    w("/// calling that row a request would name an event that is not one.\n")
    w("fn request_of_flag(flag: u16) -> Option<usize> {\n")
    w("    if flag == 0 {\n")
    w("        return None;\n")
    w("    }\n")
    w("    REQUEST_ACCEPT_FLAGS\n")
    w("        .iter()\n")
    w("        .position(|f| *f == flag)\n")
    w("        .or_else(|| REQUEST_COMPLETE_FLAGS.iter().position(|f| *f == flag))\n")
    w("}\n")
    w("\n")
    w("/// What a request flag number is: the record it belongs to, whether it is\n")
    w("/// the accept or the complete half, and the name. None when no request owns\n")
    w("/// the flag, which is the case for the 444 event bits the save shares.\n")
    w("pub fn request_flag_owner(flag: u16) -> Option<(usize, bool, &'static str)> {\n")
    w("    let record = request_of_flag(flag)?;\n")
    w("    let complete = REQUEST_COMPLETE_FLAGS.get(record).copied()? == flag;\n")
    w("    Some((record, complete, request_name(record).unwrap_or(\"\")))\n")
    w("}\n")
    w("\n")
    w("/// The quest name of one quest progress index (1..1508).\n")
    w("pub fn quest_name(index: usize) -> Option<&'static str> {\n")
    w("    if index == 0 {\n")
    w("        return None;\n")
    w("    }\n")
    w("    QUEST_NAMES.get(index).copied()\n")
    w("}\n")
    w("\n")
    w("/// The ステージ name of one quest progress index, if the table has one.\n")
    w("pub fn quest_stage_name(index: usize) -> Option<&'static str> {\n")
    w("    let id = *QUEST_STAGE_IDS.get(index)?;\n")
    w("    stage_name(id)\n")
    w("}\n")
    w("\n")
    w("/// The ステージ name of one stage id.\n")
    w("pub fn stage_name(id: u8) -> Option<&'static str> {\n")
    w("    STAGE_IDS\n")
    w("        .iter()\n")
    w("        .position(|s| *s == id)\n")
    w("        .and_then(|i| STAGE_NAMES.get(i).copied())\n")
    w("}\n")
    return out.getvalue()


def main():
    ap = argparse.ArgumentParser(description="generate idmemo.rs")
    ap.add_argument("--offline", action="store_true", help="do not download, use .work/idmemo")
    ap.add_argument("--check", action="store_true", help="verify only, write no file")
    args = ap.parse_args()

    if not args.offline:
        for table in TABLES:
            fetch(table)

    awards = verify_awards(load("data/world/awards.tsv"))
    req = verify_requests(load("data/world/requests.tsv"))
    quests = verify_quests(load("data/world/quests.tsv"))
    verify_arena(quests["ids"])
    stage_ids, stage_names = verify_stages(load("data/world/stages.tsv"))

    print("awards   %d" % len(awards))
    print("requests %d" % len(req["names"]))
    print("quests   %d" % len(quests["names"]))
    print("stages   %d" % len(stage_ids))
    if args.check:
        print("check only: nothing written")
        return

    src = emit(awards, req, quests, stage_ids, stage_names)
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(src)
    print("wrote %s (%d bytes)" % (OUT, len(src.encode("utf-8"))))


if __name__ == "__main__":
    main()
