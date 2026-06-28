# 发布与交付

本文档记录 Vekai 的 Windows、macOS 和 Linux 打包交付事实。用户使用说明见 [../README_ZH.md](../README_ZH.md) 和 [../README.md](../README.md)。

## 打包命令

先准备安装包内置 runtime。该命令会从 `ggml-org/llama.cpp` 最新 GitHub Release 下载当前平台匹配的 archive 到 `bundle-resources/downloads/`：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\prepare-bundled-runtime.ps1
```

再打包：

```powershell
npm run tauri:build
```

该命令会先执行前端构建，再构建 Tauri release 产物。

## 主要产物

Tauri 默认产物：

```text
src-tauri\target\release\vekai.exe
src-tauri\target\release\bundle\nsis\Vekai_0.1.5_x64-setup.exe
```

仓库交付目录可能包含同步副本：

```text
release\Vekai 0.1.5\Vekai.exe
release\Vekai 0.1.5\Vekai_0.1.5_x64-setup.exe
```

如果版本号或产品名变化，要同步：

- `src-tauri/tauri.conf.json`
- `README_ZH.md`
- `README.md`
- 本文档

## 运行时

llama.cpp runtime 不进 Git，也不通过 `include_bytes!` 编进 exe。发布和本地打包前使用 `scripts/prepare-bundled-runtime.ps1` 从 `ggml-org/llama.cpp` 最新 GitHub Release 自动选择当前平台 runtime，并保存到：

```text
bundle-resources\downloads
```

Tauri 打包会把该目录映射到安装包资源目录的 `downloads/`。应用启动服务时会按顺序搜索：

```text
%LOCALAPPDATA%\Vekai\downloads
<exe-dir>\downloads
<bundle-resource-dir>\downloads
<repo>\downloads
```

找到 runtime archive 后解压到：

```text
%LOCALAPPDATA%\Vekai\runtimes
```

运行时 marker 使用 archive 文件名和 SHA256 判断是否需要重新解压。更新 runtime 不要求改代码，但每个平台安装包必须包含对应平台的 `downloads\llama-b*-bin-<platform>.*`。本机数据目录和 exe 同级 `downloads\` 仍可用于调试或临时覆盖安装包内置 runtime。

支持的 runtime 平台：

| 平台 | 下载脚本参数 | llama.cpp asset |
| --- | --- | --- |
| Windows x64 | `windows-x64` | `llama-b*-bin-win-vulkan-x64.zip` |
| Windows ARM64 | `windows-arm64` | `llama-b*-bin-win-cpu-arm64.zip` |
| Ubuntu/Linux x64 | `ubuntu-x64` | `llama-b*-bin-ubuntu-x64.tar.gz` |
| Ubuntu/Linux ARM64 | `ubuntu-arm64` | `llama-b*-bin-ubuntu-arm64.tar.gz` |
| macOS x64 | `macos-x64` | `llama-b*-bin-macos-x64.tar.gz` |
| macOS ARM64 | `macos-arm64` | `llama-b*-bin-macos-arm64.tar.gz` |

## 模型交付

模型文件不进 Git，也不要求打进安装包。用户在设置页分别为 Rerank 和 Embedding 选择 `.gguf` 文件路径。默认模型由 `scripts/assets.json` 定义，并由下载脚本保存到 `models\`。

下载脚本默认模型：

```text
models\bge-reranker-v2-m3-Q4_K_M.gguf
models\Qwen3-Embedding-0.6B-Q8_0.gguf
```

给客户交付时可以选择两种方式：

1. 安装包加单独模型文件，让用户在设置页选择模型路径。
2. 绿色目录包含 exe、安装包和 `models\`，但仍以设置页选择的完整路径为准。

不要把代码改成硬编码某个开发机路径。

## 打包前检查

```powershell
git diff --check
powershell -ExecutionPolicy Bypass -File scripts\prepare-bundled-runtime.ps1
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

如果只改文档，可以只跑 `git diff --check` 并说明没有重打包。

## GitHub Release 工作流

`.github/workflows/release.yml` 在推送 `vMAJOR.MINOR.PATCH` tag 时触发，例如：

```powershell
git tag v0.1.5
git push origin master
git push origin v0.1.5
```

工作流会先校验 tag 版本号与以下文件一致：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

然后分别构建并上传这些平台安装包：

- Windows x64
- Windows ARM64
- macOS x64
- macOS ARM64
- Ubuntu x64
- Ubuntu ARM64

工作流会在各平台构建前下载对应平台的 llama.cpp runtime archive，并把它打进安装包资源目录；Release 页面发布安装包产物，不再要求用户单独下载 runtime archive。模型仍按本文档“模型交付”章节处理，不进安装包。

如果需要重跑某个已存在 tag 的发布，可以在 GitHub Actions 页面手动运行 `Release` 工作流并输入 tag，例如 `v0.1.5`。工作流会覆盖同名 Release asset。

工作流也监听 `master` 分支 push，用于让新仓库在 GitHub Actions 页面注册工作流；发布构建 job 只会在 tag push 或手动输入 tag 时运行。

Linux `.deb` 包会额外包含服务模式说明文件：

```text
/usr/share/doc/vekai/README-service.md
/usr/share/doc/vekai/examples/vekai.service
```

应用资源目录也会包含同一份 `linux-service/` 文件，方便 GUI 包内查看或复制。

## 冒烟检查

发布前至少检查：

- 双击 exe 不出现终端窗口。
- 启动后先打开设置窗口。
- 设置窗口可切换中文/英文。
- 可以为两个服务选择 `.gguf` 模型。
- `bundle-resources\downloads\` 中存在且仅存在当前平台的 `llama-b*-bin-<platform>.*`。
- Linux 上 `vekai serve` 可以以前台服务模式启动。
- Linux 上 `vekai install-service` 会安装用户级 systemd unit。
- 启动服务后“启动服务”按钮不可点。
- 设置页可配置线程数、并发槽位和 HTTP 线程数，`0` 表示由 llama.cpp 自动决定。
- 关闭设置窗口会询问最小化到托盘或退出。
- 最小化到托盘后服务继续运行。
- 日志目录为 `%LOCALAPPDATA%\Vekai\logs`。
- 默认 Rerank 地址可访问 `/v1/rerank`。
- 默认 Embedding 地址可访问 `/v1/embeddings`。

## 常见交付问题

**安装包是否包含前端资源？**

是。Tauri 构建会把 `npm run build` 产生的前端资源打进应用。

**安装包是否包含 llama.cpp 运行时？**

包含。每个平台安装包会内置对应平台的 llama.cpp runtime archive，首次启动服务时解压到 `%LOCALAPPDATA%\Vekai\runtimes`。本机数据目录或 exe 同级 `downloads\` 中的匹配 archive 可用于临时覆盖。

**安装包是否必须包含模型？**

不是。模型路径由用户在设置页选择。默认模型可以通过 `scripts\download-assets.ps1` 下载到 `models\`，也可以替换为客户自己的 GGUF 文件。

**旧日志目录还会被使用吗？**

不会。当前目录是 `%LOCALAPPDATA%\Vekai\logs`。`LocalRerankServer` 只可能是旧开发版本遗留。

**客户机器没有 Vulkan 能力怎么办？**

先把 GPU 层数设置为 `0` 验证 CPU 路径。如果 CPU 可运行，再检查显卡驱动、Vulkan 支持和 GPU 层数设置。
