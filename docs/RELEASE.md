# 发布与交付

本文档记录 Local AI Services 的 Windows 打包和交付事实。用户使用说明见 [../README_ZH.md](../README_ZH.md) 和 [../README.md](../README.md)。

## 打包命令

先下载 runtime 和默认模型：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1
```

再打包：

```powershell
npm run tauri:build
```

该命令会先执行前端构建，再构建 Tauri release 产物。

## 主要产物

Tauri 默认产物：

```text
src-tauri\target\release\local-rerank-server.exe
src-tauri\target\release\bundle\nsis\Local AI Services_0.1.0_x64-setup.exe
```

仓库交付目录可能包含同步副本：

```text
release\Local AI Services 0.1.0\Local AI Services.exe
release\Local AI Services 0.1.0\Local AI Services_0.1.0_x64-setup.exe
```

如果版本号或产品名变化，要同步：

- `src-tauri/tauri.conf.json`
- `README_ZH.md`
- `README.md`
- 本文档

## 运行时

llama.cpp Windows Vulkan x64 zip 不进 Git，也不再通过 `include_bytes!` 编进 exe。`scripts\download-assets.ps1` 会从 `ggml-org/llama.cpp` 最新 GitHub Release 自动选择 `llama-b*-bin-win-vulkan-x64.zip` 并默认下载到：

```text
%LOCALAPPDATA%\LocalAIService\downloads
```

应用启动服务时会搜索：

```text
%LOCALAPPDATA%\LocalAIService\downloads
<exe-dir>\downloads
<repo>\downloads
```

找到 zip 后解压到：

```text
%LOCALAPPDATA%\LocalAIService\runtimes
```

运行时 marker 使用 zip 文件名和 SHA256 判断是否需要重新解压。更新 zip 不要求改代码，但交付目录必须包含 exe 同级 `downloads\llama-b*-bin-win-vulkan-x64.zip`，或者客户先运行下载脚本把 runtime 放进本机数据目录。

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
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

如果只改文档，可以只跑 `git diff --check` 并说明没有重打包。

## 冒烟检查

发布前至少检查：

- 双击 exe 不出现终端窗口。
- 启动后先打开设置窗口。
- 设置窗口可切换中文/英文。
- 可以为两个服务选择 `.gguf` 模型。
- `downloads\` 中存在 `llama-b*-bin-win-vulkan-x64.zip`。
- 启动服务后“启动服务”按钮不可点。
- 关闭设置窗口会询问最小化到托盘或退出。
- 最小化到托盘后服务继续运行。
- 日志目录为 `%LOCALAPPDATA%\LocalAIService\logs`。
- 默认 Rerank 地址可访问 `/v1/rerank`。
- 默认 Embedding 地址可访问 `/v1/embeddings`。

## 常见交付问题

**安装包是否包含前端资源？**

是。Tauri 构建会把 `npm run build` 产生的前端资源打进应用。

**安装包是否包含 llama.cpp 运行时？**

不再包含。runtime zip 是下载资产，脚本默认放在 `%LOCALAPPDATA%\LocalAIService\downloads`。交付客户时可以同时提供 exe 同级 `downloads\`，或者让客户先运行 `scripts\download-assets.ps1`。

**安装包是否必须包含模型？**

不是。模型路径由用户在设置页选择。默认模型可以通过 `scripts\download-assets.ps1` 下载到 `models\`，也可以替换为客户自己的 GGUF 文件。

**旧日志目录还会被使用吗？**

不会。当前目录是 `%LOCALAPPDATA%\LocalAIService\logs`。`LocalRerankServer` 只可能是旧开发版本遗留。

**客户机器没有 Vulkan 能力怎么办？**

先把 GPU 层数设置为 `0` 验证 CPU 路径。如果 CPU 可运行，再检查显卡驱动、Vulkan 支持和 GPU 层数设置。
