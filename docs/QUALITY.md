# 质量与验证

本项目的主要风险来自 Windows 桌面生命周期、子进程管理、协议兼容和打包交付。验证应按风险选择，不追求每次都跑全量，但不能在没有证据时宣称完成。

## 基础命令

```powershell
git diff --check
powershell -ExecutionPolicy Bypass -File scripts\prepare-bundled-runtime.ps1
npm run verify:icons
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --release --locked --manifest-path src-fastpath/Cargo.toml
powershell -ExecutionPolicy Bypass -File scripts\build-fastpath.ps1
npm run tauri:build
```

`src-fastpath` 测试和构建仅适用于 Windows x64；其他平台跳过这两项。

## 分层验证

### 文档

- 运行 `git diff --check`。
- 检查新增链接是否存在。
- 检查产品名、端口、路径、协议名是否与代码一致。
- 检查大文件下载策略仍然说明为脚本下载，而不是 Git 跟踪。

### 前端

- 运行 `npm run build`。
- 检查中文默认文案和英文切换文案。
- 检查按钮禁用状态，尤其是服务运行时“启动服务”不能可点。
- 检查关闭窗口弹窗文案和动作。

### Rust 后端

- 运行 `cargo test --manifest-path src-tauri/Cargo.toml`。
- 配置变化必须覆盖默认值、校验和错误场景。
- 协议变化必须覆盖请求转换、响应转换和公开路径。
- 进程变化必须覆盖启动、停止、异常退出和重启路径。

### 打包和 Windows 行为

- 运行 `npm run tauri:build`。
- 检查 NSIS 安装包输出。
- 检查生成 exe 不应弹出终端窗口。
- 检查 `npm run verify:icons` 通过，打包 PNG 图标必须是 Tauri/macOS 可处理的 8-bit PNG。
- 检查打包前已执行 `scripts\prepare-bundled-runtime.ps1`，且安装包内置当前平台 runtime。
- 对用户可见生命周期变化，启动 release exe 做冒烟验证。

## 当前质量门槛

| 区域 | 必须保持 |
| --- | --- |
| 数据目录 | 当前写入 `Vekai`，不能回退到 `LocalRerankServer`。 |
| 端口 | Rerank 默认 `8012`，Embedding 默认 `8013`。 |
| UI 语言 | 默认中文，允许切换英文。 |
| 模型路径 | 用户通过文件选择器选择完整 `.gguf` 路径，不写死模型路径。 |
| 大资产 | llama.cpp runtime archive、`.gguf`、FP16 外部权重和 GPU DLL 不进 Git；fastpath 只提交瘦图和源码，sidecar exe 是打包前生成物。 |
| 托盘体验 | 关闭设置窗口应询问最小化到托盘还是退出。 |
| 原生协议 | Rerank `/v1/rerank`，Embedding `/v1/embeddings`。 |
| 非原生协议 | 通过本地代理转换，不直接暴露内部 llama.cpp 端口。 |

## 常见回归

- 新增配置字段但没有默认值或校验。
- 前端显示状态与 Rust 快照字段不一致。
- 服务运行中仍允许点击“启动服务”。
- 保存服务相关配置后没有重启运行中的服务。
- 打包后缺前端资源或包内 runtime archive。
- 新图标是 16-bit PNG，导致 macOS Tauri 打包失败。
- 把 `downloads\` 或 `models\*.gguf` 误提交进 Git。
- 把 fastpath wheel、外部权重、GPU DLL、sidecar exe 或任意 `target\` 目录误提交进 Git。
- ORT CUDA provider 静默回退 CPU，但健康检查仍被误判为 fastpath 可用。
- 日志写到旧目录。
- 协议下拉有选项，但 Rust 配置白名单不接受。
- 文档示例路径和实际公开路径不一致。

## 推荐测试覆盖

新增协议时至少覆盖：

- `supported_protocols` 接受新协议。
- 错误服务不接受该协议。
- `protocol_public_path` 返回正确公开路径。
- 请求体被转换为 llama.cpp 原生请求。
- llama.cpp 响应被转换为目标协议响应。

新增 Windows 行为时至少覆盖：

- 关闭窗口是否被拦截。
- 退出程序是否停止服务。
- 日志目录是否存在且可打开。
- 单实例或托盘动作是否仍能打开设置窗口。

## 不能替代验证的证据

- 只看到 TypeScript 没有红线。
- 只读了旧计划文档。
- 只在 dev server 中验证了需要 release exe 才能证明的行为。
- 只检查了文件名，没有检查 `tauri.conf.json` 或 Rust 常量。
