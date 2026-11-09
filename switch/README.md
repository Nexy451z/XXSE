# mhnx-switch — Nintendo Switch (.nro) 版 mhnxeditor

MHXX MHNX Editor の Switch homebrew フロントエンド。**Rust (no_std + alloc)** で書き、
devkitPro (devkitA64 / libnx) のツールチェーンでリンクして `XXSE.nro` を作る。
実機は不要で、ビルドが通ることと `NRO0` マジックを持つ NRO が生成されることを
`build.ps1` が自動検証する。

## 必要なもの

| 依存 | 確認済みバージョン | 備考 |
| --- | --- | --- |
| devkitPro (devkitA64 + libnx + tools) | libnx 4.12.0 / gcc 15.2.0 | `C:\devkitPro` を想定。別ドライブでも `-DevkitPro` / `DEVKITPRO` で指定可 |
| Rust nightly + `rust-src` | nightly 1.101.0 | tier-3 ターゲットなので `-Z build-std` が必須。無ければ build.ps1 が自動導入 |
| PowerShell 7 (pwsh) | 7.x | `build.ps1` の実行に使用 |
| **switch-curl** (任意) | 7.69.1-5 | 起動時の更新チェック専用。無くてもビルドは通り、機能だけが無効になる |

### switch-curl の導入（更新チェックを使う場合）

devkitPro の msys2 から入れます。**MSYS シェル**（`MSYSTEM=MSYS`）で実行すること。

```powershell
$env:MSYSTEM='MSYS'
C:\devkitPro\msys2\usr\bin\pacman.exe -S switch-curl
```

* `switch-zlib` は依存として自動で入る（libcurl は deflate を使う）。
* `switch-mbedtls` は**不要**。switch-curl は `--with-libnx` でビルドされていて、
  TLS は本体の `ssl` サービスが担当する（→「TLS 証明書」の節）。
* 未導入でも `build.ps1` / `build.rs` は警告を出して**更新チェックをコードから外す**
  （`cfg(switch_curl)`）。その場合 NRO は約 1.63 MB、導入時は約 2.16 MB。

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

* `switch/target/aarch64-nintendo-switch-libnx/release/XXSE` … rustc が作る ELF (route cargo)
* `switch/target/.../release/XXSE.elf` … リンク済み ELF (両ルートの成果物)
* `switch/target/.../release/libmhnx_switch.a` … Rust staticlib (route c 用)
* `switch/target/.../release/XXSE.nro` … 中間 NRO
* **`dist/XXSE.nro`** … 成果物 (build.ps1 がコピー)

## 実行時の動作

起動するとエディタ本体（`src/editor.rs` + `src/ui.rs`）が動きます。セーブの読み書きは
`src/savedata.rs`（本体の `save:` デバイス）と `src/editor.rs`（SD カードの `fopen`）です。

`src/app.rs` には初期の SD カードプローブ（`consoleInit` + `printf` で `/switch/XXSE` を
一覧し `hello.txt` を読む）が残っていますが、`run()` は `crate::editor::run()` を返した
あと `return` するので**到達しません**（診断用）。

`alloc` は newlib の malloc/free を `#[global_allocator]` として使用しており、
String / Vec / format! がそのまま使える。

## ファイル構成

| パス | 役割 |
| --- | --- |
| `Cargo.toml` | bin `XXSE` + lib `mhnx_switch` (rlib + staticlib) |
| `rust-toolchain.toml` | nightly + rust-src を固定 |
| `.cargo/config.toml` | ターゲット JSON、`build-std`、`json-target-spec`、DEVKITPRO 既定値 |
| `targets/aarch64-nintendo-switch-libnx.json` | カスタムターゲット定義（gcc リンカを指定） |
| `build.rs` | devkitPro のパスをリンク引数に変換（switch.specs / -lnx / libc 等） |
| `src/sys.rs` | libnx + newlib の FFI 宣言（構造体レイアウトは const assert で検証） |
| `src/net.rs` | libnx socket + switch-curl の FFI 宣言（TLS の扱いもここに書いてある） |
| `src/update.rs` | 更新チェック / ダウンロード / 差し替え / 自己再起動（スレッドと状態機械） |
| `../crates/mhnx-core/src/json.rs` | no_std の JSON リーダ（深さ制限つき） |
| `../crates/mhnx-core/src/update.rs` | 版比較・アセット選択・保存先・差し替えの規則（**ホストでテスト可能**） |
| `../crates/mhnx-core/tests/update_check.rs` | 上記のホストテスト 18 件 |
| `src/app.rs` | 入口（`editor::run()` を呼ぶ）と、診断用に残した SD カードプローブ |
| `src/editor.rs` | SD カードの読み書き・ボタン入力・メインループ（判断は `mhnx-core` の `hbedit`） |
| `src/ui.rs` | 画面の描画（キャンバス UI・内蔵キーボード・タッチ） |
| `src/text.rs` | 日本語フォント（pl:u 共有フォント → stb_truetype → フレームバッファ） |
| `src/swkbd.rs` | システムキーボード（swkbd）の FFI 宣言 |
| `src/savedata.rs` | 本体のセーブ（`save:` デバイス）の読み書き |
| `src/main.rs` | `#![no_main]` + `#[no_mangle] extern "C" fn main` |
| `src/lib.rs` | panic handler / global allocator / `mhnx_main()` (route c 用) |
| `examples/text_probe.rs` | 診断用 NRO（`-Probe` でビルド） |
| `tools/rustmain.c` | route c 用の C エントリポイント |
| `tools/font_shim.c` / `tools/stb_truetype.h` | フォント描画の C シム（`build.rs` がコンパイル） |
| `sdcard/` | SD カードへコピーする中身の置き場（`switch/XXSE/`。いまは空）。build.ps1 が `dist/` にも複製します |
| `build.ps1` | ビルド〜NRO 生成〜検証〜dist 配置 |

## 2 つのリンク経路

* **route cargo (既定)**: `cargo build --target <json>` が rustc → devkitA64 gcc を
  リンカとして呼び、ELF を直接生成する。リンク引数は `build.rs` が
  `cargo::rustc-link-arg` で渡す。
* **route c**: `cargo build` が生成した `libmhnx_switch.a` を、devkitA64 の gcc が
  `tools/rustmain.c` と一緒にリンクする（devkitPro Makefile 方式）。

どちらも同じ内容の `XXSE.nro` を生成する（サイズは switch-curl の有無で約 1.63 MB / 約 2.16 MB。「必要なもの」の表を参照）。


## 起動時の更新チェック（任意）

`dist/XXSE.nro` は、起動時に **1 回だけ** GitHub の release を見に行き、
新しい版があれば控えめなバナーで知らせます。通信できなければ何も出さずに通常起動します。

### 流れ

1. 起動直後、最初のフレームを描いた後に **専用スレッド**（libnx `threadCreate`）で
   `GET https://api.github.com/repos/Nexy451z/XXSE/releases/latest` を投げる。
   メインスレッドは待たない（UI は止まらない）。
2. 予算は **3 秒**（`CURLOPT_TIMEOUT_MS`、接続は 2.5 秒）。過ぎたら諦める。
3. `tag_name` を semver として `CARGO_PKG_VERSION` と比較する。
   * 同じ / 古い → **何も表示しない**
   * 新しい → ガイド行の右端に 1 行のバナー
     `新しいバージョン vX.Y.Z があります  [A] 詳細`
4. `[A]`（またはバナーをタップ）で確認ダイアログ。`[A]` でダウンロード開始。
5. ダウンロードは `sdmc:/switch/XXSE/XXSE.nro.new` へ。0〜100% と [A] 中止を表示。
6. 完了 → サイズ検証 → 既存を `.bak` に退避 → `.new` を `XXSE.nro` に差し替え →
   `envSetNextLoad()` で次回起動先を指定して自分を終了 → **hbmenu のローダーが新しい
   NRO を起動**（自己再起動）。
   * `envLaunchNextApplication()` は**この libnx には存在しない**（`env.h` にあるのは
     `envSetNextLoad` / `envHasNextLoad` / `envGetLastLoadResult` のみ）。
     nx-hbloader は「プロセスが終了したら NextLoadPath を読む」方式なので、
     `envSetNextLoad()` の後に終了するのが正しい自己再起動。
   * ローダーが NextLoadPath を持たない環境（古い hbmenu 等）では差し替えだけ行い、
     「手動で hbmenu から起動し直してください」と表示する。

### 入口（キーとタッチ）

| 操作 | 場所 | 動作 |
| --- | --- | --- |
| バナーをタップ | ガイド行の右端 | ダイアログを開く |
| `[A]` | **セーブ一覧（起動画面）のみ**、バナー表示中 | ダイアログを開く |
| `[R]` 長押し | セーブ一覧のみ | ダイアログを開く（設定の入口。バナーが無くても使える） |
| `[A]` | ダイアログ内 | 今すぐ確認 / ダウンロード / 中止 / もう一度 |
| `[B]` | ダイアログ内 | あとで（その起動ではもう出さない。DL 中なら中止） |
| `[Y]` | ダイアログ内 | 起動時の確認 オン / オフ（即ファイルに保存） |

> 編集画面では `[A]` を奪いません。`[A]` はその画面の「開く / 編集」であり、
> 編集中にダイアログが開くほうが害が大きいためです。編集画面からはバナーのタップで届きます。

### 設定ファイル

`sdmc:/switch/XXSE/update.cfg`（初回起動時に既定値で作られる）。

    # XXSE update check
    # 0 = 起動時に確認しない
    check_on_start=1

`check_on_start=0` のときは**起動時にソケットを一切開きません**。戻すときは
セーブ一覧で `[R]` 長押し → `[Y]`、またはこのファイルを `1` に。

## TLS 証明書

**`cacert.pem` は不要**（同梱も要求もしない）。理由:

* devkitPro の `switch-curl` は `--with-libnx --with-default-ssl-backend=libnx
  --without-mbedtls` でビルドされている（`devkitPro/pacman-packages` の
  `switch/curl/PKGBUILD`）。
* その TLS は **本体の `ssl` サービス**（libnx `sslCreateContext` /
  `sslConnectionDoHandshake`）が行い、検証は**本体の内蔵 CA ストア**に対して行われる。
  検証ビットの初期値は `SslVerifyOption_PeerCa | SslVerifyOption_HostName`。
* curl の `CURLOPT_SSL_VERIFYPEER` / `CURLOPT_SSL_VERIFYHOST` は、その 2 ビットへ
  そのまま対応づけられる（パッチの `vtls/libnx.c`）。**この実装は両方とも既定のまま**
  （1 / 2）なので、証明書検証もホスト名検証も有効。
* `CURLOPT_CAINFO` はこのバックエンドでは読まれない（設定しても無視される）。
  だから PEM を置いても意味がない。
* **検証を無効化するコードは入れていない。** `CURLOPT_SSL_VERIFYPEER=0` は設定して
  いないし、今後もすべきではない（MITM で任意の .nro を入れられる）。

リリース時の手順は「**何も同梱しない**」でよい。もし将来 devkitPro が switch-curl を
mbedTLS + ファイル CA 方式へ変えた場合は、`net.rs` の `CURLOPT_CAINFO` 定数を使って
`sdmc:/switch/XXSE/cacert.pem` を指定し、リリースに `cacert.pem`（Mozilla の
`cacert.pem`）を入れる — そのときは本節を書き換えること。

## 失敗したときの挙動

| 失敗 | 表示 | 既存の .nro | セーブ |
| --- | --- | --- | --- |
| 設定がオフ | 何も出ない（ソケットも開かない） | 無傷 | 無関係 |
| `socketInitialize` 失敗（機内モード等） | 起動時は**無言**。ダイアログを開いた場合のみ理由 | 無傷 | 無関係 |
| DNS 解決失敗 / 接続失敗 | 同上 | 無傷 | 無関係 |
| 3 秒タイムアウト | 同上 | 無傷 | 無関係 |
| HTTP 404（release 未作成）/ 5xx | 同上（404 は「更新なし」扱い） | 無傷 | 無関係 |
| JSON 不正（captive portal 等） | 同上 | 無傷 | 無関係 |
| `tag_name` が版番号でない | バナーは出る / ダイアログに理由 | 無傷 | 無関係 |
| 新しいが `.nro` アセットが無い / URL が http | バナーは出る / ダイアログに理由、[A] は「確認し直す」 | 無傷 | 無関係 |
| ダウンロード中の切断・容量不足 | 「元のファイルはそのままです」 | 無傷（`.new` は削除） | 無関係 |
| サイズ不一致 | 「ダウンロードが途中で終わりました」 | 無傷（`.new` は削除） | 無関係 |
| 中止（[A] / [B]） | 「中止しました。元のファイルはそのままです」 | 無傷（`.new` は削除） | 無関係 |
| `.bak` に退避できない | 「今のファイルを退避できませんでした。何も変更していません」 | 無傷 | 無関係 |
| `.new` を差し替えできない | 「差し替えに失敗しました。元のファイルは `XXSE.nro.bak` にあります」（可能なら自動で戻す） | `.bak` に待避、または元の位置 | 無関係 |
| ローダーが自己再起動に非対応 | 「手動で hbmenu から起動し直してください」 | 差し替え済み | 無関係 |

`.nro` の差し替えは **rename 2 回だけ**で、既存ファイルを上書き・切り詰めしない。
この順序（`.new` 完成 → `.bak` へ退避 → `.new` を昇格）と各失敗点の不変条件は
`crates/mhnx-core/src/update.rs` の `activate_staged()` にあり、ホストのテスト
（`crates/mhnx-core/tests/update_check.rs`）が偽の SD カードで検証している。

壊れたときの手動復旧: SD カードの `switch/XXSE/XXSE.nro.bak` を `XXSE.nro` に戻す。

## ネットワークを使わない部分のテスト

    cargo test -p mhnx-core --test update_check

18 件。JSON リーダ（正常/壊れた文書/エスケープ/深さ制限）、semver 比較、アセット選択、
https 必須、保存先決定、差し替えの全失敗経路、進捗%、設定ファイルの解釈、
バナー文言、リリースノートの整形を、ネットワークも SD も無しで検証する。

## トラブルシューティング

* `error: .json target specs require -Zjson-target-spec` … `.cargo/config.toml` の
  `[unstable] json-target-spec = true` が効いていない（nightly 以外で cargo を実行した）。
* `undefined reference to `printf'` など libc シンボル … rustc が付ける
  `-nodefaultlibs` の影響。`build.rs` の `-lgcc -lg -lc -lsysbase` が抜けていないか確認。
* `linker `aarch64-none-elf-gcc` not found` … `$env:PATH` に devkitA64\bin が無い。
  `build.ps1` 経由でビルドするか PATH を設定する。

`docs/` はリポジトリに含めていません（swkbd の構造体レイアウトの実測値は
`src/swkbd.rs` の `const` アサーションに残しています）。
