$ErrorActionPreference = "Stop"

# Repo-Root und Crate-Verzeichnis ermitteln
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (Test-Path (Join-Path $ScriptDir "mod4win-rs/Cargo.toml")) {
    $RootDir = $ScriptDir
    $CrateDir = Join-Path $ScriptDir "mod4win-rs"
} elseif (Test-Path (Join-Path $ScriptDir "Cargo.toml")) {
    $CrateDir = $ScriptDir
    $RootDir = Split-Path -Parent $ScriptDir
} else {
    throw "Cargo.toml konnte nicht gefunden werden."
}

if ($IsLinux) {
    $env:PATH = "/usr/local/bin:$HOME/.cargo/bin:$([System.IO.Path]::PathSeparator)$env:PATH"
    $env:XWIN_ARCH = "x86,x86_64"
}

$TargetDir = if ($env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR
} elseif ($IsLinux -and (Test-Path "/var/tmp/mod4win-target")) {
    "/var/tmp/mod4win-target"
} else {
    Join-Path $CrateDir "target"
}

$WinBin = if ($IsWindows) {
    Join-Path $TargetDir "release/mod4win-rs.exe"
} else {
    Join-Path $TargetDir "x86_64-pc-windows-msvc/release/mod4win-rs.exe"
}
$LinX64   = Join-Path $TargetDir "x86_64-unknown-linux-gnu/release/mod4win-rs"
$LinArm64 = Join-Path $TargetDir "aarch64-unknown-linux-gnu/release/mod4win-rs"

# 1. Prüfen, ob alle drei Release-Binaries existieren (ggf. automatisch bauen)
if (-not (Test-Path $WinBin)) {
    if ($IsWindows) {
        Write-Host "[Build] Windows x64 Release-Binary fehlt – starte cargo build --release..." -ForegroundColor Yellow
        cargo build --release --manifest-path (Join-Path $CrateDir "Cargo.toml")
    } else {
        Write-Host "[Build] Windows x64 Release-Binary fehlt – starte cargo xwin build --release --target x86_64-pc-windows-msvc..." -ForegroundColor Yellow
        cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path (Join-Path $CrateDir "Cargo.toml")
    }
    if ($LASTEXITCODE -ne 0) { throw "Windows Release-Build fehlgeschlagen!" }
}

if ((-not (Test-Path $LinX64)) -or (-not (Test-Path $LinArm64))) {
    Write-Host "[Build] Linux Release-Binaries fehlen – starte build-linux.ps1..." -ForegroundColor Yellow
    $BuildLinuxScript = Join-Path $CrateDir "build-linux.ps1"
    if (-not (Test-Path $BuildLinuxScript)) {
        $BuildLinuxScript = Join-Path $RootDir "build-linux.ps1"
    }
    & $BuildLinuxScript
    if ($LASTEXITCODE -ne 0) { throw "Linux Cross-Build fehlgeschlagen!" }
}

# 2. dist/-Ordner anlegen und Einzel-Binaries kopieren
$DistDir = Join-Path $RootDir "dist"
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

$DistWin   = Join-Path $DistDir "mod4win-win-x64.exe"
$DistX64   = Join-Path $DistDir "mod4win-linux-x64"
$DistArm64 = Join-Path $DistDir "mod4win-linux-arm64"
$DistUniv  = Join-Path $DistDir "mod4win-universal.exe"

Copy-Item -Path $WinBin   -Destination $DistWin   -Force
Copy-Item -Path $LinX64   -Destination $DistX64   -Force
Copy-Item -Path $LinArm64 -Destination $DistArm64 -Force

Write-Host "[Dist] Einzel-Binaries nach '$DistDir' kopiert:" -ForegroundColor Cyan
Write-Host "  - mod4win-win-x64.exe   ($([math]::Round((Get-Item $DistWin).Length / 1MB, 2)) MB)"
Write-Host "  - mod4win-linux-x64     ($([math]::Round((Get-Item $DistX64).Length / 1MB, 2)) MB)"
Write-Host "  - mod4win-linux-arm64   ($([math]::Round((Get-Item $DistArm64).Length / 1MB, 2)) MB)"

# 3. Single-File Polyglot-Binary (PE + POSIX /bin/sh + ELF x86_64 + ELF aarch64) erzeugen
[byte[]]$PeBytes    = [System.IO.File]::ReadAllBytes($DistWin)
[byte[]]$ElfX64     = [System.IO.File]::ReadAllBytes($DistX64)
[byte[]]$ElfArm64   = [System.IO.File]::ReadAllBytes($DistArm64)

# Kurzen Build-Hash (8 Hex-Zeichen aus SHA256 über beide Linux-Binaries) für Cache-Invalidierung berechnen
$Sha256 = [System.Security.Cryptography.SHA256]::Create()
$CombinedHashInput = New-Object byte[] ($ElfX64.Length + $ElfArm64.Length)
[Array]::Copy($ElfX64, 0, $CombinedHashInput, 0, $ElfX64.Length)
[Array]::Copy($ElfArm64, 0, $CombinedHashInput, $ElfX64.Length, $ElfArm64.Length)
$HashBytes = $Sha256.ComputeHash($CombinedHashInput)
$BuildId = ([BitConverter]::ToString($HashBytes).Replace("-", "").Substring(0, 8)).ToLowerInvariant()

# 1-basierter Offset des Shell-Skripts direkt hinter dem Windows-PE-Binary
$ShOffset1 = $PeBytes.Length + 1
$X64Len    = $ElfX64.Length
$Arm64Len  = $ElfArm64.Length

# Zweistufige Berechnung, damit die im Skript eingetragenen 1-basierten Offsets exakt mit der Skriptlänge übereinstimmen
function New-DispatcherScript([long]$X64Off1, [long]$Arm64Off1) {
    $script = @"
#!/bin/sh
set -e
ARCH=`$(uname -m)
BUILD_ID="$BuildId"
CACHE_DIR="`${XDG_CACHE_HOME:-`$HOME/.cache}/mod4win"
mkdir -p "`$CACHE_DIR"
case "`$ARCH" in
    x86_64|amd64)
        OFFSET=$X64Off1
        LEN=$X64Len
        ;;
    aarch64|arm64)
        OFFSET=$Arm64Off1
        LEN=$Arm64Len
        ;;
    *)
        echo "MOD4WIN: Nicht unterstuetzte Linux-Architektur: `$ARCH" >&2
        exit 1
        ;;
esac
CACHE_BIN="`$CACHE_DIR/mod4win-`$ARCH-`$BUILD_ID"
if [ ! -x "`$CACHE_BIN" ] || [ "`$(wc -c < "`$CACHE_BIN" 2>/dev/null || echo 0)" -ne "`$LEN" ]; then
    tail -c +`$OFFSET "`$0" | head -c `$LEN > "`$CACHE_BIN.tmp"
    chmod +x "`$CACHE_BIN.tmp"
    mv "`$CACHE_BIN.tmp" "`$CACHE_BIN"
fi
exec "`$CACHE_BIN" "`$@"
"@
    return $script.Replace("`r`n", "`n")
}

# Iteration bis zur Fixpunkt-Länge des Shell-Skripts
$ShText = New-DispatcherScript 10000000 30000000
for ($i = 0; $i -lt 4; $i++) {
    $ShBytesTemp = [System.Text.Encoding]::ASCII.GetBytes($ShText)
    $X64Offset1  = $PeBytes.Length + $ShBytesTemp.Length + 1
    $Arm64Offset1 = $X64Offset1 + $ElfX64.Length
    $ShText = New-DispatcherScript $X64Offset1 $Arm64Offset1
}
[byte[]]$ShBytes = [System.Text.Encoding]::ASCII.GetBytes($ShText)
$ShLen = $ShBytes.Length
$X64Offset1   = $PeBytes.Length + $ShLen + 1
$Arm64Offset1 = $X64Offset1 + $ElfX64.Length

# DOS-Header-Trampolin in Offset 0x00..0x3B (60 Bytes vor e_lfanew an 0x3C) einbetten:
# Zeile 1: "MZ=0\n" (5 Bytes: 0x00..0x04) -> erhält 'MZ'-Signatur für Windows und ist in /bin/sh eine gültige Zuweisung!
# Zeile 2: eval "$(tail -c +<SH_OFFSET> "$0"|head -c <SH_LEN>)"\n# (gepaddet mit Leerzeichen bis 0x3B)
$Trampoline = "MZ=0`neval `"`$(tail -c +$ShOffset1 `"`$0`"|head -c $ShLen)`"`n#"
[byte[]]$TrampBytes = [System.Text.Encoding]::ASCII.GetBytes($Trampoline)
if ($TrampBytes.Length -gt 60) {
    throw "Trampolin zu lang ($($TrampBytes.Length) > 60 Bytes): $Trampoline"
}

$e_lfanew_orig = [BitConverter]::ToUInt32($PeBytes, 0x3C)
for ($i = 0; $i -lt 60; $i++) {
    if ($i -lt $TrampBytes.Length) {
        $PeBytes[$i] = $TrampBytes[$i]
    } else {
        $PeBytes[$i] = 0x20 # Space-Padding bis 0x3B
    }
}
$e_lfanew_after = [BitConverter]::ToUInt32($PeBytes, 0x3C)
if ($e_lfanew_orig -ne $e_lfanew_after) {
    throw "FEHLER: e_lfanew an Offset 0x3C wurde verändert!"
}

# Polyglot-Datei zusammenfügen: [PE (mit DOS-Stub-Trampolin)] + [SH-Dispatcher] + [ELF x86_64] + [ELF aarch64]
$TotalLen = $PeBytes.Length + $ShBytes.Length + $ElfX64.Length + $ElfArm64.Length
$UnivBytes = New-Object byte[] $TotalLen
$pos = 0
[Array]::Copy($PeBytes,  0, $UnivBytes, $pos, $PeBytes.Length);  $pos += $PeBytes.Length
[Array]::Copy($ShBytes,  0, $UnivBytes, $pos, $ShBytes.Length);  $pos += $ShBytes.Length
[Array]::Copy($ElfX64,   0, $UnivBytes, $pos, $ElfX64.Length);   $pos += $ElfX64.Length
[Array]::Copy($ElfArm64, 0, $UnivBytes, $pos, $ElfArm64.Length); $pos += $ElfArm64.Length

[System.IO.File]::WriteAllBytes($DistUniv, $UnivBytes)

Write-Host "`n============================================================" -ForegroundColor Green
Write-Host " Polyglot-Binary erstellt: $DistUniv" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Green
Write-Host "  Gesamtgröße      : $([math]::Round($TotalLen / 1MB, 2)) MB ($TotalLen Bytes)"
Write-Host "  BUILD_ID         : $BuildId"
Write-Host "  e_lfanew (0x3C)  : 0x$($e_lfanew_after.ToString('X4')) (unverändert)"
Write-Host "  SH_OFFSET (1-b)  : $ShOffset1 (SH_LEN = $ShLen Bytes)"
Write-Host "  X64_OFFSET (1-b) : $X64Offset1 (X64_LEN = $X64Len Bytes)"
Write-Host "  ARM_OFFSET (1-b) : $Arm64Offset1 (ARM_LEN = $Arm64Len Bytes)"
