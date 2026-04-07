use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::MapLocalArgs,
    config::AppConfig,
    ingest::{AssetListSource, AssetSourceSpec, BundleAssetSide, LocalSnapshotIngestSource},
    pipeline::{SourceMapOutcome, preview_map_sources, run_map_local_command},
};

#[test]
fn local_ingest_scans_relative_paths_and_metadata() {
    let test_root = unique_test_dir();
    let local_root = test_root.join("local-old");
    seed_local_asset(&local_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&local_root, "Content/Weapon/Sword.weapon");

    let assets = LocalSnapshotIngestSource
        .load_assets(&local_root)
        .expect("load local assets");

    assert_eq!(assets.len(), 2);
    assert_eq!(assets[0].path, "Content/Character/HeroA/Body.mesh");
    assert_eq!(assets[0].id, "Content/Character/HeroA/Body.mesh");
    assert_eq!(assets[0].kind.as_deref(), Some("mesh"));
    assert_eq!(assets[0].metadata.logical_name.as_deref(), Some("Body"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn preview_map_sources_supports_json_vs_local() {
    let test_root = unique_test_dir();
    let input_path = test_root.join("input.json");
    let new_root = test_root.join("local-new");

    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &input_path,
        r#"
        {
          "old_assets": [
            {
              "id": "old-body",
              "path": "Content/Character/HeroA/Body.mesh",
              "kind": "mesh",
              "metadata": {
                "logical_name": "Body"
              }
            }
          ],
          "new_assets": []
        }
        "#,
    )
    .expect("write input");
    seed_local_asset(&new_root, "Content/Character/HeroA/Body.mesh");

    let report = preview_map_sources(
        &AssetSourceSpec::JsonBundle {
            path: input_path,
            side: BundleAssetSide::Old,
        },
        &AssetSourceSpec::LocalSnapshot { root: new_root },
        &AppConfig::default(),
    )
    .expect("preview source pair");

    assert_eq!(report.summary.total_old_assets, 1);
    assert_eq!(report.decisions.len(), 1);
    assert!(report.decisions[0].new_asset.is_some());

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn map_local_dry_run_does_not_write_outputs() {
    let test_root = unique_test_dir();
    let old_root = test_root.join("old");
    let new_root = test_root.join("new");
    let report_path = test_root.join("out").join("report.json");
    let mapping_path = test_root.join("out").join("mapping.json");
    let patch_path = test_root.join("out").join("patch-draft.json");

    seed_local_asset(&old_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&new_root, "Content/Character/HeroA/Body.mesh");

    let outcome = run_map_local_command(&MapLocalArgs {
        old_root,
        new_root,
        report_output: Some(report_path.clone()),
        mapping_output: Some(mapping_path.clone()),
        patch_draft_output: Some(patch_path.clone()),
        config: None,
        dry_run: true,
    })
    .expect("run local dry-run");

    match outcome {
        SourceMapOutcome::DryRun(result) => {
            assert_eq!(result.report.summary.total_old_assets, 1);
            assert_eq!(
                result.outputs.report_output.as_deref(),
                Some(report_path.as_path())
            );
            assert_eq!(
                result.outputs.mapping_output.as_deref(),
                Some(mapping_path.as_path())
            );
            assert_eq!(
                result.outputs.patch_draft_output.as_deref(),
                Some(patch_path.as_path())
            );
        }
        SourceMapOutcome::Exported(_) => panic!("expected dry-run outcome"),
    }

    assert!(!report_path.exists());
    assert!(!mapping_path.exists());
    assert!(!patch_path.exists());

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn map_local_writes_report_mapping_and_patch_draft() {
    let test_root = unique_test_dir();
    let old_root = test_root.join("old");
    let new_root = test_root.join("new");
    let config_path = test_root.join("config.json");
    let report_path = test_root.join("out").join("report.json");
    let mapping_path = test_root.join("out").join("mapping.json");
    let patch_path = test_root.join("out").join("patch-draft.json");

    seed_local_asset(&old_root, "Content/Character/HeroA/Body.mesh");
    seed_local_asset(&new_root, "Content/Character/HeroA/Body.mesh");
    fs::create_dir_all(&test_root).expect("create test root");
    fs::write(
        &config_path,
        r#"
        {
          "validator": {
            "matched_threshold": 0.50,
            "needs_review_threshold": 0.30,
            "minimum_margin": 0.10
          }
        }
        "#,
    )
    .expect("write config");

    let outcome = run_map_local_command(&MapLocalArgs {
        old_root,
        new_root,
        report_output: Some(report_path.clone()),
        mapping_output: Some(mapping_path.clone()),
        patch_draft_output: Some(patch_path.clone()),
        config: Some(config_path),
        dry_run: false,
    })
    .expect("run local export");

    match outcome {
        SourceMapOutcome::Exported(result) => {
            assert_eq!(result.report.summary.total_old_assets, 1);
            assert_eq!(result.report.summary.matched, 1);
        }
        SourceMapOutcome::DryRun(_) => panic!("expected exported outcome"),
    }

    let report = fs::read_to_string(&report_path).expect("read report");
    let mapping = fs::read_to_string(&mapping_path).expect("read mapping");
    let patch = fs::read_to_string(&patch_path).expect("read patch draft");

    assert!(report.contains("\"summary\""));
    assert!(!report.contains("\"schema_version\""));
    assert!(mapping.contains("\"schema_version\": \"whashreonator.mapping.v1\""));
    assert!(mapping.contains("\"mappings\""));
    assert!(patch.contains("\"schema_version\": \"whashreonator.patch-draft.v1\""));
    assert!(patch.contains("\"mode\": \"draft\""));
    assert!(patch.contains("\"action\": \"propose_mapping\""));

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

    std::env::temp_dir().join(format!("whashreonator-local-test-{nanos}"))
}
