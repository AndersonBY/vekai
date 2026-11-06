# 开发与维护指南

本文档整理 Vekai 的日常开发流程、代码定位方式以及各模块的验证命令。

## 常用检索与状态查看

```powershell
rg "keyword"
rg --files
git status --short
git diff -- path\to\file
```

## 模块修改指南

- **协议扩展与转换**：主要在 `src-tauri/src/config.rs`、`src-tauri/src/protocol_adapter.rs` 和 `src-tauri/src/protocol_proxy.rs` 中实现，并补充对应的协议转换单元测试。
- **设置界面与交互**：位于 `src/App.tsx`、`src/styles.css` 和 `src/types.ts`，需同步兼顾中英文界面文案。
- **窗口、托盘与进程管理**：位于 `src-tauri/src/lib.rs`、`src-tauri/src/process.rs` 和 `src-tauri/src/commands.rs`。
- **打包与运行时发现**：涉及 `src-tauri/tauri.conf.json`、`src-tauri/src/runtime.rs` 以及 `scripts/` 下的构建脚本。

## 常用验证命令

| 改动范围 | 验证命令 |
| --- | --- |
| 文档修改 | `git diff --check` |
| Rust 后端逻辑（配置、协议、进程） | `cargo test --manifest-path src-tauri/Cargo.toml` |
| 前端代码与样式 | `npm run build` |
| Embedding 快路径（Windows/Linux x64） | `cargo test --release --locked --manifest-path src-fastpath/Cargo.toml` |
| 桌面端完整打包 | `npm run tauri:build` |

## 文档同步

当修改涉及以下内容时，请同步更新对应文档：

- 端口、公开 HTTP 路径、协议名或请求/响应格式（更新 `README_ZH.md`、`README.md`、`docs/ARCHITECTURE.md`）
- 配置项或默认值（更新 `docs/ARCHITECTURE.md`）
- 数据目录、日志路径或运行时解压机制（更新 `docs/ARCHITECTURE.md`、`docs/RELEASE.md`）
- 默认模型、运行时版本或打包脚本参数（更新 `docs/RELEASE.md`）
