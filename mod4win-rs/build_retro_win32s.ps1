<#
.SYNOPSIS
    Baut das echte 32-Bit 80486 / Windows 3.11 (Win32s 1.30c) / Windows 95 Retro-Binary `MOD4WIN32.EXE`.
.DESCRIPTION
    Kompiliert `mod4win-rs` inklusive `core`, `alloc`, `panic_abort` und `std` für das
    Custom-Target `i486-win32s-windows-msvc` (reiner 80486 + x87 Befehlssatz, kein CMOV,
    kein CMPXCHG8B, kein MMX, kein SSE/SSE2, kein MSVCRT.DLL/NTDLL.DLL) und patcht
    anschließend die PE-Optional-Header-Felder `OperatingSystemVersion` und `SubsystemVersion`
    auf `3.10` (Windows NT 3.1 / Windows 3.11 Win32s 1.30c / Windows 95).
#>
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location $ScriptDir
try {
    $env:RUSTC_BOOTSTRAP = "1"
    $env:RUST_TARGET_PATH = $ScriptDir
    $env:RUSTFLAGS = "-Z unstable-options -C opt-level=2"

    Write-Host "[1/3] Kompiliere mod4win-rs (Target: i486-win32s-windows-msvc, Feature: retro-win32)..." -ForegroundColor Cyan
    cargo build --release --target i486-win32s-windows-msvc --no-default-features --features retro-win32 -Z json-target-spec -Z build-std=std,panic_abort
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build fehlgeschlagen mit Exit-Code $LASTEXITCODE"
    }

    $SrcExe = Join-Path $ScriptDir "target\i486-win32s-windows-msvc\release\mod4win-rs.exe"
    $DstExe = Join-Path $ScriptDir "MOD4WIN32.EXE"
    Copy-Item -Path $SrcExe -Destination $DstExe -Force

    Write-Host "[2/3] Patche PE32-Optional-Header auf Subsystem 3.10 (Windows 3.11 Win32s / Windows 95)..." -ForegroundColor Cyan
    $bytes = [System.IO.File]::ReadAllBytes($DstExe)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ($bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45) {
        throw "Ungültige PE-Signatur in $DstExe"
    }
    $optOffset = $peOffset + 24
    # MajorOperatingSystemVersion (Offset +40 = 0x28), MinorOperatingSystemVersion (+42 = 0x2A) -> 3.10
    [BitConverter]::GetBytes([uint16]3).CopyTo($bytes, $optOffset + 40)
    [BitConverter]::GetBytes([uint16]10).CopyTo($bytes, $optOffset + 42)
    # MajorSubsystemVersion (Offset +48 = 0x30), MinorSubsystemVersion (+50 = 0x32) -> 3.10
    [BitConverter]::GetBytes([uint16]3).CopyTo($bytes, $optOffset + 48)
    [BitConverter]::GetBytes([uint16]10).CopyTo($bytes, $optOffset + 50)
    # DllCharacteristics (Offset +70 = 0x46) -> 0x0000 (klassischer Win32s/Win95 Header)
    [BitConverter]::GetBytes([uint16]0).CopyTo($bytes, $optOffset + 70)

    [System.IO.File]::WriteAllBytes($DstExe, $bytes)
    $DstExe83 = Join-Path $ScriptDir "MOD4WI32.EXE"
    Copy-Item -Path $DstExe -Destination $DstExe83 -Force
    $sizeKb = [Math]::Round((Get-Item $DstExe).Length / 1024.0, 1)
    Write-Host "[3/3] Fertig: $DstExe & $DstExe83 ($sizeKb KB)" -ForegroundColor Green
} finally {
    Pop-Location
}

