use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    compare::{SnapshotCompareReport, SnapshotComparer},
    error::{AppError, AppResult},
    export::{
        export_inference_output, export_mapping_proposal_output,
        export_proposal_patch_draft_output, export_snapshot_compare_output, export_snapshot_output,
        export_text_output, export_version_diff_report_v2,
    },
    inference::InferenceReport,
    output_policy::{resolve_artifact_root, resolve_report_store_root},
    proposal::{MappingProposalOutput, ProposalArtifacts, ProposalPatchDraftOutput},
    report::{VersionDiffReportBuilder, VersionDiffReportV2, load_version_diff_report_v2},
    snapshot::{GameSnapshot, load_snapshot},
};

const VERSION_DIR_PREFIX: &str = "wuwa_";

#[derive(Debug, Clone)]
pub struct SavedReportBundle {
    pub directory: PathBuf,
    pub report_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ReportListEntry {
    pub path: PathBuf,
    pub old_version: String,
    pub new_version: String,
    pub generated_at_unix_ms: u128,
    pub resonators: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionArtifactKind {
    Snapshot,
    ReportBundle,
    InferenceData,
    ProposalData,
    HumanSummary,
    BufferData,
    HashData,
    Auxiliary,
    LegacySnapshot,
    LegacyReportBundle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionArtifactEntry {
    pub kind: VersionArtifactKind,
    pub path: PathBuf,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct StoredVersionEntry {
    pub version_id: String,
    pub directory: PathBuf,
    pub artifacts: Vec<VersionArtifactEntry>,
    pub has_snapshot: bool,
    pub has_report_bundle: bool,
    pub has_inference_data: bool,
    pub has_proposal_data: bool,
    pub has_human_summary: bool,
    pub has_buffer_data: bool,
    pub has_hash_data: bool,
}

#[derive(Debug, Clone)]
pub struct VersionLayoutPaths {
    pub version_dir: PathBuf,
    pub snapshot_dir: PathBuf,
    pub report_bundle_dir: PathBuf,
    pub inference_dir: PathBuf,
    pub proposal_dir: PathBuf,
    pub summary_dir: PathBuf,
    pub buffer_dir: PathBuf,
    pub hash_dir: PathBuf,
    pub auxiliary_dir: PathBuf,
    pub snapshot_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ReportStorage {
    root: PathBuf,
    legacy_root: PathBuf,
}

impl Default for ReportStorage {
    fn default() -> Self {
        Self::new(resolve_report_store_root())
    }
}

impl ReportStorage {
    pub fn new(root: PathBuf) -> Self {
        let legacy_root = root
            .parent()
            .map(|parent| parent.join("reports"))
            .unwrap_or_else(|| PathBuf::from("reports"));
        Self::with_legacy_root(root, legacy_root)
    }

    pub fn with_legacy_root(root: PathBuf, legacy_root: PathBuf) -> Self {
        Self { root, legacy_root }
    }

    pub fn reports_root(&self) -> PathBuf {
        self.root.clone()
    }

    pub fn legacy_reports_root(&self) -> PathBuf {
        self.legacy_root.clone()
    }

    pub fn build_version_directory(&self, version_id: &str) -> PathBuf {
        self.root.join(version_directory_name(version_id))
    }

    pub fn build_version_layout(&self, version_id: &str) -> VersionLayoutPaths {
        let version_dir = self.build_version_directory(version_id);
        let snapshot_dir = version_dir.join("snapshot");
        let report_bundle_dir = version_dir.join("report_bundle");
        let inference_dir = version_dir.join("inference");
        let proposal_dir = version_dir.join("proposal");
        let summary_dir = version_dir.join("summary");
        let buffer_dir = version_dir.join("buffer");
        let hash_dir = version_dir.join("hash");
        let auxiliary_dir = version_dir.join("auxiliary");
        let snapshot_path = snapshot_dir.join(format!(
            "{VERSION_DIR_PREFIX}{}.snapshot.v1.json",
            sanitize_version_segment(version_id)
        ));

        VersionLayoutPaths {
            version_dir,
            snapshot_dir,
            report_bundle_dir,
            inference_dir,
            proposal_dir,
            summary_dir,
            buffer_dir,
            hash_dir,
            auxiliary_dir,
            snapshot_path,
        }
    }

    pub fn ensure_version_layout(&self, version_id: &str) -> AppResult<VersionLayoutPaths> {
        let layout = self.build_version_layout(version_id);
        fs::create_dir_all(&layout.snapshot_dir)?;
        fs::create_dir_all(&layout.report_bundle_dir)?;
        fs::create_dir_all(&layout.inference_dir)?;
        fs::create_dir_all(&layout.proposal_dir)?;
        fs::create_dir_all(&layout.summary_dir)?;
        fs::create_dir_all(&layout.buffer_dir)?;
        fs::create_dir_all(&layout.hash_dir)?;
        fs::create_dir_all(&layout.auxiliary_dir)?;
        Ok(layout)
    }

    pub fn save_run(
        &self,
        report: &VersionDiffReportV2,
        old_snapshot: &GameSnapshot,
        new_snapshot: &GameSnapshot,
        compare: &SnapshotCompareReport,
        inference: Option<&InferenceReport>,
    ) -> AppResult<SavedReportBundle> {
        let layout = self.ensure_version_layout(&report.new_version.version_id)?;
        let run_dir = layout.report_bundle_dir.join(format!(
            "{}-to-{}-{}",
            sanitize_version_segment(&report.old_version.version_id),
            sanitize_version_segment(&report.new_version.version_id),
            report.generated_at_unix_ms
        ));
        fs::create_dir_all(&run_dir)?;

        let report_path = run_dir.join("report.v2.json");
        export_version_diff_report_v2(report, &report_path)?;
        export_snapshot_output(old_snapshot, &run_dir.join("old.snapshot.json"))?;
        export_snapshot_output(new_snapshot, &run_dir.join("new.snapshot.json"))?;
        export_snapshot_compare_output(compare, &run_dir.join("compare.v1.json"))?;
        if let Some(inference) = inference {
            export_inference_output(inference, &run_dir.join("inference.v1.json"))?;
        }

        Ok(SavedReportBundle {
            directory: run_dir,
            report_path,
        })
    }

    pub fn save_phase3_outputs(
        &self,
        report: &VersionDiffReportV2,
        old_snapshot: &GameSnapshot,
        new_snapshot: &GameSnapshot,
        compare: &SnapshotCompareReport,
        inference: &InferenceReport,
        proposals: &ProposalArtifacts,
        human_summary: &str,
    ) -> AppResult<SavedReportBundle> {
        let saved = self.save_run(report, old_snapshot, new_snapshot, compare, Some(inference))?;
        let layout = self.ensure_version_layout(&report.new_version.version_id)?;

        let stamp = inference.generated_at_unix_ms;
        let pair_label = format!(
            "{}-to-{}",
            sanitize_version_segment(&report.old_version.version_id),
            sanitize_version_segment(&report.new_version.version_id)
        );

        export_inference_output(
            inference,
            &layout
                .inference_dir
                .join(format!("{stamp}-{pair_label}.inference.v1.json")),
        )?;
        export_mapping_proposal_output(
            &proposals.mapping_proposal,
            &layout
                .proposal_dir
                .join(format!("{stamp}-{pair_label}.mapping-proposal.v1.json")),
        )?;
        export_proposal_patch_draft_output(
            &proposals.patch_draft,
            &layout
                .proposal_dir
                .join(format!("{stamp}-{pair_label}.proposal-patch-draft.v1.json")),
        )?;
        export_text_output(
            human_summary,
            &layout
                .summary_dir
                .join(format!("{stamp}-{pair_label}.human-summary.md")),
        )?;

        Ok(saved)
    }

    pub fn snapshot_path_for_version(&self, version_id: &str) -> PathBuf {
        self.build_version_layout(version_id).snapshot_path
    }

    pub fn save_snapshot_for_version(&self, snapshot: &GameSnapshot) -> AppResult<PathBuf> {
        let layout = self.ensure_version_layout(&snapshot.version_id)?;
        export_snapshot_output(snapshot, &layout.snapshot_path)?;
        Ok(layout.snapshot_path)
    }

    pub fn find_snapshot_by_version(&self, version_id: &str) -> AppResult<Option<PathBuf>> {
        let path = self.snapshot_path_for_version(version_id);
        if path.exists() {
            return Ok(Some(path));
        }

        for legacy_path in self.legacy_snapshot_paths_for_version(version_id) {
            if legacy_path.exists() {
                return Ok(Some(legacy_path));
            }
        }

        Ok(None)
    }

    pub fn load_snapshot_by_version(&self, version_id: &str) -> AppResult<Option<GameSnapshot>> {
        let Some(path) = self.find_snapshot_by_version(version_id)? else {
            return Ok(None);
        };

        Ok(Some(load_snapshot(&path)?))
    }

    pub fn list_versions(&self) -> AppResult<Vec<StoredVersionEntry>> {
        let mut by_version = BTreeMap::<String, Vec<VersionArtifactEntry>>::new();

        if self.root.exists() {
            for entry in fs::read_dir(&self.root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }

                let file_name = entry.file_name().to_string_lossy().to_string();
                let Some(version_id) = parse_version_from_version_dir(&file_name) else {
                    continue;
                };

                let artifacts = collect_artifacts_from_new_layout(&entry.path())?;
                by_version.entry(version_id).or_default().extend(artifacts);
            }
        }

        let legacy_snapshots_root = self.legacy_root.join("snapshots");
        if legacy_snapshots_root.exists() {
            for entry in fs::read_dir(&legacy_snapshots_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }

                let file_name = entry.file_name().to_string_lossy().to_string();
                let Some(version_id) = parse_version_from_snapshot_file(&file_name) else {
                    continue;
                };
                by_version
                    .entry(version_id)
                    .or_default()
                    .push(VersionArtifactEntry {
                        kind: VersionArtifactKind::LegacySnapshot,
                        label: format!("legacy snapshot | {}", entry.path().display()),
                        path: entry.path(),
                    });
            }
        }

        if self.legacy_root.exists() {
            for entry in fs::read_dir(&self.legacy_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }

                let report_path = entry.path().join("report.v2.json");
                if !report_path.exists() {
                    continue;
                }

                let report = load_version_diff_report_v2(&report_path)?;
                for version_id in [
                    &report.old_version.version_id,
                    &report.new_version.version_id,
                ] {
                    by_version
                        .entry(version_id.clone())
                        .or_default()
                        .push(VersionArtifactEntry {
                            kind: VersionArtifactKind::LegacyReportBundle,
                            label: format!(
                                "legacy report bundle {} -> {}",
                                report.old_version.version_id, report.new_version.version_id
                            ),
                            path: report_path.clone(),
                        });
                }
            }
        }

        let mut versions = by_version
            .into_iter()
            .map(|(version_id, mut artifacts)| {
                artifacts.sort_by(|left, right| {
                    left.kind
                        .cmp(&right.kind)
                        .then_with(|| left.path.cmp(&right.path))
                });
                dedup_artifacts(&mut artifacts);

                let has_snapshot = artifacts.iter().any(|artifact| {
                    matches!(
                        artifact.kind,
                        VersionArtifactKind::Snapshot | VersionArtifactKind::LegacySnapshot
                    )
                });
                let has_report_bundle = artifacts.iter().any(|artifact| {
                    matches!(
                        artifact.kind,
                        VersionArtifactKind::ReportBundle | VersionArtifactKind::LegacyReportBundle
                    )
                });
                let has_inference_data = artifacts
                    .iter()
                    .any(|artifact| artifact.kind == VersionArtifactKind::InferenceData);
                let has_proposal_data = artifacts
                    .iter()
                    .any(|artifact| artifact.kind == VersionArtifactKind::ProposalData);
                let has_human_summary = artifacts
                    .iter()
                    .any(|artifact| artifact.kind == VersionArtifactKind::HumanSummary);
                let has_buffer_data = artifacts
                    .iter()
                    .any(|artifact| artifact.kind == VersionArtifactKind::BufferData);
                let has_hash_data = artifacts
                    .iter()
                    .any(|artifact| artifact.kind == VersionArtifactKind::HashData);

                StoredVersionEntry {
                    directory: self.build_version_directory(&version_id),
                    version_id,
                    artifacts,
                    has_snapshot,
                    has_report_bundle,
                    has_inference_data,
                    has_proposal_data,
                    has_human_summary,
                    has_buffer_data,
                    has_hash_data,
                }
            })
            .collect::<Vec<_>>();

        versions.sort_by(|left, right| {
            version_sort_key(&right.version_id).cmp(&version_sort_key(&left.version_id))
        });
        Ok(versions)
    }

    pub fn list_version_artifacts(&self, version_id: &str) -> AppResult<Vec<VersionArtifactEntry>> {
        let versions = self.list_versions()?;
        Ok(versions
            .into_iter()
            .find(|entry| entry.version_id == version_id)
            .map(|entry| entry.artifacts)
            .unwrap_or_default())
    }

    pub fn load_version_report(&self, version_id: &str) -> AppResult<Option<VersionDiffReportV2>> {
        let artifacts = self.list_version_artifacts(version_id)?;
        let mut candidates = artifacts
            .into_iter()
            .filter(|artifact| {
                matches!(
                    artifact.kind,
                    VersionArtifactKind::ReportBundle | VersionArtifactKind::LegacyReportBundle
                ) && artifact
                    .path
                    .extension()
                    .is_some_and(|value| value == "json")
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.path.cmp(&left.path));

        for artifact in candidates {
            if let Ok(report) = load_version_diff_report_v2(&artifact.path) {
                return Ok(Some(report));
            }
        }

        Ok(None)
    }

    pub fn load_latest_inference(&self, version_id: &str) -> AppResult<Option<InferenceReport>> {
        let mut candidates = self
            .list_version_artifacts(version_id)?
            .into_iter()
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::InferenceData
                    && artifact.path.extension().is_some_and(|ext| ext == "json")
            })
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));

        for path in candidates {
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(parsed) = serde_json::from_str::<InferenceReport>(&content)
            {
                return Ok(Some(parsed));
            }
        }

        Ok(None)
    }

    pub fn load_latest_mapping_proposal(
        &self,
        version_id: &str,
    ) -> AppResult<Option<MappingProposalOutput>> {
        let mut candidates = self
            .list_version_artifacts(version_id)?
            .into_iter()
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::ProposalData
                    && artifact.path.extension().is_some_and(|ext| ext == "json")
                    && artifact
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains("mapping-proposal"))
            })
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));

        for path in candidates {
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(parsed) = serde_json::from_str::<MappingProposalOutput>(&content)
            {
                return Ok(Some(parsed));
            }
        }

        Ok(None)
    }

    pub fn load_latest_patch_draft(
        &self,
        version_id: &str,
    ) -> AppResult<Option<ProposalPatchDraftOutput>> {
        let mut candidates = self
            .list_version_artifacts(version_id)?
            .into_iter()
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::ProposalData
                    && artifact.path.extension().is_some_and(|ext| ext == "json")
                    && artifact
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains("proposal-patch-draft"))
            })
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));

        for path in candidates {
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(parsed) = serde_json::from_str::<ProposalPatchDraftOutput>(&content)
            {
                return Ok(Some(parsed));
            }
        }

        Ok(None)
    }

    pub fn load_latest_human_summary(&self, version_id: &str) -> AppResult<Option<String>> {
        let mut candidates = self
            .list_version_artifacts(version_id)?
            .into_iter()
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::HumanSummary
                    && artifact.path.extension().is_some_and(|ext| ext == "md")
            })
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));

        for path in candidates {
            if let Ok(content) = fs::read_to_string(&path) {
                return Ok(Some(content));
            }
        }

        Ok(None)
    }

    pub fn select_baseline_version(&self, current_version: &str) -> AppResult<Option<String>> {
        let mut versions = self
            .list_versions()?
            .into_iter()
            .filter(|entry| entry.has_snapshot)
            .map(|entry| entry.version_id)
            .collect::<Vec<_>>();
        versions.sort_by(|left, right| version_sort_key(left).cmp(&version_sort_key(right)));

        if versions.is_empty() {
            return Ok(None);
        }

        if let Some(index) = versions
            .iter()
            .position(|version| version == current_version)
        {
            if index > 0 {
                return Ok(Some(versions[index - 1].clone()));
            }
            return Ok(None);
        }

        let target_key = version_sort_key(current_version);
        let baseline = versions
            .into_iter()
            .filter(|version| version_sort_key(version) < target_key)
            .next_back();
        Ok(baseline)
    }

    pub fn compare_versions(
        &self,
        old_version: &str,
        new_version: &str,
    ) -> AppResult<VersionDiffReportV2> {
        let old_snapshot = self.load_snapshot_by_version(old_version)?.ok_or_else(|| {
            AppError::InvalidInput(format!("snapshot for version {old_version} not found"))
        })?;
        let new_snapshot = self.load_snapshot_by_version(new_version)?.ok_or_else(|| {
            AppError::InvalidInput(format!("snapshot for version {new_version} not found"))
        })?;
        let compare = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        Ok(VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare))
    }

    pub fn list_reports(&self) -> AppResult<Vec<ReportListEntry>> {
        let mut report_paths = BTreeSet::<PathBuf>::new();

        for version in self.list_versions()? {
            for artifact in &version.artifacts {
                if matches!(
                    artifact.kind,
                    VersionArtifactKind::ReportBundle | VersionArtifactKind::LegacyReportBundle
                ) && artifact.path.extension().is_some_and(|ext| ext == "json")
                {
                    report_paths.insert(artifact.path.clone());
                }
            }
        }

        let mut reports = Vec::new();
        for path in report_paths {
            let report = load_version_diff_report_v2(&path)?;
            reports.push(ReportListEntry {
                path,
                old_version: report.old_version.version_id,
                new_version: report.new_version.version_id,
                generated_at_unix_ms: report.generated_at_unix_ms,
                resonators: report
                    .resonators
                    .iter()
                    .map(|entry| entry.resonator.clone())
                    .collect(),
            });
        }

        reports.sort_by(|left, right| {
            right
                .generated_at_unix_ms
                .cmp(&left.generated_at_unix_ms)
                .then_with(|| left.path.cmp(&right.path))
        });
        Ok(reports)
    }

    pub fn load_report(&self, path: &Path) -> AppResult<VersionDiffReportV2> {
        load_version_diff_report_v2(path)
    }

    fn legacy_snapshot_paths_for_version(&self, version_id: &str) -> Vec<PathBuf> {
        let mut paths = vec![self.legacy_root.join("snapshots").join(format!(
            "{VERSION_DIR_PREFIX}{}.json",
            sanitize_version_segment(version_id)
        ))];

        let underscored = version_id.replace('.', "_");
        let underscored_path = self.legacy_root.join("snapshots").join(format!(
            "{VERSION_DIR_PREFIX}{}.json",
            sanitize_version_segment(&underscored)
        ));
        if !paths.contains(&underscored_path) {
            paths.push(underscored_path);
        }

        paths
    }
}

fn collect_artifacts_from_new_layout(version_dir: &Path) -> AppResult<Vec<VersionArtifactEntry>> {
    let mut artifacts = Vec::new();
    if !version_dir.exists() {
        return Ok(artifacts);
    }

    for entry in fs::read_dir(version_dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() {
            artifacts.push(VersionArtifactEntry {
                kind: VersionArtifactKind::Auxiliary,
                label: format!("auxiliary | {}", path.display()),
                path,
            });
            continue;
        }

        if !entry.file_type()?.is_dir() {
            continue;
        }

        let section = entry.file_name().to_string_lossy().to_string();
        for file in collect_files_recursively(&path)? {
            let kind = match section.as_str() {
                "snapshot" => VersionArtifactKind::Snapshot,
                "report_bundle" => VersionArtifactKind::ReportBundle,
                "inference" => VersionArtifactKind::InferenceData,
                "proposal" => VersionArtifactKind::ProposalData,
                "summary" => VersionArtifactKind::HumanSummary,
                "buffer" => VersionArtifactKind::BufferData,
                "hash" => VersionArtifactKind::HashData,
                _ => VersionArtifactKind::Auxiliary,
            };
            artifacts.push(VersionArtifactEntry {
                kind,
                label: format!("{section} | {}", file.display()),
                path: file,
            });
        }
    }

    Ok(artifacts)
}

fn collect_files_recursively(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut collected = Vec::new();
    if !root.exists() {
        return Ok(collected);
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else if entry.file_type()?.is_file() {
                collected.push(path);
            }
        }
    }

    collected.sort();
    Ok(collected)
}

fn dedup_artifacts(artifacts: &mut Vec<VersionArtifactEntry>) {
    let mut seen = BTreeSet::<(VersionArtifactKind, PathBuf)>::new();
    artifacts.retain(|artifact| seen.insert((artifact.kind, artifact.path.clone())));
}

fn version_directory_name(version_id: &str) -> String {
    format!(
        "{VERSION_DIR_PREFIX}{}",
        sanitize_version_segment(version_id)
    )
}

fn parse_version_from_version_dir(name: &str) -> Option<String> {
    let suffix = name.strip_prefix(VERSION_DIR_PREFIX)?;
    (!suffix.trim().is_empty()).then(|| suffix.to_string())
}

fn parse_version_from_snapshot_file(name: &str) -> Option<String> {
    let name = name.strip_suffix(".json")?;
    let suffix = name.strip_prefix(VERSION_DIR_PREFIX)?;
    if suffix.trim().is_empty() {
        return None;
    }
    Some(restore_version_for_display(suffix))
}

fn restore_version_for_display(value: &str) -> String {
    if value.contains('.') {
        value.to_string()
    } else {
        value.replace('_', ".")
    }
}

fn sanitize_version_segment(value: &str) -> String {
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

fn version_sort_key(value: &str) -> Vec<(u8, String)> {
    value
        .split('.')
        .map(|part| match part.parse::<u64>() {
            Ok(number) => (0, format!("{number:020}")),
            Err(_) => (1, part.to_string()),
        })
        .collect()
}

#[allow(dead_code)]
fn _default_legacy_root_for_debug() -> PathBuf {
    resolve_artifact_root().join("reports")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        compare::SnapshotComparer,
        domain::AssetMetadata,
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceScopeContext,
            InferenceSummary, InferredMappingHint,
        },
        proposal::{ProposalArtifacts, ProposalEngine},
        report::{VersionDiffReportBuilder, VersionDiffSummary, VersionSide},
        snapshot::{
            GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
        },
    };

    use super::{ReportStorage, VersionArtifactKind, VersionDiffReportV2};

    #[test]
    fn save_snapshot_creates_version_folder_layout() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));
        let snapshot = sample_snapshot("7.1.0", 1);

        let path = storage
            .save_snapshot_for_version(&snapshot)
            .expect("save snapshot");

        let normalized = path.to_string_lossy().replace('\\', "/");
        assert!(normalized.contains("report/wuwa_7.1.0/snapshot"));
        assert!(
            storage
                .build_version_directory("7.1.0")
                .join("report_bundle")
                .exists()
        );
        assert!(
            storage
                .build_version_directory("7.1.0")
                .join("buffer")
                .exists()
        );
        assert!(
            storage
                .build_version_directory("7.1.0")
                .join("hash")
                .exists()
        );

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn list_versions_returns_entries_and_artifacts() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));
        storage
            .save_snapshot_for_version(&sample_snapshot("3.2.1", 1))
            .expect("save 3.2.1");
        storage
            .save_snapshot_for_version(&sample_snapshot("3.3.1", 2))
            .expect("save 3.3.1");
        fs::write(
            storage
                .build_version_directory("3.2.1")
                .join("hash")
                .join("hash-index.json"),
            "{}",
        )
        .expect("write hash artifact");

        let versions = storage.list_versions().expect("list versions");
        let version_ids = versions
            .iter()
            .map(|entry| entry.version_id.as_str())
            .collect::<Vec<_>>();
        assert!(version_ids.contains(&"3.2.1"));
        assert!(version_ids.contains(&"3.3.1"));

        let v321 = versions
            .iter()
            .find(|entry| entry.version_id == "3.2.1")
            .expect("3.2.1");
        assert!(v321.has_snapshot);
        assert!(v321.has_hash_data);
        assert!(
            v321.artifacts
                .iter()
                .any(|artifact| artifact.kind == VersionArtifactKind::Snapshot)
        );

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn compare_versions_uses_snapshots_from_storage() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));
        storage
            .save_snapshot_for_version(&sample_snapshot("3.2.5", 1))
            .expect("save 3.2.5");
        storage
            .save_snapshot_for_version(&sample_snapshot("3.3.1", 3))
            .expect("save 3.3.1");

        let report = storage
            .compare_versions("3.2.5", "3.3.1")
            .expect("compare from storage");

        assert_eq!(report.old_version.version_id, "3.2.5");
        assert_eq!(report.new_version.version_id, "3.3.1");
        assert!(report.summary.changed_items > 0);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn list_versions_includes_legacy_snapshot_layout() {
        let test_root = unique_test_dir();
        let root = test_root.join("out").join("report");
        let legacy_root = test_root.join("out").join("reports");
        let storage = ReportStorage::with_legacy_root(root, legacy_root.clone());
        fs::create_dir_all(legacy_root.join("snapshots")).expect("legacy snapshot dir");
        fs::write(
            legacy_root.join("snapshots").join("wuwa_8_1_0.json"),
            serde_json::to_string_pretty(&sample_snapshot("8.1.0", 1)).expect("serialize"),
        )
        .expect("write legacy snapshot");

        let versions = storage.list_versions().expect("list versions");
        assert!(versions.iter().any(|entry| entry.version_id == "8.1.0"));

        let loaded = storage
            .load_snapshot_by_version("8.1.0")
            .expect("load snapshot")
            .expect("snapshot exists");
        assert_eq!(loaded.version_id, "8.1.0");

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn list_reports_reads_legacy_report_bundle() {
        let test_root = unique_test_dir();
        let root = test_root.join("out").join("report");
        let legacy_root = test_root.join("out").join("reports");
        let storage = ReportStorage::with_legacy_root(root, legacy_root.clone());
        let legacy_bundle_dir = legacy_root.join("2_4_0-to-2_5_0-1");
        fs::create_dir_all(&legacy_bundle_dir).expect("create legacy bundle");
        fs::write(
            legacy_bundle_dir.join("report.v2.json"),
            serde_json::to_string_pretty(&sample_report()).expect("serialize report"),
        )
        .expect("write report");

        let reports = storage.list_reports().expect("list reports");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].old_version, "2.4.0");
        assert_eq!(reports[0].new_version, "2.5.0");

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn save_phase3_outputs_are_indexed_and_loadable() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));
        let old_snapshot = sample_snapshot("3.2.1", 1);
        let new_snapshot = sample_snapshot("3.3.1", 2);
        storage
            .save_snapshot_for_version(&old_snapshot)
            .expect("save old snapshot");
        storage
            .save_snapshot_for_version(&new_snapshot)
            .expect("save new snapshot");

        let compare = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let inference = sample_inference_report("3.2.1", "3.3.1");
        let proposals: ProposalArtifacts = ProposalEngine.generate(&inference, 0.85);
        let report = VersionDiffReportBuilder.enrich_with_inference(
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare),
            &inference,
        );

        storage
            .save_phase3_outputs(
                &report,
                &old_snapshot,
                &new_snapshot,
                &compare,
                &inference,
                &proposals,
                "# summary",
            )
            .expect("save phase3 outputs");

        let versions = storage.list_versions().expect("list versions");
        let v331 = versions
            .iter()
            .find(|entry| entry.version_id == "3.3.1")
            .expect("3.3.1 exists");
        assert!(v331.has_inference_data);
        assert!(v331.has_proposal_data);
        assert!(v331.has_human_summary);

        let latest_inference = storage
            .load_latest_inference("3.3.1")
            .expect("load inference")
            .expect("inference exists");
        assert_eq!(latest_inference.compare_input.new_version_id, "3.3.1");
        assert!(
            storage
                .load_latest_mapping_proposal("3.3.1")
                .expect("load mapping proposal")
                .is_some()
        );
        assert!(
            storage
                .load_latest_patch_draft("3.3.1")
                .expect("load patch draft")
                .is_some()
        );
        assert_eq!(
            storage
                .load_latest_human_summary("3.3.1")
                .expect("load human summary")
                .as_deref(),
            Some("# summary")
        );

        let _ = fs::remove_dir_all(test_root);
    }

    fn sample_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
        GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.to_string(),
            created_at_unix_ms: 1,
            source_root: "root".to_string(),
            asset_count: 1,
            assets: vec![SnapshotAsset {
                id: "asset-1".to_string(),
                path: "Content/Character/Encore/Body.mesh".to_string(),
                kind: Some("mesh".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some("body".to_string()),
                    vertex_count: Some(vertex_count),
                    index_count: Some(1),
                    material_slots: Some(1),
                    section_count: Some(1),
                    tags: Vec::new(),
                },
                fingerprint: SnapshotFingerprint {
                    normalized_kind: Some("mesh".to_string()),
                    normalized_name: Some("body".to_string()),
                    name_tokens: vec!["body".to_string()],
                    path_tokens: vec!["content".to_string()],
                    tags: Vec::new(),
                    vertex_count: Some(vertex_count),
                    index_count: Some(1),
                    material_slots: Some(1),
                    section_count: Some(1),
                },
                hash_fields: SnapshotHashFields::default(),
            }],
            context: SnapshotContext::default(),
        }
    }

    fn sample_report() -> VersionDiffReportV2 {
        VersionDiffReportV2 {
            schema_version: "whashreonator.report.v2".to_string(),
            generated_at_unix_ms: 1,
            old_version: VersionSide {
                version_id: "2.4.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_version: VersionSide {
                version_id: "2.5.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            resonators: Vec::new(),
            summary: VersionDiffSummary {
                resonator_count: 0,
                unchanged_items: 0,
                changed_items: 0,
                added_items: 0,
                removed_items: 0,
                uncertain_items: 0,
                mapping_candidates: 0,
            },
            scope_notes: Vec::new(),
        }
    }

    fn sample_inference_report(old_version: &str, new_version: &str) -> InferenceReport {
        InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 123,
            compare_input: InferenceCompareInput {
                old_version_id: old_version.to_string(),
                new_version_id: new_version.to_string(),
                changed_assets: 1,
                added_assets: 0,
                removed_assets: 0,
                candidate_mapping_changes: 1,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 1,
                fix_like_commits: 1,
                discovered_patterns: 1,
            },
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 0,
                suggested_fixes: 0,
                candidate_mapping_hints: 1,
                highest_confidence: 0.91,
            },
            probable_crash_causes: Vec::new(),
            suggested_fixes: Vec::new(),
            candidate_mapping_hints: vec![InferredMappingHint {
                old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                new_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                confidence: 0.91,
                needs_review: false,
                ambiguous: false,
                confidence_gap: Some(0.2),
                reasons: vec!["exact path".to_string()],
                evidence: vec!["compare confidence".to_string()],
            }],
        }
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();

        std::env::temp_dir().join(format!("whashreonator-report-storage-test-{nanos}"))
    }
}
