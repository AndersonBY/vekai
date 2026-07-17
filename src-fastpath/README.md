# Vekai Embedding Fast Path

This Windows/Linux x64 sidecar serves Qwen3-Embedding-0.6B through one ONNX Runtime CUDA session. It
pads each micro-batch only to its longest input and uses a sub-millisecond batching window for
concurrent requests. Vekai launches it only when the optional verified runtime and model assets are
present; otherwise the app uses llama.cpp.
