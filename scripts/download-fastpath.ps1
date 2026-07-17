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

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($architecture -ne 'X64' -or (-not $IsWindows -and -not $IsLinux)) {
    throw 'The embedding fast path runtime is supported only on Windows x64 and Linux x64.'
}
$fastPathPlatform = if ($IsWindows) { 'windows-x64' } else { 'linux-x64' }

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $dataRoot = if ($IsWindows) {
        [System.Environment]::GetFolderPath('LocalApplicationData')
    } elseif (-not [string]::IsNullOrWhiteSpace($env:XDG_DATA_HOME)) {
        $env:XDG_DATA_HOME
    } else {
        Join-Path $HOME '.local/share'
    }
    $InstallDir = Join-Path (Join-Path $dataRoot 'Vekai') 'fastpath'
}
if ([string]::IsNullOrWhiteSpace($CacheDir)) {
    $CacheDir = Join-Path (Join-Path (Split-Path -Parent $InstallDir) 'downloads') 'fastpath-cache'
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
    }
)

$platformAssets = if ($fastPathPlatform -eq 'windows-x64') {
    @(
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
} else {
    @(
        [pscustomobject]@{
            Id = 'ort'
            FileName = 'onnxruntime-linux-x64-gpu-1.23.0.tgz'
            Url = 'https://github.com/microsoft/onnxruntime/releases/download/v1.23.0/onnxruntime-linux-x64-gpu-1.23.0.tgz'
            Size = 240863500L
            Sha256 = 'fc5d0e2dbdb893de11758da83523169761a70562dc0f6991f2cdb614f6a62c3d'
        },
        [pscustomobject]@{
            Id = 'cuda-runtime'
            FileName = 'nvidia_cuda_runtime_cu12-12.1.105-py3-none-manylinux1_x86_64.whl'
            Url = 'https://files.pythonhosted.org/packages/eb/d5/c68b1d2cdfcc59e72e8a5949a37ddb22ae6cade80cd4a57a84d4c8b55472/nvidia_cuda_runtime_cu12-12.1.105-py3-none-manylinux1_x86_64.whl'
            Size = 823596L
            Sha256 = '6e258468ddf5796e25f1dc591a31029fa317d97a0a94ed93468fc86301d61e40'
        },
        [pscustomobject]@{
            Id = 'cublas'
            FileName = 'nvidia_cublas_cu12-12.1.3.1-py3-none-manylinux1_x86_64.whl'
            Url = 'https://files.pythonhosted.org/packages/37/6d/121efd7382d5b0284239f4ab1fc1590d86d34ed4a4a2fdb13b30ca8e5740/nvidia_cublas_cu12-12.1.3.1-py3-none-manylinux1_x86_64.whl'
            Size = 410594774L
            Sha256 = 'ee53ccca76a6fc08fb9701aa95b6ceb242cdaab118c3bb152af4e579af792728'
        },
        [pscustomobject]@{
            Id = 'cufft'
            FileName = 'nvidia_cufft_cu12-11.0.2.54-py3-none-manylinux1_x86_64.whl'
            Url = 'https://files.pythonhosted.org/packages/86/94/eb540db023ce1d162e7bea9f8f5aa781d57c65aed513c33ee9a5123ead4d/nvidia_cufft_cu12-11.0.2.54-py3-none-manylinux1_x86_64.whl'
            Size = 121635161L
            Sha256 = '794e3948a1aa71fd817c3775866943936774d1c14e7628c74f6f7417224cdf56'
        },
        [pscustomobject]@{
            Id = 'curand'
            FileName = 'nvidia_curand_cu12-10.3.2.106-py3-none-manylinux1_x86_64.whl'
            Url = 'https://files.pythonhosted.org/packages/44/31/4890b1c9abc496303412947fc7dcea3d14861720642b49e8ceed89636705/nvidia_curand_cu12-10.3.2.106-py3-none-manylinux1_x86_64.whl'
            Size = 56467784L
            Sha256 = '9d264c5036dde4e64f1de8c50ae753237c12e0b1348738169cd0f8a536c0e1e0'
        },
        [pscustomobject]@{
            Id = 'cudnn'
            FileName = 'nvidia_cudnn_cu12-9.1.0.70-py3-none-manylinux2014_x86_64.whl'
            Url = 'https://files.pythonhosted.org/packages/9f/fd/713452cd72343f682b1c7b9321e23829f00b842ceaedcda96e742ea0b0b3/nvidia_cudnn_cu12-9.1.0.70-py3-none-manylinux2014_x86_64.whl'
            Size = 664752741L
            Sha256 = '165764f44ef8c61fcdfdfdbe769d687e06374059fbb388b6c89ecb0e28793a6f'
        }
    )
}
$assets += $platformAssets

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
    $curlName = if ($IsWindows) { 'curl.exe' } else { 'curl' }
    $curl = Get-Command $curlName -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($curl) {
        $args = @(
            '--fail', '--location', '--continue-at', '-', '--retry', '8',
            '--retry-delay', '3', '--connect-timeout', '30', '--output', $OutFile, $Url
        )
        & $curl.Source @args
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            throw "$curlName failed with exit code $exitCode while downloading $Url"
        }
    } else {
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -Headers @{ 'User-Agent' = 'VekaiAssetDownloader' }
    }
}

function Ensure-Download {
    param([Parameter(Mandatory = $true)]$Asset)
    if (-not [string]::IsNullOrWhiteSpace($ModelSourceDir) -and $Asset.Id -in @('weights', 'tokenizer')) {
        $sourcePath = if ($Asset.Id -eq 'weights') {
            Join-Path (Join-Path $ModelSourceDir 'onnx') 'model_fp16.onnx_data'
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

function Copy-ArchiveEntry {
    param(
        [Parameter(Mandatory = $true)][string]$ArchivePath,
        [Parameter(Mandatory = $true)][string]$EntryPath,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    if ([System.IO.Path]::GetExtension($ArchivePath) -eq '.tgz') {
        $tar = Get-Command tar -CommandType Application -ErrorAction Stop | Select-Object -First 1
        $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "vekai-fastpath-$([System.Guid]::NewGuid().ToString('N'))"
        New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
        try {
            $tarArgs = @('--extract', '--gzip', '--file', $ArchivePath, '--directory', $tempRoot, '--', $EntryPath)
            & $tar.Source @tarArgs
            $exitCode = $LASTEXITCODE
            if ($exitCode -ne 0) {
                throw "tar failed with exit code $exitCode while extracting $EntryPath"
            }
            $sourcePath = Join-Path $tempRoot $EntryPath
            if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
                throw "Archive entry $EntryPath is missing from $ArchivePath"
            }
            New-Item -ItemType Directory -Path (Split-Path -Parent $Destination) -Force | Out-Null
            Copy-Item -LiteralPath $sourcePath -Destination $Destination -Force
        } finally {
            Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
        return
    }

    $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        $entry = $archive.GetEntry($EntryPath)
        if (-not $entry) {
            throw "Archive entry $EntryPath is missing from $ArchivePath"
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

$runtimeEntries = if ($fastPathPlatform -eq 'windows-x64') {
    @(
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
} else {
    @(
        @{ Asset = 'ort'; Entry = 'onnxruntime-linux-x64-gpu-1.23.0/lib/libonnxruntime.so.1.23.0'; Name = 'libonnxruntime.so.1.23.0' },
        @{ Asset = 'ort'; Entry = 'onnxruntime-linux-x64-gpu-1.23.0/lib/libonnxruntime_providers_shared.so'; Name = 'libonnxruntime_providers_shared.so' },
        @{ Asset = 'ort'; Entry = 'onnxruntime-linux-x64-gpu-1.23.0/lib/libonnxruntime_providers_cuda.so'; Name = 'libonnxruntime_providers_cuda.so' },
        @{ Asset = 'cuda-runtime'; Entry = 'nvidia/cuda_runtime/lib/libcudart.so.12'; Name = 'libcudart.so.12' },
        @{ Asset = 'cublas'; Entry = 'nvidia/cublas/lib/libcublas.so.12'; Name = 'libcublas.so.12' },
        @{ Asset = 'cublas'; Entry = 'nvidia/cublas/lib/libcublasLt.so.12'; Name = 'libcublasLt.so.12' },
        @{ Asset = 'curand'; Entry = 'nvidia/curand/lib/libcurand.so.10'; Name = 'libcurand.so.10' },
        @{ Asset = 'cufft'; Entry = 'nvidia/cufft/lib/libcufft.so.11'; Name = 'libcufft.so.11' },
        @{ Asset = 'cudnn'; Entry = 'nvidia/cudnn/lib/libcudnn.so.9'; Name = 'libcudnn.so.9' },
        @{ Asset = 'cudnn'; Entry = 'nvidia/cudnn/lib/libcudnn_graph.so.9'; Name = 'libcudnn_graph.so.9' },
        @{ Asset = 'cudnn'; Entry = 'nvidia/cudnn/lib/libcudnn_ops.so.9'; Name = 'libcudnn_ops.so.9' }
    )
}

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
        Copy-ArchiveEntry -ArchivePath $downloads[$entry.Asset] -EntryPath $entry.Entry -Destination $destination
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
    schema_version = 2
    platform = $fastPathPlatform
    model_revision = 'c25a394dd583836952667c12f008335071b3f43d'
    onnxruntime_version = '1.23.0'
    cuda_runtime_version = '12.1.105'
    cublas_version = '12.1.3.1'
    cufft_version = '11.0.2.54'
    curand_version = '10.3.2.106'
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
