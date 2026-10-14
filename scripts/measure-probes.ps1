param(
    [string]$Executable = '.\target\release\examples\performance_audit.exe',
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [string[]]$Cases = @('syntax-init', 'viewport', 'editing', 'fuzzy', 'image-copy', 'thumbnail', 'load-save')
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$records = [System.Collections.Generic.List[object]]::new()
foreach ($case in $Cases) {
    $info = [System.Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $resolvedExecutable
    $info.Arguments = $case
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $probeProcess = [System.Diagnostics.Process]::Start($info)
    $stdout = $probeProcess.StandardOutput.ReadToEndAsync()
    $stderr = $probeProcess.StandardError.ReadToEndAsync()
    $peakWorkingSet = 0L
    $observedPrivate = 0L
    while (-not $probeProcess.HasExited) {
        try {
            $probeProcess.Refresh()
            $peakWorkingSet = [math]::Max($peakWorkingSet, $probeProcess.PeakWorkingSet64)
            $observedPrivate = [math]::Max($observedPrivate, $probeProcess.PrivateMemorySize64)
        } catch {
            if (-not $probeProcess.HasExited) { throw }
        }
        Start-Sleep -Milliseconds 10
    }
    $output = $stdout.GetAwaiter().GetResult()
    $errorOutput = $stderr.GetAwaiter().GetResult()
    if ($probeProcess.ExitCode -ne 0) { throw "Falló el caso ${case}: $errorOutput" }
    $results = @($output -split '\r?\n' | Where-Object { $_.Trim().Length -gt 0 } | ForEach-Object { $_ | ConvertFrom-Json })
    $records.Add([pscustomobject]@{
        Case = $case
        PeakWorkingSetMiB = [math]::Round($peakWorkingSet / 1MB, 2)
        ObservedPrivatePeakMiB = [math]::Round($observedPrivate / 1MB, 2)
        Results = $results
    })
    $probeProcess.Dispose()
    [pscustomobject]@{
        Date = Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'
        Executable = $resolvedExecutable
        Method = 'Microbenchmarks release del código existente. Memoria por proceso, incluido preparar fixtures; pico privado observado con sondeo de 10 ms. No mide latencia completa de la UI ni memoria GPU.'
        Results = $records.ToArray()
    } | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $OutputPath -Encoding utf8
    Write-Host "Caso ${case}: working set máximo $([math]::Round($peakWorkingSet / 1MB, 2)) MiB."
}
[pscustomobject]@{
    Date = Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'
    Executable = $resolvedExecutable
    Method = 'Microbenchmarks release del código existente. Memoria por proceso, incluido preparar fixtures; pico privado observado con sondeo de 10 ms. No mide latencia completa de la UI ni memoria GPU.'
    Results = $records.ToArray()
} | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $OutputPath -Encoding utf8
