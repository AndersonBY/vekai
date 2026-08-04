# 架构说明

Vekai 是一个桌面 GUI 与 CLI 服务双模式应用。它管理本机 `llama-server`/`llama-server.exe`，并在 Windows/Linux x64 NVIDIA 环境中可选管理 ONNX Runtime CUDA Embedding sidecar，对外保持统一的 Embedding 和 Rerank HTTP 服务。

## 产品定位与边界

Vekai 面向同一台电脑上的知识库、智能搜索、桌面助手和其他 LLM 应用，为它们提供可复用的本地 Embedding 与 Rerank 服务。上层应用通过 loopback HTTP 调用 Vekai，不需要分别集成和守护模型运行时。

Vekai 负责模型路径、运行时、硬件后端、进程生命周期、健康检查和协议适配。向量数据库、文档切分、索引、召回策略、Prompt 构造和完整 RAG 工作流属于调用方，不在 Vekai 的系统边界内。

## 系统边界

```text
AI / LLM / RAG applications       React Settings UI
              |                         |
              | local HTTP              | Tauri commands
              v                         v
Public Embedding/Rerank endpoints   Rust app state and config
              |                         |
              | native or proxy         +--> runtime archive lookup/extraction
              |                         +--> model path validation
              |                         +--> process supervision
              v                         +--> protocol proxy lifecycle
          llama-server or embedding fastpath sidecar
```

前端只负责配置、状态展示、测试按钮和用户交互。服务启动、进程管理、配置持久化、日志、安装包内置或本机 runtime archive 查找/解压和协议转换都由 Rust 侧负责。

## 主要模块

| 文件 | 职责 |
| --- | --- |
| `src/App.tsx` | 设置窗口 UI、中文/英文文案、服务控制按钮、模型文件选择、测试结果展示。 |
| `src/types.ts` | 前端与 Tauri 命令共享的 TypeScript 数据形状。 |
| `src/styles.css` | 设置窗口样式。 |
| `src-tauri/src/lib.rs` | Tauri 应用入口、托盘菜单、窗口生命周期、命令注册、退出清理。 |
| `src-tauri/src/config.rs` | 配置结构、默认值、协议白名单和配置校验。 |
| `src-tauri/src/commands.rs` | Tauri 命令实现、路径解析、启动/停止/保存/测试入口。 |
| `src-tauri/src/service.rs` | 服务枚举、默认端口、服务标识。 |
| `src-tauri/src/llama_args.rs` | 根据配置和模型路径生成 `llama-server` 参数。 |
| `src-tauri/src/cli.rs` | 解析 CLI 子命令并生成 Linux systemd unit 内容。 |
| `src-tauri/src/process.rs` | 子进程启动、健康检查、停止、崩溃重启和日志汇总。 |
| `src-tauri/src/fastpath.rs` | Windows/Linux x64 Embedding 快路径平台识别、manifest 快速校验和运行布局。 |
| `src-fastpath/` | 单 ORT CUDA session、动态 padding、微批和 OpenAI Embeddings HTTP sidecar。 |
| `src-tauri/src/runtime.rs` | 安装包内置或已下载 llama.cpp runtime archive 的查找、哈希、解压和运行时布局。 |
| `scripts/assets.json` | 默认 runtime/model 下载配置、模型大小和 SHA256。 |
| `scripts/prepare-bundled-runtime.ps1` | 打包前准备脚本，下载当前平台匹配的 latest llama.cpp runtime 到安装包资源暂存目录。 |
| `scripts/download-assets.ps1` | 资产下载脚本，可下载当前平台 runtime 和默认 GGUF 模型；用户通常只需用它下载模型。 |
| `scripts/download-fastpath.ps1` | 下载并校验 FP16 权重和当前平台固定版本的 ORT/CUDA/cuBLAS/cuFFT/cuRAND/cuDNN 动态库。 |
| `scripts/build-fastpath.ps1` | Windows/Linux x64 打包前构建原生 sidecar，并复制到 Tauri fastpath resource。 |
| `src-tauri/src/models.rs` | `.gguf` 模型发现、默认模型选择和展示数据。 |
| `src-tauri/src/protocol_adapter.rs` | 各协议到 llama.cpp 原生 JSON 的请求/响应转换。 |
| `src-tauri/src/protocol_proxy.rs` | 非原生协议的本地 HTTP 代理。 |
| `src-tauri/src/logging.rs` | 日志路径、写入和保留策略。 |
| `src-tauri/src/state.rs` | 运行状态快照、最近日志和错误状态。 |

## 配置模型

配置文件位置：

```text
%APPDATA%\Vekai\config.json
```

若 exe 同级存在 `config.json`，应用优先读取并保存该文件，用于绿色便携交付。便携配置中的相对 `model_path` 以 exe 所在目录为基准解析；普通安装未提供同级配置时仍使用上述用户配置目录。

Windows 便携交付还可在 exe 同级提供 `redist\vc_redist.x64.exe` 和 `redist\version.txt`。当三者与便携 `config.json` 同时存在时，GUI 启动会先检查 Microsoft Visual C++ 2015-2022 x64 Runtime 注册表版本；缺失或版本过旧时提示并启动包内安装器，跳过该次服务初始化。普通安装版和非 Windows 平台不执行该检查。

当前配置是以服务为单位组织的结构：

```json
{
  "host": "127.0.0.1",
  "rerank": {
    "enabled": true,
    "port": 8012,
    "model_path": "",
    "protocol": "llama_rerank",
    "parallel_slots": 0,
    "batch_size": 0,
    "ubatch_size": 0
  },
  "embedding": {
    "enabled": true,
    "port": 8013,
    "model_path": "",
    "protocol": "openai_embeddings",
    "parallel_slots": 0,
    "batch_size": 0,
    "ubatch_size": 0
  },
  "auto_start_server": true,
  "restart_on_crash": true,
  "gpu_layers": "auto",
  "threads": 0,
  "parallel_slots": 0,
  "batch_size": 0,
  "ubatch_size": 0,
  "http_threads": 0,
  "log_retention_days": 7
}
```

开发期不维护旧版顶层 Rerank 配置兼容。如果配置结构变化，直接同步文档和测试。

`threads` 映射到 `llama-server --threads`，`parallel_slots` 映射到 `--parallel`，`batch_size` 映射到 `--batch-size`，`ubatch_size` 映射到 `--ubatch-size`，`http_threads` 映射到 `--threads-http`。这些数字字段都使用 `0` 表示不传对应参数，由 llama.cpp 自动决定。每个服务内的同名 slot/batch 字段非零时覆盖通用设置。`gpu_layers` 设为 `all` 时同时传入 `--fit off`，尊重用户要求全部 GPU offload 的显式选择，并跳过自动显存拟合。

Embedding 和 Rerank 请求不复用生成式 KV 状态，因此启动参数固定包含 `--cache-ram 0`，避免 llama.cpp 的 8 GiB prompt cache 管理开销。运行时日志使用 `--log-verbosity 1`，只保留错误级输出；每个服务日志在启动前检查，超过 16 MiB 时截断并写入说明，防止高频推理长期写盘拖慢服务。

Windows x64 通用安装包默认内置纯 CPU runtime，避免客户机器因显卡或 Vulkan 驱动不兼容而启动崩溃。若下载目录中同时存在 `llama-b*-bin-win-cuda-12.4-x64.zip` 和 `cudart-llama-bin-win-cuda-12.4-x64.zip`，运行时解析会优先解压并使用 CUDA；只有一个 CUDA archive 时视为不完整并回退 CPU。仅在没有 CPU archive 时才使用 Vulkan compatibility fallback。CUDA、CPU 与 Vulkan 分别解压到独立目录。Marker 记录所有组成 archive 的文件名、大小和 SHA256；文件名与大小未变化时走快速复用路径，首次安装或元数据变化时才重新计算 SHA256。

## 运行模式

无子命令启动时进入 Tauri GUI。CLI 子命令包括：

```text
vekai serve
vekai install-service
vekai uninstall-service
```

`serve` 前台启动所有已启用服务并等待退出信号。Linux 上的 `install-service` 写入用户级 systemd unit 到 `~/.config/systemd/user/vekai.service` 并执行 `systemctl --user enable --now vekai.service`。

## 服务启动模型

每个服务都有自己的启用状态、端口、模型路径、协议和性能覆盖项。启用两个服务时先加载较大的 Embedding 模型，再加载 Rerank 模型，降低第二个模型分配 GPU 资源时的等待。启动时会：

1. 读取当前配置。
2. 确认服务已启用且模型路径存在。
3. 确保安装包内置或本机下载的 llama.cpp 运行时 archive 已解压。
4. Embedding 选择 Qwen3-Embedding-0.6B 时检查完整 fastpath manifest；不支持或不完整时继续 llama.cpp。
5. 根据协议决定是否需要本地代理，并启动 sidecar 或按配置生成参数后启动 `llama-server`。
6. 轮询 `/health`。
7. 更新状态和最近日志。

Rerank 和 Embedding 同时启用时不能使用同一个端口。
Rerank 使用独立 `llama-server`。Embedding 默认使用独立 `llama-server`；满足 Windows/Linux x64、模型名和当前平台完整资产条件时改用 sidecar。sidecar 使用单 ORT CUDA session、按 batch 最长文本动态 padding、最多 64 条分块和 750 微秒并发微批。CUDA provider 注册使用严格失败模式，不能静默回退 CPU 后继续报告 `ort_cuda`。Windows 通过 `PATH`、Linux 通过 `LD_LIBRARY_PATH` 注入私有 runtime 目录。sidecar 启动失败、健康超时或运行期退出时，supervisor 在同一端口自动切换到 llama.cpp。非原生协议仍复用同一个本地协议代理。

## 协议模型

原生协议直接暴露当前后端的 HTTP 服务：

| 服务 | 原生协议 | 默认端口 | 路径 |
| --- | --- | ---: | --- |
| Rerank | `llama_rerank` | `8012` | `/v1/rerank` |
| Embedding | `openai_embeddings` | `8013` | `/v1/embeddings` |

非原生协议使用本地协议代理。代理监听用户配置的公开端口，底层 llama.cpp 或 fastpath sidecar 运行在自动分配的内部 loopback 端口。

Rerank 协议：

| 协议 | 公开路径 | 代理 |
| --- | --- | --- |
| `llama_rerank` | `/v1/rerank` | 否 |
| `siliconflow` | `/rerank` | 是 |
| `cohere_rerank_v2` | `/v2/rerank` | 是 |
| `jina_rerank_v1` | `/v1/rerank` | 是 |
| `voyage_rerank_v1` | `/rerank` | 是 |
| `custom_json_http` | `/rerank` | 是 |

Embedding 协议：

| 协议 | 公开路径 | 代理 |
| --- | --- | --- |
| `openai_embeddings` | `/v1/embeddings` | 否 |
| `siliconflow` | `/embeddings` | 是 |
| `cohere_embed_v2` | `/v2/embed` | 是 |
| `voyage_embeddings_v1` | `/embeddings` | 是 |
| `custom_json_http` | `/embeddings` | 是 |

`custom_json_http` 是固定的本地 JSON 代理格式，不是用户可编辑的映射系统。

## 数据目录

| 内容 | 路径 |
| --- | --- |
| 用户配置 | `%APPDATA%\Vekai\config.json` |
| 本地数据根目录 | `%LOCALAPPDATA%\Vekai\` |
| 日志目录 | `%LOCALAPPDATA%\Vekai\logs` |
| 运行时解压目录 | `%LOCALAPPDATA%\Vekai\runtimes` |
| Windows Embedding 快路径模型、runtime 和 manifest | `%LOCALAPPDATA%\Vekai\fastpath` |
| Linux Embedding 快路径模型、runtime 和 manifest | `$XDG_DATA_HOME/Vekai/fastpath`，未设置时为 `~/.local/share/Vekai/fastpath` |
| 本机运行时下载目录 | `%LOCALAPPDATA%\Vekai\downloads` |
| 绿色版运行时下载目录 | exe 同级 `downloads\` |
| 安装包内置运行时目录 | Tauri resource 目录下的 `downloads/` |
| 仓库开发下载目录 | `downloads\`，仅开发调试路径 |

旧开发版本可能留下 `%LOCALAPPDATA%\LocalRerankServer\logs`。当前代码不应再写入旧目录。

## 设计约束

- Rust 侧是服务行为的事实来源；前端不要复制复杂业务规则。
- 在边界处解析和校验配置、协议和路径，不靠下游错误兜底。
- 协议兼容逻辑集中在 `protocol_adapter.rs` 和 `protocol_proxy.rs`。
- 进程生命周期集中在 `process.rs`，不要从 UI 或其他模块直接管理子进程。
- Windows 用户体验以托盘常驻为主，关闭设置窗口不应默认退出服务。
- 新增服务协议必须同步配置白名单、公开路径、请求转换、响应转换、测试和文档。
- 更新 runtime 或默认模型必须同步 `scripts/assets.json`、README 和发布文档。
