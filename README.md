# XXSE

Monster Hunter XX (3DS / Switch) のセーブエディタです。

## 動作環境

| 版 | 動くところ |
|---|---|
| XXSE.nro | Nintendo Switch（Atmosphere 1.12 以降 / ファーム 23.0.1 で確認） |
| XXSE.exe | Windows 10 / 11 |

## できること

- セーブ編集（名前・所持金・HR・アイテム・お守り・装備・狩猟記録・ギルドカード・オトモ・マイセット）
- クエスト (.arc) の編集とセーブへの導入
- 3DS ↔ Switch の形式変換
- 実機の本体セーブを直接編集

## 配置

```
SD カードのルート/
└─ switch/
   └─ XXSE/
      └─ XXSE.nro
```

hbmenu から起動します。PC 版は XXSE.exe をそのまま実行してください。

## ビルド

```powershell
# PC 版
cargo build --release -p mhnx-gui -p mhnx-cli

# Switch 版
.\switch\build.ps1

# リリース用 zip
pwsh -File .\tools\make_release.ps1
```

Switch 版のビルドには devkitPro（libnx）と rustup の `aarch64-nintendo-switch-libnx` が必要です。

## 注意

書き込み前に `/switch/mhnxeditor/backups/` へ控えを作ります。セーブは必ずバックアップを取ってから使ってください。

## ライセンス

MIT

---

ゲームファイルは含まれません。
