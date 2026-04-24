use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::{OrchestrateVersionPairArgs, QualityGateModeArg},
    compare::SnapshotCompareReport,
    error::AppError,
    inference::{InferenceReport, RepresentativeModRiskClass},
    pipeline::{VersionPairRunManifest, run_orchestrate_version_pair_command},
    proposal::MappingProposalOutput,
    report_storage::{ReportStorage, VersionArtifactKind},
    snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotCoverageSignals,
        SnapshotExtractorContext, SnapshotFingerprint, SnapshotHashFields, SnapshotLauncherContext,
        SnapshotResourceManifestContext, SnapshotScopeContext,
    },
    wwmi::{
        WwmiEvidenceCommit, WwmiFixPattern, WwmiKeywordStat, WwmiKnowledgeBase,
        WwmiKnowledgeRepoInfo, WwmiKnowledgeSummary, WwmiPatternKind,
        dependency::{
            WwmiModDependencyBaselineSet, WwmiModDependencyKind, WwmiModDependencyProfile,
            WwmiModDependencySignal, build_mod_dependency_baseline_set,
        },
    },
};

#[test]
fn orchestrate_version_pair_command_produces_expected_artifacts_and_manifest() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let output_dir = test_root.join("out").join("version-pair-run");
    let knowledge_path = test_root.join("knowledge.json");

    storage
        .save_snapshot_for_version(&shallow_snapshot("8.0.0", 120))
        .expect("save old snapshot");
    storage
        .save_snapshot_for_version(&empty_snapshot("8.1.0"))
        .expect("save new snapshot");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge");

    let result = run_orchestrate_version_pair_command(&OrchestrateVersionPairArgs {
        old_version_id: "8.0.0".to_string(),
        new_version_id: "8.1.0".to_string(),
        wwmi_knowledge: knowledge_path,
        output_dir: output_dir.clone(),
        report_root: Some(report_root),
        compare_output: None,
        inference_output: None,
        mapping_output: None,
        patch_draft_output: None,
        summary_output: None,
        manifest_output: None,
        min_confidence: 0.85,
        quality_gate_mode: QualityGateModeArg::Advisory,
    })
    .expect("run version-pair orchestration");

    let manifest_path = output_dir.join("run-manifest.v1.json");
    let compare_path = output_dir.join("compare.v1.json");
    let inference_path = output_dir.join("inference.v1.json");
    let mapping_path = output_dir.join("mapping-proposal.v1.json");
    let patch_path = output_dir.join("proposal-patch-draft.v1.json");
    let summary_path = output_dir.join("human-summary.md");

    for path in [
        &manifest_path,
        &compare_path,
        &inference_path,
        &mapping_path,
        &patch_path,
        &summary_path,
    ] {
        assert!(path.exists(), "expected output {}", path.display());
    }

    let manifest: VersionPairRunManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest output"))
            .expect("parse manifest output");
    let compare: SnapshotCompareReport =
        serde_json::from_str(&fs::read_to_string(&compare_path).expect("read compare output"))
            .expect("parse compare output");
    let inference: InferenceReport =
        serde_json::from_str(&fs::read_to_string(&inference_path).expect("read inference output"))
            .expect("parse inference output");
    let mapping: MappingProposalOutput =
        serde_json::from_str(&fs::read_to_string(&mapping_path).expect("read mapping output"))
            .expect("parse mapping output");

    assert_eq!(result.manifest, manifest);
    assert_eq!(manifest.old_version_id, "8.0.0");
    assert_eq!(manifest.new_version_id, "8.1.0");
    assert_eq!(manifest.quality_gate.mode, "advisory");
    assert_eq!(manifest.quality_gate.status, "block");
    assert!(!manifest.quality_gate.passed);
    assert!(manifest.quality_gate.key_signals.compare_low_signal);
    assert_eq!(manifest.produced_artifacts.run_directory, output_dir);
    assert_eq!(manifest.produced_artifacts.compare_report, compare_path);
    assert_eq!(manifest.produced_artifacts.inference_report, inference_path);
    assert_eq!(manifest.produced_artifacts.mapping_proposal, mapping_path);
    assert_eq!(manifest.produced_artifacts.patch_draft, patch_path);
    assert_eq!(manifest.produced_artifacts.human_summary, summary_path);
    assert_eq!(manifest.produced_artifacts.manifest, manifest_path);
    assert_eq!(
        manifest.summary.changed_assets,
        compare.summary.changed_assets
    );
    assert_eq!(manifest.summary.added_assets, compare.summary.added_assets);
    assert_eq!(
        manifest.summary.removed_assets,
        compare.summary.removed_assets
    );
    assert_eq!(
        manifest.summary.candidate_mapping_changes,
        compare.summary.candidate_mapping_changes
    );
    assert_eq!(
        manifest.summary.probable_crash_causes,
        inference.summary.probable_crash_causes
    );
    assert_eq!(
        manifest.summary.suggested_fixes,
        inference.summary.suggested_fixes
    );
    assert_eq!(
        manifest.summary.candidate_mapping_hints,
        inference.summary.candidate_mapping_hints
    );
    assert_eq!(
        manifest.summary.proposed_mappings,
        mapping.summary.proposed_mappings
    );
    assert_eq!(
        manifest.summary.needs_review_mappings,
        mapping.summary.needs_review_mappings
    );
    assert_eq!(
        manifest.summary.suggested_fix_actions,
        mapping.summary.suggested_fix_actions
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn orchestrate_version_pair_command_manifest_preserves_trust_and_recency_aware_baseline_selection()
{
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let knowledge_path = test_root.join("knowledge.json");
    let output_dir = test_root.join("out").join("baseline-selection-run");
    let old_version = "8.2.0";
    let new_version = "8.3.0";

    let canonical_old_path = storage
        .save_snapshot_for_version(&shallow_snapshot(old_version, 100))
        .expect("save canonical old snapshot");
    let richer_older_path = alternate_snapshot_path(&storage, old_version, "zzz-older-rich");
    write_snapshot(
        &richer_older_path,
        &rich_extractor_snapshot(old_version, 140),
    )
    .expect("write older rich snapshot");

    thread::sleep(Duration::from_millis(1200));

    let richer_newer_path = alternate_snapshot_path(&storage, old_version, "aaa-newer-rich");
    write_snapshot(
        &richer_newer_path,
        &rich_extractor_snapshot(old_version, 160),
    )
    .expect("write newer rich snapshot");
    storage
        .save_snapshot_for_version(&shallow_snapshot(new_version, 200))
        .expect("save new snapshot");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge");

    let result = run_orchestrate_version_pair_command(&OrchestrateVersionPairArgs {
        old_version_id: old_version.to_string(),
        new_version_id: new_version.to_string(),
        wwmi_knowledge: knowledge_path,
        output_dir: output_dir.clone(),
        report_root: Some(report_root),
        compare_output: None,
        inference_output: None,
        mapping_output: None,
        patch_draft_output: None,
        summary_output: None,
        manifest_output: None,
        min_confidence: 0.85,
        quality_gate_mode: QualityGateModeArg::Advisory,
    })
    .expect("run orchestration with richer alternate baseline");

    assert_eq!(
        canonical_old_path,
        storage.snapshot_path_for_version(old_version)
    );
    assert_eq!(
        result.manifest.selected_old_baseline.path,
        richer_newer_path,
    );
    assert_eq!(
        result.manifest.selected_old_baseline.artifact_kind,
        format!("{:?}", VersionArtifactKind::Snapshot)
    );
    assert_eq!(
        result.manifest.selected_old_baseline.evidence_posture,
        "extractor_backed_rich"
    );
    assert_eq!(
        result.manifest.selected_old_baseline.inventory_alignment,
        "aligned"
    );
    assert!(
        result
            .manifest
            .selected_old_baseline
            .selection_reason
            .contains("extractor-backed, version-aligned")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn orchestrate_version_pair_command_keeps_scope_induced_behavior_conservative_downstream() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let knowledge_path = test_root.join("knowledge.json");
    let output_dir = test_root.join("out").join("scope-induced-run");
    let old_version = "8.4.0";
    let new_version = "8.4.1";

    storage
        .save_snapshot_for_version(&scope_full_snapshot(old_version))
        .expect("save old scope snapshot");
    storage
        .save_snapshot_for_version(&scope_character_snapshot(new_version))
        .expect("save new scope snapshot");
    storage
        .save_mod_dependency_baseline_set_for_version(
            old_version,
            &sample_representative_baseline_set(old_version),
        )
        .expect("save representative baseline");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge");

    run_orchestrate_version_pair_command(&OrchestrateVersionPairArgs {
        old_version_id: old_version.to_string(),
        new_version_id: new_version.to_string(),
        wwmi_knowledge: knowledge_path,
        output_dir: output_dir.clone(),
        report_root: Some(report_root),
        compare_output: None,
        inference_output: None,
        mapping_output: None,
        patch_draft_output: None,
        summary_output: None,
        manifest_output: None,
        min_confidence: 0.85,
        quality_gate_mode: QualityGateModeArg::Advisory,
    })
    .expect("run scope-induced orchestration");

    let compare: SnapshotCompareReport = serde_json::from_str(
        &fs::read_to_string(output_dir.join("compare.v1.json")).expect("read compare output"),
    )
    .expect("parse compare output");
    let inference: InferenceReport = serde_json::from_str(
        &fs::read_to_string(output_dir.join("inference.v1.json")).expect("read inference output"),
    )
    .expect("parse inference output");

    assert!(compare.scope.scope_narrowing_detected);
    assert!(compare.scope.scope_induced_removals_likely);
    assert!(
        inference
            .surface_intersection
            .overlapping_surface_classes
            .is_empty()
    );
    assert!(inference.representative_risk_projections.iter().all(
        |projection| projection.risk_class != RepresentativeModRiskClass::MappingHashSensitive
    ));
    assert!(
        inference
            .probable_crash_causes
            .iter()
            .all(|cause| cause.code != "asset_removed_without_clear_replacement")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn orchestrate_version_pair_command_auto_loads_old_version_representative_baseline() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let knowledge_path = test_root.join("knowledge.json");
    let output_dir = test_root.join("out").join("representative-auto-load-run");
    let old_version = "8.5.0";
    let new_version = "8.6.0";

    storage
        .save_snapshot_for_version(&shallow_snapshot(old_version, 120))
        .expect("save old snapshot");
    storage
        .save_snapshot_for_version(&empty_snapshot(new_version))
        .expect("save new snapshot");
    storage
        .save_mod_dependency_baseline_set_for_version(
            old_version,
            &sample_representative_baseline_set(old_version),
        )
        .expect("save old representative baseline");
    storage
        .save_mod_dependency_baseline_set_for_version(
            new_version,
            &sample_representative_baseline_set(new_version),
        )
        .expect("save new representative baseline");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge");

    run_orchestrate_version_pair_command(&OrchestrateVersionPairArgs {
        old_version_id: old_version.to_string(),
        new_version_id: new_version.to_string(),
        wwmi_knowledge: knowledge_path,
        output_dir: output_dir.clone(),
        report_root: Some(report_root),
        compare_output: None,
        inference_output: None,
        mapping_output: None,
        patch_draft_output: None,
        summary_output: None,
        manifest_output: None,
        min_confidence: 0.85,
        quality_gate_mode: QualityGateModeArg::Advisory,
    })
    .expect("run representative auto-load orchestration");

    let inference: InferenceReport = serde_json::from_str(
        &fs::read_to_string(output_dir.join("inference.v1.json")).expect("read inference output"),
    )
    .expect("parse inference output");
    let representative = inference
        .representative_mod_baseline_input
        .as_ref()
        .expect("representative baseline input");

    assert_eq!(representative.version_id, old_version);

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn orchestrate_version_pair_command_exports_readiness_diagnostics_for_low_signal_baselines() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let knowledge_path = test_root.join("knowledge.json");
    let output_dir = test_root.join("out").join("readiness-run");
    let old_version = "8.7.0";
    let new_version = "8.8.0";

    storage
        .save_snapshot_for_version(&shallow_snapshot(old_version, 80))
        .expect("save old shallow snapshot");
    storage
        .save_snapshot_for_version(&empty_snapshot(new_version))
        .expect("save new empty snapshot");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge");

    run_orchestrate_version_pair_command(&OrchestrateVersionPairArgs {
        old_version_id: old_version.to_string(),
        new_version_id: new_version.to_string(),
        wwmi_knowledge: knowledge_path,
        output_dir: output_dir.clone(),
        report_root: Some(report_root),
        compare_output: None,
        inference_output: None,
        mapping_output: None,
        patch_draft_output: None,
        summary_output: None,
        manifest_output: None,
        min_confidence: 0.85,
        quality_gate_mode: QualityGateModeArg::Advisory,
    })
    .expect("run low-signal readiness orchestration");

    let manifest: VersionPairRunManifest = serde_json::from_str(
        &fs::read_to_string(output_dir.join("run-manifest.v1.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    let summary =
        fs::read_to_string(output_dir.join("human-summary.md")).expect("read human summary");

    assert!(manifest.readiness.compare_low_signal);
    assert!(
        manifest
            .selected_old_baseline
            .low_signal_for_character_analysis
    );
    assert!(
        manifest
            .selected_new_baseline
            .low_signal_for_character_analysis
    );
    assert!(
        manifest
            .selected_new_baseline
            .readiness_reasons
            .iter()
            .any(|reason| reason.contains("content-like coverage is still below"))
    );
    assert!(
        manifest
            .readiness
            .reasons
            .iter()
            .any(|reason| reason.contains("compare readiness is low-signal"))
    );
    assert!(
        manifest
            .readiness
            .downstream_guardrails
            .iter()
            .any(|reason| reason.contains("review-first"))
    );
    assert!(summary.contains("Readiness: low-signal compare scope"));
    assert!(summary.contains("Low-signal reason:"));
    assert!(summary.contains("manifest/hash coverage"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn orchestrate_version_pair_command_manifest_exports_quality_gate_signals() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let knowledge_path = test_root.join("knowledge.json");
    let output_dir = test_root.join("out").join("quality-gate-advisory-run");
    let old_version = "8.9.0";
    let new_version = "8.9.1";

    storage
        .save_snapshot_for_version(&scope_full_snapshot(old_version))
        .expect("save old scope snapshot");
    storage
        .save_snapshot_for_version(&scope_character_snapshot(new_version))
        .expect("save new scope snapshot");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge");

    let result = run_orchestrate_version_pair_command(&OrchestrateVersionPairArgs {
        old_version_id: old_version.to_string(),
        new_version_id: new_version.to_string(),
        wwmi_knowledge: knowledge_path,
        output_dir: output_dir.clone(),
        report_root: Some(report_root),
        compare_output: None,
        inference_output: None,
        mapping_output: None,
        patch_draft_output: None,
        summary_output: None,
        manifest_output: None,
        min_confidence: 0.85,
        quality_gate_mode: QualityGateModeArg::Advisory,
    })
    .expect("run quality gate advisory orchestration");

    let manifest: VersionPairRunManifest = serde_json::from_str(
        &fs::read_to_string(output_dir.join("run-manifest.v1.json")).expect("read manifest"),
    )
    .expect("parse manifest");

    assert_eq!(result.manifest, manifest);
    assert_eq!(manifest.quality_gate.mode, "advisory");
    assert_eq!(manifest.quality_gate.status, "block");
    assert!(!manifest.quality_gate.passed);
    assert!(manifest.quality_gate.key_signals.compare_low_signal);
    assert!(manifest.quality_gate.key_signals.scope_narrowing_detected);
    assert!(
        manifest
            .quality_gate
            .key_signals
            .scope_induced_removals_likely
    );
    assert!(
        manifest
            .quality_gate
            .reasons
            .iter()
            .any(|reason| reason.contains("scope-induced removals are likely"))
    );

    for path in [
        output_dir.join("compare.v1.json"),
        output_dir.join("inference.v1.json"),
        output_dir.join("mapping-proposal.v1.json"),
        output_dir.join("proposal-patch-draft.v1.json"),
        output_dir.join("human-summary.md"),
        output_dir.join("run-manifest.v1.json"),
    ] {
        assert!(path.exists(), "expected output {}", path.display());
    }

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn orchestrate_version_pair_command_enforce_mode_returns_error_after_persisting_audit_artifacts() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let knowledge_path = test_root.join("knowledge.json");
    let output_dir = test_root.join("out").join("quality-gate-enforce-run");
    let old_version = "9.0.0";
    let new_version = "9.0.1";

    storage
        .save_snapshot_for_version(&scope_full_snapshot(old_version))
        .expect("save old scope snapshot");
    storage
        .save_snapshot_for_version(&scope_character_snapshot(new_version))
        .expect("save new scope snapshot");
    fs::write(
        &knowledge_path,
        serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
    )
    .expect("write knowledge");

    let error = run_orchestrate_version_pair_command(&OrchestrateVersionPairArgs {
        old_version_id: old_version.to_string(),
        new_version_id: new_version.to_string(),
        wwmi_knowledge: knowledge_path,
        output_dir: output_dir.clone(),
        report_root: Some(report_root),
        compare_output: None,
        inference_output: None,
        mapping_output: None,
        patch_draft_output: None,
        summary_output: None,
        manifest_output: None,
        min_confidence: 0.85,
        quality_gate_mode: QualityGateModeArg::Enforce,
    })
    .expect_err("enforce mode should block low-quality pair");

    match error {
        AppError::InvalidInput(message) => {
            assert!(message.contains("quality gate blocked orchestrate-version-pair"));
            assert!(message.contains("status=block"));
        }
        other => panic!("unexpected error: {other}"),
    }

    let manifest: VersionPairRunManifest = serde_json::from_str(
        &fs::read_to_string(output_dir.join("run-manifest.v1.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    assert_eq!(manifest.quality_gate.mode, "enforce");
    assert_eq!(manifest.quality_gate.status, "block");
    assert!(!manifest.quality_gate.passed);
    assert!(manifest.readiness.reasons.is_empty());
    assert_eq!(manifest.summary.probable_crash_causes, 0);
    assert_eq!(manifest.summary.suggested_fixes, 0);
    assert_eq!(manifest.summary.candidate_mapping_hints, 0);
    assert_eq!(manifest.summary.proposed_mappings, 0);
    assert_eq!(manifest.summary.needs_review_mappings, 0);
    assert_eq!(manifest.summary.suggested_fix_actions, 0);
    assert!(output_dir.join("compare.v1.json").exists());
    assert!(output_dir.join("run-manifest.v1.json").exists());
    assert!(!output_dir.join("inference.v1.json").exists());
    assert!(!output_dir.join("mapping-proposal.v1.json").exists());
    assert!(!output_dir.join("proposal-patch-draft.v1.json").exists());
    assert!(!output_dir.join("human-summary.md").exists());

    let _ = fs::remove_dir_all(&test_root);
}

fn alternate_snapshot_path(storage: &ReportStorage, version_id: &str, suffix: &str) -> PathBuf {
    storage
        .build_version_directory(version_id)
        .join("snapshot")
        .join(format!("wuwa_{}.{}.snapshot.v1.json", version_id, suffix))
}

fn write_snapshot(path: &Path, snapshot: &GameSnapshot) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(snapshot)?)?;
    Ok(())
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

fn shallow_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 1,
        assets: vec![asset(
            "Content/Character/Encore/Body.mesh",
            vertex_count,
            false,
        )],
        context: SnapshotContext {
            launcher: Some(SnapshotLauncherContext {
                source_file: "launcherDownloadConfig.json".to_string(),
                detected_version: version_id.to_string(),
                reuse_version: None,
                state: Some("ready".to_string()),
                is_pre_download: false,
                app_id: Some("50004".to_string()),
            }),
            resource_manifest: Some(SnapshotResourceManifestContext {
                source_file: "LocalGameResources.json".to_string(),
                resource_count: 1,
                matched_assets: 1,
                unmatched_snapshot_assets: 0,
            }),
            extractor: None,
            scope: SnapshotScopeContext {
                acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                capture_mode: Some("local_filesystem_inventory_content_focused".to_string()),
                mostly_install_or_package_level: Some(true),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(false),
                meaningful_asset_record_enrichment: Some(false),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 1,
                    character_path_count: 1,
                    non_content_path_count: 0,
                },
                note: Some("fixture shallow fallback baseline".to_string()),
            },
            notes: Vec::new(),
        },
    }
}

fn scope_full_snapshot(version_id: &str) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 1,
        assets: vec![asset("Content/Character/Encore/Body.mesh", 120, false)],
        context: SnapshotContext {
            launcher: None,
            resource_manifest: None,
            extractor: None,
            scope: SnapshotScopeContext {
                acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                capture_mode: Some("local_filesystem_inventory".to_string()),
                mostly_install_or_package_level: Some(true),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(true),
                meaningful_asset_record_enrichment: Some(false),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 1,
                    character_path_count: 1,
                    non_content_path_count: 0,
                },
                note: Some("fixture full scope snapshot".to_string()),
            },
            notes: Vec::new(),
        },
    }
}

fn scope_character_snapshot(version_id: &str) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 0,
        assets: Vec::new(),
        context: SnapshotContext {
            launcher: None,
            resource_manifest: None,
            extractor: None,
            scope: SnapshotScopeContext {
                acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                capture_mode: Some("local_filesystem_inventory_character_focused".to_string()),
                mostly_install_or_package_level: Some(true),
                meaningful_content_coverage: Some(false),
                meaningful_character_coverage: Some(false),
                meaningful_asset_record_enrichment: Some(false),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 0,
                    character_path_count: 0,
                    non_content_path_count: 0,
                },
                note: Some(
                    "fixture character-focused snapshot with zero visible assets".to_string(),
                ),
            },
            notes: Vec::new(),
        },
    }
}

fn empty_snapshot(version_id: &str) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 0,
        assets: Vec::new(),
        context: SnapshotContext::default(),
    }
}

fn rich_extractor_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 1,
        assets: vec![asset(
            "Content/Character/Encore/Body.mesh",
            vertex_count,
            true,
        )],
        context: SnapshotContext {
            launcher: Some(SnapshotLauncherContext {
                source_file: "launcherDownloadConfig.json".to_string(),
                detected_version: version_id.to_string(),
                reuse_version: None,
                state: Some("ready".to_string()),
                is_pre_download: false,
                app_id: Some("50004".to_string()),
            }),
            resource_manifest: Some(SnapshotResourceManifestContext {
                source_file: "LocalGameResources.json".to_string(),
                resource_count: 1,
                matched_assets: 1,
                unmatched_snapshot_assets: 0,
            }),
            extractor: Some(SnapshotExtractorContext {
                inventory_path: Some("fixture-rich.json".to_string()),
                inventory_schema_version: Some("whashreonator.prepared-assets.v1".to_string()),
                inventory_version_id: Some(version_id.to_string()),
                inventory_version_matches_snapshot: Some(true),
                launcher_version_matches_inventory: Some(true),
                extraction_tool: Some("fixture-extractor".to_string()),
                extraction_kind: Some("asset_records".to_string()),
                inventory_source_root: Some("D:/fixture".to_string()),
                tags: vec!["fixture".to_string()],
                note: Some("aligned rich extractor fixture".to_string()),
                record_count: 1,
                records_with_hashes: 1,
                records_with_source_context: 1,
                records_with_rich_metadata: 1,
            }),
            scope: SnapshotScopeContext {
                acquisition_kind: Some("extractor_backed_asset_records".to_string()),
                capture_mode: Some("extractor_backed_asset_records".to_string()),
                mostly_install_or_package_level: Some(false),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(true),
                meaningful_asset_record_enrichment: Some(true),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 12,
                    character_path_count: 6,
                    non_content_path_count: 0,
                },
                note: Some("fixture rich extractor baseline".to_string()),
            },
            notes: Vec::new(),
        },
    }
}

fn asset(path: &str, vertex_count: u32, enriched: bool) -> SnapshotAsset {
    SnapshotAsset {
        id: path.to_string(),
        path: path.to_string(),
        identity_tuple: None,
        kind: Some("mesh".to_string()),
        metadata: whashreonator::domain::AssetMetadata {
            logical_name: Some("Encore Body".to_string()),
            vertex_count: Some(vertex_count),
            index_count: Some(vertex_count * 2),
            material_slots: Some(2),
            section_count: Some(1),
            layout_markers: if enriched {
                vec!["skinned".to_string(), "interleaved".to_string()]
            } else {
                Vec::new()
            },
            tags: vec!["character".to_string()],
            ..Default::default()
        },
        fingerprint: SnapshotFingerprint {
            normalized_kind: Some("mesh".to_string()),
            normalized_name: Some("encore body".to_string()),
            name_tokens: vec!["encore".to_string(), "body".to_string()],
            path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
            tags: vec!["character".to_string()],
            vertex_count: Some(vertex_count),
            index_count: Some(vertex_count * 2),
            material_slots: Some(2),
            section_count: Some(1),
            vertex_stride: if enriched { Some(32) } else { None },
            vertex_buffer_count: if enriched { Some(1) } else { None },
            index_format: if enriched {
                Some("u16".to_string())
            } else {
                None
            },
            primitive_topology: if enriched {
                Some("triangle_list".to_string())
            } else {
                None
            },
            layout_markers: if enriched {
                vec!["skinned".to_string(), "interleaved".to_string()]
            } else {
                Vec::new()
            },
            ..Default::default()
        },
        hash_fields: SnapshotHashFields {
            asset_hash: Some(format!("hash-{vertex_count}")),
            shader_hash: if enriched {
                Some("shader-shared".to_string())
            } else {
                None
            },
            signature: Some(format!("sig-{vertex_count}")),
            identity_tuple: None,
        },
        source: if enriched {
            whashreonator::domain::AssetSourceContext {
                extraction_tool: Some("fixture-extractor".to_string()),
                source_root: Some("D:/fixture".to_string()),
                source_path: Some(path.to_string()),
                container_path: Some("pakchunk0-WindowsNoEditor.pak".to_string()),
                source_kind: Some("mesh_record".to_string()),
            }
        } else {
            Default::default()
        },
    }
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "whashreonator-version-pair-orchestration-test-{nanos}"
    ))
}
