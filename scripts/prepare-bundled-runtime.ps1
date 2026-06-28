param(
    [ValidateSet('auto', 'windows-x64', 'windows-arm64', 'ubuntu-x64', 'ubuntu-arm64', 'macos-x64', 'macos-arm64')]
    [string]$RuntimePlatform = 'auto',

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
    -SkipModels `
    -DownloadDir $OutputDir `
    -Force:$Force

$runtimeArchives = Get-ChildItem -LiteralPath $OutputDir -File |
    Where-Object { $_.Name -like 'llama-*-bin-*' -and ($_.Name.EndsWith('.zip') -or $_.Name.EndsWith('.tar.gz')) }

if (($runtimeArchives | Measure-Object).Count -ne 1) {
    $names = ($runtimeArchives | Select-Object -ExpandProperty Name) -join ', '
    throw "Expected exactly one bundled runtime archive in $OutputDir, found $($runtimeArchives.Count): $names"
}

Write-Host "[bundle-runtime] Prepared $($runtimeArchives[0].FullName)"
