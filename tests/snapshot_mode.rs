use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::{SnapshotArgs, SnapshotCaptureScopeArg},
    pipeline::run_snapshot_command,
    report_storage::{ReportStorage, VersionArtifactKind},
    snapshot::GameSnapshot,
};

#[test]
fn snapshot_command_exports_machine_readable_snapshot() {
    let test_root = unique_test_dir();
    let source_root = test_root.join("game");
    let output_path = test_root.join("out").join("snapshot.json");

    seed_local_asset(&source_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&source_root, "Content/Weapon/Sword.weapon");
    seed_local_asset(&source_root, "Client/Config/DefaultGame.ini");

    let result = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "2.4.0".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Full,
        prepared_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("run snapshot command");
    let snapshot = result.snapshot;

    let output = fs::read_to_string(&output_path).expect("read snapshot output");
    let parsed: GameSnapshot = serde_json::from_str(&output).expect("parse snapshot json");

    assert_eq!(snapshot.schema_version, "whashreonator.snapshot.v1");
    assert_eq!(snapshot.version_id, "2.4.0");
    assert_eq!(snapshot.asset_count, 3);
    assert_eq!(parsed.asset_count, 3);
    assert_eq!(parsed.version_id, "2.4.0");
    assert_eq!(
        parsed.context.scope.capture_mode.as_deref(),
        Some("local_filesystem_inventory")
    );
    assert_eq!(parsed.context.scope.coverage.content_like_path_count, 2);
    assert_eq!(parsed.context.scope.coverage.character_path_count, 1);
    assert_eq!(parsed.context.scope.coverage.non_content_path_count, 1);
    assert_eq!(
        parsed.context.scope.mostly_install_or_package_level,
        Some(true)
    );
    assert!(
        parsed
            .assets
            .iter()
            .any(|asset| asset.path == "Content/Character/HeroA/Body.mesh")
    );
    assert!(
        parsed
            .assets
            .iter()
            .any(|asset| asset.path == "Client/Config/DefaultGame.ini")
    );
    assert!(
        parsed
            .assets
            .iter()
            .all(|asset| asset.fingerprint.normalized_name.is_some())
    );
    assert!(parsed.source_root.contains("game"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn snapshot_command_auto_detects_version_and_enriches_hashes() {
    let test_root = unique_test_dir();
    let source_root = test_root.join("game");
    let output_path = test_root.join("out").join("snapshot-auto.json");

    seed_local_asset(
        &source_root,
        "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak",
    );
    fs::write(
        source_root.join("launcherDownloadConfig.json"),
        r#"{"version":"3.2.1","reUseVersion":"","state":"ready","isPreDownload":false,"appId":"50004"}"#,
    )
    .expect("write launcher config");
    fs::write(
        source_root.join("LocalGameResources.json"),
        r#"{"resource":[{"dest":"Client/Content/Paks/pakchunk0-WindowsNoEditor.pak","size":123,"md5":"abc123"}]}"#,
    )
    .expect("write resource manifest");

    let result = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "auto".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Full,
        prepared_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("run snapshot command");
    let snapshot = result.snapshot;

    let output = fs::read_to_string(&output_path).expect("read snapshot output");
    let parsed: GameSnapshot = serde_json::from_str(&output).expect("parse snapshot json");

    assert_eq!(snapshot.version_id, "3.2.1");
    assert_eq!(parsed.version_id, "3.2.1");
    assert_eq!(
        parsed.context.scope.capture_mode.as_deref(),
        Some("local_filesystem_inventory")
    );
    assert_eq!(parsed.context.scope.coverage.content_like_path_count, 1);
    assert_eq!(parsed.context.scope.coverage.character_path_count, 0);
    assert_eq!(parsed.context.scope.coverage.non_content_path_count, 2);
    assert_eq!(
        parsed.context.scope.mostly_install_or_package_level,
        Some(true)
    );
    assert_eq!(
        parsed.context.scope.meaningful_content_coverage,
        Some(false)
    );
    assert_eq!(
        parsed.context.scope.meaningful_character_coverage,
        Some(false)
    );
    assert_eq!(
        parsed
            .context
            .launcher
            .as_ref()
            .map(|launcher| launcher.detected_version.as_str()),
        Some("3.2.1")
    );
    assert_eq!(
        parsed
            .context
            .resource_manifest
            .as_ref()
            .map(|manifest| manifest.matched_assets),
        Some(1)
    );
    assert!(parsed.assets.iter().any(|asset| {
        asset.path == "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak"
            && asset.hash_fields.asset_hash.as_deref() == Some("abc123")
    }));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn snapshot_command_supports_content_focused_capture_scope() {
    let test_root = unique_test_dir();
    let source_root = test_root.join("game");
    let output_path = test_root.join("out").join("snapshot-content.json");

    seed_local_asset(&source_root, "Client/Config/DefaultGame.ini");
    seed_local_asset(&source_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&source_root, "Content/Weapon/Sword.weapon");

    let result = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "2.4.0".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Content,
        prepared_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("run content-focused snapshot command");
    let snapshot = result.snapshot;

    let output = fs::read_to_string(&output_path).expect("read snapshot output");
    let parsed: GameSnapshot = serde_json::from_str(&output).expect("parse snapshot json");

    assert_eq!(snapshot.asset_count, 2);
    assert_eq!(parsed.asset_count, 2);
    assert!(
        parsed
            .assets
            .iter()
            .all(|asset| asset.path.starts_with("Content/"))
    );
    assert_eq!(
        parsed.context.scope.capture_mode.as_deref(),
        Some("local_filesystem_inventory_content_focused")
    );
    assert_eq!(parsed.context.scope.coverage.content_like_path_count, 2);
    assert_eq!(parsed.context.scope.coverage.character_path_count, 1);
    assert_eq!(parsed.context.scope.coverage.non_content_path_count, 0);
    assert!(
        parsed
            .context
            .scope
            .note
            .as_deref()
            .is_some_and(|note| { note.contains("path-based filtering only") })
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn snapshot_command_supports_character_focused_capture_scope() {
    let test_root = unique_test_dir();
    let source_root = test_root.join("game");
    let output_path = test_root.join("out").join("snapshot-character.json");

    seed_local_asset(&source_root, "Client/Config/DefaultGame.ini");
    seed_local_asset(&source_root, "Content/Weapon/Sword.weapon");
    seed_local_asset(&source_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&source_root, "Content/Character/HeroB/Body.mesh");

    let result = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "2.4.0".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Character,
        prepared_inventory: None,
        store_in_report: false,
        report_root: None,
    })
    .expect("run character-focused snapshot command");
    let snapshot = result.snapshot;

    let output = fs::read_to_string(&output_path).expect("read snapshot output");
    let parsed: GameSnapshot = serde_json::from_str(&output).expect("parse snapshot json");

    assert_eq!(snapshot.asset_count, 2);
    assert_eq!(parsed.asset_count, 2);
    assert!(
        parsed
            .assets
            .iter()
            .all(|asset| { asset.path.starts_with("Content/Character/") })
    );
    assert_eq!(
        parsed.context.scope.capture_mode.as_deref(),
        Some("local_filesystem_inventory_character_focused")
    );
    assert_eq!(parsed.context.scope.coverage.content_like_path_count, 2);
    assert_eq!(parsed.context.scope.coverage.character_path_count, 2);
    assert_eq!(parsed.context.scope.coverage.non_content_path_count, 0);

    let _ = fs::remove_dir_all(&test_root);
}

fn seed_local_asset(root: &Path, relative_path: &str) {
    let full_path = root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create asset directory");
    }

    fs::write(full_path, b"asset").expect("write asset file");
}

#[test]
fn prepared_snapshot_command_is_runtime_facing_and_stored_as_official_artifacts() {
    let test_root = unique_test_dir();
    let report_root = test_root.join("out").join("report");
    let storage = ReportStorage::new(report_root.clone());
    let old_inventory = test_root.join("prepared-old.json");
    let new_inventory = test_root.join("prepared-new.json");
    let old_output = test_root.join("out").join("prepared-old.snapshot.json");
    let new_output = test_root.join("out").join("prepared-new.snapshot.json");

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &old_inventory,
        r#"{
            "schema_version":"whashreonator.prepared-assets.v1",
            "context":{
                "extraction_tool":"fixture-extractor",
                "extraction_kind":"asset_records",
                "source_root":"D:/prepared-old"
            },
            "assets":[
                {
                    "id":"mesh:encore:body",
                    "path":"Content/Character/Encore/Body.mesh",
                    "kind":"mesh",
                    "metadata":{
                        "logical_name":"Encore Body",
                        "vertex_count":120,
                        "index_count":240,
                        "material_slots":2,
                        "section_count":3,
                        "layout_markers":["skinned","interleaved"],
                        "tags":["character","prepared"]
                    },
                    "hash_fields":{
                        "asset_hash":"body-old",
                        "signature":"sig-body"
                    },
                    "source":{
                        "extraction_tool":"fixture-extractor",
                        "source_root":"D:/prepared-old",
                        "source_path":"Content/Character/Encore/Body.mesh",
                        "source_kind":"mesh_record",
                        "container_path":"pakchunk0-WindowsNoEditor.pak"
                    }
                }
            ]
        }"#,
    )
    .expect("write old prepared inventory");
    fs::write(
        &new_inventory,
        r#"{
            "schema_version":"whashreonator.prepared-assets.v1",
            "context":{
                "extraction_tool":"fixture-extractor",
                "extraction_kind":"asset_records",
                "source_root":"D:/prepared-new"
            },
            "assets":[
                {
                    "id":"mesh:encore:body",
                    "path":"Content/Character/Encore/Body.mesh",
                    "kind":"mesh",
                    "metadata":{
                        "logical_name":"Encore Body",
                        "vertex_count":180,
                        "index_count":360,
                        "material_slots":3,
                        "section_count":4,
                        "layout_markers":["skinned","expanded"],
                        "tags":["character","prepared"]
                    },
                    "hash_fields":{
                        "asset_hash":"body-new",
                        "signature":"sig-body"
                    },
                    "source":{
                        "extraction_tool":"fixture-extractor",
                        "source_root":"D:/prepared-new",
                        "source_path":"Content/Character/Encore/Body.mesh",
                        "source_kind":"mesh_record",
                        "container_path":"pakchunk1-WindowsNoEditor.pak"
                    }
                }
            ]
        }"#,
    )
    .expect("write new prepared inventory");

    let old_result = run_snapshot_command(&SnapshotArgs {
        source_root: test_root.clone(),
        version_id: "6.0.0".to_string(),
        output: old_output.clone(),
        capture_scope: SnapshotCaptureScopeArg::Prepared,
        prepared_inventory: Some(old_inventory.clone()),
        store_in_report: true,
        report_root: Some(report_root.clone()),
    })
    .expect("store old prepared snapshot");
    let new_result = run_snapshot_command(&SnapshotArgs {
        source_root: test_root.clone(),
        version_id: "6.1.0".to_string(),
        output: new_output.clone(),
        capture_scope: SnapshotCaptureScopeArg::Prepared,
        prepared_inventory: Some(new_inventory.clone()),
        store_in_report: true,
        report_root: Some(report_root.clone()),
    })
    .expect("store new prepared snapshot");

    let old_snapshot = old_result.snapshot;
    let _new_snapshot = new_result.snapshot;

    assert_eq!(
        old_snapshot.context.scope.capture_mode.as_deref(),
        Some("prepared_asset_list_inventory")
    );
    assert_eq!(
        old_snapshot.context.scope.mostly_install_or_package_level,
        Some(false)
    );
    assert!(
        old_snapshot
            .context
            .scope
            .note
            .as_deref()
            .is_some_and(|note| note.contains("fixture-extractor"))
    );
    assert_eq!(
        old_result.stored_snapshot_path.as_deref(),
        Some(storage.snapshot_path_for_version("6.0.0").as_path())
    );
    assert!(
        old_result
            .stored_prepared_inventory_path
            .as_ref()
            .is_some_and(|path| path.exists())
    );
    assert_eq!(
        new_result.stored_snapshot_path.as_deref(),
        Some(storage.snapshot_path_for_version("6.1.0").as_path())
    );

    let stored_old = storage
        .load_snapshot_by_version("6.0.0")
        .expect("load stored old snapshot")
        .expect("stored old snapshot exists");
    assert_eq!(
        stored_old.context.scope.capture_mode.as_deref(),
        Some("prepared_asset_list_inventory")
    );
    assert_eq!(
        stored_old.assets[0].hash_fields.asset_hash.as_deref(),
        Some("body-old")
    );

    let new_artifacts = storage
        .list_version_artifacts("6.1.0")
        .expect("list new artifacts");
    assert!(new_artifacts.iter().any(|artifact| {
        artifact.kind == VersionArtifactKind::Snapshot
            && artifact.path == storage.snapshot_path_for_version("6.1.0")
    }));
    assert!(new_artifacts.iter().any(|artifact| {
        artifact.kind == VersionArtifactKind::Auxiliary
            && artifact
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("prepared-inventory"))
    }));

    let compare_report = storage
        .compare_versions("6.0.0", "6.1.0")
        .expect("compare stored prepared snapshots");
    assert_eq!(compare_report.old_version.version_id, "6.0.0");
    assert_eq!(compare_report.new_version.version_id, "6.1.0");
    assert!(compare_report.summary.changed_items > 0);
    assert_eq!(compare_report.scope_notes.len(), 2);
    assert!(
        compare_report
            .scope_notes
            .iter()
            .all(|note| !note.contains("scope warning"))
    );

    let _ = fs::remove_dir_all(&test_root);
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-snapshot-mode-test-{nanos}"))
}
