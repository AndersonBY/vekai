use anyhow::{anyhow, bail, Context};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

pub const RUNTIME_ZIP_FILE_HINT: &str = "llama-b*-bin-win-vulkan-x64.zip";
pub const RUNTIME_DIR_NAME: &str = "llama-bin-win-vulkan-x64";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMarker {
    pub zip_name: String,
    pub sha256: String,
}

impl RuntimeMarker {
    pub fn matches(&self, zip_name: &str, sha256: &str) -> bool {
        self.zip_name == zip_name && self.sha256.eq_ignore_ascii_case(sha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLayout {
    pub runtime_dir: PathBuf,
    pub server_exe: PathBuf,
    pub marker_path: PathBuf,
}

pub fn runtime_layout(runtime_root: &Path) -> RuntimeLayout {
    let runtime_dir = runtime_root.join(RUNTIME_DIR_NAME);
    RuntimeLayout {
        server_exe: runtime_dir.join("llama-server.exe"),
        marker_path: runtime_dir.join("runtime.json"),
        runtime_dir,
    }
}

pub fn ensure_runtime(runtime_root: &Path) -> anyhow::Result<RuntimeLayout> {
    let local_data_dir = runtime_root.parent().ok_or_else(|| {
        anyhow!(
            "runtime root has no parent local data directory: {}",
            runtime_root.display()
        )
    })?;
    let download_dirs = [local_data_dir.join("downloads")];
    ensure_runtime_from_downloads(runtime_root, &download_dirs)
}

pub fn ensure_runtime_from_downloads(
    runtime_root: &Path,
    download_dirs: &[PathBuf],
) -> anyhow::Result<RuntimeLayout> {
    fs::create_dir_all(runtime_root).context("failed to create runtime root")?;

    let layout = runtime_layout(runtime_root);
    let runtime_zip = find_runtime_zip(download_dirs)?;
    let sha256 = sha256_file_hex(&runtime_zip)?;
    let marker = RuntimeMarker {
        zip_name: runtime_zip
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| RUNTIME_ZIP_FILE_HINT.to_string()),
        sha256: sha256.clone(),
    };

    if layout.server_exe.exists() && layout.marker_path.exists() {
        if let Ok(existing_marker) = read_marker(&layout.marker_path) {
            if existing_marker.matches(&marker.zip_name, &sha256) {
                return Ok(layout);
            }
        }
    }

    let temp_dir = runtime_root.join(format!(".tmp-{}", Utc::now().timestamp_millis()));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)
            .with_context(|| format!("failed to clear temp runtime dir {}", temp_dir.display()))?;
    }
    fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp runtime dir {}", temp_dir.display()))?;

    if let Err(error) = extract_runtime_zip(&runtime_zip, &temp_dir, &marker) {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(error);
    }

    if !temp_dir.join("llama-server.exe").exists() {
        let _ = fs::remove_dir_all(&temp_dir);
        bail!(
            "runtime zip {} did not contain llama-server.exe",
            runtime_zip.display()
        );
    }

    replace_runtime_dir(&layout.runtime_dir, &temp_dir).with_context(|| {
        format!(
            "failed to install runtime into {}",
            layout.runtime_dir.display()
        )
    })?;

    Ok(layout)
}

fn find_runtime_zip(download_dirs: &[PathBuf]) -> anyhow::Result<PathBuf> {
    for dir in download_dirs {
        if !dir.is_dir() {
            continue;
        }

        let mut candidates = fs::read_dir(dir)
            .with_context(|| format!("failed to read runtime downloads dir {}", dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_windows_vulkan_runtime_zip)
            })
            .collect::<Vec<_>>();
        candidates.sort();
        if let Some(candidate) = candidates.pop() {
            return Ok(candidate);
        }
    }

    let searched = download_dirs
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "missing llama.cpp Windows Vulkan runtime zip. Run scripts\\download-assets.ps1 first. Searched: {searched}. Expected a file like {RUNTIME_ZIP_FILE_HINT}"
    )
}

fn is_windows_vulkan_runtime_zip(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.starts_with("llama-")
        && lower.ends_with("-bin-win-vulkan-x64.zip")
        && !lower.contains("cudart")
}

fn sha256_file_hex(path: &Path) -> anyhow::Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 64];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn read_marker(marker_path: &Path) -> anyhow::Result<RuntimeMarker> {
    let contents = fs::read_to_string(marker_path)
        .with_context(|| format!("failed to read runtime marker {}", marker_path.display()))?;
    let marker = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse runtime marker {}", marker_path.display()))?;
    Ok(marker)
}

fn extract_runtime_zip(
    zip_path: &Path,
    temp_dir: &Path,
    marker: &RuntimeMarker,
) -> anyhow::Result<()> {
    let reader =
        File::open(zip_path).with_context(|| format!("failed to open {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(reader)
        .with_context(|| format!("failed to open runtime zip {}", zip_path.display()))?;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry {index}"))?;

        let enclosed_name = file
            .enclosed_name()
            .with_context(|| format!("zip entry {index} has an unsafe path"))?;
        let out_path = temp_dir.join(enclosed_name);

        if file.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("failed to create directory {}", out_path.display()))?;
            continue;
        }

        if file.is_symlink() {
            bail!("zip entry {index} is a symlink, which is not allowed");
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create parent directory {}", parent.display())
            })?;
        }

        let mut output = File::create(&out_path)
            .with_context(|| format!("failed to create extracted file {}", out_path.display()))?;
        io::copy(&mut file, &mut output)
            .with_context(|| format!("failed to extract {}", out_path.display()))?;
        output
            .flush()
            .with_context(|| format!("failed to flush extracted file {}", out_path.display()))?;
    }

    let marker_path = temp_dir.join("runtime.json");
    let marker_json = serde_json::to_string_pretty(marker).context("failed to serialize marker")?;
    fs::write(&marker_path, marker_json)
        .with_context(|| format!("failed to write runtime marker {}", marker_path.display()))?;

    Ok(())
}

fn replace_runtime_dir(runtime_dir: &Path, temp_dir: &Path) -> anyhow::Result<()> {
    let parent = runtime_dir
        .parent()
        .ok_or_else(|| anyhow!("runtime dir has no parent: {}", runtime_dir.display()))?;

    if runtime_dir.exists() {
        let backup_dir = parent.join(format!(".old-{}", Utc::now().timestamp_millis()));
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir).with_context(|| {
                format!(
                    "failed to clear backup runtime dir {}",
                    backup_dir.display()
                )
            })?;
        }

        fs::rename(runtime_dir, &backup_dir).with_context(|| {
            format!(
                "failed to move existing runtime dir {} to backup {}",
                runtime_dir.display(),
                backup_dir.display()
            )
        })?;

        match fs::rename(temp_dir, runtime_dir) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup_dir);
                Ok(())
            }
            Err(error) => {
                let restore_result = fs::rename(&backup_dir, runtime_dir);
                if let Err(restore_error) = restore_result {
                    return Err(anyhow!(
                        "failed to install runtime: {error}; also failed to restore previous runtime: {restore_error}"
                    ));
                }
                Err(error.into())
            }
        }
    } else {
        fs::rename(temp_dir, runtime_dir).with_context(|| {
            format!(
                "failed to move temp runtime dir {} into place at {}",
                temp_dir.display(),
                runtime_dir.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn runtime_layout_points_to_expected_server() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let runtime_root = temp_dir.path().join("runtimes");
        let layout = runtime_layout(&runtime_root);

        assert_eq!(layout.runtime_dir, runtime_root.join(RUNTIME_DIR_NAME));
        assert_eq!(layout.server_exe, layout.runtime_dir.join("llama-server.exe"));
        assert_eq!(layout.marker_path, layout.runtime_dir.join("runtime.json"));
    }

    #[test]
    fn marker_matches_same_zip_name_and_hash() {
        let zip_name = "llama-b9999-bin-win-vulkan-x64.zip";
        let marker = RuntimeMarker {
            zip_name: zip_name.to_string(),
            sha256: "ABCDEF123456".to_string(),
        };

        assert!(marker.matches(zip_name, "abcdef123456"));
        assert!(!marker.matches("other.zip", "abcdef123456"));
        assert!(!marker.matches(zip_name, "deadbeef"));
    }

    #[test]
    fn ensure_runtime_extracts_server_and_marker_from_local_downloads() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let local_data_dir = temp_dir.path().join("local-data");
        let downloads_dir = local_data_dir.join("downloads");
        fs::create_dir_all(&downloads_dir).expect("downloads dir should be created");
        let zip_path = downloads_dir.join("llama-b9999-bin-win-vulkan-x64.zip");
        write_test_runtime_zip(&zip_path);

        let layout =
            ensure_runtime(&local_data_dir.join("runtimes")).expect("runtime should be ensured");

        assert!(layout.server_exe.exists());
        assert!(layout.marker_path.exists());

        let marker: RuntimeMarker = serde_json::from_str(
            &fs::read_to_string(&layout.marker_path).expect("runtime marker should be readable"),
        )
        .expect("runtime marker should parse");

        assert_eq!(marker.zip_name, "llama-b9999-bin-win-vulkan-x64.zip");
        assert_eq!(
            marker.sha256,
            sha256_file_hex(&zip_path).expect("zip hash should be computed")
        );
    }

    #[test]
    fn ensure_runtime_is_idempotent_when_marker_matches() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let local_data_dir = temp_dir.path().join("local-data");
        let downloads_dir = local_data_dir.join("downloads");
        fs::create_dir_all(&downloads_dir).expect("downloads dir should be created");
        write_test_runtime_zip(&downloads_dir.join("llama-b9999-bin-win-vulkan-x64.zip"));

        let first_layout =
            ensure_runtime(&local_data_dir.join("runtimes")).expect("first ensure should succeed");
        let first_marker =
            fs::read_to_string(&first_layout.marker_path).expect("first marker should exist");

        let second_layout =
            ensure_runtime(&local_data_dir.join("runtimes")).expect("second ensure should succeed");
        let second_marker =
            fs::read_to_string(&second_layout.marker_path).expect("second marker should exist");

        assert_eq!(first_layout, second_layout);
        assert_eq!(first_marker, second_marker);
    }

    #[test]
    fn ensure_runtime_from_downloaded_zip_extracts_server_and_marker() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let downloads_dir = temp_dir.path().join("downloads");
        fs::create_dir_all(&downloads_dir).expect("downloads dir should be created");
        let zip_path = downloads_dir.join("llama-b9999-bin-win-vulkan-x64.zip");
        write_test_runtime_zip(&zip_path);

        let layout = ensure_runtime_from_downloads(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
        )
        .expect("runtime should be ensured from downloaded zip");

        assert!(layout.server_exe.exists());
        assert!(layout.marker_path.exists());
        let marker: RuntimeMarker = serde_json::from_str(
            &fs::read_to_string(&layout.marker_path).expect("runtime marker should be readable"),
        )
        .expect("runtime marker should parse");
        assert_eq!(marker.zip_name, "llama-b9999-bin-win-vulkan-x64.zip");
        assert_eq!(
            marker.sha256,
            sha256_file_hex(&zip_path).expect("zip hash should be computed")
        );
    }

    #[test]
    fn ensure_runtime_from_downloaded_zip_reports_missing_asset() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let downloads_dir = temp_dir.path().join("downloads");

        let error = ensure_runtime_from_downloads(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
        )
        .expect_err("missing runtime zip should fail clearly");

        assert!(error.to_string().contains(RUNTIME_ZIP_FILE_HINT));
        assert!(error.to_string().contains("scripts\\download-assets.ps1"));
    }

    fn write_test_runtime_zip(path: &Path) {
        let file = fs::File::create(path).expect("test zip should be created");
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions = zip::write::FileOptions::default();
        zip.start_file("llama-server.exe", options)
            .expect("server entry should start");
        zip.write_all(b"fake server")
            .expect("server entry should be written");
        zip.finish().expect("test zip should finish");
    }
}
