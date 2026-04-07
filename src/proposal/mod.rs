use std::{
    collections::BTreeSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    compare::RiskLevel,
    error::{AppError, AppResult},
    inference::{
        InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, load_inference_report,
    },
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
        let remap_fix_available = report
            .suggested_fixes
            .iter()
            .any(|fix| fix.code == "review_candidate_asset_remaps");
        let layout_fix_available = report
            .suggested_fixes
            .iter()
            .any(|fix| fix.code == "review_buffer_layout_and_runtime_guards");

        let mut mappings = report
            .candidate_mapping_hints
            .iter()
            .map(|hint| {
                let blocked = blocked_paths.contains(&hint.old_asset_path)
                    || blocked_paths.contains(&hint.new_asset_path);
                let status = if !blocked && !hint.ambiguous && hint.confidence >= min_confidence {
                    ProposalStatus::Proposed
                } else {
                    ProposalStatus::NeedsReview
                };

                let mut reasons = hint.reasons.clone();
                if blocked {
                    reasons.push(
                        "high-risk structural drift affects one of the mapped assets; keep this mapping in review state".to_string(),
                    );
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

                let mut evidence = hint.evidence.clone();
                if blocked {
                    evidence.extend(blocking_evidence(report, &hint.old_asset_path, &hint.new_asset_path));
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

                let mut related_fix_codes = Vec::new();
                if remap_fix_available {
                    related_fix_codes.push("review_candidate_asset_remaps".to_string());
                }
                if blocked && layout_fix_available {
                    related_fix_codes.push("review_buffer_layout_and_runtime_guards".to_string());
                }

                MappingProposalEntry {
                    old_asset_path: hint.old_asset_path.clone(),
                    new_asset_path: hint.new_asset_path.clone(),
                    confidence: hint.confidence,
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
                    "buffer_layout_changed" | "asset_signature_or_hash_changed"
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

fn current_unix_ms() -> AppResult<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::InvalidInput(format!("system clock error: {error}")))?
        .as_millis())
}

#[cfg(test)]
mod tests {
    use crate::{
        compare::RiskLevel,
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceSummary,
            InferredMappingHint, ProbableCrashCause, SuggestedFix,
        },
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
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: None,
                    reasons: vec!["normalized_name_exact: same logical asset".to_string()],
                    evidence: vec!["compare candidate confidence 0.930".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Character/HeroA/Body.mesh".to_string(),
                    new_asset_path: "Content/Character/HeroA/Body_v2.mesh".to_string(),
                    confidence: 0.95,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: None,
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
}
