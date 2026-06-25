param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    [Parameter(Mandatory = $true)]
    [string]$TargetName
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$outputPath = Join-Path $root $OutputDir
New-Item -ItemType Directory -Force -Path $outputPath | Out-Null

$bundleRoot = [IO.Path]::Combine($root, "src-tauri", "target")
if (-not (Test-Path -LiteralPath $bundleRoot)) {
    throw "Tauri target directory not found: $bundleRoot"
}

$patterns = @(
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

$artifacts = foreach ($pattern in $patterns) {
    Get-ChildItem -Path (Join-Path $bundleRoot $pattern) -File -ErrorAction SilentlyContinue
}

$artifacts = $artifacts | Sort-Object FullName -Unique
if (-not $artifacts) {
    throw "No Tauri bundle artifacts found under $bundleRoot"
}

foreach ($artifact in $artifacts) {
    $extension = if ($artifact.Name.EndsWith(".app.tar.gz", [StringComparison]::OrdinalIgnoreCase)) {
        ".app.tar.gz"
    } else {
        $artifact.Extension
    }
    $baseName = if ($extension -eq ".app.tar.gz") {
        $artifact.Name.Substring(0, $artifact.Name.Length - $extension.Length)
    } else {
        [IO.Path]::GetFileNameWithoutExtension($artifact.Name)
    }
    $safeBaseName = $baseName -replace "[^A-Za-z0-9._-]+", "-"
    $destination = Join-Path $outputPath "$safeBaseName-$TargetName$extension"
    Copy-Item -LiteralPath $artifact.FullName -Destination $destination -Force
    Write-Host "Collected $($artifact.FullName) -> $destination"
}
