# Switch 実機 日本語テキスト描画サブシステム 実装メモ

対象: MHXX MHNX Editor (`C:\anyWorkspace\nx\mhxx\mhnxeditor`)
担当: Switch のテキスト描画（pl:u 共有フォント + stb_truetype + framebuffer）
作業日: 2026-10-19 / 実機なし（ビルド・リンク・ホスト側単体検証まで）

---

## 0. 結論 (TL;DR)

| 項目 | 結果 |
| --- | --- |
| 日本語描画 | pl:u の `PlSharedFontType_Standard` を stb_truetype でラスタライズし、RGBA8888 のリニア framebuffer に描画する実装を追加 |
| C ラッパ | `switch/tools/font_shim.c`（stb_truetype の構造体は Rust に写さない）+ `stb_truetype.h` v1.26 |
| ビルド | `build.ps1` 成功（NRO 485,573 バイト、`NRO0` / `HOMEBREW` 検証済み）|
| 公開 API | `Canvas::new/width/height/clear/text/rect/present` はスタブと同一。追加で `line_height` / `text_width` / `probe_shared_font` / `canvas_self_test` |
| 単体検証 | (1) C シムをホスト gcc で実フォントに対して実行し「フォント → グリフ → 非ゼロビットマップ」を確認 (2) 実機用 `text::probe_shared_font()` を example から呼べる形で用意 |
| 未検証 | 実機での pl:u 取得・framebuffer 排他・描画品質（第 9 節）|

> サイズは同時並行で編集されている `editor.rs` / `swkbd.rs` / `mhnx-core` の内容に依存する。
> 合否は `build.ps1` の自動検証（NRO0 マジック・サイズ整合）で判定すること。

---

## 1. 変更・追加したファイル

| ファイル | 内容 |
| --- | --- |
| `switch/src/text.rs` | スタブを本実装に置換（Canvas / フォント / グリフキャッシュ / 描画 / プローブ）|
| `switch/tools/font_shim.c` | stb_truetype の薄い C API（create/ascent/descent/line_gap/cff/glyph/destroy）|
| `switch/tools/stb_truetype.h` | 公式 master から取得した v1.26（199,192 バイト）|
| `switch/tools/font_shim_host_test.c` | ホスト PC で C シムを検証するテスト（NRO には入らない）|
| `switch/build.rs` | `font_shim.c` を devkitA64 gcc でコンパイルし `libfont_shim.a` としてリンク。`-lm` を既存グループ内に追加 |
| `switch/examples/text_probe.rs` | 実機用の確認コード（probe → Canvas 描画 → present）|
| `docs/switch_text.md` | 本ドキュメント |

触っていないファイル: `lib.rs` / `editor.rs` / `app.rs` / `sys.rs` / `swkbd.rs`。
(`editor.rs` は別エージェントが並行編集しており、既に `crate::text::Canvas` を Painter 経由で使用している。)

---

## 2. パイプライン

~~~
pl:u shared font  (plInitialize / plGetSharedFontByType / plExit)
  |  PlFontData.address / .size (sharedmem 上の TrueType/OpenType)
  v
stb_truetype      (switch/tools/font_shim.c, C の 7 関数だけが境界)
  |  stbtt_GetGlyphBitmapBox -> stbtt_MakeGlyphBitmap (8bit coverage, stride = w)
  v
BTreeMap<u32, CachedGlyph>   (char 単位。サイズは 24px 固定なので key は char のみ)
  |  alpha blend (coverage * Color の alpha)
  v
libnx framebuffer  (nwindowGetDefault / framebufferCreate / MakeLinear /
                    Begin / End / Close, PIXEL_FORMAT_RGBA_8888, 2 buffers)
~~~

`Canvas::new()` は「pl 初期化 → 共有フォント取得 → C シム初期化 → framebuffer 作成 → MakeLinear」の
いずれかが失敗したら `None` を返す（呼び出し側は ASCII console にフォールバックする設計）。
`probe_shared_font()` は framebuffer に触らず pl + stb だけで完結するので、
「フォント取得 → グリフ取得 → ビットマップ非ゼロ」のロジックを単体で検証できる。

---

## 3. libnx バインディング（実ヘッダの行番号）

libnx 4.12.0 / devkitA64 gcc 15.2.0。行番号はこの環境の
`C:\devkitPro\libnx\include` の実ファイルで確認したもの。

| 宣言 | ヘッダ:行 | 備考 |
| --- | --- | --- |
| `PlServiceType_User = 0` | `services/pl.h:12` | pl:u。`System = 1` は 16.0.0+ で共有フォント不可(pl.h:13) |
| `PlSharedFontType_Standard = 0` | `services/pl.h:18` | 日本/米/欧。かな・常用漢字を含む |
| `typedef struct PlFontData` | `services/pl.h:28-33` | u32 type; u32 offset; u32 size; void* address; |
| `plInitialize(PlServiceType)` | `services/pl.h:36` | `Service` セッションを張る |
| `plExit(void)` | `services/pl.h:39` | 共有メモリの unmap もここ |
| `plGetSharedFontByType(PlFontData*, type)` | `services/pl.h:48` | address は sharedmem 内を指す |
| `typedef struct Framebuffer` | `display/framebuffer.h:45-56` | 80 バイト（後述）|
| `framebufferCreate(fb, win, w, h, fmt, num_fbs)` | `display/framebuffer.h:75` | ここで NV map とバッファを登録 |
| `framebufferMakeLinear(fb)` | `display/framebuffer.h:78` | 線形シャドウバッファを確保 |
| `framebufferClose(fb)` | `display/framebuffer.h:81` | 2 回呼ばないこと |
| `framebufferBegin(fb, u32* out_stride)` | `display/framebuffer.h:94` | stride は「バイト」単位 |
| `framebufferEnd(fb)` | `display/framebuffer.h:104` | 線形 → ブロックリニア変換とキュー投入 |
| `RGBA8(r,g,b,a)` | `display/framebuffer.h:12` | 0xAABBGGRR と同値（リトルエンディアン）|
| `NWindow` | `display/native_window.h:17-39` | Rust 側では不透明ポインタとして扱う |
| `nwindowGetDefault()` | `display/native_window.h:56` | VI 初期化は起動時に libnx が実施 |
| `nwindowGetDimensions(nw, &w, &h)` | `display/native_window.h:90` | 通常 1280x720 |
| `typedef struct NvMap` | `nvidia/map.h:4-12` | 32 バイト |
| `NvKind` | `nvidia/types.h` (末行 251) | C enum = 32bit |
| `PIXEL_FORMAT_RGBA_8888 = 1` | `display/types.h:11` | |
| `consoleInit(PrintConsole*)` | `runtime/devices/console.h:170` | 既定ウィンドウに自前 framebuffer を作る（第 8 節）|

---

## 4. 構造体レイアウト検証

Rust 側は `#[repr(C)]` + `const _: () = { assert!(size_of...) }` で固定している。
値は推測ではなく、同じ devkitA64 gcc で C の `_Static_assert` をコンパイルして確認した。

検証用 C（一時ファイル。結果は gcc exit=0 で全アサート通過）:

~~~c
#include <switch/services/pl.h>
#include <switch/display/framebuffer.h>
#include <switch/nvidia/map.h>
#include <stddef.h>

_Static_assert(sizeof(PlFontData) == 24, "PlFontData size");
_Static_assert(offsetof(PlFontData, address) == 16, "PlFontData.address");
_Static_assert(sizeof(NvMap) == 32, "NvMap size");
_Static_assert(offsetof(NvMap, cpu_addr) == 16, "NvMap.cpu_addr");
_Static_assert(offsetof(NvMap, kind) == 24, "NvMap.kind");
_Static_assert(sizeof(Framebuffer) == 80, "Framebuffer size");
_Static_assert(offsetof(Framebuffer, win) == 0, "win");
_Static_assert(offsetof(Framebuffer, map) == 8, "map");
_Static_assert(offsetof(Framebuffer, buf) == 40, "buf");
_Static_assert(offsetof(Framebuffer, buf_linear) == 48, "buf_linear");
_Static_assert(offsetof(Framebuffer, stride) == 56, "stride");
_Static_assert(offsetof(Framebuffer, num_fbs) == 68, "num_fbs");
_Static_assert(offsetof(Framebuffer, fb_size) == 72, "fb_size");
_Static_assert(offsetof(Framebuffer, has_init) == 76, "has_init");
~~~

実行したコマンド:

~~~
C:\devkitPro\devkitA64\bin\aarch64-none-elf-gcc.exe -c \
  -I C:/devkitPro/libnx/include -I C:/devkitPro/portlibs/switch/include \
  -o %TEMP%\mhnx_layout_check.o %TEMP%\mhnx_layout_check.c
# gcc exit=0
~~~

Rust 側の対応する assert は `switch/src/text.rs` の `const _: () = { ... }` にある。
libnx 側のレイアウトが変われば Rust のビルドが落ちる（サイレント破壊にならない）。

---

## 5. 描画仕様

- **色**: `Color = u32` は 0xAABBGGRR。リトルエンディアンではメモリ上 R,G,B,A となり、
  `RGBA8(r,g,b,a)`（framebuffer.h:12）と一致するため変換は行わない。
  `text()` は alpha を不透明度として使い、`clear()` / `rect()` は値をそのまま書く（0xFF を推奨）。
- **座標**: `text(x, y, ...)` の y は「行の上端」。ベースラインは `y + ascent`。
  1 行のみ。`\n` / `\r` は行を終了する（折り返しなし）。
- **文字**: UTF-8 を `char` 単位でデコードし、BMP 外（絵文字など）は `'?'`。
  フォントに存在しない文字も `'?'` にフォールバックし、それも無ければ advance 12px で空白。
  0x20 未満の制御文字は空白扱い。
- **クリッピング**: すべて画面内にクリップ。
- **ブレンド**: `dst = (src*cov + dst*(255-cov)) / 255`（cov はカバレッジ×色 alpha）。
- **グリフキャッシュ**: `BTreeMap<u32, CachedGlyph>`。`no_std + alloc` には `HashMap` が無いため
  （hashbrown を足さない方針）BTreeMap を使用。24px の漢字は約 500 バイトなので
  2 MiB（約 3000 文字）を超えたら全消しする。サイズは `FONT_PIXEL_HEIGHT = 24.0` 固定。
- **フレーム**: `begin_frame()` が最初の描画呼び出しで `framebufferBegin`、
  `present()` が `framebufferEnd`。Begin/End は必ず対になる。
  MakeLinear のシャドウバッファはフレーム間で内容が残るが、**それを前提にせず毎フレーム
  clear → 描画 → present する**こと（ダブルバッファなので前フレームの見た目は保証されない）。
- **追加 API**: `line_height()`（行間）、`text_width()`（描画幅の測定）、
  `probe_shared_font()`（実機用プローブ）、`canvas_self_test()`（example 用の描画+ピクセル数チェック）。

---

## 6. build.rs と C のコンパイル

`build.rs` は既存のリンク行を維持したまま、次を追加した:

1. `tools/font_shim.c` を devkitA64 gcc でコンパイル:
   ~~~
   aarch64-none-elf-gcc -c -O2 -g0 -fno-strict-aliasing \
     -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft \
     -fPIE -ffunction-sections -fdata-sections \
     -I switch/tools -o $OUT_DIR/font_shim.o switch/tools/font_shim.c
   ~~~
2. `aarch64-none-elf-ar rcs $OUT_DIR/libfont_shim.a font_shim.o`
3. リンク引数 `-lnx` の直後に `$OUT_DIR/libfont_shim.a` を挿入。
4. 既存の `-Wl,--start-group -lgcc -lg -lc -lsysbase -Wl,--end-group` はそのまま、
   `-lc` の直後に `-lm` を **グループ内に** 追加した。

`-lm` が必要な理由: stb_truetype は SDF/GPOS 系のコードで
`sqrt/pow/cos/acos/fmod` を参照する（コンパイル単位に残るため、未使用でもリンク時に解決が要る）。
`-ffunction-sections` + `--gc-sections`（switch.specs）で実際には大半が捨てられ、
最終 ELF に残る stb_truetype のコードは約 17 KB（`nm -S` の合計）。
`-lm` は新しいライブラリを足すだけで、既存のグループ構造・`-lnx` の順序は変えていない。

`-ffunction-sections` を付けているので、`--gc-sections` が効く限り未使用の SDF コードは
リンクされない。`font_shim.o` の undefined symbol は
`acos cos fmod free malloc memcpy memset pow sqrt strlen` のみで、すべてグループ内の
`-lc/-lm` で解決される（libnx のシンボルは参照しない）。

`stb_truetype.h` は
`https://raw.githubusercontent.com/nothings/stb/master/stb_truetype.h`
から取得した v1.26（199,192 バイト）。取得後は再現性のためリポジトリに置いてある
（build.rs はネットワークにアクセスしない）。

---

## 7. 検証結果

### 7.1 NRO ビルド（必須）

~~~
pwsh -File C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\build.ps1
# ==> Verify
#     NroStart  : unused=0x14000020 mod_offset=0xF0 marker=HOMEBREW
#     NroHeader : magic=NRO0 size=0x71000 (462848 bytes)
#     file size : 485573 bytes
#     mod0      : MOD0 @ 0xF0
# OK: mhnxeditor.nro built.
~~~

cargo ルート（rustc が ELF まで生成）で成功。`dist/mhnxeditor.nro` は 485,573 バイト。
（サイズは他エージェントの editor/swkbd/mhnx-core 変更を含む。以前の 243,909 バイトは
描画サブシステム実装前の構成。）

ELF にシムと libnx のシンボルが入っていることも確認:

~~~
$ aarch64-none-elf-nm mhnxeditor.elf | grep -E "mhnx_font_create|mhnx_font_glyph|plGetSharedFontByType|framebufferBegin"
000000000001a190 T framebufferBegin
000000000002b890 T mhnx_font_create
000000000002bb80 T mhnx_font_glyph
00000000000195f0 T plGetSharedFontByType
~~~

### 7.2 確認用 example のビルド

`switch/examples/text_probe.rs` は `app.rs` を触らずに用意した実機用確認コード。
`cargo build --release` ではビルドされない（build.ps1 の成果物に影響しない）。

~~~
cd C:\anyWorkspace\nx\mhxx\mhnxeditor\switch
$env:DEVKITPRO='C:/devkitPro'
$env:DEVKITA64='C:/devkitPro/devkitA64'
$env:PATH='C:\devkitPro\devkitA64\bin;C:\devkitPro\tools\bin;' + $env:PATH
cargo build --release --example text_probe
# Finished release profile ...  => target/.../release/examples/text_probe (ELF)
~~~

example 内の処理:

1. `probe_shared_font(SAMPLE)` : pl:u 取得 → 各文字のグリフ取得 → coverage ビットマップの非ゼロ数を集計し、
   font address/size、ascent/descent、CFF かどうか、inked/blank/missing、非ゼロバイト数、
   最初にインクが出た文字を console に表示する。**framebuffer には触らない**。
2. `consoleExit()` してから `Canvas::new()` : 実際の描画（日本語 + ASCII + カラーバー）を行い、
   バックバッファの非背景ピクセル数を数えて `present()`。5 秒間そのまま表示。
3. Canvas を drop して console を再初期化し、結果を表示。

実機で動かす場合の NRO 化（任意）:

~~~
nacptool --create text_probe "mhnx project" 0.1.0 text_probe.nacp
elf2nro target/.../examples/text_probe text_probe.nro \
  --icon=C:/devkitPro/libnx/default_icon.jpg --nacp=text_probe.nacp
# 実測: text_probe.nro = 256,197 バイト, magic NRO0
~~~

### 7.3 ホスト側 C シム単体テスト

実機なしで stb_truetype 経路を検証するため、`switch/tools/font_shim_host_test.c` を
Windows の gcc（devkitPro msys2 同梱）でビルドし、実フォントに対して実行した。

~~~
gcc -O2 -I switch/tools -o font_shim_host_test \
    switch/tools/font_shim_host_test.c switch/tools/font_shim.c
./font_shim_host_test C:/Windows/Fonts/msgothic.ttc \
                      C:/Windows/Fonts/YuGothM.ttc \
                      C:/Windows/Fonts/NotoSansJP-VF.ttf
~~~

結果（すべて PASS）:

| フォント | ascent/descent | 17 文字中の inked | non-zero coverage bytes |
| --- | --- | --- | --- |
| msgothic.ttc (MS Gothic) | 21 / -4 | 14 (残り 3 は空白文字) | 1,816 |
| YuGothM.ttc (Yu Gothic) | 19 / -6 | 14 | 1,399 |
| NotoSansJP-VF.ttf (Noto Sans JP) | 20 / -5 | 14 | 868 |

各文字で box / offset / advance / 非ゼロ画素数も表示される。例:
`U+30AF box=18x21 off=2,-19 adv=24 nonzero=145`

このテストで **C シムの戻り値仕様のバグを発見して修正した**（第 8.1 節）。

---

## 8. ハマりどころ

### 8.1 グリフ取得 API の戻り値（発見済み・修正済み）

当初 `mhnx_font_glyph()` の「サイズ問い合わせ」の戻り値を `1` にしていたため、
呼び出し側（Rust / ホストテスト）が `1` バイトだけ確保して 2 回目を呼び、
常に「バッファ不足」で空グリフになっていた。ホストテストで `0 inked / 14 missing` になり発覚。
現在は **必要バイト数 `w*h` を返す**仕様に修正し、0 = 描画済み（空白グリフ含む）、
負値 = エラー、正値 = 必要サイズ、と定義した。`text.rs` の `ensure_glyph()` はこの仕様に従う。

### 8.2 console と Canvas は同じ既定ウィンドウを取り合う（重要）

libnx の `consoleInit()` は内部で `nwindowGetDefault()` + `framebufferCreate()`
を呼ぶ（libnx.a の console.o の再配置参照で確認）。したがって
**console が初期化されたまま `Canvas::new()` を呼ぶと framebuffer が衝突し、`None` になる**
（または両者が同じウィンドウを奪い合う）。必ず:

~~~
consoleInit する前に Canvas::new() を試す
  Canvas が Some  -> consoleInit しない
  Canvas が None  -> consoleInit して ASCII で動かす
Canvas を drop してから consoleInit する
~~~

現時点の `editor.rs` は `run()` の先頭（`sys::consoleInit(...)`）で console を初期化し、
その後に `Painter::new()` → `Canvas::new()` を呼んでいるため、
**この順序のままだと日本語描画に切り替わらない可能性が高い**（editor.rs 担当側の修正が必要）。

### 8.3 pl:u と pl:s

`PlServiceType_System`（pl:s）は firmware 16.0.0 以降で共有フォントを提供しない（pl.h:13 のコメント）。
必ず `PlServiceType_User`（pl:u）を使う。

### 8.4 plExit の順序

`PlFontData.address` は pl:u の共有メモリを指す。stb_truetype はそのメモリを
ポインタで保持するので、**フォント破棄（mhnx_font_destroy）→ plExit の順**を守る。
`Canvas::drop` は framebufferClose → フォント破棄 → plExit の順で処理している。

### 8.5 stride はバイト単位

`framebufferBegin()` の `out_stride` はバイト数（ピクセル数ではない）。
ピクセル位置は `y * stride + x * 4`。1280 幅でもアラインメントで stride が広がる可能性があるので
`width * 4` を決め打ちしない。

### 8.6 no_std に HashMap は無い

`alloc::collections` には BTreeMap しかない。hashbrown を追加せず BTreeMap で実装した
（数百文字のキャッシュでは性能差は無視できる）。

### 8.7 stb_truetype の構造体は Rust に写さない

`stbtt_fontinfo` はバージョン間でフィールドが変わり得る。C 側に閉じ込め、
Rust とは `void*` ハンドルと int だけでやり取りする（font_shim.c のコメント参照）。

### 8.8 swkbd（ライブラリ Applet）との関係

`swkbdShow()` はライブラリ Applet を前面に出す。呼び出し中は Canvas の framebuffer は
表示されず、戻った後に再描画が必要になることがある。editor.rs 側は swkbd から戻った際に
`need_draw = true` にするのが安全（text.rs 側では検知できない）。
また、ライブラリ Applet の起動は既定ウィンドウのバッファを一時的に手放す必要がある場合があるため、
実機で swkbd と Canvas の併用は要確認（第 9 節）。

### 8.9 stb_truetype の数値フック

`STBTT_assert` は no-op にし、`STBTT_ifloor/iceil/fabs` は自前実装に差し替えた
（ホットパスから libm を外すため）。`sqrt/pow/cos/acos/fmod` は SDF 系のみで、
`-lm` をグループ内に足して解決している。

---

## 9. 実機で未検証の点

1. **pl:u の共有フォント取得**: 実機の firmware で `plGetSharedFontByType(Standard)` が
   成功するか、アプレットモード（hbmenu 起動）で pl:u にアクセスできるか。
   → `text_probe` の probe 表示で確認できる（None なら失敗）。
2. **共有フォントのアウトライン形式**: TrueType glyf か CFF か。probe が `cff=` を表示する。
   stb_truetype は両対応だが、CFF のラスタライズ品質は未確認。
3. **consoleInit との排他**: Canvas::new() が framebufferCreate で確実に失敗するか、
   あるいはバッファを奪い合うかは実機未確認（第 8.2 節の順序を守る前提）。
4. **描画品質**: ヒンティングなし・グレースケール AA。24px で可読か、行間 22px（editor 側）で
   詰まりすぎないかは目視確認が必要。
5. **性能**: 初回グリフ生成（stb 呼び出し）と 2 回目以降（キャッシュ）のフレーム時間は未計測。
   キャッシュ後は 1280x720 の clear + 数十行のブレンドで 60fps に収まる見込み。
6. **swkbd 併用**: ライブラリ Applet 起動→復帰後の再描画と、framebuffer の所有権。
7. **ドック/アンドック**: nwindow の寸法は通常 1280x720 固定。解像度変更や
   `nwindowSetDimensions` を呼ぶケースは未対応。
8. **失敗時の後始末**: `framebufferCreate` 失敗経路では `framebufferClose` を呼んでいない
   （半初期化状態で close するリスクを避けた）。プロセス終了で回収される前提。
9. **カーニング**: stb_truetype のカーニングは使っていない（日本語は等幅前進が多いので実害は小さい想定）。
10. **フォントサイズ切替**: 24px 固定。複数サイズが必要になったら C シムをサイズごとに
    create するか、キーに (char, size) を持つ設計に拡張する。

---

## 10. 参考コマンド

~~~powershell
# 本体ビルド（NRO + ヘッダ検証）
pwsh -File C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\build.ps1

# example（実機確認用）
cd C:\anyWorkspace\nx\mhxx\mhnxeditor\switch
$env:DEVKITPRO='C:/devkitPro'; $env:DEVKITA64='C:/devkitPro/devkitA64'
$env:PATH='C:\devkitPro\devkitA64\bin;C:\devkitPro\tools\bin;' + $env:PATH
cargo build --release --example text_probe

# C シムのホストテスト
gcc -O2 -I switch/tools -o font_shim_host_test switch/tools/font_shim_host_test.c switch/tools/font_shim.c
./font_shim_host_test C:/Windows/Fonts/msgothic.ttc

# 構造体レイアウトの C 側アサート（第 4 節のファイルを用意して）
C:\devkitPro\devkitA64\bin\aarch64-none-elf-gcc.exe -c \
  -I C:/devkitPro/libnx/include -I C:/devkitPro/portlibs/switch/include \
  -o mhnx_layout_check.o mhnx_layout_check.c
~~~
