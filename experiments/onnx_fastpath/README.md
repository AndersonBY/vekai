# ONNX Fast-Path Experiment

This experiment exports the FP16 Qwen3 Embedding encoder and measures ONNX Runtime CUDA through
the same pooling and normalization path as the PyTorch ceiling benchmark. Generated ONNX files
live under `%LOCALAPPDATA%\Vekai\experiments` and are not repository assets.

```powershell
$python = "$env:LOCALAPPDATA\Vekai\experiments\encoder-fastpath\.venv\Scripts\python.exe"
$root = "$env:LOCALAPPDATA\Vekai\experiments\onnx-fastpath"
& $python experiments\onnx_fastpath\export_embedding.py `
  --output "$root\qwen3-embedding-fp16.onnx"
& $python experiments\onnx_fastpath\benchmark_embedding.py `
  --model "$root\qwen3-embedding-fp16.onnx"
```

`prepare_community_model.py` prunes the pinned public ONNX revision to Vekai's two-input,
one-output contract. The generated 562 KiB graph is the only model artifact tracked in Git; the
1.2 GB external weight file remains downloaded data.

Production comparison on the public graph showed that normal ORT CUDA matches or beats CUDA Graph
at batch 32/64 while using one session instead of two. The shipped sidecar therefore uses dynamic
CUDA execution and pads each micro-batch only to its longest input.
