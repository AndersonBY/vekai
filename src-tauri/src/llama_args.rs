use std::path::Path;

use crate::config::{AppConfig, ConfigError};
use crate::service::ServiceKind;

pub fn build_llama_server_args(
    config: &AppConfig,
    model_path: &Path,
) -> Result<Vec<String>, ConfigError> {
    build_service_llama_server_args(ServiceKind::Rerank, config, model_path)
}

pub fn build_service_llama_server_args(
    service: ServiceKind,
    config: &AppConfig,
    model_path: &Path,
) -> Result<Vec<String>, ConfigError> {
    build_service_llama_server_args_for_endpoint(
        service,
        config,
        model_path,
        &config.host,
        config.service_port(service),
    )
}

pub fn build_service_llama_server_args_for_endpoint(
    service: ServiceKind,
    config: &AppConfig,
    model_path: &Path,
    host: &str,
    port: u16,
) -> Result<Vec<String>, ConfigError> {
    config.validate()?;

    let mut args = vec![
        "-m".to_string(),
        model_path.to_string_lossy().into_owned(),
        "--embedding".to_string(),
    ];

    if service == ServiceKind::Rerank {
        args.extend([
            "--pooling".to_string(),
            "rank".to_string(),
            "--rerank".to_string(),
        ]);
    }

    args.extend([
        "--host".to_string(),
        host.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--no-webui".to_string(),
    ]);

    if !config.gpu_layers.is_empty() {
        args.push("--n-gpu-layers".to_string());
        args.push(config.gpu_layers.clone());
    }

    if config.threads > 0 {
        args.push("--threads".to_string());
        args.push(config.threads.to_string());
    }

    if config.parallel_slots > 0 {
        args.push("--parallel".to_string());
        args.push(config.parallel_slots.to_string());
    }

    if config.http_threads > 0 {
        args.push("--threads-http".to_string());
        args.push(config.http_threads.to_string());
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::config::{AppConfig, ConfigError, ServiceConfig};
    use crate::service::ServiceKind;

    use super::{build_llama_server_args, build_service_llama_server_args};

    #[test]
    fn builds_required_rerank_arguments() {
        let config = AppConfig {
            host: "0.0.0.0".to_string(),
            rerank: ServiceConfig::new(true, 9001, String::new()),
            ..AppConfig::default()
        };

        let args = build_llama_server_args(&config, Path::new("C:\\models\\rerank.gguf"))
            .expect("valid config should build llama-server args");

        assert_eq!(
            args,
            vec![
                "-m",
                "C:\\models\\rerank.gguf",
                "--embedding",
                "--pooling",
                "rank",
                "--rerank",
                "--host",
                "0.0.0.0",
                "--port",
                "9001",
                "--no-webui",
                "--n-gpu-layers",
                "auto",
            ]
        );
    }

    #[test]
    fn omits_threads_when_zero_and_includes_gpu_layers() {
        let config = AppConfig {
            gpu_layers: "auto".to_string(),
            threads: 0,
            parallel_slots: 0,
            http_threads: 0,
            ..AppConfig::default()
        };

        let args = build_llama_server_args(&config, Path::new("model.gguf"))
            .expect("valid config should build llama-server args");

        assert!(args
            .windows(2)
            .any(|window| window == ["--n-gpu-layers", "auto"]));
        assert!(!args.iter().any(|arg| arg == "--threads"));
        assert!(!args.iter().any(|arg| arg == "--parallel"));
        assert!(!args.iter().any(|arg| arg == "--threads-http"));
    }

    #[test]
    fn includes_threads_when_set() {
        let config = AppConfig {
            gpu_layers: "12".to_string(),
            threads: 8,
            ..AppConfig::default()
        };

        let args = build_llama_server_args(&config, Path::new("model.gguf"))
            .expect("valid config should build llama-server args");

        assert!(args.windows(2).any(|window| window == ["--threads", "8"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--n-gpu-layers", "12"]));
    }

    #[test]
    fn includes_parallel_and_http_threads_when_set() {
        let config = AppConfig {
            parallel_slots: 4,
            http_threads: 6,
            ..AppConfig::default()
        };

        let args = build_llama_server_args(&config, Path::new("model.gguf"))
            .expect("valid config should build llama-server args");

        assert!(args.windows(2).any(|window| window == ["--parallel", "4"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--threads-http", "6"]));
    }

    #[test]
    fn invalid_config_propagates_config_error() {
        let config = AppConfig {
            rerank: ServiceConfig::new(true, 0, String::new()),
            ..AppConfig::default()
        };

        let error = build_llama_server_args(&config, Path::new("model.gguf"))
            .expect_err("invalid config should propagate ConfigError");

        assert!(matches!(error, ConfigError::InvalidPort));
    }

    #[test]
    fn builds_embedding_arguments_without_rerank_flags() {
        let config = AppConfig {
            host: "127.0.0.1".to_string(),
            embedding: ServiceConfig::new_for(ServiceKind::Embedding, true, 9002, String::new()),
            ..AppConfig::default()
        };

        let args = build_service_llama_server_args(
            ServiceKind::Embedding,
            &config,
            Path::new("C:\\models\\embedding.gguf"),
        )
        .expect("valid config should build embedding args");

        assert_eq!(
            args,
            vec![
                "-m",
                "C:\\models\\embedding.gguf",
                "--embedding",
                "--host",
                "127.0.0.1",
                "--port",
                "9002",
                "--no-webui",
                "--n-gpu-layers",
                "auto",
            ]
        );
        assert!(!args.iter().any(|arg| arg == "--rerank"));
        assert!(!args
            .windows(2)
            .any(|window| window == ["--pooling", "rank"]));
    }
}
