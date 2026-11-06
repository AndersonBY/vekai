# Development Guide

Vekai is a desktop tray app and CLI service manager for running local Embedding and Rerank HTTP services, built with Tauri v2, Rust, React, and Vite.

## Documentation Map

- User overview: [README_ZH.md](README_ZH.md) and [README.md](README.md)
- Docs index: [docs/README.md](docs/README.md)
- Architecture & protocols: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Development workflow: [docs/AGENT_WORKFLOW.md](docs/AGENT_WORKFLOW.md)
- Testing & verification: [docs/QUALITY.md](docs/QUALITY.md)
- Packaging & releases: [docs/RELEASE.md](docs/RELEASE.md)
- Performance notes: [docs/PERFORMANCE.md](docs/PERFORMANCE.md)
- Embedded runtime library: [docs/EMBEDDING_RUNTIME.md](docs/EMBEDDING_RUNTIME.md)

## Project Snapshot

Default service endpoints:

| Service | Default protocol | Default port | Native path |
| --- | --- | ---: | --- |
| Rerank | `llama_rerank` | `8012` | `/v1/rerank` |
| Embedding | `openai_embeddings` | `8013` | `/v1/embeddings` |

Runtime data locations on Windows:

- Config: `%APPDATA%\Vekai\config.json` (or `config.json` next to the executable in portable mode)
- Logs: `%LOCALAPPDATA%\Vekai\logs`
- Extracted runtimes: `%LOCALAPPDATA%\Vekai\runtimes`
- Downloaded runtimes: `%LOCALAPPDATA%\Vekai\downloads` or `downloads\`

## Common Commands

```powershell
npm install
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1
powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1 -RuntimePlatform ubuntu-arm64 -SkipModels
npm run tauri:dev
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
git diff --check
```

## Key Modules

- App lifecycle, tray, window, and commands: `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`
- Config schema and validation: `src-tauri/src/config.rs`
- `llama-server` argument builder: `src-tauri/src/llama_args.rs`
- Process supervision and health checks: `src-tauri/src/process.rs`
- Runtime archive lookup and extraction: `src-tauri/src/runtime.rs`
- Embedding CUDA sidecar & fastpath: `src-tauri/src/fastpath.rs`, `src-fastpath/`
- Model discovery: `src-tauri/src/models.rs`
- Protocol conversion and local proxy: `src-tauri/src/protocol_adapter.rs`, `src-tauri/src/protocol_proxy.rs`
- Asset manifests and packaging scripts: `scripts/assets.json`, `scripts/download-assets.ps1`, `scripts/build-portable-windows.ps1`
- Settings UI: `src/App.tsx`, `src/styles.css`, `src/types.ts`

## Conventions

- Default UI language is Chinese, with English available in settings.
- `llama.cpp` runtime archives, `.gguf` models, and ONNX external weights are fetched via `scripts/` and excluded from Git.
- Keep documentation in `docs/` in sync when changing protocols, config fields, default models, paths, or packaging layout.
