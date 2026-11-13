#!/usr/bin/env pwsh
#Requires -Version 7.0
<#
.SYNOPSIS
    Provisioniert oder aktualisiert den 16-Bit MOD4WIN Regression-Test-Worker auf Exocomp.
.DESCRIPTION
    Kann sowohl remote von Daystrom aus (via SSH/SCP gegen root@192.168.10.64) als auch direkt
    lokal auf Exocomp ausgeführt werden. KEIN Reboot und KEINE Unterbrechung laufender Dienste
    (insbesondere vt-worker-daemon.service / HandBrakeCLI bleibt 100% unangetastet).
#>
[CmdletBinding()]
param(
    [string]$RemoteHost = 'root@192.168.10.64',
    [switch]$Local,
    [switch]$BuildBinary,
    [switch]$RebuildContainer,
    [switch]$SkipSmokeTest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Split-Path -Parent (Split-Path -Parent $ScriptDir)

if (-not $Local) {
    $DiffBinSrc = Join-Path $RepoRoot 'dist/mod4win-diff-linux-x64'
    $OrigZipSrc = Join-Path $RepoRoot 'orig_bin.zip'
    $SmokeXmSrc = Join-Path $RepoRoot 'legacy_src/IFACE32/NO-SN.XM'

    if ($BuildBinary -or -not (Test-Path $DiffBinSrc)) {
        Write-Host "[Build] Baue mod4win-diff-linux-x64 (--features diff-suite, glibc 2.28)..." -ForegroundColor Cyan
        $CrateDir  = Join-Path $RepoRoot 'mod4win-rs'
        $TargetDir = if (Test-Path '/var/tmp/mod4win-target') { '/var/tmp/mod4win-target' } else { Join-Path $CrateDir 'target' }
        $StubDir   = Join-Path $TargetDir 'alsa-stub-x64'
        New-Item -ItemType Directory -Force -Path $StubDir | Out-Null
        $SoPath    = Join-Path $StubDir 'libasound.so'
        $So2Path   = Join-Path $StubDir 'libasound.so.2'
        if (-not (Test-Path $SoPath)) {
            & zig cc -target x86_64-linux-gnu.2.28 -fPIC -shared '-Wl,-soname,libasound.so.2' "-Wl,--version-script=$CrateDir/scripts/alsa_stub.ver" "$CrateDir/scripts/alsa_stub.c" -o $SoPath
            if ($LASTEXITCODE -ne 0) { throw "zig cc ALSA-Stub fehlgeschlagen." }
            Copy-Item -Path $SoPath -Destination $So2Path -Force
        }
        $PkgConfigCmd = Join-Path $StubDir 'pkg-config'
        [System.IO.File]::WriteAllText($PkgConfigCmd, "#!/bin/sh`nfor arg in `"`$@`"; do`n  case `"`$arg`" in`n    --version) echo `"0.29.2`"; exit 0 ;;`n    --modversion) echo `"1.2.8`"; exit 0 ;;`n  esac`ndone`nexit 0`n")
        & chmod +x $PkgConfigCmd

        $OrigPath = $env:PATH
        Push-Location $CrateDir
        try {
            $env:CARGO_TARGET_DIR = $TargetDir
            $env:PKG_CONFIG = $PkgConfigCmd
            $env:PKG_CONFIG_ALLOW_CROSS = '1'
            $env:PATH = "${StubDir}:/usr/local/bin:${HOME}/.cargo/bin:$OrigPath"
            $env:RUSTFLAGS = "-C link-arg=-Wl,--no-as-needed -C link-arg=$SoPath -C link-arg=-Wl,--allow-shlib-undefined"
            & cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.28 --features diff-suite
            if ($LASTEXITCODE -ne 0) { throw "cargo zigbuild --features diff-suite fehlgeschlagen." }
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $DiffBinSrc) | Out-Null
            Copy-Item -Path "$TargetDir/x86_64-unknown-linux-gnu/release/mod4win-rs" -Destination $DiffBinSrc -Force
            & chmod +x $DiffBinSrc
        } finally {
            $env:PATH = $OrigPath
            Remove-Item Env:\CARGO_TARGET_DIR, Env:\PKG_CONFIG, Env:\PKG_CONFIG_ALLOW_CROSS, Env:\RUSTFLAGS -ErrorAction SilentlyContinue
            Pop-Location
        }
    }
    if (-not (Test-Path $OrigZipSrc)) {
        throw "Referenz-Archiv '$OrigZipSrc' fehlt."
    }

    Write-Host "[Deploy] Lege Zielstruktur auf $RemoteHost unter /opt/Mod4Win-Worker an..." -ForegroundColor Cyan
    & ssh $RemoteHost "mkdir -p /opt/Mod4Win-Worker/bin /opt/Mod4Win-Worker/deploy /opt/Mod4Win-Worker/orig_bin /opt/Mod4Win-Worker/smoke_test/mods"
    if ($LASTEXITCODE -ne 0) { throw "SSH mkdir auf $RemoteHost fehlgeschlagen." }

    Write-Host "[Deploy] Übertrage Binaries, Container-Definition und orig_bin.zip nach $RemoteHost..." -ForegroundColor Cyan
    & scp -q `
        $DiffBinSrc `
        "$RemoteHost`:/opt/Mod4Win-Worker/bin/mod4win-diff"
    if ($LASTEXITCODE -ne 0) { throw "SCP mod4win-diff fehlgeschlagen." }

    & scp -q `
        (Join-Path $ScriptDir 'Containerfile') `
        (Join-Path $ScriptDir 'm4w-container-entry.sh') `
        (Join-Path $ScriptDir 'Invoke-Mod4WinBatch.ps1') `
        (Join-Path $ScriptDir 'Install-ExocompWorker.ps1') `
        "$RemoteHost`:/opt/Mod4Win-Worker/deploy/"
    if ($LASTEXITCODE -ne 0) { throw "SCP deploy-Skripte fehlgeschlagen." }

    & scp -q $OrigZipSrc "$RemoteHost`:/opt/Mod4Win-Worker/orig_bin.zip"
    if ($LASTEXITCODE -ne 0) { throw "SCP orig_bin.zip fehlgeschlagen." }

    if (Test-Path $SmokeXmSrc) {
        & scp -q $SmokeXmSrc "$RemoteHost`:/opt/Mod4Win-Worker/smoke_test/mods/NO-SN.XM"
    }

    $RemoteFlags = @('-Local')
    if ($RebuildContainer) { $RemoteFlags += '-RebuildContainer' }
    if ($SkipSmokeTest)    { $RemoteFlags += '-SkipSmokeTest' }
    $FlagStr = $RemoteFlags -join ' '

    Write-Host "[Deploy] Führe lokale Einrichtung auf $RemoteHost aus ($FlagStr)..." -ForegroundColor Cyan
    & ssh $RemoteHost "pwsh -NoProfile -File /opt/Mod4Win-Worker/deploy/Install-ExocompWorker.ps1 $FlagStr"
    exit $LASTEXITCODE
}

# === Lokale Ausführung direkt auf Exocomp ===
$WorkerRoot = '/opt/Mod4Win-Worker'
$BinDir     = "$WorkerRoot/bin"
$DeployDir  = "$WorkerRoot/deploy"
$OrigDir    = "$WorkerRoot/orig_bin"
$OrigZip    = "$WorkerRoot/orig_bin.zip"

New-Item -ItemType Directory -Force -Path $BinDir, $DeployDir, $OrigDir | Out-Null

if (Test-Path '/mnt/nas') {
    New-Item -ItemType Directory -Force -Path '/mnt/nas/mod4win-regression/corpus', '/mnt/nas/mod4win-regression/results' | Out-Null
    Write-Host "[Setup] NAS-Verzeichnisse unter /mnt/nas/mod4win-regression bereit." -ForegroundColor Green
}

if (-not (Get-Command 'podman' -ErrorAction SilentlyContinue)) {
    Write-Host "[Setup] Installiere podman aus Rocky Linux AppStream..." -ForegroundColor Yellow
    & dnf install -y podman
    if ($LASTEXITCODE -ne 0) { throw "dnf install podman fehlgeschlagen." }
}

# orig_bin.zip entpacken (flach nach /opt/Mod4Win-Worker/orig_bin)
if (Test-Path $OrigZip) {
    Write-Host "[Setup] Entpacke 16-Bit Referenz-Binaries nach $OrigDir ..." -ForegroundColor Cyan
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $Zip = [System.IO.Compression.ZipFile]::OpenRead($OrigZip)
    try {
        foreach ($Entry in $Zip.Entries) {
            if ([string]::IsNullOrEmpty($Entry.Name)) { continue }
            $DestFile = Join-Path $OrigDir $Entry.Name
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($Entry, $DestFile, $true)
        }
    } finally {
        $Zip.Dispose()
    }
}

# Skripte & Binaries ausführbar machen + /usr/local/bin/m4w-batch verlinken
Copy-Item -Path (Join-Path $DeployDir 'Invoke-Mod4WinBatch.ps1') -Destination (Join-Path $BinDir 'Invoke-Mod4WinBatch.ps1') -Force
& chmod 0755 (Join-Path $BinDir 'mod4win-diff') (Join-Path $BinDir 'Invoke-Mod4WinBatch.ps1') (Join-Path $DeployDir 'm4w-container-entry.sh')
& ln -sfn (Join-Path $BinDir 'Invoke-Mod4WinBatch.ps1') '/usr/local/bin/m4w-batch'

# Container-Image prüfen / bauen
$ImageExists = (& podman images -q 'localhost/mod4win-reg-worker:latest' 2>$null)
if ($RebuildContainer -or [string]::IsNullOrWhiteSpace($ImageExists)) {
    Write-Host "[Setup] Baue Container-Image localhost/mod4win-reg-worker:latest ..." -ForegroundColor Cyan
    & podman build -t 'localhost/mod4win-reg-worker:latest' $DeployDir
    if ($LASTEXITCODE -ne 0) { throw "podman build fehlgeschlagen." }
} else {
    Write-Host "[Setup] Container-Image localhost/mod4win-reg-worker:latest bereits vorhanden." -ForegroundColor Green
}

# Synthetische Test-Module (.MOD + Amiga-Präfix mod.*) für Multi-Worker-Smoke-Test erzeugen
$SmokeModsDir = "$WorkerRoot/smoke_test/mods"
$SmokeResDir  = "$WorkerRoot/smoke_test/results"
New-Item -ItemType Directory -Force -Path $SmokeModsDir | Out-Null

function New-SyntheticProTrackerMod {
    param(
        [string]$OutPath,
        [uint16]$PeriodCh1 = 428,
        [uint16]$PeriodCh2 = 339
    )
    # 1084-Byte Header + 1 Pattern (1024 Bytes) + 256 Bytes 8-Bit Sinus-Sample = 2364 Bytes
    $Buf = [byte[]]::new(2364)
    $TitleBytes = [System.Text.Encoding]::ASCII.GetBytes('EXOCOMP SMOKE TEST')
    [Array]::Copy($TitleBytes, 0, $Buf, 0, $TitleBytes.Length)

    # Sample 1 Header (Offset 20..50): Länge 128 Words (256 Bytes), Finetune 0, Volume 64, kein Loop (Replen=1 Word)
    $Buf[42] = 0x00; $Buf[43] = 0x80 # Length = 128 Words = 256 Bytes
    $Buf[44] = 0x00                  # Finetune = 0
    $Buf[45] = 64                    # Volume = 64
    $Buf[46] = 0x00; $Buf[47] = 0x00 # LoopStart = 0
    $Buf[48] = 0x00; $Buf[49] = 0x01 # LoopLen = 1 Word (One-Shot)

    # SongLength = 1 Order, Restart = 127, Order[0] = Pattern 0, Signatur "M.K." @ 1080
    $Buf[950] = 1
    $Buf[951] = 127
    $Buf[952] = 0
    [Array]::Copy([System.Text.Encoding]::ASCII.GetBytes('M.K.'), 0, $Buf, 1080, 4)

    # Pattern 0 (Offset 1084..2108): Row 0 triggert Sample 1 auf Ch1 & Ch2 mit Speed F01 (64 Ticks = 1.28s, sauberes Ende ohne Loop)
    $P0 = 1084
    # Ch0 @ Row 0: Sample 1, Period $PeriodCh1
    $Buf[$P0 + 0] = [byte](($PeriodCh1 -shr 8) -band 0x0F)
    $Buf[$P0 + 1] = [byte]($PeriodCh1 -band 0xFF)
    $Buf[$P0 + 2] = 0x10
    $Buf[$P0 + 3] = 0x00
    # Ch1 @ Row 0: Sample 1, Period $PeriodCh2
    $Buf[$P0 + 4] = [byte](($PeriodCh2 -shr 8) -band 0x0F)
    $Buf[$P0 + 5] = [byte]($PeriodCh2 -band 0xFF)
    $Buf[$P0 + 6] = 0x10
    $Buf[$P0 + 7] = 0x00
    # Row 0 Ch2: F01 (Speed 1 -> 1.28s Gesamtlänge)
    $Buf[$P0 + 10] = 0x0F
    $Buf[$P0 + 11] = 0x01

    # Sample-Daten @ Offset 2108..2364: 8-Bit Signed Dreieck/Sinus-Welle
    for ($i = 0; $i -lt 256; $i++) {
        $rad = (2.0 * [Math]::PI * ($i % 32)) / 32.0
        $val = [int][Math]::Round([Math]::Sin($rad) * 96.0)
        $Buf[2108 + $i] = [byte]($val -band 0xFF)
    }

    [System.IO.File]::WriteAllBytes($OutPath, $Buf)
}

New-SyntheticProTrackerMod -OutPath "$SmokeModsDir/SMOKE_C2.MOD" -PeriodCh1 428 -PeriodCh2 339
New-SyntheticProTrackerMod -OutPath "$SmokeModsDir/SMOKE_G2.MOD" -PeriodCh1 285 -PeriodCh2 214
New-SyntheticProTrackerMod -OutPath "$SmokeModsDir/mod.amiga_prefix_test" -PeriodCh1 360 -PeriodCh2 254

if (-not $SkipSmokeTest) {
    Write-Host "`n[Smoke-Test] Starte echten 16-Bit Multi-Worker Regression-Lauf (IDO 0,1,2,3)..." -ForegroundColor Cyan
    Remove-Item -Recurse -Force -Path $SmokeResDir -ErrorAction SilentlyContinue
    & /usr/local/bin/m4w-batch -SourcePath $SmokeModsDir -ResultPath $SmokeResDir -Workers 4 -Idos '0,1,2,3'
    if ($LASTEXITCODE -ne 0) {
        throw "Smoke-Test m4w-batch meldete Exit-Code $LASTEXITCODE"
    }
}

Write-Host "`n[OK] Exocomp 16-Bit MOD4WIN Regression-Worker ist einsatzbereit (`m4w-batch`)." -ForegroundColor Green
