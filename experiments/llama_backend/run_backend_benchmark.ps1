[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Server,

    [Parameter(Mandatory = $true)]
    [string]$Model,

    [ValidateSet('embedding', 'rerank')]
    [string]$Service = 'embedding',

    [int]$Port = 18013,
    [int]$Parallel = 12,
    [int]$BatchSize = 2048,
    [int]$UBatchSize = 2048,
    [int]$Warmup = 3,
    [int]$Samples = 10,
    [string]$Counts = '1,8,32,64',
    [string]$Python = 'python.exe',
    [switch]$Aggressive,
    [string]$DumpResponse = ''
)

$ErrorActionPreference = 'Stop'

$serverPath = (Resolve-Path -LiteralPath $Server).Path
$modelPath = (Resolve-Path -LiteralPath $Model).Path
$benchmarkPath = Join-Path $PSScriptRoot 'benchmark_server.py'
$urlPath = if ($Service -eq 'embedding') { '/v1/embeddings' } else { '/v1/rerank' }

$arguments = @(
    '-m', $modelPath,
    '--embedding'
)
if ($Service -eq 'rerank') {
    $arguments += @('--pooling', 'rank', '--rerank')
}
$arguments += @(
    '--host', '127.0.0.1',
    '--port', [string]$Port,
    '--no-webui',
    '--cache-ram', '0',
    '--log-verbosity', '1',
    '--n-gpu-layers', 'all',
    '--fit', 'off',
    '--parallel', [string]$Parallel,
    '--batch-size', [string]$BatchSize,
    '--ubatch-size', [string]$UBatchSize
)
if ($Aggressive) {
    $arguments += @(
        '--flash-attn', 'on',
        '--no-host',
        '--prio', '2',
        '--poll', '100'
    )
}

$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $serverPath
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
foreach ($argument in $arguments) {
    $startInfo.ArgumentList.Add($argument)
}

$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$process = [System.Diagnostics.Process]::Start($startInfo)
try {
    $healthUrl = "http://127.0.0.1:$Port/health"
    $deadline = [DateTime]::UtcNow.AddSeconds(120)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($process.HasExited) {
            throw "llama-server exited with code $($process.ExitCode): $($process.StandardError.ReadToEnd())"
        }
        try {
            Invoke-RestMethod -Uri $healthUrl -TimeoutSec 2 | Out-Null
            break
        } catch {
            Start-Sleep -Milliseconds 250
        }
    }
    if ([DateTime]::UtcNow -ge $deadline) {
        throw "llama-server did not become healthy within 120 seconds"
    }

    $stopwatch.Stop()
    Write-Host "startup_ms=$([Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 1))"
    $benchmarkArguments = @(
        $benchmarkPath,
        '--service', $Service,
        '--url', "http://127.0.0.1:$Port$urlPath",
        '--counts', $Counts,
        '--warmup', [string]$Warmup,
        '--samples', [string]$Samples
    )
    if (-not [string]::IsNullOrWhiteSpace($DumpResponse)) {
        $benchmarkArguments += @('--dump-response', $DumpResponse)
    }
    & $Python @benchmarkArguments
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "benchmark_server.py failed with exit code $exitCode"
    }
} finally {
    if (-not $process.HasExited) {
        $process.Kill($true)
        $process.WaitForExit()
    }
}
