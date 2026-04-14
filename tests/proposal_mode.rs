use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::GenerateProposalsArgs,
    compare::{RemapCompatibility, RiskLevel},
    inference::{
        InferenceCompareInput, InferenceKnowledgeInput, InferenceModDependencyInput,
        InferenceReport, InferenceScopeContext, InferenceSummary, InferredMappingContinuityContext,
        InferredMappingHint, ProbableCrashCause, SuggestedFix,
    },
    pipeline::run_generate_proposals_command,
    proposal::{MappingProposalOutput, ProposalPatchDraftOutput, ProposalStatus},
    report::VersionContinuityRelation,
    wwmi::{WwmiPatternKind, dependency::WwmiModDependencyKind},
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

#[test]
fn generate_proposals_command_keeps_continuity_unstable_mapping_in_review() {
    let test_root = unique_test_dir();
    let inference_path = test_root.join("tmp").join("inference.json");
    let mapping_output_path = test_root.join("out").join("mapping-proposal.json");
    let summary_output_path = test_root.join("out").join("summary.md");

    fs::create_dir_all(test_root.join("tmp")).expect("create temp input root");
    fs::write(
        &inference_path,
        serde_json::to_string_pretty(&continuity_flagged_inference_report())
            .expect("serialize inference"),
    )
    .expect("write inference report");

    run_generate_proposals_command(&GenerateProposalsArgs {
        inference_report: inference_path,
        mapping_output: Some(mapping_output_path.clone()),
        patch_draft_output: None,
        summary_output: Some(summary_output_path.clone()),
        min_confidence: 0.85,
    })
    .expect("run continuity-aware generate proposals command");

    let mapping_output =
        fs::read_to_string(&mapping_output_path).expect("read mapping proposal output");
    let summary_output = fs::read_to_string(&summary_output_path).expect("read summary output");

    let parsed_mapping: MappingProposalOutput =
        serde_json::from_str(&mapping_output).expect("parse mapping proposal output");
    let mapping = parsed_mapping
        .mappings
        .iter()
        .find(|entry| entry.old_asset_path == "Content/Character/Encore/Body.mesh")
        .expect("continuity mapping exists");

    assert_eq!(mapping.status, ProposalStatus::NeedsReview);
    assert!(
        mapping
            .reasons
            .iter()
            .any(|reason| reason.contains("continuity"))
    );
    assert!(
        mapping
            .related_fix_codes
            .iter()
            .any(|code| code == "review_continuity_thread_history_before_repair")
    );
    assert!(summary_output.contains("Continuity-backed caution"));
    assert!(summary_output.contains("### Continuity Caution"));
    assert!(summary_output.contains("broader continuity history"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn generate_proposals_command_surfaces_mod_aware_review_first_context() {
    let test_root = unique_test_dir();
    let inference_path = test_root.join("tmp").join("inference-mod-aware.json");
    let mapping_output_path = test_root
        .join("out")
        .join("mapping-proposal-mod-aware.json");
    let summary_output_path = test_root.join("out").join("summary-mod-aware.md");

    fs::create_dir_all(test_root.join("tmp")).expect("create temp input root");
    fs::write(
        &inference_path,
        serde_json::to_string_pretty(&mod_aware_hook_targeting_inference_report())
            .expect("serialize inference"),
    )
    .expect("write inference report");

    run_generate_proposals_command(&GenerateProposalsArgs {
        inference_report: inference_path,
        mapping_output: Some(mapping_output_path.clone()),
        patch_draft_output: None,
        summary_output: Some(summary_output_path.clone()),
        min_confidence: 0.85,
    })
    .expect("run mod-aware generate proposals command");

    let mapping_output =
        fs::read_to_string(&mapping_output_path).expect("read mapping proposal output");
    let summary_output = fs::read_to_string(&summary_output_path).expect("read summary output");

    let parsed_mapping: MappingProposalOutput =
        serde_json::from_str(&mapping_output).expect("parse mapping proposal output");
    let mapping = parsed_mapping
        .mappings
        .iter()
        .find(|entry| entry.old_asset_path == "Content/Character/Encore/Hair.mesh")
        .expect("mod-aware mapping exists");

    assert_eq!(mapping.status, ProposalStatus::NeedsReview);
    assert!(
        mapping
            .reasons
            .iter()
            .any(|reason| reason.contains("mod dependency profile keeps this mapping review-first"))
    );
    assert!(
        mapping
            .related_fix_codes
            .iter()
            .any(|code| code == "review_draw_call_and_filter_hooks_before_remap")
    );
    assert!(summary_output.contains("Mod dependency profile"));
    assert!(summary_output.contains("Mod focus:"));
    assert!(summary_output.contains("hook-targeting-sensitive"));

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
        mod_dependency_input: None,
        representative_mod_baseline_input: None,
        scope: InferenceScopeContext::default(),
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
                compatibility: RemapCompatibility::LikelyCompatible,
                needs_review: true,
                ambiguous: false,
                confidence_gap: None,
                continuity: None,
                reasons: vec!["normalized_name_exact: same logical asset".to_string()],
                evidence: vec!["compare candidate confidence 0.940".to_string()],
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
        representative_risk_projections: Vec::new(),
    }
}

fn continuity_flagged_inference_report() -> InferenceReport {
    InferenceReport {
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
        representative_mod_baseline_input: None,
        scope: InferenceScopeContext::default(),
        summary: InferenceSummary {
            probable_crash_causes: 1,
            suggested_fixes: 1,
            candidate_mapping_hints: 1,
            highest_confidence: 0.92,
        },
        probable_crash_causes: vec![ProbableCrashCause {
            code: "continuity_thread_instability".to_string(),
            summary: "broader continuity history is unstable".to_string(),
            confidence: 0.84,
            risk: RiskLevel::High,
            affected_assets: vec!["Content/Character/Encore/Body.mesh".to_string()],
            related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
            reasons: vec!["continuity surfaces unstable thread history".to_string()],
            evidence: vec!["continuity thread Content/Character/Encore/Body_v2.mesh spans 7.0.0 -> 8.2.0; later terminates as removed in 8.2.0".to_string()],
        }],
        suggested_fixes: vec![SuggestedFix {
            code: "review_continuity_thread_history_before_repair".to_string(),
            summary: "review broader continuity history".to_string(),
            confidence: 0.81,
            priority: RiskLevel::High,
            related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
            actions: vec!["inspect continuity milestones".to_string()],
            reasons: vec!["unstable thread history".to_string()],
            evidence: vec!["continuity thread later terminates as removed in 8.2.0".to_string()],
        }],
        candidate_mapping_hints: vec![InferredMappingHint {
            old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
            new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
            confidence: 0.92,
            compatibility: RemapCompatibility::CompatibleWithCaution,
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
            reasons: vec![
                "normalized_name_exact: encore body".to_string(),
                "same_parent_directory: same folder".to_string(),
            ],
            evidence: vec!["compare candidate confidence 0.920".to_string()],
        }],
        representative_risk_projections: Vec::new(),
    }
}

fn mod_aware_hook_targeting_inference_report() -> InferenceReport {
    InferenceReport {
        schema_version: "whashreonator.inference.v1".to_string(),
        generated_at_unix_ms: 1,
        compare_input: InferenceCompareInput {
            old_version_id: "10.0.0".to_string(),
            new_version_id: "10.1.0".to_string(),
            changed_assets: 0,
            added_assets: 1,
            removed_assets: 1,
            candidate_mapping_changes: 1,
        },
        knowledge_input: InferenceKnowledgeInput {
            repo: "repo".to_string(),
            analyzed_commits: 5,
            fix_like_commits: 3,
            discovered_patterns: 2,
        },
        mod_dependency_input: Some(InferenceModDependencyInput {
            mod_name: Some("CarlottaMod".to_string()),
            mod_root: "D:/mod/WWMI/Mods/CarlottaMod".to_string(),
            ini_file_count: 2,
            signal_count: 4,
            dependency_kinds: vec![
                WwmiModDependencyKind::ObjectGuid,
                WwmiModDependencyKind::DrawCallTarget,
                WwmiModDependencyKind::FilterIndex,
            ],
        }),
        representative_mod_baseline_input: None,
        scope: InferenceScopeContext::default(),
        summary: InferenceSummary {
            probable_crash_causes: 1,
            suggested_fixes: 1,
            candidate_mapping_hints: 1,
            highest_confidence: 0.91,
        },
        probable_crash_causes: vec![ProbableCrashCause {
            code: "mod_hook_targeting_surface_requires_manual_review".to_string(),
            summary: "hook targeting still needs manual review".to_string(),
            confidence: 0.75,
            risk: RiskLevel::Medium,
            affected_assets: vec![
                "Content/Character/Encore/Hair.mesh".to_string(),
                "Content/Character/Encore/Hair_LOD0.mesh".to_string(),
            ],
            related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
            reasons: vec![
                "mod_dependency_surface: this mod uses hook-targeting-sensitive surfaces that stay review-first"
                    .to_string(),
            ],
            evidence: vec![
                "draw-call/filter/object-guid targeting is present in the scanned mod ini files"
                    .to_string(),
            ],
        }],
        suggested_fixes: vec![SuggestedFix {
            code: "review_draw_call_and_filter_hooks_before_remap".to_string(),
            summary: "review draw-call and filter hooks".to_string(),
            confidence: 0.80,
            priority: RiskLevel::High,
            related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
            actions: vec!["inspect hook-targeting sections first".to_string()],
            reasons: vec![
                "mod_dependency_surface: this mod uses hook-targeting-sensitive surfaces, so remap promotion should stay review-first"
                    .to_string(),
            ],
            evidence: vec![
                "hook-targeting ini sections often need manual retargeting beyond asset remap confidence"
                    .to_string(),
            ],
        }],
        candidate_mapping_hints: vec![InferredMappingHint {
            old_asset_path: "Content/Character/Encore/Hair.mesh".to_string(),
            new_asset_path: "Content/Character/Encore/Hair_LOD0.mesh".to_string(),
            confidence: 0.91,
            compatibility: RemapCompatibility::CompatibleWithCaution,
            needs_review: true,
            ambiguous: false,
            confidence_gap: Some(0.20),
            continuity: None,
            reasons: vec![
                "normalized_name_exact: encore hair".to_string(),
                "same_parent_directory: same folder".to_string(),
                "mod_dependency_review_first: this mod uses hook-targeting-sensitive surfaces, so even plausible remaps require manual hook revalidation"
                    .to_string(),
            ],
            evidence: vec![
                "compare candidate confidence 0.910".to_string(),
                "draw-call/filter/object-guid hooks can break independently of asset similarity"
                    .to_string(),
                "mod dependency files: CarlottaMod.ini".to_string(),
            ],
        }],
        representative_risk_projections: Vec::new(),
    }
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-proposal-mode-test-{nanos}"))
}
