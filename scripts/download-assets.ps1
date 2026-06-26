[CmdletBinding()]
param(
    [string]$AssetsFile = '',
    [string]$DownloadDir = '',
    [string]$ModelsDir = '',
    [ValidateSet('auto', 'windows-x64', 'windows-arm64', 'ubuntu-x64', 'ubuntu-arm64', 'macos-x64', 'macos-arm64')]
    [string]$RuntimePlatform = 'auto',
    [switch]$SkipRuntime,
    [switch]$SkipModels,
    [switch]$Force,
    [switch]$NoHashCheck
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$ScriptRoot = if ([string]::IsNullOrWhiteSpace($PSScriptRoot)) {
    Split-Path -Parent $MyInvocation.MyCommand.Path
} else {
    $PSScriptRoot
}
$ProjectRoot = Split-Path -Parent $ScriptRoot

if ([string]::IsNullOrWhiteSpace($AssetsFile)) {
    $AssetsFile = Join-Path $ScriptRoot 'assets.json'
}
if ([string]::IsNullOrWhiteSpace($DownloadDir)) {
    $DownloadDir = Join-Path ([System.Environment]::GetFolderPath('LocalApplicationData')) 'LocalAIService\downloads'
}
if ([string]::IsNullOrWhiteSpace($ModelsDir)) {
    $ModelsDir = Join-Path $ProjectRoot 'models'
}

function Write-Step {
    param([string]$Message)
    Write-Host "[assets] $Message"
}

function Get-GitHubApiHeaders {
    $headers = @{
        'Accept' = 'application/vnd.github+json'
        'User-Agent' = 'LocalAIServiceAssetDownloader'
        'X-GitHub-Api-Version' = '2022-11-28'
    }

    $token = if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN)) {
        $env:GITHUB_TOKEN
    } elseif (-not [string]::IsNullOrWhiteSpace($env:GH_TOKEN)) {
        $env:GH_TOKEN
    } else {
        ''
    }

    if (-not [string]::IsNullOrWhiteSpace($token)) {
        $headers['Authorization'] = "Bearer $token"
    }

    return $headers
}

function Resolve-RuntimePlatform {
    param([string]$Requested)

    if ($Requested -ne 'auto') {
        return $Requested
    }

    $os = if ($IsLinux) {
        'ubuntu'
    } elseif ($IsMacOS) {
        'macos'
    } else {
        'windows'
    }

    $arch = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
        'X64' { 'x64' }
        'Arm64' { 'arm64' }
        default {
            throw "Unsupported CPU architecture for llama.cpp runtime: $([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)"
        }
    }

    return "$os-$arch"
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hash = $sha.ComputeHash($stream)
            return -join ($hash | ForEach-Object { $_.ToString('x2') })
        } finally {
            $sha.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

function Invoke-ResumableDownload {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$OutFile
    )

    $parent = Split-Path -Parent $OutFile
    New-Item -ItemType Directory -Force -Path $parent | Out-Null

    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curl) {
        $args = @(
            '--fail',
            '--location',
            '--continue-at', '-',
            '--retry', '8',
            '--retry-delay', '3',
            '--connect-timeout', '30',
            '--output', $OutFile,
            $Url
        )
        & $curl.Source @args
        if ($LASTEXITCODE -ne 0) {
            throw "curl.exe failed with exit code $LASTEXITCODE while downloading $Url"
        }
        return
    }

    Invoke-WebRequest -Uri $Url -OutFile $OutFile -Headers @{ 'User-Agent' = 'LocalAIServiceAssetDownloader' }
}

function Test-Asset {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Nullable[long]]$ExpectedSize,
        [string]$ExpectedSha256
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $false
    }

    if ($ExpectedSize.HasValue) {
        $actualSize = (Get-Item -LiteralPath $Path).Length
        if ($actualSize -ne $ExpectedSize.Value) {
            Write-Step "Size mismatch for $Path. Expected $($ExpectedSize.Value), got $actualSize."
            return $false
        }
    }

    if (-not $NoHashCheck -and -not [string]::IsNullOrWhiteSpace($ExpectedSha256)) {
        $actualSha = Get-Sha256 -Path $Path
        if ($actualSha -ne $ExpectedSha256.ToLowerInvariant()) {
            Write-Step "SHA256 mismatch for $Path. Expected $ExpectedSha256, got $actualSha."
            return $false
        }
    }

    return $true
}

function Get-LatestRuntimeAsset {
    param(
        [Parameter(Mandatory = $true)][string]$Repo,
        [Parameter(Mandatory = $true)][string]$Pattern
    )

    $apiUrl = "https://api.github.com/repos/$Repo/releases/latest"
    Write-Step "Resolving latest llama.cpp release from $apiUrl"
    $release = Invoke-RestMethod -Uri $apiUrl -Headers (Get-GitHubApiHeaders)
    $asset = $release.assets | Where-Object { $_.name -match $Pattern } | Select-Object -First 1
    if (-not $asset) {
        throw "No runtime asset matching $Pattern found in $Repo latest release $($release.tag_name)."
    }

    [pscustomobject]@{
        Name = [string]$asset.name
        Url = [string]$asset.browser_download_url
        Size = [long]$asset.size
        Tag = [string]$release.tag_name
    }
}

if (-not (Test-Path -LiteralPath $AssetsFile -PathType Leaf)) {
    throw "Assets file not found: $AssetsFile"
}

$assets = Get-Content -Raw -LiteralPath $AssetsFile | ConvertFrom-Json
New-Item -ItemType Directory -Force -Path $DownloadDir | Out-Null
New-Item -ItemType Directory -Force -Path $ModelsDir | Out-Null

if (-not $SkipRuntime) {
    $resolvedRuntimePlatform = Resolve-RuntimePlatform -Requested $RuntimePlatform
    $runtimePlatformConfig = $assets.runtime.platforms.$resolvedRuntimePlatform
    if (-not $runtimePlatformConfig) {
        throw "No runtime asset configuration for platform '$resolvedRuntimePlatform'."
    }

    $runtime = Get-LatestRuntimeAsset -Repo $assets.runtime.githubRepo -Pattern ([string]$runtimePlatformConfig.assetPattern)
    $runtimePath = Join-Path $DownloadDir $runtime.Name
    Write-Step "Runtime platform: $resolvedRuntimePlatform"
    Write-Step "Runtime: $($runtime.Name) from $($runtime.Tag)"

    if (-not $Force -and (Test-Asset -Path $runtimePath -ExpectedSize $runtime.Size -ExpectedSha256 $null)) {
        Write-Step "Runtime already present: $runtimePath"
    } else {
        if ($Force -and (Test-Path -LiteralPath $runtimePath)) {
            Remove-Item -LiteralPath $runtimePath -Force
        }
        Invoke-ResumableDownload -Url $runtime.Url -OutFile $runtimePath
        if (-not (Test-Asset -Path $runtimePath -ExpectedSize $runtime.Size -ExpectedSha256 $null)) {
            throw "Runtime download verification failed: $runtimePath"
        }
        Write-Step "Runtime downloaded: $runtimePath"
    }
}

if (-not $SkipModels) {
    foreach ($model in $assets.models) {
        $modelPath = Join-Path $ModelsDir ([string]$model.fileName)
        $expectedSize = if ($null -ne $model.sizeBytes) { [long]$model.sizeBytes } else { $null }
        $expectedSha = if ($null -ne $model.sha256) { [string]$model.sha256 } else { $null }
        Write-Step "Model: $($model.fileName)"

        if (-not $Force -and (Test-Asset -Path $modelPath -ExpectedSize $expectedSize -ExpectedSha256 $expectedSha)) {
            Write-Step "Model already present: $modelPath"
            continue
        }

        if ($Force -and (Test-Path -LiteralPath $modelPath)) {
            Remove-Item -LiteralPath $modelPath -Force
        }
        Invoke-ResumableDownload -Url ([string]$model.url) -OutFile $modelPath
        if (-not (Test-Asset -Path $modelPath -ExpectedSize $expectedSize -ExpectedSha256 $expectedSha)) {
            throw "Model download verification failed: $modelPath"
        }
        Write-Step "Model downloaded: $modelPath"
    }
}

Write-Step "Asset download complete."
