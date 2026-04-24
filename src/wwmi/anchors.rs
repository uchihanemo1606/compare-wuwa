use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::PreparedAssetInventory,
    error::{AppError, AppResult},
    ingest::frame_analysis::{build_prepared_inventory, parse_frame_analysis_log},
};

const WWMI_ANCHOR_REPORT_SCHEMA: &str = "whashreonator.wwmi-anchor-report.v1";
const WWMI_ANCHOR_KNOWLEDGE_SCHEMA: &str = "whashreonator.wwmi-anchor-knowledge.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    #[serde(default)]
    pub candidate_replacements: Vec<WwmiAnchorCandidateReplacement>,
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
pub struct WwmiAnchorCandidateReplacement {
    pub hash: String,
    pub observed_kind: String,
    pub asset_id: String,
    pub asset_path: String,
    #[serde(default)]
    pub identity_tuple: Option<String>,
    #[serde(default)]
    pub draw_call_count: Option<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub score: u32,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiAnchorReport {
    pub schema_version: String,
    #[serde(default)]
    pub generated_at_unix_ms: u128,
    #[serde(default)]
    pub version_id: Option<String>,
    pub capture_profile: WwmiAnchorCaptureProfile,
    pub source_dump_dir: String,
    pub source_log_path: String,
    pub found_anchors: Vec<WwmiFoundAnchor>,
    pub missing_anchors: Vec<WwmiCanonicalAnchor>,
    pub unexpected_anchor_candidates: Vec<WwmiUnexpectedAnchorCandidate>,
    pub success: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiAnchorHistoricalMatch {
    pub version_id: String,
    pub capture_profile: WwmiAnchorCaptureProfile,
    pub hash: String,
    #[serde(default)]
    pub generated_at_unix_ms: u128,
    #[serde(default)]
    pub source_dump_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiVersionAnchorKnowledgeEntry {
    pub logical_name: String,
    pub expected_kind: String,
    #[serde(default)]
    pub current_exact_matches: Vec<WwmiAnchorHistoricalMatch>,
    #[serde(default)]
    pub historical_exact_matches: Vec<WwmiAnchorHistoricalMatch>,
    #[serde(default)]
    pub current_candidate_replacements: Vec<WwmiAnchorCandidateReplacement>,
    pub current_missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WwmiVersionAnchorKnowledge {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub version_id: String,
    pub report_count: usize,
    #[serde(default)]
    pub capture_profiles: Vec<WwmiAnchorCaptureProfile>,
    pub anchors: Vec<WwmiVersionAnchorKnowledgeEntry>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WwmiVersionedAnchorReport {
    pub version_id: String,
    pub report: WwmiAnchorReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CanonicalAnchorDefinition {
    hash: &'static str,
    logical_name: &'static str,
    expected_kind: &'static str,
    preferred_identity_tuple: Option<&'static str>,
    preferred_tags: &'static [&'static str],
}

const CANONICAL_ANCHORS: [CanonicalAnchorDefinition; 6] = [
    CanonicalAnchorDefinition {
        hash: "8c1ee0581cb4f0ec",
        logical_name: "CharacterMenuBackgroundParticlesVS",
        expected_kind: "vertex_shader",
        preferred_identity_tuple: None,
        preferred_tags: &[],
    },
    CanonicalAnchorDefinition {
        hash: "a24b0fd936f39dcc",
        logical_name: "UIDrawPS",
        expected_kind: "pixel_shader",
        preferred_identity_tuple: None,
        preferred_tags: &[],
    },
    CanonicalAnchorDefinition {
        hash: "2cc576e7",
        logical_name: "OutfitTileSideGradientsImage",
        expected_kind: "texture_resource",
        preferred_identity_tuple: Some("fa|tex|ps:a24b0fd936f39dcc|slot:0"),
        preferred_tags: &["wwmi-anchor-candidate", "ps_slot=0"],
    },
    CanonicalAnchorDefinition {
        hash: "ce6a251a",
        logical_name: "OutfitTileBackgroundImage",
        expected_kind: "texture_resource",
        preferred_identity_tuple: Some("fa|tex|ps:a24b0fd936f39dcc|slot:1"),
        preferred_tags: &["wwmi-anchor-candidate", "ps_slot=1"],
    },
    CanonicalAnchorDefinition {
        hash: "9bf4420c82102011",
        logical_name: "ShapeKeyLoaderCS",
        expected_kind: "compute_shader",
        preferred_identity_tuple: Some("fa|cs|tg:29747x1x1"),
        preferred_tags: &["wwmi-anchor-candidate"],
    },
    CanonicalAnchorDefinition {
        hash: "7a8396180d416117",
        logical_name: "ShapeKeyMultiplierCS",
        expected_kind: "compute_shader",
        preferred_identity_tuple: Some("fa|cs|tg:100x1x1"),
        preferred_tags: &["wwmi-anchor-candidate"],
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
    let canonical_hashes = CANONICAL_ANCHORS
        .iter()
        .map(|anchor| anchor.hash)
        .collect::<BTreeSet<_>>();

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
                candidate_replacements: rank_candidate_replacements(
                    inventory,
                    &canonical_hashes,
                    &anchor,
                ),
            });
        }
    }

    found_anchors.sort_by(|left, right| left.hash.cmp(&right.hash));
    missing_anchors.sort_by(|left, right| left.hash.cmp(&right.hash));

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
    notes.push(
        "logical WWMI anchors stay stable, but runtime hashes must be discovered from fresh Frame Analysis dumps rather than inferred from version strings"
            .to_string(),
    );

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
        generated_at_unix_ms: current_unix_ms().unwrap_or_default(),
        version_id: None,
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

pub fn extract_wwmi_anchor_report_from_dump(
    dump_dir: &Path,
    capture_profile: WwmiAnchorCaptureProfile,
) -> AppResult<WwmiAnchorReport> {
    if !dump_dir.exists() {
        return Err(AppError::InvalidInput(format!(
            "frame analysis dump directory does not exist: {}",
            dump_dir.display()
        )));
    }
    if !dump_dir.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "frame analysis dump path is not a directory: {}",
            dump_dir.display()
        )));
    }

    let dump_dir = dump_dir.canonicalize()?;
    let log_path = dump_dir.join("log.txt");
    if !log_path.exists() || !log_path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "frame analysis dump directory must contain log.txt: {}",
            dump_dir.display()
        )));
    }

    let log_text = fs::read_to_string(&log_path)?;
    let mut dump = parse_frame_analysis_log(&log_text)?;
    dump.dump_dir = dump_dir.clone();
    dump.log_path = log_path.clone();

    let inventory = build_prepared_inventory(&dump, "wwmi-anchor-audit");
    Ok(extract_wwmi_anchor_report(
        &inventory,
        capture_profile,
        dump_dir.as_path(),
        log_path.as_path(),
    ))
}

pub fn build_version_anchor_knowledge(
    version_id: &str,
    reports: &[WwmiVersionedAnchorReport],
) -> WwmiVersionAnchorKnowledge {
    let current_reports = reports
        .iter()
        .filter(|candidate| candidate.version_id == version_id)
        .collect::<Vec<_>>();
    let capture_profiles = current_reports
        .iter()
        .map(|candidate| candidate.report.capture_profile)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let anchors = CANONICAL_ANCHORS
        .iter()
        .map(|anchor| {
            let current_exact_matches =
                collect_exact_matches(&current_reports, anchor.logical_name);
            let historical_exact_matches = collect_exact_matches(
                &reports
                    .iter()
                    .filter(|candidate| candidate.version_id != version_id)
                    .collect::<Vec<_>>(),
                anchor.logical_name,
            );
            let current_candidate_replacements = if current_exact_matches.is_empty() {
                collect_current_candidate_replacements(&current_reports, anchor.logical_name)
            } else {
                Vec::new()
            };

            WwmiVersionAnchorKnowledgeEntry {
                logical_name: anchor.logical_name.to_string(),
                expected_kind: anchor.expected_kind.to_string(),
                current_missing: current_exact_matches.is_empty(),
                current_exact_matches,
                historical_exact_matches,
                current_candidate_replacements,
            }
        })
        .collect::<Vec<_>>();

    let exact_anchor_count = anchors
        .iter()
        .filter(|anchor| !anchor.current_exact_matches.is_empty())
        .count();
    let missing_anchor_count = anchors
        .iter()
        .filter(|anchor| anchor.current_missing)
        .count();

    let mut notes = if current_reports.is_empty() {
        vec![format!(
            "no stored WWMI anchor reports are available yet for version {}",
            version_id
        )]
    } else {
        vec![format!(
            "built WWMI anchor knowledge for {} from {} stored report(s)",
            version_id,
            current_reports.len()
        )]
    };
    notes.push(format!(
        "current exact anchors={} missing anchors={}",
        exact_anchor_count, missing_anchor_count
    ));
    if !capture_profiles.is_empty() {
        notes.push(format!(
            "observed capture profiles for {}: {}",
            version_id,
            capture_profiles
                .iter()
                .map(|profile| profile.label())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    notes.push(
        "anchor knowledge is version-aware and dump-driven; version strings alone must not be treated as hash oracles"
            .to_string(),
    );

    WwmiVersionAnchorKnowledge {
        schema_version: WWMI_ANCHOR_KNOWLEDGE_SCHEMA.to_string(),
        generated_at_unix_ms: current_unix_ms().unwrap_or_default(),
        version_id: version_id.to_string(),
        report_count: current_reports.len(),
        capture_profiles,
        anchors,
        notes,
    }
}

fn rank_candidate_replacements(
    inventory: &PreparedAssetInventory,
    canonical_hashes: &BTreeSet<&str>,
    anchor: &CanonicalAnchorDefinition,
) -> Vec<WwmiAnchorCandidateReplacement> {
    let mut candidates = inventory
        .assets
        .iter()
        .filter_map(|record| {
            let observed_kind = record.asset.kind.as_deref()?;
            if observed_kind != anchor.expected_kind {
                return None;
            }

            let hash = record_hash(record)?;
            if canonical_hashes.contains(hash) {
                return None;
            }

            let mut score = 50_u32;
            let mut reasons = vec![format!("kind matches expected {}", anchor.expected_kind)];
            let tags = record.asset.metadata.tags.clone();
            let identity_tuple = record.hash_fields.identity_tuple.clone();
            let draw_call_count = extract_draw_call_count(&tags);

            if let Some(draw_calls) = draw_call_count {
                score = score.saturating_add(draw_calls.min(25));
                reasons.push(format!("runtime context includes draw_calls={draw_calls}"));
            } else {
                reasons.push("no draw_call_count could be derived from tags".to_string());
            }

            if !tags.is_empty() {
                score = score.saturating_add(5);
                reasons.push("record carries runtime tags from the Frame Analysis adapter".to_string());
            }

            let matched_preferred_tags = anchor
                .preferred_tags
                .iter()
                .filter(|expected| tags.iter().any(|tag| tag == **expected))
                .count() as u32;
            if matched_preferred_tags > 0 {
                score = score.saturating_add(matched_preferred_tags * 10);
                reasons.push(format!(
                    "matched {} anchor-specific runtime tag hint(s)",
                    matched_preferred_tags
                ));
            }

            match (anchor.preferred_identity_tuple, identity_tuple.as_deref()) {
                (Some(expected), Some(observed)) if observed == expected => {
                    score = score.saturating_add(40);
                    reasons.push(format!(
                        "identity_tuple matches the known structural hint {expected}"
                    ));
                }
                (Some(expected), Some(observed)) => {
                    if identity_family(expected) == identity_family(observed) {
                        score = score.saturating_add(15);
                        reasons.push(format!(
                            "identity_tuple shares the same runtime context family as {expected}"
                        ));
                    }
                    if has_same_slot(expected, observed) {
                        score = score.saturating_add(15);
                        reasons.push("identity_tuple preserves the expected texture slot".to_string());
                    }
                    if has_same_thread_group(expected, observed) {
                        score = score.saturating_add(25);
                        reasons.push(
                            "identity_tuple preserves the expected compute thread-group tuple"
                                .to_string(),
                        );
                    }
                }
                (Some(expected), None) => reasons.push(format!(
                    "missing identity_tuple, so the known structural hint {expected} could not be compared"
                )),
                (None, Some(_)) => {
                    score = score.saturating_add(10);
                    reasons.push(
                        "identity_tuple is present, which gives the reviewer extra runtime context"
                            .to_string(),
                    );
                }
                (None, None) => reasons.push(
                    "no identity_tuple is available for this candidate in the current adapter"
                        .to_string(),
                ),
            }

            if tags.iter().any(|tag| tag == "wwmi-anchor-candidate") {
                score = score.saturating_add(10);
                reasons.push(
                    "record was explicitly surfaced as a wwmi-anchor-candidate by the adapter"
                        .to_string(),
                );
            }

            Some(WwmiAnchorCandidateReplacement {
                hash: hash.to_string(),
                observed_kind: observed_kind.to_string(),
                asset_id: record.asset.id.clone(),
                asset_path: record.asset.path.clone(),
                identity_tuple,
                draw_call_count,
                tags,
                score,
                reasons,
            })
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.draw_call_count.cmp(&left.draw_call_count))
            .then_with(|| left.hash.cmp(&right.hash))
    });
    candidates
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

fn extract_draw_call_count(tags: &[String]) -> Option<u32> {
    tags.iter().find_map(|tag| {
        tag.strip_prefix("draw_calls=")
            .and_then(|value| value.parse::<u32>().ok())
    })
}

fn identity_family(value: &str) -> Option<&'static str> {
    if value.starts_with("fa|cs|") {
        Some("fa|cs")
    } else if value.starts_with("fa|tex|") {
        Some("fa|tex")
    } else if value.starts_with("fa|vb|") {
        Some("fa|vb")
    } else {
        None
    }
}

fn has_same_slot(expected: &str, observed: &str) -> bool {
    identity_component(expected, "slot") == identity_component(observed, "slot")
        && identity_component(expected, "slot").is_some()
}

fn has_same_thread_group(expected: &str, observed: &str) -> bool {
    identity_component(expected, "tg") == identity_component(observed, "tg")
        && identity_component(expected, "tg").is_some()
}

fn identity_component<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    value
        .split('|')
        .find_map(|segment| segment.strip_prefix(&format!("{key}:")))
}

fn collect_exact_matches(
    reports: &[&WwmiVersionedAnchorReport],
    logical_name: &str,
) -> Vec<WwmiAnchorHistoricalMatch> {
    let mut seen = BTreeSet::new();
    let mut matches = Vec::new();

    for versioned in reports {
        for anchor in versioned
            .report
            .found_anchors
            .iter()
            .filter(|anchor| anchor.logical_name == logical_name)
        {
            let entry = WwmiAnchorHistoricalMatch {
                version_id: versioned.version_id.clone(),
                capture_profile: versioned.report.capture_profile,
                hash: anchor.hash.clone(),
                generated_at_unix_ms: versioned.report.generated_at_unix_ms,
                source_dump_dir: Some(versioned.report.source_dump_dir.clone()),
            };
            if seen.insert((
                entry.version_id.clone(),
                entry.capture_profile,
                entry.hash.clone(),
                entry.generated_at_unix_ms,
            )) {
                matches.push(entry);
            }
        }
    }

    matches.sort_by(|left, right| {
        left.version_id
            .cmp(&right.version_id)
            .then_with(|| left.capture_profile.cmp(&right.capture_profile))
            .then_with(|| left.hash.cmp(&right.hash))
            .then_with(|| left.generated_at_unix_ms.cmp(&right.generated_at_unix_ms))
    });
    matches
}

fn collect_current_candidate_replacements(
    reports: &[&WwmiVersionedAnchorReport],
    logical_name: &str,
) -> Vec<WwmiAnchorCandidateReplacement> {
    let mut merged = BTreeMap::<(String, String), WwmiAnchorCandidateReplacement>::new();

    for versioned in reports {
        for missing in versioned
            .report
            .missing_anchors
            .iter()
            .filter(|anchor| anchor.logical_name == logical_name)
        {
            for candidate in &missing.candidate_replacements {
                let key = (candidate.hash.clone(), candidate.observed_kind.clone());
                match merged.get_mut(&key) {
                    Some(existing) => merge_candidate_replacement(existing, candidate),
                    None => {
                        merged.insert(key, candidate.clone());
                    }
                }
            }
        }
    }

    let mut replacements = merged.into_values().collect::<Vec<_>>();
    replacements.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.draw_call_count.cmp(&left.draw_call_count))
            .then_with(|| left.hash.cmp(&right.hash))
    });
    replacements.truncate(5);
    replacements
}

fn merge_candidate_replacement(
    existing: &mut WwmiAnchorCandidateReplacement,
    candidate: &WwmiAnchorCandidateReplacement,
) {
    if candidate.score > existing.score {
        existing.asset_id = candidate.asset_id.clone();
        existing.asset_path = candidate.asset_path.clone();
        existing.identity_tuple = candidate.identity_tuple.clone();
        existing.draw_call_count = candidate.draw_call_count;
        existing.score = candidate.score;
    } else if candidate.draw_call_count > existing.draw_call_count {
        existing.draw_call_count = candidate.draw_call_count;
    }

    let mut tags = existing.tags.iter().cloned().collect::<BTreeSet<String>>();
    tags.extend(candidate.tags.iter().cloned());
    existing.tags = tags.into_iter().collect();

    let mut reasons = existing
        .reasons
        .iter()
        .cloned()
        .collect::<BTreeSet<String>>();
    reasons.extend(candidate.reasons.iter().cloned());
    existing.reasons = reasons.into_iter().collect();
}

fn current_unix_ms() -> AppResult<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::InvalidInput(format!("system clock error: {error}")))?
        .as_millis())
}
