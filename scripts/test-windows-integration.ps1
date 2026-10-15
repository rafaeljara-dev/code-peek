$ErrorActionPreference = 'Stop'
# Extract only the helper. Never execute the installer against the real user profile.
$script = Join-Path $PSScriptRoot 'windows-integration.ps1'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput((Get-Content -LiteralPath $script -Raw -Encoding UTF8), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Retire-Executable', 'Remove-RetiredExecutable')) {
    $helper = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $helper) { throw "No se encontro $name." }
    . ([scriptblock]::Create($helper.Extent.Text))
}
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('codepeek-install-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
try {
    $directory = Join-Path $scratch (([char]0x00f1).ToString() + " ' [test]")
    New-Item -ItemType Directory -Path $directory | Out-Null
    $executable = Join-Path $directory 'peek.exe'
    [IO.File]::WriteAllText($executable, 'dummy executable')
    $locked = [IO.File]::Open($executable, 'Open', 'Read', 'None')
    try {
        $failed = $false
        try { Retire-Executable $directory | Out-Null } catch { $failed = $true }
        if (-not $failed -or -not (Test-Path -LiteralPath $executable)) { throw 'Un ejecutable bloqueado debe conservar su nombre.' }
    } finally { $locked.Dispose() }
    $retired = Retire-Executable $directory
    if ((Test-Path -LiteralPath $executable) -or [IO.File]::ReadAllText($retired) -ne 'dummy executable') { throw 'La retirada no conservó el archivo.' }
    if ($null -ne (Retire-Executable $directory)) { throw 'Una segunda retirada debe ser inocua.' }
    Remove-RetiredExecutable $retired | Out-Null
    if (Test-Path -LiteralPath $retired) { throw 'No se borro el archivo retirado sin bloqueo.' }
    # Map a copy of a system PE as an image resource: no entry point or code runs.
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CodePeekImageFixture {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr LoadLibraryExW(string path, IntPtr reserved, uint flags);
    [DllImport("kernel32.dll")]
    public static extern bool FreeLibrary(IntPtr module);
}
'@
    Copy-Item -LiteralPath (Join-Path ([Environment]::SystemDirectory) 'where.exe') -Destination $executable
    $module = [CodePeekImageFixture]::LoadLibraryExW($executable, [IntPtr]::Zero, 0x20)
    if ($module -eq [IntPtr]::Zero) { throw 'No se pudo mapear el PE ficticio.' }
    try {
        $retired = Retire-Executable $directory
        Remove-RetiredExecutable $retired | Out-Null
        if (-not (Test-Path -LiteralPath $retired)) { throw 'Se esperaba conservar la imagen mapeada.' }
    } finally { [CodePeekImageFixture]::FreeLibrary($module) | Out-Null }
    Remove-RetiredExecutable $retired | Out-Null
    if (Test-Path -LiteralPath $retired) { throw 'El residuo debe borrarse al liberar la imagen.' }
    Write-Output 'Windows integration: lock, retirement and special paths verified.'
} finally {
    $resolved = [IO.Path]::GetFullPath($scratch)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Ruta temporal inesperada.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
