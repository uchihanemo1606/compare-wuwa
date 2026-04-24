use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    ingest::{
        LocalSnapshotIngestSource, SnapshotAssetExtractor,
        frame_analysis::{FrameAnalysisDump, build_prepared_inventory, parse_frame_analysis_log},
    },
    snapshot::create_snapshot_with_extractor,
};

#[test]
fn fa_vb_identity_tuple_is_stable_when_only_asset_hash_changes() {
    let old_inventory = build_prepared_inventory(&vb_dump("ab12cd34", Some("feedc0de")), "1.0.0");
    let new_inventory = build_prepared_inventory(&vb_dump("ef56ab78", Some("feedc0de")), "1.1.0");

    let old_record = old_inventory
        .assets
        .iter()
        .find(|record| record.asset.kind.as_deref() == Some("vertex_buffer"))
        .expect("old vb record");
    let new_record = new_inventory
        .assets
        .iter()
        .find(|record| record.asset.kind.as_deref() == Some("vertex_buffer"))
        .expect("new vb record");

    assert_eq!(
        old_record.hash_fields.identity_tuple,
        Some("fa|vb|shader:feedc0de".to_string())
    );
    assert_eq!(
        old_record.hash_fields.identity_tuple,
        new_record.hash_fields.identity_tuple
    );
}

#[test]
fn fa_ib_identity_tuple_is_stable_when_only_asset_hash_changes() {
    let old_inventory = build_prepared_inventory(&ib_dump("aa11bb22"), "1.0.0");
    let new_inventory = build_prepared_inventory(&ib_dump("cc33dd44"), "1.1.0");

    let old_record = old_inventory
        .assets
        .iter()
        .find(|record| record.asset.kind.as_deref() == Some("index_buffer"))
        .expect("old ib record");
    let new_record = new_inventory
        .assets
        .iter()
        .find(|record| record.asset.kind.as_deref() == Some("index_buffer"))
        .expect("new ib record");

    // The parser now normalizes IB format strings to canonical names
    // (R16_UINT, R32_UINT) so synthetic fixture values and live DXGI numeric
    // codes both produce the same identity_tuple.
    assert_eq!(
        old_record.hash_fields.identity_tuple,
        Some("fa|ib|idx_fmt:R16_UINT|idx_count:42".to_string())
    );
    assert_eq!(
        old_record.hash_fields.identity_tuple,
        new_record.hash_fields.identity_tuple
    );
}

#[test]
fn fa_vb_without_shader_hash_has_no_identity_tuple() {
    let inventory = build_prepared_inventory(&vb_dump("1234abcd", None), "1.0.0");
    let record = inventory
        .assets
        .iter()
        .find(|asset| asset.asset.kind.as_deref() == Some("vertex_buffer"))
        .expect("vb record");

    assert_eq!(record.hash_fields.identity_tuple, None);
}

#[test]
fn filesystem_assets_keep_identity_tuple_none() {
    let test_root = unique_test_dir("filesystem-identity");
    let asset_path = test_root
        .join("Content")
        .join("Character")
        .join("Encore")
        .join("Body.mesh");
    fs::create_dir_all(asset_path.parent().expect("asset parent")).expect("create asset parent");
    fs::write(&asset_path, b"mesh").expect("write asset");

    let records = LocalSnapshotIngestSource
        .extract_snapshot_assets(&test_root)
        .expect("extract local assets");
    assert!(!records.is_empty());
    assert!(
        records
            .iter()
            .all(|record| record.hash_fields.identity_tuple.is_none())
    );

    let snapshot = create_snapshot_with_extractor("fs-1", &test_root, LocalSnapshotIngestSource)
        .expect("build snapshot");
    assert!(
        snapshot
            .assets
            .iter()
            .all(|asset| asset.identity_tuple.is_none())
    );

    let _ = fs::remove_dir_all(&test_root);
}

fn vb_dump(vb_hash: &str, shader_hash: Option<&str>) -> FrameAnalysisDump {
    let vs_section = shader_hash
        .map(|hash| format!("000001 VSSetShader()\n 0: resource=0x200 hash={hash}\n"))
        .unwrap_or_default();
    let log = format!(
        "analyse_options=dump_rt dump_tex dump_vb dump_ib\n000001 IASetVertexBuffers(StartSlot:0, NumBuffers:1, ppVertexBuffers:0x1, pStrides:0x1, pOffsets:0x0)\n 0: resource=0x100 hash={vb_hash}\n{vs_section}000001 Draw(VertexCount:24, StartVertexLocation:0)\n"
    );

    parse_dump(&log)
}

fn ib_dump(ib_hash: &str) -> FrameAnalysisDump {
    let log = format!(
        "analyse_options=dump_rt dump_tex dump_vb dump_ib\n000001 IASetIndexBuffer(pIndexBuffer:0x1, Format:DXGI_FORMAT_R16_UINT, Offset:0)\n 0: resource=0x100 hash={ib_hash}\n000001 DrawIndexed(IndexCount:42, StartIndexLocation:0, BaseVertexLocation:0)\n"
    );

    parse_dump(&log)
}

fn unique_test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-{label}-{nanos}"))
}

fn parse_dump(text: &str) -> FrameAnalysisDump {
    parse_frame_analysis_log(text).expect("parse synthetic frame analysis log")
}
