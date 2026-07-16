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

if (-not $IsWindows -or [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
    throw 'The embedding fast path sidecar is currently supported only on Windows x64.'
}

$cargo = Get-Command cargo -ErrorAction Stop
$manifest = Join-Path $ProjectRoot 'src-fastpath\Cargo.toml'
$args = @('build', '--release', '--locked', '--manifest-path', $manifest)
& $cargo.Source @args
$exitCode = $LASTEXITCODE
if ($exitCode -ne 0) {
    throw "cargo build failed with exit code $exitCode"
}

$sourceExe = Join-Path $ProjectRoot 'src-fastpath\target\release\vekai-embedding-fastpath.exe'
if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) {
    throw "Fast path build did not produce $sourceExe"
}

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$destinationExe = Join-Path $OutputDir 'vekai-embedding-fastpath.exe'
Copy-Item -LiteralPath $sourceExe -Destination $destinationExe -Force

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

Write-Host "[fastpath-build] Sidecar ready: $destinationExe"
