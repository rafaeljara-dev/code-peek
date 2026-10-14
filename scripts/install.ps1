param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
if (-not $SkipBuild) {
    Push-Location $projectRoot
    try {
        cargo build --release --bin peek
        if ($LASTEXITCODE -ne 0) { throw 'Falló la compilación de CodePeek.' }
    } finally { Pop-Location }
}
$releaseExe = Join-Path $projectRoot 'target\release\peek.exe'
$installDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\CodePeek'
$installedExe = Join-Path $installDir 'peek.exe'
New-Item -ItemType Directory -Path $installDir -Force | Out-Null
$stagedExe = Join-Path $installDir ('peek.staged-{0}.exe' -f [guid]::NewGuid().ToString('N'))
$previousExe = Join-Path $installDir ('peek.previous-{0}.exe' -f [guid]::NewGuid().ToString('N'))
Copy-Item -LiteralPath $releaseExe -Destination $stagedExe
if ((Get-FileHash -LiteralPath $releaseExe).Hash -ne (Get-FileHash -LiteralPath $stagedExe).Hash) {
    Remove-Item -LiteralPath $stagedExe
    throw 'La copia provisional no coincide con la compilación.'
}
try {
    # Windows permits renaming a mapped executable; preserve any open application.
    if (Test-Path -LiteralPath $installedExe) { Move-Item -LiteralPath $installedExe -Destination $previousExe }
    try {
        Move-Item -LiteralPath $stagedExe -Destination $installedExe
    } catch {
        if (Test-Path -LiteralPath $previousExe) { Move-Item -LiteralPath $previousExe -Destination $installedExe }
        throw
    }
} finally {
    if (Test-Path -LiteralPath $stagedExe) { Remove-Item -LiteralPath $stagedExe }
}
# A running previous version keeps its executable until Windows releases the mapping.
Get-ChildItem -LiteralPath $installDir -Filter 'peek.previous-*.exe' -File | ForEach-Object {
    try { Remove-Item -LiteralPath $_.FullName -ErrorAction Stop } catch { Write-Verbose 'La versión anterior sigue abierta; se limpiará en una instalación posterior.' }
}
foreach ($asset in @('icon.ico', 'icon.png')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "assets\$asset") -Destination $installDir -Force
}
foreach ($license in @('DEVICON-LICENSE', 'CODICONS-LICENSE')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "crates\peek-ui\assets\$license") -Destination $installDir -Force
}
$shortcutShell = New-Object -ComObject WScript.Shell
foreach ($folder in @('Programs', 'DesktopDirectory')) {
    $shortcutPath = Join-Path ([Environment]::GetFolderPath($folder)) 'CodePeek.lnk'
    $shortcut = $shortcutShell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $installedExe
    $shortcut.WorkingDirectory = $installDir
    $shortcut.IconLocation = "$(Join-Path $installDir 'icon.ico'),0"
    $shortcut.Description = 'CodePeek — Inspector de código nativo'
    $shortcut.Save()
    if ($shortcutShell.CreateShortcut($shortcutPath).TargetPath -ne $installedExe) {
        throw "No se pudo verificar el acceso directo: $shortcutPath"
    }
}
$registryPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CodePeek'
New-Item -Path $registryPath -Force | Out-Null
$entries = @{
    DisplayName = 'CodePeek'
    DisplayVersion = '0.1.0'
    Publisher = 'CodePeek Contributors'
    InstallLocation = $installDir
    DisplayIcon = (Join-Path $installDir 'icon.ico')
    UninstallString = ('"{0}" --uninstall' -f $installedExe)
}
foreach ($entry in $entries.GetEnumerator()) {
    New-ItemProperty -Path $registryPath -Name $entry.Key -Value $entry.Value -PropertyType String -Force | Out-Null
}
if ((Get-FileHash -LiteralPath $releaseExe).Hash -ne (Get-FileHash -LiteralPath $installedExe).Hash) {
    throw 'El ejecutable instalado no coincide con la compilación.'
}
Write-Host "CodePeek instalado y verificado: $installedExe. Disponible en Inicio y en el escritorio."
