#!/usr/bin/env python3
r"""mp_exidx.py -- function boundaries for MHXX main.bin WITHOUT Ghidra.

The image ships a full .ARM.exidx table.  MOD0 field at +0x10 points at it.
Location (verified on C:\anyWorkspace\nx\mhxx\extract\upd\image\main.bin):
    .ARM.exidx  = 0x016E082C .. 0x01740944   (0x60118 bytes = 49187 entries x 8)
Each entry is  { u32 prel31_to_func ; u32 prel31_to_extab_or_1 }  (little endian).
prel31 value is relative to the address of the entry itself.

usage:
  python mp_exidx.py map                 # print function-start count + JSON dump
  python mp_exidx.py at 0x006D8270       # containing function + disassembly
  python mp_exidx.py funcs 0x154b822     # (no-op eq) list funcs in a range
"""
import struct, sys, json, bisect, os

IMG = r"C:\anyWorkspace\nx\mhxx\extract\upd\image\main.bin"
EXIDX_START = 0x016E082C
EXIDX_END   = 0x01740944
TEXT_END    = 0x013E6718

def prel31(w):
    v = w & 0x7FFFFFFF
    return v - 0x80000000 if v & 0x40000000 else v

def load_map(img):
    starts = []
    p = EXIDX_START
    while p + 8 <= EXIDX_END:
        w0 = struct.unpack_from('<I', img, p)[0]
        t = p + prel31(w0)
        if 0 <= t < TEXT_END:
            starts.append(t)
        p += 8
    return sorted(set(starts))

def main():
    img = open(IMG, 'rb').read()
    starts = load_map(img)
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'map'
    if cmd == 'map':
        print("function starts: %d  first=%#x last=%#x" % (len(starts), starts[0], starts[-1]))
        out = os.path.join(os.path.dirname(os.path.abspath(__file__)), '_mp_funcs.json')
        json.dump([hex(s) for s in starts], open(out, 'w'))
        print("wrote", out)
    elif cmd == 'at':
        addr = int(sys.argv[2], 0)
        i = bisect.bisect_right(starts, addr) - 1
        st = starts[i]
        nxt = starts[i+1] if i+1 < len(starts) else TEXT_END
        print("address %#x is inside function %#x (size %#x, next %#x)" % (addr, st, nxt-st, nxt))
        from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM
        md = Cs(CS_ARCH_ARM, CS_MODE_ARM)
        for ins in md.disasm(img[st:st+0x300], st):
            mark = "   <<<<" if ins.address <= addr < ins.address+4 else ""
            print("  %08x  %-8s %s%s" % (ins.address, ins.mnemonic, ins.op_str, mark))
    elif cmd == 'funcs':
        lo, hi = int(sys.argv[2],0), int(sys.argv[3],0)
        sel = [s for s in starts if lo <= s < hi]
        print("functions in [%#x,%#x): %d" % (lo,hi,len(sel)))
        for s in sel: print("   %08x" % s)

if __name__ == '__main__':
    main()
