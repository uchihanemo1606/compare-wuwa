use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{AssetInternalStructure, AssetSourceContext},
    error::AppResult,
    snapshot::{
        GameSnapshot, SnapshotAsset, assess_snapshot_scope, load_snapshot,
        snapshot_evidence_posture_from_parts, summarize_snapshot_capture_quality,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotCompareReport {
    pub schema_version: String,
    pub old_snapshot: SnapshotVersionInfo,
    pub new_snapshot: SnapshotVersionInfo,
    #[serde(default)]
    pub scope: SnapshotCompareScopeContext,
    pub summary: SnapshotCompareSummary,
    pub added_assets: Vec<SnapshotAssetChange>,
    pub removed_assets: Vec<SnapshotAssetChange>,
    pub changed_assets: Vec<SnapshotAssetChange>,
    pub candidate_mapping_changes: Vec<CandidateMappingChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotCompareScopeContext {
    pub old_snapshot: SnapshotCompareScopeInfo,
    pub new_snapshot: SnapshotCompareScopeInfo,
    pub low_signal_compare: bool,
    #[serde(default)]
    pub scope_narrowing_detected: bool,
    #[serde(default)]
    pub scope_induced_removals_likely: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotCompareScopeInfo {
    pub acquisition_kind: Option<String>,
    pub capture_mode: Option<String>,
    #[serde(default)]
    pub launcher_detected_version: Option<String>,
    #[serde(default)]
    pub launcher_reuse_version: Option<String>,
    #[serde(default)]
    pub launcher_version_matches_snapshot: Option<bool>,
    #[serde(default)]
    pub manifest_resource_count: usize,
    #[serde(default)]
    pub manifest_matched_assets: usize,
    #[serde(default)]
    pub manifest_unmatched_snapshot_assets: usize,
    #[serde(default)]
    pub assets_with_asset_hash: usize,
    #[serde(default)]
    pub assets_with_any_hash: usize,
    #[serde(default)]
    pub assets_with_signature: usize,
    #[serde(default)]
    pub assets_with_source_context: usize,
    #[serde(default)]
    pub assets_with_rich_metadata: usize,
    #[serde(default)]
    pub meaningfully_enriched_assets: usize,
    #[serde(default)]
    pub extractor_record_count: usize,
    #[serde(default)]
    pub extractor_records_with_hashes: usize,
    #[serde(default)]
    pub extractor_records_with_source_context: usize,
    #[serde(default)]
    pub extractor_records_with_rich_metadata: usize,
    #[serde(default)]
    pub extractor_inventory_schema_version: Option<String>,
    #[serde(default)]
    pub extractor_inventory_version_id: Option<String>,
    #[serde(default)]
    pub extractor_inventory_version_matches_snapshot: Option<bool>,
    #[serde(default)]
    pub launcher_version_matches_extractor_inventory: Option<bool>,
    pub mostly_install_or_package_level: bool,
    pub meaningful_content_coverage: bool,
    pub meaningful_character_coverage: bool,
    pub meaningful_asset_record_enrichment: bool,
    pub content_like_path_count: usize,
    pub character_path_count: usize,
    pub non_content_path_count: usize,
    pub low_signal_for_character_analysis: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotVersionInfo {
    pub version_id: String,
    pub source_root: String,
    pub asset_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotCompareSummary {
    pub total_old_assets: usize,
    pub total_new_assets: usize,
    pub unchanged_assets: usize,
    pub added_assets: usize,
    pub removed_assets: usize,
    pub changed_assets: usize,
    pub candidate_mapping_changes: usize,
    pub identity_changed_assets: usize,
    pub layout_changed_assets: usize,
    pub structural_changed_assets: usize,
    pub naming_only_changed_assets: usize,
    pub cosmetic_only_changed_assets: usize,
    pub provenance_changed_assets: usize,
    pub container_moved_assets: usize,
    pub lineage_rename_or_repath_assets: usize,
    pub lineage_container_movement_assets: usize,
    pub lineage_layout_drift_assets: usize,
    pub lineage_replacement_assets: usize,
    pub lineage_ambiguous_assets: usize,
    pub lineage_insufficient_evidence_assets: usize,
    pub ambiguous_candidate_mapping_changes: usize,
    pub high_confidence_candidate_mapping_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotChangeType {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemapCompatibility {
    LikelyCompatible,
    CompatibleWithCaution,
    StructurallyRisky,
    IncompatibleBlocked,
    #[default]
    InsufficientEvidence,
}

impl RemapCompatibility {
    pub fn supports_auto_proposal(&self) -> bool {
        matches!(
            self,
            RemapCompatibility::LikelyCompatible | RemapCompatibility::CompatibleWithCaution
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssetLineageKind {
    RenameOrRepath,
    ContainerMovement,
    LayoutDrift,
    Replacement,
    Ambiguous,
    #[default]
    InsufficientEvidence,
}

impl AssetLineageKind {
    fn reason_code(&self) -> &'static str {
        match self {
            AssetLineageKind::RenameOrRepath => "lineage_rename_or_repath",
            AssetLineageKind::ContainerMovement => "lineage_container_movement",
            AssetLineageKind::LayoutDrift => "lineage_layout_drift",
            AssetLineageKind::Replacement => "lineage_replacement",
            AssetLineageKind::Ambiguous => "lineage_ambiguous",
            AssetLineageKind::InsufficientEvidence => "lineage_insufficient_evidence",
        }
    }

    fn message(&self) -> &'static str {
        match self {
            AssetLineageKind::RenameOrRepath => {
                "lineage assessment: asset looks like a rename or repath of the same logical item"
            }
            AssetLineageKind::ContainerMovement => {
                "lineage assessment: asset appears to have moved with container/package regrouping"
            }
            AssetLineageKind::LayoutDrift => {
                "lineage assessment: asset is still related, but its layout drifted across versions"
            }
            AssetLineageKind::Replacement => {
                "lineage assessment: surface similarity is likely masking a real asset replacement"
            }
            AssetLineageKind::Ambiguous => {
                "lineage assessment: compare evidence is too ambiguous to trust the movement call"
            }
            AssetLineageKind::InsufficientEvidence => {
                "lineage assessment: snapshot evidence is not strong enough to classify movement safely"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotAssetChange {
    pub change_type: SnapshotChangeType,
    pub old_asset: Option<SnapshotAssetSummary>,
    pub new_asset: Option<SnapshotAssetSummary>,
    pub changed_fields: Vec<String>,
    pub probable_impact: RiskLevel,
    pub crash_risk: RiskLevel,
    pub suspected_mapping_change: bool,
    #[serde(default)]
    pub lineage: AssetLineageKind,
    pub reasons: Vec<SnapshotCompareReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotAssetSummary {
    pub id: String,
    pub path: String,
    pub kind: Option<String>,
    #[serde(default)]
    pub logical_name: Option<String>,
    pub normalized_name: Option<String>,
    pub vertex_count: Option<u32>,
    pub index_count: Option<u32>,
    pub material_slots: Option<u32>,
    pub section_count: Option<u32>,
    #[serde(default)]
    pub vertex_stride: Option<u32>,
    #[serde(default)]
    pub vertex_buffer_count: Option<u32>,
    #[serde(default)]
    pub index_format: Option<String>,
    #[serde(default)]
    pub primitive_topology: Option<String>,
    #[serde(default)]
    pub layout_markers: Vec<String>,
    #[serde(default)]
    pub internal_structure: AssetInternalStructure,
    pub asset_hash: Option<String>,
    pub shader_hash: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: AssetSourceContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotCompareReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateMappingChange {
    pub old_asset: SnapshotAssetSummary,
    pub new_asset: SnapshotAssetSummary,
    pub confidence: f32,
    #[serde(default)]
    pub compatibility: RemapCompatibility,
    #[serde(default)]
    pub lineage: AssetLineageKind,
    pub reasons: Vec<SnapshotCompareReason>,
    #[serde(default)]
    pub runner_up_confidence: Option<f32>,
    #[serde(default)]
    pub confidence_gap: Option<f32>,
    #[serde(default)]
    pub ambiguous: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SnapshotComparer;

impl SnapshotComparer {
    pub fn compare_files(
        &self,
        old_snapshot_path: &std::path::Path,
        new_snapshot_path: &std::path::Path,
    ) -> AppResult<SnapshotCompareReport> {
        let old_snapshot = load_snapshot(old_snapshot_path)?;
        let new_snapshot = load_snapshot(new_snapshot_path)?;
        Ok(self.compare(&old_snapshot, &new_snapshot))
    }

    pub fn compare(
        &self,
        old_snapshot: &GameSnapshot,
        new_snapshot: &GameSnapshot,
    ) -> SnapshotCompareReport {
        let old_by_path = old_snapshot
            .assets
            .iter()
            .map(|asset| (asset.path.as_str(), asset))
            .collect::<BTreeMap<_, _>>();
        let new_by_path = new_snapshot
            .assets
            .iter()
            .map(|asset| (asset.path.as_str(), asset))
            .collect::<BTreeMap<_, _>>();

        let mut added_assets = Vec::new();
        let mut removed_assets = Vec::new();
        let mut changed_assets = Vec::new();
        let mut unchanged_assets = 0usize;

        for (path, old_asset) in &old_by_path {
            match new_by_path.get(path) {
                Some(new_asset) => {
                    let changed_fields = changed_fields(old_asset, new_asset);
                    if changed_fields.is_empty() {
                        unchanged_assets += 1;
                    } else {
                        changed_assets.push(build_changed_asset_entry(
                            old_asset,
                            new_asset,
                            changed_fields,
                        ));
                    }
                }
                None => {
                    removed_assets.push(build_removed_asset_entry(old_asset));
                }
            }
        }

        for (path, new_asset) in &new_by_path {
            if !old_by_path.contains_key(path) {
                added_assets.push(build_added_asset_entry(new_asset));
            }
        }

        let candidate_mapping_changes =
            build_candidate_mapping_changes(&removed_assets, &added_assets);
        let changed_breakdowns = changed_assets
            .iter()
            .map(|change| classify_changed_fields(&change.changed_fields))
            .collect::<Vec<_>>();
        let identity_changed_assets = changed_breakdowns
            .iter()
            .filter(|breakdown| !breakdown.identity_fields.is_empty())
            .count();
        let structural_changed_assets = changed_breakdowns
            .iter()
            .filter(|breakdown| !breakdown.structural_fields.is_empty())
            .count();
        let naming_only_changed_assets = changed_breakdowns
            .iter()
            .filter(|breakdown| breakdown.is_naming_only())
            .count();
        let cosmetic_only_changed_assets = changed_breakdowns
            .iter()
            .filter(|breakdown| breakdown.is_cosmetic_only())
            .count();
        let provenance_changed_assets = changed_breakdowns
            .iter()
            .filter(|breakdown| !breakdown.provenance_fields.is_empty())
            .count();
        let container_moved_assets = changed_breakdowns
            .iter()
            .filter(|breakdown| breakdown.has_container_movement())
            .count();
        let ambiguous_candidate_mapping_changes = candidate_mapping_changes
            .iter()
            .filter(|candidate| candidate.ambiguous)
            .count();
        let high_confidence_candidate_mapping_changes = candidate_mapping_changes
            .iter()
            .filter(|candidate| candidate.confidence >= 0.85)
            .count();
        let lineage_rename_or_repath_assets = lineage_count(
            &changed_assets,
            &candidate_mapping_changes,
            AssetLineageKind::RenameOrRepath,
        );
        let lineage_container_movement_assets = lineage_count(
            &changed_assets,
            &candidate_mapping_changes,
            AssetLineageKind::ContainerMovement,
        );
        let lineage_layout_drift_assets = lineage_count(
            &changed_assets,
            &candidate_mapping_changes,
            AssetLineageKind::LayoutDrift,
        );
        let lineage_replacement_assets = lineage_count(
            &changed_assets,
            &candidate_mapping_changes,
            AssetLineageKind::Replacement,
        );
        let lineage_ambiguous_assets = lineage_count(
            &changed_assets,
            &candidate_mapping_changes,
            AssetLineageKind::Ambiguous,
        );
        let lineage_insufficient_evidence_assets = lineage_count(
            &changed_assets,
            &candidate_mapping_changes,
            AssetLineageKind::InsufficientEvidence,
        );
        let layout_changed_assets = changed_breakdowns
            .iter()
            .filter(|breakdown| !breakdown.layout_fields.is_empty())
            .count();
        let summary = SnapshotCompareSummary {
            total_old_assets: old_snapshot.asset_count,
            total_new_assets: new_snapshot.asset_count,
            unchanged_assets,
            added_assets: added_assets.len(),
            removed_assets: removed_assets.len(),
            changed_assets: changed_assets.len(),
            candidate_mapping_changes: candidate_mapping_changes.len(),
            identity_changed_assets,
            layout_changed_assets,
            structural_changed_assets,
            naming_only_changed_assets,
            cosmetic_only_changed_assets,
            provenance_changed_assets,
            container_moved_assets,
            lineage_rename_or_repath_assets,
            lineage_container_movement_assets,
            lineage_layout_drift_assets,
            lineage_replacement_assets,
            lineage_ambiguous_assets,
            lineage_insufficient_evidence_assets,
            ambiguous_candidate_mapping_changes,
            high_confidence_candidate_mapping_changes,
        };
        let scope = build_compare_scope_context(old_snapshot, new_snapshot, &summary);

        SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: old_snapshot.version_id.clone(),
                source_root: old_snapshot.source_root.clone(),
                asset_count: old_snapshot.asset_count,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: new_snapshot.version_id.clone(),
                source_root: new_snapshot.source_root.clone(),
                asset_count: new_snapshot.asset_count,
            },
            scope,
            summary,
            added_assets,
            removed_assets,
            changed_assets,
            candidate_mapping_changes,
        }
    }
}

fn build_compare_scope_context(
    old_snapshot: &GameSnapshot,
    new_snapshot: &GameSnapshot,
    summary: &SnapshotCompareSummary,
) -> SnapshotCompareScopeContext {
    let old_scope = assess_snapshot_scope(old_snapshot);
    let new_scope = assess_snapshot_scope(new_snapshot);
    let old_quality = summarize_snapshot_capture_quality(old_snapshot);
    let new_quality = summarize_snapshot_capture_quality(new_snapshot);
    let old_low_signal = old_scope.is_low_signal_for_character_analysis();
    let new_low_signal = new_scope.is_low_signal_for_character_analysis();

    let mut notes = Vec::new();
    if let Some(note) = old_scope.note.as_deref() {
        notes.push(format!("old snapshot {}: {note}", old_snapshot.version_id));
    }
    notes.push(format!(
        "old snapshot {} quality: launcher={} reuse={} matches_snapshot={} manifest_coverage=resources:{} matched:{} unmatched_snapshot_assets:{} hash_coverage=asset_hashes:{}/{} any_hashes:{}/{} signatures:{}/{} asset_enrichment=source_context:{}/{} rich_metadata:{}/{} enriched_assets:{}/{} extractor_records={} extractor_support=hashes:{} source_context:{} rich_metadata:{} schema={} inventory_version={} alignment={} launcher_matches_inventory={}",
        old_snapshot.version_id,
        old_quality.launcher_detected_version.as_deref().unwrap_or("missing"),
        old_quality.launcher_reuse_version.as_deref().unwrap_or("-"),
        old_quality
            .launcher_version_matches_snapshot
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown"),
        old_quality.manifest_resource_count,
        old_quality.manifest_matched_assets,
        old_quality.manifest_unmatched_snapshot_assets,
        old_quality.assets_with_asset_hash,
        old_quality.asset_count,
        old_quality.assets_with_any_hash,
        old_quality.asset_count,
        old_quality.assets_with_signature,
        old_quality.asset_count,
        old_quality.assets_with_source_context,
        old_quality.asset_count,
        old_quality.assets_with_rich_metadata,
        old_quality.asset_count,
        old_quality.meaningfully_enriched_assets,
        old_quality.asset_count,
        old_quality.extractor_record_count,
        old_quality.extractor_records_with_hashes,
        old_quality.extractor_records_with_source_context,
        old_quality.extractor_records_with_rich_metadata,
        old_quality
            .extractor_inventory_schema_version
            .as_deref()
            .unwrap_or("-"),
        old_quality
            .extractor_inventory_version_id
            .as_deref()
            .unwrap_or("-"),
        old_quality.extractor_inventory_alignment_status(),
        old_quality
            .launcher_version_matches_extractor_inventory
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown"),
    ));
    if let Some(note) = new_scope.note.as_deref() {
        notes.push(format!("new snapshot {}: {note}", new_snapshot.version_id));
    }
    notes.push(format!(
        "new snapshot {} quality: launcher={} reuse={} matches_snapshot={} manifest_coverage=resources:{} matched:{} unmatched_snapshot_assets:{} hash_coverage=asset_hashes:{}/{} any_hashes:{}/{} signatures:{}/{} asset_enrichment=source_context:{}/{} rich_metadata:{}/{} enriched_assets:{}/{} extractor_records={} extractor_support=hashes:{} source_context:{} rich_metadata:{} schema={} inventory_version={} alignment={} launcher_matches_inventory={}",
        new_snapshot.version_id,
        new_quality.launcher_detected_version.as_deref().unwrap_or("missing"),
        new_quality.launcher_reuse_version.as_deref().unwrap_or("-"),
        new_quality
            .launcher_version_matches_snapshot
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown"),
        new_quality.manifest_resource_count,
        new_quality.manifest_matched_assets,
        new_quality.manifest_unmatched_snapshot_assets,
        new_quality.assets_with_asset_hash,
        new_quality.asset_count,
        new_quality.assets_with_any_hash,
        new_quality.asset_count,
        new_quality.assets_with_signature,
        new_quality.asset_count,
        new_quality.assets_with_source_context,
        new_quality.asset_count,
        new_quality.assets_with_rich_metadata,
        new_quality.asset_count,
        new_quality.meaningfully_enriched_assets,
        new_quality.asset_count,
        new_quality.extractor_record_count,
        new_quality.extractor_records_with_hashes,
        new_quality.extractor_records_with_source_context,
        new_quality.extractor_records_with_rich_metadata,
        new_quality
            .extractor_inventory_schema_version
            .as_deref()
            .unwrap_or("-"),
        new_quality
            .extractor_inventory_version_id
            .as_deref()
            .unwrap_or("-"),
        new_quality.extractor_inventory_alignment_status(),
        new_quality
            .launcher_version_matches_extractor_inventory
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown"),
    ));
    if old_scope.observed_fallback_used || new_scope.observed_fallback_used {
        notes.push(
            "scope metadata was partially inferred from observed paths because explicit scope fields were missing"
                .to_string(),
        );
    }
    if old_scope.is_low_signal_for_character_analysis()
        || new_scope.is_low_signal_for_character_analysis()
    {
        notes.push(
            "compare scope includes shallow filesystem inventory or low-coverage/low-enrichment extractor snapshots; deep character-level interpretation is limited"
                .to_string(),
        );
    }
    if has_shallow_hash_or_manifest_only_support(&old_scope, &old_quality)
        || has_shallow_hash_or_manifest_only_support(&new_scope, &new_quality)
    {
        notes.push(
            "manifest/hash coverage may be present for this compare pair, but shallow coverage is not equivalent to rich asset-level enrichment"
                .to_string(),
        );
    }
    if old_quality.has_extractor_alignment_caution()
        || new_quality.has_extractor_alignment_caution()
    {
        let mut cautions = Vec::new();
        if let Some(reason) = old_quality.extractor_alignment_reason() {
            cautions.push(format!("old={reason}"));
        }
        if let Some(reason) = new_quality.extractor_alignment_reason() {
            cautions.push(format!("new={reason}"));
        }
        notes.push(format!(
            "extractor alignment caution: {}",
            cautions.join(" | ")
        ));
    } else if compare_evidence_tier(&old_scope, &old_quality) == "extractor_backed_rich"
        && compare_evidence_tier(&new_scope, &new_quality) == "extractor_backed_rich"
    {
        notes.push(
            "compare confidence posture: both snapshots are extractor-backed, version-aligned, and asset-enriched enough to support higher-signal structural review than shallow manifest/hash support alone"
                .to_string(),
        );
    }
    notes.push(format!(
        "compare evidence posture: old={} new={}",
        compare_evidence_tier(&old_scope, &old_quality),
        compare_evidence_tier(&new_scope, &new_quality)
    ));
    let scope_narrowing_note = scope_narrowing_interpretation_note(&old_scope, &new_scope, summary);
    if let Some(note) = scope_narrowing_note.as_deref() {
        notes.push(note.to_string());
    }

    SnapshotCompareScopeContext {
        old_snapshot: SnapshotCompareScopeInfo {
            acquisition_kind: old_scope.acquisition_kind.clone(),
            capture_mode: old_scope.capture_mode.clone(),
            launcher_detected_version: old_quality.launcher_detected_version,
            launcher_reuse_version: old_quality.launcher_reuse_version,
            launcher_version_matches_snapshot: old_quality.launcher_version_matches_snapshot,
            manifest_resource_count: old_quality.manifest_resource_count,
            manifest_matched_assets: old_quality.manifest_matched_assets,
            manifest_unmatched_snapshot_assets: old_quality.manifest_unmatched_snapshot_assets,
            assets_with_asset_hash: old_quality.assets_with_asset_hash,
            assets_with_any_hash: old_quality.assets_with_any_hash,
            assets_with_signature: old_quality.assets_with_signature,
            assets_with_source_context: old_quality.assets_with_source_context,
            assets_with_rich_metadata: old_quality.assets_with_rich_metadata,
            meaningfully_enriched_assets: old_quality.meaningfully_enriched_assets,
            extractor_record_count: old_quality.extractor_record_count,
            extractor_records_with_hashes: old_quality.extractor_records_with_hashes,
            extractor_records_with_source_context: old_quality
                .extractor_records_with_source_context,
            extractor_records_with_rich_metadata: old_quality.extractor_records_with_rich_metadata,
            extractor_inventory_schema_version: old_quality.extractor_inventory_schema_version,
            extractor_inventory_version_id: old_quality.extractor_inventory_version_id,
            extractor_inventory_version_matches_snapshot: old_quality
                .extractor_inventory_version_matches_snapshot,
            launcher_version_matches_extractor_inventory: old_quality
                .launcher_version_matches_extractor_inventory,
            mostly_install_or_package_level: old_scope.mostly_install_or_package_level,
            meaningful_content_coverage: old_scope.meaningful_content_coverage,
            meaningful_character_coverage: old_scope.meaningful_character_coverage,
            meaningful_asset_record_enrichment: old_scope.meaningful_asset_record_enrichment,
            content_like_path_count: old_scope.coverage.content_like_path_count,
            character_path_count: old_scope.coverage.character_path_count,
            non_content_path_count: old_scope.coverage.non_content_path_count,
            low_signal_for_character_analysis: old_low_signal,
            note: old_scope.note.clone(),
        },
        new_snapshot: SnapshotCompareScopeInfo {
            acquisition_kind: new_scope.acquisition_kind.clone(),
            capture_mode: new_scope.capture_mode.clone(),
            launcher_detected_version: new_quality.launcher_detected_version,
            launcher_reuse_version: new_quality.launcher_reuse_version,
            launcher_version_matches_snapshot: new_quality.launcher_version_matches_snapshot,
            manifest_resource_count: new_quality.manifest_resource_count,
            manifest_matched_assets: new_quality.manifest_matched_assets,
            manifest_unmatched_snapshot_assets: new_quality.manifest_unmatched_snapshot_assets,
            assets_with_asset_hash: new_quality.assets_with_asset_hash,
            assets_with_any_hash: new_quality.assets_with_any_hash,
            assets_with_signature: new_quality.assets_with_signature,
            assets_with_source_context: new_quality.assets_with_source_context,
            assets_with_rich_metadata: new_quality.assets_with_rich_metadata,
            meaningfully_enriched_assets: new_quality.meaningfully_enriched_assets,
            extractor_record_count: new_quality.extractor_record_count,
            extractor_records_with_hashes: new_quality.extractor_records_with_hashes,
            extractor_records_with_source_context: new_quality
                .extractor_records_with_source_context,
            extractor_records_with_rich_metadata: new_quality.extractor_records_with_rich_metadata,
            extractor_inventory_schema_version: new_quality.extractor_inventory_schema_version,
            extractor_inventory_version_id: new_quality.extractor_inventory_version_id,
            extractor_inventory_version_matches_snapshot: new_quality
                .extractor_inventory_version_matches_snapshot,
            launcher_version_matches_extractor_inventory: new_quality
                .launcher_version_matches_extractor_inventory,
            mostly_install_or_package_level: new_scope.mostly_install_or_package_level,
            meaningful_content_coverage: new_scope.meaningful_content_coverage,
            meaningful_character_coverage: new_scope.meaningful_character_coverage,
            meaningful_asset_record_enrichment: new_scope.meaningful_asset_record_enrichment,
            content_like_path_count: new_scope.coverage.content_like_path_count,
            character_path_count: new_scope.coverage.character_path_count,
            non_content_path_count: new_scope.coverage.non_content_path_count,
            low_signal_for_character_analysis: new_low_signal,
            note: new_scope.note.clone(),
        },
        low_signal_compare: old_low_signal || new_low_signal,
        scope_narrowing_detected: scope_narrowing_note.is_some(),
        scope_induced_removals_likely: scope_narrowing_note
            .as_deref()
            .is_some_and(|note| note.contains("scope-induced removal caution")),
        notes,
    }
}

fn local_capture_scope_rank(capture_mode: Option<&str>) -> Option<u8> {
    match capture_mode {
        Some("local_filesystem_inventory") => Some(3),
        Some("local_filesystem_inventory_content_focused") => Some(2),
        Some("local_filesystem_inventory_character_focused") => Some(1),
        _ => None,
    }
}

fn scope_narrowing_interpretation_note(
    old_scope: &crate::snapshot::SnapshotScopeAssessment,
    new_scope: &crate::snapshot::SnapshotScopeAssessment,
    summary: &SnapshotCompareSummary,
) -> Option<String> {
    let old_rank = local_capture_scope_rank(old_scope.capture_mode.as_deref())?;
    let new_rank = local_capture_scope_rank(new_scope.capture_mode.as_deref())?;
    if new_rank >= old_rank || summary.removed_assets == 0 {
        return None;
    }

    let old_mode = old_scope.capture_mode.as_deref().unwrap_or("unknown");
    let new_mode = new_scope.capture_mode.as_deref().unwrap_or("unknown");
    let removed_only_delta = summary.added_assets == 0
        && summary.changed_assets == 0
        && summary.candidate_mapping_changes == 0;
    let removal_share = if summary.total_old_assets == 0 {
        0.0
    } else {
        summary.removed_assets as f32 / summary.total_old_assets as f32
    };
    let new_zero_visibility = summary.total_new_assets == 0;

    if removed_only_delta || removal_share >= 0.5 || new_zero_visibility {
        let mut note = format!(
            "scope-induced removal caution: new snapshot capture mode '{}' is narrower than old '{}'; {} removed assets likely reflect scope filtering rather than true game-version drift",
            new_mode, old_mode, summary.removed_assets
        );
        if new_zero_visibility {
            note.push_str(", and the narrower scope yielded 0 visible assets");
        }
        Some(note)
    } else {
        Some(format!(
            "scope-narrowing note: new snapshot capture mode '{}' is narrower than old '{}'; review removals conservatively because part of the delta may be scope-induced rather than true game-version drift",
            new_mode, old_mode
        ))
    }
}

fn compare_evidence_tier(
    scope: &crate::snapshot::SnapshotScopeAssessment,
    quality: &crate::snapshot::SnapshotCaptureQualitySummary,
) -> &'static str {
    snapshot_evidence_posture_from_parts(scope, quality).machine_label()
}

fn has_shallow_hash_or_manifest_only_support(
    scope: &crate::snapshot::SnapshotScopeAssessment,
    quality: &crate::snapshot::SnapshotCaptureQualitySummary,
) -> bool {
    !scope.meaningful_asset_record_enrichment
        && (quality.manifest_resource_count > 0
            || quality.assets_with_asset_hash > 0
            || quality.assets_with_any_hash > 0
            || quality.assets_with_signature > 0)
}

pub fn load_snapshot_compare_report(path: &Path) -> AppResult<SnapshotCompareReport> {
    let report: SnapshotCompareReport = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(report)
}

impl From<&SnapshotAsset> for SnapshotAssetSummary {
    fn from(value: &SnapshotAsset) -> Self {
        Self {
            id: value.id.clone(),
            path: value.path.clone(),
            kind: value.kind.clone(),
            logical_name: value.metadata.logical_name.clone(),
            normalized_name: value.fingerprint.normalized_name.clone(),
            vertex_count: value.fingerprint.vertex_count,
            index_count: value.fingerprint.index_count,
            material_slots: value.fingerprint.material_slots,
            section_count: value.fingerprint.section_count,
            vertex_stride: value.fingerprint.vertex_stride,
            vertex_buffer_count: value.fingerprint.vertex_buffer_count,
            index_format: value.fingerprint.index_format.clone(),
            primitive_topology: value.fingerprint.primitive_topology.clone(),
            layout_markers: value.fingerprint.layout_markers.iter().cloned().collect(),
            internal_structure: value.fingerprint.internal_structure.clone(),
            asset_hash: value.hash_fields.asset_hash.clone(),
            shader_hash: value.hash_fields.shader_hash.clone(),
            signature: value.hash_fields.signature.clone(),
            tags: value.fingerprint.tags.clone(),
            source: value.source.clone(),
        }
    }
}

fn changed_fields(old_asset: &SnapshotAsset, new_asset: &SnapshotAsset) -> Vec<String> {
    let mut fields = Vec::new();

    push_changed_field(&mut fields, "kind", &old_asset.kind, &new_asset.kind);
    push_changed_field(
        &mut fields,
        "normalized_name",
        &old_asset.fingerprint.normalized_name,
        &new_asset.fingerprint.normalized_name,
    );
    push_changed_field(
        &mut fields,
        "logical_name",
        &old_asset.metadata.logical_name,
        &new_asset.metadata.logical_name,
    );
    push_changed_field(
        &mut fields,
        "vertex_count",
        &old_asset.metadata.vertex_count,
        &new_asset.metadata.vertex_count,
    );
    push_changed_field(
        &mut fields,
        "index_count",
        &old_asset.metadata.index_count,
        &new_asset.metadata.index_count,
    );
    push_changed_field(
        &mut fields,
        "material_slots",
        &old_asset.metadata.material_slots,
        &new_asset.metadata.material_slots,
    );
    push_changed_field(
        &mut fields,
        "section_count",
        &old_asset.metadata.section_count,
        &new_asset.metadata.section_count,
    );
    push_changed_field(
        &mut fields,
        "vertex_stride",
        &old_asset.fingerprint.vertex_stride,
        &new_asset.fingerprint.vertex_stride,
    );
    push_changed_field(
        &mut fields,
        "vertex_buffer_count",
        &old_asset.fingerprint.vertex_buffer_count,
        &new_asset.fingerprint.vertex_buffer_count,
    );
    push_changed_field(
        &mut fields,
        "index_format",
        &old_asset.fingerprint.index_format,
        &new_asset.fingerprint.index_format,
    );
    push_changed_field(
        &mut fields,
        "primitive_topology",
        &old_asset.fingerprint.primitive_topology,
        &new_asset.fingerprint.primitive_topology,
    );
    push_changed_field(
        &mut fields,
        "layout_markers",
        &old_asset.fingerprint.layout_markers,
        &new_asset.fingerprint.layout_markers,
    );
    push_changed_field(
        &mut fields,
        "internal_structure.section_labels",
        &old_asset.fingerprint.internal_structure.section_labels,
        &new_asset.fingerprint.internal_structure.section_labels,
    );
    push_changed_field(
        &mut fields,
        "internal_structure.buffer_roles",
        &old_asset.fingerprint.internal_structure.buffer_roles,
        &new_asset.fingerprint.internal_structure.buffer_roles,
    );
    push_changed_field(
        &mut fields,
        "internal_structure.binding_targets",
        &old_asset.fingerprint.internal_structure.binding_targets,
        &new_asset.fingerprint.internal_structure.binding_targets,
    );
    push_changed_field(
        &mut fields,
        "internal_structure.subresource_roles",
        &old_asset.fingerprint.internal_structure.subresource_roles,
        &new_asset.fingerprint.internal_structure.subresource_roles,
    );
    push_changed_field(
        &mut fields,
        "internal_structure.has_skeleton",
        &old_asset.fingerprint.internal_structure.has_skeleton,
        &new_asset.fingerprint.internal_structure.has_skeleton,
    );
    push_changed_field(
        &mut fields,
        "internal_structure.has_shapekey_data",
        &old_asset.fingerprint.internal_structure.has_shapekey_data,
        &new_asset.fingerprint.internal_structure.has_shapekey_data,
    );
    if old_asset.fingerprint.tags != new_asset.fingerprint.tags {
        fields.push("tags".to_string());
    }
    push_changed_field(
        &mut fields,
        "asset_hash",
        &old_asset.hash_fields.asset_hash,
        &new_asset.hash_fields.asset_hash,
    );
    push_changed_field(
        &mut fields,
        "shader_hash",
        &old_asset.hash_fields.shader_hash,
        &new_asset.hash_fields.shader_hash,
    );
    push_changed_field(
        &mut fields,
        "signature",
        &old_asset.hash_fields.signature,
        &new_asset.hash_fields.signature,
    );
    push_changed_field(
        &mut fields,
        "source_path",
        &old_asset.source.source_path,
        &new_asset.source.source_path,
    );
    push_changed_field(
        &mut fields,
        "container_path",
        &old_asset.source.container_path,
        &new_asset.source.container_path,
    );
    push_changed_field(
        &mut fields,
        "source_kind",
        &old_asset.source.source_kind,
        &new_asset.source.source_kind,
    );

    fields
}

#[derive(Debug, Default, Clone)]
struct FieldChangeBreakdown {
    identity_fields: Vec<String>,
    layout_fields: Vec<String>,
    structural_fields: Vec<String>,
    naming_fields: Vec<String>,
    cosmetic_fields: Vec<String>,
    provenance_fields: Vec<String>,
}

impl FieldChangeBreakdown {
    fn is_naming_only(&self) -> bool {
        !self.naming_fields.is_empty()
            && self.identity_fields.is_empty()
            && self.layout_fields.is_empty()
            && self.structural_fields.is_empty()
            && self.cosmetic_fields.is_empty()
    }

    fn is_cosmetic_only(&self) -> bool {
        !self.cosmetic_fields.is_empty()
            && self.identity_fields.is_empty()
            && self.layout_fields.is_empty()
            && self.structural_fields.is_empty()
            && self.naming_fields.is_empty()
            && self.provenance_fields.is_empty()
    }

    fn is_provenance_only(&self) -> bool {
        !self.provenance_fields.is_empty()
            && self.identity_fields.is_empty()
            && self.layout_fields.is_empty()
            && self.structural_fields.is_empty()
            && self.naming_fields.is_empty()
            && self.cosmetic_fields.is_empty()
    }

    fn has_container_movement(&self) -> bool {
        self.provenance_fields
            .iter()
            .any(|field| field.as_str() == "container_path")
    }

    fn has_hash_like_identity_change(&self) -> bool {
        self.identity_fields
            .iter()
            .any(|field| matches!(field.as_str(), "asset_hash" | "shader_hash" | "signature"))
    }

    fn has_layout_change(&self) -> bool {
        !self.layout_fields.is_empty()
    }
}

fn classify_changed_fields(fields: &[String]) -> FieldChangeBreakdown {
    let mut breakdown = FieldChangeBreakdown::default();

    for field in fields {
        if is_identity_field(field) {
            breakdown.identity_fields.push(field.clone());
        } else if is_layout_field(field) {
            breakdown.layout_fields.push(field.clone());
        } else if is_structural_field(field) {
            breakdown.structural_fields.push(field.clone());
        } else if is_naming_field(field) {
            breakdown.naming_fields.push(field.clone());
        } else if is_cosmetic_field(field) {
            breakdown.cosmetic_fields.push(field.clone());
        } else if is_provenance_field(field) {
            breakdown.provenance_fields.push(field.clone());
        }
    }

    breakdown
}

fn is_identity_field(field: &str) -> bool {
    matches!(field, "kind" | "asset_hash" | "shader_hash" | "signature")
}

fn is_structural_field(field: &str) -> bool {
    matches!(
        field,
        "vertex_count"
            | "index_count"
            | "material_slots"
            | "section_count"
            | "internal_structure.section_labels"
            | "internal_structure.subresource_roles"
            | "internal_structure.has_skeleton"
            | "internal_structure.has_shapekey_data"
    )
}

fn is_layout_field(field: &str) -> bool {
    matches!(
        field,
        "vertex_stride"
            | "vertex_buffer_count"
            | "index_format"
            | "primitive_topology"
            | "layout_markers"
            | "internal_structure.buffer_roles"
            | "internal_structure.binding_targets"
    )
}

fn is_naming_field(field: &str) -> bool {
    matches!(field, "normalized_name" | "logical_name")
}

fn is_cosmetic_field(field: &str) -> bool {
    matches!(field, "tags")
}

fn is_provenance_field(field: &str) -> bool {
    matches!(field, "source_path" | "container_path" | "source_kind")
}

fn push_changed_field<T>(fields: &mut Vec<String>, label: &str, old_value: &T, new_value: &T)
where
    T: PartialEq,
{
    if old_value != new_value {
        fields.push(label.to_string());
    }
}

fn build_changed_asset_entry(
    old_asset: &SnapshotAsset,
    new_asset: &SnapshotAsset,
    changed_fields: Vec<String>,
) -> SnapshotAssetChange {
    let breakdown = classify_changed_fields(&changed_fields);
    let structural_change = !breakdown.structural_fields.is_empty();
    let layout_change = breakdown.has_layout_change();
    let hash_like_identity_change = breakdown.has_hash_like_identity_change();
    let identity_only_change = !breakdown.identity_fields.is_empty()
        && breakdown.layout_fields.is_empty()
        && breakdown.structural_fields.is_empty()
        && breakdown.naming_fields.is_empty()
        && breakdown.cosmetic_fields.is_empty();
    let naming_only_change = breakdown.is_naming_only();
    let cosmetic_only_change = breakdown.is_cosmetic_only();
    let provenance_only_change = breakdown.is_provenance_only();
    let lineage = assess_changed_asset_lineage(
        &breakdown,
        structural_change,
        layout_change,
        hash_like_identity_change,
        identity_only_change,
        naming_only_change,
        cosmetic_only_change,
        provenance_only_change,
    );
    let probable_impact = if structural_change || layout_change || hash_like_identity_change {
        RiskLevel::High
    } else if identity_only_change {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };
    let crash_risk = if structural_change || layout_change || hash_like_identity_change {
        RiskLevel::High
    } else if identity_only_change {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };
    let mut reasons = Vec::new();

    if !breakdown.identity_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "identity_signal_changed".to_string(),
            message: format!(
                "identity signals changed for asset path {}: {}; hash/signature/kind-based targeting may need review",
                old_asset.path,
                breakdown.identity_fields.join(", ")
            ),
        });
    }
    if breakdown.has_layout_change() {
        reasons.push(SnapshotCompareReason {
            code: "buffer_layout_changed".to_string(),
            message: format!(
                "buffer/layout signals changed for asset path {}: {}; validate layout assumptions before remapping",
                old_asset.path,
                breakdown.layout_fields.join(", ")
            ),
        });
    }
    if !breakdown.structural_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "structural_layout_changed".to_string(),
            message: format!(
                "structural layout signals changed for asset path {}: {}; validate buffer/layout assumptions before remapping",
                old_asset.path,
                breakdown.structural_fields.join(", ")
            ),
        });
    }
    if naming_only_change {
        reasons.push(SnapshotCompareReason {
            code: "naming_only_change".to_string(),
            message: format!(
                "only naming fields changed for asset path {}; treat this as a rename/relabel signal, not layout evidence",
                old_asset.path
            ),
        });
    } else if !breakdown.naming_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "naming_signal_changed".to_string(),
            message: format!(
                "naming signals changed for asset path {}: {}",
                old_asset.path,
                breakdown.naming_fields.join(", ")
            ),
        });
    }
    if cosmetic_only_change {
        reasons.push(SnapshotCompareReason {
            code: "cosmetic_metadata_changed".to_string(),
            message: format!(
                "only cosmetic metadata changed for asset path {}; compare keeps this low-impact unless other signals disagree",
                old_asset.path
            ),
        });
    } else if !breakdown.cosmetic_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "cosmetic_metadata_changed".to_string(),
            message: format!(
                "cosmetic metadata changed for asset path {}: {}",
                old_asset.path,
                breakdown.cosmetic_fields.join(", ")
            ),
        });
    }
    if provenance_only_change {
        reasons.push(SnapshotCompareReason {
            code: "provenance_only_change".to_string(),
            message: format!(
                "only package/provenance metadata changed for asset path {}; treat this as a container-origin movement signal rather than a content identity change",
                old_asset.path
            ),
        });
    } else if !breakdown.provenance_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "provenance_signal_changed".to_string(),
            message: format!(
                "package/provenance metadata changed for asset path {}: {}",
                old_asset.path,
                breakdown.provenance_fields.join(", ")
            ),
        });
    }
    if breakdown.has_container_movement() {
        reasons.push(SnapshotCompareReason {
            code: "container_package_movement_detected".to_string(),
            message: format!(
                "container/package origin changed for asset path {}; remap review should consider package movement, not just path drift",
                old_asset.path
            ),
        });
    }

    reasons.extend(changed_fields.iter().map(|field| SnapshotCompareReason {
        code: format!("{field}_changed"),
        message: field_change_message(field, &old_asset.path),
    }));
    reasons.push(lineage_reason(&lineage));

    SnapshotAssetChange {
        change_type: SnapshotChangeType::Changed,
        old_asset: Some(SnapshotAssetSummary::from(old_asset)),
        new_asset: Some(SnapshotAssetSummary::from(new_asset)),
        changed_fields,
        probable_impact,
        crash_risk,
        suspected_mapping_change: structural_change
            || hash_like_identity_change
            || identity_only_change,
        lineage,
        reasons,
    }
}

fn field_change_message(field: &str, path: &str) -> String {
    match field {
        "asset_hash" | "shader_hash" | "signature" => format!(
            "{field} changed for asset path {path}; identity-based targeting may need review"
        ),
        "vertex_count" | "index_count" | "material_slots" | "section_count" => format!(
            "{field} changed for asset path {path}; mod mappings that assume the previous layout may need review"
        ),
        "vertex_stride"
        | "vertex_buffer_count"
        | "index_format"
        | "primitive_topology"
        | "layout_markers"
        | "internal_structure.buffer_roles"
        | "internal_structure.binding_targets" => format!(
            "{field} changed for asset path {path}; buffer/layout assumptions should be validated before remapping"
        ),
        "internal_structure.section_labels" | "internal_structure.subresource_roles" => format!(
            "{field} changed for asset path {path}; internal section/component layout likely drifted"
        ),
        "internal_structure.has_skeleton" | "internal_structure.has_shapekey_data" => format!(
            "{field} changed for asset path {path}; component presence markers changed and may reflect internal structure drift"
        ),
        "normalized_name" | "logical_name" => {
            format!("{field} changed for asset path {path}; this looks like a naming-level drift")
        }
        "tags" => format!(
            "{field} changed for asset path {path}; compare treats tag drift as low-signal metadata unless stronger evidence exists"
        ),
        "source_path" => format!(
            "{field} changed for asset path {path}; container/package origin may have moved even if the logical asset stayed related"
        ),
        "container_path" => {
            format!("{field} changed for asset path {path}; package origin moved across snapshots")
        }
        "source_kind" => {
            format!("{field} changed for asset path {path}; provenance classification changed")
        }
        _ => format!("{field} changed for asset path {path}"),
    }
}

fn build_removed_asset_entry(old_asset: &SnapshotAsset) -> SnapshotAssetChange {
    SnapshotAssetChange {
        change_type: SnapshotChangeType::Removed,
        old_asset: Some(SnapshotAssetSummary::from(old_asset)),
        new_asset: None,
        changed_fields: vec!["path_presence".to_string()],
        probable_impact: RiskLevel::Medium,
        crash_risk: RiskLevel::Medium,
        suspected_mapping_change: true,
        lineage: AssetLineageKind::InsufficientEvidence,
        reasons: vec![SnapshotCompareReason {
            code: "asset_removed".to_string(),
            message: format!(
                "asset path {} is missing in the new snapshot; mods targeting this path may fail until remapped",
                old_asset.path
            ),
        }],
    }
}

fn build_added_asset_entry(new_asset: &SnapshotAsset) -> SnapshotAssetChange {
    SnapshotAssetChange {
        change_type: SnapshotChangeType::Added,
        old_asset: None,
        new_asset: Some(SnapshotAssetSummary::from(new_asset)),
        changed_fields: vec!["path_presence".to_string()],
        probable_impact: RiskLevel::Low,
        crash_risk: RiskLevel::Low,
        suspected_mapping_change: false,
        lineage: AssetLineageKind::InsufficientEvidence,
        reasons: vec![SnapshotCompareReason {
            code: "asset_added".to_string(),
            message: format!(
                "asset path {} is new in the current snapshot",
                new_asset.path
            ),
        }],
    }
}

fn build_candidate_mapping_changes(
    removed_assets: &[SnapshotAssetChange],
    added_assets: &[SnapshotAssetChange],
) -> Vec<CandidateMappingChange> {
    const MIN_REPORTABLE_CONFIDENCE: f32 = 0.65;
    const MIN_AMBIGUITY_CONFIDENCE: f32 = 0.55;
    const AMBIGUITY_GAP_THRESHOLD: f32 = 0.08;

    removed_assets
        .iter()
        .filter_map(|removed| {
            let old_asset = removed.old_asset.as_ref()?;
            let mut scored_candidates = Vec::new();

            for added in added_assets {
                let new_asset = match added.new_asset.as_ref() {
                    Some(asset) => asset,
                    None => continue,
                };
                let (confidence, reasons) = score_candidate_mapping_change(old_asset, new_asset);
                if confidence < MIN_AMBIGUITY_CONFIDENCE {
                    continue;
                }

                scored_candidates.push(CandidateMappingChange {
                    old_asset: old_asset.clone(),
                    new_asset: new_asset.clone(),
                    confidence,
                    compatibility: RemapCompatibility::InsufficientEvidence,
                    lineage: AssetLineageKind::InsufficientEvidence,
                    reasons,
                    runner_up_confidence: None,
                    confidence_gap: None,
                    ambiguous: false,
                });
            }

            scored_candidates.sort_by(|left, right| {
                right
                    .confidence
                    .total_cmp(&left.confidence)
                    .then_with(|| left.new_asset.path.cmp(&right.new_asset.path))
            });

            if scored_candidates.is_empty() {
                return None;
            }

            let mut scored_candidates = scored_candidates;
            let mut best_candidate = scored_candidates.remove(0);
            let min_reportable_confidence = if has_reason_code(
                &best_candidate.reasons,
                "identity_conflict_detected",
            ) && !has_reportable_conflict_evidence(&best_candidate)
            {
                MIN_REPORTABLE_CONFIDENCE + 0.08
            } else {
                MIN_REPORTABLE_CONFIDENCE
            };
            if best_candidate.confidence < min_reportable_confidence {
                return None;
            }

            let runner_up_confidence = scored_candidates.first().map(|candidate| candidate.confidence);

            if let Some(runner_up_confidence) = runner_up_confidence {
                let confidence_gap = (best_candidate.confidence - runner_up_confidence).max(0.0);
                best_candidate.runner_up_confidence = Some(runner_up_confidence);
                best_candidate.confidence_gap = Some(confidence_gap);
                let ambiguity_gap_threshold = candidate_ambiguity_gap_threshold(&best_candidate)
                    .max(AMBIGUITY_GAP_THRESHOLD);
                if confidence_gap < ambiguity_gap_threshold {
                    best_candidate.ambiguous = true;
                    best_candidate.reasons.push(SnapshotCompareReason {
                        code: "ambiguous_runner_up".to_string(),
                        message: format!(
                            "runner-up candidate confidence {:.3} is within {:.3} of the best candidate; keep this remap under review",
                            runner_up_confidence, confidence_gap
                        ),
                    });
                }
            }

            best_candidate.compatibility = assess_candidate_compatibility(&best_candidate);
            best_candidate.lineage = assess_candidate_lineage(&best_candidate);
            best_candidate.reasons.push(lineage_reason(&best_candidate.lineage));
            best_candidate
                .reasons
                .push(compatibility_reason(&best_candidate.compatibility));

            Some(best_candidate)
        })
        .collect()
}

fn assess_candidate_compatibility(candidate: &CandidateMappingChange) -> RemapCompatibility {
    let has_strong_identity_anchor = has_reason_code(&candidate.reasons, "signature_exact")
        || has_reason_code(&candidate.reasons, "asset_hash_exact");
    let has_path_and_name_anchor = has_reason_code(&candidate.reasons, "normalized_name_exact")
        && has_reason_code(&candidate.reasons, "same_parent_directory");
    let has_structural_compatibility =
        has_reason_code(&candidate.reasons, "structural_layout_compatible");
    let has_layout_compatibility = has_reason_code(&candidate.reasons, "buffer_layout_compatible");
    let has_provenance_anchor = has_reason_code(&candidate.reasons, "container_path_exact")
        || has_reason_code(&candidate.reasons, "source_path_exact")
        || has_reason_code(&candidate.reasons, "source_kind_exact");
    let has_identity_conflict = has_reason_code(&candidate.reasons, "identity_conflict_detected")
        || has_reason_code(&candidate.reasons, "kind_mismatch")
        || has_reason_code(&candidate.reasons, "signature_mismatch");
    let has_structural_drift =
        has_reason_code(&candidate.reasons, "same_asset_but_structural_drift")
            || has_reason_code(&candidate.reasons, "buffer_layout_validation_needed");
    let weak_evidence = has_reason_code(&candidate.reasons, "weak_identity_evidence");
    let has_hard_layout_conflict = has_reason_code(&candidate.reasons, "index_format_mismatch")
        || has_reason_code(&candidate.reasons, "primitive_topology_mismatch");

    if has_identity_conflict {
        RemapCompatibility::IncompatibleBlocked
    } else if has_hard_layout_conflict && !has_strong_identity_anchor {
        RemapCompatibility::IncompatibleBlocked
    } else if has_structural_drift {
        RemapCompatibility::StructurallyRisky
    } else if candidate.ambiguous
        || candidate.confidence < 0.70
        || (weak_evidence && !has_strong_identity_anchor && !has_path_and_name_anchor)
    {
        RemapCompatibility::InsufficientEvidence
    } else if candidate.confidence >= 0.85
        && (has_strong_identity_anchor
            || (has_path_and_name_anchor
                && (has_structural_compatibility
                    || has_layout_compatibility
                    || has_provenance_anchor))
            || (has_provenance_anchor
                && (has_structural_compatibility || has_layout_compatibility)))
    {
        RemapCompatibility::LikelyCompatible
    } else if candidate.confidence >= 0.65
        && (has_strong_identity_anchor
            || has_path_and_name_anchor
            || has_structural_compatibility
            || has_layout_compatibility
            || has_provenance_anchor)
    {
        RemapCompatibility::CompatibleWithCaution
    } else {
        RemapCompatibility::InsufficientEvidence
    }
}

fn has_reportable_conflict_evidence(candidate: &CandidateMappingChange) -> bool {
    let has_name_anchor = has_reason_code(&candidate.reasons, "normalized_name_exact")
        || has_reason_code(&candidate.reasons, "normalized_name_token_overlap");
    let has_path_anchor = has_reason_code(&candidate.reasons, "same_parent_directory")
        || has_reason_code(&candidate.reasons, "path_token_overlap");
    let has_structural_anchor = has_reason_code(&candidate.reasons, "structural_layout_compatible")
        || has_reason_code(&candidate.reasons, "buffer_layout_compatible");

    candidate.confidence >= 0.65 && has_name_anchor && has_path_anchor && has_structural_anchor
}

fn compatibility_reason(compatibility: &RemapCompatibility) -> SnapshotCompareReason {
    match compatibility {
        RemapCompatibility::LikelyCompatible => SnapshotCompareReason {
            code: "compatibility_likely_compatible".to_string(),
            message: "repair-oriented compatibility assessment: likely compatible for a mapping-first fix path".to_string(),
        },
        RemapCompatibility::CompatibleWithCaution => SnapshotCompareReason {
            code: "compatibility_compatible_with_caution".to_string(),
            message: "repair-oriented compatibility assessment: candidate looks usable, but validate it before treating the remap as fully safe".to_string(),
        },
        RemapCompatibility::StructurallyRisky => SnapshotCompareReason {
            code: "compatibility_structurally_risky".to_string(),
            message: "repair-oriented compatibility assessment: same logical asset may exist, but structural drift makes remap-only repair risky".to_string(),
        },
        RemapCompatibility::IncompatibleBlocked => SnapshotCompareReason {
            code: "compatibility_incompatible_blocked".to_string(),
            message: "repair-oriented compatibility assessment: conflicting identity signals block this candidate from auto-remap use".to_string(),
        },
        RemapCompatibility::InsufficientEvidence => SnapshotCompareReason {
            code: "compatibility_insufficient_evidence".to_string(),
            message: "repair-oriented compatibility assessment: snapshot evidence is not strong enough to treat this candidate as repair-safe".to_string(),
        },
    }
}

fn lineage_reason(lineage: &AssetLineageKind) -> SnapshotCompareReason {
    SnapshotCompareReason {
        code: lineage.reason_code().to_string(),
        message: lineage.message().to_string(),
    }
}

fn assess_changed_asset_lineage(
    breakdown: &FieldChangeBreakdown,
    structural_change: bool,
    layout_change: bool,
    hash_like_identity_change: bool,
    identity_only_change: bool,
    naming_only_change: bool,
    cosmetic_only_change: bool,
    provenance_only_change: bool,
) -> AssetLineageKind {
    if provenance_only_change {
        AssetLineageKind::ContainerMovement
    } else if identity_only_change || hash_like_identity_change {
        AssetLineageKind::Replacement
    } else if layout_change || structural_change {
        if !breakdown.provenance_fields.is_empty() {
            AssetLineageKind::ContainerMovement
        } else {
            AssetLineageKind::LayoutDrift
        }
    } else if naming_only_change {
        AssetLineageKind::RenameOrRepath
    } else if cosmetic_only_change {
        AssetLineageKind::InsufficientEvidence
    } else {
        AssetLineageKind::InsufficientEvidence
    }
}

fn assess_candidate_lineage(candidate: &CandidateMappingChange) -> AssetLineageKind {
    let has_rename_or_repath_anchor =
        has_reason_code(&candidate.reasons, "likely_same_asset_repathed")
            || has_reason_code(&candidate.reasons, "normalized_name_exact");
    let has_container_movement_anchor = has_reason_code(&candidate.reasons, "package_origin_moved")
        || has_reason_code(&candidate.reasons, "container_path_mismatch")
        || has_reason_code(&candidate.reasons, "source_path_mismatch");
    let has_layout_drift_anchor =
        has_reason_code(&candidate.reasons, "same_asset_but_structural_drift")
            || has_reason_code(&candidate.reasons, "buffer_layout_validation_needed");
    let has_replacement_anchor = has_reason_code(&candidate.reasons, "identity_conflict_detected")
        || has_reason_code(&candidate.reasons, "kind_mismatch")
        || has_reason_code(&candidate.reasons, "signature_mismatch");

    if candidate.ambiguous {
        AssetLineageKind::Ambiguous
    } else if has_replacement_anchor {
        AssetLineageKind::Replacement
    } else if has_container_movement_anchor {
        AssetLineageKind::ContainerMovement
    } else if has_layout_drift_anchor {
        AssetLineageKind::LayoutDrift
    } else if has_rename_or_repath_anchor {
        AssetLineageKind::RenameOrRepath
    } else {
        AssetLineageKind::InsufficientEvidence
    }
}

fn lineage_count(
    changed_assets: &[SnapshotAssetChange],
    candidates: &[CandidateMappingChange],
    lineage: AssetLineageKind,
) -> usize {
    changed_assets
        .iter()
        .map(|change| &change.lineage)
        .chain(candidates.iter().map(|candidate| &candidate.lineage))
        .filter(|current| **current == lineage)
        .count()
}

fn score_candidate_mapping_change(
    old_asset: &SnapshotAssetSummary,
    new_asset: &SnapshotAssetSummary,
) -> (f32, Vec<SnapshotCompareReason>) {
    let mut confidence = 0.0;
    let mut reasons = Vec::new();
    let mut matched_identity_fields = Vec::new();
    let mut matched_layout_fields = Vec::new();
    let mut matched_structural_fields = Vec::new();
    let mut matched_provenance_fields = Vec::new();
    let mut conflicting_identity_fields = Vec::new();
    let mut conflicting_layout_fields = Vec::new();
    let mut conflicting_structural_fields = Vec::new();
    let mut conflicting_provenance_fields = Vec::new();

    if old_asset.kind.is_some() && old_asset.kind == new_asset.kind {
        confidence += 0.16;
        matched_identity_fields.push("kind");
        reasons.push(SnapshotCompareReason {
            code: "kind_exact".to_string(),
            message: format!(
                "asset kind matched exactly: {}",
                old_asset.kind.as_deref().unwrap_or("unknown")
            ),
        });
    } else if old_asset.kind.is_some() && new_asset.kind.is_some() {
        confidence -= 0.22;
        conflicting_identity_fields.push("kind");
        reasons.push(SnapshotCompareReason {
            code: "kind_mismatch".to_string(),
            message: format!(
                "asset kind changed from {} to {}; treat this candidate conservatively",
                old_asset.kind.as_deref().unwrap_or("unknown"),
                new_asset.kind.as_deref().unwrap_or("unknown")
            ),
        });
    }

    if old_asset.signature.is_some() && old_asset.signature == new_asset.signature {
        confidence += 0.24;
        matched_identity_fields.push("signature");
        reasons.push(SnapshotCompareReason {
            code: "signature_exact".to_string(),
            message: "asset signature matched exactly".to_string(),
        });
    } else if old_asset.signature.is_some() && new_asset.signature.is_some() {
        confidence -= 0.28;
        conflicting_identity_fields.push("signature");
        reasons.push(SnapshotCompareReason {
            code: "signature_mismatch".to_string(),
            message: "asset signature disagreed despite path/name similarity".to_string(),
        });
    }

    if old_asset.asset_hash.is_some() && old_asset.asset_hash == new_asset.asset_hash {
        confidence += 0.18;
        matched_identity_fields.push("asset_hash");
        reasons.push(SnapshotCompareReason {
            code: "asset_hash_exact".to_string(),
            message: "asset hash matched exactly".to_string(),
        });
    } else if old_asset.asset_hash.is_some() && new_asset.asset_hash.is_some() {
        confidence -= 0.04;
        reasons.push(SnapshotCompareReason {
            code: "asset_hash_mismatch".to_string(),
            message:
                "asset hash changed across the candidate remap; this affects confidence but can still happen on true version-to-version replacements"
                    .to_string(),
        });
    }

    if old_asset.shader_hash.is_some() && old_asset.shader_hash == new_asset.shader_hash {
        confidence += 0.08;
        matched_identity_fields.push("shader_hash");
        reasons.push(SnapshotCompareReason {
            code: "shader_hash_exact".to_string(),
            message: "shader hash matched exactly".to_string(),
        });
    } else if old_asset.shader_hash.is_some() && new_asset.shader_hash.is_some() {
        confidence -= 0.03;
        reasons.push(SnapshotCompareReason {
            code: "shader_hash_mismatch".to_string(),
            message:
                "shader hash changed; shader-bound remaps need review but this does not automatically invalidate the candidate"
                    .to_string(),
        });
    }

    if old_asset.vertex_stride.is_some() && old_asset.vertex_stride == new_asset.vertex_stride {
        confidence += 0.10;
        matched_layout_fields.push("vertex_stride");
        reasons.push(SnapshotCompareReason {
            code: "vertex_stride_exact".to_string(),
            message: "vertex stride matched exactly".to_string(),
        });
    } else if let Some((penalty, message)) = structural_mismatch_penalty(
        "vertex_stride",
        old_asset.vertex_stride,
        new_asset.vertex_stride,
    ) {
        confidence -= penalty;
        conflicting_layout_fields.push("vertex_stride");
        reasons.push(SnapshotCompareReason {
            code: "vertex_stride_mismatch".to_string(),
            message,
        });
    }

    if old_asset.vertex_buffer_count.is_some()
        && old_asset.vertex_buffer_count == new_asset.vertex_buffer_count
    {
        confidence += 0.07;
        matched_layout_fields.push("vertex_buffer_count");
        reasons.push(SnapshotCompareReason {
            code: "vertex_buffer_count_exact".to_string(),
            message: "vertex buffer count matched exactly".to_string(),
        });
    } else if let Some((penalty, message)) = structural_mismatch_penalty(
        "vertex_buffer_count",
        old_asset.vertex_buffer_count,
        new_asset.vertex_buffer_count,
    ) {
        confidence -= penalty;
        conflicting_layout_fields.push("vertex_buffer_count");
        reasons.push(SnapshotCompareReason {
            code: "vertex_buffer_count_mismatch".to_string(),
            message,
        });
    }

    if old_asset.index_format.is_some() && old_asset.index_format == new_asset.index_format {
        confidence += 0.06;
        matched_layout_fields.push("index_format");
        reasons.push(SnapshotCompareReason {
            code: "index_format_exact".to_string(),
            message: "index format matched exactly".to_string(),
        });
    } else if old_asset.index_format.is_some() && new_asset.index_format.is_some() {
        confidence -= 0.08;
        conflicting_layout_fields.push("index_format");
        reasons.push(SnapshotCompareReason {
            code: "index_format_mismatch".to_string(),
            message: "index format changed across the candidate remap".to_string(),
        });
    }

    if old_asset.primitive_topology.is_some()
        && old_asset.primitive_topology == new_asset.primitive_topology
    {
        confidence += 0.05;
        matched_layout_fields.push("primitive_topology");
        reasons.push(SnapshotCompareReason {
            code: "primitive_topology_exact".to_string(),
            message: "primitive topology matched exactly".to_string(),
        });
    } else if old_asset.primitive_topology.is_some() && new_asset.primitive_topology.is_some() {
        confidence -= 0.06;
        conflicting_layout_fields.push("primitive_topology");
        reasons.push(SnapshotCompareReason {
            code: "primitive_topology_mismatch".to_string(),
            message: "primitive topology changed across the candidate remap".to_string(),
        });
    }

    if old_asset.source.container_path.is_some()
        && old_asset.source.container_path == new_asset.source.container_path
    {
        confidence += 0.05;
        matched_provenance_fields.push("container_path");
        reasons.push(SnapshotCompareReason {
            code: "container_path_exact".to_string(),
            message: "container/package origin matched exactly".to_string(),
        });
    } else if old_asset.source.container_path.is_some() && new_asset.source.container_path.is_some()
    {
        confidence -= 0.02;
        conflicting_provenance_fields.push("container_path");
        reasons.push(SnapshotCompareReason {
            code: "container_path_mismatch".to_string(),
            message: "container/package origin changed across the candidate remap".to_string(),
        });
    }

    if old_asset.source.source_path.is_some()
        && old_asset.source.source_path == new_asset.source.source_path
    {
        confidence += 0.04;
        matched_provenance_fields.push("source_path");
        reasons.push(SnapshotCompareReason {
            code: "source_path_exact".to_string(),
            message: "source path matched exactly".to_string(),
        });
    } else if old_asset.source.source_path.is_some() && new_asset.source.source_path.is_some() {
        confidence -= 0.01;
        conflicting_provenance_fields.push("source_path");
        reasons.push(SnapshotCompareReason {
            code: "source_path_mismatch".to_string(),
            message: "source path changed across the candidate remap".to_string(),
        });
    }

    if old_asset.source.source_kind.is_some()
        && old_asset.source.source_kind == new_asset.source.source_kind
    {
        confidence += 0.02;
        matched_provenance_fields.push("source_kind");
        reasons.push(SnapshotCompareReason {
            code: "source_kind_exact".to_string(),
            message: "source kind matched exactly".to_string(),
        });
    } else if old_asset.source.source_kind.is_some() && new_asset.source.source_kind.is_some() {
        confidence -= 0.02;
        conflicting_provenance_fields.push("source_kind");
        reasons.push(SnapshotCompareReason {
            code: "source_kind_mismatch".to_string(),
            message: "source kind changed across the candidate remap".to_string(),
        });
    }

    let layout_overlap = tag_overlap(&old_asset.layout_markers, &new_asset.layout_markers);
    if !old_asset.layout_markers.is_empty() && old_asset.layout_markers == new_asset.layout_markers
    {
        confidence += 0.08;
        matched_layout_fields.push("layout_markers");
        reasons.push(SnapshotCompareReason {
            code: "layout_markers_exact".to_string(),
            message: "layout markers matched exactly".to_string(),
        });
    } else if layout_overlap > 0.0 {
        confidence += layout_overlap * 0.06;
        matched_layout_fields.push("layout_markers");
        reasons.push(SnapshotCompareReason {
            code: "layout_markers_overlap".to_string(),
            message: format!("layout marker overlap score: {layout_overlap:.3}"),
        });
    } else if !old_asset.layout_markers.is_empty() && !new_asset.layout_markers.is_empty() {
        confidence -= 0.05;
        conflicting_layout_fields.push("layout_markers");
        reasons.push(SnapshotCompareReason {
            code: "layout_markers_mismatch".to_string(),
            message: "layout markers were disjoint across the candidate remap".to_string(),
        });
    }

    let section_label_overlap = tag_overlap(
        &old_asset.internal_structure.section_labels,
        &new_asset.internal_structure.section_labels,
    );
    if !old_asset.internal_structure.section_labels.is_empty()
        && old_asset.internal_structure.section_labels
            == new_asset.internal_structure.section_labels
    {
        confidence += 0.06;
        matched_structural_fields.push("internal_structure.section_labels");
        reasons.push(SnapshotCompareReason {
            code: "internal_section_labels_exact".to_string(),
            message: "internal section labels matched exactly".to_string(),
        });
    } else if section_label_overlap > 0.0 {
        confidence += section_label_overlap * 0.05;
        matched_structural_fields.push("internal_structure.section_labels");
        reasons.push(SnapshotCompareReason {
            code: "internal_section_labels_overlap".to_string(),
            message: format!("internal section label overlap score: {section_label_overlap:.3}"),
        });
    } else if !old_asset.internal_structure.section_labels.is_empty()
        && !new_asset.internal_structure.section_labels.is_empty()
    {
        confidence -= 0.04;
        conflicting_structural_fields.push("internal_structure.section_labels");
        reasons.push(SnapshotCompareReason {
            code: "internal_section_labels_mismatch".to_string(),
            message: "internal section labels were disjoint across the candidate remap".to_string(),
        });
    }

    let buffer_role_overlap = tag_overlap(
        &old_asset.internal_structure.buffer_roles,
        &new_asset.internal_structure.buffer_roles,
    );
    if !old_asset.internal_structure.buffer_roles.is_empty()
        && old_asset.internal_structure.buffer_roles == new_asset.internal_structure.buffer_roles
    {
        confidence += 0.05;
        matched_layout_fields.push("internal_structure.buffer_roles");
        reasons.push(SnapshotCompareReason {
            code: "internal_buffer_roles_exact".to_string(),
            message: "internal buffer roles matched exactly".to_string(),
        });
    } else if buffer_role_overlap > 0.0 {
        confidence += buffer_role_overlap * 0.04;
        matched_layout_fields.push("internal_structure.buffer_roles");
        reasons.push(SnapshotCompareReason {
            code: "internal_buffer_roles_overlap".to_string(),
            message: format!("internal buffer role overlap score: {buffer_role_overlap:.3}"),
        });
    } else if !old_asset.internal_structure.buffer_roles.is_empty()
        && !new_asset.internal_structure.buffer_roles.is_empty()
    {
        confidence -= 0.04;
        conflicting_layout_fields.push("internal_structure.buffer_roles");
        reasons.push(SnapshotCompareReason {
            code: "internal_buffer_roles_mismatch".to_string(),
            message: "internal buffer roles were disjoint across the candidate remap".to_string(),
        });
    }

    let binding_target_overlap = tag_overlap(
        &old_asset.internal_structure.binding_targets,
        &new_asset.internal_structure.binding_targets,
    );
    if !old_asset.internal_structure.binding_targets.is_empty()
        && old_asset.internal_structure.binding_targets
            == new_asset.internal_structure.binding_targets
    {
        confidence += 0.05;
        matched_layout_fields.push("internal_structure.binding_targets");
        reasons.push(SnapshotCompareReason {
            code: "internal_binding_targets_exact".to_string(),
            message: "internal binding targets matched exactly".to_string(),
        });
    } else if binding_target_overlap > 0.0 {
        confidence += binding_target_overlap * 0.04;
        matched_layout_fields.push("internal_structure.binding_targets");
        reasons.push(SnapshotCompareReason {
            code: "internal_binding_targets_overlap".to_string(),
            message: format!("internal binding target overlap score: {binding_target_overlap:.3}"),
        });
    } else if !old_asset.internal_structure.binding_targets.is_empty()
        && !new_asset.internal_structure.binding_targets.is_empty()
    {
        confidence -= 0.04;
        conflicting_layout_fields.push("internal_structure.binding_targets");
        reasons.push(SnapshotCompareReason {
            code: "internal_binding_targets_mismatch".to_string(),
            message: "internal binding targets were disjoint across the candidate remap"
                .to_string(),
        });
    }

    let subresource_role_overlap = tag_overlap(
        &old_asset.internal_structure.subresource_roles,
        &new_asset.internal_structure.subresource_roles,
    );
    if !old_asset.internal_structure.subresource_roles.is_empty()
        && old_asset.internal_structure.subresource_roles
            == new_asset.internal_structure.subresource_roles
    {
        confidence += 0.04;
        matched_structural_fields.push("internal_structure.subresource_roles");
        reasons.push(SnapshotCompareReason {
            code: "internal_subresource_roles_exact".to_string(),
            message: "internal subresource roles matched exactly".to_string(),
        });
    } else if subresource_role_overlap > 0.0 {
        confidence += subresource_role_overlap * 0.03;
        matched_structural_fields.push("internal_structure.subresource_roles");
        reasons.push(SnapshotCompareReason {
            code: "internal_subresource_roles_overlap".to_string(),
            message: format!(
                "internal subresource role overlap score: {subresource_role_overlap:.3}"
            ),
        });
    } else if !old_asset.internal_structure.subresource_roles.is_empty()
        && !new_asset.internal_structure.subresource_roles.is_empty()
    {
        confidence -= 0.03;
        conflicting_structural_fields.push("internal_structure.subresource_roles");
        reasons.push(SnapshotCompareReason {
            code: "internal_subresource_roles_mismatch".to_string(),
            message: "internal subresource roles were disjoint across the candidate remap"
                .to_string(),
        });
    }

    if old_asset.internal_structure.has_skeleton.is_some()
        && old_asset.internal_structure.has_skeleton == new_asset.internal_structure.has_skeleton
    {
        confidence += 0.03;
        matched_structural_fields.push("internal_structure.has_skeleton");
        reasons.push(SnapshotCompareReason {
            code: "internal_skeleton_presence_exact".to_string(),
            message: "skeleton presence marker matched exactly".to_string(),
        });
    } else if old_asset.internal_structure.has_skeleton.is_some()
        && new_asset.internal_structure.has_skeleton.is_some()
    {
        confidence -= 0.03;
        conflicting_structural_fields.push("internal_structure.has_skeleton");
        reasons.push(SnapshotCompareReason {
            code: "internal_skeleton_presence_mismatch".to_string(),
            message: "skeleton presence marker changed across the candidate remap".to_string(),
        });
    }

    if old_asset.internal_structure.has_shapekey_data.is_some()
        && old_asset.internal_structure.has_shapekey_data
            == new_asset.internal_structure.has_shapekey_data
    {
        confidence += 0.02;
        matched_structural_fields.push("internal_structure.has_shapekey_data");
        reasons.push(SnapshotCompareReason {
            code: "internal_shapekey_presence_exact".to_string(),
            message: "shapekey presence marker matched exactly".to_string(),
        });
    } else if old_asset.internal_structure.has_shapekey_data.is_some()
        && new_asset.internal_structure.has_shapekey_data.is_some()
    {
        confidence -= 0.02;
        conflicting_structural_fields.push("internal_structure.has_shapekey_data");
        reasons.push(SnapshotCompareReason {
            code: "internal_shapekey_presence_mismatch".to_string(),
            message: "shapekey presence marker changed across the candidate remap".to_string(),
        });
    }

    if old_asset.normalized_name.is_some() && old_asset.normalized_name == new_asset.normalized_name
    {
        confidence += 0.28;
        reasons.push(SnapshotCompareReason {
            code: "normalized_name_exact".to_string(),
            message: format!(
                "normalized name matched exactly: {}",
                old_asset.normalized_name.as_deref().unwrap_or("unknown")
            ),
        });
    } else {
        let name_overlap = token_overlap(
            old_asset.normalized_name.as_deref().unwrap_or_default(),
            new_asset.normalized_name.as_deref().unwrap_or_default(),
        );
        if name_overlap > 0.0 {
            let contribution = name_overlap * 0.22;
            confidence += contribution;
            reasons.push(SnapshotCompareReason {
                code: "normalized_name_token_overlap".to_string(),
                message: format!("normalized name token overlap score: {name_overlap:.3}"),
            });
        }
    }

    if parent_directory(&old_asset.path) == parent_directory(&new_asset.path) {
        confidence += 0.25;
        reasons.push(SnapshotCompareReason {
            code: "same_parent_directory".to_string(),
            message: format!(
                "asset stayed under the same parent directory: {}",
                parent_directory(&old_asset.path).unwrap_or_default()
            ),
        });
    }

    let path_overlap = token_overlap(&old_asset.path, &new_asset.path);
    if path_overlap > 0.0 {
        let contribution = path_overlap * 0.18;
        confidence += contribution;
        reasons.push(SnapshotCompareReason {
            code: "path_token_overlap".to_string(),
            message: format!("path token overlap score: {path_overlap:.3}"),
        });
    }

    if old_asset.vertex_count.is_some() && old_asset.vertex_count == new_asset.vertex_count {
        confidence += 0.06;
        matched_structural_fields.push("vertex_count");
        reasons.push(SnapshotCompareReason {
            code: "vertex_count_exact".to_string(),
            message: "vertex count matched exactly".to_string(),
        });
    } else if let Some((penalty, message)) = structural_mismatch_penalty(
        "vertex_count",
        old_asset.vertex_count,
        new_asset.vertex_count,
    ) {
        confidence -= penalty;
        conflicting_structural_fields.push("vertex_count");
        reasons.push(SnapshotCompareReason {
            code: "vertex_count_mismatch".to_string(),
            message,
        });
    }

    if old_asset.index_count.is_some() && old_asset.index_count == new_asset.index_count {
        confidence += 0.04;
        matched_structural_fields.push("index_count");
        reasons.push(SnapshotCompareReason {
            code: "index_count_exact".to_string(),
            message: "index count matched exactly".to_string(),
        });
    } else if let Some((penalty, message)) =
        structural_mismatch_penalty("index_count", old_asset.index_count, new_asset.index_count)
    {
        confidence -= penalty;
        conflicting_structural_fields.push("index_count");
        reasons.push(SnapshotCompareReason {
            code: "index_count_mismatch".to_string(),
            message,
        });
    }

    if old_asset.material_slots.is_some() && old_asset.material_slots == new_asset.material_slots {
        confidence += 0.03;
        matched_structural_fields.push("material_slots");
        reasons.push(SnapshotCompareReason {
            code: "material_slots_exact".to_string(),
            message: "material slot count matched exactly".to_string(),
        });
    } else if old_asset.material_slots.is_some() && new_asset.material_slots.is_some() {
        confidence -= 0.03;
        conflicting_structural_fields.push("material_slots");
        reasons.push(SnapshotCompareReason {
            code: "material_slots_mismatch".to_string(),
            message: "material slot count changed across the candidate remap".to_string(),
        });
    }

    if old_asset.section_count.is_some() && old_asset.section_count == new_asset.section_count {
        confidence += 0.03;
        matched_structural_fields.push("section_count");
        reasons.push(SnapshotCompareReason {
            code: "section_count_exact".to_string(),
            message: "section count matched exactly".to_string(),
        });
    } else if old_asset.section_count.is_some() && new_asset.section_count.is_some() {
        confidence -= 0.03;
        conflicting_structural_fields.push("section_count");
        reasons.push(SnapshotCompareReason {
            code: "section_count_mismatch".to_string(),
            message: "section count changed across the candidate remap".to_string(),
        });
    }

    let tag_overlap = tag_overlap(&old_asset.tags, &new_asset.tags);
    if !old_asset.tags.is_empty() && old_asset.tags == new_asset.tags {
        confidence += 0.05;
        reasons.push(SnapshotCompareReason {
            code: "tags_exact".to_string(),
            message: "normalized tags matched exactly".to_string(),
        });
    } else if tag_overlap > 0.0 {
        let contribution = tag_overlap * 0.05;
        confidence += contribution;
        reasons.push(SnapshotCompareReason {
            code: "tags_overlap".to_string(),
            message: format!("tag overlap score: {tag_overlap:.3}"),
        });
    } else if !old_asset.tags.is_empty() && !new_asset.tags.is_empty() {
        confidence -= 0.02;
        reasons.push(SnapshotCompareReason {
            code: "tags_mismatch".to_string(),
            message: "normalized tags were disjoint across the candidate remap".to_string(),
        });
    }

    if matched_identity_fields.is_empty()
        && matched_layout_fields.is_empty()
        && matched_structural_fields.len() < 2
        && !has_reason_code(&reasons, "normalized_name_exact")
    {
        confidence -= 0.06;
        reasons.push(SnapshotCompareReason {
            code: "weak_identity_evidence".to_string(),
            message:
                "candidate relies mostly on path/name heuristics without strong identity anchors"
                    .to_string(),
        });
    }

    if !conflicting_identity_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "identity_conflict_detected".to_string(),
            message: format!(
                "identity signals conflicted for this candidate: {}; keep this remap conservative",
                conflicting_identity_fields.join(", ")
            ),
        });
    }

    if !conflicting_layout_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "buffer_layout_validation_needed".to_string(),
            message: format!(
                "buffer/layout signals drifted for this candidate: {}; validate the replacement layout before remapping",
                conflicting_layout_fields.join(", ")
            ),
        });
    }

    if !matched_layout_fields.is_empty() && conflicting_layout_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "buffer_layout_compatible".to_string(),
            message: format!(
                "buffer/layout signals stayed compatible: {}",
                matched_layout_fields.join(", ")
            ),
        });
    }

    if !matched_structural_fields.is_empty() && conflicting_structural_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "structural_layout_compatible".to_string(),
            message: format!(
                "structural layout signals stayed compatible: {}",
                matched_structural_fields.join(", ")
            ),
        });
    } else if !conflicting_structural_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "same_asset_but_structural_drift".to_string(),
            message: format!(
                "candidate still looks related, but structural fields drifted: {}",
                conflicting_structural_fields.join(", ")
            ),
        });
    }

    if !matched_provenance_fields.is_empty() && conflicting_provenance_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "package_origin_compatible".to_string(),
            message: format!(
                "package/provenance signals stayed compatible: {}",
                matched_provenance_fields.join(", ")
            ),
        });
    } else if !conflicting_provenance_fields.is_empty() {
        reasons.push(SnapshotCompareReason {
            code: "package_origin_moved".to_string(),
            message: format!(
                "package/provenance signals changed for this candidate: {}",
                conflicting_provenance_fields.join(", ")
            ),
        });
    }

    if (has_reason_code(&reasons, "normalized_name_exact")
        || !matched_identity_fields.is_empty()
        || !matched_layout_fields.is_empty())
        && !has_reason_code(&reasons, "identity_conflict_detected")
        && !has_reason_code(&reasons, "same_asset_but_structural_drift")
    {
        reasons.push(SnapshotCompareReason {
            code: "likely_same_asset_repathed".to_string(),
            message: "candidate preserves strong logical identity signals and looks like a renamed/repathed asset".to_string(),
        });
    }

    (confidence.clamp(0.0, 1.0), reasons)
}

fn structural_mismatch_penalty(
    field: &str,
    old_value: Option<u32>,
    new_value: Option<u32>,
) -> Option<(f32, String)> {
    let (Some(old_value), Some(new_value)) = (old_value, new_value) else {
        return None;
    };
    if old_value == new_value {
        return None;
    }

    let relative_delta = relative_delta(old_value, new_value);
    let penalty = if relative_delta >= 0.50 {
        0.09
    } else if relative_delta >= 0.20 {
        0.06
    } else {
        0.03
    };
    Some((
        penalty,
        format!(
            "{field} changed from {old_value} to {new_value} (relative delta {relative_delta:.3})"
        ),
    ))
}

fn relative_delta(old_value: u32, new_value: u32) -> f32 {
    let baseline = old_value.max(new_value).max(1) as f32;
    (old_value.abs_diff(new_value) as f32) / baseline
}

fn tag_overlap(left: &[String], right: &[String]) -> f32 {
    let left_tags = left.iter().cloned().collect::<BTreeSet<_>>();
    let right_tags = right.iter().cloned().collect::<BTreeSet<_>>();
    if left_tags.is_empty() || right_tags.is_empty() {
        return 0.0;
    }

    let intersection = left_tags.intersection(&right_tags).count() as f32;
    let union = left_tags.union(&right_tags).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn candidate_ambiguity_gap_threshold(candidate: &CandidateMappingChange) -> f32 {
    if has_reason_code(&candidate.reasons, "signature_exact")
        || has_reason_code(&candidate.reasons, "asset_hash_exact")
    {
        0.08
    } else if has_reason_code(&candidate.reasons, "weak_identity_evidence")
        || has_reason_code(&candidate.reasons, "identity_conflict_detected")
        || has_reason_code(&candidate.reasons, "same_asset_but_structural_drift")
        || has_reason_code(&candidate.reasons, "buffer_layout_validation_needed")
    {
        0.12
    } else {
        0.09
    }
}

fn has_reason_code(reasons: &[SnapshotCompareReason], code: &str) -> bool {
    reasons.iter().any(|reason| reason.code == code)
}

fn token_overlap(left: &str, right: &str) -> f32 {
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }

    let intersection = left_tokens.intersection(&right_tokens).count() as f32;
    let union = left_tokens.union(&right_tokens).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn tokenize(value: &str) -> BTreeSet<String> {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn parent_directory(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
        create_prepared_snapshot_from_file,
    };

    use super::{RemapCompatibility, RiskLevel, SnapshotComparer};

    #[test]
    fn compare_report_detects_added_removed_changed_and_candidate_mapping() {
        let comparer = SnapshotComparer;
        let old_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.4.0".to_string(),
            created_at_unix_ms: 1,
            source_root: "old".to_string(),
            asset_count: 2,
            assets: vec![
                asset(
                    "Content/Character/HeroA/Body.mesh",
                    Some("heroa body"),
                    Some(12000),
                ),
                asset(
                    "Content/Weapon/Sword.weapon",
                    Some("sword main"),
                    Some(1000),
                ),
            ],
            context: SnapshotContext::default(),
        };
        let mut changed_body = asset(
            "Content/Character/HeroA/Body.mesh",
            Some("heroa body"),
            Some(18000),
        );
        changed_body.metadata.section_count = Some(2);
        changed_body.fingerprint.section_count = Some(2);
        let new_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.5.0".to_string(),
            created_at_unix_ms: 2,
            source_root: "new".to_string(),
            asset_count: 2,
            assets: vec![
                changed_body,
                asset(
                    "Content/Weapon/Sword_v2.weapon",
                    Some("sword main"),
                    Some(1000),
                ),
            ],
            context: SnapshotContext::default(),
        };

        let report = comparer.compare(&old_snapshot, &new_snapshot);

        assert_eq!(report.summary.changed_assets, 1);
        assert_eq!(report.summary.removed_assets, 1);
        assert_eq!(report.summary.added_assets, 1);
        assert_eq!(report.summary.candidate_mapping_changes, 1);
        assert_eq!(report.changed_assets[0].probable_impact, RiskLevel::High);
        assert!(report.scope.low_signal_compare);
        assert!(
            report
                .scope
                .notes
                .iter()
                .any(|note| note.contains("low-coverage/low-enrichment extractor snapshots"))
        );
        assert!(
            report.changed_assets[0]
                .changed_fields
                .iter()
                .any(|field| field == "vertex_count")
        );
        assert!(report.candidate_mapping_changes[0].confidence >= 0.65);
        assert!(!report.candidate_mapping_changes[0].ambiguous);
    }

    #[test]
    fn compare_report_marks_candidate_as_ambiguous_when_runner_up_is_too_close() {
        let comparer = SnapshotComparer;
        let old_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.4.0".to_string(),
            created_at_unix_ms: 1,
            source_root: "old".to_string(),
            asset_count: 1,
            assets: vec![asset(
                "Content/Weapon/Pistol_Main.weapon",
                Some("pistol main"),
                Some(1500),
            )],
            context: SnapshotContext::default(),
        };
        let new_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.5.0".to_string(),
            created_at_unix_ms: 2,
            source_root: "new".to_string(),
            asset_count: 2,
            assets: vec![
                asset(
                    "Content/Weapon/Pistol_Main_A.weapon",
                    Some("pistol main"),
                    Some(1500),
                ),
                asset(
                    "Content/Weapon/Pistol_Main_B.weapon",
                    Some("pistol main"),
                    Some(1500),
                ),
            ],
            context: SnapshotContext::default(),
        };

        let report = comparer.compare(&old_snapshot, &new_snapshot);

        assert_eq!(report.summary.candidate_mapping_changes, 1);
        assert!(report.candidate_mapping_changes[0].ambiguous);
        assert!(
            report.candidate_mapping_changes[0]
                .confidence_gap
                .is_some_and(|gap| gap < 0.08)
        );
    }

    #[test]
    fn compare_uses_structural_metadata_from_prepared_asset_snapshots() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let old_inventory = test_root.join("old.prepared.json");
        let new_inventory = test_root.join("new.prepared.json");
        fs::write(
            &old_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":120,
                            "index_count":240,
                            "material_slots":2,
                            "section_count":3
                        }
                    }
                ]
            }"#,
        )
        .expect("write old inventory");
        fs::write(
            &new_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":180,
                            "index_count":360,
                            "material_slots":3,
                            "section_count":4
                        }
                    }
                ]
            }"#,
        )
        .expect("write new inventory");

        let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
            .expect("old snapshot");
        let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
            .expect("new snapshot");

        let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

        assert_eq!(report.summary.changed_assets, 1);
        assert!(
            report.changed_assets[0]
                .changed_fields
                .iter()
                .any(|field| field == "vertex_count")
        );
        assert!(
            report.changed_assets[0]
                .changed_fields
                .iter()
                .any(|field| field == "section_count")
        );
        assert_eq!(report.changed_assets[0].probable_impact, RiskLevel::High);

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn compare_surfaces_signature_and_detects_signature_only_changes() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let old_inventory = test_root.join("old.prepared.json");
        let new_inventory = test_root.join("new.prepared.json");
        fs::write(
            &old_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":120,
                            "index_count":240,
                            "material_slots":2,
                            "section_count":3
                        },
                        "hash_fields":{
                            "asset_hash":"asset-md5",
                            "shader_hash":"shader-md5",
                            "signature":"sig-old"
                        }
                    }
                ]
            }"#,
        )
        .expect("write old inventory");
        fs::write(
            &new_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":120,
                            "index_count":240,
                            "material_slots":2,
                            "section_count":3
                        },
                        "hash_fields":{
                            "asset_hash":"asset-md5",
                            "shader_hash":"shader-md5",
                            "signature":"sig-new"
                        }
                    }
                ]
            }"#,
        )
        .expect("write new inventory");

        let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
            .expect("old snapshot");
        let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
            .expect("new snapshot");

        let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let changed_asset = &report.changed_assets[0];

        assert_eq!(report.summary.changed_assets, 1);
        assert_eq!(
            changed_asset
                .old_asset
                .as_ref()
                .and_then(|asset| asset.signature.as_deref()),
            Some("sig-old")
        );
        assert_eq!(
            changed_asset
                .new_asset
                .as_ref()
                .and_then(|asset| asset.signature.as_deref()),
            Some("sig-new")
        );
        assert!(
            changed_asset
                .changed_fields
                .iter()
                .any(|field| field == "signature")
        );
        assert_eq!(changed_asset.probable_impact, RiskLevel::High);
        assert_eq!(changed_asset.crash_risk, RiskLevel::High);

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn compare_summary_distinguishes_naming_and_cosmetic_only_changes() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let old_inventory = test_root.join("old.prepared.json");
        let new_inventory = test_root.join("new.prepared.json");
        fs::write(
            &old_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "tags":["character","prepared"]
                        }
                    },
                    {
                        "id":"mesh:encore:hair",
                        "path":"Content/Character/Encore/Hair.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Hair",
                            "tags":["character","support"]
                        }
                    }
                ]
            }"#,
        )
        .expect("write old inventory");
        fs::write(
            &new_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body Variant",
                            "tags":["character","prepared"]
                        }
                    },
                    {
                        "id":"mesh:encore:hair",
                        "path":"Content/Character/Encore/Hair.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Hair",
                            "tags":["character","featured"]
                        }
                    }
                ]
            }"#,
        )
        .expect("write new inventory");

        let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
            .expect("old snapshot");
        let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
            .expect("new snapshot");

        let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

        assert_eq!(report.summary.changed_assets, 2);
        assert_eq!(report.summary.identity_changed_assets, 0);
        assert_eq!(report.summary.structural_changed_assets, 0);
        assert_eq!(report.summary.naming_only_changed_assets, 1);
        assert_eq!(report.summary.cosmetic_only_changed_assets, 1);

        let naming_change = report
            .changed_assets
            .iter()
            .find(|change| {
                change
                    .old_asset
                    .as_ref()
                    .is_some_and(|asset| asset.path.ends_with("Body.mesh"))
            })
            .expect("naming change");
        assert_eq!(naming_change.probable_impact, RiskLevel::Low);
        assert_eq!(naming_change.crash_risk, RiskLevel::Low);
        assert!(!naming_change.suspected_mapping_change);
        assert!(
            naming_change
                .reasons
                .iter()
                .any(|reason| reason.code == "naming_only_change")
        );

        let cosmetic_change = report
            .changed_assets
            .iter()
            .find(|change| {
                change
                    .old_asset
                    .as_ref()
                    .is_some_and(|asset| asset.path.ends_with("Hair.mesh"))
            })
            .expect("cosmetic change");
        assert_eq!(cosmetic_change.probable_impact, RiskLevel::Low);
        assert_eq!(cosmetic_change.crash_risk, RiskLevel::Low);
        assert!(!cosmetic_change.suspected_mapping_change);
        assert!(
            cosmetic_change
                .reasons
                .iter()
                .any(|reason| reason.code == "cosmetic_metadata_changed")
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn candidate_mapping_prefers_identity_and_structure_matches_over_path_similarity() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let old_inventory = test_root.join("old.prepared.json");
        let new_inventory = test_root.join("new.prepared.json");
        fs::write(
            &old_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":120,
                            "index_count":240,
                            "material_slots":2,
                            "section_count":3,
                            "tags":["character","body"]
                        },
                        "hash_fields":{
                            "asset_hash":"body-a",
                            "signature":"sig-body"
                        }
                    }
                ]
            }"#,
        )
        .expect("write old inventory");
        fs::write(
            &new_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body:new",
                        "path":"Content/Character/Encore/Body_v2.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":120,
                            "index_count":240,
                            "material_slots":2,
                            "section_count":3,
                            "tags":["character","body"]
                        },
                        "hash_fields":{
                            "asset_hash":"body-a",
                            "signature":"sig-body"
                        }
                    },
                    {
                        "id":"mesh:encore:body:decoy",
                        "path":"Content/Character/Encore/Body_variant.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":220,
                            "index_count":340,
                            "material_slots":4,
                            "section_count":5,
                            "tags":["character","decoy"]
                        },
                        "hash_fields":{
                            "asset_hash":"body-b",
                            "signature":"sig-decoy"
                        }
                    }
                ]
            }"#,
        )
        .expect("write new inventory");

        let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
            .expect("old snapshot");
        let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
            .expect("new snapshot");

        let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let candidate = report
            .candidate_mapping_changes
            .first()
            .expect("candidate remap");

        assert_eq!(report.summary.candidate_mapping_changes, 1);
        assert_eq!(report.summary.high_confidence_candidate_mapping_changes, 1);
        assert_eq!(
            candidate.new_asset.path,
            "Content/Character/Encore/Body_v2.mesh"
        );
        assert!(!candidate.ambiguous);
        assert!(candidate.confidence >= 0.85);
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "signature_exact")
        );
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "asset_hash_exact")
        );
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "structural_layout_compatible")
        );
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "likely_same_asset_repathed")
        );
        assert_eq!(
            candidate.compatibility,
            RemapCompatibility::LikelyCompatible
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn candidate_mapping_marks_same_asset_but_structural_drift() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let old_inventory = test_root.join("old.prepared.json");
        let new_inventory = test_root.join("new.prepared.json");
        fs::write(
            &old_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":120,
                            "index_count":240,
                            "material_slots":2,
                            "section_count":3
                        },
                        "hash_fields":{
                            "signature":"sig-body"
                        }
                    }
                ]
            }"#,
        )
        .expect("write old inventory");
        fs::write(
            &new_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:body:new",
                        "path":"Content/Character/Encore/Body_v2.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":240,
                            "index_count":480,
                            "material_slots":3,
                            "section_count":4
                        },
                        "hash_fields":{
                            "signature":"sig-body"
                        }
                    }
                ]
            }"#,
        )
        .expect("write new inventory");

        let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
            .expect("old snapshot");
        let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
            .expect("new snapshot");

        let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let candidate = report
            .candidate_mapping_changes
            .first()
            .expect("candidate remap");

        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "signature_exact")
        );
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "same_asset_but_structural_drift")
        );
        assert_eq!(
            candidate.compatibility,
            RemapCompatibility::StructurallyRisky
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn candidate_mapping_blocks_identity_conflicts_even_when_similarity_is_high() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let old_inventory = test_root.join("old.prepared.json");
        let new_inventory = test_root.join("new.prepared.json");
        fs::write(
            &old_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:face",
                        "path":"Content/Character/Encore/Face.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Face",
                            "vertex_count":100,
                            "index_count":200,
                            "material_slots":1,
                            "section_count":1
                        },
                        "hash_fields":{
                            "signature":"sig-old"
                        }
                    }
                ]
            }"#,
        )
        .expect("write old inventory");
        fs::write(
            &new_inventory,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "assets":[
                    {
                        "id":"mesh:encore:face:new",
                        "path":"Content/Character/Encore/Face_LOD0.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Face",
                            "vertex_count":100,
                            "index_count":200,
                            "material_slots":1,
                            "section_count":1
                        },
                        "hash_fields":{
                            "signature":"sig-new"
                        }
                    }
                ]
            }"#,
        )
        .expect("write new inventory");

        let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
            .expect("old snapshot");
        let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
            .expect("new snapshot");

        let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let candidate = report
            .candidate_mapping_changes
            .first()
            .expect("candidate remap");

        assert!(candidate.confidence >= 0.65);
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "signature_mismatch")
        );
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| reason.code == "identity_conflict_detected")
        );
        assert_eq!(
            candidate.compatibility,
            RemapCompatibility::IncompatibleBlocked
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    fn asset(
        path: &str,
        normalized_name: Option<&str>,
        vertex_count: Option<u32>,
    ) -> SnapshotAsset {
        SnapshotAsset {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            metadata: crate::domain::AssetMetadata {
                logical_name: normalized_name.map(ToOwned::to_owned),
                vertex_count,
                index_count: Some(2000),
                material_slots: Some(1),
                section_count: Some(1),
                tags: vec!["weapon".to_string()],
                ..Default::default()
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: normalized_name.map(ToOwned::to_owned),
                name_tokens: normalized_name
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
                path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
                tags: vec!["weapon".to_string()],
                vertex_count,
                index_count: Some(2000),
                material_slots: Some(1),
                section_count: Some(1),
                vertex_stride: None,
                vertex_buffer_count: None,
                index_format: None,
                primitive_topology: None,
                layout_markers: Vec::new(),
                internal_structure: crate::domain::AssetInternalStructure::default(),
            },
            hash_fields: SnapshotHashFields::default(),
            source: crate::domain::AssetSourceContext::default(),
        }
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();

        std::env::temp_dir().join(format!("whashreonator-compare-test-{nanos}"))
    }
}
