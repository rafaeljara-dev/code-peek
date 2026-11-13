# Embedded in peek.exe. A single installer implementation; paths arrive as data.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$mode = $env:CODEPEEK_INSTALL_MODE
if ($mode -notin @('install', 'uninstall', 'context', 'configure')) { throw 'Modo de integración inválido.' }
$installDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\CodePeek'
$installedExe = Join-Path $installDir 'peek.exe'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CodePeek'
$capabilitiesKey = 'HKCU:\Software\CodePeek'
$registeredAppsKey = 'HKCU:\Software\RegisteredApplications'
$progIdKey = 'HKCU:\Software\Classes\CodePeek.Document'
$appRegKey = 'HKCU:\Software\Classes\Applications\peek.exe'
$icon = Join-Path $installDir 'icon.ico'
# Folders receive the selected path as %V; files receive it as %1.
$contextRoots = [ordered]@{
    'HKCU:\Software\Classes\Directory\shell\CodePeek' = '%V'
    'HKCU:\Software\Classes\Directory\Background\shell\CodePeek' = '%V'
    'HKCU:\Software\Classes\*\shell\CodePeek' = '%1'
}
$markdownExts = @('.md', '.markdown', '.mdx', '.mdown', '.mkdn')
$codeExts = @(
    '.txt', '.log', '.json', '.jsonc', '.toml', '.yaml', '.yml', '.xml', '.ini', '.conf', '.env',
    '.rs', '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.py', '.go', '.c', '.cpp', '.h', '.hpp',
    '.cs', '.java', '.kt', '.html', '.htm', '.css', '.scss', '.sh', '.ps1', '.sql', '.svg'
)
$docExts = @('.pdf', '.xlsx', '.xlsm', '.xls', '.ods', '.csv', '.tsv', '.docx')
$allManagedExts = $markdownExts + $codeExts + $docExts

# peek.exe writes its owned resources to the payload in every mode: one source of truth.
$resources = @()
if ($env:CODEPEEK_INSTALL_ASSETS -and (Test-Path -LiteralPath $env:CODEPEEK_INSTALL_ASSETS -PathType Container)) {
    $resources = @((Get-ChildItem -LiteralPath $env:CODEPEEK_INSTALL_ASSETS -File).Name)
}

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

function Remove-OpenWithIntegration {
    foreach ($ext in $allManagedExts) {
        $progidsKey = "HKCU:\Software\Classes\$ext\OpenWithProgids"
        if (Test-Path -LiteralPath $progidsKey) {
            Remove-ItemProperty -LiteralPath $progidsKey -Name 'CodePeek.Document' -ErrorAction SilentlyContinue
        }
    }
    foreach ($key in @($progIdKey, $appRegKey, $capabilitiesKey)) {
        if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse -ErrorAction SilentlyContinue }
    }
    if (Test-Path -LiteralPath $registeredAppsKey) {
        Remove-ItemProperty -LiteralPath $registeredAppsKey -Name 'CodePeek' -ErrorAction SilentlyContinue
    }
}

function Notify-ShellAssociations {
    try {
        if (-not ([System.Management.Automation.PSTypeName]'CodePeekShellNotify').Type) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CodePeekShellNotify {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
}
'@
        }
        [CodePeekShellNotify]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
    } catch { }
}

function Get-Flag([string]$name, [bool]$defaultValue) {
    $raw = [Environment]::GetEnvironmentVariable($name)
    if ([string]::IsNullOrWhiteSpace($raw)) { return $defaultValue }
    return ($raw -eq '1' -or $raw -ieq 'true')
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
    Remove-OpenWithIntegration
    Notify-ShellAssociations
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

$iconValue = ('{0},0' -f $source)
$enableContextMenu = Get-Flag 'CODEPEEK_ENABLE_CONTEXT_MENU' $true
$enableOpenWithMarkdown = Get-Flag 'CODEPEEK_OPEN_WITH_MARKDOWN' $true
$enableOpenWithCode = Get-Flag 'CODEPEEK_OPEN_WITH_CODE' $true
$enableOpenWithDocs = Get-Flag 'CODEPEEK_OPEN_WITH_DOCS' $true

if ($enableContextMenu) {
    foreach ($root in $contextRoots.GetEnumerator()) {
        $key = $root.Key
        New-Item -Path $key -Force | Out-Null
        Set-Item -LiteralPath $key -Value 'Abrir con CodePeek'
        New-ItemProperty -LiteralPath $key -Name Icon -Value $iconValue -PropertyType String -Force | Out-Null
        $commandKey = Join-Path $key 'command'
        New-Item -Path $commandKey -Force | Out-Null
        Set-Item -LiteralPath $commandKey -Value ('"{0}" -- "{1}"' -f $source, $root.Value)
    }
} else {
    foreach ($key in $contextRoots.Keys) {
        if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse -ErrorAction SilentlyContinue }
    }
}

$enabledExts = @()
if ($enableOpenWithMarkdown) { $enabledExts += $markdownExts }
if ($enableOpenWithCode) { $enabledExts += $codeExts }
if ($enableOpenWithDocs) { $enabledExts += $docExts }

if ($enabledExts.Count -gt 0) {
    $openCommand = ('"{0}" -- "%1"' -f $source)
    New-Item -Path (Join-Path $progIdKey 'DefaultIcon') -Force | Out-Null
    Set-Item -LiteralPath $progIdKey -Value 'Documento de CodePeek'
    New-ItemProperty -LiteralPath $progIdKey -Name 'FriendlyTypeName' -Value 'CodePeek' -PropertyType String -Force | Out-Null
    Set-Item -LiteralPath (Join-Path $progIdKey 'DefaultIcon') -Value $iconValue
    New-Item -Path (Join-Path $progIdKey 'shell\open\command') -Force | Out-Null
    Set-Item -LiteralPath (Join-Path $progIdKey 'shell\open\command') -Value $openCommand

    $supportedTypesKey = Join-Path $appRegKey 'SupportedTypes'
    if (Test-Path -LiteralPath $supportedTypesKey) { Remove-Item -LiteralPath $supportedTypesKey -Recurse -ErrorAction SilentlyContinue }
    New-Item -Path $supportedTypesKey -Force | Out-Null
    New-Item -Path (Join-Path $appRegKey 'DefaultIcon') -Force | Out-Null
    New-Item -Path (Join-Path $appRegKey 'shell\open\command') -Force | Out-Null
    New-ItemProperty -LiteralPath $appRegKey -Name 'FriendlyAppName' -Value 'CodePeek' -PropertyType String -Force | Out-Null
    New-ItemProperty -LiteralPath $appRegKey -Name 'ApplicationIcon' -Value $iconValue -PropertyType String -Force | Out-Null
    Set-Item -LiteralPath (Join-Path $appRegKey 'DefaultIcon') -Value $iconValue
    New-ItemProperty -LiteralPath (Join-Path $appRegKey 'shell\open') -Name 'FriendlyAppName' -Value 'Abrir con CodePeek' -PropertyType String -Force | Out-Null
    New-ItemProperty -LiteralPath (Join-Path $appRegKey 'shell\open') -Name 'Icon' -Value $iconValue -PropertyType String -Force | Out-Null
    Set-Item -LiteralPath (Join-Path $appRegKey 'shell\open\command') -Value $openCommand

    $assocKey = Join-Path $capabilitiesKey 'Capabilities\FileAssociations'
    if (Test-Path -LiteralPath $assocKey) { Remove-Item -LiteralPath $assocKey -Recurse -ErrorAction SilentlyContinue }
    New-Item -Path $assocKey -Force | Out-Null
    New-ItemProperty -LiteralPath (Join-Path $capabilitiesKey 'Capabilities') -Name 'ApplicationName' -Value 'CodePeek' -PropertyType String -Force | Out-Null
    New-ItemProperty -LiteralPath (Join-Path $capabilitiesKey 'Capabilities') -Name 'ApplicationDescription' -Value 'Inspector y visor nativo rápido de código, Markdown y documentos' -PropertyType String -Force | Out-Null
    New-ItemProperty -LiteralPath (Join-Path $capabilitiesKey 'Capabilities') -Name 'ApplicationIcon' -Value $iconValue -PropertyType String -Force | Out-Null

    foreach ($ext in $allManagedExts) {
        $progidsKey = "HKCU:\Software\Classes\$ext\OpenWithProgids"
        if ($enabledExts -contains $ext) {
            New-Item -Path $progidsKey -Force | Out-Null
            New-ItemProperty -LiteralPath $progidsKey -Name 'CodePeek.Document' -Value '' -PropertyType String -Force | Out-Null
            New-ItemProperty -LiteralPath $supportedTypesKey -Name $ext -Value '' -PropertyType String -Force | Out-Null
            New-ItemProperty -LiteralPath $assocKey -Name $ext -Value 'CodePeek.Document' -PropertyType String -Force | Out-Null
        } elseif (Test-Path -LiteralPath $progidsKey) {
            Remove-ItemProperty -LiteralPath $progidsKey -Name 'CodePeek.Document' -ErrorAction SilentlyContinue
        }
    }

    New-Item -Path $registeredAppsKey -Force | Out-Null
    New-ItemProperty -LiteralPath $registeredAppsKey -Name 'CodePeek' -Value 'Software\CodePeek\Capabilities' -PropertyType String -Force | Out-Null
} else {
    Remove-OpenWithIntegration
}

Notify-ShellAssociations
Write-Output "CodePeek instalado y verificado: $source"