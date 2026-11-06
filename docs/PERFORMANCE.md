# 性能测试与后端选型

本文档记录在开发机上针对不同推理后端的实测数据与技术选型结论。

## Qwen3-VL-Reranker-2B 图文重排序（2026-11-06）

环境：Windows、i9-13900H、RTX 4070 Laptop 8 GiB（驱动 616.56）、llama.cpp b12110 CUDA 12.4。模型使用 `bealore/Qwen3-VL-Reranker-2B-GGUF` revision `5dc74c6bf8b65a8b47b1b0e1d6c0cbdba16c7f31` 的 Q8_0 主模型和 F16 投影。GGUF 实际元数据包含 `qwen3vl`、rank pooling、`yes/no` 标签和 `[2048, 2]` 分类权重。

重排参数：全 GPU、1 槽、上下文 8192、逻辑/物理批 2048、`--image-max-tokens 1280 --cache-ram 0`。每次一个查询与一个候选，首轮后连续 5 轮取 P50，客户端预先完成 Base64/JSON 编码，计时包含 HTTP 发送与读取响应、图像解码和推理，不包含模型加载。

| 场景 | 800 汉字候选 P50 | 2560×1440 图片候选 P50 | 文字范围 | 图片范围 |
| --- | ---: | ---: | ---: | ---: |
| 仅加载重排 | 62.5 ms | 1051 ms | 61.2–66.5 ms | 1023–1092 ms |
| 同时加载 EmbeddingGemma 2 | 72.6 ms | 1014 ms | 61.8–76.2 ms | 985–1037 ms |
| Vekai 0.1.13 原生接口，双模型常驻 | 55.3 ms | 863 ms | 54.6–57.7 ms | 831–907 ms |
| Vekai 0.1.13 Jina 代理，双模型常驻 | 73.4 ms | 926 ms | 64.9–89.2 ms | 902–939 ms |

Vekai 原生组重排批大小为 2048；Jina 组重排批大小未显式设置（llama.cpp 默认），两组均为 1 槽、同样的图片和查询。热状态及批大小不同，不能将两行差值全部归因于代理开销。实际应用两组均通过上述图文测试、超过 2 MiB 的三图片请求和响应不回传图片检查；Embedding 的文字和图片均返回有限数值的 768 维向量。

800 汉字含标点共 874 字符，查询为“本地知识检索系统”，合计 593 tokens。图片为 [Unsplash 摄影样本](https://images.unsplash.com/photo-1500530855697-b586d89ba3ee?w=2560&h=1440&fit=crop&q=85)，934444 字节，查询为 `A landscape photograph`，合计 1300 tokens。图片按 1280 视觉 token 预算缩放，结果不能代表任意图片或 OCR 精度。双服务测试中 Embedding 使用 8 槽、批大小 2048；两者均常驻，但请求顺序执行，未模拟两个服务同时满载。GPU 总占用约 6072 MiB，含桌面应用。

真实图文排序使用无说明文字的 [猫](https://raw.githubusercontent.com/scikit-image/scikit-image/v0.19.3/skimage/data/chelsea.png)、[狗](https://raw.githubusercontent.com/pytorch/hub/master/images/dog.jpg)、[宇航员](https://raw.githubusercontent.com/scikit-image/scikit-image/v0.19.3/skimage/data/astronaut.png) 图片。英文猫查询评分为猫 `0.5914`、狗 `0.4050`、宇航员 `0.2980`；中文狗查询和中文宇航员查询均命中相应图片。反转候选顺序仍命中猫。图片查询图片、图片查询文字、同一中性字幕配不同图片均正确，证明图片参与评分。这是小样本冒烟，未构成公开检索质量基准。

双服务下将重排提高到 8 槽并给每槽 8192 上下文，总 GPU 占用约 7798 MiB，三图片请求首轮耗时约 39.8 秒，出现明显显存压力。因此未显式设置时默认使用 1 个重排槽位，8GB GPU 的已有用户也应将该覆盖值调为 1。不同 GPU 和并发量需要重新测量。

### 安装包与 CPU 补充验证

通过 NSIS 安装 0.1.13 后，安装目录中的应用成功加载两个默认模型，文字/图片嵌入与全部上述图文重排用例通过。相同五轮预热测量中，800 汉字 P50 为 70.0 ms（57.2–79.1 ms），2K 图片 P50 为 853 ms（839–902 ms）。这些请求使用已安装应用管理的 CUDA 进程，重排 1 槽、批大小 2048。

另外使用 b12110 CPU 运行时，主模型 `--n-gpu-layers 0`、视觉编码器 `--no-mmproj-offload`，其余重排参数相同。800 汉字首次约 8121 ms，随后两次为 7404 / 7416 ms；2K 图片两次为 37116 / 40714 ms。中文猫查询在狗、猫两个图片候选中正确选中猫（评分约 0.4588 / 0.5807）。该组仅验证 CPU 可用性与延迟量级，样本数不足以报告稳定 P50；实际使用建议采用 GPU。

## EmbeddingGemma 2 图文嵌入（2026-11-06）

测试环境为 Windows、Intel Core i9-13900H（14 核 / 20 线程）、RTX 4070 Laptop GPU（8 GiB，驱动 616.56）、llama.cpp b12110。主模型和多模态编码器均为 `ggml-org/embeddinggemma-2-GGUF` Q8_0，输出 768 维向量。CUDA 组将主模型及编码器都加载至 GPU，CPU 组关闭两者的 GPU offload。

每次请求包含一条输入，无并发；首个请求完成后连续测量 7 次，取中位数。文字为 800 个汉字（含标点共 874 字符，533 tokens）；图片为一张 2560×1440 JPEG 摄影图片（934444 字节，编码后输入为 1012 tokens）。请求耗时从本地 HTTP 发送至读完响应，包含服务端图片解码、推理和响应序列化，不含模型加载、客户端文件读取与 Base64/JSON 编码。

| 后端 | 800 汉字预热后 P50 | 2K 图片预热后 P50 | 文字首次请求 | 图片首次请求 |
| --- | ---: | ---: | ---: | ---: |
| CPU | 931 ms | 28494 ms | 1027 ms | 29189 ms |
| CUDA | 39 ms | 858 ms | 272 ms | 1280 ms |

CPU 文字 7 次范围为 896–943 ms，图片为 27877–29603 ms；CUDA 文字为 22–47 ms，图片为 815–990 ms。图片结果来自同一张样本的重复请求，不能视为所有 2K 图片的统一性能。

测试设置 `--batch-size 2048 --ubatch-size 2048`。llama.cpp 默认物理批 512 会拒绝上述 533-token 文本；Vekai 从 0.1.11 起为未显式设置物理批的 EmbeddingGemma 2 自动使用有效逻辑批大小（默认 2048），显式配置仍优先生效。模型加载至 `/health` 就绪约 2.6–4.3 秒，未计入请求延迟。

## Qwen3 历史测试环境

- **GPU**：NVIDIA GeForce RTX 4070 Laptop GPU (8 GiB VRAM)，驱动 610.74
- **llama.cpp**：b10046
- **测试模型**：Embedding 使用 `Qwen3-Embedding-0.6B` (Q8_0 GGUF / FP16 ONNX)，Rerank 使用 `bge-reranker-v2-m3` (Q4_K_M GGUF)
- **测试负载**：每条约 30 tokens，通过本地 HTTP 接口测试端到端延迟（含 JSON 序列化与回环网络开销），取 P50

复现脚本位于 `experiments/encoder_fastpath/`、`experiments/onnx_fastpath/` 与 `experiments/llama_backend/`。

## llama.cpp 后端对比（Vulkan vs CUDA）

在相同模型、参数与请求负载下，Embedding（Qwen3-Embedding-0.6B Q8_0）端到端 P50 延迟对比：

| 批量大小 | Vulkan Q8 | CUDA Q8 | CUDA 加速比 |
| ---: | ---: | ---: | ---: |
| 1 | 9.6 ms | 9.0 ms | 1.1x |
| 8 | 55.0 ms | 24.1 ms | 2.3x |
| 32 | 791.6 ms | 343.2 ms | 2.3x |
| 64 | 1318.2 ms | 496.7 ms | 2.7x |

同时加载 Embedding 与 Rerank 两个模型后，CUDA Q8 在 32/64 条批量下实测为 `304.0 ms` / `417.5 ms`。因此在 Windows x64 下检测到完整的 CUDA 12.4 运行时与 `cudart` 依赖时，Vekai 默认优先启用 CUDA。

Rerank 模型在 CUDA 与 Vulkan 下的差距较小：1/8/32 条批量下 CUDA 快约 12%–21%，64 条批量下 CUDA（`137.8 ms`）与 Vulkan（`138.8 ms`）基本持平。

## ONNX Runtime CUDA 快路径

针对 `Qwen3-Embedding-0.6B`，`src-fastpath` 采用 `onnx-community/Qwen3-Embedding-0.6B-ONNX` FP16 权重，内置 last-token pooling 与 L2 normalize。裁剪后的结构图仅保留 `input_ids`、`attention_mask` 输入和 FP32 `embeddings` 输出。与参考实现逐向量对比：

- 平均余弦相似度：`0.9999940`（最小 `0.9999892`）
- 最大绝对误差：`0.0005550`

### 普通 CUDA Session vs CUDA Graph（固定 32 tokens，纯推理 P50）

| 批量大小 | 普通 CUDA | CUDA Graph |
| ---: | ---: | ---: |
| 1 | 12.9 ms | 12.2 ms |
| 8 | 16.0 ms | 15.9 ms |
| 32 | 42.2 ms | 43.5 ms |
| 64 | 82.1 ms | 83.6 ms |

CUDA Graph 在大批量下无明显收益，且要求固定输入形状（处理长文本需额外维护第二个 session，显存占用翻倍至约 4.3 GiB）。因此生产环境直接采用单个动态形状的 ORT CUDA session，每次仅按当前微批内的最长文本做动态 padding。

### Sidecar 端到端 HTTP 延迟（含 Rust 分词、750µs 微批聚合与 JSON 序列化）

| 批量大小 | Windows x64 本机 HTTP P50 | Linux x64 Docker HTTP P50 |
| ---: | ---: | ---: |
| 1 | 23.2 ms | 28.8 ms |
| 8 | 31.9 ms | 40.7 ms |
| 32 | 52.6 ms | 68.4 ms |
| 64 | 63.0 ms | 128.1 ms |

**资源占用与启动耗时：**
- Windows 单 session 启动并完成预热约 `3.2 s`，显存占用约 `2188 MiB`（与 Rerank llama CUDA 同时运行时合计约 `2607 MiB`）。
- Linux Docker 环境下从 ORT 初始化到预热完成约 `3.87 s`。
- 当强制终止 sidecar 进程时，Vekai 可在约 `2.5 s` 内将同一端口自动切换回 `llama.cpp` CUDA 后端继续服务。

## 性能剖析与备选方案评估

使用 Nsight Systems 对 64 条批量 FP16 推理过程采样，51.9% 的 GPU 时间消耗在 Tensor Core FP16 GEMM，5.6% 在 fused attention，pooling 与 normalize 占比极小。因此无需手写自定义 CUDA 算子，直接复用 cuBLAS Tensor Core 路径即可。

其他评估过的方案记录：

- **F16 GGUF（llama.cpp CUDA）**：32/64 条耗时约 735 / 963 ms，显著慢于 Q8_0。
- **Q4_K_M GGUF（llama.cpp CUDA）**：32/64 条耗时约 328 / 480 ms，略慢于同条件下的 Q8_0（302 / 413 ms）。
- **TensorRT**：开启 FP32 LayerNorm 保证数值稳定后，32 条批量耗时 `42.2 ms`，与 ORT CUDA 相当，但首次构建耗时约 62 秒且运行时依赖体积过大（engine 1.20 GB + builder DLL 1.82 GB），因此未采用。
- **Candle CUDA**：依赖本地 `nvcc` 与 C++ 编译工具链，不适合直接分发给终端用户。
