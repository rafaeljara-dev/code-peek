param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$ApplicationArguments = '',
    [ValidateRange(1, 10)][int]$Runs = 5,
    [ValidateRange(1, 20)][int]$SettleSeconds = 3,
    [ValidateRange(1, 20)][int]$SampleSeconds = 3,
    [long]$AffinityMask = 0,
    [Parameter(Mandatory = $true)][string]$OutputPath
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$logicalProcessors = (Get-CimInstance Win32_Processor | Measure-Object NumberOfLogicalProcessors -Sum).Sum
$records = [System.Collections.Generic.List[object]]::new()

if (-not ('CodePeekAuditWindows' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CodePeekAuditWindows {
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll")] private static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    public static IntPtr FindWindowForProcess(int processId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, unused) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner != processId) return true;
            var title = new StringBuilder(512);
            GetWindowText(window, title, title.Capacity);
            if (!title.ToString().EndsWith("CodePeek")) return true;
            found = window;
            return false;
        }, IntPtr.Zero);
        return found;
    }
    public static void Hide(IntPtr window) { if (IsWindowVisible(window)) ShowWindow(window, 0); }
    public static void Close(IntPtr window) { PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero); }
}
'@
}

for ($run = 1; $run -le $Runs; $run++) {
    $info = [System.Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $resolvedExecutable
    $info.Arguments = $ApplicationArguments
    $info.WorkingDirectory = Split-Path -Parent $resolvedExecutable
    $info.UseShellExecute = $false
    $info.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    $info.CreateNoWindow = $true
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $auditProcess = [System.Diagnostics.Process]::Start($info)
    $window = [IntPtr]::Zero
    try {
        if ($AffinityMask -gt 0) { $auditProcess.ProcessorAffinity = [IntPtr]$AffinityMask }
        while ($watch.Elapsed.TotalSeconds -lt 30) {
            if ($auditProcess.HasExited) { throw "El proceso terminó antes de mostrar una ventana: $($auditProcess.ExitCode)" }
            $window = [CodePeekAuditWindows]::FindWindowForProcess($auditProcess.Id)
            if ($window -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 5
        }
        if ($window -eq [IntPtr]::Zero) { throw 'No se encontró la ventana de CodePeek en 30 segundos.' }
        $windowObservedMs = $watch.Elapsed.TotalMilliseconds
        [CodePeekAuditWindows]::Hide($window)
        $settleWatch = [System.Diagnostics.Stopwatch]::StartNew()
        while ($settleWatch.Elapsed.TotalSeconds -lt $SettleSeconds) {
            if ($auditProcess.HasExited) { throw "La aplicación terminó durante la estabilización (ejecución $run, código $($auditProcess.ExitCode))." }
            [CodePeekAuditWindows]::Hide($window)
            Start-Sleep -Milliseconds 50
        }
        $auditProcess.Refresh()
        $cpuStart = $auditProcess.TotalProcessorTime.TotalMilliseconds
        $sampleWatch = [System.Diagnostics.Stopwatch]::StartNew()
        while ($sampleWatch.Elapsed.TotalSeconds -lt $SampleSeconds) {
            if ($auditProcess.HasExited) { throw "La aplicación terminó durante el muestreo (ejecución $run, código $($auditProcess.ExitCode))." }
            [CodePeekAuditWindows]::Hide($window)
            Start-Sleep -Milliseconds 100
        }
        $auditProcess.Refresh()
        $cpuMs = $auditProcess.TotalProcessorTime.TotalMilliseconds - $cpuStart
        $record = [pscustomobject]@{
            Run = $run
            AffinityMask = $AffinityMask
            WindowObservedMs = [math]::Round($windowObservedMs, 2)
            WorkingSetMiB = [math]::Round($auditProcess.WorkingSet64 / 1MB, 2)
            PrivateMiB = [math]::Round($auditProcess.PrivateMemorySize64 / 1MB, 2)
            PeakWorkingSetMiB = [math]::Round($auditProcess.PeakWorkingSet64 / 1MB, 2)
            IdleCpuMs = [math]::Round($cpuMs, 3)
            StartupAndSettleCpuMs = [math]::Round($cpuStart, 3)
            SampleWallMs = [math]::Round($sampleWatch.Elapsed.TotalMilliseconds, 2)
            IdleCpuOneLogicalPercent = [math]::Round(100 * $cpuMs / $sampleWatch.Elapsed.TotalMilliseconds, 3)
            IdleCpuMachinePercent = [math]::Round(100 * $cpuMs / $sampleWatch.Elapsed.TotalMilliseconds / $logicalProcessors, 3)
            Threads = $auditProcess.Threads.Count
            Handles = $auditProcess.HandleCount
        }
        $records.Add($record)
        Write-Host "Medición $run/${Runs}: ventana $($record.WindowObservedMs) ms; memoria privada $($record.PrivateMiB) MiB; CPU en reposo $($record.IdleCpuMs) ms."
    } finally {
        if (-not $auditProcess.HasExited) {
            if ($window -ne [IntPtr]::Zero) { [CodePeekAuditWindows]::Close($window) }
            if (-not $auditProcess.WaitForExit(3000)) { $auditProcess.Kill(); $auditProcess.WaitForExit() }
        }
        $auditProcess.Dispose()
    }
}

$report = [pscustomobject]@{
    Date = (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK')
    Executable = $resolvedExecutable
    Sha256 = (Get-FileHash -LiteralPath $resolvedExecutable -Algorithm SHA256).Hash
    LogicalProcessors = $logicalProcessors
    Method = 'Ventana nativa observada con sondeo de 5 ms; memoria y CPU con ventana oculta tras inicializar. No mide primer fotograma, arranque frío, GPU ni batería.'
    AffinityNote = 'La afinidad se aplica después de crear el proceso. Dos procesadores lógicos no equivalen a una laptop de gama baja.'
    Results = $records.ToArray()
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputPath -Encoding utf8
