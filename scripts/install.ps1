param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
if (-not $SkipBuild) {
    Push-Location $projectRoot
    try {
        cargo build --locked --release --bin peek
        if ($LASTEXITCODE -ne 0) { throw 'Falló la compilación de CodePeek.' }
    } finally { Pop-Location }
}
$releaseExe = Join-Path $projectRoot 'target\release\peek.exe'
if (-not (Test-Path -LiteralPath $releaseExe -PathType Leaf)) { throw 'Falta compilar target/release/peek.exe.' }
& $releaseExe --install
if ($LASTEXITCODE -ne 0) { throw 'Falló la instalación de CodePeek.' }
