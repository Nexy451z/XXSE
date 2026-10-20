# exlaunch による C コード注入で MHXX の「村クエスト読込」を観測・改変できるか

対象: `out_update/exefs/main` (14,407,198 B) / `out_update/image/main.bin` (25,604,158 B) /
`extract/base_romfs/nativeNX/arc/quest/` (1,859 ファイル)
作業: 2026-10-20 / 担当: 実現性調査（exlaunch サブエージェント）
関連: `docs/review_questloader.md`, `docs/quest_format_spec.md`, `docs/mp_reverse_engineering.md`,
`docs/cheat_port_3ds_to_switch.md`, `docs/switch_build.md`

> 表記: **【事実】** = 本調査で実測・原典で確認 / **【推定】** = 実測からの推論 / **【未確定】** = 判断材料不足
> アドレスは既存 docs と同じ「VA」規約（= `image/main.bin` 上のオフセット + 0x100）。
> この規約が「モジュール先頭からのオフセット」と一致することは §2.5 で実測確認した。

---

## 0. 結論

### 0.1 一言で

**exlaunch は MHXX では動かない。** 理由は「アーキテクチャ非対応」という 1 点に集約され、
ビルド設定の工夫やローダ側の細工では回避できない。**MHXX の `main` は AArch32 (32bit ARM) で、
exlaunch のフック実装は AArch64 専用**である（§1.5・§2.1）。

したがって本レポートの結論は「exlaunch を使わない 3 つの代替手段のどれを選ぶか」に帰着する（§8）。
一方、**依頼の 3 番「村クエストが個別 .arc か集約 .arc か」は、注入なしで決着した**（§4）。
**ゲームプレイ用データは個別 .arc が本体、集約 .arc は一覧表示用のカタログ**である。

### 0.2 依頼 6 項目への回答

| # | 依頼項目 | 結論 | 根拠 |
|---|---|---|---|
| 1 | exlaunch の仕組み / MHXX への適用 | **適用不可**。仕組みは理解でき、手順も書けるが、32bit 非対応で成立しない | §1（公式リポジトリ実測 + issue #24 の maintainer 回答） |
| 2 | フックすべき関数の候補 | 状態機械 0x37A6C / 枠探索 0x389A0・0x38A44 / Fix 0x38A74〜0x38AB4 は**再実測で確定**。**集約 .arc を開く箇所は静的には特定できない**（文字列 xref が 5 手法で 0 件） | §3 |
| 3 | 個別 .arc か集約 .arc か | **両方使われる。一覧＝集約、実プレイ＝個別**。集約には `.rem`/`.sup`/`.sem`/`.esl`/`.qdl` が 1 つも入っていないため、集約だけではクエストを開始できない（論理的必然） | §4.1（データ構造）+ §4.2（RAM 実測） |
| 4 | 注入でできることの範囲 | 読み取りログ / 値の差し替え / 関数置換の 3 段階に整理。**MHXX の `main` には code cave が無い**（text 内の最長ゼロ連続 = 10 B）ことが実測で判明し、「関数の完全置換」は現実的でない | §5 |
| 5 | 実機の前提 | CFW は Atmosphère 1.x 系 + 任意のペイロード。TID `0100C3800049C000`（本体）/ `0100C3800049C800`（更新）。**本体を壊さない手順は「SD カード上のファイルだけを触る」に尽きる** | §6 |
| 6 | 既存 cheats / existing_mod の流用 | `cheats/9d4c…txt` は**そのまま流用可**。`existing_mod/subsdk9.bin` は **AArch64 ビルドなので流用不可**（理由 3 点） | §7 |

### 0.3 「観測・改変」の可否だけを先に表にする

| やりたいこと | exlaunch | Atmosphère チート (.txt) | exefs_patches (IPS32) | LayeredFS (romfs) |
|---|---|---|---|---|
| どのファイルを開いたか記録する | **不可**（32bit） | 不可（記録機能なし。RAM ダンプで代替可） | 可（A32 アセンブラで書く必要あり） | 不可（差分実験での推定は可） |
| 状態機械の state を読む | 不可 | **可** | 可 | — |
| 値を差し替える | 不可 | **可** | **可** | 不可（データのみ） |
| 関数を丸ごと置き換える | 不可 | 不可（code cave が無い） | 条件付きで可 | 不可 |
| クエストの .arc を差し替える | — | — | — | **可（最も安全）** |

> 「不可」は「その手段では」という限定付き。§8 に代替案の組み合わせを示す。

---

## 1. exlaunch の仕組み

### 1.1 何をするものか【事実】

公式: <https://github.com/shadowninja108/exlaunch>（README: "A framework for injecting C/C++ code into
Nintendo Switch applications/applet/sysmodules."、ライセンスはリポジトリ同梱の `LICENSE`）。

構成（`git/trees/main?recursive=1` の実測、273 エントリ）:

| パス | 役割 |
|---|---|
| `source/lib/hook/` | フック API 本体。実装は **`nx64/` サブディレクトリのみ** |
| `source/lib/armv8/instructions/` | AArch64 命令エンコーダ（B/BL/LDR/ADD/MOVZ…） |
| `source/lib/init/crt0.s`, `header.s` | 起動コード（AArch64 アセンブリ） |
| `source/lib/reloc/`, `source/rtld/` | 自前の rtld（oss-rtld 由来）。モジュール解決 |
| `source/lib/util/sys/` | `svcQueryMemory` でモジュールを探す処理（`mem_layout.cpp`） |
| `source/nn/` | nn::os / nn::fs の最小ヘッダ |
| `source/program/main.cpp` | 利用者が書くエントリポイント（`exl_main`） |
| `misc/mk/`, `Makefile`, `config.mk` | devkitPro/libnx ベースのビルド |

### 1.2 Atmosphère への組み込み方【事実】

**方式は「exefs の NSO 差し替え（または追加）」**。ビルド成果物を SD カードに置くと、
Atmosphère の `loader` がプロセス生成時に SD 側のファイルを優先して使う。

`Makefile` の実測（原文）:

```make
SD_OUT := atmosphere/contents/$(PROGRAM_ID)/exefs
ifeq ($(LOAD_KIND), Module)
    BINARY_NAME := subsdk9 # TODO: support subsdkX?
```

つまり既定では `/atmosphere/contents/<PROGRAM_ID>/exefs/subsdk9` に置く。`config.mk` で

```make
LOAD_KIND  := Module                 # Module → subsdk9 / AsRtld → rtld
PROGRAM_ID := 0100801011c3e000        # ここを対象タイトルに変える
NPDM_JSON  := qlaunch.json            # misc/npdm-json/{application,qlaunch}.json
```

Atmosphère 側の受け入れ仕様（公式ドキュメント
<https://github.com/Atmosphere-NX/Atmosphere/blob/master/docs/components/modules/loader.md> より引用）:

> - プロセス生成時、loader は `rtld`, `main`, `subsdk0`, `subsdk1`, …, `subsdk9`, `sdk` の順に NSO 名を探す。
> - 見つかった NSO は**プロセスに連続してロードされる**。エントリポイントは最初にロードされた NSO（通常 `rtld` か `main`）。
> - さらに loader は exefs ディレクトリの `main.npdm` を探し、プログラムの権限記述子として使う。
> - **SD 側を先に探し、あればそれを使う。** ディレクトリは `/atmosphere/contents/<program id>/exefs/`。
> - `<name>.stub` を置くと、その NSO はロードされない。

重要な帰結:

* **もともと存在しない `subsdk9` を「追加」してもロードされる**（上記の探索は SD 側にも及ぶため）。
  exlaunch が既定で `subsdk9` を使うのはこの性質を利用している。
* パスの `titles` → `contents` の変更は **Atmosphère 0.10.0**（`docs/changelog.md` に
  "Atmosphere now uses the /atmosphere/contents directory, instead of /atmosphere/titles." と明記。
  同バージョンで起動時に自動リネームも行う）。0.9 以前向けの記事のパスは読まないこと。

### 1.3 フック API（実名）【事実】

`source/lib/hook/base.hpp` / `trampoline.hpp` / `replace.hpp` の実測:

```cpp
// 入口
extern "C" void exl_main(void* x0, void* x1) {
    exl::hook::Initialize();                       // フック環境の初期化
    MyHook::InstallAtOffset(0x37A6C);              // main の先頭からのオフセット
}

// 元の関数を呼べるフック
HOOK_DEFINE_TRAMPOLINE(MyHook) {
    static void Callback(/* 元と同じ引数型 */) { Orig(/* 引数 */); }
};

// 元を呼ばない置換
HOOK_DEFINE_REPLACE(MyHook2) {
    static bool Callback(/* … */) { return true; }
};
```

| 名前 | 所在 | 意味 |
|---|---|---|
| `exl::hook::Initialize()` | base.hpp | `arch::Initialize()`（nx64）を呼ぶ |
| `exl::hook::Hook(hook, callback, do_trampoline)` | base.hpp | 生ポインタ版。戻り値は trampoline |
| `HookInline(uintptr_t, InlineCallback)` | base.hpp | 命令 1 個単位の inline フック（`InlineCtx`） |
| `InstallAtOffset(ptrdiff_t)` | trampoline.hpp / replace.hpp | **`util::modules::GetTargetStart() + offset`** にフック |
| `InstallAtPtr(uintptr_t)` / `InstallAtFuncPtr(T)` | 同上 | 絶対アドレス / 関数ポインタ指定 |
| `util::modules::GetTargetStart()` | util/modules.hpp | `GetMainModuleInfo().m_Total.m_Start`（**`main` の先頭**） |
| `exl::hook::CallTargetEntrypoint(x0,x1)` | base.hpp | sysmodule/applet 用。元のエントリを呼ぶ |
| `namespace arch = nx64;` | base.hpp | **アーキ実装の切替点。ここが nx64 固定**（§1.5） |

`InlineCtx` / `InlineFloatCtx` でレジスタ・浮動小数点レジスタに触れる（`HookInline` の第 2 引数の型で判別）。

### 1.4 ビルドに必要なもの【事実】

`misc/mk/common.mk` の実測:

```make
ifeq ($(strip $(DEVKITPRO)),)
$(error "Please set DEVKITPRO in your environment. export DEVKITPRO=<path to>/devkitpro")
endif
include $(DEVKITPRO)/libnx/switch_rules
...
ARCH := -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIC -fvisibility=hidden
CXXFLAGS := … -fno-rtti -fno-exceptions -std=gnu++23
LDFLAGS  := -specs=$(SPECS_PATH)/$(SPECS_NAME) … -nostartfiles
```

| 必要なもの | 用途 | 備考 |
|---|---|---|
| devkitPro（devkitA64）+ libnx | コンパイラ / `switch_rules` / ヘッダ | `DEVKITPRO` 環境変数が必須 |
| Python 3.4+ | `make-npdm-json.py`（NPDM 生成） | `config.mk` の `PYTHON` |
| `npdmtool`（devkitPro 同梱） | `main.npdm` 生成 | `misc/npdm-json/application.json` の `is_64_bit: true` を使う |
| `misc/specs/module.specs` | リンカスクリプト | 出力は `deploy/` 以下 |
| 実機 or エミュレータ | 動作確認 | `deploy-sd` / `deploy-ftp` / `deploy-ryu` |

成果物は `LOAD_KIND=Module` なら **NSO 1 個**（`subsdk9`）。`main.npdm` を併置するのが慣例
（本ワークスペースの `existing_mod/` も `main.npdm` + `subsdk9` の 2 ファイル構成になっている）。

### 1.5 **致命的制約: 32bit 非対応**【事実】

証拠 4 点（すべて一次情報）:

| # | 証拠 | 出典 |
|---|---|---|
| 1 | `source/lib/hook/base.hpp` に **`/* TODO: 32-bit. */`** というコメント付きで `namespace arch = nx64;` が固定されている | リポジトリ実測 |
| 2 | フック実装ディレクトリが `source/lib/hook/nx64/` のみ（`nx32` は存在しない）。命令エンコーダも `source/lib/armv8/` のみ | `git/trees/main?recursive=1` |
| 3 | ビルドが `-march=armv8-a+crc+crypto`、NPDM テンプレートが `"is_64_bit": true` | `misc/mk/common.mk` / `misc/npdm-json/application.json` |
| 4 | Issue **#24 "Support for 32 bit games? (Like mk8d)"**（open, 2024-11-13）に対する作者 shadowninja108 の回答（2024-11-14）: **"Not at the moment, no. The main barrier to 32-bit support would be rewriting the hooking library and core kernel svc stuff."** | <https://github.com/shadowninja108/exlaunch/issues/24> |

**⇒ MHXX に exlaunch を適用する道は、現時点で存在しない。** これは「設定を変えれば済む」話ではなく、
フック実装（分岐命令の書き込み・トランポリン・レジスタ退避）を A32 用に書き直す必要がある。

---

## 2. MHXX 側の実測（前提の確定）

### 2.1 AArch32 であること【事実・再実測】

| 検証 | 実測値 |
|---|---|
| `out_update/image/main.bin` の A32 prologue（`push {…lr}` = `(w>>16)==0xE92D`） | **46,056 個** |
| 同 A64 `ret` (`0xD65F03C0`) | **0 個** |
| `out_base/image/main.bin` | A32 prologue 45,935 / A64 ret **0** |
| `out_base/exefs` の全 8 モジュール（main, rtld, sdk, subsdk0-4） | A64 `ret` **すべて 0** |
| `out_update/exefs` の全 4 モジュール | 同上 **0** |

先行レポート `mp_reverse_engineering.md` §3 も独立に
「NPDM の ACI0 flags = 0x00000000 (is64bit=0)、ARM32 プロローグ 11,721 個、ARM64 0 個」と報告しており整合する。
**MHXX の exefs はモジュール 1 個残らず AArch32** である。

> 参考: このゲームのアドレス空間には `0x40000000` を基点とする領域がある
> （`cheats/9d4c86e6ef74504a.txt` の所持金 `0x42CA1DC4`、コード中の `add r?, r?, #0x40000000`、
> および RAM ダンプで `arc\quest\vq` が `0x43C9F908` に出たこと）。
> ただし `0x42CA1DC4` はモジュール像 + bss の外側（`0x40000000` 起点で 0x2CA1DC4）なので、
> **静的な `.data` ではなくヒープ側**と考えられる【推定】。

### 2.2 exefs 構成 — **`subsdk9` が存在しない**【事実】

| 版 | exefs の中身 |
|---|---|
| base（v0） | `main`(14,770,121) `main.npdm`(1,476) `rtld`(12,840) `sdk` `subsdk0` `subsdk1` `subsdk2` `subsdk3` `subsdk4` |
| update（v1.5.1） | `main`(14,407,198) `main.npdm`(1,476) `rtld`(7,004) `sdk` `subsdk0` |

**`subsdk9` は無い。** exlaunch の既定（`LOAD_KIND := Module` → `subsdk9`）は
「SD 側に置けば追加ロードされる」という loader の仕様で成立するが、
**追加されるのは 64bit NSO であり、32bit プロセスには載らない**（§7.2）。

### 2.3 タイトル ID・ビルド ID・チートファイル名【事実】

| 項目 | 値 | 出典 |
|---|---|---|
| 本体タイトル ID | `0100C3800049C000` | `rom/` の NSP ファイル名 |
| 更新タイトル ID | `0100C3800049C800`（v393216 = 1.5.1） | 同上 |
| 更新 `main` の build id | `9d4c86e6ef74504afa3a485c9797653b0…` | `out_update/exefs/main` の 0x40 から 0x20 B |
| チートファイル名 | `9d4c86e6ef74504a.txt`（先頭 8 バイト） | `cheats/` の実ファイル名と一致 |
| base `main` の build id | `8b338f57a04a2688bc63ad1685d7122b0…` | `out_base/exefs/main` |

`out_update/manifest.json` がこの対応（`buildid` / `cheat_filename` / セグメントの memoff・size）を
全モジュール分すでに記録している。**以後の作業はこの manifest を正とするとよい。**

更新 `main` のセグメント（manifest より）:

| セグメント | memoff | size |
|---|---|---|
| text | 0 | 20,866,584 (`0x13E6618`) |
| rodata | 20,869,120 (`0x13E7000`) | 3,575,922 (`0x369072`) |
| data | 24,449,024 (`0x1751000`) | 1,161,652 (`0x11B9B4`) |
| bss | 25,610,676 | 9,422,412 |

> `image/main.bin` は **text ++ rodata ++ data を隙間なく詰めたもの**
> （20,866,584 + 3,575,922 + 1,161,652 = 25,604,158 = ファイルサイズと厳密一致）。
> メモリ上はセグメントがページ境界に置かれるので、**rodata 以降は image オフセットと VA がずれる**。
> 本レポートの VA は「image オフセット + 0x100」で統一している（§2.5 で妥当性を確認）。

### 2.4 ワークスペースの既存 exlaunch 成果物【事実】

| ファイル | サイズ | 判定 |
|---|---|---|
| `existing_mod/subsdk9.bin` | 1,573,728 | **AArch64 の exlaunch ペイロード**（先頭に `~~exlaunch uwu~~`、A64 `ret` 3,367 個 / A32 prologue 1 個） |
| `mods_disabled/exefs_MHNX_mod/subsdk9` | 987,302 | 上記の NSO 版。**ヘッダと実体が不整合**（text を `0x100` から `0x128E70` B と宣言するがファイルは `0xF10A6` B）＝**破損または切り詰め** |
| `mods_disabled/exefs_MHNX_mod/main.npdm` | 1,516 | 権限記述子（本体の 1,476 B より 40 B 大きい） |
| `existing_mod/subsdk9_strings.txt` | 124,646 | 上記ペイロードの strings ダンプ（可読文字列は `~~exlaunch uwu~~` の 1 本のみ） |

先頭 32 バイトの実測（`existing_mod/subsdk9.bin`）:

```
08 00 00 14   b   #0x20            <- A64 分岐でヘッダを飛ばす
10 90 12 00   0x00129010
2c 90 12 00   0x0012902c
1f 20 03 d5   nop                 <- A64
"~~exlaunch uwu~~"                <- 0x0C..0x1B
1f 00 00 f1   cmp x0, #0         <- A64
24 18 41 ba   tbz w4, #8, …       <- A64
```

**⇒ これは 64bit 用ビルドである。32bit プロセスには絶対に載らない。**
`mods_disabled` というディレクトリ名は、この試行が失敗して無効化されたことを示している【推定】。

### 2.5 VA 規約の確認【事実】

コード中のリテラルが「文字列の位置 + 0x100」を指すことを実測した:

```
image offset 0x2D102C の語 = 0x01589C18
"nativeLite\arc\" の実位置 = image offset 0x1589B18  (= 0x01589C18 - 0x100)
```

同様に `0x01589C18` は 1 件だけ存在し、他に候補は無い。よって
**「コード内の即値 = image オフセット + 0x100」** が成立し、
既存 docs の VA 表記はそのまま「`main` 先頭からのオフセット」として使える。

**独立した裏付け**（§6.2.1 で引用した Atmosphère の実装）: ローダは NSO の
**ファイルオフセットから 0x100 を引いて**モジュールに書き込む
（`patch_offset -= offset;  // offset = sizeof(NsoHeader)`）。
つまり「`main` 先頭からのオフセット」は「非圧縮 NSO のファイルオフセット − 0x100」であり、
本レポートの VA と同じ定義になる。**両者が一致したので、以下の VA は数値の読み替え無しで使える。**

> 実機/エミュレータで最初に確認すべきこと: `base + 0x37A6C` の 4 バイトが `F0 4F 2D E9`
> （= `push {r4-r11,lr}`）であること。ここが違えばベースかオフセットがずれている。

---

## 3. フックすべき関数の候補と根拠

### 3.1 クエスト読込の状態機械 — VA 0x37A6C【事実・再実測】

`out_update/image/main.bin` から読み直した実測値（先行レポートと一致）:

```
0037A6C  e92d4ff0   push {r4-r11,lr}
0037A70  e24dd03c   sub  sp, sp, #0x3C
0037A74  e24ddb02   sub  sp, sp, #0x800
0037A78  e1a0a000   mov  sl, r0                 ; sl = this
0037A7C  ebffdac9   bl   #0x2E5A8
0037A80  e59a0df4   ldr  r0, [sl, #0xDF4]       ; r0 = state
0037A84  e2401001   sub  r1, r0, #1
0037A88  e3510012   cmp  r1, #0x12              ; state-1 <= 18 ?
0037A8C  8a0005c1   bhi  #0x39198
0037A90  e1a01101   mov  r1, r1, lsl #2
0037A94  e28f2014   add  r2, pc, #0x14          ; r2 = 0x37AB0
0037A98  e306bb44   movw r11, #0x6B44
0037A9C  e30f5d8d   movw r5, #0xFD8D
0037AA0  e7911002   ldr  r1, [r1, r2]           ; ジャンプテーブル
0037AA4  e340b00f   movt r11, #0xF              ; r11 = 0x000F6B44
0037AA8  e34f5fff   movt r5, #0xFFFF
0037AAC  e081f002   add  pc, r1, r2             ; switch(state)
0037AB0  000000a8 / 000000b0 / 000000fc …      ; 相対オフセット表
```

| 項目 | 実測 |
|---|---|
| 状態数 | 19（`cmp r1,#0x12` = state 1..19） |
| state のフィールド | `this + 0xDF4` |
| 1 回の呼び出しで進む状態 | **1 個**（多段・非同期。1 フレームで完結しない） |
| 既知フィールド | `+0xDFC` ループ上限 / `+0xEA8`,`+0xEB4` コンテナ / `+0xFAC` 45 枠 / `+0xFA8` 160 枠 / `+0xFB0` 装備枠 |

**フック価値**: 高い。ただし「state が進んだ」ことしか分からない（§3.5）。

### 3.2 クエスト枠の探索 — VA 0x389A0（45 枠）/ 0x38A44（160 枠）【事実・再実測】

```
; 配列 A（this+0xFAC、45 件）
0038994  e5900fac   ldr  r0, [r0, #0xFAC]
0038998  e3500000   cmp  r0, #0
003899C  0a000056   beq  #0x38AFC
00389A0  e2806004   add  r6, r0, #4        ; r6 = &slot[1] (size)
00389A4  e3a03000   mov  r3, #0
00389A8  e5164004   ldr  r4, [r6, #-4]      ; slot[0] = id
00389AC  e1540002   cmp  r4, r2            ; 探しているキー
00389B0  1a000002   bne  #0x389C0
00389B4  e5964000   ldr  r4, [r6]          ; slot[1] = size
00389B8  e3540000   cmp  r4, #0
00389BC  1a00004d   bne  #0x38AF8         ; id 一致 かつ size != 0 -> 発見
00389C0  e2833001   add  r3, r3, #1
00389C4  e2866b05   add  r6, r6, #0x1400    ; stride 0x1400
00389C8  e353002d   cmp  r3, #0x2D         ; 45 件
00389CC  3afffff5   blo  #0x389A8
; 配列 B（this+0xFA8、160 件）は 0x38A44 から同型、終端は cmp r3,#0xA0 (160)
```

**フック価値**: 高い。ここは「どの ID を、どのスロットから読もうとしたか」を
**引数（r2 = キー）と戻り値（r0 = slot ポインタ or 0）で直接観測できる唯一の地点**。

### 3.3 ロード時の Fix 処理 — VA 0x38A74〜0x38AB4【事実・再実測】

```
0038A74  e0811101   add  r1, r1, r1, lsl #2   ; idx*5
0038A78  e0804501   add  r4, r0, r1, lsl #10  ; + idx*0x1400
0038A80  add  sb, r4, #0x1300
0038A88  bl   #0x13E46E4                     ; memset(slot+0x1300, 0, 0x100)
0038A90  ldr  r5, [r6, #4]!                  ; r5 = size
0038A94  cmp  r5, #0x100
0038A98  bls  #0x38AB8                       ; size <= 0x100 なら何もしない
0038A9C  add  r0, r4, r5                     ; slot + size
0038AA4  sub  r1, r0, #0xF8                  ; ペイロード末尾 - 0x100
0038AAC  bl   #0x13E4870                     ; memcpy(slot+0x1300, src, 0x100)
0038AB4  str  r0, [r6]                       ; size -= 0x100  ← セーブを書き換える
```

**改造時の注意**: この経路に入ると **セーブ上の size が 0x100 減る**。
size を「.arc の実サイズ」以外にした改造は避ける（先行レポート §3.5 の警告を再確認）。

### 3.4 集約アーカイブを開く箇所 — **静的には特定できない**【事実】

依頼項目にある「文字列 `quest\vq` 等の参照」を、5 手法で再測定した。

**まず文字列の実位置を確定**（先行レポートの `quest\q%07d = 0x154B1F6` は誤りで、
正しくは `0x154B0F6`。0x100 ずれ）。実測した文字列配置（すべて `out_update/image/main.bin`）:

```
0x154B071  sFestaFileLoader::cArchiveCase
0x154B090  sFestaFileLoader::cBuuferArray< sFestaFileLoader::cArchiveCase >
0x154B0D1  sFestaFileLoader
0x154B0E2  none
0x154B0E7  resident
0x154B0F0  quest
0x154B0F6  quest\q%07d
0x154B102  quest\gq
0x154B10B  quest\vq
0x154B114  quest\gq2
0x154B11E  quest\tq
0x154B127  result
0x154B12E  title
0x154B134  tutorial\common
0x154B144  village\hunter_note
0x154B158  facility\blacksmith
0x154B16C  facility\shoptrade
```

**`sFestaFileLoader` / `cArchiveCase` の直後に、`quest` + `q%07d` + `gq` + `vq` + `gq2` + `tq` が
連続して並ぶ**。これは「アーカイブ種別（case）→ 実ファイル名」の表である可能性が高い【推定】。
つまり **集約 4 本と個別 `q%07d` は同じローダ（`sFestaFileLoader`）の同じ case 群が扱う**。

にもかかわらず、**これらの VA を指す静的参照は 0 件**だった:

| 探索手法 | 結果 |
|---|---|
| ① 4 バイト境界の全語走査（VA 完全一致） | **0 件** |
| ② 全バイトオフセット走査（アライン無視） | `resident` に 2 件ヒットしたが、いずれも命令のバイト列と偶然一致（`movw` の即値）で**参照ではない** |
| ③ `movw`/`movt` の即値ペア | **0 件** |
| ④ `add/sub rX, pc, #imm` | **0 件** |
| ⑤ `ldr rX, [pc, #imm]` | **0 件** |
| （参考）`0x154B000`〜`0x154B400` を指す語の全走査 | **0 件** |

一方、同じ手法で `nativeLite\arc\`（0x1589C18）は **1 件**見つかる（`0x2D102C` のリテラル）。
**⇒ クエスト系の名前だけが「ポインタで参照されていない」**のは事実で、
ビルド時に生成された名前テーブル（ハッシュ + オフセット）経由で復元されていると考えるのが自然【推定】。

**では呼び出し元をどう見つけるか — 実行時メモリで見つかった**【事実】。

ワークスペースのゲスト RAM ダンプ（`scan/*.bin`、4 領域 496 MiB）を走査した結果:

```
base_130720.bin / now_132922.bin / cur_133813.bin / after6480_133516.bin  （4/4 すべて）
  0x43C9F908  "arc\quest\vq"                    <- ★ 実行時に生成された名前
  0x43C9F9D8  "quest\questData\questData_0000103_jpn"   <- vq.arc のエントリ名
  0x404D01C3  "app:/nativeNX/arc/facility/blacksmith.arc" <- ★ FS パスの実例

  0 件: "arc\quest\gq" / "arc\quest\gq2" / "arc\quest\tq" / "arc\quest\q<id>"
```

ここから確定できること:

1. ゲームは **`arc\` + `quest\vq` を連結してリソース名を作り、`app:/nativeNX/arc/quest/vq.arc` として開く**。
   （`arc\` は 0x154646B、`.arc` は 0x1546470 に実在。パス根 `nativeNX` は 0x1589D88）
2. その名前は **静的ポインタではなく実行時の連結**で作られる。だから xref が 0 件なのは矛盾しない。
3. 4 つのダンプはいずれも「`vq` だけ」で、`gq`/`gq2`/`tq`/`q<id>` は出ていない
   → ダンプを取った場面は「村のクエストボード周辺」だと推定できる【推定】。

**⇒ 集約 .arc を開く箇所を静的に指すことはできない。指すべきは次のどちらか:**

| 候補 | 根拠 | 実装難度 |
|---|---|---|
| (a) リソース名の連結（`arc\` + 名前）を行っている関数 | 実行時メモリに `arc\quest\vq` が現れる | 中（`sprintf` 系の呼び出しを掴む） |
| (b) FS オープン（`app:/nativeNX/…` を渡す関数） | `app:/nativeNX/arc/facility/blacksmith.arc` が実在 | 中（`nn::fs` の IPC は SDK 静的リンクなので、SDK 内の関数を掴む必要がある） |
| (c) **メモリ差分観測**（フック不要） | 上記 1〜3 をそのまま再現できる | **低**（§4.3） |

### 3.5 state 遷移をログすれば「どのファイルをどの順で読むか」が分かるか

**答え: 部分的にしか分からない**【推定（実測に基づく）】。

| 分かること | 分からないこと |
|---|---|
| state が 1→19 のどれを通ったか | **どのファイルを開いたか**（state はファイル名を持たない） |
| どの ID を探したか（0x389A0 の r2） | その ID が集約側から来たのか個別側から来たのか |
| スロットが見つかったか（size != 0） | .arc のパースに成功したか |
| Fix 処理（size −= 0x100）が走ったか | — |

理由: 状態機械は `this+0xDF4` を書き換えるだけで、ファイル名は別レイヤ（リソース名 → FS）にある。
また 19 状態は**数フレームで走り切る**ため、チートによる定期サンプリングでは取りこぼす
（1 フレーム 1 状態とは限らない）。

**file 順序を知りたいなら、状態機械ではなく「名前生成」か「FS オープン」を狙う**のが正解。

### 3.6 フック候補の優先順位（もし A32 で注入できるなら）

| 優先 | VA | 対象 | 得られる情報 | 備考 |
|---|---|---|---|---|
| 1 | 0x389A0 / 0x38A44 | 枠探索 | 探索キー ID・ヒット slot・どちらの配列か | 引数/戻り値が素直 |
| 2 | 0x37A6C | 状態機械 | state 遷移の系列 | 呼び出しが高頻度 |
| 3 | 0x38A74〜0x38AB4 | Fix 処理 | size 書き換えの発生 | **副作用に注意** |
| 4 | 名前連結関数（未特定） | リソース名生成 | **開いたファイル名そのもの** | 特定できれば最良 |
| 5 | FS オープン（未特定） | 実ファイル | パス文字列 + 成否 | SDK 静的リンクのため特定難 |
| 6 | 0x3B58C / 0x3B8C4 | コンテナ検索 → 詰め替え | どのフィールドが使われるか | 先行レポート §3.7 |

---

## 4. 村クエストのデータは個別 .arc から来るのか、集約 .arc から来るのか

### 4.1 構造からの証明（決定的）【事実】

まず実測:

| アーカイブ | サイズ | エントリ | 内訳 | クエスト ID | 範囲 |
|---|---|---|---|---|---|
| `vq.arc` | 325,588 | 976 | **`.mib` 488 + `.gmd` 488 のみ** | 488 | 0000101〜0130308 |
| `gq.arc` | 395,156 | 1,168 | **`.mib` 584 + `.gmd` 584 のみ** | 584 | 0010101〜0120006 |
| `gq2.arc` | 183,562 | 460 | **`.mib` 230 + `.gmd` 230 のみ** | 230 | 0011101〜0111405 |
| `tq.arc` | 113,372 | 602 | **`.mib` 301 + `.gmd` 301 のみ** | 301 | 0090000〜0090300 |
| 合計 | — | 3,206 | — | **1,603（重複なし）** | — |

個別アーカイブ:

* `quest/` 配下の `q<7桁>.arc` は **1,855 本**（+ 集約 4 本 = 1,859 エントリ）。
* 個別 .arc の典型は **12 エントリ**（実例 `q0000101.arc`）:
  `b_m00em000_00.sem` / `z_m01d_034.esl` / `z_m00d_000.esl` / `rem_210052.rem` / `rem_210001.rem` /
  `rem_000000.rem` / `rem_210051.rem` / `supp_0000101.sup` / `questPlus_0000.qdp` /
  `questLink_0000101.qdl` / `questData_0000101_jpn.gmd` / `questData_0000101.mib`

**突き合わせ（1,603 ID 全数）**:

| 検証 | 結果 |
|---|---|
| 集約に載っている ID が個別 `q<id>.arc` として存在するか | **1,603 / 1,603 存在** |
| 集約の `.mib` と個別 `.arc` 内 `.mib` のバイト一致 | **1,603 / 1,603 一致** |
| 集約の `.gmd` と個別 `.arc` 内 `.gmd` のバイト一致 | **1,603 / 1,603 一致** |

**⇒ 集約 4 本は、個別 .arc が持つ `questData`（249 B）と `questData_jpn`（gmd）を
そのまま重複収録しただけの「カタログ」である。集約側だけの情報は 1 バイトも無い。**

そして**論理的必然**:

> クエストの報酬表（`.rem`）・支給品（`.sup`）・ボス配置（`.sem`）・雑魚出現（`.esl`）・
> 参照リンク（`.qdl`）・クエストプラス（`.qdp`）は **個別 .arc にしか存在しない**
> （集約 3,206 エントリのうち 0 個）。
> ゲームはクエスト開始時にこれらを必要とする（報酬画面・支給箱・モンスター配置は実装されている）。
> **したがって「村クエストを実際に受注して開始する」経路では、個別 `q<id>.arc` が必ず読まれる。**

**⇒ 結論: 一覧表示＝集約、実プレイ＝個別。**（受注開始時に個別が読まれることは構造上確定）

### 4.2 実行時メモリの実測【事実】

§3.4 のとおり、4 ダンプすべてで `arc\quest\vq` がヒットし、`arc\quest\q<id>` は 0 件。
これは「クエストボード表示中は集約だけが開かれている」ことの直接証拠になる。

**未観測**: 個別 `q<id>.arc` の名前は、どのダンプにも現れなかった。
ダンプは 4 本とも同じ画面（村のクエストボード周辺）で取得されているため【推定】、
「受注→開始」の瞬間を捉えていない。

### 4.3 決定実験（注入なし・LayeredFS 差し替え）【手順】

これが本件の**最も安全で確実な判定法**。所要 15〜30 分/回。

**準備**

1. `base_romfs/nativeNX/arc/quest/q0000101.arc` をコピーし、`.gmd` の `title` 文字列だけを
   **同じバイト長**の別文字列に置換する（長さを変えると .arc の再構築が必要）。
   編集には `mhnx-core` の `Arc::parse` → `Gmd::from_bytes`/`to_bytes` → `Arc::to_bytes_with_level` が
   そのまま使える（`crates/mhnx-core/src/arc.rs` L129/L193、`quest.rs` L104-215）。
2. 同様に **`vq.arc` 側の同じエントリ**（`quest/questData/questData_0000101_jpn`）だけを置換した版を作る。
   **片方だけを変えた 2 種類**を用意する（両方同じにすると判定できない）。

**配置（LayeredFS）**

```
SD:/atmosphere/contents/0100C3800049C000/romfs/nativeNX/arc/quest/q0000101.arc
SD:/atmosphere/contents/0100C3800049C000/romfs/nativeNX/arc/quest/vq.arc
```

（Atmosphère の実装は `/atmosphere/contents/%016lx/romfs/%s` を組み立てる。
根拠: `stratosphere/ams_mitm/source/amsmitm_fs_utils.cpp` L35-36, L173-183）

**判定**

| 実行 | 置換した側 | 村のクエストボード（1★ #101）のタイトル | 受注してクエスト開始 |
|---|---|---|---|
| A | 個別 `q0000101.arc` のみ | 変わらなければ「一覧は集約」 | クエスト内の報酬/支給を見る |
| B | 集約 `vq.arc` のみ | **変われば「一覧は集約」確定** | — |

* A でタイトルが変わらない ∧ B でタイトルが変わる → **一覧は集約が正**（本レポートの推定どおり）
* A でもタイトルが変わる → ゲームは個別 .arc の `.gmd` を一覧に使っている（推定が外れ）

> 注意: 集約を編集する場合、**エントリ数・データオフセットを変えない**こと。
> 文字列長を変えると .arc 全体のサイズが変わり、§3.3 の Fix 処理や
> セーブの 0x1400 枠（配信クエスト側）に影響しうる。

### 4.4 判定フローチャート（今後この手を再び使うとき）

```
[1] 対象が .rem/.sup/.sem/.esl/.qdl/.qdp を持つか？
      yes -> そのファイルが実プレイの本体。集約には入っていない（本件はこれ）
      no  -> 一覧専用の可能性が高い
[2] 集約に載っている ID が個別にも存在するか？ -> 本件は 1,603/1,603 存在
[3] 集約と個別でペイロードが同一か？ -> 本件は全数一致（=どちらを読んでも内容は同じ）
[4] どちらを読んでいるかは「差分 romfs 実験」でしか切れない -> §4.3
[5] 実行時に開かれた名前は RAM ダンプに残る（"arc\quest\…" を検索） -> §3.4
```

---

## 5. 注入コードでできることの範囲とリスク

「注入できるとして」（= 将来 A32 のフック基盤ができた場合、または IPS で手書きする場合）の整理。

### 5.1 機能量とリスク

| 段階 | できること | 実装コスト | 主なリスク |
|---|---|---|---|
| **A. 読み取りログ** | 関数入口でレジスタ/引数を控え、自前バッファに書く。終了時に SD へ出す | 低〜中 | ログ出力（`nn::fs` 書き込み）を **ゲームのロード中に呼ぶとデッドロック**しうる。まず RAM バッファに溜めて後で吐く設計にする |
| **B. 値の差し替え** | 戻り値を 1 に固定 / state を強制変更 / size を書き換え | 低 | 状態機械を飛ばすと **リソース未ロードのまま次状態へ進み、NULL 参照でクラッシュ**。§3.3 の Fix を踏むと **セーブが書き換わる** |
| **C. 関数の完全置換** | 関数全体を自前実装に差し替え | 高 | **MHXX の `main` に code cave が無い**（§5.2）。既存関数を 1 つ潰す必要があり、その関数が別用途で呼ばれていると連鎖クラッシュ |

### 5.2 code cave が無いこと【事実】

注入には「命令を置く場所」が要る。`out_update/image/main.bin` の text 領域
（0〜0x13E6618）を走査した結果:

| 指標 | 実測 |
|---|---|
| text 内のゼロ連続列の総数 | 2,440,556 |
| **最長のゼロ連続長** | **10 バイト**（例: 0x6BD0CC, 0xD87A28, 0xDD3838） |
| 64 バイト以上のゼロ領域 | **0 個** |
| 256 バイト以上 | **0 個** |

また text と rodata の境界（VA 0x13E6618〜0x13E7000）は**パディングではなく rodata の実データ**
（`"C:\project\ev1627\Rabbit…"` などの文字列とポインタ表が入っている）。

**⇒ MHXX の `main` には「空いた実行領域」が存在しない。**
関数置換をするなら「使われていない関数を 1 つ見つけて潰す」しかない（要解析・高リスク）。

> 32bit プロセスの `.text` は RX でマップされ、ヒープは RW（非実行）。
> したがって「ヒープに書いてジャンプ」も不可（`svcSetMemoryPermission` を呼ぶ権限が無い）。

### 5.3 手段別のリスク一覧

| 手段 | 変更対象 | 失敗時の影響範囲 | 復旧 |
|---|---|---|---|
| Atmosphère チート (`.txt`) | 実行時メモリのみ | ゲーム 1 セッション（再起動で消える） | 再起動 or `dmnt_cheats_enabled_by_default=0` |
| exefs_patches (IPS32) | ロード時の `main` イメージ | そのゲームの起動失敗 | `.ips` を消すだけ |
| LayeredFS (romfs) | 読み込まれるアセット | クエスト表示・進行 | ファイルを消すだけ |
| exefs 差し替え (`subsdk9` 等) | プロセス構成 | 起動失敗・**最悪でシステム再起動** | SD のファイルを消すだけ |
| **セーブ編集** | ユーザーデータ | **取り返しがつかない** | バックアップのみ |

**本体（NAND / システム）を壊す経路は、上記のどれも踏まない。** すべて SD カード上のファイルで完結する。
唯一の例外は「CFW 側の設定ミス（`system_settings.ini` の不正など）」で、これは本体ではなく SD の設定の問題。

---

## 6. 実機で動かすための前提

### 6.1 必要なもの

| 項目 | 内容 |
|---|---|
| CFW | Atmosphère 1.x 系（`/atmosphere/contents/` を使うため **0.10.0 以降必須**。実運用は 1.x 推奨） |
| 起動 | Hekate / fusee いずれでも可（チートは `dmnt`、exefs は `loader`、romfs は `fs_mitm` が担当） |
| タイトル ID | 本体 `0100C3800049C000` / 更新 `0100C3800049C800` |
| `main` の build id | `9d4c86e6ef74504a`（v1.5.1） |
| ゲーム版 | v1.5.1（v393216）。**版が違うとオフセットも build id も変わる** |
| ビルド環境（exlaunch を使う場合） | devkitPro + libnx（ただし §1.5 により本件では使えない） |
| ビルド環境（IPS を作る場合） | 不要（A32 アセンブラ + パッチ生成のみ。`arm-none-eabi-as` があると楽） |
| 確認用 | RAM ダンプ手段（Ryujinx ならプロセスメモリ、実機なら `dmnt:cht` の ReadCheatProcessMemory を叩く自作 homebrew） |

### 6.2 配置パス一覧【事実】

| 目的 | パス |
|---|---|
| チート | `/atmosphere/contents/0100C3800049C000/cheats/9d4c86e6ef74504a.txt` |
| チートのトグル保存 | `/atmosphere/contents/0100C3800049C000/cheats/toggles.txt` |
| exefs 差し替え | `/atmosphere/contents/0100C3800049C000/exefs/<nso名>`（`rtld`/`main`/`subsdk0..9`/`sdk`） |
| 権限記述子 | `/atmosphere/contents/0100C3800049C000/exefs/main.npdm` |
| NSO をロードさせない | `…/exefs/<nso名>.stub`（空ファイル） |
| exefs 丸ごと差し替え | `…/exefs.nsp`（PFS0） |
| NSO の IPS パッチ | `/atmosphere/exefs_patches/<任意名>/<module id 32 桁 hex>.ips`（拡張子は常に `.ips`。IPS32 は中身の magic で判別） |
| アセット差し替え | `/atmosphere/contents/0100C3800049C000/romfs/<romfs 内のパス>` |
| アセット（単一 bin） | `…/romfs.bin` |

（出典: `docs/components/modules/loader.md`、`docs/features/cheats.md`、
`stratosphere/ams_mitm/source/amsmitm_fs_utils.cpp`）

### 6.2.1 IPS パッチの仕様（実装から確認）【事実】

`exefs_patches` は今回の本命候補なので、実装を読んで確定させた。

**ファイル名**（`libraries/libstratosphere/source/patcher/patcher_api.cpp`）

```cpp
constexpr const char IpsFileExtension[] = ".ips";      // → 拡張子は常に .ips
constexpr size_t ModuleIpsPatchLength  = 2 * sizeof(ro::ModuleId) + 7;  // 32 + 7
// IsIpsFileForModule(): 名前は hex 32 桁（末尾の 0 は省略可）+ ".ips"、長さは偶数
```

* **フォーマット判別は中身**: 先頭 magic が `PATCH`/`EOF` なら IPS、`IPS32`/`EEOF` なら IPS32。
  拡張子では区別しない（`.ips32` という名前は認識されない）。
* ファイル名は **16 バイトの module id 全体**を hex にしたもの。チートの「先頭 8 バイト」とは違う。
  MHXX v1.5.1 の `main` なら:

```
/atmosphere/exefs_patches/mhxx_quest/9d4c86e6ef74504afa3a485c9797653b.ips
```

（末尾 0 は省略できるが、`9d4c86e6ef74504a.ips`（先頭 16 桁だけ）では **一致しない**。
`MatchesModuleId()` が 16 バイト全体を `memcmp` するため。）

**オフセット**（`LocateAndApplyIpsPatchesToModule` の呼び出し元と本体）

```cpp
constexpr size_t NsoPatchesProtectedSize   = sizeof(NsoHeader);  // 0x100
constexpr size_t NsoPatchesProtectedOffset = sizeof(NsoHeader);  // 0x100
...
if (patch_offset < protected_size) { /* 先頭 0x100 B は書き換え禁止 */ }
patch_offset -= offset;              // ← 0x100 を引いてからモジュールに書く
```

**⇒ IPS のオフセット = 「`main` 先頭からのオフセット」+ 0x100**。
すなわち本レポートの VA 表記（例: 状態機械 0x37A6C）をそのまま使うなら、
```
IPS の patch offset = 0x37A6C + 0x100 = 0x37B6C
```
である。**先頭 0x100 B（NSO ヘッダ）は保護されていて書き換えられない**ので、
モジュール先頭の 1 ページを壊す事故は起きない。

**サイズ上限**: IPS のオフセットは 24 bit（〜16 MiB）。MHXX の `main` は
非圧縮で約 25.6 MB（text 20.8 MB + rodata 3.5 MB + data 1.1 MB）なので、
**text の後半（0x1000000 以降）を触るなら IPS32 が必須**。
（`docs/components/modules/loader.md` にも "if the NSO you are patching is larger than 16 MiB,
you will have to use a tool that supports IPS32" と明記）

### 6.3 本体を壊さない手順（鉄則）

1. **バックアップを最初に取る**: `/atmosphere` と該当セーブ（`save_backups/` が既にある）。
   セーブは**エミュレータ側で作業**するのが最も安全（本ワークスペースは Ryujinx のセーブを既に使っている）。
2. **1 要素ずつ入れる**。チート・romfs・exefs を同時に入れない（切り分け不能になる）。
3. **必ず「無効化スイッチ」を先に用意する**:
   * チート: `/atmosphere/config/system_settings.ini` の `[atmosphere] dmnt_cheats_enabled_by_default = u8!0x0`
   * romfs/exefs: 問題が出たら SD からファイルを消すだけ（＝消せる場所にだけ置く）
4. **起動確認は「ゲームが起動するか」→「村に行けるか」→「クエストボードが開くか」の順**。
   どこで落ちたかで原因の層が分かる（loader / romfs / データ）。
5. **セーブを書き換える実験は最後**。§3.3 の Fix 処理があるため、size を触る改造は特に危険。
6. 停電・強制終了に備え、**exFAT ではなく FAT32 の SD** を推奨（破損時の復旧が容易）。

---

## 7. 既存 `cheats/` と `existing_mod/` の流用可否

### 7.1 `cheats/9d4c86e6ef74504a.txt` — **流用可**【事実】

* ファイル名が build id の先頭 8 バイトと一致（§2.3 で照合済み）。**そのまま使える。**
* 内容は所持金 3 種（`04000000 042CA1DC4 …`）。アドレスは 32bit 前提で書かれている。
* ヘッダに「Ryujinx で 2 セッション確認 / 実機未確認」「Ryujinx の gdb stub はメモリ読み書き不可」の
  注意書きがあり、**実機検証が済んでいない**ことが明記されている。
* 流用のポイント: **コード種別 `04000000`（main モジュール相対 + 4 バイト書き込み）** の書式が
  そのまま「状態機械の state 読み書き」に使える。読み取りは `dmnt:cht` の ReadCheatProcessMemory が要る
  （`.txt` のチート自体にログ機能は無い）。

> 注意: `cheats/9d4c86e6ef74504a.txt` は**更新版 (v1.5.1) の build id**。
> 本体 v0 で動かす場合は `8b338f57a04a2688.txt` という名前になる（アドレスも別物）。

### 7.2 `existing_mod/subsdk9.bin` — **流用不可**（理由 3 点）【事実】

| # | 理由 | 根拠 |
|---|---|---|
| 1 | **AArch64 ビルド**である | 先頭 `08 00 00 14`（A64 `b`）、`1f 20 03 d5`（A64 `nop`）、A64 `ret` 3,367 個 / A32 prologue 1 個 |
| 2 | MHXX は **AArch32 プロセス**であり、64bit モジュールを同居させられない | §2.1（全モジュール A64 命令 0 個） |
| 3 | MHXX の exefs に **`subsdk9` スロットが元々無い** | §2.2（base/update とも main, rtld, sdk, subsdk0-4 のみ） |

補足: loader は SD 上の `subsdk9` を「追加モジュール」として読む仕様なので
（§1.2）、**仮に 32bit 版を作れば配置先としては成立する**。成立しないのは中身のアーキテクチャ。
つまり**このファイルは「A32 で作り直せば再利用できる配置・命名の見本」**ではある。

### 7.3 `mods_disabled/exefs_MHNX_mod/` — **流用不可**【事実】

* `subsdk9` は NSO0 ヘッダを持つが、**宣言サイズと実ファイルサイズが矛盾**（text を 0x100 から
  0x128E70 B と宣言、実ファイルは 0xF10A6 B）。切り詰めか破損。
* 中身は `existing_mod/subsdk9.bin` と同一のペイロード（`~~exlaunch uwu~~` が同位置関係で出現）。
* `main.npdm`（1,516 B）だけは**権限記述子の見本として価値がある**（本体の 1,476 B との差分 40 B は
  おそらくサービス追加分）。32bit 用に作り直す際の参考にはなる【推定】。

### 7.4 `nx/cheats/mhxx_money_ce.lua`（親ディレクトリ）— **参考になる**【事実】

Cheat Engine で Ryujinx のゲスト RAM を読む Lua。**MOD0 マジックでゲスト RAM ベース K を自動取得**する
手法（`host = guest + K`）は、まさに本レポート §3.4 で使った「RAM ダンプから名前を探す」と同じ発想。
RAM ダンプツールを自作するときの雛形になる。

---

## 8. 推奨プラン（優先順）

### P0: LayeredFS 差分実験で「個別 vs 集約」を確定（注入なし・今日できる）

§4.3 の手順。**これだけで依頼 3 番は決着する。** リスクはほぼゼロ（SD のファイルを消せば戻る）。

### P1: RAM ダンプで「受注時の個別 .arc 読み込み」を捉える（注入なし）

1. エミュレータ（または実機 + `dmnt:cht`）で村クエストを受注し、ロード中の任意の時点で RAM をダンプ。
2. `arc\quest\q` / `quest\rem\rem_` / `quest\supp\supp_` を検索する。
3. ヒットすれば「個別 .arc が読まれた」ことの直接証拠。§4.1 の論理証明と合わせて確定。
4. 本ワークスペースの `scan/*.bin` と同じ手法。**ダンプ対象にモジュール領域を含めること**
   （現行 4 本はコード/rodata を含んでいない）。

### P2: チートで状態機械を観測・改変（注入なし・実機可）

* 観測: `dmnt:cht` の ReadCheatProcessMemory で `this+0xDF4` を定期読み出し。
  `this` の所在は未確定（静的確保なら `0x40000000`〜、ヒープ確保ならそれ以降）。
  まず **RAM ダンプ上で「`+0xDF4` が 1〜19 の値を取る構造体」を探して当たりを付ける**のが早い
  （ダンプは既に 4 本ある）。
* 改変: `04000000` 種別で state を固定 → どの状態で止まるかで遷移の意味を切り分け。
* **リスク**: 状態を飛ばすとリソース未ロードでクラッシュしやすい。**セーブは事前バックアップ**。

### P3: A32 用の独自フック基盤を作る（研究開発・非推奨/長期）

exlaunch の `nx64` 実装を A32 用に書き直すのは、作者自身が「hooking library と kernel svc の書き直し」と
述べているとおり大工事。しかも本件の目的（ファイル読込の観測）は **P0/P1 で達成できる**。
投資対効果は悪い。**やるなら「IPS32 で 1 命令だけ書き換える」段階から始める**こと。

### 8.1 「もし注入できるなら」の最小構成（将来用メモ）

1. **P0/P1 で対象を確定**（個別が読まれる時刻・関数）。
2. **IPS32 で 1 命令パッチ**（例: 0x389A0 の `cmp` を `mov` に変える）で挙動が変わることを確認。
   → ここで初めてオフセット規約とページ属性が正しいことが証明される。
3. 名前連結関数が特定できたら、**関数の先頭 4 命令を潰して自前ログに差し替える**
   （関数が短ければ code cave 不要で済む場合がある）。
4. どうしても新しいコードが必要なら、**使われていない関数を 1 つ潰して置き換える**
   （§5.2 のとおり空きが無いため）。

---

## 9. 未確定事項

| # | 内容 | 現状 | 次の一手 |
|---|---|---|---|
| U1 | 集約 .arc を開く関数の VA | 静的 xref 0 件。実行時メモリに `arc\quest\vq` は出る | 名前連結 or FS オープンを P1 のダンプで追う |
| U2 | 一覧表示が集約由来か個別由来か | データは同一（1,603/1,603 一致）で静的に区別不能 | §4.3 の差分実験 |
| U3 | 受注開始時に個別 .arc が読まれるか | 構造上は必然。実測は未到達 | P1 のダンプ |
| U4 | `sFestaFileLoader` の `cArchiveCase` の意味 | 名前と並びからの推定のみ | クラス名で RTTI/vtable を探す（本イメージには参照が無い） |
| U5 | ダンプにモジュール領域が含まれない理由 | 4 本ともコード/rodata が無い | ダンプツールの取得範囲を確認・拡張 |
| U6 | 0x37A6C が実機で同アドレスか | `main` のビルドが同一なら同一のはず | 実機で `F0 4F 2D E9` を確認（§2.5） |
| U7 | MHXX の 32bit プロセスに 64bit モジュールを載せた場合の挙動 | 未検証（載らないはず） | `subsdk9` を置いて起動ログを見る（壊しても SD だけで済む） |
| U8 | `main.npdm` を差し替えると何が変わるか | 未検証 | 権限追加が必要になったとき |

---

## 10. 付録: 本レポートの実測に使った手順

### 10.1 exefs のアーキテクチャ判定

```text
各 NSO の全 4 バイト語を走査し、
  A32 prologue : (w >> 16) == 0xE92D      （push {…,lr}）
  A64 ret      : w == 0xD65F03C0
を数える。MHXX は全モジュールで A64 ret = 0。
```

### 10.2 集約 vs 個別の突き合わせ

```text
.arc を parse し（magic "ARC\0" / version 17 / u16 entryCount / 80 B エントリ）、
エントリの typeHash で .mib (0x1BBFD18E) と .gmd (0x242BB29A) を判別、
dataOffset から zlib inflate して比較する。
集約 4 本の全エントリについて、同名の個別 q<id>.arc を開いてバイト比較 → 1,603/1,603 一致。
```

### 10.3 文字列 xref（5 手法）

```text
① 4 バイト境界の全語 == VA
② 全バイトオフセットで == VA（アライン無視。偽陽性の切り分けに使う）
③ movw/movt の即値ペアが VA を構成するか
④ add/sub rX, pc, #imm12 の実効アドレス == VA
⑤ ldr rX, [pc, #imm] の実効アドレス == VA
```

### 10.4 RAM ダンプの検索

```text
scan/*.bin は 4 領域の連結（meta ファイルに範囲が書かれている）:
  0x0B000000 +0x05000000 / 0x36000000 +0x05000000 / 0x40000000 +0x0B000000 / 0x68000000 +0x0A000000
ファイルオフセット -> ゲストアドレスに変換しつつ、32 MiB ずつ走査（64 B のオーバーラップ）。
"arc\quest\" を検索 -> 4 本すべてで 1 件（0x43C9F908 "arc\quest\vq"）。
```

### 10.5 主要な出典

| 内容 | URL |
|---|---|
| exlaunch 本体 | <https://github.com/shadowninja108/exlaunch> |
| 32bit 非対応の回答 | <https://github.com/shadowninja108/exlaunch/issues/24> |
| Atmosphère loader（exefs 差し替え / IPS） | <https://github.com/Atmosphere-NX/Atmosphere/blob/master/docs/components/modules/loader.md> |
| Atmosphère チート仕様 | <https://github.com/Atmosphere-NX/Atmosphere/blob/master/docs/features/cheats.md> |
| LayeredFS のパス生成 | `stratosphere/ams_mitm/source/amsmitm_fs_utils.cpp` L35-36, L173-183 |
| titles → contents（0.10.0） | <https://github.com/Atmosphere-NX/Atmosphere/blob/master/docs/changelog.md> |
| dmnt:cht サービス定義 | <https://github.com/Atmosphere-NX/Atmosphere/blob/master/docs/components/modules/dmnt.md> |

---

## 11. まとめ（3 行）

1. **exlaunch は使えない。** MHXX は AArch32、exlaunch のフック実装は nx64 専用で、作者も
   「32bit 対応はフックライブラリの書き直しが必要」と明言している（issue #24）。
   ワークスペースの `existing_mod/subsdk9.bin` も AArch64 ビルドなので動かない。
2. **依頼 3 番は注入なしで決着した。** 集約 4 本（1,603 ID）は `.mib`+`.gmd` だけを持ち、
   個別 .arc とバイト完全一致。`.rem`/`.sup`/`.sem`/`.esl`/`.qdl`/`.qdp` は個別にしか無いので、
   **実プレイは個別 .arc が本体**。一覧表示は集約（RAM に `arc\quest\vq` が実在）で、
   最終確認は §4.3 の差分実験で 15 分。
3. **観測したいだけならチート + RAM ダンプで足りる。** `cheats/9d4c86e6ef74504a.txt` はそのまま使え、
   `dmnt:cht` の Read/WriteCheatProcessMemory で状態機械（`this+0xDF4`）を読み書きできる。
   `main` には code cave が無い（最長ゼロ連続 10 B）ので、**関数の完全置換は狙わない**のが正解。
