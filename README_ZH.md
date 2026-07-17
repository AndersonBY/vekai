<p align="center">
  <img src="src-tauri/icons/vekai-app.svg" alt="Vekai logo" width="128" height="128">
</p>

# Vekai

[English](README.md)

Vekai 是一个 Windows 托盘应用，也提供 Linux GUI/CLI 服务模式，用安装包内置或本机下载的 llama.cpp 运行时同时提供 Embedding 和 Rerank HTTP 服务。Windows 用户启动 exe 后会先看到设置窗口，配置完成后可以最小化到系统托盘，让服务在后台持续运行。

## 功能

- 同时管理本地 Embedding 服务和 Rerank 服务。
- 支持任意 `.gguf` 模型文件路径，不要求模型放在固定目录。
- 提供 `scripts\download-assets.ps1` 下载默认 GGUF 模型；源码打包和 runtime 覆盖场景也可下载当前平台匹配的最新 llama.cpp 运行时。
- Windows/Linux x64 + NVIDIA 上可选用 ONNX Runtime CUDA Embedding 快路径，资产缺失或进程异常时自动回退 llama.cpp。
- 支持 llama.cpp 原生接口，以及兼容 SiliconFlow、Cohere、Jina、Voyage 和自定义 JSON HTTP 风格的响应协议。
- 中文界面默认启用，也可以在设置页切换英文。
- Windows 托盘常驻，支持打开设置、打开日志目录和退出程序。
- 关闭设置窗口时会询问最小化到托盘还是退出程序。
- Linux/macOS/Windows Release 安装包会内置对应平台的 llama.cpp runtime；Linux 版本支持 GUI 和 `serve` 服务模式。

## 快速开始

1. 打开 `release\Vekai 0.1.9\Vekai.exe`，或运行安装包 `release\Vekai 0.1.9\Vekai_0.1.9_x64-setup.exe`。
2. 安装包已包含对应平台的 llama.cpp runtime。如需在源码仓库中下载默认测试模型，可以运行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -SkipRuntime
```

3. 在设置窗口中分别选择 Rerank 和 Embedding 的 `.gguf` 模型文件。下载脚本默认把测试模型保存到仓库的 `models\` 目录。
4. 确认端口、监听地址和接口协议。
5. 点击“保存”，再点击“启动服务”。
6. 不需要设置窗口时，关闭窗口并选择“最小化到托盘”。

下载脚本默认提供两个测试模型：

- `models\bge-reranker-v2-m3-Q4_K_M.gguf`
- `models\Qwen3-Embedding-0.6B-Q8_0.gguf`

下载其他平台 runtime 时可以指定：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -RuntimePlatform ubuntu-arm64 -SkipModels
```

NVIDIA Windows x64 机器可安装 CUDA 12.4 加速 runtime；下载完成后重启服务，Vekai 会自动优先使用完整的 CUDA 资产，缺失时仍回退安装包内置的 Vulkan runtime：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -WindowsBackend cuda -SkipModels
```

Windows/Linux x64 的 Qwen3-Embedding-0.6B 可进一步安装 FP16 ONNX CUDA 快路径。脚本固定校验当前平台的模型、ONNX Runtime、CUDA、cuBLAS、cuFFT、cuRAND 和 cuDNN 资产；安装完成后重启服务即可自动启用：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-fastpath.ps1
```

Linux 使用 `pwsh -File scripts/download-fastpath.ps1`，默认安装到 `$XDG_DATA_HOME/Vekai/fastpath`，未设置时使用 `~/.local/share/Vekai/fastpath`。

快路径只在所选文件名匹配 `Qwen3-Embedding-0.6B*.gguf` 时启用，HTTP 路径和协议保持不变。`/health` 返回 `backend: ort_cuda` 表示正在使用快路径；若 sidecar 启动失败、健康超时或运行期退出，Vekai 会自动在同一端口启动 llama.cpp。

## 默认地址

| 服务 | 默认协议 | 默认端口 | 默认接口 |
| --- | --- | ---: | --- |
| Rerank | `llama_rerank` | `8012` | `http://127.0.0.1:8012/v1/rerank` |
| Embedding | `openai_embeddings` | `8013` | `http://127.0.0.1:8013/v1/embeddings` |

如果把监听地址设置为 `0.0.0.0`，服务会监听所有网卡，只建议在可信网络中使用。

## 设置说明

| 设置 | 说明 |
| --- | --- |
| 监听地址 | 默认 `127.0.0.1`，只允许本机访问。设置为 `0.0.0.0` 可允许局域网访问。 |
| 端口 | Rerank 和 Embedding 启用时不能使用同一个端口。 |
| 接口协议 | 决定公开 HTTP 路径、请求兼容格式和响应兼容格式。 |
| 模型文件路径 | 通过“选择文件”选择 `.gguf` 文件，也可以直接粘贴完整路径。 |
| GPU 层数 | `auto` 自动选择，`all` 尽量全部放入 GPU，也可以填非负整数。 |
| 线程数 | 映射到 llama.cpp `--threads`，`0` 表示自动。 |
| 并发槽位 | 映射到 llama.cpp `--parallel`，控制同一服务的推理并发 slot，`0` 表示自动。 |
| 逻辑批大小 | 映射到 llama.cpp `--batch-size`，`0` 表示自动。 |
| 物理批大小 | 映射到 llama.cpp `--ubatch-size`，`0` 表示自动。较大的值可提高 GPU 批量吞吐，也会增加显存占用。 |
| 服务性能覆盖 | 每个服务可单独覆盖并发槽位、逻辑批大小和物理批大小；`0` 表示继承通用设置。 |
| HTTP 线程数 | 映射到 llama.cpp `--threads-http`，`0` 表示自动。 |
| 启动后自动开启已启用服务 | 应用启动后自动启动已勾选的服务。 |
| 崩溃后自动重启 | llama-server 异常退出后自动尝试重启。 |
| 日志保留天数 | 默认保留 7 天；单个 llama.cpp 服务日志超过 16 MiB 时会在下次启动前截断。 |

服务运行中修改会影响服务的配置后，保存按钮会变成“保存并重启”。

## 协议和接口

Rerank 支持：

| 协议 | 公开路径 | 说明 |
| --- | --- | --- |
| `llama_rerank` | `/v1/rerank` | llama.cpp 原生 Rerank 接口。 |
| `siliconflow` | `/rerank` | 转换为 SiliconFlow 风格响应。 |
| `cohere_rerank_v2` | `/v2/rerank` | 支持 Cohere v2 风格请求和响应。 |
| `jina_rerank_v1` | `/v1/rerank` | Jina v1 风格路径和响应兼容。 |
| `voyage_rerank_v1` | `/rerank` | Voyage v1 风格响应。 |
| `custom_json_http` | `/rerank` | 固定本地 JSON 代理格式，不是可视化自定义映射编辑器。 |

Embedding 支持：

| 协议 | 公开路径 | 说明 |
| --- | --- | --- |
| `openai_embeddings` | `/v1/embeddings` | OpenAI Embeddings 风格接口，也是 llama.cpp 原生 Embedding 路径。 |
| `siliconflow` | `/embeddings` | SiliconFlow 风格路径。 |
| `cohere_embed_v2` | `/v2/embed` | 支持 Cohere v2 的 `texts` 输入并转换响应。 |
| `voyage_embeddings_v1` | `/embeddings` | Voyage Embeddings 风格路径。 |
| `custom_json_http` | `/embeddings` | 固定本地 JSON 代理格式，不是可视化自定义映射编辑器。 |

非原生协议会启动一个本地协议代理：公开端口保持为你配置的端口，底层 `llama-server`/`llama-server.exe` 会运行在自动分配的本机内部端口。

## Linux 服务模式

Linux 安装包同时支持桌面 GUI 和 CLI 服务模式：

```bash
vekai serve
vekai install-service
vekai uninstall-service
```

`install-service` 会安装用户级 systemd unit 到 `~/.config/systemd/user/vekai.service`，并启动服务。GUI 和服务模式共用当前用户的配置、日志、runtime 和模型路径。需要无人登录后自动启动时，执行：

```bash
loginctl enable-linger "$USER"
```

Linux 安装包会内置对应平台的 llama.cpp runtime。需要临时覆盖 runtime 时，可以把匹配的 `llama-b*-bin-<platform>.*` 放到用户数据目录的 `downloads/` 或应用同级 `downloads/`。

## 请求示例

PowerShell 中建议使用 `curl.exe`。

Rerank：

```powershell
curl.exe http://127.0.0.1:8012/v1/rerank `
  -H "Content-Type: application/json" `
  -d "{\"model\":\"local-reranker\",\"query\":\"What is panda?\",\"top_n\":2,\"documents\":[\"hi\",\"it is a bear\",\"The giant panda is a bear species endemic to China.\"]}"
```

Embedding：

```powershell
curl.exe http://127.0.0.1:8013/v1/embeddings `
  -H "Content-Type: application/json" `
  -d "{\"model\":\"local-embedding\",\"input\":\"hello world\",\"encoding_format\":\"float\"}"
```

健康检查：

```powershell
curl.exe http://127.0.0.1:8012/health
curl.exe http://127.0.0.1:8013/health
```

## 文件目录

| 内容 | 位置 |
| --- | --- |
| 用户配置 | `%APPDATA%\Vekai\config.json` |
| 本地数据 | `%LOCALAPPDATA%\Vekai\` |
| 日志目录 | `%LOCALAPPDATA%\Vekai\logs` |
| 运行时来源 | 安装包内置 `downloads/`；可用 `%LOCALAPPDATA%\Vekai\downloads` 或 exe 同级 `downloads\` 覆盖 |
| 运行时解压目录 | `%LOCALAPPDATA%\Vekai\runtimes` |
| Windows Embedding 快路径资产 | `%LOCALAPPDATA%\Vekai\fastpath` |
| Linux Embedding 快路径资产 | `$XDG_DATA_HOME/Vekai/fastpath` 或 `~/.local/share/Vekai/fastpath` |
| 默认下载配置 | `scripts\assets.json` |

旧开发版本可能留下 `%LOCALAPPDATA%\LocalRerankServer\logs`，当前应用不会再写入这个旧目录。

## 开发

环境要求：

- Windows 10/11
- Node.js 和 npm
- Rust stable
- Tauri v2 所需的 Windows WebView2 运行时

常用命令：

```powershell
npm install
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -SkipRuntime
powershell -ExecutionPolicy Bypass -File scripts\prepare-bundled-runtime.ps1
powershell -ExecutionPolicy Bypass -File scripts\build-fastpath.ps1
npm run tauri:dev
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

前端单独调试：

```powershell
npm run dev
```

## 打包输出

执行：

```powershell
npm run tauri:build
```

主要输出：

- `src-tauri\target\release\vekai.exe`
- `src-tauri\target\release\bundle\nsis\Vekai_0.1.9_x64-setup.exe`

仓库也可能同步一份交付目录：

- `release\Vekai 0.1.9\Vekai.exe`
- `release\Vekai 0.1.9\Vekai_0.1.9_x64-setup.exe`

安装包和 exe 包含前端资源、对应平台的 llama.cpp runtime archive，以及 Windows/Linux x64 的小型原生 fastpath sidecar/ONNX 图；GGUF、FP16 权重和 GPU 动态库不包含在安装包内。源码仓库中运行 `scripts\download-assets.ps1 -SkipRuntime` 可以把默认测试模型下载到 `models\`。

## 常见问题

**启动失败或端口被占用**

检查 Rerank 和 Embedding 是否使用了同一个端口，或者系统中是否已有其他程序占用了对应端口。可以换成其他端口后保存并重启。

**提示没有选择模型**

在对应服务卡片中点击“选择文件”，选择正确的 `.gguf` 模型文件。

**首次启动服务较慢**

首次启动服务会把安装包内置或本机 `downloads\` 中的 `llama-b*-bin-<platform>.*` 解压到 `%LOCALAPPDATA%\Vekai\runtimes`，后续启动会复用已解压的运行时。源码仓库本地打包前先运行 `scripts\prepare-bundled-runtime.ps1`。

**Windows Defender 或防火墙提示**

应用会启动本机 HTTP 服务。只在本机使用时保持监听地址为 `127.0.0.1`；需要局域网访问时再改成 `0.0.0.0`，并只在可信网络中放行。

**CUDA、Vulkan 或 GPU 相关错误**

llama.cpp CUDA runtime 自动选择目前只用于 NVIDIA Windows x64；ORT Embedding 快路径支持 NVIDIA Windows/Linux x64，两者都要求较新的 NVIDIA 驱动。Windows CUDA 主包或 companion runtime 不完整时应用会自动使用 Vulkan；fastpath 资产不完整或 sidecar 失败时会自动回退 llama.cpp。仍有问题时，把 GPU 层数改为 `0` 验证 CPU 路径，再按机器能力调回 `auto`、`all` 或具体层数。

**找不到新日志**

当前日志目录是 `%LOCALAPPDATA%\Vekai\logs`。旧目录 `%LOCALAPPDATA%\LocalRerankServer\logs` 只是历史开发版本遗留。
