# Embedded in peek.exe. A single installer implementation; paths arrive as data.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$mode = $env:CODEPEEK_INSTALL_MODE
if ($mode -notin @('install', 'uninstall', 'context')) { throw 'Modo de integración inválido.' }
$installDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\CodePeek'
$installedExe = Join-Path $installDir 'peek.exe'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CodePeek'
$icon = Join-Path $installDir 'icon.ico'
# Folders receive the selected path as %V; files receive it as %1.
$contextRoots = [ordered]@{
    'HKCU:\Software\Classes\Directory\shell\CodePeek' = '%V'
    'HKCU:\Software\Classes\Directory\Background\shell\CodePeek' = '%V'
    'HKCU:\Software\Classes\*\shell\CodePeek' = '%1'
}
# peek.exe writes its owned resources to the payload in every mode: one source of truth.
$resources = @((Get-ChildItem -LiteralPath $env:CODEPEEK_INSTALL_ASSETS -File).Name)

function Assert-InstallDirectory {
    $component = $installDir
    while ($component) {
        if (Test-Path -LiteralPath $component) {
            $item = Get-Item -LiteralPath $component -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "La instalación no admite enlaces de directorio: $component" }
        }
        $component = Split-Path $component -Parent
    }
}

function Set-UserPath([bool]$include) {
    $old = [Environment]::GetEnvironmentVariable('PATH', 'User')
    $entries = @($old -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ine $installDir.TrimEnd('\') })
    if ($include) { $entries += $installDir }
    [Environment]::SetEnvironmentVariable('PATH', ($entries -join ';'), 'User')
}

function Retire-Executable([string]$directory) {
    $executable = Join-Path $directory 'peek.exe'
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { return $null }
    $retired = Join-Path $directory ('peek.uninstalled-{0}.exe' -f [guid]::NewGuid().ToString('N'))
    # Fail before removing integration if another process forbids renaming the file.
    Move-Item -LiteralPath $executable -Destination $retired
    return $retired
}

function Remove-RetiredExecutable([string]$retired) {
    if ($retired) {
        try { Remove-Item -LiteralPath $retired -ErrorAction Stop }
        catch [IO.IOException], [UnauthorizedAccessException] {
            Write-Output "El ejecutable retirado no pudo borrarse; se reintentará en la siguiente instalación: $retired"
        }
    }
}

Assert-InstallDirectory
if ($mode -eq 'uninstall') {
    $retired = Retire-Executable $installDir
    foreach ($folder in @('Programs', 'DesktopDirectory')) {
        $shortcut = Join-Path ([Environment]::GetFolderPath($folder)) 'CodePeek.lnk'
        if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut }
    }
    Set-UserPath $false
    foreach ($key in @($uninstallKey) + @($contextRoots.Keys)) {
        if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse }
    }
    Assert-InstallDirectory
    # Delete owned files only. Unknown files and user settings are preserved.
    foreach ($name in $resources) {
        $item = Join-Path $installDir $name
        if (Test-Path -LiteralPath $item -PathType Leaf) { Remove-Item -LiteralPath $item }
    }
    Remove-RetiredExecutable $retired
    if ((Test-Path -LiteralPath $installDir) -and -not (Get-ChildItem -LiteralPath $installDir -Force | Select-Object -First 1)) { Remove-Item -LiteralPath $installDir }
    exit 0
}

$source = $env:CODEPEEK_INSTALL_SOURCE
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'No existe el ejecutable de origen.' }
if ($mode -eq 'install') {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Assert-InstallDirectory
    if ([IO.Path]::GetFullPath($source) -ine [IO.Path]::GetFullPath($installedExe)) {
        $staged = Join-Path $installDir ('peek.staged-{0}.exe' -f [guid]::NewGuid().ToString('N'))
        $previous = Join-Path $installDir ('peek.previous-{0}.exe' -f [guid]::NewGuid().ToString('N'))
        try {
            Copy-Item -LiteralPath $source -Destination $staged
            if ((Get-FileHash -LiteralPath $source).Hash -ne (Get-FileHash -LiteralPath $staged).Hash) { throw 'La copia provisional no coincide con el origen.' }
            if (Test-Path -LiteralPath $installedExe) { Move-Item -LiteralPath $installedExe -Destination $previous }
            try { Move-Item -LiteralPath $staged -Destination $installedExe }
            catch {
                if (Test-Path -LiteralPath $previous) { Move-Item -LiteralPath $previous -Destination $installedExe }
                throw
            }
        } finally { if (Test-Path -LiteralPath $staged) { Remove-Item -LiteralPath $staged } }
    }
    foreach ($name in $resources) { Copy-Item -LiteralPath (Join-Path $env:CODEPEEK_INSTALL_ASSETS $name) -Destination (Join-Path $installDir $name) -Force }
    $shell = New-Object -ComObject WScript.Shell
    foreach ($folder in @('Programs', 'DesktopDirectory')) {
        $shortcutPath = Join-Path ([Environment]::GetFolderPath($folder)) 'CodePeek.lnk'
        $shortcut = $shell.CreateShortcut($shortcutPath)
        $shortcut.TargetPath = $installedExe
        $shortcut.WorkingDirectory = $installDir
        $shortcut.IconLocation = $icon + ',0'
        $shortcut.Description = 'CodePeek — Inspector de código nativo'
        $shortcut.Save()
        if ($shell.CreateShortcut($shortcutPath).TargetPath -ne $installedExe) { throw 'No se pudo verificar el acceso directo.' }
    }
    Set-UserPath $true
    New-Item -Path $uninstallKey -Force | Out-Null
    $values = @{DisplayName='CodePeek'; DisplayVersion=$env:CODEPEEK_INSTALL_VERSION; Publisher='CodePeek Contributors'; InstallLocation=$installDir; DisplayIcon=$icon; UninstallString=('"{0}" --uninstall' -f $installedExe)}
    foreach ($value in $values.GetEnumerator()) { New-ItemProperty -LiteralPath $uninstallKey -Name $value.Key -Value $value.Value -PropertyType String -Force | Out-Null }
    if ((Get-FileHash -LiteralPath $source).Hash -ne (Get-FileHash -LiteralPath $installedExe).Hash) { throw 'El ejecutable instalado no coincide con el origen.' }
    $source = $installedExe
    Get-ChildItem -LiteralPath $installDir -File | Where-Object { $_.Name -match '^peek\.(previous|uninstalled)-[0-9a-f]{32}\.exe$' } | ForEach-Object {
        try { Remove-Item -LiteralPath $_.FullName -ErrorAction Stop } catch { Write-Verbose 'La versión anterior sigue abierta.' }
    }
}

foreach ($root in $contextRoots.GetEnumerator()) {
    $key = $root.Key
    New-Item -Path $key -Force | Out-Null
    Set-Item -LiteralPath $key -Value 'Abrir con CodePeek'
    New-ItemProperty -LiteralPath $key -Name Icon -Value $icon -PropertyType String -Force | Out-Null
    $commandKey = Join-Path $key 'command'
    New-Item -Path $commandKey -Force | Out-Null
    Set-Item -LiteralPath $commandKey -Value ('"{0}" -- "{1}"' -f $source, $root.Value)
}
Write-Output "CodePeek instalado y verificado: $source"
