use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use whashreonator::{
    cli::{CompareSnapshotsArgs, SnapshotArgs, SnapshotCaptureScopeArg},
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
        capture_scope: SnapshotCaptureScopeArg::Full,
        extractor_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("create old snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: new_root,
        version_id: "2.5.0".to_string(),
        output: new_snapshot_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Full,
        extractor_inventory: None,
        store_in_report: false,
        report_root: None,
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
    assert_eq!(parsed.scope.old_snapshot.assets_with_any_hash, 0);
    assert_eq!(parsed.scope.new_snapshot.assets_with_any_hash, 0);

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn compare_snapshots_command_keeps_sparse_extractor_pairs_low_signal() {
    let test_root = unique_test_dir();
    let old_root = test_root.join("old");
    let new_root = test_root.join("new");
    let old_inventory = test_root.join("old.prepared.json");
    let new_inventory = test_root.join("new.prepared.json");
    let old_snapshot_path = test_root.join("snapshots").join("old.extractor.json");
    let new_snapshot_path = test_root.join("snapshots").join("new.extractor.json");
    let compare_output_path = test_root.join("out").join("compare-extractor.json");

    fs::create_dir_all(&old_root).expect("create old root");
    fs::create_dir_all(&new_root).expect("create new root");
    write_prepared_inventory(
        &old_inventory,
        "D:/prepared-old",
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
        output: old_snapshot_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Extractor,
        extractor_inventory: Some(old_inventory),
        store_in_report: false,
        report_root: None,
    })
    .expect("create old extractor snapshot");
    run_snapshot_command(&SnapshotArgs {
        source_root: new_root,
        version_id: "7.1.0".to_string(),
        output: new_snapshot_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Extractor,
        extractor_inventory: Some(new_inventory),
        store_in_report: false,
        report_root: None,
    })
    .expect("create new extractor snapshot");

    let report = run_compare_snapshots_command(&CompareSnapshotsArgs {
        old_snapshot: old_snapshot_path,
        new_snapshot: new_snapshot_path,
        output: compare_output_path.clone(),
    })
    .expect("run extractor compare command");

    let output = fs::read_to_string(&compare_output_path).expect("read compare output");
    let parsed: SnapshotCompareReport =
        serde_json::from_str(&output).expect("parse compare report");

    assert!(report.scope.low_signal_compare);
    assert!(parsed.scope.low_signal_compare);
    assert!(!parsed.scope.old_snapshot.meaningful_asset_record_enrichment);
    assert!(!parsed.scope.new_snapshot.meaningful_asset_record_enrichment);
    assert_eq!(parsed.scope.old_snapshot.extractor_record_count, 12);
    assert_eq!(parsed.scope.old_snapshot.assets_with_asset_hash, 1);
    assert_eq!(parsed.scope.old_snapshot.assets_with_source_context, 1);
    assert!(
        parsed
            .scope
            .notes
            .iter()
            .any(|note| note.contains("manifest_coverage=resources:0"))
    );
    assert!(
        parsed
            .scope
            .notes
            .iter()
            .any(|note| note.contains("low-coverage/low-enrichment extractor snapshots"))
    );

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

    std::env::temp_dir().join(format!("whashreonator-compare-mode-test-{nanos}"))
}
