$ErrorActionPreference = "Stop"

# Arbeitsverzeichnis auf mod4win-rs setzen (funktioniert sowohl aus Repo-Root als auch aus mod4win-rs/)
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (Test-Path (Join-Path $ScriptDir "mod4win-rs/Cargo.toml")) {
    $CrateDir = Join-Path $ScriptDir "mod4win-rs"
} elseif (Test-Path (Join-Path $ScriptDir "Cargo.toml")) {
    $CrateDir = $ScriptDir
} else {
    throw "Cargo.toml konnte nicht gefunden werden."
}

$TargetDir = if ($env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR
} elseif ($IsLinux -and (Test-Path "/var/tmp/mod4win-target")) {
    "/var/tmp/mod4win-target"
} else {
    Join-Path $CrateDir "target"
}

$OrigPath = $env:PATH
Push-Location $CrateDir
try {
    $PathSep = [System.IO.Path]::PathSeparator
    if ($IsLinux) {
        $env:PATH = "/usr/local/bin$PathSep$HOME/.cargo/bin$PathSep$env:PATH"
    }

    # 1. Zig im PATH sicherstellen (inkl. automatischer Erkennung einer WinGet-Zig-Installation unter Windows)
    if (-not (Get-Command "zig" -ErrorAction SilentlyContinue)) {
        if ($IsWindows) {
            $wingetZig = Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Filter "zig.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($wingetZig) {
                $zigDir = $wingetZig.DirectoryName
                Write-Host "[Zig] Gefunden unter: $zigDir" -ForegroundColor Cyan
                $env:PATH = "$zigDir$PathSep$env:PATH"
            } else {
                throw "zig.exe wurde weder im PATH noch unter WinGet gefunden!"
            }
        } else {
            throw "zig wurde nicht im PATH gefunden!"
        }
    }

    # 2. Rust-Linux-Targets sicherstellen
    Write-Host "[Rustup] Prüfe Linux-Targets (x86_64 & aarch64)..." -ForegroundColor Cyan
    rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu

    $StubSrc = Join-Path $CrateDir "scripts/alsa_stub.c"
    $StubVer = Join-Path $CrateDir "scripts/alsa_stub.ver"

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
        Write-Host " Baue MOD4WIN für $ArchLabel ($RustTarget)" -ForegroundColor Green
        Write-Host "============================================================" -ForegroundColor Green

        $StubDir = Join-Path $TargetDir $StubSubdir
        New-Item -ItemType Directory -Force -Path $StubDir | Out-Null
        Remove-Item (Join-Path $StubDir "libasound*") -Force -ErrorAction SilentlyContinue

        $SoPath = Join-Path $StubDir "libasound.so"
        $So2Path = Join-Path $StubDir "libasound.so.2"

        Write-Host "[1/3] Kompiliere dynamischen ALSA-Shared-Library-Stub ($SoPath)..." -ForegroundColor Yellow
        zig cc -target $ZigTarget -fPIC -shared "-Wl,-soname,libasound.so.2" "-Wl,--version-script=$StubVer" $StubSrc -o $SoPath
        if ($LASTEXITCODE -ne 0) {
            throw "zig cc (libasound.so) für $ArchLabel fehlgeschlagen!"
        }
        Copy-Item -Path $SoPath -Destination $So2Path -Force

        $BinBase = $RustTarget.Split('.')[0]
        if ($IsWindows) {
            $PkgConfigCmd = Join-Path $StubDir "pkg-config.bat"
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
            Set-Content -Path $PkgConfigCmd -Value $BatContent -Encoding ASCII
            $SoLinkArg = $SoPath.Replace('\', '/')
        } else {
            $PkgConfigCmd = Join-Path $StubDir "pkg-config"
            $ShContent = @"
#!/bin/sh
for arg in "`$@"; do
    case "`$arg" in
        --version) echo "0.29.2"; exit 0 ;;
        --modversion) echo "1.2.8"; exit 0 ;;
    esac
done
exit 0
"@
            [System.IO.File]::WriteAllText($PkgConfigCmd, $ShContent.Replace("`r`n", "`n"))
            chmod +x $PkgConfigCmd
            $SoLinkArg = $SoPath
        }

        Write-Host "[2/3] Konfiguriere pkg-config Stub ($PkgConfigCmd)..." -ForegroundColor Yellow
        $env:PKG_CONFIG = $PkgConfigCmd
        $env:PATH = "$StubDir$PathSep$OrigPath"
        $env:RUSTFLAGS = "-C link-arg=-Wl,--no-as-needed -C link-arg=$SoLinkArg -C link-arg=-Wl,--allow-shlib-undefined"

        $BinPath = Join-Path $TargetDir "$BinBase/release/mod4win-rs"
        Remove-Item -Path $BinPath -Force -ErrorAction SilentlyContinue

        Write-Host "[3/3] Starte cargo zigbuild --release --target $RustTarget..." -ForegroundColor Yellow
        cargo zigbuild --release --target $RustTarget
        if ($LASTEXITCODE -ne 0) {
            throw "cargo zigbuild für $RustTarget fehlgeschlagen!"
        }

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
    $env:PATH = $OrigPath
    Remove-Item Env:\RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:\PKG_CONFIG -ErrorAction SilentlyContinue
    Remove-Item Env:\PKG_CONFIG_ALLOW_CROSS -ErrorAction SilentlyContinue
    Pop-Location
}
