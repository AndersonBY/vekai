use crate::config::AppConfig;
use crate::fastpath::FastPathLayout;
use crate::llama_args::build_service_llama_server_args_for_endpoint;
use crate::logging::{append_log, prepare_server_log, LogPaths};
use crate::protocol_proxy::{
    plan_protocol_launch, start_protocol_proxy, ProtocolProxyConfig, ProtocolProxyHandle,
};
use crate::runtime::RuntimeLayout;
use crate::service::ServiceKind;
use crate::state::{ServiceStatus, SharedAppState};
use anyhow::{anyhow, bail, Context};
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const HEALTH_TIMEOUT: Duration = Duration::from_secs(90);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_millis(250);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(200);
const EXIT_MONITOR_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CRASH_RESTART_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct ProcessManager {
    kind: ServiceKind,
    child: Arc<Mutex<Option<ManagedChild>>>,
    operation_lock: Arc<Mutex<()>>,
    shutdown_requested: Arc<AtomicBool>,
    restart_state: Arc<Mutex<RestartState>>,
    next_child_generation: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
struct ProcessSupervisor {
    kind: ServiceKind,
    child: Arc<Mutex<Option<ManagedChild>>>,
    operation_lock: Arc<Mutex<()>>,
    shutdown_requested: Arc<AtomicBool>,
    restart_state: Arc<Mutex<RestartState>>,
    next_child_generation: Arc<AtomicU64>,
}

type ChildGeneration = u64;

#[derive(Debug)]
struct ManagedChild {
    child: Child,
    generation: ChildGeneration,
    backend: ServiceBackend,
    proxy: Option<ProtocolProxyHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceBackend {
    Llama,
    EmbeddingFastPath,
}

impl ServiceBackend {
    fn process_name(self) -> &'static str {
        match self {
            Self::Llama => "llama-server.exe",
            Self::EmbeddingFastPath => "vekai-embedding-fastpath.exe",
        }
    }
}

#[derive(Debug, Clone)]
struct ServiceLaunch {
    backend: ServiceBackend,
    program: PathBuf,
    current_dir: PathBuf,
    path_prefix: Option<PathBuf>,
    args: Vec<String>,
    health_url: String,
    ports: Vec<LaunchPort>,
    proxy_config: Option<ProtocolProxyConfig>,
}

#[derive(Debug, Clone)]
struct LaunchPort {
    host: String,
    port: u16,
}

#[derive(Debug, Clone)]
struct RestartContext {
    config: AppConfig,
    runtime: RuntimeLayout,
    model_path: PathBuf,
    state: SharedAppState,
    logs: LogPaths,
    fastpath: Option<FastPathLayout>,
}

#[derive(Debug, Default)]
struct RestartState {
    context: Option<RestartContext>,
    last_crash_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartReason {
    Manual,
    AutomaticRestart,
}

fn service_message(kind: ServiceKind, message: &str) -> String {
    match kind {
        ServiceKind::Rerank => message.to_string(),
        ServiceKind::Embedding => format!("{}: {message}", kind.display_name()),
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::for_service(ServiceKind::Rerank)
    }

    pub fn for_service(kind: ServiceKind) -> Self {
        Self {
            kind,
            child: Arc::new(Mutex::new(None)),
            operation_lock: Arc::new(Mutex::new(())),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            restart_state: Arc::new(Mutex::new(RestartState::default())),
            next_child_generation: Arc::new(AtomicU64::new(1)),
        }
    }

    fn supervisor(&self) -> ProcessSupervisor {
        ProcessSupervisor {
            kind: self.kind,
            child: self.child.clone(),
            operation_lock: self.operation_lock.clone(),
            shutdown_requested: self.shutdown_requested.clone(),
            restart_state: self.restart_state.clone(),
            next_child_generation: self.next_child_generation.clone(),
        }
    }

    pub fn request_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
    }

    pub fn kind(&self) -> ServiceKind {
        self.kind
    }

    pub async fn start(
        &self,
        config: AppConfig,
        runtime: RuntimeLayout,
        model_path: PathBuf,
        state: SharedAppState,
        logs: LogPaths,
    ) -> anyhow::Result<()> {
        self.start_with_fastpath(config, runtime, model_path, state, logs, None)
            .await
    }

    pub async fn start_with_fastpath(
        &self,
        config: AppConfig,
        runtime: RuntimeLayout,
        model_path: PathBuf,
        state: SharedAppState,
        logs: LogPaths,
        fastpath: Option<FastPathLayout>,
    ) -> anyhow::Result<()> {
        let context = RestartContext {
            config,
            runtime,
            model_path,
            state,
            logs,
            fastpath,
        };

        start_with_context(self.supervisor(), context, StartReason::Manual).await
    }

    pub async fn stop(&self, state: SharedAppState, logs: LogPaths) -> anyhow::Result<()> {
        let _operation_guard = self.operation_lock.lock().await;
        let stopping_message = service_message(self.kind, "Stopping llama-server.exe");
        set_status(
            &state,
            self.kind,
            ServiceStatus::Stopping,
            None,
            Some(stopping_message.as_str()),
        )
        .await;
        append_log(&logs.app_log, &stopping_message)?;

        kill_child(&self.supervisor()).await?;

        let stopped_message = service_message(self.kind, "llama-server.exe stopped");
        set_status(
            &state,
            self.kind,
            ServiceStatus::Stopped,
            None,
            Some(stopped_message.as_str()),
        )
        .await;
        append_log(&logs.app_log, &stopped_message)?;
        Ok(())
    }

    #[cfg(test)]
    fn spawn_exit_monitor(
        &self,
        child_generation: ChildGeneration,
        state: SharedAppState,
        logs: LogPaths,
    ) {
        spawn_exit_monitor(self.supervisor(), child_generation, state, logs);
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn start_with_context(
    supervisor: ProcessSupervisor,
    context: RestartContext,
    reason: StartReason,
) -> anyhow::Result<()> {
    let mut next_context = context;
    let mut next_reason = reason;

    loop {
        let attempted_context = next_context.clone();
        let startup_exit =
            match start_with_context_attempt(&supervisor, next_context, next_reason).await? {
                StartAttemptOutcome::Finished => return Ok(()),
                StartAttemptOutcome::StartupExited {
                    backend,
                    state,
                    logs,
                    message,
                } => (backend, state, logs, message),
            };

        let (backend, state, logs, message) = startup_exit;
        if backend == ServiceBackend::EmbeddingFastPath {
            let fallback_message = service_message(
                supervisor.kind,
                &format!("Embedding fast path unavailable; falling back to llama.cpp: {message}"),
            );
            push_and_append_log(supervisor.kind, &state, &logs, &fallback_message).await;
            next_context = RestartContext {
                fastpath: None,
                ..attempted_context
            };
            next_reason = StartReason::AutomaticRestart;
            continue;
        }
        match unexpected_exit_decision(&supervisor, &state, &message).await {
            RestartDecision::Ignore => return Ok(()),
            RestartDecision::Fail(failure_message) => {
                mark_unexpected_exit_failed(supervisor.kind, &state, &logs, &failure_message).await;
                return Err(anyhow!(failure_message));
            }
            RestartDecision::Restart(context) => {
                let restart_message = service_message(
                    supervisor.kind,
                    &format!("Restarting llama-server.exe after unexpected exit: {message}"),
                );
                push_and_append_log(supervisor.kind, &state, &logs, &restart_message).await;
                next_context = context;
                next_reason = StartReason::AutomaticRestart;
            }
        }
    }
}

enum StartAttemptOutcome {
    Finished,
    StartupExited {
        backend: ServiceBackend,
        state: SharedAppState,
        logs: LogPaths,
        message: String,
    },
}

async fn start_with_context_attempt(
    supervisor: &ProcessSupervisor,
    context: RestartContext,
    reason: StartReason,
) -> anyhow::Result<StartAttemptOutcome> {
    let state = context.state.clone();
    let logs = context.logs.clone();
    let launch = match build_service_launch_with_fastpath(
        supervisor.kind,
        &context.config,
        &context.model_path,
        &context.runtime,
        context.fastpath.as_ref(),
    ) {
        Ok(launch) => launch,
        Err(error) => {
            let _operation_guard = supervisor.operation_lock.lock().await;
            fail_start(
                supervisor.kind,
                &state,
                &logs,
                &format!("failed to build llama-server args: {error}"),
            )
            .await?;
            return Err(error);
        }
    };
    let child_generation = {
        let _operation_guard = supervisor.operation_lock.lock().await;
        reject_if_shutdown_requested(supervisor)?;
        if reason == StartReason::AutomaticRestart
            && !restart_still_needed(supervisor.kind, &state).await
        {
            return Ok(StartAttemptOutcome::Finished);
        }
        reject_if_child_running(supervisor).await?;

        if let Err(error) = ensure_launch_ports_available(supervisor.kind, &launch) {
            fail_start(supervisor.kind, &state, &logs, &error.to_string()).await?;
            return Err(error);
        }

        let process_name = launch.backend.process_name();
        let starting_message =
            service_message(supervisor.kind, &format!("Starting {process_name}"));
        set_status(
            &state,
            supervisor.kind,
            ServiceStatus::Starting,
            None,
            Some(starting_message.as_str()),
        )
        .await;
        append_log(&logs.app_log, &starting_message)?;

        let server_log = logs.server_log_for(supervisor.kind).to_path_buf();
        let child = match prepare_server_log(&server_log).and_then(|_| spawn_server(&launch)) {
            Ok(child) => child,
            Err(error) => {
                if launch.backend == ServiceBackend::EmbeddingFastPath {
                    return Ok(StartAttemptOutcome::StartupExited {
                        backend: launch.backend,
                        state,
                        logs,
                        message: error.to_string(),
                    });
                }
                fail_start(supervisor.kind, &state, &logs, &error.to_string()).await?;
                return Err(error);
            }
        };

        let child_generation = store_child_with_log_readers(
            supervisor,
            child,
            launch.backend,
            state.clone(),
            server_log,
        )
        .await?;
        let restart_context = if launch.backend == ServiceBackend::EmbeddingFastPath {
            RestartContext {
                fastpath: None,
                ..context.clone()
            }
        } else {
            context.clone()
        };
        remember_restart_context(supervisor, restart_context, reason).await;

        child_generation
    };

    match wait_for_health_or_child_exit(supervisor, child_generation, &launch.health_url).await {
        Ok(StartupHealthWait::Healthy) => {}
        Ok(StartupHealthWait::ChildExited(message)) => {
            return Ok(StartAttemptOutcome::StartupExited {
                backend: launch.backend,
                state,
                logs,
                message,
            });
        }
        Ok(StartupHealthWait::Superseded) => {
            return Ok(StartAttemptOutcome::Finished);
        }
        Err(error) => {
            let _operation_guard = supervisor.operation_lock.lock().await;
            if !status_allows_unexpected_exit_handling(supervisor.kind, &state).await {
                return Ok(StartAttemptOutcome::Finished);
            }

            match startup_child_state(supervisor, child_generation).await {
                Ok(StartupChildState::CurrentAlive) => {}
                Ok(StartupChildState::CurrentExited(message)) => {
                    return Ok(StartAttemptOutcome::StartupExited {
                        backend: launch.backend,
                        state,
                        logs,
                        message,
                    });
                }
                Ok(StartupChildState::Missing | StartupChildState::Replaced) => {
                    return Ok(StartAttemptOutcome::Finished);
                }
                Err(inspect_error) => {
                    fail_startup_inspect(supervisor.kind, &state, &logs, &inspect_error).await?;
                    return Err(inspect_error);
                }
            }

            let message = service_message(
                supervisor.kind,
                &format!(
                    "{} health check failed: {error}",
                    launch.backend.process_name()
                ),
            );
            let cleanup_result = kill_child(supervisor).await;
            if launch.backend == ServiceBackend::EmbeddingFastPath && cleanup_result.is_ok() {
                return Ok(StartAttemptOutcome::StartupExited {
                    backend: launch.backend,
                    state,
                    logs,
                    message,
                });
            }
            set_status(
                &state,
                supervisor.kind,
                ServiceStatus::Failed,
                Some(message.clone()),
                Some(message.as_str()),
            )
            .await;
            append_log(&logs.app_log, &message)?;

            if let Err(cleanup_error) = cleanup_result {
                return Err(anyhow!(
                    "{error}; also failed to stop llama-server.exe: {cleanup_error}"
                ));
            }
            return Err(error);
        }
    }

    if let Some(proxy_config) = launch.proxy_config.clone() {
        match start_proxy_for_child(supervisor, child_generation, &context.config, proxy_config)
            .await
        {
            Ok(true) => {}
            Ok(false) => return Ok(StartAttemptOutcome::Finished),
            Err(error) => {
                let _operation_guard = supervisor.operation_lock.lock().await;
                let message = service_message(
                    supervisor.kind,
                    &format!("protocol proxy health check failed: {error}"),
                );
                let cleanup_result = kill_child(supervisor).await;
                set_status(
                    &state,
                    supervisor.kind,
                    ServiceStatus::Failed,
                    Some(message.clone()),
                    Some(message.as_str()),
                )
                .await;
                append_log(&logs.app_log, &message)?;

                if let Err(cleanup_error) = cleanup_result {
                    return Err(anyhow!(
                        "{error}; also failed to stop llama-server.exe: {cleanup_error}"
                    ));
                }
                return Err(error);
            }
        }
    }

    let _operation_guard = supervisor.operation_lock.lock().await;
    if supervisor.shutdown_requested.load(Ordering::SeqCst)
        || !status_allows_unexpected_exit_handling(supervisor.kind, &state).await
    {
        return Ok(StartAttemptOutcome::Finished);
    }

    match startup_child_state(supervisor, child_generation).await {
        Ok(StartupChildState::CurrentAlive) => {}
        Ok(StartupChildState::CurrentExited(message)) => {
            return Ok(StartAttemptOutcome::StartupExited {
                backend: launch.backend,
                state,
                logs,
                message,
            });
        }
        Ok(StartupChildState::Missing | StartupChildState::Replaced) => {
            return Ok(StartAttemptOutcome::Finished);
        }
        Err(error) => {
            fail_startup_inspect(supervisor.kind, &state, &logs, &error).await?;
            return Err(error);
        }
    };

    let running_message = service_message(
        supervisor.kind,
        &format!("{} is running", launch.backend.process_name()),
    );
    set_status(
        &state,
        supervisor.kind,
        ServiceStatus::Running,
        None,
        Some(running_message.as_str()),
    )
    .await;
    append_log(&logs.app_log, &running_message)?;
    spawn_exit_monitor(supervisor.clone(), child_generation, state, logs);
    Ok(StartAttemptOutcome::Finished)
}

#[cfg(test)]
fn build_service_launch(
    service: ServiceKind,
    config: &AppConfig,
    model_path: &Path,
) -> anyhow::Result<ServiceLaunch> {
    let runtime = RuntimeLayout {
        runtime_dir: PathBuf::new(),
        server_exe: PathBuf::from("llama-server.exe"),
        marker_path: PathBuf::new(),
    };
    build_service_launch_with_fastpath(service, config, model_path, &runtime, None)
}

fn build_service_launch_with_fastpath(
    service: ServiceKind,
    config: &AppConfig,
    model_path: &Path,
    runtime: &RuntimeLayout,
    fastpath: Option<&FastPathLayout>,
) -> anyhow::Result<ServiceLaunch> {
    let public_port = config.service_port(service);
    let protocol = config.service_protocol(service);
    let plan = plan_protocol_launch(service, &config.host, public_port, protocol)?;
    let ports = launch_ports_for_plan(&plan, &config.host);
    let (host, port) = if plan.use_proxy {
        ("127.0.0.1".to_string(), plan.llama_port)
    } else {
        (config.host.clone(), public_port)
    };
    let (backend, program, current_dir, path_prefix, args) = if service == ServiceKind::Embedding {
        if let Some(fastpath) = fastpath {
            (
                ServiceBackend::EmbeddingFastPath,
                fastpath.server_exe.clone(),
                fastpath.runtime_dir.clone(),
                Some(fastpath.runtime_dir.clone()),
                vec![
                    "--host".to_string(),
                    host.clone(),
                    "--port".to_string(),
                    port.to_string(),
                    "--model".to_string(),
                    fastpath.model_path.to_string_lossy().into_owned(),
                    "--tokenizer".to_string(),
                    fastpath.tokenizer_path.to_string_lossy().into_owned(),
                    "--ort-dll".to_string(),
                    fastpath.ort_dll.to_string_lossy().into_owned(),
                ],
            )
        } else {
            let args = build_service_llama_server_args_for_endpoint(
                service, config, model_path, &host, port,
            )?;
            (
                ServiceBackend::Llama,
                runtime.server_exe.clone(),
                runtime.runtime_dir.clone(),
                None,
                args,
            )
        }
    } else {
        let args =
            build_service_llama_server_args_for_endpoint(service, config, model_path, &host, port)?;
        (
            ServiceBackend::Llama,
            runtime.server_exe.clone(),
            runtime.runtime_dir.clone(),
            None,
            args,
        )
    };
    let health_url = if plan.use_proxy {
        format!("http://127.0.0.1:{port}/health")
    } else {
        health_url(service, config)
    };

    Ok(ServiceLaunch {
        backend,
        program,
        current_dir,
        path_prefix,
        args,
        health_url,
        ports,
        proxy_config: plan.proxy_config,
    })
}

fn launch_ports_for_plan(
    plan: &crate::protocol_proxy::ProtocolLaunchPlan,
    public_host: &str,
) -> Vec<LaunchPort> {
    let mut ports = vec![LaunchPort {
        host: public_host.to_string(),
        port: plan.public_port,
    }];
    if plan.use_proxy {
        ports.push(LaunchPort {
            host: "127.0.0.1".to_string(),
            port: plan.llama_port,
        });
    }
    ports
}

fn ensure_launch_ports_available(
    service: ServiceKind,
    launch: &ServiceLaunch,
) -> anyhow::Result<()> {
    for port in &launch.ports {
        ensure_port_available(service, &port.host, port.port)?;
    }

    Ok(())
}

fn ensure_port_available(service: ServiceKind, host: &str, port: u16) -> anyhow::Result<()> {
    for bind_host in probe_bind_hosts(host) {
        let address = format!("{bind_host}:{port}");
        match StdTcpListener::bind(&address) {
            Ok(listener) => drop(listener),
            Err(error) => {
                bail!(
                    "{} port is already in use at {address}: {error}",
                    service.display_name()
                );
            }
        }
    }

    Ok(())
}

fn probe_bind_hosts(host: &str) -> Vec<String> {
    match host {
        "0.0.0.0" => vec!["127.0.0.1".to_string()],
        "::" => vec!["[::1]".to_string()],
        value if value.contains(':') && !value.starts_with('[') => vec![format!("[{value}]")],
        value => vec![value.to_string()],
    }
}

async fn start_proxy_for_child(
    supervisor: &ProcessSupervisor,
    child_generation: ChildGeneration,
    config: &AppConfig,
    proxy_config: ProtocolProxyConfig,
) -> anyhow::Result<bool> {
    let proxy = start_protocol_proxy(proxy_config).await?;
    if !attach_proxy_to_child(supervisor, child_generation, proxy).await? {
        return Ok(false);
    }

    verify_public_proxy(supervisor.kind, config).await?;
    Ok(true)
}

async fn attach_proxy_to_child(
    supervisor: &ProcessSupervisor,
    child_generation: ChildGeneration,
    proxy: ProtocolProxyHandle,
) -> anyhow::Result<bool> {
    let mut proxy = Some(proxy);
    let attached = {
        let mut child_guard = supervisor.child.lock().await;
        match child_guard.as_mut() {
            Some(managed_child) if managed_child.generation == child_generation => {
                managed_child.proxy = proxy.take();
                true
            }
            _ => false,
        }
    };

    if let Some(proxy) = proxy {
        proxy.shutdown().await;
    }

    Ok(attached)
}

async fn verify_public_proxy(service: ServiceKind, config: &AppConfig) -> anyhow::Result<()> {
    let url = health_url(service, config);
    let response = reqwest::Client::new()
        .get(&url)
        .timeout(HEALTH_REQUEST_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("failed to GET protocol proxy health {url}"))?;

    if response.status().is_success() {
        return Ok(());
    }

    Err(anyhow!(
        "protocol proxy health {url} returned HTTP {}",
        response.status()
    ))
}

async fn remember_restart_context(
    supervisor: &ProcessSupervisor,
    context: RestartContext,
    reason: StartReason,
) {
    let mut restart_state = supervisor.restart_state.lock().await;
    restart_state.context = Some(context);
    if reason == StartReason::Manual {
        restart_state.last_crash_at = None;
    }
}

fn reject_if_shutdown_requested(supervisor: &ProcessSupervisor) -> anyhow::Result<()> {
    if supervisor.shutdown_requested.load(Ordering::SeqCst) {
        bail!("shutdown requested; refusing to start llama-server.exe");
    }

    Ok(())
}

async fn restart_still_needed(kind: ServiceKind, state: &SharedAppState) -> bool {
    let state = state.lock().await;
    matches!(
        state.service_status(kind),
        ServiceStatus::Starting | ServiceStatus::Running
    )
}

async fn reject_if_child_running(supervisor: &ProcessSupervisor) -> anyhow::Result<()> {
    let mut child_guard = supervisor.child.lock().await;
    let mut stale_proxy = None;

    if let Some(managed_child) = child_guard.as_mut() {
        match managed_child.child.try_wait() {
            Ok(Some(_status)) => {
                stale_proxy = managed_child.proxy.take();
                *child_guard = None;
            }
            Ok(None) => bail!("llama-server.exe is already running"),
            Err(error) => {
                return Err(anyhow!(error).context("failed to inspect llama-server.exe"));
            }
        }
    }

    drop(child_guard);
    if let Some(proxy) = stale_proxy {
        proxy.shutdown().await;
    }

    Ok(())
}

async fn store_child_with_log_readers(
    supervisor: &ProcessSupervisor,
    mut child: Child,
    backend: ServiceBackend,
    state: SharedAppState,
    server_log: PathBuf,
) -> anyhow::Result<ChildGeneration> {
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(supervisor.kind, stdout, server_log.clone(), state.clone());
    }

    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(supervisor.kind, stderr, server_log, state);
    }

    let mut child_guard = supervisor.child.lock().await;
    if child_guard.is_some() {
        let _ = child.kill().await;
        bail!("llama-server.exe is already running");
    }

    let generation = supervisor
        .next_child_generation
        .fetch_add(1, Ordering::Relaxed);
    *child_guard = Some(ManagedChild {
        child,
        generation,
        backend,
        proxy: None,
    });
    Ok(generation)
}

async fn kill_child(supervisor: &ProcessSupervisor) -> anyhow::Result<bool> {
    let mut child_guard = supervisor.child.lock().await;
    let had_child = child_guard.is_some();

    let result = if let Some(managed_child) = child_guard.as_mut() {
        stop_child(&mut managed_child.child).await
    } else {
        ChildStopResult::NoChild
    };
    let proxy = if result.should_clear_child() {
        child_guard
            .as_mut()
            .and_then(|managed_child| managed_child.proxy.take())
    } else {
        None
    };

    if result.should_clear_child() {
        *child_guard = None;
    }

    drop(child_guard);

    if result.should_clear_child() {
        if let Some(proxy) = proxy {
            proxy.shutdown().await;
        }
    }

    result.into_result().map(|()| had_child)
}

enum StartupHealthWait {
    Healthy,
    ChildExited(String),
    Superseded,
}

enum StartupChildState {
    CurrentAlive,
    CurrentExited(String),
    Missing,
    Replaced,
}

async fn wait_for_health_or_child_exit(
    supervisor: &ProcessSupervisor,
    child_generation: ChildGeneration,
    health_url: &str,
) -> anyhow::Result<StartupHealthWait> {
    let client = reqwest::Client::new();
    let deadline = Instant::now() + HEALTH_TIMEOUT;

    while Instant::now() < deadline {
        match startup_child_state(supervisor, child_generation).await? {
            StartupChildState::CurrentAlive => {}
            StartupChildState::CurrentExited(message) => {
                return Ok(StartupHealthWait::ChildExited(message));
            }
            StartupChildState::Missing | StartupChildState::Replaced => {
                return Ok(StartupHealthWait::Superseded);
            }
        }

        if let Ok(response) = client
            .get(health_url)
            .timeout(HEALTH_REQUEST_TIMEOUT)
            .send()
            .await
        {
            if response.status().is_success() {
                return Ok(StartupHealthWait::Healthy);
            }
        }

        sleep(HEALTH_POLL_INTERVAL).await;
    }

    Err(anyhow!("health check timed out for {health_url}"))
}

async fn startup_child_state(
    supervisor: &ProcessSupervisor,
    child_generation: ChildGeneration,
) -> anyhow::Result<StartupChildState> {
    let mut child_guard = supervisor.child.lock().await;
    let Some(managed_child) = child_guard.as_mut() else {
        return Ok(StartupChildState::Missing);
    };

    if managed_child.generation != child_generation {
        return Ok(StartupChildState::Replaced);
    }

    match managed_child.child.try_wait() {
        Ok(Some(status)) => {
            let backend = managed_child.backend;
            let proxy = managed_child.proxy.take();
            *child_guard = None;
            drop(child_guard);
            if let Some(proxy) = proxy {
                proxy.shutdown().await;
            }
            Ok(StartupChildState::CurrentExited(format!(
                "{} exited unexpectedly: {status}",
                backend.process_name()
            )))
        }
        Ok(None) => Ok(StartupChildState::CurrentAlive),
        Err(error) => Err(anyhow!(error).context("failed to inspect llama-server.exe")),
    }
}

fn spawn_server(launch: &ServiceLaunch) -> anyhow::Result<Child> {
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .current_dir(&launch.current_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path_prefix) = &launch.path_prefix {
        let mut paths = vec![path_prefix.clone()];
        if let Some(current_path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&current_path));
        }
        command.env(
            "PATH",
            std::env::join_paths(paths).context("failed to construct fast path DLL search PATH")?,
        );
    }

    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    command
        .spawn()
        .with_context(|| format!("failed to start {}", launch.program.display()))
}

fn spawn_log_reader<T>(kind: ServiceKind, stream: T, log_path: PathBuf, state: SharedAppState)
where
    T: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream).lines();

        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    let _ = append_log(&log_path, &line);
                    let mut state = state.lock().await;
                    state.push_service_log(kind, line);
                }
                Ok(None) => break,
                Err(error) => {
                    let line = format!("failed to read llama-server.exe output: {error}");
                    let _ = append_log(&log_path, &line);
                    let mut state = state.lock().await;
                    state.push_service_log(kind, line);
                    break;
                }
            }
        }
    });
}

#[derive(Debug)]
enum ChildStopResult {
    NoChild,
    AlreadyExited,
    Killed,
    InspectFailed(anyhow::Error),
    KillFailed(anyhow::Error),
}

impl ChildStopResult {
    fn should_clear_child(&self) -> bool {
        matches!(self, Self::NoChild | Self::AlreadyExited | Self::Killed)
    }

    fn into_result(self) -> anyhow::Result<()> {
        match self {
            Self::NoChild | Self::AlreadyExited | Self::Killed => Ok(()),
            Self::InspectFailed(error) | Self::KillFailed(error) => Err(error),
        }
    }
}

async fn stop_child(child: &mut Child) -> ChildStopResult {
    match child.try_wait() {
        Ok(Some(_status)) => ChildStopResult::AlreadyExited,
        Ok(None) => match child.kill().await {
            Ok(()) => ChildStopResult::Killed,
            Err(error) => ChildStopResult::KillFailed(
                anyhow!(error).context("failed to kill llama-server.exe"),
            ),
        },
        Err(error) => ChildStopResult::InspectFailed(
            anyhow!(error).context("failed to inspect llama-server.exe"),
        ),
    }
}

fn spawn_exit_monitor(
    supervisor: ProcessSupervisor,
    child_generation: ChildGeneration,
    state: SharedAppState,
    logs: LogPaths,
) {
    tokio::spawn(async move {
        monitor_child_exit(supervisor, child_generation, state, logs).await;
    });
}

async fn monitor_child_exit(
    supervisor: ProcessSupervisor,
    child_generation: ChildGeneration,
    state: SharedAppState,
    logs: LogPaths,
) {
    loop {
        let exit_event = {
            let mut child_guard = supervisor.child.lock().await;
            let Some(managed_child) = child_guard.as_mut() else {
                return;
            };

            if managed_child.generation != child_generation {
                return;
            }

            match managed_child.child.try_wait() {
                Ok(Some(status)) => {
                    let backend = managed_child.backend;
                    let proxy = managed_child.proxy.take();
                    *child_guard = None;
                    drop(child_guard);
                    if let Some(proxy) = proxy {
                        proxy.shutdown().await;
                    }
                    Some(MonitorExitEvent::Exited(
                        backend,
                        format!("{} exited unexpectedly: {status}", backend.process_name()),
                    ))
                }
                Ok(None) => None,
                Err(error) => Some(MonitorExitEvent::MonitorFailed(format!(
                    "failed to monitor llama-server.exe exit: {error}"
                ))),
            }
        };

        if let Some(event) = exit_event {
            match event {
                MonitorExitEvent::Exited(backend, message) => {
                    handle_unexpected_exit(supervisor, state, logs, backend, message).await;
                }
                MonitorExitEvent::MonitorFailed(message) => {
                    mark_unexpected_exit_failed(supervisor.kind, &state, &logs, &message).await;
                }
            }
            return;
        }

        sleep(EXIT_MONITOR_POLL_INTERVAL).await;
    }
}

enum MonitorExitEvent {
    Exited(ServiceBackend, String),
    MonitorFailed(String),
}

fn health_url(kind: ServiceKind, config: &AppConfig) -> String {
    let host = normalize_probe_host(&config.host);
    format!("http://{host}:{}/health", config.service_port(kind))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrashExitAction {
    Ignore,
    Restart,
    Fail,
}

fn crash_exit_action(
    restart_on_crash: bool,
    last_crash_at: Option<Instant>,
    now: Instant,
    shutdown_requested: bool,
) -> CrashExitAction {
    if shutdown_requested {
        return CrashExitAction::Ignore;
    }

    if !restart_on_crash {
        return CrashExitAction::Fail;
    }

    match last_crash_at {
        Some(last_crash_at) if now.duration_since(last_crash_at) < CRASH_RESTART_WINDOW => {
            CrashExitAction::Fail
        }
        _ => CrashExitAction::Restart,
    }
}

async fn handle_unexpected_exit(
    supervisor: ProcessSupervisor,
    state: SharedAppState,
    logs: LogPaths,
    backend: ServiceBackend,
    message: String,
) -> UnexpectedExitOutcome {
    let restart_decision = if backend == ServiceBackend::EmbeddingFastPath {
        fastpath_fallback_decision(&supervisor, &state, &message).await
    } else {
        unexpected_exit_decision(&supervisor, &state, &message).await
    };

    match restart_decision {
        RestartDecision::Ignore => UnexpectedExitOutcome::Ignored,
        RestartDecision::Fail(failure_message) => {
            mark_unexpected_exit_failed(supervisor.kind, &state, &logs, &failure_message).await;
            UnexpectedExitOutcome::Failed
        }
        RestartDecision::Restart(context) => {
            let service = supervisor.kind;
            let restart_message = if backend == ServiceBackend::EmbeddingFastPath {
                service_message(
                    service,
                    &format!("Embedding fast path exited; falling back to llama.cpp: {message}"),
                )
            } else {
                service_message(
                    service,
                    &format!("Restarting llama-server.exe after unexpected exit: {message}"),
                )
            };
            push_and_append_log(service, &state, &logs, &restart_message).await;
            if let Err(error) =
                start_with_context(supervisor, context, StartReason::AutomaticRestart).await
            {
                let failure_message = service_message(
                    service,
                    &format!("Automatic restart of llama-server.exe failed: {error}"),
                );
                push_and_append_log(service, &state, &logs, &failure_message).await;
                return UnexpectedExitOutcome::Failed;
            }
            UnexpectedExitOutcome::Restarted
        }
    }
}

async fn fastpath_fallback_decision(
    supervisor: &ProcessSupervisor,
    state: &SharedAppState,
    message: &str,
) -> RestartDecision {
    if !status_allows_unexpected_exit_handling(supervisor.kind, state).await
        || supervisor.shutdown_requested.load(Ordering::SeqCst)
    {
        return RestartDecision::Ignore;
    }
    supervisor
        .restart_state
        .lock()
        .await
        .context
        .clone()
        .map(RestartDecision::Restart)
        .unwrap_or_else(|| RestartDecision::Fail(message.to_string()))
}

async fn unexpected_exit_decision(
    supervisor: &ProcessSupervisor,
    state: &SharedAppState,
    message: &str,
) -> RestartDecision {
    if !status_allows_unexpected_exit_handling(supervisor.kind, state).await {
        return RestartDecision::Ignore;
    }

    let now = Instant::now();
    let restart_on_crash = current_restart_on_crash(state).await;
    let mut restart_state = supervisor.restart_state.lock().await;
    let action = crash_exit_action(
        restart_on_crash,
        restart_state.last_crash_at,
        now,
        supervisor.shutdown_requested.load(Ordering::SeqCst),
    );

    match action {
        CrashExitAction::Ignore => RestartDecision::Ignore,
        CrashExitAction::Fail => {
            let failure_message = if restart_on_crash && restart_state.last_crash_at.is_some() {
                second_crash_failure_message(message)
            } else {
                message.to_string()
            };
            RestartDecision::Fail(failure_message)
        }
        CrashExitAction::Restart => {
            restart_state.last_crash_at = Some(now);
            match restart_state.context.clone() {
                Some(context) => RestartDecision::Restart(context),
                None => RestartDecision::Fail(message.to_string()),
            }
        }
    }
}

enum RestartDecision {
    Ignore,
    Restart(RestartContext),
    Fail(String),
}

enum UnexpectedExitOutcome {
    Ignored,
    Restarted,
    Failed,
}

async fn current_restart_on_crash(state: &SharedAppState) -> bool {
    state.lock().await.config.restart_on_crash
}

async fn status_allows_unexpected_exit_handling(kind: ServiceKind, state: &SharedAppState) -> bool {
    let state = state.lock().await;
    matches!(
        state.service_status(kind),
        ServiceStatus::Starting | ServiceStatus::Running
    )
}

fn second_crash_failure_message(message: &str) -> String {
    format!(
        "llama-server.exe exited unexpectedly after a recent automatic restart; manual restart required: {message}"
    )
}

async fn push_and_append_log(
    kind: ServiceKind,
    state: &SharedAppState,
    logs: &LogPaths,
    message: &str,
) {
    {
        let mut state = state.lock().await;
        state.push_service_log(kind, message);
    }

    let _ = append_log(&logs.app_log, message);
}

async fn set_status(
    state: &SharedAppState,
    kind: ServiceKind,
    status: ServiceStatus,
    error: Option<String>,
    log_line: Option<&str>,
) {
    let mut state = state.lock().await;
    state.set_service_status(kind, status);
    state.set_service_error(kind, error);
    if let Some(log_line) = log_line {
        state.push_service_log(kind, log_line);
    }
}

async fn fail_start(
    kind: ServiceKind,
    state: &SharedAppState,
    logs: &LogPaths,
    message: &str,
) -> anyhow::Result<()> {
    let message = service_message(
        kind,
        &format!("Failed to start llama-server.exe: {message}"),
    );
    set_status(
        state,
        kind,
        ServiceStatus::Failed,
        Some(message.clone()),
        Some(message.as_str()),
    )
    .await;
    append_log(&logs.app_log, &message)
}

async fn fail_startup_inspect(
    kind: ServiceKind,
    state: &SharedAppState,
    logs: &LogPaths,
    error: &anyhow::Error,
) -> anyhow::Result<()> {
    let message = service_message(
        kind,
        &format!("failed to inspect llama-server.exe after health check: {error}"),
    );
    set_status(
        state,
        kind,
        ServiceStatus::Failed,
        Some(message.clone()),
        Some(message.as_str()),
    )
    .await;
    append_log(&logs.app_log, &message)
}

async fn mark_unexpected_exit_failed(
    kind: ServiceKind,
    state: &SharedAppState,
    logs: &LogPaths,
    message: &str,
) {
    let should_log = {
        let mut state = state.lock().await;

        if !matches!(
            state.service_status(kind),
            ServiceStatus::Starting | ServiceStatus::Running
        ) {
            return;
        }

        state.set_service_status(kind, ServiceStatus::Failed);
        state.set_service_error(kind, Some(message.to_string()));
        state.push_service_log(kind, message);
        true
    };

    if !should_log {
        return;
    }

    let _ = append_log(&logs.app_log, message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServiceConfig};
    use crate::logging::LogPaths;
    use crate::state::{AppStateInner, ServiceStatus};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn new_starts_without_child() {
        let manager = ProcessManager::new();

        assert!(manager.child.lock().await.is_none());
    }

    #[tokio::test]
    async fn stop_without_child_sets_stopped_and_logs() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));

        manager
            .stop(state.clone(), logs.clone())
            .await
            .expect("stop without child should succeed");

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Stopped);
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Stopping llama-server.exe")));
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("llama-server.exe stopped")));

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Stopping llama-server.exe"));
        assert!(app_log.contains("llama-server.exe stopped"));
    }

    #[tokio::test]
    async fn start_after_shutdown_is_rejected_without_status_change_or_spawn() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().join("logs"),
            app_log: temp_dir.path().join("logs").join("app.log"),
            server_log: temp_dir.path().join("logs").join("server.log"),
            embedding_server_log: temp_dir.path().join("logs").join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));
        let runtime = RuntimeLayout {
            runtime_dir: temp_dir.path().join("runtime"),
            server_exe: temp_dir
                .path()
                .join("runtime")
                .join("missing-llama-server.exe"),
            marker_path: temp_dir.path().join("runtime").join("runtime.json"),
        };
        let model_path = temp_dir.path().join("models").join("model.gguf");

        manager.request_shutdown();
        let error = manager
            .start(
                AppConfig::default(),
                runtime,
                model_path,
                state.clone(),
                logs.clone(),
            )
            .await
            .expect_err("start after shutdown should be rejected");

        assert!(error.to_string().contains("shutdown requested"));
        assert!(manager.child.lock().await.is_none());
        assert!(!logs.app_log.exists());

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Stopped);
        assert!(state.error.is_none());
        assert!(state.recent_logs.is_empty());
    }

    #[test]
    fn health_url_wraps_ipv6_hosts() {
        let config = AppConfig {
            host: "::1".to_string(),
            rerank: ServiceConfig::new(true, 8012, String::new()),
            ..AppConfig::default()
        };

        assert_eq!(
            health_url(ServiceKind::Rerank, &config),
            "http://[::1]:8012/health"
        );
    }

    #[test]
    fn health_url_probes_loopback_for_wildcard_bind_hosts() {
        let ipv4 = AppConfig {
            host: "0.0.0.0".to_string(),
            rerank: ServiceConfig::new(true, 8012, String::new()),
            ..AppConfig::default()
        };
        let ipv6 = AppConfig {
            host: "::".to_string(),
            rerank: ServiceConfig::new(true, 8012, String::new()),
            ..AppConfig::default()
        };

        assert_eq!(
            health_url(ServiceKind::Rerank, &ipv4),
            "http://127.0.0.1:8012/health"
        );
        assert_eq!(
            health_url(ServiceKind::Rerank, &ipv6),
            "http://[::1]:8012/health"
        );
    }

    #[test]
    fn health_url_uses_service_specific_port() {
        let config = AppConfig {
            embedding: ServiceConfig::new_for(ServiceKind::Embedding, true, 8013, String::new()),
            ..AppConfig::default()
        };

        assert_eq!(
            health_url(ServiceKind::Embedding, &config),
            "http://127.0.0.1:8013/health"
        );
    }

    #[tokio::test]
    async fn proxy_protocol_launch_uses_internal_llama_port_and_public_proxy_port() {
        let public_listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("public port reservation should bind");
        let public_port = public_listener
            .local_addr()
            .expect("public listener address should be readable")
            .port();
        let config = AppConfig {
            rerank: ServiceConfig::new(true, public_port, String::new())
                .with_protocol("cohere_rerank_v2"),
            ..AppConfig::default()
        };

        let launch = build_service_launch(
            ServiceKind::Rerank,
            &config,
            Path::new("C:\\models\\rank.gguf"),
        )
        .expect("proxy launch should build");

        let proxy = launch
            .proxy_config
            .as_ref()
            .expect("cohere rerank should use protocol proxy");
        assert_eq!(proxy.public_port, public_port);
        assert_eq!(proxy.protocol, "cohere_rerank_v2");
        assert!(proxy.llama_base_url.starts_with("http://127.0.0.1:"));
        assert_ne!(
            proxy.llama_base_url,
            format!("http://127.0.0.1:{public_port}")
        );
        assert!(launch
            .args
            .windows(2)
            .any(|window| window == ["--host", "127.0.0.1"]));
        assert!(!launch
            .args
            .windows(2)
            .any(|window| window == ["--port", &public_port.to_string()]));
        assert_eq!(
            launch.health_url,
            format!("{}/health", proxy.llama_base_url)
        );
    }

    #[tokio::test]
    async fn native_protocol_launch_uses_public_port_without_proxy() {
        let config = AppConfig {
            embedding: ServiceConfig::new_for(ServiceKind::Embedding, true, 8013, String::new()),
            ..AppConfig::default()
        };

        let launch = build_service_launch(
            ServiceKind::Embedding,
            &config,
            Path::new("C:\\models\\embed.gguf"),
        )
        .expect("native launch should build");

        assert!(launch.proxy_config.is_none());
        assert!(launch
            .args
            .windows(2)
            .any(|window| window == ["--host", "127.0.0.1"]));
        assert!(launch
            .args
            .windows(2)
            .any(|window| window == ["--port", "8013"]));
        assert_eq!(launch.health_url, "http://127.0.0.1:8013/health");
    }

    #[test]
    fn embedding_fastpath_launch_uses_sidecar_assets() {
        let config = AppConfig {
            embedding: ServiceConfig::new_for(ServiceKind::Embedding, true, 8013, String::new()),
            ..AppConfig::default()
        };
        let runtime = RuntimeLayout {
            runtime_dir: PathBuf::from("C:\\llama"),
            server_exe: PathBuf::from("C:\\llama\\llama-server.exe"),
            marker_path: PathBuf::from("C:\\llama\\runtime.json"),
        };
        let fastpath = FastPathLayout {
            server_exe: PathBuf::from("C:\\resources\\vekai-embedding-fastpath.exe"),
            runtime_dir: PathBuf::from("C:\\fastpath\\runtime"),
            model_path: PathBuf::from("C:\\fastpath\\model\\embedding.onnx"),
            tokenizer_path: PathBuf::from("C:\\fastpath\\model\\tokenizer.json"),
            ort_dll: PathBuf::from("C:\\fastpath\\runtime\\onnxruntime.dll"),
        };

        let launch = build_service_launch_with_fastpath(
            ServiceKind::Embedding,
            &config,
            Path::new("C:\\models\\Qwen3-Embedding-0.6B-Q8_0.gguf"),
            &runtime,
            Some(&fastpath),
        )
        .expect("fast path launch should build");

        assert_eq!(launch.backend, ServiceBackend::EmbeddingFastPath);
        assert_eq!(launch.program, fastpath.server_exe);
        assert_eq!(launch.path_prefix.as_deref(), Some(fastpath.runtime_dir.as_path()));
        assert!(launch
            .args
            .windows(2)
            .any(|window| window == ["--ort-dll", "C:\\fastpath\\runtime\\onnxruntime.dll"]));
    }

    #[tokio::test]
    async fn launch_port_check_rejects_occupied_public_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("public port reservation should bind");
        let port = listener
            .local_addr()
            .expect("public listener address should be readable")
            .port();
        let config = AppConfig {
            rerank: ServiceConfig::new(true, port, String::new()).with_protocol("llama_rerank"),
            ..AppConfig::default()
        };
        let launch = build_service_launch(
            ServiceKind::Rerank,
            &config,
            Path::new("C:\\models\\rank.gguf"),
        )
        .expect("launch should build before the actual start attempt");

        let error = ensure_launch_ports_available(ServiceKind::Rerank, &launch)
            .expect_err("occupied public port should be rejected");

        assert!(error.to_string().contains(&format!("127.0.0.1:{port}")));
    }

    #[test]
    fn child_stop_result_retains_child_on_inspect_or_kill_error() {
        assert!(ChildStopResult::AlreadyExited.should_clear_child());
        assert!(ChildStopResult::Killed.should_clear_child());
        assert!(!ChildStopResult::InspectFailed(anyhow!("inspect failed")).should_clear_child());
        assert!(!ChildStopResult::KillFailed(anyhow!("kill failed")).should_clear_child());
    }

    #[test]
    fn crash_exit_policy_restarts_first_crash_when_enabled() {
        let now = Instant::now();

        assert_eq!(
            crash_exit_action(true, None, now, false),
            CrashExitAction::Restart
        );
    }

    #[tokio::test]
    async fn fastpath_exit_falls_back_even_when_crash_restart_is_disabled() {
        let manager = ProcessManager::for_service(ServiceKind::Embedding);
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let config = AppConfig {
            restart_on_crash: false,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(config.clone())));
        state
            .lock()
            .await
            .set_service_status(ServiceKind::Embedding, ServiceStatus::Running);
        manager.restart_state.lock().await.context = Some(RestartContext {
            config,
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().join("runtime"),
                server_exe: temp_dir.path().join("runtime").join("llama-server.exe"),
                marker_path: temp_dir.path().join("runtime").join("runtime.json"),
            },
            model_path: temp_dir.path().join("model.gguf"),
            state: state.clone(),
            logs,
            fastpath: None,
        });

        let decision = fastpath_fallback_decision(
            &manager.supervisor(),
            &state,
            "fast path crashed",
        )
        .await;

        assert!(matches!(decision, RestartDecision::Restart(_)));
    }

    #[test]
    fn crash_exit_policy_restarts_when_previous_crash_is_outside_window() {
        let now = Instant::now();

        assert_eq!(
            crash_exit_action(
                true,
                Some(now - CRASH_RESTART_WINDOW - Duration::from_secs(1)),
                now,
                false,
            ),
            CrashExitAction::Restart
        );
    }

    #[test]
    fn crash_exit_policy_fails_second_crash_inside_window() {
        let now = Instant::now();

        assert_eq!(
            crash_exit_action(true, Some(now - Duration::from_secs(59)), now, false,),
            CrashExitAction::Fail
        );
    }

    #[test]
    fn crash_exit_policy_fails_when_restart_disabled() {
        let now = Instant::now();

        assert_eq!(
            crash_exit_action(false, None, now, false),
            CrashExitAction::Fail
        );
    }

    #[test]
    fn crash_exit_policy_ignores_shutdown() {
        let now = Instant::now();

        assert_eq!(
            crash_exit_action(true, None, now, true),
            CrashExitAction::Ignore
        );
    }

    #[tokio::test]
    async fn unexpected_exit_does_not_override_stopping_state() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));
        {
            let mut state = state.lock().await;
            state.status = ServiceStatus::Stopping;
        }

        mark_unexpected_exit_failed(
            ServiceKind::Rerank,
            &state,
            &logs,
            "llama-server.exe exited unexpectedly",
        )
        .await;

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Stopping);
        assert!(state.error.is_none());
        assert!(state.recent_logs.is_empty());
        assert!(!logs.app_log.exists());
    }

    #[tokio::test]
    async fn unexpected_exit_with_restart_enabled_attempts_saved_context_restart() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("isolated startup test port should bind");
        let port = listener
            .local_addr()
            .expect("isolated startup test address should be available")
            .port();
        drop(listener);
        let config = AppConfig {
            rerank: ServiceConfig::new(true, port, String::new()),
            restart_on_crash: true,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(config.clone())));
        {
            let mut state = state.lock().await;
            state.status = ServiceStatus::Running;
        }
        let context = RestartContext {
            config,
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().join("runtime"),
                server_exe: temp_dir
                    .path()
                    .join("runtime")
                    .join("missing-llama-server.exe"),
                marker_path: temp_dir.path().join("runtime").join("runtime.json"),
            },
            model_path: temp_dir.path().join("models").join("model.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };
        {
            let mut restart_state = manager.restart_state.lock().await;
            restart_state.context = Some(context);
        }

        handle_unexpected_exit(
            manager.supervisor(),
            state.clone(),
            logs.clone(),
            ServiceBackend::Llama,
            "llama-server.exe exited unexpectedly: test crash".to_string(),
        )
        .await;

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Failed);
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Restarting llama-server.exe after unexpected exit")));
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Failed to start llama-server.exe")));
        drop(state);

        assert!(manager.child.lock().await.is_none());
        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Restarting llama-server.exe after unexpected exit"));
        assert!(app_log.contains("Failed to start llama-server.exe"));
    }

    #[tokio::test]
    async fn unexpected_exit_uses_current_restart_policy_when_cached_context_is_enabled() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let current_config = AppConfig {
            restart_on_crash: false,
            ..AppConfig::default()
        };
        let cached_config = AppConfig {
            restart_on_crash: true,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(current_config)));
        {
            let mut state = state.lock().await;
            state.status = ServiceStatus::Running;
        }
        let context = RestartContext {
            config: cached_config,
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().join("runtime"),
                server_exe: temp_dir
                    .path()
                    .join("runtime")
                    .join("missing-llama-server.exe"),
                marker_path: temp_dir.path().join("runtime").join("runtime.json"),
            },
            model_path: temp_dir.path().join("models").join("model.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };
        {
            let mut restart_state = manager.restart_state.lock().await;
            restart_state.context = Some(context);
        }

        handle_unexpected_exit(
            manager.supervisor(),
            state.clone(),
            logs.clone(),
            ServiceBackend::Llama,
            "llama-server.exe exited unexpectedly: test crash".to_string(),
        )
        .await;

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Failed);
        assert!(state
            .error
            .as_deref()
            .is_some_and(|error| error.contains("test crash")));
        assert!(!state
            .recent_logs
            .iter()
            .any(|line| line.contains("Restarting llama-server.exe after unexpected exit")));
        drop(state);

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("test crash"));
        assert!(!app_log.contains("Restarting llama-server.exe after unexpected exit"));
    }

    #[tokio::test]
    async fn unexpected_exit_uses_current_restart_policy_when_cached_context_is_disabled() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let current_config = AppConfig {
            restart_on_crash: true,
            ..AppConfig::default()
        };
        let cached_config = AppConfig {
            restart_on_crash: false,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(current_config)));
        {
            let mut state = state.lock().await;
            state.status = ServiceStatus::Running;
        }
        let context = RestartContext {
            config: cached_config,
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().join("runtime"),
                server_exe: temp_dir
                    .path()
                    .join("runtime")
                    .join("missing-llama-server.exe"),
                marker_path: temp_dir.path().join("runtime").join("runtime.json"),
            },
            model_path: temp_dir.path().join("models").join("model.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };
        {
            let mut restart_state = manager.restart_state.lock().await;
            restart_state.context = Some(context);
        }

        handle_unexpected_exit(
            manager.supervisor(),
            state.clone(),
            logs.clone(),
            ServiceBackend::Llama,
            "llama-server.exe exited unexpectedly: test crash".to_string(),
        )
        .await;

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Failed);
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Restarting llama-server.exe after unexpected exit")));
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Failed to start llama-server.exe")));
        drop(state);

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Restarting llama-server.exe after unexpected exit"));
        assert!(app_log.contains("Failed to start llama-server.exe"));
    }

    #[tokio::test]
    async fn startup_child_exit_uses_restart_policy_without_waiting_for_health_timeout() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("isolated startup child exit test port should bind");
        let port = listener
            .local_addr()
            .expect("isolated startup child exit test address should be available")
            .port();
        drop(listener);
        let config = AppConfig {
            rerank: ServiceConfig::new(true, port, String::new()),
            restart_on_crash: true,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(config.clone())));
        let context = RestartContext {
            config,
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().to_path_buf(),
                server_exe: short_lived_server_exe(temp_dir.path()),
                marker_path: temp_dir.path().join("runtime.json"),
            },
            model_path: temp_dir.path().join("models").join("model.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };

        let result = tokio::time::timeout(
            Duration::from_secs(3),
            start_with_context(manager.supervisor(), context, StartReason::Manual),
        )
        .await
        .expect("startup child exit should be handled before health timeout");

        assert!(result.is_err());
        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Failed);
        assert!(state
            .error
            .as_deref()
            .is_some_and(|error| error.contains("manual restart required")));
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Restarting llama-server.exe after unexpected exit")));
        drop(state);

        assert!(manager.child.lock().await.is_none());
        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Restarting llama-server.exe after unexpected exit"));
        assert!(app_log.contains("manual restart required"));
    }

    #[tokio::test]
    async fn stop_during_startup_health_wait_wins_without_restart() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("health probe port reservation should bind");
        let port = listener
            .local_addr()
            .expect("health probe listener address should be available")
            .port();
        drop(listener);
        let config = AppConfig {
            rerank: ServiceConfig::new(true, port, String::new()),
            restart_on_crash: true,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(config.clone())));
        let context = RestartContext {
            config,
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().to_path_buf(),
                server_exe: never_healthy_server_exe(temp_dir.path()),
                marker_path: temp_dir.path().join("runtime.json"),
            },
            model_path: temp_dir.path().join("models").join("model.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };

        let start_task = tokio::spawn(start_with_context(
            manager.supervisor(),
            context,
            StartReason::Manual,
        ));
        wait_for_state(&state, ServiceStatus::Starting).await;

        let stop_result = tokio::time::timeout(
            Duration::from_secs(3),
            manager.stop(state.clone(), logs.clone()),
        )
        .await;
        if stop_result.is_err() {
            manager.request_shutdown();
            let _ = kill_child(&manager.supervisor()).await;
            start_task.abort();
            panic!("stop should not wait for startup health timeout");
        }
        stop_result
            .expect("stop should finish before timeout")
            .expect("stop should succeed");

        let start_result = tokio::time::timeout(Duration::from_secs(3), start_task)
            .await
            .expect("start task should finish after stop")
            .expect("start task should not panic");
        assert!(start_result.is_ok());

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Stopped);
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Stopping llama-server.exe")));
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("llama-server.exe stopped")));
        assert!(!state
            .recent_logs
            .iter()
            .any(|line| line.contains("Restarting llama-server.exe after unexpected exit")));
        drop(state);

        assert!(manager.child.lock().await.is_none());
        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Stopping llama-server.exe"));
        assert!(app_log.contains("llama-server.exe stopped"));
        assert!(!app_log.contains("Restarting llama-server.exe after unexpected exit"));
    }

    #[tokio::test]
    async fn superseded_startup_health_wait_does_not_claim_new_child() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("health responder port reservation should bind");
        let port = listener
            .local_addr()
            .expect("health responder listener address should be available")
            .port();
        drop(listener);
        let config = AppConfig {
            rerank: ServiceConfig::new(true, port, String::new()),
            restart_on_crash: true,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(config.clone())));
        let server_exe = never_healthy_server_exe(temp_dir.path());

        let first_context = RestartContext {
            config: config.clone(),
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().to_path_buf(),
                server_exe: server_exe.clone(),
                marker_path: temp_dir.path().join("runtime-a.json"),
            },
            model_path: temp_dir.path().join("models").join("model-a.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };
        let first_start = tokio::spawn(start_with_context(
            manager.supervisor(),
            first_context,
            StartReason::Manual,
        ));
        wait_for_state(&state, ServiceStatus::Starting).await;

        manager
            .stop(state.clone(), logs.clone())
            .await
            .expect("first startup should stop");

        let second_context = RestartContext {
            config: config.clone(),
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().to_path_buf(),
                server_exe,
                marker_path: temp_dir.path().join("runtime-b.json"),
            },
            model_path: temp_dir.path().join("models").join("model-b.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };
        let second_start = tokio::spawn(start_with_context(
            manager.supervisor(),
            second_context,
            StartReason::Manual,
        ));
        wait_for_state(&state, ServiceStatus::Starting).await;
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .expect("health responder listener should bind after startup begins");
        let health_responder = spawn_health_responder(listener);

        let second_result = tokio::time::timeout(Duration::from_secs(3), second_start)
            .await
            .expect("second start should finish")
            .expect("second start should not panic");
        assert!(second_result.is_ok());

        let first_result = tokio::time::timeout(Duration::from_secs(3), first_start)
            .await
            .expect("first stale start should finish")
            .expect("first stale start should not panic");
        assert!(first_result.is_ok());

        let state_guard = state.lock().await;
        assert_eq!(state_guard.status, ServiceStatus::Running);
        let running_log_count = state_guard
            .recent_logs
            .iter()
            .filter(|line| line.as_str() == "llama-server.exe is running")
            .count();
        assert_eq!(running_log_count, 1);
        drop(state_guard);

        manager
            .stop(state.clone(), logs.clone())
            .await
            .expect("second server should stop");
        health_responder.abort();

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert_eq!(app_log.matches("llama-server.exe is running").count(), 1);
    }

    #[tokio::test]
    async fn unexpected_exit_second_crash_inside_window_marks_failed_without_restart_attempt() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let config = AppConfig {
            restart_on_crash: true,
            ..AppConfig::default()
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(config.clone())));
        {
            let mut state = state.lock().await;
            state.status = ServiceStatus::Running;
        }
        let context = RestartContext {
            config,
            runtime: RuntimeLayout {
                runtime_dir: temp_dir.path().join("runtime"),
                server_exe: temp_dir
                    .path()
                    .join("runtime")
                    .join("missing-llama-server.exe"),
                marker_path: temp_dir.path().join("runtime").join("runtime.json"),
            },
            model_path: temp_dir.path().join("models").join("model.gguf"),
            state: state.clone(),
            logs: logs.clone(),
            fastpath: None,
        };
        {
            let mut restart_state = manager.restart_state.lock().await;
            restart_state.context = Some(context);
            restart_state.last_crash_at = Some(Instant::now() - Duration::from_secs(1));
        }

        handle_unexpected_exit(
            manager.supervisor(),
            state.clone(),
            logs.clone(),
            ServiceBackend::Llama,
            "llama-server.exe exited unexpectedly: test crash".to_string(),
        )
        .await;

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Failed);
        assert!(state.error.as_deref().is_some_and(|error| {
            error.contains("exited unexpectedly after a recent automatic restart")
        }));
        assert!(!state
            .recent_logs
            .iter()
            .any(|line| line.contains("Restarting llama-server.exe after unexpected exit")));
        drop(state);

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("exited unexpectedly after a recent automatic restart"));
        assert!(!app_log.contains("Restarting llama-server.exe after unexpected exit"));
    }

    #[tokio::test]
    async fn exit_monitor_marks_running_child_exit_failed() {
        let manager = ProcessManager::new();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));
        {
            let mut state = state.lock().await;
            state.status = ServiceStatus::Running;
        }

        let child = short_lived_child();
        let child_generation = 42;
        *manager.child.lock().await = Some(ManagedChild {
            child,
            generation: child_generation,
            backend: ServiceBackend::Llama,
            proxy: None,
        });

        manager.spawn_exit_monitor(child_generation, state.clone(), logs.clone());
        wait_for_state(&state, ServiceStatus::Failed).await;

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Failed);
        assert!(state
            .error
            .as_deref()
            .is_some_and(|error| error.contains("llama-server.exe exited unexpectedly")));
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("llama-server.exe exited unexpectedly")));
        drop(state);

        assert!(manager.child.lock().await.is_none());
        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("llama-server.exe exited unexpectedly"));
    }

    async fn wait_for_state(state: &SharedAppState, expected: ServiceStatus) {
        let deadline = Instant::now() + Duration::from_secs(3);

        while Instant::now() < deadline {
            if state.lock().await.status == expected {
                return;
            }

            sleep(Duration::from_millis(25)).await;
        }

        panic!("state did not become {expected:?}");
    }

    #[cfg(target_os = "windows")]
    fn short_lived_server_exe(directory: &Path) -> PathBuf {
        let path = directory.join("short-lived-server.cmd");
        std::fs::write(&path, "@echo off\r\nexit /b 7\r\n")
            .expect("short-lived server command should be written");
        path
    }

    #[cfg(not(target_os = "windows"))]
    fn short_lived_server_exe(directory: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.join("short-lived-server.sh");
        std::fs::write(&path, "#!/bin/sh\nexit 7\n")
            .expect("short-lived server script should be written");
        let mut permissions = std::fs::metadata(&path)
            .expect("short-lived server script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions)
            .expect("short-lived server script should be executable");
        path
    }

    #[cfg(target_os = "windows")]
    fn never_healthy_server_exe(directory: &Path) -> PathBuf {
        let path = directory.join("never-healthy-server.cmd");
        std::fs::write(
            &path,
            "@echo off\r\n:loop\r\nping -n 2 127.0.0.1 >nul\r\ngoto loop\r\n",
        )
        .expect("never-healthy server command should be written");
        path
    }

    #[cfg(not(target_os = "windows"))]
    fn never_healthy_server_exe(directory: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.join("never-healthy-server.sh");
        std::fs::write(&path, "#!/bin/sh\nwhile true; do sleep 1; done\n")
            .expect("never-healthy server script should be written");
        let mut permissions = std::fs::metadata(&path)
            .expect("never-healthy server script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions)
            .expect("never-healthy server script should be executable");
        path
    }

    fn spawn_health_responder(listener: TcpListener) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _addr)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 1024];
                    let _ = socket.read(&mut buffer).await;
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}",
                        )
                        .await;
                    let _ = socket.shutdown().await;
                });
            }
        })
    }

    #[cfg(target_os = "windows")]
    fn short_lived_child() -> Child {
        Command::new("cmd")
            .args(["/C", "exit", "7"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("short-lived child should spawn")
    }

    #[cfg(not(target_os = "windows"))]
    fn short_lived_child() -> Child {
        Command::new("sh")
            .args(["-c", "exit 7"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("short-lived child should spawn")
    }
}
