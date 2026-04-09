use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::{SnapshotArgs, SnapshotCaptureScopeArg},
    pipeline::run_snapshot_command,
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

    let snapshot = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "2.4.0".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Full,
    })
    .expect("run snapshot command");

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

    let snapshot = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "auto".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Full,
    })
    .expect("run snapshot command");

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

    let snapshot = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "2.4.0".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Content,
    })
    .expect("run content-focused snapshot command");

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

    let snapshot = run_snapshot_command(&SnapshotArgs {
        source_root: source_root.clone(),
        version_id: "2.4.0".to_string(),
        output: output_path.clone(),
        capture_scope: SnapshotCaptureScopeArg::Character,
    })
    .expect("run character-focused snapshot command");

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

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-snapshot-mode-test-{nanos}"))
}
