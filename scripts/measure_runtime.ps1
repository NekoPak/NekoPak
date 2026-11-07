param([int]$Runs = 3)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$taskExecutable = Join-Path $taskRoot 'target\release\nekopak.exe'
if (-not (Test-Path -LiteralPath $taskExecutable)) { throw 'Build the Windows release first.' }
$taskMeasurements = @()
for ($taskRun = 0; $taskRun -lt $Runs; $taskRun++) {
    $taskWatch = [System.Diagnostics.Stopwatch]::StartNew()
    $taskProcess = Start-Process -FilePath $taskExecutable -PassThru -WindowStyle Hidden
    try {
        $taskWindowMs = $null
        while ($taskWatch.Elapsed.TotalSeconds -lt 10) {
            $taskProcess.Refresh()
            if ($taskProcess.HasExited) { throw ('Application exited with code ' + $taskProcess.ExitCode) }
            if ($taskProcess.MainWindowHandle -ne 0) { $taskWindowMs = $taskWatch.ElapsedMilliseconds; break }
            Start-Sleep -Milliseconds 100
        }
        Start-Sleep -Seconds 5
        $taskProcess.Refresh()
        if ($taskProcess.HasExited) { throw 'Application exited before resource sampling.' }
        $taskAllProcesses = @(Get-CimInstance Win32_Process)
        $taskIds = [System.Collections.Generic.HashSet[int]]::new()
        $taskIds.Add($taskProcess.Id) | Out-Null
        do {
            $taskAdded = $false
            foreach ($taskChild in $taskAllProcesses) {
                if ($taskIds.Contains([int]$taskChild.ParentProcessId) -and $taskIds.Add([int]$taskChild.ProcessId)) { $taskAdded = $true }
            }
        } while ($taskAdded)
        $taskCounters = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $taskIds.Contains([int]$_.IDProcess) })
        $taskSamples = @($taskCounters | ForEach-Object { @{ name = $_.Name; pid = $_.IDProcess; working_set_bytes = [long]$_.WorkingSet; private_working_set_bytes = [long]$_.WorkingSetPrivate; private_committed_bytes = [long]$_.PrivateBytes } })
        $taskMeasurements += @{
            run = $taskRun + 1
            window_handle_ms = $taskWindowMs
            process_count = $taskSamples.Count
            processes = $taskSamples
            total_working_set_bytes = ($taskSamples | Measure-Object -Property working_set_bytes -Sum).Sum
            total_private_working_set_bytes = ($taskSamples | Measure-Object -Property private_working_set_bytes -Sum).Sum
            main_window_title = $taskProcess.MainWindowTitle
            sampled_after_ms = $taskWatch.ElapsedMilliseconds
        }
    } finally {
        $taskProcess.Refresh()
        if (-not $taskProcess.HasExited) { $taskProcess.Kill($true); $taskProcess.WaitForExit(5000) | Out-Null }
        $taskProcess.Dispose()
    }
}
$taskOs = Get-CimInstance Win32_OperatingSystem
$taskReport = @{
    os = $taskOs.Caption
    version = $taskOs.Version
    architecture = $taskOs.OSArchitecture
    executable_sha256 = (Get-FileHash -LiteralPath $taskExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
    observations = $taskMeasurements
    method = '3 sequential warm launches by default; window handle availability is not first rendered frame. Sample >=5s after handle. Includes descendants such as WebView2. Total working set can double-count shared pages. No workload throughput or GUI click verification.'
    ui_automation = 'Unavailable: computer-use native pipe missing. IPC command roundtrip covered by Rust mock-runtime test.'
}
$taskOut = Join-Path $taskRoot 'dist'
[System.IO.Directory]::CreateDirectory($taskOut) | Out-Null
$taskJson = $taskReport | ConvertTo-Json -Depth 7
[System.IO.File]::WriteAllText((Join-Path $taskOut 'runtime-report.json'), $taskJson, [System.Text.UTF8Encoding]::new($false))
Write-Output $taskJson
