<p align="center">
  <img src="src-tauri/icons/vekai-app.svg" alt="Vekai logo" width="128" height="128">
</p>

# Vekai

[中文说明](README_ZH.md)

Vekai is a Windows tray application that also provides Linux GUI/CLI service mode. It runs local Embedding and Rerank HTTP services with a bundled or locally downloaded llama.cpp runtime. Windows users can start the exe, configure GGUF model paths in the settings window, and keep the services running in the background from the system tray.

## Features

- Runs local Embedding and Rerank services at the same time.
- Lets users choose any `.gguf` model file path instead of requiring a fixed model directory.
- Provides `scripts\download-assets.ps1` to download default GGUF models; source packaging and runtime override flows can also download the latest matching llama.cpp runtime.
- Supports native llama.cpp endpoints plus SiliconFlow, Cohere, Jina, Voyage, and custom JSON HTTP compatible response shapes.
- Starts with a Chinese UI by default and includes an English language switch.
- Stays in the Windows system tray with actions for settings, logs, and quit.
- Prompts on settings-window close to either minimize to tray or quit the application.
- Windows, macOS, and Linux installers include the matching llama.cpp runtime; Linux builds support both GUI and `serve` service mode.

## Quick Start

1. Open `release\Vekai 0.1.8\Vekai.exe`, or install with `release\Vekai 0.1.8\Vekai_0.1.8_x64-setup.exe`.
2. The installer includes the matching llama.cpp runtime. To download default test models in a source checkout, run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -SkipRuntime
```

3. In the settings window, choose one `.gguf` model for Rerank and one `.gguf` model for Embedding. The script downloads default test models into the repository `models\` directory.
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
vekai serve
vekai install-service
vekai uninstall-service
```

`install-service` installs and starts a user-level systemd unit at `~/.config/systemd/user/vekai.service`. GUI and service mode share the current user's config, logs, runtime, and model paths. For startup before user login, run:

```bash
loginctl enable-linger "$USER"
```

Linux packages include the matching llama.cpp runtime. To temporarily override the runtime, place a matching `llama-b*-bin-<platform>.*` archive in the user data `downloads/` directory or an app-adjacent `downloads/` directory.

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
| User config | `%APPDATA%\Vekai\config.json` |
| Local data | `%LOCALAPPDATA%\Vekai\` |
| Logs | `%LOCALAPPDATA%\Vekai\logs` |
| Runtime source | Bundled package `downloads/`; overridable via `%LOCALAPPDATA%\Vekai\downloads` or an exe-adjacent `downloads\` folder |
| Extracted runtime | `%LOCALAPPDATA%\Vekai\runtimes` |
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
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -SkipRuntime
powershell -ExecutionPolicy Bypass -File scripts\prepare-bundled-runtime.ps1
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

- `src-tauri\target\release\vekai.exe`
- `src-tauri\target\release\bundle\nsis\Vekai_0.1.8_x64-setup.exe`

The repository may also contain a copied delivery folder:

- `release\Vekai 0.1.8\Vekai.exe`
- `release\Vekai 0.1.8\Vekai_0.1.8_x64-setup.exe`

The installer and exe include frontend assets and the matching llama.cpp runtime archive. GGUF models are not bundled. In a source checkout, run `scripts\download-assets.ps1 -SkipRuntime` to download default test models into `models\`.

## Troubleshooting

**Startup fails or the port is already in use**

Check whether Rerank and Embedding share a port, or whether another process is using the configured port. Change the port, save, and restart.

**No model selected**

Click Browse in the corresponding service panel and choose the correct `.gguf` model file.

**The first service startup is slow**

The first service startup extracts the bundled or local `downloads\llama-b*-bin-<platform>.*` archive into `%LOCALAPPDATA%\Vekai\runtimes`. Later starts reuse the extracted runtime. Before local source packaging, run `scripts\prepare-bundled-runtime.ps1`.

**Windows Defender or firewall prompts**

The app starts local HTTP services. Keep the host as `127.0.0.1` for local-only use. Use `0.0.0.0` only when LAN access is required and the network is trusted.

**Vulkan or GPU errors**

Make sure the GPU driver supports Vulkan. To isolate GPU issues, set GPU layers to `0` and verify CPU execution first, then switch back to `auto`, `all`, or a specific layer count.

**New logs are missing**

Current logs are written to `%LOCALAPPDATA%\Vekai\logs`. `%LOCALAPPDATA%\LocalRerankServer\logs` is only a historical development directory.
