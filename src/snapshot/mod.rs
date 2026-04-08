use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{AssetMetadata, AssetRecord},
    error::{AppError, AppResult},
    fingerprint::{AssetFingerprint, DefaultFingerprinter, Fingerprinter},
    ingest::{LocalSnapshotIngestSource, SnapshotAssetExtractor},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameSnapshot {
    pub schema_version: String,
    pub version_id: String,
    pub created_at_unix_ms: u128,
    pub source_root: String,
    pub asset_count: usize,
    pub assets: Vec<SnapshotAsset>,
    #[serde(default)]
    pub context: SnapshotContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotAsset {
    pub id: String,
    pub path: String,
    pub kind: Option<String>,
    pub metadata: AssetMetadata,
    pub fingerprint: SnapshotFingerprint,
    pub hash_fields: SnapshotHashFields,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotFingerprint {
    pub normalized_kind: Option<String>,
    pub normalized_name: Option<String>,
    pub name_tokens: Vec<String>,
    pub path_tokens: Vec<String>,
    pub tags: Vec<String>,
    pub vertex_count: Option<u32>,
    pub index_count: Option<u32>,
    pub material_slots: Option<u32>,
    pub section_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotHashFields {
    pub asset_hash: Option<String>,
    pub shader_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotContext {
    pub launcher: Option<SnapshotLauncherContext>,
    pub resource_manifest: Option<SnapshotResourceManifestContext>,
    #[serde(default)]
    pub scope: SnapshotScopeContext,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotScopeContext {
    pub capture_mode: Option<String>,
    pub mostly_install_or_package_level: Option<bool>,
    pub meaningful_content_coverage: Option<bool>,
    pub meaningful_character_coverage: Option<bool>,
    pub coverage: SnapshotCoverageSignals,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotCoverageSignals {
    pub content_like_path_count: usize,
    pub character_path_count: usize,
    pub non_content_path_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotScopeAssessment {
    pub capture_mode: Option<String>,
    pub mostly_install_or_package_level: bool,
    pub meaningful_content_coverage: bool,
    pub meaningful_character_coverage: bool,
    pub coverage: SnapshotCoverageSignals,
    pub note: Option<String>,
    pub observed_fallback_used: bool,
}

impl SnapshotScopeAssessment {
    pub fn is_low_signal_for_character_analysis(&self) -> bool {
        self.mostly_install_or_package_level
            && !(self.meaningful_content_coverage && self.meaningful_character_coverage)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotLauncherContext {
    pub source_file: String,
    pub detected_version: String,
    pub reuse_version: Option<String>,
    pub state: Option<String>,
    pub is_pre_download: bool,
    pub app_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotResourceManifestContext {
    pub source_file: String,
    pub resource_count: usize,
    pub matched_assets: usize,
    pub unmatched_snapshot_assets: usize,
}

#[derive(Debug, Clone)]
pub struct SnapshotBuilder<S, F> {
    asset_source: S,
    fingerprinter: F,
}

impl<S, F> SnapshotBuilder<S, F>
where
    S: SnapshotAssetExtractor,
    F: Fingerprinter,
{
    pub fn new(asset_source: S, fingerprinter: F) -> Self {
        Self {
            asset_source,
            fingerprinter,
        }
    }

    pub fn build(&self, version_id: &str, source_root: &Path) -> AppResult<GameSnapshot> {
        if version_id.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "snapshot version_id must not be empty".to_string(),
            ));
        }

        let extraction_mode = self.asset_source.extraction_mode();
        let assets = self.asset_source.extract_snapshot_assets(source_root)?;
        let snapshot_assets = assets
            .iter()
            .map(|asset| SnapshotAsset::from_asset(asset, self.fingerprinter.fingerprint(asset)))
            .collect::<Vec<_>>();

        Ok(GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.trim().to_string(),
            created_at_unix_ms: current_unix_ms()?,
            source_root: normalize_source_root(source_root),
            asset_count: snapshot_assets.len(),
            assets: snapshot_assets,
            context: SnapshotContext {
                launcher: None,
                resource_manifest: None,
                scope: SnapshotScopeContext {
                    capture_mode: Some(extraction_mode.capture_mode().to_string()),
                    ..SnapshotScopeContext::default()
                },
                notes: Vec::new(),
            },
        })
    }
}

pub fn create_snapshot_with_extractor<E>(
    version_id: &str,
    source_root: &Path,
    extractor: E,
) -> AppResult<GameSnapshot>
where
    E: SnapshotAssetExtractor,
{
    // Extension seam: future asset-level extractors can plug in here without changing
    // downstream snapshot/compare/inference/proposal/report storage flows.
    let resolved_version_id = resolve_snapshot_version_id(version_id, source_root)?;
    SnapshotBuilder::new(extractor, DefaultFingerprinter).build(&resolved_version_id, source_root)
}

pub fn create_local_snapshot(version_id: &str, source_root: &Path) -> AppResult<GameSnapshot> {
    // Current default path remains install/package-level filesystem inventory.
    let mut snapshot =
        create_snapshot_with_extractor(version_id, source_root, LocalSnapshotIngestSource)?;
    enrich_snapshot_from_game_root(&mut snapshot, source_root)?;
    annotate_local_snapshot_scope(&mut snapshot);
    Ok(snapshot)
}

pub fn detect_game_version(source_root: &Path) -> AppResult<String> {
    load_launcher_context(source_root)?
        .map(|launcher| launcher.detected_version)
        .ok_or_else(|| {
            AppError::InvalidInput(
                "could not auto-detect version from launcherDownloadConfig.json; verify the game source root points to the current game folder, or set a version override in Advanced"
                    .to_string(),
            )
        })
}

pub fn resolve_snapshot_version_id(version_id: &str, source_root: &Path) -> AppResult<String> {
    resolve_snapshot_version_override(
        (!version_id.trim().is_empty()).then_some(version_id.trim()),
        source_root,
    )
}

pub fn resolve_snapshot_version_override(
    version_override: Option<&str>,
    source_root: &Path,
) -> AppResult<String> {
    let Some(version_override) = version_override.map(str::trim) else {
        return detect_game_version(source_root);
    };

    if version_override.is_empty() || version_override.eq_ignore_ascii_case("auto") {
        return detect_game_version(source_root);
    }

    Ok(version_override.to_string())
}

pub fn load_snapshot(path: &Path) -> AppResult<GameSnapshot> {
    let snapshot: GameSnapshot = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(snapshot)
}

impl SnapshotAsset {
    fn from_asset(asset: &AssetRecord, fingerprint: AssetFingerprint) -> Self {
        Self {
            id: asset.id.clone(),
            path: asset.path.clone(),
            kind: asset.kind.clone(),
            metadata: asset.metadata.clone(),
            fingerprint: SnapshotFingerprint {
                normalized_kind: fingerprint.normalized_kind,
                normalized_name: fingerprint.normalized_name,
                name_tokens: fingerprint.name_tokens.into_iter().collect(),
                path_tokens: fingerprint.path_tokens.into_iter().collect(),
                tags: fingerprint.tags.into_iter().collect(),
                vertex_count: fingerprint.vertex_count,
                index_count: fingerprint.index_count,
                material_slots: fingerprint.material_slots,
                section_count: fingerprint.section_count,
            },
            hash_fields: SnapshotHashFields::default(),
        }
    }
}

fn enrich_snapshot_from_game_root(
    snapshot: &mut GameSnapshot,
    source_root: &Path,
) -> AppResult<()> {
    let mut notes = Vec::new();

    match load_launcher_context(source_root)? {
        Some(launcher) => {
            if snapshot.version_id != launcher.detected_version {
                notes.push(format!(
                    "snapshot version_id {} differs from launcher-detected version {}",
                    snapshot.version_id, launcher.detected_version
                ));
            }
            snapshot.context.launcher = Some(launcher);
        }
        None => notes.push(
            "launcherDownloadConfig.json not found; detected_version context unavailable"
                .to_string(),
        ),
    }

    match load_resource_manifest(source_root)? {
        Some((manifest_context, manifest_entries)) => {
            let mut matched_assets = 0usize;
            for asset in &mut snapshot.assets {
                if let Some(entry) = manifest_entries.get(&asset.path) {
                    asset.hash_fields.asset_hash = Some(entry.md5.clone());
                    matched_assets += 1;
                }
            }

            snapshot.context.resource_manifest = Some(SnapshotResourceManifestContext {
                matched_assets,
                unmatched_snapshot_assets: snapshot.assets.len().saturating_sub(matched_assets),
                ..manifest_context
            });
        }
        None => notes.push("LocalGameResources.json not found; asset hashes were not enriched from launcher manifest".to_string()),
    }

    snapshot.context.notes = notes;
    Ok(())
}

const MIN_MEANINGFUL_CONTENT_PATH_COUNT: usize = 10;
const MIN_MEANINGFUL_CHARACTER_PATH_COUNT: usize = 5;

pub fn assess_snapshot_scope(snapshot: &GameSnapshot) -> SnapshotScopeAssessment {
    let scope = &snapshot.context.scope;
    let has_explicit_scope_flags = scope.mostly_install_or_package_level.is_some()
        || scope.meaningful_content_coverage.is_some()
        || scope.meaningful_character_coverage.is_some();
    let mut coverage = scope.coverage.clone();
    let mut observed_fallback_used = false;

    if !has_explicit_scope_flags
        && coverage.content_like_path_count == 0
        && coverage.character_path_count == 0
        && coverage.non_content_path_count == 0
        && snapshot.asset_count > 0
    {
        coverage = compute_scope_coverage(snapshot);
        observed_fallback_used = true;
    }

    let meaningful_content_coverage = scope
        .meaningful_content_coverage
        .unwrap_or(coverage.content_like_path_count >= MIN_MEANINGFUL_CONTENT_PATH_COUNT);
    let meaningful_character_coverage = scope
        .meaningful_character_coverage
        .unwrap_or(coverage.character_path_count >= MIN_MEANINGFUL_CHARACTER_PATH_COUNT);
    let mostly_install_or_package_level = scope
        .mostly_install_or_package_level
        .unwrap_or(!(meaningful_content_coverage && meaningful_character_coverage));

    SnapshotScopeAssessment {
        capture_mode: scope.capture_mode.clone(),
        mostly_install_or_package_level,
        meaningful_content_coverage,
        meaningful_character_coverage,
        coverage,
        note: scope.note.clone(),
        observed_fallback_used,
    }
}

fn annotate_local_snapshot_scope(snapshot: &mut GameSnapshot) {
    let coverage = compute_scope_coverage(snapshot);
    let meaningful_content_coverage =
        coverage.content_like_path_count >= MIN_MEANINGFUL_CONTENT_PATH_COUNT;
    let meaningful_character_coverage =
        coverage.character_path_count >= MIN_MEANINGFUL_CHARACTER_PATH_COUNT;
    let mostly_install_or_package_level =
        !(meaningful_content_coverage && meaningful_character_coverage);

    let note = if mostly_install_or_package_level {
        format!(
            "local snapshot looks mostly install/package-level (content-like paths: {}, character-like paths: {}, non-content paths: {})",
            coverage.content_like_path_count,
            coverage.character_path_count,
            coverage.non_content_path_count
        )
    } else {
        format!(
            "local snapshot has stronger content/character path signals (content-like paths: {}, character-like paths: {}, non-content paths: {}), but remains path-level inventory",
            coverage.content_like_path_count,
            coverage.character_path_count,
            coverage.non_content_path_count
        )
    };

    snapshot.context.scope = SnapshotScopeContext {
        capture_mode: Some("local_filesystem_inventory".to_string()),
        mostly_install_or_package_level: Some(mostly_install_or_package_level),
        meaningful_content_coverage: Some(meaningful_content_coverage),
        meaningful_character_coverage: Some(meaningful_character_coverage),
        coverage,
        note: Some(note.clone()),
    };
    snapshot.context.notes.push(note);
}

fn compute_scope_coverage(snapshot: &GameSnapshot) -> SnapshotCoverageSignals {
    let content_like_path_count = snapshot
        .assets
        .iter()
        .filter(|asset| is_content_like_path(&asset.path))
        .count();
    let character_path_count = snapshot
        .assets
        .iter()
        .filter(|asset| is_character_like_path(&asset.path))
        .count();

    SnapshotCoverageSignals {
        content_like_path_count,
        character_path_count,
        non_content_path_count: snapshot.asset_count.saturating_sub(content_like_path_count),
    }
}

fn is_content_like_path(path: &str) -> bool {
    path.replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .any(|segment| segment.eq_ignore_ascii_case("content"))
}

fn is_character_like_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    segments.windows(3).any(|window| {
        window[0].eq_ignore_ascii_case("content")
            && window[1].eq_ignore_ascii_case("character")
            && !window[2].is_empty()
    })
}

fn load_launcher_context(source_root: &Path) -> AppResult<Option<SnapshotLauncherContext>> {
    let path = source_root.join("launcherDownloadConfig.json");
    if !path.exists() {
        return Ok(None);
    }

    let config: LauncherDownloadConfig = serde_json::from_str(&fs::read_to_string(&path)?)?;
    Ok(Some(SnapshotLauncherContext {
        source_file: normalize_relative_path(
            path.strip_prefix(source_root).unwrap_or(path.as_path()),
        ),
        detected_version: config.version,
        reuse_version: empty_to_none(config.re_use_version),
        state: empty_to_none(config.state),
        is_pre_download: config.is_pre_download,
        app_id: empty_to_none(config.app_id),
    }))
}

fn load_resource_manifest(
    source_root: &Path,
) -> AppResult<
    Option<(
        SnapshotResourceManifestContext,
        BTreeMap<String, ResourceManifestEntry>,
    )>,
> {
    let path = source_root.join("LocalGameResources.json");
    if !path.exists() {
        return Ok(None);
    }

    let manifest: LocalGameResourcesManifest = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let entries = manifest
        .resource
        .into_iter()
        .map(|entry| {
            let normalized_path = entry.dest.replace('\\', "/");
            (normalized_path, ResourceManifestEntry { md5: entry.md5 })
        })
        .collect::<BTreeMap<_, _>>();
    let context = SnapshotResourceManifestContext {
        source_file: normalize_relative_path(
            path.strip_prefix(source_root).unwrap_or(path.as_path()),
        ),
        resource_count: entries.len(),
        matched_assets: 0,
        unmatched_snapshot_assets: 0,
    };

    Ok(Some((context, entries)))
}

fn empty_to_none(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[derive(Debug, Clone, Deserialize)]
struct LauncherDownloadConfig {
    version: String,
    #[serde(rename = "reUseVersion", default)]
    re_use_version: String,
    #[serde(default)]
    state: String,
    #[serde(rename = "isPreDownload", default)]
    is_pre_download: bool,
    #[serde(rename = "appId", default)]
    app_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LocalGameResourcesManifest {
    #[serde(default)]
    resource: Vec<LocalGameResourceEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocalGameResourceEntry {
    dest: String,
    md5: String,
}

#[derive(Debug, Clone)]
struct ResourceManifestEntry {
    md5: String,
}

fn current_unix_ms() -> AppResult<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::InvalidInput(format!("system clock error: {error}")))?
        .as_millis())
}

fn normalize_source_root(source_root: &Path) -> String {
    source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf())
        .display()
        .to_string()
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::ingest::PreparedSnapshotAssetExtractor;

    use super::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
        SnapshotScopeContext, assess_snapshot_scope, create_local_snapshot,
        create_snapshot_with_extractor, load_snapshot,
    };

    #[test]
    fn creates_snapshot_from_local_root() {
        let test_root = unique_test_dir();
        let local_root = test_root.join("game");
        seed_local_asset(&local_root, "Content/Character/HeroA/Body.mesh");

        let snapshot = create_local_snapshot("2.4.0", &local_root).expect("create snapshot");

        assert_eq!(snapshot.version_id, "2.4.0");
        assert_eq!(snapshot.asset_count, 1);
        assert_eq!(snapshot.assets[0].path, "Content/Character/HeroA/Body.mesh");
        assert_eq!(
            snapshot.assets[0].fingerprint.normalized_name.as_deref(),
            Some("body")
        );
        assert!(snapshot.context.launcher.is_none());
        assert!(snapshot.context.resource_manifest.is_none());
        assert_eq!(
            snapshot.context.scope.capture_mode.as_deref(),
            Some("local_filesystem_inventory")
        );
        assert_eq!(
            snapshot.context.scope.coverage.content_like_path_count,
            snapshot.asset_count
        );
        assert_eq!(snapshot.context.scope.coverage.character_path_count, 1);
        assert_eq!(snapshot.context.scope.coverage.non_content_path_count, 0);
        assert_eq!(
            snapshot.context.scope.mostly_install_or_package_level,
            Some(true)
        );
        assert_eq!(
            snapshot.context.scope.meaningful_content_coverage,
            Some(false)
        );
        assert_eq!(
            snapshot.context.scope.meaningful_character_coverage,
            Some(false)
        );
        assert!(
            snapshot
                .context
                .notes
                .iter()
                .any(|note| note.contains("install/package-level"))
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn creates_snapshot_with_launcher_and_manifest_enrichment() {
        let test_root = unique_test_dir();
        let local_root = test_root.join("game");
        seed_local_asset(
            &local_root,
            "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak",
        );
        seed_local_asset(&local_root, "Client/Config/DefaultGame.ini");
        fs::write(
            local_root.join("launcherDownloadConfig.json"),
            r#"{"version":"3.2.1","reUseVersion":"3.2.0","state":"ready","isPreDownload":false,"appId":"50004"}"#,
        )
        .expect("write launcher config");
        fs::write(
            local_root.join("LocalGameResources.json"),
            r#"{"resource":[{"dest":"Client/Content/Paks/pakchunk0-WindowsNoEditor.pak","size":123,"md5":"abc123"},{"dest":"Client/Config/DefaultGame.ini","size":10,"md5":"def456"}]}"#,
        )
        .expect("write manifest");

        let snapshot = create_local_snapshot("auto", &local_root).expect("create snapshot");

        assert_eq!(snapshot.version_id, "3.2.1");
        assert_eq!(
            snapshot
                .context
                .launcher
                .as_ref()
                .map(|launcher| launcher.detected_version.as_str()),
            Some("3.2.1")
        );
        assert_eq!(
            snapshot
                .context
                .resource_manifest
                .as_ref()
                .map(|manifest| manifest.matched_assets),
            Some(2)
        );
        assert!(snapshot.assets.iter().any(|asset| asset.path
            == "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak"
            && asset.hash_fields.asset_hash.as_deref() == Some("abc123")));
        assert_eq!(
            snapshot.context.scope.capture_mode.as_deref(),
            Some("local_filesystem_inventory")
        );
        assert_eq!(snapshot.context.scope.coverage.content_like_path_count, 1);
        assert_eq!(snapshot.context.scope.coverage.character_path_count, 0);
        assert_eq!(snapshot.context.scope.coverage.non_content_path_count, 3);
        assert_eq!(
            snapshot.context.scope.mostly_install_or_package_level,
            Some(true)
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn create_snapshot_with_extractor_accepts_prepared_extension_point() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let extractor = PreparedSnapshotAssetExtractor::new(vec![crate::domain::AssetRecord {
            id: "asset-1".to_string(),
            path: "Content/Character/Encore/Body.mesh".to_string(),
            kind: Some("mesh".to_string()),
            metadata: crate::domain::AssetMetadata::default(),
        }])
        .expect("build prepared extractor");

        let snapshot =
            create_snapshot_with_extractor("2.4.0", &test_root, extractor).expect("snapshot");

        assert_eq!(snapshot.version_id, "2.4.0");
        assert_eq!(snapshot.asset_count, 1);
        assert_eq!(
            snapshot.context.scope.capture_mode.as_deref(),
            Some("prepared_asset_list_inventory")
        );
        assert!(
            snapshot
                .context
                .scope
                .mostly_install_or_package_level
                .is_none()
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn load_snapshot_defaults_scope_context_for_legacy_json() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let snapshot_path = test_root.join("legacy.json");
        fs::write(
            &snapshot_path,
            r#"{
                "schema_version":"whashreonator.snapshot.v1",
                "version_id":"2.4.0",
                "created_at_unix_ms":1,
                "source_root":"legacy",
                "asset_count":0,
                "assets":[],
                "context":{"notes":["legacy note"]}
            }"#,
        )
        .expect("write legacy snapshot");

        let snapshot = load_snapshot(&snapshot_path).expect("load legacy snapshot");

        assert_eq!(snapshot.context.notes, vec!["legacy note".to_string()]);
        assert_eq!(snapshot.context.scope, SnapshotScopeContext::default());

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn assess_scope_falls_back_to_observed_paths_for_legacy_snapshot() {
        let snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "legacy".to_string(),
            created_at_unix_ms: 1,
            source_root: "legacy".to_string(),
            asset_count: 2,
            assets: vec![
                SnapshotAsset {
                    id: "a".to_string(),
                    path: "Content/Character/Encore/Body.mesh".to_string(),
                    kind: Some("mesh".to_string()),
                    metadata: crate::domain::AssetMetadata::default(),
                    fingerprint: SnapshotFingerprint {
                        normalized_kind: None,
                        normalized_name: None,
                        name_tokens: Vec::new(),
                        path_tokens: Vec::new(),
                        tags: Vec::new(),
                        vertex_count: None,
                        index_count: None,
                        material_slots: None,
                        section_count: None,
                    },
                    hash_fields: SnapshotHashFields::default(),
                },
                SnapshotAsset {
                    id: "b".to_string(),
                    path: "Client/Config/DefaultGame.ini".to_string(),
                    kind: Some("ini".to_string()),
                    metadata: crate::domain::AssetMetadata::default(),
                    fingerprint: SnapshotFingerprint {
                        normalized_kind: None,
                        normalized_name: None,
                        name_tokens: Vec::new(),
                        path_tokens: Vec::new(),
                        tags: Vec::new(),
                        vertex_count: None,
                        index_count: None,
                        material_slots: None,
                        section_count: None,
                    },
                    hash_fields: SnapshotHashFields::default(),
                },
            ],
            context: SnapshotContext::default(),
        };

        let scope = assess_snapshot_scope(&snapshot);

        assert!(scope.observed_fallback_used);
        assert_eq!(scope.coverage.content_like_path_count, 1);
        assert_eq!(scope.coverage.character_path_count, 1);
        assert_eq!(scope.coverage.non_content_path_count, 1);
        assert!(scope.is_low_signal_for_character_analysis());
    }

    fn seed_local_asset(root: &Path, relative_path: &str) {
        let full_path = root.join(relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("create asset directory");
        }

        fs::write(full_path, b"asset").expect("write asset file");
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();

        std::env::temp_dir().join(format!("whashreonator-snapshot-test-{nanos}"))
    }
}
