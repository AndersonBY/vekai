# 性能基线与加速路线

本文记录 2026-07-17 在开发机上的可复现实验和当前工程决策。数据用于比较后端，不能当作所有机器的固定承诺。

## 测试环境

- GPU：NVIDIA GeForce RTX 4070 Laptop GPU，8 GiB VRAM
- 驱动：610.74
- llama.cpp：b10046
- Embedding：Qwen3-Embedding-0.6B，默认 Q8_0 GGUF
- Rerank：bge-reranker-v2-m3 Q4_K_M GGUF
- HTTP 基准：约 30 token/条，包含 loopback HTTP 和 JSON 序列化，结果取 P50

实验脚本位于 `experiments/encoder_fastpath/` 和 `experiments/llama_backend/`。Python/PyTorch 只用于测性能上限，不属于正式安装包。

## llama.cpp 后端

Embedding 同模型、同参数、同请求对比：

| 条数 | Vulkan Q8 | CUDA Q8 | CUDA 加速比 |
| ---: | ---: | ---: | ---: |
| 1 | 9.6 ms | 9.0 ms | 1.1x |
| 8 | 55.0 ms | 24.1 ms | 2.3x |
| 32 | 791.6 ms | 343.2 ms | 2.3x |
| 64 | 1318.2 ms | 496.7 ms | 2.7x |

正式 release 同时加载 Embedding 和 Rerank 后，Embedding 32/64 条测得 304.0/417.5 ms。Vekai 因此在 Windows x64 检测到完整 CUDA 12.4 主包和 cudart companion 时优先 CUDA，资产不完整时回退 Vulkan。

Rerank 的 CUDA 收益较小：1/8/32 条约快 12%–21%，64 条复测为 CUDA 137.8 ms、Vulkan 138.8 ms，基本持平。

## 生产 ORT CUDA 快路径

正式 sidecar 使用公开 `onnx-community/Qwen3-Embedding-0.6B-ONNX` FP16 权重、last-token pooling 和 L2 normalize。瘦图只保留 `input_ids`、`attention_mask` 和 FP32 `embeddings`，权重继续作为外部数据文件。相对临时参考图的逐向量结果：

- 平均余弦相似度 `0.9999940`
- 最小余弦相似度 `0.9999892`
- 最大绝对误差 `0.0005550`
- 所有值有限

固定 32 token、直接 ORT CUDA P50：

| 条数 | 普通 CUDA | CUDA Graph |
| ---: | ---: | ---: |
| 1 | 12.9 ms | 12.2 ms |
| 8 | 16.0 ms | 15.9 ms |
| 32 | 42.2 ms | 43.5 ms |
| 64 | 82.1 ms | 83.6 ms |

CUDA Graph 在 32/64 条时没有收益，还要求固定地址 I/O，并使长文本需要第二个 session。最终生产实现因此使用一个普通 ORT CUDA session，并只 padding 到当前微批最长文本。短文本通过真实 HTTP、丢弃响应体的 `curl.exe` P50 为：

| 条数 | sidecar HTTP P50 |
| ---: | ---: |
| 1 | 23.2 ms |
| 8 | 31.9 ms |
| 32 | 52.6 ms |
| 64 | 63.0 ms |

这些 HTTP 数字包含 Rust tokenizer、750 微秒微批窗口、GPU 推理、FP32 JSON 序列化和 loopback 传输。单 session release 热启动约 `3.2 s`，本机显存增量约 `2188 MiB`；与 Rerank llama CUDA 同时启动时总增量约 `2607 MiB`。强制终止 sidecar 后，Embedding 在约 `2.50 s` 内自动切到 llama CUDA，同端口请求继续成功。

## Profiler 结论

Nsight Systems 对 64 条 eager FP16 热区采样：

- 51.9% GPU 时间是 Tensor Core FP16 GEMM。
- 5.6% 是 fused attention。
- 单次 forward 有 1741 次 kernel launch。
- pooling 和 normalize 只占极小比例。

因此不写自定义 GEMM、pooling 或 normalize 算子。Tensor Core GEMM 是主耗时，pooling/normalize 可忽略；自维护算子无法合理超过 cuBLAS 的 Tensor Core 路径，反而扩大驱动、架构和精度维护面。

## 已否定方案

- F16 GGUF：CUDA 下 32/64 条约 735/963 ms，明显慢于 Q8。
- 本地 Q4_K_M GGUF：32/64 条约 328/480 ms，慢于紧邻 Q8 对照的 302/413 ms。
- 强制 Flash Attention、`--no-host`、高优先级和高 polling 组合：没有稳定优于默认 CUDA 参数。
- 直接把 Python/PyTorch 打进安装包：启动、依赖体积和跨平台交付成本不符合当前产品边界。
- TensorRT 修复 FP32 LayerNorm 后 batch 32 为 `42.2 ms`，没有快于 ORT；首次加载约 `61.9 s`，engine `1.20 GB`，builder DLL `1.82 GB`，不进入产品。
- TensorRT 未启用 FP32 LayerNorm 时 batch 32 出现非有限值，不能以错误精度换速度。
- Candle CUDA 需要本机 `nvcc` 和 MSVC C++ toolchain，交付面明显大于动态加载 ORT DLL。
- CUDA Graph：固定 32 token 的 32/64 batch 略慢于普通 CUDA，双 session 显存约 `4.3 GiB`，已从生产实现移除。

## 资产和回退

`scripts/download-fastpath.ps1` 固定并校验 ONNX Runtime `1.23.0`、CUDA runtime `12.1.105`、cuBLAS `12.1.3.1`、cuFFT `11.0.2.54` 和 cuDNN `9.1.0.70`。只抽取进程实际需要的 10 个 DLL，不安装 Python/PyTorch。应用启动只校验 manifest 常量和大文件大小，并哈希 562 KiB 瘦图，避免每次启动扫描约 2.4 GB 资产。

快路径仅在 Windows x64、选择 Qwen3-Embedding-0.6B GGUF、manifest 完整且 sidecar 资源存在时启用。任何条件不满足都直接使用 llama.cpp；sidecar 启动失败、健康超时或运行期退出也会自动回退。
