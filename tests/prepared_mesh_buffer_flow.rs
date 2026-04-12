use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use whashreonator::{
    compare::SnapshotComparer,
    inference::FixInferenceEngine,
    proposal::{ProposalEngine, ProposalStatus},
    snapshot::create_prepared_snapshot_from_file,
    wwmi::load_wwmi_knowledge,
};

#[test]
fn prepared_mesh_buffer_signals_drive_layout_safe_and_layout_risky_paths() {
    let test_root = unique_test_dir();
    fs::create_dir_all(&test_root).expect("create test root");
    let old_inventory = test_root.join("old.prepared.json");
    let new_inventory = test_root.join("new.prepared.json");
    let wwmi_knowledge = fixture_dir().join("wwmi_knowledge.json");

    write_inventory(
        &old_inventory,
        vec![
            asset(
                "mesh:body",
                "Content/Character/Encore/Body.mesh",
                "Encore Body",
                32,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-body",
                "pakchunk0-WindowsNoEditor.pak",
            ),
            asset(
                "mesh:hair",
                "Content/Character/Encore/Hair.mesh",
                "Encore Hair",
                32,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-hair",
                "pakchunk0-WindowsNoEditor.pak",
            ),
            asset(
                "mesh:cloak",
                "Content/Character/Encore/Cloak.mesh",
                "Encore Cloak",
                24,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-cloak",
                "pakchunk0-WindowsNoEditor.pak",
            ),
        ],
    );
    write_inventory(
        &new_inventory,
        vec![
            asset(
                "mesh:body",
                "Content/Character/Encore/Body.mesh",
                "Encore Body",
                48,
                2,
                "u32",
                "triangle_strip",
                &["skinned", "expanded"],
                "sig-body",
                "pakchunk1-WindowsNoEditor.pak",
            ),
            asset(
                "mesh:hair",
                "Content/Character/Encore/Hair_LOD0.mesh",
                "Encore Hair",
                32,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-hair",
                "pakchunk0-WindowsNoEditor.pak",
            ),
            asset(
                "mesh:cloak",
                "Content/Character/Encore/Cloak_LOD0.mesh",
                "Encore Cloak",
                40,
                2,
                "u32",
                "triangle_strip",
                &["skinned", "split"],
                "sig-cloak",
                "pakchunk0-WindowsNoEditor.pak",
            ),
        ],
    );

    let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
        .expect("old snapshot");
    let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
        .expect("new snapshot");

    assert_eq!(old_snapshot.assets[0].fingerprint.vertex_stride, Some(32));
    assert_eq!(
        old_snapshot.assets[0].fingerprint.index_format.as_deref(),
        Some("u16")
    );
    assert_eq!(
        old_snapshot.assets[0].fingerprint.layout_markers,
        vec!["interleaved".to_string(), "skinned".to_string()]
    );

    let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
    assert_eq!(compare_report.summary.layout_changed_assets, 1);
    assert_eq!(compare_report.summary.provenance_changed_assets, 1);
    assert_eq!(compare_report.summary.container_moved_assets, 1);

    let body_change = compare_report
        .changed_assets
        .iter()
        .find(|change| {
            change
                .old_asset
                .as_ref()
                .is_some_and(|asset| asset.path.ends_with("Body.mesh"))
        })
        .expect("body change");
    assert!(
        body_change
            .changed_fields
            .iter()
            .any(|field| field == "vertex_stride")
    );
    assert!(
        body_change
            .changed_fields
            .iter()
            .any(|field| field == "layout_markers")
    );
    assert!(
        body_change
            .changed_fields
            .iter()
            .any(|field| field == "container_path")
    );
    assert!(
        body_change
            .reasons
            .iter()
            .any(|reason| reason.code == "container_package_movement_detected")
    );

    let hair_candidate = compare_report
        .candidate_mapping_changes
        .iter()
        .find(|candidate| candidate.old_asset.path.ends_with("Hair.mesh"))
        .expect("hair candidate");
    assert_eq!(
        hair_candidate.compatibility,
        whashreonator::compare::RemapCompatibility::LikelyCompatible
    );
    assert!(
        hair_candidate
            .reasons
            .iter()
            .any(|reason| reason.code == "container_path_exact")
    );

    let cloak_candidate = compare_report
        .candidate_mapping_changes
        .iter()
        .find(|candidate| candidate.old_asset.path.ends_with("Cloak.mesh"))
        .expect("cloak candidate");
    assert_eq!(
        cloak_candidate.compatibility,
        whashreonator::compare::RemapCompatibility::StructurallyRisky
    );

    let knowledge = load_wwmi_knowledge(&wwmi_knowledge).expect("load wwmi knowledge");
    let inference = FixInferenceEngine.infer(&compare_report, &knowledge);

    assert!(
        inference
            .probable_crash_causes
            .iter()
            .any(|cause| cause.code == "buffer_layout_changed")
    );
    assert!(
        inference
            .probable_crash_causes
            .iter()
            .any(|cause| cause.code == "candidate_remap_structural_drift")
    );

    let proposals = ProposalEngine.generate(&inference, 0.85);
    let hair_mapping = proposals
        .mapping_proposal
        .mappings
        .iter()
        .find(|entry| entry.old_asset_path.ends_with("Hair.mesh"))
        .expect("hair mapping");
    assert_eq!(hair_mapping.status, ProposalStatus::Proposed);

    let cloak_mapping = proposals
        .mapping_proposal
        .mappings
        .iter()
        .find(|entry| entry.old_asset_path.ends_with("Cloak.mesh"))
        .expect("cloak mapping");
    assert_eq!(cloak_mapping.status, ProposalStatus::NeedsReview);

    let diff_report = whashreonator::report::VersionDiffReportBuilder.from_compare(
        &old_snapshot,
        &new_snapshot,
        &compare_report,
    );
    let body_report_item = diff_report
        .resonators
        .iter()
        .flat_map(|resonator| resonator.items.iter())
        .find(|item| {
            item.item_type == whashreonator::report::ReportItemType::Asset
                && item
                    .old
                    .as_ref()
                    .and_then(|asset| asset.path.as_deref())
                    .is_some_and(|path| path.ends_with("Body.mesh"))
        })
        .expect("body report item");
    assert_eq!(
        body_report_item.old.as_ref().and_then(|item| item
            .metadata
            .source
            .container_path
            .as_deref()),
        Some("pakchunk0-WindowsNoEditor.pak")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn prepared_internal_structure_signals_drive_section_drift_and_buffer_role_classification() {
    let test_root = unique_test_dir();
    fs::create_dir_all(&test_root).expect("create test root");
    let old_inventory = test_root.join("old.prepared.json");
    let new_inventory = test_root.join("new.prepared.json");

    write_inventory(
        &old_inventory,
        vec![
            asset_with_internal_structure(
                "mesh:body",
                "Content/Character/Encore/Body.mesh",
                "Encore Body",
                32,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-body",
                "pakchunk0-WindowsNoEditor.pak",
                &["base", "trim"],
                &["position", "normal"],
                &["albedo", "normal"],
                &["geometry"],
                Some(true),
                Some(false),
            ),
            asset_with_internal_structure(
                "mesh:hair",
                "Content/Character/Encore/Hair.mesh",
                "Encore Hair",
                24,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-hair",
                "pakchunk0-WindowsNoEditor.pak",
                &["section-a"],
                &["position", "normal"],
                &["albedo"],
                &["geometry"],
                Some(true),
                Some(false),
            ),
        ],
    );
    write_inventory(
        &new_inventory,
        vec![
            asset_with_internal_structure(
                "mesh:body",
                "Content/Character/Encore/Body.mesh",
                "Encore Body",
                32,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-body",
                "pakchunk0-WindowsNoEditor.pak",
                &["base", "trim", "accessory"],
                &["position", "normal"],
                &["albedo", "normal"],
                &["geometry", "mask"],
                Some(true),
                Some(false),
            ),
            asset_with_internal_structure(
                "mesh:hair",
                "Content/Character/Encore/Hair_LOD0.mesh",
                "Encore Hair",
                24,
                1,
                "u16",
                "triangle_list",
                &["skinned", "interleaved"],
                "sig-hair",
                "pakchunk0-WindowsNoEditor.pak",
                &["section-a"],
                &["tangent"],
                &["albedo"],
                &["geometry"],
                Some(true),
                Some(false),
            ),
        ],
    );

    let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
        .expect("old snapshot");
    let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
        .expect("new snapshot");

    let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

    let body_change = compare_report
        .changed_assets
        .iter()
        .find(|change| {
            change
                .old_asset
                .as_ref()
                .is_some_and(|asset| asset.path.ends_with("Body.mesh"))
        })
        .expect("body change");
    assert_eq!(
        body_change.lineage,
        whashreonator::compare::AssetLineageKind::LayoutDrift
    );
    assert!(
        body_change
            .changed_fields
            .iter()
            .any(|field| field == "internal_structure.section_labels")
    );
    assert!(
        body_change
            .changed_fields
            .iter()
            .any(|field| field == "internal_structure.subresource_roles")
    );

    let hair_candidate = compare_report
        .candidate_mapping_changes
        .iter()
        .find(|candidate| candidate.old_asset.path.ends_with("Hair.mesh"))
        .expect("hair candidate");
    assert_eq!(
        hair_candidate.compatibility,
        whashreonator::compare::RemapCompatibility::StructurallyRisky
    );
    assert!(
        hair_candidate
            .reasons
            .iter()
            .any(|reason| reason.code == "internal_buffer_roles_mismatch")
    );

    let diff_report = whashreonator::report::VersionDiffReportBuilder.from_compare(
        &old_snapshot,
        &new_snapshot,
        &compare_report,
    );
    let body_report_item = diff_report
        .resonators
        .iter()
        .flat_map(|resonator| resonator.items.iter())
        .find(|item| {
            item.item_type == whashreonator::report::ReportItemType::Asset
                && item
                    .old
                    .as_ref()
                    .and_then(|asset| asset.path.as_deref())
                    .is_some_and(|path| path.ends_with("Body.mesh"))
        })
        .expect("body report item");
    assert_eq!(
        body_report_item.old.as_ref().map(|item| item
            .metadata
            .internal_structure
            .section_labels
            .clone()),
        Some(vec!["base".to_string(), "trim".to_string()])
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn prepared_internal_structure_order_variations_do_not_trigger_false_drift() {
    let test_root = unique_test_dir();
    fs::create_dir_all(&test_root).expect("create test root");
    let old_inventory = test_root.join("old.prepared.json");
    let new_inventory = test_root.join("new.prepared.json");

    write_inventory(
        &old_inventory,
        vec![asset_with_internal_structure(
            "mesh:body",
            "Content/Character/Encore/Body.mesh",
            "Encore Body",
            32,
            1,
            "u16",
            "triangle_list",
            &["skinned", "interleaved"],
            "sig-body",
            "pakchunk0-WindowsNoEditor.pak",
            &["base", "trim"],
            &["position", "normal"],
            &["albedo", "normal"],
            &["geometry", "mask"],
            Some(true),
            Some(false),
        )],
    );
    write_inventory(
        &new_inventory,
        vec![asset_with_internal_structure(
            "mesh:body",
            "Content/Character/Encore/Body.mesh",
            "Encore Body",
            32,
            1,
            "u16",
            "triangle_list",
            &["skinned", "interleaved"],
            "sig-body",
            "pakchunk0-WindowsNoEditor.pak",
            &["trim", "base"],
            &["normal", "position"],
            &["normal", "albedo"],
            &["mask", "geometry"],
            Some(true),
            Some(false),
        )],
    );

    let old_snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &old_inventory)
        .expect("old snapshot");
    let new_snapshot = create_prepared_snapshot_from_file("6.1.0", &test_root, &new_inventory)
        .expect("new snapshot");

    assert_eq!(
        old_snapshot.assets[0]
            .fingerprint
            .internal_structure
            .section_labels,
        vec!["base".to_string(), "trim".to_string()]
    );
    assert_eq!(
        old_snapshot.assets[0]
            .fingerprint
            .internal_structure
            .buffer_roles,
        vec!["normal".to_string(), "position".to_string()]
    );
    assert_eq!(
        old_snapshot.assets[0]
            .fingerprint
            .internal_structure
            .binding_targets,
        vec!["albedo".to_string(), "normal".to_string()]
    );
    assert_eq!(
        old_snapshot.assets[0]
            .fingerprint
            .internal_structure
            .subresource_roles,
        vec!["geometry".to_string(), "mask".to_string()]
    );

    let compare_report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
    assert_eq!(compare_report.summary.changed_assets, 0);
    assert_eq!(compare_report.summary.unchanged_assets, 1);

    let _ = fs::remove_dir_all(&test_root);
}

fn asset(
    id: &str,
    path: &str,
    logical_name: &str,
    vertex_stride: u32,
    vertex_buffer_count: u32,
    index_format: &str,
    primitive_topology: &str,
    layout_markers: &[&str],
    signature: &str,
    container_path: &str,
) -> serde_json::Value {
    json!({
        "id": id,
        "path": path,
        "kind": "mesh",
        "metadata": {
            "logical_name": logical_name,
            "vertex_count": 12000,
            "index_count": 24000,
            "material_slots": 2,
            "section_count": 1,
            "vertex_stride": vertex_stride,
            "vertex_buffer_count": vertex_buffer_count,
            "index_format": index_format,
            "primitive_topology": primitive_topology,
            "layout_markers": layout_markers,
            "tags": ["character", "encore"]
        },
        "hash_fields": {
            "asset_hash": format!("hash-{id}"),
            "shader_hash": "shader-shared",
            "signature": signature
        },
        "source": {
            "extraction_tool": "fixture-extractor",
            "source_root": "D:/prepared",
            "source_path": path,
            "container_path": container_path,
            "source_kind": "mesh_record"
        }
    })
}

fn asset_with_internal_structure(
    id: &str,
    path: &str,
    logical_name: &str,
    vertex_stride: u32,
    vertex_buffer_count: u32,
    index_format: &str,
    primitive_topology: &str,
    layout_markers: &[&str],
    signature: &str,
    container_path: &str,
    section_labels: &[&str],
    buffer_roles: &[&str],
    binding_targets: &[&str],
    subresource_roles: &[&str],
    has_skeleton: Option<bool>,
    has_shapekey_data: Option<bool>,
) -> serde_json::Value {
    json!({
        "id": id,
        "path": path,
        "kind": "mesh",
        "metadata": {
            "logical_name": logical_name,
            "vertex_count": 12000,
            "index_count": 24000,
            "material_slots": 2,
            "section_count": 1,
            "vertex_stride": vertex_stride,
            "vertex_buffer_count": vertex_buffer_count,
            "index_format": index_format,
            "primitive_topology": primitive_topology,
            "layout_markers": layout_markers,
            "tags": ["character", "encore"],
            "internal_structure": {
                "section_labels": section_labels,
                "buffer_roles": buffer_roles,
                "binding_targets": binding_targets,
                "subresource_roles": subresource_roles,
                "has_skeleton": has_skeleton,
                "has_shapekey_data": has_shapekey_data
            }
        },
        "hash_fields": {
            "asset_hash": format!("hash-{id}"),
            "shader_hash": "shader-shared",
            "signature": signature
        },
        "source": {
            "extraction_tool": "fixture-extractor",
            "source_root": "D:/prepared",
            "source_path": path,
            "container_path": container_path,
            "source_kind": "mesh_record"
        }
    })
}

fn write_inventory(path: &PathBuf, assets: Vec<serde_json::Value>) {
    let inventory = json!({
        "schema_version": "whashreonator.prepared-assets.v1",
        "context": {
            "extraction_tool": "fixture-extractor",
            "extraction_kind": "asset_records",
            "source_root": "D:/prepared",
            "meaningful_content_coverage": true,
            "meaningful_character_coverage": true
        },
        "assets": assets
    });
    fs::write(
        path,
        serde_json::to_string_pretty(&inventory).expect("serialize inventory"),
    )
    .expect("write inventory");
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("version-regression")
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();
    std::env::temp_dir().join(format!("whashreonator-mesh-buffer-flow-{nanos}"))
}
