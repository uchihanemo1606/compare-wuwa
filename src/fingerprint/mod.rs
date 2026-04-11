use std::{collections::BTreeSet, path::Path};

use crate::domain::{AssetInternalStructure, AssetRecord};

pub trait Fingerprinter {
    fn fingerprint(&self, asset: &AssetRecord) -> AssetFingerprint;
}

#[derive(Debug, Default, Clone)]
pub struct DefaultFingerprinter;

#[derive(Debug, Clone)]
pub struct AssetFingerprint {
    pub asset: AssetRecord,
    pub normalized_kind: Option<String>,
    pub normalized_name: Option<String>,
    pub name_tokens: BTreeSet<String>,
    pub path_tokens: BTreeSet<String>,
    pub tags: BTreeSet<String>,
    pub vertex_count: Option<u32>,
    pub index_count: Option<u32>,
    pub material_slots: Option<u32>,
    pub section_count: Option<u32>,
    pub vertex_stride: Option<u32>,
    pub vertex_buffer_count: Option<u32>,
    pub index_format: Option<String>,
    pub primitive_topology: Option<String>,
    pub layout_markers: BTreeSet<String>,
    pub internal_structure: AssetInternalStructure,
}

impl Fingerprinter for DefaultFingerprinter {
    fn fingerprint(&self, asset: &AssetRecord) -> AssetFingerprint {
        let derived_name = asset
            .metadata
            .logical_name
            .clone()
            .or_else(|| file_stem(&asset.path))
            .or_else(|| Some(asset.id.clone()));

        AssetFingerprint {
            asset: asset.clone(),
            normalized_kind: asset.kind.as_deref().map(normalize_text),
            normalized_name: derived_name.as_deref().map(normalize_text),
            name_tokens: derived_name.as_deref().map(tokenize).unwrap_or_default(),
            path_tokens: tokenize(&asset.path),
            tags: asset
                .metadata
                .tags
                .iter()
                .map(|tag| normalize_text(tag))
                .filter(|tag| !tag.is_empty())
                .collect(),
            vertex_count: asset.metadata.vertex_count,
            index_count: asset.metadata.index_count,
            material_slots: asset.metadata.material_slots,
            section_count: asset.metadata.section_count,
            vertex_stride: asset.metadata.vertex_stride,
            vertex_buffer_count: asset.metadata.vertex_buffer_count,
            index_format: asset.metadata.index_format.clone(),
            primitive_topology: asset.metadata.primitive_topology.clone(),
            layout_markers: asset
                .metadata
                .layout_markers
                .iter()
                .map(|marker| normalize_text(marker))
                .filter(|marker| !marker.is_empty())
                .collect(),
            internal_structure: normalize_internal_structure(&asset.metadata.internal_structure),
        }
    }
}

fn file_stem(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
}

pub(crate) fn normalize_text(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>();

    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn tokenize(value: &str) -> BTreeSet<String> {
    normalize_text(value)
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn normalize_internal_structure(structure: &AssetInternalStructure) -> AssetInternalStructure {
    AssetInternalStructure {
        section_labels: normalize_text_list(&structure.section_labels),
        buffer_roles: normalize_text_list(&structure.buffer_roles),
        binding_targets: normalize_text_list(&structure.binding_targets),
        subresource_roles: normalize_text_list(&structure.subresource_roles),
        has_skeleton: structure.has_skeleton,
        has_shapekey_data: structure.has_shapekey_data,
    }
}

fn normalize_text_list(values: &[String]) -> Vec<String> {
    let mut normalized = values
        .iter()
        .map(|value| normalize_text(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized
}

#[cfg(test)]
mod tests {
    use super::{normalize_text, tokenize};

    #[test]
    fn normalize_text_collapses_symbols() {
        assert_eq!(normalize_text("HeroA_Body-v2.mesh"), "heroa body v2 mesh");
    }

    #[test]
    fn tokenize_deduplicates_tokens() {
        let tokens = tokenize("Hero Hero Body");
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains("hero"));
        assert!(tokens.contains("body"));
    }
}
