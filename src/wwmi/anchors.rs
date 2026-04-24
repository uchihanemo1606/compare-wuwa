use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::PreparedAssetInventory;

const WWMI_ANCHOR_REPORT_SCHEMA: &str = "whashreonator.wwmi-anchor-report.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WwmiAnchorCaptureProfile {
    MenuUi,
    ShapekeyRuntime,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiCanonicalAnchor {
    pub hash: String,
    pub logical_name: String,
    pub expected_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiFoundAnchor {
    pub hash: String,
    pub logical_name: String,
    pub expected_kind: String,
    pub asset_id: String,
    pub asset_path: String,
    #[serde(default)]
    pub identity_tuple: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiUnexpectedAnchorCandidate {
    pub hash: String,
    pub observed_kind: String,
    pub asset_id: String,
    pub asset_path: String,
    #[serde(default)]
    pub identity_tuple: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiAnchorReport {
    pub schema_version: String,
    pub capture_profile: WwmiAnchorCaptureProfile,
    pub source_dump_dir: String,
    pub source_log_path: String,
    pub found_anchors: Vec<WwmiFoundAnchor>,
    pub missing_anchors: Vec<WwmiCanonicalAnchor>,
    pub unexpected_anchor_candidates: Vec<WwmiUnexpectedAnchorCandidate>,
    pub success: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CanonicalAnchorDefinition {
    hash: &'static str,
    logical_name: &'static str,
    expected_kind: &'static str,
}

const CANONICAL_ANCHORS: [CanonicalAnchorDefinition; 6] = [
    CanonicalAnchorDefinition {
        hash: "8c1ee0581cb4f0ec",
        logical_name: "CharacterMenuBackgroundParticlesVS",
        expected_kind: "vertex_shader",
    },
    CanonicalAnchorDefinition {
        hash: "a24b0fd936f39dcc",
        logical_name: "UIDrawPS",
        expected_kind: "pixel_shader",
    },
    CanonicalAnchorDefinition {
        hash: "2cc576e7",
        logical_name: "OutfitTileSideGradientsImage",
        expected_kind: "texture_resource",
    },
    CanonicalAnchorDefinition {
        hash: "ce6a251a",
        logical_name: "OutfitTileBackgroundImage",
        expected_kind: "texture_resource",
    },
    CanonicalAnchorDefinition {
        hash: "9bf4420c82102011",
        logical_name: "ShapeKeyLoaderCS",
        expected_kind: "compute_shader",
    },
    CanonicalAnchorDefinition {
        hash: "7a8396180d416117",
        logical_name: "ShapeKeyMultiplierCS",
        expected_kind: "compute_shader",
    },
];

impl WwmiAnchorCaptureProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::MenuUi => "menu_ui",
            Self::ShapekeyRuntime => "shapekey_runtime",
            Self::Full => "full",
        }
    }

    fn expects(self, anchor: &CanonicalAnchorDefinition) -> bool {
        match self {
            Self::MenuUi => matches!(
                anchor.hash,
                "8c1ee0581cb4f0ec" | "a24b0fd936f39dcc" | "2cc576e7" | "ce6a251a"
            ),
            Self::ShapekeyRuntime => {
                matches!(anchor.hash, "9bf4420c82102011" | "7a8396180d416117")
            }
            Self::Full => true,
        }
    }
}

pub fn extract_wwmi_anchor_report(
    inventory: &PreparedAssetInventory,
    capture_profile: WwmiAnchorCaptureProfile,
    source_dump_dir: &Path,
    source_log_path: &Path,
) -> WwmiAnchorReport {
    let mut found_anchors = Vec::new();
    let mut missing_anchors = Vec::new();

    for anchor in CANONICAL_ANCHORS {
        let found = inventory.assets.iter().find(|record| {
            record.asset.kind.as_deref() == Some(anchor.expected_kind)
                && record_hash(record).as_deref() == Some(anchor.hash)
        });

        if let Some(record) = found {
            found_anchors.push(WwmiFoundAnchor {
                hash: anchor.hash.to_string(),
                logical_name: anchor.logical_name.to_string(),
                expected_kind: anchor.expected_kind.to_string(),
                asset_id: record.asset.id.clone(),
                asset_path: record.asset.path.clone(),
                identity_tuple: record.hash_fields.identity_tuple.clone(),
            });
        } else if capture_profile.expects(&anchor) {
            missing_anchors.push(WwmiCanonicalAnchor {
                hash: anchor.hash.to_string(),
                logical_name: anchor.logical_name.to_string(),
                expected_kind: anchor.expected_kind.to_string(),
            });
        }
    }

    found_anchors.sort_by(|left, right| left.hash.cmp(&right.hash));
    missing_anchors.sort_by(|left, right| left.hash.cmp(&right.hash));

    let canonical_hashes = CANONICAL_ANCHORS
        .iter()
        .map(|anchor| anchor.hash)
        .collect::<std::collections::BTreeSet<_>>();
    let unexpected_anchor_candidates = inventory
        .assets
        .iter()
        .filter_map(|record| {
            let kind = record.asset.kind.as_deref()?;
            if !matches!(
                kind,
                "vertex_shader" | "pixel_shader" | "texture_resource" | "compute_shader"
            ) {
                return None;
            }
            let hash = record_hash(record)?;
            if canonical_hashes.contains(hash) {
                return None;
            }
            Some(WwmiUnexpectedAnchorCandidate {
                hash: hash.to_string(),
                observed_kind: kind.to_string(),
                asset_id: record.asset.id.clone(),
                asset_path: record.asset.path.clone(),
                identity_tuple: record.hash_fields.identity_tuple.clone(),
            })
        })
        .collect::<Vec<_>>();

    let expected_anchor_count = CANONICAL_ANCHORS
        .iter()
        .filter(|anchor| capture_profile.expects(anchor))
        .count();
    let found_expected_count = found_anchors
        .iter()
        .filter(|anchor| {
            CANONICAL_ANCHORS.iter().any(|definition| {
                definition.hash == anchor.hash && capture_profile.expects(definition)
            })
        })
        .count();

    let mut notes = vec![format!(
        "capture profile {} expects {} canonical anchor(s); found {}, missing {}",
        capture_profile.label(),
        expected_anchor_count,
        found_expected_count,
        missing_anchors.len()
    )];

    let found_outside_profile = found_anchors
        .iter()
        .filter(|anchor| {
            CANONICAL_ANCHORS.iter().any(|definition| {
                definition.hash == anchor.hash && !capture_profile.expects(definition)
            })
        })
        .count();
    if found_outside_profile > 0 {
        notes.push(format!(
            "found {} canonical anchor(s) outside the requested capture profile",
            found_outside_profile
        ));
    }
    if !unexpected_anchor_candidates.is_empty() {
        notes.push(format!(
            "observed {} non-canonical shader/texture candidate(s) in the dump",
            unexpected_anchor_candidates.len()
        ));
    }

    WwmiAnchorReport {
        schema_version: WWMI_ANCHOR_REPORT_SCHEMA.to_string(),
        capture_profile,
        source_dump_dir: normalize_path(source_dump_dir),
        source_log_path: normalize_path(source_log_path),
        found_anchors,
        missing_anchors,
        unexpected_anchor_candidates,
        success: found_expected_count == expected_anchor_count,
        notes,
    }
}

fn normalize_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized
        .strip_prefix("//?/")
        .unwrap_or(normalized.as_str())
        .to_string()
}

fn record_hash(record: &crate::domain::ExtractedAssetRecord) -> Option<&str> {
    record
        .hash_fields
        .asset_hash
        .as_deref()
        .or(record.hash_fields.shader_hash.as_deref())
}
