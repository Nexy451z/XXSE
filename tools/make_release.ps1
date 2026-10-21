#Requires -Version 5.1
<#
  make_release.ps1 - pack the build into a release zip.

  Layout, the same shape other Switch homebrew uses:

    XXSE-Alpha-v0.1.0.zip
      XXSE-Alpha-v0.1.0/
        switch/XXSE/XXSE.nro     <- extract to the SD card root
        XXSE.exe                 <- PC build
        README.txt

  Usage: pwsh -File .\tools\make_release.ps1
#>
[CmdletBinding()]
param(
    [string]$Version = 'Alpha v0.1.0',
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = 'Stop'
$dist = Join-Path $Root 'dist'
$nro  = Join-Path $dist 'XXSE.nro'
$exe  = Join-Path $dist 'XXSE.exe'
foreach ($f in @($nro, $exe)) {
    if (-not (Test-Path $f)) { throw "missing: $f (build first)" }
}

# The bundle of official quests must not reach a release: it is game data.
# Detect it by the names inside the quest archives the blob carries.
foreach ($f in @($nro, $exe)) {
    $bytes = [System.IO.File]::ReadAllBytes($f)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    if ($text.Contains('questData_')) {
        throw "$f contains the official quest bundle. Rebuild without dlc_bundle_gen.rs and official_dlc.bin present."
    }
}

$name = "XXSE-$($Version.Replace(' ', '-'))"
$outDir = Join-Path $Root 'release'
$stage  = Join-Path $outDir $name
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

# The console half, laid out exactly as it must sit on the card.
$sdApp = Join-Path $stage 'switch\XXSE'
New-Item -ItemType Directory -Force -Path $sdApp | Out-Null
Copy-Item $nro (Join-Path $sdApp 'XXSE.nro') -Force

# The PC half.
Copy-Item $exe (Join-Path $stage 'XXSE.exe') -Force

# Anything extra the build stages for the SD card root.
$sdcard = Join-Path $Root 'switch\sdcard'
if (Test-Path $sdcard) {
    Copy-Item (Join-Path $sdcard '*') $stage -Recurse -Force
}

$readme = @'
XXSE - Monster Hunter XX (NX) Save Editor
Alpha v0.1.0

Monster Hunter XX (3DS / Switch) のセーブエディタです。

動作環境
--------
  XXSE.nro  Nintendo Switch (Atmosphere 1.12 以降 / ファーム 23.0.1 で確認)
  XXSE.exe  Windows 10 / 11

配置
----
  SD カードのルートに展開してください。

    SD カードのルート/
    └─ switch/
       └─ XXSE/
          └─ XXSE.nro

  hbmenu から起動します。PC 版は XXSE.exe をそのまま実行してください。

ビルド
------
  cargo build --release -p mhnx-gui -p mhnx-cli   PC 版
  .\switch\build.ps1                             Switch 版
  pwsh -File .\tools\make_release.ps1            リリース用 zip

  Switch 版のビルドには devkitPro (libnx) と rustup の
  aarch64-nintendo-switch-libnx が必要です。

注意
----
  書き込み前に /switch/mhnxeditor/backups/ へ控えを作ります。
  セーブは必ずバックアップを取ってから使ってください。

ライセンス: MIT

---

ゲームファイルは含まれません。
'@
$readme | Set-Content (Join-Path $stage 'README.txt') -Encoding utf8

$zip = Join-Path $outDir "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal

Remove-Item -Recurse -Force $stage

Write-Output "$zip"
Write-Output ((Get-Item $zip).Length.ToString() + ' bytes')

# Show what went in, so it can be checked before uploading.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$z = [System.IO.Compression.ZipFile]::OpenRead($zip)
foreach ($e in $z.Entries) { Write-Output ('  ' + $e.FullName + '  ' + $e.Length) }
$z.Dispose()
