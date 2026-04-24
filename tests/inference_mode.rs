use std::{
    fs,
    path::Path,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

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
        dependency::{
            WwmiModDependencyBaselineSet, WwmiModDependencyBaselineStrength, WwmiModDependencyKind,
            WwmiModDependencyProfile, WwmiModDependencySignal, WwmiModDependencySurfaceClass,
            build_mod_dependency_baseline_set,
        },
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
        representative_mod_baseline_set: None,
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
        representative_mod_baseline_set: None,
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
        representative_mod_baseline_set: None,
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
        representative_mod_baseline_set: None,
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
    assert_eq!(
        mod_report.surface_intersection.overlapping_surface_classes,
        vec![WwmiModDependencySurfaceClass::MappingHash]
    );
    assert_eq!(
        mod_report.surface_intersection.overlap_posture,
        whashreonator::inference::InferenceSurfaceOverlapPosture::Partial
    );
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| { note.contains("mod-side dependency surfaces: mapping_hash") })
    );
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| note.contains("surface overlap posture: partial"))
    );
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| { note.contains("game-side change surfaces: mapping_hash") })
    );
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| note.contains("surface intersection overlap: mapping_hash"))
    );
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
fn infer_fixes_command_does_not_treat_resource_only_profiles_as_mapping_hash_sensitive() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare.json");
    let knowledge_path = test_root.join("knowledge.json");
    let base_output_path = test_root.join("out").join("inference-base.json");
    let mod_output_path = test_root.join("out").join("inference-resource-only.json");
    let mod_root = test_root.join("mods").join("ResourceOnlyMod");

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
        representative_mod_baseline_set: None,
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
        representative_mod_baseline_set: None,
        output: mod_output_path,
    })
    .expect("run resource-only mod-aware inference command");

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

    assert!(mod_report.mod_dependency_input.is_some());
    assert_eq!(mod_fix.confidence, base_fix.confidence);
    assert!(
        mod_report
            .surface_intersection
            .mod_side_surfaces
            .iter()
            .any(|surface| {
                surface.surface_class == WwmiModDependencySurfaceClass::ResourceSkeleton
            })
    );
    assert!(
        mod_report
            .surface_intersection
            .overlapping_surface_classes
            .is_empty()
    );
    assert_eq!(
        mod_report.surface_intersection.overlap_posture,
        whashreonator::inference::InferenceSurfaceOverlapPosture::None
    );
    assert!(mod_report.surface_intersection.weak_or_absent_overlap);
    assert!(mod_report.scope.notes.iter().any(|note| {
        note.contains("no explicit mod/game surface overlap")
            || note.contains("review-only context")
    }));
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| note.contains("surface overlap posture: none"))
    );
    assert!(
        mod_fix
            .reasons
            .iter()
            .all(|reason| !reason.contains("mapping/hash-sensitive"))
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
        representative_mod_baseline_set: None,
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
        representative_mod_baseline_set: None,
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
    assert_eq!(
        mod_report.surface_intersection.overlapping_surface_classes,
        vec![WwmiModDependencySurfaceClass::BufferLayout]
    );
    assert_eq!(
        mod_report.surface_intersection.overlap_posture,
        whashreonator::inference::InferenceSurfaceOverlapPosture::Strong
    );
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| { note.contains("mod-side dependency surfaces: buffer_layout") })
    );
    assert!(mod_report.scope.notes.iter().any(|note| {
        note.contains("game-side change surfaces:") && note.contains("buffer_layout")
    }));
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| note.contains("surface intersection overlap: buffer_layout"))
    );
    assert!(
        mod_report
            .scope
            .notes
            .iter()
            .any(|note| note.contains("surface overlap posture: strong"))
    );
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
    assert!(
        mod_fix
            .evidence
            .iter()
            .any(|evidence| evidence.contains("mod-side dependency profile"))
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
        representative_mod_baseline_set: None,
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

#[test]
fn infer_fixes_command_loads_representative_baseline_set_from_report_root_and_projects_surfaces() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let compare_path = test_root.join("compare.json");
    let knowledge_path = test_root.join("knowledge.json");
    let output_path = test_root.join("out").join("inference-representative.json");

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
    storage
        .save_mod_dependency_baseline_set_for_version(
            "2.4.0",
            &sample_representative_baseline_set("2.4.0"),
        )
        .expect("store representative baseline set");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: Some(report_root),
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: None,
        output: output_path,
    })
    .expect("run representative baseline inference");

    let representative = report
        .representative_mod_baseline_input
        .as_ref()
        .expect("representative baseline input");
    assert_eq!(representative.version_id, "2.4.0");
    assert_eq!(representative.profile_count, 4);
    assert_eq!(representative.included_mod_count, 4);
    assert_eq!(
        representative.strength,
        WwmiModDependencyBaselineStrength::Partial
    );
    assert!(representative.material_for_repair_review);
    assert_eq!(
        representative.represented_surface_classes,
        vec![
            WwmiModDependencySurfaceClass::MappingHash,
            WwmiModDependencySurfaceClass::BufferLayout,
            WwmiModDependencySurfaceClass::ResourceSkeleton,
            WwmiModDependencySurfaceClass::DrawCallFilterHook,
        ]
    );
    assert!(
        representative
            .caution_notes
            .iter()
            .any(|note| note.contains("not an exhaustive"))
    );
    assert!(report.scope.notes.iter().any(|note| {
        note.contains("strength=Partial")
            && note.contains(
                "surfaces=mapping_hash, buffer_layout, resource_skeleton, draw_call_filter_hook",
            )
    }));

    assert!(
        report
            .representative_risk_projections
            .iter()
            .any(|projection| {
                projection.risk_class
                    == whashreonator::inference::RepresentativeModRiskClass::MappingHashSensitive
            })
    );
    assert!(
        report
            .representative_risk_projections
            .iter()
            .any(|projection| {
                projection.risk_class
                    == whashreonator::inference::RepresentativeModRiskClass::BufferLayoutSensitive
            })
    );
    assert!(
        report
            .representative_risk_projections
            .iter()
            .all(|projection| {
                projection.risk_class
                    != whashreonator::inference::RepresentativeModRiskClass::DrawCallFilterHookSensitive
            })
    );
    assert!(
        report
            .representative_risk_projections
            .iter()
            .all(|projection| {
                projection
                    .evidence
                    .iter()
                    .any(|item| item.contains("representative sample mods"))
            })
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_rejects_mismatched_explicit_representative_baseline_version() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare.json");
    let knowledge_path = test_root.join("knowledge.json");
    let baseline_path = test_root.join("mismatched-baselines.json");
    let output_path = test_root
        .join("out")
        .join("inference-mismatched-representative.json");

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
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&sample_representative_baseline_set("2.5.0"))
            .expect("serialize baseline set"),
    )
    .expect("write representative baseline set");

    let error = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: Some(baseline_path),
        output: output_path,
    })
    .expect_err("mismatched representative baseline should fail");

    assert!(
        error
            .to_string()
            .contains("does not match compare old snapshot version")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_does_not_autoload_new_version_representative_baseline() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let compare_path = test_root.join("compare.json");
    let knowledge_path = test_root.join("knowledge.json");
    let output_path = test_root.join("out").join("inference-no-old-baseline.json");

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
    storage
        .save_mod_dependency_baseline_set_for_version(
            "2.5.0",
            &sample_representative_baseline_set("2.5.0"),
        )
        .expect("store new-version representative baseline set");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: Some(report_root),
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: None,
        output: output_path,
    })
    .expect("run inference without old-version representative baseline");

    assert!(report.representative_mod_baseline_input.is_none());
    assert!(report.representative_risk_projections.is_empty());

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_projects_resource_skeleton_surface_from_explicit_representative_set() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-resource.json");
    let knowledge_path = test_root.join("knowledge.json");
    let baseline_path = test_root.join("resource-baselines.json");
    let output_path = test_root
        .join("out")
        .join("inference-resource-representative.json");

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
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&sample_representative_baseline_set("6.5.0"))
            .expect("serialize baseline set"),
    )
    .expect("write representative baseline set");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: Some(baseline_path),
        output: output_path,
    })
    .expect("run explicit representative baseline inference");

    let projection = report
        .representative_risk_projections
        .iter()
        .find(|projection| {
            projection.risk_class
                == whashreonator::inference::RepresentativeModRiskClass::ResourceSkeletonSensitive
        })
        .expect("resource/skeleton representative projection");
    assert_eq!(projection.priority, RiskLevel::High);
    assert!(
        projection
            .triggering_compare_signals
            .iter()
            .any(|signal| signal == "resource_or_skeleton_field_drift")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_projects_hook_sensitive_surface_only_when_hook_context_drifts() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-hook.json");
    let knowledge_path = test_root.join("knowledge.json");
    let baseline_path = test_root.join("hook-baselines.json");
    let output_path = test_root
        .join("out")
        .join("inference-hook-representative.json");

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&sample_hook_sensitive_compare_report())
            .expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&sample_representative_baseline_set("6.5.0"))
            .expect("serialize baseline set"),
    )
    .expect("write representative baseline set");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: Some(baseline_path),
        output: output_path,
    })
    .expect("run hook-sensitive representative inference");

    let projection = report
        .representative_risk_projections
        .iter()
        .find(|projection| {
            projection.risk_class
                == whashreonator::inference::RepresentativeModRiskClass::DrawCallFilterHookSensitive
        })
        .expect("hook-sensitive representative projection");
    assert!(projection.review_first);
    assert!(
        projection
            .triggering_compare_signals
            .iter()
            .any(|signal| signal == "hook_targeting_context_drift")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_keeps_representative_projection_review_first_in_low_signal_scope() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-low-signal.json");
    let knowledge_path = test_root.join("knowledge.json");
    let baseline_path = test_root.join("low-signal-baselines.json");
    let output_path = test_root
        .join("out")
        .join("inference-low-signal-representative.json");

    let mut compare = sample_compare_report();
    compare.scope.low_signal_compare = true;
    compare.scope.old_snapshot.low_signal_for_character_analysis = true;
    compare
        .scope
        .old_snapshot
        .meaningful_asset_record_enrichment = false;
    compare.scope.new_snapshot.low_signal_for_character_analysis = true;
    compare
        .scope
        .new_snapshot
        .meaningful_asset_record_enrichment = false;
    compare
        .scope
        .notes
        .push("low-signal fixture scope".to_string());

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&compare).expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&sample_representative_baseline_set("2.4.0"))
            .expect("serialize baseline set"),
    )
    .expect("write representative baseline set");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: Some(baseline_path),
        output: output_path,
    })
    .expect("run low-signal representative inference");

    assert!(!report.representative_risk_projections.is_empty());
    assert!(
        report
            .representative_risk_projections
            .iter()
            .all(|projection| projection.review_first)
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_does_not_promote_scope_induced_removals_into_crash_causes() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-scope-induced.json");
    let knowledge_path = test_root.join("knowledge.json");
    let output_path = test_root.join("out").join("inference-scope-induced.json");

    let mut compare = sample_compare_report();
    compare.new_snapshot.version_id = "2.4.0".to_string();
    compare.new_snapshot.asset_count = 0;
    compare.summary.total_new_assets = 0;
    compare.summary.added_assets = 0;
    compare.summary.removed_assets = 1;
    compare.summary.changed_assets = 0;
    compare.summary.candidate_mapping_changes = 0;
    compare.added_assets.clear();
    compare.changed_assets.clear();
    compare.candidate_mapping_changes.clear();
    compare.scope.low_signal_compare = true;
    compare.scope.old_snapshot.capture_mode = Some("local_filesystem_inventory".to_string());
    compare.scope.new_snapshot.capture_mode =
        Some("local_filesystem_inventory_character_focused".to_string());
    compare.scope.old_snapshot.low_signal_for_character_analysis = true;
    compare.scope.new_snapshot.low_signal_for_character_analysis = true;
    compare.scope.scope_narrowing_detected = true;
    compare.scope.scope_induced_removals_likely = true;
    compare.scope.notes.push(
        "scope-induced removal caution: new snapshot capture mode 'local_filesystem_inventory_character_focused' is narrower than old 'local_filesystem_inventory'; 1 removed assets likely reflect scope filtering rather than true game-version drift, and the narrower scope yielded 0 visible assets"
            .to_string(),
    );

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&compare).expect("serialize compare"),
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
        representative_mod_baseline_set: None,
        output: output_path,
    })
    .expect("run scope-induced inference");

    assert!(
        report
            .probable_crash_causes
            .iter()
            .all(|cause| cause.code != "asset_removed_without_clear_replacement")
    );
    assert!(
        report
            .scope
            .notes
            .iter()
            .any(|note| { note.contains("scope-induced were kept out of crash-cause promotion") })
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_does_not_create_false_mapping_hash_surface_overlap_from_scope_induced_removals()
 {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-scope-induced-overlap.json");
    let knowledge_path = test_root.join("knowledge.json");
    let output_path = test_root
        .join("out")
        .join("inference-scope-induced-overlap.json");
    let mod_root = test_root.join("mods").join("HashFocusedMod");

    let mut compare = sample_compare_report();
    compare.new_snapshot.version_id = "2.4.0".to_string();
    compare.new_snapshot.asset_count = 0;
    compare.summary.total_new_assets = 0;
    compare.summary.added_assets = 0;
    compare.summary.removed_assets = 1;
    compare.summary.changed_assets = 0;
    compare.summary.candidate_mapping_changes = 0;
    compare.added_assets.clear();
    compare.changed_assets.clear();
    compare.candidate_mapping_changes.clear();
    compare.scope.low_signal_compare = true;
    compare.scope.old_snapshot.capture_mode = Some("local_filesystem_inventory".to_string());
    compare.scope.new_snapshot.capture_mode =
        Some("local_filesystem_inventory_character_focused".to_string());
    compare.scope.old_snapshot.low_signal_for_character_analysis = true;
    compare.scope.new_snapshot.low_signal_for_character_analysis = true;
    compare.scope.scope_narrowing_detected = true;
    compare.scope.scope_induced_removals_likely = true;
    compare.scope.notes.push(
        "scope-induced removal caution: new snapshot capture mode 'local_filesystem_inventory_character_focused' is narrower than old 'local_filesystem_inventory'; 1 removed assets likely reflect scope filtering rather than true game-version drift, and the narrower scope yielded 0 visible assets"
            .to_string(),
    );

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&compare).expect("serialize compare"),
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
"#,
    );

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: Some(mod_root),
        mod_dependency_profile: None,
        representative_mod_baseline_set: None,
        output: output_path,
    })
    .expect("run scope-induced overlap inference");

    assert!(
        report
            .surface_intersection
            .game_side_surfaces
            .iter()
            .all(|surface| surface.surface_class != WwmiModDependencySurfaceClass::MappingHash)
    );
    assert!(
        report
            .surface_intersection
            .overlapping_surface_classes
            .is_empty()
    );
    assert_eq!(
        report.surface_intersection.overlap_posture,
        whashreonator::inference::InferenceSurfaceOverlapPosture::None
    );
    assert!(report.surface_intersection.weak_or_absent_overlap);

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_does_not_project_mapping_hash_risk_from_scope_induced_removals() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-scope-induced-representative.json");
    let knowledge_path = test_root.join("knowledge.json");
    let baseline_path = test_root.join("representative-baseline.json");
    let output_path = test_root
        .join("out")
        .join("inference-scope-induced-representative.json");

    let mut compare = sample_compare_report();
    compare.new_snapshot.version_id = "2.4.0".to_string();
    compare.new_snapshot.asset_count = 0;
    compare.summary.total_new_assets = 0;
    compare.summary.added_assets = 0;
    compare.summary.removed_assets = 1;
    compare.summary.changed_assets = 0;
    compare.summary.candidate_mapping_changes = 0;
    compare.added_assets.clear();
    compare.changed_assets.clear();
    compare.candidate_mapping_changes.clear();
    compare.scope.low_signal_compare = true;
    compare.scope.old_snapshot.capture_mode = Some("local_filesystem_inventory".to_string());
    compare.scope.new_snapshot.capture_mode =
        Some("local_filesystem_inventory_character_focused".to_string());
    compare.scope.old_snapshot.low_signal_for_character_analysis = true;
    compare.scope.new_snapshot.low_signal_for_character_analysis = true;
    compare.scope.scope_narrowing_detected = true;
    compare.scope.scope_induced_removals_likely = true;
    compare.scope.notes.push(
        "scope-induced removal caution: new snapshot capture mode 'local_filesystem_inventory_character_focused' is narrower than old 'local_filesystem_inventory'; 1 removed assets likely reflect scope filtering rather than true game-version drift, and the narrower scope yielded 0 visible assets"
            .to_string(),
    );

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&compare).expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&sample_representative_baseline_set("2.4.0"))
            .expect("serialize baseline set"),
    )
    .expect("write representative baseline set");

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: Some(baseline_path),
        output: output_path,
    })
    .expect("run scope-induced representative inference");

    assert!(
        report
            .representative_risk_projections
            .iter()
            .all(|projection| {
                projection.risk_class
                    != whashreonator::inference::RepresentativeModRiskClass::MappingHashSensitive
            })
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn infer_fixes_command_keeps_mapping_hash_projection_for_non_scope_induced_removals() {
    let test_root = unique_test_dir();
    let compare_path = test_root.join("compare-non-scope-induced-removal.json");
    let knowledge_path = test_root.join("knowledge.json");
    let baseline_path = test_root.join("representative-baseline.json");
    let output_path = test_root
        .join("out")
        .join("inference-non-scope-induced-removal.json");
    let mod_root = test_root.join("mods").join("HashFocusedMod");

    let mut compare = sample_compare_report();
    compare.new_snapshot.version_id = "2.4.0".to_string();
    compare.summary.total_new_assets = 1;
    compare.summary.added_assets = 0;
    compare.summary.removed_assets = 1;
    compare.summary.changed_assets = 0;
    compare.summary.candidate_mapping_changes = 0;
    compare.added_assets.clear();
    compare.changed_assets.clear();
    compare.candidate_mapping_changes.clear();
    compare.scope.low_signal_compare = false;
    compare.scope.scope_narrowing_detected = false;
    compare.scope.scope_induced_removals_likely = false;

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &compare_path,
        serde_json::to_string_pretty(&compare).expect("serialize compare"),
    )
    .expect("write compare report");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge report");
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&sample_representative_baseline_set("2.4.0"))
            .expect("serialize baseline set"),
    )
    .expect("write representative baseline set");
    write_mod_ini(
        &mod_root,
        "mod.ini",
        r#"
[TextureOverrideSword]
hash = 0xDEADBEEF
"#,
    );

    let report = run_infer_fixes_command(&InferFixesArgs {
        compare_report: compare_path,
        wwmi_knowledge: knowledge_path,
        continuity_artifact: None,
        report_root: None,
        mod_root: Some(mod_root),
        mod_dependency_profile: None,
        representative_mod_baseline_set: Some(baseline_path),
        output: output_path,
    })
    .expect("run non-scope-induced removal inference");

    let mapping_surface = report
        .surface_intersection
        .game_side_surfaces
        .iter()
        .find(|surface| surface.surface_class == WwmiModDependencySurfaceClass::MappingHash)
        .expect("mapping-hash game-side surface");
    let projection = report
        .representative_risk_projections
        .iter()
        .find(|projection| {
            projection.risk_class
                == whashreonator::inference::RepresentativeModRiskClass::MappingHashSensitive
        })
        .expect("mapping-hash representative projection");

    assert_eq!(
        report.surface_intersection.overlapping_surface_classes,
        vec![WwmiModDependencySurfaceClass::MappingHash]
    );
    assert!(
        mapping_surface
            .compare_signals
            .iter()
            .any(|signal| signal == "removed_assets")
    );
    assert!(
        projection
            .triggering_compare_signals
            .iter()
            .any(|signal| signal == "removed_assets")
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

fn sample_hook_sensitive_compare_report() -> SnapshotCompareReport {
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
            added_assets: 1,
            removed_assets: 1,
            changed_assets: 1,
            candidate_mapping_changes: 1,
            identity_changed_assets: 0,
            layout_changed_assets: 0,
            structural_changed_assets: 0,
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
        removed_assets: vec![SnapshotAssetChange {
            change_type: SnapshotChangeType::Removed,
            old_asset: Some(asset_summary("Content/Character/HeroA/Hair.mesh")),
            new_asset: None,
            changed_fields: vec!["path_presence".to_string()],
            probable_impact: RiskLevel::Medium,
            crash_risk: RiskLevel::Medium,
            suspected_mapping_change: true,
            lineage: whashreonator::compare::AssetLineageKind::ContainerMovement,
            reasons: vec![SnapshotCompareReason {
                code: "asset_removed".to_string(),
                message: "asset removed".to_string(),
            }],
        }],
        changed_assets: vec![SnapshotAssetChange {
            change_type: SnapshotChangeType::Changed,
            old_asset: Some(asset_summary("Content/Character/HeroA/Hair.mesh")),
            new_asset: Some(asset_summary("Content/Character/HeroA/Hair.mesh")),
            changed_fields: vec!["container_path".to_string(), "source_kind".to_string()],
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
            old_asset: asset_summary("Content/Character/HeroA/Hair.mesh"),
            new_asset: asset_summary("Content/Character/HeroA/Hair_LOD0.mesh"),
            confidence: 0.86,
            compatibility: RemapCompatibility::CompatibleWithCaution,
            lineage: whashreonator::compare::AssetLineageKind::ContainerMovement,
            reasons: vec![SnapshotCompareReason {
                code: "container_path_mismatch".to_string(),
                message: "container path changed".to_string(),
            }],
            runner_up_confidence: None,
            confidence_gap: Some(0.14),
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

fn sample_representative_baseline_set(version_id: &str) -> WwmiModDependencyBaselineSet {
    let mut baseline_set = build_mod_dependency_baseline_set(
        version_id,
        vec![
            WwmiModDependencyProfile {
                mod_name: Some("HashFocusedMod".to_string()),
                mod_root: "D:/mods/HashFocusedMod".to_string(),
                ini_file_count: 1,
                signals: vec![WwmiModDependencySignal {
                    kind: WwmiModDependencyKind::TextureOverrideHash,
                    value: "0xDEADBEEF".to_string(),
                    source_file: "HashFocusedMod.ini".to_string(),
                    section: Some("TextureOverrideBody".to_string()),
                }],
            },
            WwmiModDependencyProfile {
                mod_name: Some("BufferFocusedMod".to_string()),
                mod_root: "D:/mods/BufferFocusedMod".to_string(),
                ini_file_count: 1,
                signals: vec![WwmiModDependencySignal {
                    kind: WwmiModDependencyKind::BufferLayoutHint,
                    value: "override_byte_stride=32".to_string(),
                    source_file: "BufferFocusedMod.ini".to_string(),
                    section: Some("TextureOverrideBody".to_string()),
                }],
            },
            WwmiModDependencyProfile {
                mod_name: Some("ResourceSkeletonMod".to_string()),
                mod_root: "D:/mods/ResourceSkeletonMod".to_string(),
                ini_file_count: 1,
                signals: vec![
                    WwmiModDependencySignal {
                        kind: WwmiModDependencyKind::ResourceFileReference,
                        value: "BodyDiffuse.dds".to_string(),
                        source_file: "ResourceSkeletonMod.ini".to_string(),
                        section: Some("ResourceBody".to_string()),
                    },
                    WwmiModDependencySignal {
                        kind: WwmiModDependencyKind::SkeletonMergeDependency,
                        value: "run = CommandListMergeSkeleton".to_string(),
                        source_file: "ResourceSkeletonMod.ini".to_string(),
                        section: Some("CommandListMergeSkeleton".to_string()),
                    },
                ],
            },
            WwmiModDependencyProfile {
                mod_name: Some("HookFocusedMod".to_string()),
                mod_root: "D:/mods/HookFocusedMod".to_string(),
                ini_file_count: 1,
                signals: vec![
                    WwmiModDependencySignal {
                        kind: WwmiModDependencyKind::DrawCallTarget,
                        value: "match_first_index=100".to_string(),
                        source_file: "HookFocusedMod.ini".to_string(),
                        section: Some("TextureOverrideHair".to_string()),
                    },
                    WwmiModDependencySignal {
                        kind: WwmiModDependencyKind::FilterIndex,
                        value: "7".to_string(),
                        source_file: "HookFocusedMod.ini".to_string(),
                        section: Some("TextureOverrideHair".to_string()),
                    },
                ],
            },
        ],
    )
    .expect("build representative baseline set");
    baseline_set.generated_at_unix_ms = 1;
    baseline_set
}

fn asset_summary(path: &str) -> SnapshotAssetSummary {
    SnapshotAssetSummary {
        id: path.to_string(),
        path: path.to_string(),
        identity_tuple: None,
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
            identity_tuple: None,
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
                identity_tuple: None,
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
