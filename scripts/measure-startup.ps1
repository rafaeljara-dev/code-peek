param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][string]$Executable,
    [string[]]$Arguments = @(),
    [ValidateRange(1, 20)][int]$Runs = 5
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$results = @()

for ($run = 1; $run -le $Runs; $run++) {
    $startedAt = Get-Date
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $launcher = Start-Process -FilePath $resolvedExecutable -ArgumentList $Arguments -PassThru
    $interactive = $null

    while ($watch.Elapsed.TotalSeconds -lt 30) {
        $candidates = Get-Process -ErrorAction SilentlyContinue | Where-Object {
            try { $_.Path -eq $resolvedExecutable -and $_.StartTime -ge $startedAt } catch { $false }
        }
        $interactive = $candidates | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if ($interactive) { break }
        Start-Sleep -Milliseconds 10
    }

    if (-not $interactive) {
        throw "$Name no mostró una ventana interactiva dentro de 30 segundos."
    }

    $startupMs = [math]::Round($watch.Elapsed.TotalMilliseconds, 1)
    Start-Sleep -Seconds 3
    $launched = Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -eq $resolvedExecutable -and $_.StartTime -ge $startedAt } catch { $false }
    }
    $workingSetMiB = [math]::Round((($launched | Measure-Object WorkingSet64 -Sum).Sum / 1MB), 1)
    $results += [pscustomobject]@{
        Application = $Name
        Run = $run
        StartupMs = $startupMs
        WorkingSetMiB = $workingSetMiB
    }
    $launched | Stop-Process -Force
    $launcher.WaitForExit(5000) | Out-Null
}

$results | Format-Table -AutoSize
$results | ConvertTo-Json
