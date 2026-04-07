use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::GenerateProposalsArgs,
    compare::RiskLevel,
    inference::{
        InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceSummary,
        InferredMappingHint, ProbableCrashCause, SuggestedFix,
    },
    pipeline::run_generate_proposals_command,
    proposal::{MappingProposalOutput, ProposalPatchDraftOutput, ProposalStatus},
    wwmi::WwmiPatternKind,
};

#[test]
fn generate_proposals_command_exports_mapping_and_patch_draft() {
    let test_root = unique_test_dir();
    let inference_path = test_root.join("tmp").join("inference.json");
    let mapping_output_path = test_root.join("out").join("mapping-proposal.json");
    let patch_output_path = test_root.join("out").join("proposal-patch-draft.json");
    let summary_output_path = test_root.join("out").join("summary.md");

    fs::create_dir_all(test_root.join("tmp")).expect("create temp input root");
    fs::write(
        &inference_path,
        serde_json::to_string_pretty(&sample_inference_report()).expect("serialize inference"),
    )
    .expect("write inference report");

    let result = run_generate_proposals_command(&GenerateProposalsArgs {
        inference_report: inference_path,
        mapping_output: Some(mapping_output_path.clone()),
        patch_draft_output: Some(patch_output_path.clone()),
        summary_output: Some(summary_output_path.clone()),
        min_confidence: 0.90,
    })
    .expect("run generate proposals command");

    let mapping_output =
        fs::read_to_string(&mapping_output_path).expect("read mapping proposal output");
    let patch_output =
        fs::read_to_string(&patch_output_path).expect("read proposal patch draft output");
    let summary_output = fs::read_to_string(&summary_output_path).expect("read summary output");

    let parsed_mapping: MappingProposalOutput =
        serde_json::from_str(&mapping_output).expect("parse mapping proposal output");
    let parsed_patch: ProposalPatchDraftOutput =
        serde_json::from_str(&patch_output).expect("parse proposal patch draft output");

    assert_eq!(
        result.artifacts.mapping_proposal.schema_version,
        "whashreonator.mapping-proposal.v1"
    );
    assert_eq!(
        parsed_patch.schema_version,
        "whashreonator.proposal-patch-draft.v1"
    );
    assert_eq!(parsed_mapping.summary.proposed_mappings, 1);
    assert_eq!(parsed_mapping.summary.needs_review_mappings, 1);
    assert!(parsed_mapping.mappings.iter().any(|entry| {
        entry.old_asset_path == "Content/Weapon/Sword.weapon"
            && entry.status == ProposalStatus::Proposed
    }));
    assert!(parsed_mapping.mappings.iter().any(|entry| {
        entry.old_asset_path == "Content/Character/HeroA/Body.mesh"
            && entry.status == ProposalStatus::NeedsReview
    }));
    assert!(
        parsed_patch
            .actions
            .iter()
            .any(|action| action.action == "propose_mapping")
    );
    assert!(
        parsed_patch
            .actions
            .iter()
            .any(|action| action.action == "review_fix")
    );
    assert!(summary_output.contains("# WhashReonator Summary"));
    assert!(summary_output.contains("## Fix Before Remap"));
    assert!(summary_output.contains("## Safe To Try Now"));
    assert!(summary_output.contains("Content/Weapon/Sword.weapon"));

    let _ = fs::remove_dir_all(&test_root);
}

fn sample_inference_report() -> InferenceReport {
    InferenceReport {
        schema_version: "whashreonator.inference.v1".to_string(),
        generated_at_unix_ms: 1,
        compare_input: InferenceCompareInput {
            old_version_id: "2.4.0".to_string(),
            new_version_id: "2.5.0".to_string(),
            changed_assets: 1,
            added_assets: 2,
            removed_assets: 2,
            candidate_mapping_changes: 2,
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
            candidate_mapping_hints: 2,
            highest_confidence: 0.94,
        },
        probable_crash_causes: vec![
            ProbableCrashCause {
                code: "buffer_layout_changed".to_string(),
                summary: "body mesh layout changed".to_string(),
                confidence: 0.91,
                risk: RiskLevel::High,
                affected_assets: vec!["Content/Character/HeroA/Body.mesh".to_string()],
                related_patterns: vec![WwmiPatternKind::BufferLayoutOrCapacityFix],
                reasons: vec!["vertex count changed".to_string()],
                evidence: vec!["WWMI buffer fix history".to_string()],
            },
            ProbableCrashCause {
                code: "asset_paths_or_mapping_shifted".to_string(),
                summary: "weapon path changed".to_string(),
                confidence: 0.88,
                risk: RiskLevel::High,
                affected_assets: vec![
                    "Content/Weapon/Sword.weapon -> Content/Weapon/Sword_v2.weapon".to_string(),
                ],
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                reasons: vec!["candidate replacement exists".to_string()],
                evidence: vec!["WWMI mapping fix history".to_string()],
            },
        ],
        suggested_fixes: vec![
            SuggestedFix {
                code: "review_candidate_asset_remaps".to_string(),
                summary: "review mapping hints".to_string(),
                confidence: 0.88,
                priority: RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec!["inspect mapping hints".to_string()],
                reasons: vec!["mapping drift detected".to_string()],
                evidence: vec!["WWMI mapping fixes".to_string()],
            },
            SuggestedFix {
                code: "review_buffer_layout_and_runtime_guards".to_string(),
                summary: "review runtime guards".to_string(),
                confidence: 0.90,
                priority: RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::BufferLayoutOrCapacityFix],
                actions: vec!["inspect layout drift".to_string()],
                reasons: vec!["structural drift detected".to_string()],
                evidence: vec!["WWMI buffer fixes".to_string()],
            },
        ],
        candidate_mapping_hints: vec![
            InferredMappingHint {
                old_asset_path: "Content/Weapon/Sword.weapon".to_string(),
                new_asset_path: "Content/Weapon/Sword_v2.weapon".to_string(),
                confidence: 0.94,
                needs_review: true,
                ambiguous: false,
                confidence_gap: None,
                reasons: vec!["normalized_name_exact: same logical asset".to_string()],
                evidence: vec!["compare candidate confidence 0.940".to_string()],
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
    }
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-proposal-mode-test-{nanos}"))
}
