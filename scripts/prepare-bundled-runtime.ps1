param(
    [ValidateSet('auto', 'windows-x64', 'windows-arm64', 'ubuntu-x64', 'ubuntu-arm64', 'macos-x64', 'macos-arm64')]
    [string]$RuntimePlatform = 'auto',

    [ValidateSet('default', 'vulkan', 'cuda')]
    [string]$WindowsBackend = 'default',

    [string]$OutputDir = '',

    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$ScriptRoot = if ([string]::IsNullOrWhiteSpace($PSScriptRoot)) {
    Split-Path -Parent $MyInvocation.MyCommand.Path
} else {
    $PSScriptRoot
}
$ProjectRoot = Split-Path -Parent $ScriptRoot

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $ProjectRoot 'bundle-resources\downloads'
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
Get-ChildItem -LiteralPath $OutputDir -Force |
    Where-Object { $_.Name -ne '.gitkeep' } |
    Remove-Item -Recurse -Force

& (Join-Path $ScriptRoot 'download-assets.ps1') `
    -RuntimePlatform $RuntimePlatform `
    -WindowsBackend $WindowsBackend `
    -SkipModels `
    -DownloadDir $OutputDir `
    -Force:$Force

$runtimeArchives = Get-ChildItem -LiteralPath $OutputDir -File |
    Where-Object {
        ($_.Name -like 'llama-*-bin-*' -or $_.Name -like 'cudart-llama-bin-*') -and
        ($_.Name.EndsWith('.zip') -or $_.Name.EndsWith('.tar.gz'))
    }

$expectedArchiveCount = if ($WindowsBackend -eq 'cuda') { 2 } else { 1 }
if (($runtimeArchives | Measure-Object).Count -ne $expectedArchiveCount) {
    $names = ($runtimeArchives | Select-Object -ExpandProperty Name) -join ', '
    throw "Expected $expectedArchiveCount bundled runtime archive(s) in $OutputDir, found $($runtimeArchives.Count): $names"
}

Write-Host "[bundle-runtime] Prepared $($runtimeArchives.FullName -join ', ')"
