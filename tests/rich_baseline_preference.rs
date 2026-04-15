use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    report_storage::{ReportStorage, VersionArtifactKind},
    snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotCoverageSignals,
        SnapshotExtractorContext, SnapshotFingerprint, SnapshotHashFields, SnapshotLauncherContext,
        SnapshotResourceManifestContext, SnapshotScopeContext,
    },
};

#[test]
fn rich_aligned_extractor_baseline_is_preferred_over_shallow_fallback_for_same_version() {
    let test_root = unique_test_dir();
    let storage = ReportStorage::new(test_root.join("out").join("report"));
    let version_id = "6.0.0";

    let shallow = shallow_snapshot(version_id, 100);
    storage
        .save_snapshot_for_version(&shallow)
        .expect("save shallow baseline");
    let rich_path = alternate_snapshot_path(&storage, version_id, "extractor-rich");
    write_snapshot(&rich_path, &rich_extractor_snapshot(version_id, 140))
        .expect("write rich baseline");

    let selected = storage
        .select_snapshot_baseline_for_version(version_id)
        .expect("select baseline")
        .expect("baseline exists");

    assert_eq!(selected.path, rich_path);
    assert_eq!(selected.artifact_kind, VersionArtifactKind::Snapshot);
    assert_eq!(selected.evidence_posture, "extractor_backed_rich");
    assert_eq!(selected.inventory_alignment, "aligned");
    assert!(
        selected
            .selection_reason
            .contains("extractor-backed, version-aligned")
    );

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn misaligned_extractor_baseline_is_not_preferred_over_safer_shallow_baseline() {
    let test_root = unique_test_dir();
    let storage = ReportStorage::new(test_root.join("out").join("report"));
    let version_id = "6.1.0";

    let shallow = shallow_snapshot(version_id, 100);
    let canonical_path = storage
        .save_snapshot_for_version(&shallow)
        .expect("save shallow baseline");
    let misaligned_path = alternate_snapshot_path(&storage, version_id, "extractor-misaligned");
    write_snapshot(
        &misaligned_path,
        &misaligned_extractor_snapshot(version_id, 180),
    )
    .expect("write misaligned baseline");

    let selected = storage
        .select_snapshot_baseline_for_version(version_id)
        .expect("select baseline")
        .expect("baseline exists");

    assert_eq!(selected.path, canonical_path);
    assert_eq!(selected.evidence_posture, "shallow_support_only");
    assert_eq!(selected.inventory_alignment, "not_applicable");

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn weak_unverified_extractor_baseline_does_not_beat_stronger_safer_shallow_baseline() {
    let test_root = unique_test_dir();
    let storage = ReportStorage::new(test_root.join("out").join("report"));
    let version_id = "6.2.0";

    let shallow = shallow_snapshot(version_id, 100);
    let canonical_path = storage
        .save_snapshot_for_version(&shallow)
        .expect("save shallow baseline");
    let weak_path = alternate_snapshot_path(&storage, version_id, "extractor-unverified");
    write_snapshot(
        &weak_path,
        &weak_unverified_extractor_snapshot(version_id, 220),
    )
    .expect("write weak extractor baseline");

    let selected = storage
        .select_snapshot_baseline_for_version(version_id)
        .expect("select baseline")
        .expect("baseline exists");

    assert_eq!(selected.path, canonical_path);
    assert_eq!(selected.evidence_posture, "shallow_support_only");
    assert!(selected.selection_reason.contains("safer stored fallback"));

    let _ = fs::remove_dir_all(&test_root);
}

#[test]
fn compare_report_scope_notes_state_selected_baseline_and_why() {
    let test_root = unique_test_dir();
    let storage = ReportStorage::new(test_root.join("out").join("report"));
    let old_version = "7.0.0";
    let new_version = "7.1.0";

    storage
        .save_snapshot_for_version(&shallow_snapshot(old_version, 100))
        .expect("save old shallow baseline");
    let rich_old_path = alternate_snapshot_path(&storage, old_version, "extractor-rich");
    write_snapshot(&rich_old_path, &rich_extractor_snapshot(old_version, 140))
        .expect("write old rich baseline");
    storage
        .save_snapshot_for_version(&shallow_snapshot(new_version, 180))
        .expect("save new baseline");

    let report = storage
        .compare_versions(old_version, new_version)
        .expect("compare versions");

    assert!(report.scope_notes.iter().any(|note| {
        note.contains("selected baseline 7.0.0")
            && note.contains("posture=extractor_backed_rich")
            && note.contains("alignment=aligned")
            && note.contains("extractor-rich")
    }));
    assert!(report.scope_notes.iter().any(|note| {
        note.contains("selected baseline 7.1.0") && note.contains("posture=shallow_support_only")
    }));

    let _ = fs::remove_dir_all(&test_root);
}

fn alternate_snapshot_path(storage: &ReportStorage, version_id: &str, suffix: &str) -> PathBuf {
    storage
        .build_version_directory(version_id)
        .join("snapshot")
        .join(format!("wuwa_{}.{}.snapshot.v1.json", version_id, suffix))
}

fn write_snapshot(path: &Path, snapshot: &GameSnapshot) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(snapshot)?)?;
    Ok(())
}

fn shallow_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 1,
        assets: vec![asset(
            "Content/Character/Encore/Body.mesh",
            vertex_count,
            false,
        )],
        context: SnapshotContext {
            launcher: Some(SnapshotLauncherContext {
                source_file: "launcherDownloadConfig.json".to_string(),
                detected_version: version_id.to_string(),
                reuse_version: None,
                state: Some("ready".to_string()),
                is_pre_download: false,
                app_id: Some("50004".to_string()),
            }),
            resource_manifest: Some(SnapshotResourceManifestContext {
                source_file: "LocalGameResources.json".to_string(),
                resource_count: 1,
                matched_assets: 1,
                unmatched_snapshot_assets: 0,
            }),
            extractor: None,
            scope: SnapshotScopeContext {
                acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
                capture_mode: Some("local_filesystem_inventory_content_focused".to_string()),
                mostly_install_or_package_level: Some(true),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(false),
                meaningful_asset_record_enrichment: Some(false),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 1,
                    character_path_count: 1,
                    non_content_path_count: 0,
                },
                note: Some("fixture shallow fallback baseline".to_string()),
            },
            notes: Vec::new(),
        },
    }
}

fn rich_extractor_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 1,
        assets: vec![asset(
            "Content/Character/Encore/Body.mesh",
            vertex_count,
            true,
        )],
        context: SnapshotContext {
            launcher: Some(SnapshotLauncherContext {
                source_file: "launcherDownloadConfig.json".to_string(),
                detected_version: version_id.to_string(),
                reuse_version: None,
                state: Some("ready".to_string()),
                is_pre_download: false,
                app_id: Some("50004".to_string()),
            }),
            resource_manifest: Some(SnapshotResourceManifestContext {
                source_file: "LocalGameResources.json".to_string(),
                resource_count: 1,
                matched_assets: 1,
                unmatched_snapshot_assets: 0,
            }),
            extractor: Some(SnapshotExtractorContext {
                inventory_path: Some("fixture-rich.json".to_string()),
                inventory_schema_version: Some("whashreonator.prepared-assets.v1".to_string()),
                inventory_version_id: Some(version_id.to_string()),
                inventory_version_matches_snapshot: Some(true),
                launcher_version_matches_inventory: Some(true),
                extraction_tool: Some("fixture-extractor".to_string()),
                extraction_kind: Some("asset_records".to_string()),
                inventory_source_root: Some("D:/fixture".to_string()),
                tags: vec!["fixture".to_string()],
                note: Some("aligned rich extractor fixture".to_string()),
                record_count: 1,
                records_with_hashes: 1,
                records_with_source_context: 1,
                records_with_rich_metadata: 1,
            }),
            scope: SnapshotScopeContext {
                acquisition_kind: Some("extractor_backed_asset_records".to_string()),
                capture_mode: Some("extractor_backed_asset_records".to_string()),
                mostly_install_or_package_level: Some(false),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(true),
                meaningful_asset_record_enrichment: Some(true),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 12,
                    character_path_count: 6,
                    non_content_path_count: 0,
                },
                note: Some("fixture rich extractor baseline".to_string()),
            },
            notes: Vec::new(),
        },
    }
}

fn misaligned_extractor_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
    let mut snapshot = rich_extractor_snapshot(version_id, vertex_count);
    if let Some(extractor) = snapshot.context.extractor.as_mut() {
        extractor.inventory_version_id = Some("6.0.9".to_string());
        extractor.inventory_version_matches_snapshot = Some(false);
        extractor.launcher_version_matches_inventory = Some(false);
        extractor.note = Some("misaligned extractor fixture".to_string());
    }
    snapshot.context.scope.note = Some("fixture misaligned extractor baseline".to_string());
    snapshot
}

fn weak_unverified_extractor_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-root".to_string(),
        asset_count: 1,
        assets: vec![asset(
            "Content/Character/Encore/Body.mesh",
            vertex_count,
            false,
        )],
        context: SnapshotContext {
            launcher: None,
            resource_manifest: None,
            extractor: Some(SnapshotExtractorContext {
                inventory_path: Some("fixture-unverified.json".to_string()),
                inventory_schema_version: Some("whashreonator.prepared-assets.v1".to_string()),
                inventory_version_id: None,
                inventory_version_matches_snapshot: None,
                launcher_version_matches_inventory: None,
                extraction_tool: Some("fixture-extractor".to_string()),
                extraction_kind: Some("asset_records".to_string()),
                inventory_source_root: Some("D:/fixture".to_string()),
                tags: vec!["fixture".to_string()],
                note: Some("undeclared weak extractor fixture".to_string()),
                record_count: 1,
                records_with_hashes: 1,
                records_with_source_context: 0,
                records_with_rich_metadata: 0,
            }),
            scope: SnapshotScopeContext {
                acquisition_kind: Some("extractor_backed_asset_records".to_string()),
                capture_mode: Some("extractor_backed_asset_records".to_string()),
                mostly_install_or_package_level: Some(false),
                meaningful_content_coverage: Some(true),
                meaningful_character_coverage: Some(false),
                meaningful_asset_record_enrichment: Some(false),
                coverage: SnapshotCoverageSignals {
                    content_like_path_count: 1,
                    character_path_count: 1,
                    non_content_path_count: 0,
                },
                note: Some("fixture weak extractor baseline".to_string()),
            },
            notes: Vec::new(),
        },
    }
}

fn asset(path: &str, vertex_count: u32, enriched: bool) -> SnapshotAsset {
    SnapshotAsset {
        id: path.to_string(),
        path: path.to_string(),
        kind: Some("mesh".to_string()),
        metadata: whashreonator::domain::AssetMetadata {
            logical_name: Some("Encore Body".to_string()),
            vertex_count: Some(vertex_count),
            index_count: Some(vertex_count * 2),
            material_slots: Some(2),
            section_count: Some(1),
            layout_markers: if enriched {
                vec!["skinned".to_string(), "interleaved".to_string()]
            } else {
                Vec::new()
            },
            internal_structure: if enriched {
                whashreonator::domain::AssetInternalStructure {
                    section_labels: vec!["base".to_string()],
                    buffer_roles: vec!["position".to_string()],
                    binding_targets: vec!["albedo".to_string()],
                    subresource_roles: vec!["geometry".to_string()],
                    has_skeleton: Some(true),
                    has_shapekey_data: Some(false),
                }
            } else {
                Default::default()
            },
            tags: vec!["character".to_string()],
            ..Default::default()
        },
        fingerprint: SnapshotFingerprint {
            normalized_kind: Some("mesh".to_string()),
            normalized_name: Some("encore body".to_string()),
            name_tokens: vec!["encore".to_string(), "body".to_string()],
            path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
            tags: vec!["character".to_string()],
            vertex_count: Some(vertex_count),
            index_count: Some(vertex_count * 2),
            material_slots: Some(2),
            section_count: Some(1),
            vertex_stride: if enriched { Some(32) } else { None },
            vertex_buffer_count: if enriched { Some(1) } else { None },
            index_format: if enriched {
                Some("u16".to_string())
            } else {
                None
            },
            primitive_topology: if enriched {
                Some("triangle_list".to_string())
            } else {
                None
            },
            layout_markers: if enriched {
                vec!["skinned".to_string(), "interleaved".to_string()]
            } else {
                Vec::new()
            },
            internal_structure: if enriched {
                whashreonator::domain::AssetInternalStructure {
                    section_labels: vec!["base".to_string()],
                    buffer_roles: vec!["position".to_string()],
                    binding_targets: vec!["albedo".to_string()],
                    subresource_roles: vec!["geometry".to_string()],
                    has_skeleton: Some(true),
                    has_shapekey_data: Some(false),
                }
            } else {
                Default::default()
            },
        },
        hash_fields: SnapshotHashFields {
            asset_hash: Some(format!("hash-{vertex_count}")),
            shader_hash: if enriched {
                Some("shader-shared".to_string())
            } else {
                None
            },
            signature: Some(format!("sig-{vertex_count}")),
        },
        source: if enriched {
            whashreonator::domain::AssetSourceContext {
                extraction_tool: Some("fixture-extractor".to_string()),
                source_root: Some("D:/fixture".to_string()),
                source_path: Some(path.to_string()),
                container_path: Some("pakchunk0-WindowsNoEditor.pak".to_string()),
                source_kind: Some("mesh_record".to_string()),
            }
        } else {
            Default::default()
        },
    }
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();
    std::env::temp_dir().join(format!("whashreonator-rich-baseline-test-{nanos}"))
}
