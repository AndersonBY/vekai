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

## FP16 快路径上限

Hugging Face FP16 encoder 使用 SDPA、last-token pooling 和 L2 normalize：

| 条数 | PyTorch eager | `torch.compile` + Triton |
| ---: | ---: | ---: |
| 1 | 约 66–96 ms | 8.3 ms |
| 8 | 约 54–98 ms | 11.3 ms |
| 32 | 54.3 ms | 41.8 ms |
| 64 | 101.1 ms | 80.3 ms |

固定 shape CUDA Graph 的 64 条结果为 92.3 ms，比 eager 快约 13%；Inductor/Triton 自动融合后进一步降到 80.3 ms。Q8 llama.cpp 与 FP16 的 64 个向量逐条比较，余弦相似度最小 0.998890、平均 0.999251。

`torch.compile` 首次为不同 shape 生成内核需要数分钟，因此不能在桌面应用首次请求时现场编译。正式快路径必须使用离线构建并缓存的固定 bucket engine。

## Profiler 结论

Nsight Systems 对 64 条 eager FP16 热区采样：

- 51.9% GPU 时间是 Tensor Core FP16 GEMM。
- 5.6% 是 fused attention。
- 单次 forward 有 1741 次 kernel launch。
- pooling 和 normalize 只占极小比例。

因此不应先手写 pooling、normalize 或矩阵乘算子。剩余机会主要是跨层 elementwise、RMSNorm、激活和 launch 融合；Triton 已证明自动融合有效。只有在 TensorRT/ONNX 图融合后 profiler 仍显示某个未覆盖算子占比超过约 20% 时，才值得维护自定义 CUDA 算子。

## 已否定方案

- F16 GGUF：CUDA 下 32/64 条约 735/963 ms，明显慢于 Q8。
- 本地 Q4_K_M GGUF：32/64 条约 328/480 ms，慢于紧邻 Q8 对照的 302/413 ms。
- 强制 Flash Attention、`--no-host`、高优先级和高 polling 组合：没有稳定优于默认 CUDA 参数。
- 直接把 Python/PyTorch 打进安装包：启动、依赖体积和跨平台交付成本不符合当前产品边界。

## 下一阶段

生产级极限路径应按以下顺序推进：

1. 导出 Qwen3 Embedding FP16 ONNX，验证 ONNX Runtime CUDA 和 TensorRT 固定 batch/sequence bucket。
2. 预构建常用 batch 8/32/64 及 sequence 32/64/128/256/512 的 engine，禁止首次请求在线编译。
3. Rust HTTP 层按请求规模路由：小请求继续 llama.cpp，大批量进入 fused encoder；非 NVIDIA 和 engine 不可用时自动回退。
4. 除逐向量余弦外，增加真实检索集的 Recall@K/NDCG 回归，再决定默认阈值。
