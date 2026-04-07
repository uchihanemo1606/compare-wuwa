use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::{CompareSnapshotsArgs, SnapshotArgs},
    compare::SnapshotCompareReport,
    pipeline::{run_compare_snapshots_command, run_snapshot_command},
};

#[test]
fn compare_snapshots_command_exports_change_categories() {
    let test_root = unique_test_dir();
    let old_root = test_root.join("old");
    let new_root = test_root.join("new");
    let old_snapshot_path = test_root.join("snapshots").join("old.json");
    let new_snapshot_path = test_root.join("snapshots").join("new.json");
    let compare_output_path = test_root.join("out").join("compare.json");

    seed_local_asset(&old_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&old_root, "Content/Weapon/Sword.weapon");
    seed_local_asset(&new_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&new_root, "Content/Weapon/Sword_v2.weapon");

    run_snapshot_command(&SnapshotArgs {
        source_root: old_root,
        version_id: "2.4.0".to_string(),
        output: old_snapshot_path.clone(),
    })
    .expect("create old snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: new_root,
        version_id: "2.5.0".to_string(),
        output: new_snapshot_path.clone(),
    })
    .expect("create new snapshot");

    let report = run_compare_snapshots_command(&CompareSnapshotsArgs {
        old_snapshot: old_snapshot_path,
        new_snapshot: new_snapshot_path,
        output: compare_output_path.clone(),
    })
    .expect("run compare snapshots command");

    let output = fs::read_to_string(&compare_output_path).expect("read compare output");
    let parsed: SnapshotCompareReport =
        serde_json::from_str(&output).expect("parse compare report");

    assert_eq!(report.schema_version, "whashreonator.snapshot-compare.v1");
    assert_eq!(parsed.old_snapshot.version_id, "2.4.0");
    assert_eq!(parsed.new_snapshot.version_id, "2.5.0");
    assert_eq!(parsed.summary.removed_assets, 1);
    assert_eq!(parsed.summary.added_assets, 1);
    assert_eq!(parsed.summary.changed_assets, 0);
    assert_eq!(parsed.summary.candidate_mapping_changes, 1);

    let _ = fs::remove_dir_all(&test_root);
}

fn seed_local_asset(root: &Path, relative_path: &str) {
    let full_path = root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create asset directory");
    }

    fs::write(full_path, b"asset").expect("write asset file");
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-compare-mode-test-{nanos}"))
}
