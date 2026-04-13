use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        AssetInternalStructure, AssetMetadata, AssetRecord, AssetSourceContext,
        ExtractedAssetRecord,
    },
    error::{AppError, AppResult},
    fingerprint::{AssetFingerprint, DefaultFingerprinter, Fingerprinter},
    ingest::{
        FilteredLocalSnapshotAssetExtractor, LocalSnapshotCaptureScope, LocalSnapshotIngestSource,
        PreparedSnapshotAssetExtractor, SnapshotAssetExtractor, SnapshotExtractionScopeHint,
    },
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
    #[serde(default)]
    pub source: AssetSourceContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
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
    #[serde(default)]
    pub vertex_stride: Option<u32>,
    #[serde(default)]
    pub vertex_buffer_count: Option<u32>,
    #[serde(default)]
    pub index_format: Option<String>,
    #[serde(default)]
    pub primitive_topology: Option<String>,
    #[serde(default)]
    pub layout_markers: Vec<String>,
    #[serde(default)]
    pub internal_structure: AssetInternalStructure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotHashFields {
    pub asset_hash: Option<String>,
    pub shader_hash: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotContext {
    pub launcher: Option<SnapshotLauncherContext>,
    pub resource_manifest: Option<SnapshotResourceManifestContext>,
    #[serde(default)]
    pub extractor: Option<SnapshotExtractorContext>,
    #[serde(default)]
    pub scope: SnapshotScopeContext,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotScopeContext {
    pub acquisition_kind: Option<String>,
    pub capture_mode: Option<String>,
    pub mostly_install_or_package_level: Option<bool>,
    pub meaningful_content_coverage: Option<bool>,
    pub meaningful_character_coverage: Option<bool>,
    pub meaningful_asset_record_enrichment: Option<bool>,
    pub coverage: SnapshotCoverageSignals,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SnapshotExtractorContext {
    pub inventory_path: Option<String>,
    pub extraction_tool: Option<String>,
    pub extraction_kind: Option<String>,
    pub inventory_source_root: Option<String>,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub record_count: usize,
    pub records_with_hashes: usize,
    pub records_with_source_context: usize,
    pub records_with_rich_metadata: usize,
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
    pub acquisition_kind: Option<String>,
    pub capture_mode: Option<String>,
    pub mostly_install_or_package_level: bool,
    pub meaningful_content_coverage: bool,
    pub meaningful_character_coverage: bool,
    pub meaningful_asset_record_enrichment: bool,
    pub coverage: SnapshotCoverageSignals,
    pub note: Option<String>,
    pub observed_fallback_used: bool,
}

impl SnapshotScopeAssessment {
    pub fn is_low_signal_for_character_analysis(&self) -> bool {
        match self.acquisition_kind.as_deref() {
            Some("shallow_filesystem_inventory") => true,
            Some("extractor_backed_asset_records") => {
                !self.meaningful_content_coverage
                    || !self.meaningful_character_coverage
                    || !self.meaningful_asset_record_enrichment
            }
            _ => {
                self.mostly_install_or_package_level
                    || !self.meaningful_content_coverage
                    || !self.meaningful_character_coverage
                    || !self.meaningful_asset_record_enrichment
            }
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotCaptureQualitySummary {
    pub launcher_detected_version: Option<String>,
    pub launcher_reuse_version: Option<String>,
    pub launcher_version_matches_snapshot: Option<bool>,
    pub manifest_resource_count: usize,
    pub manifest_matched_assets: usize,
    pub manifest_unmatched_snapshot_assets: usize,
    pub asset_count: usize,
    pub assets_with_asset_hash: usize,
    pub assets_with_any_hash: usize,
    pub assets_with_signature: usize,
    pub assets_with_source_context: usize,
    pub assets_with_rich_metadata: usize,
    pub meaningfully_enriched_assets: usize,
    pub extractor_record_count: usize,
    pub extractor_records_with_hashes: usize,
    pub extractor_records_with_source_context: usize,
    pub extractor_records_with_rich_metadata: usize,
}

impl SnapshotCaptureQualitySummary {
    pub fn low_signal_reasons(&self, scope: &SnapshotScopeAssessment) -> Vec<String> {
        let mut reasons = Vec::new();
        if scope.mostly_install_or_package_level {
            reasons.push("install/package-level coverage dominates this snapshot".to_string());
        }
        if !scope.meaningful_content_coverage {
            reasons
                .push("content-like coverage is still below the meaningful threshold".to_string());
        }
        if !scope.meaningful_character_coverage {
            reasons.push(
                "character-like coverage is still below the meaningful threshold".to_string(),
            );
        }
        if !scope.meaningful_asset_record_enrichment {
            reasons.push(
                "asset-level enrichment is still too sparse for strong later compare confidence"
                    .to_string(),
            );
        }
        if self.launcher_detected_version.is_none() {
            reasons.push("launcher version evidence is missing".to_string());
        }
        if self.manifest_resource_count == 0 {
            reasons.push("resource manifest evidence is missing".to_string());
        } else if self.manifest_matched_assets == 0 {
            reasons.push("resource manifest did not match any snapshot assets".to_string());
        }

        reasons
    }
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
        let scope_hint = self.asset_source.scope_hint();
        let assets = self.asset_source.extract_snapshot_assets(source_root)?;
        let extractor_context = matches!(
            extraction_mode,
            crate::ingest::SnapshotExtractionMode::ExtractorBackedAssetRecords
        )
        .then(|| build_extractor_context(&scope_hint, &assets));
        let snapshot_assets = assets
            .iter()
            .map(|asset| {
                SnapshotAsset::from_asset(
                    &asset.asset,
                    self.fingerprinter.fingerprint(&asset.asset),
                    &asset.hash_fields,
                    &asset.source,
                )
            })
            .collect::<Vec<_>>();

        let mut snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.trim().to_string(),
            created_at_unix_ms: current_unix_ms()?,
            source_root: normalize_source_root(source_root),
            asset_count: snapshot_assets.len(),
            assets: snapshot_assets,
            context: SnapshotContext {
                launcher: None,
                resource_manifest: None,
                extractor: extractor_context,
                scope: SnapshotScopeContext {
                    acquisition_kind: Some(extraction_mode.acquisition_kind().to_string()),
                    capture_mode: Some(extraction_mode.capture_mode().to_string()),
                    ..SnapshotScopeContext::default()
                },
                notes: Vec::new(),
            },
        };

        annotate_snapshot_scope_from_extractor(&mut snapshot, extraction_mode, scope_hint);
        Ok(snapshot)
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
    create_local_snapshot_with_capture_scope(
        version_id,
        source_root,
        LocalSnapshotCaptureScope::FullInventory,
    )
}

pub fn create_local_snapshot_with_capture_scope(
    version_id: &str,
    source_root: &Path,
    capture_scope: LocalSnapshotCaptureScope,
) -> AppResult<GameSnapshot> {
    // Current default path remains install/package-level filesystem inventory.
    let mut snapshot = match capture_scope {
        LocalSnapshotCaptureScope::FullInventory => {
            create_snapshot_with_extractor(version_id, source_root, LocalSnapshotIngestSource)?
        }
        LocalSnapshotCaptureScope::ContentFocused | LocalSnapshotCaptureScope::CharacterFocused => {
            create_snapshot_with_extractor(
                version_id,
                source_root,
                FilteredLocalSnapshotAssetExtractor::new(capture_scope),
            )?
        }
    };
    enrich_snapshot_from_game_root(&mut snapshot, source_root)?;
    annotate_local_snapshot_scope(&mut snapshot);
    Ok(snapshot)
}

pub fn create_prepared_snapshot(
    version_id: &str,
    source_root: &Path,
    extractor: PreparedSnapshotAssetExtractor,
) -> AppResult<GameSnapshot> {
    create_extractor_backed_snapshot(version_id, source_root, extractor)
}

pub fn create_extractor_backed_snapshot(
    version_id: &str,
    source_root: &Path,
    extractor: PreparedSnapshotAssetExtractor,
) -> AppResult<GameSnapshot> {
    let mut snapshot = create_snapshot_with_extractor(version_id, source_root, extractor)?;
    enrich_snapshot_from_game_root(&mut snapshot, source_root)?;
    Ok(snapshot)
}

pub fn create_prepared_snapshot_from_inventory(
    version_id: &str,
    source_root: &Path,
    inventory: crate::domain::PreparedAssetInventory,
) -> AppResult<GameSnapshot> {
    create_extractor_backed_snapshot_from_inventory(version_id, source_root, inventory)
}

pub fn create_extractor_backed_snapshot_from_inventory(
    version_id: &str,
    source_root: &Path,
    inventory: crate::domain::PreparedAssetInventory,
) -> AppResult<GameSnapshot> {
    create_prepared_snapshot(
        version_id,
        source_root,
        PreparedSnapshotAssetExtractor::from_inventory(inventory)?,
    )
}

pub fn create_prepared_snapshot_from_file(
    version_id: &str,
    source_root: &Path,
    inventory_path: &Path,
) -> AppResult<GameSnapshot> {
    create_extractor_backed_snapshot_from_file(version_id, source_root, inventory_path)
}

pub fn create_extractor_backed_snapshot_from_file(
    version_id: &str,
    source_root: &Path,
    inventory_path: &Path,
) -> AppResult<GameSnapshot> {
    create_prepared_snapshot(
        version_id,
        source_root,
        PreparedSnapshotAssetExtractor::from_json(inventory_path)?,
    )
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
    fn from_asset(
        asset: &AssetRecord,
        fingerprint: AssetFingerprint,
        hash_fields: &crate::domain::AssetHashFields,
        source: &AssetSourceContext,
    ) -> Self {
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
                vertex_stride: fingerprint.vertex_stride,
                vertex_buffer_count: fingerprint.vertex_buffer_count,
                index_format: fingerprint.index_format,
                primitive_topology: fingerprint.primitive_topology,
                layout_markers: fingerprint.layout_markers.into_iter().collect(),
                internal_structure: fingerprint.internal_structure,
            },
            hash_fields: SnapshotHashFields {
                asset_hash: hash_fields.asset_hash.clone(),
                shader_hash: hash_fields.shader_hash.clone(),
                signature: hash_fields.signature.clone(),
            },
            source: source.clone(),
        }
    }
}

fn build_extractor_context(
    scope_hint: &SnapshotExtractionScopeHint,
    assets: &[ExtractedAssetRecord],
) -> SnapshotExtractorContext {
    SnapshotExtractorContext {
        inventory_path: scope_hint.inventory_path.clone(),
        extraction_tool: scope_hint.extraction_tool.clone(),
        extraction_kind: scope_hint.extraction_kind.clone(),
        inventory_source_root: scope_hint.inventory_source_root.clone(),
        tags: scope_hint.tags.clone(),
        note: scope_hint.note.clone(),
        record_count: assets.len(),
        records_with_hashes: assets
            .iter()
            .filter(|asset| has_hash_fields(&asset.hash_fields))
            .count(),
        records_with_source_context: assets
            .iter()
            .filter(|asset| has_source_context(&asset.source))
            .count(),
        records_with_rich_metadata: assets
            .iter()
            .filter(|asset| has_rich_asset_metadata(&asset.asset.metadata))
            .count(),
    }
}

fn has_hash_fields(hash_fields: &crate::domain::AssetHashFields) -> bool {
    hash_fields.asset_hash.is_some()
        || hash_fields.shader_hash.is_some()
        || hash_fields.signature.is_some()
}

fn has_source_context(source: &AssetSourceContext) -> bool {
    source.extraction_tool.is_some()
        || source.source_root.is_some()
        || source.source_path.is_some()
        || source.container_path.is_some()
        || source.source_kind.is_some()
}

fn has_rich_asset_metadata(metadata: &AssetMetadata) -> bool {
    metadata.vertex_count.is_some()
        || metadata.index_count.is_some()
        || metadata.material_slots.is_some()
        || metadata.section_count.is_some()
        || metadata.vertex_stride.is_some()
        || metadata.vertex_buffer_count.is_some()
        || metadata.index_format.is_some()
        || metadata.primitive_topology.is_some()
        || !metadata.layout_markers.is_empty()
        || metadata.internal_structure != AssetInternalStructure::default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AssetRecordEnrichmentSummary {
    record_count: usize,
    enriched_record_count: usize,
    meaningful_enriched_record_threshold: usize,
}

fn has_snapshot_hash_fields(hash_fields: &SnapshotHashFields) -> bool {
    hash_fields.asset_hash.is_some()
        || hash_fields.shader_hash.is_some()
        || hash_fields.signature.is_some()
}

fn is_meaningfully_enriched_snapshot_asset(asset: &SnapshotAsset) -> bool {
    has_snapshot_hash_fields(&asset.hash_fields)
        || has_source_context(&asset.source)
        || has_rich_asset_metadata(&asset.metadata)
}

fn min_meaningful_enriched_record_count(record_count: usize) -> usize {
    match record_count {
        0 => return 0,
        1 | 2 => return MIN_FULLY_ENRICHED_SMALL_SNAPSHOT_RECORD_COUNT,
        3 | 4 => return record_count,
        _ => {}
    }

    let numerator = record_count.saturating_mul(MIN_MEANINGFUL_ENRICHED_RECORD_RATIO_NUMERATOR);
    let ratio_threshold = numerator / MIN_MEANINGFUL_ENRICHED_RECORD_RATIO_DENOMINATOR
        + usize::from(numerator % MIN_MEANINGFUL_ENRICHED_RECORD_RATIO_DENOMINATOR != 0);

    MIN_MEANINGFUL_ENRICHED_RECORD_COUNT.max(ratio_threshold)
}

fn summarize_asset_record_enrichment(
    snapshot: &GameSnapshot,
) -> Option<AssetRecordEnrichmentSummary> {
    let extractor = snapshot.context.extractor.as_ref()?;
    let record_count = extractor.record_count.max(snapshot.asset_count);
    let enriched_record_count = snapshot
        .assets
        .iter()
        .filter(|asset| is_meaningfully_enriched_snapshot_asset(asset))
        .count();

    Some(AssetRecordEnrichmentSummary {
        record_count,
        enriched_record_count,
        meaningful_enriched_record_threshold: min_meaningful_enriched_record_count(record_count),
    })
}

fn has_meaningful_asset_record_enrichment(snapshot: &GameSnapshot) -> bool {
    summarize_asset_record_enrichment(snapshot).is_some_and(|summary| {
        summary.record_count > 0
            && summary.enriched_record_count >= summary.meaningful_enriched_record_threshold
    })
}

fn annotate_snapshot_scope_from_extractor(
    snapshot: &mut GameSnapshot,
    extraction_mode: crate::ingest::SnapshotExtractionMode,
    scope_hint: SnapshotExtractionScopeHint,
) {
    if !matches!(
        extraction_mode,
        crate::ingest::SnapshotExtractionMode::ExtractorBackedAssetRecords
    ) {
        return;
    }

    let coverage = compute_scope_coverage(snapshot);
    let meaningful_content_coverage = scope_hint
        .meaningful_content_coverage
        .unwrap_or(coverage.content_like_path_count >= MIN_MEANINGFUL_CONTENT_PATH_COUNT);
    let meaningful_character_coverage = scope_hint
        .meaningful_character_coverage
        .unwrap_or(coverage.character_path_count >= MIN_MEANINGFUL_CHARACTER_PATH_COUNT);
    let meaningful_asset_record_enrichment = has_meaningful_asset_record_enrichment(snapshot);
    let mut extractor_note = scope_hint.note.unwrap_or_else(|| {
        "extractor-backed asset records were bridged into runtime snapshot acquisition".to_string()
    });
    extractor_note.push_str(
        "; snapshot uses richer extractor-backed asset records instead of shallow install/package inventory",
    );
    if let Some(extractor_context) = snapshot.context.extractor.as_ref() {
        extractor_note.push_str(&format!(
            "; records={} hashes={} source_context={} rich_metadata={}",
            extractor_context.record_count,
            extractor_context.records_with_hashes,
            extractor_context.records_with_source_context,
            extractor_context.records_with_rich_metadata
        ));
        if let Some(inventory_path) = extractor_context.inventory_path.as_deref() {
            extractor_note.push_str(&format!("; inventory_path={inventory_path}"));
        }
    }
    if let Some(summary) = summarize_asset_record_enrichment(snapshot) {
        extractor_note.push_str(&format!(
            "; enriched_records={}/{} threshold={}",
            summary.enriched_record_count,
            summary.record_count,
            summary.meaningful_enriched_record_threshold
        ));
    }
    if !(meaningful_content_coverage && meaningful_character_coverage) {
        extractor_note.push_str(
            ", but extracted coverage is still partial for deep character-level analysis",
        );
    }
    if !meaningful_asset_record_enrichment {
        extractor_note.push_str(
            ", and current extractor records still preserve little asset-level enrichment beyond path identity across the full snapshot",
        );
    }

    snapshot.context.scope = SnapshotScopeContext {
        acquisition_kind: Some(extraction_mode.acquisition_kind().to_string()),
        capture_mode: Some(extraction_mode.capture_mode().to_string()),
        mostly_install_or_package_level: Some(false),
        meaningful_content_coverage: Some(meaningful_content_coverage),
        meaningful_character_coverage: Some(meaningful_character_coverage),
        meaningful_asset_record_enrichment: Some(meaningful_asset_record_enrichment),
        coverage,
        note: Some(extractor_note.clone()),
    };
    snapshot
        .context
        .notes
        .push("extractor-backed runtime snapshot created from richer asset records".to_string());
    snapshot.context.notes.push(extractor_note);
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
            let mut preserved_existing_hashes = 0usize;
            let mut conflicting_existing_hashes = 0usize;
            for asset in &mut snapshot.assets {
                if let Some(entry) = manifest_entries.get(&asset.path) {
                    matched_assets += 1;
                    match asset.hash_fields.asset_hash.as_deref() {
                        None => asset.hash_fields.asset_hash = Some(entry.md5.clone()),
                        Some(existing) if existing == entry.md5 => {
                            preserved_existing_hashes += 1;
                        }
                        Some(_) => {
                            conflicting_existing_hashes += 1;
                        }
                    }
                }
            }

            snapshot.context.resource_manifest = Some(SnapshotResourceManifestContext {
                matched_assets,
                unmatched_snapshot_assets: snapshot.assets.len().saturating_sub(matched_assets),
                ..manifest_context
            });
            if preserved_existing_hashes > 0 || conflicting_existing_hashes > 0 {
                notes.push(format!(
                    "resource manifest matched {matched_assets} asset paths; preserved existing extractor asset_hash values for {} assets (conflicts: {})",
                    preserved_existing_hashes + conflicting_existing_hashes,
                    conflicting_existing_hashes
                ));
            }
        }
        None => notes.push("LocalGameResources.json not found; asset hashes were not enriched from launcher manifest".to_string()),
    }

    snapshot.context.notes.extend(notes);
    Ok(())
}

const MIN_MEANINGFUL_CONTENT_PATH_COUNT: usize = 10;
const MIN_MEANINGFUL_CHARACTER_PATH_COUNT: usize = 5;
const MIN_FULLY_ENRICHED_SMALL_SNAPSHOT_RECORD_COUNT: usize = 3;
const MIN_MEANINGFUL_ENRICHED_RECORD_COUNT: usize = 5;
const MIN_MEANINGFUL_ENRICHED_RECORD_RATIO_NUMERATOR: usize = 2;
const MIN_MEANINGFUL_ENRICHED_RECORD_RATIO_DENOMINATOR: usize = 5;

pub fn assess_snapshot_scope(snapshot: &GameSnapshot) -> SnapshotScopeAssessment {
    let scope = &snapshot.context.scope;
    let acquisition_kind = scope
        .acquisition_kind
        .clone()
        .or_else(|| infer_acquisition_kind_from_capture_mode(scope.capture_mode.as_deref()));
    let has_explicit_scope_flags = scope.mostly_install_or_package_level.is_some()
        || scope.meaningful_content_coverage.is_some()
        || scope.meaningful_character_coverage.is_some()
        || scope.meaningful_asset_record_enrichment.is_some();
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
    let meaningful_asset_record_enrichment = scope
        .meaningful_asset_record_enrichment
        .unwrap_or_else(|| has_meaningful_asset_record_enrichment(snapshot));
    let mostly_install_or_package_level =
        scope.mostly_install_or_package_level.unwrap_or_else(|| {
            if acquisition_kind.as_deref() == Some("extractor_backed_asset_records") {
                false
            } else {
                !(meaningful_content_coverage && meaningful_character_coverage)
            }
        });

    SnapshotScopeAssessment {
        acquisition_kind,
        capture_mode: scope.capture_mode.clone(),
        mostly_install_or_package_level,
        meaningful_content_coverage,
        meaningful_character_coverage,
        meaningful_asset_record_enrichment,
        coverage,
        note: scope.note.clone(),
        observed_fallback_used,
    }
}

pub fn summarize_snapshot_capture_quality(
    snapshot: &GameSnapshot,
) -> SnapshotCaptureQualitySummary {
    SnapshotCaptureQualitySummary {
        launcher_detected_version: snapshot
            .context
            .launcher
            .as_ref()
            .map(|launcher| launcher.detected_version.clone()),
        launcher_reuse_version: snapshot
            .context
            .launcher
            .as_ref()
            .and_then(|launcher| launcher.reuse_version.clone()),
        launcher_version_matches_snapshot: snapshot
            .context
            .launcher
            .as_ref()
            .map(|launcher| launcher.detected_version == snapshot.version_id),
        manifest_resource_count: snapshot
            .context
            .resource_manifest
            .as_ref()
            .map(|manifest| manifest.resource_count)
            .unwrap_or_default(),
        manifest_matched_assets: snapshot
            .context
            .resource_manifest
            .as_ref()
            .map(|manifest| manifest.matched_assets)
            .unwrap_or_default(),
        manifest_unmatched_snapshot_assets: snapshot
            .context
            .resource_manifest
            .as_ref()
            .map(|manifest| manifest.unmatched_snapshot_assets)
            .unwrap_or_default(),
        asset_count: snapshot.asset_count,
        assets_with_asset_hash: snapshot
            .assets
            .iter()
            .filter(|asset| asset.hash_fields.asset_hash.is_some())
            .count(),
        assets_with_any_hash: snapshot
            .assets
            .iter()
            .filter(|asset| has_snapshot_hash_fields(&asset.hash_fields))
            .count(),
        assets_with_signature: snapshot
            .assets
            .iter()
            .filter(|asset| asset.hash_fields.signature.is_some())
            .count(),
        assets_with_source_context: snapshot
            .assets
            .iter()
            .filter(|asset| has_source_context(&asset.source))
            .count(),
        assets_with_rich_metadata: snapshot
            .assets
            .iter()
            .filter(|asset| has_rich_asset_metadata(&asset.metadata))
            .count(),
        meaningfully_enriched_assets: snapshot
            .assets
            .iter()
            .filter(|asset| is_meaningfully_enriched_snapshot_asset(asset))
            .count(),
        extractor_record_count: snapshot
            .context
            .extractor
            .as_ref()
            .map(|extractor| extractor.record_count)
            .unwrap_or_default(),
        extractor_records_with_hashes: snapshot
            .context
            .extractor
            .as_ref()
            .map(|extractor| extractor.records_with_hashes)
            .unwrap_or_default(),
        extractor_records_with_source_context: snapshot
            .context
            .extractor
            .as_ref()
            .map(|extractor| extractor.records_with_source_context)
            .unwrap_or_default(),
        extractor_records_with_rich_metadata: snapshot
            .context
            .extractor
            .as_ref()
            .map(|extractor| extractor.records_with_rich_metadata)
            .unwrap_or_default(),
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
    let capture_mode = snapshot
        .context
        .scope
        .capture_mode
        .clone()
        .unwrap_or_else(|| "local_filesystem_inventory".to_string());

    let mut note = if mostly_install_or_package_level {
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
    if capture_mode != "local_filesystem_inventory" {
        note.push_str(&format!(
            "; capture mode '{}' narrows paths with path-based filtering only (not deep semantic extraction)",
            capture_mode
        ));
    }
    note.push_str(
        "; this remains shallow filesystem inventory fallback and should be treated conservatively",
    );

    snapshot.context.scope = SnapshotScopeContext {
        acquisition_kind: Some("shallow_filesystem_inventory".to_string()),
        capture_mode: Some(capture_mode),
        mostly_install_or_package_level: Some(mostly_install_or_package_level),
        meaningful_content_coverage: Some(meaningful_content_coverage),
        meaningful_character_coverage: Some(meaningful_character_coverage),
        meaningful_asset_record_enrichment: Some(false),
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

fn infer_acquisition_kind_from_capture_mode(capture_mode: Option<&str>) -> Option<String> {
    match capture_mode {
        Some("extractor_backed_asset_records") | Some("prepared_asset_list_inventory") => {
            Some("extractor_backed_asset_records".to_string())
        }
        Some(mode) if mode.starts_with("local_filesystem_inventory") => {
            Some("shallow_filesystem_inventory".to_string())
        }
        _ => None,
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

    use crate::ingest::{LocalSnapshotCaptureScope, PreparedSnapshotAssetExtractor};

    use super::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
        SnapshotScopeContext, assess_snapshot_scope, create_local_snapshot,
        create_local_snapshot_with_capture_scope, create_prepared_snapshot_from_file,
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
        let extractor = PreparedSnapshotAssetExtractor::new(vec![
            crate::domain::AssetRecord {
                id: "asset-1".to_string(),
                path: "Content/Character/Encore/Body.mesh".to_string(),
                kind: Some("mesh".to_string()),
                metadata: crate::domain::AssetMetadata::default(),
            }
            .into(),
        ])
        .expect("build prepared extractor");

        let snapshot =
            create_snapshot_with_extractor("2.4.0", &test_root, extractor).expect("snapshot");

        assert_eq!(snapshot.version_id, "2.4.0");
        assert_eq!(snapshot.asset_count, 1);
        assert_eq!(
            snapshot.context.scope.acquisition_kind.as_deref(),
            Some("extractor_backed_asset_records")
        );
        assert_eq!(
            snapshot.context.scope.capture_mode.as_deref(),
            Some("extractor_backed_asset_records")
        );
        assert_eq!(
            snapshot.context.scope.mostly_install_or_package_level,
            Some(false)
        );
        assert_eq!(
            snapshot.context.scope.meaningful_asset_record_enrichment,
            Some(false)
        );
        assert!(
            snapshot
                .context
                .scope
                .note
                .as_deref()
                .is_some_and(|note| note.contains("extractor-backed"))
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn prepared_asset_inventory_preserves_asset_level_fields_even_when_sample_is_small() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let inventory_path = test_root.join("prepared-assets.json");
        fs::write(
            &inventory_path,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "context":{
                    "extraction_tool":"fixture-extractor",
                    "extraction_kind":"asset_records",
                    "source_root":"D:/prepared",
                    "meaningful_content_coverage":true,
                    "meaningful_character_coverage":true
                },
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "vertex_count":120,
                            "index_count":240,
                            "material_slots":2,
                            "section_count":3,
                            "tags":["character","prepared"]
                        },
                        "hash_fields":{
                            "asset_hash":"asset-md5",
                            "shader_hash":"shader-md5",
                            "signature":"sig-001"
                        },
                        "source":{
                            "extraction_tool":"fixture-extractor",
                            "source_root":"D:/prepared",
                            "source_path":"Content/Character/Encore/Body.mesh",
                            "source_kind":"mesh_record",
                            "container_path":"pakchunk0-WindowsNoEditor.pak"
                        }
                    }
                ]
            }"#,
        )
        .expect("write prepared inventory");

        let snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &inventory_path)
            .expect("snapshot");

        assert_eq!(snapshot.asset_count, 1);
        assert_eq!(
            snapshot.context.scope.acquisition_kind.as_deref(),
            Some("extractor_backed_asset_records")
        );
        assert_eq!(
            snapshot.context.scope.capture_mode.as_deref(),
            Some("extractor_backed_asset_records")
        );
        assert_eq!(
            snapshot.context.scope.mostly_install_or_package_level,
            Some(false)
        );
        assert_eq!(
            snapshot.context.scope.meaningful_content_coverage,
            Some(true)
        );
        assert_eq!(
            snapshot.context.scope.meaningful_character_coverage,
            Some(true)
        );
        assert_eq!(
            snapshot.context.scope.meaningful_asset_record_enrichment,
            Some(false)
        );
        assert!(assess_snapshot_scope(&snapshot).is_low_signal_for_character_analysis());
        assert!(snapshot.context.scope.note.as_deref().is_some_and(|note| {
            note.contains("fixture-extractor") && note.contains("enriched_records=1/1")
        }));
        assert_eq!(snapshot.assets[0].metadata.vertex_count, Some(120));
        assert_eq!(snapshot.assets[0].fingerprint.vertex_count, Some(120));
        assert_eq!(snapshot.assets[0].fingerprint.index_count, Some(240));
        assert_eq!(snapshot.assets[0].fingerprint.material_slots, Some(2));
        assert_eq!(snapshot.assets[0].fingerprint.section_count, Some(3));
        assert_eq!(
            snapshot.assets[0].hash_fields.asset_hash.as_deref(),
            Some("asset-md5")
        );
        assert_eq!(
            snapshot.assets[0].hash_fields.shader_hash.as_deref(),
            Some("shader-md5")
        );
        assert_eq!(
            snapshot.assets[0].hash_fields.signature.as_deref(),
            Some("sig-001")
        );
        assert_eq!(
            snapshot.assets[0].source.extraction_tool.as_deref(),
            Some("fixture-extractor")
        );
        assert_eq!(
            snapshot.assets[0].source.source_root.as_deref(),
            Some("D:/prepared")
        );
        assert_eq!(
            snapshot.assets[0].source.source_path.as_deref(),
            Some("Content/Character/Encore/Body.mesh")
        );
        assert_eq!(
            snapshot.assets[0].source.source_kind.as_deref(),
            Some("mesh_record")
        );
        assert_eq!(
            snapshot.assets[0].source.container_path.as_deref(),
            Some("pakchunk0-WindowsNoEditor.pak")
        );
        assert_eq!(
            snapshot
                .context
                .extractor
                .as_ref()
                .and_then(|context| context.inventory_path.clone()),
            Some(inventory_path.to_string_lossy().replace('\\', "/"))
        );
        assert_eq!(
            snapshot
                .context
                .extractor
                .as_ref()
                .map(|context| context.records_with_hashes),
            Some(1)
        );
        assert_eq!(
            snapshot
                .context
                .extractor
                .as_ref()
                .map(|context| context.records_with_source_context),
            Some(1)
        );
        assert_eq!(
            snapshot
                .context
                .extractor
                .as_ref()
                .map(|context| context.records_with_rich_metadata),
            Some(1)
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn partial_extractor_backed_snapshot_stays_low_signal_without_explicit_coverage_hints() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let inventory_path = test_root.join("prepared-assets.json");
        fs::write(
            &inventory_path,
            r#"{
                "schema_version":"whashreonator.prepared-assets.v1",
                "context":{
                    "extraction_tool":"fixture-extractor",
                    "extraction_kind":"asset_records",
                    "source_root":"D:/prepared"
                },
                "assets":[
                    {
                        "id":"mesh:encore:body",
                        "path":"Content/Character/Encore/Body.mesh",
                        "kind":"mesh",
                        "metadata":{
                            "logical_name":"Encore Body",
                            "tags":["character","prepared"]
                        },
                        "hash_fields":{
                            "asset_hash":"asset-md5"
                        },
                        "source":{
                            "extraction_tool":"fixture-extractor",
                            "source_root":"D:/prepared",
                            "source_path":"Content/Character/Encore/Body.mesh",
                            "source_kind":"mesh_record",
                            "container_path":"pakchunk0-WindowsNoEditor.pak"
                        }
                    }
                ]
            }"#,
        )
        .expect("write prepared inventory");

        let snapshot = create_prepared_snapshot_from_file("6.0.0", &test_root, &inventory_path)
            .expect("snapshot");
        let scope = assess_snapshot_scope(&snapshot);

        assert_eq!(
            scope.acquisition_kind.as_deref(),
            Some("extractor_backed_asset_records")
        );
        assert!(!scope.mostly_install_or_package_level);
        assert!(!scope.meaningful_content_coverage);
        assert!(!scope.meaningful_character_coverage);
        assert!(!scope.meaningful_asset_record_enrichment);
        assert!(scope.is_low_signal_for_character_analysis());
        assert!(
            snapshot
                .context
                .scope
                .note
                .as_deref()
                .is_some_and(|note| note.contains("enriched_records=1/1"))
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn extractor_backed_path_only_records_stay_low_signal_even_with_broad_character_coverage() {
        let test_root = unique_test_dir();
        fs::create_dir_all(&test_root).expect("create test root");
        let extractor = PreparedSnapshotAssetExtractor::new(
            (0..12)
                .map(|index| crate::domain::AssetRecord {
                    id: format!("asset-{index}"),
                    path: format!("Content/Character/Encore/Variant{index}.mesh"),
                    kind: Some("mesh".to_string()),
                    metadata: crate::domain::AssetMetadata::default(),
                })
                .map(Into::into)
                .collect(),
        )
        .expect("build extractor");

        let snapshot =
            create_snapshot_with_extractor("6.2.0", &test_root, extractor).expect("snapshot");
        let scope = assess_snapshot_scope(&snapshot);

        assert_eq!(
            scope.acquisition_kind.as_deref(),
            Some("extractor_backed_asset_records")
        );
        assert!(!scope.mostly_install_or_package_level);
        assert!(scope.meaningful_content_coverage);
        assert!(scope.meaningful_character_coverage);
        assert!(!scope.meaningful_asset_record_enrichment);
        assert!(scope.is_low_signal_for_character_analysis());
        assert!(
            snapshot
                .context
                .scope
                .note
                .as_deref()
                .is_some_and(|note| note.contains("preserve little asset-level enrichment"))
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
    fn create_local_snapshot_with_content_focus_filters_non_content_paths() {
        let test_root = unique_test_dir();
        let local_root = test_root.join("game");
        seed_local_asset(&local_root, "Client/Config/DefaultGame.ini");
        seed_local_asset(&local_root, "Content/Character/HeroA/Body.mesh");
        seed_local_asset(&local_root, "Content/Weapon/Sword.weapon");

        let snapshot = create_local_snapshot_with_capture_scope(
            "2.4.0",
            &local_root,
            LocalSnapshotCaptureScope::ContentFocused,
        )
        .expect("create content-focused snapshot");

        assert_eq!(snapshot.asset_count, 2);
        assert!(
            snapshot
                .assets
                .iter()
                .all(|asset| asset.path.starts_with("Content/"))
        );
        assert_eq!(
            snapshot.context.scope.capture_mode.as_deref(),
            Some("local_filesystem_inventory_content_focused")
        );
        assert_eq!(snapshot.context.scope.coverage.content_like_path_count, 2);
        assert_eq!(snapshot.context.scope.coverage.non_content_path_count, 0);
        assert!(
            snapshot
                .context
                .scope
                .note
                .as_deref()
                .is_some_and(|note| note.contains("path-based filtering only"))
        );

        let _ = fs::remove_dir_all(&test_root);
    }

    #[test]
    fn create_local_snapshot_with_character_focus_keeps_character_paths_only() {
        let test_root = unique_test_dir();
        let local_root = test_root.join("game");
        seed_local_asset(&local_root, "Content/Character/HeroA/Body.mesh");
        seed_local_asset(&local_root, "Content/Weapon/Sword.weapon");
        seed_local_asset(&local_root, "Client/Config/DefaultGame.ini");

        let snapshot = create_local_snapshot_with_capture_scope(
            "2.4.0",
            &local_root,
            LocalSnapshotCaptureScope::CharacterFocused,
        )
        .expect("create character-focused snapshot");

        assert_eq!(snapshot.asset_count, 1);
        assert_eq!(snapshot.assets[0].path, "Content/Character/HeroA/Body.mesh");
        assert_eq!(
            snapshot.context.scope.capture_mode.as_deref(),
            Some("local_filesystem_inventory_character_focused")
        );
        assert_eq!(snapshot.context.scope.coverage.content_like_path_count, 1);
        assert_eq!(snapshot.context.scope.coverage.character_path_count, 1);
        assert_eq!(snapshot.context.scope.coverage.non_content_path_count, 0);

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
                        ..Default::default()
                    },
                    hash_fields: SnapshotHashFields::default(),
                    source: crate::domain::AssetSourceContext::default(),
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
                        ..Default::default()
                    },
                    hash_fields: SnapshotHashFields::default(),
                    source: crate::domain::AssetSourceContext::default(),
                },
            ],
            context: SnapshotContext::default(),
        };

        let scope = assess_snapshot_scope(&snapshot);

        assert!(scope.observed_fallback_used);
        assert_eq!(scope.coverage.content_like_path_count, 1);
        assert_eq!(scope.coverage.character_path_count, 1);
        assert_eq!(scope.coverage.non_content_path_count, 1);
        assert!(!scope.meaningful_asset_record_enrichment);
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
