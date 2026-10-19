#!/usr/bin/env python3
"""Wrap indented layout diagrams in Rust doc comments with fenced text blocks."""
import os, re

SRC = r"C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src"
FILES = ["lib.rs", "save.rs", "arc.rs", "quest.rs", "dlc.rs", "itembox.rs", "crc.rs", "gamedata.rs"]
FENCE = chr(96) * 3

pat = re.compile(r"^(//!|///)(\s{4,})(\S.*)$")

for fn in FILES:
    p = os.path.join(SRC, fn)
    if not os.path.exists(p):
        continue
    lines = open(p, encoding="utf-8").read().split("\n")
    out = []
    i = 0
    changed = 0
    while i < len(lines):
        m = pat.match(lines[i])
        if m:
            prefix = m.group(1)
            j = i
            while j < len(lines):
                mj = pat.match(lines[j])
                if not mj or mj.group(1) != prefix:
                    break
                j += 1
            run = lines[i:j]
            prev_ok = (i == 0) or (lines[i - 1].strip() in ("//!", "///"))
            next_ok = (j >= len(lines)) or (lines[j].strip() in ("//!", "///", ""))
            if len(run) >= 3 and prev_ok and next_ok:
                out.append(prefix + " " + FENCE + "text")
                out.extend(run)
                out.append(prefix + " " + FENCE)
                changed += 1
            else:
                out.extend(run)
            i = j
        else:
            out.append(lines[i])
            i += 1
    open(p, "w", encoding="utf-8").write("\n".join(out))
    print("%-12s fenced blocks: %d" % (fn, changed))
