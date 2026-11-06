# Embedding Runtime 库与离线资产包

除了作为独立的 HTTP sidecar 运行外，`src-fastpath` 还提供可供宿主程序直接调用的 Rust 库接口（`vekai_embedding_fastpath`）。

## Rust 库调用方式

```rust
use vekai_embedding_fastpath::{EmbeddingRuntime, EmbeddingRuntimeConfig};

let runtime = EmbeddingRuntime::start(EmbeddingRuntimeConfig {
    model: model_path,
    tokenizer: tokenizer_path,
    ort_library: ort_library_path,
    max_sequence_length: 8192,
})?;
let vectors = runtime.embed(&texts)?;
```

调用方通过 `EmbeddingRuntimeConfig` 传入模型、分词器与 ONNX Runtime 动态库的绝对路径。该库本身不读取 Vekai 的配置文件或数据目录，便于集成到其他桌面或服务端应用中。

在 Windows 上，`EmbeddingRuntime` 会在生命周期内将指定的 ONNX Runtime 目录注册到系统 DLL 搜索路径，因此宿主进程无需修改全局 `PATH` 环境变量即可加载私有的 CUDA / cuDNN 依赖库。可通过 `EmbeddingRuntime::health` 查询当前生效的后端、输出向量维度和最大序列长度。

## 构建 Reqbase 离线资产包

在已安装 Vekai Fastpath 资产的 Windows x64 机器上，可通过以下脚本将模型与运行时打包为供 Reqbase 使用的离线资产包：

```powershell
pwsh.exe -NoLogo -NoProfile -File scripts\build-reqbase-ai-assets.ps1
```

该脚本会读取并校验 `%LOCALAPPDATA%\Vekai\fastpath\manifest.json`（要求 schema version 2），将 ONNX 结构图、FP16 外部权重、分词器以及 ONNX Runtime / CUDA / cuDNN 动态库打包至 `artifacts\` 下的时间戳目录，并生成对应的 `.zip` 压缩包、`.sha256` 校验文件以及符合 `reqbase.ai-assets.v1` 规范的 `manifest.json`。
