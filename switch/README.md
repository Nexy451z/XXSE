# mhnx-switch — Nintendo Switch (.nro) 版 mhnxeditor

MHXX MHNX Editor の Switch homebrew フロントエンド。**Rust (no_std + alloc)** で書き、
devkitPro (devkitA64 / libnx) のツールチェーンでリンクして `mhnxeditor.nro` を作る。
実機は不要で、ビルドが通ることと `NRO0` マジックを持つ NRO が生成されることを
`build.ps1` が自動検証する。

## 必要なもの

| 依存 | 確認済みバージョン | 備考 |
| --- | --- | --- |
| devkitPro (devkitA64 + libnx + tools) | libnx 4.12.0 / gcc 15.2.0 | `C:\devkitPro` を想定。別ドライブでも `-DevkitPro` / `DEVKITPRO` で指定可 |
| Rust nightly + `rust-src` | nightly 1.101.0 | tier-3 ターゲットなので `-Z build-std` が必須。無ければ build.ps1 が自動導入 |
| PowerShell 7 (pwsh) | 7.x | `build.ps1` の実行に使用 |

> 環境変数 `DEVKITPRO` が msys2 形式 (`/opt/devkitpro`) のままでも、build.ps1 が
> Windows パスへ読み替えるので問題ない。

## ビルド

```powershell
cd C:\anyWorkspace\nx\mhxx\mhnxeditor\switch
pwsh -File .\build.ps1                 # rustc がリンクまで行う既定ルート
pwsh -File .\build.ps1 -Clean          # 完全なクリーンビルド
pwsh -File .\build.ps1 -Route c        # devkitA64 gcc が Rust staticlib をリンクするルート
```

出力:

* `switch/target/aarch64-nintendo-switch-libnx/release/mhnxeditor` … ELF (route cargo)
* `switch/target/.../release/libmhnx_switch.a` … Rust staticlib (route c 用)
* `switch/target/.../release/mhnxeditor.nro` … 中間 NRO
* **`dist/mhnxeditor.nro`** … 成果物 (build.ps1 がコピー)

## 実行時の動作

1. libnx の `consoleInit(NULL)` + `printf` でテキスト表示
2. `argc/argv` を表示（argv[0] = NRO のパス）
3. SD カードプローブ
   * `fsInitialize()` → `fsOpenSdCardFileSystem()`
   * `fsFsGetFreeSpace/TotalSpace("/")`
   * `fsFsOpenDirectory("/switch/mhnxeditor")` + `fsDirRead()` で一覧表示
   * `fsFsOpenFile()/fsFileRead()` で `/switch/mhnxeditor/hello.txt` を読む
   * さらに newlib の `fopen()/fread()` でも同じファイルを読む（libnx が fsdev 経由で
     `sdmc` を stdio に繋いでいることの確認）
4. **A ボタン**で再プローブ、**＋ボタンまたは B ボタン**で終了して hbmenu に戻る

`alloc` は newlib の malloc/free を `#[global_allocator]` として使用しており、
String / Vec / format! がそのまま使える（`mhnx-core` を載せる前提の確認）。

## ファイル構成

| パス | 役割 |
| --- | --- |
| `Cargo.toml` | bin `mhnxeditor` + lib `mhnx_switch` (rlib + staticlib) |
| `rust-toolchain.toml` | nightly + rust-src を固定 |
| `.cargo/config.toml` | ターゲット JSON、`build-std`、`json-target-spec`、DEVKITPRO 既定値 |
| `targets/aarch64-nintendo-switch-libnx.json` | カスタムターゲット定義（gcc リンカを指定） |
| `build.rs` | devkitPro のパスをリンク引数に変換（switch.specs / -lnx / libc 等） |
| `src/sys.rs` | libnx + newlib の FFI 宣言（構造体レイアウトは const assert で検証） |
| `src/app.rs` | 画面表示・SD カードプローブ本体 |
| `src/main.rs` | `#![no_main]` + `#[no_mangle] extern "C" fn main` |
| `src/lib.rs` | panic handler / global allocator / `mhnx_main()` (route c 用) |
| `tools/rustmain.c` | route c 用の C エントリポイント |
| `sdcard/` | SD カードにコピーするサンプル (`/switch/mhnxeditor/hello.txt`) |
| `build.ps1` | ビルド〜NRO 生成〜検証〜dist 配置 |

## 2 つのリンク経路

* **route cargo (既定)**: `cargo build --target <json>` が rustc → devkitA64 gcc を
  リンカとして呼び、ELF を直接生成する。リンク引数は `build.rs` が
  `cargo::rustc-link-arg` で渡す。
* **route c**: `cargo build` が生成した `libmhnx_switch.a` を、devkitA64 の gcc が
  `tools/rustmain.c` と一緒にリンクする（devkitPro Makefile 方式）。

どちらも同じ内容の `mhnxeditor.nro` (235,717 バイト) を生成することを確認済み。

## トラブルシューティング

* `error: .json target specs require -Zjson-target-spec` … `.cargo/config.toml` の
  `[unstable] json-target-spec = true` が効いていない（nightly 以外で cargo を実行した）。
* `undefined reference to `printf'` など libc シンボル … rustc が付ける
  `-nodefaultlibs` の影響。`build.rs` の `-lgcc -lg -lc -lsysbase` が抜けていないか確認。
* `linker `aarch64-none-elf-gcc` not found` … `$env:PATH` に devkitA64\bin が無い。
  `build.ps1` 経由でビルドするか PATH を設定する。

詳しい調査ログ・ハマりどころは `../docs/switch_build.md` を参照。
