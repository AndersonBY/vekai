# Embedding Runtime Library

`src-fastpath` exposes a host-callable Rust library in addition to the existing
HTTP sidecar binary.

The library entry points are:

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

The host must inject absolute paths for the ONNX model, tokenizer, and ONNX
Runtime library. The library does not discover models, read Vekai configuration,
or use Vekai data directories. This keeps embedding lifecycle and asset
ownership under the host application.

On Windows, the library registers the injected ONNX Runtime library directory
with the system DLL search API for the lifetime of `EmbeddingRuntime`. CUDA,
cuDNN, and provider dependencies therefore remain private to the declared
runtime directory; the host does not need to modify its process `PATH`.

`EmbeddingRuntime::health` reports the active backend, output dimension, and
maximum sequence length. Embeddings are derived data and must not be treated as
business decisions or authoritative records.

## Reqbase offline asset package

On Windows x64, an existing Vekai fastpath installation can be converted into
an offline asset package for Reqbase:

```powershell
pwsh.exe -NoLogo -NoProfile -File scripts\build-reqbase-ai-assets.ps1
```

The script reads `%LOCALAPPDATA%\Vekai\fastpath\manifest.json`, requires schema
version 2, verifies every declared source file, and includes the ONNX graph,
external weights, tokenizer, ONNX Runtime, CUDA, and cuDNN libraries. It creates
a new timestamped directory under `artifacts\`, plus a ZIP and a `.sha256`
file. Existing output directories and archives are never overwritten.

The generated `manifest.json` uses `reqbase.ai-assets.v1`. It declares every
file with its size and SHA-256, and carries the combined model asset hash used
by the host-callable embedding runtime. Model and runtime binaries remain
delivery artifacts and must not be committed to Git.
