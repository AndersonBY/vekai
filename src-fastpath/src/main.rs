use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    ffi::OsStr,
    io::Write,
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use vekai_embedding_fastpath::{
    EmbeddingOutput as EngineOutput, EmbeddingRuntime as Engine, EmbeddingRuntimeConfig,
    EMBEDDING_DIMENSION,
};

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
    let mut engine = match Engine::start(EmbeddingRuntimeConfig {
        model: config.model,
        tokenizer: config.tokenizer,
        ort_library: config.ort_library,
        max_sequence_length: config.max_sequence_length,
    }) {
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
    fn reduced_dimensions_are_renormalized() {
        let mut embedding = vec![0.0; EMBEDDING_DIMENSION];
        embedding[0] = 3.0;
        embedding[1] = 4.0;

        let reduced = reduce_dimensions(embedding, 2);

        assert_eq!(reduced.len(), 2);
        assert!((reduced[0] - 0.6).abs() < 1e-6);
        assert!((reduced[1] - 0.8).abs() < 1e-6);
    }
}
