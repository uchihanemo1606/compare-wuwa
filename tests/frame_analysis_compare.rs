use whashreonator::{
    compare::SnapshotComparer,
    domain::{AssetMetadata, AssetSourceContext},
    report::{VersionContinuityIndex, VersionDiffReportBuilder},
    snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
    },
};

#[test]
fn fa_snapshots_emit_changed_when_only_asset_hash_differs() {
    let old_snapshot = fa_snapshot("1.0.0", "old_hash", "shared_shader");
    let new_snapshot = fa_snapshot("1.1.0", "new_hash", "shared_shader");

    let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

    assert_eq!(report.summary.added_assets, 0);
    assert_eq!(report.summary.removed_assets, 0);
    assert_eq!(report.summary.changed_assets, 1);
    assert!(
        report.changed_assets[0]
            .changed_fields
            .iter()
            .any(|field| field == "asset_hash")
    );
}

#[test]
fn filesystem_snapshots_path_keyed_diff_unchanged_regression() {
    let old_snapshot = filesystem_snapshot("1.0.0", "Content/Character/Encore/Body.mesh");
    let new_snapshot = filesystem_snapshot("1.1.0", "Content/Character/Encore/BodyV2.mesh");

    let report = SnapshotComparer.compare(&old_snapshot, &new_snapshot);

    assert_eq!(report.summary.added_assets, 1);
    assert_eq!(report.summary.removed_assets, 1);
    assert_eq!(report.summary.changed_assets, 0);
}

#[test]
fn fa_continuity_thread_stays_stable_across_hash_shift() {
    let v1 = fa_snapshot("1.0.0", "hash_a", "shared_shader");
    let v2 = fa_snapshot("1.1.0", "hash_b", "shared_shader");
    let v3 = fa_snapshot("1.2.0", "hash_c", "shared_shader");

    let compare_12 = SnapshotComparer.compare(&v1, &v2);
    let compare_23 = SnapshotComparer.compare(&v2, &v3);
    let report_12 = VersionDiffReportBuilder.from_compare(&v1, &v2, &compare_12);
    let report_23 = VersionDiffReportBuilder.from_compare(&v2, &v3, &compare_23);
    let continuity = VersionContinuityIndex::from_reports(&[report_12, report_23]);

    assert_eq!(continuity.summary.thread_count, 1);
    assert_eq!(continuity.threads.len(), 1);
    assert_eq!(continuity.threads[0].observations.len() + 1, 3);
}

fn fa_snapshot(version_id: &str, asset_hash: &str, shader_hash: &str) -> GameSnapshot {
    let path = format!("runtime/vb/{asset_hash}");
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-fa".to_string(),
        asset_count: 1,
        assets: vec![SnapshotAsset {
            id: format!("vb_{asset_hash}"),
            path,
            identity_tuple: Some(format!("fa|vb|shader:{shader_hash}")),
            kind: Some("vertex_buffer".to_string()),
            metadata: AssetMetadata {
                logical_name: Some(format!("vb_{asset_hash}")),
                vertex_count: Some(24),
                vertex_buffer_count: Some(1),
                tags: vec!["draw_calls=1".to_string()],
                ..AssetMetadata::default()
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("vertex_buffer".to_string()),
                normalized_name: Some("vertex buffer".to_string()),
                name_tokens: vec!["vertex".to_string(), "buffer".to_string()],
                path_tokens: vec!["runtime".to_string(), "vb".to_string()],
                tags: vec!["draw_calls=1".to_string()],
                vertex_count: Some(24),
                vertex_buffer_count: Some(1),
                ..SnapshotFingerprint::default()
            },
            hash_fields: SnapshotHashFields {
                asset_hash: Some(asset_hash.to_string()),
                shader_hash: Some(shader_hash.to_string()),
                signature: None,
                identity_tuple: Some(format!("fa|vb|shader:{shader_hash}")),
            },
            source: AssetSourceContext::default(),
        }],
        context: SnapshotContext::default(),
    }
}

fn filesystem_snapshot(version_id: &str, path: &str) -> GameSnapshot {
    GameSnapshot {
        schema_version: "whashreonator.snapshot.v1".to_string(),
        version_id: version_id.to_string(),
        created_at_unix_ms: 1,
        source_root: "fixture-fs".to_string(),
        asset_count: 1,
        assets: vec![SnapshotAsset {
            id: path.to_string(),
            path: path.to_string(),
            identity_tuple: None,
            kind: Some("mesh".to_string()),
            metadata: AssetMetadata {
                logical_name: Some("Body".to_string()),
                vertex_count: Some(24),
                index_count: Some(48),
                ..AssetMetadata::default()
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: Some("body".to_string()),
                name_tokens: vec!["body".to_string()],
                path_tokens: path.split('/').map(|segment| segment.to_string()).collect(),
                vertex_count: Some(24),
                index_count: Some(48),
                ..SnapshotFingerprint::default()
            },
            hash_fields: SnapshotHashFields {
                asset_hash: Some(format!("hash-{path}")),
                shader_hash: None,
                signature: None,
                identity_tuple: None,
            },
            source: AssetSourceContext::default(),
        }],
        context: SnapshotContext::default(),
    }
}
