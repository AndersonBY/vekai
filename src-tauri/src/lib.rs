use crate::commands::{build_app_paths, load_initial_config, AppPaths, ServiceManagers};
use crate::logging::{append_log, cleanup_old_logs, ensure_log_dir, LogPaths};
use crate::state::{AppStateInner, ServiceStatus, SharedAppState};
use std::path::Path;
use std::sync::Arc;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent, Wry};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

const SETTINGS_WINDOW_LABEL: &str = "settings";
const TRAY_OPEN_SETTINGS_ID: &str = "open_settings";
const TRAY_START_SERVER_ID: &str = "start_server";
const TRAY_STOP_SERVER_ID: &str = "stop_server";
const TRAY_RESTART_SERVER_ID: &str = "restart_server";
const TRAY_OPEN_LOGS_ID: &str = "open_logs";
const TRAY_QUIT_ID: &str = "quit";
const PRODUCT_NAME: &str = "Vekai";
const CLI_BINARY_NAME: &str = "vekai";
#[cfg(target_os = "linux")]
const SYSTEMD_SERVICE_NAME: &str = "vekai.service";
const CLOSE_CONFIRM_SCRIPT: &str = r#"
(() => {
  window.dispatchEvent(new CustomEvent('vekai-close-requested'));
})();
"#;

pub mod cli;
pub mod commands;
pub mod config;
pub mod llama_args;
pub mod logging;
pub mod models;
pub mod process;
pub mod protocol_adapter;
pub mod protocol_proxy;
pub mod runtime;
pub mod service;
pub mod state;

pub async fn run_service_foreground() -> anyhow::Result<()> {
    let paths = build_app_paths()?;
    ensure_log_dir(&paths.logs)?;
    let initial = load_initial_config(&paths)?;
    if let Some(warning) = initial.warning.as_deref() {
        append_log(&paths.logs.app_log, warning)?;
    }
    cleanup_old_logs(&paths.logs.log_dir, initial.config.log_retention_days)?;

    let state: SharedAppState = Arc::new(Mutex::new(AppStateInner::new(initial.config)));
    let managers = Arc::new(ServiceManagers::new());
    commands::start_servers_inner(&state, &managers, &paths).await?;
    append_log(&paths.logs.app_log, "Vekai CLI service started")?;

    match wait_for_shutdown_signal().await {
        Ok(()) => {
            append_log(
                &paths.logs.app_log,
                "Vekai CLI service stopping",
            )?;
        }
        Err(error) => {
            append_log(
                &paths.logs.app_log,
                &format!("Failed to listen for shutdown signal: {error}"),
            )?;
        }
    }

    for manager in managers.all() {
        manager.request_shutdown();
        manager.stop(state.clone(), paths.logs.clone()).await?;
    }
    append_log(&paths.logs.app_log, "Vekai CLI service stopped")?;
    Ok(())
}

async fn wait_for_shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}

pub fn print_cli_help() {
    println!(
        "{PRODUCT_NAME}\n\nUsage:\n  {CLI_BINARY_NAME}                 Start GUI tray app\n  {CLI_BINARY_NAME} serve           Run enabled services in foreground\n  {CLI_BINARY_NAME} install-service Install user systemd service on Linux\n  {CLI_BINARY_NAME} uninstall-service Remove user systemd service on Linux"
    );
}

#[cfg(target_os = "linux")]
pub fn install_user_service() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::fs;

    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let service_dir = dirs::home_dir()
        .context("failed to resolve home directory")?
        .join(".config")
        .join("systemd")
        .join("user");
    fs::create_dir_all(&service_dir)
        .with_context(|| format!("failed to create {}", service_dir.display()))?;
    let service_path = service_dir.join(SYSTEMD_SERVICE_NAME);
    fs::write(&service_path, cli::linux_systemd_unit(&exe))
        .with_context(|| format!("failed to write {}", service_path.display()))?;

    run_systemctl_user(["daemon-reload"])?;
    run_systemctl_user(["enable", "--now", SYSTEMD_SERVICE_NAME])?;
    println!(
        "Installed and started user service: {}",
        service_path.display()
    );
    println!("For boot without login, run: loginctl enable-linger $USER");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn install_user_service() -> anyhow::Result<()> {
    anyhow::bail!("systemd service installation is only supported on Linux")
}

#[cfg(target_os = "linux")]
pub fn uninstall_user_service() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::fs;

    let service_path = dirs::home_dir()
        .context("failed to resolve home directory")?
        .join(".config")
        .join("systemd")
        .join("user")
        .join(SYSTEMD_SERVICE_NAME);
    let _ = run_systemctl_user(["disable", "--now", SYSTEMD_SERVICE_NAME]);
    if service_path.exists() {
        fs::remove_file(&service_path)
            .with_context(|| format!("failed to remove {}", service_path.display()))?;
    }
    run_systemctl_user(["daemon-reload"])?;
    println!("Removed user service: {}", service_path.display());
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn uninstall_user_service() -> anyhow::Result<()> {
    anyhow::bail!("systemd service removal is only supported on Linux")
}

#[cfg(target_os = "linux")]
fn run_systemctl_user<const N: usize>(args: [&str; N]) -> anyhow::Result<()> {
    use anyhow::{anyhow, Context};
    use std::process::Command;

    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("failed to run systemctl --user")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("systemctl --user exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::logging::LogPaths;
    use crate::state::ServiceStatus;

    #[tokio::test]
    async fn record_action_failure_handles_tray_label() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));

        record_action_failure(
            &state,
            &logs.app_log,
            "Tray action failed",
            anyhow::anyhow!("missing model"),
        )
        .await;

        let state = state.lock().await;
        assert_eq!(
            state.error.as_deref(),
            Some("Tray action failed: missing model")
        );
        assert_eq!(
            state.recent_logs.back().map(String::as_str),
            Some("Tray action failed: missing model")
        );
        drop(state);

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Tray action failed: missing model"));
    }

    #[tokio::test]
    async fn record_action_failure_uses_action_label_in_state_and_app_log() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));

        record_action_failure(
            &state,
            &logs.app_log,
            "Auto-start failed",
            anyhow::anyhow!("runtime missing"),
        )
        .await;

        let state = state.lock().await;
        assert_eq!(
            state.error.as_deref(),
            Some("Auto-start failed: runtime missing")
        );
        assert_eq!(
            state.recent_logs.back().map(String::as_str),
            Some("Auto-start failed: runtime missing")
        );
        drop(state);

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Auto-start failed: runtime missing"));
    }

    #[tokio::test]
    async fn record_action_failure_does_not_replace_existing_failed_error() {
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
            state.status = ServiceStatus::Failed;
            state.error = Some(
                "Failed to start llama-server.exe: failed to start C:\\runtime\\llama-server.exe"
                    .to_string(),
            );
        }

        record_action_failure(
            &state,
            &logs.app_log,
            "Auto-start failed",
            anyhow::anyhow!("failed to start C:\\runtime\\llama-server.exe"),
        )
        .await;

        let state = state.lock().await;
        assert_eq!(
            state.error.as_deref(),
            Some("Failed to start llama-server.exe: failed to start C:\\runtime\\llama-server.exe")
        );
        assert_eq!(
            state.recent_logs.back().map(String::as_str),
            Some("Auto-start failed: failed to start C:\\runtime\\llama-server.exe")
        );
        drop(state);

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(
            app_log.contains("Auto-start failed: failed to start C:\\runtime\\llama-server.exe")
        );
    }

    #[tokio::test]
    async fn auto_start_enabled_reads_loaded_config_from_state() {
        let enabled_state = Arc::new(Mutex::new(AppStateInner::new(AppConfig {
            auto_start_server: true,
            ..AppConfig::default()
        })));
        let disabled_state = Arc::new(Mutex::new(AppStateInner::new(AppConfig {
            auto_start_server: false,
            ..AppConfig::default()
        })));

        assert!(auto_start_enabled(&enabled_state).await);
        assert!(!auto_start_enabled(&disabled_state).await);
    }

    #[tokio::test]
    async fn stop_server_for_quit_uses_service_managers_stop() {
        let managers = Arc::new(ServiceManagers::new());
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));

        stop_server_for_quit(state.clone(), managers, logs.clone())
            .await
            .expect("quit cleanup should stop the process manager");

        let state = state.lock().await;
        assert_eq!(state.status, ServiceStatus::Stopped);
        assert_eq!(state.embedding_status, ServiceStatus::Stopped);
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("Stopping llama-server.exe")));
        assert!(state
            .recent_logs
            .iter()
            .any(|line| line.contains("llama-server.exe stopped")));
        drop(state);

        let app_log = std::fs::read_to_string(&logs.app_log).expect("app log should exist");
        assert!(app_log.contains("Stopping llama-server.exe"));
        assert!(app_log.contains("llama-server.exe stopped"));
    }

    #[tokio::test]
    async fn cleanup_server_for_quit_returns_true_after_stop_succeeds() {
        let managers = Arc::new(ServiceManagers::new());
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().to_path_buf(),
            app_log: temp_dir.path().join("app.log"),
            server_log: temp_dir.path().join("server.log"),
            embedding_server_log: temp_dir.path().join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));

        let should_exit = cleanup_server_for_quit(state, managers, logs).await;

        assert!(should_exit);
    }

    #[tokio::test]
    async fn cleanup_server_for_quit_requests_shutdown_before_later_starts() {
        let managers = Arc::new(ServiceManagers::new());
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let logs = LogPaths {
            log_dir: temp_dir.path().join("logs"),
            app_log: temp_dir.path().join("logs").join("app.log"),
            server_log: temp_dir.path().join("logs").join("server.log"),
            embedding_server_log: temp_dir.path().join("logs").join("embedding-server.log"),
        };
        let state = Arc::new(Mutex::new(AppStateInner::new(AppConfig::default())));

        let should_exit =
            cleanup_server_for_quit(state.clone(), managers.clone(), logs.clone()).await;

        assert!(should_exit);
        let log_count_after_cleanup = state.lock().await.recent_logs.len();
        let error = managers
            .rerank
            .start(
                AppConfig::default(),
                crate::runtime::RuntimeLayout {
                    runtime_dir: temp_dir.path().join("runtime"),
                    server_exe: temp_dir.path().join("runtime").join("llama-server.exe"),
                    marker_path: temp_dir.path().join("runtime").join("runtime.json"),
                },
                temp_dir.path().join("models").join("model.gguf"),
                state.clone(),
                logs.clone(),
            )
            .await
            .expect_err("start after quit cleanup should be rejected");

        assert!(error.to_string().contains("shutdown requested"));
        {
            let state_guard = state.lock().await;
            assert_eq!(state_guard.status, ServiceStatus::Stopped);
            assert_eq!(state_guard.recent_logs.len(), log_count_after_cleanup);
        }
        let embedding_error = managers
            .embedding
            .start(
                AppConfig::default(),
                crate::runtime::RuntimeLayout {
                    runtime_dir: temp_dir.path().join("runtime-embedding"),
                    server_exe: temp_dir
                        .path()
                        .join("runtime-embedding")
                        .join("llama-server.exe"),
                    marker_path: temp_dir
                        .path()
                        .join("runtime-embedding")
                        .join("runtime.json"),
                },
                temp_dir.path().join("models").join("model.gguf"),
                state.clone(),
                logs.clone(),
            )
            .await
            .expect_err("embedding start after quit cleanup should be rejected");
        assert!(embedding_error.to_string().contains("shutdown requested"));
    }

    #[test]
    fn close_confirm_script_offers_quit_and_tray_choices() {
        assert!(!close_confirm_script().contains("window.confirm"));
        assert!(!close_confirm_script().contains("quit_app"));
        assert!(close_confirm_script().contains("vekai-close-requested"));
    }

    #[test]
    fn settings_close_intercept_only_targets_settings_window() {
        assert!(should_intercept_window_close(SETTINGS_WINDOW_LABEL));
        assert!(!should_intercept_window_close("logs"));
        assert!(!should_intercept_window_close(""));
    }

    #[cfg(unix)]
    #[test]
    fn unix_service_shutdown_listener_includes_sigterm() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
        )
        .expect("lib.rs should be readable");

        assert!(source.contains("SignalKind::terminate"));
        assert!(source.contains("tokio::select!"));
    }
}

fn show_settings_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn open_logs_dir(app: &AppHandle<Wry>) {
    let paths = app.state::<AppPaths>();
    let logs_dir = paths.logs.log_dir.to_string_lossy().into_owned();
    if let Err(error) = app.opener().open_path(logs_dir, None::<&str>) {
        let _ = append_log(
            &paths.logs.app_log,
            &format!("Failed to open logs directory: {error}"),
        );
    }
}

async fn record_action_failure(
    state: &SharedAppState,
    app_log: &Path,
    action_label: &str,
    error: anyhow::Error,
) {
    let message = format!("{action_label}: {error}");
    {
        let mut state = state.lock().await;
        if !matches!(state.status, ServiceStatus::Failed) || state.error.is_none() {
            state.error = Some(message.clone());
        }
        state.push_log(message.clone());
    }
    let _ = append_log(app_log, &message);
}

async fn auto_start_enabled(state: &SharedAppState) -> bool {
    state.lock().await.config.auto_start_server
}

fn run_server_action(
    state: SharedAppState,
    managers: Arc<ServiceManagers>,
    paths: AppPaths,
    failure_label: &'static str,
    action: impl FnOnce(
            SharedAppState,
            Arc<ServiceManagers>,
            AppPaths,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>
        + Send
        + 'static,
) {
    let log_path = paths.logs.app_log.clone();
    let state_for_error = state.clone();

    tauri::async_runtime::spawn(async move {
        if let Err(error) = action(state, managers, paths).await {
            record_action_failure(&state_for_error, &log_path, failure_label, error).await;
        }
    });
}

fn run_managed_server_action(
    app: &AppHandle<Wry>,
    failure_label: &'static str,
    action: impl FnOnce(
            SharedAppState,
            Arc<ServiceManagers>,
            AppPaths,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>
        + Send
        + 'static,
) {
    run_server_action(
        app.state::<SharedAppState>().inner().clone(),
        app.state::<Arc<ServiceManagers>>().inner().clone(),
        app.state::<AppPaths>().inner().clone(),
        failure_label,
        action,
    );
}

fn start_server_from_tray(app: &AppHandle<Wry>) {
    run_managed_server_action(app, "Tray action failed", |state, managers, paths| {
        Box::pin(async move {
            commands::start_servers_inner(&state, &managers, &paths)
                .await
                .map(|_| ())
        })
    });
}

fn stop_server_from_tray(app: &AppHandle<Wry>) {
    run_managed_server_action(app, "Tray action failed", |state, managers, paths| {
        Box::pin(async move {
            commands::stop_servers_inner(&state, &managers, &paths)
                .await
                .map(|_| ())
        })
    });
}

fn restart_server_from_tray(app: &AppHandle<Wry>) {
    run_managed_server_action(app, "Tray action failed", |state, managers, paths| {
        Box::pin(async move {
            commands::restart_servers_inner(&state, &managers, &paths)
                .await
                .map(|_| ())
        })
    });
}

fn auto_start_server(app: &AppHandle<Wry>) {
    run_managed_server_action(app, "Auto-start failed", |state, managers, paths| {
        Box::pin(async move {
            commands::start_servers_inner(&state, &managers, &paths)
                .await
                .map(|_| ())
        })
    });
}

async fn stop_server_for_quit(
    state: SharedAppState,
    managers: Arc<ServiceManagers>,
    logs: LogPaths,
) -> anyhow::Result<()> {
    for manager in managers.all() {
        manager.stop(state.clone(), logs.clone()).await?;
    }

    Ok(())
}

async fn cleanup_server_for_quit(
    state: SharedAppState,
    managers: Arc<ServiceManagers>,
    logs: LogPaths,
) -> bool {
    for manager in managers.all() {
        manager.request_shutdown();
    }

    if let Err(error) = stop_server_for_quit(state.clone(), managers, logs.clone()).await {
        record_action_failure(&state, &logs.app_log, "Quit cleanup failed", error).await;
        return false;
    }

    true
}

fn quit_after_cleanup(app: &AppHandle<Wry>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let cleanup_succeeded = if let (Some(state), Some(managers), Some(paths)) = (
            app.try_state::<SharedAppState>(),
            app.try_state::<Arc<ServiceManagers>>(),
            app.try_state::<AppPaths>(),
        ) {
            let state = state.inner().clone();
            let managers = managers.inner().clone();
            let logs = paths.logs.clone();
            cleanup_server_for_quit(state, managers, logs).await
        } else {
            true
        };

        if cleanup_succeeded {
            app.exit(0);
        }
    });
}

fn close_confirm_script() -> &'static str {
    CLOSE_CONFIRM_SCRIPT
}

fn should_intercept_window_close(label: &str) -> bool {
    label == SETTINGS_WINDOW_LABEL
}

fn handle_window_event(window: &tauri::Window<Wry>, event: &WindowEvent) {
    if !should_intercept_window_close(window.label()) {
        return;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        if let Some(settings_window) = window
            .app_handle()
            .get_webview_window(SETTINGS_WINDOW_LABEL)
        {
            let _ = settings_window.eval(close_confirm_script());
        }
    }
}

#[tauri::command]
fn hide_settings_window(app: AppHandle<Wry>) -> Result<(), String> {
    app.get_webview_window(SETTINGS_WINDOW_LABEL)
        .ok_or_else(|| "settings window is not available".to_string())?
        .hide()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn quit_app(app: AppHandle<Wry>) {
    quit_after_cleanup(&app);
}

fn setup_tray(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(TRAY_OPEN_SETTINGS_ID, "打开设置")
        .separator()
        .text(TRAY_START_SERVER_ID, "启动服务")
        .text(TRAY_STOP_SERVER_ID, "停止服务")
        .text(TRAY_RESTART_SERVER_ID, "重启服务")
        .separator()
        .text(TRAY_OPEN_LOGS_ID, "打开日志")
        .separator()
        .text(TRAY_QUIT_ID, "退出")
        .build()?;
    let icon = app.default_window_icon().cloned();
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(PRODUCT_NAME)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_SETTINGS_ID => show_settings_window(app),
            TRAY_START_SERVER_ID => start_server_from_tray(app),
            TRAY_STOP_SERVER_ID => stop_server_from_tray(app),
            TRAY_RESTART_SERVER_ID => restart_server_from_tray(app),
            TRAY_OPEN_LOGS_ID => open_logs_dir(app),
            TRAY_QUIT_ID => quit_after_cleanup(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_settings_window(tray.app_handle());
            }
        });

    if let Some(icon) = icon {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .on_window_event(handle_window_event)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_settings_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = build_app_paths()?;
            ensure_log_dir(&paths.logs)?;
            let initial = load_initial_config(&paths)?;
            if let Some(warning) = initial.warning.as_deref() {
                let _ = append_log(&paths.logs.app_log, warning);
            }
            cleanup_old_logs(&paths.logs.log_dir, initial.config.log_retention_days)?;

            let mut state_inner = AppStateInner::new(initial.config);
            if let Some(warning) = initial.warning {
                state_inner.error = Some(warning.clone());
                state_inner.push_log(warning);
            }
            let state: SharedAppState = Arc::new(Mutex::new(state_inner));
            app.manage(paths);
            app.manage(state);
            app.manage(Arc::new(ServiceManagers::new()));
            setup_tray(app.handle())?;
            if tauri::async_runtime::block_on(auto_start_enabled(
                app.state::<SharedAppState>().inner(),
            )) {
                auto_start_server(app.handle());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::save_config,
            commands::refresh_models,
            commands::start_servers,
            commands::stop_servers,
            commands::restart_servers,
            commands::start_server,
            commands::stop_server,
            commands::restart_server,
            commands::test_rerank,
            commands::test_embedding,
            hide_settings_window,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Vekai");
}
