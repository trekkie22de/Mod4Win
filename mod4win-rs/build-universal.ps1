$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootScript = Join-Path (Split-Path -Parent $ScriptDir) "build-universal.ps1"
if (Test-Path $RootScript) {
    & $RootScript
} else {
    throw "build-universal.ps1 im Repo-Root nicht gefunden."
}

