use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::service::ServiceKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    pub enabled: bool,
    pub port: u16,
    pub model_path: String,
    pub protocol: String,
    pub parallel_slots: u16,
    pub batch_size: u16,
    pub ubatch_size: u16,
}

impl ServiceConfig {
    pub fn new(enabled: bool, port: u16, model_path: String) -> Self {
        Self::new_for(ServiceKind::Rerank, enabled, port, model_path)
    }

    pub fn new_for(service: ServiceKind, enabled: bool, port: u16, model_path: String) -> Self {
        Self {
            enabled,
            port,
            model_path,
            protocol: default_protocol(service).to_string(),
            parallel_slots: 0,
            batch_size: 0,
            ubatch_size: 0,
        }
    }

    pub fn with_protocol(mut self, protocol: impl Into<String>) -> Self {
        self.protocol = protocol.into();
        self
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    pub fn default_for(service: ServiceKind) -> Self {
        Self {
            enabled: true,
            port: service.default_port(),
            model_path: String::new(),
            protocol: default_protocol(service).to_string(),
            parallel_slots: 0,
            batch_size: 0,
            ubatch_size: 0,
        }
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self::default_for(ServiceKind::Rerank)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub host: String,
    pub rerank: ServiceConfig,
    pub embedding: ServiceConfig,
    pub auto_start_server: bool,
    pub restart_on_crash: bool,
    pub gpu_layers: String,
    pub threads: u16,
    pub parallel_slots: u16,
    pub batch_size: u16,
    pub ubatch_size: u16,
    pub http_threads: u16,
    pub log_retention_days: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            rerank: ServiceConfig::default_for(ServiceKind::Rerank),
            embedding: ServiceConfig::default_for(ServiceKind::Embedding),
            auto_start_server: true,
            restart_on_crash: true,
            gpu_layers: "auto".to_string(),
            threads: 0,
            parallel_slots: 0,
            batch_size: 0,
            ubatch_size: 0,
            http_threads: 0,
            log_retention_days: 7,
        }
    }
}

impl AppConfig {
    pub fn from_json_with_defaults(json: &str) -> Result<Self, ConfigError> {
        let mut value: Value = serde_json::from_str(json)?;
        let defaults = serde_json::to_value(Self::default())?;
        merge_missing_json_fields(&mut value, &defaults);

        let config: Self = serde_json::from_value(value)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.host.trim().is_empty() {
            return Err(ConfigError::EmptyHost);
        }

        if self.rerank.port == 0 {
            return Err(ConfigError::InvalidPort);
        }

        if self.embedding.port == 0 {
            return Err(ConfigError::InvalidEmbeddingPort);
        }

        if self.rerank.enabled && self.embedding.enabled && self.rerank.port == self.embedding.port
        {
            return Err(ConfigError::DuplicateServicePorts {
                port: self.rerank.port,
            });
        }

        if self.log_retention_days == 0 {
            return Err(ConfigError::InvalidLogRetentionDays);
        }

        if !is_valid_gpu_layers(&self.gpu_layers) {
            return Err(ConfigError::InvalidGpuLayers {
                value: self.gpu_layers.clone(),
            });
        }

        validate_protocol(ServiceKind::Rerank, self.rerank.protocol())?;
        validate_protocol(ServiceKind::Embedding, self.embedding.protocol())?;

        Ok(())
    }

    pub fn service_config(&self, service: ServiceKind) -> ServiceConfig {
        match service {
            ServiceKind::Rerank => self.rerank.clone(),
            ServiceKind::Embedding => self.embedding.clone(),
        }
    }

    pub fn service_enabled(&self, service: ServiceKind) -> bool {
        self.service_config(service).enabled
    }

    pub fn service_port(&self, service: ServiceKind) -> u16 {
        self.service_config(service).port
    }

    pub fn service_model_path(&self, service: ServiceKind) -> &str {
        match service {
            ServiceKind::Rerank => &self.rerank.model_path,
            ServiceKind::Embedding => &self.embedding.model_path,
        }
    }

    pub fn service_protocol(&self, service: ServiceKind) -> &str {
        match service {
            ServiceKind::Rerank => &self.rerank.protocol,
            ServiceKind::Embedding => &self.embedding.protocol,
        }
    }

    pub fn set_service_model_path(&mut self, service: ServiceKind, model_path: String) {
        match service {
            ServiceKind::Rerank => self.rerank.model_path = model_path,
            ServiceKind::Embedding => self.embedding.model_path = model_path,
        }
    }
}

pub fn default_protocol(service: ServiceKind) -> &'static str {
    match service {
        ServiceKind::Rerank => "llama_rerank",
        ServiceKind::Embedding => "openai_embeddings",
    }
}

pub fn supported_protocols(service: ServiceKind) -> &'static [&'static str] {
    match service {
        ServiceKind::Rerank => &[
            "llama_rerank",
            "siliconflow",
            "cohere_rerank_v2",
            "jina_rerank_v1",
            "voyage_rerank_v1",
            "custom_json_http",
        ],
        ServiceKind::Embedding => &[
            "openai_embeddings",
            "siliconflow",
            "cohere_embed_v2",
            "voyage_embeddings_v1",
            "custom_json_http",
        ],
    }
}

fn is_supported_protocol(service: ServiceKind, protocol: &str) -> bool {
    supported_protocols(service)
        .iter()
        .any(|candidate| candidate == &protocol)
}

fn validate_protocol(service: ServiceKind, protocol: &str) -> Result<(), ConfigError> {
    if is_supported_protocol(service, protocol) {
        return Ok(());
    }

    Err(ConfigError::InvalidProtocol {
        service: service.slug(),
        protocol: protocol.to_string(),
    })
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing EmbeddingGemma 2 multimodal projector: {path}. Download the matching mmproj GGUF alongside the model with scripts/download-assets.ps1")]
    MissingMultimodalProjector { path: String },
    #[error("EmbeddingGemma 2 physical batch size ({batch_size}) must be divisible by parallel slots ({parallel_slots}) to avoid a llama.cpp mean-pooling crash. Use 8 parallel slots with batch_size=2048 and ubatch_size=2048, or align both batch sizes with the slot count")]
    IncompatibleEmbeddingBatch { batch_size: u16, parallel_slots: u16 },
    #[error("failed to parse config JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("host cannot be empty")]
    EmptyHost,
    #[error("port must be between 1 and 65535")]
    InvalidPort,
    #[error("embedding_port must be between 1 and 65535")]
    InvalidEmbeddingPort,
    #[error("enabled rerank and embedding services cannot share port {port}")]
    DuplicateServicePorts { port: u16 },
    #[error("log_retention_days must be greater than 0")]
    InvalidLogRetentionDays,
    #[error(
        "gpu_layers must be \"auto\", \"all\", or a non-negative integer string, got {value:?}"
    )]
    InvalidGpuLayers { value: String },
    #[error("unsupported {service} protocol {protocol:?}")]
    InvalidProtocol {
        service: &'static str,
        protocol: String,
    },
}

fn is_valid_gpu_layers(value: &str) -> bool {
    matches!(value, "auto" | "all") || value.parse::<u32>().is_ok()
}

fn merge_missing_json_fields(value: &mut Value, defaults: &Value) {
    let (Some(value_object), Some(default_object)) = (value.as_object_mut(), defaults.as_object())
    else {
        return;
    };

    for (key, default_value) in default_object {
        match value_object.get_mut(key) {
            Some(current_value) => merge_missing_json_fields(current_value, default_value),
            None => {
                value_object.insert(key.clone(), default_value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::service::ServiceKind;

    use super::{AppConfig, ConfigError, ServiceConfig};

    #[test]
    fn default_config_uses_loopback_and_service_ports() {
        let config = AppConfig::default();

        assert_eq!(config.host, "127.0.0.1");
        assert!(config.rerank.enabled);
        assert_eq!(config.rerank.port, 8012);
        assert_eq!(config.rerank.model_path, "");
        assert_eq!(config.rerank.protocol, "llama_rerank");
        assert!(config.embedding.enabled);
        assert_eq!(config.embedding.port, 8013);
        assert_eq!(config.embedding.model_path, "");
        assert_eq!(config.embedding.protocol, "openai_embeddings");
        assert!(config.auto_start_server);
        assert!(config.restart_on_crash);
        assert_eq!(config.gpu_layers, "auto");
        assert_eq!(config.threads, 0);
        assert_eq!(config.parallel_slots, 0);
        assert_eq!(config.batch_size, 0);
        assert_eq!(config.ubatch_size, 0);
        assert_eq!(config.http_threads, 0);
        assert_eq!(config.log_retention_days, 7);
    }

    #[test]
    fn validates_gpu_layers_values() {
        for value in ["auto", "all", "0", "12", &u32::MAX.to_string()] {
            let json = format!(r#"{{"gpu_layers":"{value}"}}"#);
            assert!(
                AppConfig::from_json_with_defaults(&json).is_ok(),
                "{value} should be accepted"
            );
        }

        for value in ["auto ", "-1", "", "1.5", "foo"] {
            let json = format!(r#"{{"gpu_layers":"{value}"}}"#);
            assert!(
                AppConfig::from_json_with_defaults(&json).is_err(),
                "{value} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_empty_host_and_zero_retention() {
        assert!(AppConfig::from_json_with_defaults(r#"{"host":"   "}"#).is_err());
        assert!(AppConfig::from_json_with_defaults(r#"{"log_retention_days":0}"#).is_err());
    }

    #[test]
    fn rejects_zero_port() {
        assert!(AppConfig::from_json_with_defaults(r#"{"rerank":{"port":0}}"#).is_err());
        assert!(AppConfig::from_json_with_defaults(r#"{"embedding":{"port":0}}"#).is_err());
    }

    #[test]
    fn rejects_duplicate_enabled_service_ports() {
        let error = AppConfig::from_json_with_defaults(
            r#"{"rerank":{"port":8012},"embedding":{"port":8012}}"#,
        )
        .expect_err("duplicate enabled service ports should fail");

        assert!(matches!(
            error,
            ConfigError::DuplicateServicePorts { port: 8012 }
        ));
    }

    #[test]
    fn allows_duplicate_ports_when_one_service_is_disabled() {
        let config = AppConfig::from_json_with_defaults(
            r#"{"rerank":{"enabled":false,"port":8012},"embedding":{"port":8012}}"#,
        )
        .expect("duplicate ports should be allowed when rerank is disabled");

        assert!(!config.rerank.enabled);
        assert_eq!(config.rerank.port, 8012);
        assert_eq!(config.embedding.port, 8012);
    }

    #[test]
    fn rejects_legacy_top_level_rerank_config_fields() {
        assert!(AppConfig::from_json_with_defaults(
            r#"{"port":9001,"model_path":"C:\\models\\rerank.gguf"}"#,
        )
        .is_err());
    }

    #[test]
    fn fills_missing_fields_from_defaults() {
        let config = AppConfig::from_json_with_defaults(
            r#"{"host":"0.0.0.0","rerank":{"model_path":"C:\\models\\rerank.gguf"}}"#,
        )
        .expect("partial config should be merged with defaults");

        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.rerank.port, 8012);
        assert_eq!(config.rerank.model_path, r#"C:\models\rerank.gguf"#);
        assert_eq!(config.rerank.protocol, "llama_rerank");
        assert!(config.rerank.enabled);
        assert!(config.embedding.enabled);
        assert_eq!(config.embedding.port, 8013);
        assert_eq!(config.embedding.model_path, "");
        assert_eq!(config.embedding.protocol, "openai_embeddings");
        assert!(config.auto_start_server);
        assert!(config.restart_on_crash);
        assert_eq!(config.gpu_layers, "auto");
        assert_eq!(config.threads, 0);
        assert_eq!(config.parallel_slots, 0);
        assert_eq!(config.batch_size, 0);
        assert_eq!(config.ubatch_size, 0);
        assert_eq!(config.http_threads, 0);
        assert_eq!(config.log_retention_days, 7);
    }

    #[test]
    fn exposes_service_specific_config_views() {
        let config = AppConfig {
            rerank: ServiceConfig::new(false, 9001, r#"C:\models\rerank.gguf"#.to_string()),
            embedding: ServiceConfig::new_for(
                ServiceKind::Embedding,
                true,
                9002,
                r#"C:\models\embedding.gguf"#.to_string(),
            ),
            ..AppConfig::default()
        };

        let rerank = config.service_config(ServiceKind::Rerank);
        assert!(!rerank.enabled);
        assert_eq!(rerank.port, 9001);
        assert_eq!(rerank.model_path, r#"C:\models\rerank.gguf"#);

        let embedding = config.service_config(ServiceKind::Embedding);
        assert!(embedding.enabled);
        assert_eq!(embedding.port, 9002);
        assert_eq!(embedding.model_path, r#"C:\models\embedding.gguf"#);
    }

    #[test]
    fn accepts_vv_llm_retrieval_protocols() {
        for protocol in [
            "llama_rerank",
            "siliconflow",
            "cohere_rerank_v2",
            "jina_rerank_v1",
            "voyage_rerank_v1",
            "custom_json_http",
        ] {
            let json = format!(r#"{{"rerank":{{"protocol":"{protocol}"}}}}"#);
            AppConfig::from_json_with_defaults(&json).unwrap_or_else(|error| {
                panic!("rerank protocol {protocol} should be valid: {error}")
            });
        }

        for protocol in [
            "openai_embeddings",
            "siliconflow",
            "cohere_embed_v2",
            "voyage_embeddings_v1",
            "custom_json_http",
        ] {
            let json = format!(r#"{{"embedding":{{"protocol":"{protocol}"}}}}"#);
            AppConfig::from_json_with_defaults(&json).unwrap_or_else(|error| {
                panic!("embedding protocol {protocol} should be valid: {error}")
            });
        }
    }

    #[test]
    fn rejects_wrong_or_unknown_retrieval_protocols() {
        assert!(AppConfig::from_json_with_defaults(
            r#"{"rerank":{"protocol":"openai_embeddings"}}"#
        )
        .is_err());
        assert!(AppConfig::from_json_with_defaults(
            r#"{"embedding":{"protocol":"jina_rerank_v1"}}"#
        )
        .is_err());
        assert!(
            AppConfig::from_json_with_defaults(r#"{"rerank":{"protocol":"unknown"}}"#).is_err()
        );
        assert!(AppConfig::from_json_with_defaults(r#"{"embedding":{"protocol":""}}"#).is_err());
    }
}
