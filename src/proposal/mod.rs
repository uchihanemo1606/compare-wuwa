use std::{
    collections::BTreeSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    compare::{RemapCompatibility, RiskLevel},
    error::{AppError, AppResult},
    inference::{
        InferenceCompareInput, InferenceKnowledgeInput, InferenceReport,
        InferredMappingContinuityContext, InferredMappingHint, ProbableCrashCause,
        load_inference_report,
    },
    report::VersionContinuityRelation,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Proposed,
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposalSummary {
    pub total_mapping_candidates: usize,
    pub proposed_mappings: usize,
    pub needs_review_mappings: usize,
    pub suggested_fix_actions: usize,
    pub highest_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MappingProposalOutput {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub source_schema_version: String,
    pub compare_input: InferenceCompareInput,
    pub knowledge_input: InferenceKnowledgeInput,
    pub min_confidence: f32,
    pub summary: ProposalSummary,
    pub mappings: Vec<MappingProposalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MappingProposalEntry {
    pub old_asset_path: String,
    pub new_asset_path: String,
    pub confidence: f32,
    #[serde(default)]
    pub compatibility: RemapCompatibility,
    #[serde(default)]
    pub continuity: Option<InferredMappingContinuityContext>,
    pub status: ProposalStatus,
    pub reasons: Vec<String>,
    pub evidence: Vec<String>,
    pub related_fix_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposalPatchDraftOutput {
    pub schema_version: String,
    pub mode: String,
    pub generated_at_unix_ms: u128,
    pub source_schema_version: String,
    pub compare_input: InferenceCompareInput,
    pub knowledge_input: InferenceKnowledgeInput,
    pub min_confidence: f32,
    pub summary: ProposalSummary,
    pub actions: Vec<ProposalPatchDraftAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposalPatchDraftAction {
    pub action: String,
    pub target: String,
    pub old_asset_path: Option<String>,
    pub new_asset_path: Option<String>,
    pub confidence: f32,
    pub status: ProposalStatus,
    pub notes: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposalArtifacts {
    pub mapping_proposal: MappingProposalOutput,
    pub patch_draft: ProposalPatchDraftOutput,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProposalEngine;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ContinuityProposalCaution {
    instability_cause: bool,
    repeated_layout_drift: bool,
    terminal_history: bool,
    review_required_history: bool,
}

impl ContinuityProposalCaution {
    fn blocks_auto_proposal(&self) -> bool {
        self.instability_cause
            || self.repeated_layout_drift
            || self.terminal_history
            || self.review_required_history
    }
}

impl ProposalEngine {
    pub fn generate_files(
        &self,
        inference_report_path: &Path,
        min_confidence: f32,
    ) -> AppResult<ProposalArtifacts> {
        validate_min_confidence(min_confidence)?;
        let report = load_inference_report(inference_report_path)?;
        Ok(self.generate(&report, min_confidence))
    }

    pub fn generate(&self, report: &InferenceReport, min_confidence: f32) -> ProposalArtifacts {
        let generated_at_unix_ms = current_unix_ms().unwrap_or(report.generated_at_unix_ms);
        let blocked_paths = structurally_blocked_paths(report);
        let low_signal_compare = report.scope.low_signal_compare;
        let remap_fix_available = report
            .suggested_fixes
            .iter()
            .any(|fix| fix.code == "review_candidate_asset_remaps");
        let continuity_fix_available = report
            .suggested_fixes
            .iter()
            .any(|fix| fix.code == "review_continuity_thread_history_before_repair");
        let layout_fix_available = report.suggested_fixes.iter().any(|fix| {
            matches!(
                fix.code.as_str(),
                "review_buffer_layout_and_runtime_guards"
                    | "validate_candidate_remaps_against_layout"
            )
        });

        let mut mappings = report
            .candidate_mapping_hints
            .iter()
            .map(|hint| {
                let blocked = blocked_paths.contains(&hint.old_asset_path)
                    || blocked_paths.contains(&hint.new_asset_path);
                let continuity_caution = continuity_proposal_caution(report, hint);
                let strong_low_signal_justification =
                    has_strong_low_signal_justification(hint, min_confidence, blocked);
                let compatibility_allows_proposal = hint.compatibility.supports_auto_proposal();
                let status = if compatibility_allows_proposal
                    && !blocked
                    && !hint.ambiguous
                    && !continuity_caution.blocks_auto_proposal()
                    && hint.confidence >= min_confidence
                    && (!low_signal_compare || strong_low_signal_justification)
                {
                    ProposalStatus::Proposed
                } else {
                    ProposalStatus::NeedsReview
                };

                let mut reasons = hint.reasons.clone();
                if !compatibility_allows_proposal {
                    reasons.push(format!(
                        "compatibility assessment is {}; keep this mapping in review state",
                        compatibility_label(&hint.compatibility)
                    ));
                } else if blocked {
                    reasons.push(
                        "high-risk structural drift affects one of the mapped assets; keep this mapping in review state".to_string(),
                    );
                } else if continuity_caution.blocks_auto_proposal() {
                    reasons.extend(continuity_caution_reasons(&continuity_caution));
                } else if hint.ambiguous {
                    reasons.push(
                        "multiple remap candidates remain too close after compare scoring; keep this mapping in review state".to_string(),
                    );
                } else if hint.confidence >= min_confidence {
                    reasons.push(format!(
                        "confidence {:.3} meets proposal threshold {:.3} and no blocking structural drift was inferred",
                        hint.confidence, min_confidence
                    ));
                } else {
                    reasons.push(format!(
                        "confidence {:.3} is below proposal threshold {:.3}; keep this mapping in review state",
                        hint.confidence, min_confidence
                    ));
                }
                if low_signal_compare {
                    if status == ProposalStatus::NeedsReview {
                        reasons.push(
                            "compare scope is low-signal; defaulting this mapping to NeedsReview unless evidence is exceptionally strong"
                                .to_string(),
                        );
                    } else {
                        reasons.push(
                            "compare scope is low-signal, but this mapping passed elevated strong-evidence checks"
                                .to_string(),
                        );
                    }
                }

                let mut evidence = hint.evidence.clone();
                if blocked {
                    evidence.extend(blocking_evidence(report, &hint.old_asset_path, &hint.new_asset_path));
                }
                if continuity_caution.blocks_auto_proposal() {
                    evidence.extend(continuity_caution_evidence(report, hint));
                }
                if hint.ambiguous {
                    evidence.push(format!(
                        "mapping hint marked ambiguous with confidence gap {}",
                        hint
                            .confidence_gap
                            .map(|gap| format!("{gap:.3}"))
                            .unwrap_or_else(|| "unknown".to_string())
                    ));
                }
                if low_signal_compare {
                    evidence.extend(
                        report
                            .scope
                            .notes
                            .iter()
                            .take(3)
                            .map(|note| format!("scope context: {note}")),
                    );
                }

                let mut related_fix_codes = Vec::new();
                if remap_fix_available {
                    related_fix_codes.push("review_candidate_asset_remaps".to_string());
                }
                if continuity_fix_available && continuity_caution.blocks_auto_proposal() {
                    related_fix_codes.push("review_continuity_thread_history_before_repair".to_string());
                }
                if blocked && layout_fix_available {
                    related_fix_codes.push("review_buffer_layout_and_runtime_guards".to_string());
                }

                MappingProposalEntry {
                    old_asset_path: hint.old_asset_path.clone(),
                    new_asset_path: hint.new_asset_path.clone(),
                    confidence: hint.confidence,
                    compatibility: hint.compatibility.clone(),
                    continuity: hint.continuity.clone(),
                    status,
                    reasons,
                    evidence,
                    related_fix_codes,
                }
            })
            .collect::<Vec<_>>();
        mappings.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.old_asset_path.cmp(&right.old_asset_path))
        });

        let proposed_mappings = mappings
            .iter()
            .filter(|entry| entry.status == ProposalStatus::Proposed)
            .count();
        let needs_review_mappings = mappings.len().saturating_sub(proposed_mappings);
        let summary = ProposalSummary {
            total_mapping_candidates: mappings.len(),
            proposed_mappings,
            needs_review_mappings,
            suggested_fix_actions: report.suggested_fixes.len(),
            highest_confidence: report.summary.highest_confidence,
        };

        let mut actions = mappings
            .iter()
            .map(|mapping| ProposalPatchDraftAction {
                action: match mapping.status {
                    ProposalStatus::Proposed => "propose_mapping".to_string(),
                    ProposalStatus::NeedsReview => "review_mapping".to_string(),
                },
                target: format!("{} -> {}", mapping.old_asset_path, mapping.new_asset_path),
                old_asset_path: Some(mapping.old_asset_path.clone()),
                new_asset_path: Some(mapping.new_asset_path.clone()),
                confidence: mapping.confidence,
                status: mapping.status.clone(),
                notes: mapping.reasons.clone(),
                evidence: mapping.evidence.clone(),
            })
            .collect::<Vec<_>>();

        actions.extend(report.suggested_fixes.iter().map(|fix| {
            ProposalPatchDraftAction {
                action: "review_fix".to_string(),
                target: fix.code.clone(),
                old_asset_path: None,
                new_asset_path: None,
                confidence: fix.confidence,
                status: ProposalStatus::NeedsReview,
                notes: std::iter::once(fix.summary.clone())
                    .chain(fix.actions.iter().cloned())
                    .chain(fix.reasons.iter().cloned())
                    .collect(),
                evidence: fix.evidence.clone(),
            }
        }));
        actions.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.target.cmp(&right.target))
        });

        ProposalArtifacts {
            mapping_proposal: MappingProposalOutput {
                schema_version: "whashreonator.mapping-proposal.v1".to_string(),
                generated_at_unix_ms,
                source_schema_version: report.schema_version.clone(),
                compare_input: report.compare_input.clone(),
                knowledge_input: report.knowledge_input.clone(),
                min_confidence,
                summary: summary.clone(),
                mappings,
            },
            patch_draft: ProposalPatchDraftOutput {
                schema_version: "whashreonator.proposal-patch-draft.v1".to_string(),
                mode: "draft".to_string(),
                generated_at_unix_ms,
                source_schema_version: report.schema_version.clone(),
                compare_input: report.compare_input.clone(),
                knowledge_input: report.knowledge_input.clone(),
                min_confidence,
                summary,
                actions,
            },
        }
    }
}

fn validate_min_confidence(min_confidence: f32) -> AppResult<()> {
    if !(0.0..=1.0).contains(&min_confidence) {
        return Err(AppError::InvalidInput(format!(
            "min_confidence must be between 0.0 and 1.0, got {min_confidence}"
        )));
    }

    Ok(())
}

fn structurally_blocked_paths(report: &InferenceReport) -> BTreeSet<String> {
    report
        .probable_crash_causes
        .iter()
        .filter(|cause| {
            cause.risk == RiskLevel::High
                && matches!(
                    cause.code.as_str(),
                    "buffer_layout_changed"
                        | "asset_signature_or_hash_changed"
                        | "candidate_remap_structural_drift"
                        | "candidate_remap_identity_conflict"
                )
        })
        .flat_map(|cause| cause.affected_assets.iter().cloned())
        .collect()
}

fn blocking_evidence(
    report: &InferenceReport,
    old_asset_path: &str,
    new_asset_path: &str,
) -> Vec<String> {
    report
        .probable_crash_causes
        .iter()
        .filter(|cause| {
            cause.risk == RiskLevel::High
                && cause
                    .affected_assets
                    .iter()
                    .any(|asset| asset == old_asset_path || asset == new_asset_path)
        })
        .flat_map(|cause| {
            std::iter::once(format!(
                "blocking crash cause {} with confidence {:.3}: {}",
                cause.code, cause.confidence, cause.summary
            ))
            .chain(cause.evidence.iter().cloned())
        })
        .collect()
}

fn continuity_proposal_caution(
    report: &InferenceReport,
    hint: &InferredMappingHint,
) -> ContinuityProposalCaution {
    let instability_cause = report.probable_crash_causes.iter().any(|cause| {
        cause.code == "continuity_thread_instability" && cause_matches_hint(cause, hint)
    });
    if let Some(continuity) = hint.continuity.as_ref() {
        return ContinuityProposalCaution {
            instability_cause,
            repeated_layout_drift: continuity.total_layout_drift_steps >= 2,
            terminal_history: continuity.terminal_after_current
                || continuity.terminal_relation.is_some(),
            review_required_history: continuity.review_required_history,
        };
    }

    let legacy = legacy_continuity_proposal_caution(hint);
    ContinuityProposalCaution {
        instability_cause,
        repeated_layout_drift: legacy.repeated_layout_drift,
        terminal_history: legacy.terminal_history,
        review_required_history: legacy.review_required_history,
    }
}

fn legacy_continuity_proposal_caution(hint: &InferredMappingHint) -> ContinuityProposalCaution {
    let repeated_layout_drift = hint
        .reasons
        .iter()
        .chain(hint.evidence.iter())
        .any(|item| item.contains("repeated layout drift"));
    let terminal_history = hint.reasons.iter().chain(hint.evidence.iter()).any(|item| {
        item.contains("terminal state")
            || item.contains("later marks this thread as")
            || item.contains("later terminates")
    });
    let review_required_history = hint
        .reasons
        .iter()
        .chain(hint.evidence.iter())
        .any(|item| item.contains("review-required"));

    ContinuityProposalCaution {
        instability_cause: false,
        repeated_layout_drift,
        terminal_history,
        review_required_history,
    }
}

fn continuity_caution_reasons(caution: &ContinuityProposalCaution) -> Vec<String> {
    let mut reasons = Vec::new();

    if caution.instability_cause {
        reasons.push(
            "continuity-backed inference flagged broader thread instability for this mapping; keep it in review state"
                .to_string(),
        );
    }
    if caution.repeated_layout_drift {
        reasons.push(
            "broader continuity history shows repeated layout drift on this asset thread; keep this mapping in review state"
                .to_string(),
        );
    }
    if caution.terminal_history {
        reasons.push(
            "broader continuity history reaches a later terminal state for this thread; do not auto-promote this mapping"
                .to_string(),
        );
    }
    if caution.review_required_history {
        reasons.push(
            "broader continuity history already marked this thread review-required; keep review-first behavior"
                .to_string(),
        );
    }

    reasons
}

fn continuity_caution_evidence(
    report: &InferenceReport,
    hint: &InferredMappingHint,
) -> Vec<String> {
    let mut evidence = BTreeSet::<String>::new();

    if let Some(continuity) = hint.continuity.as_ref() {
        if let (Some(first_seen), Some(latest_observed)) = (
            continuity.first_seen_version.as_deref(),
            continuity.latest_observed_version.as_deref(),
        ) {
            evidence.insert(format!(
                "structured continuity thread span: {} -> {}",
                first_seen, latest_observed
            ));
        }
        if continuity.total_layout_drift_steps >= 2 {
            evidence.insert(format!(
                "structured continuity history records {} layout-drift step(s)",
                continuity.total_layout_drift_steps
            ));
        }
        if continuity.review_required_history {
            evidence.insert(
                "structured continuity history already marked this thread review-required"
                    .to_string(),
            );
        }
        if let Some(relation) = continuity.terminal_relation.as_ref() {
            evidence.insert(format!(
                "structured continuity history reaches terminal state {} in {}",
                continuity_relation_label(relation),
                continuity.terminal_version.as_deref().unwrap_or("unknown")
            ));
        }
    }

    for cause in report.probable_crash_causes.iter().filter(|cause| {
        cause.code == "continuity_thread_instability" && cause_matches_hint(cause, hint)
    }) {
        evidence.insert(format!(
            "continuity-backed crash cause {} with confidence {:.3}: {}",
            cause.code, cause.confidence, cause.summary
        ));
        for item in cause.evidence.iter().take(3) {
            evidence.insert(item.clone());
        }
    }

    if continuity_proposal_caution(report, hint).blocks_auto_proposal() {
        for fix in report
            .suggested_fixes
            .iter()
            .filter(|fix| fix.code == "review_continuity_thread_history_before_repair")
        {
            evidence.insert(format!(
                "continuity-backed fix {} with confidence {:.3}: {}",
                fix.code, fix.confidence, fix.summary
            ));
            for item in fix.evidence.iter().take(2) {
                evidence.insert(item.clone());
            }
        }
    }

    evidence.into_iter().collect()
}

fn cause_matches_hint(cause: &ProbableCrashCause, hint: &InferredMappingHint) -> bool {
    cause
        .affected_assets
        .iter()
        .any(|asset| asset == &hint.old_asset_path || asset == &hint.new_asset_path)
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

fn has_strong_low_signal_justification(
    hint: &crate::inference::InferredMappingHint,
    min_confidence: f32,
    blocked: bool,
) -> bool {
    if blocked || hint.ambiguous {
        return false;
    }

    if hint.compatibility != RemapCompatibility::LikelyCompatible {
        return false;
    }

    let has_path_and_name_anchor = has_reason_code(&hint.reasons, "normalized_name_exact")
        && has_reason_code(&hint.reasons, "same_parent_directory");
    let has_identity_anchor = has_reason_code(&hint.reasons, "signature_exact")
        || has_reason_code(&hint.reasons, "asset_hash_exact");
    let has_structural_compatibility =
        has_reason_code(&hint.reasons, "structural_layout_compatible")
            || has_reason_code(&hint.reasons, "buffer_layout_compatible");
    let has_conflict = has_reason_code(&hint.reasons, "identity_conflict_detected")
        || has_reason_code(&hint.reasons, "same_asset_but_structural_drift")
        || has_reason_code(&hint.reasons, "buffer_layout_validation_needed")
        || has_reason_code(&hint.reasons, "weak_identity_evidence");

    if has_conflict {
        return false;
    }

    let strong_confidence_threshold = if has_path_and_name_anchor && has_structural_compatibility {
        (min_confidence + 0.02).max(0.88).clamp(0.0, 1.0)
    } else {
        (min_confidence + 0.03).max(0.90).clamp(0.0, 1.0)
    };
    if hint.confidence < strong_confidence_threshold {
        return false;
    }

    if hint.confidence_gap.is_some_and(|gap| gap < 0.12) {
        return false;
    }

    (has_path_and_name_anchor && has_structural_compatibility) || has_identity_anchor
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

fn has_reason_code(reasons: &[String], code: &str) -> bool {
    let prefix = format!("{code}:");
    reasons
        .iter()
        .any(|reason| reason.trim() == code || reason.trim_start().starts_with(&prefix))
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
        compare::{RemapCompatibility, RiskLevel},
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceScopeContext,
            InferenceSummary, InferredMappingContinuityContext, InferredMappingHint,
            ProbableCrashCause, SuggestedFix,
        },
        report::VersionContinuityRelation,
        wwmi::WwmiPatternKind,
    };

    use super::{ProposalEngine, ProposalStatus};

    #[test]
    fn proposal_engine_only_promotes_non_blocked_high_confidence_mappings() {
        let report = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "2.4.0".to_string(),
                new_version_id: "2.5.0".to_string(),
                changed_assets: 1,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 2,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 10,
                fix_like_commits: 5,
                discovered_patterns: 3,
            },
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 1,
                suggested_fixes: 2,
                candidate_mapping_hints: 2,
                highest_confidence: 0.93,
            },
            probable_crash_causes: vec![ProbableCrashCause {
                code: "buffer_layout_changed".to_string(),
                summary: "body mesh layout changed".to_string(),
                confidence: 0.91,
                risk: RiskLevel::High,
                affected_assets: vec!["Content/Character/HeroA/Body.mesh".to_string()],
                related_patterns: vec![WwmiPatternKind::BufferLayoutOrCapacityFix],
                reasons: vec!["vertex count changed".to_string()],
                evidence: vec!["WWMI buffer fix history".to_string()],
            }],
            suggested_fixes: vec![
                SuggestedFix {
                    code: "review_candidate_asset_remaps".to_string(),
                    summary: "review mapping hints".to_string(),
                    confidence: 0.88,
                    priority: RiskLevel::High,
                    related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                    actions: vec!["inspect mapping".to_string()],
                    reasons: vec!["mapping drift detected".to_string()],
                    evidence: vec!["WWMI mapping fixes".to_string()],
                },
                SuggestedFix {
                    code: "review_buffer_layout_and_runtime_guards".to_string(),
                    summary: "review layout guards".to_string(),
                    confidence: 0.90,
                    priority: RiskLevel::High,
                    related_patterns: vec![WwmiPatternKind::BufferLayoutOrCapacityFix],
                    actions: vec!["inspect layout".to_string()],
                    reasons: vec!["structural drift detected".to_string()],
                    evidence: vec!["WWMI buffer fixes".to_string()],
                },
            ],
            candidate_mapping_hints: vec![
                InferredMappingHint {
                    old_asset_path: "Content/Weapon/Sword.weapon".to_string(),
                    new_asset_path: "Content/Weapon/Sword_v2.weapon".to_string(),
                    confidence: 0.93,
                    compatibility: RemapCompatibility::CompatibleWithCaution,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: None,
                    continuity: None,
                    reasons: vec!["normalized_name_exact: same logical asset".to_string()],
                    evidence: vec!["compare candidate confidence 0.930".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Character/HeroA/Body.mesh".to_string(),
                    new_asset_path: "Content/Character/HeroA/Body_v2.mesh".to_string(),
                    confidence: 0.95,
                    compatibility: RemapCompatibility::StructurallyRisky,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: None,
                    continuity: None,
                    reasons: vec!["same_parent_directory: same folder".to_string()],
                    evidence: vec!["compare candidate confidence 0.950".to_string()],
                },
            ],
        };

        let artifacts = ProposalEngine.generate(&report, 0.90);

        assert_eq!(artifacts.mapping_proposal.summary.proposed_mappings, 1);
        assert_eq!(artifacts.mapping_proposal.summary.needs_review_mappings, 1);
        assert_eq!(
            artifacts.mapping_proposal.mappings[0].status,
            ProposalStatus::NeedsReview
        );
        assert_eq!(
            artifacts.mapping_proposal.mappings[1].status,
            ProposalStatus::Proposed
        );
        assert!(
            artifacts
                .patch_draft
                .actions
                .iter()
                .any(|action| action.action == "propose_mapping")
        );
        assert!(
            artifacts
                .patch_draft
                .actions
                .iter()
                .any(|action| action.action == "review_fix")
        );
    }

    #[test]
    fn proposal_engine_defaults_low_signal_hints_to_review_unless_strong() {
        let report = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "3.0.0".to_string(),
                new_version_id: "3.1.0".to_string(),
                changed_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 2,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 3,
                fix_like_commits: 1,
                discovered_patterns: 1,
            },
            scope: InferenceScopeContext {
                low_signal_compare: true,
                old_snapshot_low_signal: true,
                new_snapshot_low_signal: true,
                notes: vec!["install/package-level scope".to_string()],
            },
            summary: InferenceSummary {
                probable_crash_causes: 0,
                suggested_fixes: 1,
                candidate_mapping_hints: 2,
                highest_confidence: 0.96,
            },
            probable_crash_causes: Vec::new(),
            suggested_fixes: vec![SuggestedFix {
                code: "review_candidate_asset_remaps".to_string(),
                summary: "review remaps".to_string(),
                confidence: 0.70,
                priority: RiskLevel::Medium,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec!["inspect hints".to_string()],
                reasons: vec!["low-signal scope".to_string()],
                evidence: vec!["scope context".to_string()],
            }],
            candidate_mapping_hints: vec![
                InferredMappingHint {
                    old_asset_path: "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak".to_string(),
                    new_asset_path: "Client/Content/Paks/pakchunk1-WindowsNoEditor.pak".to_string(),
                    confidence: 0.93,
                    compatibility: RemapCompatibility::InsufficientEvidence,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: Some(0.20),
                    continuity: None,
                    reasons: vec![
                        "same_parent_directory: same folder".to_string(),
                        "path_token_overlap: overlap".to_string(),
                    ],
                    evidence: vec!["compare candidate confidence 0.930".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Character/Encore/Hair.mesh".to_string(),
                    new_asset_path: "Content/Character/Encore/Hair_LOD0.mesh".to_string(),
                    confidence: 0.96,
                    compatibility: RemapCompatibility::LikelyCompatible,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: Some(0.25),
                    continuity: None,
                    reasons: vec![
                        "normalized_name_exact: encore hair".to_string(),
                        "same_parent_directory: same folder".to_string(),
                        "structural_layout_compatible: vertex_count, index_count, material_slots, section_count".to_string(),
                    ],
                    evidence: vec!["compare candidate confidence 0.960".to_string()],
                },
            ],
        };

        let artifacts = ProposalEngine.generate(&report, 0.90);

        let install_mapping = artifacts
            .mapping_proposal
            .mappings
            .iter()
            .find(|entry| entry.old_asset_path.contains("pakchunk0"))
            .expect("install mapping");
        assert_eq!(install_mapping.status, ProposalStatus::NeedsReview);
        assert!(
            install_mapping
                .reasons
                .iter()
                .any(|reason| reason.contains("low-signal"))
        );

        let strong_mapping = artifacts
            .mapping_proposal
            .mappings
            .iter()
            .find(|entry| entry.old_asset_path.contains("Hair.mesh"))
            .expect("strong mapping");
        assert_eq!(strong_mapping.status, ProposalStatus::Proposed);
    }

    #[test]
    fn proposal_engine_keeps_structurally_drifted_remaps_in_review() {
        let report = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "6.0.0".to_string(),
                new_version_id: "6.1.0".to_string(),
                changed_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 1,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 2,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 1,
                suggested_fixes: 1,
                candidate_mapping_hints: 1,
                highest_confidence: 0.94,
            },
            probable_crash_causes: vec![ProbableCrashCause {
                code: "candidate_remap_structural_drift".to_string(),
                summary: "candidate remap has structural drift".to_string(),
                confidence: 0.94,
                risk: RiskLevel::High,
                affected_assets: vec![
                    "Content/Character/Encore/Body.mesh".to_string(),
                    "Content/Character/Encore/Body_v2.mesh".to_string(),
                ],
                related_patterns: vec![WwmiPatternKind::BufferLayoutOrCapacityFix],
                reasons: vec!["structural drift detected".to_string()],
                evidence: vec!["compare found structural drift".to_string()],
            }],
            suggested_fixes: vec![SuggestedFix {
                code: "validate_candidate_remaps_against_layout".to_string(),
                summary: "validate remap against layout".to_string(),
                confidence: 0.93,
                priority: RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::BufferLayoutOrCapacityFix],
                actions: vec!["compare layout".to_string()],
                reasons: vec!["remap is not layout-safe yet".to_string()],
                evidence: vec!["compare found drift".to_string()],
            }],
            candidate_mapping_hints: vec![InferredMappingHint {
                old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                confidence: 0.94,
                compatibility: RemapCompatibility::StructurallyRisky,
                needs_review: true,
                ambiguous: false,
                confidence_gap: Some(0.25),
                continuity: None,
                reasons: vec![
                    "signature_exact: exact identity anchor".to_string(),
                    "same_asset_but_structural_drift: structure changed".to_string(),
                ],
                evidence: vec!["compare candidate confidence 0.940".to_string()],
            }],
        };

        let artifacts = ProposalEngine.generate(&report, 0.90);

        assert_eq!(artifacts.mapping_proposal.summary.proposed_mappings, 0);
        assert_eq!(
            artifacts.mapping_proposal.mappings[0].status,
            ProposalStatus::NeedsReview
        );
        assert!(
            artifacts.mapping_proposal.mappings[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("review"))
        );
    }

    #[test]
    fn proposal_engine_uses_compatibility_not_confidence_alone() {
        let report = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "7.0.0".to_string(),
                new_version_id: "7.1.0".to_string(),
                changed_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 2,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 2,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 0,
                suggested_fixes: 0,
                candidate_mapping_hints: 2,
                highest_confidence: 0.97,
            },
            probable_crash_causes: Vec::new(),
            suggested_fixes: Vec::new(),
            candidate_mapping_hints: vec![
                InferredMappingHint {
                    old_asset_path: "Content/Character/Encore/Hair.mesh".to_string(),
                    new_asset_path: "Content/Character/Encore/Hair_LOD0.mesh".to_string(),
                    confidence: 0.97,
                    compatibility: RemapCompatibility::LikelyCompatible,
                    needs_review: false,
                    ambiguous: false,
                    confidence_gap: Some(0.20),
                    continuity: None,
                    reasons: vec![
                        "normalized_name_exact: encore hair".to_string(),
                        "same_parent_directory: same folder".to_string(),
                        "structural_layout_compatible: vertex_count".to_string(),
                    ],
                    evidence: vec!["compare candidate confidence 0.970".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Character/Encore/Face.mesh".to_string(),
                    new_asset_path: "Content/Character/Encore/Face_LOD0.mesh".to_string(),
                    confidence: 0.97,
                    compatibility: RemapCompatibility::InsufficientEvidence,
                    needs_review: false,
                    ambiguous: false,
                    confidence_gap: Some(0.20),
                    continuity: None,
                    reasons: vec![
                        "weak_identity_evidence: weak".to_string(),
                        "path_token_overlap: overlap".to_string(),
                    ],
                    evidence: vec!["compare candidate confidence 0.970".to_string()],
                },
            ],
        };

        let artifacts = ProposalEngine.generate(&report, 0.90);
        let compatible = artifacts
            .mapping_proposal
            .mappings
            .iter()
            .find(|entry| entry.old_asset_path.ends_with("Hair.mesh"))
            .expect("compatible mapping");
        let insufficient = artifacts
            .mapping_proposal
            .mappings
            .iter()
            .find(|entry| entry.old_asset_path.ends_with("Face.mesh"))
            .expect("insufficient mapping");

        assert_eq!(compatible.status, ProposalStatus::Proposed);
        assert_eq!(
            compatible.compatibility,
            RemapCompatibility::LikelyCompatible
        );
        assert_eq!(insufficient.status, ProposalStatus::NeedsReview);
        assert_eq!(
            insufficient.compatibility,
            RemapCompatibility::InsufficientEvidence
        );
    }

    #[test]
    fn proposal_engine_keeps_continuity_unstable_mapping_in_review() {
        let report = InferenceReport {
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
                analyzed_commits: 3,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 1,
                suggested_fixes: 1,
                candidate_mapping_hints: 1,
                highest_confidence: 0.95,
            },
            probable_crash_causes: vec![ProbableCrashCause {
                code: "continuity_thread_instability".to_string(),
                summary: "broader thread history is unstable".to_string(),
                confidence: 0.84,
                risk: RiskLevel::High,
                affected_assets: vec!["Content/Character/Encore/Body.mesh".to_string()],
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                reasons: vec!["continuity surfaces unstable thread history".to_string()],
                evidence: vec!["continuity thread Content/Character/Encore/Body_v2.mesh spans 7.0.0 -> 8.2.0; later terminates as removed in 8.2.0".to_string()],
            }],
            suggested_fixes: vec![SuggestedFix {
                code: "review_continuity_thread_history_before_repair".to_string(),
                summary: "review continuity history".to_string(),
                confidence: 0.82,
                priority: RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec!["inspect continuity milestones".to_string()],
                reasons: vec!["broader thread history is unstable".to_string()],
                evidence: vec!["continuity thread later terminates as removed in 8.2.0".to_string()],
            }],
            candidate_mapping_hints: vec![InferredMappingHint {
                old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                confidence: 0.95,
                compatibility: RemapCompatibility::CompatibleWithCaution,
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
                    "normalized_name_exact: encore body".to_string(),
                    "same_parent_directory: same folder".to_string(),
                ],
                evidence: vec!["compare candidate confidence 0.950".to_string()],
            }],
        };

        let artifacts = ProposalEngine.generate(&report, 0.90);
        let mapping = artifacts
            .mapping_proposal
            .mappings
            .first()
            .expect("mapping exists");

        assert_eq!(mapping.status, ProposalStatus::NeedsReview);
        assert!(
            mapping
                .reasons
                .iter()
                .any(|reason| reason.contains("terminal state"))
        );
        assert!(
            mapping
                .evidence
                .iter()
                .any(|item| item.contains("continuity-backed crash cause"))
        );
        assert!(
            mapping
                .related_fix_codes
                .iter()
                .any(|code| code == "review_continuity_thread_history_before_repair")
        );
    }

    #[test]
    fn proposal_engine_does_not_penalize_stable_continuity_context_by_itself() {
        let report = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "9.0.0".to_string(),
                new_version_id: "9.1.0".to_string(),
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
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 0,
                suggested_fixes: 0,
                candidate_mapping_hints: 1,
                highest_confidence: 0.94,
            },
            probable_crash_causes: Vec::new(),
            suggested_fixes: Vec::new(),
            candidate_mapping_hints: vec![InferredMappingHint {
                old_asset_path: "Content/Character/Encore/Hair.mesh".to_string(),
                new_asset_path: "Content/Character/Encore/Hair_LOD0.mesh".to_string(),
                confidence: 0.94,
                compatibility: RemapCompatibility::LikelyCompatible,
                needs_review: true,
                ambiguous: false,
                confidence_gap: Some(0.24),
                continuity: Some(InferredMappingContinuityContext {
                    thread_id: Some("encore_hair".to_string()),
                    first_seen_version: Some("8.0.0".to_string()),
                    latest_observed_version: Some("9.1.0".to_string()),
                    latest_live_version: Some("9.1.0".to_string()),
                    stable_before_current_change: true,
                    total_rename_steps: 1,
                    total_container_movement_steps: 0,
                    total_layout_drift_steps: 0,
                    review_required_history: false,
                    terminal_relation: None,
                    terminal_version: None,
                    terminal_after_current: false,
                    instability_detected: false,
                }),
                reasons: vec![
                    "normalized_name_exact: encore hair".to_string(),
                    "same_parent_directory: same folder".to_string(),
                ],
                evidence: vec!["compare candidate confidence 0.940".to_string()],
            }],
        };

        let artifacts = ProposalEngine.generate(&report, 0.90);
        let mapping = artifacts
            .mapping_proposal
            .mappings
            .first()
            .expect("mapping exists");

        assert_eq!(mapping.status, ProposalStatus::Proposed);
    }
}
