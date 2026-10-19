# deploy_sdcard.ps1 - put mhnxeditor and its data onto a Switch SD card.
#
# Examples:
#   .\deploy_sdcard.ps1 -Target E:\
#   .\deploy_sdcard.ps1 -Target E:\ -Save .\my_system -Quests .\my_quests
#   .\deploy_sdcard.ps1 -Target .\sdcard_test -Save "C:\path\to\3ds\system"
#
# The script never deletes anything. It only creates folders and copies files,
# and it converts a 3DS save into the Switch container pair when needed.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Target,
    [string]$Save,
    [string]$Quests,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

# --------------------------------------------------------------------------
# 1. make sure the artefacts exist
# --------------------------------------------------------------------------
$nro = Join-Path $root 'dist\mhnxeditor.nro'
$cli = Join-Path $root 'dist\mhnx.exe'

if (-not $SkipBuild) {
    if (-not (Test-Path $nro)) {
        Step 'mhnxeditor.nro not found - building'
        & (Join-Path $root 'switch\build.ps1')
    }
    if (-not (Test-Path $cli)) {
        Step 'mhnx.exe not found - building'
        Push-Location $root
        cargo build --release -p mhnx-cli
        Pop-Location
        Copy-Item (Join-Path $root 'target\release\mhnx.exe') $cli -Force
    }
}
if (-not (Test-Path $nro)) { throw "mhnxeditor.nro is missing and could not be built" }

# --------------------------------------------------------------------------
# 2. create the SD card layout
# --------------------------------------------------------------------------
$t = (Resolve-Path -LiteralPath $Target -ErrorAction SilentlyContinue)
if (-not $t) {
    # Allow a drive root or a folder that does not exist yet.
    if ($Target -match '^[A-Za-z]:\\?$') {
        if (-not (Test-Path $Target)) { throw "drive $Target is not available" }
        $t = $Target
    } else {
        New-Item -ItemType Directory -Force -Path $Target | Out-Null
        $t = (Resolve-Path -LiteralPath $Target).Path
    }
} else { $t = $t.Path }

$switchDir = Join-Path $t 'switch'
$dataDir   = Join-Path $switchDir 'mhnxeditor'
$savesDir  = Join-Path $dataDir 'saves'
$questDir  = Join-Path $dataDir 'quest'

Step "target: $t"
foreach ($d in @($switchDir, $dataDir, $savesDir, $questDir)) {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    Write-Host "    ok  $d"
}

# --------------------------------------------------------------------------
# 3. copy the homebrew
# --------------------------------------------------------------------------
Copy-Item $nro (Join-Path $switchDir 'mhnxeditor.nro') -Force
Write-Host ("    nro {0:N0} bytes" -f (Get-Item (Join-Path $switchDir 'mhnxeditor.nro')).Length)

# The diagnostics NRO: run this if the editor shows a blank screen.
$probe = Join-Path $root 'dist\mhnx_probe.nro'
if (Test-Path $probe) {
    Copy-Item $probe (Join-Path $switchDir 'mhnx_probe.nro') -Force
    Write-Host ("    probe {0:N0} bytes (diagnostics, only needed if something looks wrong)" -f (Get-Item (Join-Path $switchDir 'mhnx_probe.nro')).Length)
}

# --------------------------------------------------------------------------
# 4. save file
# --------------------------------------------------------------------------
if ($Save) {
    $src = (Resolve-Path -LiteralPath $Save).Path
    $len = (Get-Item $src).Length
    Step "save: $src ($len bytes)"

    switch ($len) {
        4726188 {
            Write-Host '    already a Switch MHXX save'
            Copy-Item $src (Join-Path $savesDir 'system') -Force
        }
        5159100 {
            Write-Host '    MHGU save - copying as-is (the editor supports MHGU)'
            Copy-Item $src (Join-Path $savesDir 'system') -Force
        }
        4726152 {
            if (-not (Test-Path $cli)) { throw 'mhnx.exe is required to convert a 3DS save' }
            Write-Host '    3DS save - converting to the Switch container'
            & $cli to-switch $src $savesDir
        }
        default {
            throw "unsupported save size $len (expected 4726152, 4726188 or 5159100)"
        }
    }
    Get-ChildItem $savesDir -File | ForEach-Object {
        Write-Host ("    {0,-16} {1:N0} bytes" -f $_.Name, $_.Length)
    }
}

# --------------------------------------------------------------------------
# 5. quest files
# --------------------------------------------------------------------------
if ($Quests) {
    Step "quests: $Quests"
    $files = @()
    $item = Get-Item -LiteralPath $Quests
    if ($item.PSIsContainer) {
        $files = Get-ChildItem -LiteralPath $Quests -Filter *.arc -File -Recurse
    } else {
        $files = @($item)
    }
    if ($files.Count -eq 0) { Write-Host '    no .arc files found' }
    foreach ($f in $files) {
        Copy-Item $f.FullName (Join-Path $questDir $f.Name) -Force
    }
    Write-Host ("    copied {0} quest file(s)" -f $files.Count)
}

# --------------------------------------------------------------------------
# 6. summary
# --------------------------------------------------------------------------
Step 'done. SD card layout:'
Get-ChildItem -Recurse -File $switchDir | ForEach-Object {
    $rel = $_.FullName.Substring($t.Length).TrimStart('\')
    Write-Host ("    {0,-52} {1,10:N0} B" -f $rel, $_.Length)
}
Write-Host ''
Write-Host 'Next: insert the SD card, start hbmenu, run mhnxeditor.nro.' -ForegroundColor Green
Write-Host 'In the editor:  A = open / save,  B = back,  X = step,  Y = save/quest switch,  + = exit'
