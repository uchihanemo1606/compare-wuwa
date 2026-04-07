use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::InferFixesArgs,
    compare::{
        CandidateMappingChange, RiskLevel, SnapshotAssetChange, SnapshotAssetSummary,
        SnapshotChangeType, SnapshotCompareReason, SnapshotCompareReport, SnapshotCompareSummary,
        SnapshotVersionInfo,
    },
    inference::InferenceReport,
    pipeline::run_infer_fixes_command,
    wwmi::{
        WwmiEvidenceCommit, WwmiFixPattern, WwmiKeywordStat, WwmiKnowledgeBase,
        WwmiKnowledgeRepoInfo, WwmiKnowledgeSummary, WwmiPatternKind,
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
        normalized_name: Some("asset".to_string()),
        vertex_count: Some(1000),
        index_count: Some(2000),
        material_slots: Some(1),
        section_count: Some(1),
        asset_hash: None,
        shader_hash: None,
    }
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-inference-mode-test-{nanos}"))
}
