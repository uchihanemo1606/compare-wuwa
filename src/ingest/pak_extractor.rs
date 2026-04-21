use std::{
    fs,
    io::{BufReader, BufWriter},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use aes::{Aes256, cipher::KeyInit};
use serde::Serialize;
use tracing::warn;

use crate::error::{AppError, AppResult};

const MANIFEST_SCHEMA_VERSION: &str = "whashreonator.pak-extraction-manifest.v1";
const MANIFEST_FILE_NAME: &str = "extraction_manifest.v1.json";
const MANIFEST_ENTRY_CAP: usize = 1000;

pub struct PakExtractionRequest {
    pub pak_path: PathBuf,
    pub aes_key: Option<AesKey>,
    pub output_dir: PathBuf,
    pub path_filter: Option<String>,
}

#[derive(PartialEq, Eq)]
pub struct AesKey {
    bytes: [u8; 32],
}

impl core::fmt::Debug for AesKey {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("AesKey([redacted])")
    }
}

impl AesKey {
    pub fn from_hex(hex: &str) -> AppResult<Self> {
        let normalized = hex.trim().strip_prefix("0x").unwrap_or(hex.trim());
        if normalized.len() != 64 {
            return Err(AppError::InvalidInput(
                "AES-256 key must be exactly 64 hex characters".to_string(),
            ));
        }

        let mut bytes = [0u8; 32];
        for (index, chunk) in normalized.as_bytes().chunks_exact(2).enumerate() {
            let pair = std::str::from_utf8(chunk).map_err(|_| {
                AppError::InvalidInput("AES-256 key must contain only ASCII hex".to_string())
            })?;
            bytes[index] = u8::from_str_radix(pair, 16).map_err(|_| {
                AppError::InvalidInput(
                    "AES-256 key must contain only hexadecimal characters".to_string(),
                )
            })?;
        }

        Ok(Self { bytes })
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub(crate) fn to_repak_key(&self) -> AppResult<Aes256> {
        Aes256::new_from_slice(self.as_bytes()).map_err(|_| {
            AppError::InvalidInput("failed to construct AES-256 key for pak reader".to_string())
        })
    }
}

#[derive(Debug, Clone)]
pub struct PakExtractionResult {
    pub manifest: PakExtractionManifest,
    pub manifest_path: PathBuf,
}

pub fn extract_pak(request: PakExtractionRequest) -> AppResult<PakExtractionResult> {
    if !request.pak_path.exists() {
        return Err(AppError::InvalidInput(format!(
            "pak file does not exist: {}",
            request.pak_path.display()
        )));
    }
    if !request.pak_path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "pak path is not a file: {}",
            request.pak_path.display()
        )));
    }

    fs::create_dir_all(&request.output_dir)?;
    let output_dir_canonical = request.output_dir.canonicalize()?;
    let started_at = SystemTime::now();
    let extracted_at_unix_ms = unix_time_ms(started_at)?;
    let pak_metadata = fs::metadata(&request.pak_path)?;
    let pak_display_path = normalize_path_for_json(&request.pak_path);

    let pak = {
        let file = fs::File::open(&request.pak_path)?;
        let mut reader = BufReader::new(file);
        let mut builder = repak::PakBuilder::new();
        if let Some(aes_key) = request.aes_key.as_ref() {
            builder = builder.key(aes_key.to_repak_key()?);
        }
        builder.reader(&mut reader).map_err(map_repak_error)?
    };

    let mount_point = pak.mount_point().to_string();
    let all_entries = pak.files();
    let total_entries_in_pak = all_entries.len();
    let mut extracted_entries = Vec::new();
    let mut extracted_bytes = 0u64;
    let mut filtered_out_entries = 0usize;
    let mut extracted_entry_count = 0usize;

    for pak_path in all_entries {
        if !matches_filter(&pak_path, request.path_filter.as_deref()) {
            filtered_out_entries += 1;
            continue;
        }

        let Some(relative_path) = normalize_pak_entry_path(&pak_path) else {
            warn!(pak_path = %pak_path, "skipping pak entry with unsafe traversal path");
            filtered_out_entries += 1;
            continue;
        };

        let output_path =
            resolve_output_path(&output_dir_canonical, relative_path.as_path(), &pak_path)?;
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
            let parent_canonical = parent.canonicalize()?;
            if !parent_canonical.starts_with(&output_dir_canonical) {
                warn!(
                    pak_path = %pak_path,
                    output_path = %output_path.display(),
                    "skipping pak entry that resolves outside output directory"
                );
                filtered_out_entries += 1;
                continue;
            }
        }

        let pak_file = fs::File::open(&request.pak_path)?;
        let mut pak_reader = BufReader::new(pak_file);
        let out_file = fs::File::create(&output_path)?;
        let mut out_writer = BufWriter::new(out_file);
        pak.read_file(&pak_path, &mut pak_reader, &mut out_writer)
            .map_err(map_repak_error)?;
        drop(out_writer);

        let size_bytes = fs::metadata(&output_path)?.len();
        extracted_bytes += size_bytes;
        extracted_entry_count += 1;

        if extracted_entries.len() < MANIFEST_ENTRY_CAP {
            extracted_entries.push(PakExtractionEntry {
                pak_path: pak_path.clone(),
                output_relative_path: normalize_relative_path(relative_path.as_path()),
                size_bytes,
            });
        }
    }

    let duration_ms = started_at
        .elapsed()
        .map_err(|error| {
            AppError::InvalidInput(format!("failed to measure extraction duration: {error}"))
        })?
        .as_millis();
    let truncated_entries = extracted_entry_count.saturating_sub(extracted_entries.len());

    let manifest = PakExtractionManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        pak_source: PakSourceInfo {
            path: pak_display_path,
            size_bytes: pak_metadata.len(),
            mount_point,
        },
        aes_key_used: request.aes_key.is_some(),
        extracted_at_unix_ms,
        filter_pattern: request.path_filter.clone(),
        summary: PakExtractionSummary {
            total_entries_in_pak,
            extracted_entries: extracted_entry_count,
            filtered_out_entries,
            extracted_bytes,
            duration_ms,
        },
        entries: extracted_entries,
        truncated_entries,
    };
    let manifest_path = output_dir_canonical.join(MANIFEST_FILE_NAME);
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    Ok(PakExtractionResult {
        manifest,
        manifest_path,
    })
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PakExtractionManifest {
    pub schema_version: String,
    pub pak_source: PakSourceInfo,
    pub aes_key_used: bool,
    pub extracted_at_unix_ms: u128,
    pub filter_pattern: Option<String>,
    pub summary: PakExtractionSummary,
    /// Entry metadata is intentionally minimal because repak's public API only exposes
    /// file paths and byte payload reads. Compression kind and compressed sizes are not
    /// available per entry without patching vendored repak, so this diagnostic manifest
    /// stores only reviewer-useful fields that can be derived after extraction.
    pub entries: Vec<PakExtractionEntry>,
    pub truncated_entries: usize,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PakExtractionEntry {
    pub pak_path: String,
    pub output_relative_path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PakExtractionSummary {
    pub total_entries_in_pak: usize,
    pub extracted_entries: usize,
    pub filtered_out_entries: usize,
    pub extracted_bytes: u64,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PakSourceInfo {
    pub path: String,
    pub size_bytes: u64,
    pub mount_point: String,
}

fn matches_filter(pak_path: &str, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let normalized = filter.replace("**", "");
    if normalized.is_empty() {
        return true;
    }
    pak_path.contains(&normalized)
}

fn normalize_pak_entry_path(pak_path: &str) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in Path::new(pak_path).components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn resolve_output_path(
    output_dir: &Path,
    relative_path: &Path,
    pak_path: &str,
) -> AppResult<PathBuf> {
    let output_path = output_dir.join(relative_path);
    let Some(parent) = output_path.parent() else {
        return Err(AppError::InvalidInput(format!(
            "failed to resolve output parent for pak entry: {pak_path}"
        )));
    };

    if !parent.starts_with(output_dir) {
        return Err(AppError::InvalidInput(format!(
            "pak entry resolves outside output directory: {pak_path}"
        )));
    }

    Ok(output_path)
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_path_for_json(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn unix_time_ms(time: SystemTime) -> AppResult<u128> {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| {
            AppError::InvalidInput(format!("system clock is before UNIX_EPOCH: {error}"))
        })
}

fn map_repak_error(error: repak::Error) -> AppError {
    match error {
        repak::Error::Io(inner) => AppError::Io(inner),
        other => AppError::InvalidInput(format!("pak extraction failed: {other}")),
    }
}
