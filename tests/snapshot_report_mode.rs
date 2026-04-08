use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::{SnapshotArgs, SnapshotReportArgs},
    pipeline::{run_snapshot_command, run_snapshot_report_command},
};

#[test]
fn snapshot_report_command_exports_markdown_tables() {
    let test_root = unique_test_dir();
    let output_path = test_root.join("out").join("snapshot-report.md");
    let old_snapshot = PathBuf::from("tests/fixtures/version-regression/old_snapshot.json");
    let new_snapshot = PathBuf::from("tests/fixtures/version-regression/new_snapshot.json");

    let report = run_snapshot_report_command(&SnapshotReportArgs {
        snapshots: vec![old_snapshot, new_snapshot],
        output: output_path.clone(),
    })
    .expect("run snapshot report command");

    let output = fs::read_to_string(&output_path).expect("read snapshot report");

    assert_eq!(report.version_count, 2);
    assert_eq!(report.resonator_count, 1);
    assert_eq!(report.pair_count, 1);
    assert!(output.contains("# Snapshot Report"));
    assert!(output.contains("## Version Summary"));
    assert!(output.contains("## Scope & Coverage"));
    assert!(output.contains("## Analysis Limitations"));
    assert!(output.contains("| Version | Reuse Version | Total Assets | Resonators | Character Assets | Other Assets | Source Root |"));
    assert!(output.contains("| 2.4.0 | - | 3 | 1 | 2 | 1 | fixtures/2.4.0 |"));
    assert!(output.contains("## Resonator Matrix"));
    assert!(output.contains("| Encore | 2 | 2 |"));
    assert!(output.contains("### 2.4.0 -> 2.5.0"));
    assert!(output.contains("Scope note: this pair includes"));
    assert!(output.contains("| Changed resonators | Encore |"));
    assert!(output.contains("#### Candidate Remaps"));
    assert!(output.contains("Content/Character/Encore/Hair.mesh"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn snapshot_report_flags_install_level_snapshots_as_low_signal() {
    let test_root = unique_test_dir();
    let old_root = test_root.join("old-game");
    let new_root = test_root.join("new-game");
    let old_snapshot = test_root.join("out").join("old.snapshot.json");
    let new_snapshot = test_root.join("out").join("new.snapshot.json");
    let report_output = test_root.join("out").join("snapshot-report-low-signal.md");

    seed_local_asset(
        &old_root,
        "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak",
    );
    seed_local_asset(
        &new_root,
        "Client/Content/Paks/pakchunk1-WindowsNoEditor.pak",
    );

    run_snapshot_command(&SnapshotArgs {
        source_root: old_root,
        version_id: "3.0.0".to_string(),
        output: old_snapshot.clone(),
    })
    .expect("export old snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: new_root,
        version_id: "3.1.0".to_string(),
        output: new_snapshot.clone(),
    })
    .expect("export new snapshot");

    run_snapshot_report_command(&SnapshotReportArgs {
        snapshots: vec![old_snapshot, new_snapshot],
        output: report_output.clone(),
    })
    .expect("run snapshot report command");

    let output = fs::read_to_string(&report_output).expect("read output");

    assert!(output.contains("## Version Summary"));
    assert!(output.contains("## Analysis Limitations"));
    assert!(output.contains(
        "install/package-level or low-coverage snapshot; resonator-level and mapping-level interpretation can be incomplete."
    ));
    assert!(output.contains("## Version-to-Version Changes"));

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

    std::env::temp_dir().join(format!("whashreonator-snapshot-report-mode-test-{nanos}"))
}
