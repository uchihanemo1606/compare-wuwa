use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    compare::{
        CandidateMappingChange, SnapshotAssetChange, SnapshotAssetSummary, SnapshotChangeType,
        SnapshotCompareReason, SnapshotCompareReport,
    },
    inference::InferenceReport,
    snapshot::{GameSnapshot, assess_snapshot_scope},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionDiffReportV2 {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub old_version: VersionSide,
    pub new_version: VersionSide,
    pub resonators: Vec<ResonatorDiffEntry>,
    pub summary: VersionDiffSummary,
    #[serde(default)]
    pub scope_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionSide {
    pub version_id: String,
    pub source_root: String,
    pub asset_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionDiffSummary {
    pub resonator_count: usize,
    pub unchanged_items: usize,
    pub changed_items: usize,
    pub added_items: usize,
    pub removed_items: usize,
    pub uncertain_items: usize,
    pub mapping_candidates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResonatorDiffEntry {
    pub resonator: String,
    pub old_version: ResonatorVersionView,
    pub new_version: ResonatorVersionView,
    pub items: Vec<ResonatorItemDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ResonatorVersionView {
    pub asset_count: usize,
    pub buffer_count: usize,
    pub mapping_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResonatorItemDiff {
    pub item_type: ReportItemType,
    pub status: DiffStatus,
    pub confidence: Option<f32>,
    pub old: Option<VersionedItem>,
    pub new: Option<VersionedItem>,
    pub reasons: Vec<ReportReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportItemType {
    Asset,
    Buffer,
    Mapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Unchanged,
    Changed,
    Added,
    Removed,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionedItem {
    pub key: String,
    pub label: String,
    pub path: Option<String>,
    pub metadata: TechnicalMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct TechnicalMetadata {
    pub kind: Option<String>,
    pub normalized_name: Option<String>,
    pub vertex_count: Option<u32>,
    pub index_count: Option<u32>,
    pub material_slots: Option<u32>,
    pub section_count: Option<u32>,
    pub asset_hash: Option<String>,
    pub shader_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct VersionDiffReportBuilder;

impl VersionDiffReportBuilder {
    pub fn from_compare(
        &self,
        old_snapshot: &GameSnapshot,
        new_snapshot: &GameSnapshot,
        compare_report: &SnapshotCompareReport,
    ) -> VersionDiffReportV2 {
        let mut groups = BTreeMap::<String, ResonatorCollector>::new();

        for asset in unchanged_assets(old_snapshot, new_snapshot) {
            let resonator =
                infer_resonator_name(&asset.path).unwrap_or_else(|| "Unknown".to_string());
            groups.entry(resonator).or_default().push_asset(
                DiffStatus::Unchanged,
                Some(asset.into()),
                Some(asset.into()),
                Vec::new(),
            );
        }

        for change in &compare_report.changed_assets {
            let resonator = infer_change_resonator(change);
            groups.entry(resonator).or_default().push_change(change);
        }

        for change in &compare_report.added_assets {
            let resonator = infer_change_resonator(change);
            groups.entry(resonator).or_default().push_change(change);
        }

        for change in &compare_report.removed_assets {
            let resonator = infer_change_resonator(change);
            groups.entry(resonator).or_default().push_change(change);
        }

        for candidate in &compare_report.candidate_mapping_changes {
            let resonator = infer_mapping_resonator(candidate);
            groups
                .entry(resonator)
                .or_default()
                .push_mapping_candidate(candidate);
        }

        let resonators = groups
            .into_iter()
            .map(|(resonator, collector)| collector.finish(resonator))
            .collect::<Vec<_>>();

        let summary = summarize(&resonators);

        VersionDiffReportV2 {
            schema_version: "whashreonator.report.v2".to_string(),
            generated_at_unix_ms: current_unix_ms(),
            old_version: VersionSide {
                version_id: compare_report.old_snapshot.version_id.clone(),
                source_root: compare_report.old_snapshot.source_root.clone(),
                asset_count: compare_report.old_snapshot.asset_count,
            },
            new_version: VersionSide {
                version_id: compare_report.new_snapshot.version_id.clone(),
                source_root: compare_report.new_snapshot.source_root.clone(),
                asset_count: compare_report.new_snapshot.asset_count,
            },
            resonators,
            summary,
            scope_notes: build_scope_notes(old_snapshot, new_snapshot),
        }
    }

    pub fn enrich_with_inference(
        &self,
        mut report: VersionDiffReportV2,
        inference: &InferenceReport,
    ) -> VersionDiffReportV2 {
        let mut mapping_hints = inference
            .candidate_mapping_hints
            .iter()
            .map(|hint| {
                (
                    (hint.old_asset_path.clone(), hint.new_asset_path.clone()),
                    hint,
                )
            })
            .collect::<BTreeMap<_, _>>();

        for resonator in &mut report.resonators {
            for item in &mut resonator.items {
                if item.item_type != ReportItemType::Mapping {
                    continue;
                }
                let Some(old_item) = item.old.as_ref() else {
                    continue;
                };
                let Some(new_item) = item.new.as_ref() else {
                    continue;
                };

                if let Some(hint) =
                    mapping_hints.remove(&(old_item.key.clone(), new_item.key.clone()))
                {
                    item.confidence = Some(hint.confidence);
                    item.status = if hint.ambiguous || hint.needs_review {
                        DiffStatus::Uncertain
                    } else {
                        DiffStatus::Changed
                    };
                    item.reasons
                        .extend(hint.reasons.iter().map(|reason| ReportReason {
                            code: "inference".to_string(),
                            message: reason.clone(),
                        }));
                }
            }
        }

        report.summary = summarize(&report.resonators);
        report
    }
}

#[derive(Debug, Default)]
struct ResonatorCollector {
    old_asset_count: usize,
    new_asset_count: usize,
    old_buffer_count: usize,
    new_buffer_count: usize,
    old_mapping_count: usize,
    new_mapping_count: usize,
    items: Vec<ResonatorItemDiff>,
}

impl ResonatorCollector {
    fn push_asset(
        &mut self,
        status: DiffStatus,
        old: Option<VersionedItem>,
        new: Option<VersionedItem>,
        reasons: Vec<ReportReason>,
    ) {
        if old.is_some() {
            self.old_asset_count += 1;
            self.old_buffer_count += 1;
        }
        if new.is_some() {
            self.new_asset_count += 1;
            self.new_buffer_count += 1;
        }

        let old_buffer = old.as_ref().map(buffer_item_from);
        let new_buffer = new.as_ref().map(buffer_item_from);

        self.items.push(ResonatorItemDiff {
            item_type: ReportItemType::Asset,
            status: status.clone(),
            confidence: None,
            old,
            new,
            reasons: reasons.clone(),
        });
        self.items.push(ResonatorItemDiff {
            item_type: ReportItemType::Buffer,
            status,
            confidence: None,
            old: old_buffer,
            new: new_buffer,
            reasons,
        });
    }

    fn push_change(&mut self, change: &SnapshotAssetChange) {
        let status = match change.change_type {
            SnapshotChangeType::Added => DiffStatus::Added,
            SnapshotChangeType::Removed => DiffStatus::Removed,
            SnapshotChangeType::Changed => DiffStatus::Changed,
        };
        let reasons = change
            .reasons
            .iter()
            .map(reason_from_compare)
            .collect::<Vec<_>>();
        self.push_asset(
            status,
            change.old_asset.as_ref().map(VersionedItem::from),
            change.new_asset.as_ref().map(VersionedItem::from),
            reasons,
        );
    }

    fn push_mapping_candidate(&mut self, candidate: &CandidateMappingChange) {
        self.old_mapping_count += 1;
        self.new_mapping_count += 1;
        self.items.push(ResonatorItemDiff {
            item_type: ReportItemType::Mapping,
            status: if candidate.ambiguous {
                DiffStatus::Uncertain
            } else {
                DiffStatus::Changed
            },
            confidence: Some(candidate.confidence),
            old: Some(VersionedItem::from(&candidate.old_asset)),
            new: Some(VersionedItem::from(&candidate.new_asset)),
            reasons: candidate.reasons.iter().map(reason_from_compare).collect(),
        });
    }

    fn finish(self, resonator: String) -> ResonatorDiffEntry {
        ResonatorDiffEntry {
            resonator,
            old_version: ResonatorVersionView {
                asset_count: self.old_asset_count,
                buffer_count: self.old_buffer_count,
                mapping_count: self.old_mapping_count,
            },
            new_version: ResonatorVersionView {
                asset_count: self.new_asset_count,
                buffer_count: self.new_buffer_count,
                mapping_count: self.new_mapping_count,
            },
            items: self.items,
        }
    }
}

impl From<&SnapshotAssetSummary> for VersionedItem {
    fn from(value: &SnapshotAssetSummary) -> Self {
        Self {
            key: value.path.clone(),
            label: value
                .normalized_name
                .clone()
                .or_else(|| value.kind.clone())
                .unwrap_or_else(|| value.path.clone()),
            path: Some(value.path.clone()),
            metadata: TechnicalMetadata {
                kind: value.kind.clone(),
                normalized_name: value.normalized_name.clone(),
                vertex_count: value.vertex_count,
                index_count: value.index_count,
                material_slots: value.material_slots,
                section_count: value.section_count,
                asset_hash: value.asset_hash.clone(),
                shader_hash: value.shader_hash.clone(),
            },
        }
    }
}

impl From<&crate::snapshot::SnapshotAsset> for VersionedItem {
    fn from(value: &crate::snapshot::SnapshotAsset) -> Self {
        Self {
            key: value.path.clone(),
            label: value
                .fingerprint
                .normalized_name
                .clone()
                .or_else(|| value.kind.clone())
                .unwrap_or_else(|| value.path.clone()),
            path: Some(value.path.clone()),
            metadata: TechnicalMetadata {
                kind: value.kind.clone(),
                normalized_name: value.fingerprint.normalized_name.clone(),
                vertex_count: value.fingerprint.vertex_count,
                index_count: value.fingerprint.index_count,
                material_slots: value.fingerprint.material_slots,
                section_count: value.fingerprint.section_count,
                asset_hash: value.hash_fields.asset_hash.clone(),
                shader_hash: value.hash_fields.shader_hash.clone(),
            },
        }
    }
}

fn summarize(resonators: &[ResonatorDiffEntry]) -> VersionDiffSummary {
    let mut summary = VersionDiffSummary {
        resonator_count: resonators.len(),
        unchanged_items: 0,
        changed_items: 0,
        added_items: 0,
        removed_items: 0,
        uncertain_items: 0,
        mapping_candidates: 0,
    };

    for resonator in resonators {
        for item in &resonator.items {
            match item.status {
                DiffStatus::Unchanged => summary.unchanged_items += 1,
                DiffStatus::Changed => summary.changed_items += 1,
                DiffStatus::Added => summary.added_items += 1,
                DiffStatus::Removed => summary.removed_items += 1,
                DiffStatus::Uncertain => summary.uncertain_items += 1,
            }
            if item.item_type == ReportItemType::Mapping {
                summary.mapping_candidates += 1;
            }
        }
    }

    summary
}

fn buffer_item_from(item: &VersionedItem) -> VersionedItem {
    VersionedItem {
        key: format!("buffer:{}", item.key),
        label: format!("Buffer {}", item.label),
        path: item.path.clone(),
        metadata: item.metadata.clone(),
    }
}

fn infer_change_resonator(change: &SnapshotAssetChange) -> String {
    change
        .old_asset
        .as_ref()
        .and_then(|asset| infer_resonator_name(&asset.path))
        .or_else(|| {
            change
                .new_asset
                .as_ref()
                .and_then(|asset| infer_resonator_name(&asset.path))
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

fn infer_mapping_resonator(candidate: &CandidateMappingChange) -> String {
    infer_resonator_name(&candidate.old_asset.path)
        .or_else(|| infer_resonator_name(&candidate.new_asset.path))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn infer_resonator_name(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let parts = normalized.split('/').collect::<Vec<_>>();
    parts
        .windows(3)
        .find(|window| {
            window[0].eq_ignore_ascii_case("content")
                && window[1].eq_ignore_ascii_case("character")
                && !window[2].is_empty()
        })
        .map(|window| window[2].to_string())
}

fn build_scope_notes(old_snapshot: &GameSnapshot, new_snapshot: &GameSnapshot) -> Vec<String> {
    let old_scope = assess_snapshot_scope(old_snapshot);
    let new_scope = assess_snapshot_scope(new_snapshot);

    let mut notes = vec![
        format!(
            "old snapshot {} scope: mode={} install_or_package_level={} meaningful_content={} meaningful_character={} content_like_paths={} character_paths={} non_content_paths={}",
            old_snapshot.version_id,
            old_scope.capture_mode.as_deref().unwrap_or("unknown"),
            old_scope.mostly_install_or_package_level,
            old_scope.meaningful_content_coverage,
            old_scope.meaningful_character_coverage,
            old_scope.coverage.content_like_path_count,
            old_scope.coverage.character_path_count,
            old_scope.coverage.non_content_path_count
        ),
        format!(
            "new snapshot {} scope: mode={} install_or_package_level={} meaningful_content={} meaningful_character={} content_like_paths={} character_paths={} non_content_paths={}",
            new_snapshot.version_id,
            new_scope.capture_mode.as_deref().unwrap_or("unknown"),
            new_scope.mostly_install_or_package_level,
            new_scope.meaningful_content_coverage,
            new_scope.meaningful_character_coverage,
            new_scope.coverage.content_like_path_count,
            new_scope.coverage.character_path_count,
            new_scope.coverage.non_content_path_count
        ),
    ];

    if old_scope.is_low_signal_for_character_analysis()
        || new_scope.is_low_signal_for_character_analysis()
    {
        notes.push(
            "scope warning: compare results are based on install/package-level or low-coverage snapshots; deep character-level interpretation may be limited."
                .to_string(),
        );
    }

    notes
}

fn unchanged_assets<'a>(
    old_snapshot: &'a GameSnapshot,
    new_snapshot: &'a GameSnapshot,
) -> Vec<&'a crate::snapshot::SnapshotAsset> {
    let new_by_path = new_snapshot
        .assets
        .iter()
        .map(|asset| (asset.path.as_str(), asset))
        .collect::<BTreeMap<_, _>>();

    old_snapshot
        .assets
        .iter()
        .filter(|old_asset| {
            new_by_path
                .get(old_asset.path.as_str())
                .is_some_and(|new_asset| *old_asset == *new_asset)
        })
        .collect()
}

fn reason_from_compare(reason: &SnapshotCompareReason) -> ReportReason {
    ReportReason {
        code: reason.code.clone(),
        message: reason.message.clone(),
    }
}

fn current_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub fn load_version_diff_report_v2(
    path: &std::path::Path,
) -> crate::error::AppResult<VersionDiffReportV2> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

#[cfg(test)]
mod tests {
    use crate::{
        compare::SnapshotComparer,
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceScopeContext,
            InferenceSummary, InferredMappingHint,
        },
        snapshot::{
            GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotCoverageSignals,
            SnapshotFingerprint, SnapshotHashFields, SnapshotScopeContext,
        },
    };

    use super::{DiffStatus, ReportItemType, VersionDiffReportBuilder};

    #[test]
    fn builder_creates_resonator_scoped_v2_report() {
        let old_snapshot = sample_snapshot(
            "2.4.0",
            vec![
                asset(
                    "Content/Character/Encore/Body.mesh",
                    "encore body",
                    Some(100),
                ),
                asset("Content/Weapon/Pistol_Main.weapon", "pistol", Some(40)),
            ],
        );
        let new_snapshot = sample_snapshot(
            "2.5.0",
            vec![
                asset(
                    "Content/Character/Encore/Body.mesh",
                    "encore body",
                    Some(120),
                ),
                asset("Content/Weapon/Pistol_Main_A.weapon", "pistol", Some(40)),
            ],
        );
        let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

        let report =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare_report);

        assert_eq!(report.schema_version, "whashreonator.report.v2");
        assert_eq!(report.old_version.version_id, "2.4.0");
        assert_eq!(report.new_version.version_id, "2.5.0");
        assert!(
            report
                .resonators
                .iter()
                .any(|entry| entry.resonator == "Encore")
        );
        assert!(report.summary.mapping_candidates >= 1);
        assert!(
            report
                .scope_notes
                .iter()
                .any(|note| note.contains("scope warning"))
        );
    }

    #[test]
    fn builder_enriches_mapping_entries_with_inference_confidence() {
        let old_snapshot = sample_snapshot(
            "2.4.0",
            vec![asset(
                "Content/Weapon/Pistol_Main.weapon",
                "pistol",
                Some(40),
            )],
        );
        let new_snapshot = sample_snapshot(
            "2.5.0",
            vec![asset(
                "Content/Weapon/Pistol_Main_A.weapon",
                "pistol",
                Some(40),
            )],
        );
        let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let report =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare_report);
        let inference = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "2.4.0".to_string(),
                new_version_id: "2.5.0".to_string(),
                changed_assets: 0,
                added_assets: 1,
                removed_assets: 1,
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
                old_asset_path: "Content/Weapon/Pistol_Main.weapon".to_string(),
                new_asset_path: "Content/Weapon/Pistol_Main_A.weapon".to_string(),
                confidence: 0.91,
                needs_review: false,
                ambiguous: false,
                confidence_gap: Some(0.2),
                reasons: vec!["strong evidence".to_string()],
                evidence: Vec::new(),
            }],
        };

        let report = VersionDiffReportBuilder.enrich_with_inference(report, &inference);
        let mapping_item = report
            .resonators
            .iter()
            .flat_map(|entry| entry.items.iter())
            .find(|item| item.item_type == ReportItemType::Mapping)
            .expect("mapping item");

        assert_eq!(mapping_item.confidence, Some(0.91));
        assert_eq!(mapping_item.status, DiffStatus::Changed);
    }

    #[test]
    fn builder_scope_notes_stay_informational_for_meaningful_scope() {
        let old_snapshot = sample_snapshot_with_scope(
            "6.0.0",
            vec![asset(
                "Content/Character/Encore/Body.mesh",
                "encore body",
                Some(120),
            )],
            SnapshotScopeContext {
                capture_mode: Some("local_filesystem_inventory".to_string()),
                mostly_install_or_package_level: Some(false),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(true),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 12,
                    character_path_count: 6,
                    non_content_path_count: 1,
                },
                note: Some("rich local scan".to_string()),
            },
        );
        let new_snapshot = sample_snapshot_with_scope(
            "6.1.0",
            vec![asset(
                "Content/Character/Encore/Body_v2.mesh",
                "encore body",
                Some(140),
            )],
            SnapshotScopeContext {
                capture_mode: Some("local_filesystem_inventory".to_string()),
                mostly_install_or_package_level: Some(false),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(true),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 14,
                    character_path_count: 7,
                    non_content_path_count: 1,
                },
                note: Some("rich local scan".to_string()),
            },
        );
        let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

        let report =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare_report);

        assert_eq!(report.scope_notes.len(), 2);
        assert!(
            report
                .scope_notes
                .iter()
                .all(|note| !note.contains("scope warning"))
        );
    }

    fn sample_snapshot(version_id: &str, assets: Vec<SnapshotAsset>) -> GameSnapshot {
        sample_snapshot_with_scope(version_id, assets, SnapshotScopeContext::default())
    }

    fn sample_snapshot_with_scope(
        version_id: &str,
        assets: Vec<SnapshotAsset>,
        scope: SnapshotScopeContext,
    ) -> GameSnapshot {
        GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.to_string(),
            created_at_unix_ms: 1,
            source_root: version_id.to_string(),
            asset_count: assets.len(),
            assets,
            context: SnapshotContext {
                launcher: None,
                resource_manifest: None,
                scope,
                notes: Vec::new(),
            },
        }
    }

    fn asset(path: &str, logical_name: &str, vertex_count: Option<u32>) -> SnapshotAsset {
        SnapshotAsset {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            metadata: crate::domain::AssetMetadata {
                logical_name: Some(logical_name.to_string()),
                vertex_count,
                index_count: Some(2),
                material_slots: Some(1),
                section_count: Some(1),
                tags: Vec::new(),
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: Some(logical_name.to_string()),
                name_tokens: logical_name
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
                path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
                tags: Vec::new(),
                vertex_count,
                index_count: Some(2),
                material_slots: Some(1),
                section_count: Some(1),
            },
            hash_fields: SnapshotHashFields::default(),
        }
    }
}
