use anyhow::Context;
use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::service::ServiceKind;

pub const MAX_SERVER_LOG_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Default)]
pub struct InferenceLogFormatter {
    active_tasks: HashMap<u64, Instant>,
}

impl InferenceLogFormatter {
    pub fn format(&mut self, line: &str) -> String {
        self.task_summary(line).unwrap_or_else(|| line.to_string())
    }

    fn task_summary(&mut self, line: &str) -> Option<String> {
        if !line.contains(" I slot ") {
            return None;
        }
        let task = line
            .split_once("| task")?
            .1
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?;
        if line.contains("processing task,") {
            self.active_tasks.insert(task, Instant::now());
            return Some(format!("推理开始 task={task}"));
        }
        if line.contains("stop processing:") {
            let started = self.active_tasks.remove(&task)?;
            let tokens = line
                .split_once("n_tokens = ")?
                .1
                .split(',')
                .next()?
                .trim()
                .parse::<u64>()
                .ok()?;
            return Some(format!(
                "推理结束 task={task} tokens={tokens} 耗时={} ms",
                started.elapsed().as_millis()
            ));
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPaths {
    pub log_dir: PathBuf,
    pub app_log: PathBuf,
    pub server_log: PathBuf,
    pub embedding_server_log: PathBuf,
}

pub fn log_paths(local_data_dir: &Path) -> LogPaths {
    let log_dir = local_data_dir.join("logs");

    LogPaths {
        app_log: log_dir.join("app.log"),
        server_log: log_dir.join("rerank-server.log"),
        embedding_server_log: log_dir.join("embedding-server.log"),
        log_dir,
    }
}

impl LogPaths {
    pub fn server_log_for(&self, service: ServiceKind) -> &Path {
        match service {
            ServiceKind::Rerank => &self.server_log,
            ServiceKind::Embedding => &self.embedding_server_log,
        }
    }
}

pub fn ensure_log_dir(paths: &LogPaths) -> anyhow::Result<()> {
    fs::create_dir_all(&paths.log_dir)
        .with_context(|| format!("failed to create log directory {}", paths.log_dir.display()))
}

pub fn append_log(path: &Path, line: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create log parent directory {}", parent.display())
        })?;
    }

    let timestamp = Utc::now().to_rfc3339();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open log file {}", path.display()))?;

    writeln!(file, "[{timestamp}] {line}")
        .with_context(|| format!("failed to append log line to {}", path.display()))
}

pub fn prepare_server_log(path: &Path) -> anyhow::Result<bool> {
    truncate_log_if_oversized(path, MAX_SERVER_LOG_BYTES)
}

fn truncate_log_if_oversized(path: &Path, max_bytes: u64) -> anyhow::Result<bool> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(false);
    };

    if metadata.len() <= max_bytes {
        return Ok(false);
    }

    let timestamp = Utc::now().to_rfc3339();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("failed to truncate oversized log file {}", path.display()))?;

    writeln!(
        file,
        "[{timestamp}] Runtime log exceeded {max_bytes} bytes and was truncated before restart."
    )
    .with_context(|| format!("failed to write truncation notice to {}", path.display()))?;

    Ok(true)
}

pub fn cleanup_old_logs(log_dir: &Path, retention_days: u16) -> anyhow::Result<()> {
    if !log_dir.exists() {
        return Ok(());
    }

    let cutoff =
        std::time::SystemTime::from(Utc::now() - Duration::days(i64::from(retention_days)));

    for entry in fs::read_dir(log_dir)
        .with_context(|| format!("failed to read log directory {}", log_dir.display()))?
    {
        let entry = entry.with_context(|| {
            format!(
                "failed to read log directory entry in {}",
                log_dir.display()
            )
        })?;
        let file_type = entry.file_type().with_context(|| {
            format!(
                "failed to read log directory entry type for {}",
                entry.path().display()
            )
        })?;

        if !file_type.is_file() {
            continue;
        }

        let metadata = entry.metadata().with_context(|| {
            format!(
                "failed to read log file metadata for {}",
                entry.path().display()
            )
        })?;
        let modified = metadata.modified().with_context(|| {
            format!(
                "failed to read log file modified time for {}",
                entry.path().display()
            )
        })?;

        if modified < cutoff {
            let _ = fs::remove_file(entry.path());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn inference_summaries_track_interleaved_tasks_and_preserve_other_output() {
        let mut formatter = InferenceLogFormatter::default();
        for task in [10, 11] {
            assert_eq!(formatter.format(&format!("0.18.846.379 I slot launch_slot_: id 0 | task {task} | processing task, is_child = 0")), format!("推理开始 task={task}"));
        }
        for task in [11, 10] {
            let result = formatter.format(&format!("0.18.986.671 I slot release: id 0 | task {task} | stop processing: n_tokens = 85, truncated = 0"));
            assert!(result.starts_with(&format!("推理结束 task={task} tokens=85 耗时=")));
            assert!(result.ends_with(" ms"));
        }
        assert!(formatter.active_tasks.is_empty());
        for line in [
            "error: failed to encode image",
            "fastpath engine ready",
            "I slot release: id 0 | task 22 | stop processing: n_tokens = 5",
            "I slot release: unknown format",
        ] {
            assert_eq!(formatter.format(line), line);
        }
    }

    #[test]
    fn log_paths_points_to_expected_files_under_logs_dir() {
        let paths = log_paths(Path::new(r"C:\Users\LocalData"));

        assert_eq!(paths.log_dir, Path::new(r"C:\Users\LocalData").join("logs"));
        assert_eq!(
            paths.app_log,
            Path::new(r"C:\Users\LocalData")
                .join("logs")
                .join("app.log")
        );
        assert_eq!(
            paths.server_log,
            Path::new(r"C:\Users\LocalData")
                .join("logs")
                .join("rerank-server.log")
        );
        assert_eq!(
            paths.embedding_server_log,
            Path::new(r"C:\Users\LocalData")
                .join("logs")
                .join("embedding-server.log")
        );
    }

    #[test]
    fn service_log_lookup_returns_service_specific_server_logs() {
        let paths = log_paths(Path::new(r"C:\Users\LocalData"));

        assert!(paths
            .server_log_for(ServiceKind::Rerank)
            .ends_with("rerank-server.log"));
        assert!(paths
            .server_log_for(ServiceKind::Embedding)
            .ends_with("embedding-server.log"));
    }

    #[test]
    fn append_log_creates_parent_directory_and_writes_line() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let log_path = temp_dir.path().join("nested").join("app.log");

        append_log(&log_path, "hello world").expect("log line should be appended");

        let contents = fs::read_to_string(log_path).expect("log should be readable");
        assert!(contents.contains("hello world"));
        assert_eq!(contents.lines().count(), 1);
    }

    #[test]
    fn cleanup_old_logs_accepts_missing_directory() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let missing_dir = temp_dir.path().join("missing");

        cleanup_old_logs(&missing_dir, 7).expect("missing log dir should be ok");
    }

    #[test]
    fn oversized_server_log_is_truncated_with_notice() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let log_path = temp_dir.path().join("server.log");
        fs::write(&log_path, vec![b'x'; 9]).expect("test log should be written");

        assert!(truncate_log_if_oversized(&log_path, 8).expect("oversized log should be truncated"));

        let contents = fs::read_to_string(&log_path).expect("truncated log should be readable");
        assert!(contents.contains("Runtime log exceeded 8 bytes"));
        assert!(contents.len() < 256);
    }

    #[test]
    fn server_log_within_limit_is_preserved() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let log_path = temp_dir.path().join("server.log");
        fs::write(&log_path, b"keep me").expect("test log should be written");

        assert!(!truncate_log_if_oversized(&log_path, 8).expect("small log should be accepted"));
        assert_eq!(
            fs::read(&log_path).expect("log should be readable"),
            b"keep me"
        );
    }
}
