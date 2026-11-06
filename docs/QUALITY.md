# 测试与校验

本文档列出 Vekai 本地开发和打包前的检查命令与测试重点。

## 常用构建与检查命令

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

其中 `src-fastpath` 的测试与构建适用于 Windows x64 和 Linux x64 平台；其他架构可直接运行 `npm run tauri:build`。

## 模块测试重点

### 前端与 UI

- 运行 `npm run build` 确保 TypeScript 类型检查与前端打包通过。
- 检查中英文界面切换文案是否完整。
- 确认服务运行期间“启动服务”按钮处于禁用状态，且修改保存运行中的服务配置后会触发重启。
- 确认关闭主窗口时弹出“最小化到托盘”或“退出”确认框。

### Rust 后端

- 运行 `cargo test --manifest-path src-tauri/Cargo.toml`。
- **配置变更**：覆盖默认值、字段范围校验及异常输入。
- **EmbeddingGemma 2**：验证配套 `mmproj` 自动加载和缺失错误、物理批默认跟随逻辑批、显式性能配置优先级，以及纯 CPU 模式下编码器不进行 GPU offload；实际请求至少覆盖长文本和图片。
- **协议变更**：覆盖 `supported_protocols` 白名单、`protocol_public_path` 公开路径映射以及请求/响应 JSON 转换。
- **进程管理**：覆盖正常启停、健康检查超时、异常退出自动重启以及 Fastpath 到 `llama.cpp` 的自动回退。

### 安装包与桌面端冒烟

- 运行 `npm run verify:icons` 确保应用图标为符合 Tauri / macOS 要求的 8-bit PNG。
- 打包前执行 `scripts\prepare-bundled-runtime.ps1`，确保 `bundle-resources\downloads\` 下包含当前平台的 `llama.cpp` 运行时归档。
- Windows 下启动构建出的 `release` 可执行文件，确认不会弹出额外的控制台黑框，且系统托盘、日志目录打开、单实例唤醒均工作正常。

## 仓库资产约定

- **大文件不入库**：`llama.cpp` 运行时压缩包、`.gguf` 模型文件、ONNX FP16 外部权重、CUDA/cuDNN 动态库、构建产物（`target/`、`release/`）以及原生 sidecar 二进制均通过脚本下载或构建生成，不提交至 Git。
- **Fastpath 源码与瘦图**：仓库仅跟踪 `src-fastpath/` 源码与 562 KiB 的 ONNX 结构图（`src-tauri/resources/fastpath/qwen3-embedding-0.6b-fp16.onnx`）。
