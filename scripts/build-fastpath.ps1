[CmdletBinding()]
param(
    [string]$OutputDir = '',
    [switch]$SkipGraphCheck
)

$ErrorActionPreference = 'Stop'

$ScriptRoot = if ([string]::IsNullOrWhiteSpace($PSScriptRoot)) {
    Split-Path -Parent $MyInvocation.MyCommand.Path
} else {
    $PSScriptRoot
}
$ProjectRoot = Split-Path -Parent $ScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $ProjectRoot 'src-tauri\resources\fastpath'
}

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($architecture -ne 'X64' -or (-not $IsWindows -and -not $IsLinux)) {
    throw 'The embedding fast path sidecar is supported only on Windows x64 and Linux x64.'
}

$platform = if ($IsWindows) { 'windows-x64' } else { 'linux-x64' }
$binaryName = if ($IsWindows) { 'vekai-embedding-fastpath.exe' } else { 'vekai-embedding-fastpath' }

$cargo = Get-Command cargo -ErrorAction Stop
$manifest = Join-Path $ProjectRoot 'src-fastpath\Cargo.toml'
$args = @('build', '--release', '--locked', '--manifest-path', $manifest)
& $cargo.Source @args
$exitCode = $LASTEXITCODE
if ($exitCode -ne 0) {
    throw "cargo build failed with exit code $exitCode"
}

$targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    Join-Path $ProjectRoot 'src-fastpath\target'
} elseif ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
    $env:CARGO_TARGET_DIR
} else {
    Join-Path $ProjectRoot $env:CARGO_TARGET_DIR
}
$sourceBinary = Join-Path (Join-Path $targetRoot 'release') $binaryName
if (-not (Test-Path -LiteralPath $sourceBinary -PathType Leaf)) {
    throw "Fast path build did not produce $sourceBinary"
}

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$destinationBinary = Join-Path $OutputDir $binaryName
foreach ($generatedName in @('vekai-embedding-fastpath.exe', 'vekai-embedding-fastpath')) {
    $generatedPath = Join-Path $OutputDir $generatedName
    if ($generatedName -ne $binaryName -and (Test-Path -LiteralPath $generatedPath -PathType Leaf)) {
        Remove-Item -LiteralPath $generatedPath -Force
    }
}
Copy-Item -LiteralPath $sourceBinary -Destination $destinationBinary -Force

if ($IsLinux) {
    $chmod = Get-Command chmod -CommandType Application -ErrorAction Stop | Select-Object -First 1
    $chmodArgs = @('755', '--', $destinationBinary)
    & $chmod.Source @chmodArgs
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "chmod failed with exit code $exitCode for $destinationBinary"
    }
}

if (-not $SkipGraphCheck) {
    $graph = Join-Path $OutputDir 'qwen3-embedding-0.6b-fp16.onnx'
    if (-not (Test-Path -LiteralPath $graph -PathType Leaf)) {
        throw "Prepared fast path graph is missing: $graph"
    }
    $item = Get-Item -LiteralPath $graph
    $hash = (Get-FileHash -LiteralPath $graph -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($item.Length -ne 575769 -or $hash -ne '40bb4922e22c9d9fdfef69fe71db29be0b41715e88d9442a902df2afaef6fae4') {
        throw "Prepared fast path graph failed verification: $graph"
    }
}

Write-Host "[fastpath-build] $platform sidecar ready: $destinationBinary"
