use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MODEL_REVISION: &str = "c25a394dd583836952667c12f008335071b3f43d";
const ONNXRUNTIME_VERSION: &str = "1.23.0";
const CUDA_RUNTIME_VERSION: &str = "12.1.105";
const CUBLAS_VERSION: &str = "12.1.3.1";
const CUFFT_VERSION: &str = "11.0.2.54";
const CUDNN_VERSION: &str = "9.1.0.70";

const GRAPH_FILE_NAME: &str = "qwen3-embedding-0.6b-fp16.onnx";
const GRAPH_SIZE: u64 = 575_769;
const GRAPH_SHA256: &str = "40bb4922e22c9d9fdfef69fe71db29be0b41715e88d9442a902df2afaef6fae4";
const SIDECAR_FILE_NAME: &str = "vekai-embedding-fastpath.exe";

const WEIGHTS_PATH: &str = "model/model_fp16.onnx_data";
const WEIGHTS_SIZE: u64 = 1_199_927_296;
const WEIGHTS_SHA256: &str = "06fbfded30166e3a33fe5252d7002ed1c370a4ec1bf45aba778cdf6cf7d31439";
const TOKENIZER_PATH: &str = "model/tokenizer.json";
const TOKENIZER_SIZE: u64 = 11_423_705;
const TOKENIZER_SHA256: &str = "def76fb086971c7867b829c23a26261e38d9d74e02139253b38aeb9df8b4b50a";

const REQUIRED_RUNTIME_FILES: [&str; 10] = [
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
    "onnxruntime_providers_cuda.dll",
    "cudart64_12.dll",
    "cublas64_12.dll",
    "cublasLt64_12.dll",
    "cufft64_11.dll",
    "cudnn64_9.dll",
    "cudnn_graph64_9.dll",
    "cudnn_ops64_9.dll",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastPathLayout {
    pub server_exe: PathBuf,
    pub runtime_dir: PathBuf,
    pub model_path: PathBuf,
    pub tokenizer_path: PathBuf,
    pub ort_dll: PathBuf,
}

#[derive(Debug, Deserialize)]
struct FastPathManifest {
    schema_version: u32,
    model_revision: String,
    onnxruntime_version: String,
    cuda_runtime_version: String,
    cublas_version: String,
    cufft_version: String,
    cudnn_version: String,
    weights: ManifestFile,
    tokenizer: ManifestFile,
    runtime_files: Vec<ManifestFile>,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

pub fn discover_embedding_fastpath(
    local_data_dir: &Path,
    exe_dir: &Path,
    download_dirs: &[PathBuf],
    selected_model: &Path,
) -> anyhow::Result<Option<FastPathLayout>> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64"))
        || !is_supported_embedding_model(selected_model)
    {
        return Ok(None);
    }

    let root = local_data_dir.join("fastpath");
    let manifest_path = root.join("manifest.json");
    let manifest_contents = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "embedding fast path manifest is missing; run scripts\\download-fastpath.ps1 ({})",
            manifest_path.display()
        )
    })?;
    let manifest: FastPathManifest = serde_json::from_str(&manifest_contents)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    validate_manifest(&root, &manifest)?;

    let resource_dir = find_resource_dir(exe_dir, download_dirs)?;
    let server_exe = resource_dir.join(SIDECAR_FILE_NAME);
    validate_nonempty_file(&server_exe)?;

    let resource_graph = resource_dir.join(GRAPH_FILE_NAME);
    validate_file_hash(&resource_graph, GRAPH_SIZE, GRAPH_SHA256)?;
    let model_path = root.join("model").join(GRAPH_FILE_NAME);
    if validate_file_hash(&model_path, GRAPH_SIZE, GRAPH_SHA256).is_err() {
        fs::create_dir_all(model_path.parent().expect("model path has a parent"))?;
        fs::copy(&resource_graph, &model_path).with_context(|| {
            format!(
                "failed to copy fast path graph from {} to {}",
                resource_graph.display(),
                model_path.display()
            )
        })?;
        validate_file_hash(&model_path, GRAPH_SIZE, GRAPH_SHA256)?;
    }

    let runtime_dir = root.join("runtime");
    Ok(Some(FastPathLayout {
        server_exe,
        ort_dll: runtime_dir.join("onnxruntime.dll"),
        runtime_dir,
        model_path,
        tokenizer_path: root.join(TOKENIZER_PATH),
    }))
}

fn is_supported_embedding_model(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().contains("qwen3-embedding-0.6b"))
}

fn validate_manifest(root: &Path, manifest: &FastPathManifest) -> anyhow::Result<()> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION
        || manifest.model_revision != MODEL_REVISION
        || manifest.onnxruntime_version != ONNXRUNTIME_VERSION
        || manifest.cuda_runtime_version != CUDA_RUNTIME_VERSION
        || manifest.cublas_version != CUBLAS_VERSION
        || manifest.cufft_version != CUFFT_VERSION
        || manifest.cudnn_version != CUDNN_VERSION
    {
        bail!("embedding fast path manifest versions do not match this Vekai build");
    }

    validate_expected_manifest_file(
        root,
        &manifest.weights,
        WEIGHTS_PATH,
        WEIGHTS_SIZE,
        WEIGHTS_SHA256,
    )?;
    validate_expected_manifest_file(
        root,
        &manifest.tokenizer,
        TOKENIZER_PATH,
        TOKENIZER_SIZE,
        TOKENIZER_SHA256,
    )?;

    let runtime_files = manifest
        .runtime_files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<HashMap<_, _>>();
    for name in REQUIRED_RUNTIME_FILES {
        let manifest_path = format!("runtime/{name}");
        let file = runtime_files
            .get(manifest_path.as_str())
            .ok_or_else(|| anyhow!("embedding fast path manifest is missing {manifest_path}"))?;
        validate_manifest_file_metadata(root, file)?;
    }
    Ok(())
}

fn validate_expected_manifest_file(
    root: &Path,
    file: &ManifestFile,
    expected_path: &str,
    expected_size: u64,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    if file.path != expected_path
        || file.size != expected_size
        || !file.sha256.eq_ignore_ascii_case(expected_sha256)
    {
        bail!("embedding fast path manifest entry {expected_path} does not match this build");
    }
    validate_manifest_file_metadata(root, file)
}

fn validate_manifest_file_metadata(root: &Path, file: &ManifestFile) -> anyhow::Result<()> {
    if file.size == 0 || !is_sha256(&file.sha256) {
        bail!("invalid fast path manifest metadata for {}", file.path);
    }
    let relative = safe_relative_path(&file.path)?;
    let path = root.join(relative);
    let actual_size = path
        .metadata()
        .with_context(|| format!("missing embedding fast path asset {}", path.display()))?
        .len();
    if actual_size != file.size {
        bail!(
            "embedding fast path asset size mismatch for {}: expected {}, got {}",
            path.display(),
            file.size,
            actual_size
        );
    }
    Ok(())
}

fn find_resource_dir(exe_dir: &Path, download_dirs: &[PathBuf]) -> anyhow::Result<PathBuf> {
    let mut candidates = vec![exe_dir.join("fastpath")];
    candidates.extend(
        download_dirs
            .iter()
            .filter_map(|dir| dir.parent())
            .map(|dir| dir.join("fastpath")),
    );
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(Path::new(manifest_dir).join("resources").join("fastpath"));
    }
    candidates
        .into_iter()
        .find(|dir| dir.join(SIDECAR_FILE_NAME).is_file() && dir.join(GRAPH_FILE_NAME).is_file())
        .ok_or_else(|| {
            anyhow!(
                "embedding fast path sidecar resources are missing; run scripts\\build-fastpath.ps1"
            )
        })
}

fn safe_relative_path(value: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unsafe embedding fast path manifest path: {value}");
    }
    Ok(path.to_path_buf())
}

fn validate_nonempty_file(path: &Path) -> anyhow::Result<()> {
    let size = path
        .metadata()
        .with_context(|| format!("missing embedding fast path resource {}", path.display()))?
        .len();
    if size == 0 {
        bail!("embedding fast path resource is empty: {}", path.display());
    }
    Ok(())
}

fn validate_file_hash(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    let metadata = path
        .metadata()
        .with_context(|| format!("missing embedding fast path resource {}", path.display()))?;
    if metadata.len() != expected_size {
        bail!(
            "embedding fast path resource size mismatch: {}",
            path.display()
        );
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        bail!(
            "embedding fast path resource hash mismatch: {}",
            path.display()
        );
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_qwen3_embedding_06b_gguf_enables_probe() {
        assert!(is_supported_embedding_model(Path::new(
            "Qwen3-Embedding-0.6B-Q8_0.gguf"
        )));
        assert!(!is_supported_embedding_model(Path::new(
            "Qwen3-Embedding-4B-Q4_K_M.gguf"
        )));
        assert!(!is_supported_embedding_model(Path::new(
            "Qwen3-Embedding-0.6B.onnx"
        )));
    }

    #[test]
    fn manifest_paths_cannot_escape_asset_root() {
        assert!(safe_relative_path("runtime/onnxruntime.dll").is_ok());
        assert!(safe_relative_path("../onnxruntime.dll").is_err());
        assert!(safe_relative_path("C:/onnxruntime.dll").is_err());
        assert!(safe_relative_path("").is_err());
    }

    #[test]
    fn sha256_validation_is_strict() {
        assert!(is_sha256(GRAPH_SHA256));
        assert!(!is_sha256("abc"));
        assert!(!is_sha256(&"z".repeat(64)));
    }
}
