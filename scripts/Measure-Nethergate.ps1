param(
    [ValidateRange(4, 3600)][int]$DurationSeconds = 30,
    [ValidateRange(1, 60)][int]$IntervalSeconds = 2,
    [string]$Executable = (Join-Path $PSScriptRoot '../src-tauri/target/debug/nethergate-desktop.exe'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../hub-data/resource-measurements')
)
$ErrorActionPreference = 'Stop'
$resolvedExecutable = [IO.Path]::GetFullPath($Executable)
$logicalCores = [Environment]::ProcessorCount
$samples = [Collections.Generic.List[object]]::new()
$previous = @{}
$timer = [Diagnostics.Stopwatch]::StartNew()
do {
    $processes = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, ExecutablePath, CommandLine, Name)
    $roles = @{}
    foreach ($item in $processes) {
        if ($item.ExecutablePath -and [string]::Equals($item.ExecutablePath, $resolvedExecutable, [StringComparison]::OrdinalIgnoreCase)) {
            $roles[[int]$item.ProcessId] = if ($item.CommandLine -match '(^|[\s"])--engine($|[\s"])') { 'engine' } else { 'desktop' }
        }
    }
    if ($roles.Count -eq 0 -and $samples.Count -eq 0) { throw 'Abre primeiro o executável Nethergate indicado em -Executable.' }
    for ($depth = 0; $depth -lt 16; $depth++) {
        $added = $false
        foreach ($item in $processes) {
            $processId = [int]$item.ProcessId
            if (-not $roles.ContainsKey($processId) -and $roles.ContainsKey([int]$item.ParentProcessId)) {
                $roles[$processId] = if ($item.Name -match '^javaw?\.exe$') { 'java' } elseif ($item.Name -eq 'msedgewebview2.exe') { 'webview' } else { 'helper' }
                $added = $true
            }
        }
        if (-not $added) { break }
    }
    $elapsed = $timer.Elapsed.TotalSeconds
    foreach ($processId in @($roles.Keys)) {
        try {
            $process = Get-Process -Id $processId -ErrorAction Stop
            $started = $process.StartTime.ToUniversalTime().Ticks
            $cpu = [double]$process.CPU
            $percent = $null
            $prior = $previous[$processId]
            if ($prior -and $prior.Started -eq $started -and $elapsed -gt $prior.Elapsed) {
                $percent = [Math]::Round([Math]::Max(0, ($cpu - $prior.Cpu) / ($elapsed - $prior.Elapsed) / $logicalCores * 100), 3)
            }
            $samples.Add([pscustomobject]@{ Seconds = [Math]::Round($elapsed, 3); Role = $roles[$processId]; ProcessId = $processId; CpuPercent = $percent; WorkingSetBytes = $process.WorkingSet64; PrivateBytes = $process.PrivateMemorySize64 })
            $previous[$processId] = @{ Started = $started; Cpu = $cpu; Elapsed = $elapsed }
        } catch [Microsoft.PowerShell.Commands.ProcessCommandException] { }
    }
    if ($timer.Elapsed.TotalSeconds -lt $DurationSeconds) { Start-Sleep -Seconds $IntervalSeconds }
} while ($timer.Elapsed.TotalSeconds -lt $DurationSeconds)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
$csv = Join-Path $OutputDirectory "$stamp.csv"
$summaryPath = Join-Path $OutputDirectory "$stamp.json"
$samples | Export-Csv -LiteralPath $csv -NoTypeInformation -Encoding utf8
$summary = @($samples | Group-Object Role | ForEach-Object {
    $group = $_
    $totals = @($group.Group | Group-Object Seconds | ForEach-Object {
        $validCpu = @($_.Group | Where-Object { $null -ne $_.CpuPercent })
        [pscustomobject]@{ WorkingSetBytes = ($_.Group | Measure-Object WorkingSetBytes -Sum).Sum; PrivateBytes = ($_.Group | Measure-Object PrivateBytes -Sum).Sum; CpuPercent = if ($validCpu.Count -gt 0) { ($validCpu | Measure-Object CpuPercent -Sum).Sum } else { $null } }
    })
    $validCpuTotals = @($totals | Where-Object { $null -ne $_.CpuPercent })
    @{ role = $group.Name; peakWorkingSetMiB = [Math]::Round(($totals | Measure-Object WorkingSetBytes -Maximum).Maximum / 1MB, 2); peakPrivateMiB = [Math]::Round(($totals | Measure-Object PrivateBytes -Maximum).Maximum / 1MB, 2); meanCpuPercent = if ($validCpuTotals.Count -gt 0) { [Math]::Round(($validCpuTotals | Measure-Object CpuPercent -Average).Average, 3) } else { $null } }
})
@{ durationSeconds = $timer.Elapsed.TotalSeconds; logicalCores = $logicalCores; roles = $summary; note = 'CPU normalizada por núcleos lógicos; Working Set soma páginas partilhadas e não equivale a RAM exclusiva. Primeira amostra CPU sem delta. Dados desta sessão, não uma garantia.' } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $summaryPath -Encoding utf8
Write-Output "CSV: $csv"
Write-Output "Resumo: $summaryPath"
