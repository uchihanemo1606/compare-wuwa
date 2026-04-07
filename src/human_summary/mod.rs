use crate::{
    compare::RiskLevel,
    inference::{InferenceReport, ProbableCrashCause, SuggestedFix},
    proposal::{MappingProposalEntry, ProposalArtifacts, ProposalStatus},
};

#[derive(Debug, Default, Clone, Copy)]
pub struct HumanSummaryRenderer;

impl HumanSummaryRenderer {
    pub fn render(&self, inference: &InferenceReport, proposals: &ProposalArtifacts) -> String {
        let mut lines = Vec::new();
        let proposed = proposals
            .mapping_proposal
            .mappings
            .iter()
            .filter(|entry| entry.status == ProposalStatus::Proposed)
            .collect::<Vec<_>>();
        let needs_review = proposals
            .mapping_proposal
            .mappings
            .iter()
            .filter(|entry| entry.status == ProposalStatus::NeedsReview)
            .collect::<Vec<_>>();
        let mut sorted_causes = inference.probable_crash_causes.iter().collect::<Vec<_>>();
        sorted_causes.sort_by(|left, right| {
            review_risk_rank(&right.risk)
                .cmp(&review_risk_rank(&left.risk))
                .then_with(|| right.confidence.total_cmp(&left.confidence))
                .then_with(|| left.code.cmp(&right.code))
        });
        let mut sorted_fixes = inference.suggested_fixes.iter().collect::<Vec<_>>();
        sorted_fixes.sort_by(|left, right| {
            review_risk_rank(&right.priority)
                .cmp(&review_risk_rank(&left.priority))
                .then_with(|| right.confidence.total_cmp(&left.confidence))
                .then_with(|| left.code.cmp(&right.code))
        });
        let mut sorted_needs_review = needs_review.clone();
        sorted_needs_review.sort_by(|left, right| {
            mapping_review_rank(right)
                .cmp(&mapping_review_rank(left))
                .then_with(|| right.confidence.total_cmp(&left.confidence))
                .then_with(|| left.old_asset_path.cmp(&right.old_asset_path))
        });

        lines.push("# WhashReonator Summary".to_string());
        lines.push(String::new());
        lines.push(format!(
            "- Versions: `{}` -> `{}`",
            inference.compare_input.old_version_id, inference.compare_input.new_version_id
        ));
        lines.push(format!(
            "- WWMI knowledge: repo `{}`, analyzed_commits={}, discovered_patterns={}",
            inference.knowledge_input.repo,
            inference.knowledge_input.analyzed_commits,
            inference.knowledge_input.discovered_patterns
        ));
        lines.push(format!(
            "- Highest confidence: {:.3}",
            proposals.mapping_proposal.summary.highest_confidence
        ));
        lines.push(format!(
            "- Mapping proposals: proposed={}, needs_review={}, total_candidates={}",
            proposals.mapping_proposal.summary.proposed_mappings,
            proposals.mapping_proposal.summary.needs_review_mappings,
            proposals.mapping_proposal.summary.total_mapping_candidates
        ));
        lines.push(String::new());

        lines.push("## Fix Before Remap".to_string());
        if inference.probable_crash_causes.is_empty()
            && inference.suggested_fixes.is_empty()
            && needs_review.is_empty()
        {
            lines.push("- No blocking issue was inferred before remapping.".to_string());
        } else {
            lines.push("### Likely Crash Causes".to_string());
            if sorted_causes.is_empty() {
                lines.push(
                    "- No strong crash cause was inferred from the current inputs.".to_string(),
                );
            } else {
                for cause in sorted_causes.into_iter().take(5) {
                    lines.extend(render_cause(cause));
                }
            }

            lines.push(String::new());
            lines.push("### Suggested Fixes".to_string());
            if sorted_fixes.is_empty() {
                lines.push("- No concrete fix suggestions were inferred yet.".to_string());
            } else {
                for fix in sorted_fixes.into_iter().take(5) {
                    lines.extend(render_fix(fix));
                }
            }

            lines.push(String::new());
            lines.push("### Needs Review".to_string());
            if sorted_needs_review.is_empty() {
                lines.push("- No mapping candidate is currently blocked for review.".to_string());
            } else {
                for entry in sorted_needs_review.into_iter().take(5) {
                    lines.extend(render_mapping(entry));
                }
            }
        }
        lines.push(String::new());

        lines.push("## Safe To Try Now".to_string());
        if proposed.is_empty() {
            lines.push(
                "- No mapping candidate is strong enough to propose automatically.".to_string(),
            );
        } else {
            lines.push("### Proposed Mappings".to_string());
            for entry in proposed.iter().take(5) {
                lines.extend(render_mapping(entry));
            }
        }
        lines.push(String::new());

        lines.push("## Next Steps".to_string());
        lines.push(
            "- Resolve `Fix Before Remap` items before treating any mapping as stable.".to_string(),
        );
        lines.push("- Validate `Safe To Try Now` mappings against real mod/runtime behavior before applying them broadly.".to_string());
        lines.push("- Prioritize `needs_review` entries with structural drift or ambiguous runner-up evidence.".to_string());
        lines.push(
            "- Re-scan snapshots when richer game metadata or hashes become available.".to_string(),
        );

        lines.join("\n")
    }
}

fn render_cause(cause: &ProbableCrashCause) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "- `{}` [{:?}, {:.3}] {}",
        cause.code, cause.risk, cause.confidence, cause.summary
    ));
    if !cause.affected_assets.is_empty() {
        lines.push(format!(
            "  Affected assets: {}",
            cause
                .affected_assets
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(reason) = cause.reasons.first() {
        lines.push(format!("  Why: {reason}"));
    }
    lines
}

fn render_fix(fix: &SuggestedFix) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "- `{}` [{:?}, {:.3}] {}",
        fix.code, fix.priority, fix.confidence, fix.summary
    ));
    if let Some(action) = fix.actions.first() {
        lines.push(format!("  First action: {action}"));
    }
    if let Some(reason) = fix.reasons.first() {
        lines.push(format!("  Why: {reason}"));
    }
    lines
}

fn render_mapping(entry: &MappingProposalEntry) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "- `{}` -> `{}` [{} {:.3}]",
        entry.old_asset_path,
        entry.new_asset_path,
        status_label(&entry.status),
        entry.confidence
    ));
    if let Some(reason) = entry.reasons.first() {
        lines.push(format!("  Reason: {reason}"));
    }
    if let Some(evidence) = entry.evidence.first() {
        lines.push(format!("  Evidence: {evidence}"));
    }
    lines
}

fn status_label(status: &ProposalStatus) -> &'static str {
    match status {
        ProposalStatus::Proposed => "proposed",
        ProposalStatus::NeedsReview => "needs_review",
    }
}

fn review_risk_rank(risk: &RiskLevel) -> u8 {
    match risk {
        RiskLevel::High => 3,
        RiskLevel::Medium => 2,
        RiskLevel::Low => 1,
    }
}

fn mapping_review_rank(entry: &MappingProposalEntry) -> u8 {
    if entry
        .reasons
        .iter()
        .any(|reason| reason.contains("high-risk structural drift"))
        || entry
            .evidence
            .iter()
            .any(|evidence| evidence.contains("blocking crash cause"))
    {
        3
    } else if entry
        .reasons
        .iter()
        .any(|reason| reason.contains("ambiguous"))
        || entry
            .evidence
            .iter()
            .any(|evidence| evidence.contains("confidence gap"))
    {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compare::RiskLevel,
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceSummary,
            InferredMappingHint, ProbableCrashCause, SuggestedFix,
        },
        proposal::{ProposalArtifacts, ProposalEngine},
        wwmi::WwmiPatternKind,
    };

    use super::HumanSummaryRenderer;

    #[test]
    fn renderer_outputs_human_readable_sections() {
        let inference = InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "2.4.0".to_string(),
                new_version_id: "2.5.0".to_string(),
                changed_assets: 1,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 1,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 10,
                fix_like_commits: 6,
                discovered_patterns: 3,
            },
            summary: InferenceSummary {
                probable_crash_causes: 2,
                suggested_fixes: 2,
                candidate_mapping_hints: 3,
                highest_confidence: 0.91,
            },
            probable_crash_causes: vec![
                ProbableCrashCause {
                    code: "asset_removed_without_clear_replacement".to_string(),
                    summary: "replacement uncertain".to_string(),
                    confidence: 0.72,
                    risk: RiskLevel::Medium,
                    affected_assets: vec!["Content/Weapon/Old.weapon".to_string()],
                    related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                    reasons: vec!["removed path has weak replacements".to_string()],
                    evidence: vec!["WWMI mapping fix history".to_string()],
                },
                ProbableCrashCause {
                    code: "buffer_layout_changed".to_string(),
                    summary: "mesh layout changed".to_string(),
                    confidence: 0.91,
                    risk: RiskLevel::High,
                    affected_assets: vec!["Content/Character/Encore/Body.mesh".to_string()],
                    related_patterns: vec![WwmiPatternKind::BufferLayoutOrCapacityFix],
                    reasons: vec!["vertex count changed".to_string()],
                    evidence: vec!["WWMI buffer fix history".to_string()],
                },
            ],
            suggested_fixes: vec![
                SuggestedFix {
                    code: "review_runtime_init_path".to_string(),
                    summary: "review runtime init".to_string(),
                    confidence: 0.44,
                    priority: RiskLevel::Low,
                    related_patterns: vec![WwmiPatternKind::StartupTimingAdjustment],
                    actions: vec!["check init ordering".to_string()],
                    reasons: vec!["timing signal detected".to_string()],
                    evidence: vec!["WWMI timing fix history".to_string()],
                },
                SuggestedFix {
                    code: "review_candidate_asset_remaps".to_string(),
                    summary: "review remap candidates".to_string(),
                    confidence: 0.88,
                    priority: RiskLevel::High,
                    related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                    actions: vec!["inspect mapping hints".to_string()],
                    reasons: vec!["asset path drift detected".to_string()],
                    evidence: vec!["WWMI mapping fix history".to_string()],
                },
            ],
            candidate_mapping_hints: vec![
                InferredMappingHint {
                    old_asset_path: "Content/Character/Encore/Hair.mesh".to_string(),
                    new_asset_path: "Content/Character/Encore/Hair_LOD0.mesh".to_string(),
                    confidence: 0.89,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: Some(0.20),
                    reasons: vec!["same_parent_directory: same folder".to_string()],
                    evidence: vec!["compare candidate confidence 0.890".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                    new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                    confidence: 0.93,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: Some(0.15),
                    reasons: vec!["same_parent_directory: same folder".to_string()],
                    evidence: vec!["compare candidate confidence 0.930".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Weapon/Old.weapon".to_string(),
                    new_asset_path: "Content/Weapon/New.weapon".to_string(),
                    confidence: 0.61,
                    needs_review: true,
                    ambiguous: true,
                    confidence_gap: Some(0.03),
                    reasons: vec!["ambiguous runner-up remap candidate".to_string()],
                    evidence: vec![
                        "mapping hint marked ambiguous with confidence gap 0.030".to_string(),
                    ],
                },
            ],
        };
        let proposals: ProposalArtifacts = ProposalEngine.generate(&inference, 0.85);

        let markdown = HumanSummaryRenderer.render(&inference, &proposals);

        assert!(markdown.contains("# WhashReonator Summary"));
        assert!(markdown.contains("## Fix Before Remap"));
        assert!(markdown.contains("### Likely Crash Causes"));
        assert!(markdown.contains("### Suggested Fixes"));
        assert!(markdown.contains("## Safe To Try Now"));
        assert!(markdown.contains("### Proposed Mappings"));
        assert!(markdown.contains("Hair.mesh"));
        assert!(
            markdown.find("buffer_layout_changed")
                < markdown.find("asset_removed_without_clear_replacement")
        );
        assert!(
            markdown.find("review_candidate_asset_remaps")
                < markdown.find("review_runtime_init_path")
        );
        assert!(
            markdown.find("Content/Character/Encore/Body.mesh")
                < markdown.find("Content/Weapon/Old.weapon")
        );
    }
}
