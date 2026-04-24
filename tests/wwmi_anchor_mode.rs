use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::{ExtractWwmiAnchorsArgs, WwmiAnchorCaptureProfileArg},
    ingest::frame_analysis::{build_prepared_inventory, parse_frame_analysis_log},
    pipeline::run_extract_wwmi_anchors_command,
    wwmi::anchors::{WwmiAnchorCaptureProfile, WwmiAnchorReport, extract_wwmi_anchor_report},
};

const FULL_ANCHOR_LOG: &str = "analyse_options: 0000063d
000001 CSSetShader(pComputeShader:0x000000005D8B1E38, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=9bf4420c82102011
000001 Dispatch(ThreadGroupCountX:29747, ThreadGroupCountY:1, ThreadGroupCountZ:1)
000003 CSSetShader(pComputeShader:0x000000008984C1B8, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=7a8396180d416117
000003 Dispatch(ThreadGroupCountX:100, ThreadGroupCountY:1, ThreadGroupCountZ:1)
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

const MENU_UI_WITH_EXTRA_CANDIDATE_LOG: &str = "analyse_options: 0000063d
000005 IASetVertexBuffers(StartSlot:0, NumBuffers:1, ppVertexBuffers:0x00000000567CFDB0, pStrides:0x00000000567CFDC8, pOffsets:0x00000000567CFDB8)
       0: resource=0x000000004AAF4D60 hash=5e31595c
000005 VSSetShader(pVertexShader:0x000000005D8B06B8, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=8c1ee0581cb4f0ec
000005 PSSetShader(pPixelShader:0x00000002ABAE8B38, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=a24b0fd936f39dcc
000005 PSSetShaderResources(StartSlot:0, NumViews:1, ppShaderResourceViews:0x00000000567CFDB8)
       0: view=0x00000002B36B5AE8 resource=0x00000002B3847560 hash=2cc576e7
000005 PSSetShaderResources(StartSlot:1, NumViews:1, ppShaderResourceViews:0x00000000567CFDB8)
       1: view=0x00000002B36B53E8 resource=0x00000002B38472A0 hash=ce6a251a
000005 PSSetShaderResources(StartSlot:3, NumViews:1, ppShaderResourceViews:0x00000000567CFDB8)
       3: view=0x00000002B36B53E8 resource=0x00000002B38472A0 hash=1234abcd
000005 DrawIndexed(IndexCount:6, StartIndexLocation:0, BaseVertexLocation:0)
";

#[test]
fn full_profile_reports_all_six_canonical_anchors() {
    let report = build_report(
        FULL_ANCHOR_LOG,
        WwmiAnchorCaptureProfile::Full,
        PathBuf::from("out/full-dump"),
    );

    assert!(report.success);
    assert_eq!(report.found_anchors.len(), 6);
    assert!(report.missing_anchors.is_empty());
    assert!(report.unexpected_anchor_candidates.is_empty());
    assert_eq!(
        report
            .found_anchors
            .iter()
            .find(|anchor| anchor.hash == "9bf4420c82102011")
            .and_then(|anchor| anchor.identity_tuple.as_deref()),
        Some("fa|cs|tg:29747x1x1")
    );
}

#[test]
fn menu_ui_profile_succeeds_without_shapekey_anchors() {
    let report = build_report(
        MENU_UI_ONLY_LOG,
        WwmiAnchorCaptureProfile::MenuUi,
        PathBuf::from("out/menu-ui-dump"),
    );

    assert!(report.success);
    assert_eq!(report.found_anchors.len(), 4);
    assert!(report.missing_anchors.is_empty());
}

#[test]
fn full_profile_marks_missing_required_anchor_coverage() {
    let report = build_report(
        MENU_UI_ONLY_LOG,
        WwmiAnchorCaptureProfile::Full,
        PathBuf::from("out/full-missing"),
    );

    assert!(!report.success);
    let missing_hashes = report
        .missing_anchors
        .iter()
        .map(|anchor| anchor.hash.as_str())
        .collect::<Vec<_>>();
    assert_eq!(missing_hashes, vec!["7a8396180d416117", "9bf4420c82102011"]);
}

#[test]
fn report_surfaces_non_canonical_shader_and_texture_candidates() {
    let report = build_report(
        MENU_UI_WITH_EXTRA_CANDIDATE_LOG,
        WwmiAnchorCaptureProfile::MenuUi,
        PathBuf::from("out/menu-ui-extra"),
    );

    assert!(report.success);
    assert_eq!(report.unexpected_anchor_candidates.len(), 1);
    assert_eq!(report.unexpected_anchor_candidates[0].hash, "1234abcd");
    assert_eq!(
        report.unexpected_anchor_candidates[0].observed_kind,
        "texture_resource"
    );
}

#[test]
fn pipeline_command_writes_profile_aware_anchor_report_json() {
    let test_root = unique_test_dir();
    let dump_dir = test_root.join("dump");
    let output_path = test_root.join("out").join("wwmi-anchor-report.json");
    fs::create_dir_all(&dump_dir).expect("create dump dir");
    fs::write(dump_dir.join("log.txt"), MENU_UI_ONLY_LOG).expect("write fixture log");

    let report = run_extract_wwmi_anchors_command(&ExtractWwmiAnchorsArgs {
        dump_dir: dump_dir.clone(),
        capture_profile: WwmiAnchorCaptureProfileArg::MenuUi,
        output: output_path.clone(),
    })
    .expect("run wwmi anchor extraction");

    assert!(report.success);
    let contents = fs::read_to_string(&output_path).expect("read report");
    let parsed: WwmiAnchorReport = serde_json::from_str(&contents).expect("parse report");
    assert_eq!(parsed.schema_version, "whashreonator.wwmi-anchor-report.v1");
    assert_eq!(parsed.capture_profile, WwmiAnchorCaptureProfile::MenuUi);
    assert_eq!(parsed.found_anchors.len(), 4);
    assert_eq!(
        parsed.source_dump_dir,
        dump_dir.to_string_lossy().replace('\\', "/")
    );
    assert_eq!(
        parsed.source_log_path,
        dump_dir
            .join("log.txt")
            .to_string_lossy()
            .replace('\\', "/")
    );

    let _ = fs::remove_dir_all(&test_root);
}

fn build_report(
    log_text: &str,
    capture_profile: WwmiAnchorCaptureProfile,
    dump_dir: PathBuf,
) -> WwmiAnchorReport {
    let mut dump = parse_frame_analysis_log(log_text).expect("parse log");
    dump.dump_dir = dump_dir.clone();
    dump.log_path = dump_dir.join("log.txt");
    let inventory = build_prepared_inventory(&dump, "fixture");
    extract_wwmi_anchor_report(
        &inventory,
        capture_profile,
        dump.dump_dir.as_path(),
        dump.log_path.as_path(),
    )
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-wwmi-anchor-test-{nanos}"))
}
