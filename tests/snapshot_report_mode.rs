use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use whashreonator::{
    cli::{SnapshotArgs, SnapshotCaptureScopeArg, SnapshotReportArgs},
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
    assert!(output.contains("## Capture Quality"));
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
        capture_scope: SnapshotCaptureScopeArg::Full,
        extractor_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("export old snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: new_root,
        version_id: "3.1.0".to_string(),
        output: new_snapshot.clone(),
        capture_scope: SnapshotCaptureScopeArg::Full,
        extractor_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("export new snapshot");

    run_snapshot_report_command(&SnapshotReportArgs {
        snapshots: vec![old_snapshot, new_snapshot],
        output: report_output.clone(),
    })
    .expect("run snapshot report command");

    let output = fs::read_to_string(&report_output).expect("read output");

    assert!(output.contains("## Version Summary"));
    assert!(output.contains("## Capture Quality"));
    assert!(output.contains("## Analysis Limitations"));
    assert!(output.contains("| 3.0.0 | shallow_filesystem_inventory | local_filesystem_inventory | shallow support only | yes | missing | missing | asset_hashes=0/1 any_hashes=0/1 signatures=0/1 | source_context=0/1 rich_metadata=0/1 enriched_assets=0/1 |"));
    assert!(output.contains(
        "shallow filesystem inventory or low-coverage/low-enrichment extractor snapshot; resonator-level and mapping-level interpretation can be incomplete."
    ));
    assert!(output.contains("## Version-to-Version Changes"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn snapshot_report_marks_sparse_extractor_snapshots_as_partial_and_low_signal() {
    let test_root = unique_test_dir();
    let old_root = test_root.join("old-game");
    let new_root = test_root.join("new-game");
    let old_inventory = test_root.join("old.prepared.json");
    let new_inventory = test_root.join("new.prepared.json");
    let old_snapshot = test_root.join("out").join("old.extractor.snapshot.json");
    let new_snapshot = test_root.join("out").join("new.extractor.snapshot.json");
    let report_output = test_root
        .join("out")
        .join("snapshot-report-extractor-low-signal.md");

    fs::create_dir_all(&old_root).expect("create old root");
    fs::create_dir_all(&new_root).expect("create new root");
    write_prepared_inventory(
        &old_inventory,
        "D:/prepared-old",
        None,
        12,
        1,
        "body-old",
        "sig-old",
        120,
        240,
        2,
        3,
        "pakchunk0-WindowsNoEditor.pak",
    );
    write_prepared_inventory(
        &new_inventory,
        "D:/prepared-new",
        None,
        12,
        1,
        "body-new",
        "sig-new",
        180,
        360,
        3,
        4,
        "pakchunk1-WindowsNoEditor.pak",
    );

    run_snapshot_command(&SnapshotArgs {
        source_root: old_root,
        version_id: "7.0.0".to_string(),
        output: old_snapshot.clone(),
        capture_scope: SnapshotCaptureScopeArg::Extractor,
        extractor_inventory: Some(old_inventory),
        store_in_report: false,
        report_root: None,
    })
    .expect("export old extractor snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: new_root,
        version_id: "7.1.0".to_string(),
        output: new_snapshot.clone(),
        capture_scope: SnapshotCaptureScopeArg::Extractor,
        extractor_inventory: Some(new_inventory),
        store_in_report: false,
        report_root: None,
    })
    .expect("export new extractor snapshot");

    run_snapshot_report_command(&SnapshotReportArgs {
        snapshots: vec![old_snapshot, new_snapshot],
        output: report_output.clone(),
    })
    .expect("run sparse extractor snapshot report");

    let output = fs::read_to_string(&report_output).expect("read output");

    assert!(output.contains("## Scope & Coverage"));
    assert!(output.contains("## Capture Quality"));
    assert!(output.contains(
        "| 7.0.0 | extractor_backed_asset_records | extractor_backed_asset_records | mixed or partial coverage |"
    ));
    assert!(output.contains(
        "| 7.0.0 | extractor_backed_asset_records | extractor_backed_asset_records | extractor-backed alignment-unverified | yes | missing | missing | asset_hashes=1/12 any_hashes=1/12 signatures=1/12 | source_context=1/12 rich_metadata=1/12 enriched_assets=1/12 |"
    ));
    assert!(output.contains(
        "manifest/hash coverage exists, but it remains shallow support and should not be read as rich asset-level enrichment."
    ));
    assert!(output.contains("enriched_records=1/12 threshold=5"));
    assert!(output.contains("extractor-backed alignment caution"));
    assert!(output.contains("alignment=undeclared"));
    assert!(
        output.contains(
            "The following snapshots are low-signal for deep character/resonator analysis:"
        )
    );
    assert!(output.contains(
        "shallow filesystem inventory or low-coverage/low-enrichment extractor snapshot; resonator-level and mapping-level interpretation can be incomplete."
    ));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn snapshot_report_highlights_version_aligned_rich_extractor_evidence() {
    let test_root = unique_test_dir();
    let old_root = test_root.join("old-game");
    let new_root = test_root.join("new-game");
    let old_inventory = test_root.join("old-rich.prepared.json");
    let new_inventory = test_root.join("new-rich.prepared.json");
    let old_snapshot = test_root.join("out").join("old.rich.snapshot.json");
    let new_snapshot = test_root.join("out").join("new.rich.snapshot.json");
    let report_output = test_root
        .join("out")
        .join("snapshot-report-extractor-rich.md");

    fs::create_dir_all(&old_root).expect("create old root");
    fs::create_dir_all(&new_root).expect("create new root");
    write_prepared_inventory(
        &old_inventory,
        "D:/prepared-old",
        Some("8.0.0"),
        12,
        6,
        "body-old",
        "sig-old",
        120,
        240,
        2,
        3,
        "pakchunk0-WindowsNoEditor.pak",
    );
    write_prepared_inventory(
        &new_inventory,
        "D:/prepared-new",
        Some("8.1.0"),
        12,
        6,
        "body-new",
        "sig-new",
        180,
        360,
        3,
        4,
        "pakchunk1-WindowsNoEditor.pak",
    );

    run_snapshot_command(&SnapshotArgs {
        source_root: old_root,
        version_id: "8.0.0".to_string(),
        output: old_snapshot.clone(),
        capture_scope: SnapshotCaptureScopeArg::Extractor,
        extractor_inventory: Some(old_inventory),
        store_in_report: false,
        report_root: None,
    })
    .expect("export old rich extractor snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: new_root,
        version_id: "8.1.0".to_string(),
        output: new_snapshot.clone(),
        capture_scope: SnapshotCaptureScopeArg::Extractor,
        extractor_inventory: Some(new_inventory),
        store_in_report: false,
        report_root: None,
    })
    .expect("export new rich extractor snapshot");

    run_snapshot_report_command(&SnapshotReportArgs {
        snapshots: vec![old_snapshot, new_snapshot],
        output: report_output.clone(),
    })
    .expect("run rich extractor snapshot report");

    let output = fs::read_to_string(&report_output).expect("read output");

    assert!(output.contains(
        "| 8.0.0 | extractor_backed_asset_records | extractor_backed_asset_records | extractor-backed rich evidence | no |"
    ));
    assert!(output.contains("| Evidence posture | old=extractor-backed rich evidence new=extractor-backed rich evidence |"));
    assert!(output.contains("matches_snapshot=yes"));
    assert!(output.contains(
        "All analyzed snapshots are extractor-backed, version-aligned, and content/character-rich enough"
    ));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn snapshot_report_explains_empty_character_scope_and_scope_induced_pair_removals() {
    let test_root = unique_test_dir();
    let root = test_root.join("game");
    let full_snapshot = test_root.join("out").join("full.snapshot.json");
    let character_snapshot = test_root.join("out").join("character.snapshot.json");
    let report_output = test_root
        .join("out")
        .join("snapshot-report-character-empty.md");

    seed_local_asset(&root, "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak");
    seed_local_asset(&root, "Client/Config/DefaultGame.ini");

    run_snapshot_command(&SnapshotArgs {
        source_root: root.clone(),
        version_id: "9.0.0".to_string(),
        output: full_snapshot.clone(),
        capture_scope: SnapshotCaptureScopeArg::Full,
        extractor_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("export full snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: root,
        version_id: "9.0.1".to_string(),
        output: character_snapshot.clone(),
        capture_scope: SnapshotCaptureScopeArg::Character,
        extractor_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("export character-focused snapshot");

    run_snapshot_report_command(&SnapshotReportArgs {
        snapshots: vec![full_snapshot, character_snapshot],
        output: report_output.clone(),
    })
    .expect("run character-empty snapshot report");

    let output = fs::read_to_string(&report_output).expect("read output");

    assert!(output.contains(
        "character-focused path filter found 0 paths matching Content/Character/<Name>/..."
    ));
    assert!(output.contains("Scope interpretation: scope-induced removal caution:"));
    assert!(output.contains("likely reflect scope filtering rather than true game-version drift"));

    let _ = fs::remove_dir_all(&test_root);
}

fn seed_local_asset(root: &Path, relative_path: &str) {
    let full_path = root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create asset directory");
    }

    fs::write(full_path, b"asset").expect("write asset file");
}

fn write_prepared_inventory(
    path: &Path,
    source_root: &str,
    inventory_version_id: Option<&str>,
    total_assets: usize,
    enriched_assets: usize,
    body_asset_hash: &str,
    body_signature: &str,
    body_vertex_count: u32,
    body_index_count: u32,
    body_material_slots: u32,
    body_section_count: u32,
    body_container_path: &str,
) {
    let assets = (0..total_assets)
        .map(|index| {
            let (
                character_name,
                logical_name,
                asset_hash,
                signature,
                vertex_count,
                index_count,
                material_slots,
                section_count,
                container_path,
            ) = if index == 0 {
                (
                    "Encore".to_string(),
                    "Encore Body".to_string(),
                    body_asset_hash.to_string(),
                    body_signature.to_string(),
                    body_vertex_count,
                    body_index_count,
                    body_material_slots,
                    body_section_count,
                    body_container_path.to_string(),
                )
            } else {
                (
                    format!("Fixture{index:02}"),
                    format!("Fixture{index:02} Body"),
                    format!("asset-{index}"),
                    format!("sig-{index}"),
                    120 + index as u32,
                    240 + index as u32,
                    2,
                    3,
                    "pakchunk0-WindowsNoEditor.pak".to_string(),
                )
            };
            let asset_path = format!("Content/Character/{character_name}/Body.mesh");
            let enriched = index < enriched_assets;
            let metadata = if enriched {
                json!({
                    "logical_name": logical_name,
                    "vertex_count": vertex_count,
                    "index_count": index_count,
                    "material_slots": material_slots,
                    "section_count": section_count,
                    "layout_markers": ["skinned"],
                    "tags": ["character", "prepared"]
                })
            } else {
                json!({
                    "logical_name": logical_name,
                    "tags": ["character", "prepared"]
                })
            };
            let hash_fields = if enriched {
                json!({
                    "asset_hash": asset_hash,
                    "signature": signature
                })
            } else {
                json!({})
            };
            let source = if enriched {
                json!({
                    "extraction_tool": "fixture-extractor",
                    "source_root": source_root,
                    "source_path": asset_path,
                    "source_kind": "mesh_record",
                    "container_path": container_path
                })
            } else {
                json!({})
            };

            json!({
                "id": format!("mesh:{}:body", character_name.to_lowercase()),
                "path": asset_path,
                "kind": "mesh",
                "metadata": metadata,
                "hash_fields": hash_fields,
                "source": source
            })
        })
        .collect::<Vec<_>>();

    let inventory = json!({
        "schema_version": "whashreonator.prepared-assets.v1",
        "context": {
            "extraction_tool": "fixture-extractor",
            "extraction_kind": "asset_records",
            "source_root": source_root,
            "version_id": inventory_version_id,
            "meaningful_content_coverage": true,
            "meaningful_character_coverage": true
        },
        "assets": assets
    });

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create inventory directory");
    }

    fs::write(
        path,
        serde_json::to_string_pretty(&inventory).expect("serialize prepared inventory"),
    )
    .expect("write prepared inventory");
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-snapshot-report-mode-test-{nanos}"))
}
