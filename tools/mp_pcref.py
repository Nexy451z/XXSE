#!/usr/bin/env python3
r"""mp_pcref.py -- find rodata/string references in MHXX main.bin WITHOUT Ghidra.

MHXX (Switch, ARM32) does NOT reference rodata strings with absolute pointers.
It uses a PC-relative delta pattern:

    ldr  r1, [pc, #imm]      ; r1 = DELTA (signed), pool word
    ...
    add  r0, pc, r1          ; r0 = (addr_of_this_insn + 8) + DELTA  == target

So the literal pool word is  target - (add_pc + 8), NOT the target address.

This tool decodes every "ldr rX,[pc,#imm]" in .text, follows rX forward up to
--window instructions looking for "add rZ, pc, rX" (or "add rZ, rX, pc"), and
reports the resolved 32-bit target.  Also resolves plain absolute pools.

usage:
  python mp_pcref.py scan                 # build full table -> _mp_pcref.json
  python mp_pcref.py find 0x0154b822 ...  # report refs to given target addrs
  python mp_pcref.py near 0x154b800 0x154bb00   # refs into an address range
"""
import struct, sys, json, os, re
from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM

IMG = r"C:\anyWorkspace\nx\mhxx\extract\upd\image\main.bin"
TEXT_END = 0x013E6718
HERE = os.path.dirname(os.path.abspath(__file__))

RX_LDR_PC = re.compile(r'^r(\d+), \[pc, #(0x[0-9a-f]+)\]$')
RX_ADD = re.compile(r'^r(\d+), (pc|r\d+), (pc|r\d+)$')

def build(img, window=16):
    md = Cs(CS_ARCH_ARM, CS_MODE_ARM)
    text = img[:TEXT_END]
    insns = [(i.address, i.mnemonic, i.op_str) for i in md.disasm(text, 0)]
    refs = []
    for idx, (a, m, o) in enumerate(insns):
        if m != 'ldr':
            continue
        mo = RX_LDR_PC.match(o)
        if not mo:
            continue
        reg = int(mo.group(1)); imm = int(mo.group(2), 16)
        pool = a + 8 + imm
        if pool + 4 > len(img):
            continue
        val = struct.unpack_from('<I', img, pool)[0]
        if reg == 15:
            continue
        # absolute pool?
        if 0x1000 <= val < len(img):
            refs.append((a, pool, val, 'abs'))
        # pc-relative delta
        for j in range(idx+1, min(idx+1+window, len(insns))):
            a2, m2, o2 = insns[j]
            if m2 not in ('add', 'sub'):
                continue
            mo2 = RX_ADD.match(o2)
            if not mo2:
                continue
            rd, src1, src2 = int(mo2.group(1)), mo2.group(2), mo2.group(3)
            if m2 == 'add' and 'pc' in (src1, src2) and str(reg) in (src1.replace('r',''), src2.replace('r','')) and 'pc' != (src1 if src2=='pc' else src2):
                tgt = (a2 + 8 + val) & 0xFFFFFFFF
                refs.append((a2, pool, tgt, 'pcrel'))
                break
            if m2 == 'sub' and src1 == 'pc' and src2 == 'r%d' % reg:
                tgt = (a2 + 8 - val) & 0xFFFFFFFF
                refs.append((a2, pool, tgt, 'pcsub'))
                break
    return refs

def main():
    img = open(IMG, 'rb').read()
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'scan'
    if cmd == 'scan':
        refs = build(img)
        out = os.path.join(HERE, '_mp_pcref.json')
        json.dump([[hex(a), hex(p), hex(t), k] for a, p, t, k in refs], open(out, 'w'))
        print("resolved references: %d -> %s" % (len(refs), out))
    elif cmd == 'find':
        refs = build(img)
        wants = [int(x, 0) for x in sys.argv[2:]]
        for a, p, t, k in refs:
            if t in wants:
                print("  ref %#010x (pool %#x, %s) -> %#010x  %r" % (a, p, k, t, img[t:t+40].split(b'\x00')[0]))
    elif cmd == 'near':
        lo, hi = int(sys.argv[2],0), int(sys.argv[3],0)
        refs = build(img)
        c = 0
        for a, p, t, k in refs:
            if lo <= t < hi:
                c += 1
                print("  ref %#010x (%s) -> %#010x  %r" % (a, k, t, img[t:t+48].split(b'\x00')[0]))
        print("total %d" % c)

if __name__ == '__main__':
    main()
