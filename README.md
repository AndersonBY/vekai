# Local AI Services

[中文说明](README_ZH.md)

Local AI Services is a Windows tray application that also provides Linux GUI/CLI service mode. It runs local Embedding and Rerank HTTP services with a locally downloaded llama.cpp runtime. Windows users can start the exe, configure GGUF model paths in the settings window, and keep the services running in the background from the system tray.

## Features

- Runs local Embedding and Rerank services at the same time.
- Lets users choose any `.gguf` model file path instead of requiring a fixed model directory.
- Provides `scripts\download-assets.ps1` to download the latest matching llama.cpp runtime for the current platform and default GGUF models.
- Supports native llama.cpp endpoints plus SiliconFlow, Cohere, Jina, Voyage, and custom JSON HTTP compatible response shapes.
- Starts with a Chinese UI by default and includes an English language switch.
- Stays in the Windows system tray with actions for settings, logs, and quit.
- Prompts on settings-window close to either minimize to tray or quit the application.
- Release assets include matching llama.cpp runtime archives for Windows, macOS, and Linux; Linux builds support both GUI and `serve` service mode.

## Quick Start

1. Open `release\Local AI Services 0.1.3\Local AI Services.exe`, or install with `release\Local AI Services 0.1.3\Local AI Services_0.1.3_x64-setup.exe`.
2. Before first use, run the asset downloader:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1
```

3. In the settings window, choose one `.gguf` model for Rerank and one `.gguf` model for Embedding. The script downloads default models into the repository `models\` directory.
4. Confirm the host, ports, and protocol options.
5. Click Save, then Start Services.
6. Close the settings window and choose Minimize to Tray when you want the services to keep running in the background.

The asset downloader provides two default test models:

- `models\bge-reranker-v2-m3-Q4_K_M.gguf`
- `models\Qwen3-Embedding-0.6B-Q8_0.gguf`

To download a runtime for another platform:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -RuntimePlatform ubuntu-arm64 -SkipModels
```

## Default URLs

| Service | Default protocol | Default port | Default endpoint |
| --- | --- | ---: | --- |
| Rerank | `llama_rerank` | `8012` | `http://127.0.0.1:8012/v1/rerank` |
| Embedding | `openai_embeddings` | `8013` | `http://127.0.0.1:8013/v1/embeddings` |

When the host is set to `0.0.0.0`, services listen on all network interfaces. Use that only on trusted networks.

## Settings

| Setting | Description |
| --- | --- |
| Host | Defaults to `127.0.0.1` for local-only access. Use `0.0.0.0` for LAN access. |
| Port | Enabled Rerank and Embedding services cannot share the same port. |
| Protocol | Controls the public HTTP path, compatible request shape, and compatible response shape. |
| Model file path | Use Browse to select a `.gguf` file, or paste a full path directly. |
| GPU layers | `auto` lets llama.cpp choose, `all` offloads as much as possible, or use a non-negative integer. |
| Threads | Maps to llama.cpp `--threads`; `0` means automatic. |
| Parallel slots | Maps to llama.cpp `--parallel` and controls same-service inference slots; `0` means automatic. |
| HTTP threads | Maps to llama.cpp `--threads-http`; `0` means automatic. |
| Auto-start enabled services | Starts enabled services when the app starts. |
| Restart on crash | Restarts `llama-server`/`llama-server.exe` after an unexpected exit. |
| Log retention days | Defaults to 7 days. |

If a service-impacting setting changes while a service is running, the Save button becomes Save & Restart.

## Protocols and Paths

Rerank protocols:

| Protocol | Public path | Notes |
| --- | --- | --- |
| `llama_rerank` | `/v1/rerank` | Native llama.cpp Rerank endpoint. |
| `siliconflow` | `/rerank` | Converts responses to a SiliconFlow-style shape. |
| `cohere_rerank_v2` | `/v2/rerank` | Accepts and returns Cohere v2 style JSON. |
| `jina_rerank_v1` | `/v1/rerank` | Jina v1 compatible path and response shape. |
| `voyage_rerank_v1` | `/rerank` | Voyage v1 style response shape. |
| `custom_json_http` | `/rerank` | Fixed local JSON proxy format, not a user-editable mapping editor. |

Embedding protocols:

| Protocol | Public path | Notes |
| --- | --- | --- |
| `openai_embeddings` | `/v1/embeddings` | OpenAI Embeddings style, also the native llama.cpp embedding path. |
| `siliconflow` | `/embeddings` | SiliconFlow-style path. |
| `cohere_embed_v2` | `/v2/embed` | Accepts Cohere v2 `texts` input and converts the response. |
| `voyage_embeddings_v1` | `/embeddings` | Voyage Embeddings style path. |
| `custom_json_http` | `/embeddings` | Fixed local JSON proxy format, not a user-editable mapping editor. |

Non-native protocols run through an app-managed local protocol proxy. The public port remains the configured port, while the underlying `llama-server`/`llama-server.exe` process runs on an automatically allocated internal loopback port.

## Linux Service Mode

Linux packages support both desktop GUI startup and CLI service mode:

```bash
local-rerank-server serve
local-rerank-server install-service
local-rerank-server uninstall-service
```

`install-service` installs and starts a user-level systemd unit at `~/.config/systemd/user/local-ai-services.service`. GUI and service mode share the current user's config, logs, runtime, and model paths. For startup before user login, run:

```bash
loginctl enable-linger "$USER"
```

The Release page includes matching llama.cpp runtime archives, for example `llama-b*-bin-ubuntu-arm64.tar.gz`. Place the matching archive in the user data `downloads/` directory or an app-adjacent `downloads/` directory before starting services.

## Request Examples

Use `curl.exe` from PowerShell.

Rerank:

```powershell
curl.exe http://127.0.0.1:8012/v1/rerank `
  -H "Content-Type: application/json" `
  -d "{\"model\":\"local-reranker\",\"query\":\"What is panda?\",\"top_n\":2,\"documents\":[\"hi\",\"it is a bear\",\"The giant panda is a bear species endemic to China.\"]}"
```

Embedding:

```powershell
curl.exe http://127.0.0.1:8013/v1/embeddings `
  -H "Content-Type: application/json" `
  -d "{\"model\":\"local-embedding\",\"input\":\"hello world\",\"encoding_format\":\"float\"}"
```

Health checks:

```powershell
curl.exe http://127.0.0.1:8012/health
curl.exe http://127.0.0.1:8013/health
```

## Files and Directories

| Content | Location |
| --- | --- |
| User config | `%APPDATA%\LocalAIService\config.json` |
| Local data | `%LOCALAPPDATA%\LocalAIService\` |
| Logs | `%LOCALAPPDATA%\LocalAIService\logs` |
| Runtime downloads | `%LOCALAPPDATA%\LocalAIService\downloads`, or an exe-adjacent `downloads\` folder |
| Extracted runtime | `%LOCALAPPDATA%\LocalAIService\runtimes` |
| Default asset manifest | `scripts\assets.json` |

Older development builds may have created `%LOCALAPPDATA%\LocalRerankServer\logs`. Current builds no longer write to that old directory.

## Development

Requirements:

- Windows 10/11
- Node.js and npm
- Rust stable
- Windows WebView2 runtime required by Tauri v2

Common commands:

```powershell
npm install
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1
npm run tauri:dev
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

Frontend-only development:

```powershell
npm run dev
```

## Packaging

Run:

```powershell
npm run tauri:build
```

Main outputs:

- `src-tauri\target\release\local-rerank-server.exe`
- `src-tauri\target\release\bundle\nsis\Local AI Services_0.1.3_x64-setup.exe`

The repository may also contain a copied delivery folder:

- `release\Local AI Services 0.1.3\Local AI Services.exe`
- `release\Local AI Services 0.1.3\Local AI Services_0.1.3_x64-setup.exe`

The installer and exe include frontend assets, but no longer include the llama.cpp runtime archive or GGUF models. Run `scripts\download-assets.ps1` to download the runtime into `%LOCALAPPDATA%\LocalAIService\downloads` and default models into `models\`. For customer delivery, ship those downloaded assets beside the app or ask the customer to run the script first.

## Troubleshooting

**Startup fails or the port is already in use**

Check whether Rerank and Embedding share a port, or whether another process is using the configured port. Change the port, save, and restart.

**No model selected**

Click Browse in the corresponding service panel and choose the correct `.gguf` model file.

**The first service startup is slow**

The first service startup extracts `%LOCALAPPDATA%\LocalAIService\downloads\llama-b*-bin-<platform>.*` into `%LOCALAPPDATA%\LocalAIService\runtimes`. Later starts reuse the extracted runtime. If the runtime archive is missing, run `scripts\download-assets.ps1` first.

**Windows Defender or firewall prompts**

The app starts local HTTP services. Keep the host as `127.0.0.1` for local-only use. Use `0.0.0.0` only when LAN access is required and the network is trusted.

**Vulkan or GPU errors**

Make sure the GPU driver supports Vulkan. To isolate GPU issues, set GPU layers to `0` and verify CPU execution first, then switch back to `auto`, `all`, or a specific layer count.

**New logs are missing**

Current logs are written to `%LOCALAPPDATA%\LocalAIService\logs`. `%LOCALAPPDATA%\LocalRerankServer\logs` is only a historical development directory.
