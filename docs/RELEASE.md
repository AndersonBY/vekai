# 打包与发布

本文档说明 Vekai 在 Windows、macOS 和 Linux 平台上的打包流程、运行时与模型分发方式，以及 GitHub Release 自动发布配置。

## 本地打包流程

1. 准备安装包内置的 `llama.cpp` 运行时（自动从 `ggml-org/llama.cpp` 下载当前平台匹配的归档到 `bundle-resources/downloads/`）：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\prepare-bundled-runtime.ps1
```

2. 构建 Embedding 快路径 sidecar（仅 Windows x64 / Linux x64 需要）并执行 Tauri 打包：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-fastpath.ps1
npm run tauri:build
```

Windows 默认构建产物位于：

```text
src-tauri\target\release\vekai.exe
src-tauri\target\release\bundle\nsis\Vekai_0.1.12_x64-setup.exe
```

### 构建 Windows 便携包（Portable ZIP）

完成上述构建并下载默认模型后，可组装解压即用的 Windows x64 便携包：

```powershell
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cpu
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cuda
```

便携包构建脚本会校验 `scripts/assets.json` 中定义的模型与微软 VC++ x64 运行库文件（SHA-256 及大小），将可执行文件、相对路径 `config.json`、模型、指定后端运行时、Fastpath 资源、`redist\` 目录以及 `SHA256SUMS.txt` 一并输出至 `release\Vekai <version>\`。

便携版启动时若同时检测到同级 `config.json` 和 `redist\version.txt`，会先检查系统注册表中的 Microsoft Visual C++ 2015-2022 x64 Runtime 版本；若缺失或版本偏旧，会弹窗提示并拉起内置的 `redist\vc_redist.x64.exe`。

## 运行时分发机制

`llama.cpp` 运行时打包在安装包的 `downloads/` 资源目录中，首次启动服务时解压至 `%LOCALAPPDATA%\Vekai\runtimes`。启动时按以下优先级查找运行时归档：

```text
%LOCALAPPDATA%\Vekai\downloads
<exe-dir>\downloads
<bundle-resource-dir>\downloads
<repo>\downloads
```

因此，将新版或特定后端的运行时放到 `%LOCALAPPDATA%\Vekai\downloads` 或程序同级 `downloads\` 目录下即可直接覆盖内置运行时。

启用 EmbeddingGemma 2 时会跳过低于 b11457 的归档。旧 CUDA 缓存不满足模型要求时，应用会继续寻找其他目录中的兼容 CUDA，随后尝试兼容 CPU / Vulkan；没有兼容版本时提示更新运行时。Windows CUDA 用户可运行 `pwsh -File scripts/download-assets.ps1 -SkipModels -WindowsBackend cuda` 更新实际优先使用的本机下载目录。

各平台对应的运行时归档：

| 平台 | 脚本参数 | `llama.cpp` 资产名称 |
| --- | --- | --- |
| Windows x64 | `windows-x64` | 默认 `llama-b*-bin-win-cpu-x64.zip`；可选 `-WindowsBackend vulkan` 或 `-WindowsBackend cuda`（含 `cudart`） |
| Windows ARM64 | `windows-arm64` | `llama-b*-bin-win-cpu-arm64.zip` |
| Ubuntu / Linux x64 | `ubuntu-x64` | `llama-b*-bin-ubuntu-x64.tar.gz` |
| Ubuntu / Linux ARM64 | `ubuntu-arm64` | `llama-b*-bin-ubuntu-arm64.tar.gz` |
| macOS x64 | `macos-x64` | `llama-b*-bin-macos-x64.tar.gz` |
| macOS ARM64 | `macos-arm64` | `llama-b*-bin-macos-arm64.tar.gz` |

## 模型与 Fastpath 资产分发

### 默认 GGUF 模型

安装包默认不包含模型权重。通过 `scripts/download-assets.ps1` 下载的默认模型保存在 `models\` 目录：

```text
models\bge-reranker-v2-m3-Q4_K_M.gguf
models\embeddinggemma-2-Q8_0.gguf
models\mmproj-embeddinggemma-2-Q8_0.gguf
```

EmbeddingGemma 2 主模型与 `mmproj-embeddinggemma-2-Q8_0.gguf` 需保持在同一目录，配置中只需指定主模型 `embeddinggemma-2-Q8_0.gguf`，且要求 `llama.cpp` 版本不低于 b11457。

主模型和编码器固定使用 `ggml-org/embeddinggemma-2-GGUF` revision `6ea3e3a309d12a4779cfda98b8c19800d2d5da71`，文件大小与 SHA256 记录在 `scripts/assets.json`。下载脚本支持从最新 Release 的 `nightly-tag.txt` 解析实际运行时版本。EmbeddingGemma 2 默认通过 llama.cpp 运行，Qwen 专用 ONNX 快路径不适用于该模型。

### ONNX Runtime CUDA 快路径（可选）

Windows x64 和 Linux x64 安装包已内置原生 Rust sidecar 与 ONNX 结构图。如需为 `Qwen3-Embedding-0.6B` 启用 ORT CUDA 快路径，可在目标机器运行：

```powershell
# Windows
powershell -ExecutionPolicy Bypass -File scripts\download-fastpath.ps1

# Linux
pwsh -File scripts/download-fastpath.ps1
```

该脚本会将 FP16 权重及固定版本的 ONNX Runtime / CUDA / cuDNN 动态库安装到 `%LOCALAPPDATA%\Vekai\fastpath`（Linux 下为 `$XDG_DATA_HOME/Vekai/fastpath` 或 `~/.local/share/Vekai/fastpath`），并在校验通过后生成 `manifest.json`（schema 2）。

## GitHub Release 自动发布

`.github/workflows/release.yml` 负责跨平台构建与发布：

- **日常回归构建**：推送到 `master` 分支时自动运行六平台矩阵构建（Windows x64/ARM64、macOS x64/ARM64、Ubuntu x64/ARM64），产物上传至 Actions Artifacts。
- **正式版本发布**：推送 `vMAJOR.MINOR.PATCH` 标签（或手动触发工作流并指定 tag）时，先校验 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 版本号一致，随后构建并将各平台安装包发布到 GitHub Release。

当前构建环境使用 GitHub 已提供的新版 runner，并固定系统 / 工具链标签：

| 构建目标 | GitHub runner |
| --- | --- |
| Windows x64 | `windows-2025-vs2026` |
| Windows ARM64 | `windows-11-vs2026-arm` |
| macOS Intel | `macos-26-intel` |
| macOS ARM64 | `macos-26` |
| Ubuntu x64 | `ubuntu-26.04` |
| Ubuntu ARM64 | `ubuntu-26.04-arm` |

版本校验和发布任务也使用 `ubuntu-26.04`。六个平台构建统一使用 Node.js 26 LTS 和 Rust stable；固定 Node.js 主版本与 runner 标签，补丁更新由各工具链供应方维护。可用 runner 见 [GitHub 镜像清单](https://github.com/actions/runner-images#available-images)，Node.js LTS 版本见 [Node.js 下载页](https://nodejs.org/en/download)。

GitHub 已于 2026-11-02 下线 macOS 14 runner，不能再使用 `macos-14`（见[官方公告](https://github.blog/changelog/2026-10-01-github-actions-macos-14-runner-image-retirement/)）。

```powershell
git tag v0.1.12
git push origin master
git push origin v0.1.12
```

若版本标签创建后需要修复工作流，先将工作流修复提交推送到 `master`，再从 `master` 手动调度该版本。工作流定义取自 `master`，应用源码仍检出指定标签，避免移动已发布的版本标签。直接重新运行旧作业会继续使用旧工作流定义。

```powershell
gh workflow run release.yml --ref master -f tag=v0.1.12
```

Linux `.deb` 安装包还会附带 `systemd` 服务说明与示例单元文件（位于 `/usr/share/doc/vekai/`）。

## 发布前检查清单

- [ ] 版本号在 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 中保持一致。
- [ ] Windows 下双击启动不弹出命令行窗口，关闭主窗口时正常提示最小化到托盘或退出。
- [ ] 便携包移动目录后仍可通过相对路径正常启动 Rerank（`8012`）与 Embedding（`8013`）服务。
- [ ] Linux 下 `vekai serve` 与 `vekai install-service` 工作正常。
