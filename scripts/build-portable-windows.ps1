[CmdletBinding()]
param(
    [string]$OutputRoot = '',
    [ValidateSet('cpu', 'cuda')]
    [string]$WindowsBackend = 'cpu',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$ScriptRoot = if ([string]::IsNullOrWhiteSpace($PSScriptRoot)) {
    Split-Path -Parent $MyInvocation.MyCommand.Path
} else {
    $PSScriptRoot
}
$ProjectRoot = Split-Path -Parent $ScriptRoot

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $OutputRoot = Join-Path $ProjectRoot 'release'
}

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if (-not $IsWindows -or $architecture -ne 'X64') {
    throw 'The portable Windows package can only be assembled on Windows x64.'
}

$tauriConfigPath = Join-Path $ProjectRoot 'src-tauri\tauri.conf.json'
$assetsPath = Join-Path $ScriptRoot 'assets.json'
$tauriConfig = Get-Content -Raw -LiteralPath $tauriConfigPath | ConvertFrom-Json
$assets = Get-Content -Raw -LiteralPath $assetsPath | ConvertFrom-Json
$version = [string]$tauriConfig.version
$packageName = "Vekai-$version-windows-x64-$WindowsBackend-portable"
$releaseDir = Join-Path $OutputRoot "Vekai $version"
$stagingDir = Join-Path $releaseDir $packageName
$zipPath = Join-Path $releaseDir "$packageName.zip"

function Get-NormalizedFullPath {
    param([Parameter(Mandatory = $true)][string]$Path)
    [System.IO.Path]::GetFullPath($Path).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
}

function Assert-ChildPath {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $normalizedRoot = Get-NormalizedFullPath -Path $Root
    $normalizedPath = Get-NormalizedFullPath -Path $Path
    $prefix = "$normalizedRoot$([System.IO.Path]::DirectorySeparatorChar)"
    if (-not $normalizedPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify path outside output root: $normalizedPath"
    }
}

function Assert-File {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required portable package file is missing: $Path"
    }
}

function Assert-Hash {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][long]$ExpectedSize,
        [Parameter(Mandatory = $true)][string]$ExpectedSha256
    )

    $item = Get-Item -LiteralPath $Path
    if ($item.Length -ne $ExpectedSize) {
        throw "Size mismatch for $Path. Expected $ExpectedSize, got $($item.Length)."
    }

    $actualHash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $ExpectedSha256.ToLowerInvariant()) {
        throw "SHA256 mismatch for $Path. Expected $ExpectedSha256, got $actualHash."
    }
}

function Get-VerifiedAsset {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][long]$ExpectedSize,
        [Parameter(Mandatory = $true)][string]$ExpectedSha256
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $parent = Split-Path -Parent $Path
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
        $temporaryPath = "$Path.download-$([guid]::NewGuid().ToString('N'))"
        try {
            Invoke-WebRequest -Uri $Url -OutFile $temporaryPath
            Assert-Hash `
                -Path $temporaryPath `
                -ExpectedSize $ExpectedSize `
                -ExpectedSha256 $ExpectedSha256
            Move-Item -LiteralPath $temporaryPath -Destination $Path
        } finally {
            if (Test-Path -LiteralPath $temporaryPath) {
                Remove-Item -LiteralPath $temporaryPath -Force
            }
        }
    }

    Assert-Hash `
        -Path $Path `
        -ExpectedSize $ExpectedSize `
        -ExpectedSha256 $ExpectedSha256
}

Assert-ChildPath -Root $OutputRoot -Path $stagingDir
Assert-ChildPath -Root $OutputRoot -Path $zipPath
New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null

foreach ($target in @($stagingDir, $zipPath)) {
    if (Test-Path -LiteralPath $target) {
        if (-not $Force) {
            throw "Portable package target already exists: $target. Use -Force to replace it."
        }
        Remove-Item -LiteralPath $target -Recurse -Force
    }
}

$exeSource = Join-Path $ProjectRoot 'src-tauri\target\release\vekai.exe'
$fastpathSource = Join-Path $ProjectRoot 'src-tauri\resources\fastpath'
$runtimeSourceDir = Join-Path $ProjectRoot 'bundle-resources\downloads'
$vcRedist = $assets.windowsPrerequisites.vcRedistX64
$vcRedistSource = Join-Path (Join-Path $ProjectRoot 'bundle-resources\redist') ([string]$vcRedist.fileName)
Assert-File -Path $exeSource
Assert-File -Path (Join-Path $fastpathSource 'vekai-embedding-fastpath.exe')
Assert-File -Path (Join-Path $fastpathSource 'qwen3-embedding-0.6b-fp16.onnx')
Get-VerifiedAsset `
    -Path $vcRedistSource `
    -Url ([string]$vcRedist.url) `
    -ExpectedSize ([long]$vcRedist.sizeBytes) `
    -ExpectedSha256 ([string]$vcRedist.sha256)
$vcRedistFileVersion = (Get-Item -LiteralPath $vcRedistSource).VersionInfo.FileVersion
if ($vcRedistFileVersion -ne [string]$vcRedist.version) {
    throw "VC++ Redistributable version mismatch. Expected $($vcRedist.version), got $vcRedistFileVersion."
}

$runtimeFiles = @(Get-ChildItem -LiteralPath $runtimeSourceDir -File)
if ($WindowsBackend -eq 'cuda') {
    $mainArchives = @(
        $runtimeFiles | Where-Object { $_.Name -match '^llama-b[0-9]+-bin-win-cuda-12\.4-x64\.zip$' }
    )
    $companionArchives = @(
        $runtimeFiles | Where-Object { $_.Name -eq 'cudart-llama-bin-win-cuda-12.4-x64.zip' }
    )
    if ($mainArchives.Count -ne 1 -or $companionArchives.Count -ne 1) {
        throw "Expected one CUDA 12.4 runtime and one cudart companion, found $($mainArchives.Count) main and $($companionArchives.Count) companion archive(s)."
    }
    $runtimeArchives = @($mainArchives[0], $companionArchives[0])
} else {
    $runtimeArchives = @(
        $runtimeFiles | Where-Object { $_.Name -match '^llama-b[0-9]+-bin-win-cpu-x64\.zip$' }
    )
    if ($runtimeArchives.Count -ne 1) {
        throw "Expected exactly one Windows x64 CPU runtime archive, found $($runtimeArchives.Count)."
    }
}

foreach ($model in $assets.models) {
    $modelPath = Join-Path (Join-Path $ProjectRoot 'models') ([string]$model.fileName)
    Assert-File -Path $modelPath
    Assert-Hash `
        -Path $modelPath `
        -ExpectedSize ([long]$model.sizeBytes) `
        -ExpectedSha256 ([string]$model.sha256)
}

New-Item -ItemType Directory -Path $stagingDir | Out-Null
New-Item -ItemType Directory -Path (Join-Path $stagingDir 'models') | Out-Null
New-Item -ItemType Directory -Path (Join-Path $stagingDir 'downloads') | Out-Null
New-Item -ItemType Directory -Path (Join-Path $stagingDir 'fastpath') | Out-Null
New-Item -ItemType Directory -Path (Join-Path $stagingDir 'redist') | Out-Null

Copy-Item -LiteralPath $exeSource -Destination (Join-Path $stagingDir 'Vekai.exe')
foreach ($runtimeArchive in $runtimeArchives) {
    Copy-Item -LiteralPath $runtimeArchive.FullName -Destination (Join-Path $stagingDir 'downloads')
}
Copy-Item -LiteralPath (Join-Path $fastpathSource 'vekai-embedding-fastpath.exe') `
    -Destination (Join-Path $stagingDir 'fastpath')
Copy-Item -LiteralPath (Join-Path $fastpathSource 'qwen3-embedding-0.6b-fp16.onnx') `
    -Destination (Join-Path $stagingDir 'fastpath')
Copy-Item -LiteralPath $vcRedistSource -Destination (Join-Path $stagingDir 'redist')
([string]$vcRedist.version) |
    Set-Content -LiteralPath (Join-Path $stagingDir 'redist\version.txt') -Encoding ascii -NoNewline

foreach ($model in $assets.models) {
    $modelPath = Join-Path (Join-Path $ProjectRoot 'models') ([string]$model.fileName)
    Copy-Item -LiteralPath $modelPath -Destination (Join-Path $stagingDir 'models')
}

$rerankModel = $assets.models | Where-Object { $_.service -eq 'rerank' } | Select-Object -First 1
$embeddingModel = $assets.models | Where-Object { $_.service -eq 'embedding' } | Select-Object -First 1
if (-not $rerankModel -or -not $embeddingModel) {
    throw 'assets.json must define one rerank model and one embedding model.'
}

$portableConfig = [ordered]@{
    host = '127.0.0.1'
    rerank = [ordered]@{
        enabled = $true
        port = 8012
        model_path = "models/$([string]$rerankModel.fileName)"
        protocol = 'llama_rerank'
        parallel_slots = 0
        batch_size = 0
        ubatch_size = 0
    }
    embedding = [ordered]@{
        enabled = $true
        port = 8013
        model_path = "models/$([string]$embeddingModel.fileName)"
        protocol = 'openai_embeddings'
        parallel_slots = 0
        batch_size = 0
        ubatch_size = 0
    }
    auto_start_server = $true
    restart_on_crash = $true
    gpu_layers = 'auto'
    threads = 0
    parallel_slots = 0
    batch_size = 0
    ubatch_size = 0
    http_threads = 0
    log_retention_days = 7
}
$portableConfig |
    ConvertTo-Json -Depth 4 |
    Set-Content -LiteralPath (Join-Path $stagingDir 'config.json') -Encoding utf8

$backendLabel = if ($WindowsBackend -eq 'cuda') { 'CUDA 12.4' } else { 'CPU' }
$readme = @"
Vekai Windows x64 $backendLabel 便携版

1. 请先完整解压缩，再双击 Vekai.exe。
2. 程序会自动启动本机服务：
   - Rerank: http://127.0.0.1:8012/v1/rerank
   - Embedding: http://127.0.0.1:8013/v1/embeddings
3. 模型和配置已随包提供。移动整个解压目录后仍可使用。
4. 首次启动会把 llama.cpp runtime 解压到 %LOCALAPPDATA%\Vekai\runtimes。
5. Windows 防火墙若询问网络权限，仅本机调用时可以取消公用网络访问。
6. 当前运行后端：$backendLabel。
7. 若 Microsoft Visual C++ x64 运行库缺失或过旧，Vekai 会自动启动包内微软官方安装程序；安装完成后请重新打开 Vekai。

运行日志位于 %LOCALAPPDATA%\Vekai\logs。
"@
$readme | Set-Content -LiteralPath (Join-Path $stagingDir '使用说明.txt') -Encoding utf8

$checksumLines = Get-ChildItem -LiteralPath $stagingDir -Recurse -File |
    Sort-Object FullName |
    ForEach-Object {
        $relativePath = [System.IO.Path]::GetRelativePath($stagingDir, $_.FullName)
        $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash *$relativePath"
    }
$checksumLines | Set-Content -LiteralPath (Join-Path $stagingDir 'SHA256SUMS.txt') -Encoding utf8

Compress-Archive -LiteralPath $stagingDir -DestinationPath $zipPath -CompressionLevel Optimal

$zipItem = Get-Item -LiteralPath $zipPath
$zipHash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "[portable] Directory: $stagingDir"
Write-Host "[portable] ZIP: $zipPath"
Write-Host "[portable] ZIP size: $($zipItem.Length)"
Write-Host "[portable] ZIP SHA256: $zipHash"
