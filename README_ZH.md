<p align="center">
  <img src="src-tauri/icons/vekai-app.svg" alt="Vekai logo" width="128" height="128">
</p>

# Vekai

Vekai 是一个本地 Embedding 与 Rerank 服务管理工具。它在本地启动并守护模型推理进程，向外提供标准 HTTP 接口，供知识库、RAG 和桌面 AI 应用直接调用。

[English](README.md) | [下载](https://github.com/AndersonBY/vekai/releases/latest) | [文档](docs/README.md)

## 快速上手

1. 从 [Releases](https://github.com/AndersonBY/vekai/releases/latest) 下载对应平台的安装包。
2. 启动 Vekai，为 Rerank 和 Embedding 分别选择本地 `.gguf` 模型文件。
3. 点击**保存**，再点击**启动服务**。

默认服务地址：

```text
Rerank:    http://127.0.0.1:8012/v1/rerank
Embedding: http://127.0.0.1:8013/v1/embeddings
```

安装包已内置 `llama.cpp` 运行时，不含模型文件。如需下载默认模型，在仓库目录下运行：

```powershell
pwsh -File scripts\download-assets.ps1 -SkipRuntime
```

默认 Embedding 模型为 [EmbeddingGemma 2](https://ai.google.dev/gemma/docs/embeddinggemma) Q8_0（768 维，支持文本与图片嵌入）。脚本会同时下载主模型 `embeddinggemma-2-Q8_0.gguf` 和多模态编码器 `mmproj-embeddinggemma-2-Q8_0.gguf`。将两个文件放在同一目录下，在设置页选择主模型即可，Vekai 会自动加载配套编码器。

从旧版本升级时，请确保 `llama.cpp` 版本不低于 b11457（去掉 `-SkipRuntime` 重新运行下载脚本即可更新运行时；Windows CUDA 用户可附加 `-WindowsBackend cuda`）。切换不同 Embedding 模型后，需要重新生成业务侧的向量索引。

EmbeddingGemma 2 未设置物理批大小时，Vekai 自动使其与逻辑批大小一致（默认 `2048` tokens）。单条输入必须完整放入一个物理批；处理更长文本时，请同时提高逻辑批和物理批大小。

## 接口调用示例

```powershell
# 健康检查
curl.exe http://127.0.0.1:8012/health
curl.exe http://127.0.0.1:8013/health

# Rerank 请求
curl.exe http://127.0.0.1:8012/v1/rerank `
  -H "Content-Type: application/json" `
  -d '{"model":"local-reranker","query":"什么是大熊猫？","top_n":2,"documents":["你好","大熊猫是中国特有的熊科动物","今天天气很好"]}'

# Embedding 请求
curl.exe http://127.0.0.1:8013/v1/embeddings `
  -H "Content-Type: application/json" `
  -d '{"model":"local-embedding","input":["你好世界"],"encoding_format":"float"}'
```

### 图片嵌入

在默认的 `openai_embeddings` 协议下，`/v1/embeddings` 支持 `llama.cpp` 的多模态扩展格式——在每个 `input` 元素中传入 `content` 数组：

```powershell
$image = [Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $PWD 'example.png')))
$body = @{
    model = 'local-embedding'
    input = @(@{ content = @(
        @{ type = 'image_url'; image_url = @{ url = "data:image/png;base64,$image" } }
    ) })
    encoding_format = 'float'
} | ConvertTo-Json -Depth 8
Invoke-RestMethod -Uri 'http://127.0.0.1:8013/v1/embeddings' -Method Post `
    -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body))
```

同一个 `content` 数组里也可以同时传入 `@{ type = 'text'; text = '图片说明' }`，生成图文联合向量。

## 功能特性

- **双服务管理**：同时或单独运行 Embedding 与 Rerank 服务。
- **多协议适配**：原生支持 OpenAI / llama.cpp 接口，内置 Cohere、Jina、SiliconFlow、Voyage 协议转换代理。
- **多后端加速**：支持 CPU、Vulkan 和 CUDA；针对 Qwen3-Embedding-0.6B 提供可选的 ONNX Runtime CUDA 加速通道。
- **桌面与服务双模式**：Windows 下支持系统托盘常驻与开机自启；Linux 下支持 GUI、`vekai serve` 前台运行及 `systemd` 用户服务安装。
- **绿色便携包**：支持将程序、配置、运行时和模型打包为独立目录，解压即用。

## 配置说明

| 设置项 | 说明 |
| --- | --- |
| 监听地址 | 默认 `127.0.0.1`（仅本机访问）；设为 `0.0.0.0` 可开放局域网访问 |
| 端口 | Rerank 默认 `8012`，Embedding 默认 `8013` |
| 接口协议 | 对外暴露的 API 格式（如 OpenAI、Cohere、Jina 等） |
| 模型路径 | 本地 `.gguf` 模型文件路径 |
| GPU 层数 | `auto`（自动）、`all`（全部加载到 GPU）、具体层数或 `0`（仅 CPU） |
| 自动启动 | 启动 Vekai 时自动拉起已启用的服务 |
| 崩溃重启 | 子进程异常退出时自动拉起 |
| 线程 / 批处理 | 设为 `0` 时由 `llama.cpp` 自动决定 |

完整配置字段与协议说明见 [架构说明](docs/ARCHITECTURE.md)。

## 目录结构

| 用途 | 路径 |
| --- | --- |
| 配置文件 | `%APPDATA%\Vekai\config.json`（便携版位于 `exe` 同级目录） |
| 日志目录 | `%LOCALAPPDATA%\Vekai\logs` |
| 运行时解压目录 | `%LOCALAPPDATA%\Vekai\runtimes` |

## 本地开发

```powershell
npm install
pwsh -File scripts\download-assets.ps1 -SkipRuntime
npm run tauri:dev
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

构建 Windows 便携包：

```powershell
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cpu
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cuda
```

更多开发文档：[架构说明](docs/ARCHITECTURE.md) · [测试与校验](docs/QUALITY.md) · [打包与发布](docs/RELEASE.md)

## 常见问题

- **服务无法启动**：检查 `%LOCALAPPDATA%\Vekai\logs` 下的日志，确认 `.gguf` 模型文件存在且对应端口未被占用。
- **首次启动较慢**：首次启动需要解压 `llama.cpp` 运行时并加载模型，后续启动会直接复用已解压的运行时。
- **显卡后端报错或崩溃**：可将 GPU 层数设为 `0` 切换到纯 CPU 模式排查驱动问题，或直接使用 CPU 版便携包。
- **局域网其他机器无法访问**：默认监听 `127.0.0.1` 仅限本机访问；如需局域网调用，请将监听地址改为 `0.0.0.0` 并配置防火墙规则。
- **便携版提示安装 VC++ 运行库**：Windows 便携版启动时若检测到系统缺少 Visual C++ 2015-2022 运行库，会自动拉起内置的微软官方安装程序，安装完成后重新打开即可。
