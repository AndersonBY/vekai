# Encoder Fast-Path Experiment

This experiment measures a dedicated FP16 CUDA encoder against Vekai's llama.cpp GGUF backend.
It is not part of the shipped runtime.

```powershell
$python = "$env:LOCALAPPDATA\Vekai\experiments\encoder-fastpath\.venv\Scripts\python.exe"
& $python experiments\encoder_fastpath\benchmark_embedding.py
```

The benchmark uses Hugging Face `Qwen/Qwen3-Embedding-0.6B`, SDPA attention, last-token pooling,
L2 normalization, and explicit CUDA synchronization around each sample.
On Windows, `--compile` applies a process-local `os.replace` workaround for PyTorch 2.5's
Inductor cache writer; it does not modify the installed PyTorch files.

For PyTorch 2.5.1 on Windows, install the matching Triton build before using `--compile`:

```powershell
& $python -m pip install triton-windows==3.1.0.post17
```

Compare eager execution with a fixed-shape CUDA Graph:

```powershell
& $python experiments\encoder_fastpath\benchmark_embedding.py `
  --counts 64 --batch-sizes 64 --pad-to-length 64
& $python experiments\encoder_fastpath\benchmark_embedding.py `
  --counts 64 --batch-sizes 64 --pad-to-length 64 --cuda-graph
```

Capture only the timed inference region with Nsight Systems:

```powershell
$nsys = 'C:\Program Files\NVIDIA Corporation\Nsight Systems 2023.1.2\target-windows-x64\nsys.exe'
& $nsys profile --trace=cuda,nvtx --capture-range=cudaProfilerApi `
  --capture-range-end=stop --output=encoder-eager `
  $python experiments\encoder_fastpath\benchmark_embedding.py `
  --counts 64 --batch-sizes 64 --pad-to-length 64 --warmup 3 --samples 1 --cuda-profile
```

`compare_embeddings.py` compares dumped FP16 vectors with a llama.cpp HTTP response by cosine
similarity before a new backend is considered compatible.
