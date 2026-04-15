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
        export_mod_dependency_baseline_set_output, export_mod_dependency_profile_output,
        export_proposal_patch_draft_output, export_snapshot_compare_output, export_snapshot_output,
        export_text_output, export_version_continuity_output, export_version_diff_report_v2,
    },
    human_summary::ReviewBundleRenderer,
    inference::InferenceReport,
    output_policy::{resolve_artifact_root, resolve_report_store_root},
    proposal::{MappingProposalOutput, ProposalArtifacts, ProposalPatchDraftOutput},
    report::{
        VersionContinuityArtifact, VersionContinuityIndex, VersionDiffReportBuilder,
        VersionDiffReportV2, VersionLineageSection, load_version_continuity_artifact,
        load_version_diff_report_v2,
    },
    snapshot::{GameSnapshot, SnapshotEvidencePosture, load_snapshot, snapshot_evidence_posture},
    wwmi::dependency::{
        WwmiModDependencyBaselineSet, WwmiModDependencyProfile, load_mod_dependency_baseline_set,
    },
};

const VERSION_DIR_PREFIX: &str = "wuwa_";

#[derive(Debug, Clone)]
pub struct SavedReportBundle {
    pub directory: PathBuf,
    pub report_path: PathBuf,
    pub review_markdown_path: Option<PathBuf>,
    pub continuity_path: Option<PathBuf>,
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
    ContinuityData,
    InferenceData,
    ProposalData,
    HumanSummary,
    ExtractorInventory,
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
    pub has_continuity_data: bool,
    pub has_inference_data: bool,
    pub has_proposal_data: bool,
    pub has_human_summary: bool,
    pub has_buffer_data: bool,
    pub has_hash_data: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSnapshotBaseline {
    pub version_id: String,
    pub path: PathBuf,
    pub artifact_kind: VersionArtifactKind,
    pub artifact_label: String,
    pub evidence_posture: String,
    pub inventory_alignment: String,
    pub selection_reason: String,
    pub snapshot: GameSnapshot,
}

#[derive(Debug, Clone)]
pub struct VersionLayoutPaths {
    pub version_dir: PathBuf,
    pub snapshot_dir: PathBuf,
    pub report_bundle_dir: PathBuf,
    pub continuity_dir: PathBuf,
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
        let continuity_dir = version_dir.join("continuity");
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
            continuity_dir,
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
        fs::create_dir_all(&layout.continuity_dir)?;
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

        let continuity_path = self.build_and_save_version_continuity_artifact()?;

        Ok(SavedReportBundle {
            directory: run_dir,
            report_path,
            review_markdown_path: None,
            continuity_path,
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
        let bundled_report = VersionDiffReportBuilder.enrich_with_review_surface(
            report.clone(),
            inference,
            &proposals.mapping_proposal,
        );
        let saved = self.save_run(
            &bundled_report,
            old_snapshot,
            new_snapshot,
            compare,
            Some(inference),
        )?;
        let review_markdown_path = saved.directory.join("review.md");
        export_text_output(
            &ReviewBundleRenderer.render(&bundled_report),
            &review_markdown_path,
        )?;
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

        Ok(SavedReportBundle {
            review_markdown_path: Some(review_markdown_path),
            ..saved
        })
    }

    pub fn snapshot_path_for_version(&self, version_id: &str) -> PathBuf {
        self.build_version_layout(version_id).snapshot_path
    }

    pub fn save_snapshot_for_version(&self, snapshot: &GameSnapshot) -> AppResult<PathBuf> {
        let layout = self.ensure_version_layout(&snapshot.version_id)?;
        export_snapshot_output(snapshot, &layout.snapshot_path)?;
        Ok(layout.snapshot_path)
    }

    pub fn save_mod_dependency_profile_for_version(
        &self,
        version_id: &str,
        profile: &WwmiModDependencyProfile,
    ) -> AppResult<PathBuf> {
        let layout = self.ensure_version_layout(version_id)?;
        let profile_name = sanitize_version_segment(
            profile
                .mod_name
                .as_deref()
                .unwrap_or(profile.mod_root.as_str()),
        );
        let target_path = layout.auxiliary_dir.join(format!(
            "{VERSION_DIR_PREFIX}{}.{}.mod-dependency-profile.v1.json",
            sanitize_version_segment(version_id),
            profile_name
        ));
        export_mod_dependency_profile_output(profile, &target_path)?;
        Ok(target_path)
    }

    pub fn save_mod_dependency_baseline_set_for_version(
        &self,
        version_id: &str,
        baseline_set: &WwmiModDependencyBaselineSet,
    ) -> AppResult<PathBuf> {
        let layout = self.ensure_version_layout(version_id)?;
        let target_path = layout.auxiliary_dir.join(format!(
            "{VERSION_DIR_PREFIX}{}.mod-dependency-baselines.v1.json",
            sanitize_version_segment(version_id)
        ));
        export_mod_dependency_baseline_set_output(baseline_set, &target_path)?;
        Ok(target_path)
    }

    pub fn save_extractor_inventory_input_for_version(
        &self,
        version_id: &str,
        inventory_path: &Path,
    ) -> AppResult<PathBuf> {
        if !inventory_path.exists() || !inventory_path.is_file() {
            return Err(AppError::InvalidInput(format!(
                "extractor inventory input does not exist or is not a file: {}",
                inventory_path.display()
            )));
        }

        let layout = self.ensure_version_layout(version_id)?;
        let target_path = layout.auxiliary_dir.join(format!(
            "{VERSION_DIR_PREFIX}{}.extractor-inventory.v1.json",
            sanitize_version_segment(version_id)
        ));

        if inventory_path != target_path {
            fs::copy(inventory_path, &target_path)?;
        }

        Ok(target_path)
    }

    pub fn save_prepared_inventory_input_for_version(
        &self,
        version_id: &str,
        inventory_path: &Path,
    ) -> AppResult<PathBuf> {
        self.save_extractor_inventory_input_for_version(version_id, inventory_path)
    }

    pub fn save_version_continuity_artifact(
        &self,
        artifact: &VersionContinuityArtifact,
    ) -> AppResult<PathBuf> {
        let latest_version_id = artifact.latest_version_id.as_deref().ok_or_else(|| {
            AppError::InvalidInput(
                "continuity artifact must declare latest_version_id before it can be stored"
                    .to_string(),
            )
        })?;
        let layout = self.ensure_version_layout(latest_version_id)?;
        let target_path = layout.continuity_dir.join(format!(
            "{:020}-{VERSION_DIR_PREFIX}{}.continuity.v1.json",
            artifact.generated_at_unix_ms,
            sanitize_version_segment(latest_version_id)
        ));
        export_version_continuity_output(artifact, &target_path)?;
        Ok(target_path)
    }

    pub fn build_and_save_version_continuity_artifact(&self) -> AppResult<Option<PathBuf>> {
        let reports = self.collect_continuity_source_reports()?;
        if reports.is_empty() {
            return Ok(None);
        }

        let artifact = VersionContinuityArtifact::from_reports(&reports);
        Ok(Some(self.save_version_continuity_artifact(&artifact)?))
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
        Ok(self
            .select_snapshot_baseline_for_version(version_id)?
            .map(|baseline| baseline.snapshot))
    }

    pub fn select_snapshot_baseline_for_version(
        &self,
        version_id: &str,
    ) -> AppResult<Option<SelectedSnapshotBaseline>> {
        let mut candidates = self.snapshot_baseline_candidates_for_version(version_id)?;
        candidates.sort_by(|left, right| right.sort_key.cmp(&left.sort_key));
        Ok(candidates
            .into_iter()
            .next()
            .map(|candidate| candidate.selected))
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
                let has_continuity_data = artifacts
                    .iter()
                    .any(|artifact| artifact.kind == VersionArtifactKind::ContinuityData);
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
                    has_continuity_data,
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

    pub fn load_lineage_for_pair(
        &self,
        old_version: &str,
        new_version: &str,
    ) -> AppResult<Option<VersionLineageSection>> {
        let mut candidates = self
            .list_version_artifacts(new_version)?
            .into_iter()
            .filter(|artifact| {
                matches!(
                    artifact.kind,
                    VersionArtifactKind::ReportBundle | VersionArtifactKind::LegacyReportBundle
                ) && artifact
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == "report.v2.json")
            })
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));

        for path in candidates {
            if let Ok(report) = load_version_diff_report_v2(&path)
                && report.old_version.version_id == old_version
                && report.new_version.version_id == new_version
            {
                return Ok(Some(report.lineage));
            }
        }

        Ok(None)
    }

    pub fn load_latest_continuity_artifact(&self) -> AppResult<Option<VersionContinuityArtifact>> {
        let candidates = self
            .list_versions()?
            .into_iter()
            .flat_map(|entry| entry.artifacts)
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::ContinuityData
                    && artifact.path.extension().is_some_and(|ext| ext == "json")
            })
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();

        let mut latest: Option<(VersionContinuityArtifact, PathBuf)> = None;
        for path in candidates {
            let Ok(parsed) = load_version_continuity_artifact(&path) else {
                continue;
            };

            let replace = match latest.as_ref() {
                Some((current, current_path)) => {
                    parsed.generated_at_unix_ms > current.generated_at_unix_ms
                        || (parsed.generated_at_unix_ms == current.generated_at_unix_ms
                            && path > *current_path)
                }
                None => true,
            };

            if replace {
                latest = Some((parsed, path));
            }
        }

        Ok(latest.map(|(artifact, _)| artifact))
    }

    pub fn load_version_continuity_index(&self) -> AppResult<VersionContinuityIndex> {
        if let Some(artifact) = self.load_latest_continuity_artifact()? {
            return Ok(artifact.continuity);
        }

        let reports = self.collect_continuity_source_reports()?;
        Ok(VersionContinuityIndex::from_reports(&reports))
    }

    pub fn load_version_continuity_index_for_pair(
        &self,
        old_version: &str,
        new_version: &str,
    ) -> AppResult<Option<VersionContinuityIndex>> {
        if let Some(artifact) = self.load_latest_continuity_artifact()?
            && continuity_index_contains_pair(&artifact.continuity, old_version, new_version)
        {
            return Ok(Some(artifact.continuity));
        }

        let reports = self.collect_continuity_source_reports()?;
        if reports.is_empty() {
            return Ok(None);
        }

        let index = VersionContinuityIndex::from_reports(&reports);
        if continuity_index_contains_pair(&index, old_version, new_version) {
            Ok(Some(index))
        } else {
            Ok(None)
        }
    }

    fn collect_continuity_source_reports(&self) -> AppResult<Vec<VersionDiffReportV2>> {
        let mut by_pair = BTreeMap::<(String, String), VersionDiffReportV2>::new();

        for entry in self.list_reports()? {
            if let Ok(report) = self.load_report(&entry.path) {
                let key = (
                    report.old_version.version_id.clone(),
                    report.new_version.version_id.clone(),
                );
                by_pair.entry(key).or_insert(report);
            }
        }

        let mut reports = by_pair.into_values().collect::<Vec<_>>();
        reports.sort_by(|left, right| {
            version_sort_key(&left.old_version.version_id)
                .cmp(&version_sort_key(&right.old_version.version_id))
                .then_with(|| {
                    version_sort_key(&left.new_version.version_id)
                        .cmp(&version_sort_key(&right.new_version.version_id))
                })
                .then_with(|| left.generated_at_unix_ms.cmp(&right.generated_at_unix_ms))
        });

        Ok(reports)
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

    pub fn load_latest_mod_dependency_baseline_set(
        &self,
        version_id: &str,
    ) -> AppResult<Option<WwmiModDependencyBaselineSet>> {
        let mut candidates = self
            .list_version_artifacts(version_id)?
            .into_iter()
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::Auxiliary
                    && artifact.path.extension().is_some_and(|ext| ext == "json")
                    && artifact
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains("mod-dependency-baselines"))
            })
            .map(|artifact| artifact.path)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));

        for path in candidates {
            if let Ok(parsed) = load_mod_dependency_baseline_set(&path) {
                return Ok(Some(parsed));
            }
        }

        Ok(None)
    }

    pub fn load_latest_extractor_inventory_input(
        &self,
        version_id: &str,
    ) -> AppResult<Option<String>> {
        let mut candidates = self
            .list_version_artifacts(version_id)?
            .into_iter()
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::ExtractorInventory
                    && artifact.path.extension().is_some_and(|ext| ext == "json")
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
        let old_baseline = self
            .select_snapshot_baseline_for_version(old_version)?
            .ok_or_else(|| {
                AppError::InvalidInput(format!("snapshot for version {old_version} not found"))
            })?;
        let new_baseline = self
            .select_snapshot_baseline_for_version(new_version)?
            .ok_or_else(|| {
                AppError::InvalidInput(format!("snapshot for version {new_version} not found"))
            })?;
        let old_snapshot = old_baseline.snapshot.clone();
        let new_snapshot = new_baseline.snapshot.clone();
        let compare = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let mut report =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare);
        report.scope_notes.push(format!(
            "selected baseline {}: posture={} alignment={} source={} artifact_kind={:?}; {}",
            old_version,
            old_baseline.evidence_posture,
            old_baseline.inventory_alignment,
            old_baseline.path.display(),
            old_baseline.artifact_kind,
            old_baseline.selection_reason
        ));
        report.scope_notes.push(format!(
            "selected baseline {}: posture={} alignment={} source={} artifact_kind={:?}; {}",
            new_version,
            new_baseline.evidence_posture,
            new_baseline.inventory_alignment,
            new_baseline.path.display(),
            new_baseline.artifact_kind,
            new_baseline.selection_reason
        ));
        Ok(report)
    }

    pub fn list_reports(&self) -> AppResult<Vec<ReportListEntry>> {
        let mut report_paths = BTreeSet::<PathBuf>::new();

        for version in self.list_versions()? {
            for artifact in &version.artifacts {
                if matches!(
                    artifact.kind,
                    VersionArtifactKind::ReportBundle | VersionArtifactKind::LegacyReportBundle
                ) && artifact
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == "report.v2.json")
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

    fn snapshot_baseline_candidates_for_version(
        &self,
        version_id: &str,
    ) -> AppResult<Vec<SnapshotBaselineCandidate>> {
        let mut candidates = Vec::new();
        let mut artifacts = self
            .list_version_artifacts(version_id)?
            .into_iter()
            .filter(|artifact| match artifact.kind {
                VersionArtifactKind::Snapshot | VersionArtifactKind::LegacySnapshot => true,
                VersionArtifactKind::ReportBundle => artifact
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".snapshot.json")),
                _ => false,
            })
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.path.cmp(&right.path));

        for artifact in artifacts {
            let Ok(snapshot) = load_snapshot(&artifact.path) else {
                continue;
            };
            if snapshot.version_id != version_id {
                continue;
            }
            let (posture, scope, quality) = snapshot_evidence_posture(&snapshot);
            let rank = baseline_preference_rank(posture, &scope, &quality);
            candidates.push(SnapshotBaselineCandidate {
                sort_key: (
                    rank,
                    quality.launcher_version_matches_snapshot == Some(true),
                    quality.manifest_matched_assets,
                    quality.meaningfully_enriched_assets,
                    quality.assets_with_source_context,
                    quality.assets_with_rich_metadata,
                    snapshot.asset_count,
                    artifact.path == self.snapshot_path_for_version(version_id),
                    snapshot.created_at_unix_ms,
                    artifact.path.clone(),
                ),
                selected: SelectedSnapshotBaseline {
                    version_id: version_id.to_string(),
                    path: artifact.path.clone(),
                    artifact_kind: artifact.kind,
                    artifact_label: artifact.label.clone(),
                    evidence_posture: posture.machine_label().to_string(),
                    inventory_alignment: quality.extractor_inventory_alignment_status().to_string(),
                    selection_reason: baseline_preference_reason(
                        posture, &scope, &quality, &artifact,
                    ),
                    snapshot,
                },
            });
        }

        for legacy_path in self.legacy_snapshot_paths_for_version(version_id) {
            if !legacy_path.exists()
                || candidates
                    .iter()
                    .any(|candidate| candidate.selected.path == legacy_path)
            {
                continue;
            }
            let Ok(snapshot) = load_snapshot(&legacy_path) else {
                continue;
            };
            if snapshot.version_id != version_id {
                continue;
            }
            let (posture, scope, quality) = snapshot_evidence_posture(&snapshot);
            let rank = baseline_preference_rank(posture, &scope, &quality);
            candidates.push(SnapshotBaselineCandidate {
                sort_key: (
                    rank,
                    quality.launcher_version_matches_snapshot == Some(true),
                    quality.manifest_matched_assets,
                    quality.meaningfully_enriched_assets,
                    quality.assets_with_source_context,
                    quality.assets_with_rich_metadata,
                    snapshot.asset_count,
                    false,
                    snapshot.created_at_unix_ms,
                    legacy_path.clone(),
                ),
                selected: SelectedSnapshotBaseline {
                    version_id: version_id.to_string(),
                    path: legacy_path.clone(),
                    artifact_kind: VersionArtifactKind::LegacySnapshot,
                    artifact_label: format!("legacy snapshot | {}", legacy_path.display()),
                    evidence_posture: posture.machine_label().to_string(),
                    inventory_alignment: quality.extractor_inventory_alignment_status().to_string(),
                    selection_reason: baseline_preference_reason(
                        posture,
                        &scope,
                        &quality,
                        &VersionArtifactEntry {
                            kind: VersionArtifactKind::LegacySnapshot,
                            path: legacy_path.clone(),
                            label: format!("legacy snapshot | {}", legacy_path.display()),
                        },
                    ),
                    snapshot,
                },
            });
        }

        Ok(candidates)
    }
}

#[derive(Debug, Clone)]
struct SnapshotBaselineCandidate {
    sort_key: (u8, bool, usize, usize, usize, usize, usize, bool, u128, PathBuf),
    selected: SelectedSnapshotBaseline,
}

fn baseline_preference_rank(
    posture: SnapshotEvidencePosture,
    scope: &crate::snapshot::SnapshotScopeAssessment,
    quality: &crate::snapshot::SnapshotCaptureQualitySummary,
) -> u8 {
    match posture {
        SnapshotEvidencePosture::ExtractorBackedRich => 6,
        SnapshotEvidencePosture::ShallowSupportOnly
            if quality.launcher_version_matches_snapshot == Some(true)
                || quality.manifest_matched_assets > 0 =>
        {
            5
        }
        SnapshotEvidencePosture::MixedOrPartial => 4,
        SnapshotEvidencePosture::ShallowSupportOnly => 3,
        SnapshotEvidencePosture::ExtractorBackedPartial
            if scope.meaningful_content_coverage
                && scope.meaningful_character_coverage
                && quality.has_version_aligned_extractor_inventory() =>
        {
            3
        }
        SnapshotEvidencePosture::ExtractorBackedAlignmentUnverified => 2,
        SnapshotEvidencePosture::ExtractorBackedPartial => 2,
        SnapshotEvidencePosture::MixedOrLowSignal => 1,
        SnapshotEvidencePosture::ExtractorBackedMisaligned => 0,
    }
}

fn baseline_preference_reason(
    posture: SnapshotEvidencePosture,
    scope: &crate::snapshot::SnapshotScopeAssessment,
    quality: &crate::snapshot::SnapshotCaptureQualitySummary,
    artifact: &VersionArtifactEntry,
) -> String {
    match posture {
        SnapshotEvidencePosture::ExtractorBackedRich => format!(
            "preferred this baseline because it is extractor-backed, version-aligned, and meaningfully enriched; artifact={}",
            artifact.label
        ),
        SnapshotEvidencePosture::ShallowSupportOnly
            if quality.launcher_version_matches_snapshot == Some(true)
                || quality.manifest_matched_assets > 0 =>
        {
            format!(
                "preferred this baseline as the safer stored fallback because launcher/manifest evidence is present and no stronger trustworthy extractor-backed baseline outranked it; artifact={}",
                artifact.label
            )
        }
        SnapshotEvidencePosture::ExtractorBackedMisaligned => format!(
            "this baseline remains available only as a last resort because extractor version evidence is mismatched; artifact={}",
            artifact.label
        ),
        SnapshotEvidencePosture::ExtractorBackedAlignmentUnverified => format!(
            "kept this extractor-backed baseline review-first because version alignment is {}; artifact={}",
            quality.extractor_inventory_alignment_status(),
            artifact.label
        ),
        SnapshotEvidencePosture::ExtractorBackedPartial => format!(
            "kept this extractor-backed baseline below stronger safer candidates because asset-level enrichment remains partial (content={} character={} enrichment={}); artifact={}",
            scope.meaningful_content_coverage,
            scope.meaningful_character_coverage,
            scope.meaningful_asset_record_enrichment,
            artifact.label
        ),
        SnapshotEvidencePosture::MixedOrPartial | SnapshotEvidencePosture::MixedOrLowSignal => {
            format!(
                "selected the best remaining baseline from stored candidates with conservative review-first posture; artifact={}",
                artifact.label
            )
        }
        SnapshotEvidencePosture::ShallowSupportOnly => format!(
            "selected a shallow baseline because it was the best remaining stored candidate, but it still stays low-signal for deeper analysis; artifact={}",
            artifact.label
        ),
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
                "continuity" => VersionArtifactKind::ContinuityData,
                "inference" => VersionArtifactKind::InferenceData,
                "proposal" => VersionArtifactKind::ProposalData,
                "summary" => VersionArtifactKind::HumanSummary,
                "buffer" => VersionArtifactKind::BufferData,
                "hash" => VersionArtifactKind::HashData,
                "auxiliary"
                    if file
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains("extractor-inventory")) =>
                {
                    VersionArtifactKind::ExtractorInventory
                }
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

fn continuity_index_contains_pair(
    index: &VersionContinuityIndex,
    old_version: &str,
    new_version: &str,
) -> bool {
    index.threads.iter().any(|thread| {
        thread.observations.iter().any(|observation| {
            observation.from_version_id == old_version && observation.to_version_id == new_version
        })
    })
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
        compare::{RiskLevel, SnapshotComparer},
        domain::{AssetInternalStructure, AssetMetadata},
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceScopeContext,
            InferenceSummary, InferredMappingContinuityContext, InferredMappingHint,
            ProbableCrashCause, SuggestedFix,
        },
        proposal::{ProposalArtifacts, ProposalEngine},
        report::{
            VersionContinuityArtifact, VersionContinuityIndex, VersionContinuityRelation,
            VersionContinuitySummary, VersionDiffReportBuilder, VersionDiffSummary, VersionSide,
        },
        snapshot::{
            GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
        },
        wwmi::WwmiPatternKind,
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
                .join("continuity")
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
        assert!(!v321.has_continuity_data);
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

        let saved = storage
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
        assert!(v331.has_continuity_data);
        assert!(v331.has_inference_data);
        assert!(v331.has_proposal_data);
        assert!(v331.has_human_summary);
        let review_markdown_path = saved
            .review_markdown_path
            .as_ref()
            .expect("review markdown path");
        assert!(review_markdown_path.exists());
        assert_eq!(
            review_markdown_path.parent(),
            Some(saved.directory.as_path())
        );
        assert!(v331.artifacts.iter().any(|artifact| {
            artifact.kind == VersionArtifactKind::ReportBundle
                && artifact.path == *review_markdown_path
        }));

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
        let lineage = storage
            .load_lineage_for_pair("3.2.1", "3.3.1")
            .expect("load lineage")
            .expect("lineage exists");
        assert_eq!(lineage.summary.total_entries, 1);
        assert_eq!(lineage.summary.layout_drift_assets, 1);
        assert_eq!(
            lineage.entries[0].lineage,
            crate::compare::AssetLineageKind::LayoutDrift
        );
        let reports = storage.list_reports().expect("list reports");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].old_version, "3.2.1");
        assert_eq!(reports[0].new_version, "3.3.1");
        assert!(
            v331.artifacts
                .iter()
                .any(|artifact| artifact.kind == VersionArtifactKind::ContinuityData)
        );

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn save_phase3_outputs_persists_continuity_review_surface_in_saved_report_bundle() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));
        let old_snapshot = sample_snapshot("8.0.0", 100);
        let mut new_snapshot = sample_snapshot("8.1.0", 100);
        new_snapshot.assets[0].path = "Content/Character/Encore/Body_v2.mesh".to_string();
        new_snapshot.assets[0].id = "asset-2".to_string();

        storage
            .save_snapshot_for_version(&old_snapshot)
            .expect("save old snapshot");
        storage
            .save_snapshot_for_version(&new_snapshot)
            .expect("save new snapshot");

        let compare = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let inference = sample_continuity_review_inference_report("8.0.0", "8.1.0");
        let proposals: ProposalArtifacts = ProposalEngine.generate(&inference, 0.85);
        let report = VersionDiffReportBuilder.enrich_with_inference(
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare),
            &inference,
        );

        let saved = storage
            .save_phase3_outputs(
                &report,
                &old_snapshot,
                &new_snapshot,
                &compare,
                &inference,
                &proposals,
                "# continuity summary",
            )
            .expect("save phase3 outputs");

        let saved_report = storage
            .load_report(&saved.report_path)
            .expect("load saved report bundle report");
        let review_markdown = fs::read_to_string(
            saved
                .review_markdown_path
                .as_ref()
                .expect("review markdown path"),
        )
        .expect("read review markdown");

        assert!(saved_report.review.summary.continuity_caution_present);
        assert_eq!(
            saved_report.review.summary.continuity_review_mapping_count,
            1
        );
        assert_eq!(saved_report.review.continuity.cause_count, 1);
        assert_eq!(saved_report.review.continuity.fix_count, 1);
        assert_eq!(saved_report.review.continuity.review_mapping_count, 1);
        assert!(
            saved_report
                .review
                .continuity
                .notes
                .iter()
                .any(|note| note.contains("continuity-backed caution present"))
        );
        let mapping = saved_report
            .review
            .continuity
            .mappings
            .first()
            .expect("continuity review mapping");
        assert_eq!(mapping.old_asset_path, "Content/Character/Encore/Body.mesh");
        assert_eq!(
            mapping.new_asset_path,
            "Content/Character/Encore/Body_v2.mesh"
        );
        assert!(
            mapping
                .continuity_notes
                .iter()
                .any(|note| note.contains("thread span 7.0.0 -> 8.2.0"))
        );
        assert!(
            mapping
                .continuity_notes
                .iter()
                .any(|note| note.contains("later terminal removed in 8.2.0"))
        );
        assert!(
            mapping
                .related_fix_codes
                .iter()
                .any(|code| code == "review_continuity_thread_history_before_repair")
        );
        assert!(review_markdown.contains("# WhashReonator Review Bundle"));
        assert!(review_markdown.contains("| `8.0.0` | `8.1.0` | Yes | 1 | 1 | 1 | 1 |"));
        assert!(review_markdown.contains("continuity_thread_instability"));
        assert!(review_markdown.contains("review_continuity_thread_history_before_repair"));
        assert!(review_markdown.contains("Content/Character/Encore/Body.mesh"));
        assert!(review_markdown.contains("thread span 7.0.0 -> 8.2.0"));
        assert!(review_markdown.contains("later terminal removed in 8.2.0"));

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn save_phase3_outputs_keeps_saved_report_review_surface_empty_without_continuity_caution() {
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

        let saved = storage
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

        let saved_report = storage
            .load_report(&saved.report_path)
            .expect("load saved report bundle report");
        let review_markdown = fs::read_to_string(
            saved
                .review_markdown_path
                .as_ref()
                .expect("review markdown path"),
        )
        .expect("read review markdown");

        assert!(!saved_report.review.summary.continuity_caution_present);
        assert_eq!(saved_report.review.summary.review_mapping_count, 0);
        assert_eq!(
            saved_report.review.summary.continuity_review_mapping_count,
            0
        );
        assert!(!saved_report.review.continuity.caution_present);
        assert!(saved_report.review.continuity.causes.is_empty());
        assert!(saved_report.review.continuity.fixes.is_empty());
        assert!(saved_report.review.continuity.mappings.is_empty());
        assert!(review_markdown.contains("# WhashReonator Review Bundle"));
        assert!(review_markdown.contains("| `3.2.1` | `3.3.1` | No | 0 | 0 | 0 | 0 |"));
        assert!(review_markdown.contains("Continuity caution is not present"));
        assert!(review_markdown.contains("No continuity-backed causes were saved in this bundle."));
        assert!(review_markdown.contains("No continuity-backed fixes were saved in this bundle."));
        assert!(
            review_markdown.contains(
                "No mapping stays in `NeedsReview` because of broader continuity history."
            )
        );

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn save_run_persists_continuity_artifact_and_loads_it_back() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));

        let old_snapshot = continuity_snapshot(
            "4.0.0",
            continuity_asset(
                "Content/Character/Encore/Body.mesh",
                "body",
                "sig-body",
                &["base", "trim"],
                &["position", "normal"],
            ),
        );
        let mid_snapshot = continuity_snapshot(
            "4.1.0",
            continuity_asset(
                "Content/Character/Encore/Body_LOD0.mesh",
                "body",
                "sig-body",
                &["base", "trim"],
                &["position", "normal"],
            ),
        );
        let new_snapshot = continuity_snapshot(
            "4.2.0",
            continuity_asset(
                "Content/Character/Encore/Body_LOD0.mesh",
                "body",
                "sig-body",
                &["base", "trim", "accessory"],
                &["position", "normal", "tangent"],
            ),
        );

        let compare_ab = SnapshotComparer.compare(&old_snapshot, &mid_snapshot);
        let report_ab =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &mid_snapshot, &compare_ab);
        let saved_ab = storage
            .save_run(&report_ab, &old_snapshot, &mid_snapshot, &compare_ab, None)
            .expect("save ab");
        assert!(
            saved_ab
                .continuity_path
                .as_ref()
                .is_some_and(|path| path.exists())
        );

        let compare_bc = SnapshotComparer.compare(&mid_snapshot, &new_snapshot);
        let report_bc =
            VersionDiffReportBuilder.from_compare(&mid_snapshot, &new_snapshot, &compare_bc);
        let saved_bc = storage
            .save_run(&report_bc, &mid_snapshot, &new_snapshot, &compare_bc, None)
            .expect("save bc");
        assert!(
            saved_bc
                .continuity_path
                .as_ref()
                .is_some_and(|path| path.exists())
        );

        let latest = storage
            .load_latest_continuity_artifact()
            .expect("load latest continuity")
            .expect("continuity exists");
        assert_eq!(latest.schema_version, "whashreonator.continuity.v1");
        assert_eq!(latest.latest_version_id.as_deref(), Some("4.2.0"));
        assert_eq!(latest.report_count, 2);
        assert_eq!(latest.continuity.summary.thread_count, 1);

        let thread_summary = latest
            .continuity
            .thread_summaries
            .iter()
            .find(|summary| {
                summary.anchor.path.as_deref() == Some("Content/Character/Encore/Body.mesh")
            })
            .expect("thread summary");
        assert_eq!(
            thread_summary
                .first_rename_or_repath_step
                .as_ref()
                .map(|step| (step.from_version_id.as_str(), step.to_version_id.as_str())),
            Some(("4.0.0", "4.1.0"))
        );
        assert_eq!(
            thread_summary
                .first_layout_drift_step
                .as_ref()
                .map(|step| (step.from_version_id.as_str(), step.to_version_id.as_str())),
            Some(("4.1.0", "4.2.0"))
        );

        let stored_index = storage
            .load_version_continuity_index()
            .expect("load continuity index");
        assert_eq!(
            stored_index.thread_summaries,
            latest.continuity.thread_summaries
        );

        let latest_artifacts = storage
            .list_version_artifacts("4.2.0")
            .expect("list latest version artifacts");
        assert!(latest_artifacts.iter().any(|artifact| {
            artifact.kind == VersionArtifactKind::ContinuityData
                && artifact
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(".continuity.v1.json"))
        }));

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn load_latest_continuity_artifact_prefers_newest_saved_timestamp() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));

        let older = sample_continuity_artifact("4.1.0", 10, 1);
        let newer = sample_continuity_artifact("4.2.0", 20, 2);
        let older_path = storage
            .save_version_continuity_artifact(&older)
            .expect("save older continuity");
        let newer_path = storage
            .save_version_continuity_artifact(&newer)
            .expect("save newer continuity");

        let latest = storage
            .load_latest_continuity_artifact()
            .expect("load latest continuity")
            .expect("latest continuity exists");

        assert_eq!(latest.generated_at_unix_ms, 20);
        assert_eq!(latest.latest_version_id.as_deref(), Some("4.2.0"));
        assert!(older_path.exists());
        assert!(newer_path.exists());

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn continuity_index_tracks_rename_then_structural_drift_across_three_versions() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));

        let old_snapshot = continuity_snapshot(
            "4.0.0",
            continuity_asset(
                "Content/Character/Encore/Body.mesh",
                "body",
                "sig-body",
                &["base", "trim"],
                &["position", "normal"],
            ),
        );
        let mid_snapshot = continuity_snapshot(
            "4.1.0",
            continuity_asset(
                "Content/Character/Encore/Body_LOD0.mesh",
                "body",
                "sig-body",
                &["base", "trim"],
                &["position", "normal"],
            ),
        );
        let new_snapshot = continuity_snapshot(
            "4.2.0",
            continuity_asset(
                "Content/Character/Encore/Body_LOD0.mesh",
                "body",
                "sig-body",
                &["base", "trim", "accessory"],
                &["position", "normal", "tangent"],
            ),
        );

        let compare_ab = SnapshotComparer.compare(&old_snapshot, &mid_snapshot);
        let report_ab =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &mid_snapshot, &compare_ab);
        storage
            .save_run(&report_ab, &old_snapshot, &mid_snapshot, &compare_ab, None)
            .expect("save ab");

        let compare_bc = SnapshotComparer.compare(&mid_snapshot, &new_snapshot);
        let report_bc =
            VersionDiffReportBuilder.from_compare(&mid_snapshot, &new_snapshot, &compare_bc);
        storage
            .save_run(&report_bc, &mid_snapshot, &new_snapshot, &compare_bc, None)
            .expect("save bc");

        let compare_ac = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let report_ac =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare_ac);
        storage
            .save_run(&report_ac, &old_snapshot, &new_snapshot, &compare_ac, None)
            .expect("save ac");

        let continuity = storage
            .load_version_continuity_index()
            .expect("load continuity");
        let thread = continuity
            .threads
            .iter()
            .find(|thread| thread.anchor_version_id == "4.0.0")
            .expect("continuity thread");
        let thread_summary = continuity
            .thread_summaries
            .iter()
            .find(|summary| summary.thread_id == thread.thread_id)
            .expect("continuity summary");

        assert_eq!(continuity.summary.thread_count, 1);
        assert_eq!(
            thread.anchor.path.as_deref(),
            Some("Content/Character/Encore/Body.mesh")
        );
        assert_eq!(thread.observations.len(), 2);
        assert_eq!(
            thread.observations[0].relation,
            VersionContinuityRelation::RenameOrRepath
        );
        assert_eq!(
            thread.observations[1].relation,
            VersionContinuityRelation::LayoutDrift
        );
        assert!(thread.review_required);
        assert_eq!(continuity.summary.ongoing_threads, 1);
        assert_eq!(thread_summary.first_seen_version, "4.0.0");
        assert_eq!(thread_summary.latest_observed_version, "4.2.0");
        assert_eq!(thread_summary.latest_live_version.as_deref(), Some("4.2.0"));
        assert_eq!(
            thread_summary
                .first_rename_or_repath_step
                .as_ref()
                .map(|step| (step.from_version_id.as_str(), step.to_version_id.as_str())),
            Some(("4.0.0", "4.1.0"))
        );
        assert_eq!(
            thread_summary
                .first_layout_drift_step
                .as_ref()
                .map(|step| (step.from_version_id.as_str(), step.to_version_id.as_str())),
            Some(("4.1.0", "4.2.0"))
        );
        assert!(thread_summary.first_container_movement_step.is_none());
        assert!(thread_summary.terminal_relation.is_none());
        assert!(thread_summary.terminal_version.is_none());
        assert!(thread_summary.review_required);
        assert!(!thread_summary.purely_persisted);
        assert!(thread_summary.materially_changed);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn continuity_index_counts_container_movement_threads_as_ongoing() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));

        let old_snapshot = continuity_snapshot(
            "6.0.0",
            continuity_asset_with_source(
                "Content/Character/Encore/Body.mesh",
                "body",
                "sig-body",
                &["base", "trim"],
                &["position", "normal"],
                Some("pakchunk0-WindowsNoEditor.pak/character/encore/body.mesh"),
                Some("pakchunk0-WindowsNoEditor.pak"),
            ),
        );
        let new_snapshot = continuity_snapshot(
            "6.1.0",
            continuity_asset_with_source(
                "Content/Character/Encore/Body.mesh",
                "body",
                "sig-body",
                &["base", "trim"],
                &["position", "normal"],
                Some("pakchunk1-WindowsNoEditor.pak/character/encore/body.mesh"),
                Some("pakchunk1-WindowsNoEditor.pak"),
            ),
        );

        let compare = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let report = VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare);
        storage
            .save_run(&report, &old_snapshot, &new_snapshot, &compare, None)
            .expect("save run");

        let continuity = storage
            .load_version_continuity_index()
            .expect("load continuity");
        let thread = continuity.threads.first().expect("thread exists");
        let thread_summary = continuity
            .thread_summaries
            .first()
            .expect("thread summary exists");

        assert_eq!(thread.observations.len(), 1);
        assert_eq!(
            thread.observations[0].relation,
            VersionContinuityRelation::ContainerMovement
        );
        assert_eq!(continuity.summary.container_movement_threads, 1);
        assert_eq!(continuity.summary.ongoing_threads, 1);
        assert_eq!(thread_summary.first_seen_version, "6.0.0");
        assert_eq!(thread_summary.latest_observed_version, "6.1.0");
        assert_eq!(thread_summary.latest_live_version.as_deref(), Some("6.1.0"));
        assert_eq!(
            thread_summary
                .first_container_movement_step
                .as_ref()
                .map(|step| (step.from_version_id.as_str(), step.to_version_id.as_str())),
            Some(("6.0.0", "6.1.0"))
        );
        assert!(thread_summary.first_rename_or_repath_step.is_none());
        assert!(thread_summary.first_layout_drift_step.is_none());
        assert!(thread_summary.terminal_relation.is_none());
        assert!(thread_summary.terminal_version.is_none());
        assert!(!thread_summary.purely_persisted);
        assert!(thread_summary.materially_changed);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn continuity_index_keeps_ambiguous_remaps_conservative() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));

        let old_snapshot = continuity_snapshot(
            "5.0.0",
            continuity_asset(
                "Content/Character/Encore/Face.mesh",
                "face",
                "sig-face-old",
                &["face"],
                &["position", "normal"],
            ),
        );
        let mid_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "5.1.0".to_string(),
            created_at_unix_ms: 1,
            source_root: "root".to_string(),
            asset_count: 2,
            assets: vec![
                continuity_asset(
                    "Content/Character/Encore/Face_A.mesh",
                    "face",
                    "sig-face-mid-a",
                    &["face"],
                    &["position", "normal"],
                ),
                continuity_asset(
                    "Content/Character/Encore/Face_B.mesh",
                    "face",
                    "sig-face-mid-b",
                    &["face"],
                    &["position", "normal"],
                ),
            ],
            context: SnapshotContext::default(),
        };
        let chosen_path = "Content/Character/Encore/Face_A.mesh".to_string();
        let new_snapshot = continuity_snapshot(
            &"5.2.0",
            continuity_asset(
                &chosen_path,
                "face",
                "sig-face-mid-a",
                &["face"],
                &["position", "normal"],
            ),
        );

        let compare_ab = SnapshotComparer.compare(&old_snapshot, &mid_snapshot);
        assert!(
            compare_ab
                .candidate_mapping_changes
                .iter()
                .any(|candidate| candidate.ambiguous)
        );
        let report_ab =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &mid_snapshot, &compare_ab);
        storage
            .save_run(&report_ab, &old_snapshot, &mid_snapshot, &compare_ab, None)
            .expect("save ab");

        let compare_bc = SnapshotComparer.compare(&mid_snapshot, &new_snapshot);
        let report_bc =
            VersionDiffReportBuilder.from_compare(&mid_snapshot, &new_snapshot, &compare_bc);
        storage
            .save_run(&report_bc, &mid_snapshot, &new_snapshot, &compare_bc, None)
            .expect("save bc");

        let continuity = storage
            .load_version_continuity_index()
            .expect("load continuity");
        let ambiguous_thread = continuity
            .threads
            .iter()
            .find(|thread| thread.anchor_version_id == "5.0.0")
            .expect("ambiguous thread");
        let carried_thread = continuity
            .threads
            .iter()
            .find(|thread| {
                thread.anchor_version_id == "5.1.0"
                    && thread.anchor.path.as_deref() == Some(chosen_path.as_str())
            })
            .expect("carried thread");
        let ambiguous_summary = continuity
            .thread_summaries
            .iter()
            .find(|summary| summary.thread_id == ambiguous_thread.thread_id)
            .expect("ambiguous summary");
        let carried_summary = continuity
            .thread_summaries
            .iter()
            .find(|summary| summary.thread_id == carried_thread.thread_id)
            .expect("carried summary");

        assert!(ambiguous_thread.review_required);
        assert_eq!(
            ambiguous_thread
                .observations
                .first()
                .expect("ambiguous obs")
                .relation,
            VersionContinuityRelation::Ambiguous
        );
        assert_eq!(carried_thread.observations.len(), 1);
        assert_eq!(
            carried_thread.observations[0].relation,
            VersionContinuityRelation::Persisted
        );
        assert_eq!(
            ambiguous_summary.terminal_relation,
            Some(VersionContinuityRelation::Ambiguous)
        );
        assert_eq!(ambiguous_summary.terminal_version.as_deref(), Some("5.1.0"));
        assert!(ambiguous_summary.latest_live_version.is_none());
        assert!(!ambiguous_summary.purely_persisted);
        assert!(ambiguous_summary.materially_changed);
        assert!(carried_summary.terminal_relation.is_none());
        assert_eq!(
            carried_summary.latest_live_version.as_deref(),
            Some("5.2.0")
        );
        assert!(carried_summary.purely_persisted);
        assert!(!carried_summary.materially_changed);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn continuity_index_exposes_terminal_milestones_for_removed_and_replacement_threads() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));

        let removed_old = continuity_snapshot(
            "7.0.0",
            continuity_asset(
                "Content/Character/Encore/Removed.mesh",
                "removed",
                "sig-removed",
                &["base"],
                &["position"],
            ),
        );
        let removed_new = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "7.1.0".to_string(),
            created_at_unix_ms: 1,
            source_root: "root".to_string(),
            asset_count: 0,
            assets: Vec::new(),
            context: SnapshotContext::default(),
        };
        let compare_removed = SnapshotComparer.compare(&removed_old, &removed_new);
        let report_removed =
            VersionDiffReportBuilder.from_compare(&removed_old, &removed_new, &compare_removed);
        storage
            .save_run(
                &report_removed,
                &removed_old,
                &removed_new,
                &compare_removed,
                None,
            )
            .expect("save removed report");

        let replacement_old = continuity_snapshot(
            "8.0.0",
            continuity_asset(
                "Content/Character/Encore/Mask.mesh",
                "mask",
                "sig-mask-old",
                &["mask"],
                &["position", "normal"],
            ),
        );
        let mut replacement_asset = continuity_asset(
            "Content/Character/Encore/Mask.mesh",
            "mask",
            "sig-mask-new",
            &["mask"],
            &["position", "normal"],
        );
        replacement_asset.hash_fields.asset_hash = Some("hash-mask-replaced".to_string());
        let replacement_new = continuity_snapshot("8.1.0", replacement_asset);
        let compare_replacement = SnapshotComparer.compare(&replacement_old, &replacement_new);
        let report_replacement = VersionDiffReportBuilder.from_compare(
            &replacement_old,
            &replacement_new,
            &compare_replacement,
        );
        storage
            .save_run(
                &report_replacement,
                &replacement_old,
                &replacement_new,
                &compare_replacement,
                None,
            )
            .expect("save replacement report");

        let continuity = storage
            .load_version_continuity_index()
            .expect("load continuity");
        let removed_summary = continuity
            .thread_summaries
            .iter()
            .find(|summary| {
                summary.anchor.path.as_deref() == Some("Content/Character/Encore/Removed.mesh")
            })
            .expect("removed summary");
        let replacement_summary = continuity
            .thread_summaries
            .iter()
            .find(|summary| {
                summary.anchor.path.as_deref() == Some("Content/Character/Encore/Mask.mesh")
            })
            .expect("replacement summary");

        assert_eq!(
            removed_summary.terminal_relation,
            Some(VersionContinuityRelation::Removed)
        );
        assert_eq!(removed_summary.terminal_version.as_deref(), Some("7.1.0"));
        assert!(removed_summary.latest_live_version.is_none());
        assert!(!removed_summary.purely_persisted);
        assert!(removed_summary.materially_changed);
        assert_eq!(
            replacement_summary.terminal_relation,
            Some(VersionContinuityRelation::Replacement)
        );
        assert_eq!(
            replacement_summary.terminal_version.as_deref(),
            Some("8.1.0")
        );
        assert!(replacement_summary.latest_live_version.is_none());
        assert!(!replacement_summary.purely_persisted);
        assert!(replacement_summary.materially_changed);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn continuity_index_marks_purely_persisted_threads_without_material_milestones() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::new(test_root.join("out").join("report"));

        let old_snapshot = continuity_snapshot(
            "9.0.0",
            continuity_asset(
                "Content/Character/Encore/Persisted.mesh",
                "persisted",
                "sig-persisted",
                &["base"],
                &["position", "normal"],
            ),
        );
        let new_snapshot = continuity_snapshot(
            "9.1.0",
            continuity_asset(
                "Content/Character/Encore/Persisted.mesh",
                "persisted",
                "sig-persisted",
                &["base"],
                &["position", "normal"],
            ),
        );
        let compare = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let report = VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare);
        storage
            .save_run(&report, &old_snapshot, &new_snapshot, &compare, None)
            .expect("save persisted report");

        let continuity = storage
            .load_version_continuity_index()
            .expect("load continuity");
        let summary = continuity
            .thread_summaries
            .iter()
            .find(|summary| {
                summary.anchor.path.as_deref() == Some("Content/Character/Encore/Persisted.mesh")
            })
            .expect("persisted summary");

        assert_eq!(summary.first_seen_version, "9.0.0");
        assert_eq!(summary.latest_observed_version, "9.1.0");
        assert_eq!(summary.latest_live_version.as_deref(), Some("9.1.0"));
        assert!(summary.first_rename_or_repath_step.is_none());
        assert!(summary.first_container_movement_step.is_none());
        assert!(summary.first_layout_drift_step.is_none());
        assert!(summary.terminal_relation.is_none());
        assert!(summary.terminal_version.is_none());
        assert!(!summary.review_required);
        assert!(summary.purely_persisted);
        assert!(!summary.materially_changed);

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
                    ..Default::default()
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
                    ..Default::default()
                },
                hash_fields: SnapshotHashFields::default(),
                source: crate::domain::AssetSourceContext::default(),
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
            lineage: Default::default(),
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
            review: Default::default(),
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
            mod_dependency_input: None,
            representative_mod_baseline_input: None,
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
                compatibility: crate::compare::RemapCompatibility::LikelyCompatible,
                needs_review: false,
                ambiguous: false,
                confidence_gap: Some(0.2),
                continuity: None,
                reasons: vec!["exact path".to_string()],
                evidence: vec!["compare confidence".to_string()],
            }],
            representative_risk_projections: Vec::new(),
        }
    }

    fn sample_continuity_review_inference_report(
        old_version: &str,
        new_version: &str,
    ) -> InferenceReport {
        InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 321,
            compare_input: InferenceCompareInput {
                old_version_id: old_version.to_string(),
                new_version_id: new_version.to_string(),
                changed_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 1,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 3,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            mod_dependency_input: None,
            representative_mod_baseline_input: None,
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 1,
                suggested_fixes: 1,
                candidate_mapping_hints: 1,
                highest_confidence: 0.95,
            },
            probable_crash_causes: vec![ProbableCrashCause {
                code: "continuity_thread_instability".to_string(),
                summary: "broader continuity history is unstable".to_string(),
                confidence: 0.84,
                risk: RiskLevel::High,
                affected_assets: vec!["Content/Character/Encore/Body.mesh".to_string()],
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                reasons: vec!["continuity surfaces unstable thread history".to_string()],
                evidence: vec![
                    "continuity thread Content/Character/Encore/Body_v2.mesh spans 7.0.0 -> 8.2.0; later terminates as removed in 8.2.0".to_string(),
                ],
            }],
            suggested_fixes: vec![SuggestedFix {
                code: "review_continuity_thread_history_before_repair".to_string(),
                summary: "review broader continuity history".to_string(),
                confidence: 0.82,
                priority: RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec!["inspect continuity milestones".to_string()],
                reasons: vec!["broader continuity history is unstable".to_string()],
                evidence: vec![
                    "continuity thread later terminates as removed in 8.2.0".to_string(),
                ],
            }],
            candidate_mapping_hints: vec![InferredMappingHint {
                old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                confidence: 0.95,
                compatibility: crate::compare::RemapCompatibility::CompatibleWithCaution,
                needs_review: true,
                ambiguous: false,
                confidence_gap: Some(0.20),
                continuity: Some(InferredMappingContinuityContext {
                    thread_id: Some("encore_body".to_string()),
                    first_seen_version: Some("7.0.0".to_string()),
                    latest_observed_version: Some("8.2.0".to_string()),
                    latest_live_version: None,
                    stable_before_current_change: false,
                    total_rename_steps: 1,
                    total_container_movement_steps: 0,
                    total_layout_drift_steps: 0,
                    review_required_history: true,
                    terminal_relation: Some(VersionContinuityRelation::Removed),
                    terminal_version: Some("8.2.0".to_string()),
                    terminal_after_current: true,
                    instability_detected: true,
                }),
                reasons: vec![
                    "same_parent_directory: same folder".to_string(),
                    "broader continuity history reaches a later terminal state for this thread; do not auto-promote this mapping".to_string(),
                ],
                evidence: vec![
                    "structured continuity thread span: 7.0.0 -> 8.2.0".to_string(),
                    "structured continuity history reaches terminal state removed in 8.2.0".to_string(),
                ],
            }],
            representative_risk_projections: Vec::new(),
        }
    }

    fn sample_continuity_artifact(
        latest_version_id: &str,
        generated_at_unix_ms: u128,
        thread_count: usize,
    ) -> VersionContinuityArtifact {
        VersionContinuityArtifact {
            schema_version: "whashreonator.continuity.v1".to_string(),
            generated_at_unix_ms,
            report_count: thread_count,
            latest_version_id: Some(latest_version_id.to_string()),
            continuity: VersionContinuityIndex {
                summary: VersionContinuitySummary {
                    thread_count,
                    observation_count: thread_count,
                    ongoing_threads: thread_count,
                    ..VersionContinuitySummary::default()
                },
                threads: Vec::new(),
                thread_summaries: Vec::new(),
            },
        }
    }

    fn continuity_snapshot(version_id: &str, asset: SnapshotAsset) -> GameSnapshot {
        GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.to_string(),
            created_at_unix_ms: 1,
            source_root: "root".to_string(),
            asset_count: 1,
            assets: vec![asset],
            context: SnapshotContext::default(),
        }
    }

    fn continuity_asset(
        path: &str,
        logical_name: &str,
        signature: &str,
        section_labels: &[&str],
        buffer_roles: &[&str],
    ) -> SnapshotAsset {
        continuity_asset_with_source(
            path,
            logical_name,
            signature,
            section_labels,
            buffer_roles,
            Some("pakchunk0-WindowsNoEditor.pak/character/encore/body.mesh"),
            Some("pakchunk0-WindowsNoEditor.pak"),
        )
    }

    fn continuity_asset_with_source(
        path: &str,
        logical_name: &str,
        signature: &str,
        section_labels: &[&str],
        buffer_roles: &[&str],
        source_path: Option<&str>,
        container_path: Option<&str>,
    ) -> SnapshotAsset {
        SnapshotAsset {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            metadata: AssetMetadata {
                logical_name: Some(logical_name.to_string()),
                vertex_count: Some(120),
                index_count: Some(240),
                material_slots: Some(2),
                section_count: Some(1),
                layout_markers: vec!["skinned".to_string(), "interleaved".to_string()],
                internal_structure: AssetInternalStructure {
                    section_labels: section_labels
                        .iter()
                        .map(|value| value.to_string())
                        .collect(),
                    buffer_roles: buffer_roles.iter().map(|value| value.to_string()).collect(),
                    binding_targets: vec!["albedo".to_string()],
                    subresource_roles: vec!["geometry".to_string()],
                    has_skeleton: Some(true),
                    has_shapekey_data: Some(false),
                },
                tags: vec!["character".to_string(), "encore".to_string()],
                ..Default::default()
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: Some(logical_name.to_string()),
                name_tokens: logical_name
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
                path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
                tags: vec!["character".to_string(), "encore".to_string()],
                vertex_count: Some(120),
                index_count: Some(240),
                material_slots: Some(2),
                section_count: Some(1),
                vertex_stride: Some(32),
                vertex_buffer_count: Some(1),
                index_format: Some("u16".to_string()),
                primitive_topology: Some("triangle_list".to_string()),
                layout_markers: vec!["interleaved".to_string(), "skinned".to_string()],
                internal_structure: AssetInternalStructure {
                    section_labels: section_labels
                        .iter()
                        .map(|value| value.to_string())
                        .collect(),
                    buffer_roles: buffer_roles.iter().map(|value| value.to_string()).collect(),
                    binding_targets: vec!["albedo".to_string()],
                    subresource_roles: vec!["geometry".to_string()],
                    has_skeleton: Some(true),
                    has_shapekey_data: Some(false),
                },
            },
            hash_fields: crate::snapshot::SnapshotHashFields {
                asset_hash: Some(format!("hash-{path}")),
                shader_hash: Some("shader-shared".to_string()),
                signature: Some(signature.to_string()),
            },
            source: crate::domain::AssetSourceContext {
                extraction_tool: Some("fixture-extractor".to_string()),
                source_root: Some("root".to_string()),
                source_path: source_path.map(ToOwned::to_owned),
                container_path: container_path.map(ToOwned::to_owned),
                source_kind: Some("mesh_record".to_string()),
            },
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
