# Nintendo Switch (.nro) Rust ビルド環境 構築レポート

対象: MHXX MHNX Editor (`C:\anyWorkspace\nx\mhxx\mhnxeditor`)
作業日: 2026-10-19 / 実機なし（ビルドとコンテナ形式の検証まで）

---

## 0. 結論 (TL;DR)

**Rust から直接 Nintendo Switch homebrew (.nro) をビルドできる環境を構築できた。**
現行ツールチェーン（Rust nightly 1.101.0 + devkitA64 gcc 15.2.0 + libnx 4.12.0）で動作し、
追加の Cargo サブコマンドや古いクレートには依存しない。

| 項目 | 結果 |
| --- | --- |
| 採用方式 | カスタム target JSON + `-Z build-std` + `linker = aarch64-none-elf-gcc`（rustc が devkitA64 の gcc をリンカとして駆動） |
| 成果物 | `dist/mhnxeditor.nro` — 235,717 バイト、`NRO0` マジック確認済み |
| SHA256 (cargo ルート, この時点) | `EB548043FDA871DF01ECC2E575852AAA1637CD114B712F31D6473867E530FEAC` |
| クリーンビルド時間 | 14.5 秒（`cargo clean` 直後、core/alloc/compiler_builtins のビルドを含む） |
| ライブラリ | libnx を通常どおり使用（`consoleInit` / `printf` / newlib stdio / `fs` API） |
| 代替ルート | devkitA64 gcc が Rust staticlib をリンクする方式も検証済み（同一サイズの NRO を生成） |
| 再現 | `switch/build.ps1`（nightly / rust-src が無ければ自動導入） |

> **注（リビジョン依存の数値）**: 本文中のサイズ・SHA256 は 2026-10-19 12:23 時点の最小構成
> （画面表示＋SD プローブのみ）の実測値。その後 `switch/src/editor.rs` などが追加され、
> **同じビルド手順のまま** NRO は 243,909 バイトに増えている（12:25 時点の再ビルドで
> `NRO0` / `HOMEBREW` / `ASET` とセグメント整合性を再確認済み）。
> 数値はクレートの内容に応じて変わるため、合否判定には `build.ps1` の自動検証を使うこと。

---

## 1. 環境

| ツール | バージョン / パス |
| --- | --- |
| OS | Windows 11 + PowerShell 7 (pwsh) |
| rustup | 1.29.x / toolchains: stable-x86_64-pc-windows-msvc, nightly-x86_64-pc-windows-msvc |
| rustc (nightly) | 1.101.0-nightly (b6eeb93c9 2026-10-18) |
| rustc (stable) | 1.97.1（nightly 必須のためビルドには未使用） |
| devkitPro | C:\devkitPro（devkitA64 / libnx / tools / portlibs / msys2） |
| devkitA64 gcc | aarch64-none-elf-gcc 15.2.0 |
| libnx | 4.12.0（`libnx.a` に switch_crt0.o を含む） |
| elf2nro / nacptool | C:\devkitPro\tools\bin |
| Python | 3.12.10（NRO 構造の検証に使用） |
| DEVKITPRO 環境変数 | `/opt/devkitpro`（msys2 形式。Windows パスへの読み替えが必要 → §5.5） |

`rustc --print target-list` には **`aarch64-nintendo-switch-freestanding` (tier 3)** が含まれる。
ただしこれは「pure Rust ツールチェーン」（rust-lld + 独自リンクスクリプト + libnx 非依存）向けの定義であり、
libnx / newlib をリンクする本件ではカスタム target JSON を使う（§3.1）。

---

## 2. 既存手法の調査（2026-10 時点）と選定理由

| 手法 | 調査結果 | 採否 |
| --- | --- | --- |
| **cargo-nx** (aarch64-switch-rs) | crates.io には**未公開**（`cargo install cargo-nx` は即失敗）。ただし **git からは導入成功**（v0.2.0 を 45 秒でビルド・インストール）。GitHub の最終 push は 2025-06-25、star 96。純 Rust の `nx` クレートと組で使う前提で、libnx / newlib を使わない世界観 | △（導入は可能。要件が libnx の consoleInit/printf のため今回は不採用） |
| **aarch64-switch-rs/nx**（純 Rust libnx 代替） | 活発（最終 push 2026-09-10、star 153）。libnx を使わない独自ランタイム | △（将来の選択肢） |
| **nx-std/tools** | 2026-10-19 に push されたばかりの新興 fork（star 0）。cargo-nx 互換を称する | △（情報が少なく保留） |
| **rusty-horizon/nx-rs** | 最終 push 2019-08-27。libstd fork 前提で現代の nightly では動かない | ✗ |
| crates.io `nx` 0.3.0 | 2018 年公開の「NX ファイル読み込み」ライブラリ。Switch homebrew とは無関係 | ✗ |
| **github.com/rust-switch** | 参照先が 404（存在しない） | ✗ |
| **自前 target JSON + `-Z build-std` + devkitA64 リンカ** | 追加ツール不要。rustc が gcc をリンカとして呼び、switch.specs 経由で libnx にリンク | **採用（動作確認済み）** |
| **C の libnx 最小プロジェクト + Rust staticlib をリンク** | devkitA64 gcc が `libmhnx_switch.a` をリンクする devkitPro Makefile 方式 | **採用（route c として動作確認済み）** |

### 2.1 cargo-nx の実試行ログ

crates.io 経由（失敗）:

```text
PS> cargo install cargo-nx
    Updating crates.io index
error: could not find `cargo-nx` in registry `crates-io` with version `*`
EXITCODE=101
```

GitHub の git 経由（成功）:

```text
PS> cargo install --git https://github.com/aarch64-switch-rs/cargo-nx cargo-nx
   Compiling linkle v0.2.10 (https://github.com/XorTroll/linkle?branch=npdm#10212224)
   Compiling netloader v0.1.0 (...)
   Compiling cargo-nx v0.2.0 (C:\Users\haruk\.cargo\git\checkouts\cargo-nx-...)
    Finished `release` profile [optimized] target(s) in 45.18s
  Installing C:\Users\haruk\.cargo\bin\cargo-nx.exe
   Installed package `cargo-nx v0.2.0` (executable `cargo-nx.exe`)
EXITCODE=0
```

つまり「cargo-nx が 2026 年に使えるか」という問いへの答えは **「git から入れれば入る」**。
ただし cargo-nx は target 定義・リンクスクリプトを内蔵し、libnx ではなく pure-Rust の
`nx` ランタイムを前提とするため、「libnx の consoleInit / printf を使う」という今回の要件とは
噛み合わない。よって **採用はせず**（インストール済みなので将来の比較実験には使える）、
動くことを実証できた「自前 target + devkitA64 リンカ」を主経路とした。

---

## 3. 採用した構成

```text
switch/
├─ Cargo.toml                                  bin mhnxeditor + lib mhnx_switch (rlib+staticlib)
├─ rust-toolchain.toml                         nightly + rust-src を固定
├─ .cargo/config.toml                          target / build-std / json-target-spec / DEVKITPRO
├─ targets/aarch64-nintendo-switch-libnx.json  カスタム target（gcc リンカ指定）
├─ build.rs                                    devkitPro パス → リンク引数
├─ src/sys.rs                                  libnx + newlib の FFI（レイアウトを const assert）
├─ src/app.rs                                  画面表示・SD カードプローブ
├─ src/main.rs                                 #![no_main] + #[no_mangle] extern "C" fn main
├─ src/lib.rs                                  panic handler / global allocator / mhnx_main()
├─ tools/rustmain.c                            route c 用の C エントリポイント
├─ sdcard/switch/mhnxeditor/hello.txt          SD カード用サンプル
├─ build.ps1                                   ビルド〜NRO 生成〜検証〜dist 配置
└─ README.md                                   使い方
```

### 3.1 カスタム target JSON（要点）

```json
{
  "arch": "aarch64",
  "llvm-target": "aarch64-unknown-none",
  "os": "horizon",
  "features": "+v8a,+neon,+crc,+crypto",
  "linker": "aarch64-none-elf-gcc",
  "linker-flavor": "gcc",
  "dynamic-linking": true,
  "position-independent-executables": true,
  "relocation-model": "pic",
  "panic-strategy": "abort",
  "crt-objects-fallback": "false",
  "eh-frame-header": false,
  "stack-probes": { "kind": "inline" },
  "max-atomic-width": 128,
  "target-pointer-width": 64
}
```

* `linker-flavor: gcc` が重要。組み込みの `aarch64-nintendo-switch-freestanding` は `gnu-lld` flavour で
  rust-lld 前提のフラグを渡すため、gcc ドライバとは噛み合わない。
* `crt-objects-fallback: "false"` は組み込み target と同じ。rustc が crt1.o 等を足さない
  （_start は libnx の switch_crt0.o が提供する）。
* `position-independent-executables: true` + `dynamic-linking: true` で `-pie` が付き、
  switch.ld / switch.specs の前提（PIE）と一致する。

### 3.2 build.rs が渡すリンク引数

```text
-specs=$DEVKITPRO/libnx/switch.specs      # -T switch.ld / -pie / --gc-sections / --nx-module-name / crti+crtbegin
-march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE
-L$DEVKITPRO/libnx/lib
-L$DEVKITPRO/portlibs/switch/lib
-lnx
-Wl,--start-group -lgcc -lg -lc -lsysbase -Wl,--end-group   # ← これが無いと libc がリンクされない（§5.4）
```

devkitPro のアプリケーションテンプレート Makefile の `$(LDFLAGS)` / `$(LIBS)` / `$(LIBPATHS)` を
1 フラグずつ再現したもの。

---

## 4. ビルド手順（再現用）

### 4.1 一発ビルド

```powershell
cd C:\anyWorkspace\nx\mhxx\mhnxeditor\switch
pwsh -File .\build.ps1              # rustc がリンクまで行う（既定）
pwsh -File .\build.ps1 -Clean       # 完全クリーンビルド
pwsh -File .\build.ps1 -Route c     # devkitA64 gcc が Rust staticlib をリンク
```

build.ps1 が行うこと:

1. devkitPro の解決（`-DevkitPro` → `$env:DEVKITPRO` → `C:/devkitPro`。msys2 形式は自動変換）
2. `DEVKITPRO` / `DEVKITA64` / `PATH` を設定（devkitA64\bin, tools\bin を前置）
3. nightly と rust-src の確認（無ければ `rustup toolchain install nightly --profile minimal --component rust-src`）
4. `cargo +nightly build --target targets/aarch64-nintendo-switch-libnx.json --release`
5. `nacptool --create` で control.nacp 生成
6. `elf2nro <elf> <nro> --icon=$DEVKITPRO/libnx/default_icon.jpg --nacp=<nacp>`
7. NRO ヘッダ検証（`NRO0` マジック / size / mod_offset / ファイルサイズ）
8. `dist/` へ配置（`dist/mhnxeditor.nro` と SD カード用 `dist/switch/...`）

### 4.2 手動で同じことをする場合

```powershell
$env:DEVKITPRO = 'C:/devkitPro'                         # フォワードスラッシュ推奨（§5.6）
$env:PATH = "C:\devkitPro\devkitA64\bin;C:\devkitPro\tools\bin;$env:PATH"
cd C:\anyWorkspace\nx\mhxx\mhnxeditor\switch

# ビルド（nightly / build-std / json-target-spec は .cargo/config.toml に設定済み）
cargo +nightly build --release --target targets/aarch64-nintendo-switch-libnx.json

$out = "target/aarch64-nintendo-switch-libnx/release"
nacptool --create mhnxeditor "mhnx project" 0.1.0 "$out/mhnxeditor.nacp"
elf2nro "$out/mhnxeditor" "$out/mhnxeditor.nro" --icon=C:/devkitPro/libnx/default_icon.jpg --nacp="$out/mhnxeditor.nacp"
Copy-Item "$out/mhnxeditor.nro" ..\dist\mhnxeditor.nro -Force
```

`.cargo/config.toml` の中身（ビルドを成立させている設定）:

```toml
[build]
target = "targets/aarch64-nintendo-switch-libnx.json"

[unstable]
build-std = ["core", "alloc"]     # tier-3 target なので std は同梱されない
json-target-spec = true           # 無いと ".json target specs require -Zjson-target-spec"（§5.2）

[env]
DEVKITPRO = { value = "C:/devkitPro", force = false }   # 既存の環境変数を優先
```

---

## 5. ハマりどころ（実際に出たエラーと対処）

### 5.1 build-override プロファイルに panic は書けない

```text
error: failed to parse manifest at `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\Cargo.toml`
Caused by:
  `panic` may not be specified in a `build-override` profile
```

build.rs は host 用にコンパイルされるため「unwind にしておこう」と考えて
`[profile.release.build-override] panic = "unwind"` を書いたのが原因。**セクションごと削除**して解決
（build.rs は panic しないので `panic = "abort"` のままで問題ない）。

### 5.2 カスタム target JSON には -Zjson-target-spec が必要

```text
error: `.json` target specs require -Zjson-target-spec to be added to the cargo invocation
```

コマンドラインの `-Zjson-target-spec` でもよいが、`.cargo/config.toml` の
`[unstable] json-target-spec = true` で永続化できる（nightly cargo が必要）。
これで素の `cargo build` が通るようになる。

### 5.3 target-pointer-width は数値（文字列は不可）

```text
error: error loading target specification: target-pointer-width: invalid type: string "64", expected u16 at line 19 column 32
    = help: did you mean `aarch64-nintendo-switch-freestanding`?
```

`"target-pointer-width": "64"` → `"target-pointer-width": 64` に修正。
（`rustc -Z unstable-options --print target-spec-json --target aarch64-nintendo-switch-freestanding`
の出力をそのまま真似るのが確実）

### 5.4 最重要: rustc が -nodefaultlibs を付けるため libc / libgcc がリンクされない

`crt-objects-fallback: "false"` の target では、rustc が gcc ドライバに **`-nodefaultlibs` を渡す**。
gcc が通常末尾に付ける `-lc` / `-lgcc` などが消えるため、libnx が使う libc シンボルが軒並み未定義になる:

```text
ld.exe: C:/devkitPro/libnx/lib\libnx.a(fs_dev.o): in function `fsdev_dirnext`:
  fs_dev.c:1435: undefined reference to `memset`
  fs_dev.c:1775: undefined reference to `bsearch`
  fs_dev.c:322:  undefined reference to `strncpy`
  fs_dev.c:324:  undefined reference to `AddDevice`
  fs_dev.c:339:  undefined reference to `GetDeviceOpTab`
  fs_dev.c:341:  undefined reference to `setDefaultDevice`
  fs_dev.c:589:  undefined reference to `chdir`
libnx.a(nv.o):     undefined reference to `strlen`
libnx.a(parcel.o): undefined reference to `memcpy`
collect2.exe: error: ld returned 1 exit status
```

原因特定のため、同じ switch.specs で **C の最小プログラムをリンクして collect2 の行を観察**した:

```text
PS> aarch64-none-elf-gcc -specs=C:/devkitPro/libnx/switch.specs -g -march=armv8-a+crc+crypto ^
      -mtune=cortex-a57 -mtp=soft -fPIE -O2 -I C:/devkitPro/libnx/include main.c ^
      -L C:/devkitPro/libnx/lib -lnx -v -o ctest.elf

collect2.exe ... -T C:/devkitPro/libnx/switch.ld -pie --no-dynamic-linker --spare-dynamic-tags=0
  --gc-sections -z text -z now -z nodynamic-undefined-weak -z pack-relative-relocs --build-id=sha1
  --nx-module-name -o ctest.elf .../pic/crti.o .../pic/crtbegin.o --require-defined=main
  -L.../libnx/lib ... ctest.o -lnx --start-group -lgcc -lg -lc -lsysbase --end-group
  .../pic/crtend.o .../pic/crtn.o
```

失われていたのは `--start-group -lgcc -lg -lc -lsysbase --end-group` の 4 ライブラリ。
`build.rs` から `cargo::rustc-link-arg` で同じ並びを渡して解決した:

```text
-Wl,--start-group -lgcc -lg -lc -lsysbase -Wl,--end-group
```

順序も重要で、`-lnx` は `-lc` より前（Makefile と同じ並び）に置く。
crti.o / crtbegin.o / crtend.o / crtn.o は `-nodefaultlibs` では消えない（消えるのは `-nostartfiles`）ため、
specs の `*startfile` 指定がそのまま効いている。

### 5.5 DEVKITPRO が msys2 形式（/opt/devkitpro）

```text
==> devkitPro
ERROR: devkitPro not found at '/opt/devkitpro' (pass -DevkitPro, or set the DEVKITPRO environment variable)
```

環境変数は msys2 のシェル用に `/opt/devkitpro` になっている。build.ps1 に
「候補を順に試し、`/…` 形式なら各ドライブの `<drive>:\<rest>` に展開する」リゾルバを入れ、
libnx の存在で判定して `C:/devkitPro` を自動選択するようにした。

### 5.6 specs の %:getenv(DEVKITPRO ...) にはフォワードスラッシュを渡す

`switch.specs` の `*link` は `-T %:getenv(DEVKITPRO /libnx/switch.ld)` を展開する。
Windows のバックスラッシュを混ぜると specs のエスケープ解釈と衝突しうるため、
スクリプトは `DEVKITPRO=C:/devkitPro`（フォワードスラッシュ）を設定している。

### 5.7 lib を staticlib だけにすると bin から依存できない

`crate-type = ["staticlib"]` のみだと、同一パッケージの bin がその lib を extern crate として
参照できない（rlib のメタデータが無い）。→ `crate-type = ["rlib", "staticlib"]` にして解決。
route c 用の `.a` も同時に生成される。

### 5.8 #[panic_handler] / #[global_allocator] の置き場所

bin と staticlib の両方を自己完結させるため、lang item は **lib (`src/lib.rs`) 側に 1 回だけ**定義し、
`src/main.rs` は `#![no_main]` + `#[no_mangle] extern "C" fn main` のみにした。
bin 側にも書くと lang item 重複エラーになる。

### 5.9 alloc を有効にする

`#[global_allocator]` を newlib の `malloc` / `free` に繋いでいる。
`alloc_error_handler` は Rust 1.68 以降は既定実装があるため定義不要。
`-Z build-std=core,alloc` で core と alloc をソースからビルドしている。

---

## 6. 生成物の検証 — 本当に NRO か

### 6.1 NRO の実構造（libnx `include/switch/nro.h`）

```c
typedef struct { u32 unused; u32 mod_offset; u8 padding[8]; } NroStart;   // 0x00, 16 バイト
typedef struct { u32 magic; u32 unk1; u32 size; u32 unk2;
                 NroSegment segments[3]; u32 bss_size; u32 unk3;
                 u8 build_id[0x20]; u8 padding[0x20]; } NroHeader;        // 0x10, 0x70 バイト
```

**`NRO0` マジックはファイル先頭ではなく 0x10 にある**（先頭 16 バイトの `NroStart` が前にあるため）。
検証時は offset 0x10 を見る必要がある。先頭 4 バイトは `0x14000020` = AArch64 の `B +0x80` 命令で、
0x80 バイトのヘッダを飛び越えて本体コードへ分岐する（＝ヘッダは ELF の `.text` 先頭 = 仮想アドレス 0 に
埋め込まれ、エントリポイント 0 から実行される）。ここを「先頭が NRO0 でない＝壊れている」と誤判定しやすい。

### 6.2 実測値（`dist/mhnxeditor.nro`）

（下記は最小構成時の値。拡張後のクレートでは size=0x36000 / 243,909 バイトになるが、
構造と検証項目は同じ）

```text
file size : 235717
0x00 20 00 00 14           NroStart.unused = B +0x80
0x04 F0 00 00 00           NroStart.mod_offset = 0xF0 (MOD0 構造体)
0x08 "HOMEBREW"            マーカー（devkitPro の C ビルドと一致）
0x10 "NRO0"                NroHeader.magic = 0x304F524E
0x18 0x34000               NroHeader.size (= 3 セグメントの合計)
0x20 segments[0] off=0x0     size=0x25000  (.text,   ELF filesz 0x245F8 -> ページ切上げ)
0x28 segments[1] off=0x25000 size=0xA000   (.rodata, ELF filesz 0x996C)
0x30 segments[2] off=0x2F000 size=0x5000   (.data,   ELF filesz 0x4390)
0x38 bss_size = 0x4000
0x40 build_id[0x20]
0x34000 "ASET"             NroAssetHeader（以降にアイコン JPEG 0xFFD8FF と control.nacp）
```

ELF との対応も機械的に確認した（Python で ELF の各 LOAD セグメント先頭バイト列を NRO 内から探索）:

* ELF `LOAD1`（rodata, ELF off 0x35000）→ NRO 0x25000 に一致
* ELF `LOAD2`（data, ELF off 0x3F000）→ NRO 0x2F000 に一致
* セグメント 0 は NRO 先頭 0x0 から（NRO ヘッダ自体が ELF のテキスト先頭に含まれ、
  elf2nro が `size` / `segments` を書き込む）

### 6.3 devkitPro の C リファレンスとの一致

比較用に C の libnx アプリ（`consoleInit` + `printf` + `padConfigureInput`）を同じ手順で
ELF → NRO 化し、ヘッダ先頭 0x1C バイトを比較:

```text
C 参照 : 20 00 00 14 F0 00 00 00 48 4F 4D 45 42 52 45 57 4E 52 4F 30 00 00 00 00 00 D0 02 00 00
Rust   : 20 00 00 14 F0 00 00 00 48 4F 4D 45 42 52 45 57 4E 52 4F 30 00 00 00 00 00 40 03 00 00
                                                     ^ magic まで完全一致（size はバイナリ依存）
```

さらに `readelf -l` のプログラムヘッダ構成も完全一致（4 本: text R-E / rodata R / data RW / DYNAMIC、
セクションマッピングも同一）。`nm` で `_start` (vaddr 0x0) と `main` の存在も確認:

```text
Rust ELF: Entry point 0x0 / Type DYN (PIE) / Machine AArch64 / 4 program headers
  LOAD 0x10000 vaddr 0x0      filesz 0x245F8  R E
  LOAD 0x35000 vaddr 0x25000  filesz 0x996C   R
  LOAD 0x3F000 vaddr 0x2F000  filesz 0x4390   RW  (memsz 0x8B60)
  DYNAMIC ...
0000000000000000 T _start
0000000000000444 T main
```

`build.ps1` は毎回このうち「`NRO0` マジック / `HOMEBREW` マーカー / size / ファイルサイズ」を自動検証し、
不一致ならエラー終了する。

---

## 7. 代替ルート（route c: C の libnx プロジェクト + Rust staticlib）

「Rust から直接リンクできない場合」に備えた経路だが、実際には**こちらも問題なく動作した**
（build.ps1 に `-Route c` として実装済み）。実行したコマンド:

```powershell
# 1) Rust 側は staticlib としてビルド（Cargo の [lib] crate-type = ["rlib","staticlib"]）
cargo +nightly build --release --target targets/aarch64-nintendo-switch-libnx.json

# 2) C のエントリポイント（tools/rustmain.c）をコンパイル
aarch64-none-elf-gcc -c -g -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE ^
  -O2 -o rustmain.o tools/rustmain.c

# 3) devkitA64 の gcc でリンク（devkitPro Makefile と同じ流儀）
aarch64-none-elf-gcc -specs=C:/devkitPro/libnx/switch.specs -g ^
  -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE ^
  -L C:/devkitPro/libnx/lib ^
  -o mhnxeditor.elf rustmain.o libmhnx_switch.a -lnx

# 4) NRO 化
elf2nro mhnxeditor.elf mhnxeditor.nro --icon=C:/devkitPro/libnx/default_icon.jpg --nacp=mhnxeditor.nacp
```

結果:

| ルート | ELF | NRO | SHA256 |
| --- | --- | --- | --- |
| cargo（rustc が gcc を駆動） | 2,960,352 B | 235,717 B | `EB548043FDA871DF01ECC2E575852AAA1637CD114B712F31D6473867E530FEAC` |
| c（gcc が staticlib をリンク） | 2,961,192 B | 235,717 B | `34812263B9AED831AEB061A604C257873BBBFD7F77AF2124B54AAD8633483316` |

サイズ・セグメント構成は同一。SHA256 が異なるのはリンク入力（C の main スタブと build-id）が
違うためで、どちらも `NRO0` / `HOMEBREW` / `ASET` を持つ正しい NRO。
`src/lib.rs` が `mhnx_main()` を公開しているので、C 側の足回りと Rust のロジックを
組み合わせる構成も可能。

---

## 8. NRO の動作内容（実装済み）

* libnx `consoleInit(NULL)` + `printf` でテキスト表示、`consoleUpdate(NULL)` で毎フレーム転送
* `argc` / `argv` 表示（argv[0] = NRO のパス。libnx が SD 上の自分の位置を渡す）
* SD カードプローブ:
  `fsInitialize` → `fsOpenSdCardFileSystem` → `fsFsGetFreeSpace` / `fsFsGetTotalSpace` →
  `fsFsOpenDirectory` + `fsDirRead` で `/switch/mhnxeditor` の一覧 →
  `fsFsOpenFile` + `fsFileRead` で `hello.txt` 読み出し
* 同ファイルを newlib の `fopen` / `fread` でも読み、stdio 経由（fsdev）でも SD に届くことを確認
* A ボタンで再プローブ、**＋ボタンまたは B ボタンで終了**（hbmenu に戻る）
* Rust の `alloc`（String / Vec / format!）が newlib の malloc 上で動作

FFI 宣言（`src/sys.rs`）は libnx ヘッダから手で書き起こし、構造体レイアウトは
`const _: () = assert!(...)` でビルド時に検証している
（`Service`=16B, `FsDirectoryEntry`=784B, `PadState`=56B, offset_of! でフィールド位置も確認）。

---

## 9. 未検証・残課題

1. **実機（またはエミュレータ）での起動は未検証**。NRO コンテナの形式・ヘッダ・セグメント整合性まで
   検証したが、hbmenu からのロードと画面表示は実機が無いため未確認。
2. SD カード読み出しはコード中に実装済みだが、実行時の結果も未確認（SD 未挿入時の分岐も実装済み）。
3. cargo-nx は git からインストール済み（v0.2.0）。pure-Rust `nx` エコシステム向けのため
   本件では未使用。比較実験をするなら `nx-std/tools` と併せて別途検証が必要。
4. `mhnx-core`（共通ロジック）との連携は未実装。switch クレートは現状自己完結。
   統合する場合は `switch/Cargo.toml` に `mhnx-core = { path = "../crates/mhnx-core" }` を足し、
   `no_std + alloc` を維持していることを確認する。
5. `crates/mhnx-nro` は空ディレクトリのまま（本作業では `switch/` 側に実装した）。
6. LTO / strip は無効のまま（NRO 235 KB）。`[profile.release] lto = "thin"` 等で縮小の余地あり。
7. elf2nro の `--alignedheader` や RomFS 埋め込みは今回未使用。

---

## 10. 付録

### A. rustc が実際に発行したリンク行（`cargo build -v` より抜粋）

```text
aarch64-none-elf-gcc <objects...> -Wl,--as-needed -Wl,-Bstatic <rlibs: mhnx_switch, alloc, core, compiler_builtins>
  -L .../raw-dylibs -Wl,-Bdynamic -Wl,-z,noexecstack
  -o .../mhnxeditor -Wl,--gc-sections -pie -Wl,-z,norelro
  -nodefaultlibs                                  <-- ★ これが libc を消す
  -specs=C:/devkitPro/libnx/switch.specs -march=armv8-a+crc+crypto -mtune=cortex-a57
  -mtp=soft -fPIE -LC:/devkitPro/libnx/lib -LC:/devkitPro/portlibs/switch/lib
  -lnx -Wl,--start-group -lgcc -lg -lc -lsysbase -Wl,--end-group   <-- ★ build.rs が追加
```

### B. クリーンビルドのログ（抜粋）

```text
==> devkitPro
    devkitPro  = C:/devkitPro
    DEVKITPRO = C:/devkitPro
==> Rust toolchain
    rustc 1.101.0-nightly (b6eeb93c9 2026-10-18)
==> cargo clean
     Removed 93 files, 90.0MiB total
==> cargo build (release) for aarch64-nintendo-switch-libnx
   Compiling compiler_builtins v0.1.160
   Compiling core v0.0.0
   Compiling mhnx-switch v0.1.0
   Compiling alloc v0.0.0
    Finished `release` profile [optimized] target(s) in 14.49s
==> Link (cargo route)
==> nacptool
==> elf2nro
==> Verify
    NroStart  : unused=0x14000020 mod_offset=0xF0 marker=HOMEBREW
    NroHeader : magic=NRO0 size=0x34000 (212992 bytes)
    file size : 235717 bytes
    mod0      : MOD0 @ 0xF0
==> Stage -> C:\anyWorkspace\nx\mhxx\mhnxeditor\dist
    C:\anyWorkspace\nx\mhxx\mhnxeditor\dist\mhnxeditor.nro
    235717 bytes
OK: mhnxeditor.nro built.
```

### C. コマンド早見表

```powershell
rustup toolchain install nightly --profile minimal --component rust-src   # 初回のみ
cd C:\anyWorkspace\nx\mhxx\mhnxeditor\switch
pwsh -File .\build.ps1                                                   # ビルド〜dist 配置

# NRO マジック確認（0x10 を見る点に注意）
$b = [IO.File]::ReadAllBytes('..\dist\mhnxeditor.nro')
[Text.Encoding]::ASCII.GetString($b, 16, 4)   # -> NRO0
[Text.Encoding]::ASCII.GetString($b, 8, 8)    # -> HOMEBREW
```

### D. 参考リンク

* libnx: https://github.com/switchbrew/libnx （`include/switch/nro.h`, `switch.specs`, `switch.ld`）
* devkitPro アプリケーションテンプレート: `C:\devkitPro\examples\switch\templates\application`
* rustc のビルトイン target: `rustc -Z unstable-options --print target-spec-json --target aarch64-nintendo-switch-freestanding`
* aarch64-switch-rs/cargo-nx: https://github.com/aarch64-switch-rs/cargo-nx （git からのみ導入可）
