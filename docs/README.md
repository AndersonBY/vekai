# 文档地图

本目录是 Vekai 的仓库知识库。根目录 [../AGENTS.md](../AGENTS.md) 只作为智能体入口地图，长期事实、设计解释、质量要求和发布步骤都应沉淀在这里。

## 文档索引

| 文档 | 用途 |
| --- | --- |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 当前架构、模块边界、协议代理和数据目录说明。 |
| [AGENT_WORKFLOW.md](AGENT_WORKFLOW.md) | Codex 或其他智能体在本仓库工作的步骤、上下文获取方式和交付要求。 |
| [QUALITY.md](QUALITY.md) | 测试、验证、质量门槛和常见风险清单。 |
| [RELEASE.md](RELEASE.md) | Windows、macOS、Linux 打包、交付目录、运行时和模型交付说明。 |

`docs/superpowers/` 是本地智能体规划历史目录，不作为 Git 提交内容。当前事实以本目录的顶层文档、README 和代码为准。

## 维护原则

- `AGENTS.md` 是目录，不是手册。新增长期规则时优先放入本目录。
- 文档必须描述当前代码行为。改代码时同步更新相关文档。
- 不把临时聊天结论当作事实；如果结论会影响后续开发，把它写进仓库。
- 不在文档中保留过时兼容策略，除非产品明确需要。
- 命令、路径、端口、协议名要可复制并与代码一致。

## 当前项目事实

- 产品名：`Vekai`
- 当前数据目录名：`Vekai`
- 默认 Rerank 端口：`8012`
- 默认 Embedding 端口：`8013`
- 运行时下载：`scripts/download-assets.ps1` 动态获取当前平台匹配的最新 llama.cpp runtime archive
- 模型下载：`scripts/assets.json` 中定义默认 GGUF 模型 URL、大小和 SHA256
- 默认 UI 语言：中文，可切换英文
- 主要技术栈：Tauri v2、Rust、React、Vite、llama.cpp

## 文档更新检查

涉及以下变化时必须检查本目录：

- 服务协议、HTTP 路径或请求/响应格式变化
- 配置字段、默认值或校验规则变化
- Windows 托盘、Linux 服务模式、窗口关闭、启动流程变化
- 日志、配置、运行时目录变化
- runtime 或模型下载 URL、hash、大小变化
- 打包产物、安装包名称或交付目录变化
- 模型选择或运行时提取机制变化
