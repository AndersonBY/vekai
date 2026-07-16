# llama.cpp Backend Benchmark

This benchmark compares llama.cpp runtime backends through the same HTTP API and payloads.
Launch one `llama-server` at a time with identical model and tuning arguments, then run:

```powershell
python experiments\llama_backend\benchmark_server.py `
  --service embedding `
  --url http://127.0.0.1:18013/v1/embeddings
```

The reported latency includes local HTTP handling and JSON serialization.

`run_backend_benchmark.ps1` starts and stops one runtime safely around the same benchmark:

```powershell
pwsh -File experiments\llama_backend\run_backend_benchmark.ps1 `
  -Server C:\path\to\llama-server.exe `
  -Model models\Qwen3-Embedding-0.6B-Q8_0.gguf
```
