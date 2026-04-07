use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    snapshot::{GameSnapshot, SnapshotAsset, load_snapshot},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotCompareReport {
    pub schema_version: String,
    pub old_snapshot: SnapshotVersionInfo,
    pub new_snapshot: SnapshotVersionInfo,
    pub summary: SnapshotCompareSummary,
    pub added_assets: Vec<SnapshotAssetChange>,
    pub removed_assets: Vec<SnapshotAssetChange>,
    pub changed_assets: Vec<SnapshotAssetChange>,
    pub candidate_mapping_changes: Vec<CandidateMappingChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotVersionInfo {
    pub version_id: String,
    pub source_root: String,
    pub asset_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotCompareSummary {
    pub total_old_assets: usize,
    pub total_new_assets: usize,
    pub unchanged_assets: usize,
    pub added_assets: usize,
    pub removed_assets: usize,
    pub changed_assets: usize,
    pub candidate_mapping_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotChangeType {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotAssetChange {
    pub change_type: SnapshotChangeType,
    pub old_asset: Option<SnapshotAssetSummary>,
    pub new_asset: Option<SnapshotAssetSummary>,
    pub changed_fields: Vec<String>,
    pub probable_impact: RiskLevel,
    pub crash_risk: RiskLevel,
    pub suspected_mapping_change: bool,
    pub reasons: Vec<SnapshotCompareReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotAssetSummary {
    pub id: String,
    pub path: String,
    pub kind: Option<String>,
    pub normalized_name: Option<String>,
    pub vertex_count: Option<u32>,
    pub index_count: Option<u32>,
    pub material_slots: Option<u32>,
    pub section_count: Option<u32>,
    pub asset_hash: Option<String>,
    pub shader_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotCompareReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateMappingChange {
    pub old_asset: SnapshotAssetSummary,
    pub new_asset: SnapshotAssetSummary,
    pub confidence: f32,
    pub reasons: Vec<SnapshotCompareReason>,
    #[serde(default)]
    pub runner_up_confidence: Option<f32>,
    #[serde(default)]
    pub confidence_gap: Option<f32>,
    #[serde(default)]
    pub ambiguous: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SnapshotComparer;

impl SnapshotComparer {
    pub fn compare_files(
        &self,
        old_snapshot_path: &std::path::Path,
        new_snapshot_path: &std::path::Path,
    ) -> AppResult<SnapshotCompareReport> {
        let old_snapshot = load_snapshot(old_snapshot_path)?;
        let new_snapshot = load_snapshot(new_snapshot_path)?;
        Ok(self.compare(&old_snapshot, &new_snapshot))
    }

    pub fn compare(
        &self,
        old_snapshot: &GameSnapshot,
        new_snapshot: &GameSnapshot,
    ) -> SnapshotCompareReport {
        let old_by_path = old_snapshot
            .assets
            .iter()
            .map(|asset| (asset.path.as_str(), asset))
            .collect::<BTreeMap<_, _>>();
        let new_by_path = new_snapshot
            .assets
            .iter()
            .map(|asset| (asset.path.as_str(), asset))
            .collect::<BTreeMap<_, _>>();

        let mut added_assets = Vec::new();
        let mut removed_assets = Vec::new();
        let mut changed_assets = Vec::new();
        let mut unchanged_assets = 0usize;

        for (path, old_asset) in &old_by_path {
            match new_by_path.get(path) {
                Some(new_asset) => {
                    let changed_fields = changed_fields(old_asset, new_asset);
                    if changed_fields.is_empty() {
                        unchanged_assets += 1;
                    } else {
                        changed_assets.push(build_changed_asset_entry(
                            old_asset,
                            new_asset,
                            changed_fields,
                        ));
                    }
                }
                None => {
                    removed_assets.push(build_removed_asset_entry(old_asset));
                }
            }
        }

        for (path, new_asset) in &new_by_path {
            if !old_by_path.contains_key(path) {
                added_assets.push(build_added_asset_entry(new_asset));
            }
        }

        let candidate_mapping_changes =
            build_candidate_mapping_changes(&removed_assets, &added_assets);

        SnapshotCompareReport {
            schema_version: "whashreonator.snapshot-compare.v1".to_string(),
            old_snapshot: SnapshotVersionInfo {
                version_id: old_snapshot.version_id.clone(),
                source_root: old_snapshot.source_root.clone(),
                asset_count: old_snapshot.asset_count,
            },
            new_snapshot: SnapshotVersionInfo {
                version_id: new_snapshot.version_id.clone(),
                source_root: new_snapshot.source_root.clone(),
                asset_count: new_snapshot.asset_count,
            },
            summary: SnapshotCompareSummary {
                total_old_assets: old_snapshot.asset_count,
                total_new_assets: new_snapshot.asset_count,
                unchanged_assets,
                added_assets: added_assets.len(),
                removed_assets: removed_assets.len(),
                changed_assets: changed_assets.len(),
                candidate_mapping_changes: candidate_mapping_changes.len(),
            },
            added_assets,
            removed_assets,
            changed_assets,
            candidate_mapping_changes,
        }
    }
}

pub fn load_snapshot_compare_report(path: &Path) -> AppResult<SnapshotCompareReport> {
    let report: SnapshotCompareReport = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(report)
}

impl From<&SnapshotAsset> for SnapshotAssetSummary {
    fn from(value: &SnapshotAsset) -> Self {
        Self {
            id: value.id.clone(),
            path: value.path.clone(),
            kind: value.kind.clone(),
            normalized_name: value.fingerprint.normalized_name.clone(),
            vertex_count: value.fingerprint.vertex_count,
            index_count: value.fingerprint.index_count,
            material_slots: value.fingerprint.material_slots,
            section_count: value.fingerprint.section_count,
            asset_hash: value.hash_fields.asset_hash.clone(),
            shader_hash: value.hash_fields.shader_hash.clone(),
        }
    }
}

fn changed_fields(old_asset: &SnapshotAsset, new_asset: &SnapshotAsset) -> Vec<String> {
    let mut fields = Vec::new();

    push_changed_field(&mut fields, "kind", &old_asset.kind, &new_asset.kind);
    push_changed_field(
        &mut fields,
        "normalized_name",
        &old_asset.fingerprint.normalized_name,
        &new_asset.fingerprint.normalized_name,
    );
    push_changed_field(
        &mut fields,
        "logical_name",
        &old_asset.metadata.logical_name,
        &new_asset.metadata.logical_name,
    );
    push_changed_field(
        &mut fields,
        "vertex_count",
        &old_asset.metadata.vertex_count,
        &new_asset.metadata.vertex_count,
    );
    push_changed_field(
        &mut fields,
        "index_count",
        &old_asset.metadata.index_count,
        &new_asset.metadata.index_count,
    );
    push_changed_field(
        &mut fields,
        "material_slots",
        &old_asset.metadata.material_slots,
        &new_asset.metadata.material_slots,
    );
    push_changed_field(
        &mut fields,
        "section_count",
        &old_asset.metadata.section_count,
        &new_asset.metadata.section_count,
    );
    if old_asset.metadata.tags != new_asset.metadata.tags {
        fields.push("tags".to_string());
    }
    push_changed_field(
        &mut fields,
        "asset_hash",
        &old_asset.hash_fields.asset_hash,
        &new_asset.hash_fields.asset_hash,
    );
    push_changed_field(
        &mut fields,
        "shader_hash",
        &old_asset.hash_fields.shader_hash,
        &new_asset.hash_fields.shader_hash,
    );

    fields
}

fn push_changed_field<T>(fields: &mut Vec<String>, label: &str, old_value: &T, new_value: &T)
where
    T: PartialEq,
{
    if old_value != new_value {
        fields.push(label.to_string());
    }
}

fn build_changed_asset_entry(
    old_asset: &SnapshotAsset,
    new_asset: &SnapshotAsset,
    changed_fields: Vec<String>,
) -> SnapshotAssetChange {
    let structural_change = changed_fields.iter().any(|field| {
        matches!(
            field.as_str(),
            "vertex_count"
                | "index_count"
                | "material_slots"
                | "section_count"
                | "asset_hash"
                | "shader_hash"
        )
    });
    let probable_impact = if structural_change {
        RiskLevel::High
    } else {
        RiskLevel::Medium
    };
    let crash_risk = if structural_change {
        RiskLevel::High
    } else {
        RiskLevel::Low
    };
    let reasons = changed_fields
        .iter()
        .map(|field| SnapshotCompareReason {
            code: format!("{field}_changed"),
            message: format!(
                "{field} changed for asset path {}; mod mappings that assume the previous layout may need review",
                old_asset.path
            ),
        })
        .collect();

    SnapshotAssetChange {
        change_type: SnapshotChangeType::Changed,
        old_asset: Some(SnapshotAssetSummary::from(old_asset)),
        new_asset: Some(SnapshotAssetSummary::from(new_asset)),
        changed_fields,
        probable_impact,
        crash_risk,
        suspected_mapping_change: structural_change,
        reasons,
    }
}

fn build_removed_asset_entry(old_asset: &SnapshotAsset) -> SnapshotAssetChange {
    SnapshotAssetChange {
        change_type: SnapshotChangeType::Removed,
        old_asset: Some(SnapshotAssetSummary::from(old_asset)),
        new_asset: None,
        changed_fields: vec!["path_presence".to_string()],
        probable_impact: RiskLevel::Medium,
        crash_risk: RiskLevel::Medium,
        suspected_mapping_change: true,
        reasons: vec![SnapshotCompareReason {
            code: "asset_removed".to_string(),
            message: format!(
                "asset path {} is missing in the new snapshot; mods targeting this path may fail until remapped",
                old_asset.path
            ),
        }],
    }
}

fn build_added_asset_entry(new_asset: &SnapshotAsset) -> SnapshotAssetChange {
    SnapshotAssetChange {
        change_type: SnapshotChangeType::Added,
        old_asset: None,
        new_asset: Some(SnapshotAssetSummary::from(new_asset)),
        changed_fields: vec!["path_presence".to_string()],
        probable_impact: RiskLevel::Low,
        crash_risk: RiskLevel::Low,
        suspected_mapping_change: false,
        reasons: vec![SnapshotCompareReason {
            code: "asset_added".to_string(),
            message: format!(
                "asset path {} is new in the current snapshot",
                new_asset.path
            ),
        }],
    }
}

fn build_candidate_mapping_changes(
    removed_assets: &[SnapshotAssetChange],
    added_assets: &[SnapshotAssetChange],
) -> Vec<CandidateMappingChange> {
    const MIN_REPORTABLE_CONFIDENCE: f32 = 0.65;
    const MIN_AMBIGUITY_CONFIDENCE: f32 = 0.55;
    const AMBIGUITY_GAP_THRESHOLD: f32 = 0.08;

    removed_assets
        .iter()
        .filter_map(|removed| {
            let old_asset = removed.old_asset.as_ref()?;
            let mut scored_candidates = Vec::new();

            for added in added_assets {
                let new_asset = match added.new_asset.as_ref() {
                    Some(asset) => asset,
                    None => continue,
                };
                let (confidence, reasons) = score_candidate_mapping_change(old_asset, new_asset);
                if confidence < MIN_AMBIGUITY_CONFIDENCE {
                    continue;
                }

                scored_candidates.push(CandidateMappingChange {
                    old_asset: old_asset.clone(),
                    new_asset: new_asset.clone(),
                    confidence,
                    reasons,
                    runner_up_confidence: None,
                    confidence_gap: None,
                    ambiguous: false,
                });
            }

            scored_candidates.sort_by(|left, right| {
                right
                    .confidence
                    .total_cmp(&left.confidence)
                    .then_with(|| left.new_asset.path.cmp(&right.new_asset.path))
            });

            if scored_candidates.is_empty() {
                return None;
            }

            let mut scored_candidates = scored_candidates;
            let mut best_candidate = scored_candidates.remove(0);
            if best_candidate.confidence < MIN_REPORTABLE_CONFIDENCE {
                return None;
            }

            let runner_up_confidence = scored_candidates.first().map(|candidate| candidate.confidence);

            if let Some(runner_up_confidence) = runner_up_confidence {
                let confidence_gap = (best_candidate.confidence - runner_up_confidence).max(0.0);
                best_candidate.runner_up_confidence = Some(runner_up_confidence);
                best_candidate.confidence_gap = Some(confidence_gap);
                if confidence_gap < AMBIGUITY_GAP_THRESHOLD {
                    best_candidate.ambiguous = true;
                    best_candidate.reasons.push(SnapshotCompareReason {
                        code: "ambiguous_runner_up".to_string(),
                        message: format!(
                            "runner-up candidate confidence {:.3} is within {:.3} of the best candidate; keep this remap under review",
                            runner_up_confidence, confidence_gap
                        ),
                    });
                }
            }

            Some(best_candidate)
        })
        .collect()
}

fn score_candidate_mapping_change(
    old_asset: &SnapshotAssetSummary,
    new_asset: &SnapshotAssetSummary,
) -> (f32, Vec<SnapshotCompareReason>) {
    let mut confidence = 0.0;
    let mut reasons = Vec::new();

    if old_asset.kind.is_some() && old_asset.kind == new_asset.kind {
        confidence += 0.20;
        reasons.push(SnapshotCompareReason {
            code: "kind_exact".to_string(),
            message: format!(
                "asset kind matched exactly: {}",
                old_asset.kind.as_deref().unwrap_or("unknown")
            ),
        });
    }

    if old_asset.normalized_name.is_some() && old_asset.normalized_name == new_asset.normalized_name
    {
        confidence += 0.45;
        reasons.push(SnapshotCompareReason {
            code: "normalized_name_exact".to_string(),
            message: format!(
                "normalized name matched exactly: {}",
                old_asset.normalized_name.as_deref().unwrap_or("unknown")
            ),
        });
    } else {
        let name_overlap = token_overlap(
            old_asset.normalized_name.as_deref().unwrap_or_default(),
            new_asset.normalized_name.as_deref().unwrap_or_default(),
        );
        if name_overlap > 0.0 {
            let contribution = name_overlap * 0.20;
            confidence += contribution;
            reasons.push(SnapshotCompareReason {
                code: "normalized_name_token_overlap".to_string(),
                message: format!("normalized name token overlap score: {name_overlap:.3}"),
            });
        }
    }

    if parent_directory(&old_asset.path) == parent_directory(&new_asset.path) {
        confidence += 0.30;
        reasons.push(SnapshotCompareReason {
            code: "same_parent_directory".to_string(),
            message: format!(
                "asset stayed under the same parent directory: {}",
                parent_directory(&old_asset.path).unwrap_or_default()
            ),
        });
    }

    let path_overlap = token_overlap(&old_asset.path, &new_asset.path);
    if path_overlap > 0.0 {
        let contribution = path_overlap * 0.15;
        confidence += contribution;
        reasons.push(SnapshotCompareReason {
            code: "path_token_overlap".to_string(),
            message: format!("path token overlap score: {path_overlap:.3}"),
        });
    }

    if old_asset.vertex_count.is_some() && old_asset.vertex_count == new_asset.vertex_count {
        confidence += 0.10;
        reasons.push(SnapshotCompareReason {
            code: "vertex_count_exact".to_string(),
            message: "vertex count matched exactly".to_string(),
        });
    }

    if old_asset.index_count.is_some() && old_asset.index_count == new_asset.index_count {
        confidence += 0.05;
        reasons.push(SnapshotCompareReason {
            code: "index_count_exact".to_string(),
            message: "index count matched exactly".to_string(),
        });
    }

    if old_asset.material_slots.is_some() && old_asset.material_slots == new_asset.material_slots {
        confidence += 0.05;
        reasons.push(SnapshotCompareReason {
            code: "material_slots_exact".to_string(),
            message: "material slot count matched exactly".to_string(),
        });
    }

    (confidence.clamp(0.0, 1.0), reasons)
}

fn token_overlap(left: &str, right: &str) -> f32 {
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }

    let intersection = left_tokens.intersection(&right_tokens).count() as f32;
    let union = left_tokens.union(&right_tokens).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn tokenize(value: &str) -> BTreeSet<String> {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn parent_directory(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
    };

    use super::{RiskLevel, SnapshotComparer};

    #[test]
    fn compare_report_detects_added_removed_changed_and_candidate_mapping() {
        let comparer = SnapshotComparer;
        let old_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.4.0".to_string(),
            created_at_unix_ms: 1,
            source_root: "old".to_string(),
            asset_count: 2,
            assets: vec![
                asset(
                    "Content/Character/HeroA/Body.mesh",
                    Some("heroa body"),
                    Some(12000),
                ),
                asset(
                    "Content/Weapon/Sword.weapon",
                    Some("sword main"),
                    Some(1000),
                ),
            ],
            context: SnapshotContext::default(),
        };
        let mut changed_body = asset(
            "Content/Character/HeroA/Body.mesh",
            Some("heroa body"),
            Some(18000),
        );
        changed_body.metadata.section_count = Some(2);
        changed_body.fingerprint.section_count = Some(2);
        let new_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.5.0".to_string(),
            created_at_unix_ms: 2,
            source_root: "new".to_string(),
            asset_count: 2,
            assets: vec![
                changed_body,
                asset(
                    "Content/Weapon/Sword_v2.weapon",
                    Some("sword main"),
                    Some(1000),
                ),
            ],
            context: SnapshotContext::default(),
        };

        let report = comparer.compare(&old_snapshot, &new_snapshot);

        assert_eq!(report.summary.changed_assets, 1);
        assert_eq!(report.summary.removed_assets, 1);
        assert_eq!(report.summary.added_assets, 1);
        assert_eq!(report.summary.candidate_mapping_changes, 1);
        assert_eq!(report.changed_assets[0].probable_impact, RiskLevel::High);
        assert!(
            report.changed_assets[0]
                .changed_fields
                .iter()
                .any(|field| field == "vertex_count")
        );
        assert!(report.candidate_mapping_changes[0].confidence >= 0.65);
        assert!(!report.candidate_mapping_changes[0].ambiguous);
    }

    #[test]
    fn compare_report_marks_candidate_as_ambiguous_when_runner_up_is_too_close() {
        let comparer = SnapshotComparer;
        let old_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.4.0".to_string(),
            created_at_unix_ms: 1,
            source_root: "old".to_string(),
            asset_count: 1,
            assets: vec![asset(
                "Content/Weapon/Pistol_Main.weapon",
                Some("pistol main"),
                Some(1500),
            )],
            context: SnapshotContext::default(),
        };
        let new_snapshot = GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: "2.5.0".to_string(),
            created_at_unix_ms: 2,
            source_root: "new".to_string(),
            asset_count: 2,
            assets: vec![
                asset(
                    "Content/Weapon/Pistol_Main_A.weapon",
                    Some("pistol main"),
                    Some(1500),
                ),
                asset(
                    "Content/Weapon/Pistol_Main_B.weapon",
                    Some("pistol main"),
                    Some(1500),
                ),
            ],
            context: SnapshotContext::default(),
        };

        let report = comparer.compare(&old_snapshot, &new_snapshot);

        assert_eq!(report.summary.candidate_mapping_changes, 1);
        assert!(report.candidate_mapping_changes[0].ambiguous);
        assert!(
            report.candidate_mapping_changes[0]
                .confidence_gap
                .is_some_and(|gap| gap < 0.08)
        );
    }

    fn asset(
        path: &str,
        normalized_name: Option<&str>,
        vertex_count: Option<u32>,
    ) -> SnapshotAsset {
        SnapshotAsset {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            metadata: crate::domain::AssetMetadata {
                logical_name: normalized_name.map(ToOwned::to_owned),
                vertex_count,
                index_count: Some(2000),
                material_slots: Some(1),
                section_count: Some(1),
                tags: vec!["weapon".to_string()],
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: normalized_name.map(ToOwned::to_owned),
                name_tokens: normalized_name
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
                path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
                tags: vec!["weapon".to_string()],
                vertex_count,
                index_count: Some(2000),
                material_slots: Some(1),
                section_count: Some(1),
            },
            hash_fields: SnapshotHashFields::default(),
        }
    }
}
