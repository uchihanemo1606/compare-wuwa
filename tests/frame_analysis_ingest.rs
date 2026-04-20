use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::IngestFrameAnalysisArgs,
    domain::PreparedAssetInventory,
    ingest::frame_analysis::{
        FrameAnalysisDraw, build_prepared_inventory, parse_frame_analysis_log,
    },
    pipeline::run_ingest_frame_analysis_command,
};

#[test]
fn parses_synthetic_fixture_log_into_drawcalls() {
    let dump = parse_fixture_dump();

    assert_eq!(dump.draw_calls.len(), 4);
    assert_eq!(
        dump.draw_calls
            .iter()
            .map(|draw_call| draw_call.drawcall)
            .collect::<Vec<_>>(),
        vec![123, 124, 125, 126]
    );

    let draw_123 = &dump.draw_calls[0];
    assert_eq!(draw_123.vb_bindings.len(), 2);
    assert_eq!(draw_123.vb_bindings[0].slot, "0");
    assert_eq!(draw_123.vb_bindings[0].hash, "4a7d9c1f");
    assert_eq!(draw_123.vb_bindings[1].slot, "1");
    assert_eq!(draw_123.vb_bindings[1].hash, "8e2b5a9f");
    assert_eq!(
        draw_123
            .ib_binding
            .as_ref()
            .map(|binding| binding.hash.as_str()),
        Some("c1d3f072")
    );
    assert_eq!(
        draw_123
            .vs_binding
            .as_ref()
            .map(|binding| binding.hash.as_str()),
        Some("aabbccdd")
    );
    assert_eq!(
        draw_123
            .ps_binding
            .as_ref()
            .map(|binding| binding.hash.as_str()),
        Some("11223344")
    );
    assert!(matches!(
        draw_123.draw,
        Some(FrameAnalysisDraw::Indexed {
            index_count: 15234,
            start_index: 0,
            base_vertex: 0,
        })
    ));

    let draw_125 = &dump.draw_calls[2];
    assert_eq!(draw_125.vb_bindings.len(), 1);
    assert_eq!(draw_125.vb_bindings[0].hash, "2a3b4c5d");
    assert_eq!(
        draw_125
            .ib_binding
            .as_ref()
            .map(|binding| binding.hash.as_str()),
        Some("6e7f8a9b")
    );
    assert!(draw_125.vs_binding.is_none());
    assert!(draw_125.ps_binding.is_none());
}

#[test]
fn parser_handles_view_prefix_on_bind_lines() {
    let dump = parse_fixture_dump();
    let draw_126 = dump
        .draw_calls
        .iter()
        .find(|draw_call| draw_call.drawcall == 126)
        .expect("fixture drawcall 126 exists");

    assert_eq!(draw_126.vb_bindings.len(), 1);
    assert_eq!(
        draw_126.vb_bindings[0].view_address.as_deref(),
        Some("0xDEADBEEF")
    );
    assert_eq!(draw_126.vb_bindings[0].hash, "4a7d9c1f");
}

#[test]
fn parser_skips_unknown_api_calls() {
    let mut text = load_fixture_log_text();
    text.push_str(
        "\n000127 OMSetRenderTargets(NumViews:1, ppRenderTargetViews:0x12345678, pDepthStencilView:0x0)\n",
    );

    let dump = parse_frame_analysis_log(&text).expect("parse fixture with unknown API");

    assert_eq!(dump.draw_calls.len(), 4);
    assert!(
        !dump
            .draw_calls
            .iter()
            .any(|draw_call| draw_call.drawcall == 127)
    );
}

#[test]
fn dedupes_repeated_hashes_into_single_asset() {
    let dump = build_fixture_dump_for_inventory();
    let inventory = build_prepared_inventory(&dump, "0.0.0-fixture");

    let vb_assets = inventory
        .assets
        .iter()
        .filter(|asset| asset.hash_fields.asset_hash.as_deref() == Some("4a7d9c1f"))
        .collect::<Vec<_>>();

    assert_eq!(vb_assets.len(), 1);
    assert_eq!(vb_assets[0].asset.id, "vb_4a7d9c1f");
    assert_eq!(
        vb_assets[0].asset.metadata.tags,
        vec!["draw_calls=2".to_string()]
    );
}

#[test]
fn inventory_schema_round_trips_through_serde() {
    let dump = build_fixture_dump_for_inventory();
    let inventory = build_prepared_inventory(&dump, "0.0.0-fixture");

    let serialized = serde_json::to_string_pretty(&inventory).expect("serialize inventory");
    let parsed: PreparedAssetInventory =
        serde_json::from_str(&serialized).expect("deserialize inventory");

    assert_eq!(parsed, inventory);
}

#[test]
fn cli_command_writes_inventory_under_temp_dir() {
    let test_root = unique_test_dir();
    let output_path = test_root.join("out").join("inventory.json");

    run_ingest_frame_analysis_command(IngestFrameAnalysisArgs {
        dump_dir: fixture_dump_dir(),
        version_id: "0.0.0-fixture".to_string(),
        output: output_path.clone(),
        store_snapshot: false,
        report_root: None,
    })
    .expect("run ingest frame analysis command");

    assert!(output_path.exists());
    let contents = fs::read_to_string(&output_path).expect("read generated inventory");
    let inventory: PreparedAssetInventory =
        serde_json::from_str(&contents).expect("parse generated inventory");
    assert_eq!(inventory.schema_version, "whashreonator.prepared-assets.v1");
    assert_eq!(
        inventory.context.version_id.as_deref(),
        Some("0.0.0-fixture")
    );
    assert_eq!(inventory.assets.len(), 10);

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn cli_command_rejects_writing_into_src_or_tests() {
    let error = run_ingest_frame_analysis_command(IngestFrameAnalysisArgs {
        dump_dir: fixture_dump_dir(),
        version_id: "0.0.0-fixture".to_string(),
        output: PathBuf::from("src/frame-analysis-invalid.json"),
        store_snapshot: false,
        report_root: None,
    })
    .expect_err("src output path should be rejected");

    assert!(error.to_string().contains("not allowed under src"));
}

fn parse_fixture_dump() -> whashreonator::ingest::frame_analysis::FrameAnalysisDump {
    parse_frame_analysis_log(&load_fixture_log_text()).expect("parse synthetic fixture")
}

fn build_fixture_dump_for_inventory() -> whashreonator::ingest::frame_analysis::FrameAnalysisDump {
    let mut dump = parse_fixture_dump();
    dump.dump_dir = fixture_dump_dir()
        .canonicalize()
        .expect("canonicalize fixture dump dir");
    dump.log_path = fixture_log_path()
        .canonicalize()
        .expect("canonicalize fixture log path");
    dump
}

fn load_fixture_log_text() -> String {
    fs::read_to_string(fixture_log_path()).expect("read frame analysis fixture")
}

fn fixture_dump_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_frame_analysis")
}

fn fixture_log_path() -> PathBuf {
    fixture_dump_dir().join("log.txt")
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-frame-analysis-test-{nanos}"))
}
