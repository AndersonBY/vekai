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
    let service_config = config.service_config(service);

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
        "--cache-ram".to_string(),
        "0".to_string(),
        "--log-verbosity".to_string(),
        "1".to_string(),
    ]);

    if !config.gpu_layers.is_empty() {
        args.push("--n-gpu-layers".to_string());
        args.push(config.gpu_layers.clone());
    }

    if config.gpu_layers == "all" {
        args.extend(["--fit".to_string(), "off".to_string()]);
    }

    if config.threads > 0 {
        args.push("--threads".to_string());
        args.push(config.threads.to_string());
    }

    let parallel_slots = if service_config.parallel_slots > 0 {
        service_config.parallel_slots
    } else {
        config.parallel_slots
    };
    if parallel_slots > 0 {
        args.push("--parallel".to_string());
        args.push(parallel_slots.to_string());
    }

    let batch_size = if service_config.batch_size > 0 {
        service_config.batch_size
    } else {
        config.batch_size
    };
    if batch_size > 0 {
        args.push("--batch-size".to_string());
        args.push(batch_size.to_string());
    }

    let ubatch_size = if service_config.ubatch_size > 0 {
        service_config.ubatch_size
    } else {
        config.ubatch_size
    };
    if ubatch_size > 0 {
        args.push("--ubatch-size".to_string());
        args.push(ubatch_size.to_string());
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
                "--cache-ram",
                "0",
                "--log-verbosity",
                "1",
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
            batch_size: 0,
            ubatch_size: 0,
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
        assert!(!args.iter().any(|arg| arg == "--batch-size"));
        assert!(!args.iter().any(|arg| arg == "--ubatch-size"));
        assert!(!args.iter().any(|arg| arg == "--threads-http"));
        assert!(args.windows(2).any(|window| window == ["--cache-ram", "0"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--log-verbosity", "1"]));
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
    fn includes_parallel_batch_and_http_threads_when_set() {
        let config = AppConfig {
            parallel_slots: 4,
            batch_size: 2048,
            ubatch_size: 1024,
            http_threads: 6,
            ..AppConfig::default()
        };

        let args = build_llama_server_args(&config, Path::new("model.gguf"))
            .expect("valid config should build llama-server args");

        assert!(args.windows(2).any(|window| window == ["--parallel", "4"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--threads-http", "6"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--batch-size", "2048"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--ubatch-size", "1024"]));
    }

    #[test]
    fn all_gpu_layers_disables_automatic_fit() {
        let config = AppConfig {
            gpu_layers: "all".to_string(),
            ..AppConfig::default()
        };

        let args = build_llama_server_args(&config, Path::new("model.gguf"))
            .expect("valid config should build llama-server args");

        assert!(args
            .windows(2)
            .any(|window| window == ["--n-gpu-layers", "all"]));
        assert!(args.windows(2).any(|window| window == ["--fit", "off"]));
    }

    #[test]
    fn service_batch_settings_override_shared_defaults() {
        let mut rerank = ServiceConfig::new(true, 9001, String::new());
        rerank.parallel_slots = 8;
        rerank.batch_size = 2048;
        rerank.ubatch_size = 2048;
        let config = AppConfig {
            rerank,
            parallel_slots: 4,
            batch_size: 512,
            ubatch_size: 512,
            ..AppConfig::default()
        };

        let args = build_llama_server_args(&config, Path::new("model.gguf"))
            .expect("valid config should build llama-server args");

        assert!(args.windows(2).any(|window| window == ["--parallel", "8"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--batch-size", "2048"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--ubatch-size", "2048"]));
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
                "--cache-ram",
                "0",
                "--log-verbosity",
                "1",
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
