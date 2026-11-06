<p align="center">
  <img src="src-tauri/icons/vekai-app.svg" alt="Vekai logo" width="128" height="128">
</p>

# Vekai

Vekai is a local service manager for Embedding and Rerank models. It runs and supervises local inference processes and exposes standard HTTP APIs on `127.0.0.1` for knowledge bases, RAG pipelines, and desktop AI apps.

[中文说明](README_ZH.md) | [Releases](https://github.com/AndersonBY/vekai/releases/latest) | [Docs](docs/README.md)

## Quick Start

1. Download the installer for your platform from [Releases](https://github.com/AndersonBY/vekai/releases/latest).
2. Open Vekai and select a local `.gguf` model for Rerank and Embedding.
3. Click **Save**, then **Start Services**.

Default endpoints:

```text
Rerank:    http://127.0.0.1:8012/v1/rerank
Embedding: http://127.0.0.1:8013/v1/embeddings
```

The installer bundles the `llama.cpp` runtime, without model weights. To download the default GGUF models from a source checkout:

```powershell
pwsh -File scripts\download-assets.ps1 -SkipRuntime
```

The default Embedding model is [EmbeddingGemma 2](https://ai.google.dev/gemma/docs/embeddinggemma) Q8_0 (768 dimensions, text + image embeddings). The download script fetches both `embeddinggemma-2-Q8_0.gguf` and the multimodal projector `mmproj-embeddinggemma-2-Q8_0.gguf`. Keep both files in the same folder and select `embeddinggemma-2-Q8_0.gguf` in settings; Vekai loads the projector automatically.

If you are upgrading from an older installation, make sure your `llama.cpp` runtime is b11457 or newer (rerun `download-assets.ps1` without `-SkipRuntime`, and add `-WindowsBackend cuda` if you use the Windows CUDA runtime). When switching between different Embedding models, re-index your existing vector collections.

For EmbeddingGemma 2, an unset physical batch size follows the logical batch size (2048 tokens by default). Each input must fit entirely in one physical batch; increase both batch sizes for longer text.

## API Examples

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

### Image Embeddings

With the default `openai_embeddings` protocol, `/v1/embeddings` supports `llama.cpp`'s multimodal input extension by passing a `content` array inside each `input` item:

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

You can also include `@{ type = 'text'; text = 'Image description' }` in the same `content` array to produce a combined text-and-image vector.

### Multimodal reranking

The default reranker is [Qwen3-VL-Reranker-2B](https://huggingface.co/Qwen/Qwen3-VL-Reranker-2B) Q8_0. The asset downloader supplies `Qwen3-VL-Reranker-2B.Q8_0.gguf` and `Qwen3-VL-Reranker-2B.mmproj-f16.gguf`; keep them together and select the main model. This conversion requires llama.cpp b12110 or newer. Saved BGE selections remain valid.

Multimodal reranking defaults to one slot, 8192 context tokens per slot, and at most 1280 visual tokens per image. Use one rerank slot when running both default models on an 8GB GPU. More slots consume more VRAM. Images are resized to fit the visual token budget; evaluate small-text accuracy for your data. See [measured performance](docs/PERFORMANCE.md).

POST this body to `http://127.0.0.1:8012/v1/rerank`:

```json
{
  "query": "A photograph of a cat",
  "documents": [
    {"image": "data:image/jpeg;base64,<image-base64>"},
    {"text": "A photograph", "image": "data:image/png;base64,<image-base64>"}
  ],
  "top_n": 2
}
```

Both queries and documents accept strings or objects with `text` and/or `image`. Image queries use `"query": {"image": "data:image/png;base64,..."}`. Native and proxied protocols preserve these inputs. Proxy request bodies are bounded at 32 MiB and responses omit image data. This is a Vekai/llama.cpp extension; clients must support these fields.

## Features

- **Dual Services**: Run Embedding and Rerank together or independently.
- **Protocol Adaptation**: Native OpenAI / `llama.cpp` endpoints plus built-in protocol translation for Cohere, Jina, SiliconFlow, and Voyage.
- **Hardware Acceleration**: Supports CPU, Vulkan, and CUDA, with an optional ONNX Runtime CUDA fast path for Qwen3-Embedding-0.6B.
- **Desktop & Service Modes**: Runs in the Windows system tray or as a Linux GUI app, foreground CLI (`vekai serve`), or user `systemd` service (`vekai install-service`).
- **Portable Builds**: Package Vekai, runtimes, config, and models into a self-contained directory.

## Configuration

| Setting | Description |
| --- | --- |
| Host | `127.0.0.1` (local only) or `0.0.0.0` (LAN access) |
| Port | Defaults to `8012` for Rerank and `8013` for Embedding |
| Protocol | Public API format exposed to caller applications |
| Model path | Path to the local `.gguf` model file |
| GPU layers | `auto`, `all`, a specific layer count, or `0` for CPU-only |
| Auto-start | Start enabled services automatically when Vekai launches |
| Restart on crash | Automatically restart a service process if it exits unexpectedly |
| Threads / batch | `0` lets `llama.cpp` choose automatically |

See [Architecture](docs/ARCHITECTURE.md) for the full configuration schema and protocol mappings.

## Paths

| Purpose | Path |
| --- | --- |
| Config | `%APPDATA%\Vekai\config.json` (or next to the executable in portable mode) |
| Logs | `%LOCALAPPDATA%\Vekai\logs` |
| Extracted runtimes | `%LOCALAPPDATA%\Vekai\runtimes` |

## Development

```powershell
npm install
pwsh -File scripts\download-assets.ps1 -SkipRuntime
npm run tauri:dev
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

Build portable Windows packages:

```powershell
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cpu
pwsh -File scripts\build-portable-windows.ps1 -WindowsBackend cuda
```

Further documentation: [Architecture](docs/ARCHITECTURE.md) · [Quality & Testing](docs/QUALITY.md) · [Release Guide](docs/RELEASE.md)

## Troubleshooting

- **Service fails to start**: Check `%LOCALAPPDATA%\Vekai\logs`, verify the `.gguf` file exists, and make sure the configured port is free.
- **Slow first launch**: Runtimes are extracted on first run and reused on subsequent launches.
- **CUDA or Vulkan errors**: Set GPU layers to `0` to test CPU mode, or switch to the CPU portable build.
- **Cannot connect from another machine**: `127.0.0.1` only accepts local connections. Change the host to `0.0.0.0` and allow the port through your firewall for LAN access.
- **VC++ installer prompt on Windows**: Portable builds check for the Microsoft Visual C++ 2015-2022 Redistributable on startup and launch the bundled installer when needed. Restart Vekai once installation finishes.
