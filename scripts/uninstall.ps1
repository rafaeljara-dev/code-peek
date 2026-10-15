# Script de desinstalación para Windows 11 — CodePeek
$ErrorActionPreference = "Stop"

$InstalledExe = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\CodePeek\peek.exe'
if (Test-Path -LiteralPath $InstalledExe -PathType Leaf) {
    & $InstalledExe --uninstall
    if ($LASTEXITCODE -ne 0) { throw 'Falló la desinstalación de CodePeek.' }
} else {
    $ReleaseExe = Join-Path $PSScriptRoot "..\target\release\peek.exe"
    if (Test-Path -LiteralPath $ReleaseExe -PathType Leaf) {
        & $ReleaseExe --uninstall
        if ($LASTEXITCODE -ne 0) { throw 'Falló la desinstalación de CodePeek.' }
    } else {
        Write-Host "CodePeek no parece estar instalado en este sistema." -ForegroundColor Yellow
    }
}
