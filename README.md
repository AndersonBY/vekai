<p align="center">
  <img src="src-tauri/icons/vekai-app.svg" alt="Vekai logo" width="128" height="128">
</p>

# Vekai

Run Embedding and Rerank models locally, serve them over HTTP. Your apps talk to `127.0.0.1` — no cloud, no API keys, nothing leaves your machine.

[中文说明](README_ZH.md) | [Releases](https://github.com/AndersonBY/vekai/releases/latest) | [Docs](docs/README.md)

## Quick Start

1. Grab the [latest release](https://github.com/AndersonBY/vekai/releases/latest) for your platform.
2. Launch Vekai. Pick a `.gguf` model for Rerank and another for Embedding.
3. Click **Save**, then **Start Services**.
4. Done. Your services are live at:

```
Rerank:    http://127.0.0.1:8012/v1/rerank
Embedding: http://127.0.0.1:8013/v1/embeddings
```

**Don't have models?** The installer bundles the llama.cpp runtime but not GGUF files. Grab the defaults:

```powershell
pwsh -File scripts\download-assets.ps1 -SkipRuntime
```

## Try It

```powershell
# Health check
curl.exe http://127.0.0.1:8012/health
curl.exe http://127.0.0.1:8013/health

# Rerank
curl.exe http://127.0.0.1:8012/v1/rerank `
  -H "Content-Type: application/json" `
  -d '{"model":"local-reranker","query":"What is a panda?","top_n":2,"documents":["Hello","The giant panda is a bear native to China.","Sunny today."]}'

# Embedding
curl.exe http://127.0.0.1:8013/v1/embeddings `
  -H "Content-Type: application/json" `
  -d '{"model":"local-embedding","input":["Hello world"],"encoding_format":"float"}'
```

## What You Get

- **Two APIs, one process.** Embedding + Rerank. Enable both, or just one.
- **Offline-first.** Listens on `127.0.0.1`. No account, no internet required.
- **Pick your model.** Any `.gguf` file, any folder. No restrictions.
- **Protocol compatibility.** Speaks OpenAI, Cohere, Jina, SiliconFlow, Voyage formats. Pick the one your app expects — Vekai translates automatically.
- **GPU when you want it.** CUDA, Vulkan, or CPU on Windows. CUDA fast-path for NVIDIA on Windows and Linux.
- **Tray app.** Close the window, it keeps running in the system tray.
- **Linux service mode.** `vekai serve` for foreground, `vekai install-service` for systemd.
- **Portable packages.** Ship Vekai + models + config + runtimes as a single folder. No installation.

## Settings

| Setting | What it does |
| --- | --- |
| Host | `127.0.0.1` (local only) or `0.0.0.0` (LAN — use with care) |
| Port | One per service; can't be shared |
| Protocol | API format your app connects to |
| Model path | `.gguf` file per service |
| GPU layers | `auto`, `all`, a number, or `0` for CPU |
| Auto-start | Launch services when Vekai opens |
| Restart on crash | Recover if a model process dies |
| Threads / batch | `0` = let llama.cpp decide |

[Full config reference →](docs/ARCHITECTURE.md)

## Files

| What | Where |
| --- | --- |
| Config | `%APPDATA%\Vekai\config.json` (or next to the exe for portable builds) |
| Logs | `%LOCALAPPDATA%\Vekai\logs` |
| Runtimes | `%LOCALAPPDATA%\Vekai\runtimes` |

## Development

```powershell
npm install
pwsh -File scripts\download-assets.ps1 -SkipRuntime
npm run tauri:dev
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

Build a portable Windows package:

```powershell
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cpu
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cuda
```

More: [Architecture](docs/ARCHITECTURE.md) · [Quality](docs/QUALITY.md) · [Release](docs/RELEASE.md)

## Troubleshooting

**Service won't start.** Check `%LOCALAPPDATA%\Vekai\logs`. Make sure model files exist and ports aren't in use. Hit `/health` to verify.

**Slow first launch.** Runtimes are extracted on first run and cached. Model loading also takes a moment on cold start.

**CUDA / Vulkan crashes.** Try the CPU portable package, or set GPU layers to `0` to isolate the issue.

**Can't reach from another machine.** `127.0.0.1` is local-only by design. Use `0.0.0.0` and firewall rules if you need LAN access. Don't expose it to the internet.

**VC++ prompt on Windows.** The portable build checks for Microsoft Visual C++ and launches the installer if needed. Restart Vekai after.
