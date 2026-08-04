<p align="center">
  <img src="src-tauri/icons/vekai-app.svg" alt="Vekai logo" width="128" height="128">
</p>

# Vekai

在本地运行 Embedding 和 Rerank 模型，通过 HTTP 接口暴露出去。你的应用连 `127.0.0.1` 就能用——不上云、不需要 API Key、数据不出本机。

[English](README.md) | [下载](https://github.com/AndersonBY/vekai/releases/latest) | [文档](docs/README.md)

## 快速上手

1. 从 [Releases](https://github.com/AndersonBY/vekai/releases/latest) 下载对应平台的安装包。
2. 启动 Vekai，给 Rerank 和 Embedding 各选一个 `.gguf` 模型。
3. 点**保存**，再点**启动服务**。
4. 搞定。服务地址如下：

```
Rerank:    http://127.0.0.1:8012/v1/rerank
Embedding: http://127.0.0.1:8013/v1/embeddings
```

**没有模型？** 安装包自带 llama.cpp 运行环境，但不含 GGUF 模型文件。下载默认模型：

```powershell
pwsh -File scripts\download-assets.ps1 -SkipRuntime
```

## 试一下

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

## 能力一览

- **两种服务，一个应用。** Embedding + Rerank 同时跑，也可以只开一个。
- **纯本地运行。** 默认监听 `127.0.0.1`，不需要账号，不需要联网。
- **模型自由选。** 任意目录下的 `.gguf` 文件，不限制路径。
- **多协议兼容。** 支持 OpenAI、Cohere、Jina、SiliconFlow、Voyage 等 API 格式。你的应用用哪种协议，Vekai 就给你转成哪种。
- **GPU 加速。** Windows 上可选 CUDA、Vulkan 或 CPU；NVIDIA 显卡在 Windows 和 Linux 上还能走 CUDA 快通道。
- **托盘常驻。** 关掉窗口，服务在系统托盘继续跑。
- **Linux 服务模式。** `vekai serve` 前台运行，`vekai install-service` 装成 systemd 服务。
- **便携打包。** 把 Vekai + 模型 + 配置 + 运行时打成一个文件夹，解压即用。

## 设置项

| 设置 | 说明 |
| --- | --- |
| 监听地址 | `127.0.0.1` 仅本机；`0.0.0.0` 开放局域网（谨慎使用） |
| 端口 | 每个服务独立端口，不能共用 |
| 接口协议 | 决定了你的应用用什么 API 格式调用 |
| 模型路径 | 每个服务单独指定 `.gguf` 文件 |
| GPU 层数 | `auto` 自动、`all` 全部、填数字、或 `0` 纯 CPU |
| 自动启动 | Vekai 启动后自动拉起服务 |
| 崩溃重启 | 模型进程异常退出后自动恢复 |
| 线程 / 批处理 | `0` 交给 llama.cpp 自己决定 |

[完整配置说明 →](docs/ARCHITECTURE.md)

## 文件位置

| 内容 | 路径 |
| --- | --- |
| 配置 | `%APPDATA%\Vekai\config.json`（便携版在 exe 同目录） |
| 日志 | `%LOCALAPPDATA%\Vekai\logs` |
| 运行时 | `%LOCALAPPDATA%\Vekai\runtimes` |

## 开发

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

更多：[架构说明](docs/ARCHITECTURE.md) · [质量规范](docs/QUALITY.md) · [发布指南](docs/RELEASE.md)

## 常见问题

**服务启动不了。** 看 `%LOCALAPPDATA%\Vekai\logs` 日志，确认模型文件存在、端口没被占用，再手动请求 `/health`。

**第一次启动慢。** 首次使用会解压运行时环境并缓存，后续就快了。模型加载本身也需要时间。

**CUDA 或 Vulkan 崩溃。** 换 CPU 便携包试试，或者把 GPU 层数设成 `0` 先用 CPU 跑通排查。

**其他电脑访问不了。** `127.0.0.1` 就是只给本机用的。真要开放局域网就改成 `0.0.0.0` 并配好防火墙，别直接暴露到公网。

**Windows 弹出 VC++ 安装提示。** 便携版检测到系统缺少 Visual C++ 运行库时会自动拉起微软官方安装程序，装完重新打开 Vekai 就行。
