use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct AssetBundle {
    pub old_assets: Vec<AssetRecord>,
    pub new_assets: Vec<AssetRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRecord {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub metadata: AssetMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct AssetMetadata {
    pub logical_name: Option<String>,
    pub vertex_count: Option<u32>,
    pub index_count: Option<u32>,
    pub material_slots: Option<u32>,
    pub section_count: Option<u32>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetSummary {
    pub id: String,
    pub path: String,
    pub kind: Option<String>,
}

impl From<&AssetRecord> for AssetSummary {
    fn from(value: &AssetRecord) -> Self {
        Self {
            id: value.id.clone(),
            path: value.path.clone(),
            kind: value.kind.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchReason {
    pub code: String,
    pub message: String,
    pub contribution: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Matched,
    NeedsReview,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchDecision {
    pub old_asset: AssetSummary,
    pub new_asset: Option<AssetSummary>,
    pub confidence: f32,
    pub status: MatchStatus,
    pub reasons: Vec<MatchReason>,
    pub top_candidate_gap: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineSummary {
    pub total_old_assets: usize,
    pub matched: usize,
    pub needs_review: usize,
    pub rejected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineReport {
    pub assumptions: Vec<String>,
    pub summary: PipelineSummary,
    pub decisions: Vec<MatchDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionedMappingOutput {
    pub schema_version: String,
    pub summary: PipelineSummary,
    pub mappings: Vec<MappingEntry>,
}

impl From<&PipelineReport> for VersionedMappingOutput {
    fn from(value: &PipelineReport) -> Self {
        Self {
            schema_version: "whashreonator.mapping.v1".to_string(),
            summary: value.summary.clone(),
            mappings: value.decisions.iter().map(MappingEntry::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MappingEntry {
    pub old_asset: AssetSummary,
    pub new_asset: Option<AssetSummary>,
    pub confidence: f32,
    pub status: MatchStatus,
    pub reasons: Vec<MatchReason>,
    pub top_candidate_gap: Option<f32>,
}

impl From<&MatchDecision> for MappingEntry {
    fn from(value: &MatchDecision) -> Self {
        Self {
            old_asset: value.old_asset.clone(),
            new_asset: value.new_asset.clone(),
            confidence: value.confidence,
            status: value.status.clone(),
            reasons: value.reasons.clone(),
            top_candidate_gap: value.top_candidate_gap,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatchDraftOutput {
    pub schema_version: String,
    pub mode: String,
    pub summary: PipelineSummary,
    pub actions: Vec<PatchDraftAction>,
}

impl From<&PipelineReport> for PatchDraftOutput {
    fn from(value: &PipelineReport) -> Self {
        Self {
            schema_version: "whashreonator.patch-draft.v1".to_string(),
            mode: "draft".to_string(),
            summary: value.summary.clone(),
            actions: value.decisions.iter().map(PatchDraftAction::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatchDraftAction {
    pub action: String,
    pub old_asset: AssetSummary,
    pub new_asset: Option<AssetSummary>,
    pub confidence: f32,
    pub status: MatchStatus,
    pub notes: Vec<String>,
}

impl From<&MatchDecision> for PatchDraftAction {
    fn from(value: &MatchDecision) -> Self {
        let action = match value.status {
            MatchStatus::Matched if value.new_asset.is_some() => "propose_mapping",
            MatchStatus::NeedsReview => "review_mapping",
            MatchStatus::Rejected => "skip",
            MatchStatus::Matched => "skip",
        };

        Self {
            action: action.to_string(),
            old_asset: value.old_asset.clone(),
            new_asset: value.new_asset.clone(),
            confidence: value.confidence,
            status: value.status.clone(),
            notes: value
                .reasons
                .iter()
                .map(|reason| reason.message.clone())
                .collect(),
        }
    }
}
