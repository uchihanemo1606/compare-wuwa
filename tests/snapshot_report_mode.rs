use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{cli::SnapshotReportArgs, pipeline::run_snapshot_report_command};

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
    assert!(output.contains("| Version | Reuse Version | Total Assets | Resonators | Character Assets | Other Assets | Source Root |"));
    assert!(output.contains("| 2.4.0 | - | 3 | 1 | 2 | 1 | fixtures/2.4.0 |"));
    assert!(output.contains("## Resonator Matrix"));
    assert!(output.contains("| Encore | 2 | 2 |"));
    assert!(output.contains("### 2.4.0 -> 2.5.0"));
    assert!(output.contains("| Changed resonators | Encore |"));
    assert!(output.contains("#### Candidate Remaps"));
    assert!(output.contains("Content/Character/Encore/Hair.mesh"));

    let _ = fs::remove_dir_all(&test_root);
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-snapshot-report-mode-test-{nanos}"))
}
