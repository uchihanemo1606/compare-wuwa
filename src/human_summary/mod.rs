use crate::{
    compare::{RemapCompatibility, RiskLevel},
    inference::{InferenceReport, ProbableCrashCause, SuggestedFix},
    proposal::{MappingProposalEntry, ProposalArtifacts, ProposalStatus},
    report::{VersionDiffReportV2, VersionReviewCause, VersionReviewFix, VersionReviewMapping},
};

#[derive(Debug, Default, Clone, Copy)]
pub struct HumanSummaryRenderer;

#[derive(Debug, Default, Clone, Copy)]
pub struct ReviewBundleRenderer;

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
        let continuity_causes = continuity_causes(inference);
        let continuity_fixes = continuity_fixes(inference);
        let continuity_review_mappings = needs_review
            .iter()
            .filter(|entry| mapping_has_continuity_caution(entry))
            .count();

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
        if let Some(mod_dependency) = inference.mod_dependency_input.as_ref() {
            lines.push(format!(
                "- Mod dependency profile: `{}` ini_files={} signals={} kinds={}",
                mod_dependency
                    .mod_name
                    .as_deref()
                    .unwrap_or(mod_dependency.mod_root.as_str()),
                mod_dependency.ini_file_count,
                mod_dependency.signal_count,
                mod_dependency
                    .dependency_kinds
                    .iter()
                    .map(mod_dependency_kind_label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !continuity_causes.is_empty()
            || !continuity_fixes.is_empty()
            || continuity_review_mappings > 0
        {
            lines.push(format!(
                "- Continuity-backed caution: causes={}, fixes={}, review_only_mappings={}",
                continuity_causes.len(),
                continuity_fixes.len(),
                continuity_review_mappings
            ));
        }
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

            if !continuity_causes.is_empty()
                || !continuity_fixes.is_empty()
                || continuity_review_mappings > 0
            {
                lines.push(String::new());
                lines.push("### Continuity Caution".to_string());
                lines.push(format!(
                    "- Broader continuity history is keeping {} mapping candidate(s) in review-first state.",
                    continuity_review_mappings
                ));
                if let Some(cause) = continuity_causes.first() {
                    lines.push(format!(
                        "- `{}` [{:?}, {:.3}] {}",
                        cause.code, cause.risk, cause.confidence, cause.summary
                    ));
                }
                if let Some(fix) = continuity_fixes.first() {
                    lines.push(format!(
                        "- `{}` [{:?}, {:.3}] {}",
                        fix.code, fix.priority, fix.confidence, fix.summary
                    ));
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
        if !continuity_causes.is_empty()
            || !continuity_fixes.is_empty()
            || continuity_review_mappings > 0
        {
            lines.push(
                "- Review broader continuity thread history before trusting remap-only repair on continuity-flagged mappings."
                    .to_string(),
            );
        }
        lines.push(
            "- Re-scan snapshots when richer game metadata or hashes become available.".to_string(),
        );

        lines.join("\n")
    }
}

impl ReviewBundleRenderer {
    pub fn render(&self, report: &VersionDiffReportV2) -> String {
        let mut lines = Vec::new();
        lines.push("# WhashReonator Review Bundle".to_string());
        lines.push(String::new());
        lines.push("| Baseline | Current | Continuity caution | Review-only mappings | Continuity-backed review mappings | Causes | Fixes |".to_string());
        lines.push("| --- | --- | --- | ---: | ---: | ---: | ---: |".to_string());
        lines.push(markdown_table_row(&[
            code_cell(&report.old_version.version_id),
            code_cell(&report.new_version.version_id),
            yes_no(report.review.summary.continuity_caution_present).to_string(),
            report.review.summary.review_mapping_count.to_string(),
            report
                .review
                .summary
                .continuity_review_mapping_count
                .to_string(),
            report.review.continuity.cause_count.to_string(),
            report.review.continuity.fix_count.to_string(),
        ]));
        lines.push(String::new());

        lines.push("## Continuity Summary".to_string());
        lines.push(format!(
            "- Continuity caution is {} for this saved version pair.",
            if report.review.summary.continuity_caution_present {
                "present"
            } else {
                "not present"
            }
        ));
        lines.push(format!(
            "- Review-only mappings: {} total, {} kept review-first by broader continuity history.",
            report.review.summary.review_mapping_count,
            report.review.summary.continuity_review_mapping_count
        ));
        if report.review.continuity.notes.is_empty() {
            lines.push("- No extra continuity notes were saved in the review surface.".to_string());
        } else {
            for note in report.review.continuity.notes.iter().take(3) {
                lines.push(format!("- {note}"));
            }
        }
        lines.push(String::new());

        lines.push("## Continuity-backed Causes".to_string());
        if report.review.continuity.causes.is_empty() {
            lines.push("No continuity-backed causes were saved in this bundle.".to_string());
        } else {
            lines.push("| Code | Confidence | Summary | Evidence |".to_string());
            lines.push("| --- | ---: | --- | --- |".to_string());
            for cause in &report.review.continuity.causes {
                lines.push(markdown_table_row(&[
                    code_cell(&cause.code),
                    format!("{:.3}", cause.confidence),
                    cause.summary.clone(),
                    cause_detail(cause),
                ]));
            }
        }
        lines.push(String::new());

        lines.push("## Continuity-backed Fixes".to_string());
        if report.review.continuity.fixes.is_empty() {
            lines.push("No continuity-backed fixes were saved in this bundle.".to_string());
        } else {
            lines.push("| Code | Confidence | Summary | First action |".to_string());
            lines.push("| --- | ---: | --- | --- |".to_string());
            for fix in &report.review.continuity.fixes {
                lines.push(markdown_table_row(&[
                    code_cell(&fix.code),
                    format!("{:.3}", fix.confidence),
                    fix.summary.clone(),
                    first_fix_action(fix),
                ]));
            }
        }
        lines.push(String::new());

        lines.push("## Continuity-backed Review Mappings".to_string());
        if report.review.continuity.mappings.is_empty() {
            lines.push(
                "No mapping stays in `NeedsReview` because of broader continuity history."
                    .to_string(),
            );
        } else {
            lines.push("| Old asset | New asset | Status | Confidence | Compatibility | Why kept review-only | Continuity note |".to_string());
            lines.push("| --- | --- | --- | ---: | --- | --- | --- |".to_string());
            for mapping in &report.review.continuity.mappings {
                lines.push(markdown_table_row(&[
                    code_cell(&mapping.old_asset_path),
                    code_cell(&mapping.new_asset_path),
                    status_label(&mapping.status).to_string(),
                    format!("{:.3}", mapping.confidence),
                    compatibility_label(&mapping.compatibility).to_string(),
                    mapping_review_reason(mapping),
                    mapping_continuity_note(mapping),
                ]));
            }
        }

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
    if let Some(reason) = mod_dependency_reason(&cause.reasons) {
        lines.push(format!("  Mod focus: {reason}"));
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
    if let Some(reason) = mod_dependency_reason(&fix.reasons) {
        lines.push(format!("  Mod focus: {reason}"));
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
    if let Some(reason) = continuity_reason(entry) {
        lines.push(format!("  Continuity: {reason}"));
    }
    if let Some(reason) = mod_dependency_reason(&entry.reasons) {
        lines.push(format!("  Mod focus: {reason}"));
    }
    if let Some(evidence) = entry.evidence.first() {
        lines.push(format!("  Evidence: {evidence}"));
    }
    if let Some(evidence) = continuity_evidence(entry) {
        lines.push(format!("  Continuity evidence: {evidence}"));
    }
    if let Some(evidence) = mod_dependency_evidence(&entry.evidence) {
        lines.push(format!("  Mod evidence: {evidence}"));
    }
    lines
}

fn mod_dependency_reason(reasons: &[String]) -> Option<String> {
    reasons
        .iter()
        .find_map(|reason| strip_mod_dependency_prefix(reason))
}

fn mod_dependency_evidence(evidence: &[String]) -> Option<String> {
    evidence
        .iter()
        .find(|item| {
            item.contains("mod dependency")
                || item.contains("mod-side")
                || item.contains("hook-targeting-sensitive")
                || item.contains("buffer/layout-sensitive")
                || item.contains("resource/skeleton-sensitive")
        })
        .cloned()
}

fn strip_mod_dependency_prefix(value: &str) -> Option<String> {
    value
        .strip_prefix("mod_dependency_surface: ")
        .or_else(|| value.strip_prefix("mod_dependency_review_first: "))
        .map(ToOwned::to_owned)
}

fn mod_dependency_kind_label(
    kind: &crate::wwmi::dependency::WwmiModDependencyKind,
) -> &'static str {
    match kind {
        crate::wwmi::dependency::WwmiModDependencyKind::ObjectGuid => "object_guid",
        crate::wwmi::dependency::WwmiModDependencyKind::DrawCallTarget => "draw_call_target",
        crate::wwmi::dependency::WwmiModDependencyKind::TextureOverrideHash => {
            "texture_override_hash"
        }
        crate::wwmi::dependency::WwmiModDependencyKind::ResourceFileReference => {
            "resource_file_reference"
        }
        crate::wwmi::dependency::WwmiModDependencyKind::MeshVertexCount => "mesh_vertex_count",
        crate::wwmi::dependency::WwmiModDependencyKind::ShapeKeyVertexCount => {
            "shapekey_vertex_count"
        }
        crate::wwmi::dependency::WwmiModDependencyKind::BufferLayoutHint => "buffer_layout_hint",
        crate::wwmi::dependency::WwmiModDependencyKind::SkeletonMergeDependency => {
            "skeleton_merge_dependency"
        }
        crate::wwmi::dependency::WwmiModDependencyKind::FilterIndex => "filter_index",
    }
}

fn markdown_table_row(cells: &[String]) -> String {
    format!(
        "| {} |",
        cells
            .iter()
            .map(|value| sanitize_markdown_table_cell(value))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

fn sanitize_markdown_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn code_cell(value: &str) -> String {
    format!("`{value}`")
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

fn cause_detail(cause: &VersionReviewCause) -> String {
    cause
        .evidence
        .first()
        .cloned()
        .or_else(|| {
            cause
                .affected_assets
                .first()
                .map(|asset| format!("affected asset {asset}"))
        })
        .unwrap_or_else(|| "-".to_string())
}

fn first_fix_action(fix: &VersionReviewFix) -> String {
    fix.actions
        .first()
        .cloned()
        .unwrap_or_else(|| "-".to_string())
}

fn mapping_review_reason(mapping: &VersionReviewMapping) -> String {
    mapping
        .reasons
        .first()
        .cloned()
        .or_else(|| mapping.evidence.first().cloned())
        .or_else(|| {
            mapping
                .related_fix_codes
                .first()
                .map(|code| format!("related fix `{code}`"))
        })
        .unwrap_or_else(|| "saved as review-first without an extra note".to_string())
}

fn mapping_continuity_note(mapping: &VersionReviewMapping) -> String {
    if mapping.continuity_notes.is_empty() {
        return "-".to_string();
    }

    let mut selected = Vec::<String>::new();
    if let Some(first_note) = mapping.continuity_notes.first() {
        selected.push(first_note.clone());
    }

    let priority_note = mapping
        .continuity_notes
        .iter()
        .find(|note| note.contains("terminal"))
        .or_else(|| {
            mapping
                .continuity_notes
                .iter()
                .find(|note| note.contains("review-required"))
        })
        .or_else(|| {
            mapping
                .continuity_notes
                .iter()
                .find(|note| note.contains("layout drift"))
        })
        .or_else(|| {
            mapping
                .continuity_notes
                .iter()
                .find(|note| note.contains("container movement"))
        });

    if let Some(priority_note) = priority_note
        && !selected.contains(priority_note)
    {
        selected.push(priority_note.clone());
    }

    for note in &mapping.continuity_notes {
        if selected.len() >= 2 {
            break;
        }
        if !selected.contains(note) {
            selected.push(note.clone());
        }
    }

    selected.join("; ")
}

fn status_label(status: &ProposalStatus) -> &'static str {
    match status {
        ProposalStatus::Proposed => "proposed",
        ProposalStatus::NeedsReview => "needs_review",
    }
}

fn compatibility_label(compatibility: &RemapCompatibility) -> &'static str {
    match compatibility {
        RemapCompatibility::LikelyCompatible => "likely_compatible",
        RemapCompatibility::CompatibleWithCaution => "compatible_with_caution",
        RemapCompatibility::StructurallyRisky => "structurally_risky",
        RemapCompatibility::IncompatibleBlocked => "incompatible_blocked",
        RemapCompatibility::InsufficientEvidence => "insufficient_evidence",
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
    if mod_dependency_reason(&entry.reasons).is_some() {
        5
    } else if mapping_has_continuity_caution(entry) {
        4
    } else if entry
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

fn continuity_causes<'a>(inference: &'a InferenceReport) -> Vec<&'a ProbableCrashCause> {
    inference
        .probable_crash_causes
        .iter()
        .filter(|cause| cause.code == "continuity_thread_instability")
        .collect()
}

fn continuity_fixes<'a>(inference: &'a InferenceReport) -> Vec<&'a SuggestedFix> {
    inference
        .suggested_fixes
        .iter()
        .filter(|fix| fix.code == "review_continuity_thread_history_before_repair")
        .collect()
}

fn mapping_has_continuity_caution(entry: &MappingProposalEntry) -> bool {
    entry
        .continuity
        .as_ref()
        .is_some_and(|continuity| continuity.has_review_caution())
        || first_legacy_continuity_reason(entry).is_some()
        || first_legacy_continuity_evidence(entry).is_some()
}

fn continuity_reason(entry: &MappingProposalEntry) -> Option<String> {
    entry
        .continuity
        .as_ref()
        .and_then(structured_continuity_reason)
        .or_else(|| first_legacy_continuity_reason(entry).map(|value| value.to_string()))
}

fn continuity_evidence(entry: &MappingProposalEntry) -> Option<String> {
    entry
        .continuity
        .as_ref()
        .and_then(structured_continuity_evidence)
        .or_else(|| first_legacy_continuity_evidence(entry).map(|value| value.to_string()))
}

fn structured_continuity_reason(
    continuity: &crate::inference::InferredMappingContinuityContext,
) -> Option<String> {
    if continuity.total_layout_drift_steps >= 2 {
        Some(format!(
            "broader continuity history shows repeated layout drift across {} step(s); keep review-first",
            continuity.total_layout_drift_steps
        ))
    } else if let Some(relation) = continuity.terminal_relation.as_ref() {
        let timing = if continuity.terminal_after_current {
            "later reaches"
        } else {
            "reaches"
        };
        Some(format!(
            "broader continuity history {timing} terminal state {} in {}",
            continuity_relation_label(relation),
            continuity.terminal_version.as_deref().unwrap_or("unknown")
        ))
    } else if continuity.review_required_history {
        Some("broader continuity history already marked this thread review-required".to_string())
    } else {
        None
    }
}

fn structured_continuity_evidence(
    continuity: &crate::inference::InferredMappingContinuityContext,
) -> Option<String> {
    if !continuity.has_review_caution() {
        return None;
    }

    let mut details = Vec::new();
    if let (Some(first_seen), Some(latest_observed)) = (
        continuity.first_seen_version.as_deref(),
        continuity.latest_observed_version.as_deref(),
    ) {
        details.push(format!("thread span {first_seen} -> {latest_observed}"));
    }
    if continuity.total_layout_drift_steps >= 2 {
        details.push(format!(
            "layout drift steps={}",
            continuity.total_layout_drift_steps
        ));
    }
    if continuity.review_required_history {
        details.push("review-required elsewhere in the chain".to_string());
    }
    if let Some(relation) = continuity.terminal_relation.as_ref() {
        details.push(format!(
            "terminal {} in {}",
            continuity_relation_label(relation),
            continuity.terminal_version.as_deref().unwrap_or("unknown")
        ));
    }

    if details.is_empty() {
        None
    } else {
        Some(details.join("; "))
    }
}

fn first_legacy_continuity_reason(entry: &MappingProposalEntry) -> Option<&str> {
    entry
        .reasons
        .iter()
        .find(|reason| is_legacy_continuity_text(reason))
        .map(|value| value.as_str())
}

fn first_legacy_continuity_evidence(entry: &MappingProposalEntry) -> Option<&str> {
    entry
        .evidence
        .iter()
        .find(|evidence| is_legacy_continuity_text(evidence))
        .map(|value| value.as_str())
}

fn is_legacy_continuity_text(value: &str) -> bool {
    value.contains("continuity")
        || value.contains("terminal state")
        || value.contains("review-required")
        || value.contains("repeated layout drift")
}

fn continuity_relation_label(relation: &crate::report::VersionContinuityRelation) -> &'static str {
    match relation {
        crate::report::VersionContinuityRelation::Persisted => "persisted",
        crate::report::VersionContinuityRelation::RenameOrRepath => "rename/repath",
        crate::report::VersionContinuityRelation::ContainerMovement => "container movement",
        crate::report::VersionContinuityRelation::LayoutDrift => "layout drift",
        crate::report::VersionContinuityRelation::Replacement => "replacement",
        crate::report::VersionContinuityRelation::Ambiguous => "ambiguous",
        crate::report::VersionContinuityRelation::InsufficientEvidence => "insufficient evidence",
        crate::report::VersionContinuityRelation::Removed => "removed",
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compare::RemapCompatibility,
        compare::RiskLevel,
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceScopeContext,
            InferenceSummary, InferredMappingContinuityContext, InferredMappingHint,
            ProbableCrashCause, SuggestedFix,
        },
        proposal::{ProposalArtifacts, ProposalEngine, ProposalStatus},
        report::{
            VersionContinuityRelation, VersionContinuityReviewSection, VersionDiffReportV2,
            VersionDiffSummary, VersionLineageSection, VersionReviewCause, VersionReviewFix,
            VersionReviewMapping, VersionReviewSection, VersionReviewSummary, VersionSide,
        },
        wwmi::WwmiPatternKind,
    };

    use super::{HumanSummaryRenderer, ReviewBundleRenderer};

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
            mod_dependency_input: None,
            scope: InferenceScopeContext::default(),
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
                    compatibility: crate::compare::RemapCompatibility::CompatibleWithCaution,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: Some(0.20),
                    continuity: None,
                    reasons: vec!["same_parent_directory: same folder".to_string()],
                    evidence: vec!["compare candidate confidence 0.890".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                    new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                    confidence: 0.93,
                    compatibility: crate::compare::RemapCompatibility::StructurallyRisky,
                    needs_review: true,
                    ambiguous: false,
                    confidence_gap: Some(0.15),
                    continuity: None,
                    reasons: vec!["same_parent_directory: same folder".to_string()],
                    evidence: vec!["compare candidate confidence 0.930".to_string()],
                },
                InferredMappingHint {
                    old_asset_path: "Content/Weapon/Old.weapon".to_string(),
                    new_asset_path: "Content/Weapon/New.weapon".to_string(),
                    confidence: 0.61,
                    compatibility: crate::compare::RemapCompatibility::InsufficientEvidence,
                    needs_review: true,
                    ambiguous: true,
                    confidence_gap: Some(0.03),
                    continuity: None,
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

    #[test]
    fn renderer_surfaces_continuity_backed_caution() {
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
                analyzed_commits: 4,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            mod_dependency_input: None,
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 1,
                suggested_fixes: 1,
                candidate_mapping_hints: 1,
                highest_confidence: 0.88,
            },
            probable_crash_causes: vec![ProbableCrashCause {
                code: "continuity_thread_instability".to_string(),
                summary: "broader continuity history is unstable".to_string(),
                confidence: 0.83,
                risk: RiskLevel::High,
                affected_assets: vec!["Content/Character/Encore/Body.mesh".to_string()],
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                reasons: vec!["continuity thread later degrades".to_string()],
                evidence: vec![
                    "continuity thread Content/Character/Encore/Body_v2.mesh spans 7.0.0 -> 8.2.0"
                        .to_string(),
                ],
            }],
            suggested_fixes: vec![SuggestedFix {
                code: "review_continuity_thread_history_before_repair".to_string(),
                summary: "review broader continuity history".to_string(),
                confidence: 0.81,
                priority: RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec!["inspect continuity milestones".to_string()],
                reasons: vec!["unstable thread history".to_string()],
                evidence: vec![
                    "continuity thread later terminates as removed in 8.2.0".to_string(),
                ],
            }],
            candidate_mapping_hints: vec![InferredMappingHint {
                old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                confidence: 0.92,
                compatibility: crate::compare::RemapCompatibility::CompatibleWithCaution,
                needs_review: true,
                ambiguous: false,
                confidence_gap: Some(0.18),
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
                reasons: vec!["same_parent_directory: same folder".to_string()],
                evidence: vec!["compare candidate confidence 0.920".to_string()],
            }],
        };
        let proposals: ProposalArtifacts = ProposalEngine.generate(&inference, 0.85);

        let markdown = HumanSummaryRenderer.render(&inference, &proposals);

        assert!(markdown.contains("Continuity-backed caution"));
        assert!(markdown.contains("### Continuity Caution"));
        assert!(markdown.contains("broader continuity history"));
        assert!(markdown.contains("Continuity:"));
        assert!(markdown.contains("Continuity evidence:"));
    }

    #[test]
    fn review_bundle_renderer_uses_saved_review_context() {
        let markdown = ReviewBundleRenderer.render(&sample_review_report(true));

        assert!(markdown.contains("# WhashReonator Review Bundle"));
        assert!(markdown.contains("| `8.0.0` | `8.1.0` | Yes | 1 | 1 | 1 | 1 |"));
        assert!(markdown.contains("## Continuity-backed Causes"));
        assert!(markdown.contains("continuity_thread_instability"));
        assert!(markdown.contains("## Continuity-backed Fixes"));
        assert!(markdown.contains("review_continuity_thread_history_before_repair"));
        assert!(markdown.contains("## Continuity-backed Review Mappings"));
        assert!(markdown.contains("Content/Character/Encore/Body.mesh"));
        assert!(markdown.contains("thread span 7.0.0 -> 8.2.0"));
        assert!(markdown.contains("later terminal removed in 8.2.0"));
    }

    #[test]
    fn review_bundle_renderer_stays_sensible_without_continuity_caution() {
        let markdown = ReviewBundleRenderer.render(&sample_review_report(false));

        assert!(markdown.contains("| `3.2.1` | `3.3.1` | No | 0 | 0 | 0 | 0 |"));
        assert!(markdown.contains("Continuity caution is not present"));
        assert!(markdown.contains("No continuity-backed causes were saved in this bundle."));
        assert!(markdown.contains("No continuity-backed fixes were saved in this bundle."));
        assert!(
            markdown.contains(
                "No mapping stays in `NeedsReview` because of broader continuity history."
            )
        );
    }

    fn sample_review_report(with_caution: bool) -> VersionDiffReportV2 {
        VersionDiffReportV2 {
            schema_version: "whashreonator.report.v2".to_string(),
            generated_at_unix_ms: 1,
            old_version: VersionSide {
                version_id: if with_caution {
                    "8.0.0".to_string()
                } else {
                    "3.2.1".to_string()
                },
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_version: VersionSide {
                version_id: if with_caution {
                    "8.1.0".to_string()
                } else {
                    "3.3.1".to_string()
                },
                source_root: "new".to_string(),
                asset_count: 1,
            },
            resonators: Vec::new(),
            lineage: VersionLineageSection::default(),
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
            review: if with_caution {
                VersionReviewSection {
                    summary: VersionReviewSummary {
                        review_mapping_count: 1,
                        continuity_review_mapping_count: 1,
                        continuity_caution_present: true,
                    },
                    continuity: VersionContinuityReviewSection {
                        caution_present: true,
                        cause_count: 1,
                        fix_count: 1,
                        review_mapping_count: 1,
                        causes: vec![VersionReviewCause {
                            code: "continuity_thread_instability".to_string(),
                            summary: "broader continuity history is unstable".to_string(),
                            confidence: 0.84,
                            affected_assets: vec![
                                "Content/Character/Encore/Body.mesh".to_string(),
                            ],
                            evidence: vec![
                                "continuity thread Content/Character/Encore/Body_v2.mesh spans 7.0.0 -> 8.2.0; later terminates as removed in 8.2.0".to_string(),
                            ],
                        }],
                        fixes: vec![VersionReviewFix {
                            code: "review_continuity_thread_history_before_repair".to_string(),
                            summary: "review broader continuity history".to_string(),
                            confidence: 0.82,
                            actions: vec!["inspect continuity milestones".to_string()],
                            evidence: vec![
                                "continuity thread later terminates as removed in 8.2.0"
                                    .to_string(),
                            ],
                        }],
                        mappings: vec![VersionReviewMapping {
                            old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                            new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                            status: ProposalStatus::NeedsReview,
                            confidence: 0.95,
                            compatibility: RemapCompatibility::CompatibleWithCaution,
                            continuity: None,
                            continuity_notes: vec![
                                "thread span 7.0.0 -> 8.2.0".to_string(),
                                "later terminal removed in 8.2.0".to_string(),
                            ],
                            reasons: vec![
                                "broader continuity history reaches a later terminal state for this thread; do not auto-promote this mapping".to_string(),
                            ],
                            evidence: vec![
                                "structured continuity history reaches terminal state removed in 8.2.0".to_string(),
                            ],
                            related_fix_codes: vec![
                                "review_continuity_thread_history_before_repair".to_string(),
                            ],
                        }],
                        notes: vec![
                            "continuity-backed caution present: causes=1 fixes=1 review_mappings=1"
                                .to_string(),
                            "mapping Content/Character/Encore/Body.mesh -> Content/Character/Encore/Body_v2.mesh stays review-first because broader continuity history is unstable".to_string(),
                        ],
                    },
                }
            } else {
                VersionReviewSection::default()
            },
        }
    }
}
