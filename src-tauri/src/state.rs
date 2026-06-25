use crate::config::AppConfig;
use crate::config::ServiceConfig;
use crate::service::ServiceKind;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

const RECENT_LOG_CAPACITY: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSnapshot {
    pub status: ServiceStatus,
    pub config: AppConfig,
    pub recent_logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceView {
    pub status: ServiceStatus,
    pub config: ServiceConfig,
    pub recent_logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub config: AppConfig,
    pub rerank: ServiceView,
    pub embedding: ServiceView,
    pub recent_logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct ServiceState {
    pub status: ServiceStatus,
    pub recent_logs: VecDeque<String>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct AppStateInner {
    pub status: ServiceStatus,
    pub embedding_status: ServiceStatus,
    pub config: AppConfig,
    pub recent_logs: VecDeque<String>,
    pub embedding_recent_logs: VecDeque<String>,
    pub error: Option<String>,
    pub embedding_error: Option<String>,
}

pub type SharedAppState = Arc<Mutex<AppStateInner>>;

impl AppStateInner {
    pub fn new(config: AppConfig) -> Self {
        Self {
            status: ServiceStatus::Stopped,
            embedding_status: ServiceStatus::Stopped,
            config,
            recent_logs: VecDeque::new(),
            embedding_recent_logs: VecDeque::new(),
            error: None,
            embedding_error: None,
        }
    }

    pub fn push_log(&mut self, line: impl Into<String>) {
        if self.recent_logs.len() >= RECENT_LOG_CAPACITY {
            self.recent_logs.pop_front();
        }
        self.recent_logs.push_back(line.into());
    }

    pub fn push_service_log(&mut self, service: ServiceKind, line: impl Into<String>) {
        match service {
            ServiceKind::Rerank => self.push_log(line),
            ServiceKind::Embedding => {
                if self.embedding_recent_logs.len() >= RECENT_LOG_CAPACITY {
                    self.embedding_recent_logs.pop_front();
                }
                self.embedding_recent_logs.push_back(line.into());
            }
        }
    }

    pub fn set_service_status(&mut self, service: ServiceKind, status: ServiceStatus) {
        match service {
            ServiceKind::Rerank => self.status = status,
            ServiceKind::Embedding => self.embedding_status = status,
        }
    }

    pub fn set_service_error(&mut self, service: ServiceKind, error: Option<String>) {
        match service {
            ServiceKind::Rerank => self.error = error,
            ServiceKind::Embedding => self.embedding_error = error,
        }
    }

    pub fn service_status(&self, service: ServiceKind) -> ServiceStatus {
        match service {
            ServiceKind::Rerank => self.status,
            ServiceKind::Embedding => self.embedding_status,
        }
    }

    pub fn service_snapshot(&self, service: ServiceKind) -> ServiceView {
        let (status, recent_logs, error, config) = match service {
            ServiceKind::Rerank => (
                self.status,
                self.recent_logs.iter().cloned().collect(),
                self.error.clone(),
                self.config.service_config(ServiceKind::Rerank),
            ),
            ServiceKind::Embedding => (
                self.embedding_status,
                self.embedding_recent_logs.iter().cloned().collect(),
                self.embedding_error.clone(),
                self.config.service_config(ServiceKind::Embedding),
            ),
        };

        ServiceView {
            status,
            config,
            recent_logs,
            error,
        }
    }

    pub fn snapshot_v2(&self) -> AppSnapshot {
        AppSnapshot {
            config: self.config.clone(),
            rerank: self.service_snapshot(ServiceKind::Rerank),
            embedding: self.service_snapshot(ServiceKind::Embedding),
            recent_logs: self.recent_logs.iter().cloned().collect(),
            error: self.error.clone(),
        }
    }

    pub fn snapshot(&self) -> ServiceSnapshot {
        ServiceSnapshot {
            status: self.status,
            config: self.config.clone(),
            recent_logs: self.recent_logs.iter().cloned().collect(),
            error: self.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServiceConfig};
    use crate::service::ServiceKind;

    #[test]
    fn push_log_keeps_only_latest_300_entries() {
        let mut state = AppStateInner::new(AppConfig::default());

        for index in 0..305 {
            state.push_log(format!("line {index}"));
        }

        assert_eq!(state.recent_logs.len(), 300);
        assert_eq!(state.recent_logs.front().unwrap(), "line 5");
        assert_eq!(state.recent_logs.back().unwrap(), "line 304");
    }

    #[test]
    fn snapshot_returns_current_state_with_logs_and_error() {
        let config = AppConfig {
            host: "0.0.0.0".to_string(),
            rerank: ServiceConfig::new(true, 9001, String::new()),
            ..AppConfig::default()
        };
        let mut state = AppStateInner::new(config.clone());
        state.status = ServiceStatus::Failed;
        state.error = Some("server exited".to_string());
        state.push_log("first".to_string());
        state.push_log("second".to_string());

        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, ServiceStatus::Failed);
        assert_eq!(snapshot.config, config);
        assert_eq!(snapshot.recent_logs, vec!["first", "second"]);
        assert_eq!(snapshot.error.as_deref(), Some("server exited"));
    }

    #[test]
    fn snapshot_v2_includes_rerank_and_embedding_views() {
        let config = AppConfig {
            rerank: ServiceConfig::new(true, 9001, String::new()),
            embedding: ServiceConfig::new_for(
                ServiceKind::Embedding,
                true,
                9002,
                r#"C:\models\embedding.gguf"#.to_string(),
            ),
            ..AppConfig::default()
        };
        let mut state = AppStateInner::new(config.clone());
        state.status = ServiceStatus::Running;
        state.embedding_status = ServiceStatus::Failed;
        state.error = Some("rerank failed".to_string());
        state.embedding_error = Some("embedding failed".to_string());
        state.push_log("rerank log");
        state.push_service_log(ServiceKind::Embedding, "embedding log");

        let snapshot = state.snapshot_v2();

        assert_eq!(snapshot.config, config);
        assert_eq!(snapshot.rerank.status, ServiceStatus::Running);
        assert_eq!(snapshot.rerank.config.port, 9001);
        assert_eq!(snapshot.embedding.status, ServiceStatus::Failed);
        assert_eq!(snapshot.embedding.config.port, 9002);
        assert_eq!(
            snapshot.embedding.config.model_path,
            r#"C:\models\embedding.gguf"#
        );
        assert_eq!(snapshot.embedding.recent_logs, vec!["embedding log"]);
    }
}
