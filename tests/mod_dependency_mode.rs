use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::ScanModDependenciesArgs,
    pipeline::run_scan_mod_dependencies_command,
    report_storage::{ReportStorage, VersionArtifactKind},
    wwmi::dependency::{
        WwmiModDependencyBaselineSet, WwmiModDependencyBaselineStrength, WwmiModDependencyKind,
        WwmiModDependencySurfaceClass, load_mod_dependency_baseline_set,
    },
};

#[test]
fn scan_mod_dependencies_command_exports_and_stores_curated_profiles() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let output_path = test_root.join("out").join("mod-baselines.json");
    let aemeth_root = test_root.join("mods").join("Aemeth");
    let carlotta_root = test_root.join("mods").join("CarlottaMod");

    write_mod_ini(
        &aemeth_root,
        "mod.ini",
        r#"
[TextureOverrideBody]
hash = 0xDEADBEEF

[ResourceBody]
filename = BodyDiffuse.dds
"#,
    );
    write_mod_ini(
        &carlotta_root,
        "mod.ini",
        r#"
[TextureOverrideHair]
hash = 0xABCD1234
match_first_index = 100
filter_index = 7
override_byte_stride = 32

[CommandListMergeSkeleton]
run = CommandListMergeSkeleton
"#,
    );

    let result = run_scan_mod_dependencies_command(&ScanModDependenciesArgs {
        version_id: "3.2.1".to_string(),
        mod_roots: vec![carlotta_root, aemeth_root],
        output: output_path.clone(),
        store_in_report: true,
        report_root: Some(report_root),
    })
    .expect("run mod dependency baseline command");

    let output = fs::read_to_string(&output_path).expect("read baseline output");
    let parsed: WwmiModDependencyBaselineSet =
        serde_json::from_str(&output).expect("parse baseline output");

    assert_eq!(
        parsed.schema_version,
        "whashreonator.wwmi-mod-dependency-baselines.v1"
    );
    assert_eq!(parsed.version_id, "3.2.1");
    assert_eq!(parsed.profile_count, 2);
    assert_eq!(parsed.profiles.len(), 2);
    assert_eq!(parsed.review.included_mod_count, 2);
    assert_eq!(
        parsed.review.strength,
        WwmiModDependencyBaselineStrength::Sparse
    );
    assert!(!parsed.review.material_for_repair_review);
    assert_eq!(
        parsed
            .represented_surface_classes()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [
            WwmiModDependencySurfaceClass::MappingHash,
            WwmiModDependencySurfaceClass::BufferLayout,
            WwmiModDependencySurfaceClass::ResourceSkeleton,
            WwmiModDependencySurfaceClass::DrawCallFilterHook,
        ]
        .into_iter()
        .collect()
    );
    assert!(
        parsed.review.caution_notes.iter().any(|note| {
            note.contains("reviewer guidance") || note.contains("not an exhaustive")
        })
    );
    assert_eq!(result.baseline_set.profile_count, 2);
    assert!(result.stored_baseline_set_path.is_some());
    assert!(result.stored_baseline_summary_path.is_some());
    assert_eq!(result.stored_profile_paths.len(), 2);

    let aemeth = parsed
        .profiles
        .iter()
        .find(|profile| profile.mod_name.as_deref() == Some("Aemeth"))
        .expect("Aemeth profile");
    assert!(aemeth.has_kind(WwmiModDependencyKind::TextureOverrideHash));
    assert!(aemeth.has_kind(WwmiModDependencyKind::ResourceFileReference));

    let carlotta = parsed
        .profiles
        .iter()
        .find(|profile| profile.mod_name.as_deref() == Some("CarlottaMod"))
        .expect("Carlotta profile");
    assert!(carlotta.has_kind(WwmiModDependencyKind::TextureOverrideHash));
    assert!(carlotta.has_kind(WwmiModDependencyKind::DrawCallTarget));
    assert!(carlotta.has_kind(WwmiModDependencyKind::FilterIndex));
    assert!(carlotta.has_kind(WwmiModDependencyKind::BufferLayoutHint));
    assert!(carlotta.has_kind(WwmiModDependencyKind::SkeletonMergeDependency));

    let artifacts = storage
        .list_version_artifacts("3.2.1")
        .expect("list stored artifacts");
    assert!(artifacts.iter().any(|artifact| {
        artifact.kind == VersionArtifactKind::Auxiliary
            && artifact
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("mod-dependency-baselines"))
    }));
    assert!(
        artifacts
            .iter()
            .filter(|artifact| {
                artifact.kind == VersionArtifactKind::Auxiliary
                    && artifact
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains("mod-dependency-profile"))
            })
            .count()
            >= 2
    );

    let summary_path = result
        .stored_baseline_summary_path
        .as_ref()
        .expect("stored baseline summary path");
    let summary = fs::read_to_string(summary_path).expect("read baseline summary");
    assert!(summary.contains("Target version: `3.2.1`"));
    assert!(summary.contains("Included profiles/mod roots: 2 / 2"));
    assert!(summary.contains(
        "Represented dependency surface classes: mapping_hash, buffer_layout, resource_skeleton, draw_call_filter_hook"
    ));
    assert!(summary.contains("Material for repair review: No"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn load_legacy_baseline_set_backfills_review_metadata_from_profiles() {
    let test_root = unique_test_dir();
    let baseline_path = test_root.join("legacy-baseline.json");
    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &baseline_path,
        serde_json::json!({
            "schema_version": "whashreonator.wwmi-mod-dependency-baselines.v1",
            "generated_at_unix_ms": 1,
            "version_id": "3.2.1",
            "profile_count": 2,
            "profiles": [
                {
                    "mod_name": "HashFocusedMod",
                    "mod_root": "D:/mods/HashFocusedMod",
                    "ini_file_count": 1,
                    "signals": [
                        {
                            "kind": "texture_override_hash",
                            "value": "0xDEADBEEF",
                            "source_file": "mod.ini",
                            "section": "TextureOverrideBody"
                        }
                    ]
                },
                {
                    "mod_name": "ResourceOnlyMod",
                    "mod_root": "D:/mods/ResourceOnlyMod",
                    "ini_file_count": 1,
                    "signals": [
                        {
                            "kind": "resource_file_reference",
                            "value": "BodyDiffuse.dds",
                            "source_file": "mod.ini",
                            "section": "ResourceBody"
                        }
                    ]
                }
            ]
        })
        .to_string(),
    )
    .expect("write legacy baseline");

    let parsed = load_mod_dependency_baseline_set(&baseline_path).expect("load legacy baseline");

    assert_eq!(parsed.review.included_mod_count, 2);
    assert_eq!(
        parsed
            .represented_surface_classes()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [
            WwmiModDependencySurfaceClass::MappingHash,
            WwmiModDependencySurfaceClass::ResourceSkeleton,
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        parsed.review.strength,
        WwmiModDependencyBaselineStrength::Sparse
    );
    assert!(!parsed.review.material_for_repair_review);
    assert!(!parsed.review.caution_notes.is_empty());

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn duplicate_mod_roots_do_not_inflate_curated_baseline_strength() {
    let test_root = unique_test_dir();
    let output_path = test_root.join("out").join("duplicate-mod-baselines.json");
    let hash_root = test_root.join("mods").join("HashFocusedMod");
    let buffer_root = test_root.join("mods").join("BufferFocusedMod");

    write_mod_ini(
        &hash_root,
        "mod.ini",
        r#"
[TextureOverrideBody]
hash = 0xDEADBEEF
"#,
    );
    write_mod_ini(
        &buffer_root,
        "mod.ini",
        r#"
[TextureOverrideHair]
override_byte_stride = 32
"#,
    );

    let result = run_scan_mod_dependencies_command(&ScanModDependenciesArgs {
        version_id: "3.2.1".to_string(),
        mod_roots: vec![
            hash_root.clone(),
            hash_root,
            buffer_root.clone(),
            buffer_root,
        ],
        output: output_path,
        store_in_report: false,
        report_root: None,
    })
    .expect("run mod dependency baseline command with duplicates");

    assert_eq!(result.baseline_set.profile_count, 2);
    assert_eq!(result.baseline_set.profiles.len(), 2);
    assert_eq!(result.baseline_set.review.included_mod_count, 2);
    assert_eq!(
        result.baseline_set.review.strength,
        WwmiModDependencyBaselineStrength::Sparse
    );
    assert!(!result.baseline_set.review.material_for_repair_review);
    assert!(result.baseline_set.review.caution_notes.iter().any(|note| {
        note.contains("sample stays sparse with only 2 included profile(s)/mod root(s)")
    }));

    let _ = fs::remove_dir_all(&test_root);
}

fn write_mod_ini(root: &Path, relative_path: &str, content: &str) {
    let full_path = root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create mod ini parent");
    }

    fs::write(full_path, content.trim()).expect("write mod ini");
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-mod-dependency-mode-test-{nanos}"))
}
