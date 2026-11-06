# 架构说明

Vekai 是一个支持桌面 GUI 与 CLI 服务双模式的应用。它在本地管理 `llama-server` 进程（以及在 Windows/Linux x64 NVIDIA 环境下可选的 ONNX Runtime CUDA sidecar），对外提供统一的 Embedding 和 Rerank HTTP 接口。

## 系统概览

Vekai 负责本地模型路径解析、运行时解压、硬件后端选择、子进程守护、健康检查与多协议适配，供本机的知识库、智能搜索或桌面 AI 应用通过 HTTP 直接调用。

```text
AI / LLM / RAG Applications       React Settings UI
              |                         |
              | local HTTP              | Tauri commands
              v                         v
Public Embedding/Rerank endpoints   Rust app state and config
              |                         |
              | native or proxy         +--> runtime archive lookup & extraction
              |                         +--> model path validation
              |                         +--> process supervision
              v                         +--> protocol proxy lifecycle
          llama-server or embedding fastpath sidecar
```

前端负责配置表单、运行状态展示、接口连通性测试与中英文切换；服务生命周期、配置持久化、日志轮转、运行时管理与协议代理均在 Rust 后端完成。

## 模块职责

| 文件 / 目录 | 职责说明 |
| --- | --- |
| `src/App.tsx` | 设置窗口 UI、中英文文案、服务启停控制、模型选择与测试结果展示 |
| `src/types.ts` | 前端与 Tauri 命令共享的 TypeScript 类型定义 |
| `src/styles.css` | 设置窗口样式 |
| `src-tauri/src/lib.rs` | Tauri 应用入口、系统托盘菜单、窗口事件处理、命令注册与退出清理 |
| `src-tauri/src/config.rs` | 配置数据结构、默认值、协议白名单与参数校验 |
| `src-tauri/src/commands.rs` | Tauri 命令实现、路径解析及启停/保存/测试调用入口 |
| `src-tauri/src/service.rs` | 服务类型定义、默认端口与标识 |
| `src-tauri/src/llama_args.rs` | 根据服务配置与模型特性拼装 `llama-server` 启动参数 |
| `src-tauri/src/cli.rs` | CLI 子命令解析与 Linux `systemd` 用户服务配置生成 |
| `src-tauri/src/process.rs` | 子进程拉起、`/health` 轮询、进程终止、崩溃自动重启与日志收集 |
| `src-tauri/src/fastpath.rs` | Windows/Linux x64 Embedding 快路径环境检测、manifest 校验与运行目录布局 |
| `src-fastpath/` | 基于 ONNX Runtime CUDA 的 Embedding HTTP sidecar 与可复用推理库 |
| `src-tauri/src/runtime.rs` | `llama.cpp` 运行时压缩包查找、校验、解压与后端优先级选择 |
| `src-tauri/src/models.rs` | 本地 `.gguf` 模型扫描、默认模型匹配与元数据读取 |
| `src-tauri/src/protocol_adapter.rs` | 第三方协议与 `llama.cpp` 原生 JSON 格式的双向转换 |
| `src-tauri/src/protocol_proxy.rs` | 非原生协议的本地 HTTP 转发代理 |
| `src-tauri/src/logging.rs` | 日志文件路径管理、写入与过期清理 |
| `src-tauri/src/state.rs` | 运行时状态快照、最近日志缓存与错误记录 |
| `scripts/assets.json` | 默认模型与运行时的下载源、文件大小及 SHA256 清单 |
| `scripts/prepare-bundled-runtime.ps1` | 打包前拉取当前平台匹配的 `llama.cpp` 运行时到安装包资源目录 |
| `scripts/download-assets.ps1` | 下载当前平台运行时与默认 GGUF 模型 |
| `scripts/download-fastpath.ps1` | 下载并校验 Qwen3-Embedding-0.6B FP16 权重及固定版本的 ORT/CUDA/cuDNN 动态库 |
| `scripts/build-fastpath.ps1` | 构建 Windows/Linux x64 原生 sidecar 并复制到 Tauri 资源目录 |

## 配置结构

默认配置文件路径为 `%APPDATA%\Vekai\config.json`。

当 `exe` 同级目录存在 `config.json` 时，应用自动进入便携模式，优先读写该文件，并将配置中的相对 `model_path` 按 `exe` 所在目录解析。若便携包同时包含 `redist\vc_redist.x64.exe` 和 `redist\version.txt`，Windows GUI 启动时会先检查系统是否已安装满足版本要求的 Microsoft Visual C++ 2015-2022 x64 Runtime；若未安装或版本偏低，则会提示并拉起内置安装器。

配置文件结构如下：

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

### 参数映射与运行时选择

- `threads`、`parallel_slots`、`batch_size`、`ubatch_size`、`http_threads` 分别对应 `llama-server` 的 `--threads`、`--parallel`、`--batch-size`、`--ubatch-size` 和 `--threads-http`。值为 `0` 时不传该参数，交由 `llama.cpp` 自动决定；单服务下的非零配置会覆盖全局同名配置。
- Qwen3-VL-Reranker-2B Q8_0 的并发未设置时使用 1 个槽位，每槽预留 8192 tokens 上下文（总 `--ctx-size = 8192 × 槽位数`），并附加 `--image-max-tokens 1280` 控制图片编码开销。服务级和全局显式并发仍优先生效。8GB GPU 同时运行两个默认模型时建议重排并发为 1；增加并发会增加 KV 缓存显存占用。
- EmbeddingGemma 2 采用非因果注意力，单条输入必须完整放进一个物理批。未显式设置 `ubatch_size` 时，该模型使用有效的 `batch_size`（未设置时为 `2048`），避免 llama.cpp 默认物理批 `512` 导致普通长文本报错。显式设置的服务级或全局物理批仍优先生效；更长输入需要同时提高逻辑批和物理批大小。
- EmbeddingGemma 2 显式设置并发槽位时，实际物理批大小（逻辑批与物理批的较小值）必须能被槽位数整除，否则 llama.cpp 的 mean pooling 初始化可能触发断言。Vekai 启动前拒绝这种参数组合并给出提示；例如并发 `8`、逻辑批 `2048`、物理批 `2048` 可用，并发 `12` 配合上述批大小不可用。
- `gpu_layers` 设为 `all` 时会同时传入 `--fit off`，跳过自动显存拟合，强制将全部层加载至 GPU。
- 启动参数固定附加 `--cache-ram 0`，禁用额外的主机提示缓存。Qwen3-VL 因果重排在推理时仍需要 KV 缓存，其显存占用随总上下文及槽位数增加。
- 子进程日志固定附加 `--log-verbosity 1`（仅记录错误级输出）；每次启动前若单个日志文件超过 16 MiB 会自动截断。
- 在 Windows x64 上，通用安装包默认内置 CPU 运行时以保证兼容性；若下载目录中同时存在完整的 CUDA 12.4 主包与 `cudart` 依赖包，则优先解压并使用 CUDA 运行时，其次为 CPU，最后为 Vulkan。解压目录通过记录文件名、大小与 SHA256 的 marker 文件判断是否需要更新。
- 启用 EmbeddingGemma 2 时，共用运行时要求至少 b11457。选择归档前先按构建号过滤不兼容版本，避免本机旧 CUDA 缓存覆盖包内新版 CPU / CUDA 运行时；各后端仍按上述顺序选择。目录内按数字构建号选择最新归档；没有兼容版本时返回明确的运行时更新命令。

## 运行模式与启动流程

不带参数启动时进入 Tauri 桌面界面；命令行支持以下子命令：

```text
vekai serve
vekai install-service
vekai uninstall-service
```

- `vekai serve`：在前台启动所有已启用的服务并监听退出信号。
- `vekai install-service`（Linux）：生成 `~/.config/systemd/user/vekai.service` 并通过 `systemctl --user enable --now vekai.service` 启动服务。

### 服务启动顺序

Rerank 与 Embedding 不能共用同一端口。当两个服务同时启用时，会先启动 Embedding 模型，再启动 Rerank 模型。单次启动流程如下：

1. 读取并校验配置与模型文件。
2. 查找并按需解压 `llama.cpp` 运行时归档。
3. 若 Embedding 选择了 `Qwen3-Embedding-0.6B` 且当前环境（Windows/Linux x64）具备完整的 fastpath 资产，则优先使用 ONNX Runtime CUDA sidecar；否则使用 `llama-server`。
4. 若所选协议为非原生协议，先在公开端口启动本地协议代理，并将模型进程绑定到内部随机回环端口。
5. 轮询 `/health` 直至就绪，更新运行状态快照。

### EmbeddingGemma 2 多模态支持与 Fastpath 回退

- **Qwen3-VL-Reranker-2B**：默认主模型 `Qwen3-VL-Reranker-2B.Q8_0.gguf` 自动加载同目录的 `Qwen3-VL-Reranker-2B.mmproj-f16.gguf`。该转换包含 rank pooling、yes/no 标签与 `[2048, 2]` 分类头，不能用普通生成模型替代。要求经过验证的 llama.cpp b12110 及以上；启用时与 Embedding 要求取最大构建号。CPU 模式同样禁用视觉编码器 GPU offload，缺少投影文件会在启动前报错。模型列表过滤 `mmproj-` 前缀和 `.mmproj-` 文件。

- **EmbeddingGemma 2**：默认主模型 `embeddinggemma-2-Q8_0.gguf` 需要与多模态编码器 `mmproj-embeddinggemma-2-Q8_0.gguf` 放在同一目录（要求 `llama.cpp` b11457 及以上）。启动时会自动附加 `--mmproj` 参数；当 `gpu_layers` 为 `0` 时同步附加 `--no-mmproj-offload`。模型扫描列表会自动过滤 `mmproj-` 前缀文件，避免误选。
- **Fastpath Sidecar**：采用单 ORT CUDA session，按当前微批最长文本动态 padding（单批上限 64 条，并发聚合窗口 750 微秒）。Windows 通过 `PATH`、Linux 通过 `LD_LIBRARY_PATH` 加载私有运行时目录。若 sidecar 启动失败、健康检查超时或运行期异常退出，进程管理器会在同一端口回退至 `llama-server`。

## 协议适配

原生协议直接由底层推理进程对外提供服务，非原生协议通过内置 HTTP 代理完成路径与 JSON 结构转换。

**原生协议默认端点：**

| 服务 | 原生协议 | 默认端口 | 路径 |
| --- | --- | ---: | --- |
| Rerank | `llama_rerank` | `8012` | `/v1/rerank` |
| Embedding | `openai_embeddings` | `8013` | `/v1/embeddings` |

**Rerank 支持的协议：**

| 协议标识 | 公开路径 | 是否经过代理 |
| --- | --- | --- |
| `llama_rerank` | `/v1/rerank` | 否 |
| `siliconflow` | `/rerank` | 是 |
| `cohere_rerank_v2` | `/v2/rerank` | 是 |
| `jina_rerank_v1` | `/v1/rerank` | 是 |
| `voyage_rerank_v1` | `/rerank` | 是 |
| `custom_json_http` | `/rerank` | 是 |

**Embedding 支持的协议：**

| 协议标识 | 公开路径 | 是否经过代理 |
| --- | --- | --- |
| `openai_embeddings` | `/v1/embeddings` | 否 |
| `siliconflow` | `/embeddings` | 是 |
| `cohere_embed_v2` | `/v2/embed` | 是 |
| `voyage_embeddings_v1` | `/embeddings` | 是 |
| `custom_json_http` | `/embeddings` | 是 |

在默认的 `openai_embeddings` 协议下，EmbeddingGemma 2 的文本和图片嵌入共享 768 维向量空间，支持通过 `input: [{"content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}]}]` 传入图片或图文混合内容。

Qwen3-VL 重排的 `query` 和各 `documents` 元素支持字符串或 `{ "text": "说明", "image": "data:image/png;base64,..." }`，也可只含图片。代理保留图片字段和文字数组，响应仅保留文档文字、索引和评分，避免回传 base64。非原生协议代理请求体上限为 32 MiB；这属于本地扩展，客户端需支持构造对应字段。原生协议由 llama.cpp 自行处理请求大小及响应。

## 数据目录汇总

| 用途 | 路径 |
| --- | --- |
| 用户配置文件 | `%APPDATA%\Vekai\config.json`（便携模式为 `<exe-dir>\config.json`） |
| 本地数据根目录 | `%LOCALAPPDATA%\Vekai\` |
| 服务日志目录 | `%LOCALAPPDATA%\Vekai\logs` |
| 运行时解压目录 | `%LOCALAPPDATA%\Vekai\runtimes` |
| Embedding 快路径资产（Windows） | `%LOCALAPPDATA%\Vekai\fastpath` |
| Embedding 快路径资产（Linux） | `$XDG_DATA_HOME/Vekai/fastpath`（默认 `~/.local/share/Vekai/fastpath`） |
| 运行时查找顺序 | `%LOCALAPPDATA%\Vekai\downloads` → `<exe-dir>\downloads` → `<bundle-resource-dir>\downloads` → `<repo>\downloads` |
