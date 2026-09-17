# Script de desinstalación para Windows 11 — CodePeek
$ErrorActionPreference = "Stop"

$InstalledExe = "$env:LOCALAPPDATA\Programs\CodePeek\peek.exe"
if (Test-Path $InstalledExe) {
    & $InstalledExe --uninstall
} else {
    $ReleaseExe = Join-Path $PSScriptRoot "..\target\release\peek.exe"
    if (Test-Path $ReleaseExe) {
        & $ReleaseExe --uninstall
    } else {
        Write-Host "CodePeek no parece estar instalado en este sistema." -ForegroundColor Yellow
    }
}
