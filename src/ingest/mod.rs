use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    domain::{AssetBundle, AssetMetadata, AssetRecord},
    error::{AppError, AppResult},
};

pub trait IngestSource {
    fn load_bundle(&self, path: &Path) -> AppResult<AssetBundle>;
}

pub trait AssetListSource {
    fn load_assets(&self, path: &Path) -> AppResult<Vec<AssetRecord>>;
}

/// Extraction strategy used when building snapshots from a local source root.
///
/// This is intentionally narrow: today we only support install/package-level filesystem
/// inventory, but the trait provides a concrete seam for future asset-level extractors.
pub trait SnapshotAssetExtractor {
    fn extraction_mode(&self) -> SnapshotExtractionMode;
    fn extract_snapshot_assets(&self, path: &Path) -> AppResult<Vec<AssetRecord>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotExtractionMode {
    /// Current default implementation: recursive local filesystem inventory.
    InstallFilesystemInventory,
    /// Hook for tests or future adapter-based ingestion of already prepared assets.
    PreparedAssetList,
    /// Future extension point; proprietary format parsing is intentionally not implemented yet.
    AssetLevelExtractorPlaceholder,
}

impl SnapshotExtractionMode {
    pub fn capture_mode(self) -> &'static str {
        match self {
            SnapshotExtractionMode::InstallFilesystemInventory => "local_filesystem_inventory",
            SnapshotExtractionMode::PreparedAssetList => "prepared_asset_list_inventory",
            SnapshotExtractionMode::AssetLevelExtractorPlaceholder => {
                "asset_level_extractor_placeholder"
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct JsonFileIngestSource;

/// Default snapshot extraction strategy used by current CLI/GUI flows.
///
/// This scanner collects install/package-level inventory from local paths. It does not parse
/// proprietary containers (for example `.pak`) into semantic asset-level records.
#[derive(Debug, Default, Clone)]
pub struct LocalSnapshotIngestSource;

#[derive(Debug, Clone)]
pub struct PreparedSnapshotAssetExtractor {
    assets: Vec<AssetRecord>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AssetLevelSnapshotExtractorPlaceholder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleAssetSide {
    Old,
    New,
}

#[derive(Debug, Clone)]
pub enum AssetSourceSpec {
    JsonBundle {
        path: PathBuf,
        side: BundleAssetSide,
    },
    LocalSnapshot {
        root: PathBuf,
    },
}

impl IngestSource for JsonFileIngestSource {
    fn load_bundle(&self, path: &Path) -> AppResult<AssetBundle> {
        let bundle: AssetBundle = serde_json::from_str(&fs::read_to_string(path)?)?;
        validate_bundle(&bundle)?;
        Ok(bundle)
    }
}

impl SnapshotAssetExtractor for LocalSnapshotIngestSource {
    fn extraction_mode(&self) -> SnapshotExtractionMode {
        SnapshotExtractionMode::InstallFilesystemInventory
    }

    fn extract_snapshot_assets(&self, path: &Path) -> AppResult<Vec<AssetRecord>> {
        scan_local_assets(path)
    }
}

impl PreparedSnapshotAssetExtractor {
    pub fn new(assets: Vec<AssetRecord>) -> AppResult<Self> {
        validate_assets("prepared_assets", &assets)?;
        Ok(Self { assets })
    }
}

impl SnapshotAssetExtractor for PreparedSnapshotAssetExtractor {
    fn extraction_mode(&self) -> SnapshotExtractionMode {
        SnapshotExtractionMode::PreparedAssetList
    }

    fn extract_snapshot_assets(&self, _path: &Path) -> AppResult<Vec<AssetRecord>> {
        Ok(self.assets.clone())
    }
}

impl SnapshotAssetExtractor for AssetLevelSnapshotExtractorPlaceholder {
    fn extraction_mode(&self) -> SnapshotExtractionMode {
        SnapshotExtractionMode::AssetLevelExtractorPlaceholder
    }

    fn extract_snapshot_assets(&self, path: &Path) -> AppResult<Vec<AssetRecord>> {
        Err(AppError::InvalidInput(format!(
            "asset-level extraction is not implemented yet for {}; keep using install-level inventory extraction until a real parser is integrated",
            path.display()
        )))
    }
}

impl<T> AssetListSource for T
where
    T: SnapshotAssetExtractor,
{
    fn load_assets(&self, path: &Path) -> AppResult<Vec<AssetRecord>> {
        self.extract_snapshot_assets(path)
    }
}

pub fn load_bundle_from_sources(
    old_source: &AssetSourceSpec,
    new_source: &AssetSourceSpec,
) -> AppResult<AssetBundle> {
    let bundle = AssetBundle {
        old_assets: load_assets_from_source(old_source)?,
        new_assets: load_assets_from_source(new_source)?,
    };
    validate_bundle(&bundle)?;
    Ok(bundle)
}

fn load_assets_from_source(source: &AssetSourceSpec) -> AppResult<Vec<AssetRecord>> {
    match source {
        AssetSourceSpec::JsonBundle { path, side } => {
            let bundle = JsonFileIngestSource.load_bundle(path)?;
            Ok(match side {
                BundleAssetSide::Old => bundle.old_assets,
                BundleAssetSide::New => bundle.new_assets,
            })
        }
        AssetSourceSpec::LocalSnapshot { root } => LocalSnapshotIngestSource.load_assets(root),
    }
}

fn validate_bundle(bundle: &AssetBundle) -> AppResult<()> {
    validate_assets("old_assets", &bundle.old_assets)?;
    validate_assets("new_assets", &bundle.new_assets)?;
    Ok(())
}

fn validate_assets(label: &str, assets: &[AssetRecord]) -> AppResult<()> {
    for (index, asset) in assets.iter().enumerate() {
        if asset.id.trim().is_empty() {
            return Err(AppError::InvalidInput(format!(
                "{label}[{index}].id must not be empty"
            )));
        }

        if asset.path.trim().is_empty() {
            return Err(AppError::InvalidInput(format!(
                "{label}[{index}].path must not be empty"
            )));
        }
    }

    Ok(())
}

fn scan_local_assets(root: &Path) -> AppResult<Vec<AssetRecord>> {
    if !root.exists() {
        return Err(AppError::InvalidInput(format!(
            "local source root does not exist: {}",
            root.display()
        )));
    }

    if !root.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "local source root is not a directory: {}",
            root.display()
        )));
    }

    let mut assets = Vec::new();
    scan_dir(root, root, &mut assets)?;
    assets.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.id.cmp(&right.id))
    });
    validate_assets("local_assets", &assets)?;
    Ok(assets)
}

fn scan_dir(root: &Path, current: &Path, assets: &mut Vec<AssetRecord>) -> AppResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(path.as_path());

        if path.is_dir() {
            if should_skip_dir(relative) {
                continue;
            }
            scan_dir(root, &path, assets)?;
            continue;
        }

        if path.is_file() {
            if should_skip_file(relative) {
                continue;
            }
            assets.push(build_local_asset(root, &path)?);
        }
    }

    Ok(())
}

fn build_local_asset(root: &Path, path: &Path) -> AppResult<AssetRecord> {
    let relative = path.strip_prefix(root).map_err(|_| {
        AppError::InvalidInput(format!(
            "failed to normalize local asset path relative to root: {}",
            path.display()
        ))
    })?;
    let relative_path = normalize_relative_path(relative);

    Ok(AssetRecord {
        id: relative_path.clone(),
        path: relative_path,
        kind: path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase()),
        metadata: AssetMetadata {
            logical_name: path
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned()),
            ..AssetMetadata::default()
        },
    })
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn should_skip_dir(relative: &Path) -> bool {
    relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|component| {
            matches_ignore_ascii_case(component, "saved")
                || matches_ignore_ascii_case(component, "launcherdownload")
                || matches_ignore_ascii_case(component, ".quality")
        })
}

fn should_skip_file(relative: &Path) -> bool {
    relative
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
}

fn matches_ignore_ascii_case(value: &str, expected: &str) -> bool {
    value.eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        AssetLevelSnapshotExtractorPlaceholder, AssetListSource, PreparedSnapshotAssetExtractor,
        scan_local_assets,
    };

    #[test]
    fn local_scan_skips_runtime_and_cache_directories() {
        let test_root = unique_test_dir();
        seed_file(
            &test_root,
            "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak",
        );
        seed_file(&test_root, "Client/Saved/Logs/Client.log");
        seed_file(
            &test_root,
            "Client/Binaries/Win64/.quality/performance/perf.data",
        );
        seed_file(&test_root, "launcherDownload/cache.tmp");

        let assets = scan_local_assets(&test_root).expect("scan local assets");

        assert_eq!(assets.len(), 1);
        assert_eq!(
            assets[0].path,
            "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak"
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn prepared_snapshot_asset_extractor_returns_seeded_assets() {
        let extractor = PreparedSnapshotAssetExtractor::new(vec![crate::domain::AssetRecord {
            id: "asset-1".to_string(),
            path: "Content/Character/Encore/Body.mesh".to_string(),
            kind: Some("mesh".to_string()),
            metadata: crate::domain::AssetMetadata::default(),
        }])
        .expect("build prepared extractor");

        let loaded = extractor
            .load_assets(Path::new("ignored"))
            .expect("load prepared assets");

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].path, "Content/Character/Encore/Body.mesh");
    }

    #[test]
    fn asset_level_placeholder_is_explicitly_not_implemented() {
        let error = AssetLevelSnapshotExtractorPlaceholder
            .load_assets(Path::new("D:/fake-game"))
            .expect_err("placeholder should not parse proprietary containers yet");

        assert!(
            error
                .to_string()
                .contains("asset-level extraction is not implemented yet")
        );
    }

    fn seed_file(root: &Path, relative_path: &str) {
        let full_path = root.join(relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }

        fs::write(full_path, b"test").expect("write file");
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();

        std::env::temp_dir().join(format!("whashreonator-ingest-test-{nanos}"))
    }
}
