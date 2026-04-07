use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    compare::{
        CandidateMappingChange, RiskLevel, SnapshotAssetChange, SnapshotChangeType,
        SnapshotCompareReason, SnapshotCompareReport, load_snapshot_compare_report,
    },
    error::{AppError, AppResult},
    wwmi::{WwmiKnowledgeBase, WwmiPatternKind, load_wwmi_knowledge},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferenceReport {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub compare_input: InferenceCompareInput,
    pub knowledge_input: InferenceKnowledgeInput,
    pub summary: InferenceSummary,
    pub probable_crash_causes: Vec<ProbableCrashCause>,
    pub suggested_fixes: Vec<SuggestedFix>,
    pub candidate_mapping_hints: Vec<InferredMappingHint>,
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
    pub needs_review: bool,
    #[serde(default)]
    pub ambiguous: bool,
    #[serde(default)]
    pub confidence_gap: Option<f32>,
    pub reasons: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FixInferenceEngine;

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
                .clamp(0.0, 1.0);
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
                    "Compare vertex_count/index_count/material_slots/section_count against the previous version.".to_string(),
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
        if !removed_assets.is_empty() && !compare_report.candidate_mapping_changes.is_empty() {
            let confidence =
                (0.55 + 0.25 * mapping_support + 0.10 * shader_support + 0.05).clamp(0.0, 1.0);
            probable_crash_causes.push(ProbableCrashCause {
                code: "asset_paths_or_mapping_shifted".to_string(),
                summary: "Assets disappeared from their old paths but plausible replacements exist in the new snapshot; the mod likely needs remapping.".to_string(),
                confidence,
                risk: RiskLevel::High,
                affected_assets: compare_report
                    .candidate_mapping_changes
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
                        compare_report.candidate_mapping_changes.len()
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
            let confidence = (0.45 + 0.20 * mapping_support).clamp(0.0, 1.0);
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

        let name_or_hash_changes = compare_report
            .changed_assets
            .iter()
            .filter(|change| {
                change.changed_fields.iter().any(|field| {
                    matches!(
                        field.as_str(),
                        "normalized_name" | "logical_name" | "asset_hash" | "shader_hash" | "kind"
                    )
                })
            })
            .collect::<Vec<_>>();
        if !name_or_hash_changes.is_empty() {
            let confidence =
                (0.45 + 0.20 * mapping_support + 0.10 * shader_support).clamp(0.0, 1.0);
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
                confidence: (0.25 + 0.25 * timing_support).clamp(0.0, 1.0),
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
                confidence: (0.25 + 0.25 * timing_support).clamp(0.0, 1.0),
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
            .map(|candidate| infer_mapping_hint(candidate, mapping_support))
            .collect::<Vec<_>>();
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
            summary: InferenceSummary {
                probable_crash_causes: probable_crash_causes.len(),
                suggested_fixes: suggested_fixes.len(),
                candidate_mapping_hints: candidate_mapping_hints.len(),
                highest_confidence,
            },
            probable_crash_causes,
            suggested_fixes,
            candidate_mapping_hints,
        }
    }
}

pub fn load_inference_report(path: &Path) -> AppResult<InferenceReport> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn is_structural_change(change: &SnapshotAssetChange) -> bool {
    change.changed_fields.iter().any(|field| {
        matches!(
            field.as_str(),
            "vertex_count"
                | "index_count"
                | "material_slots"
                | "section_count"
                | "asset_hash"
                | "shader_hash"
        )
    })
}

fn infer_mapping_hint(
    candidate: &CandidateMappingChange,
    mapping_support: f32,
) -> InferredMappingHint {
    let mut confidence = (candidate.confidence * 0.70 + mapping_support * 0.30).clamp(0.0, 1.0);
    if candidate.ambiguous {
        confidence = (confidence - 0.12).clamp(0.0, 1.0);
    }

    let mut reasons = candidate
        .reasons
        .iter()
        .map(reason_to_string)
        .collect::<Vec<_>>();
    if candidate.ambiguous {
        reasons.push(
            "compare detected a near-tie runner-up candidate; confidence was penalized to keep this mapping under review".to_string(),
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

    InferredMappingHint {
        old_asset_path: candidate.old_asset.path.clone(),
        new_asset_path: candidate.new_asset.path.clone(),
        confidence,
        needs_review: true,
        ambiguous: candidate.ambiguous,
        confidence_gap: candidate.confidence_gap,
        reasons,
        evidence,
    }
}

fn reason_to_string(reason: &SnapshotCompareReason) -> String {
    format!("{}: {}", reason.code, reason.message)
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
            CandidateMappingChange, RiskLevel, SnapshotAssetChange, SnapshotAssetSummary,
            SnapshotChangeType, SnapshotCompareReason, SnapshotCompareReport,
            SnapshotCompareSummary, SnapshotVersionInfo,
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
            summary: SnapshotCompareSummary {
                total_old_assets: 2,
                total_new_assets: 2,
                unchanged_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                changed_assets: 1,
                candidate_mapping_changes: 1,
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
                reasons: vec![SnapshotCompareReason {
                    code: "vertex_count_changed".to_string(),
                    message: "vertex count changed".to_string(),
                }],
            }],
            candidate_mapping_changes: vec![CandidateMappingChange {
                old_asset: asset_summary("Content/Weapon/Sword.weapon"),
                new_asset: asset_summary("Content/Weapon/Sword_v2.weapon"),
                confidence: 0.82,
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
        assert!(report.summary.highest_confidence >= 0.75);
    }

    fn asset_summary(path: &str) -> SnapshotAssetSummary {
        SnapshotAssetSummary {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            normalized_name: Some("asset".to_string()),
            vertex_count: Some(1000),
            index_count: Some(2000),
            material_slots: Some(1),
            section_count: Some(1),
            asset_hash: None,
            shader_hash: None,
        }
    }
}
