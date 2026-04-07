use std::path::{Component, Path, PathBuf};

use crate::{
    error::{AppError, AppResult},
    report::VersionDiffReportV2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildProfile {
    DebugLike,
    Release,
}

pub fn active_profile() -> BuildProfile {
    if cfg!(debug_assertions) {
        BuildProfile::DebugLike
    } else {
        BuildProfile::Release
    }
}

pub fn default_artifact_root() -> &'static str {
    match active_profile() {
        BuildProfile::DebugLike => "out",
        BuildProfile::Release => "release",
    }
}

pub fn resolve_artifact_root() -> PathBuf {
    let project_root = detect_project_root().unwrap_or_else(fallback_current_dir);
    artifact_root_for_project(&project_root, active_profile())
}

pub fn resolve_report_store_root() -> PathBuf {
    resolve_artifact_root().join("report")
}

pub fn artifact_root_for_project(project_root: &Path, profile: BuildProfile) -> PathBuf {
    match profile {
        BuildProfile::DebugLike => project_root.join("out"),
        BuildProfile::Release => project_root.join("release"),
    }
}

pub fn validate_artifact_output_path(path: &Path) -> AppResult<()> {
    if path.as_os_str().is_empty() {
        return Err(AppError::InvalidInput(
            "artifact output path must not be empty".to_string(),
        ));
    }

    if path.is_absolute() {
        let project_root = detect_project_root().unwrap_or_else(fallback_current_dir);
        if path.starts_with(project_root.join("src"))
            || path.starts_with(project_root.join("tests"))
        {
            return Err(AppError::InvalidInput(format!(
                "artifact output path {} is not allowed under source directories",
                path.display(),
            )));
        }
        return Ok(());
    }

    let normalized = normalize_components(path);
    if normalized.is_empty() {
        return Err(AppError::InvalidInput(
            "artifact output path must not be empty".to_string(),
        ));
    }

    let root_name = normalized[0].to_ascii_lowercase();
    if matches!(root_name.as_str(), "src" | "tests") {
        return Err(AppError::InvalidInput(format!(
            "artifact output path {} is not allowed under {}",
            path.display(),
            normalized[0]
        )));
    }

    if normalized.len() == 1 {
        return Err(AppError::InvalidInput(format!(
            "artifact output path {} must be written under a dedicated directory, not the project root",
            path.display()
        )));
    }

    if active_profile() == BuildProfile::Release && root_name != "release" {
        return Err(AppError::InvalidInput(format!(
            "release build artifacts must be written under release/, got {}",
            path.display()
        )));
    }

    Ok(())
}

pub fn default_report_library_path(report: &VersionDiffReportV2) -> PathBuf {
    let safe_old = sanitize_segment(&report.old_version.version_id);
    let safe_new = sanitize_segment(&report.new_version.version_id);
    resolve_report_store_root()
        .join(format!("wuwa_{safe_new}"))
        .join("report_bundle")
        .join(format!("{safe_old}-to-{safe_new}.report.v2.json"))
}

fn sanitize_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || character == '-'
                || character == '_'
                || character == '.'
            {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn normalize_components(path: &Path) -> Vec<String> {
    let mut collected = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => collected.push(value.to_string_lossy().to_string()),
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => collected.push("..".to_string()),
        }
    }
    collected
}

fn detect_project_root() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && let Some(root) = find_project_root(dir)
    {
        return Some(root);
    }

    if let Ok(cwd) = std::env::current_dir()
        && let Some(root) = find_project_root(&cwd)
    {
        return Some(root);
    }

    None
}

fn find_project_root(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("Cargo.toml").exists() {
            return Some(candidate.to_path_buf());
        }
    }
    None
}

fn fallback_current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        BuildProfile, active_profile, artifact_root_for_project, default_artifact_root,
        validate_artifact_output_path,
    };

    #[test]
    fn output_policy_rejects_src_tests_and_root_paths() {
        assert!(validate_artifact_output_path(Path::new("src/report.json")).is_err());
        assert!(validate_artifact_output_path(Path::new("tests/report.json")).is_err());
        assert!(validate_artifact_output_path(Path::new("report.json")).is_err());
    }

    #[test]
    fn output_policy_accepts_profile_default_root() {
        let path = match active_profile() {
            BuildProfile::DebugLike => Path::new("out/report.json"),
            BuildProfile::Release => Path::new("release/report.json"),
        };

        assert!(validate_artifact_output_path(path).is_ok());
        assert!(matches!(default_artifact_root(), "out" | "release"));
    }

    #[test]
    fn artifact_root_for_release_does_not_duplicate_release_segment() {
        let root = artifact_root_for_project(Path::new("D:/repo"), BuildProfile::Release);
        let rendered = root
            .to_string_lossy()
            .to_ascii_lowercase()
            .replace('\\', "/");
        assert!(rendered.ends_with("/repo/release"));
        assert!(!rendered.contains("/release/release"));
    }
}
