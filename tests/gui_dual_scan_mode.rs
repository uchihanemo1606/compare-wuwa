use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    gui_app::{FrameAnalysisScanForm, GuiController, ScanForm, ScanRunResult},
    report_storage::ReportStorage,
    wwmi::anchors::{WwmiAnchorCaptureProfile, WwmiVersionAnchorKnowledge},
};

const MENU_UI_ONLY_LOG: &str = "analyse_options: 0000063d
000005 IASetVertexBuffers(StartSlot:0, NumBuffers:1, ppVertexBuffers:0x00000000567CFDB0, pStrides:0x00000000567CFDC8, pOffsets:0x00000000567CFDB8)
       0: resource=0x000000004AAF4D60 hash=5e31595c
000005 IASetIndexBuffer(pIndexBuffer:0x000000004AAF5020, Format:57, Offset:0) hash=647cd8c5
000005 VSSetShader(pVertexShader:0x000000005D8B06B8, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=8c1ee0581cb4f0ec
000005 PSSetShader(pPixelShader:0x00000002ABAE8B38, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=a24b0fd936f39dcc
000005 PSSetShaderResources(StartSlot:0, NumViews:1, ppShaderResourceViews:0x00000000567CFDB8)
       0: view=0x00000002B36B5AE8 resource=0x00000002B3847560 hash=2cc576e7
000005 PSSetShaderResources(StartSlot:1, NumViews:1, ppShaderResourceViews:0x00000000567CFDB8)
       1: view=0x00000002B36B53E8 resource=0x00000002B38472A0 hash=ce6a251a
000005 DrawIndexed(IndexCount:6, StartIndexLocation:0, BaseVertexLocation:0)
";

const SHAPEKEY_ONLY_LOG: &str = "analyse_options: 0000063d
000001 CSSetShader(pComputeShader:0x000000005D8B1E38, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=9bf4420c82102011
000001 Dispatch(ThreadGroupCountX:29747, ThreadGroupCountY:1, ThreadGroupCountZ:1)
000003 CSSetShader(pComputeShader:0x000000008984C1B8, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=7a8396180d416117
000003 Dispatch(ThreadGroupCountX:100, ThreadGroupCountY:1, ThreadGroupCountZ:1)
";

const SHAPEKEY_DISCOVERY_LOG: &str = "analyse_options: 0000063d
000001 CSSetShader(pComputeShader:0x000000005D8B1E38, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=1111111111111111
000001 Dispatch(ThreadGroupCountX:29747, ThreadGroupCountY:1, ThreadGroupCountZ:1)
000003 CSSetShader(pComputeShader:0x000000008984C1B8, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=2222222222222222
000003 Dispatch(ThreadGroupCountX:100, ThreadGroupCountY:1, ThreadGroupCountZ:1)
000004 CSSetShader(pComputeShader:0x000000008984C1B8, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=3333333333333333
000004 Dispatch(ThreadGroupCountX:4, ThreadGroupCountY:4, ThreadGroupCountZ:1)
";

#[test]
fn game_root_scan_flow_still_creates_snapshot_versions() {
    let test_root = unique_test_dir();
    let storage = test_storage(&test_root);
    let controller = GuiController::new(storage.clone());
    let game_root = test_root.join("game");
    seed_game_root(&game_root, "8.0.0");

    let prepared = match controller
        .prepare_scan(&ScanForm {
            source_root: game_root.display().to_string(),
            version_override: String::new(),
            knowledge_path: String::new(),
        })
        .expect("prepare game root scan")
    {
        whashreonator::gui_app::ScanStartResult::Ready(prepared) => prepared,
        other => panic!("expected ready scan, got {other:?}"),
    };

    let result = controller
        .run_scan(&prepared, false, "")
        .expect("run game root scan");
    match result {
        ScanRunResult::Created { version_id, .. } => assert_eq!(version_id, "8.0.0"),
        other => panic!("expected created result, got {other:?}"),
    }
    assert!(
        storage
            .load_snapshot_by_version("8.0.0")
            .expect("load snapshot")
            .is_some()
    );

    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn frame_analysis_scan_creates_hash_only_version_and_renders_anchor_summary() {
    let test_root = unique_test_dir();
    let storage = test_storage(&test_root);
    let controller = GuiController::new(storage.clone());
    let dump_dir = write_dump(&test_root.join("dump-menu"), MENU_UI_ONLY_LOG);

    let prepared = controller
        .prepare_frame_analysis_scan(&FrameAnalysisScanForm {
            dump_dir: dump_dir.display().to_string(),
            version_id: "9.0.0".to_string(),
            capture_profile: WwmiAnchorCaptureProfile::MenuUi,
        })
        .expect("prepare frame analysis scan");

    let result = controller
        .run_frame_analysis_scan(&prepared)
        .expect("run frame analysis scan");
    match result {
        ScanRunResult::Created {
            version_id,
            summary,
            ..
        } => {
            assert_eq!(version_id, "9.0.0");
            assert!(summary.contains("WWMI Anchor Scan 9.0.0"));
            assert!(summary.contains("Capture profile: menu_ui"));
        }
        other => panic!("expected created result, got {other:?}"),
    }

    let versions = controller.list_versions().expect("list versions");
    let entry = versions
        .iter()
        .find(|entry| entry.version_id == "9.0.0")
        .expect("hash-only version row");
    assert!(entry.label.contains("[hash-only]"));

    let artifacts = storage
        .list_version_artifacts("9.0.0")
        .expect("list artifacts");
    assert!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind
                == whashreonator::report_storage::VersionArtifactKind::HashData)
            .count()
            >= 2
    );

    let detail = controller
        .open_version("9.0.0")
        .expect("open hash-only version");
    assert!(detail.summary.contains("Snapshot: not found"));
    assert!(
        detail
            .summary
            .contains("WWMI anchors: reports=1 profiles=menu_ui exact=4 missing=2")
    );
    assert!(detail.summary.contains("anchor [ShapeKeyLoaderCS] missing"));

    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn frame_analysis_multi_profile_scan_builds_complete_current_version_knowledge() {
    let test_root = unique_test_dir();
    let storage = test_storage(&test_root);
    let controller = GuiController::new(storage.clone());
    let menu_dump = write_dump(&test_root.join("dump-menu"), MENU_UI_ONLY_LOG);
    let shapekey_dump = write_dump(&test_root.join("dump-shapekey"), SHAPEKEY_ONLY_LOG);

    let prepared_menu = controller
        .prepare_frame_analysis_scan(&FrameAnalysisScanForm {
            dump_dir: menu_dump.display().to_string(),
            version_id: "9.1.0".to_string(),
            capture_profile: WwmiAnchorCaptureProfile::MenuUi,
        })
        .expect("prepare menu scan");
    controller
        .run_frame_analysis_scan(&prepared_menu)
        .expect("run menu scan");

    let prepared_shapekey = controller
        .prepare_frame_analysis_scan(&FrameAnalysisScanForm {
            dump_dir: shapekey_dump.display().to_string(),
            version_id: "9.1.0".to_string(),
            capture_profile: WwmiAnchorCaptureProfile::ShapekeyRuntime,
        })
        .expect("prepare shapekey scan");
    let result = controller
        .run_frame_analysis_scan(&prepared_shapekey)
        .expect("run shapekey scan");
    match result {
        ScanRunResult::Overwritten { version_id, .. } => assert_eq!(version_id, "9.1.0"),
        other => panic!("expected overwritten result, got {other:?}"),
    }

    let knowledge = storage
        .load_latest_wwmi_anchor_knowledge("9.1.0")
        .expect("load knowledge")
        .expect("knowledge exists");
    assert_eq!(knowledge.report_count, 2);
    assert_eq!(knowledge.capture_profiles.len(), 2);
    assert!(
        knowledge
            .notes
            .iter()
            .any(|note| note.contains("exact anchors=6 missing anchors=0"))
    );
    assert!(
        knowledge
            .anchors
            .iter()
            .all(|anchor| !anchor.current_missing)
    );

    let detail = controller.open_version("9.1.0").expect("open version");
    assert!(
        detail.summary.contains(
            "WWMI anchors: reports=2 profiles=menu_ui, shapekey_runtime exact=6 missing=0"
        )
    );
    assert!(
        detail
            .summary
            .contains("anchor [UIDrawPS] matched hash=a24b0fd936f39dcc")
    );
    assert!(
        detail
            .summary
            .contains("anchor [ShapeKeyLoaderCS] matched hash=9bf4420c82102011")
    );

    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn frame_analysis_version_history_keeps_prior_exact_match_and_current_candidates() {
    let test_root = unique_test_dir();
    let storage = test_storage(&test_root);
    let controller = GuiController::new(storage.clone());
    let old_dump = write_dump(&test_root.join("dump-old"), SHAPEKEY_ONLY_LOG);
    let new_dump = write_dump(&test_root.join("dump-new"), SHAPEKEY_DISCOVERY_LOG);

    let prepared_old = controller
        .prepare_frame_analysis_scan(&FrameAnalysisScanForm {
            dump_dir: old_dump.display().to_string(),
            version_id: "8.9.0".to_string(),
            capture_profile: WwmiAnchorCaptureProfile::ShapekeyRuntime,
        })
        .expect("prepare old scan");
    controller
        .run_frame_analysis_scan(&prepared_old)
        .expect("run old scan");

    let prepared_new = controller
        .prepare_frame_analysis_scan(&FrameAnalysisScanForm {
            dump_dir: new_dump.display().to_string(),
            version_id: "9.2.0".to_string(),
            capture_profile: WwmiAnchorCaptureProfile::ShapekeyRuntime,
        })
        .expect("prepare new scan");
    controller
        .run_frame_analysis_scan(&prepared_new)
        .expect("run new scan");

    let knowledge: WwmiVersionAnchorKnowledge = storage
        .load_latest_wwmi_anchor_knowledge("9.2.0")
        .expect("load knowledge")
        .expect("knowledge exists");
    let loader = knowledge
        .anchors
        .iter()
        .find(|anchor| anchor.logical_name == "ShapeKeyLoaderCS")
        .expect("loader entry");
    assert!(loader.current_missing);
    assert_eq!(
        loader
            .historical_exact_matches
            .iter()
            .map(|entry| entry.version_id.as_str())
            .collect::<Vec<_>>(),
        vec!["8.9.0"]
    );
    assert_eq!(
        loader.current_candidate_replacements[0].hash,
        "1111111111111111"
    );

    let detail = controller.open_version("9.2.0").expect("open version");
    assert!(detail.summary.contains("history: 8.9.0=9bf4420c82102011"));
    assert!(
        detail
            .summary
            .contains("anchor [ShapeKeyLoaderCS] missing | top candidate=1111111111111111")
    );

    let _ = fs::remove_dir_all(test_root);
}

fn test_storage(root: &Path) -> ReportStorage {
    ReportStorage::with_legacy_root(
        root.join("out").join("report"),
        root.join("out").join("reports"),
    )
}

fn write_dump(path: &Path, log: &str) -> PathBuf {
    fs::create_dir_all(path).expect("create dump dir");
    fs::write(path.join("log.txt"), log).expect("write log");
    path.to_path_buf()
}

fn seed_game_root(root: &Path, version: &str) {
    let asset_path = root
        .join("Content")
        .join("Character")
        .join("Encore")
        .join("Body.mesh");
    fs::create_dir_all(asset_path.parent().expect("asset parent")).expect("create asset parent");
    fs::write(asset_path, b"asset").expect("write asset");
    fs::write(
        root.join("launcherDownloadConfig.json"),
        format!(
            r#"{{"version":"{version}","reUseVersion":"","state":"ready","isPreDownload":false,"appId":"50004"}}"#
        ),
    )
    .expect("write launcher config");
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-gui-dual-scan-test-{nanos}"))
}
