use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WwmiModDependencyKind {
    ObjectGuid,
    DrawCallTarget,
    TextureOverrideHash,
    ResourceFileReference,
    MeshVertexCount,
    ShapeKeyVertexCount,
    BufferLayoutHint,
    SkeletonMergeDependency,
    FilterIndex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct WwmiModDependencySignal {
    pub kind: WwmiModDependencyKind,
    pub value: String,
    pub source_file: String,
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct WwmiModDependencyProfile {
    pub mod_name: Option<String>,
    pub mod_root: String,
    pub ini_file_count: usize,
    pub signals: Vec<WwmiModDependencySignal>,
}

impl WwmiModDependencyProfile {
    pub fn has_kind(&self, kind: WwmiModDependencyKind) -> bool {
        self.signals.iter().any(|signal| signal.kind == kind)
    }

    pub fn kinds(&self) -> BTreeSet<WwmiModDependencyKind> {
        self.signals
            .iter()
            .map(|signal| signal.kind.clone())
            .collect()
    }
}

pub fn scan_mod_dependency_profile(mod_root: &Path) -> AppResult<WwmiModDependencyProfile> {
    if !mod_root.exists() || !mod_root.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "WWMI mod sample root does not exist or is not a directory: {}",
            mod_root.display()
        )));
    }

    let mut ini_files = Vec::new();
    collect_ini_files(mod_root, &mut ini_files)?;

    let mut signals = BTreeSet::<WwmiModDependencySignal>::new();
    for ini_file in &ini_files {
        parse_ini_file(mod_root, ini_file, &mut signals)?;
    }

    Ok(WwmiModDependencyProfile {
        mod_name: mod_root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        mod_root: mod_root.display().to_string(),
        ini_file_count: ini_files.len(),
        signals: signals.into_iter().collect(),
    })
}

pub fn load_mod_dependency_profile(path: &Path) -> AppResult<WwmiModDependencyProfile> {
    let profile: WwmiModDependencyProfile = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(profile)
}

fn collect_ini_files(root: &Path, out: &mut Vec<PathBuf>) -> AppResult<()> {
    for entry in fs::read_dir(root).map_err(|error| {
        AppError::InvalidInput(format!(
            "failed to read WWMI mod sample directory {}: {error}",
            root.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            AppError::InvalidInput(format!(
                "failed to read WWMI mod sample entry under {}: {error}",
                root.display()
            ))
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_ini_files(&path, out)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
        {
            out.push(path);
        }
    }

    Ok(())
}

fn parse_ini_file(
    mod_root: &Path,
    ini_file: &Path,
    signals: &mut BTreeSet<WwmiModDependencySignal>,
) -> AppResult<()> {
    let content = fs::read_to_string(ini_file).map_err(|error| {
        AppError::InvalidInput(format!(
            "failed to read WWMI mod sample file {}: {error}",
            ini_file.display()
        ))
    })?;
    let source_file = ini_file
        .strip_prefix(mod_root)
        .unwrap_or(ini_file)
        .display()
        .to_string()
        .replace('\\', "/");
    let mut current_section: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = Some(trimmed[1..trimmed.len() - 1].trim().to_string());
            continue;
        }

        let Some((lhs, rhs)) = trimmed.split_once('=') else {
            continue;
        };
        let key = lhs.trim();
        let value = rhs.trim();
        let section = current_section.clone();
        let section_lower = section.as_deref().unwrap_or_default().to_ascii_lowercase();
        let lower_line = trimmed.to_ascii_lowercase();

        if lower_line.starts_with("global $object_guid") {
            push_signal(
                signals,
                WwmiModDependencyKind::ObjectGuid,
                value,
                &source_file,
                section.as_deref(),
            );
        }
        if lower_line.starts_with("global $mesh_vertex_count") {
            push_signal(
                signals,
                WwmiModDependencyKind::MeshVertexCount,
                value,
                &source_file,
                section.as_deref(),
            );
        }
        if lower_line.starts_with("global $shapekey_vertex_count") {
            push_signal(
                signals,
                WwmiModDependencyKind::ShapeKeyVertexCount,
                value,
                &source_file,
                section.as_deref(),
            );
        }
        if key.eq_ignore_ascii_case("hash") && section_lower.starts_with("textureoverride") {
            push_signal(
                signals,
                WwmiModDependencyKind::TextureOverrideHash,
                value,
                &source_file,
                section.as_deref(),
            );
        }
        if matches!(key, "match_first_index" | "match_index_count") {
            push_signal(
                signals,
                WwmiModDependencyKind::DrawCallTarget,
                format!("{key}={value}"),
                &source_file,
                section.as_deref(),
            );
        }
        if key.eq_ignore_ascii_case("filter_index") {
            push_signal(
                signals,
                WwmiModDependencyKind::FilterIndex,
                value,
                &source_file,
                section.as_deref(),
            );
        }
        if matches!(
            key,
            "override_byte_stride" | "override_vertex_count" | "override_index_count"
        ) || lower_line.contains("vg_count")
            || lower_line.contains("vg_offset")
        {
            push_signal(
                signals,
                WwmiModDependencyKind::BufferLayoutHint,
                format!("{key}={value}"),
                &source_file,
                section.as_deref(),
            );
        }
        if key.eq_ignore_ascii_case("filename") && section_lower.starts_with("resource") {
            push_signal(
                signals,
                WwmiModDependencyKind::ResourceFileReference,
                value,
                &source_file,
                section.as_deref(),
            );
        }
        if lower_line.contains("resourcemergedskeleton")
            || lower_line.contains("resourceextramergedskeleton")
            || lower_line.contains("commandlistmergeskeleton")
            || lower_line.contains("commandlistupdatemergedskeleton")
            || lower_line.contains("commandlistremapmergedskeleton")
            || lower_line.contains("resourceextraremappedskeleton")
            || lower_line.contains("resourceremappedskeleton")
        {
            push_signal(
                signals,
                WwmiModDependencyKind::SkeletonMergeDependency,
                trimmed,
                &source_file,
                section.as_deref(),
            );
        }
    }

    Ok(())
}

fn push_signal(
    signals: &mut BTreeSet<WwmiModDependencySignal>,
    kind: WwmiModDependencyKind,
    value: impl Into<String>,
    source_file: &str,
    section: Option<&str>,
) {
    signals.insert(WwmiModDependencySignal {
        kind,
        value: value.into(),
        source_file: source_file.to_string(),
        section: section.map(|value| value.to_string()),
    });
}
