# Switch swkbd (システムソフトウェアキーボード) 実装メモ

対象: `switch/src/swkbd.rs` (本実装) / `switch/src/lib.rs` の `pub mod swkbd;` は変更なし。

`mhnxeditor` の実機エディタでキャラクター名・クエスト名・シャウトアウトなどを入力するため、
Nintendo Switch のシステムキーボード (LibraryApplet `swkbd`) を libnx 経由で呼び出す。
日本語 (かなフリック + 漢字変換) も ASCII も同じ経路で入力できる。

---

## 1. 使用する libnx API とヘッダ該当行

ヘッダ: `C:/devkitPro/libnx/include/switch/applets/swkbd.h` (1027 行, libnx 4.12.0)

| 関数 / 型 | ヘッダ行 | 用途 |
|---|---|---|
| `typedef struct { ... } SwkbdConfig;` | 174–187 | 設定 + 作業バッファ。実装側で完全ミラー |
| `Result swkbdCreate(SwkbdConfig* c, s32 max_dictwords);` | 349 | 初期化。`workbuf` を確保 (max_dictwords=0 → ユーザ辞書なし) |
| `void swkbdClose(SwkbdConfig* c);` | 355 | `workbuf` を free して 0 埋め |
| `void swkbdConfigMakePresetDefault(SwkbdConfig* c);` | 363 | 既定プリセット (QWERTY / カーソル末尾 / Return 有効 / 背景ぼかし) |
| `void swkbdConfigSetHeaderText(SwkbdConfig* c, const char* str);` | 416 | 見出し (UTF-8, NUL 終端) |
| `void swkbdConfigSetGuideText(SwkbdConfig* c, const char* str);` | 432 | ガイド文 (カーソル位置 0 のときだけ表示) |
| `void swkbdConfigSetInitialText(SwkbdConfig* c, const char* str);` | 439 | 初期文字列 (workbuf に UTF-16 で格納) |
| `Result swkbdShow(SwkbdConfig* c, char* out_string, size_t out_string_size);` | 587 | 実行。戻るまでブロックし、UTF-8 を書き込む |
| `static inline swkbdConfigSetType / SetStringLenMax` | 469 / 505 | ヘッダ内 inline (シンボル無し) → Rust 側はフィールド直接書き |
| `AppletType appletGetAppletType(void);` | `services/applet.h:387` | アプリケーションモード判定 |

列挙値: `SwkbdType_Normal` = 0 (`swkbd.h:21`) / `SwkbdType_QWERTY` = 2 (22 行) /
`SwkbdType_All` = 8 / `AppletType_Application` = 0, `AppletType_SystemApplication` = 4,
`AppletType_LibraryApplet` = 2 (`services/applet.h:24-31`)。
`SwkbdConfig*` を受け取る C 関数は `swkbdCreate / swkbdConfigMakePreset* / swkbdConfigSet{Ok,Left,Right,Header,Sub,Guide,Initial,Dictionary,CustomizedDictionaries,TextCheckCallback} / swkbdShow / swkbdClose` のみが
`libnx.a` に実在する (`aarch64-none-elf-nm libnx.a` で確認)。それ以外の `swkbdConfigSet*` は
`static inline` なので、Rust 側で同じフィールド書きを実装している。

---

## 2. 構造体レイアウトの実測値

**方法**: `sizeof` / `offsetof` ごとに 1 個のグローバル変数を定義した C のプローブを、
ビルドと同じヘッダ・同じコンパイラ (devkitA64 `aarch64-none-elf-gcc 15.2.0`) でコンパイルし、
`nm --print-size` で値を読み出した (計算値ではなく実測値)。

`$ gcc -c -std=gnu11 -O1 -D__SWITCH__ -march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE -I C:/devkitPro/libnx/include -o probe.o probe.c`
`$ aarch64-none-elf-nm --print-size --size-sort probe.o`

### サイズ / アラインメント (実測)

| 型 | sizeof (hex) | sizeof (dec) | align |
|---|---|---|---|
| `SwkbdArgCommon` | 0x3D4 | 980 | 4 |
| `SwkbdArgV0` | 0x3E8 | 1000 | 8 |
| `SwkbdArgV7` | 0x408 | 1032 | 8 |
| `SwkbdArgVB` (未使用・参考) | 0x4D0 | 1232 | 8 |
| `SwkbdCustomizedDictionarySet` (`NX_PACKED`) | 0xCE | 206 | 1 |
| `SwkbdDictWord` (未使用・参考) | 0x64 | 100 | 1 |
| **`SwkbdConfig`** | **0x4F0** | **1264** | **8** |

### `SwkbdConfig` のオフセット (実測)

| フィールド | オフセット |
|---|---|
| `arg` (`SwkbdArgV7`) | 0x000 |
| `workbuf` | 0x408 |
| `workbuf_size` | 0x410 |
| `max_dictwords` | 0x418 |
| `customizedDictionarySet` | 0x41C (packed なので 4 バイト境界のまま) |
| `unkFlag` | 0x4EA |
| `trigger` | 0x4EB |
| `version` | 0x4EC |
| `arg.arg.textCheckCb` | 0x3E0 |
| `arg.textGrouping` | 0x3E8 |
| `arg.arg.arg.type` | 0x000 |
| `arg.arg.arg.keySetDisableBitmask` | 0x01C |
| `arg.arg.arg.headerText` | 0x024 |
| `arg.arg.arg.guideText` | 0x1A8 |
| `arg.arg.arg.stringLenMax` | 0x3AC |
| `arg.arg.arg.textDrawType` | 0x3B8 |
| `arg.arg.arg.initialStringOffset` | 0x3C0 |
| `arg.arg.arg.initialStringSize` | 0x3C4 |
| `customizedDictionarySet.buffer_size / .entries / .total_entries` | 0x08 / 0x10 / 0xCC (パック時) |

### libnx 本体との突き合わせ (逆アセンブリによる独立検証)

`libnx.a` から `swkbd.o` を取り出して `objdump -d` で読んだ結果、上の実測値と一致する:

* `swkbdCreate`: 先頭で `memset(c, 0, 0x4F0)` → **SwkbdConfig = 0x4F0**。直後に `c+0x4EC` を
  version として読み (実測 `version` = 0x4EC)、`c+0x408` に `workbuf`、`c+0x410` に
  `workbuf_size` を書く。`max_dictwords = 0` のとき `workbuf_size = 0x1000` (4096) で
  `__libnx_aligned_alloc(0x1000, 4096)` を呼ぶ (= 4096 バイト境界)。
* `swkbdClose`: `x0+0x408` を `__libnx_free` してから `memset(c, 0, 0x4F0)`。
* `swkbdConfigMakePresetDefault`: `memset(c, 0, 0x3E8)` (textCheckCb までクリア) のあと
  `c+0=2` (SwkbdType_QWERTY), `c+0x20=1` (initialCursorPos), `c+0x3B8=1` (古い sysver のみ Box),
  `c+0x3BC=1` (returnButtonFlag), `c+0x3BE=1` (blurBackground)。
* `swkbdConfigSetHeaderText`: `utf8_to_utf16(&c->headerText(0x24), str, 64)`。
* `swkbdConfigSetGuideText`: `utf8_to_utf16(&c->guideText(0x1A8), str, 256)`。
* `swkbdConfigSetInitialText`: `dst = c->workbuf + 0x14`, `len = 249`,
  成功時 `initialStringOffset(0x3C0) = 20`, `initialStringSize(0x3C4) = 2 * units`。
* `swkbdShow`: `out_string == NULL || out_string_size == 0` を `0x1759` (BadInput) で拒否。
  `appletCreateLibraryApplet(holder, 0x11 /* swkbd */, 0 /* AllForeground */)` を呼び、version に応じて
  `SwkbdArgV7` 0x408 バイト or スタック上の `SwkbdArgVB` を push。`workbuf` は
  TransferMemory storage として渡される。アプレットから UTF-16 を最大 2004 バイト読み、
  `utf16_to_utf8(out_string, tmp, out_string_size - 1)` で変換して NUL 終端する。

---

## 3. const assert による検証 (ビルドで強制)

`swkbd.rs` の `const _: () = { ... }` ブロックが、上の実測値をそのまま期待値として
`size_of` / `align_of` / `offset_of!` を検証する (合計 34 個)。
libnx 側が変われば **コンパイルが落ちる**。検証内容:

* `SwkbdArgCommon`: size 980 / align 4 + オフセット 14 個 (0x00, 0x1C, 0x20, 0x24, 0xA6, 0x1A8, 0x3AC,
  0x3B0, 0x3B8, 0x3BC, 0x3BE, 0x3C0, 0x3C4, 0x3D0)
* `SwkbdArgV0`: size 1000 / align 8 / `pad_x3d1` = 0x3D4 / `textCheckCb` = 0x3E0
* `SwkbdArgV7`: size 1032 / align 8 / `textGrouping` = 0x3E8
* `SwkbdCustomizedDictionarySet`: size 206 / align 1
* `SwkbdConfig`: size **1264** / align **8** / `workbuf` = 0x408 / `workbuf_size` = 0x410 /
  `max_dictwords` = 0x418 / `customizedDictionarySet` = 0x41C / `unkFlag` = 0x4EA /
  `trigger` = 0x4EB / `version` = 0x4EC

**assert が実際に評価されていることの確認** (ビルドが通るだけでは不十分なので、意図的に値を壊して確認):

```text
$ rustc --edition 2021 --crate-type lib -o mirror.rlib mirror.rs   # swkbd.rs の構造体+assert を抜き出したもの
mirror-ok exit=0
$ rustc ... mirror_bad.rs   # size_of::<SwkbdConfig>() == 1264 を 1265 に改変
error[E0080]: evaluation panicked: assertion failed: size_of::<SwkbdConfig>() == 1265
```

= 期待値を 1 バイト変えるだけで必ずコンパイルエラーになる (`const _` は常に評価される)。

---

## 4. 公開 API (シグネチャは不変)

```rust
pub fn input_text(header: &str, initial: &str, max_len: usize) -> Option<String>;
pub fn is_available() -> bool;   // 追加 (タスク 5 の安全策)
```

* `input_text`: 成功時 `Some(UTF-8 文字列)`。空文字で OK した場合も `Some("")`
  (キャンセルと区別できるようにするため)。
* キャンセル / 起動失敗 / 割り当て失敗 / UTF-8 でない戻り値 → `None`。呼び出し側は
  ASCII フォールバックに落とせる。
* `header` は 64、`initial` は `min(max_len, 249)`、`max_len` は 248 を上限に
  **UTF-16 コードユニット数** で文字境界切り詰め (バイト数でも `char` 数でもない。swkbd 自身の数え方に合わせる)。
  `max_len == 0` は「上限指定なし」= 248。
* 出力バッファは `4 * chars + 16` バイト (BMP 3 バイト/ユニット、サロゲートペア 2 バイト/ユニット、
  libnx は `size - 1` しか使わない)。
* `swkbdCreate` 成功後は `swkbdShow` の直後に無条件で `swkbdClose` を呼ぶ
  (間に早期 return を置かない)。`swkbdCreate` が失敗した場合のみ close しない (workbuf は NULL で
  アプレットも起動していないため)。`swkbdClose` は `workbuf` を free + 0 埋めするので
  2 重呼び出しでも安全 (今回のコードでは 1 回だけ)。
* `is_available` は `appletGetAppletType()` が `AppletType_Application / SystemApplication` の
  ときだけ true。`input_text` も最初にこれを確認し、アプレットモードでは libnx のアプレット機構に
  触らず即 `None` を返す。
* キーボード種別は、プリセット既定の `SwkbdType_QWERTY` を `SwkbdType_Normal` (0) に上書きしている。
  日本語のかなフリック入力と地球儀キー (言語切替) を使うため。QWERTY 固定にしたい場合は
  `swkbd.rs` の `SWKBD_TYPE_NORMAL` を 2 に変える。

---

## 5. 挙動の根拠 (逆アセンブリで確認した Result 値)

| Result | 意味 | 発生箇所 |
|---|---|---|
| `0x00000000` | 成功 | `swkbdShow` が UTF-8 を書き込み済み |
| `0x00001759` | `MAKERESULT(Module_Libnx=0x159, LibnxError_BadInput=11)` | `out_string == NULL` or `size == 0` |
| `0x00000559` | `MAKERESULT(0x159, LibnxError_OutOfMemory=2)` | `swkbdCreate` の `aligned_alloc` 失敗 |
| `0x00005D59` | `MAKERESULT(0x159, LibnxError_LibAppletBadExit=46)` | **キャンセル** (exit reason 1/2)、または出力ストレージ先頭 4 バイト ≠ 0 |

つまり「キャンセル」も「アプレットが起動できない」も同じように非 0 で戻るため、実装は Result が
非 0 ならすべて `None` にしている (呼び出し側の ASCII フォールバックがそのまま効く)。

---

## 6. 制約・注意

1. **アプリケーションモード必須**。LibraryApplet は `ILibraryAppletCreator` を持つ
   `AppletType_*Application` からしか起動できない (`services/applet.h:1174` の
   `appletCreateLibraryApplet`)。アルバム起動のアプレットモードでは失敗する。
2. `input_text` は **ブロックする** (`swkbdShow` 内部で `appletHolderWaitInteractiveOut` を回す)。
   実行中に `appletMainLoop` を呼ぶ必要は無いし、呼んではいけない。メインスレッド / UI ループから
  1 箇所で呼ぶこと。`switch/src/editor.rs:399`, `switch/src/app.rs:74` のループの外で使う想定。
3. アプレット終了後は画面 (コンソールのフレームバッファ) が戻るので、呼び出し側で
   `consoleUpdate(NULL)` を 1 回呼んで描き直すのが安全 (未検証: libnx が自動で再描画するかは端末依存)。
4. 長い `header` / `guide` / `initial` は libnx 側で切り詰められる。ただし
   `utf8_to_utf16` は「切り詰めた場合、入力が生成するはずの長さ」を返す仕様
   (`runtime/util/utf.h:57-71`) で、`swkbdConfigSetHeaderText` 等はその戻り値の位置に NUL を書く。
   Rust 側で先に文字境界で切り詰めているのはこのため (超過分を渡すと固定長配列の外に書きうる)。
5. `textCheckCb` / ユーザ辞書 / カスタム辞書 / `swkbdInline*` (非同期版) は使っていない。
   `swkbdConfigMakePresetDefault` がコールバックを 0 にするので libnx からコールバックは来ない。
6. 入力上限は 248 文字 (UTF-16 コードユニット)。アプレットが返せる UTF-16 は 2004 バイト = 1002 ユニット、
   libnx の initial 変換は 249 ユニットなので、その小さい方に合わせた安全側の値。
7. 1 回の呼び出しで確保するメモリ: `SwkbdConfig` 1264 バイト (スタック) + libnx の workbuf 4096 バイト
   (アプレット側が 4KB 境界の TransferMemory として使う) + 出力バッファ最大 1008 バイト (ヒープ)。

---

## 7. 未検証点 (実機が無いため未確認)

* **実行は一切していない**。ビルド (リンク・NRO 生成) とコンパイル時アサート、逆アセンブリによる
  静的検証のみ。アプレットが実際に起動すること、日本語のかな/漢字変換が使えること、
  戻り文字列が期待どおりであることは未確認。
* `SwkbdType_Normal` でかなフリック・地球儀キーが出るかは switchbrew の列挙値の説明に基づく推定。
  実機で QWERTY しか出ない場合は `SWKBD_TYPE_NORMAL` を `SwkbdType_All` (8) に変える。
* アプレットモード判定 (`appletGetAppletType`) が実際の hbmenu 起動経路でどう返るかは未検証。
  hbmenu をアルバムから起動した場合は `AppletType_Application` になる想定 (だから swkbd が使える)。
* `utf8_to_utf16` の「len 超過時に元の長さを返す」挙動はヘッダのドキュメントと
  `swkbdConfigSetHeaderText` の逆アセンブリからの推論で、実際に境界超過を流して
  メモリ破壊が起きるかは試していない (Rust 側で先に切っているので発生しない)。
* 出力バッファ不足時に libnx が成功を返しつつ空文字列になる可能性 (逆アセンブリでは
  `utf16_to_utf8` が負を返しても Result は 0 のまま) は、バッファを 4 バイト/ユニットで
  確保しているため通常は起きないが、実機での確認は無い。
* 実機の sysver によって `swkbdCreate` の workbuf_size (0x1000 / version > 0x8000C で別値) や
  `SwkbdArgVB` 経路が変わるが、いずれも libnx 内部で吸収されるため Rust 側の分岐は無い
  (未検証: 古い sysver での動作)。
* 並行スレッドから呼んだ場合の安全性は未検証 (メインスレッド 1 箇所からの利用を前提)。

---

## 8. 再現手順

```powershell
# 1) レイアウト実測 (プローブは C:/devkitPro のヘッダをそのまま使う)
aarch64-none-elf-gcc -c -std=gnu11 -O1 -D__SWITCH__ -march=armv8-a+crc+crypto -mtune=cortex-a57 ^
    -mtp=soft -fPIE -I C:/devkitPro/libnx/include -o probe.o probe.c
aarch64-none-elf-nm --print-size --size-sort probe.o

# 2) libnx 側の実装確認
aarch64-none-elf-nm --print-size C:/devkitPro/libnx/lib/libnx.a | Select-String swkbd
aarch64-none-elf-ar x C:/devkitPro/libnx/lib/libnx.a swkbd.o
aarch64-none-elf-objdump -dr --disassemble=swkbdShow swkbd.o

# 3) ビルド (NRO まで + ヘッダ検証)
pwsh -File C:/anyWorkspace/nx/mhxx/mhnxeditor/switch/build.ps1
```

## 9. ビルド結果 (この実装で確認)

* `switch/src/swkbd.rs`: 19,297 文字 (スタブ 14 行 → 本実装)。
* `cargo build` (release, `aarch64-nintendo-switch-libnx`): 成功。`libmhnx_switch.a` に
  `input_text` (0x1F4 バイト) / `is_available` (0x18 バイト) と `U swkbdCreate, swkbdShow,
  swkbdClose, swkbdConfigMakePresetDefault, swkbdConfigSet{Header,Guide,Initial}Text, appletGetAppletType` が入る。
* リンク後の ELF で `input_text` の呼び出し順を確認:
  `appletGetAppletType → memset → swkbdCreate → swkbdConfigMakePresetDefault → swkbdConfigSetHeaderText →
  swkbdConfigSetGuideText → swkbdConfigSetInitialText → (alloc/resize) → swkbdShow → swkbdClose → from_utf8`。
  `swkbdClose` は `swkbdShow` の直後に分岐なしで実行される。
* `pwsh -File switch/build.ps1` → `OK: mhnxeditor.nro built.`
  (`dist/mhnxeditor.nro` = 485,573 バイト, `NroHeader.magic=NRO0`, `mod0` = 0xF0,
  `NroStart.marker=HOMEBREW`)。
  ※ 直前の成果物は 243,909 バイトだったが、これは並行作業中の他ファイル (editor.rs 等) の変更を含む
  リポジトリ全体のビルド結果であり、swkbd 追加分そのものではない (swkbd のコードは
  `input_text` 500 バイト + `is_available` 24 バイト + ヒープ確保のみ)。
