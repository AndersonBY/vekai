use anyhow::{anyhow, bail, Context};
use chrono::Utc;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use tar::{Archive, EntryType};
use zip::ZipArchive;

pub const RUNTIME_ARCHIVE_FILE_HINT: &str = "llama-b*-bin-<platform>.<zip|tar.gz>";
pub const RUNTIME_ZIP_FILE_HINT: &str = "llama-b*-bin-win-vulkan-x64.zip";
pub const RUNTIME_DIR_NAME: &str = "llama-bin-win-vulkan-x64";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePlatform {
    WindowsVulkanX64,
    WindowsCpuArm64,
    UbuntuX64,
    UbuntuArm64,
    MacosX64,
    MacosArm64,
}

impl RuntimePlatform {
    pub fn current() -> Self {
        current_runtime_platform()
    }

    pub fn runtime_dir_name(self) -> &'static str {
        match self {
            Self::WindowsVulkanX64 => "llama-bin-win-vulkan-x64",
            Self::WindowsCpuArm64 => "llama-bin-win-cpu-arm64",
            Self::UbuntuX64 => "llama-bin-ubuntu-x64",
            Self::UbuntuArm64 => "llama-bin-ubuntu-arm64",
            Self::MacosX64 => "llama-bin-macos-x64",
            Self::MacosArm64 => "llama-bin-macos-arm64",
        }
    }

    pub fn server_file_name(self) -> &'static str {
        if matches!(self, Self::WindowsVulkanX64 | Self::WindowsCpuArm64) {
            "llama-server.exe"
        } else {
            "llama-server"
        }
    }

    pub fn archive_hint(self) -> &'static str {
        match self {
            Self::WindowsVulkanX64 => "llama-b*-bin-win-vulkan-x64.zip",
            Self::WindowsCpuArm64 => "llama-b*-bin-win-cpu-arm64.zip",
            Self::UbuntuX64 => "llama-b*-bin-ubuntu-x64.tar.gz",
            Self::UbuntuArm64 => "llama-b*-bin-ubuntu-arm64.tar.gz",
            Self::MacosX64 => "llama-b*-bin-macos-x64.tar.gz",
            Self::MacosArm64 => "llama-b*-bin-macos-arm64.tar.gz",
        }
    }

    pub fn matches_asset_name(self, file_name: &str) -> bool {
        let lower = file_name.to_ascii_lowercase();
        match self {
            Self::WindowsVulkanX64 => {
                lower.starts_with("llama-")
                    && lower.ends_with("-bin-win-vulkan-x64.zip")
                    && !lower.contains("cudart")
            }
            Self::WindowsCpuArm64 => {
                lower.starts_with("llama-") && lower.ends_with("-bin-win-cpu-arm64.zip")
            }
            Self::UbuntuX64 => {
                lower.starts_with("llama-") && lower.ends_with("-bin-ubuntu-x64.tar.gz")
            }
            Self::UbuntuArm64 => {
                lower.starts_with("llama-") && lower.ends_with("-bin-ubuntu-arm64.tar.gz")
            }
            Self::MacosX64 => {
                lower.starts_with("llama-") && lower.ends_with("-bin-macos-x64.tar.gz")
            }
            Self::MacosArm64 => {
                lower.starts_with("llama-") && lower.ends_with("-bin-macos-arm64.tar.gz")
            }
        }
    }
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::WindowsVulkanX64
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::WindowsCpuArm64
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::UbuntuX64
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::UbuntuArm64
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::MacosX64
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::MacosArm64
}

#[cfg(not(any(
    all(
        target_os = "windows",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(
        target_os = "macos",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::UbuntuX64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMarker {
    #[serde(alias = "zip_name")]
    pub archive_name: String,
    pub sha256: String,
}

impl RuntimeMarker {
    pub fn matches(&self, archive_name: &str, sha256: &str) -> bool {
        self.archive_name == archive_name && self.sha256.eq_ignore_ascii_case(sha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLayout {
    pub runtime_dir: PathBuf,
    pub server_exe: PathBuf,
    pub marker_path: PathBuf,
}

pub fn runtime_layout(runtime_root: &Path) -> RuntimeLayout {
    runtime_layout_for_platform(runtime_root, RuntimePlatform::current())
}

pub fn runtime_layout_for_platform(
    runtime_root: &Path,
    platform: RuntimePlatform,
) -> RuntimeLayout {
    let runtime_dir = runtime_root.join(platform.runtime_dir_name());
    RuntimeLayout {
        server_exe: runtime_dir.join(platform.server_file_name()),
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
    ensure_runtime_from_downloads_for_platform(
        runtime_root,
        download_dirs,
        RuntimePlatform::current(),
    )
}

pub fn ensure_runtime_from_downloads_for_platform(
    runtime_root: &Path,
    download_dirs: &[PathBuf],
    platform: RuntimePlatform,
) -> anyhow::Result<RuntimeLayout> {
    fs::create_dir_all(runtime_root).context("failed to create runtime root")?;

    let layout = runtime_layout_for_platform(runtime_root, platform);
    let runtime_archive = find_runtime_archive(download_dirs, platform)?;
    let sha256 = sha256_file_hex(&runtime_archive)?;
    let marker = RuntimeMarker {
        archive_name: runtime_archive
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| platform.archive_hint().to_string()),
        sha256: sha256.clone(),
    };

    if layout.server_exe.exists() && layout.marker_path.exists() {
        if let Ok(existing_marker) = read_marker(&layout.marker_path) {
            if existing_marker.matches(&marker.archive_name, &sha256) {
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

    if let Err(error) = extract_runtime_archive(&runtime_archive, &temp_dir, &marker) {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(error);
    }

    normalize_runtime_root(&temp_dir, platform.server_file_name())?;

    if !temp_dir.join(platform.server_file_name()).exists() {
        let _ = fs::remove_dir_all(&temp_dir);
        bail!(
            "runtime archive {} did not contain {}",
            runtime_archive.display(),
            platform.server_file_name()
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

fn find_runtime_archive(
    download_dirs: &[PathBuf],
    platform: RuntimePlatform,
) -> anyhow::Result<PathBuf> {
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
                    .is_some_and(|name| platform.matches_asset_name(name))
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
        "missing llama.cpp runtime archive for current platform. Run scripts\\download-assets.ps1 first. Searched: {searched}. Expected a file like {}",
        platform.archive_hint()
    )
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

fn extract_runtime_archive(
    archive_path: &Path,
    temp_dir: &Path,
    marker: &RuntimeMarker,
) -> anyhow::Result<()> {
    let file_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if file_name.ends_with(".zip") {
        extract_runtime_zip(archive_path, temp_dir, marker)
    } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        extract_runtime_tar_gz(archive_path, temp_dir, marker)
    } else {
        bail!(
            "unsupported runtime archive format: {}",
            archive_path.display()
        )
    }
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

fn extract_runtime_tar_gz(
    archive_path: &Path,
    temp_dir: &Path,
    marker: &RuntimeMarker,
) -> anyhow::Result<()> {
    let reader = File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let decoder = GzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    for entry in archive
        .entries()
        .with_context(|| format!("failed to read runtime archive {}", archive_path.display()))?
    {
        let mut entry = entry
            .with_context(|| format!("failed to read entry from {}", archive_path.display()))?;
        let entry_type = entry.header().entry_type();
        if matches!(entry_type, EntryType::Symlink | EntryType::Link) {
            validate_tar_link_target(&entry)?;
        }
        let extracted = entry.unpack_in(temp_dir).with_context(|| {
            format!(
                "failed to extract runtime archive {}",
                archive_path.display()
            )
        })?;
        if !extracted {
            bail!("runtime archive contains unsafe entry path");
        }
    }

    write_runtime_marker(temp_dir, marker)
}

fn validate_tar_link_target<R: Read>(entry: &tar::Entry<'_, R>) -> anyhow::Result<()> {
    let Some(link_name) = entry
        .link_name()
        .context("failed to read runtime archive link")?
    else {
        bail!("runtime archive contains a link entry without a target");
    };

    if !is_safe_relative_path(&link_name) {
        bail!("runtime archive contains unsafe link target");
    }

    Ok(())
}

fn is_safe_relative_path(path: &Path) -> bool {
    let mut has_component = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => has_component = true,
            _ => return false,
        }
    }

    has_component
}

fn write_runtime_marker(temp_dir: &Path, marker: &RuntimeMarker) -> anyhow::Result<()> {
    let marker_path = temp_dir.join("runtime.json");
    let marker_json = serde_json::to_string_pretty(marker).context("failed to serialize marker")?;
    fs::write(&marker_path, marker_json)
        .with_context(|| format!("failed to write runtime marker {}", marker_path.display()))?;

    Ok(())
}

fn normalize_runtime_root(temp_dir: &Path, server_file_name: &str) -> anyhow::Result<()> {
    if temp_dir.join(server_file_name).exists() {
        return Ok(());
    }

    let mut directories = fs::read_dir(temp_dir)
        .with_context(|| format!("failed to inspect {}", temp_dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .filter(|path| path.join(server_file_name).exists())
        .collect::<Vec<_>>();
    directories.sort();
    if directories.len() != 1 {
        return Ok(());
    }

    let source_dir = directories.remove(0);
    for entry in fs::read_dir(&source_dir)
        .with_context(|| format!("failed to inspect {}", source_dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to inspect {}", source_dir.display()))?;
        let destination = temp_dir.join(entry.file_name());
        fs::rename(entry.path(), &destination)
            .with_context(|| format!("failed to move runtime file to {}", destination.display()))?;
    }
    fs::remove_dir_all(&source_dir)
        .with_context(|| format!("failed to remove {}", source_dir.display()))?;

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
        let layout = runtime_layout_for_platform(&runtime_root, RuntimePlatform::WindowsVulkanX64);

        assert_eq!(layout.runtime_dir, runtime_root.join(RUNTIME_DIR_NAME));
        assert_eq!(
            layout.server_exe,
            layout.runtime_dir.join("llama-server.exe")
        );
        assert_eq!(layout.marker_path, layout.runtime_dir.join("runtime.json"));
    }

    #[test]
    fn runtime_layout_uses_unix_server_name_for_linux_targets() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let runtime_root = temp_dir.path().join("runtimes");
        let layout = runtime_layout_for_platform(&runtime_root, RuntimePlatform::UbuntuArm64);

        assert_eq!(
            layout.runtime_dir,
            runtime_root.join("llama-bin-ubuntu-arm64")
        );
        assert_eq!(layout.server_exe, layout.runtime_dir.join("llama-server"));
        assert_eq!(layout.marker_path, layout.runtime_dir.join("runtime.json"));
    }

    #[test]
    fn runtime_asset_matching_is_platform_specific() {
        assert!(RuntimePlatform::WindowsVulkanX64
            .matches_asset_name("llama-b9999-bin-win-vulkan-x64.zip"));
        assert!(
            RuntimePlatform::UbuntuArm64.matches_asset_name("llama-b9999-bin-ubuntu-arm64.tar.gz")
        );
        assert!(RuntimePlatform::UbuntuX64.matches_asset_name("llama-b9999-bin-ubuntu-x64.tar.gz"));
        assert!(
            RuntimePlatform::MacosArm64.matches_asset_name("llama-b9999-bin-macos-arm64.tar.gz")
        );
        assert!(!RuntimePlatform::UbuntuArm64
            .matches_asset_name("llama-b9999-bin-ubuntu-vulkan-arm64.tar.gz"));
        assert!(
            !RuntimePlatform::UbuntuArm64.matches_asset_name("llama-b9999-bin-win-vulkan-x64.zip")
        );
    }

    #[test]
    fn marker_matches_same_zip_name_and_hash() {
        let zip_name = "llama-b9999-bin-win-vulkan-x64.zip";
        let marker = RuntimeMarker {
            archive_name: zip_name.to_string(),
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

        let layout = ensure_runtime_from_downloads_for_platform(
            &local_data_dir.join("runtimes"),
            &[downloads_dir],
            RuntimePlatform::WindowsVulkanX64,
        )
        .expect("runtime should be ensured");

        assert!(layout.server_exe.exists());
        assert!(layout.marker_path.exists());

        let marker: RuntimeMarker = serde_json::from_str(
            &fs::read_to_string(&layout.marker_path).expect("runtime marker should be readable"),
        )
        .expect("runtime marker should parse");

        assert_eq!(marker.archive_name, "llama-b9999-bin-win-vulkan-x64.zip");
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

        let first_layout = ensure_runtime_from_downloads_for_platform(
            &local_data_dir.join("runtimes"),
            std::slice::from_ref(&downloads_dir),
            RuntimePlatform::WindowsVulkanX64,
        )
        .expect("first ensure should succeed");
        let first_marker =
            fs::read_to_string(&first_layout.marker_path).expect("first marker should exist");

        let second_layout = ensure_runtime_from_downloads_for_platform(
            &local_data_dir.join("runtimes"),
            &[downloads_dir],
            RuntimePlatform::WindowsVulkanX64,
        )
        .expect("second ensure should succeed");
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

        let layout = ensure_runtime_from_downloads_for_platform(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
            RuntimePlatform::WindowsVulkanX64,
        )
        .expect("runtime should be ensured from downloaded zip");

        assert!(layout.server_exe.exists());
        assert!(layout.marker_path.exists());
        let marker: RuntimeMarker = serde_json::from_str(
            &fs::read_to_string(&layout.marker_path).expect("runtime marker should be readable"),
        )
        .expect("runtime marker should parse");
        assert_eq!(marker.archive_name, "llama-b9999-bin-win-vulkan-x64.zip");
        assert_eq!(
            marker.sha256,
            sha256_file_hex(&zip_path).expect("zip hash should be computed")
        );
    }

    #[test]
    fn ensure_runtime_from_downloaded_zip_reports_missing_asset() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let downloads_dir = temp_dir.path().join("downloads");

        let error = ensure_runtime_from_downloads_for_platform(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
            RuntimePlatform::WindowsVulkanX64,
        )
        .expect_err("missing runtime zip should fail clearly");

        assert!(error.to_string().contains(RUNTIME_ZIP_FILE_HINT));
        assert!(error.to_string().contains("scripts\\download-assets.ps1"));
    }

    #[test]
    fn ensure_runtime_from_linux_tar_gz_extracts_server_and_marker() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let downloads_dir = temp_dir.path().join("downloads");
        fs::create_dir_all(&downloads_dir).expect("downloads dir should be created");
        let archive_path = downloads_dir.join("llama-b9999-bin-ubuntu-arm64.tar.gz");
        write_test_runtime_tar_gz(&archive_path);

        let layout = ensure_runtime_from_downloads_for_platform(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
            RuntimePlatform::UbuntuArm64,
        )
        .expect("runtime should be ensured from downloaded tar.gz");

        assert!(layout.server_exe.exists());
        assert!(layout.marker_path.exists());
        let marker: RuntimeMarker = serde_json::from_str(
            &fs::read_to_string(&layout.marker_path).expect("runtime marker should be readable"),
        )
        .expect("runtime marker should parse");
        assert_eq!(marker.archive_name, "llama-b9999-bin-ubuntu-arm64.tar.gz");
        assert_eq!(
            marker.sha256,
            sha256_file_hex(&archive_path).expect("archive hash should be computed")
        );
    }

    #[test]
    fn ensure_runtime_from_linux_tar_gz_flattens_single_top_level_dir() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let downloads_dir = temp_dir.path().join("downloads");
        fs::create_dir_all(&downloads_dir).expect("downloads dir should be created");
        let archive_path = downloads_dir.join("llama-b9999-bin-ubuntu-arm64.tar.gz");
        write_test_runtime_tar_gz_with_top_level_dir(&archive_path);

        let layout = ensure_runtime_from_downloads_for_platform(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
            RuntimePlatform::UbuntuArm64,
        )
        .expect("runtime should be ensured from official-style tar.gz");

        assert!(layout.server_exe.exists());
        assert!(layout.runtime_dir.join("LICENSE").exists());
        assert!(!layout.runtime_dir.join("llama-b9999").exists());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_runtime_from_linux_tar_gz_allows_safe_relative_symlinks() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let downloads_dir = temp_dir.path().join("downloads");
        fs::create_dir_all(&downloads_dir).expect("downloads dir should be created");
        let archive_path = downloads_dir.join("llama-b9999-bin-ubuntu-arm64.tar.gz");
        write_test_runtime_tar_gz_with_safe_symlink(&archive_path);

        let layout = ensure_runtime_from_downloads_for_platform(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
            RuntimePlatform::UbuntuArm64,
        )
        .expect("runtime archive with safe relative symlink should be extracted");

        assert!(layout.server_exe.exists());
        assert!(layout.runtime_dir.join("libllama.so").exists());
    }

    #[test]
    fn ensure_runtime_rejects_tar_gz_unsafe_link_entries() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let downloads_dir = temp_dir.path().join("downloads");
        fs::create_dir_all(&downloads_dir).expect("downloads dir should be created");
        let archive_path = downloads_dir.join("llama-b9999-bin-ubuntu-arm64.tar.gz");
        write_test_runtime_tar_gz_with_unsafe_link(&archive_path);

        let error = ensure_runtime_from_downloads_for_platform(
            temp_dir.path().join("runtimes").as_path(),
            &[downloads_dir],
            RuntimePlatform::UbuntuArm64,
        )
        .expect_err("runtime archive with link entries should be rejected");

        assert!(error.to_string().contains("unsafe link target"));
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

    fn write_test_runtime_tar_gz(path: &Path) {
        let file = fs::File::create(path).expect("test tar.gz should be created");
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        let contents = b"fake server";
        header
            .set_path("llama-server")
            .expect("server path should be set");
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append(&header, &contents[..])
            .expect("server entry should be written");
        let encoder = tar.into_inner().expect("tar should finish");
        encoder.finish().expect("gzip should finish");
    }

    fn write_test_runtime_tar_gz_with_top_level_dir(path: &Path) {
        let file = fs::File::create(path).expect("test tar.gz should be created");
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);

        let mut server_header = tar::Header::new_gnu();
        let server_contents = b"fake server";
        server_header
            .set_path("llama-b9999/llama-server")
            .expect("server path should be set");
        server_header.set_size(server_contents.len() as u64);
        server_header.set_mode(0o755);
        server_header.set_cksum();
        tar.append(&server_header, &server_contents[..])
            .expect("server entry should be written");

        let mut license_header = tar::Header::new_gnu();
        let license_contents = b"license";
        license_header
            .set_path("llama-b9999/LICENSE")
            .expect("license path should be set");
        license_header.set_size(license_contents.len() as u64);
        license_header.set_mode(0o644);
        license_header.set_cksum();
        tar.append(&license_header, &license_contents[..])
            .expect("license entry should be written");

        let encoder = tar.into_inner().expect("tar should finish");
        encoder.finish().expect("gzip should finish");
    }

    #[cfg(unix)]
    fn write_test_runtime_tar_gz_with_safe_symlink(path: &Path) {
        let file = fs::File::create(path).expect("test tar.gz should be created");
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);

        let mut server_header = tar::Header::new_gnu();
        let server_contents = b"fake server";
        server_header
            .set_path("llama-server")
            .expect("server path should be set");
        server_header.set_size(server_contents.len() as u64);
        server_header.set_mode(0o755);
        server_header.set_cksum();
        tar.append(&server_header, &server_contents[..])
            .expect("server entry should be written");

        let mut library_header = tar::Header::new_gnu();
        let library_contents = b"fake library";
        library_header
            .set_path("libllama.so.0")
            .expect("library path should be set");
        library_header.set_size(library_contents.len() as u64);
        library_header.set_mode(0o755);
        library_header.set_cksum();
        tar.append(&library_header, &library_contents[..])
            .expect("library entry should be written");

        let mut link_header = tar::Header::new_gnu();
        link_header
            .set_path("libllama.so")
            .expect("link path should be set");
        link_header
            .set_link_name("libllama.so.0")
            .expect("link target should be set");
        link_header.set_entry_type(tar::EntryType::Symlink);
        link_header.set_size(0);
        link_header.set_cksum();
        tar.append(&link_header, std::io::empty())
            .expect("link entry should be written");

        let encoder = tar.into_inner().expect("tar should finish");
        encoder.finish().expect("gzip should finish");
    }

    fn write_test_runtime_tar_gz_with_unsafe_link(path: &Path) {
        let file = fs::File::create(path).expect("test tar.gz should be created");
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header
            .set_path("llama-server")
            .expect("link path should be set");
        header
            .set_link_name("../outside")
            .expect("link target should be set");
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        tar.append(&header, std::io::empty())
            .expect("link entry should be written");
        let encoder = tar.into_inner().expect("tar should finish");
        encoder.finish().expect("gzip should finish");
    }
}
