use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    compare::{
        AssetLineageKind, CandidateMappingChange, RemapCompatibility, RiskLevel,
        SnapshotAssetChange,
        SnapshotAssetSummary, SnapshotChangeType, SnapshotCompareReason, SnapshotCompareReport,
    },
    domain::{AssetInternalStructure, AssetSourceContext},
    inference::{InferenceReport, InferredMappingContinuityContext, RepresentativeModRiskClass},
    proposal::{MappingProposalEntry, MappingProposalOutput, ProposalStatus},
    snapshot::{GameSnapshot, assess_snapshot_scope, summarize_snapshot_capture_quality},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionDiffReportV2 {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub old_version: VersionSide,
    pub new_version: VersionSide,
    pub resonators: Vec<ResonatorDiffEntry>,
    #[serde(default)]
    pub lineage: VersionLineageSection,
    pub summary: VersionDiffSummary,
    #[serde(default)]
    pub scope_notes: Vec<String>,
    #[serde(default)]
    pub review: VersionReviewSection,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VersionLineageSection {
    pub summary: VersionLineageSummary,
    pub entries: Vec<VersionLineageEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VersionContinuityIndex {
    pub summary: VersionContinuitySummary,
    pub threads: Vec<VersionContinuityThread>,
    #[serde(default)]
    pub thread_summaries: Vec<VersionContinuityThreadSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VersionContinuityArtifact {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub report_count: usize,
    pub latest_version_id: Option<String>,
    pub continuity: VersionContinuityIndex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionContinuitySummary {
    pub thread_count: usize,
    pub observation_count: usize,
    pub introduced_threads: usize,
    pub persisted_threads: usize,
    pub rename_or_repath_threads: usize,
    pub container_movement_threads: usize,
    pub layout_drift_threads: usize,
    pub replacement_threads: usize,
    pub ambiguous_threads: usize,
    pub insufficient_evidence_threads: usize,
    pub removed_threads: usize,
    pub review_required_threads: usize,
    pub ongoing_threads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VersionContinuityRelation {
    Persisted,
    RenameOrRepath,
    ContainerMovement,
    LayoutDrift,
    Replacement,
    Ambiguous,
    #[default]
    InsufficientEvidence,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VersionContinuitySource {
    #[default]
    UnchangedAsset,
    ChangedAsset,
    CandidateMapping,
    AddedAsset,
    RemovedAsset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionContinuityThread {
    pub thread_id: String,
    pub anchor_version_id: String,
    pub anchor: VersionedItem,
    pub observations: Vec<VersionContinuityObservation>,
    #[serde(default)]
    pub review_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionContinuityObservation {
    pub from_version_id: String,
    pub to_version_id: String,
    pub from_path: String,
    #[serde(default)]
    pub to_path: Option<String>,
    #[serde(default)]
    pub relation: VersionContinuityRelation,
    #[serde(default)]
    pub status: DiffStatus,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub compatibility: Option<RemapCompatibility>,
    #[serde(default)]
    pub source: VersionContinuitySource,
    #[serde(default)]
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionContinuityThreadSummary {
    pub thread_id: String,
    pub anchor_version_id: String,
    pub anchor: VersionedItem,
    pub first_seen_version: String,
    pub latest_observed_version: String,
    pub latest_live_version: Option<String>,
    #[serde(default)]
    pub first_rename_or_repath_step: Option<VersionContinuityMilestoneStep>,
    #[serde(default)]
    pub first_container_movement_step: Option<VersionContinuityMilestoneStep>,
    #[serde(default)]
    pub first_layout_drift_step: Option<VersionContinuityMilestoneStep>,
    #[serde(default)]
    pub terminal_relation: Option<VersionContinuityRelation>,
    #[serde(default)]
    pub terminal_version: Option<String>,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default)]
    pub purely_persisted: bool,
    #[serde(default)]
    pub materially_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionContinuityMilestoneStep {
    pub from_version_id: String,
    pub to_version_id: String,
    pub from_path: String,
    pub to_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionLineageSummary {
    pub total_entries: usize,
    pub rename_or_repath_assets: usize,
    pub container_movement_assets: usize,
    pub layout_drift_assets: usize,
    pub replacement_assets: usize,
    pub ambiguous_assets: usize,
    pub insufficient_evidence_assets: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VersionLineageSource {
    ChangedAsset,
    CandidateMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionLineageEntry {
    pub source: VersionLineageSource,
    #[serde(default)]
    pub status: DiffStatus,
    #[serde(default)]
    pub lineage: AssetLineageKind,
    pub old: Option<VersionedItem>,
    pub new: Option<VersionedItem>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub compatibility: Option<RemapCompatibility>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
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
    #[serde(default)]
    pub compatibility: Option<RemapCompatibility>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    #[default]
    Unchanged,
    Changed,
    Added,
    Removed,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
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
    pub signature: Option<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: AssetSourceContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VersionReviewSection {
    pub summary: VersionReviewSummary,
    pub continuity: VersionContinuityReviewSection,
    #[serde(default)]
    pub representative_mod_risks: VersionRepresentativeModRiskReviewSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionReviewSummary {
    pub review_mapping_count: usize,
    pub continuity_review_mapping_count: usize,
    pub continuity_caution_present: bool,
    #[serde(default)]
    pub representative_projection_count: usize,
    #[serde(default)]
    pub representative_review_first_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VersionContinuityReviewSection {
    pub caution_present: bool,
    pub cause_count: usize,
    pub fix_count: usize,
    pub review_mapping_count: usize,
    pub causes: Vec<VersionReviewCause>,
    pub fixes: Vec<VersionReviewFix>,
    pub mappings: Vec<VersionReviewMapping>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VersionRepresentativeModRiskReviewSection {
    pub projection_count: usize,
    pub review_first_count: usize,
    pub projections: Vec<VersionRepresentativeModRiskProjection>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionRepresentativeModRiskProjection {
    pub risk_class: RepresentativeModRiskClass,
    pub summary: String,
    pub confidence: f32,
    pub priority: RiskLevel,
    pub representative_profile_count: usize,
    #[serde(default)]
    pub review_first: bool,
    #[serde(default)]
    pub triggering_compare_signals: Vec<String>,
    #[serde(default)]
    pub sample_mod_names: Vec<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionReviewCause {
    pub code: String,
    pub summary: String,
    pub confidence: f32,
    #[serde(default)]
    pub affected_assets: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionReviewFix {
    pub code: String,
    pub summary: String,
    pub confidence: f32,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionReviewMapping {
    pub old_asset_path: String,
    pub new_asset_path: String,
    pub status: ProposalStatus,
    pub confidence: f32,
    #[serde(default)]
    pub compatibility: RemapCompatibility,
    #[serde(default)]
    pub continuity: Option<InferredMappingContinuityContext>,
    #[serde(default)]
    pub continuity_notes: Vec<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub related_fix_codes: Vec<String>,
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

        let lineage = build_lineage_section(compare_report);
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
            lineage,
            summary,
            scope_notes: build_scope_notes(old_snapshot, new_snapshot),
            review: VersionReviewSection::default(),
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
                    item.compatibility = Some(hint.compatibility.clone());
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

    pub fn enrich_with_review_surface(
        &self,
        mut report: VersionDiffReportV2,
        inference: &InferenceReport,
        mapping_proposal: &MappingProposalOutput,
    ) -> VersionDiffReportV2 {
        report.review = build_review_section(inference, mapping_proposal);
        report
    }
}

fn build_review_section(
    inference: &InferenceReport,
    mapping_proposal: &MappingProposalOutput,
) -> VersionReviewSection {
    let review_mapping_count = mapping_proposal
        .mappings
        .iter()
        .filter(|mapping| mapping.status == ProposalStatus::NeedsReview)
        .count();

    let continuity_causes = inference
        .probable_crash_causes
        .iter()
        .filter(|cause| cause.code == "continuity_thread_instability")
        .map(|cause| VersionReviewCause {
            code: cause.code.clone(),
            summary: cause.summary.clone(),
            confidence: cause.confidence,
            affected_assets: cause.affected_assets.iter().take(5).cloned().collect(),
            evidence: cause.evidence.iter().take(3).cloned().collect(),
        })
        .collect::<Vec<_>>();

    let continuity_fixes = inference
        .suggested_fixes
        .iter()
        .filter(|fix| fix.code == "review_continuity_thread_history_before_repair")
        .map(|fix| VersionReviewFix {
            code: fix.code.clone(),
            summary: fix.summary.clone(),
            confidence: fix.confidence,
            actions: fix.actions.iter().take(3).cloned().collect(),
            evidence: fix.evidence.iter().take(3).cloned().collect(),
        })
        .collect::<Vec<_>>();

    let continuity_review_mappings = mapping_proposal
        .mappings
        .iter()
        .filter(|mapping| {
            mapping.status == ProposalStatus::NeedsReview && mapping_has_continuity_caution(mapping)
        })
        .map(build_review_mapping)
        .collect::<Vec<_>>();

    let caution_present = !continuity_causes.is_empty()
        || !continuity_fixes.is_empty()
        || !continuity_review_mappings.is_empty();
    let representative_projections = inference
        .representative_risk_projections
        .iter()
        .map(|projection| VersionRepresentativeModRiskProjection {
            risk_class: projection.risk_class.clone(),
            summary: projection.summary.clone(),
            confidence: projection.confidence,
            priority: projection.priority.clone(),
            representative_profile_count: projection.representative_profile_count,
            review_first: projection.review_first,
            triggering_compare_signals: projection.triggering_compare_signals.clone(),
            sample_mod_names: projection.sample_mod_names.clone(),
            reasons: projection.reasons.clone(),
            evidence: projection.evidence.clone(),
        })
        .collect::<Vec<_>>();
    let representative_review_first_count = representative_projections
        .iter()
        .filter(|projection| projection.review_first)
        .count();

    let mut continuity_notes = Vec::new();
    if caution_present {
        continuity_notes.push(format!(
            "continuity-backed caution present: causes={} fixes={} review_mappings={}",
            continuity_causes.len(),
            continuity_fixes.len(),
            continuity_review_mappings.len()
        ));
        if let Some(mapping) = continuity_review_mappings.first() {
            continuity_notes.push(format!(
                "mapping {} -> {} stays review-first because broader continuity history is unstable",
                mapping.old_asset_path, mapping.new_asset_path
            ));
        }
    }

    VersionReviewSection {
        summary: VersionReviewSummary {
            review_mapping_count,
            continuity_review_mapping_count: continuity_review_mappings.len(),
            continuity_caution_present: caution_present,
            representative_projection_count: representative_projections.len(),
            representative_review_first_count,
        },
        continuity: VersionContinuityReviewSection {
            caution_present,
            cause_count: continuity_causes.len(),
            fix_count: continuity_fixes.len(),
            review_mapping_count: continuity_review_mappings.len(),
            causes: continuity_causes,
            fixes: continuity_fixes,
            mappings: continuity_review_mappings,
            notes: continuity_notes,
        },
        representative_mod_risks: VersionRepresentativeModRiskReviewSection {
            projection_count: representative_projections.len(),
            review_first_count: representative_review_first_count,
            notes: representative_projection_notes(&representative_projections),
            projections: representative_projections,
        },
    }
}

fn representative_projection_notes(
    projections: &[VersionRepresentativeModRiskProjection],
) -> Vec<String> {
    if projections.is_empty() {
        return Vec::new();
    }

    let review_first_count = projections
        .iter()
        .filter(|projection| projection.review_first)
        .count();
    let mut notes = vec![format!(
        "representative mod risk projection present: projections={} review_first={}",
        projections.len(),
        review_first_count
    )];
    if let Some(first) = projections.first() {
        notes.push(format!(
            "top representative risk surface: {:?} across {} representative profile(s)",
            first.risk_class, first.representative_profile_count
        ));
    }
    notes
}

fn build_review_mapping(mapping: &MappingProposalEntry) -> VersionReviewMapping {
    VersionReviewMapping {
        old_asset_path: mapping.old_asset_path.clone(),
        new_asset_path: mapping.new_asset_path.clone(),
        status: mapping.status.clone(),
        confidence: mapping.confidence,
        compatibility: mapping.compatibility.clone(),
        continuity: mapping.continuity.clone(),
        continuity_notes: mapping
            .continuity
            .as_ref()
            .map(structured_continuity_notes)
            .unwrap_or_default(),
        reasons: collect_continuity_items(&mapping.reasons),
        evidence: collect_continuity_items(&mapping.evidence),
        related_fix_codes: mapping
            .related_fix_codes
            .iter()
            .filter(|code| code.contains("continuity"))
            .cloned()
            .collect(),
    }
}

fn mapping_has_continuity_caution(mapping: &MappingProposalEntry) -> bool {
    mapping
        .continuity
        .as_ref()
        .is_some_and(|continuity| continuity.has_review_caution())
        || mapping
            .related_fix_codes
            .iter()
            .any(|code| code == "review_continuity_thread_history_before_repair")
        || mapping
            .reasons
            .iter()
            .chain(mapping.evidence.iter())
            .any(|value| is_continuity_text(value))
}

fn structured_continuity_notes(continuity: &InferredMappingContinuityContext) -> Vec<String> {
    let mut notes = Vec::new();

    if let (Some(first_seen), Some(latest_observed)) = (
        continuity.first_seen_version.as_deref(),
        continuity.latest_observed_version.as_deref(),
    ) {
        notes.push(format!("thread span {first_seen} -> {latest_observed}"));
    }
    if continuity.total_rename_steps > 0 {
        notes.push(format!(
            "rename/repath steps={}",
            continuity.total_rename_steps
        ));
    }
    if continuity.total_container_movement_steps > 0 {
        notes.push(format!(
            "container movement steps={}",
            continuity.total_container_movement_steps
        ));
    }
    if continuity.total_layout_drift_steps > 0 {
        notes.push(format!(
            "layout drift steps={}",
            continuity.total_layout_drift_steps
        ));
    }
    if continuity.review_required_history {
        notes.push("review-required elsewhere in the broader chain".to_string());
    }
    if let Some(relation) = continuity.terminal_relation.as_ref() {
        let timing = if continuity.terminal_after_current {
            "later terminal"
        } else {
            "terminal"
        };
        notes.push(format!(
            "{timing} {} in {}",
            continuity_relation_label(relation),
            continuity.terminal_version.as_deref().unwrap_or("unknown")
        ));
    } else if let Some(latest_live_version) = continuity.latest_live_version.as_deref() {
        notes.push(format!("latest live version {latest_live_version}"));
    }

    notes
}

fn collect_continuity_items(values: &[String]) -> Vec<String> {
    let mut collected = Vec::new();
    let mut seen = BTreeSet::<String>::new();

    for value in values.iter().filter(|value| is_continuity_text(value)) {
        if seen.insert(value.clone()) {
            collected.push(value.clone());
        }
        if collected.len() >= 3 {
            break;
        }
    }

    collected
}

fn is_continuity_text(value: &str) -> bool {
    value.contains("continuity")
        || value.contains("terminal state")
        || value.contains("review-required")
        || value.contains("repeated layout drift")
}

fn continuity_relation_label(relation: &VersionContinuityRelation) -> &'static str {
    match relation {
        VersionContinuityRelation::Persisted => "persisted",
        VersionContinuityRelation::RenameOrRepath => "rename/repath",
        VersionContinuityRelation::ContainerMovement => "container movement",
        VersionContinuityRelation::LayoutDrift => "layout drift",
        VersionContinuityRelation::Replacement => "replacement",
        VersionContinuityRelation::Ambiguous => "ambiguous",
        VersionContinuityRelation::InsufficientEvidence => "insufficient evidence",
        VersionContinuityRelation::Removed => "removed",
    }
}

impl VersionContinuityIndex {
    pub fn from_reports(reports: &[VersionDiffReportV2]) -> Self {
        let reports = select_adjacent_reports(reports);
        let mut threads = Vec::<VersionContinuityThread>::new();
        let mut open_threads = BTreeMap::<(String, String), usize>::new();

        for report in reports {
            let mut inputs = collect_continuity_inputs(report);
            inputs.sort_by(|left, right| {
                continuity_input_sort_key(left).cmp(&continuity_input_sort_key(right))
            });

            for input in inputs {
                match input.kind {
                    ContinuityInputKind::Seed => {
                        let Some(item) = input.to.clone() else {
                            continue;
                        };
                        let key = (input.to_version_id.clone(), item.key.clone());
                        if open_threads.contains_key(&key) {
                            continue;
                        }

                        let thread_index = threads.len();
                        threads.push(VersionContinuityThread {
                            thread_id: format!("{}:{}", input.to_version_id, item.key),
                            anchor_version_id: input.to_version_id,
                            anchor: item,
                            observations: Vec::new(),
                            review_required: false,
                        });
                        open_threads.insert(key, thread_index);
                    }
                    ContinuityInputKind::Transition => {
                        let thread_index = if let Some(from) = input.from.as_ref() {
                            let key = (input.from_version_id.clone(), from.key.clone());
                            open_threads.remove(&key).unwrap_or_else(|| {
                                let thread_index = threads.len();
                                threads.push(VersionContinuityThread {
                                    thread_id: format!("{}:{}", input.from_version_id, from.key),
                                    anchor_version_id: input.from_version_id.clone(),
                                    anchor: from.clone(),
                                    observations: Vec::new(),
                                    review_required: false,
                                });
                                thread_index
                            })
                        } else if let Some(to) = input.to.as_ref() {
                            let thread_index = threads.len();
                            threads.push(VersionContinuityThread {
                                thread_id: format!("{}:{}", input.to_version_id, to.key),
                                anchor_version_id: input.to_version_id.clone(),
                                anchor: to.clone(),
                                observations: Vec::new(),
                                review_required: false,
                            });
                            thread_index
                        } else {
                            continue;
                        };

                        let observation = VersionContinuityObservation {
                            from_version_id: input.from_version_id.clone(),
                            to_version_id: input.to_version_id.clone(),
                            from_path: input
                                .from
                                .as_ref()
                                .map(|item| item.key.clone())
                                .unwrap_or_default(),
                            to_path: input.to.as_ref().map(|item| item.key.clone()),
                            relation: input.relation.clone(),
                            status: input.status.clone(),
                            confidence: input.confidence,
                            compatibility: input.compatibility.clone(),
                            source: input.source.clone(),
                            reason_codes: input.reason_codes.clone(),
                        };

                        let continue_key = observation.to_path.as_ref().and_then(|to_path| {
                            if observation.relation.continues_thread() {
                                Some((observation.to_version_id.clone(), to_path.clone()))
                            } else {
                                None
                            }
                        });

                        let review_required = input.requires_review();
                        let terminal = matches!(
                            observation.relation,
                            VersionContinuityRelation::Replacement
                                | VersionContinuityRelation::Ambiguous
                                | VersionContinuityRelation::InsufficientEvidence
                                | VersionContinuityRelation::Removed
                        ) || observation.status == DiffStatus::Uncertain;

                        let thread = threads
                            .get_mut(thread_index)
                            .expect("thread index should exist");
                        thread.observations.push(observation);
                        thread.review_required |= review_required;

                        if let Some(continue_key) = continue_key {
                            open_threads.insert(continue_key, thread_index);
                        }
                        if terminal {
                            if let Some(from) = input.from.as_ref() {
                                open_threads
                                    .remove(&(input.from_version_id.clone(), from.key.clone()));
                            }
                        }
                    }
                }
            }
        }

        threads.sort_by(|left, right| {
            left.anchor_version_id
                .cmp(&right.anchor_version_id)
                .then_with(|| left.anchor.key.cmp(&right.anchor.key))
                .then_with(|| left.thread_id.cmp(&right.thread_id))
        });

        let thread_summaries = build_continuity_thread_summaries(&threads);
        let summary = summarize_continuity(&threads);
        VersionContinuityIndex {
            summary,
            threads,
            thread_summaries,
        }
    }
}

impl VersionContinuityArtifact {
    pub fn from_reports(reports: &[VersionDiffReportV2]) -> Self {
        let latest_version_id = reports
            .iter()
            .map(|report| report.new_version.version_id.clone())
            .max_by(|left, right| version_sort_key(left).cmp(&version_sort_key(right)));

        Self {
            schema_version: "whashreonator.continuity.v1".to_string(),
            generated_at_unix_ms: current_unix_ms(),
            report_count: reports.len(),
            latest_version_id,
            continuity: VersionContinuityIndex::from_reports(reports),
        }
    }
}

fn select_adjacent_reports<'a>(reports: &'a [VersionDiffReportV2]) -> Vec<&'a VersionDiffReportV2> {
    let mut by_pair = BTreeMap::<(String, String), &VersionDiffReportV2>::new();

    for report in reports {
        let key = (
            report.old_version.version_id.clone(),
            report.new_version.version_id.clone(),
        );
        by_pair.entry(key).or_insert(report);
    }

    let mut version_ids = BTreeSet::<String>::new();
    for report in by_pair.values() {
        version_ids.insert(report.old_version.version_id.clone());
        version_ids.insert(report.new_version.version_id.clone());
    }

    let mut ordered_versions = version_ids.into_iter().collect::<Vec<_>>();
    ordered_versions.sort_by(|left, right| version_sort_key(left).cmp(&version_sort_key(right)));
    let version_positions = ordered_versions
        .into_iter()
        .enumerate()
        .map(|(index, version_id)| (version_id, index))
        .collect::<BTreeMap<_, _>>();

    let mut selected = by_pair
        .into_values()
        .filter(|report| {
            let Some(old_index) = version_positions.get(&report.old_version.version_id) else {
                return false;
            };
            let Some(new_index) = version_positions.get(&report.new_version.version_id) else {
                return false;
            };

            new_index.checked_sub(*old_index) == Some(1)
        })
        .collect::<Vec<_>>();

    selected.sort_by(|left, right| {
        version_sort_key(&left.old_version.version_id)
            .cmp(&version_sort_key(&right.old_version.version_id))
            .then_with(|| {
                version_sort_key(&left.new_version.version_id)
                    .cmp(&version_sort_key(&right.new_version.version_id))
            })
            .then_with(|| left.generated_at_unix_ms.cmp(&right.generated_at_unix_ms))
    });

    selected
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

impl VersionContinuityRelation {
    fn continues_thread(&self) -> bool {
        matches!(
            self,
            VersionContinuityRelation::Persisted
                | VersionContinuityRelation::RenameOrRepath
                | VersionContinuityRelation::ContainerMovement
                | VersionContinuityRelation::LayoutDrift
        )
    }

    fn is_terminal(&self) -> bool {
        !self.continues_thread()
    }
}

struct ContinuityInput {
    kind: ContinuityInputKind,
    from_version_id: String,
    to_version_id: String,
    from: Option<VersionedItem>,
    to: Option<VersionedItem>,
    relation: VersionContinuityRelation,
    status: DiffStatus,
    confidence: Option<f32>,
    compatibility: Option<RemapCompatibility>,
    source: VersionContinuitySource,
    reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContinuityInputKind {
    Seed,
    Transition,
}

fn collect_continuity_inputs(report: &VersionDiffReportV2) -> Vec<ContinuityInput> {
    let mut inputs = Vec::new();
    let mut lineage_paths = BTreeSet::<String>::new();

    for entry in &report.lineage.entries {
        if let Some(old) = entry.old.as_ref() {
            lineage_paths.insert(old.key.clone());
        }
        if let Some(new) = entry.new.as_ref() {
            lineage_paths.insert(new.key.clone());
        }
    }

    for entry in &report.lineage.entries {
        let relation = continuity_relation_from_lineage(&entry.lineage);
        inputs.push(ContinuityInput {
            kind: ContinuityInputKind::Transition,
            from_version_id: report.old_version.version_id.clone(),
            to_version_id: report.new_version.version_id.clone(),
            from: entry.old.clone(),
            to: entry.new.clone(),
            relation,
            status: entry.status.clone(),
            confidence: entry.confidence,
            compatibility: entry.compatibility.clone(),
            source: entry.source.clone().into(),
            reason_codes: entry.reason_codes.clone(),
        });
    }

    for resonator in &report.resonators {
        for item in &resonator.items {
            if item.item_type != ReportItemType::Asset {
                continue;
            }

            match item.status {
                DiffStatus::Unchanged => {
                    if let (Some(old), Some(new)) = (item.old.clone(), item.new.clone()) {
                        inputs.push(ContinuityInput {
                            kind: ContinuityInputKind::Transition,
                            from_version_id: report.old_version.version_id.clone(),
                            to_version_id: report.new_version.version_id.clone(),
                            from: Some(old),
                            to: Some(new),
                            relation: VersionContinuityRelation::Persisted,
                            status: DiffStatus::Unchanged,
                            confidence: item.confidence,
                            compatibility: item.compatibility.clone(),
                            source: VersionContinuitySource::UnchangedAsset,
                            reason_codes: item
                                .reasons
                                .iter()
                                .map(|reason| reason.code.clone())
                                .collect(),
                        });
                    }
                }
                DiffStatus::Added => {
                    if let Some(new) = item.new.clone() {
                        if lineage_paths.contains(&new.key) {
                            continue;
                        }
                        inputs.push(ContinuityInput {
                            kind: ContinuityInputKind::Seed,
                            from_version_id: report.old_version.version_id.clone(),
                            to_version_id: report.new_version.version_id.clone(),
                            from: None,
                            to: Some(new),
                            relation: VersionContinuityRelation::InsufficientEvidence,
                            status: DiffStatus::Added,
                            confidence: item.confidence,
                            compatibility: item.compatibility.clone(),
                            source: VersionContinuitySource::AddedAsset,
                            reason_codes: item
                                .reasons
                                .iter()
                                .map(|reason| reason.code.clone())
                                .collect(),
                        });
                    }
                }
                DiffStatus::Removed => {
                    if let Some(old) = item.old.clone() {
                        if lineage_paths.contains(&old.key) {
                            continue;
                        }
                        inputs.push(ContinuityInput {
                            kind: ContinuityInputKind::Transition,
                            from_version_id: report.old_version.version_id.clone(),
                            to_version_id: report.new_version.version_id.clone(),
                            from: Some(old),
                            to: None,
                            relation: VersionContinuityRelation::Removed,
                            status: DiffStatus::Removed,
                            confidence: item.confidence,
                            compatibility: item.compatibility.clone(),
                            source: VersionContinuitySource::RemovedAsset,
                            reason_codes: item
                                .reasons
                                .iter()
                                .map(|reason| reason.code.clone())
                                .collect(),
                        });
                    }
                }
                DiffStatus::Changed | DiffStatus::Uncertain => {}
            }
        }
    }

    inputs
}

fn continuity_relation_from_lineage(lineage: &AssetLineageKind) -> VersionContinuityRelation {
    match lineage {
        AssetLineageKind::RenameOrRepath => VersionContinuityRelation::RenameOrRepath,
        AssetLineageKind::ContainerMovement => VersionContinuityRelation::ContainerMovement,
        AssetLineageKind::LayoutDrift => VersionContinuityRelation::LayoutDrift,
        AssetLineageKind::Replacement => VersionContinuityRelation::Replacement,
        AssetLineageKind::Ambiguous => VersionContinuityRelation::Ambiguous,
        AssetLineageKind::InsufficientEvidence => VersionContinuityRelation::InsufficientEvidence,
    }
}

impl From<VersionLineageSource> for VersionContinuitySource {
    fn from(value: VersionLineageSource) -> Self {
        match value {
            VersionLineageSource::ChangedAsset => VersionContinuitySource::ChangedAsset,
            VersionLineageSource::CandidateMapping => VersionContinuitySource::CandidateMapping,
        }
    }
}

fn continuity_input_sort_key(input: &ContinuityInput) -> (u8, String, String, u8) {
    let source_rank = match input.source {
        VersionContinuitySource::UnchangedAsset => 0,
        VersionContinuitySource::ChangedAsset => 1,
        VersionContinuitySource::CandidateMapping => 2,
        VersionContinuitySource::AddedAsset => 3,
        VersionContinuitySource::RemovedAsset => 4,
    };
    let path = input
        .from
        .as_ref()
        .or(input.to.as_ref())
        .map(|item| item.key.clone())
        .unwrap_or_default();
    (
        source_rank,
        input.from_version_id.clone(),
        path,
        if input.kind == ContinuityInputKind::Seed {
            1
        } else {
            0
        },
    )
}

fn summarize_continuity(threads: &[VersionContinuityThread]) -> VersionContinuitySummary {
    let mut summary = VersionContinuitySummary {
        thread_count: threads.len(),
        observation_count: 0,
        introduced_threads: 0,
        persisted_threads: 0,
        rename_or_repath_threads: 0,
        container_movement_threads: 0,
        layout_drift_threads: 0,
        replacement_threads: 0,
        ambiguous_threads: 0,
        insufficient_evidence_threads: 0,
        removed_threads: 0,
        review_required_threads: 0,
        ongoing_threads: 0,
    };

    for thread in threads {
        summary.observation_count += thread.observations.len();
        let continues = thread
            .observations
            .last()
            .is_some_and(|observation| observation.relation.continues_thread());

        if thread.observations.is_empty() {
            summary.introduced_threads += 1;
            summary.ongoing_threads += 1;
            if thread.review_required {
                summary.review_required_threads += 1;
            }
            continue;
        }

        if thread.review_required {
            summary.review_required_threads += 1;
        }
        if matches!(
            thread
                .observations
                .last()
                .map(|observation| &observation.relation),
            Some(VersionContinuityRelation::Removed)
        ) {
            summary.removed_threads += 1;
            continue;
        }
        if matches!(
            thread
                .observations
                .last()
                .map(|observation| &observation.relation),
            Some(VersionContinuityRelation::Replacement)
        ) {
            summary.replacement_threads += 1;
            continue;
        }
        if matches!(
            thread
                .observations
                .last()
                .map(|observation| &observation.relation),
            Some(VersionContinuityRelation::Ambiguous)
        ) {
            summary.ambiguous_threads += 1;
            continue;
        }
        if matches!(
            thread
                .observations
                .last()
                .map(|observation| &observation.relation),
            Some(VersionContinuityRelation::InsufficientEvidence)
        ) {
            summary.insufficient_evidence_threads += 1;
            continue;
        }
        if matches!(
            thread
                .observations
                .last()
                .map(|observation| &observation.relation),
            Some(VersionContinuityRelation::LayoutDrift)
        ) {
            summary.layout_drift_threads += 1;
            if continues {
                summary.ongoing_threads += 1;
            }
            continue;
        }
        if matches!(
            thread
                .observations
                .last()
                .map(|observation| &observation.relation),
            Some(VersionContinuityRelation::ContainerMovement)
        ) {
            summary.container_movement_threads += 1;
            if continues {
                summary.ongoing_threads += 1;
            }
            continue;
        }
        if matches!(
            thread
                .observations
                .last()
                .map(|observation| &observation.relation),
            Some(VersionContinuityRelation::RenameOrRepath)
        ) {
            summary.rename_or_repath_threads += 1;
            if continues {
                summary.ongoing_threads += 1;
            }
            continue;
        }
        summary.persisted_threads += 1;
        if continues {
            summary.ongoing_threads += 1;
        }
    }

    summary
}

fn build_continuity_thread_summaries(
    threads: &[VersionContinuityThread],
) -> Vec<VersionContinuityThreadSummary> {
    threads.iter().map(summarize_continuity_thread).collect()
}

fn summarize_continuity_thread(thread: &VersionContinuityThread) -> VersionContinuityThreadSummary {
    let latest_observed_version = thread
        .observations
        .last()
        .map(|observation| observation.to_version_id.clone())
        .unwrap_or_else(|| thread.anchor_version_id.clone());
    let latest_relation = thread
        .observations
        .last()
        .map(|observation| &observation.relation);
    let latest_live_version = if latest_relation.is_none_or(|relation| relation.continues_thread())
    {
        Some(latest_observed_version.clone())
    } else {
        None
    };
    let terminal_relation = latest_relation
        .filter(|relation| relation.is_terminal())
        .cloned();
    let terminal_version = terminal_relation
        .as_ref()
        .map(|_| latest_observed_version.clone());
    let purely_persisted = !thread.observations.is_empty()
        && thread
            .observations
            .iter()
            .all(|observation| observation.relation == VersionContinuityRelation::Persisted);
    let materially_changed = thread
        .observations
        .iter()
        .any(|observation| observation.relation != VersionContinuityRelation::Persisted);

    VersionContinuityThreadSummary {
        thread_id: thread.thread_id.clone(),
        anchor_version_id: thread.anchor_version_id.clone(),
        anchor: thread.anchor.clone(),
        first_seen_version: thread.anchor_version_id.clone(),
        latest_observed_version,
        latest_live_version,
        first_rename_or_repath_step: first_milestone_step(
            thread,
            VersionContinuityRelation::RenameOrRepath,
        ),
        first_container_movement_step: first_milestone_step(
            thread,
            VersionContinuityRelation::ContainerMovement,
        ),
        first_layout_drift_step: first_milestone_step(
            thread,
            VersionContinuityRelation::LayoutDrift,
        ),
        terminal_relation,
        terminal_version,
        review_required: thread.review_required,
        purely_persisted,
        materially_changed,
    }
}

fn first_milestone_step(
    thread: &VersionContinuityThread,
    relation: VersionContinuityRelation,
) -> Option<VersionContinuityMilestoneStep> {
    thread
        .observations
        .iter()
        .find(|observation| observation.relation == relation)
        .map(|observation| VersionContinuityMilestoneStep {
            from_version_id: observation.from_version_id.clone(),
            to_version_id: observation.to_version_id.clone(),
            from_path: observation.from_path.clone(),
            to_path: observation.to_path.clone(),
        })
}

impl ContinuityInput {
    fn requires_review(&self) -> bool {
        self.status == DiffStatus::Uncertain
            || matches!(
                self.relation,
                VersionContinuityRelation::LayoutDrift
                    | VersionContinuityRelation::Replacement
                    | VersionContinuityRelation::Ambiguous
                    | VersionContinuityRelation::InsufficientEvidence
                    | VersionContinuityRelation::Removed
            )
            || matches!(
                self.compatibility,
                Some(
                    RemapCompatibility::StructurallyRisky | RemapCompatibility::IncompatibleBlocked
                )
            )
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
            compatibility: None,
            old,
            new,
            reasons: reasons.clone(),
        });
        self.items.push(ResonatorItemDiff {
            item_type: ReportItemType::Buffer,
            status,
            confidence: None,
            compatibility: None,
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
            compatibility: Some(candidate.compatibility.clone()),
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
                logical_name: value.logical_name.clone(),
                normalized_name: value.normalized_name.clone(),
                vertex_count: value.vertex_count,
                index_count: value.index_count,
                material_slots: value.material_slots,
                section_count: value.section_count,
                vertex_stride: value.vertex_stride,
                vertex_buffer_count: value.vertex_buffer_count,
                index_format: value.index_format.clone(),
                primitive_topology: value.primitive_topology.clone(),
                layout_markers: value.layout_markers.clone(),
                internal_structure: value.internal_structure.clone(),
                asset_hash: value.asset_hash.clone(),
                shader_hash: value.shader_hash.clone(),
                signature: value.signature.clone(),
                tags: value.tags.clone(),
                source: value.source.clone(),
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

fn build_lineage_section(compare_report: &SnapshotCompareReport) -> VersionLineageSection {
    let mut entries = compare_report
        .changed_assets
        .iter()
        .filter_map(|change| {
            let (Some(old_asset), Some(new_asset)) =
                (change.old_asset.as_ref(), change.new_asset.as_ref())
            else {
                return None;
            };

            Some(VersionLineageEntry {
                source: VersionLineageSource::ChangedAsset,
                status: DiffStatus::Changed,
                lineage: change.lineage.clone(),
                old: Some(old_asset.into()),
                new: Some(new_asset.into()),
                confidence: None,
                compatibility: None,
                reason_codes: change
                    .reasons
                    .iter()
                    .map(|reason| reason.code.clone())
                    .collect(),
            })
        })
        .chain(
            compare_report
                .candidate_mapping_changes
                .iter()
                .map(|candidate| VersionLineageEntry {
                    source: VersionLineageSource::CandidateMapping,
                    status: if candidate.ambiguous {
                        DiffStatus::Uncertain
                    } else {
                        DiffStatus::Changed
                    },
                    lineage: candidate.lineage.clone(),
                    old: Some((&candidate.old_asset).into()),
                    new: Some((&candidate.new_asset).into()),
                    confidence: Some(candidate.confidence),
                    compatibility: Some(candidate.compatibility.clone()),
                    reason_codes: candidate
                        .reasons
                        .iter()
                        .map(|reason| reason.code.clone())
                        .collect(),
                }),
        )
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        lineage_sort_key(left)
            .cmp(&lineage_sort_key(right))
            .then_with(|| {
                left.old
                    .as_ref()
                    .map(|item| item.key.as_str())
                    .cmp(&right.old.as_ref().map(|item| item.key.as_str()))
            })
            .then_with(|| {
                left.new
                    .as_ref()
                    .map(|item| item.key.as_str())
                    .cmp(&right.new.as_ref().map(|item| item.key.as_str()))
            })
    });

    VersionLineageSection {
        summary: summarize_lineage(&entries),
        entries,
    }
}

fn summarize_lineage(entries: &[VersionLineageEntry]) -> VersionLineageSummary {
    let mut summary = VersionLineageSummary {
        total_entries: entries.len(),
        rename_or_repath_assets: 0,
        container_movement_assets: 0,
        layout_drift_assets: 0,
        replacement_assets: 0,
        ambiguous_assets: 0,
        insufficient_evidence_assets: 0,
    };

    for entry in entries {
        match entry.lineage {
            AssetLineageKind::RenameOrRepath => summary.rename_or_repath_assets += 1,
            AssetLineageKind::ContainerMovement => summary.container_movement_assets += 1,
            AssetLineageKind::LayoutDrift => summary.layout_drift_assets += 1,
            AssetLineageKind::Replacement => summary.replacement_assets += 1,
            AssetLineageKind::Ambiguous => summary.ambiguous_assets += 1,
            AssetLineageKind::InsufficientEvidence => summary.insufficient_evidence_assets += 1,
        }
    }

    summary
}

fn lineage_sort_key(entry: &VersionLineageEntry) -> (u8, String, String) {
    let lineage_rank = match entry.lineage {
        AssetLineageKind::RenameOrRepath => 0,
        AssetLineageKind::ContainerMovement => 1,
        AssetLineageKind::LayoutDrift => 2,
        AssetLineageKind::Replacement => 3,
        AssetLineageKind::Ambiguous => 4,
        AssetLineageKind::InsufficientEvidence => 5,
    };
    let source_rank = match entry.source {
        VersionLineageSource::ChangedAsset => 0,
        VersionLineageSource::CandidateMapping => 1,
    };

    (
        lineage_rank * 10 + source_rank,
        entry
            .old
            .as_ref()
            .map(|item| item.key.clone())
            .unwrap_or_default(),
        entry
            .new
            .as_ref()
            .map(|item| item.key.clone())
            .unwrap_or_default(),
    )
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
    let old_quality = summarize_snapshot_capture_quality(old_snapshot);
    let new_quality = summarize_snapshot_capture_quality(new_snapshot);

    let mut notes = vec![
        format!(
            "old snapshot {} scope: acquisition={} mode={} install_or_package_level={} meaningful_content={} meaningful_character={} meaningful_asset_enrichment={} content_like_paths={} character_paths={} non_content_paths={}",
            old_snapshot.version_id,
            old_scope.acquisition_kind.as_deref().unwrap_or("unknown"),
            old_scope.capture_mode.as_deref().unwrap_or("unknown"),
            old_scope.mostly_install_or_package_level,
            old_scope.meaningful_content_coverage,
            old_scope.meaningful_character_coverage,
            old_scope.meaningful_asset_record_enrichment,
            old_scope.coverage.content_like_path_count,
            old_scope.coverage.character_path_count,
            old_scope.coverage.non_content_path_count
        ),
        format!(
            "new snapshot {} scope: acquisition={} mode={} install_or_package_level={} meaningful_content={} meaningful_character={} meaningful_asset_enrichment={} content_like_paths={} character_paths={} non_content_paths={}",
            new_snapshot.version_id,
            new_scope.acquisition_kind.as_deref().unwrap_or("unknown"),
            new_scope.capture_mode.as_deref().unwrap_or("unknown"),
            new_scope.mostly_install_or_package_level,
            new_scope.meaningful_content_coverage,
            new_scope.meaningful_character_coverage,
            new_scope.meaningful_asset_record_enrichment,
            new_scope.coverage.content_like_path_count,
            new_scope.coverage.character_path_count,
            new_scope.coverage.non_content_path_count
        ),
        format!(
            "old snapshot {} quality: launcher={} reuse={} matches_snapshot={} manifest_coverage=resources:{} matched:{} unmatched_snapshot_assets:{} hash_coverage=asset_hashes:{}/{} any_hashes:{}/{} signatures:{}/{} asset_enrichment=source_context:{}/{} rich_metadata:{}/{} enriched_assets:{}/{} extractor_records={}",
            old_snapshot.version_id,
            old_quality
                .launcher_detected_version
                .as_deref()
                .unwrap_or("missing"),
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
            old_quality.extractor_record_count
        ),
        format!(
            "new snapshot {} quality: launcher={} reuse={} matches_snapshot={} manifest_coverage=resources:{} matched:{} unmatched_snapshot_assets:{} hash_coverage=asset_hashes:{}/{} any_hashes:{}/{} signatures:{}/{} asset_enrichment=source_context:{}/{} rich_metadata:{}/{} enriched_assets:{}/{} extractor_records={}",
            new_snapshot.version_id,
            new_quality
                .launcher_detected_version
                .as_deref()
                .unwrap_or("missing"),
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
            new_quality.extractor_record_count
        ),
    ];

    if old_scope.is_low_signal_for_character_analysis()
        || new_scope.is_low_signal_for_character_analysis()
    {
        notes.push(
            "scope warning: compare results are based on shallow filesystem inventory or low-coverage/low-enrichment extractor snapshots; deep character-level interpretation may be limited."
                .to_string(),
        );
    }
    if has_shallow_hash_or_manifest_only_support(&old_scope, &old_quality)
        || has_shallow_hash_or_manifest_only_support(&new_scope, &new_quality)
    {
        notes.push(
            "scope warning: manifest/hash coverage may be present here, but shallow coverage should not be read as rich asset-level enrichment."
                .to_string(),
        );
    }

    notes
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

pub fn load_version_continuity_artifact(
    path: &std::path::Path,
) -> crate::error::AppResult<VersionContinuityArtifact> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

#[cfg(test)]
mod tests {
    use crate::{
        compare::{RiskLevel, SnapshotComparer},
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceScopeContext,
            InferenceSummary, InferredMappingContinuityContext, InferredMappingHint,
        },
        proposal::ProposalEngine,
        snapshot::{
            GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotCoverageSignals,
            SnapshotFingerprint, SnapshotHashFields, SnapshotScopeContext,
        },
        wwmi::WwmiPatternKind,
    };

    use super::{
        DiffStatus, ReportItemType, VersionContinuityRelation, VersionDiffReportBuilder,
        VersionLineageSource,
    };

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
        let changed_asset = report
            .resonators
            .iter()
            .flat_map(|entry| entry.items.iter())
            .find(|item| {
                item.item_type == ReportItemType::Asset && item.status == DiffStatus::Changed
            })
            .expect("changed asset item");
        assert_eq!(
            changed_asset
                .old
                .as_ref()
                .and_then(|item| item.metadata.signature.as_deref()),
            Some("sig-a")
        );
        assert_eq!(
            changed_asset
                .new
                .as_ref()
                .and_then(|item| item.metadata.signature.as_deref()),
            Some("sig-b")
        );
    }

    #[test]
    fn builder_exposes_lineage_section_for_pairwise_classifications() {
        let mut body_old = asset("Content/Character/Encore/Body.mesh", "body", Some(100));
        body_old.source.container_path = Some("pakchunk0-WindowsNoEditor.pak".to_string());
        let mut body_new = asset("Content/Character/Encore/Body.mesh", "body", Some(100));
        body_new.source.container_path = Some("pakchunk1-WindowsNoEditor.pak".to_string());

        let mut face_old = asset("Content/Character/Encore/Face.mesh", "face", Some(80));
        face_old.metadata.index_count = Some(160);
        face_old.fingerprint.index_count = Some(160);
        face_old.metadata.section_count = Some(1);
        face_old.fingerprint.section_count = Some(1);
        let mut face_new = face_old.clone();
        face_new.metadata.vertex_count = Some(120);
        face_new.fingerprint.vertex_count = Some(120);
        face_new.metadata.index_count = Some(240);
        face_new.fingerprint.index_count = Some(240);
        face_new.metadata.section_count = Some(2);
        face_new.fingerprint.section_count = Some(2);

        let mut mask_old = asset("Content/Character/Encore/Mask.mesh", "mask", Some(100));
        mask_old.hash_fields.signature = Some("sig-mask-old".to_string());
        let mut mask_new = asset("Content/Character/Encore/Mask.mesh", "mask", Some(100));
        mask_new.hash_fields.signature = Some("sig-mask-new".to_string());

        let mut charm_old = asset("Content/Character/Encore/Charm.mesh", "charm", Some(40));
        charm_old.fingerprint.tags = vec!["character".to_string(), "v1".to_string()];
        charm_old.metadata.tags = charm_old.fingerprint.tags.clone();
        let mut charm_new = asset("Content/Character/Encore/Charm.mesh", "charm", Some(40));
        charm_new.fingerprint.tags = vec!["character".to_string(), "v2".to_string()];
        charm_new.metadata.tags = charm_new.fingerprint.tags.clone();

        let mut hair_old = asset("Content/Character/Encore/Hair.mesh", "hair", Some(120));
        hair_old.hash_fields.signature = Some("sig-hair".to_string());
        let mut hair_new = asset("Content/Character/Encore/Hair_LOD0.mesh", "hair", Some(120));
        hair_new.hash_fields.signature = Some("sig-hair".to_string());

        let old_snapshot = sample_snapshot(
            "6.0.0",
            vec![body_old, face_old, mask_old, charm_old, hair_old],
        );
        let new_snapshot = sample_snapshot(
            "6.1.0",
            vec![body_new, face_new, mask_new, charm_new, hair_new],
        );
        let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let report =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare_report);

        assert_eq!(report.lineage.summary.rename_or_repath_assets, 1);
        assert_eq!(report.lineage.summary.container_movement_assets, 1);
        assert_eq!(report.lineage.summary.layout_drift_assets, 1);
        assert_eq!(report.lineage.summary.replacement_assets, 1);
        assert_eq!(report.lineage.summary.insufficient_evidence_assets, 1);
        assert_eq!(report.lineage.summary.ambiguous_assets, 0);
        assert_eq!(report.lineage.summary.total_entries, 5);

        let repath = report
            .lineage
            .entries
            .iter()
            .find(|entry| {
                entry.source == VersionLineageSource::CandidateMapping
                    && entry.lineage == crate::compare::AssetLineageKind::RenameOrRepath
            })
            .expect("repath lineage");
        assert_eq!(
            repath.old.as_ref().and_then(|item| item.path.as_deref()),
            Some("Content/Character/Encore/Hair.mesh")
        );
        assert_eq!(
            repath.new.as_ref().and_then(|item| item.path.as_deref()),
            Some("Content/Character/Encore/Hair_LOD0.mesh")
        );

        let container_move = report
            .lineage
            .entries
            .iter()
            .find(|entry| {
                entry.source == VersionLineageSource::ChangedAsset
                    && entry.lineage == crate::compare::AssetLineageKind::ContainerMovement
            })
            .expect("container movement lineage");
        assert_eq!(
            container_move
                .old
                .as_ref()
                .and_then(|item| item.path.as_deref()),
            Some("Content/Character/Encore/Body.mesh")
        );
        assert_eq!(
            container_move
                .new
                .as_ref()
                .and_then(|item| item.path.as_deref()),
            Some("Content/Character/Encore/Body.mesh")
        );

        let replacement = report
            .lineage
            .entries
            .iter()
            .find(|entry| {
                entry.source == VersionLineageSource::ChangedAsset
                    && entry.lineage == crate::compare::AssetLineageKind::Replacement
            })
            .expect("replacement lineage");
        assert_eq!(
            replacement
                .old
                .as_ref()
                .and_then(|item| item.path.as_deref()),
            Some("Content/Character/Encore/Mask.mesh")
        );
        assert_eq!(
            replacement
                .new
                .as_ref()
                .and_then(|item| item.path.as_deref()),
            Some("Content/Character/Encore/Mask.mesh")
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
                old_asset_path: "Content/Weapon/Pistol_Main.weapon".to_string(),
                new_asset_path: "Content/Weapon/Pistol_Main_A.weapon".to_string(),
                confidence: 0.91,
                compatibility: crate::compare::RemapCompatibility::LikelyCompatible,
                needs_review: false,
                ambiguous: false,
                confidence_gap: Some(0.2),
                continuity: None,
                reasons: vec!["strong evidence".to_string()],
                evidence: Vec::new(),
            }],
            representative_risk_projections: Vec::new(),
        };

        let report = VersionDiffReportBuilder.enrich_with_inference(report, &inference);
        let mapping_item = report
            .resonators
            .iter()
            .flat_map(|entry| entry.items.iter())
            .find(|item| item.item_type == ReportItemType::Mapping)
            .expect("mapping item");

        assert_eq!(mapping_item.confidence, Some(0.91));
        assert_eq!(
            mapping_item.compatibility,
            Some(crate::compare::RemapCompatibility::LikelyCompatible)
        );
        assert_eq!(mapping_item.status, DiffStatus::Changed);
    }

    #[test]
    fn builder_enriches_report_with_continuity_review_surface() {
        let old_snapshot = sample_snapshot(
            "8.0.0",
            vec![asset(
                "Content/Character/Encore/Body.mesh",
                "encore body",
                Some(100),
            )],
        );
        let new_snapshot = sample_snapshot(
            "8.1.0",
            vec![asset(
                "Content/Character/Encore/Body_v2.mesh",
                "encore body",
                Some(100),
            )],
        );
        let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let report =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare_report);
        let inference = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "8.0.0".to_string(),
                new_version_id: "8.1.0".to_string(),
                changed_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 1,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 2,
                fix_like_commits: 1,
                discovered_patterns: 1,
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
            probable_crash_causes: vec![crate::inference::ProbableCrashCause {
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
            suggested_fixes: vec![crate::inference::SuggestedFix {
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
        };
        let proposals = ProposalEngine.generate(&inference, 0.85);

        let report = VersionDiffReportBuilder.enrich_with_review_surface(
            report,
            &inference,
            &proposals.mapping_proposal,
        );

        assert!(report.review.summary.continuity_caution_present);
        assert_eq!(report.review.summary.review_mapping_count, 1);
        assert_eq!(report.review.summary.continuity_review_mapping_count, 1);
        assert_eq!(report.review.continuity.cause_count, 1);
        assert_eq!(report.review.continuity.fix_count, 1);
        assert_eq!(report.review.continuity.review_mapping_count, 1);
        assert!(
            report
                .review
                .continuity
                .notes
                .iter()
                .any(|note| { note.contains("continuity-backed caution present") })
        );
        let mapping = report
            .review
            .continuity
            .mappings
            .first()
            .expect("continuity review mapping");
        assert_eq!(mapping.status, crate::proposal::ProposalStatus::NeedsReview);
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
    }

    #[test]
    fn report_defaults_review_section_for_legacy_json() {
        let legacy = serde_json::json!({
            "schema_version": "whashreonator.report.v2",
            "generated_at_unix_ms": 1,
            "old_version": {
                "version_id": "2.4.0",
                "source_root": "old",
                "asset_count": 1
            },
            "new_version": {
                "version_id": "2.5.0",
                "source_root": "new",
                "asset_count": 1
            },
            "resonators": [],
            "lineage": {
                "summary": {
                    "total_entries": 0,
                    "rename_or_repath_assets": 0,
                    "container_movement_assets": 0,
                    "layout_drift_assets": 0,
                    "replacement_assets": 0,
                    "ambiguous_assets": 0,
                    "insufficient_evidence_assets": 0
                },
                "entries": []
            },
            "summary": {
                "resonator_count": 0,
                "unchanged_items": 0,
                "changed_items": 0,
                "added_items": 0,
                "removed_items": 0,
                "uncertain_items": 0,
                "mapping_candidates": 0
            },
            "scope_notes": []
        });

        let parsed: super::VersionDiffReportV2 =
            serde_json::from_value(legacy).expect("parse legacy report");

        assert_eq!(parsed.review, super::VersionReviewSection::default());
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
                acquisition_kind: Some("extractor_backed_asset_records".to_string()),
                capture_mode: Some("extractor_backed_asset_records".to_string()),
                mostly_install_or_package_level: Some(false),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(true),
                meaningful_asset_record_enrichment: Some(true),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 12,
                    character_path_count: 6,
                    non_content_path_count: 1,
                },
                note: Some("extractor-backed asset records".to_string()),
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
                acquisition_kind: Some("extractor_backed_asset_records".to_string()),
                capture_mode: Some("extractor_backed_asset_records".to_string()),
                mostly_install_or_package_level: Some(false),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(true),
                meaningful_asset_record_enrichment: Some(true),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 14,
                    character_path_count: 7,
                    non_content_path_count: 1,
                },
                note: Some("extractor-backed asset records".to_string()),
            },
        );
        let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

        let report =
            VersionDiffReportBuilder.from_compare(&old_snapshot, &new_snapshot, &compare_report);

        assert_eq!(report.scope_notes.len(), 4);
        assert!(
            report
                .scope_notes
                .iter()
                .any(|note| note.contains("quality: launcher=missing")
                    && note.contains("manifest_coverage=resources:"))
        );
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
                extractor: None,
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
                tags: Vec::new(),
                vertex_count,
                index_count: Some(2),
                material_slots: Some(1),
                section_count: Some(1),
                ..Default::default()
            },
            hash_fields: SnapshotHashFields {
                asset_hash: None,
                shader_hash: None,
                signature: match vertex_count {
                    Some(100) => Some("sig-a".to_string()),
                    Some(120) => Some("sig-b".to_string()),
                    _ => None,
                },
            },
            source: crate::domain::AssetSourceContext::default(),
        }
    }
}
