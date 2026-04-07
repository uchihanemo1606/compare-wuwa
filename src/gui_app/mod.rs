use std::path::{Path, PathBuf};

use crate::{
    error::AppResult,
    output_policy::resolve_artifact_root,
    report::{ReportItemType, ResonatorDiffEntry, VersionDiffReportV2},
    report_storage::{ReportStorage, VersionArtifactKind},
    scan::{
        ExecuteVersionScanResult, LocalSnapshotFactory, PrepareVersionScanResult,
        PreparedVersionScan, VersionScanService,
    },
    snapshot::GameSnapshot,
};

#[derive(Debug, Clone)]
pub struct ScanForm {
    pub source_root: String,
    pub version_override: String,
    pub knowledge_path: String,
}

impl Default for ScanForm {
    fn default() -> Self {
        Self {
            source_root: String::new(),
            version_override: String::new(),
            knowledge_path: resolve_artifact_root()
                .join("wwmi-knowledge.json")
                .display()
                .to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VersionRowView {
    pub version_id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct VersionDetailView {
    pub summary: String,
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReportDetailView {
    pub summary: String,
    pub old_column: String,
    pub new_column: String,
}

#[derive(Debug, Clone)]
pub enum ScanStartResult {
    Ready(PreparedVersionScan),
    VersionAlreadyExists(PreparedVersionScan),
}

#[derive(Debug, Clone)]
pub enum ScanRunResult {
    Created {
        version_id: String,
        saved_path: PathBuf,
        summary: String,
    },
    NoChangesDetected {
        version_id: String,
        saved_path: PathBuf,
        summary: String,
    },
    Overwritten {
        version_id: String,
        saved_path: PathBuf,
        summary: String,
    },
}

#[derive(Debug, Clone)]
pub struct GuiController {
    storage: ReportStorage,
}

impl Default for GuiController {
    fn default() -> Self {
        Self {
            storage: ReportStorage::default(),
        }
    }
}

impl GuiController {
    pub fn new(storage: ReportStorage) -> Self {
        Self { storage }
    }

    pub fn prepare_scan(&self, form: &ScanForm) -> AppResult<ScanStartResult> {
        let service = VersionScanService::new(self.storage.clone(), LocalSnapshotFactory);
        let source_root = Path::new(form.source_root.trim());
        let version_override = optional_text(&form.version_override);

        match service.prepare_scan(source_root, version_override.as_deref())? {
            PrepareVersionScanResult::Ready(prepared) => Ok(ScanStartResult::Ready(prepared)),
            PrepareVersionScanResult::VersionAlreadyExists(prepared) => {
                Ok(ScanStartResult::VersionAlreadyExists(prepared))
            }
        }
    }

    pub fn run_scan(
        &self,
        prepared: &PreparedVersionScan,
        force_rescan: bool,
    ) -> AppResult<ScanRunResult> {
        let service = VersionScanService::new(self.storage.clone(), LocalSnapshotFactory);

        match service.execute_scan(prepared, force_rescan)? {
            ExecuteVersionScanResult::Created {
                version_id,
                snapshot_path,
                snapshot,
            } => Ok(ScanRunResult::Created {
                version_id,
                saved_path: snapshot_path,
                summary: render_scan_summary(&snapshot),
            }),
            ExecuteVersionScanResult::NoChangesDetected {
                version_id,
                snapshot_path,
            } => Ok(ScanRunResult::NoChangesDetected {
                summary: format!(
                    "Scan Version {}\nNo changes detected.\nSnapshot: {}",
                    version_id,
                    snapshot_path.display()
                ),
                version_id,
                saved_path: snapshot_path,
            }),
            ExecuteVersionScanResult::Overwritten {
                version_id,
                snapshot_path,
                snapshot,
            } => Ok(ScanRunResult::Overwritten {
                version_id,
                saved_path: snapshot_path,
                summary: render_scan_summary(&snapshot),
            }),
        }
    }

    pub fn list_versions(&self) -> AppResult<Vec<VersionRowView>> {
        let versions = self.storage.list_versions()?;
        Ok(versions
            .into_iter()
            .map(|entry| VersionRowView {
                label: format!(
                    "wuwa_{} | snapshot={} report_bundle={} buffer={} hash={} artifacts={}",
                    entry.version_id,
                    yes_no(entry.has_snapshot),
                    yes_no(entry.has_report_bundle),
                    yes_no(entry.has_buffer_data),
                    yes_no(entry.has_hash_data),
                    entry.artifacts.len()
                ),
                version_id: entry.version_id,
            })
            .collect())
    }

    pub fn open_version(&self, version_id: &str) -> AppResult<VersionDetailView> {
        let artifacts = self.storage.list_version_artifacts(version_id)?;
        let snapshot = self.storage.load_snapshot_by_version(version_id)?;
        let report = self.storage.load_version_report(version_id)?;

        let summary =
            render_version_summary(version_id, snapshot.as_ref(), report.as_ref(), &artifacts);
        let artifacts = render_version_artifact_lines(&artifacts);

        Ok(VersionDetailView { summary, artifacts })
    }

    pub fn compare_versions(
        &self,
        old_version: &str,
        new_version: &str,
        resonator_filter: &str,
    ) -> AppResult<ReportDetailView> {
        let report = self.storage.compare_versions(old_version, new_version)?;
        Ok(render_detail_view(&report, resonator_filter))
    }

    pub fn reports_root_label(&self) -> String {
        self.storage.reports_root().display().to_string()
    }

    pub fn artifact_root_label(&self) -> String {
        resolve_artifact_root().display().to_string()
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn optional_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn render_scan_summary(snapshot: &crate::snapshot::GameSnapshot) -> String {
    format!(
        "Scan Version {}\nAssets: {}\nSource root: {}\nLauncher version: {}",
        snapshot.version_id,
        snapshot.asset_count,
        snapshot.source_root,
        snapshot
            .context
            .launcher
            .as_ref()
            .map(|launcher| launcher.detected_version.as_str())
            .unwrap_or("n/a")
    )
}

fn render_version_summary(
    version_id: &str,
    snapshot: Option<&GameSnapshot>,
    report: Option<&VersionDiffReportV2>,
    artifacts: &[crate::report_storage::VersionArtifactEntry],
) -> String {
    let mut lines = vec![
        format!("Version: wuwa_{version_id}"),
        format!("Artifacts: {}", artifacts.len()),
    ];

    if let Some(snapshot) = snapshot {
        lines.push(format!(
            "Snapshot: assets={} source_root={}",
            snapshot.asset_count, snapshot.source_root
        ));
        if let Some(launcher) = snapshot.context.launcher.as_ref() {
            lines.push(format!(
                "Launcher detected_version={} reuse_version={}",
                launcher.detected_version,
                launcher.reuse_version.as_deref().unwrap_or("-")
            ));
        }
        if let Some(manifest) = snapshot.context.resource_manifest.as_ref() {
            lines.push(format!(
                "Hash context: resources={} matched_assets={} unmatched_assets={}",
                manifest.resource_count,
                manifest.matched_assets,
                manifest.unmatched_snapshot_assets
            ));
        }
    } else {
        lines.push("Snapshot: not found".to_string());
    }

    if let Some(report) = report {
        lines.push(format!(
            "Latest report bundle: {} -> {} | changed={} added={} removed={} mapping_candidates={}",
            report.old_version.version_id,
            report.new_version.version_id,
            report.summary.changed_items,
            report.summary.added_items,
            report.summary.removed_items,
            report.summary.mapping_candidates
        ));
    } else {
        lines.push("Latest report bundle: not found".to_string());
    }

    lines.join("\n")
}

fn render_version_artifact_lines(
    artifacts: &[crate::report_storage::VersionArtifactEntry],
) -> Vec<String> {
    artifacts
        .iter()
        .map(|artifact| {
            format!(
                "{} | {}",
                artifact_kind_label(artifact.kind),
                artifact.path.display()
            )
        })
        .collect()
}

fn artifact_kind_label(kind: VersionArtifactKind) -> &'static str {
    match kind {
        VersionArtifactKind::Snapshot => "snapshot",
        VersionArtifactKind::ReportBundle => "report_bundle",
        VersionArtifactKind::BufferData => "buffer",
        VersionArtifactKind::HashData => "hash",
        VersionArtifactKind::Auxiliary => "auxiliary",
        VersionArtifactKind::LegacySnapshot => "legacy_snapshot",
        VersionArtifactKind::LegacyReportBundle => "legacy_report_bundle",
    }
}

pub fn render_detail_view(
    report: &VersionDiffReportV2,
    resonator_filter: &str,
) -> ReportDetailView {
    let filter = resonator_filter.to_ascii_lowercase();
    let resonators = report
        .resonators
        .iter()
        .filter(|entry| filter.is_empty() || entry.resonator.to_ascii_lowercase().contains(&filter))
        .collect::<Vec<_>>();

    ReportDetailView {
        summary: format!(
            "Compare {} -> {}\nResonators: {}\nUnchanged: {}\nChanged: {}\nAdded: {}\nRemoved: {}\nUncertain: {}\nMapping candidates: {}",
            report.old_version.version_id,
            report.new_version.version_id,
            report.summary.resonator_count,
            report.summary.unchanged_items,
            report.summary.changed_items,
            report.summary.added_items,
            report.summary.removed_items,
            report.summary.uncertain_items,
            report.summary.mapping_candidates
        ),
        old_column: render_side_column(&report.old_version.version_id, &resonators, true),
        new_column: render_side_column(&report.new_version.version_id, &resonators, false),
    }
}

fn render_side_column(
    version_label: &str,
    resonators: &[&ResonatorDiffEntry],
    render_old: bool,
) -> String {
    let mut lines = vec![format!("{version_label}")];
    for entry in resonators {
        let counts = if render_old {
            &entry.old_version
        } else {
            &entry.new_version
        };
        lines.push(String::new());
        lines.push(format!(
            "Resonator: {} | assets={} buffers={} mappings={}",
            entry.resonator, counts.asset_count, counts.buffer_count, counts.mapping_count
        ));
        for item in &entry.items {
            let side = if render_old { &item.old } else { &item.new };
            let Some(side) = side else {
                continue;
            };
            lines.push(format!(
                "- {:?} | {} | status={:?} | confidence={}",
                item.item_type,
                side.path.clone().unwrap_or_else(|| side.label.clone()),
                item.status,
                item.confidence
                    .map(|value| format!("{value:.3}"))
                    .unwrap_or_else(|| "-".to_string())
            ));
            if matches!(
                item.item_type,
                ReportItemType::Asset | ReportItemType::Buffer
            ) {
                lines.push(format!(
                    "  metadata: kind={:?}, name={:?}, vertex={:?}, index={:?}, slots={:?}, sections={:?}, asset_hash={:?}, shader_hash={:?}",
                    side.metadata.kind,
                    side.metadata.normalized_name,
                    side.metadata.vertex_count,
                    side.metadata.index_count,
                    side.metadata.material_slots,
                    side.metadata.section_count,
                    side.metadata.asset_hash,
                    side.metadata.shader_hash
                ));
            }
            if !item.reasons.is_empty() {
                lines.push(format!(
                    "  reasons: {}",
                    item.reasons
                        .iter()
                        .map(|reason| format!("[{}] {}", reason.code, reason.message))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use crate::{
        domain::AssetMetadata,
        report::{
            DiffStatus, ReportItemType, ReportReason, ResonatorDiffEntry, ResonatorItemDiff,
            ResonatorVersionView, TechnicalMetadata, VersionDiffReportV2, VersionDiffSummary,
            VersionSide, VersionedItem,
        },
        report_storage::ReportStorage,
        snapshot::{
            GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
        },
    };

    use super::{GuiController, ScanForm, ScanRunResult, render_detail_view};

    #[test]
    fn detail_view_renders_two_columns_and_resonator_items() {
        let report = VersionDiffReportV2 {
            schema_version: "whashreonator.report.v2".to_string(),
            generated_at_unix_ms: 1,
            old_version: VersionSide {
                version_id: "2.4.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_version: VersionSide {
                version_id: "2.5.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            summary: VersionDiffSummary {
                resonator_count: 1,
                unchanged_items: 0,
                changed_items: 1,
                added_items: 0,
                removed_items: 0,
                uncertain_items: 0,
                mapping_candidates: 0,
            },
            resonators: vec![ResonatorDiffEntry {
                resonator: "Encore".to_string(),
                old_version: ResonatorVersionView {
                    asset_count: 1,
                    buffer_count: 1,
                    mapping_count: 0,
                },
                new_version: ResonatorVersionView {
                    asset_count: 1,
                    buffer_count: 1,
                    mapping_count: 0,
                },
                items: vec![ResonatorItemDiff {
                    item_type: ReportItemType::Asset,
                    status: DiffStatus::Changed,
                    confidence: Some(0.9),
                    old: Some(item("Content/Character/Encore/Body.mesh")),
                    new: Some(item("Content/Character/Encore/Body_v2.mesh")),
                    reasons: vec![ReportReason {
                        code: "vertex_count_changed".to_string(),
                        message: "vertex count changed".to_string(),
                    }],
                }],
            }],
        };

        let detail = render_detail_view(&report, "");
        assert!(detail.summary.contains("Compare 2.4.0 -> 2.5.0"));
        assert!(detail.old_column.contains("Resonator: Encore"));
        assert!(detail.new_column.contains("Body_v2.mesh"));
    }

    #[test]
    fn controller_lists_version_after_scan_flow() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::with_legacy_root(
            test_root.join("out").join("report"),
            test_root.join("out").join("reports"),
        );
        let controller = GuiController::new(storage);
        let game_root = test_root.join("game");
        seed_game_root(&game_root, "3.2.1");

        let prepare = controller
            .prepare_scan(&ScanForm {
                source_root: game_root.display().to_string(),
                version_override: String::new(),
                knowledge_path: String::new(),
            })
            .expect("prepare scan");
        let prepared = match prepare {
            super::ScanStartResult::Ready(prepared) => prepared,
            other => panic!("expected ready, got {other:?}"),
        };

        let result = controller.run_scan(&prepared, false).expect("run scan");
        assert!(matches!(result, ScanRunResult::Created { .. }));

        let versions = controller.list_versions().expect("list versions");
        assert!(versions.iter().any(|version| version.version_id == "3.2.1"));

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn controller_compares_two_versions_from_storage() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::with_legacy_root(
            test_root.join("out").join("report"),
            test_root.join("out").join("reports"),
        );
        storage
            .save_snapshot_for_version(&sample_snapshot("3.2.5", 1))
            .expect("save 3.2.5");
        std::thread::sleep(Duration::from_millis(2));
        storage
            .save_snapshot_for_version(&sample_snapshot("3.3.1", 2))
            .expect("save 3.3.1");
        let controller = GuiController::new(storage);

        let detail = controller
            .compare_versions("3.2.5", "3.3.1", "")
            .expect("compare");
        assert!(detail.summary.contains("Compare 3.2.5 -> 3.3.1"));

        let _ = fs::remove_dir_all(test_root);
    }

    fn seed_game_root(root: &Path, version: &str) {
        let asset_path = root
            .join("Content")
            .join("Character")
            .join("Encore")
            .join("Body.mesh");
        fs::create_dir_all(asset_path.parent().expect("asset parent"))
            .expect("create asset parent");
        fs::write(asset_path, b"asset").expect("write asset");
        fs::write(
            root.join("launcherDownloadConfig.json"),
            format!(
                r#"{{"version":"{version}","reUseVersion":"","state":"ready","isPreDownload":false,"appId":"50004"}}"#
            ),
        )
        .expect("write launcher config");
    }

    fn sample_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
        GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.to_string(),
            created_at_unix_ms: 1,
            source_root: "root".to_string(),
            asset_count: 1,
            assets: vec![SnapshotAsset {
                id: "asset-1".to_string(),
                path: "Content/Character/Encore/Body.mesh".to_string(),
                kind: Some("mesh".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some("body".to_string()),
                    vertex_count: Some(vertex_count),
                    index_count: Some(1),
                    material_slots: Some(1),
                    section_count: Some(1),
                    tags: Vec::new(),
                },
                fingerprint: SnapshotFingerprint {
                    normalized_kind: Some("mesh".to_string()),
                    normalized_name: Some("body".to_string()),
                    name_tokens: vec!["body".to_string()],
                    path_tokens: vec!["content".to_string()],
                    tags: Vec::new(),
                    vertex_count: Some(vertex_count),
                    index_count: Some(1),
                    material_slots: Some(1),
                    section_count: Some(1),
                },
                hash_fields: SnapshotHashFields::default(),
            }],
            context: SnapshotContext::default(),
        }
    }

    fn item(path: &str) -> VersionedItem {
        VersionedItem {
            key: path.to_string(),
            label: path.to_string(),
            path: Some(path.to_string()),
            metadata: TechnicalMetadata::default(),
        }
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();

        std::env::temp_dir().join(format!("whashreonator-gui-app-test-{nanos}"))
    }
}
