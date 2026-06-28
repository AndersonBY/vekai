param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    [Parameter(Mandatory = $true)]
    [string]$TargetName,

    [ValidateSet('', 'windows-x64', 'windows-arm64', 'ubuntu-x64', 'ubuntu-arm64', 'macos-x64', 'macos-arm64')]
    [string]$RuntimePlatform = ''
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$outputPath = Join-Path $root $OutputDir
New-Item -ItemType Directory -Force -Path $outputPath | Out-Null

$packageJson = Get-Content -Raw -LiteralPath (Join-Path $root "package.json") | ConvertFrom-Json
$version = [string]$packageJson.version

$bundleRoot = [IO.Path]::Combine($root, "src-tauri", "target")
if (-not (Test-Path -LiteralPath $bundleRoot)) {
    throw "Tauri target directory not found: $bundleRoot"
}

$bundlePatterns = @(
    "release/bundle/nsis/*.exe",
    "release/bundle/msi/*.msi",
    "release/bundle/dmg/*.dmg",
    "release/bundle/macos/*.app.tar.gz",
    "release/bundle/appimage/*.AppImage",
    "release/bundle/deb/*.deb",
    "release/bundle/rpm/*.rpm",
    "*/release/bundle/nsis/*.exe",
    "*/release/bundle/msi/*.msi",
    "*/release/bundle/dmg/*.dmg",
    "*/release/bundle/macos/*.app.tar.gz",
    "*/release/bundle/appimage/*.AppImage",
    "*/release/bundle/deb/*.deb",
    "*/release/bundle/rpm/*.rpm"
)

$bundleArtifacts = foreach ($pattern in $bundlePatterns) {
    Get-ChildItem -Path (Join-Path $bundleRoot $pattern) -File -ErrorAction SilentlyContinue
}

$bundleArtifacts = $bundleArtifacts | Where-Object {
    $_.Name -like "*$version*"
}
if (-not $bundleArtifacts) {
    throw "No Tauri bundle artifacts for version $version found under $bundleRoot"
}

$artifacts = @($bundleArtifacts)
$artifacts = $artifacts | Sort-Object FullName -Unique
if (-not $artifacts) {
    throw "No Tauri bundle artifacts found under $bundleRoot"
}

foreach ($artifact in $artifacts) {
    $extension = if ($artifact.Name.EndsWith(".app.tar.gz", [StringComparison]::OrdinalIgnoreCase)) {
        ".app.tar.gz"
    } elseif ($artifact.Name.EndsWith(".tar.gz", [StringComparison]::OrdinalIgnoreCase)) {
        ".tar.gz"
    } else {
        $artifact.Extension
    }
    $baseName = if ($extension -eq ".app.tar.gz" -or $extension -eq ".tar.gz") {
        $artifact.Name.Substring(0, $artifact.Name.Length - $extension.Length)
    } else {
        [IO.Path]::GetFileNameWithoutExtension($artifact.Name)
    }
    $safeBaseName = $baseName -replace "[^A-Za-z0-9._-]+", "-"
    $destination = Join-Path $outputPath "$safeBaseName-$TargetName$extension"
    Copy-Item -LiteralPath $artifact.FullName -Destination $destination -Force
    Write-Host "Collected $($artifact.FullName) -> $destination"
}
