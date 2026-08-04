use anyhow::{Context, Result};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
use winreg::RegKey;

const PORTABLE_CONFIG_FILE: &str = "config.json";
const REDIST_DIR: &str = "redist";
const VC_REDIST_INSTALLER: &str = "vc_redist.x64.exe";
const VC_REDIST_VERSION_FILE: &str = "version.txt";
const VC_RUNTIME_REGISTRY_KEY: &str = r"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RuntimeVersion([u32; 4]);

impl fmt::Display for RuntimeVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{}.{}.{}",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

impl FromStr for RuntimeVersion {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let value = value.trim().trim_start_matches(['v', 'V']);
        let parts = value.split('.').collect::<Vec<_>>();
        if parts.is_empty() || parts.len() > 4 || parts.iter().any(|part| part.is_empty()) {
            anyhow::bail!("invalid runtime version: {value}");
        }

        let mut version = [0_u32; 4];
        for (index, part) in parts.into_iter().enumerate() {
            version[index] = part
                .parse::<u32>()
                .with_context(|| format!("invalid runtime version component: {part}"))?;
        }
        Ok(Self(version))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VcRuntimeRequirement {
    pub installer_path: PathBuf,
    pub installed_version: Option<RuntimeVersion>,
    pub required_version: RuntimeVersion,
}

pub fn detect_portable_vc_runtime_requirement() -> Result<Option<VcRuntimeRequirement>> {
    let executable = std::env::current_exe().context("failed to resolve Vekai executable")?;
    let executable_dir = executable
        .parent()
        .context("Vekai executable has no parent directory")?;
    requirement_for_layout(executable_dir, installed_vc_runtime_version())
}

fn requirement_for_layout(
    executable_dir: &Path,
    installed_version: Option<RuntimeVersion>,
) -> Result<Option<VcRuntimeRequirement>> {
    let installer_path = executable_dir.join(REDIST_DIR).join(VC_REDIST_INSTALLER);
    let version_path = executable_dir.join(REDIST_DIR).join(VC_REDIST_VERSION_FILE);

    // The prerequisite gate is intentionally portable-only. Normal installations and
    // development builds do not have this complete exe-adjacent layout.
    if !executable_dir.join(PORTABLE_CONFIG_FILE).is_file()
        || !installer_path.is_file()
        || !version_path.is_file()
    {
        return Ok(None);
    }

    let required_version = fs::read_to_string(&version_path)
        .with_context(|| format!("failed to read {}", version_path.display()))?
        .parse::<RuntimeVersion>()
        .with_context(|| format!("invalid version in {}", version_path.display()))?;

    if installed_version.is_some_and(|version| version >= required_version) {
        return Ok(None);
    }

    Ok(Some(VcRuntimeRequirement {
        installer_path,
        installed_version,
        required_version,
    }))
}

fn installed_vc_runtime_version() -> Option<RuntimeVersion> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let runtime = machine
        .open_subkey_with_flags(VC_RUNTIME_REGISTRY_KEY, KEY_READ | KEY_WOW64_64KEY)
        .ok()?;
    let installed = runtime.get_value::<u32, _>("Installed").unwrap_or(0);
    if installed != 1 {
        return None;
    }

    runtime
        .get_value::<String, _>("Version")
        .ok()?
        .parse::<RuntimeVersion>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_portable_layout(root: &Path, required_version: &str) {
        fs::write(root.join(PORTABLE_CONFIG_FILE), "{}").expect("config should be written");
        let redist_dir = root.join(REDIST_DIR);
        fs::create_dir_all(&redist_dir).expect("redist directory should be created");
        fs::write(redist_dir.join(VC_REDIST_INSTALLER), b"installer")
            .expect("installer should be written");
        fs::write(redist_dir.join(VC_REDIST_VERSION_FILE), required_version)
            .expect("version should be written");
    }

    #[test]
    fn runtime_versions_accept_registry_prefix_and_compare_numerically() {
        let old = "v14.22.27821.0"
            .parse::<RuntimeVersion>()
            .expect("old version should parse");
        let required = "14.44.35211.0"
            .parse::<RuntimeVersion>()
            .expect("required version should parse");
        assert!(old < required);
        assert_eq!(required.to_string(), "14.44.35211.0");
    }

    #[test]
    fn incomplete_layout_does_not_enable_portable_prerequisite_gate() {
        let temp_dir = tempfile::tempdir().expect("temp directory should be created");
        fs::write(temp_dir.path().join(PORTABLE_CONFIG_FILE), "{}")
            .expect("config should be written");

        let requirement = requirement_for_layout(temp_dir.path(), None)
            .expect("incomplete layout should be accepted");
        assert!(requirement.is_none());
    }

    #[test]
    fn missing_or_old_runtime_requires_bundled_installer() {
        let temp_dir = tempfile::tempdir().expect("temp directory should be created");
        write_portable_layout(temp_dir.path(), "14.44.35211.0");

        let missing = requirement_for_layout(temp_dir.path(), None)
            .expect("missing runtime should be detected")
            .expect("installer should be required");
        assert_eq!(missing.installed_version, None);
        assert_eq!(
            missing.installer_path,
            temp_dir.path().join(REDIST_DIR).join(VC_REDIST_INSTALLER)
        );

        let old = "14.22.27821.0"
            .parse::<RuntimeVersion>()
            .expect("old version should parse");
        assert!(requirement_for_layout(temp_dir.path(), Some(old))
            .expect("old runtime should be detected")
            .is_some());
    }

    #[test]
    fn current_runtime_allows_portable_app_to_continue() {
        let temp_dir = tempfile::tempdir().expect("temp directory should be created");
        write_portable_layout(temp_dir.path(), "14.44.35211.0");
        let current = "14.50.35710.0"
            .parse::<RuntimeVersion>()
            .expect("current version should parse");

        assert!(requirement_for_layout(temp_dir.path(), Some(current))
            .expect("current runtime should be accepted")
            .is_none());
    }
}
