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
==========================================

Version : Alpha v0.1.0
Author  : Nexy451z

!! This is an alpha build. Nothing is guaranteed to work.
!! Back up your save before using it, and keep the copy.


What is in this zip
-------------------
  switch/XXSE/XXSE.nro   Switch homebrew. Extract this zip to the root of
                         your SD card so the path becomes
                         /switch/XXSE/XXSE.nro, then start it from hbmenu.
  XXSE.exe               PC build (Windows). No installer, no DLLs.


Requirements (Switch)
---------------------
  - Atmosphre (custom firmware) on a console that can already run homebrew
  - Firmware 21 or newer needs a recent libnx; this build uses 4.10+
  - Start it from hbmenu. Launching from the album works too, but the system
    keyboard is unavailable there, so use the built-in one.


Backups
--------
  Before it writes to the console save, the tool copies what is there to
  /switch/mhnxeditor/backups/ on the SD card. If a write fails, that copy is
  what you restore from. On the PC side a .bak is left next to any file it
  overwrites.


About the download quest feature
--------------------------------
  This release does NOT include any game data. The built-in list of official
  download quests is produced by tools/gen_official_dlc.py from your own copy
  of the game; without it the tool works normally and that one convenience
  feature is simply unavailable.

  Edit .arc files, change your save, install quests you already have - all of
  that works. Only the 'import every official quest' shortcut needs the data.


Licence
-------
  MIT. See the LICENSE file in the repository.


No warranty
------------
  Provided as is, with no warranty of any kind. This is an unofficial tool
  and is not affiliated with or endorsed by Capcom or Nintendo. Use at your
  own risk; a save editor can destroy a save.
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
