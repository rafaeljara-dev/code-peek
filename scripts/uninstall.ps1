# Script de desinstalación para Windows 11 — CodePeek
$ErrorActionPreference = "Stop"

$InstalledExe = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\CodePeek\peek.exe'
$ReleaseExe = Join-Path $PSScriptRoot "..	argetelease\peek.exe"
$Exe = @($InstalledExe, $ReleaseExe) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if ($Exe) {
    & $Exe --uninstall
    if ($LASTEXITCODE -ne 0) { throw 'Falló la desinstalación de CodePeek.' }
} else {
    Write-Host "CodePeek no parece estar instalado en este sistema." -ForegroundColor Yellow
}
