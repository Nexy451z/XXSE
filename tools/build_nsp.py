import hashlib
import os
import struct
import subprocess
import tempfile
from Crypto.Cipher import AES

REPO_ROOT = r"C:\anyWorkspace\nx\mhxx\mhnxeditor"
ELF_PATH = os.path.join(
    REPO_ROOT, r"switch\target\aarch64-nintendo-switch-libnx\release\mhnxeditor.elf"
)
ICON_PATH = os.path.join(REPO_ROOT, r"switch\icon.jpg")
OUT_NSP = os.path.join(REPO_ROOT, r"dist\mhnxeditor.nsp")
KEYS_PATH = r"C:\Users\haruk\AppData\Roaming\Ryujinx\system\prod.keys"

ELF2NSO = r"C:\devkitPro\tools\bin\elf2nso.exe"
NACPTOOL = r"C:\devkitPro\tools\bin\nacptool.exe"
BUILD_ROMFS = r"C:\devkitPro\tools\bin\build_romfs.exe"

TITLE_ID = 0x0105C3800049C000
TITLE_NAME = "MHXX Editor"
TITLE_AUTHOR = "mhnx project"
TITLE_VERSION = "1.1.0"


def load_keys(path: str) -> dict:
    out = {}
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            if "=" in line:
                k, v = line.strip().split("=", 1)
                out[k.strip()] = bytes.fromhex(v.strip())
    return out


def gf_mul_x(val: int) -> int:
    res = (val << 1) & ((1 << 128) - 1)
    if val & (1 << 127):
        res ^= 0x87
    return res


def aes_xts_encrypt_sector(key1: bytes, key2: bytes, sector_idx: int, data: bytes) -> bytes:
    assert len(data) == 0x200
    c1 = AES.new(key1, AES.MODE_ECB)
    c2 = AES.new(key2, AES.MODE_ECB)
    tweak_in = sector_idx.to_bytes(16, "big")
    tweak = int.from_bytes(c2.encrypt(tweak_in), "little")
    out = bytearray(0x200)
    for i in range(0, 0x200, 16):
        t_bytes = tweak.to_bytes(16, "little")
        blk = bytes(a ^ b for a, b in zip(data[i : i + 16], t_bytes))
        enc = c1.encrypt(blk)
        out[i : i + 16] = bytes(a ^ b for a, b in zip(enc, t_bytes))
        tweak = gf_mul_x(tweak)
    return bytes(out)


def aes_xts_encrypt_header(header_key: bytes, hdr: bytes) -> bytes:
    assert len(hdr) == 0xC00
    k1 = header_key[:16]
    k2 = header_key[16:32]
    out = bytearray()
    for s in range(6):
        out.extend(aes_xts_encrypt_sector(k1, k2, s, hdr[s * 0x200 : (s + 1) * 0x200]))
    return bytes(out)


def aes_ctr_crypt(key: bytes, file_offset: int, data: bytes) -> bytes:
    assert file_offset % 16 == 0
    init_ctr = file_offset >> 4
    cipher = AES.new(
        key,
        AES.MODE_CTR,
        nonce=b"\x00" * 8,
        initial_value=init_ctr,
    )
    return cipher.encrypt(data)


def align_up(v: int, a: int) -> int:
    return (v + a - 1) & ~(a - 1)


def pad_to(b: bytes, align: int) -> bytes:
    rem = len(b) % align
    if rem == 0:
        return b
    return b + b"\x00" * (align - rem)


def build_pfs0(files: list[tuple[str, bytes]]) -> bytes:
    cnt = len(files)
    str_tab = bytearray()
    str_offs = []
    for name, _ in files:
        str_offs.append(len(str_tab))
        str_tab.extend(name.encode("utf-8") + b"\x00")
    hdr_raw_len = 16 + cnt * 24 + len(str_tab)
    pad_len = align_up(hdr_raw_len, 0x20) - hdr_raw_len
    str_tab.extend(b"\x00" * pad_len)

    entries = bytearray()
    cur_off = 0
    for (_, data), noff in zip(files, str_offs):
        entries.extend(struct.pack("<QQII", cur_off, len(data), noff, 0))
        cur_off += len(data)

    out = bytearray()
    out.extend(b"PFS0")
    out.extend(struct.pack("<III", cnt, len(str_tab), 0))
    out.extend(entries)
    out.extend(str_tab)
    for _, data in files:
        out.extend(data)
    return bytes(out)


def build_pfs0_nca(
    content_type: int,
    title_id: int,
    pfs0_bytes: bytes,
    header_key: bytes,
    kaek_app_00: bytes,
    block_size: int = 0x1000,
) -> bytes:
    hash_tab = bytearray()
    for i in range(0, len(pfs0_bytes), block_size):
        chunk = pfs0_bytes[i : i + block_size]
        hash_tab.extend(hashlib.sha256(chunk).digest())
    master_hash = hashlib.sha256(hash_tab).digest()

    pfs0_offset = align_up(len(hash_tab), 0x200)
    sec_raw = bytearray(b"\x00" * pfs0_offset)
    sec_raw[: len(hash_tab)] = hash_tab
    sec_raw.extend(pfs0_bytes)
    sec_padded = pad_to(bytes(sec_raw), 0x200)

    sec_hdr = bytearray(0x200)
    struct.pack_into("<HBBBB", sec_hdr, 0x00, 2, 1, 2, 3, 0)  # v2, PFS0(1), HierSha256(2), AesCtr(3)
    sec_hdr[0x08:0x28] = master_hash
    struct.pack_into("<II", sec_hdr, 0x28, block_size, 2)
    struct.pack_into("<QQ", sec_hdr, 0x30, 0, len(hash_tab))
    struct.pack_into("<QQ", sec_hdr, 0x40, pfs0_offset, len(pfs0_bytes))
    sec_hdr_hash = hashlib.sha256(sec_hdr).digest()

    ctr_key = b"MHNXEDITORKEY123"
    ka_plain = bytearray(0x40)
    ka_plain[0x20:0x30] = ctr_key
    ka_enc = AES.new(kaek_app_00, AES.MODE_ECB).encrypt(bytes(ka_plain))

    total_size = 0xC00 + len(sec_padded)
    hdr = bytearray(0xC00)
    hdr[0x200:0x204] = b"NCA3"
    hdr[0x204] = 0  # dist
    hdr[0x205] = content_type
    hdr[0x206] = 0  # crypto type
    hdr[0x207] = 0  # kaek index (Application)
    struct.pack_into("<Q", hdr, 0x208, total_size)
    struct.pack_into("<Q", hdr, 0x210, title_id)
    struct.pack_into("<I", hdr, 0x218, 0)
    struct.pack_into("<I", hdr, 0x21C, 0x000B0000)
    struct.pack_into("<IIB", hdr, 0x240, 0xC00 // 0x200, total_size // 0x200, 1)
    hdr[0x280:0x2A0] = sec_hdr_hash
    hdr[0x300:0x340] = ka_enc
    hdr[0x400:0x600] = sec_hdr

    enc_hdr = aes_xts_encrypt_header(header_key, bytes(hdr))
    enc_sec = aes_ctr_crypt(ctr_key, 0xC00, sec_padded)
    return enc_hdr + enc_sec


def build_romfs_nca(
    content_type: int,
    title_id: int,
    romfs_bytes: bytes,
    header_key: bytes,
    kaek_app_00: bytes,
) -> bytes:
    block_log2 = 14
    block_size = 1 << block_log2  # 0x4000 (16 KB)

    levels_data = [None] * 6
    levels_size = [0] * 6
    levels_data[5] = pad_to(romfs_bytes, block_size)
    levels_size[5] = len(romfs_bytes)

    for lvl in range(4, -1, -1):
        child = levels_data[lvl + 1]
        hashes = bytearray()
        for off in range(0, len(child), block_size):
            hashes.extend(hashlib.sha256(child[off : off + block_size]).digest())
        levels_size[lvl] = len(hashes)
        levels_data[lvl] = pad_to(bytes(hashes), block_size)

    master_hash = hashlib.sha256(levels_data[0][:block_size]).digest()

    sec_body = bytearray()
    logical_offsets = []
    cur_off = 0
    for lvl in range(6):
        logical_offsets.append(cur_off)
        sec_body.extend(levels_data[lvl])
        cur_off += len(levels_data[lvl])
    sec_padded = pad_to(bytes(sec_body), 0x200)

    sec_hdr = bytearray(0x200)
    struct.pack_into("<HBBBB", sec_hdr, 0x00, 2, 0, 3, 3, 0)  # v2, RomFS(0), IVFC(3), AesCtr(3)
    sec_hdr[0x08:0x0C] = b"IVFC"
    struct.pack_into("<III", sec_hdr, 0x0C, 0x20000, 0x20, 7)
    for lvl in range(6):
        base = 0x18 + lvl * 24
        struct.pack_into("<QQII", sec_hdr, base, logical_offsets[lvl], levels_size[lvl], block_log2, 0)
    # 0x18 + 6*24 = 0xA8; 0xA8..0xC8 is 32-byte salt; 0xC8..0xE8 is 32-byte master_hash!
    sec_hdr[0xC8:0xE8] = master_hash
    sec_hdr_hash = hashlib.sha256(sec_hdr).digest()

    ctr_key = b"MHNXEDITORKEY123"
    ka_plain = bytearray(0x40)
    ka_plain[0x20:0x30] = ctr_key
    ka_enc = AES.new(kaek_app_00, AES.MODE_ECB).encrypt(bytes(ka_plain))

    total_size = 0xC00 + len(sec_padded)
    hdr = bytearray(0xC00)
    hdr[0x200:0x204] = b"NCA3"
    hdr[0x204] = 0
    hdr[0x205] = content_type
    hdr[0x206] = 0
    hdr[0x207] = 0
    struct.pack_into("<Q", hdr, 0x208, total_size)
    struct.pack_into("<Q", hdr, 0x210, title_id)
    struct.pack_into("<I", hdr, 0x218, 0)
    struct.pack_into("<I", hdr, 0x21C, 0x000B0000)
    struct.pack_into("<IIB", hdr, 0x240, 0xC00 // 0x200, total_size // 0x200, 1)
    hdr[0x280:0x2A0] = sec_hdr_hash
    hdr[0x300:0x340] = ka_enc
    hdr[0x400:0x600] = sec_hdr

    enc_hdr = aes_xts_encrypt_header(header_key, bytes(hdr))
    enc_sec = aes_ctr_crypt(ctr_key, 0xC00, sec_padded)
    return enc_hdr + enc_sec


def make_npdm_from_hbl(hbl_npdm: bytes, title_id: int) -> bytes:
    npdm = bytearray(hbl_npdm)
    aci0_off, aci0_sz, acid_off, acid_sz = struct.unpack_from("<IIII", npdm, 0x70)
    # 0x1C..0x20 is main_thread_stack_size (keep 0x100000!); 0x20..0x30 is title_name
    npdm[0x20:0x30] = b"Application\x00\x00\x00\x00\x00"
    # Set Title ID in ACI0 (+0x10) and ACID (+0x210..+0x220 min/max)
    struct.pack_into("<Q", npdm, aci0_off + 0x10, title_id)
    struct.pack_into("<QQ", npdm, acid_off + 0x210, title_id, title_id)
    assert struct.unpack_from("<I", npdm, 0x1C)[0] >= 0x100000
    return bytes(npdm)


def main():
    keys = load_keys(KEYS_PATH)
    header_key = keys["header_key"]
    kaek_app_00 = keys["key_area_key_application_00"]

    with tempfile.TemporaryDirectory() as tmp:
        # 1. Build main NSO from mhnxeditor.elf
        main_nso = os.path.join(tmp, "main")
        subprocess.run([ELF2NSO, ELF_PATH, main_nso], check=True)
        nso_bytes = open(main_nso, "rb").read()

        # 2. Fetch hbl.nsp's full-permission main.npdm template
        from ftplib import FTP
        import io

        ftp = FTP()
        ftp.connect("192.168.1.12", 5000, timeout=5)
        ftp.login()
        buf = io.BytesIO()
        ftp.retrbinary("RETR /atmosphere/hbl.nsp", buf.write)
        ftp.quit()
        hbl = buf.getvalue()
        cnt, st_sz = struct.unpack_from("<II", hbl, 4)
        hdr_end = 16 + cnt * 24 + st_sz
        hbl_npdm = None
        for i in range(cnt):
            off, sz, noff = struct.unpack_from("<QQI", hbl, 16 + i * 24)
            name = hbl[16 + cnt * 24 + noff :].split(b"\x00", 1)[0].decode()
            if name == "main.npdm":
                hbl_npdm = hbl[hdr_end + off : hdr_end + off + sz]
        assert hbl_npdm is not None
        npdm_bytes = make_npdm_from_hbl(hbl_npdm, TITLE_ID)

        exefs_pfs0 = build_pfs0([("main", nso_bytes), ("main.npdm", npdm_bytes)])
        prog_nca = build_pfs0_nca(0, TITLE_ID, exefs_pfs0, header_key, kaek_app_00, block_size=0x10000)
        prog_hash = hashlib.sha256(prog_nca).digest()
        prog_id = prog_hash[:16].hex()

        # 3. Build Control NCA (control.nacp + icon_*.dat)
        ctrl_dir = os.path.join(tmp, "control")
        os.makedirs(ctrl_dir, exist_ok=True)
        nacp_path = os.path.join(ctrl_dir, "control.nacp")
        subprocess.run(
            [NACPTOOL, "--create", TITLE_NAME, TITLE_AUTHOR, TITLE_VERSION, nacp_path],
            check=True,
        )
        nacp = bytearray(open(nacp_path, "rb").read())
        struct.pack_into("<Q", nacp, 0x3038, TITLE_ID)
        struct.pack_into("<Q", nacp, 0x3078, TITLE_ID)
        struct.pack_into("<Q", nacp, 0x30B0, TITLE_ID)
        open(nacp_path, "wb").write(nacp)

        icon_bytes = open(ICON_PATH, "rb").read()
        for lang in ("AmericanEnglish", "Japanese"):
            open(os.path.join(ctrl_dir, f"icon_{lang}.dat"), "wb").write(icon_bytes)

        ctrl_romfs_path = os.path.join(tmp, "control.romfs")
        subprocess.run([BUILD_ROMFS, ctrl_dir, ctrl_romfs_path], check=True)
        ctrl_romfs = open(ctrl_romfs_path, "rb").read()
        ctrl_nca = build_romfs_nca(2, TITLE_ID, ctrl_romfs, header_key, kaek_app_00)
        ctrl_hash = hashlib.sha256(ctrl_nca).digest()
        ctrl_id = ctrl_hash[:16].hex()

        # 4. Build Meta (.cnmt) NCA
        # Exact 32-byte PackagedContentMetaHeader (0x00..0x20)
        cnmt_hdr = struct.pack(
            "<QIBBHHHBBHII",
            TITLE_ID,  # 0x00..0x08: title_id
            0,         # 0x08..0x0C: version
            0x80,      # 0x0C: type (0x80 = Application)
            0,         # 0x0D: reserved
            16,        # 0x0E..0x10: extended_header_size (16)
            2,         # 0x10..0x12: content_count (2: Program + Control)
            0,         # 0x12..0x14: content_meta_count (0)
            0,         # 0x14: attributes
            0,         # 0x15: storage_id
            0,         # 0x16..0x18: content_install_type
            0,         # 0x18..0x1C: required_download_system_version
            0,         # 0x1C..0x20: reserved
        )
        assert len(cnmt_hdr) == 0x20, f"bad cnmt_hdr len {len(cnmt_hdr)}"

        # Exact 16-byte ApplicationExtendedHeader (0x20..0x30)
        cnmt_ext = struct.pack("<QII", TITLE_ID | 0x800, 0, 0)
        assert len(cnmt_ext) == 0x10

        # Content Entry 0 (0x30..0x68, 56 bytes): Program NCA (content_type = 1)
        prog_sz_6 = len(prog_nca).to_bytes(6, "little")
        entry_prog = prog_hash + bytes.fromhex(prog_id) + prog_sz_6 + bytes([1, 0])
        assert len(entry_prog) == 56

        # Content Entry 1 (0x68..0xA0, 56 bytes): Control NCA (content_type = 3)
        ctrl_sz_6 = len(ctrl_nca).to_bytes(6, "little")
        entry_ctrl = ctrl_hash + bytes.fromhex(ctrl_id) + ctrl_sz_6 + bytes([3, 0])
        assert len(entry_ctrl) == 56

        digest = b"\x00" * 32
        cnmt = cnmt_hdr + cnmt_ext + entry_prog + entry_ctrl + digest
        assert len(cnmt) == 0xC0
        assert cnmt[0x30 + 0x20 : 0x30 + 0x30].hex() == prog_id
        assert cnmt[0x68 + 0x20 : 0x68 + 0x30].hex() == ctrl_id

        cnmt_name = f"Application_{TITLE_ID:016x}.cnmt"
        cnmt_pfs0 = build_pfs0([(cnmt_name, cnmt)])
        meta_nca = build_pfs0_nca(1, TITLE_ID, cnmt_pfs0, header_key, kaek_app_00, block_size=0x1000)
        meta_hash = hashlib.sha256(meta_nca).digest()
        meta_id = meta_hash[:16].hex()

        # 5. Pack outer NSP (PFS0)
        nsp_bytes = build_pfs0(
            [
                (f"{prog_id}.nca", prog_nca),
                (f"{ctrl_id}.nca", ctrl_nca),
                (f"{meta_id}.cnmt.nca", meta_nca),
            ]
        )
        open(OUT_NSP, "wb").write(nsp_bytes)
        print(
            f"Built {OUT_NSP} ({len(nsp_bytes)} bytes) [TitleID: 0x{TITLE_ID:016X}]"
        )
        print(f"  Program NCA : {prog_id}.nca ({len(prog_nca)} B)")
        print(f"  Control NCA : {ctrl_id}.nca ({len(ctrl_nca)} B)")
        print(f"  Meta NCA    : {meta_id}.cnmt.nca ({len(meta_nca)} B)")


if __name__ == "__main__":
    main()
