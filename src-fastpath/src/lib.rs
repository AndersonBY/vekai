use anyhow::{bail, Context, Result};
use ort::{
    ep,
    session::{builder::GraphOptimizationLevel, RunOptions, Session},
    value::Tensor,
};
use std::path::{Path, PathBuf};
use tokenizers::{Tokenizer, TruncationParams};

pub const EMBEDDING_DIMENSION: usize = 1024;
const PREWARM_SEQUENCE_LENGTH: usize = 32;
const PREWARM_BATCHES: [usize; 4] = [1, 8, 32, 64];
const QWEN_PAD_TOKEN_ID: u32 = 151_643;

#[derive(Debug, Clone)]
pub struct EmbeddingRuntimeConfig {
    pub model: PathBuf,
    pub tokenizer: PathBuf,
    pub ort_library: PathBuf,
    pub max_sequence_length: usize,
}

impl EmbeddingRuntimeConfig {
    pub fn validate(&self) -> Result<()> {
        validate_injected_file(&self.model, "embedding model")?;
        validate_injected_file(&self.tokenizer, "tokenizer")?;
        validate_injected_file(&self.ort_library, "ONNX Runtime library")?;
        if self.max_sequence_length == 0 {
            bail!("max_sequence_length must be greater than zero");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRuntimeHealth {
    pub backend: &'static str,
    pub dimension: usize,
    pub max_sequence_length: usize,
}

pub struct EmbeddingOutput {
    pub embeddings: Vec<Vec<f32>>,
    pub token_counts: Vec<usize>,
}

struct EncodedBatch {
    input_ids: Vec<Vec<u32>>,
    token_counts: Vec<usize>,
    max_sequence_length: usize,
}

pub struct EmbeddingRuntime {
    tokenizer: Tokenizer,
    session: Session,
    run_options: RunOptions,
    max_sequence_length: usize,
    _dll_search_path: DllSearchPathGuard,
}

impl EmbeddingRuntime {
    pub fn start(config: EmbeddingRuntimeConfig) -> Result<Self> {
        config.validate()?;
        let dll_search_path = DllSearchPathGuard::install(&config.ort_library)?;
        eprintln!("fastpath stage=ort-init");
        let _ = ort::init_from(&config.ort_library)?
            .with_name("vekai-embedding-fastpath")
            .commit();

        eprintln!("fastpath stage=tokenizer");
        let mut tokenizer = Tokenizer::from_file(&config.tokenizer)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: config.max_sequence_length,
                ..Default::default()
            }))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        eprintln!("fastpath stage=session-build");
        let session = build_session(&config.model)?;
        let run_options = RunOptions::new()?;
        eprintln!("fastpath stage=session-ready");

        let mut runtime = Self {
            tokenizer,
            session,
            run_options,
            max_sequence_length: config.max_sequence_length,
            _dll_search_path: dll_search_path,
        };
        runtime.prewarm()?;
        Ok(runtime)
    }

    pub fn health(&self) -> EmbeddingRuntimeHealth {
        EmbeddingRuntimeHealth {
            backend: "ort_cuda",
            dimension: EMBEDDING_DIMENSION,
            max_sequence_length: self.max_sequence_length,
        }
    }

    pub fn embed(&mut self, inputs: &[String]) -> Result<EmbeddingOutput> {
        if inputs.is_empty() {
            bail!("embedding input must not be empty");
        }
        if inputs.iter().any(|input| input.trim().is_empty()) {
            bail!("embedding input strings must not be empty");
        }
        let mut embeddings = Vec::with_capacity(inputs.len());
        let mut token_counts = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(64) {
            let encoded = self.encode(chunk)?;
            token_counts.extend_from_slice(&encoded.token_counts);
            let mut chunk_embeddings = self.run(&encoded)?;
            embeddings.append(&mut chunk_embeddings);
        }
        ensure_finite(&embeddings)?;
        Ok(EmbeddingOutput {
            embeddings,
            token_counts,
        })
    }

    fn prewarm(&mut self) -> Result<()> {
        let seed = self
            .tokenizer
            .encode("Vekai local embedding warmup", true)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .get_ids()
            .to_vec();
        let seed = seed
            .iter()
            .copied()
            .cycle()
            .take(PREWARM_SEQUENCE_LENGTH)
            .collect::<Vec<_>>();
        for batch_size in PREWARM_BATCHES {
            eprintln!("fastpath stage=prewarm batch={batch_size}");
            let encoded = EncodedBatch {
                input_ids: vec![seed.clone(); batch_size],
                token_counts: vec![PREWARM_SEQUENCE_LENGTH; batch_size],
                max_sequence_length: PREWARM_SEQUENCE_LENGTH,
            };
            for _ in 0..2 {
                ensure_finite(&self.run(&encoded)?)?;
            }
        }
        Ok(())
    }

    fn encode(&self, inputs: &[String]) -> Result<EncodedBatch> {
        let encodings = self
            .tokenizer
            .encode_batch(inputs.to_vec(), true)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let input_ids = encodings
            .iter()
            .map(|encoding| encoding.get_ids().to_vec())
            .collect::<Vec<_>>();
        let token_counts = input_ids.iter().map(Vec::len).collect::<Vec<_>>();
        let max_sequence_length = token_counts.iter().copied().max().unwrap_or(0).max(1);
        Ok(EncodedBatch {
            input_ids,
            token_counts,
            max_sequence_length,
        })
    }

    fn run(&mut self, encoded: &EncodedBatch) -> Result<Vec<Vec<f32>>> {
        let batch_size = encoded.input_ids.len();
        let sequence_length = encoded.max_sequence_length;
        let mut input_ids = vec![i64::from(QWEN_PAD_TOKEN_ID); batch_size * sequence_length];
        let mut attention_mask = vec![0_i64; batch_size * sequence_length];
        fill_left_padded(
            &encoded.input_ids,
            sequence_length,
            &mut input_ids,
            &mut attention_mask,
        );
        let input_ids =
            Tensor::from_array(([batch_size, sequence_length], input_ids.into_boxed_slice()))?;
        let attention_mask = Tensor::from_array((
            [batch_size, sequence_length],
            attention_mask.into_boxed_slice(),
        ))?;
        let outputs = self
            .session
            .run_with_options(ort::inputs![input_ids, attention_mask], &self.run_options)?;
        let values = outputs["embeddings"].try_extract_tensor::<f32>()?.1;
        embeddings_from_flat(values, batch_size)
    }
}

#[cfg(windows)]
struct DllSearchPathGuard {
    cookie: usize,
}

#[cfg(windows)]
impl DllSearchPathGuard {
    fn install(ort_library: &Path) -> Result<Self> {
        use std::{os::windows::ffi::OsStrExt, sync::OnceLock};
        use windows_sys::Win32::System::LibraryLoader::{
            AddDllDirectory, SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            LOAD_LIBRARY_SEARCH_USER_DIRS,
        };

        static DEFAULT_SEARCH_INITIALIZED: OnceLock<i32> = OnceLock::new();
        let initialization_error = *DEFAULT_SEARCH_INITIALIZED.get_or_init(|| {
            let configured = unsafe {
                SetDefaultDllDirectories(
                    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS,
                )
            };
            if configured == 0 {
                std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
            } else {
                0
            }
        });
        if initialization_error != 0 {
            bail!(
                "failed to configure the Windows DLL search policy: {}",
                std::io::Error::from_raw_os_error(initialization_error)
            );
        }

        let runtime_dir = ort_library
            .parent()
            .ok_or_else(|| anyhow::anyhow!("ONNX Runtime library has no parent directory"))?;
        let wide = runtime_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let cookie = unsafe { AddDllDirectory(wide.as_ptr()) };
        if cookie.is_null() {
            return Err(std::io::Error::last_os_error())
                .context("failed to register the ONNX Runtime dependency directory");
        }
        Ok(Self {
            cookie: cookie as usize,
        })
    }
}

#[cfg(windows)]
impl Drop for DllSearchPathGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::LibraryLoader::RemoveDllDirectory;

        unsafe {
            RemoveDllDirectory(self.cookie as *const core::ffi::c_void);
        }
    }
}

#[cfg(not(windows))]
struct DllSearchPathGuard;

#[cfg(not(windows))]
impl DllSearchPathGuard {
    fn install(_ort_library: &Path) -> Result<Self> {
        Ok(Self)
    }
}

fn build_session(model: &Path) -> Result<Session> {
    let builder = Session::builder().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let builder = builder
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let mut builder = builder
        .with_execution_providers([ep::CUDA::default()
            .with_device_id(0)
            .with_tf32(true)
            .build()
            .error_on_failure()])
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    builder
        .commit_from_file(model)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn fill_left_padded(
    rows: &[Vec<u32>],
    sequence_length: usize,
    input_ids: &mut [i64],
    attention_mask: &mut [i64],
) {
    for (row_index, row) in rows.iter().enumerate() {
        let visible = row.len().min(sequence_length);
        let source_start = row.len() - visible;
        let target_start = row_index * sequence_length + sequence_length - visible;
        for (offset, token) in row[source_start..].iter().enumerate() {
            input_ids[target_start + offset] = i64::from(*token);
            attention_mask[target_start + offset] = 1;
        }
    }
}

fn embeddings_from_flat(values: &[f32], count: usize) -> Result<Vec<Vec<f32>>> {
    if values.len() < count * EMBEDDING_DIMENSION {
        bail!("embedding output is shorter than expected");
    }
    Ok(values
        .chunks_exact(EMBEDDING_DIMENSION)
        .take(count)
        .map(<[f32]>::to_vec)
        .collect())
}

fn ensure_finite(embeddings: &[Vec<f32>]) -> Result<()> {
    if embeddings.iter().flatten().any(|value| !value.is_finite()) {
        bail!("embedding engine returned a non-finite value");
    }
    Ok(())
}

fn validate_injected_file(path: &Path, label: &str) -> Result<()> {
    if !path.is_absolute() {
        bail!("{label} path must be absolute");
    }
    if !path.is_file() {
        bail!("{label} file does not exist: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_paths_are_required_and_no_default_directory_is_used() {
        let config = EmbeddingRuntimeConfig {
            model: PathBuf::from("model.onnx"),
            tokenizer: PathBuf::from("tokenizer.json"),
            ort_library: PathBuf::from("onnxruntime.dll"),
            max_sequence_length: 8192,
        };
        let error = config.validate().expect_err("relative paths must fail");
        assert!(error.to_string().contains("path must be absolute"));
    }

    #[test]
    fn zero_sequence_length_is_rejected_after_file_validation() {
        let directory = tempfile::tempdir().expect("temp dir should be created");
        let model = directory.path().join("model.onnx");
        let tokenizer = directory.path().join("tokenizer.json");
        let ort_library = directory.path().join("onnxruntime.dll");
        std::fs::write(&model, b"model").expect("model fixture should be written");
        std::fs::write(&tokenizer, b"{}").expect("tokenizer fixture should be written");
        std::fs::write(&ort_library, b"runtime").expect("runtime fixture should be written");
        let error = EmbeddingRuntimeConfig {
            model,
            tokenizer,
            ort_library,
            max_sequence_length: 0,
        }
        .validate()
        .expect_err("zero sequence length must fail");
        assert!(error.to_string().contains("greater than zero"));
    }

    #[test]
    fn left_padding_keeps_tokens_at_sequence_end() {
        let rows = vec![vec![10, 11], vec![20, 21, 22, 23, 24]];
        let mut input_ids = vec![i64::from(QWEN_PAD_TOKEN_ID); 8];
        let mut attention_mask = vec![0_i64; 8];
        fill_left_padded(&rows, 4, &mut input_ids, &mut attention_mask);
        assert_eq!(
            input_ids,
            vec![
                i64::from(QWEN_PAD_TOKEN_ID),
                i64::from(QWEN_PAD_TOKEN_ID),
                10,
                11,
                21,
                22,
                23,
                24,
            ]
        );
        assert_eq!(attention_mask, vec![0, 0, 1, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn flat_output_must_contain_every_requested_embedding() {
        let error = embeddings_from_flat(&vec![0.0; EMBEDDING_DIMENSION], 2)
            .expect_err("short output should fail");
        assert!(error.to_string().contains("shorter than expected"));
    }
}
