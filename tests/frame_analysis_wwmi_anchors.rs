use whashreonator::ingest::frame_analysis::{
    FrameAnalysisDraw, build_prepared_inventory, parse_frame_analysis_log,
};

const SAMPLE_LOG: &str = "analyse_options: 0000063d
000001 CSSetShader(pComputeShader:0x000000005D8B1E38, ppClassInstances:0x0000000000000000, NumClassInstances:0) hash=9bf4420c82102011
000001 Dispatch(ThreadGroupCountX:29747, ThreadGroupCountY:1, ThreadGroupCountZ:1)
000002 CSSetShader(pComputeShader:0x0000000000000000, ppClassInstances:0x0000000000000000, NumClassInstances:0)
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

#[test]
fn parses_cssetshader_inline_hash_and_dispatch_thread_group() {
    let dump = parse_frame_analysis_log(SAMPLE_LOG).expect("parse log");

    let first_dispatch = dump
        .draw_calls
        .iter()
        .find(|dc| dc.drawcall == 1)
        .expect("drawcall 1 present");
    let cs = first_dispatch
        .cs_binding
        .as_ref()
        .expect("cs binding parsed on drawcall 1");
    assert_eq!(cs.hash, "9bf4420c82102011");
    matches!(
        first_dispatch.draw,
        Some(FrameAnalysisDraw::Compute {
            thread_group_x: 29747,
            thread_group_y: 1,
            thread_group_z: 1
        })
    );
}

#[test]
fn unbound_compute_shader_line_does_not_error_or_emit_asset() {
    // drawcall 2's CSSetShader has pComputeShader:0x0 and no inline hash:
    // this is a runtime unbind, not a binding. Parser must handle gracefully.
    let dump = parse_frame_analysis_log(SAMPLE_LOG).expect("parse log");
    let drawcall_2 = dump.draw_calls.iter().find(|dc| dc.drawcall == 2);
    // Either drawcall 2 is absent (no meaningful binding, draw, etc.) or it
    // exists with cs_binding None — both are acceptable. The important thing
    // is we did not error out of parse_frame_analysis_log.
    if let Some(dc) = drawcall_2 {
        assert!(dc.cs_binding.is_none());
    }
}

#[test]
fn inventory_surfaces_compute_shader_and_ps_texture_anchors() {
    let dump = parse_frame_analysis_log(SAMPLE_LOG).expect("parse log");
    let inventory = build_prepared_inventory(&dump, "0.0.0-fixture");

    let cs_hashes: Vec<_> = inventory
        .assets
        .iter()
        .filter(|record| record.asset.kind.as_deref() == Some("compute_shader"))
        .filter_map(|record| record.hash_fields.asset_hash.clone())
        .collect();
    assert!(cs_hashes.contains(&"9bf4420c82102011".to_string()));
    assert!(cs_hashes.contains(&"7a8396180d416117".to_string()));

    let texture_hashes: Vec<_> = inventory
        .assets
        .iter()
        .filter(|record| record.asset.kind.as_deref() == Some("texture_resource"))
        .filter_map(|record| record.hash_fields.asset_hash.clone())
        .collect();
    assert!(texture_hashes.contains(&"2cc576e7".to_string()));
    assert!(texture_hashes.contains(&"ce6a251a".to_string()));
}

#[test]
fn compute_shader_records_carry_wwmi_anchor_candidate_tag() {
    let dump = parse_frame_analysis_log(SAMPLE_LOG).expect("parse log");
    let inventory = build_prepared_inventory(&dump, "0.0.0-fixture");

    for record in &inventory.assets {
        if record.asset.kind.as_deref() == Some("compute_shader")
            || record.asset.kind.as_deref() == Some("texture_resource")
        {
            assert!(
                record
                    .asset
                    .metadata
                    .tags
                    .iter()
                    .any(|tag| tag == "wwmi-anchor-candidate"),
                "{:?} record should carry wwmi-anchor-candidate tag, got tags {:?}",
                record.asset.kind,
                record.asset.metadata.tags
            );
        }
    }
}

#[test]
fn compute_shader_identity_tuple_uses_single_observed_thread_group() {
    let dump = parse_frame_analysis_log(SAMPLE_LOG).expect("parse log");
    let inventory = build_prepared_inventory(&dump, "0.0.0-fixture");

    let shape_key_loader = inventory
        .assets
        .iter()
        .find(|record| record.hash_fields.asset_hash.as_deref() == Some("9bf4420c82102011"))
        .expect("shape key loader cs record present");
    assert_eq!(
        shape_key_loader.hash_fields.identity_tuple.as_deref(),
        Some("fa|cs|tg:29747x1x1")
    );

    let shape_key_multiplier = inventory
        .assets
        .iter()
        .find(|record| record.hash_fields.asset_hash.as_deref() == Some("7a8396180d416117"))
        .expect("shape key multiplier cs record present");
    assert_eq!(
        shape_key_multiplier.hash_fields.identity_tuple.as_deref(),
        Some("fa|cs|tg:100x1x1")
    );
}

#[test]
fn ps_texture_identity_tuple_ties_to_parent_pixel_shader_and_slot() {
    let dump = parse_frame_analysis_log(SAMPLE_LOG).expect("parse log");
    let inventory = build_prepared_inventory(&dump, "0.0.0-fixture");

    let side_gradients = inventory
        .assets
        .iter()
        .find(|record| record.hash_fields.asset_hash.as_deref() == Some("2cc576e7"))
        .expect("side-gradients texture record");
    assert_eq!(
        side_gradients.hash_fields.identity_tuple.as_deref(),
        Some("fa|tex|ps:a24b0fd936f39dcc|slot:0")
    );

    let background = inventory
        .assets
        .iter()
        .find(|record| record.hash_fields.asset_hash.as_deref() == Some("ce6a251a"))
        .expect("background texture record");
    assert_eq!(
        background.hash_fields.identity_tuple.as_deref(),
        Some("fa|tex|ps:a24b0fd936f39dcc|slot:1")
    );
}
