[CmdletBinding()]
param(
    [string]$InstallDir = '',
    [string]$CacheDir = '',
    [string]$ModelSourceDir = '',
    [switch]$Force,
    [switch]$KeepDownloadCache
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not $IsWindows -or [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
    throw 'The embedding fast path runtime is currently supported only on Windows x64.'
}

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path ([System.Environment]::GetFolderPath('LocalApplicationData')) 'Vekai\fastpath'
}
if ([string]::IsNullOrWhiteSpace($CacheDir)) {
    $CacheDir = Join-Path ([System.Environment]::GetFolderPath('LocalApplicationData')) 'Vekai\downloads\fastpath-cache'
}

$assets = @(
    [pscustomobject]@{
        Id = 'weights'
        FileName = 'model_fp16.onnx_data'
        Url = 'https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX/resolve/c25a394dd583836952667c12f008335071b3f43d/onnx/model_fp16.onnx_data'
        Size = 1199927296L
        Sha256 = '06fbfded30166e3a33fe5252d7002ed1c370a4ec1bf45aba778cdf6cf7d31439'
    },
    [pscustomobject]@{
        Id = 'tokenizer'
        FileName = 'tokenizer.json'
        Url = 'https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX/resolve/c25a394dd583836952667c12f008335071b3f43d/tokenizer.json'
        Size = 11423705L
        Sha256 = 'def76fb086971c7867b829c23a26261e38d9d74e02139253b38aeb9df8b4b50a'
    },
    [pscustomobject]@{
        Id = 'ort'
        FileName = 'onnxruntime_gpu-1.23.0-cp312-cp312-win_amd64.whl'
        Url = 'https://files.pythonhosted.org/packages/e8/34/1eb9b6ea10045fe20fb2835dcab42472537c9bdbfb390f8a8addaf20a553/onnxruntime_gpu-1.23.0-cp312-cp312-win_amd64.whl'
        Size = 285364792L
        Sha256 = '17575b3f7b9f4d1867e3fbe6f751669cebfd369b47425ff7fa8813587a5e8c28'
    },
    [pscustomobject]@{
        Id = 'cuda-runtime'
        FileName = 'nvidia_cuda_runtime_cu12-12.1.105-py3-none-win_amd64.whl'
        Url = 'https://files.pythonhosted.org/packages/9f/e2/7a2b4b5064af56ea8ea2d8b2776c0f2960d95c88716138806121ae52a9c9/nvidia_cuda_runtime_cu12-12.1.105-py3-none-win_amd64.whl'
        Size = 821226L
        Sha256 = 'dfb46ef84d73fababab44cf03e3b83f80700d27ca300e537f85f636fac474344'
    },
    [pscustomobject]@{
        Id = 'cublas'
        FileName = 'nvidia_cublas_cu12-12.1.3.1-py3-none-win_amd64.whl'
        Url = 'https://files.pythonhosted.org/packages/c5/ef/32a375b74bea706c93deea5613552f7c9104f961b21df423f5887eca713b/nvidia_cublas_cu12-12.1.3.1-py3-none-win_amd64.whl'
        Size = 439918445L
        Sha256 = '2b964d60e8cf11b5e1073d179d85fa340c120e99b3067558f3cf98dd69d02906'
    },
    [pscustomobject]@{
        Id = 'cufft'
        FileName = 'nvidia_cufft_cu12-11.0.2.54-py3-none-win_amd64.whl'
        Url = 'https://files.pythonhosted.org/packages/f7/57/7927a3aa0e19927dfed30256d1c854caf991655d847a4e7c01fe87e3d4ac/nvidia_cufft_cu12-11.0.2.54-py3-none-win_amd64.whl'
        Size = 121344196L
        Sha256 = 'd9ac353f78ff89951da4af698f80870b1534ed69993f10a4cf1d96f21357e253'
    },
    [pscustomobject]@{
        Id = 'cudnn'
        FileName = 'nvidia_cudnn_cu12-9.1.0.70-py3-none-win_amd64.whl'
        Url = 'https://files.pythonhosted.org/packages/3f/d0/f90ee6956a628f9f04bf467932c0a25e5a7e706a684b896593c06c82f460/nvidia_cudnn_cu12-9.1.0.70-py3-none-win_amd64.whl'
        Size = 679925892L
        Sha256 = '6278562929433d68365a07a4a1546c237ba2849852c0d4b2262a486e805b977a'
    }
)

function Write-Step {
    param([string]$Message)
    Write-Host "[fastpath] $Message"
}

function Test-Asset {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][long]$Size,
        [Parameter(Mandatory = $true)][string]$Sha256
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $false
    }
    if ((Get-Item -LiteralPath $Path).Length -ne $Size) {
        return $false
    }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -eq $Sha256
}

function Invoke-ResumableDownload {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$OutFile
    )
    New-Item -ItemType Directory -Path (Split-Path -Parent $OutFile) -Force | Out-Null
    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curl) {
        $args = @(
            '--fail', '--location', '--continue-at', '-', '--retry', '8',
            '--retry-delay', '3', '--connect-timeout', '30', '--output', $OutFile, $Url
        )
        & $curl.Source @args
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            throw "curl.exe failed with exit code $exitCode while downloading $Url"
        }
    } else {
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -Headers @{ 'User-Agent' = 'VekaiAssetDownloader' }
    }
}

function Ensure-Download {
    param([Parameter(Mandatory = $true)]$Asset)
    if (-not [string]::IsNullOrWhiteSpace($ModelSourceDir) -and $Asset.Id -in @('weights', 'tokenizer')) {
        $sourcePath = if ($Asset.Id -eq 'weights') {
            Join-Path $ModelSourceDir 'onnx\model_fp16.onnx_data'
        } else {
            Join-Path $ModelSourceDir 'tokenizer.json'
        }
        if (Test-Asset -Path $sourcePath -Size $Asset.Size -Sha256 $Asset.Sha256) {
            Write-Step "Using verified local $($Asset.FileName)"
            return $sourcePath
        }
        throw "Local model source failed verification: $sourcePath"
    }
    $path = Join-Path $CacheDir $Asset.FileName
    if (-not $Force -and (Test-Asset -Path $path -Size $Asset.Size -Sha256 $Asset.Sha256)) {
        Write-Step "Verified cached $($Asset.FileName)"
        return $path
    }
    if ($Force -and (Test-Path -LiteralPath $path)) {
        Remove-Item -LiteralPath $path -Force
    }
    Write-Step "Downloading $($Asset.FileName)"
    Invoke-ResumableDownload -Url $Asset.Url -OutFile $path
    if (-not (Test-Asset -Path $path -Size $Asset.Size -Sha256 $Asset.Sha256)) {
        throw "Download verification failed: $path"
    }
    return $path
}

function Copy-WheelEntry {
    param(
        [Parameter(Mandatory = $true)][string]$WheelPath,
        [Parameter(Mandatory = $true)][string]$EntryPath,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    $archive = [System.IO.Compression.ZipFile]::OpenRead($WheelPath)
    try {
        $entry = $archive.GetEntry($EntryPath)
        if (-not $entry) {
            throw "Wheel entry $EntryPath is missing from $WheelPath"
        }
        New-Item -ItemType Directory -Path (Split-Path -Parent $Destination) -Force | Out-Null
        $source = $entry.Open()
        try {
            $target = [System.IO.File]::Open($Destination, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
            try {
                $source.CopyTo($target)
            } finally {
                $target.Dispose()
            }
        } finally {
            $source.Dispose()
        }
    } finally {
        $archive.Dispose()
    }
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
New-Item -ItemType Directory -Path $InstallDir,$CacheDir -Force | Out-Null
$manifestPath = Join-Path $InstallDir 'manifest.json'
$previousManifest = if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
    try { Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json } catch { $null }
} else {
    $null
}
if (Test-Path -LiteralPath $manifestPath) {
    Remove-Item -LiteralPath $manifestPath -Force
}

$downloads = @{}
foreach ($asset in $assets | Where-Object { $_.Id -in @('weights', 'tokenizer') }) {
    $downloads[$asset.Id] = Ensure-Download -Asset $asset
}

$modelDir = Join-Path $InstallDir 'model'
$runtimeDir = Join-Path $InstallDir 'runtime'
New-Item -ItemType Directory -Path $modelDir,$runtimeDir -Force | Out-Null

foreach ($id in @('weights', 'tokenizer')) {
    $asset = $assets | Where-Object Id -eq $id
    $destination = Join-Path $modelDir $asset.FileName
    if ($Force -or -not (Test-Asset -Path $destination -Size $asset.Size -Sha256 $asset.Sha256)) {
        Copy-Item -LiteralPath $downloads[$id] -Destination $destination -Force
    }
}

$runtimeEntries = @(
    @{ Asset = 'ort'; Entry = 'onnxruntime/capi/onnxruntime.dll'; Name = 'onnxruntime.dll' },
    @{ Asset = 'ort'; Entry = 'onnxruntime/capi/onnxruntime_providers_shared.dll'; Name = 'onnxruntime_providers_shared.dll' },
    @{ Asset = 'ort'; Entry = 'onnxruntime/capi/onnxruntime_providers_cuda.dll'; Name = 'onnxruntime_providers_cuda.dll' },
    @{ Asset = 'cuda-runtime'; Entry = 'nvidia/cuda_runtime/bin/cudart64_12.dll'; Name = 'cudart64_12.dll' },
    @{ Asset = 'cublas'; Entry = 'nvidia/cublas/bin/cublas64_12.dll'; Name = 'cublas64_12.dll' },
    @{ Asset = 'cublas'; Entry = 'nvidia/cublas/bin/cublasLt64_12.dll'; Name = 'cublasLt64_12.dll' },
    @{ Asset = 'cufft'; Entry = 'nvidia/cufft/bin/cufft64_11.dll'; Name = 'cufft64_11.dll' },
    @{ Asset = 'cudnn'; Entry = 'nvidia/cudnn/bin/cudnn64_9.dll'; Name = 'cudnn64_9.dll' },
    @{ Asset = 'cudnn'; Entry = 'nvidia/cudnn/bin/cudnn_graph64_9.dll'; Name = 'cudnn_graph64_9.dll' },
    @{ Asset = 'cudnn'; Entry = 'nvidia/cudnn/bin/cudnn_ops64_9.dll'; Name = 'cudnn_ops64_9.dll' }
)

foreach ($entry in $runtimeEntries) {
    $destination = Join-Path $runtimeDir $entry.Name
    $previousFile = if ($previousManifest) {
        $previousManifest.runtime_files | Where-Object path -eq "runtime/$($entry.Name)" | Select-Object -First 1
    } else {
        $null
    }
    $isValid = $previousFile -and (Test-Asset -Path $destination -Size ([long]$previousFile.size) -Sha256 ([string]$previousFile.sha256))
    if ($Force -or -not $isValid) {
        if (-not $downloads.ContainsKey($entry.Asset)) {
            $asset = $assets | Where-Object Id -eq $entry.Asset
            $downloads[$entry.Asset] = Ensure-Download -Asset $asset
        }
        Write-Step "Extracting $($entry.Name)"
        Copy-WheelEntry -WheelPath $downloads[$entry.Asset] -EntryPath $entry.Entry -Destination $destination
    }
}

$runtimeFiles = foreach ($entry in $runtimeEntries) {
    $path = Join-Path $runtimeDir $entry.Name
    $item = Get-Item -LiteralPath $path
    [ordered]@{
        path = "runtime/$($entry.Name)"
        size = [long]$item.Length
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

$weights = $assets | Where-Object Id -eq 'weights'
$tokenizer = $assets | Where-Object Id -eq 'tokenizer'
$manifest = [ordered]@{
    schema_version = 1
    model_revision = 'c25a394dd583836952667c12f008335071b3f43d'
    onnxruntime_version = '1.23.0'
    cuda_runtime_version = '12.1.105'
    cublas_version = '12.1.3.1'
    cufft_version = '11.0.2.54'
    cudnn_version = '9.1.0.70'
    weights = [ordered]@{
        path = 'model/model_fp16.onnx_data'
        size = [long]$weights.Size
        sha256 = $weights.Sha256
    }
    tokenizer = [ordered]@{
        path = 'model/tokenizer.json'
        size = [long]$tokenizer.Size
        sha256 = $tokenizer.Sha256
    }
    runtime_files = @($runtimeFiles)
}

$json = $manifest | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText($manifestPath, $json, [System.Text.UTF8Encoding]::new($false))
Write-Step "Fast path assets ready: $InstallDir"

if (-not $KeepDownloadCache) {
    foreach ($asset in $assets) {
        $path = Join-Path $CacheDir $asset.FileName
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
        }
    }
}
