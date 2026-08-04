[CmdletBinding()]
param(
    [string]$SourceDir = '',
    [string]$OutputRoot = ''
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not $IsWindows) {
    throw 'Reqbase AI assets can currently be built only on Windows.'
}
if ([string]::IsNullOrWhiteSpace($SourceDir)) {
    $SourceDir = Join-Path ([System.Environment]::GetFolderPath('LocalApplicationData')) 'Vekai\fastpath'
}
if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $OutputRoot = Join-Path (Split-Path -Parent $PSScriptRoot) 'artifacts'
}

$sourceRoot = [System.IO.Path]::GetFullPath($SourceDir)
$outputRootPath = [System.IO.Path]::GetFullPath($OutputRoot)
$sourceManifestPath = Join-Path $sourceRoot 'manifest.json'
if (-not (Test-Path -LiteralPath $sourceManifestPath -PathType Leaf)) {
    throw "Vekai fastpath manifest is missing: $sourceManifestPath"
}

$sourceManifest = Get-Content -Raw -LiteralPath $sourceManifestPath | ConvertFrom-Json
if ([int]$sourceManifest.schema_version -ne 2) {
    throw 'Only Vekai fastpath manifest schema version 2 is supported.'
}
if ([string]$sourceManifest.platform -ne 'windows-x64') {
    throw 'Only Vekai Windows x64 fastpath assets are supported.'
}

function Resolve-SafeSourceFile {
    param([Parameter(Mandatory = $true)][string]$RelativePath)

    if ([string]::IsNullOrWhiteSpace($RelativePath) -or
        [System.IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath -match '(^|[\\/])\.\.([\\/]|$)') {
        throw "Unsafe relative asset path: $RelativePath"
    }
    $normalized = $RelativePath.Replace('/', [System.IO.Path]::DirectorySeparatorChar)
    $resolved = [System.IO.Path]::GetFullPath((Join-Path $sourceRoot $normalized))
    $sourcePrefix = $sourceRoot.TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($sourcePrefix, [System.StringComparison]::OrdinalIgnoreCase) -or
        -not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
        throw "Asset file is missing or outside the source directory: $RelativePath"
    }
    return $resolved
}

function Assert-DeclaredFile {
    param([Parameter(Mandatory = $true)]$Declaration)

    $path = Resolve-SafeSourceFile -RelativePath ([string]$Declaration.path)
    $item = Get-Item -LiteralPath $path
    if ([long]$item.Length -ne [long]$Declaration.size) {
        throw "Asset size does not match the Vekai manifest: $($Declaration.path)"
    }
    $actualHash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne ([string]$Declaration.sha256).ToLowerInvariant()) {
        throw "Asset hash does not match the Vekai manifest: $($Declaration.path)"
    }
    return [pscustomobject]@{
        RelativePath = ([string]$Declaration.path).Replace('\', '/')
        SourcePath = $path
        Size = [long]$item.Length
        Sha256 = $actualHash
    }
}

$modelGraphs = @(Get-ChildItem -LiteralPath (Join-Path $sourceRoot 'model') -File -Filter '*.onnx')
if ($modelGraphs.Count -ne 1) {
    throw "Expected exactly one ONNX model graph, found $($modelGraphs.Count)."
}
$modelGraph = $modelGraphs[0]
$modelGraphRelative = "model/$($modelGraph.Name)"
$declaredFiles = @(
    Assert-DeclaredFile -Declaration $sourceManifest.weights
    Assert-DeclaredFile -Declaration $sourceManifest.tokenizer
    foreach ($runtimeFile in @($sourceManifest.runtime_files)) {
        Assert-DeclaredFile -Declaration $runtimeFile
    }
)
$graphHash = (Get-FileHash -LiteralPath $modelGraph.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
$allFiles = @(
    [pscustomobject]@{
        RelativePath = $modelGraphRelative
        SourcePath = $modelGraph.FullName
        Size = [long]$modelGraph.Length
        Sha256 = $graphHash
    }
    $declaredFiles
)

$duplicatePath = $allFiles |
    Group-Object { $_.RelativePath.ToLowerInvariant() } |
    Where-Object Count -gt 1 |
    Select-Object -First 1
if ($duplicatePath) {
    throw "Duplicate asset path in source manifest: $($duplicatePath.Name)"
}

function Get-ModelSetHash {
    param([Parameter(Mandatory = $true)][object[]]$Files)

    $ordered = @($Files | Sort-Object -Property @{ Expression = { [string]$_.SourcePath } })
    $setHasher = [System.Security.Cryptography.IncrementalHash]::CreateHash(
        [System.Security.Cryptography.HashAlgorithmName]::SHA256
    )
    try {
        foreach ($file in $ordered) {
            $nameBytes = [System.Text.Encoding]::UTF8.GetBytes(
                [System.IO.Path]::GetFileName([string]$file.SourcePath)
            )
            $lengthBytes = [System.BitConverter]::GetBytes([uint64]$nameBytes.Length)
            if ([System.BitConverter]::IsLittleEndian) {
                [Array]::Reverse($lengthBytes)
            }
            $hashBytes = [System.Text.Encoding]::ASCII.GetBytes(
                ([string]$file.Sha256).ToLowerInvariant()
            )
            $setHasher.AppendData($lengthBytes)
            $setHasher.AppendData($nameBytes)
            $setHasher.AppendData($hashBytes)
        }
        return [System.Convert]::ToHexString($setHasher.GetHashAndReset()).ToLowerInvariant()
    } finally {
        $setHasher.Dispose()
    }
}

$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$packageName = "reqbase-ai-assets-windows-x64-$timestamp"
$outputDir = Join-Path $outputRootPath $packageName
$zipPath = "$outputDir.zip"
$checksumPath = "$zipPath.sha256"
foreach ($path in @($outputDir, $zipPath, $checksumPath)) {
    if (Test-Path -LiteralPath $path) {
        throw "Output already exists and will not be overwritten: $path"
    }
}

New-Item -ItemType Directory -Path $outputDir | Out-Null
foreach ($file in $allFiles) {
    $destination = Join-Path $outputDir $file.RelativePath.Replace(
        '/',
        [System.IO.Path]::DirectorySeparatorChar
    )
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath $file.SourcePath -Destination $destination
}

$weightsRelative = ([string]$sourceManifest.weights.path).Replace('\', '/')
$tokenizerRelative = ([string]$sourceManifest.tokenizer.path).Replace('\', '/')
$runtimePaths = @($sourceManifest.runtime_files | ForEach-Object {
    ([string]$_.path).Replace('\', '/')
})
$ortLibrary = $runtimePaths | Where-Object {
    [System.IO.Path]::GetFileName($_) -eq 'onnxruntime.dll'
} | Select-Object -First 1
if (-not $ortLibrary) {
    throw 'onnxruntime.dll is not declared in the Vekai runtime files.'
}

$manifestFiles = @($allFiles | Sort-Object RelativePath | ForEach-Object {
    [ordered]@{
        path = $_.RelativePath
        size_bytes = [long]$_.Size
        sha256 = $_.Sha256
    }
})
$modelFileEntries = @($allFiles | Where-Object {
    $_.RelativePath -in @($modelGraphRelative, $weightsRelative)
})
$manifest = [ordered]@{
    format = 'reqbase.ai-assets.v1'
    model_id = "Qwen3-Embedding-0.6B-ONNX@$($sourceManifest.model_revision)"
    model_graph = $modelGraphRelative
    model_files = @($modelGraphRelative, $weightsRelative)
    tokenizer = $tokenizerRelative
    ort_library = $ortLibrary
    files = $manifestFiles
    model_hash = Get-ModelSetHash -Files $modelFileEntries
    max_sequence_length = 8192
    document_template_version = 'req-document.v1'
    query_template_version = 'req-query.v1'
}
$manifestJson = $manifest | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText(
    (Join-Path $outputDir 'manifest.json'),
    $manifestJson,
    [System.Text.UTF8Encoding]::new($false)
)

Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $outputDir,
    $zipPath,
    [System.IO.Compression.CompressionLevel]::Optimal,
    $false
)
$zipHash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
[System.IO.File]::WriteAllText(
    $checksumPath,
    "$zipHash  $([System.IO.Path]::GetFileName($zipPath))`n",
    [System.Text.UTF8Encoding]::new($false)
)

Write-Host "Reqbase AI assets: $outputDir"
Write-Host "ZIP: $zipPath"
Write-Host "Checksum: $checksumPath"
