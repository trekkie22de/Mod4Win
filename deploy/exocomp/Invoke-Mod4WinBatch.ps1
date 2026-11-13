#!/usr/bin/env pwsh
#Requires -Version 7.0
<#
.SYNOPSIS
    Startet, überwacht oder stoppt einen 16-Bit MOD4WIN Regression-Test-Batch auf Exocomp.
.DESCRIPTION
    Führt das originale 16-Bit MOD4WIN 2.40 Beta (MOD4WIN_IDO0..3.EXE + DPMI PLAYER32.DLL)
    parallelisiert auf allen Kernen in einem isolierten Podman-Container (Wine32 + Xvfb + /dev/shm)
    gegen den 64-Bit-Rust-Port (mod4win-diff) aus.

    Koexistenz mit der Video-Transcoding-Pipeline (vt-worker-daemon.service):
    Da vt-worker-daemon.service mit Nice=10 / CPUWeight=25 läuft, erhält dieser Container
    mit Standard-Gewichtung (--cpu-shares 1024 = CPUWeight=100) automatisch ~80% der 16 Kerne
    über den Linux cgroup-v2 EEVDF-Scheduler und gibt unmittelbar nach Batch-Ende 100% der
    Ressourcen wieder an HandBrakeCLI frei – ohne Service-Neustart oder Unterbrechung.
#>
[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
    [Parameter(ParameterSetName = 'Run', Mandatory = $true, Position = 0)]
    [string]$SourcePath,

    [Parameter(ParameterSetName = 'Run', Position = 1)]
    [string]$ResultPath = '',

    [Parameter(ParameterSetName = 'Run')]
    [int]$Workers = 48,

    [Parameter(ParameterSetName = 'Run')]
    [string]$Idos = '0,1,2,3',

    [Parameter(ParameterSetName = 'Run')]
    [int]$MaxIdleSec = 8,

    [Parameter(ParameterSetName = 'Run')]
    [int]$MaxPeakSec = 600,

    [Parameter(ParameterSetName = 'Run')]
    [int]$MaxStallSec = 25,

    [Parameter(ParameterSetName = 'Run')]
    [int]$MaxWavMb = 350,

    [Parameter(ParameterSetName = 'Run')]
    [int]$CpuShares = 1024,

    [Parameter(ParameterSetName = 'Run')]
    [string]$ShmSize = '4g',

    [Parameter(ParameterSetName = 'Run')]
    [switch]$RetryFails,

    [Parameter(ParameterSetName = 'Run')]
    [switch]$RetryTimeouts,

    [Parameter(ParameterSetName = 'Run')]
    [switch]$Allow1Lsb,

    [Parameter(ParameterSetName = 'Run')]
    [switch]$Detach,

    [Parameter(ParameterSetName = 'Status', Mandatory = $true)]
    [switch]$Status,

    [Parameter(ParameterSetName = 'Status')]
    [string]$CheckResultPath = '',

    [Parameter(ParameterSetName = 'Stop', Mandatory = $true)]
    [switch]$Stop
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ContainerName = 'm4w-reg-batch'
$ImageName     = 'localhost/mod4win-reg-worker:latest'
$WorkerRoot    = '/opt/Mod4Win-Worker'
$DiffBin       = "$WorkerRoot/bin/mod4win-diff"
$OrigBinDir    = "$WorkerRoot/orig_bin"
$NasRegRoot    = '/mnt/nas/mod4win-regression'

if ($PSCmdlet.ParameterSetName -eq 'Stop') {
    Write-Host "[m4w-batch] Stoppe Container '$ContainerName' (falls aktiv)..." -ForegroundColor Yellow
    & podman stop -t 5 $ContainerName 2>$null | Out-Null
    & podman rm -f $ContainerName 2>$null | Out-Null
    Write-Host "[m4w-batch] Gestoppt." -ForegroundColor Green
    exit 0
}

if ($PSCmdlet.ParameterSetName -eq 'Status') {
    Write-Host "=== Container-Status ($ContainerName) ===" -ForegroundColor Cyan
    & podman ps -a --filter "name=^$ContainerName$" --format "table {{.Names}}\t{{.Status}}\t{{.RunningFor}}"

    $TargetRes = $CheckResultPath
    if ([string]::IsNullOrWhiteSpace($TargetRes) -and (Test-Path "$NasRegRoot/results")) {
        $Latest = Get-ChildItem -Path "$NasRegRoot/results" -Directory -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if ($Latest) {
            $TargetRes = $Latest.FullName
        }
    }

    if (-not [string]::IsNullOrWhiteSpace($TargetRes) -and (Test-Path $TargetRes)) {
        $PassCnt = @(Get-ChildItem -Path (Join-Path $TargetRes 'pass') -File -ErrorAction SilentlyContinue).Count
        $FailCnt = @(Get-ChildItem -Path (Join-Path $TargetRes 'fail') -File -ErrorAction SilentlyContinue).Count
        $TimeCnt = @(Get-ChildItem -Path (Join-Path $TargetRes 'timeout') -File -ErrorAction SilentlyContinue).Count
        Write-Host "`n=== Ergebnis-Zähler ($TargetRes) ===" -ForegroundColor Cyan
        Write-Host ("PASS: {0} | FAIL: {1} | TIMEOUT: {2}" -f $PassCnt, $FailCnt, $TimeCnt) -ForegroundColor Green
    }

    $Running = (& podman ps -q --filter "name=^$ContainerName$" 2>$null)
    if ($Running) {
        Write-Host "`n=== Letzte 15 Log-Zeilen ===" -ForegroundColor Cyan
        & podman logs --tail 15 $ContainerName
    }
    exit 0
}

if (-not (Test-Path $SourcePath)) {
    throw "Quellpfad '$SourcePath' existiert nicht."
}
$ResolvedSource = (Resolve-Path -LiteralPath $SourcePath).Path

if ([string]::IsNullOrWhiteSpace($ResultPath)) {
    $SafeLeaf = Split-Path -Leaf $ResolvedSource
    if ([string]::IsNullOrWhiteSpace($SafeLeaf)) {
        $SafeLeaf = 'batch'
    }
    $SafeLeaf = ($SafeLeaf -replace '[^A-Za-z0-9._-]', '_')
    $BaseResults = if (Test-Path '/mnt/nas') { "$NasRegRoot/results" } else { "$WorkerRoot/results" }
    $ResultPath = Join-Path $BaseResults $SafeLeaf
}

New-Item -ItemType Directory -Force -Path $ResultPath | Out-Null
$ResolvedResult = (Resolve-Path -LiteralPath $ResultPath).Path

if (-not (Test-Path $DiffBin)) {
    throw "mod4win-diff Binary fehlt unter '$DiffBin'. Bitte zuerst Install-ExocompWorker.ps1 ausführen."
}
if (-not (Test-Path (Join-Path $OrigBinDir 'MOD4WIN.EXE')) -and -not (Test-Path (Join-Path $OrigBinDir 'MOD4WIN_IDO0.EXE'))) {
    throw "Originale 16-Bit MOD4WIN-Binaries fehlen unter '$OrigBinDir'."
}

$ExistingRunning = (& podman ps -q --filter "name=^$ContainerName$" 2>$null)
if ($ExistingRunning) {
    throw "Es läuft bereits ein Batch-Container '$ContainerName'. Prüfe mit 'm4w-batch -Status' oder stoppe mit 'm4w-batch -Stop'."
}
& podman rm -f $ContainerName 2>$null | Out-Null

# Volume-Mounts zusammenstellen (identische Host-/Container-Pfade; Quellarchive Read-Only!)
$RwMountDirs = [System.Collections.Generic.HashSet[string]]::new()
$RoMountDirs = [System.Collections.Generic.HashSet[string]]::new()
[void]$RwMountDirs.Add($WorkerRoot)
if (Test-Path '/mnt/nas') {
    [void]$RwMountDirs.Add('/mnt/nas')
}
if (Test-Path '/mnt/mods') {
    [void]$RoMountDirs.Add('/mnt/mods')
}

$ResultMountDir = if (Test-Path -LiteralPath $ResolvedResult -PathType Container) {
    $ResolvedResult
} else {
    Split-Path -Parent $ResolvedResult
}
$ResCovered = $false
foreach ($Root in $RwMountDirs) {
    if ($ResultMountDir -eq $Root -or $ResultMountDir.StartsWith("$Root/")) {
        $ResCovered = $true
        break
    }
}
if (-not $ResCovered) {
    [void]$RwMountDirs.Add($ResultMountDir)
}

$SourceMountDir = if (Test-Path -LiteralPath $ResolvedSource -PathType Container) {
    $ResolvedSource
} else {
    Split-Path -Parent $ResolvedSource
}
$SrcCovered = $false
foreach ($Root in (@($RwMountDirs) + @($RoMountDirs))) {
    if ($SourceMountDir -eq $Root -or $SourceMountDir.StartsWith("$Root/")) {
        $SrcCovered = $true
        break
    }
}
if (-not $SrcCovered) {
    [void]$RoMountDirs.Add($SourceMountDir)
}

$PodmanArgs = [System.Collections.Generic.List[string]]::new()
$PodmanArgs.Add('run')
if ($Detach) {
    $PodmanArgs.Add('-d')
} else {
    $PodmanArgs.Add('--rm')
}
$PodmanArgs.AddRange([string[]]@(
    '--init',
    '--name', $ContainerName,
    '--security-opt', 'seccomp=unconfined',
    '--security-opt', 'label=disable',
    "--shm-size=$ShmSize",
    '--pids-limit=16384',
    "--cpu-shares=$CpuShares"
))

foreach ($Mnt in $RwMountDirs) {
    $PodmanArgs.Add('-v')
    $PodmanArgs.Add("${Mnt}:${Mnt}:rw")
}
foreach ($Mnt in $RoMountDirs) {
    $PodmanArgs.Add('-v')
    $PodmanArgs.Add("${Mnt}:${Mnt}:ro")
}

$PodmanArgs.Add($ImageName)
$PodmanArgs.AddRange([string[]]@(
    $DiffBin,
    '--batch-diff', $ResolvedSource, $ResolvedResult,
    '--m4w-dir', $OrigBinDir,
    '--wine', 'wine',
    '--ffmpeg', 'ffmpeg',
    '--work-dir', '/dev/shm/m4w_work',
    '--ido', $Idos,
    '--max-idle-sec', "$MaxIdleSec",
    '--max-peak-sec', "$MaxPeakSec",
    '--max-stall-sec', "$MaxStallSec",
    '--max-wav-mb', "$MaxWavMb"
))

if ($Workers -gt 0) {
    $PodmanArgs.Add('--workers')
    $PodmanArgs.Add("$Workers")
}
if ($RetryFails) {
    $PodmanArgs.Add('--retry-fails')
}
if ($RetryTimeouts) {
    $PodmanArgs.Add('--retry-timeouts')
}
if ($Allow1Lsb) {
    $PodmanArgs.Add('--allow-1lsb')
}

Write-Host "======================================================================" -ForegroundColor Cyan
Write-Host " MOD4WIN 16-Bit Regression-Worker auf $(hostname)" -ForegroundColor Cyan
Write-Host "======================================================================" -ForegroundColor Cyan
Write-Host "Quelle:      $ResolvedSource"
Write-Host "Ergebnisse:  $ResolvedResult"
Write-Host "Container:   $ImageName (--shm-size=$ShmSize, --cpu-shares=$CpuShares)"
Write-Host "Modus:       $(if ($Detach) { 'Detached (Hintergrund)' } else { 'Synchron (Live-Output)' })"
Write-Host "----------------------------------------------------------------------" -ForegroundColor Cyan

& podman @PodmanArgs
$ExitCode = $LASTEXITCODE

if ($Detach -and $ExitCode -eq 0) {
    Write-Host "`n[OK] Hintergrund-Batch gestartet! Live-Fortschritt verfolgen mit:" -ForegroundColor Green
    Write-Host "  podman logs -f $ContainerName" -ForegroundColor Yellow
    Write-Host "  m4w-batch -Status -CheckResultPath '$ResolvedResult'" -ForegroundColor Yellow
}

exit $ExitCode
