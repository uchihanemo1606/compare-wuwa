use std::path::{Path, PathBuf};

use crate::{
    error::AppResult,
    report_storage::ReportStorage,
    snapshot::{
        GameSnapshot, create_local_snapshot, detect_game_version, resolve_snapshot_version_override,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedVersionScan {
    pub source_root: PathBuf,
    pub version_id: String,
    pub existing_snapshot_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareVersionScanResult {
    Ready(PreparedVersionScan),
    VersionAlreadyExists(PreparedVersionScan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteVersionScanResult {
    Created {
        version_id: String,
        snapshot_path: PathBuf,
        snapshot: GameSnapshot,
    },
    NoChangesDetected {
        version_id: String,
        snapshot_path: PathBuf,
    },
    Overwritten {
        version_id: String,
        snapshot_path: PathBuf,
        snapshot: GameSnapshot,
    },
}

pub trait SnapshotFactory {
    fn create_snapshot(&self, version_id: &str, source_root: &Path) -> AppResult<GameSnapshot>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalSnapshotFactory;

impl SnapshotFactory for LocalSnapshotFactory {
    fn create_snapshot(&self, version_id: &str, source_root: &Path) -> AppResult<GameSnapshot> {
        create_local_snapshot(version_id, source_root)
    }
}

#[derive(Debug, Clone)]
pub struct VersionScanService<F = LocalSnapshotFactory> {
    storage: ReportStorage,
    snapshot_factory: F,
}

impl Default for VersionScanService<LocalSnapshotFactory> {
    fn default() -> Self {
        Self {
            storage: ReportStorage::default(),
            snapshot_factory: LocalSnapshotFactory,
        }
    }
}

impl<F> VersionScanService<F> {
    pub fn new(storage: ReportStorage, snapshot_factory: F) -> Self {
        Self {
            storage,
            snapshot_factory,
        }
    }
}

impl<F> VersionScanService<F>
where
    F: SnapshotFactory,
{
    pub fn detect_version(
        &self,
        source_root: &Path,
        version_override: Option<&str>,
    ) -> AppResult<String> {
        match version_override.map(str::trim) {
            Some("") | Some("auto") | Some("AUTO") | None => detect_game_version(source_root),
            Some(value) => resolve_snapshot_version_override(Some(value), source_root),
        }
    }

    pub fn prepare_scan(
        &self,
        source_root: &Path,
        version_override: Option<&str>,
    ) -> AppResult<PrepareVersionScanResult> {
        let version_id = self.detect_version(source_root, version_override)?;
        let existing_snapshot_path = self.storage.find_stored_snapshot_by_version(&version_id)?;
        let prepared = PreparedVersionScan {
            source_root: source_root.to_path_buf(),
            version_id,
            existing_snapshot_path,
        };

        if prepared.existing_snapshot_path.is_some() {
            Ok(PrepareVersionScanResult::VersionAlreadyExists(prepared))
        } else {
            Ok(PrepareVersionScanResult::Ready(prepared))
        }
    }

    pub fn execute_scan(
        &self,
        prepared: &PreparedVersionScan,
        force_rescan: bool,
    ) -> AppResult<ExecuteVersionScanResult> {
        if prepared.existing_snapshot_path.is_some() && !force_rescan {
            return Ok(ExecuteVersionScanResult::NoChangesDetected {
                version_id: prepared.version_id.clone(),
                snapshot_path: self.storage.snapshot_path_for_version(&prepared.version_id),
            });
        }

        let snapshot = self
            .snapshot_factory
            .create_snapshot(&prepared.version_id, &prepared.source_root)?;
        let snapshot_path = self.storage.snapshot_path_for_version(&prepared.version_id);

        if let Some(existing) = self
            .storage
            .load_stored_snapshot_by_version(&prepared.version_id)?
        {
            if snapshots_equivalent(&existing, &snapshot) {
                return Ok(ExecuteVersionScanResult::NoChangesDetected {
                    version_id: prepared.version_id.clone(),
                    snapshot_path,
                });
            }

            let saved_path = self.storage.save_snapshot_for_version(&snapshot)?;
            return Ok(ExecuteVersionScanResult::Overwritten {
                version_id: prepared.version_id.clone(),
                snapshot_path: saved_path,
                snapshot,
            });
        }

        let saved_path = self.storage.save_snapshot_for_version(&snapshot)?;
        Ok(ExecuteVersionScanResult::Created {
            version_id: prepared.version_id.clone(),
            snapshot_path: saved_path,
            snapshot,
        })
    }
}

pub fn snapshots_equivalent(left: &GameSnapshot, right: &GameSnapshot) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.created_at_unix_ms = 0;
    right.created_at_unix_ms = 0;
    left.source_root.clear();
    right.source_root.clear();
    left == right
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        fs,
        path::{Path, PathBuf},
        rc::Rc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
    };

    use super::{
        ExecuteVersionScanResult, PrepareVersionScanResult, SnapshotFactory, VersionScanService,
    };

    #[test]
    fn auto_detect_version_succeeds_from_launcher_config() {
        let test_root = unique_test_dir();
        let game_root = test_root.join("game");
        fs::create_dir_all(&game_root).expect("create game root");
        fs::write(
            game_root.join("launcherDownloadConfig.json"),
            r#"{"version":"9.9.9","reUseVersion":"","state":"ready","isPreDownload":false,"appId":"50004"}"#,
        )
        .expect("write launcher config");

        let service = VersionScanService::default();
        let version = service
            .detect_version(&game_root, None)
            .expect("detect version");

        assert_eq!(version, "9.9.9");

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn existing_version_blocks_default_flow() {
        let test_root = unique_test_dir();
        let storage = crate::report_storage::ReportStorage::new(test_root.join("reports"));
        storage
            .save_snapshot_for_version(&sample_snapshot("3.0.0", "old-root", 1))
            .expect("seed snapshot");

        let service = VersionScanService::new(storage.clone(), FixedSnapshotFactory::default());
        let prepared = service
            .prepare_scan(Path::new("D:/fake-game"), Some("3.0.0"))
            .expect("prepare scan");

        match prepared {
            PrepareVersionScanResult::VersionAlreadyExists(prepared) => {
                assert_eq!(prepared.version_id, "3.0.0");
                assert!(prepared.existing_snapshot_path.is_some());
            }
            other => panic!("expected duplicate block, got {other:?}"),
        }

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn rescan_runs_again_after_confirmation() {
        let test_root = unique_test_dir();
        let storage = crate::report_storage::ReportStorage::new(test_root.join("reports"));
        storage
            .save_snapshot_for_version(&sample_snapshot("4.0.0", "seed-root", 1))
            .expect("seed snapshot");

        let calls = Rc::new(RefCell::new(0usize));
        let factory = FixedSnapshotFactory::with_calls(
            vec![sample_snapshot("4.0.0", "rescanned-root", 2)],
            calls.clone(),
        );
        let service = VersionScanService::new(storage.clone(), factory);
        let prepared = match service
            .prepare_scan(Path::new("D:/fake-game"), Some("4.0.0"))
            .expect("prepare")
        {
            PrepareVersionScanResult::VersionAlreadyExists(prepared) => prepared,
            other => panic!("expected duplicate result, got {other:?}"),
        };

        let result = service
            .execute_scan(&prepared, true)
            .expect("execute rescan");

        assert!(matches!(
            result,
            ExecuteVersionScanResult::Overwritten { .. }
        ));
        assert_eq!(*calls.borrow(), 1);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn unchanged_rescan_does_not_create_extra_record() {
        let test_root = unique_test_dir();
        let storage = crate::report_storage::ReportStorage::new(test_root.join("reports"));
        storage
            .save_snapshot_for_version(&sample_snapshot("5.0.0", "seed-root", 1))
            .expect("seed snapshot");

        let service = VersionScanService::new(
            storage.clone(),
            FixedSnapshotFactory::from_snapshots(vec![sample_snapshot("5.0.0", "other-root", 1)]),
        );
        let prepared = match service
            .prepare_scan(Path::new("D:/fake-game"), Some("5.0.0"))
            .expect("prepare")
        {
            PrepareVersionScanResult::VersionAlreadyExists(prepared) => prepared,
            other => panic!("expected duplicate result, got {other:?}"),
        };

        let result = service
            .execute_scan(&prepared, true)
            .expect("execute rescan");

        assert!(matches!(
            result,
            ExecuteVersionScanResult::NoChangesDetected { .. }
        ));
        let snapshot_dir = storage.build_version_layout("5.0.0").snapshot_dir;
        assert_eq!(count_snapshot_files(snapshot_dir), 1);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn changed_rescan_overwrites_existing_snapshot() {
        let test_root = unique_test_dir();
        let storage = crate::report_storage::ReportStorage::new(test_root.join("reports"));
        storage
            .save_snapshot_for_version(&sample_snapshot("6.0.0", "seed-root", 1))
            .expect("seed snapshot");

        let service = VersionScanService::new(
            storage.clone(),
            FixedSnapshotFactory::from_snapshots(vec![sample_snapshot("6.0.0", "new-root", 3)]),
        );
        let prepared = match service
            .prepare_scan(Path::new("D:/fake-game"), Some("6.0.0"))
            .expect("prepare")
        {
            PrepareVersionScanResult::VersionAlreadyExists(prepared) => prepared,
            other => panic!("expected duplicate result, got {other:?}"),
        };

        let result = service
            .execute_scan(&prepared, true)
            .expect("execute rescan");

        match result {
            ExecuteVersionScanResult::Overwritten { snapshot, .. } => {
                assert_eq!(snapshot.asset_count, 1);
                assert_eq!(snapshot.assets[0].fingerprint.vertex_count, Some(3));
            }
            other => panic!("expected overwrite, got {other:?}"),
        }

        let saved = storage
            .load_snapshot_by_version("6.0.0")
            .expect("load saved snapshot")
            .expect("snapshot exists");
        assert_eq!(saved.assets[0].fingerprint.vertex_count, Some(3));
        let snapshot_dir = storage.build_version_layout("6.0.0").snapshot_dir;
        assert_eq!(count_snapshot_files(snapshot_dir), 1);

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn alternate_snapshot_only_does_not_block_prepare_scan_as_officially_stored() {
        let test_root = unique_test_dir();
        let storage = crate::report_storage::ReportStorage::new(test_root.join("reports"));
        let version_id = "6.5.0";
        let alternate_path = storage
            .build_version_directory(version_id)
            .join("report_bundle")
            .join("fixture")
            .join("old.snapshot.json");
        if let Some(parent) = alternate_path.parent() {
            fs::create_dir_all(parent).expect("create alternate snapshot parent");
        }
        fs::write(
            &alternate_path,
            serde_json::to_string_pretty(&sample_snapshot(version_id, "alt-root", 2))
                .expect("serialize alternate snapshot"),
        )
        .expect("write alternate snapshot");

        let service = VersionScanService::new(storage, FixedSnapshotFactory::default());
        let prepared = service
            .prepare_scan(Path::new("D:/fake-game"), Some(version_id))
            .expect("prepare scan");

        match prepared {
            PrepareVersionScanResult::Ready(prepared) => {
                assert_eq!(prepared.version_id, version_id);
                assert!(prepared.existing_snapshot_path.is_none());
            }
            other => panic!("expected ready result, got {other:?}"),
        }

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn execute_scan_without_force_does_not_return_missing_canonical_path() {
        let test_root = unique_test_dir();
        let storage = crate::report_storage::ReportStorage::new(test_root.join("reports"));
        let version_id = "6.6.0";
        let alternate_snapshot = sample_snapshot(version_id, "alt-root", 5);
        let alternate_path = storage
            .build_version_directory(version_id)
            .join("report_bundle")
            .join("fixture")
            .join("old.snapshot.json");
        if let Some(parent) = alternate_path.parent() {
            fs::create_dir_all(parent).expect("create alternate snapshot parent");
        }
        fs::write(
            &alternate_path,
            serde_json::to_string_pretty(&alternate_snapshot)
                .expect("serialize alternate snapshot"),
        )
        .expect("write alternate snapshot");

        let service = VersionScanService::new(
            storage.clone(),
            FixedSnapshotFactory::from_snapshots(vec![sample_snapshot(version_id, "new-root", 5)]),
        );
        let prepared = match service
            .prepare_scan(Path::new("D:/fake-game"), Some(version_id))
            .expect("prepare scan")
        {
            PrepareVersionScanResult::Ready(prepared) => prepared,
            other => panic!("expected ready result, got {other:?}"),
        };

        let result = service
            .execute_scan(&prepared, false)
            .expect("execute scan");

        match result {
            ExecuteVersionScanResult::Created { snapshot_path, .. } => {
                assert_eq!(snapshot_path, storage.snapshot_path_for_version(version_id));
                assert!(snapshot_path.exists());
            }
            other => panic!("expected created result, got {other:?}"),
        }

        let _ = fs::remove_dir_all(test_root);
    }

    #[derive(Default, Clone)]
    struct FixedSnapshotFactory {
        snapshots: Rc<RefCell<VecDeque<GameSnapshot>>>,
        calls: Rc<RefCell<usize>>,
    }

    impl FixedSnapshotFactory {
        fn from_snapshots(snapshots: Vec<GameSnapshot>) -> Self {
            Self {
                snapshots: Rc::new(RefCell::new(snapshots.into())),
                calls: Rc::new(RefCell::new(0)),
            }
        }

        fn with_calls(snapshots: Vec<GameSnapshot>, calls: Rc<RefCell<usize>>) -> Self {
            Self {
                snapshots: Rc::new(RefCell::new(snapshots.into())),
                calls,
            }
        }
    }

    impl SnapshotFactory for FixedSnapshotFactory {
        fn create_snapshot(
            &self,
            _version_id: &str,
            _source_root: &Path,
        ) -> crate::error::AppResult<GameSnapshot> {
            *self.calls.borrow_mut() += 1;
            Ok(self
                .snapshots
                .borrow_mut()
                .pop_front()
                .expect("snapshot available"))
        }
    }

    fn sample_snapshot(version_id: &str, source_root: &str, vertex_count: u32) -> GameSnapshot {
        let assets = vec![SnapshotAsset {
            id: "asset-1".to_string(),
            path: "Content/Character/Encore/Body.mesh".to_string(),
            identity_tuple: None,
            kind: Some("mesh".to_string()),
            metadata: crate::domain::AssetMetadata {
                logical_name: Some("body".to_string()),
                vertex_count: Some(vertex_count),
                index_count: Some(2),
                material_slots: Some(1),
                section_count: Some(1),
                tags: Vec::new(),
                ..Default::default()
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: Some("body".to_string()),
                name_tokens: vec!["body".to_string()],
                path_tokens: vec!["Content".to_string()],
                tags: Vec::new(),
                vertex_count: Some(vertex_count),
                index_count: Some(2),
                material_slots: Some(1),
                section_count: Some(1),
                ..Default::default()
            },
            hash_fields: SnapshotHashFields::default(),
            source: crate::domain::AssetSourceContext::default(),
        }];

        GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.to_string(),
            created_at_unix_ms: 1,
            source_root: source_root.to_string(),
            asset_count: assets.len(),
            assets,
            context: SnapshotContext::default(),
        }
    }

    fn count_snapshot_files(root: PathBuf) -> usize {
        fs::read_dir(root)
            .expect("read snapshot root")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "json")
            })
            .count()
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();

        std::env::temp_dir().join(format!("whashreonator-version-scan-test-{nanos}"))
    }
}
