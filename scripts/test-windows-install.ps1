param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
# This test changes installation state only on a disposable GitHub runner.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') { throw 'Este test requiere un runner Windows desechable de GitHub.' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$directory = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\CodePeek'
$installed = Join-Path $directory 'peek.exe'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CodePeek'
if ((Test-Path -LiteralPath $directory) -or (Test-Path -LiteralPath $uninstallKey)) { throw 'El runner ya tiene una instalacion de CodePeek.' }
function Invoke-Integration([string]$program, [string]$mode) {
    $process = Start-Process -FilePath $program -ArgumentList $mode -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Fallo $mode con codigo $($process.ExitCode)." }
}
function Assert-Installed {
    if ((Get-FileHash -LiteralPath $Executable).Hash -ne (Get-FileHash -LiteralPath $installed).Hash) { throw 'El ejecutable instalado no coincide.' }
    if (-not (Test-Path -LiteralPath (Join-Path $directory 'THIRD-PARTY-NOTICES.md'))) { throw 'Faltan los avisos de terceros.' }
    $shell = New-Object -ComObject WScript.Shell
    foreach ($folder in @('Programs','DesktopDirectory')) {
        $shortcut = Join-Path ([Environment]::GetFolderPath($folder)) 'CodePeek.lnk'
        if ($shell.CreateShortcut($shortcut).TargetPath -ne $installed) { throw 'El acceso directo apunta a otra version.' }
    }
    if ((Get-ItemProperty -LiteralPath $uninstallKey).InstallLocation -ne $directory) { throw 'Registro de desinstalacion incorrecto.' }
}
function Assert-Uninstalled {
    if ((Test-Path -LiteralPath $installed) -or (Test-Path -LiteralPath $uninstallKey)) { throw 'La desinstalacion no retiro el ejecutable o el registro.' }
    foreach ($folder in @('Programs','DesktopDirectory')) {
        if (Test-Path -LiteralPath (Join-Path ([Environment]::GetFolderPath($folder)) 'CodePeek.lnk')) { throw 'Quedo un acceso directo.' }
    }
    $entries = [Environment]::GetEnvironmentVariable('PATH','User') -split ';'
    if ($entries -contains $directory) { throw 'Quedo la entrada del PATH.' }
}
Invoke-Integration $Executable '--install'
Assert-Installed
# Exercise removal while the installed executable itself is still mapped.
Invoke-Integration $installed '--uninstall'
Assert-Uninstalled
Invoke-Integration $Executable '--install'
Assert-Installed
if (Get-ChildItem -LiteralPath $directory -Filter 'peek.uninstalled-*.exe') { throw 'No se limpiaron las versiones retiradas.' }
Invoke-Integration $Executable '--uninstall'
Assert-Uninstalled
Write-Output 'Instalacion, retirada de la instancia activa y reinstalacion verificadas.'
