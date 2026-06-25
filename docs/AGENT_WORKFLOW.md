# 智能体工作流

本仓库按“智能体可读”原则维护。目标是让 Codex 或其他智能体从仓库内文档、代码和测试直接推理当前事实，而不是依赖聊天记录。

## 工作入口

每个任务先读：

1. [../AGENTS.md](../AGENTS.md)
2. 与任务相关的文档：
   - 架构或协议： [ARCHITECTURE.md](ARCHITECTURE.md)
   - 验证策略： [QUALITY.md](QUALITY.md)
   - 打包交付： [RELEASE.md](RELEASE.md)
3. 当前代码文件，而不是只读历史计划。

`docs/superpowers/specs/` 和 `docs/superpowers/plans/` 是设计历史。它们能解释为什么这么做，但当前行为必须以代码和当前文档为准。

## 常规循环

1. 明确用户目标和最新指令。
2. 检查工作区状态，识别已有未提交改动。
3. 搜索相关代码和文档。
4. 做最小范围改动。
5. 运行与改动风险匹配的验证命令。
6. 汇报改动文件、验证结果和未验证风险。

## 上下文获取

优先使用：

```powershell
rg "keyword"
rg --files
git status --short
git diff -- path\to\file
```

不要在没有读取代码的情况下根据文件名推断行为。尤其是协议路径、数据目录、端口、配置默认值和打包输出，必须查当前代码或当前 README。

## 修改边界

- 文档任务只改文档，除非发现文档无法描述当前代码。
- 协议任务集中修改 `config.rs`、`protocol_adapter.rs`、`protocol_proxy.rs` 和对应测试。
- UI 设置任务集中修改 `src/App.tsx`、`src/styles.css`、`src/types.ts`。
- Windows 生命周期任务集中修改 `src-tauri/src/lib.rs`、`process.rs`、`commands.rs`。
- 打包任务要检查 `src-tauri/tauri.conf.json`、`src-tauri/src/runtime.rs` 和 release 输出。

## 验证选择

| 改动类型 | 最低验证 |
| --- | --- |
| Markdown 文档 | `git diff --check`，并人工读回关键段落。 |
| Rust 配置、协议、进程逻辑 | `cargo test --manifest-path src-tauri/Cargo.toml`。 |
| 前端 TypeScript 或样式 | `npm run build`。 |
| Tauri 窗口、托盘、运行时、打包 | `npm run tauri:build`，必要时启动 exe 冒烟。 |
| 协议请求/响应 | Rust 单元测试覆盖转换逻辑，并用 UI 测试按钮或 curl 冒烟。 |

## 文档同步

以下变化必须同步文档：

- 端口、路径、协议名、响应格式
- 配置字段和默认值
- 日志、配置、运行时目录
- 打包文件名、安装包路径、交付目录
- 用户可见交互，比如关闭窗口、托盘菜单、语言默认值

优先更新：

- 用户使用说明：`README_ZH.md`、`README.md`
- 智能体事实：`docs/ARCHITECTURE.md`
- 验证规则：`docs/QUALITY.md`
- 发布交付：`docs/RELEASE.md`

## 交付报告

最终回复保持简洁，至少包含：

- 做了什么
- 改了哪些关键文件
- 跑了哪些验证
- 哪些验证没有跑，以及原因

如果工作区已有无关改动，不要声称工作区干净。只说明本次改动范围。
