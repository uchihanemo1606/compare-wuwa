use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::ScanModDependenciesArgs,
    pipeline::run_scan_mod_dependencies_command,
    report_storage::{ReportStorage, VersionArtifactKind},
    wwmi::dependency::{WwmiModDependencyBaselineSet, WwmiModDependencyKind},
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
    assert_eq!(result.baseline_set.profile_count, 2);
    assert!(result.stored_baseline_set_path.is_some());
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
