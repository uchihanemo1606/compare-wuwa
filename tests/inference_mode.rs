use std::{
    fs,
    path::Path,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use whashreonator::{
    cli::InferFixesArgs,
    compare::{
        CandidateMappingChange, RemapCompatibility, RiskLevel, SnapshotAssetChange,
        SnapshotAssetSummary, SnapshotChangeType, SnapshotCompareReason, SnapshotCompareReport,
        SnapshotCompareScopeContext, SnapshotCompareSummary, SnapshotComparer, SnapshotVersionInfo,
    },
    domain::{AssetInternalStructure, AssetMetadata, AssetSourceContext},
    inference::InferenceReport,
    pipeline::run_infer_fixes_command,
    report::VersionDiffReportBuilder,
    report_storage::ReportStorage,
    snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
    },
    wwmi::{
        WwmiEvidenceCommit, WwmiFixPattern, WwmiKeywordStat, WwmiKnowledgeBase,
        WwmiKnowledgeRepoInfo, WwmiKnowledgeSummary, WwmiPatternKind,
        dependency::{WwmiModDependencyKind, WwmiModDependencyProfile, WwmiModDependencySignal},
    },
};

#[test]
fn infer_fixes_command_exports_crash_causes_and_suggested_fixes() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare.json");
    let knowledge_path = test_root.join("knowledge.json");
    let output_path = test_root.join("out").join("inference.json");

    fs::create_dir_all(&test_root).expect("create test root");

    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&sample_compare_report()).expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        output: output_path.clone(),
    })
    .expect("run inference command");

    let output = fs::read_to_string(&output_path).expect("read inference output");
    let parsed: InferenceReport = serde_json::from_str(&output).expect("parse inference report");

    assert_eq!(report.schema_version, "whashreonator.inference.v1");
    assert!(
        parsed
            .probable_crash_causes
            .iter()
            .any(|cause| cause.code == "buffer_layout_changed")
    );
    assert!(
        parsed
            .suggested_fixes
            .iter()
            .any(|fix| fix.code == "review_candidate_asset_remaps")
    );
    assert_eq!(parsed.candidate_mapping_hints.len(), 1);
    assert!(!parsed.scope.low_signal_compare);
    assert!(
        parsed
            .probable_crash_causes
            .iter()
            .all(|cause| cause.code != "continuity_thread_instability")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_uses_report_root_continuity_history() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let knowledge_path = test_root.join("knowledge.json");
    let output_path = test_root.join("out").join("inference-with-continuity.json");

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");

    let snapshot_a = continuity_snapshot("4.0.0", "Content/Character/Encore/Body.mesh", "body");
    let snapshot_b =
        continuity_snapshot("4.1.0", "Content/Character/Encore/Body_LOD0.mesh", "body");
    let snapshot_c = empty_snapshot("4.2.0");

    let compare_ab = SnapshotComparer.compare(&snapshot_a, &snapshot_b);
    let report_ab = VersionDiffReportBuilder.from_compare(&snapshot_a, &snapshot_b, &compare_ab);
    let saved_ab = storage
        .save_run(&report_ab, &snapshot_a, &snapshot_b, &compare_ab, None)
        .expect("save ab");

    let compare_bc = SnapshotComparer.compare(&snapshot_b, &snapshot_c);
    let report_bc = VersionDiffReportBuilder.from_compare(&snapshot_b, &snapshot_c, &compare_bc);
    storage
        .save_run(&report_bc, &snapshot_b, &snapshot_c, &compare_bc, None)
        .expect("save bc");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: saved_ab.directory.join("compare.v1.json"),
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: Some(report_root),
        mod_root: None,
        mod_dependency_profile: None,
        output: output_path.clone(),
    })
    .expect("run inference command with continuity");

    let output = fs::read_to_string(&output_path).expect("read inference output");
    let parsed: InferenceReport = serde_json::from_str(&output).expect("parse inference report");
    let hint = parsed
        .candidate_mapping_hints
        .iter()
        .find(|hint| hint.old_asset_path.ends_with("Body.mesh"))
        .expect("body mapping hint");

    assert!(
        report
            .probable_crash_causes
            .iter()
            .any(|cause| cause.code == "continuity_thread_instability")
    );
    assert!(
        parsed
            .suggested_fixes
            .iter()
            .any(|fix| fix.code == "review_continuity_thread_history_before_repair")
    );
    assert!(
        hint.reasons
            .iter()
            .any(|reason| reason.contains("removed in 4.2.0"))
    );
    assert!(
        hint.evidence
            .iter()
            .any(|evidence| evidence.contains("spans 4.0.0 -> 4.2.0"))
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_uses_mod_root_to_prioritize_mapping_hash_surfaces() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare.json");
    let knowledge_path = test_root.join("knowledge.json");
    let base_output_path = test_root.join("out").join("inference-base.json");
    let mod_output_path = test_root.join("out").join("inference-mod.json");
    let mod_root = test_root.join("mods").join("HashFocusedMod");

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&sample_compare_report()).expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");
    write_mod_ini(
        &mod_root,
        "mod.ini",
        r#"
[TextureOverrideSword]
hash = 0xDEADBEEF

[ResourceSword]
filename = SwordDiffuse.dds
"#,
    );

    let base_report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path.clone(),
        wwmi_knowledge: knowledge_path.clone(),
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        output: base_output_path,
    })
    .expect("run baseline inference command");
    let mod_report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: Some(mod_root),
        mod_dependency_profile: None,
        output: mod_output_path,
    })
    .expect("run mod-aware inference command");

    let base_fix = base_report
        .suggested_fixes
        .iter()
        .find(|fix| fix.code == "review_candidate_asset_remaps")
        .expect("baseline remap fix");
    let mod_fix = mod_report
        .suggested_fixes
        .iter()
        .find(|fix| fix.code == "review_candidate_asset_remaps")
        .expect("mod-aware remap fix");
    let mod_cause = mod_report
        .probable_crash_causes
        .iter()
        .find(|cause| cause.code == "asset_paths_or_mapping_shifted")
        .expect("mod-aware mapping cause");

    assert!(mod_report.mod_dependency_input.is_some());
    assert!(mod_fix.confidence > base_fix.confidence);
    assert!(
        mod_fix
            .reasons
            .iter()
            .any(|reason| reason.contains("mapping/hash-sensitive"))
    );
    assert!(
        mod_cause
            .evidence
            .iter()
            .any(|evidence| evidence.contains("mod dependency files"))
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_uses_mod_dependency_profile_to_keep_buffer_sensitive_remaps_review_first() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-structural.json");
    let knowledge_path = test_root.join("knowledge.json");
    let profile_path = test_root.join("buffer-mod-profile.json");
    let base_output_path = test_root.join("out").join("inference-structural-base.json");
    let mod_output_path = test_root.join("out").join("inference-structural-mod.json");

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&sample_structural_remap_compare_report())
            .expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");
    fs::write(
        &profile_path,
        serde_json::to_string_pretty(&WwmiModDependencyProfile {
            mod_name: Some("BufferFocusedMod".to_string()),
            mod_root: "D:/mods/BufferFocusedMod".to_string(),
            ini_file_count: 1,
            signals: vec![
                WwmiModDependencySignal {
                    kind: WwmiModDependencyKind::BufferLayoutHint,
                    value: "override_byte_stride=32".to_string(),
                    source_file: "mod.ini".to_string(),
                    section: Some("TextureOverrideBody".to_string()),
                },
                WwmiModDependencySignal {
                    kind: WwmiModDependencyKind::MeshVertexCount,
                    value: "12500".to_string(),
                    source_file: "mod.ini".to_string(),
                    section: Some("TextureOverrideBody".to_string()),
                },
            ],
        })
        .expect("serialize mod dependency profile"),
    )
    .expect("write mod dependency profile");

    let base_report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path.clone(),
        wwmi_knowledge: knowledge_path.clone(),
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        output: base_output_path,
    })
    .expect("run baseline inference");
    let mod_report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: Some(profile_path),
        output: mod_output_path,
    })
    .expect("run mod-aware inference");

    let base_hint = base_report
        .candidate_mapping_hints
        .iter()
        .find(|hint| hint.old_asset_path.ends_with("Body.mesh"))
        .expect("baseline body hint");
    let mod_hint = mod_report
        .candidate_mapping_hints
        .iter()
        .find(|hint| hint.old_asset_path.ends_with("Body.mesh"))
        .expect("mod-aware body hint");
    let mod_fix = mod_report
        .suggested_fixes
        .iter()
        .find(|fix| fix.code == "validate_candidate_remaps_against_layout")
        .expect("layout fix");

    assert!(mod_hint.confidence < base_hint.confidence);
    assert!(
        mod_hint
            .reasons
            .iter()
            .any(|reason| reason.contains("mod_dependency_review_first"))
    );
    assert!(
        mod_fix
            .reasons
            .iter()
            .any(|reason| reason.contains("buffer/layout-sensitive"))
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_surfaces_resource_and_skeleton_review_when_mod_depends_on_those_surfaces() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-resource.json");
    let knowledge_path = test_root.join("knowledge.json");
    let output_path = test_root.join("out").join("inference-resource.json");
    let mod_root = test_root.join("mods").join("SkeletonResourceMod");

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&sample_resource_skeleton_compare_report())
            .expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");
    write_mod_ini(
        &mod_root,
        "mod.ini",
        r#"
[ResourceBody]
filename = BodyDiffuse.dds

[CommandListMergeSkeleton]
run = CommandListMergeSkeleton
"#,
    );

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: Some(mod_root),
        mod_dependency_profile: None,
        output: output_path,
    })
    .expect("run resource/skeleton-aware inference");

    let cause = report
        .probable_crash_causes
        .iter()
        .find(|cause| cause.code == "mod_resource_or_skeleton_surface_changed")
        .expect("resource/skeleton cause");
    let fix = report
        .suggested_fixes
        .iter()
        .find(|fix| fix.code == "review_resource_and_skeleton_bindings")
        .expect("resource/skeleton fix");

    assert!(
        cause
            .reasons
            .iter()
            .any(|reason| reason.contains("resource/skeleton-sensitive"))
    );
    assert!(
        fix.actions
            .iter()
            .any(|action| action.contains("merged-skeleton"))
    );

    let _ = fs::remove_dir_all(&test_root);
}

fn sample_compare_report() -> SnapshotCompareReport {
    SnapshotCompareReport {
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
            lineage: whashreonator::compare::AssetLineageKind::InsufficientEvidence,
            reasons: vec![SnapshotCompareReason {
                code: "asset_removed".to_string(),
                message: "asset removed".to_string(),
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
            lineage: whashreonator::compare::AssetLineageKind::Replacement,
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
            lineage: whashreonator::compare::AssetLineageKind::RenameOrRepath,
            reasons: vec![SnapshotCompareReason {
                code: "normalized_name_exact".to_string(),
                message: "normalized name matched".to_string(),
            }],
            runner_up_confidence: None,
            confidence_gap: None,
            ambiguous: false,
        }],
    }
}

fn sample_structural_remap_compare_report() -> SnapshotCompareReport {
    SnapshotCompareReport {
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
            structural_changed_assets: 1,
            naming_only_changed_assets: 0,
            cosmetic_only_changed_assets: 0,
            provenance_changed_assets: 0,
            container_moved_assets: 0,
            lineage_rename_or_repath_assets: 0,
            lineage_container_movement_assets: 0,
            lineage_layout_drift_assets: 1,
            lineage_replacement_assets: 0,
            lineage_ambiguous_assets: 0,
            lineage_insufficient_evidence_assets: 0,
            ambiguous_candidate_mapping_changes: 0,
            high_confidence_candidate_mapping_changes: 1,
        },
        added_assets: Vec::new(),
        removed_assets: vec![SnapshotAssetChange {
            change_type: SnapshotChangeType::Removed,
            old_asset: Some(asset_summary("Content/Character/HeroA/Body.mesh")),
            new_asset: None,
            changed_fields: vec!["path_presence".to_string()],
            probable_impact: RiskLevel::High,
            crash_risk: RiskLevel::High,
            suspected_mapping_change: true,
            lineage: whashreonator::compare::AssetLineageKind::LayoutDrift,
            reasons: vec![SnapshotCompareReason {
                code: "asset_removed".to_string(),
                message: "asset removed".to_string(),
            }],
        }],
        changed_assets: Vec::new(),
        candidate_mapping_changes: vec![CandidateMappingChange {
            old_asset: asset_summary("Content/Character/HeroA/Body.mesh"),
            new_asset: asset_summary("Content/Character/HeroA/Body_v2.mesh"),
            confidence: 0.93,
            compatibility: RemapCompatibility::StructurallyRisky,
            lineage: whashreonator::compare::AssetLineageKind::LayoutDrift,
            reasons: vec![
                SnapshotCompareReason {
                    code: "signature_exact".to_string(),
                    message: "asset signature matched exactly".to_string(),
                },
                SnapshotCompareReason {
                    code: "same_asset_but_structural_drift".to_string(),
                    message: "structure changed".to_string(),
                },
                SnapshotCompareReason {
                    code: "buffer_layout_validation_needed".to_string(),
                    message: "layout validation needed".to_string(),
                },
            ],
            runner_up_confidence: None,
            confidence_gap: Some(0.20),
            ambiguous: false,
        }],
    }
}

fn sample_resource_skeleton_compare_report() -> SnapshotCompareReport {
    SnapshotCompareReport {
        schema_version: "whashreonator.snapshot-compare.v1".to_string(),
        old_snapshot: SnapshotVersionInfo {
            version_id: "6.5.0".to_string(),
            source_root: "old".to_string(),
            asset_count: 1,
        },
        new_snapshot: SnapshotVersionInfo {
            version_id: "6.6.0".to_string(),
            source_root: "new".to_string(),
            asset_count: 1,
        },
        scope: SnapshotCompareScopeContext::default(),
        summary: SnapshotCompareSummary {
            total_old_assets: 1,
            total_new_assets: 1,
            unchanged_assets: 0,
            added_assets: 0,
            removed_assets: 0,
            changed_assets: 1,
            candidate_mapping_changes: 1,
            identity_changed_assets: 0,
            layout_changed_assets: 0,
            structural_changed_assets: 1,
            naming_only_changed_assets: 0,
            cosmetic_only_changed_assets: 0,
            provenance_changed_assets: 1,
            container_moved_assets: 1,
            lineage_rename_or_repath_assets: 0,
            lineage_container_movement_assets: 1,
            lineage_layout_drift_assets: 0,
            lineage_replacement_assets: 0,
            lineage_ambiguous_assets: 0,
            lineage_insufficient_evidence_assets: 0,
            ambiguous_candidate_mapping_changes: 0,
            high_confidence_candidate_mapping_changes: 1,
        },
        added_assets: Vec::new(),
        removed_assets: Vec::new(),
        changed_assets: vec![SnapshotAssetChange {
            change_type: SnapshotChangeType::Changed,
            old_asset: Some(asset_summary("Content/Character/HeroA/Body.mesh")),
            new_asset: Some(asset_summary("Content/Character/HeroA/Body.mesh")),
            changed_fields: vec![
                "container_path".to_string(),
                "source_kind".to_string(),
                "internal_structure.has_skeleton".to_string(),
            ],
            probable_impact: RiskLevel::High,
            crash_risk: RiskLevel::High,
            suspected_mapping_change: true,
            lineage: whashreonator::compare::AssetLineageKind::ContainerMovement,
            reasons: vec![SnapshotCompareReason {
                code: "container_package_movement_detected".to_string(),
                message: "container changed".to_string(),
            }],
        }],
        candidate_mapping_changes: vec![CandidateMappingChange {
            old_asset: asset_summary("Content/Character/HeroA/Body.mesh"),
            new_asset: asset_summary("Content/Character/HeroA/Body_v2.mesh"),
            confidence: 0.88,
            compatibility: RemapCompatibility::CompatibleWithCaution,
            lineage: whashreonator::compare::AssetLineageKind::ContainerMovement,
            reasons: vec![
                SnapshotCompareReason {
                    code: "container_path_mismatch".to_string(),
                    message: "container path changed".to_string(),
                },
                SnapshotCompareReason {
                    code: "internal_skeleton_presence_mismatch".to_string(),
                    message: "skeleton presence changed".to_string(),
                },
            ],
            runner_up_confidence: None,
            confidence_gap: Some(0.15),
            ambiguous: false,
        }],
    }
}

fn sample_knowledge() -> WwmiKnowledgeBase {
    WwmiKnowledgeBase {
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
    }
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
        internal_structure: AssetInternalStructure::default(),
        asset_hash: None,
        shader_hash: None,
        signature: None,
        tags: vec!["character".to_string()],
        source: AssetSourceContext::default(),
    }
}

fn continuity_snapshot(version_id: &str, path: &str, logical_name: &str) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "root".to_string(),
        asset_count: 1,
        assets: vec![SnapshotAsset {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            metadata: AssetMetadata {
                logical_name: Some(logical_name.to_string()),
                vertex_count: Some(120),
                index_count: Some(240),
                material_slots: Some(1),
                section_count: Some(1),
                tags: vec!["character".to_string()],
                ..Default::default()
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: Some(logical_name.to_string()),
                name_tokens: vec![logical_name.to_string()],
                path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
                tags: vec!["character".to_string()],
                vertex_count: Some(120),
                index_count: Some(240),
                material_slots: Some(1),
                section_count: Some(1),
                ..Default::default()
            },
            hash_fields: SnapshotHashFields {
                asset_hash: Some(format!("hash-{path}")),
                shader_hash: Some("shader-shared".to_string()),
                signature: Some(format!("sig-{logical_name}")),
            },
            source: AssetSourceContext::default(),
        }],
        context: SnapshotContext::default(),
    }
}

fn empty_snapshot(version_id: &str) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "root".to_string(),
        asset_count: 0,
        assets: Vec::new(),
        context: SnapshotContext::default(),
    }
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-inference-mode-test-{nanos}"))
}

fn write_mod_ini(root: &Path, relative_path: &str, content: &str) {
    let full_path = root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create mod ini parent");
    }
    fs::write(full_path, content.trim()).expect("write mod ini");
}
