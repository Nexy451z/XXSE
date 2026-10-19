# MHXX クエストデータ形式 完全仕様

対象: Monster Hunter XX (3DS / Switch) のダウンロードクエスト資源
目的: `mhnx-core` (Rust, no_std+alloc 可) で read/write できるようにするための形式確定
作成: 2026-10-19
関連: `docs/quest_fields.json` (機械可読仕様), `docs/quedixx_field_map.md` (Quedixx UI 対応表)

---

## 0. この文書の根拠と読み方

### 0.1 一次データ

| 種別 | 内容 |
|---|---|
| 実データ | `mhxx_quetool/quest/q#######.arc` **141 ファイル / 全 1,950 エントリ**を zlib 展開して全数解析 |
| 逆コンパイル | `Quedixx.exe` / `Quest_Construct.exe` / `new_quest.exe` は 3 つとも .NET (MSIL) アセンブリ。**ILSpy 9.1 (ilspycmd) で完全な C# ソースに復元**し、読み書きメソッド本体を直接読んだ |
| 突き合わせ | `quest_open/q1010165_皐月に竜と泳ぐ/` の展開済み 15 ファイル、`new_quest/` の雛形 8 ファイル |

逆コンパイル結果 (作業用、リポジトリ外):
`.work/decomp/Quedixx/Quedixx.decompiled.cs` (352 KB) / `.work/decomp/Quest_Construct/Quest_Construct.decompiled.cs`
以下の行番号はこの 2 ファイルのもの。

### 0.2 確度の表記

* **【事実】** = 実データ全件、または逆コンパイル済みソースの該当行で直接確認した
* **【推定】** = 実データからの推定。本文中に [推定] と明記
* **【未確定】** = 判断材料が足りない

---

## 1. .arc コンテナ

MT Framework 系の汎用コンテナ。**全 141 ファイルが version 17 / zlib 圧縮**。
読み書きの全ルールは `Quest_Construct.decompiled.cs` の `arc_read` (L241-291) と `arc_save` (L293-344) に書かれている。【事実】

### 1.1 全体構造

```
+0x00  12 バイト        ヘッダ
+0x0C  80 * entryCount  エントリテーブル
       (パディング 0 埋め)
       data             zlib ペイロードが隙間なく連続
```

### 1.2 ヘッダ (12 バイト)【事実】

| offset | size | type | 名前 | 値 / 実測 |
|---|---|---|---|---|
| 0x00 | 4 | u32 | magic | `0x00435241` = バイト列 `41 52 43 00` = "ARC\0" (141/141) |
| 0x04 | 2 | u16 | version | `17` (141/141) |
| 0x06 | 2 | u16 | entryCount | 12〜16 (本サンプル) |
| 0x08 | 4 | u32 | flags | `0` (141/141) |

`arc_save` L322 は `BitConverter.GetBytes(4411969)` を書いており、4411969 = 0x00435241 = LE バイト列 `41 52 43 00`。【事実・一致】

### 1.3 エントリ (80 バイト)【事実】

| offset | size | type | 名前 | 内容 |
|---|---|---|---|---|
| +0x00 | 64 | char[64] | name | NUL 終端。**ディレクトリ区切りはバックスラッシュ**。**拡張子は格納されない**。例 `quest\boss\setEmMain\b_m01em080_00` |
| +0x40 | 4 | u32 | typeHash | 資源型 ID (1.5 の表) |
| +0x44 | 4 | u32 | compressedSize | .arc 内に置かれているバイト数 |
| +0x48 | 4 | u32 | flags | `0x40000000 | uncompressedSize` |
| +0x4C | 4 | u32 | dataOffset | .arc 先頭からの絶対オフセット |

`arc_save` L334-338 がそのまま対応する:
```csharp
Array.Copy(BitConverter.GetBytes(file_ei[num6]), 0, array5, num4 + 64, 4);   // typeHash
Array.Copy(BitConverter.GetBytes(num5),           0, array5, num4 + 68, 4);  // compressedSize
Array.Copy(BitConverter.GetBytes(array4[i]),      0, array5, num4 + 72, 3);  // uncompressedSize (3バイト)
Array.Copy(BitConverter.GetBytes(64),             0, array5, num4 + 75, 1);  // 0x40 固定
Array.Copy(BitConverter.GetBytes(num3),           0, array5, num4 + 76, 4);  // dataOffset
```

→ **上位バイト 0x40 は固定リテラル**であってサイズの一部ではない。展開後サイズは実質 24 bit。
検証: 1,950/1,950 エントリで `(flags & 0xFF000000) == 0x40000000` かつ `(flags & 0x00FFFFFF) == 展開後バイト数`。【事実】

### 1.4 データ部とアラインメント

* 圧縮は **zlib (RFC1950)**。先頭 2 バイトの実測は `78 01` が 189 件、`78 9C` が 1,761 件。Python `zlib.decompress` で 1,950/1,950 成功。【事実】
* **最初のデータオフセット = align32(12 + 80 * entryCount)**。この式は 141 ファイル中 140 ファイルで一致 (残り 1 ファイルは Quest_Construct 製で後述)。【事実】

| entryCount | 12+80n | align32 | 実測(dataOffset 最小値) |
|---|---|---|---|
| 12 | 972 | 992 | 992 (14 ファイル) |
| 13 | 1052 | 1056 | 1056 (33) |
| 14 | 1132 | 1152 | 1152 (66) |
| 15 | 1212 | 1216 | 1216 (18) / **1232 (1)** |
| 16 | 1292 | 1312 | 1312 (9) |

* 唯一の例外 `q1010165.arc` (dataOffset 1232) は `12 + 80*15 + 20` = 1232 で、**Quest_Construct の `arc_save` L321 (`num3 = 12 + 80 * num2 + 20`) の式と一致**する。つまりこのファイルは Capcom 製ではなくこのツールで作り直されたもの。【事実＋推定】
* データ部は隙間なしで連続。**ファイルサイズ == 最終エントリの dataOffset + compressedSize** (141/141)。エントリテーブル末尾〜最初のデータの間は全部 0 (141/141)。【事実】
* エントリ順は「boss → zako → rem → supp → plus → questLink → questData(_jpn) → questData」のフォルダ順で、フォルダ内の順序は .qdl の順序と一致しないことがある (`q1010165.arc`)。【事実】

### 1.5 typeHash 対応表【事実】

`Quest_Construct.decompiled.cs` L102-109 の整数リテラルと .arc 実測値が完全一致する。

| typeHash | 拡張子 | フォルダ | 出現数 | 名前 |
|---|---|---|---|---|
| `0x1BBFD18E` | .mib | `quest\questData` | 141 | `questData_<id>` |
| `0x242BB29A` | .gmd | `quest\questData` | 141 | `questData_<id>_jpn` |
| `0x00E0BB1C` | .qdl | `quest\questLink` | 141 | `questLink_<id>` |
| `0x2553701D` | .sem | `quest\boss\setEmMain` | 356 | `b_*` |
| `0x32CA92F8` | .esl | `quest\zako\emSetList` | 225 | `z_*` |
| `0x5B3C302D` | .rem | `quest\rem` | 664 | `rem_*` |
| `0x54539AEE` | .sup | `quest\supp` | 141 | `supp_<id>` |
| `0x26BEC21C` | .qdp | `quest\plus` | 141 | `questPlus_*` |

**計算式は特定できなかった。** 「拡張子 → 型名」の候補文字列 466 種 × ハッシュ関数 17 種
(CRC32 / CRC32C / CRC32-BZIP2 / CRC32K、FNV-1、FNV-1a、DJB2 2 変種、SDBM、Jenkins one-at-a-time、
`h*31+c`、`h*33+c`、`h*0x21+c`、`h*0x21^c`、`h*0x25+c`、MurmurHash3 x86_32 (seed 0/1)、xxHash32、Adler32)
を総当たりしたが 1 件も一致しなかった。**定数テーブルとして扱うこと。**

同じ 32bit 値がペイロード内部にも現れる:
* `questLink_<id>.qdl` のレコード先頭 (参照先資源の型)
* `.mib` の +0xDF (参照する .gmd の型 = 0x242BB29A 固定)

### 1.6 読み込み手順 (Rust)

```text
1. u32 magic == 0x00435241 を検証
2. u16 version, u16 entryCount を読む
3. entryCount 回、80 バイトのエントリを読む
   name は NUL までを Shift_JIS/ASCII として取り出し、フォルダ名から拡張子を決める (1.5 の表)
4. payload = zlib::decompress(&buf[dataOffset .. dataOffset+compressedSize])
5. payload.len() == (flags & 0x00FFFFFF) を検証
```

### 1.7 再構築手順 (Rust)

```text
1. ファイル群を集める (quest/<folder>/<name>.<ext>)
2. 各ファイルを zlib 圧縮 (level は Default 相当 -> 78 9C。78 01 でもゲームは受理する)
3. entryCount = ファイル数
   dataStart = 12 + 80*entryCount + 20          (Quest_Construct 互換)
   dataStart = align32(12 + 80*entryCount)      (Capcom 原本互換。こちらを既定にするのが安全)
4. ヘッダ: magic 0x00435241 / version 17 / entryCount / 0
5. 各エントリ:
     name   = "quest\" + folder + "\" + 拡張子を除いたファイル名 (Shift_JIS, 64 バイト以内, 0 埋め)
     type   = 1.5 の表
     csize  = zlib 後のバイト数
     flags  = 0x40000000 | 非圧縮サイズ (3 バイト + 0x40 の並びで書く)
     offset = dataStart から順に積む
6. dataStart からペイロードを隙間なく書く
```

**往復検証【事実】**: `quest_open/q1010165_皐月に竜と泳ぐ/` の 15 ファイルは `quest/q1010165.arc` の 15 エントリを zlib 展開した結果とバイト単位で一致した (既存解析で確認済み)。本解析でも 141 ファイル全 1,950 エントリの「展開 → サイズ一致」を確認している。

### 1.8 落とし穴 (実データで確認)

* **ファイル名と中身の ID が一致しない .arc が存在する**。`q1011045.arc` の 16 エントリのうち `supp_1011043` / `questLink_1011043` / `questData_1011043_jpn` / `questPlus_0072` は 1011043 の資源で、`.mib` だけが 1011045。【事実】
  → **エディタはファイル名から名前を合成してはいけない。必ずエントリ名を読むこと。**
* `.arc` の物理エントリ集合は「**15 スロットの .qdl が参照する名前の重複除去 + `questLink_<id>` + `questData_<id>_jpn` + `questData_<id>`**」とほぼ一致する (141 件中 139 件は順序まで一致)。ダミー資源 (`b_m00em000_00`, `z_m00d_000`, `rem_000000`, `questPlus_0000`) は .qdl に複数回現れても実体は 1 つだけ。【事実＋推定】
* `supp` / `plus` / `questLink` / `questData(.mib)` / `questData(.gmd)` は必ず 1 本ずつ存在する (141/141)。【事実】

---

## 2. `.mib` — questData_<id> (クエスト定義本体)

* **サイズは 249 バイト固定** (141/141)。【事実】
* リトルエンディアン。マジックは先頭 4 バイト `00 00 4B 43` (= u32 LE `0x434B0000`)。**`.qdl` と共通のヘッダ**。【事実】
* +0x0C がクエスト ID (u32 LE)。141/141 でファイル名の ID と一致 (`q1011045.arc` の中の .mib だけは中身も 1011045)。【事実】
* Quedixx の `main_read` (L1414-1562) / `main_edit` (L1564-1670) が全フィールドをオフセット直書きで読み書きしている。**これがフィールド表の一次根拠**。

### 2.1 全 249 バイトのフィールド表

「値域」「既定」列は **141 ファイル全数**から求めた観測値 (作業ファイル `.work/mib_bytestats.json`)。

| offset | size | type | key | 日本語 | Quedixx UI | 観測値域 | 最頻値 | 備考 |
|---|---|---|---|---|---|---|---|---|
| 0x00 | 2 | u16 | reserved0 | 予約 | - |  |  | 全141ファイルで 0x0000 |
| 0x02 | 2 | char[2] | magic_tail | マジック後半 | - |  |  | バイト列 "K","C"。ファイル先頭 4B は 00 00 4B 43 (.qdl と共通) |
| 0x04 | 4 | u32 | version | バージョン | - |  |  | 全141ファイルで 1 |
| 0x08 | 4 | u32 | reserved8 | 予約 | - |  |  | 全141ファイルで 0 |
| 0x0C | 4 | u32 | quest_id | クエストID | oq_id |  |  | ファイル名 q<ID>.arc と一致 (141/141) |
| 0x10 | 1 | u8 | quest_kind | クエスト種類 | oq_syu | 0..5 | 0 | enum enums.syu_ (7項目)。Quedixx は選択インデックスをそのまま格納 |
| 0x11 | 1 | u8 | village_rank | 村ランク | oq_va | 9..10 | 10 | 9=村下位? 10=村上位? ラベルは「村：」 |
| 0x12 | 1 | u8 | quest_level | クエストLv | oq_lv | 2..14 | 7 | 読み: >100 なら -85。書き: idx<16 なら idx, それ以外 idx+85。= 二つ名Lv は 101..115 で格納 |
| 0x13 | 1 | u8 | rank1 | ランク | oq_ran1 | 0..5 | 3 | enum enums.ran1_。0..5。読み書きともインデックスそのまま |
| 0x14 | 1 | u8 | map_id | 目的地(マップ) | oq_map | 1..104 | 9 | 読み: >100 なら -73。書き: idx<28 なら idx, それ以外 idx+73 |
| 0x15 | 1 | u8 | start_pos | 初期位置 | oq_syo | 0..1 | 0 | enum enums.syo_ (BC/ランダム/特殊) |
| 0x16 | 1 | u8 | time_limit | 制限時間 | oq_time | 35..50 | 50 | 観測 35..50、140/141 が 50 |
| 0x17 | 1 | u8 | revive_count | 復活回数 | oq_z4 | 3..3 | 3 | 全141ファイルで 3 (固定) |
| 0x18 | 1 | u8 | reserved24 | 予約 | - | 0..0 | 0 | 全141ファイルで 0 |
| 0x19 | 1 | u8 | bgm | BGM | oq_bgm | 0..1 | 0 | 観測 0..1 |
| 0x1A | 1 | u8 | cond1 | 参加条件1 | oq_san1 | 0..47 | 3 | enum enums.san_ (50項目)。fox_c=true のとき書き込みは 47 (=ニャンター不可) に固定 |
| 0x1B | 1 | u8 | cond2 | 参加条件2 | oq_san2 | 0..46 | 0 | enum enums.san_ |
| 0x1C | 1 | u8 | reserved28 | 予約 | - | 0..0 | 0 | 全141ファイルで 0 |
| 0x1D | 1 | u8 | t0 | ターゲット演算子 | oq_t0 | 0..2 | 0 | enum enums.t0_ (1のみ / & / or) |
| 0x1E | 1 | u8 | oo5 | 撃退HP% | oq_oo5 | 0..25 | 0 | 観測 0..25 (140/141 が 0) |
| 0x1F | 1 | u8 | tt1 | ターゲット1 種類 | oq_tt1 | 1..8 | 1 | enum enums.syu_ (20項目) |
| 0x20 | 2 | u16 | com0 | ターゲット1 ID | oq_t12 |  |  | comt[0]=2 なら mons_ 経由のモンスターID、comt[0]=1 なら item_ のアイテムID |
| 0x22 | 2 | i16 | t13 | ターゲット1 数 | oq_t13 |  |  | 符号付き (Minimum は -32768 相当の decimal) |
| 0x24 | 1 | u8 | tt2 | ターゲット2 種類 | oq_tt2 | 0..7 | 0 | enum enums.syu_ |
| 0x25 | 2 | u16 | com1 | ターゲット2 ID | oq_t22 |  |  |  |
| 0x27 | 2 | i16 | t23 | ターゲット2 数 | oq_t23 |  |  |  |
| 0x29 | 1 | u8 | tt3 | サブターゲット 種類 | oq_tt3 | 0..9 | 0 | enum enums.syu_ |
| 0x2A | 2 | u16 | com2 | サブターゲット ID | oq_t32 |  |  |  |
| 0x2C | 2 | i16 | t33 | サブターゲット 数 | oq_t33 |  |  |  |
| 0x2E | 1 | u8 | ran2 | 剥ぎ取り | oq_ran2 | 1..4 | 2 | enum enums.ran2_。格納値 = インデックス+1 (読: -1, 書: +1) |
| 0x2F | 1 | u8 | ran3 | 採集 | oq_ran3 | 0..9 | 2 | enum enums.ran3_。格納値 = インデックス+1 |
| 0x30 | 1 | u8 | ran4 | 釣り | oq_ran4 | 1..4 | 2 | enum enums.ran4_。格納値 = インデックス+1 |
| 0x31 | 4 | u32 | z1 | 契約金 | oq_z1 |  |  | 観測 300..8100 |
| 0x35 | 4 | u32 | p1 | 村ポイント | oq_p1 |  |  | 全141ファイルで 0 |
| 0x39 | 4 | u32 | z2 | 報酬金 | oq_z2 |  |  | 観測 2100..80400 |
| 0x3D | 4 | u32 | z3 | サブ報酬金 | oq_z3 |  |  | 観測 0..34200 |
| 0x41 | 4 | u32 | p2 | RRIP | oq_p2 |  |  | 観測 210..6440 |
| 0x45 | 4 | u32 | p3 | 減少RRIP | oq_p3 |  |  | 全141ファイルで 0 |
| 0x49 | 4 | u32 | p4 | サブRRIP | oq_p4 |  |  | 観測 0..3420 |
| 0x4D | 4 | u32 | p5 | HRP | oq_p5 |  |  | 観測 120..4070 |
| 0x51 | 4 | u32 | p6 | サブHRP | oq_p6 |  |  | 観測 0..1480 |
| 0x55 | 1 | u8 | rem1 | 報酬枠1 | oq_rem1 | 0..2 | 0 | .rem スロット番号。0=未使用 |
| 0x56 | 1 | u8 | rem2 | 報酬枠2 | oq_rem2 | 0..2 | 0 |  |
| 0x57 | 1 | u8 | rem3 | 報酬枠3 | oq_rem3 | 0..255 | 0 | 0 / 2 / 255 を観測 (255 は「なし」扱いの可能性) |
| 0x58 | 12 | u8[12] | unknown_58 | 未解析領域 | - |  |  | Quedixx は読み書きしない。28種類のパターン。88/89 と 94/95 が同値になる傾向 (u16 ペアの可能性) [推定] |
| 0x64 | 13 | struct | mob_slot[0] | モンスタースロット0 | dataGridView3 row0 |  |  |  |
| 0x71 | 13 | struct | mob_slot[1] | モンスタースロット1 | dataGridView3 row1 |  |  |  |
| 0x7E | 13 | struct | mob_slot[2] | モンスタースロット2 | dataGridView3 row2 |  |  |  |
| 0x8B | 13 | struct | mob_slot[3] | モンスタースロット3 | dataGridView3 row3 |  |  |  |
| 0x98 | 13 | struct | mob_slot[4] | モンスタースロット4 | dataGridView3 row4 |  |  |  |
| 0xA5 | 1 | u8 | ko1 | 小型モンスター1 | oq_ko1 | 19..77 | 42 | monster id (mint_ 空間)。観測 19..77、既定 42 |
| 0xA6 | 1 | u8 | ko2 | 小型モンスター2 | oq_ko2 | 10..127 | 48 | 観測 10..127、既定 48 |
| 0xA7 | 1 | u8 | ko3 | 小型モンスター3 | oq_ko3 | 11..84 | 32 | 観測 11..84、既定 32 |
| 0xA8 | 1 | u8 | tt4 | 配置1 種類 | oq_tt4 | 0..3 | 0 | enum enums.syu2_ (なし/狩猟討伐/採取/納品) |
| 0xA9 | 2 | u16 | com3 | 配置1 ID | oq_o1 / oq_o1_ / oq_o1__ |  |  | comt[3] により item_ か mons_ を切替 |
| 0xAB | 2 | i16 | o31 | 配置1 数 | oq_o31 |  |  |  |
| 0xAD | 1 | u8 | tt5 | 配置2 種類 | oq_tt5 | 0..2 | 0 | enum enums.syu2_ |
| 0xAE | 2 | u16 | com4 | 配置2 ID | oq_o2 |  |  |  |
| 0xB0 | 2 | i16 | o32 | 配置2 数 | oq_o32 |  |  |  |
| 0xB2 | 1 | u8 | syutu | 出現 | oq_syutu | 0..8 | 0 | 観測 0..8 |
| 0xB3 | 1 | u8 | tt6 | 配置3 種類 | oq_tt6 | 0..3 | 0 | enum enums.syu2_ |
| 0xB4 | 2 | u16 | com5 | 配置3 ID | oq_o3 |  |  |  |
| 0xB6 | 2 | i16 | o33 | 配置3 数 | oq_o33 |  |  |  |
| 0xB8 | 1 | u8 | tt7 | 配置4 種類 | oq_tt7 | 0..5 | 0 | enum enums.syu2_ |
| 0xB9 | 2 | u16 | com6 | 配置4 ID | oq_o4 |  |  |  |
| 0xBB | 2 | i16 | o34 | 配置4 数 | oq_o34 |  |  |  |
| 0xBD | 1 | u8 | tt8 | 配置5 種類 | oq_tt8 | 0..0 | 0 | enum enums.syu2_ |
| 0xBE | 2 | u16 | com7 | 配置5 ID | oq_o5 |  |  |  |
| 0xC0 | 2 | i16 | o35 | 配置5 数 | oq_o35 |  |  |  |
| 0xC2 | 1 | u8 | tt9 | 配置6 種類 | oq_tt9 | 0..0 | 0 | enum enums.syu2_ |
| 0xC3 | 2 | u16 | com8 | 配置6 ID | oq_o6 |  |  |  |
| 0xC5 | 2 | i16 | o36 | 配置6 数 | oq_o36 |  |  |  |
| 0xC7 | 1 | u8 | tt10 | 配置7 種類 | oq_tt10 | 0..0 | 0 | enum enums.syu2_ |
| 0xC8 | 2 | u16 | com9 | 配置7 ID | oq_o7 |  |  |  |
| 0xCA | 2 | i16 | o37 | 配置7 数 | oq_o37 |  |  |  |
| 0xCC | 1 | u8 | oo1 | 乱入1 | oq_oo1 | 0..100 | 0 | 観測 0..100 (確率%) |
| 0xCD | 1 | u8 | ex1 | 乱入(小)1 | oq_ex1 | 0..0 | 0 | 観測 0..100 |
| 0xCE | 1 | u8 | ex2 | 乱入(小)2 | oq_ex2 | 0..100 | 0 |  |
| 0xCF | 1 | u8 | ex3 | 乱入(小)3 | oq_ex3 | 0..0 | 0 |  |
| 0xD0 | 1 | u8 | ex4 | 乱入(小)4 | oq_ex4 | 0..0 | 0 |  |
| 0xD1 | 1 | u8 | ex5 | 乱入(小)5 | oq_ex5 | 0..0 | 0 |  |
| 0xD2 | 1 | u8 | oo2 | 乱入2 | oq_oo2 | 0..100 | 0 | 観測 0..100 |
| 0xD3 | 1 | u8 | oo3 | 乱入3 | oq_oo3 | 0..0 | 0 | 全141ファイルで 0 |
| 0xD4 | 1 | u8 | oo4 | 乱入4 | oq_oo4 | 0..0 | 0 | 全141ファイルで 0 |
| 0xD5 | 1 | u8 | mExtraTicket | mExtraTicket | numericUpDown1 | 0..0 | 0 | 全141ファイルで 0 |
| 0xD6 | 1 | u8 | icn1 | アイコン1 | oq_icn1 | 1..186 | 42 | enum enums.icn_ (188項目) |
| 0xD7 | 1 | u8 | icn2 | アイコン2 | oq_icn2 | 0..184 | 0 |  |
| 0xD8 | 1 | u8 | icn3 | アイコン3 | oq_icn3 | 0..185 | 0 |  |
| 0xD9 | 1 | u8 | icn4 | アイコン4 | oq_icn4 | 0..183 | 0 |  |
| 0xDA | 1 | u8 | icn5 | アイコン5 | oq_icn5 | 0..172 | 0 |  |
| 0xDB | 4 | u32 | reserved_219 | 予約 | - |  |  | 全141ファイルで 0 |
| 0xDF | 4 | u32 | sibling_gmd_typehash | 参照 .gmd の型ハッシュ | - |  |  | 全141ファイルで 0x242BB29A (= .gmd の typeHash)。リンク先リソース型の宣言 |
| 0xE3 | 14 | char[14] | gmd_ref_name | 参照テキスト資源名 | textBox8 |  |  | 全141ファイルで "<quest_id>_jpn\0" (11+NUL バイト、余りは 0 埋め) |
| 0xF1 | 4 | u32 | mVillagePointG | mVillagePointG | numericUpDown2 |  |  | 全141ファイルで 0 |
| 0xF5 | 4 | u32 | mFlag | mFlag | numericUpDown3 |  |  | 観測 11 種: 0x00000000,0x00020000,0x00080000,0x00420000,0x03000000,0x03020000,0x03080000,0x030A0000,0x04000000,0x04020000,0x04100000 (下位16bit は常に 0 = ビットマスク) |

### 2.2 モンスタースロット (5 スロット × 13 バイト、+0x64〜)

```
slot i : offset = 0x64 + 13*i   (i = 0..4)
```

| slot 内 offset | size | type | key | 日本語 |
|---|---|---|---|---|
| +0 | 2 | u16 | monster_id | モンスターID |
| +2 | 1 | u8 | state | 状態 |
| +3 | 1 | u8 | tamemono | 獰猛 |
| +4 | 1 | u8 | count | 匹数 |
| +5 | 1 | u8 | hp | 体力 |
| +6 | 1 | u8 | attack | 攻撃 |
| +7 | 1 | u8 | defense | 防御 |
| +8 | 1 | u8 | difficulty | 難易度 |
| +9 | 2 | u16 | size | サイズ |
| +11 | 1 | u8 | size_delta | ± |
| +12 | 1 | u8 | stamina | スタミナ |

* `id` は `henC.mint_` 空間の生モンスター ID (131 種、値域例 0,1,2,3,5,7,11,14,18,22,30,32,34,42,43,46,49,57,80,85,86,513,1323)。【事実】
* Quedixx は `henC.mons_` (表示名) と `henC.mint_` を突き合わせて変換する (`get_mons` L2017)。
  **書き込み時は `henC.mint_[コンボ選択インデックス]` を u16 で格納**する (L1626)。

### 2.3 変動バイト / 固定バイト【事実】

* **141 ファイル間で 1 度も変化しないバイト (96 個)**:
  `0-11, 15, 23, 24, 28, 40, 45, 51-56, 60, 63, 64, 67-72, 75, 76, 79, 80, 83, 84, 93, 97, 99, 128, 141, 147, 160, 162, 172, 177, 183, 188-203, 205, 207-209, 211-213, 219-227, 229, 234-246`
  → u32 フィールドの上位バイトが 0 であること、`.gmd` の型ハッシュ (223-226 = `9A B2 2B 24`) が固定であること、`"_jpn"` (234-237) が固定であることが読み取れる。
* **変化するバイト (153 個)**: 上記以外すべて。特に +0x10〜+0x16 (種別/村ランク/Lv/ランク/マップ/初期位置/制限時間) と +0x64〜+0xA4 (モンスター 5 スロット) が大きい。
* 既定値の例: 復活回数 (+0x17) = **常に 3**、制限時間 (+0x16) = 140/141 が **50**、`.gmd` 参照名 (+0xE3) = **"<id>_jpn"** (141/141)、`mVillagePointG` (+0xF1) = **常に 0**、`mExtraTicket` (+0xD5) = **常に 0**。

### 2.4 未解析領域 +0x58〜+0x63 (12 バイト)【未確定】

* Quedixx は **読まないし書かない** (`main_read`/`main_edit` のどちらにも現れない)。
* しかし 141 ファイル中 73 ファイルで非ゼロ。28 種類のパターンを取る。
  代表例 (16 進、これは offset 84..99 の 16 バイト表示):
  `00000000000000000000000000000000` (27 件) / `00000000010100000000040400000000` (22 件) / `00000200000000000000000000000000` (11 件)
* 88/89 と 94/95 が同値になる傾向があり、u16 ペアの可能性がある [推定]。
* Rust 実装では **`reserved: [u8; 12]` として保持し、読み書き時にそのまま温存する**のが安全。

---

## 3. `.gmd` — questData_<id>_jpn (テキストテーブル)

Capcom/MT Framework の GMD。**UTF-8 / NUL 終端の文字列 7 本**。【事実】

### 3.1 ヘッダ (40 バイト)【事実】

| offset | size | type | 名前 | 実測 |
|---|---|---|---|---|
| 0x00 | 4 | char[4] | magic | `GMD\0` (141/141) |
| 0x04 | 2 | u16 | version | `0x0302` (141/141) |
| 0x06 | 2 | u16 | languageCount | `1` (141/141) |
| 0x08 | 4 | u32 | - | 0 |
| 0x0C | 4 | u32 | resourceHash | 139 件が `0x432F44B3`。例外 `q1010172`=`0x44AAB360`, `q1011028`=`0x44A0B4EC` |
| 0x10 | 4 | u32 | - | 0 |
| 0x14 | 4 | u32 | - | 0 |
| 0x18 | 4 | u32 | stringCount | `7` (141/141) |
| 0x1C | 4 | u32 | - | 0 |
| 0x20 | 4 | u32 | blobSize | **= ファイルサイズ - 58** (141/141) |
| 0x24 | 4 | u32 | nameLength | `17` (141/141) = "questData_XXXXXXX" の長さ (NUL を含まない) |
| 0x28 | nameLength+1 | char[] | resourceName | `questData_<quest_id>` + NUL。**141 件中 1 件 (`q1011045`) は `questData_1011043`** |

`0x0C` のハッシュは資源名の CRC32 等では説明できなかった。**不透明値として温存**すること。

### 3.2 文字列テーブル (offset 58 = 0x3A から)【事実】

7 本の NUL 終端 UTF-8 文字列。改行は CRLF。並び順は Quedixx の `txt_read` (L1729-1757) と
tabPage2 のラベル (`Quedixx.decompiled.cs` L4212-4251) から確定:

| # | key | 日本語 | UI |
|---|---|---|---|
| 0 | quest_title | クエスト名 | textBox1 |
| 1 | client | 依頼主 | textBox2 |
| 2 | description | 依頼内容 | textBox3 |
| 3 | main_monsters | 主なモンスター | textBox4 |
| 4 | success | 成功条件 | textBox5 |
| 5 | failure | 失敗条件 | textBox6 |
| 6 | sub_success | サブ成功条件 | textBox7 |

実データ例:

* `q1010165` (490 B): 皐月に竜と泳ぐ / 子煩悩の貴族 / (長文) / なし / 全ての大型モンスターの狩猟 / 報酬金ゼロ␍␊タイムアップ / なし
* `q1010001` (552 B): ＵＳＪ・巨大斬竜との激闘 / ゴールドルナ装備の女 / (長文) / リモセトス␍␊マッカォ / ディノバルド１頭の狩猟 / 報酬金ゼロ␍␊タイムアップ / ドスマッカォ１頭の狩猟

【事実】`blobSize == Σ(len(utf8)+1)` が 141/141 で成立。7 本目の NUL の直後がファイル末尾 (141/141)。
**多言語版は存在しない**。本サンプルには `_jpn` のみ。`languageCount = 1` は全件。

### 3.3 書き換えルール (Quedixx `txt_edit` L1759-1780)

```text
new_size = 58 + Σ(utf8_len(s_i) + 1)
out[0..58] = old[0..58]                (ヘッダ + 資源名はそのまま)
out[32..36] = u32 Σ(utf8_len(s_i)+1)   (blobSize)
out[58..]  = s_0 \0 s_1 \0 ... s_6 \0
```
※ `58` は `nameLength == 17` を前提にしたハードコード。一般には `40 + nameLength + 1`。

---

## 4. `.qdl` — questLink_<id> (参照資源テーブル)

* **サイズは常に 308 バイト = 8 + 15 × 20**。【事実】141/141 で剰余 0、レコード数 15。
* ヘッダ 8 バイト: u32 magic `0x434B0000` (バイト列 `00 00 4B 43`、.mib と共通) + u32 version `1`。【事実】
* レコード 20 バイト = u32 typeHash + char[16] name (NUL 埋め)。【事実】

| slot | 意味 | 期待 typeHash |
|---|---|---|
| 0 | 大型モンスター1 | `0x2553701D (.sem)` |
| 1 | 大型モンスター2 | `0x2553701D` |
| 2 | 大型モンスター3 | `0x2553701D` |
| 3 | 大型モンスター4 | `0x2553701D` |
| 4 | 大型モンスター5 | `0x2553701D` |
| 5 | 小型モンスター1 | `0x32CA92F8 (.esl)` |
| 6 | 小型モンスター2 | `0x32CA92F8` |
| 7 | 小型モンスター3 | `0x32CA92F8` |
| 8 | メイン報酬品1 | `0x5B3C302D (.rem)` |
| 9 | メイン報酬品2 | `0x5B3C302D` |
| 10 | 特別報酬品1 | `0x5B3C302D` |
| 11 | 特別報酬品2 | `0x5B3C302D` |
| 12 | サブ報酬品 | `0x5B3C302D` |
| 13 | 支給品 | `0x54539AEE (.sup)` |
| 14 | Plus | `0x26BEC21C (.qdp)` |

* スロット番号と名前の対応は `Quest_Construct.decompiled.cs` の `arc_read` L281-288 (`file_ex[]` の並び) と
  Quedixx の tabPage14 ラベル (L4750-4895) の両方から一致して確認できる。【事実】
* 名前は **arc エントリ名の末尾トークン**。解決規則 (`arc_save` L331 の逆):
  * slot 0-4 → `quest\boss\setEmMain\<name>`
  * slot 5-7 → `quest\zako\emSetList\<name>`
  * slot 8-12 → `quest\rem\<name>`
  * slot 13 → `quest\supp\<name>`
  * slot 14 → `quest\plus\questPlus_<name>`
* **同じ名前が複数スロットに入りうる** (未使用スロットはダミー名 `b_m00em000_00` / `z_m00d_000` / `rem_000000` で埋まる)。物理 .arc には重複除去されて 1 つだけ入る。【事実＋推定】
* `.gmd` / `.mib` / `.qdl` 自身はこのテーブルに現れない。
* Quedixx は 15 スロット固定で読む (`link_read` L1702-1713) ため、**テーブルを増減できない**。

---

## 5. `.sem` — boss/setEmMain (ボス初期配置)

* **32 バイト固定** (356/356)。【事実】

| offset | size | type | key | 日本語 | UI | 観測 |
|---|---|---|---|---|---|---|
| 0x00 | 4 | f32 | const | - | - | 常に `1.0f` (`00 00 80 3F`, 1065353216) |
| 0x04 | 4 | u32 | version | - | - | 常に 1 |
| 0x08 | 4 | u32 | flag8 | - | - | 0 (140) / 1 (1) [推定] |
| 0x0C | 4 | u32 | area | エリア | o_o0 | 0..10 |
| 0x10 | 4 | f32 | rot | 向き | o_o1 | -176.0 .. 289.0 |
| 0x14 | 4 | f32 | x | X | o_o2 | -5362.0 .. 15776.0 |
| 0x18 | 4 | f32 | z | Z | o_o3 | -632.0 .. 2640.0 |
| 0x1C | 4 | f32 | y | Y | o_o4 | -6000.0 .. 16592.0 |

UI 対応根拠 = tabPage6 の Location ペア (エリア/X/Z/Y ラベルと o_o0..o_o4)。
ダミー `b_m00em000_00.sem` は先頭 12 バイト以外すべて 0。

---

## 6. `.esl` — zako/emSetList (小型モンスター出現リスト)

* 可変長 (64 〜 1,290+ バイト)。**60/141 は 64 バイトの完全空**。【事実】

### 6.1 ヘッダ (64 バイト)【事実】

| offset | size | type | 名前 | 内容 |
|---|---|---|---|---|
| 0x00 | 4 | char[4] | magic | `esl\0` |
| 0x04 | 4 | u32 | version | `2` |
| 0x08 | 56 | u32[14] | chunkOffsets | 各 ESD チャンクの絶対オフセット。**0 = 空きスロット**。チャンクの終端は「次の非ゼロオフセット」または EOF。実際に使われたのは index 0..10 のみ (11 個)。11..13 は常に 0 |

### 6.2 ESD チャンク【事実 (サイズ則) / 推定 (内部)】

```text
+0x00  4   "ESD\0"
+0x04  4   0x20151214  (バイト列 14 12 15 20) — 全チャンクで同一
+0x08  2   u16 recordCount
+0x0A  ... recordCount × 36 バイトのレコード
```

**サイズ則 `chunkSize == 10 + 36 * recordCount` は 141 ファイル全 674 チャンクで成立。**【事実】
レコード数の分布: 1 が 12、2 が 239、3 が 264、4 が 115、5 が 31、6 が 7、7 が 1。

レコード (36 バイト) の内部は **【推定】** (Quedixx は `komons_read`/`komons_edit` が共に何もしない):

```text
+0x00 u8   +0x01 u8 (常に 0x10)   +0x02 u8   +0x03 u8 (常に 0xCD)
+0x04 u8 (0xFF) +0x05 u8 +0x06 u8 +0x07 u8
+0x08 u32  +0x0C u16 (0xCDCD)
+0x0E f32  +0x12 f32  +0x16 f32      ← 座標とみられる
+0x1A u32  +0x1E u32  +0x22 u32
```

実測例 (slot0 of `q1010001`): `45 53 44 00 | 14 12 15 20 | 03 00 | 03 10 03 cd ff 01 01 00 00 00 cd cd 6b 81 29 c3 60 50 b3 c3 eb ad ff c3 00 00 00 00 30 6b 00 00 00 00 00 00`
3 番目の u32 (`30 6b 00 00` = 27440) がモンスター ID らしい [推定]。

---

## 7. `.rem` — rem (報酬品テーブル)

* **184 バイト固定** (664/664)。【事実】

| offset | size | type | key | 内容 |
|---|---|---|---|---|
| 0x00 | 4 | f32 | const | `1.0f` (全件 `00 00 80 3F`) |
| 0x04 | 4 | u32 | version | `1` |
| 0x08 | 16 | u8[8][2] | headerTable | Quedixx は 8 行 × 2 列 (`dataGridView5` の「何か」「枠数」) として往復させる [推定]。col0 = 1..8、col1 = 1..0x16 |
| 0x18 | 160 | record[40] | entries | 4 バイト × 40 |

レコード【事実 (Quedixx の読み書きから)】:

| rec 内 | size | type | key | 日本語 | UI | 観測 |
|---|---|---|---|---|---|---|
| +0 | 2 | u16 | item_id | アイテム | dataGridView2 col ID | 0..2976 |
| +2 | 1 | u8 | count | 個数 | 個数 | 0..10 |
| +3 | 1 | u8 | probability | 確率 | 確率 | 0..255 (0/5/7/8/10/12/14/255 など) |

* `rem_000000.rem` は **101 ファイルすべてで完全な全ゼロ (ヘッダ以外)**。空スロットのひな形として使える。【事実】
* ファイル名 `rem_######` の番号はモンスター帯と推定 (5xxxx/6xxxx/7xxxx 系)。**同じ名前の .rem は全クエストで同一バイト列** (例 `rem_630087`)。【事実】

---

## 8. `.sup` — supp_<id> (支給品テーブル)

* **168 バイト固定** (141/141)。【事実】

| offset | size | type | key | 内容 |
|---|---|---|---|---|
| 0x00 | 4 | f32 | const | `1.0f` (141/141) |
| 0x04 | 4 | u32 | version | `1` |
| 0x08 | 160 | record[40] | entries | 4 バイト × 40 |

| rec 内 | size | type | key | 日本語 | UI | 観測 |
|---|---|---|---|---|---|---|
| +0 | 2 | u16 | item_id | アイテム | dataGridView1 col ID | 0..1735 |
| +2 | 1 | u8 | count | 個数 | 個数 | 0..60 |
| +3 | 1 | u8 | flag | 何か | 何か | 0..4 |

読み出しコード `sikyu_read` L1782-1815: `num = (arere==0 ? 8 : 24)`、40 レコードを `num + 4*i` から。
→ **.sup はヘッダ 8 バイト直後、.rem はヘッダ + 追加 16 バイトの後**からレコードが始まる。【事実】

---

## 9. `.qdp` — questPlus (plus/questPlus_*)

* **25 バイト固定** (141/141)。【事実】
* `00 00 80 3F` (1.0f) + `01 00 00 00` (version 1) + **17 バイトのペイロード**。
* Quedixx の `plus_read` / `plus_edit` は **読み書きとも何もしない** (バイト列をそのまま保持)。【事実】
* 観測されたペイロードは 6 通り:

| ペイロード (17 B, hex) | 件数 | 含まれる値 |
|---|---|---|
| `0000000000000000000000000000000000` | 124 | - |
| `01005a0000001400000000000000000000` | 7 | 1, 90 (0x5A), 20 (0x14) |
| `0100280000001400000000000000000000` | 4 | 1, 40 (0x28), 20 |
| `0000000000000000010000000000000000` | 3 | 1 |
| `0000000000000000010000580200000000` | 2 | 1, 600 (0x0258) |
| `0001000000000000010000000058025802` | 1 | 1, 600, 600 |

**【未確定】** フィールド境界。u16 ペア (低 u16 が常に 1) に見えるが確定できない。
Rust 実装では `payload: [u8; 17]` として不透明に扱うこと。

---

## 10. クエスト ID 体系

**7 桁 = d1 | d2d3 | d4 | d5d6d7**。【事実 — 141 ファイル名 + `questID_memo.txt` で検証】

| 桁 | 名前 | 値 |
|---|---|---|
| d1 | プレフィックス | `1` (MHXX ダウンロードクエスト) |
| d2d3 | 種別 | `01` = ハンター / `02` = 訓練 / `11` = オトモ(ネコ) / `12` = 訓練ネコ |
| d4 | ランク | `0` = 下位 / `1` = 上位 / `2` = 訓練 (上位ネコは d4=1、G 級は 1011xxx / 1111xxx) |
| d5d6d7 | 通し番号 | `001`〜 |

本サンプルの分布 (実在ファイル名から機械集計):

| d1 | d2d3 | d4 | 件数 | 例 |
|---|---|---|---|---|
| 1 | 01 | 0 | 6 | 1010001-1010006 (下位) |
| 1 | 01 | 1 | 112 | 1010101-1010175, 1011001-1011045 (上位 / G 級) |
| 1 | 11 | 0 | 17 | 1110001-1110007, 1110101-1110113 (下位ネコ / 上位ネコ) |
| 1 | 11 | 1 | 7 | 1111001-1111007 (G 級ネコ) |

`questID_memo.txt` (Shift_JIS) の記載:

```text
下位      1010001-1010006
上位      1010101-1010176   (1010126,1010132,1010136,1010139,1010160,1010162 は無い)
G級       1011001-1011047   (1011014,1011038 は無い)
下位ネコ   1110001-1110008
上位ネコ   1110101-1110113
G級ネコ    1111001-1011009   ← 明らかなタイポ (1111009 の誤り)
訓練      1020001-1020029
訓練ネコ   1120001-1120012
```

* Quedixx の `oq_id` (NumericUpDown) は Minimum 101 / Maximum 1120050 / 既定 1000001。
* ID は `.mib` +0x0C と資源名 (`questData_<id>`, `supp_<id>`, `questLink_<id>`) に現れる。
  `main_file_edit` (L1672) は **ID を変更したときファイル名の中の旧 ID 文字列を置換してリネーム**する。
* ID を変更するツールを書く場合は、`.mib` +0x0C、`.mib` +0xE3 の `<id>_jpn`、`.gmd` の資源名、
  `supp_*` / `questLink_*` のファイル名、`.arc` エントリ名、`.qdl` の該当スロットを**全部**書き換える必要がある。

---

## 11. Rust 実装メモ

### 11.1 型の目安

```rust
pub const ARC_MAGIC: u32 = 0x0043_5241;      // "ARC\0" LE
pub const USER_MAGIC: u32 = 0x434B_0000;     // .mib / .qdl 共通ヘッダ
pub const TYPE_MIB: u32 = 0x1BBF_D18E;
pub const TYPE_GMD: u32 = 0x242B_B29A;
pub const TYPE_QDL: u32 = 0x00E0_BB1C;
pub const TYPE_SEM: u32 = 0x2553_701D;
pub const TYPE_ESL: u32 = 0x32CA_92F8;
pub const TYPE_REM: u32 = 0x5B3C_302D;
pub const TYPE_SUP: u32 = 0x5453_9AEE;
pub const TYPE_QDP: u32 = 0x26BE_C21C;

pub const ARC_HEADER_SIZE: usize = 12;
pub const ARC_ENTRY_SIZE: usize = 80;
pub const ARC_ENTRY_NAME_LEN: usize = 64;
pub const ARC_FLAG_COMPRESSED: u32 = 0x4000_0000;

pub const MIB_SIZE: usize = 249;
pub const QDL_SIZE: usize = 308;
pub const SEM_SIZE: usize = 32;
pub const REM_SIZE: usize = 184;
pub const SUP_SIZE: usize = 168;
pub const QDP_SIZE: usize = 25;
pub const ESL_HEADER_SIZE: usize = 64;
```

### 11.2 no_std + alloc での注意

* 圧縮は `miniz_oxide` (no_std 可) の `inflate::decompress_to_vec_zlib` / `deflate` が使える。
  MHXX の `.arc` は RFC1950 の zlib そのものなので追加処理は不要。
* `.mib` / `.gmd` は可変長の書き換えで **サイズが変わる**。エディタは「構造体に読み込んで変更 → 再シリアライズ」ではなく
  **「元バイト列を保持し、既知オフセットだけ patch する」**方式が最も安全 (Quedixx 自身がこの方式)。
  サイズが変わるのは `.gmd` の文字列と `.arc` のみ。
* 文字列は UTF-8 (`.gmd` 内) と Shift_JIS (`.arc` エントリ名)。`encoding_rs` は no_std では重いので、
  `.arc` の名前は ASCII 範囲 (実データは全部 ASCII) として扱い、必要なら `alloc` 付きで変換するのが現実的。

### 11.3 推奨 API 形

```rust
struct ArcEntry { name: String, ty: u32, comp: Vec<u8>, raw_len: u32, offset: u32 }
fn parse_arc(bytes: &[u8]) -> Result<Vec<ArcEntry>>;
fn build_arc(entries: &[ArcEntry], align: AlignMode) -> Result<Vec<u8>>;  // AlignMode::{Capcom32, Tool20}
struct Mib<'a>(&'a [u8]);   // patch-in-place; typed accessors get/set u8/u16/u32 at fixed offsets
struct Gmd { name: String, raw_hash: u32, strings: [String; 7] }
struct Qdl { records: [(u32, String); 15] }
```

---

## 12. 未確定事項

| # | 内容 | 状況 |
|---|---|---|
| U1 | typeHash の計算式 | 466 文字列 × 17 ハッシュで不一致。定数表で運用 |
| U2 | `.mib` +0x58〜+0x63 (12 B) の意味 | 28 パターン。Quedixx 非対応。[推定] u16 ペア |
| U3 | `.esl` ESD レコード 36 B のフィールド境界 | サイズ則のみ確定。座標 3 × f32 とモンスター ID らしき u32 を確認 [推定] |
| U4 | `.qdp` 17 B ペイロードの意味 | 6 パターン。u16 ペアの可能性 [未確定] |
| U5 | `.rem` +0x08〜+0x17 (16 B) の意味 | Quedixx は 8×2 グリッドとして往復させるだけ [推定] |
| U6 | `.gmd` +0x0C のハッシュ | 139/141 が同一値。算出法不明 |
| U7 | `.sem` +0x08 の値 (0/1) の意味 | 1 件のみ 1 |
| U8 | `rem_######` の番号体系 | 5xxxx/6xxxx/7xxxx がモンスター帯らしい [推定] |
| U9 | 音声/多言語の GMD | 本サンプルは `_jpn` のみ。Switch 版の追加言語は未確認 |

---

## 13. 根拠ファイル

| パス | 用途 |
|---|---|
| `mhxx_quetool/quest/q#######.arc` (141) | .arc / 全内部形式の全数解析 |
| `mhxx_quetool/quest_open/q1010165_皐月に竜と泳ぐ/` (15) | 展開結果の突き合わせ |
| `mhxx_quetool/new_quest/` (8) | 雛形 (空スロットのダミー資源) |
| `mhxx_quetool/questID_memo.txt` | ID 体系 |
| `mhxx_quetool/Quest_Construct.ini` / `new_quest.ini` | 展開/再構築のパス規則 |
| `.work/decomp/Quedixx/Quedixx.decompiled.cs` | .mib/.gmd/.qdl/.sem/.rem/.sup のフィールドと UI 対応 (ILSpy 9.1) |
| `.work/decomp/Quest_Construct/Quest_Construct.decompiled.cs` | .arc の読み書きアルゴリズムと typeHash 定数 |
| `.work/extract/<qid>/quest/...` | 展開済み 1,950 ファイル (作業用) |
