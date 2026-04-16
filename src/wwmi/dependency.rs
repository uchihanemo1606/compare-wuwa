use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum WwmiModDependencySurfaceClass {
    #[default]
    MappingHash,
    BufferLayout,
    ResourceSkeleton,
    DrawCallFilterHook,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WwmiModDependencyBaselineStrength {
    #[default]
    Sparse,
    Partial,
    Broad,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct WwmiModDependencySurfaceClassCount {
    pub surface_class: WwmiModDependencySurfaceClass,
    pub profile_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct WwmiModDependencyBaselineReview {
    pub source_roots: Vec<String>,
    pub included_mod_count: usize,
    pub surface_class_counts: Vec<WwmiModDependencySurfaceClassCount>,
    pub strength: WwmiModDependencyBaselineStrength,
    pub material_for_repair_review: bool,
    pub caution_notes: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct WwmiModDependencyBaselineSet {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub version_id: String,
    pub profile_count: usize,
    pub profiles: Vec<WwmiModDependencyProfile>,
    #[serde(default)]
    pub review: WwmiModDependencyBaselineReview,
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

    pub fn surface_classes(&self) -> BTreeSet<WwmiModDependencySurfaceClass> {
        self.kinds()
            .into_iter()
            .map(WwmiModDependencySurfaceClass::from_kind)
            .collect()
    }
}

impl WwmiModDependencySurfaceClass {
    pub fn from_kind(kind: WwmiModDependencyKind) -> Self {
        match kind {
            WwmiModDependencyKind::TextureOverrideHash => Self::MappingHash,
            WwmiModDependencyKind::BufferLayoutHint
            | WwmiModDependencyKind::MeshVertexCount
            | WwmiModDependencyKind::ShapeKeyVertexCount => Self::BufferLayout,
            WwmiModDependencyKind::ResourceFileReference
            | WwmiModDependencyKind::SkeletonMergeDependency => Self::ResourceSkeleton,
            WwmiModDependencyKind::ObjectGuid
            | WwmiModDependencyKind::DrawCallTarget
            | WwmiModDependencyKind::FilterIndex => Self::DrawCallFilterHook,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::MappingHash => "mapping_hash",
            Self::BufferLayout => "buffer_layout",
            Self::ResourceSkeleton => "resource_skeleton",
            Self::DrawCallFilterHook => "draw_call_filter_hook",
        }
    }
}

impl WwmiModDependencyBaselineSet {
    pub fn represented_surface_classes(&self) -> Vec<WwmiModDependencySurfaceClass> {
        self.review
            .surface_class_counts
            .iter()
            .filter(|entry| entry.profile_count > 0)
            .map(|entry| entry.surface_class.clone())
            .collect()
    }

    pub fn represented_surface_labels(&self) -> Vec<&'static str> {
        self.represented_surface_classes()
            .into_iter()
            .map(|surface| surface.label())
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

pub fn load_mod_dependency_baseline_set(path: &Path) -> AppResult<WwmiModDependencyBaselineSet> {
    let mut baseline_set: WwmiModDependencyBaselineSet =
        serde_json::from_str(&fs::read_to_string(path)?)?;
    hydrate_baseline_set_review(&mut baseline_set);
    Ok(baseline_set)
}

pub fn build_mod_dependency_baseline_set(
    version_id: &str,
    mut profiles: Vec<WwmiModDependencyProfile>,
) -> AppResult<WwmiModDependencyBaselineSet> {
    profiles = normalize_profiles(profiles);
    profiles.sort_by(|left, right| {
        left.mod_name
            .as_deref()
            .unwrap_or(left.mod_root.as_str())
            .cmp(right.mod_name.as_deref().unwrap_or(right.mod_root.as_str()))
    });

    Ok(WwmiModDependencyBaselineSet {
        schema_version: "whashreonator.wwmi-mod-dependency-baselines.v1".to_string(),
        generated_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| AppError::InvalidInput(format!("system clock error: {error}")))?
            .as_millis(),
        version_id: version_id.to_string(),
        profile_count: profiles.len(),
        review: build_baseline_review(&profiles),
        profiles,
    })
}

fn normalize_profiles(profiles: Vec<WwmiModDependencyProfile>) -> Vec<WwmiModDependencyProfile> {
    let mut merged = BTreeMap::<String, WwmiModDependencyProfile>::new();
    for profile in profiles {
        match merged.entry(profile.mod_root.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(profile);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                if existing.mod_name.is_none() {
                    existing.mod_name = profile.mod_name.clone();
                }
                existing.ini_file_count = existing.ini_file_count.max(profile.ini_file_count);
                existing.signals = existing
                    .signals
                    .iter()
                    .cloned()
                    .chain(profile.signals.into_iter())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
            }
        }
    }

    merged.into_values().collect()
}

fn hydrate_baseline_set_review(baseline_set: &mut WwmiModDependencyBaselineSet) {
    if baseline_set.profile_count == 0 && !baseline_set.profiles.is_empty() {
        baseline_set.profile_count = baseline_set.profiles.len();
    }
    if review_metadata_is_missing(&baseline_set.review) {
        baseline_set.review = build_baseline_review(&baseline_set.profiles);
    }
}

fn review_metadata_is_missing(review: &WwmiModDependencyBaselineReview) -> bool {
    review.source_roots.is_empty()
        && review.included_mod_count == 0
        && review.surface_class_counts.is_empty()
        && review.caution_notes.is_empty()
        && matches!(review.strength, WwmiModDependencyBaselineStrength::Sparse)
        && !review.material_for_repair_review
}

fn build_baseline_review(profiles: &[WwmiModDependencyProfile]) -> WwmiModDependencyBaselineReview {
    let source_roots = profiles
        .iter()
        .map(|profile| profile.mod_root.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let surface_class_counts = [
        WwmiModDependencySurfaceClass::MappingHash,
        WwmiModDependencySurfaceClass::BufferLayout,
        WwmiModDependencySurfaceClass::ResourceSkeleton,
        WwmiModDependencySurfaceClass::DrawCallFilterHook,
    ]
    .into_iter()
    .map(|surface_class| WwmiModDependencySurfaceClassCount {
        profile_count: profiles
            .iter()
            .filter(|profile| profile.surface_classes().contains(&surface_class))
            .count(),
        surface_class,
    })
    .filter(|entry| entry.profile_count > 0)
    .collect::<Vec<_>>();

    let represented_surface_count = surface_class_counts.len();
    let included_mod_count = source_roots.len();
    let material_for_repair_review = included_mod_count >= 4 && represented_surface_count >= 2;
    let strength = if included_mod_count >= 8 && represented_surface_count >= 3 {
        WwmiModDependencyBaselineStrength::Broad
    } else if material_for_repair_review {
        WwmiModDependencyBaselineStrength::Partial
    } else {
        WwmiModDependencyBaselineStrength::Sparse
    };

    let mut caution_notes = vec![
        "curated representative baseline from sampled mod roots only; it is reviewer guidance, not an exhaustive live-mod census".to_string(),
    ];
    if included_mod_count < 4 {
        caution_notes.push(format!(
            "sample stays sparse with only {} included profile(s)/mod root(s)",
            included_mod_count
        ));
    }
    if represented_surface_count <= 1 {
        caution_notes.push(
            "surface coverage is narrow; only one dependency surface class is represented"
                .to_string(),
        );
    } else if represented_surface_count <= 2 {
        caution_notes.push(format!(
            "surface coverage is still partial; only {} dependency surface classes are represented",
            represented_surface_count
        ));
    }
    if !material_for_repair_review {
        caution_notes.push(
            "baseline exists but remains too limited to materially steer repair review on its own"
                .to_string(),
        );
    }

    WwmiModDependencyBaselineReview {
        source_roots,
        included_mod_count,
        surface_class_counts,
        strength,
        material_for_repair_review,
        caution_notes,
    }
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
