<#
.SYNOPSIS
    Builds XXSE.nro (Nintendo Switch homebrew) from the Rust crate in this folder.

.DESCRIPTION
    Route "cargo" (default)
        rustc compiles the crate for the custom target
        targets/aarch64-nintendo-switch-libnx.json (nightly + -Z build-std, see
        .cargo/config.toml) and drives devkitA64 aarch64-none-elf-gcc as the linker.
        The extra link arguments (switch.specs, -lnx, libc, ...) come from build.rs.

    Route "c"
        The same crate is built as a staticlib (libmhnx_switch.a) and linked by the
        devkitA64 gcc driver together with tools/rustmain.c, exactly like a devkitPro
        Makefile project would. Both routes produce the same .nro.

    devkitPro must be installed; the path is taken from -DevkitPro, then $env:DEVKITPRO,
    then C:/devkitPro. The nightly toolchain + rust-src component are installed
    automatically when missing (disable with -NoInstall).

.EXAMPLE
    pwsh -File .\build.ps1
.EXAMPLE
    pwsh -File .\build.ps1 -Clean -Route c
#>
[CmdletBinding()]
param(
    [string] $DevkitPro = $env:DEVKITPRO,
    [ValidateSet('cargo', 'c')] [string] $Route = 'cargo',
    [ValidateSet('release', 'debug')] [string] $Profile = 'release',
    [switch] $Clean,
    [switch] $NoInstall,
    # Also build the diagnostics NRO (examples/text_probe.rs) next to the editor.
    # Run it on hardware when the editor shows a blank screen: it prints what the
    # shared-font / framebuffer path is doing.
    [switch] $Probe
)

$ErrorActionPreference = 'Stop'

$switchDir  = $PSScriptRoot
$repoRoot   = Split-Path -Parent $switchDir
$distDir    = Join-Path $repoRoot 'dist'
$targetJson = Join-Path $switchDir 'targets\aarch64-nintendo-switch-libnx.json'
$targetName = 'aarch64-nintendo-switch-libnx'
$appName    = 'XXSE'
# Shown by hbmenu: one line of title, one of author.
$appTitle   = 'XXSE'
$appAuthor  = 'Nexy451z'
$appVersion = '1.0.0'

function Step([string] $Message) { Write-Host ""; Write-Host "==> $Message" -ForegroundColor Cyan }
function Info([string] $Message) { Write-Host "    $Message" }
function Die([string] $Message)  { Write-Host "ERROR: $Message" -ForegroundColor Red; exit 1 }

function Invoke-Tool([string] $Exe, [string[]] $ToolArgs) {
    & $Exe @ToolArgs
    if ($LASTEXITCODE -ne 0) { Die "$Exe failed with exit code $LASTEXITCODE" }
}

# ---------------------------------------------------------------------------
# 1. devkitPro
# ---------------------------------------------------------------------------
Step 'devkitPro'
# DEVKITPRO is usually the msys2-style path (/opt/devkitpro) that devkitPro own
# shell uses, so it has to be translated to a Windows path first. Try every
# candidate and use the first one that actually contains libnx.
function Test-DevkitPro([string] $Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { return $false }
    return (Test-Path (Join-Path ($Path.TrimEnd('\', '/') -replace '/', '\') 'libnx'))
}
$candidates = @()
if (-not [string]::IsNullOrWhiteSpace($DevkitPro)) { $candidates += $DevkitPro }
if (-not [string]::IsNullOrWhiteSpace($env:DEVKITPRO)) { $candidates += $env:DEVKITPRO }
$candidates += 'C:\devkitPro'
foreach ($candidate in @($candidates)) {
    $trimmed = $candidate.TrimEnd('\', '/').Replace('\', '/')
    if ($trimmed -match '^/(opt/)?(.*)$') {
        $rest = $matches[2]
        foreach ($drive in (Get-PSDrive -PSProvider FileSystem | Select-Object -ExpandProperty Name)) {
            $candidates += "$($drive):\$rest"
        }
    }
}
$resolved = $null
foreach ($candidate in $candidates) {
    if (Test-DevkitPro $candidate) { $resolved = $candidate.TrimEnd('\', '/').Replace('\', '/'); break }
}
if (-not $resolved) {
    Die "devkitPro not found (tried: $($candidates -join ', ')). Pass -DevkitPro <path> or set the DEVKITPRO environment variable."
}
$DevkitPro = $resolved
Info "devkitPro  = $DevkitPro"
$gcc      = Join-Path $DevkitPro 'devkitA64\bin\aarch64-none-elf-gcc.exe'
$elf2nro  = Join-Path $DevkitPro 'tools\bin\elf2nro.exe'
$nacptool = Join-Path $DevkitPro 'tools\bin\nacptool.exe'
foreach ($tool in @($gcc, $elf2nro, $nacptool)) {
    if (-not (Test-Path $tool)) { Die "missing devkitPro tool: $tool" }
}

# devkitPro's specs expand paths with %:getenv(DEVKITPRO ...), so forward slashes
# keep gcc happy on Windows.
$devkitProFwd = $DevkitPro.Replace('\', '/')
$env:DEVKITPRO = $devkitProFwd
$env:DEVKITA64 = "$devkitProFwd/devkitA64"
$env:PATH = "$(Join-Path $DevkitPro 'devkitA64\bin');$(Join-Path $DevkitPro 'tools\bin');$env:PATH"
Info "DEVKITPRO = $env:DEVKITPRO"

# ---------------------------------------------------------------------------
# 2. Rust toolchain (nightly + rust-src: required for a tier-3 target / -Z build-std)
# ---------------------------------------------------------------------------
Step 'Rust toolchain'
$nightly = (& rustup toolchain list) | Select-String -Pattern '^nightly'
if (-not $nightly) {
    if ($NoInstall) { Die 'nightly toolchain missing. Run: rustup toolchain install nightly --profile minimal --component rust-src' }
    Info 'installing nightly toolchain (rust-src included) ...'
    Invoke-Tool 'rustup' @('toolchain', 'install', 'nightly', '--profile', 'minimal', '--component', 'rust-src')
}
$rustSrc = (& rustup component list --toolchain nightly --installed) | Select-String -Pattern '^rust-src'
if (-not $rustSrc) {
    if ($NoInstall) { Die 'rust-src component missing for nightly. Run: rustup component add rust-src --toolchain nightly' }
    Info 'adding rust-src component ...'
    Invoke-Tool 'rustup' @('component', 'add', 'rust-src', '--toolchain', 'nightly')
}
Info ((& rustc +nightly --version) -join '')

# ---------------------------------------------------------------------------
# 3. Compile the crate
# ---------------------------------------------------------------------------
Push-Location $switchDir
try {
    if ($Clean) { Step 'cargo clean'; Invoke-Tool 'cargo' @('clean') }

    Step "cargo build ($Profile) for $targetName"
    $cargoArgs = @('+nightly', 'build', '--target', $targetJson)
    if ($Profile -eq 'release') { $cargoArgs += '--release' }
    Invoke-Tool 'cargo' $cargoArgs
}
finally { Pop-Location }

$outDir    = Join-Path $switchDir "target\$targetName\$Profile"
$staticlib = Join-Path $outDir 'libmhnx_switch.a'
if (-not (Test-Path $staticlib)) { Die "staticlib not found: $staticlib" }

# ---------------------------------------------------------------------------
# 4. Link
# ---------------------------------------------------------------------------
Step "Link ($Route route)"
$elf  = Join-Path $outDir "$appName.elf"
$arch = @('-march=armv8-a+crc+crypto', '-mtune=cortex-a57', '-mtp=soft', '-fPIE')
$linkCommon = @("-specs=$devkitProFwd/libnx/switch.specs", "-g", "-L", "$devkitProFwd/libnx/lib") + $arch

if ($Route -eq 'cargo') {
    # Route A: rustc produced the ELF directly (build.rs adds the same flags).
    $built = Join-Path $outDir $appName
    if (-not (Test-Path $built)) { Die "ELF not found: $built" }
    Copy-Item $built $elf -Force
} else {
    # Route B: devkitA64 gcc links the Rust staticlib, like a devkitPro Makefile.
    $cMain = Join-Path $switchDir 'tools\rustmain.c'
    if (-not (Test-Path $cMain)) { Die "missing $cMain" }
    $obj = Join-Path $outDir 'rustmain.o'
    Invoke-Tool $gcc (@('-c', '-g') + $arch + @('-O2', '-o', $obj, $cMain))
    Invoke-Tool $gcc ($linkCommon + @('-o', $elf, $obj, $staticlib, '-lnx'))
}

# ---------------------------------------------------------------------------
# 5. control.nacp + elf2nro
# ---------------------------------------------------------------------------
Step 'nacptool'
$nacp = Join-Path $outDir "$appName.nacp"
Invoke-Tool $nacptool @('--create', $appTitle, $appAuthor, $appVersion, $nacp)

Step 'elf2nro'
$nro = Join-Path $outDir "$appName.nro"
# Use our own icon when present; fall back to libnx's default.
$icon = Join-Path $PSScriptRoot 'icon.jpg'
if (-not (Test-Path $icon)) { $icon = "$devkitProFwd/libnx/default_icon.jpg" }
Invoke-Tool $elf2nro @($elf, $nro, "--icon=$icon", "--nacp=$nacp")

# ---------------------------------------------------------------------------
# 6. Verify the NRO container
# ---------------------------------------------------------------------------
Step 'Verify'
$bytes = [System.IO.File]::ReadAllBytes($nro)
if ($bytes.Length -lt 0x40) { Die "file too small to be an NRO: $nro" }
$homebrew = [System.Text.Encoding]::ASCII.GetString($bytes, 8, 8)
# NroStart (16 bytes: unused, mod_offset, padding) comes first, then NroHeader.magic.
$magic = [System.Text.Encoding]::ASCII.GetString($bytes, 16, 4)
if ($magic -ne "NRO0") { Die "not an NRO (NroHeader.magic = '$magic', expected NRO0)" }
if ($homebrew -ne "HOMEBREW") { Write-Host "    warning: NroStart marker is '$homebrew' (expected HOMEBREW)" }
$modOffset = [BitConverter]::ToUInt32($bytes, 4)
$nroSize   = [BitConverter]::ToUInt32($bytes, 24)
Info ("NroStart  : unused=0x{0:X8} mod_offset=0x{1:X} marker={2}" -f [BitConverter]::ToUInt32($bytes, 0), $modOffset, $homebrew)
Info ("NroHeader : magic={0} size=0x{1:X} ({1} bytes)" -f $magic, $nroSize)
Info ("file size : {0} bytes" -f $bytes.Length)
if ($modOffset + 4 -le $bytes.Length) {
    # mod_offset points at the MOD0 metadata block linked in by --nx-module-name.
    Info ("mod0      : {0} @ 0x{1:X}" -f [System.Text.Encoding]::ASCII.GetString($bytes, $modOffset, 4), $modOffset)
}

# ---------------------------------------------------------------------------
# 7. Stage the deliverables
# ---------------------------------------------------------------------------
Step "Stage -> $distDir"
if (-not (Test-Path $distDir)) { New-Item -ItemType Directory -Path $distDir | Out-Null }
$distNro = Join-Path $distDir 'XXSE.nro'
Copy-Item $nro $distNro -Force
$sdcard = Join-Path $switchDir 'sdcard'
if (Test-Path $sdcard) {
    Copy-Item (Join-Path $sdcard '*') $distDir -Recurse -Force
}
Info $distNro
Info (((Get-Item $distNro).Length).ToString() + " bytes")

# ---------------------------------------------------------------------------
# 8. Optional diagnostics NRO
# ---------------------------------------------------------------------------
if ($Probe) {
    Step 'Build the text probe example'
    Push-Location $switchDir
    try {
        & cargo build --profile $Profile --example text_probe 2>&1 | Write-Host
    } finally {
        Pop-Location
    }
    $probeElf = Join-Path $outDir 'examples\text_probe'
    if (-not (Test-Path $probeElf)) { Die "probe ELF not found: $probeElf" }
    $probeNacp = Join-Path $outDir 'mhnx_probe.nacp'
    Invoke-Tool $nacptool @('--create', 'XXSE probe', $appAuthor, $appVersion, $probeNacp)
    $probeNro = Join-Path $outDir 'mhnx_probe.nro'
    Invoke-Tool $elf2nro @($probeElf, $probeNro, "--icon=$devkitProFwd/libnx/default_icon.jpg", "--nacp=$probeNacp")
    if (-not (Test-Path $distDir)) { New-Item -ItemType Directory -Path $distDir | Out-Null }
    $distProbe = Join-Path $distDir 'mhnx_probe.nro'
    Copy-Item $probeNro $distProbe -Force
    $pb = [System.IO.File]::ReadAllBytes($distProbe)
    $pmagic = [System.Text.Encoding]::ASCII.GetString($pb, 16, 4)
    if ($pmagic -ne 'NRO0') { Die "probe is not an NRO (magic '$pmagic')" }
    Info ("probe nro : {0} bytes (NRO0 ok)" -f $pb.Length)
    Info $distProbe
}

Write-Host ''
Write-Host 'OK: XXSE.nro built.' -ForegroundColor Green
Write-Host "    ELF : $elf"
Write-Host "    NRO : $distNro"
Write-Host '    Copy dist/XXSE.nro and dist/switch/ to the SD card root, then start it from hbmenu.'
