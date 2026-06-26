# Agent Guide

This repository is optimized for agentic development. Keep this file short: it is the map, not the encyclopedia. Long-lived project facts belong in `docs/`.

## Start Here

- Product overview and user setup: [README_ZH.md](README_ZH.md) and [README.md](README.md)
- Documentation map: [docs/README.md](docs/README.md)
- Architecture and module boundaries: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Agent working loop: [docs/AGENT_WORKFLOW.md](docs/AGENT_WORKFLOW.md)
- Quality and verification rules: [docs/QUALITY.md](docs/QUALITY.md)
- Packaging and delivery notes: [docs/RELEASE.md](docs/RELEASE.md)

## Product Snapshot

Local AI Services is a Windows tray app and Linux GUI/CLI service app for running local GGUF-based Embedding and Rerank HTTP services. It is built with Tauri v2, Rust, React, and Vite. Matching llama.cpp runtimes and default GGUF models are downloaded by `scripts/download-assets.ps1` and are intentionally not tracked in Git.

The app name for user-facing data is `LocalAIService`:

- Config: `%APPDATA%\LocalAIService\config.json`
- Logs: `%LOCALAPPDATA%\LocalAIService\logs`
- Runtime extraction: `%LOCALAPPDATA%\LocalAIService\runtimes`
- Runtime downloads: `%LOCALAPPDATA%\LocalAIService\downloads` or repository `downloads\`

Do not reintroduce `LocalRerankServer` for current paths. Mentions of that name should be historical only.

## Working Rules

- Prefer repository facts over memory or assumptions. Inspect current code before changing behavior.
- Keep `AGENTS.md` as a compact routing file. Add durable explanations to `docs/`.
- Do not add legacy config compatibility unless the user explicitly asks. This project is still in active development.
- Preserve user work in the dirty tree. Stage or edit only files relevant to the current task.
- Use `rg` for search and `apply_patch` for manual file edits.
- Keep UI text Chinese by default, with English as the secondary language.
- When changing Windows app behavior, verify the packaged app assumptions, not just the dev server.

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

For docs-only changes, run at least `git diff --check` and a focused read-through of changed Markdown.

## Current Defaults

| Service | Default protocol | Default port | Native path |
| --- | --- | ---: | --- |
| Rerank | `llama_rerank` | `8012` | `/v1/rerank` |
| Embedding | `openai_embeddings` | `8013` | `/v1/embeddings` |

Supported protocol details live in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Where To Change Things

- Tauri window, tray, lifecycle, and command registration: `src-tauri/src/lib.rs`
- Config schema and validation: `src-tauri/src/config.rs`
- App commands and path resolution: `src-tauri/src/commands.rs`
- llama.cpp argument construction: `src-tauri/src/llama_args.rs`
- Process supervision and restart behavior: `src-tauri/src/process.rs`
- Runtime extraction: `src-tauri/src/runtime.rs`
- Asset download manifest and script: `scripts/assets.json`, `scripts/download-assets.ps1`
- Model discovery helpers: `src-tauri/src/models.rs`
- Protocol adapters and proxying: `src-tauri/src/protocol_adapter.rs`, `src-tauri/src/protocol_proxy.rs`
- React settings UI: `src/App.tsx`, `src/styles.css`, `src/types.ts`

## Verification Expectations

- Rust behavior: run `cargo test --manifest-path src-tauri/Cargo.toml`.
- Frontend/type changes: run `npm run build`.
- Packaging or Windows app behavior: run `npm run tauri:build` when practical.
- Linux service behavior: verify `local-rerank-server serve` and, when possible, `install-service` under WSL/Docker/Linux.
- Runtime or model asset changes: run `powershell -ExecutionPolicy Bypass -File scripts\download-assets.ps1` and verify the downloaded files are ignored by Git.
- Protocol changes: add or update Rust tests around request/response conversion.
- UI changes: verify the settings window behavior, tray behavior, and Chinese/English text.
- Docs changes: check links, commands, product names, and data directories.

## Documentation Maintenance

When a decision becomes durable, update `docs/` in the same change. Good candidates:

- new service protocols or endpoint behavior
- config schema changes
- packaging, installer, or release layout changes
- asset download URLs, hashes, sizes, or runtime lookup changes
- data directory, log, or runtime extraction changes
- agent workflow rules learned from a failure

Avoid adding task-specific notes to this file. Link to the relevant doc instead.
