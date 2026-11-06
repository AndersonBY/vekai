# 文档索引

本目录包含 Vekai 的架构设计、构建打包与性能验证文档。用户快速上手请参考根目录的 [README_ZH.md](../README_ZH.md) 或 [README.md](../README.md)。

## 文档列表

| 文档 | 内容说明 |
| --- | --- |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 整体架构、模块职责、配置结构、协议转换与数据目录 |
| [RELEASE.md](RELEASE.md) | 多平台安装包构建、Windows 便携包组装、运行时与模型分发 |
| [QUALITY.md](QUALITY.md) | 本地构建与测试命令、模块验证重点、常见问题排查 |
| [PERFORMANCE.md](PERFORMANCE.md) | `llama.cpp` 与 ONNX Runtime CUDA 后端的性能测试记录与方案选型 |
| [EMBEDDING_RUNTIME.md](EMBEDDING_RUNTIME.md) | `src-fastpath` Rust 库接口与 Reqbase 离线资产打包说明 |
| [AGENT_WORKFLOW.md](AGENT_WORKFLOW.md) | 代码结构导航与日常开发维护约定 |

## 项目概览

- **应用名称**：`Vekai`
- **核心功能**：本地 Embedding 与 Rerank 推理服务的桌面托盘应用与 CLI 服务管理器
- **默认端口**：Rerank 为 `8012`（`/v1/rerank`），Embedding 为 `8013`（`/v1/embeddings`）
- **技术栈**：Tauri v2、Rust、React、Vite、`llama.cpp`、ONNX Runtime
- **资产管理**：运行时归档由 `scripts/prepare-bundled-runtime.ps1` 在打包前拉取；默认模型定义在 `scripts/assets.json` 中，通过 `scripts/download-assets.ps1` 按需下载。
