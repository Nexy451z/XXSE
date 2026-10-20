# 装備名の日本語データ調査レポート

調査日: 2026-10-20
担当: 装備名日本語データ調査
対象: `C:\anyWorkspace\nx\mhxx\mhnxeditor`（MHXX セーブエディタ）

---

## 1. 結論（先に要点）

**装備名の日本語データは「見つかった」のではない。この PC 上に最初からあった。**
ユーザー自身が吸い出した MHXX (Switch) の NSP の中に、日本語名の完全なテーブルが入っている。

| 項目 | 結果 |
|---|---|
| 日本語装備名の入手 | **可能（確認済み・実データ抽出済み）** |
| 出どころ | `C:\anyWorkspace\nx\mhxx\rom\Monster Hunter XX Switch Ver [0100C3800049C000][v0][JPN].nsp`（10.5 GB） |
| 復号鍵 | `C:\anyWorkspace\nx\mhxx\extract\upd\prod.keys`（既存）＋ hactool 1.4.0（既存 `C:\anyWorkspace\nx\tools\hactool\hactool.exe`） |
| 抽出コスト | **41 秒**（9.77 GiB / 14,581 ファイル） |
| ID 対応 | **武器・防具とも既存の英語表と 1:1 で対応する（件数一致を 14/14 種で確認）** |
| ネット上の公開データ | **ID 順・日本語・ライセンス明確なものは見つからなかった**（第 7 節） |

つまり「英語名のまま + 読み仮名」のような妥協案は不要。**既存の英語表をそのまま日本語表に差し替えられる。**

---

## 2. 出どころの詳細

### 2.1 ROM の所在（既存）

```
C:\anyWorkspace\nx\mhxx\rom\Monster Hunter XX Switch Ver [0100C3800049C000][v0][JPN].nsp   10,531,056,692 B
C:\anyWorkspace\nx\mhxx\rom\Monster Hunter XX [0100C3800049C800][v393216][JP][Up v1.5.1].nsp   62,266,816 B
```

Ryujinx の `Config.json` の `game_dirs` に `C:\anyWorkspace\nx\mhxx\rom` が登録されており、
Ryujinx 側にもプレイ実績（`games\0100c3800049c000\gui\metadata.json` = "MONSTER HUNTER XX Nintendo Switch Ver."）がある。

### 2.2 NCA の構造（hactool で確認）

`C:\anyWorkspace\nx\mhxx\out_base\program.nca`（10,530,586,624 B）を hactool で開いた結果:

```
Magic: NCA3   Title ID: 0100c3800049c000   Content Type: Program
Master Key Revision: 0x0   Encryption Type: Standard crypto
Sections:
  Section 0: ExeFS   size 0x0000014dc000
  Section 1: RomFS   size 0x0002725c8000   ← 日本語名はここ
  Section 2: PFS0    size 0x000000018000
```

**鍵は既に揃っていた。** `prod.keys`（231 行）はそのままでは hactool が
`Key (...) must be 32 hex digits!` で停止する壊れた行を含むため、
32/64 hex 桁の正しい行だけに濾した作業コピーを作って使った（196 行が有効）。

### 2.3 再現手順（そのまま実行できる）

```powershell
# 1) prod.keys から壊れた行を落として作業コピーを作る
python - <<'PY'
src = r"C:\anyWorkspace\nx\mhxx\extract\upd\prod.keys"
dst = r"C:\Users\haruk\AppData\Local\Temp\mhxx_scratch\prod.keys"
with open(src, encoding="utf-8") as f, open(dst, "w", encoding="utf-8") as g:
    for line in f:
        s = line.strip()
        if not s or s.startswith("#") or "=" not in s: continue
        k, v = [x.strip() for x in s.split("=", 1)]
        if len(v) in (32, 64) and len(v) % 2 == 0:
            g.write(f"{k} = {v}\n")
PY

# 2) romfs を丸ごと展開（41 秒 / 9.77 GiB）
cd C:\Users\haruk\AppData\Local\Temp\mhxx_scratch
C:\anyWorkspace\nx\tools\hactool\hactool.exe `
  -k prod.keys `
  --romfsdir='C:\anyWorkspace\nx\mhxx\extract\base_romfs' `
  'C:\anyWorkspace\nx\mhxx\out_base\program.nca'

# 3) 日本語名テーブルを CSV 化
python C:\anyWorkspace\nx\mhxx\extract\jp_names\gen.py
```

### 2.4 romfs の全体像（参考）

トップディレクトリ別ファイル数:

```
arc\player      5729    arc\stage       510
arc\weapon      3347    arc\ui          185
arc\quest       1859    arc\ev          150
arc\otomo        784    arc\enemy       137
sound\bgm        556    table\*         約 700
```

日本語名は `nativeNX\table\` 配下の `*_jpn.gmd` に入っている。
GMD は Capcom MT Framework のメッセージ形式で、既に
`crates/mhnx-core/src/quest.rs` の `Gmd::from_bytes` が実装済み
（`"GMD\0"`, `+0x18` = 文字列数, `+0x24` = key 長, `+0x28` 以降 = NUL 区切り UTF-8）。
**既存コードをそのまま流用できる。**

---

## 3. 日本語名テーブルの一覧（すべて実データで確認）

| ファイル (`nativeNX\table\`) | 文字列数 | 名前/説明の構造 | 対応する既存表 |
|---|---|---|---|
| `weapon00MsgData_jpn.gmd` | 822 | ID `i` の name = `strings[6i]` | `EQUIP_GREAT_SWORD_NAMES` (137) |
| `weapon01MsgData_jpn.gmd` | 774 | 同上 | `EQUIP_SWORDN_SHIELD_NAMES` (129) |
| `weapon02..14MsgData_jpn.gmd` | — | 同上 | 各武器種 |
| `armorSeriesData_jpn.gmd` | 12,870 | 系統 `s`・部位 `p` の name = `strings[10s + p]` | `EQUIP_{HEAD,CHEST,ARMS,WAIST,LEGS}_NAMES` |
| `amuletData_jpn.gmd` | 22 | ID `i` の name = `strings[2i]` | `EQUIP_TALISMAN_NAMES` (11) |
| `itemData_jpn.gmd` | 5,982 | ID `i` の name = `strings[2i]` | `ITEM_NAME_LIST` (2,991) |
| `skillData_jpn.gmd` | 656 | index `i` の name = `strings[2i]` | `SKILL_NAMES`（※件数不一致。第 8 節） |
| `otWeaponData_jpn.gmd` | 1,018 | ID `i` の name = `strings[2i]` | `PALICO_WEAPON_NAMES` (509) |
| `otArmorData_jpn.gmd` | 2,108 | 系統 `s` の頭 = `strings[4s]`, 胴 = `strings[4s+1]` | `PALICO_HEAD_NAMES` (502) / `PALICO_ARMOR_NAMES` (524) |

すべて「名前, 説明」または「名前群, 説明群」の並び。説明文は不要なので、
**名前だけ抜けばそのまま既存配列の置き換えになる。**

---

## 4. 武器名の ID 対応（最重要）

### 4.1 件数の一致（14/14 種で完全一致）

romfs には `weaponNNMsgData.wNNm` という**メッセージ索引テーブル**がある
（先頭 `u32` = 0x40000000、`+0x04` = 件数、以降 `u32` × 件数）。

この件数と、既存の英語表・および `weaponNNBaseData.wNNd` のレコード数が**完全に一致**した:

| type | `.wNNm` 件数 | BaseData 件数 | 既存の英語表 | 武器種（第 1 武器名から判定） |
|---|---|---|---|---|
| 00 | 137 | 137 | EquipGreatSwordNames 137 | 大剣（ベルダーブレイド） |
| 01 | 129 | 129 | EquipSwordnShieldNames 129 | 片手剣（ベルダーソード） |
| 02 | 132 | 132 | EquipHammerNames 132 | ハンマー（ベルダーハンマー） |
| 03 | 127 | 127 | EquipLanceNames 127 | ランス（ベルダーランス） |
| 04 | 109 | 109 | EquipHeavyBowgunNames 109 | ヘビィボウガン（ベルダーキャノン） |
| 05 | — | — | — | **存在しない（欠番）** |
| 06 | 110 | 110 | EquipLightBowgunNames 110 | ライトボウガン（ベルダーバレット） |
| 07 | 127 | 127 | EquipLongswordNames 127 | 太刀（ベルダーサーベル） |
| 08 | 110 | 110 | EquipSwitchAxeNames 110 | スラッシュアックス（ベルダーアックス） |
| 09 | 115 | 115 | EquipGunlanceNames 115 | ガンランス（ベルダーガンランス） |
| 10 | 108 | 108 | EquipBowNames 108 | 弓（ベルダーボウ） |
| 11 | 126 | 126 | EquipDualBladesNames 126 | 双剣（ベルダーダガー） |
| 12 | 112 | 112 | EquipHuntingHornNames 112 | 狩猟笛（ベルダーホルン） |
| 13 | 83 | 83 | EquipInsectGlaiveNames 83 | 操虫棍（ベルダーロッド） |
| 14 | 81 | 81 | EquipChargeBladeNames 81 | チャージアックス（ベルダーアームズ） |

**結論: 武器 ID の空間は既存の英語表と完全に同一。** 索引配列（`*_I_DS`）は武器には不要で、
`names[weapon_id]` で正しい（C# の `MainForm.cs` も `EquipGreatSwordNames[eqID]` と直接引いている）。

> 武器種の対応（00=大剣 … 14=チャージアックス）は、件数の一致と各 type の第 1 武器名
> （ベルダー～）の両方から判定した。**この対応付け自体は推測ではなく、2 系統の独立した証拠が一致している。**
> ただし type 05 が欠番である理由は不明（未確認）。

### 4.2 名前の引き方

武器種 `t`、武器 ID `wid`、表示形態 `form` の名前:

```
name = parse_gmd("nativeNX/table/weapon%02dMsgData_jpn.gmd" % t)["strings"][6*wid + 2*form]
desc = 同 [6*wid + 2*form + 1]
```

**`form` は 0/1/2 の 3 段階**（`6*wid` から 3 組の name/desc が並ぶ）。
`form=0` が既存の英語表と一致する基本名。

実例（大剣 `wid=1`）:

| form | 名前 | 説明 |
|---|---|---|
| 0 | ベルダーブレイド | 龍歴院から提供された化石で造られた大剣。… |
| 1 | サージュブレイド | ベルダーブレイド最終強化型。… |
| 2 | ソフォスブレイド | 【　究　極　強　化　】狩人の魂が呼び覚ました、ベルダーブレイドの極致。 |

英語表との対応（`form=0` で検証、24 件以上を目視一致確認）:

| wid | 既存英語名 | 日本語（form 0） |
|---|---|---|
| 0 | (None) | 装備なし |
| 1 | Petrified Blade | ベルダーブレイド |
| 2 | Obsidian Blade | オブシドブレイド |
| 3 | Halberd | ハルバード |
| 4 | Decider | ディシジョン |
| 5 | Igneous Blade | 火砕斬 |
| 6 | Type 41 Wyvernator | ４１式対飛竜大剣 |
| 7 | Khezu Shock Sword | フルミナントソード |
| 8 | Defender | ディフェンダー |
| 9 | Usurper's Storm | 王大剣フウライ |
| 11 | Iron Sword | アイアンソード |
| 12 | Lagiacrus Blade | ラギアブレイド |
| 19 | Hidden Blade | ヒドゥンブレイズ |

### 4.3 表示形態（form）の選び方

`weaponNNLevelData.wNNd` が「武器 ID × レベル」ごとの形態を持つ（15 byte/レコード）:

```
+0..3  u32  レコード番号
+4     u8   武器 ID
+5     u8   レベル (1..N)
+14    u8   名前形態 (0..3)
```

大剣の例（`.w00d` は 959 レコード = 137 武器のレベル合計）:

```
wid=1: (level,form) = (1,0)(2,0)(3,0)(4,0)(5,0)(6,1)(7,1)(8,1)(9,2)(10,2)(11,3)
wid=2: (1,0)…(6,0)(7,1)(8,2)
wid=3: すべて form 0
```

セーブ側の装備レコードには**レベル 5 bit** がある
（`analysis/switch_save_format.md` §3.6: `+0/+1` の `typeLevelBits.Substring(6,5)`）。
そのため「レベルから form を引く」ことで、強化段階に応じた正しい名前を出せる。

> **推測（未検証）**: `form` は 0〜3 の 4 値を取るが、GMD には 1 武器あたり 3 組しかない。
> `form=3` を素直に `6*wid+6` で引くと**次の武器の名前**になるため、そのまま使ってはいけない。
> `form=3` は極限強化後の状態で、表示は `form=2` と同じ（または別テーブル）と考えるのが自然だが、
> **実機で確認していない。** 実装時は `form = min(form, 2)` でクランプし、要検証事項として残すこと。

---

## 5. 防具名の ID 対応

`armorSeriesData_jpn.gmd` は **1 系統あたり 10 文字列**:

```
strings[10*s + 0..4]  = 頭 / 胴 / 腕 / 腰 / 脚 の名前
strings[10*s + 5..9]  = 同じ順の説明
```

`armorSeriesData.asd` の件数 = **1,287**、GMD も `12,870 = 10 × 1,287` で一致。

**防具 ID は系統 ID そのもの**（部位ごとに独立した ID 空間）。
根拠:

| 検証 | 結果 |
|---|---|
| `EquipHeadIDs` の最大値 | 1,282 < 1,287 ✓ |
| `EquipChestIDs` の最大値 | 1,286 < 1,287 ✓ |
| `EquipArmsIDs` / `Waist` / `Legs` の最大値 | 1,282 ✓ |
| 系統 1 の頭 | レザーヘッド = "Leather Headgear"（`EquipHeadNames[1]`）✓ |
| 系統 2 の頭 | チェーンヘッド = "Chainmail Headgear" ✓ |
| 系統 3 の頭 | ベルダーターバン = "Bherna Turban" ✓ |
| 系統 1 の胴 | レザーベスト = "Leather Vest"（`EquipChestNames[1]`）✓ |

つまり `name(part, id) = strings[10*id + part_index]`（part_index: 頭=0, 胴=1, 腕=2, 腰=3, 脚=4）。
**`EQUIP_*_I_DS` は今まで通り必要**（ID に欠番があるため）。

オトモ防具は `otArmorData_jpn.gmd` が **1 系統 4 文字列**
（`strings[4s]`=頭, `[4s+1]`=胴, `[4s+2]`=頭の説明, `[4s+3]`=胴の説明）。
系統数 `2,108 / 4 = 527` で、`PalicoHeadIDs` の最大 526・`PalicoArmorIDs` の最大 526 と整合 ✓。

---

## 6. その他の表（おまけ・既存データの検証にもなる）

### 6.1 アイテム名

`itemData_jpn.gmd` = 5,982 文字列 = 2 × **2,991**。
`itemData.itm` の件数も **2,991** で、`ITEM_NAME_LIST` と一致。

既存の `ITEM_NAME_LIST` と ROM の名前を**添字で突き合わせた結果、2,880 / 2,991 が完全一致**。
差分 111 件は表記ゆれのみ:

```
ID 0    既存 "-----"           ROM "－－－－－－－－"
ID 93   既存 "(なし)"           ROM "アイテム非選択"
ID 132  既存 "LV1 通常弾 [?]"   ROM "LV1 通常弾"
```

→ **アイテム名は既に ROM と実質同じ。作業不要。**

### 6.2 スキル名（要注意）

`skillData_jpn.gmd` = 656 文字列 = 2 × **328**。`skillData.skd` の件数も **328**。

しかし `crates/mhnx-core/src/skills.rs` の `SKILL_NAMES` は **306 件**で、しかも**添字 0 から一致しない**:

```
添字 0: 既存 "毒"     ROM "スキル無し"
添字 1: 既存 "麻痺"   ROM "毒耐性"
添字 2: 既存 "睡眠"   ROM "毒倍加"
```

306 vs 328 で 22 件の差があり、並び順も別系統（handoff の「MHGU Save Manager 由来なので
並び順が違う可能性がある」という警告どおり）。
**この 306 件の表が正しいかは本調査では判定できない。** ROM 側が 328 件・`skillData.skd`
と件数一致という強い根拠を持つため、**別途レビュー対象にすることを推奨**（本レポートの範囲外）。

---

## 7. PC 内の探索範囲と結果

### 7.1 実際にやったこと

1. **ドライブ列挙**: C:（952 GB 中 648 GB 使用）と G:（952 GB 中 663 GB 使用）を確認。
2. **全ドライブのディレクトリ名スキャン**（`Get-ChildItem -Recurse -Directory`、
   `mhxx|mhgu|モンハン|monsut|romfs|monsterhunter|monster_hunter` に一致するもの）:
   - C: **40 ディレクトリ**ヒット（`.work/_scan_c_dirs.txt` に保存）
   - G: **0 件**
3. **エミュレータ領域**:
   - `%APPDATA%\Ryujinx` を全走査。`games\0100c3800049c000` に **MHXX のプレイ実績あり**。
   - `mods\` は SSBU のみ。MHXX 用 romfs mod は無し（`mods_disabled\exefs_MHNX_mod` は exefs のみ）。
   - `bis\user\save\` の 6 スロットはすべて **SSBU のセーブ**（4,726,188 B で同一）。
     **MHXX のセーブデータはこの PC に無い。**
4. **Ryujinx の `game_dirs` を辿る**: `nx\mhxx\rom`, `nx\mhxx\extract\titles` を発見 → 第 2 節へ。
5. **プロジェクト内の生成物**: `nx\mhxx\out_base\`（展開済み exefs + 10.5 GB NCA）、
   `nx\mhxx\extract\upd\`（更新版の exefs + prod.keys）、`nx\mhxx\scan\`（メモリスナップ）、
   `nx\mhxx\ghidra\` を確認。
6. **exefs 内の日本語**: `existing_mod\subsdk9_strings.txt`・`extract\upd\strings.txt` を調べたが
   **日本語文字は 0 件**（装備名はコードではなく romfs 側にあるため。想定どおり）。
7. **参照実装の全ファイル**: 第 8 節。
8. **Git 履歴**: 3 リポジトリ（`MHXXSaveEditor` / `MHXX-GUSaveEditor` / `MHXXSwitchSaveEditor`）を確認。
   `GameConstants.cs` は 3 つとも同一内容。

### 7.2 見つかったが「ハズレ」だったもの

| パス | 中身 | 判定 |
|---|---|---|
| `C:\anyWorkspace\nx\mhxx\refs\SJ-site\MHGU\save-manager\scripts\MHXX_SWITCH.js` | SJ 氏の MHXX Switch セーブマネージャ（29.8 MB, 295k 行） | **日本語 0 文字**。英語版のみ |
| `C:\anyWorkspace\nx\mhxx\refs\mhx_data_manager` | 3DS 用 homebrew（C ソース） | 名前テーブル無し |
| `C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool` | Quedixx / Quest_Construct / new_quest | クエスト用。装備名無し |
| `C:\res\mhxx-exefs` | Switch の exefs コピー | 名前テーブル無し |
| `C:\anyWorkspace\3ds\mhxx_upd` | 3DS 版の更新コード（code.bin 等） | romfs 無し |
| `C:\roms\mhxx` | **空** | — |
| `%APPDATA%\Ryujinx\mods\contents\01006a800016e000` 等 | SSBU の mod 群 | 無関係 |

### 7.3 探索していない範囲（正直な限界）

- **ファイル名での全ドライブ走査はしていない**（ディレクトリ名スキャンのみ）。
  `*.gmd` / `*.arc` / `*.bin` 等のファイル名検索、および内容 grep は未実施。
  ただし装備名データの入り口になり得る場所（プロジェクト・エミュレータ・ROM・参照実装）は
  個別に全て開いて確認済み。
- G: ドライブはディレクトリ名スキャンで 0 件。**中身のファイル走査はしていない。**
- ネットワークドライブ・外付け・クラウドは対象外。

---

## 8. 参照実装（C#）の日本語併記について

**併記は無い。** `GameConstants.cs`（293,758 B）を全行走査した結果:

- 非 ASCII 行は **3 行のみ**。
- その 3 行は `PoogieCostumes`（ブーメラン…ではなくプーギー衣装）で、
  `"でぃあぶろすはぁと"`, `"ぷるぷるエアシップ"`, `"ガムートのきもち"` の 3 件だけ。
- **`Equip*` / `ItemNameList` / `SkillNames` / `JwlNames` / `KinsectNames` に日本語は一切ない。**
- コメント欄にも日本語なし。別配列・別ファイル（`.resx` 等）にも装備名の日本語は無い。

3 リポジトリ（`MHXXSaveEditor` / `MHXX-GUSaveEditor` / `MHXXSwitchSaveEditor`）の
`GameConstants.cs` は同一内容だったため、いずれにも日本語は無い。

**ライセンス**: `LICENSE` は 35,147 B = **GPL-3.0**。英語名の表はここから取っている。
日本語名を C# 実装から取る必要はもう無い（ROM が一次情報）。

---

## 9. ネット上の公開データ（第 3 項の調査）

日本語の装備名一覧そのものは多数あるが、**「ゲーム内 ID 順」かつ「ライセンスが明確」なものは
見つからなかった。**

| 候補 | 日本語名 | ゲーム内 ID | ライセンス | 判定 |
|---|---|---|---|---|
| `mhxx.kiranico.com` | ○ | **×**（URL の値は不透明なハッシュ。handoff で検証済み） | 不明 | 使えない |
| `mhx-wiki.com`（MHXX大剣一覧 等） | ○ | ×（武器ツリー順） | 不明（著作権留保） | 使えない |
| `kuroyonhon.com/mhxx/program/` | ○ | × | 不明 | 使えない |
| `github.com/mineminemine/MHXXSaveEditor` | ×（英語） | ○ | 要確認 | 不要 |
| `github.com/Bahtya/mhxxdoc` | △（**中国語**） | ○ | 要確認 | 日本語でない |
| `github.com/AngryChocobo/monster-hunter-web-data` | △（**中国語**） | ○ | 要確認 | 日本語でない |
| `github.com/Jemdot/mh-things/wiki/Weapon-IDs` | ×（英語） | ○ | Wiki | 日本語でない |

**結論**: ネット由来のデータを使う必要はない。ROM が一次情報で、ID 順も件数も検証済み。

---

## 10. 著作権・ライセンスの整理

今回のデータは**すべてユーザー自身が所有するゲームのダンプから抽出したもの**であり、
第三者が公開している文章・表をコピーしたものではない。

- 抽出元: ユーザーのローカル NSP（`nx\mhxx\rom\`）
- 抽出方法: 既にプロジェクトに同梱されていた `hactool 1.4.0` と既存の `prod.keys`
- 生成物: 名前と説明文のテキストを CSV 化したもの（`extract\jp_names\`）

**このリポジトリにコミットする際の方針は要判断**（下記 11.3）。
ゲーム内テキストをリポジトリに同梱することの是非は、本調査の範囲を超える法務判断であり、
「ROM から各自生成する」方式（スクリプトのみ同梱・生成物は `.gitignore`）が最も安全。

---

## 11. 実装への提案

### 11.1 そのまま差し替えられるもの

既存の英語表と件数・添字が完全一致するので、**配列の中身を日本語にするだけ**:

| 既存 | 件数 | 生成元 |
|---|---|---|
| `EQUIP_GREAT_SWORD_NAMES` | 137 | `weapons.csv` の type_id=0, form=0 |
| `EQUIP_SWORDN_SHIELD_NAMES` | 129 | type_id=1, form=0 |
| `EQUIP_HAMMER_NAMES` | 132 | type_id=2 |
| `EQUIP_LANCE_NAMES` | 127 | type_id=3 |
| `EQUIP_HEAVY_BOWGUN_NAMES` | 109 | type_id=4 |
| `EQUIP_LIGHT_BOWGUN_NAMES` | 110 | type_id=6 |
| `EQUIP_LONGSWORD_NAMES` | 127 | type_id=7 |
| `EQUIP_SWITCH_AXE_NAMES` | 110 | type_id=8 |
| `EQUIP_GUNLANCE_NAMES` | 115 | type_id=9 |
| `EQUIP_BOW_NAMES` | 108 | type_id=10 |
| `EQUIP_DUAL_BLADES_NAMES` | 126 | type_id=11 |
| `EQUIP_HUNTING_HORN_NAMES` | 112 | type_id=12 |
| `EQUIP_INSECT_GLAIVE_NAMES` | 83 | type_id=13 |
| `EQUIP_CHARGE_BLADE_NAMES` | 81 | type_id=14 |
| `EQUIP_TALISMAN_NAMES` | 11 | `amulet.csv` |
| `PALICO_WEAPON_NAMES` | 509 | `palico_weapon.csv` |
| `EQUIP_HEAD/CHEST/ARMS/WAIST/LEGS_NAMES` | 1204/1189/1158/1108/1177 | `armor_series.csv` を部位で絞り、`*_I_DS` の順に並べ替え |

**武器は `form=0` を使えば現状の挙動と完全に同じ（ID の意味も件数も不変）。**

### 11.2 段階的にやるなら

1. **まず武器だけ日本語化**（14 表・計 1,481 件）。ここが一番効果が大きく、検証も容易。
2. 次に防具（`armor_series.csv` + `*_I_DS` の並べ替え）。
3. 護石・オトモは件数が少ないので最後でよい。

### 11.3 生成物の扱い（要判断）

- **案 A（推奨）**: `tools/gen_jp_names.py` だけをリポジトリに入れ、生成した CSV / `gamedata.rs` は
  `.gitignore`。ROM を持っている人だけが生成できる。ゲーム内テキストの同梱を避けられる。
- **案 B**: 生成済みの日本語表をコミットする。実装は楽だが、ゲーム内テキストの再配布になる。
- どちらでも、**本レポートの抽出スクリプトはそのまま流用できる**（`extract\jp_names\gen.py`）。

### 11.4 表示の改善（任意）

`form` とレベルを使えば「強化段階に応じた名前」も出せる（4.3 節）。
ただし `form=3` の扱いが未確定なので、**まずは `form=0` 固定で出すのが安全。**

---

## 12. 未確定事項（推測と明記）

| # | 内容 | 状態 |
|---|---|---|
| 1 | `form=3` の表示名 | **推測**: `form=2` と同じか別テーブル。GMD に 4 組目が無いためそのまま引くとバグる。実機検証が必要 |
| 2 | 武器種 `05` が欠番である理由 | 未確認（14 種しか無いので欠番自体は自然） |
| 3 | `SKILL_NAMES` 306 件の正しさ | **未判定**。ROM は 328 件で並び順も違う。別途レビュー推奨 |
| 4 | 防具 ID = 系統 ID という結論 | 証拠は強い（最大 ID < 1,287、頭/胴の名前一致）が、**全 1,287 系統を総当たりでは照合していない** |
| 5 | G: ドライブの中身 | ディレクトリ名スキャンのみ。ファイル単位の走査は未実施 |
| 6 | 護石の「スキル名」 | `amuletData_jpn.gmd` は護石の**種類名 11 件のみ**。付くスキル名は `skillData_jpn.gmd` 側 |

---

## 13. 生成物・副作用の一覧

### 13.1 本レポート以外に作ったもの（既存ファイルの変更はゼロ）

| パス | 内容 | サイズ |
|---|---|---|
| `C:\anyWorkspace\nx\mhxx\extract\base_romfs\` | hactool で展開した romfs 全体（14,581 ファイル） | 9.77 GiB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\gen.py` | 抽出・CSV 生成スクリプト | 4.8 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\weapons.csv` | 武器名（14 種 × form 0-2）4,818 行 | 893 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\armor_series.csv` | 防具名（1,287 系統 × 5 部位）6,435 行 | 923 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\items.csv` | アイテム名 2,991 行 | 399 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\skills.csv` | スキル名 328 行 | 31 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\amulet.csv` | 護石名 11 行 | 1.5 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\palico_weapon.csv` | オトモ武器名 509 行 | 81 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\palico_armor.csv` | オトモ防具名（527 系統 × 2 部位） | 168 KB |
| `C:\anyWorkspace\nx\mhxx\extract\jp_names\weapon_counts.json` | 武器種ごとの件数 | 332 B |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\.work\_scan_c_dirs.txt` | C: のディレクトリ名スキャン結果（40 行） | — |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\.work\_scan_g_dirs.txt` | G: の同名スキャン結果（0 行） | — |

- 既存ファイルの変更: **なし**（エディタのソース・データは一切触っていない）
- 一時作業: `%TEMP%\mhxx_scratch\`（鍵の整形、GMD パーサ、検証スクリプト）
- ディスク消費: ROM 展開で約 9.8 GiB。不要なら `extract\base_romfs` を削除してよい
  （`jp_names\` の CSV は独立して残る）。

### 13.2 CSV のスキーマ

`weapons.csv`
```
type_id,type,weapon_id,form,levels,name,desc
0,大剣,1,0,"1;2;3;4;5",ベルダーブレイド,"龍歴院から提供された化石で…"
```
`levels` はその形態になるレベル一覧（`weaponNNLevelData` 由来。空 = 到達不能な形態）。

`armor_series.csv`
```
series_id,part,name,desc
1,頭,レザーヘッド,"モンスターの毛皮をなめして…"
```

`items.csv` / `skills.csv` / `amulet.csv` / `palico_weapon.csv`
```
id,name,desc
9,回復薬,…
```

CSV は `utf-8-sig`（BOM 付き、Excel でそのまま開ける）。説明文の改行は実データどおり `\n`。

---

## 14. 要点（まとめ）

1. **日本語装備名はこの PC の ROM から完全に取れる。** 追加のダウンロードも外部データも不要。
2. **既存の英語表と ID 空間・件数が完全一致（武器 14/14 種）。** 配列の中身を差し替えるだけで済む。
3. 武器は `weaponNNMsgData_jpn.gmd` の `strings[6*id]`、防具は `armorSeriesData_jpn.gmd` の
   `strings[10*id + 部位]`。
4. 参照実装（C#）に日本語は無い（非 ASCII はプーギー衣装 3 件のみ）。
5. ネットの日本語一覧は「ID 順」のものが無く、ライセンスも不明。使う必要はない。
6. 残タスクは「`form=3` の扱い」「`SKILL_NAMES` 306 件の妥当性」「再配布方針の決定」の 3 点。
