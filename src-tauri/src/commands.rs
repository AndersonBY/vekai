use crate::config::AppConfig;
use crate::fastpath::discover_embedding_fastpath;
use crate::logging::{append_log, log_paths, LogPaths};
use crate::models::{
    choose_default_model_for_service, discover_model_catalog, discover_models, ModelCatalog,
};
use crate::process::ProcessManager;
use crate::protocol_proxy::protocol_public_path;
use crate::runtime::ensure_runtime_from_downloads;
use crate::service::ServiceKind;
use crate::state::{AppSnapshot, AppStateInner, ServiceStatus, SharedAppState};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

const APP_DATA_DIR_NAME: &str = "Vekai";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub exe_dir: PathBuf,
    pub model_base_dir: PathBuf,
    pub config_path: PathBuf,
    pub local_data_dir: PathBuf,
    pub download_dirs: Vec<PathBuf>,
    pub runtime_root: PathBuf,
    pub logs: LogPaths,
}

#[derive(Debug, Clone)]
pub struct InitialConfig {
    pub config: AppConfig,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ServiceManagers {
    pub rerank: Arc<ProcessManager>,
    pub embedding: Arc<ProcessManager>,
}

impl ServiceManagers {
    pub fn new() -> Self {
        Self {
            rerank: Arc::new(ProcessManager::for_service(ServiceKind::Rerank)),
            embedding: Arc::new(ProcessManager::for_service(ServiceKind::Embedding)),
        }
    }

    pub fn manager(&self, service: ServiceKind) -> Arc<ProcessManager> {
        match service {
            ServiceKind::Rerank => self.rerank.clone(),
            ServiceKind::Embedding => self.embedding.clone(),
        }
    }

    pub fn all(&self) -> [Arc<ProcessManager>; 2] {
        [self.rerank.clone(), self.embedding.clone()]
    }
}

impl Default for ServiceManagers {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppBootstrap {
    pub snapshot: AppSnapshot,
    pub models: ModelCatalog,
    pub logs_dir: String,
    pub warning: Option<String>,
}

#[tauri::command]
pub async fn get_bootstrap(
    state: State<'_, SharedAppState>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppBootstrap, String> {
    get_bootstrap_inner(state.inner(), paths.inner())
        .await
        .map_err(error_to_string)
}

#[tauri::command]
pub async fn get_snapshot(
    state: State<'_, SharedAppState>,
) -> std::result::Result<AppSnapshot, String> {
    Ok(get_snapshot_inner(state.inner()).await)
}

#[tauri::command]
pub async fn save_config(
    config: AppConfig,
    state: State<'_, SharedAppState>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppSnapshot, String> {
    save_config_inner(config, state.inner(), paths.inner())
        .await
        .map_err(error_to_string)
}

#[tauri::command]
pub async fn refresh_models(
    state: State<'_, SharedAppState>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<ModelCatalog, String> {
    refresh_models_inner(state.inner(), paths.inner())
        .await
        .map_err(error_to_string)
}

#[tauri::command]
pub async fn start_servers(
    state: State<'_, SharedAppState>,
    managers: State<'_, Arc<ServiceManagers>>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppSnapshot, String> {
    start_servers_inner(state.inner(), managers.inner(), paths.inner())
        .await
        .map_err(error_to_string)
}

#[tauri::command]
pub async fn stop_servers(
    state: State<'_, SharedAppState>,
    managers: State<'_, Arc<ServiceManagers>>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppSnapshot, String> {
    stop_servers_inner(state.inner(), managers.inner(), paths.inner())
        .await
        .map_err(error_to_string)
}

#[tauri::command]
pub async fn restart_servers(
    state: State<'_, SharedAppState>,
    managers: State<'_, Arc<ServiceManagers>>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppSnapshot, String> {
    restart_servers_inner(state.inner(), managers.inner(), paths.inner())
        .await
        .map_err(error_to_string)
}

#[tauri::command]
pub async fn start_server(
    state: State<'_, SharedAppState>,
    managers: State<'_, Arc<ServiceManagers>>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppSnapshot, String> {
    start_servers(state, managers, paths).await
}

#[tauri::command]
pub async fn stop_server(
    state: State<'_, SharedAppState>,
    managers: State<'_, Arc<ServiceManagers>>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppSnapshot, String> {
    stop_servers(state, managers, paths).await
}

#[tauri::command]
pub async fn restart_server(
    state: State<'_, SharedAppState>,
    managers: State<'_, Arc<ServiceManagers>>,
    paths: State<'_, AppPaths>,
) -> std::result::Result<AppSnapshot, String> {
    restart_servers(state, managers, paths).await
}

#[tauri::command]
pub async fn test_rerank(
    state: State<'_, SharedAppState>,
) -> std::result::Result<serde_json::Value, String> {
    test_rerank_inner(state.inner())
        .await
        .map_err(error_to_string)
}

#[tauri::command]
pub async fn test_embedding(
    state: State<'_, SharedAppState>,
) -> std::result::Result<serde_json::Value, String> {
    test_embedding_inner(state.inner())
        .await
        .map_err(error_to_string)
}

pub fn build_app_paths() -> anyhow::Result<AppPaths> {
    build_app_paths_with_resource_dir(None)
}

pub fn build_app_paths_with_resource_dir(resource_dir: Option<&Path>) -> anyhow::Result<AppPaths> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| anyhow!("current executable has no parent directory"))?
        .to_path_buf();
    let config_dir = dirs::data_dir()
        .ok_or_else(|| anyhow!("failed to resolve data directory"))?
        .join(APP_DATA_DIR_NAME);
    let local_data_dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow!("failed to resolve local data directory"))?
        .join(APP_DATA_DIR_NAME);
    let current_dir = std::env::current_dir().context("failed to resolve current directory")?;
    let cargo_manifest_dir = option_env!("CARGO_MANIFEST_DIR").map(Path::new);
    let model_base_dir = resolve_model_base_dir(&exe_dir, cargo_manifest_dir, &current_dir);
    let resource_dirs = resolve_resource_dirs(&exe_dir, resource_dir);
    let download_dirs = resolve_download_dirs(
        &exe_dir,
        &local_data_dir,
        &resource_dirs,
        cargo_manifest_dir,
        &current_dir,
    );
    let logs = log_paths(&local_data_dir);

    Ok(AppPaths {
        exe_dir,
        model_base_dir,
        config_path: config_dir.join("config.json"),
        download_dirs,
        runtime_root: local_data_dir.join("runtimes"),
        local_data_dir,
        logs,
    })
}

pub fn load_initial_config(paths: &AppPaths) -> anyhow::Result<InitialConfig> {
    if paths.config_path.exists() {
        match fs::read_to_string(&paths.config_path)
            .with_context(|| format!("failed to read config file {}", paths.config_path.display()))
            .and_then(|contents| {
                AppConfig::from_json_with_defaults(&contents).with_context(|| {
                    format!(
                        "failed to parse config file {}",
                        paths.config_path.display()
                    )
                })
            }) {
            Ok(config) => {
                return Ok(InitialConfig {
                    config,
                    warning: None,
                });
            }
            Err(error) => {
                let mut initial = default_initial_config(paths)?;
                initial.warning = Some(format!(
                    "Failed to load config {}; using defaults: {error}",
                    paths.config_path.display()
                ));
                return Ok(initial);
            }
        }
    }

    default_initial_config(paths)
}

fn default_initial_config(paths: &AppPaths) -> anyhow::Result<InitialConfig> {
    let mut config = AppConfig::default();
    apply_default_model_paths(&paths.model_base_dir, &mut config)?;

    Ok(InitialConfig {
        config,
        warning: None,
    })
}

fn apply_default_model_paths(model_base_dir: &Path, config: &mut AppConfig) -> anyhow::Result<()> {
    let catalog = discover_model_catalog(model_base_dir, None, None)?;

    if let Some(model) = choose_default_model_for_service(ServiceKind::Rerank, &catalog.rerank) {
        config.set_service_model_path(
            ServiceKind::Rerank,
            model.path.to_string_lossy().into_owned(),
        );
    }

    if let Some(model) =
        choose_default_model_for_service(ServiceKind::Embedding, &catalog.embedding)
    {
        config.set_service_model_path(
            ServiceKind::Embedding,
            model.path.to_string_lossy().into_owned(),
        );
    }

    Ok(())
}

pub async fn get_bootstrap_inner(
    state: &SharedAppState,
    paths: &AppPaths,
) -> anyhow::Result<AppBootstrap> {
    let snapshot = snapshot(state).await;
    let models = model_catalog_for_config(&paths.model_base_dir, &snapshot.config)?;
    let warning = snapshot.error.clone();

    Ok(AppBootstrap {
        snapshot,
        models,
        logs_dir: paths.logs.log_dir.to_string_lossy().into_owned(),
        warning,
    })
}

pub async fn get_snapshot_inner(state: &SharedAppState) -> AppSnapshot {
    snapshot(state).await
}

pub async fn save_config_inner(
    config: AppConfig,
    state: &SharedAppState,
    paths: &AppPaths,
) -> anyhow::Result<AppSnapshot> {
    config.validate()?;

    save_config_file_atomic(&paths.config_path, &config)?;
    append_log(&paths.logs.app_log, "Saved configuration")?;

    let mut state = state.lock().await;
    let previous_config = state.config.clone();
    clear_stale_service_failures_after_config_save(&mut state, &previous_config, &config);
    state.config = config;
    state.push_log("Saved configuration");
    Ok(state.snapshot_v2())
}

pub async fn refresh_models_inner(
    state: &SharedAppState,
    paths: &AppPaths,
) -> anyhow::Result<ModelCatalog> {
    let config = {
        let state = state.lock().await;
        state.config.clone()
    };

    model_catalog_for_config(&paths.model_base_dir, &config)
}

pub async fn start_servers_inner(
    state: &SharedAppState,
    managers: &Arc<ServiceManagers>,
    paths: &AppPaths,
) -> anyhow::Result<AppSnapshot> {
    let mut config = {
        let state = state.lock().await;
        state.config.clone()
    };
    config.validate()?;

    let runtime = ensure_runtime_from_downloads(&paths.runtime_root, &paths.download_dirs)?;
    for service in ServiceKind::START_ORDER {
        if !config.service_enabled(service) {
            continue;
        }

        let model_path =
            match resolve_service_model_config_for_start(&paths.model_base_dir, &config, service) {
                Ok((resolved_config, model_path)) => {
                    config = resolved_config;
                    let mut state = state.lock().await;
                    state.config = config.clone();
                    model_path
                }
                Err(error) => {
                    record_service_failure(
                        state,
                        &paths.logs,
                        service,
                        &format!("failed to resolve model: {error}"),
                    )
                    .await?;
                    continue;
                }
            };

        let fastpath = if service == ServiceKind::Embedding {
            match discover_embedding_fastpath(
                &paths.local_data_dir,
                &paths.exe_dir,
                &paths.download_dirs,
                &model_path,
            ) {
                Ok(layout) => layout,
                Err(error) => {
                    let message =
                        format!("Embedding fast path unavailable; using llama.cpp: {error}");
                    append_log(&paths.logs.app_log, &message)?;
                    state.lock().await.push_service_log(service, message);
                    None
                }
            }
        } else {
            None
        };

        if let Err(error) = managers
            .manager(service)
            .start_with_fastpath(
                config.clone(),
                runtime.clone(),
                model_path,
                state.clone(),
                paths.logs.clone(),
                fastpath,
            )
            .await
        {
            record_service_failure(
                state,
                &paths.logs,
                service,
                &format!("failed to start service: {error}"),
            )
            .await?;
        }
    }

    Ok(snapshot(state).await)
}

pub async fn stop_servers_inner(
    state: &SharedAppState,
    managers: &Arc<ServiceManagers>,
    paths: &AppPaths,
) -> anyhow::Result<AppSnapshot> {
    for manager in managers.all() {
        if let Err(error) = manager.stop(state.clone(), paths.logs.clone()).await {
            record_service_failure(
                state,
                &paths.logs,
                manager.kind(),
                &format!("failed to stop service: {error}"),
            )
            .await?;
        }
    }

    Ok(snapshot(state).await)
}

pub async fn restart_servers_inner(
    state: &SharedAppState,
    managers: &Arc<ServiceManagers>,
    paths: &AppPaths,
) -> anyhow::Result<AppSnapshot> {
    stop_servers_inner(state, managers, paths).await?;
    start_servers_inner(state, managers, paths).await
}

pub async fn test_rerank_inner(state: &SharedAppState) -> anyhow::Result<serde_json::Value> {
    let config = {
        let state = state.lock().await;
        state.config.clone()
    };
    let (url, body) = rerank_test_request(&config);

    let response = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("failed to POST {url}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .with_context(|| format!("failed to read rerank response from {url}"))?;

    if !status.is_success() {
        return Err(anyhow!("rerank test failed with HTTP {status}: {text}"));
    }

    serde_json::from_str(&text).context("failed to parse rerank response JSON")
}

pub async fn test_embedding_inner(state: &SharedAppState) -> anyhow::Result<serde_json::Value> {
    let config = {
        let state = state.lock().await;
        state.config.clone()
    };
    let (url, body) = embedding_test_request(&config);

    let response = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("failed to POST {url}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .with_context(|| format!("failed to read embedding response from {url}"))?;

    if !status.is_success() {
        return Err(anyhow!("embedding test failed with HTTP {status}: {text}"));
    }

    serde_json::from_str(&text).context("failed to parse embedding response JSON")
}

async fn snapshot(state: &SharedAppState) -> AppSnapshot {
    let state = state.lock().await;
    state.snapshot_v2()
}

fn selected_service_model_path(
    exe_dir: &Path,
    config: &AppConfig,
    service: ServiceKind,
) -> anyhow::Result<PathBuf> {
    if let Some(configured_path) = path_if_present(config.service_model_path(service)) {
        if configured_path.is_file() && is_gguf_path(&configured_path) {
            return Ok(dunce::canonicalize(&configured_path).unwrap_or(configured_path));
        }

        return Err(anyhow!(
            "configured {} model does not exist or is not a GGUF file: {}",
            service.display_name(),
            configured_path.display()
        ));
    }

    choose_default_model_for_service(service, &discover_models(exe_dir, None)?)
        .map(|model| model.path)
        .ok_or_else(|| {
            anyhow!(
                "No GGUF model found for {} service in {}",
                service.display_name(),
                exe_dir.join("models").display(),
            )
        })
}

fn resolve_service_model_config_for_start(
    model_base_dir: &Path,
    config: &AppConfig,
    service: ServiceKind,
) -> anyhow::Result<(AppConfig, PathBuf)> {
    let model_path = selected_service_model_path(model_base_dir, config, service)?;
    let mut resolved_config = config.clone();
    resolved_config.set_service_model_path(service, model_path.to_string_lossy().into_owned());
    Ok((resolved_config, model_path))
}

fn model_catalog_for_config(
    model_base_dir: &Path,
    config: &AppConfig,
) -> anyhow::Result<ModelCatalog> {
    let rerank_model = path_if_present(config.service_model_path(ServiceKind::Rerank));
    let embedding_model = path_if_present(config.service_model_path(ServiceKind::Embedding));

    discover_model_catalog(
        model_base_dir,
        rerank_model.as_deref(),
        embedding_model.as_deref(),
    )
}

fn clear_stale_service_failures_after_config_save(
    state: &mut AppStateInner,
    previous: &AppConfig,
    next: &AppConfig,
) {
    for service in ServiceKind::ALL {
        if state.service_status(service) != ServiceStatus::Failed {
            continue;
        }

        if service_launch_config_changed(service, previous, next) {
            state.set_service_status(service, ServiceStatus::Stopped);
            state.set_service_error(service, None);
        }
    }
}

fn service_launch_config_changed(
    service: ServiceKind,
    previous: &AppConfig,
    next: &AppConfig,
) -> bool {
    previous.service_config(service) != next.service_config(service)
        || previous.host != next.host
        || previous.gpu_layers != next.gpu_layers
        || previous.threads != next.threads
        || previous.parallel_slots != next.parallel_slots
        || previous.http_threads != next.http_threads
}

async fn record_service_failure(
    state: &SharedAppState,
    logs: &LogPaths,
    service: ServiceKind,
    message: &str,
) -> anyhow::Result<()> {
    let message = format!("{}: {message}", service.display_name());
    {
        let mut state = state.lock().await;
        state.set_service_status(service, ServiceStatus::Failed);
        state.set_service_error(service, Some(message.clone()));
        state.push_service_log(service, message.clone());
    }

    append_log(&logs.app_log, &message)
}

fn save_config_file_atomic(config_path: &Path, config: &AppConfig) -> anyhow::Result<()> {
    let config_json = serde_json::to_string_pretty(config).context("failed to serialize config")?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let temp_path = temp_config_path(config_path);
    fs::write(&temp_path, config_json)
        .with_context(|| format!("failed to write temp config file {}", temp_path.display()))?;

    match fs::rename(&temp_path, config_path) {
        Ok(()) => Ok(()),
        Err(first_error) if config_path.exists() => {
            let backup_path = temp_config_path(config_path);
            fs::rename(config_path, &backup_path).with_context(|| {
                format!(
                    "failed to move existing config {} before replacing it: {first_error}",
                    config_path.display()
                )
            })?;

            match fs::rename(&temp_path, config_path) {
                Ok(()) => {
                    let _ = fs::remove_file(&backup_path);
                    Ok(())
                }
                Err(second_error) => {
                    let _ = fs::rename(&backup_path, config_path);
                    let _ = fs::remove_file(&temp_path);
                    Err(anyhow!(
                        "failed to replace config file {}: {second_error}",
                        config_path.display()
                    ))
                }
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(anyhow!(
                "failed to replace config file {}: {error}",
                config_path.display()
            ))
        }
    }
}

fn temp_config_path(config_path: &Path) -> PathBuf {
    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = config_path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "config.json".into());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    parent.join(format!("{file_name}.tmp-{}-{nanos}", std::process::id()))
}

fn resolve_model_base_dir(
    exe_dir: &Path,
    #[cfg_attr(not(debug_assertions), allow(unused_variables))] cargo_manifest_dir: Option<&Path>,
    #[cfg_attr(not(debug_assertions), allow(unused_variables))] current_dir: &Path,
) -> PathBuf {
    if exe_dir.join("models").exists() {
        return exe_dir.to_path_buf();
    }

    #[cfg(debug_assertions)]
    {
        let mut candidates = Vec::new();
        if let Some(manifest_dir) = cargo_manifest_dir {
            if let Some(parent) = manifest_dir.parent() {
                candidates.push(parent.to_path_buf());
            }
            candidates.push(manifest_dir.to_path_buf());
        }
        candidates.push(current_dir.to_path_buf());

        for candidate in candidates {
            if candidate.join("models").exists() {
                return candidate;
            }
        }
    }

    exe_dir.to_path_buf()
}

fn resolve_download_dirs(
    exe_dir: &Path,
    local_data_dir: &Path,
    resource_dirs: &[PathBuf],
    #[cfg_attr(not(debug_assertions), allow(unused_variables))] cargo_manifest_dir: Option<&Path>,
    #[cfg_attr(not(debug_assertions), allow(unused_variables))] current_dir: &Path,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(local_data_dir.join("downloads"));
    candidates.push(exe_dir.join("downloads"));
    for resource_dir in resource_dirs {
        candidates.push(resource_dir.join("downloads"));
    }

    #[cfg(debug_assertions)]
    {
        if let Some(manifest_dir) = cargo_manifest_dir {
            if let Some(parent) = manifest_dir.parent() {
                candidates.push(parent.join("downloads"));
            }
            candidates.push(manifest_dir.join("downloads"));
        }
        candidates.push(current_dir.join("downloads"));
    }

    dedupe_paths(candidates)
}

fn resolve_resource_dirs(exe_dir: &Path, resource_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(resource_dir) = resource_dir {
        candidates.push(resource_dir.to_path_buf());
    }
    candidates.extend(default_resource_dirs(exe_dir));
    dedupe_paths(candidates)
}

fn default_resource_dirs(exe_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "windows")]
    {
        candidates.push(exe_dir.to_path_buf());
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(exe_dir.join("../Resources"));
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(appdir) = std::env::var_os("APPDIR") {
            candidates.push(PathBuf::from(appdir).join("usr").join("lib").join("vekai"));
        }
        candidates.push(PathBuf::from("/usr/lib/vekai"));
        if exe_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "debug" || name == "release")
        {
            if let Some(target_dir) = exe_dir.parent() {
                candidates.push(target_dir.join("lib").join("vekai"));
            }
        }
    }

    candidates
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }
    deduped
}

fn path_if_present(path: &str) -> Option<PathBuf> {
    if path.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn is_gguf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
}

fn rerank_url(config: &AppConfig) -> String {
    let protocol = config.service_protocol(ServiceKind::Rerank);
    service_url(
        config,
        ServiceKind::Rerank,
        protocol_public_path(ServiceKind::Rerank, protocol),
    )
}

fn embedding_url(config: &AppConfig) -> String {
    let protocol = config.service_protocol(ServiceKind::Embedding);
    service_url(
        config,
        ServiceKind::Embedding,
        protocol_public_path(ServiceKind::Embedding, protocol),
    )
}

fn rerank_test_request(config: &AppConfig) -> (String, serde_json::Value) {
    let protocol = config.service_protocol(ServiceKind::Rerank);
    let documents = serde_json::json!([
        "hi",
        "it is a bear",
        "The giant panda is a bear species endemic to China."
    ]);
    let mut body = serde_json::Map::new();
    body.insert(
        "model".to_string(),
        serde_json::Value::String("local-reranker".to_string()),
    );
    body.insert(
        "query".to_string(),
        serde_json::Value::String("What is panda?".to_string()),
    );
    body.insert("documents".to_string(), documents);

    match protocol {
        "voyage_rerank_v1" => {
            body.insert("top_k".to_string(), serde_json::json!(2));
            body.insert("return_documents".to_string(), serde_json::json!(true));
        }
        "cohere_rerank_v2" | "jina_rerank_v1" => {
            body.insert("top_n".to_string(), serde_json::json!(2));
            body.insert("return_documents".to_string(), serde_json::json!(true));
        }
        "siliconflow" => {}
        _ => {
            body.insert("top_n".to_string(), serde_json::json!(2));
        }
    }

    (rerank_url(config), serde_json::Value::Object(body))
}

fn embedding_test_request(config: &AppConfig) -> (String, serde_json::Value) {
    let protocol = config.service_protocol(ServiceKind::Embedding);
    let body = match protocol {
        "cohere_embed_v2" => serde_json::json!({
            "model": "local-embedding",
            "texts": ["hello world"],
            "input_type": "search_document"
        }),
        "voyage_embeddings_v1" => serde_json::json!({
            "model": "local-embedding",
            "input": "hello world"
        }),
        _ => serde_json::json!({
            "model": "local-embedding",
            "input": "hello world",
            "encoding_format": "float"
        }),
    };

    (embedding_url(config), body)
}

fn service_url(config: &AppConfig, service: ServiceKind, path: &str) -> String {
    let host = normalize_probe_host(&config.host);
    format!("http://{host}:{}{}", config.service_port(service), path)
}

fn normalize_probe_host(host: &str) -> String {
    match host.trim() {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" => "[::1]".to_string(),
        host => normalize_url_host(host),
    }
}

fn normalize_url_host(host: &str) -> String {
    let host = host.trim();

    if host.contains(':') && !host.starts_with('[') && !host.ends_with(']') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn error_to_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::logging::log_paths;
    use crate::service::ServiceKind;
    use crate::state::AppStateInner;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn load_initial_config_defaults_to_discovered_reranker_when_config_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        let models_dir = exe_dir.join("models");
        fs::create_dir_all(&models_dir).expect("models dir should be created");
        fs::write(models_dir.join("alpha.gguf"), b"alpha").expect("model should be written");
        fs::write(models_dir.join("bge-reranker.gguf"), b"reranker")
            .expect("model should be written");

        let paths = test_paths(exe_dir, temp_dir.path().join("config").join("config.json"));

        let initial = load_initial_config(&paths).expect("initial config should load");

        assert_eq!(initial.config.host, AppConfig::default().host);
        assert!(initial
            .config
            .rerank
            .model_path
            .ends_with("bge-reranker.gguf"));
        assert!(initial.warning.is_none());
    }

    #[test]
    fn app_paths_use_current_product_data_directory_name() {
        let paths = build_app_paths().expect("app paths should resolve");

        assert!(paths.config_path.to_string_lossy().contains("Vekai"));
        assert!(paths.local_data_dir.to_string_lossy().contains("Vekai"));
        assert!(paths.logs.log_dir.to_string_lossy().contains("Vekai"));
        assert!(!paths
            .config_path
            .to_string_lossy()
            .contains("LocalAIService"));
        assert!(!paths
            .local_data_dir
            .to_string_lossy()
            .contains("LocalAIService"));
        assert!(!paths
            .logs
            .log_dir
            .to_string_lossy()
            .contains("LocalAIService"));
        assert!(!paths
            .config_path
            .to_string_lossy()
            .contains("LocalRerankServer"));
        assert!(!paths
            .local_data_dir
            .to_string_lossy()
            .contains("LocalRerankServer"));
        assert!(!paths
            .logs
            .log_dir
            .to_string_lossy()
            .contains("LocalRerankServer"));
    }

    #[test]
    fn download_dirs_include_bundled_resource_downloads() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        let local_data_dir = temp_dir.path().join("local-data");
        let resource_dir = temp_dir.path().join("resources");

        let download_dirs = resolve_download_dirs(
            &exe_dir,
            &local_data_dir,
            &[resource_dir.clone()],
            None,
            temp_dir.path(),
        );

        assert_eq!(download_dirs[0], local_data_dir.join("downloads"));
        assert_eq!(download_dirs[1], exe_dir.join("downloads"));
        assert_eq!(download_dirs[2], resource_dir.join("downloads"));
    }

    #[test]
    fn load_initial_config_uses_persisted_config_with_defaults() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let config_path = temp_dir.path().join("config").join("config.json");
        fs::create_dir_all(config_path.parent().unwrap()).expect("config dir should be created");
        fs::write(
            &config_path,
            r#"{"host":"0.0.0.0","rerank":{"model_path":"C:\\models\\selected.gguf"}}"#,
        )
        .expect("config should be written");
        let paths = test_paths(temp_dir.path().join("app"), config_path);

        let initial = load_initial_config(&paths).expect("persisted config should load");

        assert_eq!(initial.config.host, "0.0.0.0");
        assert_eq!(initial.config.rerank.port, AppConfig::default().rerank.port);
        assert_eq!(
            initial.config.rerank.model_path,
            r#"C:\models\selected.gguf"#
        );
        assert!(initial.warning.is_none());
    }

    #[test]
    fn malformed_initial_config_falls_back_to_defaults_with_warning() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        let models_dir = exe_dir.join("models");
        fs::create_dir_all(&models_dir).expect("models dir should be created");
        fs::write(models_dir.join("bge-reranker.gguf"), b"reranker")
            .expect("model should be written");
        let config_path = temp_dir.path().join("config").join("config.json");
        fs::create_dir_all(config_path.parent().unwrap()).expect("config dir should be created");
        fs::write(&config_path, "{not valid json").expect("bad config should be written");
        let paths = test_paths(exe_dir, config_path);

        let initial = load_initial_config(&paths).expect("bad config should not abort startup");

        assert_eq!(initial.config.host, AppConfig::default().host);
        assert!(initial
            .config
            .rerank
            .model_path
            .ends_with("bge-reranker.gguf"));
        let warning = initial.warning.expect("bad config should produce warning");
        assert!(warning.contains("Failed to load config"));
        assert!(warning.contains("config.json"));
    }

    #[test]
    fn save_config_file_atomically_writes_pretty_final_json() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let config_path = temp_dir.path().join("config").join("config.json");
        let config = AppConfig {
            host: "0.0.0.0".to_string(),
            rerank: crate::config::ServiceConfig::new(true, 9001, String::new()),
            ..AppConfig::default()
        };

        save_config_file_atomic(&config_path, &config).expect("config should be saved");

        let contents = fs::read_to_string(&config_path).expect("config should be readable");
        assert!(contents.contains("\n  \"host\": \"0.0.0.0\""));
        assert!(contents.contains("\n  \"rerank\""));
        assert!(contents.contains("\n    \"port\": 9001"));
        assert_eq!(
            AppConfig::from_json_with_defaults(&contents).expect("config should parse"),
            config
        );
        assert!(
            fs::read_dir(config_path.parent().unwrap())
                .expect("config dir should be readable")
                .all(|entry| !entry
                    .expect("dir entry should be readable")
                    .file_name()
                    .to_string_lossy()
                    .contains(".tmp-")),
            "atomic temp file should be renamed away"
        );
    }

    #[tokio::test]
    async fn save_config_clears_stale_failed_model_error_when_model_path_changes() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        fs::create_dir_all(exe_dir.join("models")).expect("models dir should be created");
        let selected = temp_dir
            .path()
            .join("external-models")
            .join("selected-rerank.gguf");
        fs::create_dir_all(selected.parent().unwrap()).expect("model dir should be created");
        fs::write(&selected, b"selected").expect("model should be written");
        let paths = test_paths(exe_dir, temp_dir.path().join("config").join("config.json"));
        let state: SharedAppState = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));
        {
            let mut state = state.lock().await;
            state.set_service_status(ServiceKind::Rerank, ServiceStatus::Failed);
            state.set_service_error(
                ServiceKind::Rerank,
                Some("Rerank: failed to resolve model".to_string()),
            );
        }
        let config = AppConfig {
            rerank: crate::config::ServiceConfig::new(
                true,
                8012,
                selected.to_string_lossy().into_owned(),
            ),
            ..AppConfig::default()
        };

        let snapshot = save_config_inner(config.clone(), &state, &paths)
            .await
            .expect("config should save");

        assert_eq!(snapshot.config.rerank.model_path, config.rerank.model_path);
        assert_eq!(snapshot.rerank.status, ServiceStatus::Stopped);
        assert!(snapshot.rerank.error.is_none());
        assert!(snapshot.error.is_none());
    }

    #[tokio::test]
    async fn get_snapshot_inner_returns_latest_runtime_status() {
        let state: SharedAppState = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));
        {
            let mut state = state.lock().await;
            state.set_service_status(ServiceKind::Rerank, ServiceStatus::Running);
            state.push_service_log(
                ServiceKind::Rerank,
                "llama-server.exe is running".to_string(),
            );
        }

        let snapshot = get_snapshot_inner(&state).await;

        assert_eq!(snapshot.rerank.status, ServiceStatus::Running);
        assert_eq!(
            snapshot.rerank.recent_logs,
            vec!["llama-server.exe is running".to_string()]
        );
    }

    #[test]
    fn dev_model_root_falls_back_to_cargo_manifest_models_dir() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("target").join("debug");
        let manifest_dir = temp_dir.path().join("src-tauri");
        let manifest_models_dir = manifest_dir.join("models");
        fs::create_dir_all(&exe_dir).expect("exe dir should be created");
        fs::create_dir_all(&manifest_models_dir).expect("manifest models dir should be created");

        let model_base = resolve_model_base_dir(&exe_dir, Some(&manifest_dir), temp_dir.path());

        assert_eq!(model_base, manifest_dir);
    }

    #[test]
    fn rerank_model_config_for_start_errors_when_configured_model_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        let models_dir = exe_dir.join("models");
        fs::create_dir_all(&models_dir).expect("models dir should be created");
        let fallback = models_dir.join("bge-reranker.gguf");
        fs::write(&fallback, b"reranker").expect("model should be written");
        let config = AppConfig {
            rerank: crate::config::ServiceConfig::new(
                true,
                8012,
                models_dir
                    .join("missing.gguf")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..AppConfig::default()
        };

        let error = resolve_service_model_config_for_start(&exe_dir, &config, ServiceKind::Rerank)
            .expect_err("configured missing model should not fall back");

        assert!(error
            .to_string()
            .contains("configured Rerank model does not exist"));
        assert!(error.to_string().contains("missing.gguf"));
    }

    #[test]
    fn selected_rerank_model_path_prefers_existing_configured_model() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        let models_dir = exe_dir.join("models");
        fs::create_dir_all(&models_dir).expect("models dir should be created");
        let selected = models_dir.join("selected.gguf");
        fs::write(&selected, b"selected").expect("model should be written");
        fs::write(models_dir.join("bge-reranker.gguf"), b"reranker")
            .expect("model should be written");
        let config = AppConfig {
            rerank: crate::config::ServiceConfig::new(
                true,
                8012,
                selected.to_string_lossy().into_owned(),
            ),
            ..AppConfig::default()
        };

        let model_path = selected_service_model_path(&exe_dir, &config, ServiceKind::Rerank)
            .expect("model should be selected");

        assert_eq!(
            model_path,
            dunce::canonicalize(&selected).expect("selected model path should canonicalize")
        );
    }

    #[test]
    fn selected_rerank_model_path_errors_when_configured_model_is_stale() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        let models_dir = exe_dir.join("models");
        fs::create_dir_all(&models_dir).expect("models dir should be created");
        let fallback = models_dir.join("bge-reranker.gguf");
        fs::write(&fallback, b"reranker").expect("model should be written");
        let config = AppConfig {
            rerank: crate::config::ServiceConfig::new(
                true,
                8012,
                models_dir
                    .join("missing.gguf")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..AppConfig::default()
        };

        let error = selected_service_model_path(&exe_dir, &config, ServiceKind::Rerank)
            .expect_err("stale configured model should return an error");

        assert!(error
            .to_string()
            .contains("configured Rerank model does not exist"));
        assert!(error.to_string().contains("missing.gguf"));
    }

    #[test]
    fn selected_service_model_path_errors_when_no_gguf_exists() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let exe_dir = temp_dir.path().join("app");
        fs::create_dir_all(exe_dir.join("models")).expect("models dir should be created");

        let error =
            selected_service_model_path(&exe_dir, &AppConfig::default(), ServiceKind::Rerank)
                .expect_err("missing models should return an error");

        assert!(error.to_string().contains("No GGUF model found"));
        assert!(error.to_string().contains("models"));
    }

    #[test]
    fn embedding_test_request_uses_selected_cohere_protocol() {
        let config = AppConfig {
            embedding: crate::config::ServiceConfig::new_for(
                ServiceKind::Embedding,
                true,
                8013,
                String::new(),
            )
            .with_protocol("cohere_embed_v2"),
            ..AppConfig::default()
        };

        let (url, body) = embedding_test_request(&config);

        assert_eq!(url, "http://127.0.0.1:8013/v2/embed");
        assert_eq!(
            body,
            json!({
                "model": "local-embedding",
                "texts": ["hello world"],
                "input_type": "search_document"
            })
        );
    }

    #[test]
    fn rerank_test_request_uses_selected_voyage_protocol() {
        let config = AppConfig {
            rerank: crate::config::ServiceConfig::new(true, 8012, String::new())
                .with_protocol("voyage_rerank_v1"),
            ..AppConfig::default()
        };

        let (url, body) = rerank_test_request(&config);

        assert_eq!(url, "http://127.0.0.1:8012/rerank");
        assert_eq!(body["top_k"], json!(2));
        assert!(body.get("top_n").is_none());
    }

    fn test_paths(exe_dir: PathBuf, config_path: PathBuf) -> AppPaths {
        let local_data_dir = exe_dir.join("local-data");
        AppPaths {
            exe_dir,
            model_base_dir: local_data_dir
                .parent()
                .map(|parent| parent.to_path_buf())
                .unwrap_or_else(|| local_data_dir.clone()),
            config_path,
            download_dirs: vec![local_data_dir.join("downloads")],
            runtime_root: local_data_dir.join("runtimes"),
            logs: log_paths(&local_data_dir),
            local_data_dir,
        }
    }
}
