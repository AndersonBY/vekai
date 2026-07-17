use anyhow::{bail, Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ort::{
    ep,
    session::{builder::GraphOptimizationLevel, RunOptions, Session},
    value::Tensor,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    ffi::OsStr,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};
use tokenizers::{Tokenizer, TruncationParams};
use tokio::sync::oneshot;

const EMBEDDING_DIMENSION: usize = 1024;
const PREWARM_SEQUENCE_LENGTH: usize = 32;
const PREWARM_BATCHES: [usize; 4] = [1, 8, 32, 64];
const QWEN_PAD_TOKEN_ID: u32 = 151_643;
const MICRO_BATCH_DELAY: Duration = Duration::from_micros(750);
const WORK_QUEUE_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
struct Config {
    host: String,
    port: u16,
    model: PathBuf,
    tokenizer: PathBuf,
    ort_library: PathBuf,
    max_sequence_length: usize,
}

impl Config {
    fn parse() -> Result<Self> {
        let mut args = pico_args::Arguments::from_env();
        Ok(Self {
            host: args
                .opt_value_from_str("--host")?
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            port: args.opt_value_from_str("--port")?.unwrap_or(8013),
            model: args.value_from_os_str("--model", parse_path)?,
            tokenizer: args.value_from_os_str("--tokenizer", parse_path)?,
            ort_library: args.value_from_os_str("--ort-library", parse_path)?,
            max_sequence_length: args
                .opt_value_from_str("--max-sequence-length")?
                .unwrap_or(8192),
        })
    }
}

fn parse_path(value: &OsStr) -> std::result::Result<PathBuf, Infallible> {
    Ok(PathBuf::from(value))
}

struct EncodedBatch {
    input_ids: Vec<Vec<u32>>,
    token_counts: Vec<usize>,
    max_sequence_length: usize,
}

struct EngineOutput {
    embeddings: Vec<Vec<f32>>,
    token_counts: Vec<usize>,
}

struct Engine {
    tokenizer: Tokenizer,
    session: Session,
    run_options: RunOptions,
}

impl Engine {
    fn load(config: &Config) -> Result<Self> {
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

        let mut engine = Self {
            tokenizer,
            session,
            run_options,
        };
        engine.prewarm()?;
        Ok(engine)
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

    fn embed(&mut self, inputs: &[String]) -> Result<EngineOutput> {
        let mut embeddings = Vec::with_capacity(inputs.len());
        let mut token_counts = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(64) {
            let encoded = self.encode(chunk)?;
            token_counts.extend_from_slice(&encoded.token_counts);
            let mut chunk_embeddings = self.run(&encoded)?;
            embeddings.append(&mut chunk_embeddings);
        }
        ensure_finite(&embeddings)?;
        Ok(EngineOutput {
            embeddings,
            token_counts,
        })
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

struct Work {
    inputs: Vec<String>,
    response: oneshot::Sender<Result<EngineOutput, String>>,
}

#[derive(Clone)]
struct AppState {
    work_tx: SyncSender<Work>,
}

fn start_worker(config: Config) -> Result<SyncSender<Work>> {
    let (work_tx, work_rx) = mpsc::sync_channel::<Work>(WORK_QUEUE_CAPACITY);
    let (ready_tx, ready_rx) = mpsc::sync_channel::<()>(1);
    thread::Builder::new()
        .name("vekai-ort-worker".to_string())
        .spawn(move || worker_main(config, work_rx, ready_tx))?;
    ready_rx
        .recv_timeout(Duration::from_secs(120))
        .context("embedding engine initialization timed out")?;
    Ok(work_tx)
}

fn worker_main(config: Config, work_rx: Receiver<Work>, ready_tx: SyncSender<()>) -> ! {
    let mut engine = match Engine::load(&config) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("fastpath initialization failed: {error:#}");
            std::process::exit(1);
        }
    };
    eprintln!("fastpath engine ready");
    let _ = ready_tx.send(());

    loop {
        let first = match work_rx.recv() {
            Ok(work) => work,
            Err(_) => std::process::exit(0),
        };
        let mut jobs = vec![first];
        let deadline = Instant::now() + MICRO_BATCH_DELAY;
        let mut total_inputs = jobs[0].inputs.len();
        while total_inputs < 64 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match work_rx.recv_timeout(remaining) {
                Ok(work) => {
                    total_inputs += work.inputs.len();
                    jobs.push(work);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let lengths = jobs.iter().map(|job| job.inputs.len()).collect::<Vec<_>>();
        let mut combined = Vec::with_capacity(total_inputs);
        for job in &mut jobs {
            combined.append(&mut job.inputs);
        }
        match engine.embed(&combined) {
            Ok(output) => {
                let mut embedding_offset = 0;
                for (job, length) in jobs.into_iter().zip(lengths) {
                    let end = embedding_offset + length;
                    let result = EngineOutput {
                        embeddings: output.embeddings[embedding_offset..end].to_vec(),
                        token_counts: output.token_counts[embedding_offset..end].to_vec(),
                    };
                    let _ = job.response.send(Ok(result));
                    embedding_offset = end;
                }
            }
            Err(error) => {
                let message = format!("embedding inference failed: {error:#}");
                for job in jobs {
                    let _ = job.response.send(Err(message.clone()));
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct EmbeddingRequest {
    input: EmbeddingInput,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    encoding_format: Option<String>,
    #[serde(default)]
    dimensions: Option<usize>,
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "backend": "ort_cuda"}))
}

async fn embeddings(
    State(state): State<AppState>,
    Json(request): Json<EmbeddingRequest>,
) -> impl IntoResponse {
    let inputs = match request.input {
        EmbeddingInput::Single(value) => vec![value],
        EmbeddingInput::Multiple(values) => values,
    };
    if inputs.is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "input must not be empty");
    }
    if inputs.iter().any(String::is_empty) {
        return api_error(StatusCode::BAD_REQUEST, "input strings must not be empty");
    }
    let dimensions = request.dimensions.unwrap_or(EMBEDDING_DIMENSION);
    if !(1..=EMBEDDING_DIMENSION).contains(&dimensions) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "dimensions must be between 1 and 1024",
        );
    }
    let encoding_format = request.encoding_format.as_deref().unwrap_or("float");
    if !matches!(encoding_format, "float" | "base64") {
        return api_error(
            StatusCode::BAD_REQUEST,
            "encoding_format must be float or base64",
        );
    }

    let (response_tx, response_rx) = oneshot::channel();
    if state
        .work_tx
        .try_send(Work {
            inputs,
            response: response_tx,
        })
        .is_err()
    {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "embedding queue is full");
    }
    let output = match response_rx.await {
        Ok(Ok(output)) => output,
        Ok(Err(message)) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, &message),
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "embedding worker is unavailable",
            )
        }
    };

    let data = output
        .embeddings
        .into_iter()
        .enumerate()
        .map(|(index, embedding)| {
            let embedding = reduce_dimensions(embedding, dimensions);
            let encoded = if encoding_format == "base64" {
                let mut bytes = Vec::with_capacity(embedding.len() * 4);
                for value in embedding {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                json!(BASE64.encode(bytes))
            } else {
                json!(embedding)
            };
            json!({"object": "embedding", "embedding": encoded, "index": index})
        })
        .collect::<Vec<_>>();
    let prompt_tokens = output.token_counts.into_iter().sum::<usize>();
    (
        StatusCode::OK,
        Json(json!({
            "object": "list",
            "data": data,
            "model": request.model.unwrap_or_else(|| "local-embedding".to_string()),
            "usage": {"prompt_tokens": prompt_tokens, "total_tokens": prompt_tokens}
        })),
    )
}

fn reduce_dimensions(mut embedding: Vec<f32>, dimensions: usize) -> Vec<f32> {
    embedding.truncate(dimensions);
    if dimensions == EMBEDDING_DIMENSION {
        return embedding;
    }
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt()
        .max(1e-12);
    for value in &mut embedding {
        *value /= norm;
    }
    embedding
}

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error": {"message": message}})))
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("fastpath server failed: {error:#}");
        let _ = std::io::stderr().flush();
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let config = Config::parse()?;
    let listener = tokio::net::TcpListener::bind((config.host.as_str(), config.port))
        .await
        .with_context(|| format!("failed to bind {}:{}", config.host, config.port))?;
    let work_tx = start_worker(config)?;
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/embeddings", post(embeddings))
        .with_state(AppState { work_tx });
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn reduced_dimensions_are_renormalized() {
        let mut embedding = vec![0.0; EMBEDDING_DIMENSION];
        embedding[0] = 3.0;
        embedding[1] = 4.0;

        let reduced = reduce_dimensions(embedding, 2);

        assert_eq!(reduced.len(), 2);
        assert!((reduced[0] - 0.6).abs() < 1e-6);
        assert!((reduced[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn flat_output_must_contain_every_requested_embedding() {
        let values = vec![0.0; EMBEDDING_DIMENSION - 1];
        assert!(embeddings_from_flat(&values, 1).is_err());
    }
}
