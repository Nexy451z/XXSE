# 日本語名テーブル差し替えの引き継ぎ

このドキュメントは「アイテム名・スキル名などを日本語にしたい」作業を別の担当に渡すためのものです。
**英語名の表はすでに完成しており、それと同じ並び順の日本語表を作れば差し替えだけで済みます。**

## 1. いま入っている英語表の場所

### アイテム名（アイテム ID から名前を引く）

- ファイル: `crates/mhnx-core/src/gamedata.rs`
- 行: **6 行目**
- 定義: `pub static ITEM_NAME_LIST: &[&str] = &[...]`
- 要素数: **2991**
- 添字の意味: **配列の添字 = ゲーム内アイテム ID**（0 始まり。0 は `\"-----\"` = 空き）

### スキル名

- ファイル: `crates/mhnx-core/src/skills.rs`
- 行: **9 行目**
- 定義: `pub static SKILL_NAMES: &[&str]`
- 添字の意味: **スキル ID から 1 を引いた値**（ID は 1 始まり）
- 変換関数: `skill_name(id: u8)` @ 319 行

### その他の装備名

`gamedata.rs` の 9〜99 行に以下があります。すべて「英語名の配列」と「(必要なものは) ID の配列」の対です。

```
 9  EQUIPMENT_TYPES          装備部位
12  PALICO_EQUIP             オトモ装備種別
15  EQUIP_GREAT_SWORD_NAMES  大剣
18  EQUIP_SWORDN_SHIELD_NAMES
21  EQUIP_DUAL_BLADES_NAMES
24  EQUIP_LONGSWORD_NAMES
27  EQUIP_INSECT_GLAIVE_NAMES
30  EQUIP_HAMMER_NAMES
33  EQUIP_HUNTING_HORN_NAMES
36  EQUIP_BOW_NAMES
39  EQUIP_HEAVY_BOWGUN_NAMES
42  EQUIP_LIGHT_BOWGUN_NAMES
45  EQUIP_CHARGE_BLADE_NAMES
48  EQUIP_SWITCH_AXE_NAMES
51  EQUIP_LANCE_NAMES
54  EQUIP_GUNLANCE_NAMES
57  PALICO_WEAPON_NAMES
60  EQUIP_HEAD_NAMES   63 EQUIP_HEAD_I_DS
66  EQUIP_CHEST_NAMES  69 EQUIP_CHEST_I_DS
72  EQUIP_ARMS_NAMES   75 EQUIP_ARMS_I_DS
78  EQUIP_WAIST_NAMES  81 EQUIP_WAIST_I_DS
84  EQUIP_LEGS_NAMES   87 EQUIP_LEGS_I_DS
90  PALICO_HEAD_NAMES  93 PALICO_HEAD_I_DS
96  PALICO_ARMOR_NAMES 99 PALICO_ARMOR_I_DS
```

**`*_I_DS` が付いている表は「名前の添字と装備 ID が一致しません」。** ID 配列とセットで扱ってください。

## 2. この英語表の出どころ（元ネタ）

```
C:\anyWorkspace\nx\mhxx\save_editor\repos\MHXXSwitchSaveEditor\MHXXSaveEditor\Data\GameConstants.cs
```

**6 行目** の `public static readonly string[] ItemNameList = {...}` が元です。要素数 2991 で、我々の表と完全に一致します。

同ファイルには**日本語化したい表が他にも大量に**あります（すべて英語）。

```
ItemNameList, EquipmentTypes, PalicoEquip,
EquipGreatSwordNames, EquipSwordnShieldNames, EquipDualBladesNames, EquipLongswordNames,
EquipInsectGlaiveNames, EquipHammerNames, EquipHuntingHornNames, EquipBowNames,
EquipHeavyBowgunNames, EquipLightBowgunNames, EquipChargeBladeNames, EquipSwitchAxeNames,
EquipLanceNames, EquipGunlanceNames, PalicoWeaponNames,
EquipHeadNames, EquipChestNames, EquipArmsNames, EquipWaistNames, EquipLegsNames,
PalicoHeadNames, PalicoArmorNames, EquipTalismanNames,
TalismanMysterySkill, TalismanShiningSkill, TalismanTimewornSkill, TalismanEnduringSkill,
JwlNames, SkillNames, KinsectNames,
PalicoForte, PalicoTarget, PalicoSupportMoves, PalicoSkills, ...
```

同じファイルが 3 つのリポジトリに重複してあります（中身は同一）。

```
C:\anyWorkspace\nx\mhxx\save_editor\repos\MHXXSaveEditor\MHXXSaveEditor\Data\GameConstants.cs
C:\anyWorkspace\nx\mhxx\save_editor\repos\MHXX-GUSaveEditor\MHXXSaveEditor\Data\GameConstants.cs
C:\anyWorkspace\nx\mhxx\save_editor\repos\MHXXSwitchSaveEditor\MHXXSaveEditor\Data\GameConstants.cs
```

## 3. 必要な成果物

### アイテム名

`crates/mhnx-core/src/gamedata.rs` の `ITEM_NAME_LIST` を、
**要素数 2991・同じ並び順のまま**日本語に置き換えるだけです。

```rust
pub static ITEM_NAME_LIST: &[&str] = &[
    "-----",          // ID 0  (空き)
    "調合書①入門編",   // ID 1  ← 既存は "Book of Combos 1"
    "調合書②初級編",   // ID 2
    // ... 2991 個ぶん、既存の並びを崩さずに ...
    "回復薬",          // ID 9  ← 既存は "Potion"
    "回復薬グレート",   // ID 10 ← 既存は "Mega Potion"
];
```

**2991 行すべてを並べる必要があります。** 上は一部だけを示した省略形です。
既存の英語配列をコピーして、文字列だけを置き換えるのが安全です。

**並び順を 1 つでも間違えると、別のアイテム名が表示されます。** 要素数の確認を必ずしてください。

### スキル名

`crates/mhnx-core/src/skills.rs` の `SKILL_NAMES` を同じ添字で日本語化。

**注意**: こちらは MHGU Save Manager 由来なので、`GameConstants.cs` の `SkillNames` とは
**並び順が違う可能性があります**。既存の順序を維持してください。混ぜないでください。

## 4. 表示側（変更不要）

| 場所 | 役割 |
|---|---|
| `crates/mhnx-gui/src/app.rs:2289` | `pub fn item_name(id: u16) -> String` 表を引くだけ |
| `crates/mhnx-gui/src/app.rs:1082` | アイテム名検索ピッカー（部分一致） |
| `crates/mhnx-gui/src/app.rs:1441` | 報酬/支給品 (.rem/.sup) の一覧表示 |
| `crates/mhnx-cli/src/main.rs:347` | CLI の `items` コマンド |
| `crates/mhnx-core/src/skills.rs:319` | `skill_name(id)` |

**表を差し替えれば、これらは自動的に日本語になります。** コードの変更は不要です。

## 5. 検証手順

```powershell
cd C:\anyWorkspace\nx\mhxx\mhnxeditor
cargo test --workspace
cargo run --release -p mhnx-cli -- items "C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system"
```

要素数が変わっていないことの確認（**必須**）:

```powershell
$rs = Get-Content crates\mhnx-core\src\gamedata.rs -Raw
$m = [regex]::Match($rs, 'ITEM_NAME_LIST: &\[&str\] = &\[(?s)(.*?)\];')
([regex]::Matches($m.Groups[1].Value, '"(?:[^"\\]|\\.)*"')).Count
# 2991 になること
```

## 6. つまずいた点（重要）

### kiranico の URL ID はゲーム内 ID ではない

`https://mhxx.kiranico.com/item/05b35`（回復薬）のような URL の値は**不透明なハッシュ**で、
ゲーム内アイテム ID とは無関係でした。連番でもビット反転でもありません。
詳細ページにも数値 ID はありません。

**kiranico からの ID 対応表作成は諦めてください。** 名前だけ欲しいなら使えますが、ID と紐付けられません。

### 名前が取れそうな他の候補

- 日本語版のセーブエディタ（`Jiapeiyao/MMHXX_SaveEditor` 等）のリソース
- Wiki の一覧を ID 付きでダンプしたもの
- `Quedixx.exe` / `Quest_Construct.exe` の文字列（`C:\anyWorkspace\nx\mhxx\save_editor\analysis\quetool_strings\` に抽出済み）
- ゲーム本体のメッセージデータ

## 7. 関連ファイル

```
C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\gamedata.rs   アイテム・装備名
C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\skills.rs     スキル名
C:\anyWorkspace\nx\mhxx\mhnxeditor\tools\gen_skills.py                skills.rs の生成元
```