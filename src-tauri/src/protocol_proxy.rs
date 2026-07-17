use crate::protocol_adapter::{
    embedding_request_for_llama, embedding_response_for_protocol, rerank_request_for_llama,
    rerank_response_for_protocol,
};
use crate::service::ServiceKind;
use anyhow::Context;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    CONTENT_TYPE,
};
use axum::http::{HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::fmt;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct ProtocolProxyConfig {
    pub service: ServiceKind,
    pub protocol: String,
    pub public_host: String,
    pub public_port: u16,
    pub llama_base_url: String,
}

#[derive(Debug, Clone)]
pub struct ProtocolLaunchPlan {
    pub public_port: u16,
    pub llama_port: u16,
    pub protocol: String,
    pub use_proxy: bool,
    pub proxy_config: Option<ProtocolProxyConfig>,
}

pub struct ProtocolProxyHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl ProtocolProxyHandle {
    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl fmt::Debug for ProtocolProxyHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ProtocolProxyHandle").finish()
    }
}

impl Drop for ProtocolProxyHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[derive(Debug)]
struct ProxyState {
    service: ServiceKind,
    protocol: String,
    llama_base_url: String,
    client: reqwest::Client,
}

pub fn protocol_requires_proxy(service: ServiceKind, protocol: &str) -> bool {
    match service {
        ServiceKind::Rerank => protocol != "llama_rerank",
        ServiceKind::Embedding => protocol != "openai_embeddings",
    }
}

pub fn plan_protocol_launch(
    service: ServiceKind,
    public_host: &str,
    public_port: u16,
    protocol: &str,
) -> anyhow::Result<ProtocolLaunchPlan> {
    let use_proxy = protocol_requires_proxy(service, protocol);
    if !use_proxy {
        return Ok(ProtocolLaunchPlan {
            public_port,
            llama_port: public_port,
            protocol: protocol.to_string(),
            use_proxy: false,
            proxy_config: None,
        });
    }

    let llama_port = allocate_loopback_port()?;
    let proxy_config = ProtocolProxyConfig {
        service,
        protocol: protocol.to_string(),
        public_host: public_host.to_string(),
        public_port,
        llama_base_url: format!("http://127.0.0.1:{llama_port}"),
    };

    Ok(ProtocolLaunchPlan {
        public_port,
        llama_port,
        protocol: protocol.to_string(),
        use_proxy: true,
        proxy_config: Some(proxy_config),
    })
}

pub fn protocol_public_path(service: ServiceKind, protocol: &str) -> &'static str {
    match service {
        ServiceKind::Embedding => match protocol {
            "cohere_embed_v2" => "/v2/embed",
            "openai_embeddings" => "/v1/embeddings",
            "siliconflow" | "voyage_embeddings_v1" | "custom_json_http" => "/embeddings",
            _ => "/embeddings",
        },
        ServiceKind::Rerank => match protocol {
            "cohere_rerank_v2" => "/v2/rerank",
            "llama_rerank" | "jina_rerank_v1" => "/v1/rerank",
            "siliconflow" | "voyage_rerank_v1" | "custom_json_http" => "/rerank",
            _ => "/rerank",
        },
    }
}

pub async fn start_protocol_proxy(
    config: ProtocolProxyConfig,
) -> anyhow::Result<ProtocolProxyHandle> {
    let listener = TcpListener::bind((config.public_host.as_str(), config.public_port))
        .await
        .with_context(|| {
            format!(
                "failed to bind {} protocol proxy on {}:{}",
                config.service.display_name(),
                config.public_host,
                config.public_port
            )
        })?;
    let state = Arc::new(ProxyState {
        service: config.service,
        protocol: config.protocol,
        llama_base_url: config.llama_base_url.trim_end_matches('/').to_string(),
        client: reqwest::Client::new(),
    });
    let app = Router::new()
        .route("/health", get(health))
        .fallback(proxy_request)
        .with_state(state);
    let (shutdown, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
        if let Err(error) = result {
            tracing::warn!("protocol proxy exited with error: {error}");
        }
    });

    Ok(ProtocolProxyHandle {
        shutdown: Some(shutdown),
        task: Some(task),
    })
}

async fn health(State(state): State<Arc<ProxyState>>) -> Response {
    let url = format!("{}/health", state.llama_base_url);
    match state.client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            json_response(StatusCode::OK, json!({"status": "ok"}))
        }
        Ok(response) => json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({
                "status": "unavailable",
                "upstream_status": response.status().as_u16()
            }),
        ),
        Err(error) => json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({
                "status": "unavailable",
                "error": error.to_string()
            }),
        ),
    }
}

async fn proxy_request(
    State(state): State<Arc<ProxyState>>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> Response {
    if method == Method::OPTIONS {
        return empty_response(StatusCode::NO_CONTENT);
    }

    if method != Method::POST {
        return json_response(
            StatusCode::METHOD_NOT_ALLOWED,
            json!({"error": "method must be POST"}),
        );
    }

    let expected_path = protocol_public_path(state.service, &state.protocol);
    if !path_matches(uri.path(), expected_path) {
        return json_response(
            StatusCode::NOT_FOUND,
            json!({
                "error": "unsupported protocol path",
                "expected_path": expected_path
            }),
        );
    }

    let incoming = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(error) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"error": format!("invalid JSON request body: {error}")}),
            );
        }
    };

    match state.service {
        ServiceKind::Embedding => proxy_embedding(state, incoming).await,
        ServiceKind::Rerank => proxy_rerank(state, incoming).await,
    }
}

async fn proxy_embedding(state: Arc<ProxyState>, incoming: Value) -> Response {
    let llama_body = embedding_request_for_llama(&state.protocol, incoming);

    let raw = match post_llama_json(&state, "/v1/embeddings", &llama_body).await {
        Ok(value) => value,
        Err(response) => return response,
    };

    json_response(
        StatusCode::OK,
        embedding_response_for_protocol(&state.protocol, raw),
    )
}

async fn proxy_rerank(state: Arc<ProxyState>, incoming: Value) -> Response {
    let (llama_body, documents) = rerank_request_for_llama(&state.protocol, incoming);

    let raw = match post_llama_json(&state, "/v1/rerank", &llama_body).await {
        Ok(value) => value,
        Err(response) => return response,
    };

    json_response(
        StatusCode::OK,
        rerank_response_for_protocol(&state.protocol, raw, &documents),
    )
}

async fn post_llama_json(state: &ProxyState, path: &str, body: &Value) -> Result<Value, Response> {
    let url = format!("{}{}", state.llama_base_url, path);
    let response = state
        .client
        .post(&url)
        .json(body)
        .send()
        .await
        .map_err(|error| {
            json_response(
                StatusCode::BAD_GATEWAY,
                json!({"error": format!("failed to forward request to llama-server: {error}")}),
            )
        })?;
    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let text = response.text().await.map_err(|error| {
        json_response(
            StatusCode::BAD_GATEWAY,
            json!({"error": format!("failed to read llama-server response: {error}")}),
        )
    })?;

    if !status.is_success() {
        return Err(text_response(status, text));
    }

    serde_json::from_str(&text).map_err(|error| {
        json_response(
            StatusCode::BAD_GATEWAY,
            json!({"error": format!("llama-server returned invalid JSON: {error}")}),
        )
    })
}

fn allocate_loopback_port() -> anyhow::Result<u16> {
    let listener = StdTcpListener::bind(("127.0.0.1", 0))
        .context("failed to allocate a loopback port for protocol proxy")?;
    let port = listener
        .local_addr()
        .context("failed to read allocated loopback port")?
        .port();
    drop(listener);
    Ok(port)
}

fn path_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual.trim_end_matches('/') == expected
}

fn empty_response(status: StatusCode) -> Response {
    add_common_headers(status.into_response())
}

fn text_response(status: StatusCode, text: String) -> Response {
    let mut response = (status, text).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    add_common_headers(response)
}

fn json_response(status: StatusCode, value: Value) -> Response {
    add_common_headers((status, Json(value)).into_response())
}

fn add_common_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET,POST,OPTIONS"),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization,content-type"),
    );
    response
}

#[cfg(test)]
mod tests {
    use crate::service::ServiceKind;
    use axum::extract::State;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde_json::{json, Value};
    use std::net::TcpListener as StdTcpListener;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    use super::{
        protocol_public_path, protocol_requires_proxy, start_protocol_proxy, ProtocolProxyConfig,
    };

    #[test]
    fn vv_llm_protocols_map_to_public_paths() {
        assert_eq!(
            protocol_public_path(ServiceKind::Embedding, "openai_embeddings"),
            "/v1/embeddings"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Embedding, "cohere_embed_v2"),
            "/v2/embed"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Embedding, "siliconflow"),
            "/embeddings"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Embedding, "voyage_embeddings_v1"),
            "/embeddings"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Embedding, "custom_json_http"),
            "/embeddings"
        );

        assert_eq!(
            protocol_public_path(ServiceKind::Rerank, "llama_rerank"),
            "/v1/rerank"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Rerank, "cohere_rerank_v2"),
            "/v2/rerank"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Rerank, "jina_rerank_v1"),
            "/v1/rerank"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Rerank, "voyage_rerank_v1"),
            "/rerank"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Rerank, "siliconflow"),
            "/rerank"
        );
        assert_eq!(
            protocol_public_path(ServiceKind::Rerank, "custom_json_http"),
            "/rerank"
        );
    }

    #[test]
    fn native_llama_protocols_do_not_need_proxy() {
        assert!(!protocol_requires_proxy(
            ServiceKind::Embedding,
            "openai_embeddings"
        ));
        assert!(!protocol_requires_proxy(
            ServiceKind::Rerank,
            "llama_rerank"
        ));
        assert!(protocol_requires_proxy(
            ServiceKind::Embedding,
            "cohere_embed_v2"
        ));
        assert!(protocol_requires_proxy(
            ServiceKind::Rerank,
            "cohere_rerank_v2"
        ));
    }

    #[tokio::test]
    async fn cohere_rerank_proxy_forwards_to_llama_and_maps_response() {
        let _network_guard = crate::NETWORK_TEST_LOCK.lock().await;
        let captured = Arc::new(Mutex::new(None::<Value>));
        let backend_listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("backend listener should bind");
        let backend_addr = backend_listener
            .local_addr()
            .expect("backend listener address should be readable");
        let backend_app =
            Router::new()
                .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
                .route(
                    "/v1/rerank",
                    post(
                        |State(captured): State<Arc<Mutex<Option<Value>>>>,
                         Json(body): Json<Value>| async move {
                            *captured.lock().await = Some(body);
                            Json(json!({
                                "results": [
                                    {"index": 1, "relevance_score": 0.91}
                                ]
                            }))
                        },
                    ),
                )
                .with_state(captured.clone());
        let backend_task = tokio::spawn(async move {
            axum::serve(backend_listener, backend_app)
                .await
                .expect("backend server should run");
        });
        let public_port = free_port();
        let proxy = start_protocol_proxy(ProtocolProxyConfig {
            service: ServiceKind::Rerank,
            protocol: "cohere_rerank_v2".to_string(),
            public_host: "127.0.0.1".to_string(),
            public_port,
            llama_base_url: format!("http://{backend_addr}"),
        })
        .await
        .expect("proxy should start");

        let response = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{public_port}/v2/rerank"))
            .json(&json!({
                "model": "rerank-v3.5",
                "query": "q",
                "documents": [{"text": "doc1"}, {"text": "doc2"}],
                "top_n": 1
            }))
            .send()
            .await
            .expect("proxy request should send");
        assert!(response.status().is_success());
        let body = response
            .json::<Value>()
            .await
            .expect("proxy response should be JSON");

        assert_eq!(
            captured
                .lock()
                .await
                .clone()
                .expect("llama backend should receive request"),
            json!({
                "model": "rerank-v3.5",
                "query": "q",
                "documents": ["doc1", "doc2"],
                "top_n": 1
            })
        );
        assert_eq!(
            body,
            json!({
                "results": [
                    {"index": 1, "relevance_score": 0.91, "document": {"text": "doc2"}}
                ],
                "meta": {
                    "billed_units": {"search_units": 1}
                }
            })
        );

        proxy.shutdown().await;
        backend_task.abort();
    }

    fn free_port() -> u16 {
        let listener = StdTcpListener::bind(("127.0.0.1", 0)).expect("free port should bind");
        listener
            .local_addr()
            .expect("free port address should be readable")
            .port()
    }
}
