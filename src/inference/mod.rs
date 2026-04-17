use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    compare::{
        CandidateMappingChange, RemapCompatibility, RiskLevel, SnapshotAssetChange,
        SnapshotChangeType, SnapshotCompareReason, SnapshotCompareReport,
        load_snapshot_compare_report,
    },
    error::{AppError, AppResult},
    report::{
        VersionContinuityIndex, VersionContinuityObservation, VersionContinuityRelation,
        VersionContinuityThread, VersionContinuityThreadSummary,
    },
    wwmi::{
        WwmiKnowledgeBase, WwmiPatternKind,
        dependency::{
            WwmiModDependencyBaselineSet, WwmiModDependencyBaselineStrength, WwmiModDependencyKind,
            WwmiModDependencyProfile, WwmiModDependencySurfaceClass,
        },
        load_wwmi_knowledge,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferenceReport {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub compare_input: InferenceCompareInput,
    pub knowledge_input: InferenceKnowledgeInput,
    #[serde(default)]
    pub mod_dependency_input: Option<InferenceModDependencyInput>,
    #[serde(default)]
    pub representative_mod_baseline_input: Option<InferenceRepresentativeModBaselineInput>,
    #[serde(default)]
    pub scope: InferenceScopeContext,
    pub summary: InferenceSummary,
    pub probable_crash_causes: Vec<ProbableCrashCause>,
    pub suggested_fixes: Vec<SuggestedFix>,
    pub candidate_mapping_hints: Vec<InferredMappingHint>,
    #[serde(default)]
    pub surface_intersection: InferenceSurfaceIntersection,
    #[serde(default)]
    pub representative_risk_projections: Vec<RepresentativeModRiskProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferenceScopeContext {
    pub low_signal_compare: bool,
    pub old_snapshot_low_signal: bool,
    pub new_snapshot_low_signal: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InferenceCompareInput {
    pub old_version_id: String,
    pub new_version_id: String,
    pub changed_assets: usize,
    pub added_assets: usize,
    pub removed_assets: usize,
    pub candidate_mapping_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InferenceKnowledgeInput {
    pub repo: String,
    pub analyzed_commits: usize,
    pub fix_like_commits: usize,
    pub discovered_patterns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferenceModDependencyInput {
    pub mod_name: Option<String>,
    pub mod_root: String,
    pub ini_file_count: usize,
    pub signal_count: usize,
    pub dependency_kinds: Vec<WwmiModDependencyKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferenceRepresentativeModBaselineInput {
    pub version_id: String,
    pub profile_count: usize,
    #[serde(default)]
    pub included_mod_count: usize,
    #[serde(default)]
    pub represented_surface_classes: Vec<WwmiModDependencySurfaceClass>,
    #[serde(default)]
    pub risk_class_counts: Vec<InferenceRepresentativeRiskClassCount>,
    #[serde(default)]
    pub strength: WwmiModDependencyBaselineStrength,
    #[serde(default)]
    pub material_for_repair_review: bool,
    #[serde(default)]
    pub caution_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferenceRepresentativeRiskClassCount {
    pub risk_class: RepresentativeModRiskClass,
    pub profile_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferenceSurfaceIntersection {
    pub mod_side_surfaces: Vec<InferenceModSideSurface>,
    pub game_side_surfaces: Vec<InferenceGameSideSurface>,
    pub overlapping_surface_classes: Vec<WwmiModDependencySurfaceClass>,
    #[serde(default)]
    pub overlap_posture: InferenceSurfaceOverlapPosture,
    pub weak_or_absent_overlap: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum InferenceSurfaceOverlapPosture {
    #[default]
    None,
    Partial,
    Strong,
}

impl InferenceSurfaceOverlapPosture {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Partial => "partial",
            Self::Strong => "strong",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferenceModSideSurface {
    pub surface_class: WwmiModDependencySurfaceClass,
    pub sources: Vec<String>,
    pub dependency_kinds: Vec<WwmiModDependencyKind>,
    pub signal_count: usize,
    pub representative_profile_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferenceGameSideSurface {
    pub surface_class: WwmiModDependencySurfaceClass,
    pub compare_signals: Vec<String>,
    pub affected_assets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferenceSummary {
    pub probable_crash_causes: usize,
    pub suggested_fixes: usize,
    pub candidate_mapping_hints: usize,
    pub highest_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbableCrashCause {
    pub code: String,
    pub summary: String,
    pub confidence: f32,
    pub risk: RiskLevel,
    pub affected_assets: Vec<String>,
    pub related_patterns: Vec<WwmiPatternKind>,
    pub reasons: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuggestedFix {
    pub code: String,
    pub summary: String,
    pub confidence: f32,
    pub priority: RiskLevel,
    pub related_patterns: Vec<WwmiPatternKind>,
    pub actions: Vec<String>,
    pub reasons: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferredMappingHint {
    pub old_asset_path: String,
    pub new_asset_path: String,
    pub confidence: f32,
    #[serde(default)]
    pub compatibility: RemapCompatibility,
    pub needs_review: bool,
    #[serde(default)]
    pub ambiguous: bool,
    #[serde(default)]
    pub confidence_gap: Option<f32>,
    #[serde(default)]
    pub continuity: Option<InferredMappingContinuityContext>,
    pub reasons: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum RepresentativeModRiskClass {
    #[default]
    MappingHashSensitive,
    BufferLayoutSensitive,
    ResourceSkeletonSensitive,
    DrawCallFilterHookSensitive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepresentativeModRiskProjection {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InferredMappingContinuityContext {
    pub thread_id: Option<String>,
    pub first_seen_version: Option<String>,
    pub latest_observed_version: Option<String>,
    pub latest_live_version: Option<String>,
    pub stable_before_current_change: bool,
    pub total_rename_steps: usize,
    pub total_container_movement_steps: usize,
    pub total_layout_drift_steps: usize,
    pub review_required_history: bool,
    pub terminal_relation: Option<VersionContinuityRelation>,
    pub terminal_version: Option<String>,
    pub terminal_after_current: bool,
    pub instability_detected: bool,
}

impl InferredMappingContinuityContext {
    pub fn has_review_caution(&self) -> bool {
        self.instability_detected
            || self.total_layout_drift_steps >= 2
            || self.review_required_history
            || self.terminal_relation.is_some()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FixInferenceEngine;

#[derive(Debug, Clone, Default)]
struct InferenceContinuityContext {
    change_signals: BTreeMap<(String, Option<String>), ContinuitySignal>,
    mapping_signals: BTreeMap<(String, String), ContinuitySignal>,
}

#[derive(Debug, Clone)]
struct ContinuitySignal {
    thread_id: String,
    current_from_path: String,
    current_to_path: Option<String>,
    first_seen_version: String,
    latest_observed_version: String,
    latest_live_version: Option<String>,
    current_relation: VersionContinuityRelation,
    prior_persisted_steps: usize,
    prior_rename_steps: usize,
    prior_container_movement_steps: usize,
    prior_layout_drift_steps: usize,
    prior_material_steps: usize,
    later_rename_steps: usize,
    later_container_movement_steps: usize,
    later_layout_drift_steps: usize,
    terminal_relation: Option<VersionContinuityRelation>,
    terminal_version: Option<String>,
    review_required: bool,
}

#[derive(Debug, Clone)]
struct ModDependencyInsights {
    input: InferenceModDependencyInput,
    mapping_hash: Option<ModDependencySurfaceSummary>,
    buffer_layout: Option<ModDependencySurfaceSummary>,
    resource_or_skeleton: Option<ModDependencySurfaceSummary>,
    hook_targeting: Option<ModDependencySurfaceSummary>,
}

#[derive(Debug, Clone)]
struct RepresentativeModBaselineInsights {
    input: InferenceRepresentativeModBaselineInput,
    mapping_hash: Option<RepresentativeBaselineSurfaceSummary>,
    buffer_layout: Option<RepresentativeBaselineSurfaceSummary>,
    resource_or_skeleton: Option<RepresentativeBaselineSurfaceSummary>,
    draw_call_filter_hook: Option<RepresentativeBaselineSurfaceSummary>,
}

#[derive(Debug, Clone)]
struct ModDependencySurfaceSummary {
    dependency_kinds: Vec<WwmiModDependencyKind>,
    signal_count: usize,
    source_files: Vec<String>,
    sections: Vec<String>,
    label: &'static str,
}

#[derive(Debug, Clone)]
struct RepresentativeBaselineSurfaceSummary {
    risk_class: RepresentativeModRiskClass,
    profile_count: usize,
    sample_mod_names: Vec<String>,
    dependency_kinds: Vec<WwmiModDependencyKind>,
    label: &'static str,
}

#[derive(Debug, Clone, Default)]
struct SurfaceIntersectionBuildEntry {
    sources: BTreeSet<String>,
    dependency_kinds: BTreeSet<WwmiModDependencyKind>,
    signal_count: usize,
    representative_profile_count: usize,
}

impl FixInferenceEngine {
    pub fn infer_files(
        &self,
        compare_report_path: &std::path::Path,
        wwmi_knowledge_path: &std::path::Path,
    ) -> AppResult<InferenceReport> {
        let compare_report = load_snapshot_compare_report(compare_report_path)?;
        let wwmi_knowledge = load_wwmi_knowledge(wwmi_knowledge_path)?;
        Ok(self.infer(&compare_report, &wwmi_knowledge))
    }

    pub fn infer(
        &self,
        compare_report: &SnapshotCompareReport,
        wwmi_knowledge: &WwmiKnowledgeBase,
    ) -> InferenceReport {
        self.infer_with_context(compare_report, wwmi_knowledge, None, None, None)
    }

    pub fn infer_with_continuity(
        &self,
        compare_report: &SnapshotCompareReport,
        wwmi_knowledge: &WwmiKnowledgeBase,
        continuity: Option<&VersionContinuityIndex>,
    ) -> InferenceReport {
        self.infer_with_context(compare_report, wwmi_knowledge, continuity, None, None)
    }

    pub fn infer_with_continuity_and_mod_profile(
        &self,
        compare_report: &SnapshotCompareReport,
        wwmi_knowledge: &WwmiKnowledgeBase,
        continuity: Option<&VersionContinuityIndex>,
        mod_dependency_profile: Option<&WwmiModDependencyProfile>,
    ) -> InferenceReport {
        self.infer_with_context(
            compare_report,
            wwmi_knowledge,
            continuity,
            mod_dependency_profile,
            None,
        )
    }

    pub fn infer_with_context(
        &self,
        compare_report: &SnapshotCompareReport,
        wwmi_knowledge: &WwmiKnowledgeBase,
        continuity: Option<&VersionContinuityIndex>,
        mod_dependency_profile: Option<&WwmiModDependencyProfile>,
        representative_mod_baseline_set: Option<&WwmiModDependencyBaselineSet>,
    ) -> InferenceReport {
        let mod_dependency = mod_dependency_profile.map(build_mod_dependency_insights);
        let representative_mod_baseline =
            representative_mod_baseline_set.map(build_representative_mod_baseline_insights);
        let surface_intersection = build_surface_intersection(
            compare_report,
            mod_dependency.as_ref(),
            representative_mod_baseline.as_ref(),
        );
        let scope = build_inference_scope_context(
            compare_report,
            mod_dependency.as_ref(),
            representative_mod_baseline.as_ref(),
            &surface_intersection,
        );
        let confidence_scale = if scope.low_signal_compare { 0.85 } else { 1.0 };
        let scope_induced_removals_likely = compare_report.scope.scope_induced_removals_likely;
        let continuity_context =
            continuity.map(|index| build_inference_continuity_context(compare_report, index));

        let mapping_support = pattern_support(wwmi_knowledge, WwmiPatternKind::MappingOrHashUpdate);
        let buffer_support =
            pattern_support(wwmi_knowledge, WwmiPatternKind::BufferLayoutOrCapacityFix);
        let runtime_support = pattern_support(wwmi_knowledge, WwmiPatternKind::RuntimeConfigChange);
        let shader_support = pattern_support(wwmi_knowledge, WwmiPatternKind::ShaderLogicChange);
        let timing_support =
            pattern_support(wwmi_knowledge, WwmiPatternKind::StartupTimingAdjustment);

        let mut probable_crash_causes = Vec::new();
        let mut suggested_fixes = Vec::new();

        let structural_changes = compare_report
            .changed_assets
            .iter()
            .filter(|change| is_structural_change(change))
            .collect::<Vec<_>>();
        if !structural_changes.is_empty() {
            let confidence = (0.50
                + 0.20 * buffer_support
                + 0.10 * runtime_support
                + 0.10 * shader_support
                + 0.10)
                .clamp(0.0, 1.0)
                * confidence_scale;
            let affected_assets = structural_changes
                .iter()
                .filter_map(|change| change.new_asset.as_ref().or(change.old_asset.as_ref()))
                .map(|asset| asset.path.clone())
                .collect::<Vec<_>>();
            let changed_fields = structural_changes
                .iter()
                .flat_map(|change| change.changed_fields.iter().cloned())
                .collect::<Vec<_>>();
            probable_crash_causes.push(ProbableCrashCause {
                code: "buffer_layout_changed".to_string(),
                summary: "Structural asset metadata changed between game versions; mods may be reading outdated buffer/layout assumptions.".to_string(),
                confidence,
                risk: RiskLevel::High,
                affected_assets,
                related_patterns: vec![
                    WwmiPatternKind::BufferLayoutOrCapacityFix,
                    WwmiPatternKind::RuntimeConfigChange,
                    WwmiPatternKind::ShaderLogicChange,
                ],
                reasons: vec![
                    format!("changed structural fields: {}", changed_fields.join(", ")),
                    "compare report marks these assets as changed with structural differences".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[
                        WwmiPatternKind::BufferLayoutOrCapacityFix,
                        WwmiPatternKind::RuntimeConfigChange,
                        WwmiPatternKind::ShaderLogicChange,
                    ],
                ),
            });
            suggested_fixes.push(SuggestedFix {
                code: "review_buffer_layout_and_runtime_guards".to_string(),
                summary: "Review mappings and runtime assumptions for buffer size/layout changes before re-enabling the mod.".to_string(),
                confidence,
                priority: RiskLevel::High,
                related_patterns: vec![
                    WwmiPatternKind::BufferLayoutOrCapacityFix,
                    WwmiPatternKind::RuntimeConfigChange,
                ],
                actions: vec![
                    "Compare vertex_count/index_count/material_slots/section_count and any internal section/binding signals against the previous version.".to_string(),
                    "Check whether the mod needs a new mapping or a larger buffer/layout-aware path similar to WWMI buffer-capacity fixes.".to_string(),
                    "Avoid auto-applying mappings for these assets until the new layout is validated.".to_string(),
                ],
                reasons: vec![
                    "WWMI history shows recurring fixes around buffer/layout/capacity changes.".to_string(),
                    "The snapshot diff shows structure-level metadata drift on affected assets.".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[
                        WwmiPatternKind::BufferLayoutOrCapacityFix,
                        WwmiPatternKind::RuntimeConfigChange,
                    ],
                ),
            });
        }

        let removed_assets = compare_report
            .removed_assets
            .iter()
            .filter(|change| change.change_type == SnapshotChangeType::Removed)
            .collect::<Vec<_>>();
        let plausible_repair_candidates = compare_report
            .candidate_mapping_changes
            .iter()
            .filter(|candidate| {
                matches!(
                    candidate.compatibility,
                    RemapCompatibility::LikelyCompatible
                        | RemapCompatibility::CompatibleWithCaution
                        | RemapCompatibility::StructurallyRisky
                )
            })
            .collect::<Vec<_>>();
        if scope_induced_removals_likely && !removed_assets.is_empty() {
            // Scope narrowing can hide paths without representing true version drift.
        } else if !removed_assets.is_empty() && !plausible_repair_candidates.is_empty() {
            let confidence = (0.55 + 0.25 * mapping_support + 0.10 * shader_support + 0.05)
                .clamp(0.0, 1.0)
                * confidence_scale;
            probable_crash_causes.push(ProbableCrashCause {
                code: "asset_paths_or_mapping_shifted".to_string(),
                summary: "Assets disappeared from their old paths but plausible replacements exist in the new snapshot; the mod likely needs remapping.".to_string(),
                confidence,
                risk: RiskLevel::High,
                affected_assets: plausible_repair_candidates
                    .iter()
                    .map(|candidate| format!("{} -> {}", candidate.old_asset.path, candidate.new_asset.path))
                    .collect(),
                related_patterns: vec![
                    WwmiPatternKind::MappingOrHashUpdate,
                    WwmiPatternKind::ShaderLogicChange,
                ],
                reasons: vec![
                    format!(
                        "{} removed assets have candidate replacements",
                        plausible_repair_candidates.len()
                    ),
                    "compare heuristics found plausible old->new asset remap candidates".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[
                        WwmiPatternKind::MappingOrHashUpdate,
                        WwmiPatternKind::ShaderLogicChange,
                    ],
                ),
            });
            suggested_fixes.push(SuggestedFix {
                code: "review_candidate_asset_remaps".to_string(),
                summary: "Review the inferred old->new asset remap candidates and update mappings for removed paths.".to_string(),
                confidence,
                priority: RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec![
                    "Inspect candidate_mapping_hints and verify that the replacement asset is semantically equivalent.".to_string(),
                    "If validated, prepare a mapping proposal instead of patching blindly.".to_string(),
                ],
                reasons: vec![
                    "WWMI history frequently resolves update breakage through mapping/hash updates.".to_string(),
                    "The compare report shows removed assets with plausible replacements.".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[WwmiPatternKind::MappingOrHashUpdate],
                ),
            });
        } else if !removed_assets.is_empty() {
            let confidence = (0.45 + 0.20 * mapping_support).clamp(0.0, 1.0) * confidence_scale;
            probable_crash_causes.push(ProbableCrashCause {
                code: "asset_removed_without_clear_replacement".to_string(),
                summary: "Some old asset paths disappeared and the tool could not find strong replacements; the mod may target stale assets.".to_string(),
                confidence,
                risk: RiskLevel::Medium,
                affected_assets: removed_assets
                    .iter()
                    .filter_map(|change| change.old_asset.as_ref())
                    .map(|asset| asset.path.clone())
                    .collect(),
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                reasons: vec![
                    "removed assets exist without strong compare-level remap candidates".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[WwmiPatternKind::MappingOrHashUpdate],
                ),
            });
        }

        let structurally_drifted_remaps = compare_report
            .candidate_mapping_changes
            .iter()
            .filter(|candidate| candidate.compatibility == RemapCompatibility::StructurallyRisky)
            .collect::<Vec<_>>();
        if !structurally_drifted_remaps.is_empty() {
            let confidence = (0.52 + 0.22 * buffer_support + 0.18 * mapping_support)
                .clamp(0.0, 1.0)
                * confidence_scale;
            probable_crash_causes.push(ProbableCrashCause {
                code: "candidate_remap_structural_drift".to_string(),
                summary: "Some remap candidates look logically related, but their structural metadata drifted; a path remap alone may still break the mod.".to_string(),
                confidence,
                risk: RiskLevel::High,
                affected_assets: structurally_drifted_remaps
                    .iter()
                    .flat_map(|candidate| {
                        [
                            candidate.old_asset.path.clone(),
                            candidate.new_asset.path.clone(),
                        ]
                    })
                    .collect(),
                related_patterns: vec![
                    WwmiPatternKind::BufferLayoutOrCapacityFix,
                    WwmiPatternKind::MappingOrHashUpdate,
                ],
                reasons: vec![
                    "compare found candidate remaps that still carry structural drift".to_string(),
                    "same logical asset does not automatically mean layout-compatible replacement".to_string(),
                ],
                evidence: structurally_drifted_remaps
                    .iter()
                    .flat_map(|candidate| {
                        candidate
                            .reasons
                .iter()
                .filter(|reason| {
                    matches!(
                        reason.code.as_str(),
                        "same_asset_but_structural_drift"
                            | "buffer_layout_validation_needed"
                            | "vertex_count_mismatch"
                            | "index_count_mismatch"
                            | "material_slots_mismatch"
                            | "section_count_mismatch"
                            | "vertex_stride_mismatch"
                            | "vertex_buffer_count_mismatch"
                            | "index_format_mismatch"
                            | "primitive_topology_mismatch"
                            | "layout_markers_mismatch"
                    )
                })
                .map(reason_to_string)
            })
                    .chain(pattern_evidence(
                        wwmi_knowledge,
                        &[
                            WwmiPatternKind::BufferLayoutOrCapacityFix,
                            WwmiPatternKind::MappingOrHashUpdate,
                        ],
                    ))
                    .collect(),
            });
            suggested_fixes.push(SuggestedFix {
                code: "validate_candidate_remaps_against_layout".to_string(),
                summary: "Validate remap candidates against the new mesh/buffer layout before applying path updates.".to_string(),
                confidence,
                priority: RiskLevel::High,
                related_patterns: vec![
                    WwmiPatternKind::BufferLayoutOrCapacityFix,
                    WwmiPatternKind::MappingOrHashUpdate,
                ],
                actions: vec![
                    "Review candidate remaps that report structural drift and compare their vertex/index/material/section metadata.".to_string(),
                    "Keep these mappings in review state until the replacement asset is proven layout-compatible.".to_string(),
                ],
                reasons: vec![
                    "compare surfaced remap candidates with structural drift".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[
                        WwmiPatternKind::BufferLayoutOrCapacityFix,
                        WwmiPatternKind::MappingOrHashUpdate,
                    ],
                ),
            });
        }

        let incompatible_candidates = compare_report
            .candidate_mapping_changes
            .iter()
            .filter(|candidate| candidate.compatibility == RemapCompatibility::IncompatibleBlocked)
            .collect::<Vec<_>>();
        if !incompatible_candidates.is_empty() {
            let confidence = (0.48 + 0.24 * mapping_support + 0.12 * shader_support)
                .clamp(0.0, 1.0)
                * confidence_scale;
            probable_crash_causes.push(ProbableCrashCause {
                code: "candidate_remap_identity_conflict".to_string(),
                summary: "Some high-similarity remap candidates are blocked by identity conflicts; the next step is manual inspection, not auto-remap.".to_string(),
                confidence,
                risk: RiskLevel::High,
                affected_assets: incompatible_candidates
                    .iter()
                    .flat_map(|candidate| {
                        [
                            candidate.old_asset.path.clone(),
                            candidate.new_asset.path.clone(),
                        ]
                    })
                    .collect(),
                related_patterns: vec![
                    WwmiPatternKind::MappingOrHashUpdate,
                    WwmiPatternKind::ShaderLogicChange,
                ],
                reasons: vec![
                    "candidate remaps contain explicit identity conflicts".to_string(),
                ],
                evidence: incompatible_candidates
                    .iter()
                    .flat_map(|candidate| {
                        candidate
                            .reasons
                            .iter()
                            .filter(|reason| {
                                matches!(
                                    reason.code.as_str(),
                                    "identity_conflict_detected"
                                        | "kind_mismatch"
                                        | "signature_mismatch"
                                )
                            })
                            .map(reason_to_string)
                    })
                    .chain(pattern_evidence(
                        wwmi_knowledge,
                        &[
                            WwmiPatternKind::MappingOrHashUpdate,
                            WwmiPatternKind::ShaderLogicChange,
                        ],
                    ))
                    .collect(),
            });
            suggested_fixes.push(SuggestedFix {
                code: "inspect_identity_conflicts_before_remap".to_string(),
                summary: "Inspect candidates with blocked identity signals before attempting any remap update.".to_string(),
                confidence,
                priority: RiskLevel::High,
                related_patterns: vec![
                    WwmiPatternKind::MappingOrHashUpdate,
                    WwmiPatternKind::ShaderLogicChange,
                ],
                actions: vec![
                    "Review conflicting kind/signature/hash signals on blocked candidates.".to_string(),
                    "Do not auto-apply these remaps until a human confirms the replacement asset.".to_string(),
                ],
                reasons: vec![
                    "candidate compatibility is blocked by conflicting identity evidence".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[
                        WwmiPatternKind::MappingOrHashUpdate,
                        WwmiPatternKind::ShaderLogicChange,
                    ],
                ),
            });
        }

        let insufficient_candidates = compare_report
            .candidate_mapping_changes
            .iter()
            .filter(|candidate| candidate.compatibility == RemapCompatibility::InsufficientEvidence)
            .collect::<Vec<_>>();
        if !insufficient_candidates.is_empty() {
            let confidence = (0.32 + 0.18 * mapping_support).clamp(0.0, 1.0) * confidence_scale;
            probable_crash_causes.push(ProbableCrashCause {
                code: "candidate_remap_insufficient_evidence".to_string(),
                summary: "Some remap candidates lack enough asset-level evidence to recommend a safe repair path yet.".to_string(),
                confidence,
                risk: RiskLevel::Medium,
                affected_assets: insufficient_candidates
                    .iter()
                    .flat_map(|candidate| {
                        [
                            candidate.old_asset.path.clone(),
                            candidate.new_asset.path.clone(),
                        ]
                    })
                    .collect(),
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                reasons: vec![
                    "candidate compatibility remained in insufficient-evidence state".to_string(),
                ],
                evidence: insufficient_candidates
                    .iter()
                    .flat_map(|candidate| {
                        candidate
                            .reasons
                            .iter()
                            .filter(|reason| {
                                matches!(
                                    reason.code.as_str(),
                                    "weak_identity_evidence"
                                        | "ambiguous_runner_up"
                                        | "compatibility_insufficient_evidence"
                                )
                            })
                            .map(reason_to_string)
                    })
                    .chain(pattern_evidence(
                        wwmi_knowledge,
                        &[WwmiPatternKind::MappingOrHashUpdate],
                    ))
                    .collect(),
            });
            suggested_fixes.push(SuggestedFix {
                code: "gather_stronger_asset_evidence".to_string(),
                summary: "Gather stronger asset-level evidence before treating low-evidence remap candidates as repair-safe.".to_string(),
                confidence,
                priority: RiskLevel::Medium,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec![
                    "Prefer prepared asset-level snapshots with richer identity or structural metadata.".to_string(),
                    "Keep low-evidence remap candidates in manual review until stronger evidence appears.".to_string(),
                ],
                reasons: vec![
                    "snapshot evidence is insufficient to classify some remaps as repair-safe".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[WwmiPatternKind::MappingOrHashUpdate],
                ),
            });
        }

        let name_or_hash_changes = compare_report
            .changed_assets
            .iter()
            .filter(|change| {
                change.changed_fields.iter().any(|field| {
                    matches!(
                        field.as_str(),
                        "normalized_name"
                            | "logical_name"
                            | "asset_hash"
                            | "shader_hash"
                            | "signature"
                            | "kind"
                    )
                })
            })
            .collect::<Vec<_>>();
        if !name_or_hash_changes.is_empty() {
            let confidence = (0.45 + 0.20 * mapping_support + 0.10 * shader_support)
                .clamp(0.0, 1.0)
                * confidence_scale;
            probable_crash_causes.push(ProbableCrashCause {
                code: "asset_signature_or_hash_changed".to_string(),
                summary: "Asset identity signals changed across versions; shader or hash-based targeting may no longer match.".to_string(),
                confidence,
                risk: if name_or_hash_changes.iter().any(|change| change.crash_risk == RiskLevel::High) {
                    RiskLevel::High
                } else {
                    RiskLevel::Medium
                },
                affected_assets: name_or_hash_changes
                    .iter()
                    .filter_map(|change| change.new_asset.as_ref().or(change.old_asset.as_ref()))
                    .map(|asset| asset.path.clone())
                    .collect(),
                related_patterns: vec![
                    WwmiPatternKind::MappingOrHashUpdate,
                    WwmiPatternKind::ShaderLogicChange,
                ],
                reasons: vec![
                    "changed assets include name/hash/signature fields".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[
                        WwmiPatternKind::MappingOrHashUpdate,
                        WwmiPatternKind::ShaderLogicChange,
                    ],
                ),
            });
        }

        if probable_crash_causes.is_empty() && timing_support >= 0.50 {
            probable_crash_causes.push(ProbableCrashCause {
                code: "possible_runtime_initialization_issue".to_string(),
                summary: "The snapshot diff alone is inconclusive; WWMI history suggests some crashes are caused by timing/init-order issues rather than asset inventory drift.".to_string(),
                confidence: (0.25 + 0.25 * timing_support).clamp(0.0, 1.0) * confidence_scale,
                risk: RiskLevel::Low,
                affected_assets: Vec::new(),
                related_patterns: vec![WwmiPatternKind::StartupTimingAdjustment],
                reasons: vec![
                    "compare report did not surface strong asset-diff evidence".to_string(),
                    "WWMI history contains startup/timing-related fixes".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[WwmiPatternKind::StartupTimingAdjustment],
                ),
            });
            suggested_fixes.push(SuggestedFix {
                code: "review_runtime_init_path".to_string(),
                summary: "If asset diffs do not explain the crash, review startup/init ordering and runtime guards before patching mappings.".to_string(),
                confidence: (0.25 + 0.25 * timing_support).clamp(0.0, 1.0) * confidence_scale,
                priority: RiskLevel::Low,
                related_patterns: vec![WwmiPatternKind::StartupTimingAdjustment],
                actions: vec![
                    "Compare current runtime config against known-working WWMI timing-related fixes.".to_string(),
                    "Check whether the crash happens before mod data is actually consumed.".to_string(),
                ],
                reasons: vec![
                    "WWMI history shows timing/init changes as a recurring crash-fix pattern.".to_string(),
                ],
                evidence: pattern_evidence(
                    wwmi_knowledge,
                    &[WwmiPatternKind::StartupTimingAdjustment],
                ),
            });
        }

        let mut candidate_mapping_hints = compare_report
            .candidate_mapping_changes
            .iter()
            .map(|candidate| {
                let continuity_signal = continuity_context.as_ref().and_then(|context| {
                    context.mapping_signals.get(&(
                        candidate.old_asset.path.clone(),
                        candidate.new_asset.path.clone(),
                    ))
                });
                infer_mapping_hint(
                    candidate,
                    mapping_support,
                    &scope,
                    continuity_signal,
                    &compare_report.new_snapshot.version_id,
                )
            })
            .collect::<Vec<_>>();

        if let Some(continuity_context) = continuity_context.as_ref() {
            apply_continuity_inference_adjustments(
                &mut probable_crash_causes,
                &mut suggested_fixes,
                compare_report,
                continuity_context,
                mapping_support,
                buffer_support,
                confidence_scale,
            );
        }

        if scope.low_signal_compare {
            apply_low_signal_inference_guardrails(
                &mut probable_crash_causes,
                &mut suggested_fixes,
                &mut candidate_mapping_hints,
                &scope,
            );
        }

        if let Some(mod_dependency) = mod_dependency.as_ref() {
            apply_mod_dependency_inference_adjustments(
                &mut probable_crash_causes,
                &mut suggested_fixes,
                &mut candidate_mapping_hints,
                compare_report,
                mod_dependency,
                &surface_intersection,
                mapping_support,
                buffer_support,
                runtime_support,
                confidence_scale,
            );
        }

        let representative_risk_projections = representative_mod_baseline
            .as_ref()
            .map(|baseline| {
                build_representative_risk_projections(
                    compare_report,
                    scope.low_signal_compare,
                    baseline,
                )
            })
            .unwrap_or_default();

        candidate_mapping_hints.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.old_asset_path.cmp(&right.old_asset_path))
        });

        probable_crash_causes.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.code.cmp(&right.code))
        });
        suggested_fixes.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.code.cmp(&right.code))
        });

        let highest_confidence = probable_crash_causes
            .iter()
            .map(|cause| cause.confidence)
            .chain(suggested_fixes.iter().map(|fix| fix.confidence))
            .chain(candidate_mapping_hints.iter().map(|hint| hint.confidence))
            .fold(0.0, f32::max);

        InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: current_unix_ms().unwrap_or_default(),
            compare_input: InferenceCompareInput {
                old_version_id: compare_report.old_snapshot.version_id.clone(),
                new_version_id: compare_report.new_snapshot.version_id.clone(),
                changed_assets: compare_report.summary.changed_assets,
                added_assets: compare_report.summary.added_assets,
                removed_assets: compare_report.summary.removed_assets,
                candidate_mapping_changes: compare_report.summary.candidate_mapping_changes,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: wwmi_knowledge.repo.input.clone(),
                analyzed_commits: wwmi_knowledge.summary.analyzed_commits,
                fix_like_commits: wwmi_knowledge.summary.fix_like_commits,
                discovered_patterns: wwmi_knowledge.summary.discovered_patterns,
            },
            mod_dependency_input: mod_dependency.as_ref().map(|value| value.input.clone()),
            representative_mod_baseline_input: representative_mod_baseline
                .as_ref()
                .map(|value| value.input.clone()),
            scope,
            summary: InferenceSummary {
                probable_crash_causes: probable_crash_causes.len(),
                suggested_fixes: suggested_fixes.len(),
                candidate_mapping_hints: candidate_mapping_hints.len(),
                highest_confidence,
            },
            probable_crash_causes,
            suggested_fixes,
            candidate_mapping_hints,
            surface_intersection,
            representative_risk_projections,
        }
    }
}

pub fn load_inference_report(path: &Path) -> AppResult<InferenceReport> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn build_mod_dependency_insights(profile: &WwmiModDependencyProfile) -> ModDependencyInsights {
    ModDependencyInsights {
        input: InferenceModDependencyInput {
            mod_name: profile.mod_name.clone(),
            mod_root: profile.mod_root.clone(),
            ini_file_count: profile.ini_file_count,
            signal_count: profile.signals.len(),
            dependency_kinds: profile.kinds().into_iter().collect(),
        },
        mapping_hash: build_mod_dependency_surface_summary(
            profile,
            &[WwmiModDependencyKind::TextureOverrideHash],
            "mapping/hash-sensitive",
        ),
        buffer_layout: build_mod_dependency_surface_summary(
            profile,
            &[
                WwmiModDependencyKind::BufferLayoutHint,
                WwmiModDependencyKind::MeshVertexCount,
                WwmiModDependencyKind::ShapeKeyVertexCount,
            ],
            "buffer/layout-sensitive",
        ),
        resource_or_skeleton: build_mod_dependency_surface_summary(
            profile,
            &[
                WwmiModDependencyKind::ResourceFileReference,
                WwmiModDependencyKind::SkeletonMergeDependency,
            ],
            "resource/skeleton-sensitive",
        ),
        hook_targeting: build_mod_dependency_surface_summary(
            profile,
            &[
                WwmiModDependencyKind::ObjectGuid,
                WwmiModDependencyKind::DrawCallTarget,
                WwmiModDependencyKind::FilterIndex,
            ],
            "hook-targeting-sensitive",
        ),
    }
}

fn build_representative_mod_baseline_insights(
    baseline_set: &WwmiModDependencyBaselineSet,
) -> RepresentativeModBaselineInsights {
    let mapping_hash = build_representative_surface_summary(
        &baseline_set.profiles,
        &[WwmiModDependencyKind::TextureOverrideHash],
        RepresentativeModRiskClass::MappingHashSensitive,
        "mapping/hash-sensitive",
    );
    let buffer_layout = build_representative_surface_summary(
        &baseline_set.profiles,
        &[
            WwmiModDependencyKind::BufferLayoutHint,
            WwmiModDependencyKind::MeshVertexCount,
            WwmiModDependencyKind::ShapeKeyVertexCount,
        ],
        RepresentativeModRiskClass::BufferLayoutSensitive,
        "buffer/layout-sensitive",
    );
    let resource_or_skeleton = build_representative_surface_summary(
        &baseline_set.profiles,
        &[
            WwmiModDependencyKind::ResourceFileReference,
            WwmiModDependencyKind::SkeletonMergeDependency,
        ],
        RepresentativeModRiskClass::ResourceSkeletonSensitive,
        "resource/skeleton-sensitive",
    );
    let draw_call_filter_hook = build_representative_surface_summary(
        &baseline_set.profiles,
        &[
            WwmiModDependencyKind::ObjectGuid,
            WwmiModDependencyKind::DrawCallTarget,
            WwmiModDependencyKind::FilterIndex,
        ],
        RepresentativeModRiskClass::DrawCallFilterHookSensitive,
        "draw-call/filter/hook-sensitive",
    );

    let mut risk_class_counts = Vec::new();
    for surface in [
        mapping_hash.as_ref(),
        buffer_layout.as_ref(),
        resource_or_skeleton.as_ref(),
        draw_call_filter_hook.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        risk_class_counts.push(InferenceRepresentativeRiskClassCount {
            risk_class: surface.risk_class.clone(),
            profile_count: surface.profile_count,
        });
    }

    RepresentativeModBaselineInsights {
        input: InferenceRepresentativeModBaselineInput {
            version_id: baseline_set.version_id.clone(),
            profile_count: baseline_set.profile_count,
            included_mod_count: baseline_set.review.included_mod_count,
            represented_surface_classes: baseline_set.represented_surface_classes(),
            risk_class_counts,
            strength: baseline_set.review.strength.clone(),
            material_for_repair_review: baseline_set.review.material_for_repair_review,
            caution_notes: baseline_set.review.caution_notes.clone(),
        },
        mapping_hash,
        buffer_layout,
        resource_or_skeleton,
        draw_call_filter_hook,
    }
}

fn build_mod_dependency_surface_summary(
    profile: &WwmiModDependencyProfile,
    kinds: &[WwmiModDependencyKind],
    label: &'static str,
) -> Option<ModDependencySurfaceSummary> {
    let matched = profile
        .signals
        .iter()
        .filter(|signal| kinds.contains(&signal.kind))
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return None;
    }

    let dependency_kinds = matched
        .iter()
        .map(|signal| signal.kind.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let source_files = matched
        .iter()
        .map(|signal| signal.source_file.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let sections = matched
        .iter()
        .filter_map(|signal| signal.section.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    Some(ModDependencySurfaceSummary {
        dependency_kinds,
        signal_count: matched.len(),
        source_files,
        sections,
        label,
    })
}

fn build_representative_surface_summary(
    profiles: &[WwmiModDependencyProfile],
    kinds: &[WwmiModDependencyKind],
    risk_class: RepresentativeModRiskClass,
    label: &'static str,
) -> Option<RepresentativeBaselineSurfaceSummary> {
    let matched_profiles = profiles
        .iter()
        .filter(|profile| {
            profile
                .signals
                .iter()
                .any(|signal| kinds.contains(&signal.kind))
        })
        .collect::<Vec<_>>();
    if matched_profiles.is_empty() {
        return None;
    }

    let dependency_kinds = matched_profiles
        .iter()
        .flat_map(|profile| profile.signals.iter())
        .filter(|signal| kinds.contains(&signal.kind))
        .map(|signal| signal.kind.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let sample_mod_names = matched_profiles
        .iter()
        .map(|profile| {
            profile
                .mod_name
                .clone()
                .unwrap_or_else(|| profile.mod_root.clone())
        })
        .take(3)
        .collect::<Vec<_>>();

    Some(RepresentativeBaselineSurfaceSummary {
        risk_class,
        profile_count: matched_profiles.len(),
        sample_mod_names,
        dependency_kinds,
        label,
    })
}

fn build_surface_intersection(
    compare_report: &SnapshotCompareReport,
    mod_dependency: Option<&ModDependencyInsights>,
    representative_mod_baseline: Option<&RepresentativeModBaselineInsights>,
) -> InferenceSurfaceIntersection {
    let mut mod_side =
        BTreeMap::<WwmiModDependencySurfaceClass, SurfaceIntersectionBuildEntry>::new();

    if let Some(mod_dependency) = mod_dependency {
        register_mod_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::MappingHash,
            mod_dependency.mapping_hash.as_ref(),
            "mod_dependency_profile",
        );
        register_mod_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::BufferLayout,
            mod_dependency.buffer_layout.as_ref(),
            "mod_dependency_profile",
        );
        register_mod_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::ResourceSkeleton,
            mod_dependency.resource_or_skeleton.as_ref(),
            "mod_dependency_profile",
        );
        register_mod_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::DrawCallFilterHook,
            mod_dependency.hook_targeting.as_ref(),
            "mod_dependency_profile",
        );
    }

    if let Some(baseline) = representative_mod_baseline {
        register_representative_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::MappingHash,
            baseline.mapping_hash.as_ref(),
        );
        register_representative_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::BufferLayout,
            baseline.buffer_layout.as_ref(),
        );
        register_representative_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::ResourceSkeleton,
            baseline.resource_or_skeleton.as_ref(),
        );
        register_representative_surface(
            &mut mod_side,
            WwmiModDependencySurfaceClass::DrawCallFilterHook,
            baseline.draw_call_filter_hook.as_ref(),
        );
    }

    let game_side = collect_game_side_surfaces(compare_report);
    let overlapping_surface_classes = ordered_surface_classes()
        .into_iter()
        .filter(|surface_class| mod_side.contains_key(surface_class))
        .filter(|surface_class| {
            game_side
                .iter()
                .any(|surface| surface.surface_class == *surface_class)
        })
        .collect::<Vec<_>>();

    let overlap_posture = if overlapping_surface_classes.is_empty() {
        InferenceSurfaceOverlapPosture::None
    } else if overlapping_surface_classes.len() == mod_side.len() {
        InferenceSurfaceOverlapPosture::Strong
    } else {
        InferenceSurfaceOverlapPosture::Partial
    };
    let weak_or_absent_overlap =
        !mod_side.is_empty() && overlap_posture != InferenceSurfaceOverlapPosture::Strong;
    let mut notes = Vec::new();
    if !mod_side.is_empty() || !game_side.is_empty() {
        notes.push(format!(
            "surface overlap posture is {}",
            overlap_posture.label()
        ));
    }
    match overlap_posture {
        InferenceSurfaceOverlapPosture::None if !mod_side.is_empty() => {
            notes.push(
                "no explicit mod/game surface overlap was observed in current compare evidence; keep mod-side dependency signals as reviewer context only"
                    .to_string(),
            );
        }
        InferenceSurfaceOverlapPosture::Partial => {
            notes.push(format!(
                "surface overlap is partial: overlap={} while mod-side surfaces={}; keep overlap-aware prioritization review-first until more game-side evidence is available",
                join_surface_labels(overlapping_surface_classes.iter()),
                join_surface_labels(mod_side.keys())
            ));
        }
        _ => {}
    }

    InferenceSurfaceIntersection {
        mod_side_surfaces: ordered_surface_classes()
            .into_iter()
            .filter_map(|surface_class| {
                mod_side
                    .remove(&surface_class)
                    .map(|entry| InferenceModSideSurface {
                        surface_class,
                        sources: entry.sources.into_iter().collect(),
                        dependency_kinds: entry.dependency_kinds.into_iter().collect(),
                        signal_count: entry.signal_count,
                        representative_profile_count: entry.representative_profile_count,
                    })
            })
            .collect(),
        game_side_surfaces: game_side,
        overlapping_surface_classes,
        overlap_posture,
        weak_or_absent_overlap,
        notes,
    }
}

fn register_mod_surface(
    mod_side: &mut BTreeMap<WwmiModDependencySurfaceClass, SurfaceIntersectionBuildEntry>,
    surface_class: WwmiModDependencySurfaceClass,
    surface: Option<&ModDependencySurfaceSummary>,
    source: &str,
) {
    let Some(surface) = surface else {
        return;
    };

    let entry = mod_side.entry(surface_class).or_default();
    entry.sources.insert(source.to_string());
    entry.signal_count += surface.signal_count;
    entry
        .dependency_kinds
        .extend(surface.dependency_kinds.iter().cloned());
}

fn register_representative_surface(
    mod_side: &mut BTreeMap<WwmiModDependencySurfaceClass, SurfaceIntersectionBuildEntry>,
    surface_class: WwmiModDependencySurfaceClass,
    surface: Option<&RepresentativeBaselineSurfaceSummary>,
) {
    let Some(surface) = surface else {
        return;
    };

    let entry = mod_side.entry(surface_class).or_default();
    entry
        .sources
        .insert("representative_mod_baseline".to_string());
    entry.representative_profile_count += surface.profile_count;
    entry
        .dependency_kinds
        .extend(surface.dependency_kinds.iter().cloned());
}

fn collect_game_side_surfaces(
    compare_report: &SnapshotCompareReport,
) -> Vec<InferenceGameSideSurface> {
    let mut surfaces = Vec::new();
    for surface_class in ordered_surface_classes() {
        let compare_signals = match surface_class {
            WwmiModDependencySurfaceClass::MappingHash => {
                mapping_hash_projection_signals(compare_report)
            }
            WwmiModDependencySurfaceClass::BufferLayout => {
                buffer_layout_projection_signals(compare_report)
            }
            WwmiModDependencySurfaceClass::ResourceSkeleton => {
                resource_skeleton_projection_signals(compare_report)
            }
            WwmiModDependencySurfaceClass::DrawCallFilterHook => {
                draw_call_filter_hook_projection_signals(compare_report)
            }
        };
        if compare_signals.is_empty() {
            continue;
        }

        surfaces.push(InferenceGameSideSurface {
            surface_class: surface_class.clone(),
            compare_signals,
            affected_assets: game_side_surface_affected_assets(compare_report, &surface_class),
        });
    }
    surfaces
}

fn game_side_surface_affected_assets(
    compare_report: &SnapshotCompareReport,
    surface_class: &WwmiModDependencySurfaceClass,
) -> Vec<String> {
    let mut assets = BTreeSet::new();
    match surface_class {
        WwmiModDependencySurfaceClass::MappingHash => {
            for change in &compare_report.removed_assets {
                if let Some(asset) = change.old_asset.as_ref() {
                    assets.insert(asset.path.clone());
                }
            }
            for candidate in &compare_report.candidate_mapping_changes {
                assets.insert(candidate.old_asset.path.clone());
                assets.insert(candidate.new_asset.path.clone());
            }
            for change in &compare_report.changed_assets {
                if change.changed_fields.iter().any(|field| {
                    matches!(
                        field.as_str(),
                        "asset_hash" | "shader_hash" | "signature" | "path_presence"
                    )
                }) {
                    if let Some(asset) = change.new_asset.as_ref().or(change.old_asset.as_ref()) {
                        assets.insert(asset.path.clone());
                    }
                }
            }
        }
        WwmiModDependencySurfaceClass::BufferLayout => {
            for change in &compare_report.changed_assets {
                if is_structural_change(change) {
                    if let Some(asset) = change.new_asset.as_ref().or(change.old_asset.as_ref()) {
                        assets.insert(asset.path.clone());
                    }
                }
            }
            for candidate in &compare_report.candidate_mapping_changes {
                if candidate.compatibility == RemapCompatibility::StructurallyRisky {
                    assets.insert(candidate.old_asset.path.clone());
                    assets.insert(candidate.new_asset.path.clone());
                }
            }
        }
        WwmiModDependencySurfaceClass::ResourceSkeleton => {
            assets.extend(resource_or_skeleton_affected_assets(compare_report));
        }
        WwmiModDependencySurfaceClass::DrawCallFilterHook => {
            if has_hook_targeting_review_surface(compare_report) {
                for candidate in &compare_report.candidate_mapping_changes {
                    assets.insert(candidate.old_asset.path.clone());
                    assets.insert(candidate.new_asset.path.clone());
                }
                for change in &compare_report.changed_assets {
                    if change.changed_fields.iter().any(|field| {
                        matches!(
                            field.as_str(),
                            "container_path" | "source_kind" | "asset_hash" | "signature"
                        )
                    }) && let Some(asset) =
                        change.new_asset.as_ref().or(change.old_asset.as_ref())
                    {
                        assets.insert(asset.path.clone());
                    }
                }
            }
        }
    }
    assets.into_iter().collect()
}

fn ordered_surface_classes() -> Vec<WwmiModDependencySurfaceClass> {
    vec![
        WwmiModDependencySurfaceClass::MappingHash,
        WwmiModDependencySurfaceClass::BufferLayout,
        WwmiModDependencySurfaceClass::ResourceSkeleton,
        WwmiModDependencySurfaceClass::DrawCallFilterHook,
    ]
}

fn join_surface_labels<'a>(
    surfaces: impl IntoIterator<Item = &'a WwmiModDependencySurfaceClass>,
) -> String {
    let labels = surfaces
        .into_iter()
        .map(WwmiModDependencySurfaceClass::label)
        .collect::<Vec<_>>();
    if labels.is_empty() {
        "none".to_string()
    } else {
        labels.join(", ")
    }
}

fn surface_intersection_has_overlap(
    surface_intersection: &InferenceSurfaceIntersection,
    surface_class: WwmiModDependencySurfaceClass,
) -> bool {
    surface_intersection
        .overlapping_surface_classes
        .contains(&surface_class)
}

fn is_structural_change(change: &SnapshotAssetChange) -> bool {
    change.changed_fields.iter().any(|field| {
        matches!(
            field.as_str(),
            "vertex_count"
                | "index_count"
                | "material_slots"
                | "section_count"
                | "internal_structure.section_labels"
                | "internal_structure.buffer_roles"
                | "internal_structure.binding_targets"
                | "internal_structure.subresource_roles"
                | "internal_structure.has_skeleton"
                | "internal_structure.has_shapekey_data"
                | "vertex_stride"
                | "vertex_buffer_count"
                | "index_format"
                | "primitive_topology"
                | "layout_markers"
                | "asset_hash"
                | "shader_hash"
                | "signature"
        )
    })
}

fn infer_mapping_hint(
    candidate: &CandidateMappingChange,
    mapping_support: f32,
    scope: &InferenceScopeContext,
    continuity_signal: Option<&ContinuitySignal>,
    current_new_version: &str,
) -> InferredMappingHint {
    let mut confidence = (candidate.confidence * 0.70 + mapping_support * 0.30).clamp(0.0, 1.0);
    if candidate.ambiguous {
        confidence = (confidence - 0.12).clamp(0.0, 1.0);
    }
    if scope.low_signal_compare {
        confidence = (confidence - 0.03).clamp(0.0, 1.0);
    }
    let mut compatibility = adjust_hint_compatibility(candidate, scope);

    let mut reasons = candidate
        .reasons
        .iter()
        .map(reason_to_string)
        .collect::<Vec<_>>();
    reasons.push(format!(
        "compatibility: {}",
        compatibility_label(&compatibility)
    ));
    if candidate_has_reason_code(candidate, "likely_same_asset_repathed") {
        reasons.push(
            "compare suggests this is a likely same-asset rename/repath candidate".to_string(),
        );
    }
    if candidate_has_reason_code(candidate, "same_asset_but_structural_drift") {
        reasons.push(
            "compare also detected structural drift on this candidate, so remap-only fixes may be unsafe".to_string(),
        );
    }
    if candidate.ambiguous {
        reasons.push(
            "compare detected a near-tie runner-up candidate; confidence was penalized to keep this mapping under review".to_string(),
        );
    }
    if scope.low_signal_compare {
        reasons.push(
            "snapshot scope is install/package-level or low-coverage; treat remap confidence as low-signal and require conservative review"
                .to_string(),
        );
    }

    let mut evidence = vec![format!(
        "compare candidate confidence {:.3} adjusted with WWMI mapping-pattern support {:.3}",
        candidate.confidence, mapping_support
    )];
    if let Some(runner_up_confidence) = candidate.runner_up_confidence {
        evidence.push(format!(
            "runner-up candidate confidence {:.3} with gap {}",
            runner_up_confidence,
            candidate
                .confidence_gap
                .map(|gap| format!("{gap:.3}"))
                .unwrap_or_else(|| "unknown".to_string())
        ));
    }
    if candidate.ambiguous {
        evidence.push(
            "mapping hint confidence penalized by 0.120 because the runner-up candidate was too close".to_string(),
        );
    }
    evidence.extend(
        candidate
            .reasons
            .iter()
            .filter(|reason| {
                matches!(
                    reason.code.as_str(),
                    "likely_same_asset_repathed"
                        | "same_asset_but_structural_drift"
                        | "identity_conflict_detected"
                        | "weak_identity_evidence"
                )
            })
            .map(reason_to_string),
    );
    if scope.low_signal_compare {
        evidence.extend(
            scope
                .notes
                .iter()
                .take(3)
                .map(|note| format!("scope context: {note}")),
        );
    }

    if let Some(signal) = continuity_signal {
        apply_continuity_to_mapping_hint(
            &mut confidence,
            &mut compatibility,
            &mut reasons,
            &mut evidence,
            signal,
            current_new_version,
        );
    }

    reasons.push(format!(
        "continuity-aware compatibility: {}",
        compatibility_label(&compatibility)
    ));
    let continuity = continuity_signal
        .map(|signal| build_mapping_hint_continuity_context(signal, current_new_version));

    InferredMappingHint {
        old_asset_path: candidate.old_asset.path.clone(),
        new_asset_path: candidate.new_asset.path.clone(),
        confidence,
        compatibility,
        needs_review: true,
        ambiguous: candidate.ambiguous,
        confidence_gap: candidate.confidence_gap,
        continuity,
        reasons,
        evidence,
    }
}

fn build_inference_scope_context(
    compare_report: &SnapshotCompareReport,
    mod_dependency: Option<&ModDependencyInsights>,
    representative_mod_baseline: Option<&RepresentativeModBaselineInsights>,
    surface_intersection: &InferenceSurfaceIntersection,
) -> InferenceScopeContext {
    let mut notes = compare_report.scope.notes.clone();
    if compare_report.scope.low_signal_compare {
        notes.push(
            "inference confidence is conservative because compare scope is low-signal for deep character-level analysis"
                .to_string(),
        );
    }
    if compare_report.scope.scope_induced_removals_likely {
        notes.push(
            "removed-only compare deltas flagged as scope-induced were kept out of crash-cause promotion; review snapshot scope before treating missing paths as real version drift"
                .to_string(),
        );
    }
    append_scope_capture_quality_guardrail(&mut notes, &compare_report.scope.old_snapshot);
    append_scope_capture_quality_guardrail(&mut notes, &compare_report.scope.new_snapshot);
    if let Some(mod_dependency) = mod_dependency {
        notes.push(format!(
            "mod dependency profile {} scanned {} ini file(s), {} signal(s), dependency kinds={}",
            mod_dependency
                .input
                .mod_name
                .as_deref()
                .unwrap_or(mod_dependency.input.mod_root.as_str()),
            mod_dependency.input.ini_file_count,
            mod_dependency.input.signal_count,
            join_dependency_kind_labels(&mod_dependency.input.dependency_kinds)
        ));
    }
    if let Some(representative_mod_baseline) = representative_mod_baseline {
        let surface_labels = representative_mod_baseline
            .input
            .represented_surface_classes
            .iter()
            .map(|surface| surface.label())
            .collect::<Vec<_>>();
        notes.push(format!(
            "representative mod baseline set {} loaded with {} profile(s), {} included mod root(s), strength={:?}, surfaces={}",
            representative_mod_baseline.input.version_id,
            representative_mod_baseline.input.profile_count,
            representative_mod_baseline.input.included_mod_count,
            representative_mod_baseline.input.strength,
            if surface_labels.is_empty() {
                "none".to_string()
            } else {
                surface_labels.join(", ")
            }
        ));
        if !representative_mod_baseline.input.material_for_repair_review {
            notes.push(
                "representative mod baseline remains limited and should stay review-only support rather than strong repair evidence"
                    .to_string(),
            );
        }
    }
    if !surface_intersection.mod_side_surfaces.is_empty() {
        notes.push(format!(
            "mod-side dependency surfaces: {}",
            join_surface_labels(
                surface_intersection
                    .mod_side_surfaces
                    .iter()
                    .map(|surface| &surface.surface_class)
            )
        ));
    }
    if !surface_intersection.game_side_surfaces.is_empty() {
        notes.push(format!(
            "game-side change surfaces: {}",
            join_surface_labels(
                surface_intersection
                    .game_side_surfaces
                    .iter()
                    .map(|surface| &surface.surface_class)
            )
        ));
    }
    if !surface_intersection.overlapping_surface_classes.is_empty() {
        notes.push(format!(
            "surface intersection overlap: {}",
            join_surface_labels(surface_intersection.overlapping_surface_classes.iter())
        ));
    } else if !surface_intersection.mod_side_surfaces.is_empty() {
        notes.push(
            "mod-side dependency surfaces were detected, but compare did not expose overlapping game-side surfaces; keep this as review-only context rather than strong repair evidence"
                .to_string(),
        );
    }
    if !surface_intersection.mod_side_surfaces.is_empty()
        || !surface_intersection.game_side_surfaces.is_empty()
    {
        notes.push(format!(
            "surface overlap posture: {}",
            surface_intersection.overlap_posture.label()
        ));
    }
    notes.extend(surface_intersection.notes.iter().cloned());

    InferenceScopeContext {
        low_signal_compare: compare_report.scope.low_signal_compare,
        old_snapshot_low_signal: compare_report
            .scope
            .old_snapshot
            .low_signal_for_character_analysis,
        new_snapshot_low_signal: compare_report
            .scope
            .new_snapshot
            .low_signal_for_character_analysis,
        notes,
    }
}

fn append_scope_capture_quality_guardrail(
    notes: &mut Vec<String>,
    scope: &crate::compare::SnapshotCompareScopeInfo,
) {
    let has_hash_or_manifest_coverage = scope.manifest_resource_count > 0
        || scope.assets_with_asset_hash > 0
        || scope.assets_with_any_hash > 0
        || scope.assets_with_signature > 0;
    if has_hash_or_manifest_coverage && !scope.meaningful_asset_record_enrichment {
        notes.push(
            "manifest/hash coverage is present, but that remains shallow evidence and should not be treated as rich asset-level enrichment"
                .to_string(),
        );
    }
}

fn apply_mod_dependency_inference_adjustments(
    probable_crash_causes: &mut Vec<ProbableCrashCause>,
    suggested_fixes: &mut Vec<SuggestedFix>,
    candidate_mapping_hints: &mut Vec<InferredMappingHint>,
    compare_report: &SnapshotCompareReport,
    mod_dependency: &ModDependencyInsights,
    surface_intersection: &InferenceSurfaceIntersection,
    mapping_support: f32,
    buffer_support: f32,
    runtime_support: f32,
    confidence_scale: f32,
) {
    if let Some(surface) = mod_dependency.mapping_hash.as_ref()
        && surface_intersection_has_overlap(
            surface_intersection,
            WwmiModDependencySurfaceClass::MappingHash,
        )
    {
        for code in [
            "asset_paths_or_mapping_shifted",
            "asset_removed_without_clear_replacement",
            "asset_signature_or_hash_changed",
            "candidate_remap_identity_conflict",
            "candidate_remap_insufficient_evidence",
        ] {
            if let Some(cause) = probable_crash_causes
                .iter_mut()
                .find(|cause| cause.code == code)
            {
                cause.confidence = (cause.confidence + 0.05).clamp(0.0, 1.0);
                push_unique(
                    &mut cause.reasons,
                    format!(
                        "mod_dependency_surface: this mod uses {} surfaces, so mapping/hash change evidence is more repair-relevant here",
                        surface.label
                    ),
                );
                append_surface_evidence(
                    &mut cause.evidence,
                    surface,
                    "mod-side ini files already depend on hash/resource-driven targeting",
                );
            }
        }
        for code in [
            "review_candidate_asset_remaps",
            "inspect_identity_conflicts_before_remap",
            "gather_stronger_asset_evidence",
        ] {
            if let Some(fix) = suggested_fixes.iter_mut().find(|fix| fix.code == code) {
                fix.confidence = (fix.confidence + 0.05).clamp(0.0, 1.0);
                push_unique(
                    &mut fix.reasons,
                    format!(
                        "mod_dependency_surface: this mod uses {} surfaces, so mapping/hash repair paths deserve stronger priority",
                        surface.label
                    ),
                );
                if code == "review_candidate_asset_remaps" {
                    push_unique(
                        &mut fix.actions,
                        "Start with texture/resource override sections from the mod dependency profile before broad asset browsing."
                            .to_string(),
                    );
                }
                append_surface_evidence(
                    &mut fix.evidence,
                    surface,
                    "mod-side dependency profile points at mapping/hash-sensitive repair touchpoints",
                );
            }
        }
    }

    if let Some(surface) = mod_dependency.buffer_layout.as_ref()
        && surface_intersection_has_overlap(
            surface_intersection,
            WwmiModDependencySurfaceClass::BufferLayout,
        )
    {
        for code in [
            "buffer_layout_changed",
            "candidate_remap_structural_drift",
            "asset_signature_or_hash_changed",
        ] {
            if let Some(cause) = probable_crash_causes
                .iter_mut()
                .find(|cause| cause.code == code)
            {
                cause.confidence = (cause.confidence + 0.06).clamp(0.0, 1.0);
                push_unique(
                    &mut cause.reasons,
                    format!(
                        "mod_dependency_surface: this mod uses {} surfaces, so structural drift is directly repair-relevant",
                        surface.label
                    ),
                );
                append_surface_evidence(
                    &mut cause.evidence,
                    surface,
                    "mod-side ini files already encode buffer/layout assumptions",
                );
            }
        }
        for code in [
            "review_buffer_layout_and_runtime_guards",
            "validate_candidate_remaps_against_layout",
        ] {
            if let Some(fix) = suggested_fixes.iter_mut().find(|fix| fix.code == code) {
                fix.confidence = (fix.confidence + 0.06).clamp(0.0, 1.0);
                push_unique(
                    &mut fix.reasons,
                    format!(
                        "mod_dependency_surface: this mod uses {} surfaces, so layout-sensitive fixes should be inspected first",
                        surface.label
                    ),
                );
                push_unique(
                    &mut fix.actions,
                    "Cross-check the mod's override stride/count/layout sections before approving any remap or runtime guard."
                        .to_string(),
                );
                append_surface_evidence(
                    &mut fix.evidence,
                    surface,
                    "mod-side dependency profile points at buffer/layout-sensitive hooks",
                );
            }
        }
    }

    if let Some(surface) = mod_dependency.resource_or_skeleton.as_ref()
        && surface_intersection_has_overlap(
            surface_intersection,
            WwmiModDependencySurfaceClass::ResourceSkeleton,
        )
    {
        let affected_assets = resource_or_skeleton_affected_assets(compare_report);
        if !affected_assets.is_empty() {
            probable_crash_causes.push(ProbableCrashCause {
                code: "mod_resource_or_skeleton_surface_changed".to_string(),
                summary: "The mod depends on skeleton/resource reference surfaces, and compare evidence suggests those surfaces moved or changed.".to_string(),
                confidence: (0.46 + 0.18 * buffer_support + 0.14 * runtime_support)
                    .clamp(0.0, 1.0)
                    * confidence_scale,
                risk: RiskLevel::High,
                affected_assets: affected_assets.clone(),
                related_patterns: vec![
                    WwmiPatternKind::BufferLayoutOrCapacityFix,
                    WwmiPatternKind::RuntimeConfigChange,
                    WwmiPatternKind::MappingOrHashUpdate,
                ],
                reasons: vec![format!(
                    "mod_dependency_surface: this mod uses {} surfaces and compare evidence shows related movement/drift",
                    surface.label
                )],
                evidence: surface_evidence(
                    surface,
                    "resource/container/skeleton signals changed on game-side assets that this mod style commonly touches",
                ),
            });
            suggested_fixes.push(SuggestedFix {
                code: "review_resource_and_skeleton_bindings".to_string(),
                summary: "Review resource references, skeleton merge bindings, and moved package/context surfaces before trusting remaps.".to_string(),
                confidence: (0.45 + 0.15 * buffer_support + 0.10 * mapping_support)
                    .clamp(0.0, 1.0)
                    * confidence_scale,
                priority: RiskLevel::High,
                related_patterns: vec![
                    WwmiPatternKind::BufferLayoutOrCapacityFix,
                    WwmiPatternKind::RuntimeConfigChange,
                    WwmiPatternKind::MappingOrHashUpdate,
                ],
                actions: vec![
                    "Inspect resource filename references and merged-skeleton related ini sections first."
                        .to_string(),
                    "Verify container/source-kind movement and skeleton presence markers before promoting a remap."
                        .to_string(),
                ],
                reasons: vec![format!(
                    "mod_dependency_surface: this mod uses {} surfaces, so resource/skeleton drift is a first-pass review target",
                    surface.label
                )],
                evidence: surface_evidence(
                    surface,
                    "mod-side dependency profile includes resource references or skeleton merge hooks",
                ),
            });
        }
    }

    if let Some(surface) = mod_dependency.hook_targeting.as_ref()
        && surface_intersection_has_overlap(
            surface_intersection,
            WwmiModDependencySurfaceClass::DrawCallFilterHook,
        )
        && has_hook_targeting_review_surface(compare_report)
    {
        probable_crash_causes.push(ProbableCrashCause {
            code: "mod_hook_targeting_surface_requires_manual_review".to_string(),
            summary: "This mod uses draw-call/filter/object-guid targeting, so plausible remaps still need manual hook revalidation.".to_string(),
            confidence: (0.42 + 0.18 * mapping_support).clamp(0.0, 1.0) * confidence_scale,
            risk: RiskLevel::Medium,
            affected_assets: hint_paths(candidate_mapping_hints),
            related_patterns: vec![
                WwmiPatternKind::MappingOrHashUpdate,
                WwmiPatternKind::RuntimeConfigChange,
            ],
            reasons: vec![format!(
                "mod_dependency_surface: this mod uses {} surfaces that stay review-first even when compare candidates look plausible",
                surface.label
            )],
            evidence: surface_evidence(
                surface,
                "hook-targeting ini sections often need manual retargeting beyond asset remap confidence",
            ),
        });
        suggested_fixes.push(SuggestedFix {
            code: "review_draw_call_and_filter_hooks_before_remap".to_string(),
            summary: "Review draw-call/filter/object-guid hook sections before promoting any mapping candidate.".to_string(),
            confidence: (0.44 + 0.15 * mapping_support).clamp(0.0, 1.0) * confidence_scale,
            priority: RiskLevel::High,
            related_patterns: vec![
                WwmiPatternKind::MappingOrHashUpdate,
                WwmiPatternKind::RuntimeConfigChange,
            ],
            actions: vec![
                "Start with match_first_index/match_index_count/filter_index/object_guid sections from the mod dependency profile."
                    .to_string(),
                "Keep remaps in manual review until hook targeting is revalidated on real runtime behavior."
                    .to_string(),
            ],
            reasons: vec![format!(
                "mod_dependency_surface: this mod uses {} surfaces, so remap promotion should stay review-first",
                surface.label
            )],
            evidence: surface_evidence(
                surface,
                "draw-call/filter/object-guid targeting is present in the scanned mod ini files",
            ),
        });
    }

    for hint in candidate_mapping_hints {
        if let Some(surface) = mod_dependency.mapping_hash.as_ref()
            && surface_intersection_has_overlap(
                surface_intersection,
                WwmiModDependencySurfaceClass::MappingHash,
            )
        {
            if has_reason_code(&hint.reasons, "asset_hash_exact")
                || has_reason_code(&hint.reasons, "signature_exact")
            {
                hint.confidence = (hint.confidence + 0.03).clamp(0.0, 1.0);
                push_unique(
                    &mut hint.reasons,
                    format!(
                        "mod_dependency_surface: this mod uses {} surfaces, so this hash/identity-aligned remap is more repair-relevant",
                        surface.label
                    ),
                );
                append_surface_evidence(
                    &mut hint.evidence,
                    surface,
                    "texture/resource override sections provide a direct reviewer entry point for this remap",
                );
            }
        }

        if let Some(surface) = mod_dependency.buffer_layout.as_ref()
            && surface_intersection_has_overlap(
                surface_intersection,
                WwmiModDependencySurfaceClass::BufferLayout,
            )
            && (hint.compatibility == RemapCompatibility::StructurallyRisky
                || has_reason_code(&hint.reasons, "same_asset_but_structural_drift")
                || has_reason_code(&hint.reasons, "buffer_layout_validation_needed"))
        {
            hint.confidence = (hint.confidence - 0.06).clamp(0.0, 1.0);
            push_unique(
                &mut hint.reasons,
                format!(
                    "mod_dependency_review_first: this mod uses {} surfaces, so structurally drifted remaps stay review-first",
                    surface.label
                ),
            );
            append_surface_evidence(
                &mut hint.evidence,
                surface,
                "buffer/layout-sensitive mod hooks raise the cost of a wrong remap",
            );
        }

        if let Some(surface) = mod_dependency.resource_or_skeleton.as_ref()
            && surface_intersection_has_overlap(
                surface_intersection,
                WwmiModDependencySurfaceClass::ResourceSkeleton,
            )
            && is_resource_or_skeleton_hint(hint)
        {
            hint.confidence = (hint.confidence - 0.03).clamp(0.0, 1.0);
            push_unique(
                &mut hint.reasons,
                format!(
                    "mod_dependency_review_first: this mod uses {} surfaces, so container/resource/skeleton movement stays review-first",
                    surface.label
                ),
            );
            append_surface_evidence(
                &mut hint.evidence,
                surface,
                "resource references or skeleton merge hooks are present in the mod dependency profile",
            );
        }

        if let Some(surface) = mod_dependency.hook_targeting.as_ref()
            && surface_intersection_has_overlap(
                surface_intersection,
                WwmiModDependencySurfaceClass::DrawCallFilterHook,
            )
        {
            hint.confidence = (hint.confidence - 0.07).clamp(0.0, 1.0);
            hint.compatibility = downgrade_hook_targeting_compatibility(hint.compatibility.clone());
            push_unique(
                &mut hint.reasons,
                format!(
                    "mod_dependency_review_first: this mod uses {} surfaces, so even plausible remaps require manual hook revalidation",
                    surface.label
                ),
            );
            append_surface_evidence(
                &mut hint.evidence,
                surface,
                "draw-call/filter/object-guid hooks can break independently of asset similarity",
            );
        }
    }
}

fn build_representative_risk_projections(
    compare_report: &SnapshotCompareReport,
    low_signal_compare: bool,
    baseline: &RepresentativeModBaselineInsights,
) -> Vec<RepresentativeModRiskProjection> {
    let mut projections = Vec::new();

    if let Some(surface) = baseline.mapping_hash.as_ref() {
        let signals = mapping_hash_projection_signals(compare_report);
        if !signals.is_empty() {
            projections.push(build_representative_projection(
                surface,
                "Compare drift touches removed paths, remap candidates, or identity/hash-adjacent changes that commonly affect mapping/hash-sensitive mods.",
                signals,
                low_signal_compare,
                RiskLevel::High,
                0.64,
            ));
        }
    }

    if let Some(surface) = baseline.buffer_layout.as_ref() {
        let signals = buffer_layout_projection_signals(compare_report);
        if !signals.is_empty() {
            projections.push(build_representative_projection(
                surface,
                "Compare drift carries structural or layout-sensitive movement that representative buffer/layout-sensitive mods should review first.",
                signals,
                low_signal_compare,
                RiskLevel::High,
                0.69,
            ));
        }
    }

    if let Some(surface) = baseline.resource_or_skeleton.as_ref() {
        let signals = resource_skeleton_projection_signals(compare_report);
        if !signals.is_empty() {
            projections.push(build_representative_projection(
                surface,
                "Compare drift touches container/resource/skeleton surfaces that representative resource/skeleton-sensitive mods commonly bind to.",
                signals,
                low_signal_compare,
                RiskLevel::High,
                0.61,
            ));
        }
    }

    if let Some(surface) = baseline.draw_call_filter_hook.as_ref() {
        let signals = draw_call_filter_hook_projection_signals(compare_report);
        if !signals.is_empty() {
            projections.push(build_representative_projection(
                surface,
                "Compare drift would still require manual hook retargeting for representative draw-call/filter/hook-sensitive mods.",
                signals,
                true,
                RiskLevel::Medium,
                0.55,
            ));
        }
    }

    projections.sort_by(|left, right| {
        review_risk_rank(&right.priority)
            .cmp(&review_risk_rank(&left.priority))
            .then_with(|| right.confidence.total_cmp(&left.confidence))
            .then_with(|| {
                representative_risk_class_label(&left.risk_class)
                    .cmp(representative_risk_class_label(&right.risk_class))
            })
    });
    projections
}

fn build_representative_projection(
    surface: &RepresentativeBaselineSurfaceSummary,
    summary: &str,
    triggering_compare_signals: Vec<String>,
    review_first: bool,
    priority: RiskLevel,
    base_confidence: f32,
) -> RepresentativeModRiskProjection {
    let confidence = if review_first {
        (base_confidence - 0.04).clamp(0.0, 1.0)
    } else {
        base_confidence.clamp(0.0, 1.0)
    };

    RepresentativeModRiskProjection {
        risk_class: surface.risk_class.clone(),
        summary: summary.to_string(),
        confidence,
        priority,
        representative_profile_count: surface.profile_count,
        review_first,
        triggering_compare_signals: triggering_compare_signals.clone(),
        sample_mod_names: surface.sample_mod_names.clone(),
        reasons: vec![
            format!(
                "representative baseline set contains {} {} profile(s)",
                surface.profile_count, surface.label
            ),
            format!(
                "projection triggered by compare signals: {}",
                triggering_compare_signals.join(", ")
            ),
        ],
        evidence: vec![
            format!(
                "representative dependency kinds: {}",
                join_dependency_kind_labels(&surface.dependency_kinds)
            ),
            format!(
                "representative sample mods: {}",
                if surface.sample_mod_names.is_empty() {
                    "-".to_string()
                } else {
                    surface.sample_mod_names.join(", ")
                }
            ),
        ],
    }
}

fn mapping_hash_projection_signals(compare_report: &SnapshotCompareReport) -> Vec<String> {
    let mut signals = Vec::new();
    if !compare_report.scope.scope_induced_removals_likely
        && !compare_report.removed_assets.is_empty()
    {
        signals.push("removed_assets".to_string());
    }
    if !compare_report.candidate_mapping_changes.is_empty() {
        signals.push("candidate_mapping_changes".to_string());
    }
    if compare_report.changed_assets.iter().any(|change| {
        change.changed_fields.iter().any(|field| {
            matches!(
                field.as_str(),
                "asset_hash" | "shader_hash" | "signature" | "path_presence"
            )
        })
    }) {
        signals.push("hash_or_identity_field_drift".to_string());
    }
    signals
}

fn buffer_layout_projection_signals(compare_report: &SnapshotCompareReport) -> Vec<String> {
    let mut signals = Vec::new();
    if compare_report
        .changed_assets
        .iter()
        .any(|change| is_structural_change(change))
    {
        signals.push("structural_changed_assets".to_string());
    }
    if compare_report
        .candidate_mapping_changes
        .iter()
        .any(|candidate| candidate.compatibility == RemapCompatibility::StructurallyRisky)
    {
        signals.push("structurally_risky_candidate_remaps".to_string());
    }
    if compare_report.summary.lineage_layout_drift_assets > 0 {
        signals.push("lineage_layout_drift".to_string());
    }
    signals
}

fn resource_skeleton_projection_signals(compare_report: &SnapshotCompareReport) -> Vec<String> {
    let mut signals = Vec::new();
    if compare_report.summary.container_moved_assets > 0 {
        signals.push("container_moved_assets".to_string());
    }
    if !resource_or_skeleton_affected_assets(compare_report).is_empty() {
        signals.push("resource_or_skeleton_field_drift".to_string());
    }
    signals
}

fn draw_call_filter_hook_projection_signals(compare_report: &SnapshotCompareReport) -> Vec<String> {
    let mut signals = Vec::new();
    let has_mapping_shift = !compare_report.candidate_mapping_changes.is_empty()
        && !compare_report.removed_assets.is_empty();
    let has_hook_context_drift = compare_report.changed_assets.iter().any(|change| {
        change.changed_fields.iter().any(|field| {
            matches!(
                field.as_str(),
                "container_path" | "source_kind" | "asset_hash" | "signature"
            )
        })
    }) || compare_report.summary.container_moved_assets > 0;

    if !has_hook_context_drift {
        return signals;
    }

    if has_mapping_shift {
        signals.push("candidate_mapping_changes".to_string());
        signals.push("removed_assets".to_string());
    }
    signals.push("hook_targeting_context_drift".to_string());
    signals
}

fn representative_risk_class_label(risk_class: &RepresentativeModRiskClass) -> &'static str {
    match risk_class {
        RepresentativeModRiskClass::MappingHashSensitive => "mapping/hash-sensitive",
        RepresentativeModRiskClass::BufferLayoutSensitive => "buffer/layout-sensitive",
        RepresentativeModRiskClass::ResourceSkeletonSensitive => "resource/skeleton-sensitive",
        RepresentativeModRiskClass::DrawCallFilterHookSensitive => {
            "draw-call/filter/hook-sensitive"
        }
    }
}

fn review_risk_rank(risk: &RiskLevel) -> u8 {
    match risk {
        RiskLevel::High => 3,
        RiskLevel::Medium => 2,
        RiskLevel::Low => 1,
    }
}

fn resource_or_skeleton_affected_assets(compare_report: &SnapshotCompareReport) -> Vec<String> {
    let changed_assets = compare_report
        .changed_assets
        .iter()
        .filter(|change| {
            change.changed_fields.iter().any(|field| {
                matches!(
                    field.as_str(),
                    "container_path"
                        | "source_kind"
                        | "internal_structure.binding_targets"
                        | "internal_structure.subresource_roles"
                        | "internal_structure.has_skeleton"
                        | "internal_structure.has_shapekey_data"
                )
            })
        })
        .filter_map(|change| change.new_asset.as_ref().or(change.old_asset.as_ref()))
        .map(|asset| asset.path.clone());
    let candidate_assets = compare_report
        .candidate_mapping_changes
        .iter()
        .filter(|candidate| is_resource_or_skeleton_candidate(candidate))
        .flat_map(|candidate| {
            [
                candidate.old_asset.path.clone(),
                candidate.new_asset.path.clone(),
            ]
        });

    changed_assets
        .chain(candidate_assets)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_resource_or_skeleton_candidate(candidate: &CandidateMappingChange) -> bool {
    [
        "container_path_exact",
        "container_path_mismatch",
        "source_kind_exact",
        "source_kind_mismatch",
        "internal_binding_targets_exact",
        "internal_binding_targets_overlap",
        "internal_binding_targets_mismatch",
        "internal_subresource_roles_exact",
        "internal_subresource_roles_overlap",
        "internal_subresource_roles_mismatch",
        "internal_skeleton_presence_exact",
        "internal_skeleton_presence_mismatch",
    ]
    .iter()
    .any(|code| candidate_has_reason_code(candidate, code))
}

fn is_resource_or_skeleton_hint(hint: &InferredMappingHint) -> bool {
    [
        "container_path_exact",
        "container_path_mismatch",
        "source_kind_exact",
        "source_kind_mismatch",
        "internal_binding_targets_exact",
        "internal_binding_targets_overlap",
        "internal_binding_targets_mismatch",
        "internal_subresource_roles_exact",
        "internal_subresource_roles_overlap",
        "internal_subresource_roles_mismatch",
        "internal_skeleton_presence_exact",
        "internal_skeleton_presence_mismatch",
    ]
    .iter()
    .any(|code| has_reason_code(&hint.reasons, code))
}

fn has_hook_targeting_review_surface(compare_report: &SnapshotCompareReport) -> bool {
    !compare_report.candidate_mapping_changes.is_empty()
        || !compare_report.removed_assets.is_empty()
        || compare_report.changed_assets.iter().any(|change| {
            change.changed_fields.iter().any(|field| {
                matches!(
                    field.as_str(),
                    "asset_hash" | "shader_hash" | "signature" | "container_path" | "source_kind"
                )
            })
        })
}

fn hint_paths(hints: &[InferredMappingHint]) -> Vec<String> {
    hints
        .iter()
        .flat_map(|hint| [hint.old_asset_path.clone(), hint.new_asset_path.clone()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn downgrade_hook_targeting_compatibility(compatibility: RemapCompatibility) -> RemapCompatibility {
    match compatibility {
        RemapCompatibility::LikelyCompatible => RemapCompatibility::CompatibleWithCaution,
        other => other,
    }
}

fn append_surface_evidence(
    evidence: &mut Vec<String>,
    surface: &ModDependencySurfaceSummary,
    detail: &str,
) {
    for item in surface_evidence(surface, detail) {
        push_unique(evidence, item);
    }
}

fn surface_evidence(surface: &ModDependencySurfaceSummary, detail: &str) -> Vec<String> {
    let mut evidence = vec![format!(
        "{detail}; matched dependency kinds={} across {} signal(s)",
        join_dependency_kind_labels(&surface.dependency_kinds),
        surface.signal_count
    )];
    if !surface.source_files.is_empty() {
        evidence.push(format!(
            "mod dependency files: {}",
            surface
                .source_files
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !surface.sections.is_empty() {
        evidence.push(format!(
            "mod dependency sections: {}",
            surface
                .sections
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    evidence
}

fn join_dependency_kind_labels(kinds: &[WwmiModDependencyKind]) -> String {
    kinds
        .iter()
        .map(mod_dependency_kind_label)
        .collect::<Vec<_>>()
        .join(", ")
}

fn mod_dependency_kind_label(kind: &WwmiModDependencyKind) -> &'static str {
    match kind {
        WwmiModDependencyKind::ObjectGuid => "object_guid",
        WwmiModDependencyKind::DrawCallTarget => "draw_call_target",
        WwmiModDependencyKind::TextureOverrideHash => "texture_override_hash",
        WwmiModDependencyKind::ResourceFileReference => "resource_file_reference",
        WwmiModDependencyKind::MeshVertexCount => "mesh_vertex_count",
        WwmiModDependencyKind::ShapeKeyVertexCount => "shapekey_vertex_count",
        WwmiModDependencyKind::BufferLayoutHint => "buffer_layout_hint",
        WwmiModDependencyKind::SkeletonMergeDependency => "skeleton_merge_dependency",
        WwmiModDependencyKind::FilterIndex => "filter_index",
    }
}

fn push_unique(target: &mut Vec<String>, value: String) {
    if !target.contains(&value) {
        target.push(value);
    }
}

fn build_inference_continuity_context(
    compare_report: &SnapshotCompareReport,
    continuity: &VersionContinuityIndex,
) -> InferenceContinuityContext {
    let mut context = InferenceContinuityContext::default();
    let old_version = compare_report.old_snapshot.version_id.as_str();
    let new_version = compare_report.new_snapshot.version_id.as_str();

    for change in compare_report
        .changed_assets
        .iter()
        .chain(compare_report.removed_assets.iter())
    {
        let Some((old_path, new_path)) = asset_change_transition_key(change) else {
            continue;
        };
        let Some(signal) = find_continuity_signal(
            continuity,
            old_version,
            new_version,
            &old_path,
            new_path.as_deref(),
        ) else {
            continue;
        };
        context.change_signals.insert((old_path, new_path), signal);
    }

    for candidate in &compare_report.candidate_mapping_changes {
        let Some(signal) = find_continuity_signal(
            continuity,
            old_version,
            new_version,
            &candidate.old_asset.path,
            Some(candidate.new_asset.path.as_str()),
        ) else {
            continue;
        };
        context.mapping_signals.insert(
            (
                candidate.old_asset.path.clone(),
                candidate.new_asset.path.clone(),
            ),
            signal,
        );
    }

    context
}

fn asset_change_transition_key(change: &SnapshotAssetChange) -> Option<(String, Option<String>)> {
    let old_path = change.old_asset.as_ref()?.path.clone();
    let new_path = change.new_asset.as_ref().map(|asset| asset.path.clone());
    Some((old_path, new_path))
}

fn continuity_signal_for_change(
    context: &InferenceContinuityContext,
    change: &SnapshotAssetChange,
) -> Option<ContinuitySignal> {
    let key = asset_change_transition_key(change)?;
    context.change_signals.get(&key).cloned()
}

fn continuity_signal_for_candidate(
    context: &InferenceContinuityContext,
    candidate: &CandidateMappingChange,
) -> Option<ContinuitySignal> {
    context
        .mapping_signals
        .get(&(
            candidate.old_asset.path.clone(),
            candidate.new_asset.path.clone(),
        ))
        .cloned()
}

fn find_continuity_signal(
    continuity: &VersionContinuityIndex,
    old_version: &str,
    new_version: &str,
    old_path: &str,
    new_path: Option<&str>,
) -> Option<ContinuitySignal> {
    continuity.threads.iter().find_map(|thread| {
        let (index, observation) =
            thread
                .observations
                .iter()
                .enumerate()
                .find(|(_, observation)| {
                    observation.from_version_id == old_version
                        && observation.to_version_id == new_version
                        && observation.from_path == old_path
                        && observation.to_path.as_deref() == new_path
                })?;
        let summary = continuity
            .thread_summaries
            .iter()
            .find(|summary| summary.thread_id == thread.thread_id)
            .cloned()
            .unwrap_or_else(|| summarize_continuity_thread_for_inference(thread));
        let previous = &thread.observations[..index];
        let later = &thread.observations[index + 1..];

        Some(ContinuitySignal {
            thread_id: thread.thread_id.clone(),
            current_from_path: observation.from_path.clone(),
            current_to_path: observation.to_path.clone(),
            first_seen_version: summary.first_seen_version,
            latest_observed_version: summary.latest_observed_version,
            latest_live_version: summary.latest_live_version,
            current_relation: observation.relation.clone(),
            prior_persisted_steps: count_continuity_relation(
                previous,
                VersionContinuityRelation::Persisted,
            ),
            prior_rename_steps: count_continuity_relation(
                previous,
                VersionContinuityRelation::RenameOrRepath,
            ),
            prior_container_movement_steps: count_continuity_relation(
                previous,
                VersionContinuityRelation::ContainerMovement,
            ),
            prior_layout_drift_steps: count_continuity_relation(
                previous,
                VersionContinuityRelation::LayoutDrift,
            ),
            prior_material_steps: previous
                .iter()
                .filter(|observation| observation.relation != VersionContinuityRelation::Persisted)
                .count(),
            later_rename_steps: count_continuity_relation(
                later,
                VersionContinuityRelation::RenameOrRepath,
            ),
            later_container_movement_steps: count_continuity_relation(
                later,
                VersionContinuityRelation::ContainerMovement,
            ),
            later_layout_drift_steps: count_continuity_relation(
                later,
                VersionContinuityRelation::LayoutDrift,
            ),
            terminal_relation: summary.terminal_relation,
            terminal_version: summary.terminal_version,
            review_required: summary.review_required,
        })
    })
}

fn summarize_continuity_thread_for_inference(
    thread: &VersionContinuityThread,
) -> VersionContinuityThreadSummary {
    let latest_observed_version = thread
        .observations
        .last()
        .map(|observation| observation.to_version_id.clone())
        .unwrap_or_else(|| thread.anchor_version_id.clone());
    let latest_relation = thread
        .observations
        .last()
        .map(|observation| &observation.relation);
    let latest_live_version =
        if latest_relation.is_none_or(|relation| continuity_relation_continues_thread(relation)) {
            Some(latest_observed_version.clone())
        } else {
            None
        };
    let terminal_relation = latest_relation
        .filter(|relation| !continuity_relation_continues_thread(relation))
        .cloned();
    let terminal_version = terminal_relation
        .as_ref()
        .map(|_| latest_observed_version.clone());

    VersionContinuityThreadSummary {
        thread_id: thread.thread_id.clone(),
        anchor_version_id: thread.anchor_version_id.clone(),
        anchor: thread.anchor.clone(),
        first_seen_version: thread.anchor_version_id.clone(),
        latest_observed_version,
        latest_live_version,
        terminal_relation,
        terminal_version,
        review_required: thread.review_required,
        ..VersionContinuityThreadSummary::default()
    }
}

fn continuity_relation_continues_thread(relation: &VersionContinuityRelation) -> bool {
    matches!(
        relation,
        VersionContinuityRelation::Persisted
            | VersionContinuityRelation::RenameOrRepath
            | VersionContinuityRelation::ContainerMovement
            | VersionContinuityRelation::LayoutDrift
    )
}

fn count_continuity_relation(
    observations: &[VersionContinuityObservation],
    relation: VersionContinuityRelation,
) -> usize {
    observations
        .iter()
        .filter(|observation| observation.relation == relation)
        .count()
}

impl ContinuitySignal {
    fn stable_before_current_change(&self) -> bool {
        self.prior_persisted_steps > 0
            && self.prior_material_steps == 0
            && self.current_relation != VersionContinuityRelation::Persisted
    }

    fn total_rename_steps(&self) -> usize {
        self.prior_rename_steps
            + usize::from(self.current_relation == VersionContinuityRelation::RenameOrRepath)
            + self.later_rename_steps
    }

    fn total_container_movement_steps(&self) -> usize {
        self.prior_container_movement_steps
            + usize::from(self.current_relation == VersionContinuityRelation::ContainerMovement)
            + self.later_container_movement_steps
    }

    fn total_layout_drift_steps(&self) -> usize {
        self.prior_layout_drift_steps
            + usize::from(self.current_relation == VersionContinuityRelation::LayoutDrift)
            + self.later_layout_drift_steps
    }

    fn has_terminal_or_review_history(&self) -> bool {
        self.review_required || self.terminal_relation.is_some()
    }

    fn terminal_after_current(&self, current_new_version: &str) -> bool {
        self.terminal_version
            .as_deref()
            .is_some_and(|version| version != current_new_version)
    }
}

fn build_mapping_hint_continuity_context(
    signal: &ContinuitySignal,
    current_new_version: &str,
) -> InferredMappingContinuityContext {
    InferredMappingContinuityContext {
        thread_id: Some(signal.thread_id.clone()),
        first_seen_version: Some(signal.first_seen_version.clone()),
        latest_observed_version: Some(signal.latest_observed_version.clone()),
        latest_live_version: signal.latest_live_version.clone(),
        stable_before_current_change: signal.stable_before_current_change(),
        total_rename_steps: signal.total_rename_steps(),
        total_container_movement_steps: signal.total_container_movement_steps(),
        total_layout_drift_steps: signal.total_layout_drift_steps(),
        review_required_history: signal.review_required,
        terminal_relation: signal.terminal_relation.clone(),
        terminal_version: signal.terminal_version.clone(),
        terminal_after_current: signal.terminal_after_current(current_new_version),
        instability_detected: signal.total_layout_drift_steps() >= 2
            || signal.review_required
            || signal.terminal_relation.is_some(),
    }
}

fn apply_continuity_to_mapping_hint(
    confidence: &mut f32,
    compatibility: &mut RemapCompatibility,
    reasons: &mut Vec<String>,
    evidence: &mut Vec<String>,
    signal: &ContinuitySignal,
    current_new_version: &str,
) {
    if signal.stable_before_current_change() {
        *confidence = (*confidence + 0.03).clamp(0.0, 1.0);
        reasons.push(format!(
            "continuity kept this thread stable for {} earlier step(s) before the current {}",
            signal.prior_persisted_steps,
            continuity_relation_label(&signal.current_relation)
        ));
    }

    if signal.total_rename_steps() >= 2 {
        reasons.push(format!(
            "continuity shows {} rename/repath steps on this thread; path movement recurs across versions, so keep the remap review-first",
            signal.total_rename_steps()
        ));
    }

    if signal.total_container_movement_steps() >= 2 {
        reasons.push(format!(
            "continuity shows {} container/package movement steps on this thread; verify package context as well as path equivalence",
            signal.total_container_movement_steps()
        ));
    }

    if signal.total_layout_drift_steps() >= 2 {
        *confidence = (*confidence - 0.08).clamp(0.0, 1.0);
        *compatibility = downgrade_compatibility_for_layout_history(compatibility.clone());
        reasons.push(
            "continuity records repeated layout drift on this thread; remap-only repair is risky"
                .to_string(),
        );
    }

    if signal.review_required {
        *confidence = (*confidence - 0.05).clamp(0.0, 1.0);
        reasons.push(
            "continuity already marked this thread review-required elsewhere in the broader chain"
                .to_string(),
        );
    }

    if let Some(relation) = signal.terminal_relation.as_ref() {
        let penalty = match relation {
            VersionContinuityRelation::Ambiguous
            | VersionContinuityRelation::InsufficientEvidence => 0.12,
            VersionContinuityRelation::Replacement | VersionContinuityRelation::Removed => 0.10,
            _ => 0.0,
        };
        *confidence = (*confidence - penalty).clamp(0.0, 1.0);
        *compatibility =
            downgrade_compatibility_for_terminal_history(compatibility.clone(), relation);
        let timing = if signal.terminal_after_current(current_new_version) {
            "later marks"
        } else {
            "marks"
        };
        reasons.push(format!(
            "continuity {timing} this thread as {} in {}",
            continuity_relation_label(relation),
            signal
                .terminal_version
                .as_deref()
                .unwrap_or(signal.latest_observed_version.as_str())
        ));
    }

    evidence.push(continuity_signal_evidence(signal, current_new_version));
}

fn downgrade_compatibility_for_layout_history(
    compatibility: RemapCompatibility,
) -> RemapCompatibility {
    match compatibility {
        RemapCompatibility::LikelyCompatible => RemapCompatibility::CompatibleWithCaution,
        RemapCompatibility::CompatibleWithCaution => RemapCompatibility::StructurallyRisky,
        other => other,
    }
}

fn downgrade_compatibility_for_terminal_history(
    compatibility: RemapCompatibility,
    relation: &VersionContinuityRelation,
) -> RemapCompatibility {
    match relation {
        VersionContinuityRelation::Ambiguous | VersionContinuityRelation::InsufficientEvidence => {
            RemapCompatibility::InsufficientEvidence
        }
        VersionContinuityRelation::Replacement => match compatibility {
            RemapCompatibility::LikelyCompatible => RemapCompatibility::CompatibleWithCaution,
            RemapCompatibility::CompatibleWithCaution => RemapCompatibility::StructurallyRisky,
            other => other,
        },
        VersionContinuityRelation::Removed => match compatibility {
            RemapCompatibility::LikelyCompatible => RemapCompatibility::CompatibleWithCaution,
            other => other,
        },
        _ => compatibility,
    }
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

fn continuity_signal_evidence(signal: &ContinuitySignal, current_new_version: &str) -> String {
    let focus = signal
        .current_to_path
        .as_deref()
        .unwrap_or(signal.current_from_path.as_str());
    let mut details = vec![format!(
        "continuity thread {focus} spans {} -> {}",
        signal.first_seen_version, signal.latest_observed_version
    )];
    if signal.stable_before_current_change() {
        details.push(format!(
            "{} prior persisted step(s) before current {}",
            signal.prior_persisted_steps,
            continuity_relation_label(&signal.current_relation)
        ));
    }
    if signal.total_rename_steps() >= 2 {
        details.push(format!(
            "{} rename/repath step(s)",
            signal.total_rename_steps()
        ));
    }
    if signal.total_container_movement_steps() >= 2 {
        details.push(format!(
            "{} container movement step(s)",
            signal.total_container_movement_steps()
        ));
    }
    if signal.total_layout_drift_steps() >= 2 {
        details.push(format!(
            "{} layout-drift step(s)",
            signal.total_layout_drift_steps()
        ));
    }
    if signal.review_required {
        details.push("thread marked review-required".to_string());
    }
    if let Some(relation) = signal.terminal_relation.as_ref() {
        let timing = if signal.terminal_after_current(current_new_version) {
            "later terminates"
        } else {
            "terminates"
        };
        details.push(format!(
            "{timing} as {} in {}",
            continuity_relation_label(relation),
            signal
                .terminal_version
                .as_deref()
                .unwrap_or(signal.latest_observed_version.as_str())
        ));
    } else if let Some(latest_live_version) = signal.latest_live_version.as_deref() {
        details.push(format!("latest live version {latest_live_version}"));
    }

    details.join("; ")
}

fn apply_continuity_inference_adjustments(
    probable_crash_causes: &mut Vec<ProbableCrashCause>,
    suggested_fixes: &mut Vec<SuggestedFix>,
    compare_report: &SnapshotCompareReport,
    continuity_context: &InferenceContinuityContext,
    mapping_support: f32,
    buffer_support: f32,
    confidence_scale: f32,
) {
    let current_new_version = compare_report.new_snapshot.version_id.as_str();
    let structural_signals = dedup_continuity_signals(
        compare_report
            .changed_assets
            .iter()
            .filter(|change| is_structural_change(change))
            .filter_map(|change| continuity_signal_for_change(continuity_context, change))
            .collect(),
    );
    let removed_signals = dedup_continuity_signals(
        compare_report
            .removed_assets
            .iter()
            .filter(|change| change.change_type == SnapshotChangeType::Removed)
            .filter_map(|change| continuity_signal_for_change(continuity_context, change))
            .collect(),
    );
    let candidate_signals = dedup_continuity_signals(
        compare_report
            .candidate_mapping_changes
            .iter()
            .filter_map(|candidate| continuity_signal_for_candidate(continuity_context, candidate))
            .collect(),
    );
    let structurally_drifted_signals = dedup_continuity_signals(
        compare_report
            .candidate_mapping_changes
            .iter()
            .filter(|candidate| candidate.compatibility == RemapCompatibility::StructurallyRisky)
            .filter_map(|candidate| continuity_signal_for_candidate(continuity_context, candidate))
            .collect(),
    );

    if let Some(cause) = probable_crash_causes
        .iter_mut()
        .find(|cause| cause.code == "buffer_layout_changed")
    {
        append_continuity_signal_context(
            &mut cause.reasons,
            &mut cause.evidence,
            &structural_signals,
            current_new_version,
            "structurally changed",
        );
        if structural_signals.iter().any(|signal| {
            signal.total_layout_drift_steps() >= 2 || signal.has_terminal_or_review_history()
        }) {
            cause.confidence = (cause.confidence + 0.05).clamp(0.0, 1.0);
        }
    }

    if let Some(fix) = suggested_fixes
        .iter_mut()
        .find(|fix| fix.code == "review_buffer_layout_and_runtime_guards")
        && !structural_signals.is_empty()
    {
        append_continuity_signal_context(
            &mut fix.reasons,
            &mut fix.evidence,
            &structural_signals,
            current_new_version,
            "layout-sensitive",
        );
        fix.actions.push(
            "Inspect continuity milestones for repeated layout drift or later terminal states before promoting any repair path."
                .to_string(),
        );
        if structural_signals
            .iter()
            .any(|signal| signal.total_layout_drift_steps() >= 2)
        {
            fix.confidence = (fix.confidence + 0.04).clamp(0.0, 1.0);
        }
    }

    if let Some(cause) = probable_crash_causes
        .iter_mut()
        .find(|cause| cause.code == "asset_paths_or_mapping_shifted")
        && !candidate_signals.is_empty()
    {
        append_continuity_signal_context(
            &mut cause.reasons,
            &mut cause.evidence,
            &candidate_signals,
            current_new_version,
            "mapping-candidate",
        );
        let stable_support = candidate_signals
            .iter()
            .filter(|signal| signal.stable_before_current_change())
            .count() as f32;
        let instability = candidate_signals
            .iter()
            .filter(|signal| {
                signal.total_layout_drift_steps() >= 2 || signal.has_terminal_or_review_history()
            })
            .count() as f32;
        cause.confidence =
            (cause.confidence + 0.02 * stable_support - 0.06 * instability).clamp(0.0, 1.0);
    }

    if let Some(fix) = suggested_fixes
        .iter_mut()
        .find(|fix| fix.code == "review_candidate_asset_remaps")
        && !candidate_signals.is_empty()
    {
        append_continuity_signal_context(
            &mut fix.reasons,
            &mut fix.evidence,
            &candidate_signals,
            current_new_version,
            "remap-candidate",
        );
        fix.actions.push(
            "Cross-check continuity history for recent stability, recurring renames, and any later terminal state before approving a mapping."
                .to_string(),
        );
    }

    if let Some(cause) = probable_crash_causes
        .iter_mut()
        .find(|cause| cause.code == "candidate_remap_structural_drift")
        && !structurally_drifted_signals.is_empty()
    {
        append_continuity_signal_context(
            &mut cause.reasons,
            &mut cause.evidence,
            &structurally_drifted_signals,
            current_new_version,
            "structurally drifted remap",
        );
        if structurally_drifted_signals.iter().any(|signal| {
            signal.total_layout_drift_steps() >= 2 || signal.has_terminal_or_review_history()
        }) {
            cause.confidence = (cause.confidence + 0.06).clamp(0.0, 1.0);
        }
    }

    if let Some(fix) = suggested_fixes
        .iter_mut()
        .find(|fix| fix.code == "validate_candidate_remaps_against_layout")
        && !structurally_drifted_signals.is_empty()
    {
        append_continuity_signal_context(
            &mut fix.reasons,
            &mut fix.evidence,
            &structurally_drifted_signals,
            current_new_version,
            "layout-risk remap",
        );
        fix.actions.push(
            "Escalate to manual validation if continuity shows repeated layout drift or a later ambiguous/replacement/removed state."
                .to_string(),
        );
    }

    if let Some(cause) = probable_crash_causes
        .iter_mut()
        .find(|cause| cause.code == "asset_removed_without_clear_replacement")
        && !removed_signals.is_empty()
    {
        append_continuity_signal_context(
            &mut cause.reasons,
            &mut cause.evidence,
            &removed_signals,
            current_new_version,
            "removed",
        );
        if removed_signals
            .iter()
            .any(|signal| signal.has_terminal_or_review_history())
        {
            cause.confidence = (cause.confidence + 0.04).clamp(0.0, 1.0);
        }
    }

    let instability_signals = dedup_continuity_signals(
        structural_signals
            .iter()
            .chain(candidate_signals.iter())
            .chain(removed_signals.iter())
            .filter(|signal| {
                signal.total_layout_drift_steps() >= 2 || signal.has_terminal_or_review_history()
            })
            .cloned()
            .collect(),
    );
    if instability_signals.is_empty() {
        return;
    }

    let instability_count = instability_signals.len().min(3) as f32;
    let confidence =
        (0.42 + 0.08 * instability_count + 0.10 * buffer_support + 0.08 * mapping_support)
            .clamp(0.0, 1.0)
            * confidence_scale;
    let risk = if instability_signals
        .iter()
        .any(|signal| signal.total_layout_drift_steps() >= 2)
    {
        RiskLevel::High
    } else {
        RiskLevel::Medium
    };
    let mut reasons = vec![
        format!(
            "continuity surfaces {} relevant thread(s) with repeated drift or terminal/review history",
            instability_signals.len()
        ),
        "broader version history suggests one-shot remap assumptions may not hold for every affected asset thread"
            .to_string(),
    ];
    let mut evidence = instability_signals
        .iter()
        .take(4)
        .map(|signal| continuity_signal_evidence(signal, current_new_version))
        .collect::<Vec<_>>();
    evidence.push(format!(
        "continuity-informed confidence blended instability count with WWMI buffer {:.3} and mapping {:.3} support",
        buffer_support, mapping_support
    ));

    probable_crash_causes.push(ProbableCrashCause {
        code: "continuity_thread_instability".to_string(),
        summary: "Broader continuity history shows some affected asset threads drift repeatedly or terminate ambiguously, so remap-only repair may not stay safe across versions.".to_string(),
        confidence,
        risk: risk.clone(),
        affected_assets: collect_continuity_affected_assets(&instability_signals),
        related_patterns: vec![
            WwmiPatternKind::BufferLayoutOrCapacityFix,
            WwmiPatternKind::MappingOrHashUpdate,
        ],
        reasons: reasons.clone(),
        evidence: evidence.clone(),
    });

    reasons.push(
        "continuity should be treated as supporting evidence; keep fixes review-oriented when the thread later degrades or terminates"
            .to_string(),
    );
    evidence.push(
        "use continuity milestones to decide whether repair should stay manual rather than auto-promoted"
            .to_string(),
    );
    suggested_fixes.push(SuggestedFix {
        code: "review_continuity_thread_history_before_repair".to_string(),
        summary: "Review continuity milestones for unstable asset threads before trusting remap-only repair decisions.".to_string(),
        confidence,
        priority: risk,
        related_patterns: vec![
            WwmiPatternKind::BufferLayoutOrCapacityFix,
            WwmiPatternKind::MappingOrHashUpdate,
        ],
        actions: vec![
            "Inspect the full continuity thread for each flagged asset and note any repeated layout drift, recurring rename/repath, or terminal state.".to_string(),
            "Downgrade repair confidence when the broader chain later becomes ambiguous, replacement-like, or removed.".to_string(),
            "Keep continuity-unstable mappings in review until a human validates both the current pair and the broader version history.".to_string(),
        ],
        reasons,
        evidence,
    });
}

fn append_continuity_signal_context(
    reasons: &mut Vec<String>,
    evidence: &mut Vec<String>,
    signals: &[ContinuitySignal],
    current_new_version: &str,
    subject: &str,
) {
    if signals.is_empty() {
        return;
    }

    let stable_count = signals
        .iter()
        .filter(|signal| signal.stable_before_current_change())
        .count();
    let recurring_rename_count = signals
        .iter()
        .filter(|signal| signal.total_rename_steps() >= 2)
        .count();
    let recurring_layout_count = signals
        .iter()
        .filter(|signal| signal.total_layout_drift_steps() >= 2)
        .count();
    let terminal_or_review_count = signals
        .iter()
        .filter(|signal| signal.has_terminal_or_review_history())
        .count();

    if stable_count > 0 {
        reasons.push(format!(
            "continuity shows {stable_count} {subject} thread(s) stayed stable across earlier versions before the current pair drifted"
        ));
    }
    if recurring_rename_count > 0 {
        reasons.push(format!(
            "continuity shows recurring rename/repath history on {recurring_rename_count} {subject} thread(s)"
        ));
    }
    if recurring_layout_count > 0 {
        reasons.push(format!(
            "continuity records repeated layout drift on {recurring_layout_count} {subject} thread(s)"
        ));
    }
    if terminal_or_review_count > 0 {
        reasons.push(format!(
            "continuity marks {terminal_or_review_count} {subject} thread(s) as terminal or review-required somewhere in the broader chain"
        ));
    }

    evidence.extend(
        signals
            .iter()
            .take(4)
            .map(|signal| continuity_signal_evidence(signal, current_new_version)),
    );
}

fn collect_continuity_affected_assets(signals: &[ContinuitySignal]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut affected_assets = Vec::new();

    for signal in signals {
        for path in [
            Some(signal.current_from_path.as_str()),
            signal.current_to_path.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if seen.insert(path.to_string()) {
                affected_assets.push(path.to_string());
            }
        }
    }

    affected_assets
}

fn dedup_continuity_signals(signals: Vec<ContinuitySignal>) -> Vec<ContinuitySignal> {
    let mut seen = BTreeSet::new();
    signals
        .into_iter()
        .filter(|signal| seen.insert(signal.thread_id.clone()))
        .collect()
}

fn apply_low_signal_inference_guardrails(
    probable_crash_causes: &mut [ProbableCrashCause],
    suggested_fixes: &mut [SuggestedFix],
    candidate_mapping_hints: &mut [InferredMappingHint],
    scope: &InferenceScopeContext,
) {
    for cause in probable_crash_causes {
        cause.reasons.push(
            "low-signal compare scope limits semantic certainty; confidence should be interpreted conservatively"
                .to_string(),
        );
        cause.evidence.extend(
            scope
                .notes
                .iter()
                .take(3)
                .map(|note| format!("scope context: {note}")),
        );
    }

    for fix in suggested_fixes {
        fix.reasons.push(
            "low-signal compare scope limits semantic certainty; validate fixes manually before applying changes"
                .to_string(),
        );
        fix.evidence.extend(
            scope
                .notes
                .iter()
                .take(3)
                .map(|note| format!("scope context: {note}")),
        );
    }

    for hint in candidate_mapping_hints {
        hint.needs_review = true;
        hint.reasons.push(
            "low-signal compare scope keeps this mapping hint in review-first mode".to_string(),
        );
    }
}

fn reason_to_string(reason: &SnapshotCompareReason) -> String {
    format!("{}: {}", reason.code, reason.message)
}

fn has_reason_code(reasons: &[String], code: &str) -> bool {
    let prefix = format!("{code}:");
    reasons
        .iter()
        .any(|reason| reason.trim() == code || reason.trim_start().starts_with(&prefix))
}

fn candidate_has_reason_code(candidate: &CandidateMappingChange, code: &str) -> bool {
    candidate.reasons.iter().any(|reason| reason.code == code)
}

fn adjust_hint_compatibility(
    candidate: &CandidateMappingChange,
    scope: &InferenceScopeContext,
) -> RemapCompatibility {
    if !scope.low_signal_compare {
        return candidate.compatibility.clone();
    }

    match candidate.compatibility {
        RemapCompatibility::LikelyCompatible => {
            if candidate_has_reason_code(candidate, "signature_exact")
                || candidate_has_reason_code(candidate, "asset_hash_exact")
            {
                RemapCompatibility::CompatibleWithCaution
            } else if has_low_signal_structural_remap_anchor(candidate) {
                RemapCompatibility::LikelyCompatible
            } else {
                RemapCompatibility::InsufficientEvidence
            }
        }
        RemapCompatibility::CompatibleWithCaution => RemapCompatibility::InsufficientEvidence,
        _ => candidate.compatibility.clone(),
    }
}

fn has_low_signal_structural_remap_anchor(candidate: &CandidateMappingChange) -> bool {
    let has_path_and_name_anchor = candidate_has_reason_code(candidate, "normalized_name_exact")
        && candidate_has_reason_code(candidate, "same_parent_directory");
    let has_structural_compatibility =
        candidate_has_reason_code(candidate, "structural_layout_compatible")
            || candidate_has_reason_code(candidate, "buffer_layout_compatible");
    let has_conflict = candidate_has_reason_code(candidate, "identity_conflict_detected")
        || candidate_has_reason_code(candidate, "same_asset_but_structural_drift")
        || candidate_has_reason_code(candidate, "buffer_layout_validation_needed")
        || candidate_has_reason_code(candidate, "weak_identity_evidence")
        || candidate_has_reason_code(candidate, "signature_mismatch")
        || candidate_has_reason_code(candidate, "kind_mismatch");

    has_path_and_name_anchor
        && has_structural_compatibility
        && !has_conflict
        && candidate.confidence >= 0.90
        && candidate.confidence_gap.is_none_or(|gap| gap >= 0.12)
}

fn compatibility_label(compatibility: &RemapCompatibility) -> &'static str {
    match compatibility {
        RemapCompatibility::LikelyCompatible => "likely compatible",
        RemapCompatibility::CompatibleWithCaution => "compatible with caution",
        RemapCompatibility::StructurallyRisky => "structurally risky",
        RemapCompatibility::IncompatibleBlocked => "incompatible/blocked",
        RemapCompatibility::InsufficientEvidence => "insufficient evidence",
    }
}

fn pattern_support(knowledge: &WwmiKnowledgeBase, kind: WwmiPatternKind) -> f32 {
    knowledge
        .patterns
        .iter()
        .find(|pattern| pattern.kind == kind)
        .map(|pattern| pattern.average_fix_likelihood.clamp(0.0, 1.0))
        .unwrap_or(0.0)
}

fn pattern_evidence(knowledge: &WwmiKnowledgeBase, kinds: &[WwmiPatternKind]) -> Vec<String> {
    let mut evidence = Vec::new();
    for kind in kinds {
        if let Some(pattern) = knowledge
            .patterns
            .iter()
            .find(|pattern| &pattern.kind == kind)
        {
            evidence.push(format!(
                "WWMI pattern {:?} seen {} times with average fix likelihood {:.3}; example commits: {}",
                pattern.kind,
                pattern.frequency,
                pattern.average_fix_likelihood,
                pattern.example_commits.join(", ")
            ));
        }
    }
    evidence
}

fn current_unix_ms() -> AppResult<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::InvalidInput(format!("system clock error: {error}")))?
        .as_millis())
}

#[cfg(test)]
mod tests {
    use crate::{
        compare::{
            CandidateMappingChange, RemapCompatibility, RiskLevel, SnapshotAssetChange,
            SnapshotAssetSummary, SnapshotChangeType, SnapshotCompareReason, SnapshotCompareReport,
            SnapshotCompareScopeContext, SnapshotCompareScopeInfo, SnapshotCompareSummary,
            SnapshotVersionInfo,
        },
        report::{
            DiffStatus, TechnicalMetadata, VersionContinuityIndex, VersionContinuityObservation,
            VersionContinuityRelation, VersionContinuitySource, VersionContinuityThread,
            VersionedItem,
        },
        wwmi::{
            WwmiEvidenceCommit, WwmiFixPattern, WwmiKeywordStat, WwmiKnowledgeBase,
            WwmiKnowledgeRepoInfo, WwmiKnowledgeSummary, WwmiPatternKind,
        },
    };

    use super::FixInferenceEngine;

    #[test]
    fn inference_combines_compare_and_wwmi_patterns() {
        let engine = FixInferenceEngine;
        let compare_report = SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: "2.4.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 2,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: "2.5.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 2,
            },
            scope: SnapshotCompareScopeContext::default(),
            summary: SnapshotCompareSummary {
                total_old_assets: 2,
                total_new_assets: 2,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 1,
                candidate_mapping_changes: 1,
                identity_changed_assets: 0,
                layout_changed_assets: 0,
                structural_changed_assets: 1,
                naming_only_changed_assets: 0,
                cosmetic_only_changed_assets: 0,
                provenance_changed_assets: 0,
                container_moved_assets: 0,
                lineage_rename_or_repath_assets: 0,
                lineage_container_movement_assets: 0,
                lineage_layout_drift_assets: 0,
                lineage_replacement_assets: 0,
                lineage_ambiguous_assets: 0,
                lineage_insufficient_evidence_assets: 0,
                ambiguous_candidate_mapping_changes: 0,
                high_confidence_candidate_mapping_changes: 0,
            },
            added_assets: Vec::new(),
            removed_assets: vec![SnapshotAssetChange {
                change_type: SnapshotChangeType::Removed,
                old_asset: Some(asset_summary("Content/Weapon/Sword.weapon")),
                new_asset: None,
                changed_fields: vec!["path_presence".to_string()],
                probable_impact: RiskLevel::Medium,
                crash_risk: RiskLevel::Medium,
                suspected_mapping_change: true,
                lineage: crate::compare::AssetLineageKind::InsufficientEvidence,
                reasons: vec![SnapshotCompareReason {
                    code: "asset_removed".to_string(),
                    message: "old path removed".to_string(),
                }],
            }],
            changed_assets: vec![SnapshotAssetChange {
                change_type: SnapshotChangeType::Changed,
                old_asset: Some(asset_summary("Content/Character/HeroA/Body.mesh")),
                new_asset: Some(asset_summary("Content/Character/HeroA/Body.mesh")),
                changed_fields: vec!["vertex_count".to_string(), "section_count".to_string()],
                probable_impact: RiskLevel::High,
                crash_risk: RiskLevel::High,
                suspected_mapping_change: true,
                lineage: crate::compare::AssetLineageKind::LayoutDrift,
                reasons: vec![SnapshotCompareReason {
                    code: "vertex_count_changed".to_string(),
                    message: "vertex count changed".to_string(),
                }],
            }],
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Content/Weapon/Sword.weapon"),
                new_asset: asset_summary("Content/Weapon/Sword_v2.weapon"),
                confidence: 0.82,
                compatibility: RemapCompatibility::CompatibleWithCaution,
                lineage: crate::compare::AssetLineageKind::RenameOrRepath,
                reasons: vec![SnapshotCompareReason {
                    code: "normalized_name_exact".to_string(),
                    message: "normalized name matched".to_string(),
                }],
                runner_up_confidence: None,
                confidence_gap: None,
                ambiguous: false,
            }],
        };
        let knowledge = WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: 1,
            repo: WwmiKnowledgeRepoInfo {
                input: "repo".to_string(),
                resolved_path: "repo".to_string(),
                origin_url: Some("https://github.com/SpectrumQT/WWMI-Package".to_string()),
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: 10,
                fix_like_commits: 6,
                discovered_patterns: 3,
            },
            patterns: vec![
                WwmiFixPattern {
                    kind: WwmiPatternKind::BufferLayoutOrCapacityFix,
                    description: "buffer".to_string(),
                    frequency: 2,
                    average_fix_likelihood: 0.80,
                    example_commits: vec!["abc123".to_string()],
                },
                WwmiFixPattern {
                    kind: WwmiPatternKind::MappingOrHashUpdate,
                    description: "mapping".to_string(),
                    frequency: 3,
                    average_fix_likelihood: 0.75,
                    example_commits: vec!["def456".to_string()],
                },
                WwmiFixPattern {
                    kind: WwmiPatternKind::RuntimeConfigChange,
                    description: "runtime".to_string(),
                    frequency: 1,
                    average_fix_likelihood: 0.70,
                    example_commits: vec!["ghi789".to_string()],
                },
            ],
            keyword_stats: vec![WwmiKeywordStat {
                keyword: "mapping".to_string(),
                count: 3,
            }],
            evidence_commits: vec![WwmiEvidenceCommit {
                hash: "abc123".to_string(),
                subject: "Fixed startup crash".to_string(),
                unix_time: 1,
                decorations: String::new(),
                commit_url: None,
                fix_likelihood: 0.80,
                changed_files: vec!["WWMI/d3dx.ini".to_string()],
                detected_patterns: vec![WwmiPatternKind::RuntimeConfigChange],
                detected_keywords: vec!["crash".to_string()],
                reasons: vec!["subject contains fix".to_string()],
            }],
        };

        let report = engine.infer(&compare_report, &knowledge);

        assert!(
            report
                .probable_crash_causes
                .iter()
                .any(|cause| cause.code == "buffer_layout_changed")
        );
        assert!(
            report
                .probable_crash_causes
                .iter()
                .any(|cause| cause.code == "asset_paths_or_mapping_shifted")
        );
        assert!(
            report
                .suggested_fixes
                .iter()
                .any(|fix| fix.code == "review_candidate_asset_remaps")
        );
        assert_eq!(report.candidate_mapping_hints.len(), 1);
        assert!(!report.candidate_mapping_hints[0].ambiguous);
        assert_eq!(
            report.candidate_mapping_hints[0].compatibility,
            RemapCompatibility::CompatibleWithCaution
        );
        assert!(report.summary.highest_confidence >= 0.75);
        assert!(!report.scope.low_signal_compare);
    }

    #[test]
    fn inference_adds_low_signal_caution_without_stopping_generation() {
        let engine = FixInferenceEngine;
        let mut compare_report = SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: "3.0.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: "3.1.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            scope: SnapshotCompareScopeContext {
                old_snapshot: SnapshotCompareScopeInfo {
                    acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                    capture_mode: Some("local_filesystem_inventory".to_string()),
                    mostly_install_or_package_level: true,
                    meaningful_content_coverage: false,
                    meaningful_character_coverage: false,
                    meaningful_asset_record_enrichment: false,
                    content_like_path_count: 1,
                    character_path_count: 0,
                    non_content_path_count: 2,
                    low_signal_for_character_analysis: true,
                    note: Some("install-level snapshot".to_string()),
                    ..Default::default()
                },
                new_snapshot: SnapshotCompareScopeInfo {
                    acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                    capture_mode: Some("local_filesystem_inventory".to_string()),
                    mostly_install_or_package_level: true,
                    meaningful_content_coverage: false,
                    meaningful_character_coverage: false,
                    meaningful_asset_record_enrichment: false,
                    content_like_path_count: 1,
                    character_path_count: 0,
                    non_content_path_count: 3,
                    low_signal_for_character_analysis: true,
                    note: Some("install-level snapshot".to_string()),
                    ..Default::default()
                },
                low_signal_compare: true,
                scope_narrowing_detected: false,
                scope_induced_removals_likely: false,
                notes: vec!["low-signal compare scope".to_string()],
            },
            summary: SnapshotCompareSummary {
                total_old_assets: 1,
                total_new_assets: 1,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 0,
                candidate_mapping_changes: 1,
                identity_changed_assets: 0,
                layout_changed_assets: 0,
                structural_changed_assets: 0,
                naming_only_changed_assets: 0,
                cosmetic_only_changed_assets: 0,
                provenance_changed_assets: 0,
                container_moved_assets: 0,
                lineage_rename_or_repath_assets: 0,
                lineage_container_movement_assets: 0,
                lineage_layout_drift_assets: 0,
                lineage_replacement_assets: 0,
                lineage_ambiguous_assets: 0,
                lineage_insufficient_evidence_assets: 0,
                ambiguous_candidate_mapping_changes: 0,
                high_confidence_candidate_mapping_changes: 1,
            },
            added_assets: Vec::new(),
            removed_assets: vec![SnapshotAssetChange {
                change_type: SnapshotChangeType::Removed,
                old_asset: Some(asset_summary(
                    "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak",
                )),
                new_asset: None,
                changed_fields: vec!["path_presence".to_string()],
                probable_impact: RiskLevel::Medium,
                crash_risk: RiskLevel::Medium,
                suspected_mapping_change: true,
                lineage: crate::compare::AssetLineageKind::InsufficientEvidence,
                reasons: vec![SnapshotCompareReason {
                    code: "asset_removed".to_string(),
                    message: "old path removed".to_string(),
                }],
            }],
            changed_assets: Vec::new(),
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Client/Content/Paks/pakchunk0-WindowsNoEditor.pak"),
                new_asset: asset_summary("Client/Content/Paks/pakchunk1-WindowsNoEditor.pak"),
                confidence: 0.90,
                compatibility: RemapCompatibility::InsufficientEvidence,
                lineage: crate::compare::AssetLineageKind::ContainerMovement,
                reasons: vec![SnapshotCompareReason {
                    code: "same_parent_directory".to_string(),
                    message: "same folder".to_string(),
                }],
                runner_up_confidence: None,
                confidence_gap: None,
                ambiguous: false,
            }],
        };
        // Ensure summary counts stay aligned with payload to keep behavior deterministic.
        compare_report.summary.candidate_mapping_changes =
            compare_report.candidate_mapping_changes.len();
        compare_report.summary.removed_assets = compare_report.removed_assets.len();

        let knowledge = WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: 1,
            repo: WwmiKnowledgeRepoInfo {
                input: "repo".to_string(),
                resolved_path: "repo".to_string(),
                origin_url: None,
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: 1,
                fix_like_commits: 1,
                discovered_patterns: 1,
            },
            patterns: vec![WwmiFixPattern {
                kind: WwmiPatternKind::MappingOrHashUpdate,
                description: "mapping".to_string(),
                frequency: 1,
                average_fix_likelihood: 0.8,
                example_commits: vec!["abc".to_string()],
            }],
            keyword_stats: vec![WwmiKeywordStat {
                keyword: "mapping".to_string(),
                count: 1,
            }],
            evidence_commits: vec![WwmiEvidenceCommit {
                hash: "abc".to_string(),
                subject: "fix mapping".to_string(),
                unix_time: 1,
                decorations: String::new(),
                commit_url: None,
                fix_likelihood: 0.8,
                changed_files: vec!["WWMI/d3dx.ini".to_string()],
                detected_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                detected_keywords: vec!["mapping".to_string()],
                reasons: vec!["subject contains fix".to_string()],
            }],
        };

        let report = engine.infer(&compare_report, &knowledge);

        assert!(report.scope.low_signal_compare);
        assert!(
            report
                .scope
                .notes
                .iter()
                .any(|note| note.contains("low-signal"))
        );
        assert!(!report.candidate_mapping_hints.is_empty());
        assert!(report.candidate_mapping_hints[0].needs_review);
        assert_eq!(
            report.candidate_mapping_hints[0].compatibility,
            RemapCompatibility::InsufficientEvidence
        );
        assert!(
            report.candidate_mapping_hints[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("low-signal"))
        );
    }

    #[test]
    fn inference_keeps_strong_structural_remaps_visible_in_low_signal_scope() {
        let engine = FixInferenceEngine;
        let compare_report = SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: "2.4.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: "2.5.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            scope: SnapshotCompareScopeContext {
                old_snapshot: SnapshotCompareScopeInfo {
                    acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                    capture_mode: Some("local_filesystem_inventory".to_string()),
                    mostly_install_or_package_level: true,
                    meaningful_content_coverage: false,
                    meaningful_character_coverage: false,
                    meaningful_asset_record_enrichment: false,
                    content_like_path_count: 2,
                    character_path_count: 1,
                    non_content_path_count: 2,
                    low_signal_for_character_analysis: true,
                    note: Some("legacy low-signal snapshot".to_string()),
                    ..Default::default()
                },
                new_snapshot: SnapshotCompareScopeInfo {
                    acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                    capture_mode: Some("local_filesystem_inventory".to_string()),
                    mostly_install_or_package_level: true,
                    meaningful_content_coverage: false,
                    meaningful_character_coverage: false,
                    meaningful_asset_record_enrichment: false,
                    content_like_path_count: 2,
                    character_path_count: 1,
                    non_content_path_count: 2,
                    low_signal_for_character_analysis: true,
                    note: Some("legacy low-signal snapshot".to_string()),
                    ..Default::default()
                },
                low_signal_compare: true,
                scope_narrowing_detected: false,
                scope_induced_removals_likely: false,
                notes: vec!["low-signal compare scope".to_string()],
            },
            summary: SnapshotCompareSummary {
                total_old_assets: 1,
                total_new_assets: 1,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 0,
                candidate_mapping_changes: 1,
                identity_changed_assets: 0,
                layout_changed_assets: 0,
                structural_changed_assets: 0,
                naming_only_changed_assets: 0,
                cosmetic_only_changed_assets: 0,
                provenance_changed_assets: 0,
                container_moved_assets: 0,
                lineage_rename_or_repath_assets: 0,
                lineage_container_movement_assets: 0,
                lineage_layout_drift_assets: 0,
                lineage_replacement_assets: 0,
                lineage_ambiguous_assets: 0,
                lineage_insufficient_evidence_assets: 0,
                ambiguous_candidate_mapping_changes: 0,
                high_confidence_candidate_mapping_changes: 1,
            },
            added_assets: Vec::new(),
            removed_assets: Vec::new(),
            changed_assets: Vec::new(),
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Content/Character/Encore/Hair.mesh"),
                new_asset: asset_summary("Content/Character/Encore/Hair_LOD0.mesh"),
                confidence: 0.96,
                compatibility: RemapCompatibility::LikelyCompatible,
                lineage: crate::compare::AssetLineageKind::RenameOrRepath,
                reasons: vec![
                    SnapshotCompareReason {
                        code: "normalized_name_exact".to_string(),
                        message: "same logical asset".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "same_parent_directory".to_string(),
                        message: "same folder".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "structural_layout_compatible".to_string(),
                        message: "layout signals stayed aligned".to_string(),
                    },
                ],
                runner_up_confidence: Some(0.70),
                confidence_gap: Some(0.26),
                ambiguous: false,
            }],
        };

        let knowledge = WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: 1,
            repo: WwmiKnowledgeRepoInfo {
                input: "repo".to_string(),
                resolved_path: "repo".to_string(),
                origin_url: None,
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: 1,
                fix_like_commits: 1,
                discovered_patterns: 1,
            },
            patterns: vec![WwmiFixPattern {
                kind: WwmiPatternKind::MappingOrHashUpdate,
                description: "mapping".to_string(),
                frequency: 1,
                average_fix_likelihood: 0.8,
                example_commits: vec!["abc".to_string()],
            }],
            keyword_stats: vec![WwmiKeywordStat {
                keyword: "mapping".to_string(),
                count: 1,
            }],
            evidence_commits: vec![WwmiEvidenceCommit {
                hash: "abc".to_string(),
                subject: "fix mapping".to_string(),
                unix_time: 1,
                decorations: String::new(),
                commit_url: None,
                fix_likelihood: 0.8,
                changed_files: vec!["WWMI/d3dx.ini".to_string()],
                detected_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                detected_keywords: vec!["mapping".to_string()],
                reasons: vec!["subject contains fix".to_string()],
            }],
        };

        let report = engine.infer(&compare_report, &knowledge);

        assert_eq!(
            report.candidate_mapping_hints[0].compatibility,
            RemapCompatibility::LikelyCompatible
        );
        assert!(report.candidate_mapping_hints[0].needs_review);
    }

    #[test]
    fn inference_flags_structurally_drifted_remap_candidates() {
        let engine = FixInferenceEngine;
        let compare_report = SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: "6.0.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: "6.1.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            scope: SnapshotCompareScopeContext::default(),
            summary: SnapshotCompareSummary {
                total_old_assets: 1,
                total_new_assets: 1,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 0,
                candidate_mapping_changes: 1,
                identity_changed_assets: 0,
                layout_changed_assets: 0,
                structural_changed_assets: 0,
                naming_only_changed_assets: 0,
                cosmetic_only_changed_assets: 0,
                provenance_changed_assets: 0,
                container_moved_assets: 0,
                lineage_rename_or_repath_assets: 0,
                lineage_container_movement_assets: 0,
                lineage_layout_drift_assets: 0,
                lineage_replacement_assets: 0,
                lineage_ambiguous_assets: 0,
                lineage_insufficient_evidence_assets: 0,
                ambiguous_candidate_mapping_changes: 0,
                high_confidence_candidate_mapping_changes: 1,
            },
            added_assets: Vec::new(),
            removed_assets: Vec::new(),
            changed_assets: Vec::new(),
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Content/Character/Encore/Body.mesh"),
                new_asset: asset_summary("Content/Character/Encore/Body_v2.mesh"),
                confidence: 0.91,
                compatibility: RemapCompatibility::StructurallyRisky,
                lineage: crate::compare::AssetLineageKind::LayoutDrift,
                reasons: vec![
                    SnapshotCompareReason {
                        code: "signature_exact".to_string(),
                        message: "asset signature matched".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "same_asset_but_structural_drift".to_string(),
                        message: "structural fields drifted".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "vertex_count_mismatch".to_string(),
                        message: "vertex count changed".to_string(),
                    },
                ],
                runner_up_confidence: None,
                confidence_gap: Some(0.22),
                ambiguous: false,
            }],
        };
        let knowledge = WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: 1,
            repo: WwmiKnowledgeRepoInfo {
                input: "repo".to_string(),
                resolved_path: "repo".to_string(),
                origin_url: None,
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: 3,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            patterns: vec![
                WwmiFixPattern {
                    kind: WwmiPatternKind::BufferLayoutOrCapacityFix,
                    description: "buffer".to_string(),
                    frequency: 2,
                    average_fix_likelihood: 0.85,
                    example_commits: vec!["abc".to_string()],
                },
                WwmiFixPattern {
                    kind: WwmiPatternKind::MappingOrHashUpdate,
                    description: "mapping".to_string(),
                    frequency: 2,
                    average_fix_likelihood: 0.80,
                    example_commits: vec!["def".to_string()],
                },
            ],
            keyword_stats: Vec::new(),
            evidence_commits: Vec::new(),
        };

        let report = engine.infer(&compare_report, &knowledge);

        assert!(
            report
                .probable_crash_causes
                .iter()
                .any(|cause| cause.code == "candidate_remap_structural_drift")
        );
        assert!(
            report
                .suggested_fixes
                .iter()
                .any(|fix| fix.code == "validate_candidate_remaps_against_layout")
        );
        assert_eq!(
            report.candidate_mapping_hints[0].compatibility,
            RemapCompatibility::StructurallyRisky
        );
        assert!(
            report.candidate_mapping_hints[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("structural drift"))
        );
    }

    #[test]
    fn inference_surfaces_blocked_identity_conflicts() {
        let engine = FixInferenceEngine;
        let compare_report = SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: "6.0.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: "6.1.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            scope: SnapshotCompareScopeContext::default(),
            summary: SnapshotCompareSummary {
                total_old_assets: 1,
                total_new_assets: 1,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 0,
                candidate_mapping_changes: 1,
                identity_changed_assets: 0,
                layout_changed_assets: 0,
                structural_changed_assets: 0,
                naming_only_changed_assets: 0,
                cosmetic_only_changed_assets: 0,
                provenance_changed_assets: 0,
                container_moved_assets: 0,
                lineage_rename_or_repath_assets: 0,
                lineage_container_movement_assets: 0,
                lineage_layout_drift_assets: 0,
                lineage_replacement_assets: 0,
                lineage_ambiguous_assets: 0,
                lineage_insufficient_evidence_assets: 0,
                ambiguous_candidate_mapping_changes: 0,
                high_confidence_candidate_mapping_changes: 1,
            },
            added_assets: Vec::new(),
            removed_assets: Vec::new(),
            changed_assets: Vec::new(),
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Content/Character/Encore/Face.mesh"),
                new_asset: asset_summary("Content/Character/Encore/Face_LOD0.mesh"),
                confidence: 0.87,
                compatibility: RemapCompatibility::IncompatibleBlocked,
                lineage: crate::compare::AssetLineageKind::Replacement,
                reasons: vec![
                    SnapshotCompareReason {
                        code: "signature_mismatch".to_string(),
                        message: "signature changed".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "identity_conflict_detected".to_string(),
                        message: "identity conflict".to_string(),
                    },
                ],
                runner_up_confidence: None,
                confidence_gap: Some(0.18),
                ambiguous: false,
            }],
        };
        let knowledge = WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: 1,
            repo: WwmiKnowledgeRepoInfo {
                input: "repo".to_string(),
                resolved_path: "repo".to_string(),
                origin_url: None,
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: 2,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            patterns: vec![
                WwmiFixPattern {
                    kind: WwmiPatternKind::MappingOrHashUpdate,
                    description: "mapping".to_string(),
                    frequency: 2,
                    average_fix_likelihood: 0.80,
                    example_commits: vec!["abc".to_string()],
                },
                WwmiFixPattern {
                    kind: WwmiPatternKind::ShaderLogicChange,
                    description: "shader".to_string(),
                    frequency: 1,
                    average_fix_likelihood: 0.70,
                    example_commits: vec!["def".to_string()],
                },
            ],
            keyword_stats: Vec::new(),
            evidence_commits: Vec::new(),
        };

        let report = engine.infer(&compare_report, &knowledge);

        assert_eq!(
            report.candidate_mapping_hints[0].compatibility,
            RemapCompatibility::IncompatibleBlocked
        );
        assert!(
            report
                .probable_crash_causes
                .iter()
                .any(|cause| cause.code == "candidate_remap_identity_conflict")
        );
        assert!(
            report
                .suggested_fixes
                .iter()
                .any(|fix| fix.code == "inspect_identity_conflicts_before_remap")
        );
    }

    #[test]
    fn infer_with_none_continuity_keeps_pairwise_behavior() {
        let engine = FixInferenceEngine;
        let compare_report = continuity_mapping_compare_report();
        let knowledge = continuity_test_knowledge();

        let mut pairwise = engine.infer(&compare_report, &knowledge);
        let mut with_none = engine.infer_with_continuity(&compare_report, &knowledge, None);

        pairwise.generated_at_unix_ms = 0;
        with_none.generated_at_unix_ms = 0;

        assert_eq!(pairwise, with_none);
    }

    #[test]
    fn inference_uses_continuity_history_for_mapping_review_and_new_fix() {
        let engine = FixInferenceEngine;
        let compare_report = continuity_mapping_compare_report();
        let knowledge = continuity_test_knowledge();
        let continuity = VersionContinuityIndex {
            summary: Default::default(),
            threads: vec![VersionContinuityThread {
                thread_id: "1.0.0:Content/Character/Encore/Body.mesh".to_string(),
                anchor_version_id: "1.0.0".to_string(),
                anchor: continuity_item("Content/Character/Encore/Body.mesh"),
                observations: vec![
                    continuity_observation(
                        "1.0.0",
                        "1.1.0",
                        "Content/Character/Encore/Body.mesh",
                        Some("Content/Character/Encore/Body.mesh"),
                        VersionContinuityRelation::Persisted,
                    ),
                    continuity_observation(
                        "1.1.0",
                        "1.2.0",
                        "Content/Character/Encore/Body.mesh",
                        Some("Content/Character/Encore/Body.mesh"),
                        VersionContinuityRelation::Persisted,
                    ),
                    continuity_observation(
                        "1.2.0",
                        "1.3.0",
                        "Content/Character/Encore/Body.mesh",
                        Some("Content/Character/Encore/Body_LOD0.mesh"),
                        VersionContinuityRelation::RenameOrRepath,
                    ),
                    continuity_observation(
                        "1.3.0",
                        "1.4.0",
                        "Content/Character/Encore/Body_LOD0.mesh",
                        None,
                        VersionContinuityRelation::Removed,
                    ),
                ],
                review_required: false,
            }],
            thread_summaries: Vec::new(),
        };

        let base_report = engine.infer(&compare_report, &knowledge);
        let continuity_report =
            engine.infer_with_continuity(&compare_report, &knowledge, Some(&continuity));
        let base_hint = continuity_mapping_hint(&base_report);
        let continuity_hint = continuity_mapping_hint(&continuity_report);

        assert!(
            continuity_report
                .probable_crash_causes
                .iter()
                .any(|cause| cause.code == "continuity_thread_instability")
        );
        assert!(
            continuity_report
                .suggested_fixes
                .iter()
                .any(|fix| fix.code == "review_continuity_thread_history_before_repair")
        );
        assert!(
            base_report
                .probable_crash_causes
                .iter()
                .all(|cause| cause.code != "continuity_thread_instability")
        );
        assert!(
            base_report
                .suggested_fixes
                .iter()
                .all(|fix| fix.code != "review_continuity_thread_history_before_repair")
        );
        assert!(continuity_hint.confidence < base_hint.confidence);
        assert_eq!(
            continuity_hint.compatibility,
            RemapCompatibility::CompatibleWithCaution
        );
        assert!(
            continuity_hint
                .reasons
                .iter()
                .any(|reason| reason.contains("continuity kept this thread stable"))
        );
        assert!(
            continuity_hint
                .reasons
                .iter()
                .any(|reason| reason.contains("removed in 1.4.0"))
        );
        assert!(
            continuity_hint
                .evidence
                .iter()
                .any(|evidence| evidence.contains("spans 1.0.0 -> 1.4.0"))
        );
    }

    #[test]
    fn inference_downgrades_mapping_hints_when_continuity_shows_repeated_layout_drift() {
        let engine = FixInferenceEngine;
        let compare_report = continuity_layout_compare_report();
        let knowledge = continuity_test_knowledge();
        let continuity = VersionContinuityIndex {
            summary: Default::default(),
            threads: vec![VersionContinuityThread {
                thread_id: "2.0.0:Content/Character/Encore/Cloak.mesh".to_string(),
                anchor_version_id: "2.0.0".to_string(),
                anchor: continuity_item("Content/Character/Encore/Cloak.mesh"),
                observations: vec![
                    continuity_observation(
                        "2.0.0",
                        "2.1.0",
                        "Content/Character/Encore/Cloak.mesh",
                        Some("Content/Character/Encore/Cloak.mesh"),
                        VersionContinuityRelation::LayoutDrift,
                    ),
                    continuity_observation(
                        "2.1.0",
                        "2.2.0",
                        "Content/Character/Encore/Cloak.mesh",
                        Some("Content/Character/Encore/Cloak_v2.mesh"),
                        VersionContinuityRelation::RenameOrRepath,
                    ),
                    continuity_observation(
                        "2.2.0",
                        "2.3.0",
                        "Content/Character/Encore/Cloak_v2.mesh",
                        Some("Content/Character/Encore/Cloak_v3.mesh"),
                        VersionContinuityRelation::LayoutDrift,
                    ),
                ],
                review_required: true,
            }],
            thread_summaries: Vec::new(),
        };

        let base_report = engine.infer(&compare_report, &knowledge);
        let continuity_report =
            engine.infer_with_continuity(&compare_report, &knowledge, Some(&continuity));
        let base_hint = continuity_mapping_hint(&base_report);
        let continuity_hint = continuity_mapping_hint(&continuity_report);

        assert_eq!(
            base_hint.compatibility,
            RemapCompatibility::CompatibleWithCaution
        );
        assert_eq!(
            continuity_hint.compatibility,
            RemapCompatibility::StructurallyRisky
        );
        assert!(continuity_hint.confidence < base_hint.confidence);
        assert!(
            continuity_hint
                .reasons
                .iter()
                .any(|reason| reason.contains("repeated layout drift"))
        );
        assert!(
            continuity_report
                .probable_crash_causes
                .iter()
                .any(|cause| cause.code == "continuity_thread_instability")
        );
        assert!(
            continuity_report
                .suggested_fixes
                .iter()
                .any(|fix| fix.code == "review_continuity_thread_history_before_repair")
        );
    }

    fn asset_summary(path: &str) -> SnapshotAssetSummary {
        SnapshotAssetSummary {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            logical_name: Some("Asset".to_string()),
            normalized_name: Some("asset".to_string()),
            vertex_count: Some(1000),
            index_count: Some(2000),
            material_slots: Some(1),
            section_count: Some(1),
            vertex_stride: None,
            vertex_buffer_count: None,
            index_format: None,
            primitive_topology: None,
            layout_markers: Vec::new(),
            internal_structure: crate::domain::AssetInternalStructure::default(),
            asset_hash: None,
            shader_hash: None,
            signature: None,
            tags: vec!["character".to_string()],
            source: crate::domain::AssetSourceContext::default(),
        }
    }

    fn continuity_mapping_compare_report() -> SnapshotCompareReport {
        SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: "1.2.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: "1.3.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            scope: SnapshotCompareScopeContext::default(),
            summary: SnapshotCompareSummary {
                total_old_assets: 1,
                total_new_assets: 1,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 0,
                candidate_mapping_changes: 1,
                identity_changed_assets: 0,
                layout_changed_assets: 0,
                structural_changed_assets: 0,
                naming_only_changed_assets: 0,
                cosmetic_only_changed_assets: 0,
                provenance_changed_assets: 0,
                container_moved_assets: 0,
                lineage_rename_or_repath_assets: 0,
                lineage_container_movement_assets: 0,
                lineage_layout_drift_assets: 0,
                lineage_replacement_assets: 0,
                lineage_ambiguous_assets: 0,
                lineage_insufficient_evidence_assets: 0,
                ambiguous_candidate_mapping_changes: 0,
                high_confidence_candidate_mapping_changes: 1,
            },
            added_assets: Vec::new(),
            removed_assets: vec![SnapshotAssetChange {
                change_type: SnapshotChangeType::Removed,
                old_asset: Some(asset_summary("Content/Character/Encore/Body.mesh")),
                new_asset: None,
                changed_fields: vec!["path_presence".to_string()],
                probable_impact: RiskLevel::Medium,
                crash_risk: RiskLevel::Medium,
                suspected_mapping_change: true,
                lineage: crate::compare::AssetLineageKind::RenameOrRepath,
                reasons: vec![SnapshotCompareReason {
                    code: "asset_removed".to_string(),
                    message: "old path removed".to_string(),
                }],
            }],
            changed_assets: Vec::new(),
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Content/Character/Encore/Body.mesh"),
                new_asset: asset_summary("Content/Character/Encore/Body_LOD0.mesh"),
                confidence: 0.95,
                compatibility: RemapCompatibility::LikelyCompatible,
                lineage: crate::compare::AssetLineageKind::RenameOrRepath,
                reasons: vec![
                    SnapshotCompareReason {
                        code: "normalized_name_exact".to_string(),
                        message: "same logical asset".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "same_parent_directory".to_string(),
                        message: "same folder".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "structural_layout_compatible".to_string(),
                        message: "layout stayed aligned".to_string(),
                    },
                ],
                runner_up_confidence: None,
                confidence_gap: Some(0.22),
                ambiguous: false,
            }],
        }
    }

    fn continuity_layout_compare_report() -> SnapshotCompareReport {
        SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: "2.1.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: "2.2.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            scope: SnapshotCompareScopeContext::default(),
            summary: SnapshotCompareSummary {
                total_old_assets: 1,
                total_new_assets: 1,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 0,
                candidate_mapping_changes: 1,
                identity_changed_assets: 0,
                layout_changed_assets: 0,
                structural_changed_assets: 0,
                naming_only_changed_assets: 0,
                cosmetic_only_changed_assets: 0,
                provenance_changed_assets: 0,
                container_moved_assets: 0,
                lineage_rename_or_repath_assets: 0,
                lineage_container_movement_assets: 0,
                lineage_layout_drift_assets: 0,
                lineage_replacement_assets: 0,
                lineage_ambiguous_assets: 0,
                lineage_insufficient_evidence_assets: 0,
                ambiguous_candidate_mapping_changes: 0,
                high_confidence_candidate_mapping_changes: 1,
            },
            added_assets: Vec::new(),
            removed_assets: vec![SnapshotAssetChange {
                change_type: SnapshotChangeType::Removed,
                old_asset: Some(asset_summary("Content/Character/Encore/Cloak.mesh")),
                new_asset: None,
                changed_fields: vec!["path_presence".to_string()],
                probable_impact: RiskLevel::Medium,
                crash_risk: RiskLevel::Medium,
                suspected_mapping_change: true,
                lineage: crate::compare::AssetLineageKind::LayoutDrift,
                reasons: vec![SnapshotCompareReason {
                    code: "asset_removed".to_string(),
                    message: "old path removed".to_string(),
                }],
            }],
            changed_assets: Vec::new(),
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Content/Character/Encore/Cloak.mesh"),
                new_asset: asset_summary("Content/Character/Encore/Cloak_v2.mesh"),
                confidence: 0.93,
                compatibility: RemapCompatibility::CompatibleWithCaution,
                lineage: crate::compare::AssetLineageKind::RenameOrRepath,
                reasons: vec![
                    SnapshotCompareReason {
                        code: "normalized_name_exact".to_string(),
                        message: "same logical asset".to_string(),
                    },
                    SnapshotCompareReason {
                        code: "same_parent_directory".to_string(),
                        message: "same folder".to_string(),
                    },
                ],
                runner_up_confidence: None,
                confidence_gap: Some(0.18),
                ambiguous: false,
            }],
        }
    }

    fn continuity_mapping_hint<'a>(
        report: &'a crate::inference::InferenceReport,
    ) -> &'a crate::inference::InferredMappingHint {
        report
            .candidate_mapping_hints
            .iter()
            .find(|hint| {
                hint.old_asset_path.contains("Body") || hint.old_asset_path.contains("Cloak")
            })
            .expect("mapping hint")
    }

    fn continuity_observation(
        from_version_id: &str,
        to_version_id: &str,
        from_path: &str,
        to_path: Option<&str>,
        relation: VersionContinuityRelation,
    ) -> VersionContinuityObservation {
        VersionContinuityObservation {
            from_version_id: from_version_id.to_string(),
            to_version_id: to_version_id.to_string(),
            from_path: from_path.to_string(),
            to_path: to_path.map(ToOwned::to_owned),
            relation,
            status: DiffStatus::Changed,
            confidence: Some(0.90),
            compatibility: None,
            source: VersionContinuitySource::CandidateMapping,
            reason_codes: Vec::new(),
        }
    }

    fn continuity_item(path: &str) -> VersionedItem {
        VersionedItem {
            key: path.to_string(),
            label: path.to_string(),
            path: Some(path.to_string()),
            metadata: TechnicalMetadata::default(),
        }
    }

    fn continuity_test_knowledge() -> WwmiKnowledgeBase {
        WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: 1,
            repo: WwmiKnowledgeRepoInfo {
                input: "repo".to_string(),
                resolved_path: "repo".to_string(),
                origin_url: None,
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: 2,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            patterns: vec![
                WwmiFixPattern {
                    kind: WwmiPatternKind::MappingOrHashUpdate,
                    description: "mapping".to_string(),
                    frequency: 2,
                    average_fix_likelihood: 0.80,
                    example_commits: vec!["abc".to_string()],
                },
                WwmiFixPattern {
                    kind: WwmiPatternKind::BufferLayoutOrCapacityFix,
                    description: "buffer".to_string(),
                    frequency: 2,
                    average_fix_likelihood: 0.85,
                    example_commits: vec!["def".to_string()],
                },
            ],
            keyword_stats: Vec::new(),
            evidence_commits: Vec::new(),
        }
    }
}
