$ErrorActionPreference = "Stop"

# Arbeitsverzeichnis auf mod4win-rs setzen (funktioniert sowohl aus Repo-Root als auch aus mod4win-rs/)
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (Test-Path (Join-Path $ScriptDir "mod4win-rs\Cargo.toml")) {
    $CrateDir = Join-Path $ScriptDir "mod4win-rs"
} elseif (Test-Path (Join-Path $ScriptDir "Cargo.toml")) {
    $CrateDir = $ScriptDir
} else {
    throw "Cargo.toml konnte nicht gefunden werden."
}

Push-Location $CrateDir
try {
    # 1. Zig im PATH sicherstellen (inkl. automatischer Erkennung einer WinGet-Zig-Installation)
    if (-not (Get-Command "zig" -ErrorAction SilentlyContinue)) {
        $wingetZig = Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Filter "zig.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($wingetZig) {
            $zigDir = $wingetZig.DirectoryName
            Write-Host "[Zig] Gefunden unter: $zigDir" -ForegroundColor Cyan
            $env:PATH = "$zigDir;$env:PATH"
        } else {
            throw "zig.exe wurde weder im PATH noch unter WinGet gefunden!"
        }
    }

    # 2. Rust-Linux-Targets sicherstellen
    Write-Host "[Rustup] Prüfe Linux-Targets (x86_64 & aarch64)..." -ForegroundColor Cyan
    rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu

    $StubSrc = Join-Path $CrateDir "scripts\alsa_stub.c"
    if (-not (Test-Path $StubSrc)) {
        $StubSrc = Join-Path $ScriptDir "scripts\alsa_stub.c"
    }

    $OrigPath = $env:PATH
    $env:PKG_CONFIG_ALLOW_CROSS = "1"

    function Build-LinuxTarget {
        param(
            [string]$ArchLabel,
            [string]$ZigTarget,
            [string]$RustTarget,
            [string]$StubSubdir
        )

        Write-Host "`n============================================================" -ForegroundColor Green
        Write-Host " Baue MOD4WIN 3.0 Beta für $ArchLabel ($RustTarget)" -ForegroundColor Green
        Write-Host "============================================================" -ForegroundColor Green

        $StubDir = Join-Path $CrateDir "target\$StubSubdir"
        New-Item -ItemType Directory -Force -Path $StubDir | Out-Null
        Remove-Item (Join-Path $StubDir "libasound*") -Force -ErrorAction SilentlyContinue

        $SoPath = Join-Path $StubDir "libasound.so"
        $So2Path = Join-Path $StubDir "libasound.so.2"
        $SoRelPath = "target/$StubSubdir/libasound.so"

        Write-Host "[1/3] Kompiliere dynamischen ALSA-Shared-Library-Stub ($SoPath)..." -ForegroundColor Yellow
        zig cc -target $ZigTarget -fPIC -shared "-Wl,-soname,libasound.so.2" $StubSrc -o $SoPath
        if ($LASTEXITCODE -ne 0) {
            throw "zig cc (libasound.so) für $ArchLabel fehlgeschlagen!"
        }
        Copy-Item -Path $SoPath -Destination $So2Path -Force

        # Temporären pkg-config.bat Wrapper erzeugen:
        # Hinweis: Unter Zig 0.17 auf Windows wandelt 'zig cc' ein '-lasound' intern in '-l:F:\...\libasound.so' um,
        # was ld.lld wegen des Windows-Laufwerks-Doppelpunkts ablehnt. Darum gibt pkg-config.bat bei --libs keinen
        # '-lasound'-Schalter aus, und wir übergeben die Shared Library 'target/.../libasound.so' direkt per link-arg!
        $PkgConfigBat = Join-Path $StubDir "pkg-config.bat"
        $BatContent = @"
@echo off
setlocal
for %%A in (%*) do (
    if "%%~A"=="--version" (
        echo 0.29.2
        exit /b 0
    )
    if "%%~A"=="--modversion" (
        echo 1.2.8
        exit /b 0
    )
)
exit /b 0
"@
        $BinBase = $RustTarget.Split('.')[0]
        if (-not (Test-Path $PkgConfigBat)) {
            $AlsaBuildDirs = Join-Path $CrateDir "target\$BinBase\release\build\alsa-sys-*"
            Remove-Item -Path $AlsaBuildDirs -Recurse -Force -ErrorAction SilentlyContinue
        }
        Set-Content -Path $PkgConfigBat -Value $BatContent -Encoding ASCII

        Write-Host "[2/3] Konfiguriere pkg-config Stub ($PkgConfigBat)..." -ForegroundColor Yellow
        $env:PKG_CONFIG = $PkgConfigBat
        $env:PATH = "$StubDir;$OrigPath"
        $env:RUSTFLAGS = "-C link-arg=-Wl,--no-as-needed -C link-arg=$SoRelPath -C link-arg=-Wl,--allow-shlib-undefined"

        Write-Host "[3/3] Starte cargo zigbuild --release --target $RustTarget..." -ForegroundColor Yellow
        cargo zigbuild --release --target $RustTarget
        if ($LASTEXITCODE -ne 0) {
            throw "cargo zigbuild für $RustTarget fehlgeschlagen!"
        }

        $BinPath = Join-Path $CrateDir "target\$BinBase\release\mod4win-rs"
        if (Test-Path $BinPath) {
            $SizeMB = [math]::Round((Get-Item $BinPath).Length / 1MB, 2)
            Write-Host "[OK] Linux-Binary erstellt: $BinPath ($SizeMB MB)" -ForegroundColor Green
        }
    }

    # x86_64 Linux (glibc 2.28)
    Build-LinuxTarget `
        -ArchLabel "Linux x86_64" `
        -ZigTarget "x86_64-linux-gnu.2.28" `
        -RustTarget "x86_64-unknown-linux-gnu.2.28" `
        -StubSubdir "alsa-stub-x64"

    # aarch64 Raspberry Pi / ARM64 Linux (glibc 2.28)
    Build-LinuxTarget `
        -ArchLabel "Linux ARM64 / Raspberry Pi (aarch64)" `
        -ZigTarget "aarch64-linux-gnu.2.28" `
        -RustTarget "aarch64-unknown-linux-gnu.2.28" `
        -StubSubdir "alsa-stub-arm64"

    Write-Host "`nAlle Linux-Cross-Builds (x86_64 & aarch64) erfolgreich abgeschlossen!" -ForegroundColor Green
} finally {
    Remove-Item Env:\RUSTFLAGS -ErrorAction SilentlyContinue
    Pop-Location
}
