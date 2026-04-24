use std::{fs, path::Path};

use serde::Serialize;

use crate::{
    compare::SnapshotCompareReport,
    domain::{PatchDraftOutput, PipelineReport, VersionedMappingOutput},
    error::AppResult,
    inference::InferenceReport,
    output_policy::validate_artifact_output_path,
    proposal::{MappingProposalOutput, ProposalPatchDraftOutput},
    report::{VersionContinuityArtifact, VersionDiffReportV2},
    snapshot::GameSnapshot,
    wwmi::{
        WwmiKnowledgeBase,
        anchors::WwmiAnchorReport,
        dependency::{WwmiModDependencyBaselineSet, WwmiModDependencyProfile},
    },
};

pub trait Exporter {
    fn export(&self, report: &PipelineReport, output: &Path) -> AppResult<()>;
}

#[derive(Debug, Default, Clone)]
pub struct JsonExporter;

impl Exporter for JsonExporter {
    fn export(&self, report: &PipelineReport, output: &Path) -> AppResult<()> {
        write_pretty_json(output, report)?;
        Ok(())
    }
}

pub fn export_mapping_output(report: &PipelineReport, output: &Path) -> AppResult<()> {
    write_pretty_json(output, &VersionedMappingOutput::from(report))
}

pub fn export_patch_draft_output(report: &PipelineReport, output: &Path) -> AppResult<()> {
    write_pretty_json(output, &PatchDraftOutput::from(report))
}

pub fn export_snapshot_output(snapshot: &GameSnapshot, output: &Path) -> AppResult<()> {
    write_pretty_json(output, snapshot)
}

pub fn export_snapshot_compare_output(
    report: &SnapshotCompareReport,
    output: &Path,
) -> AppResult<()> {
    write_pretty_json(output, report)
}

pub fn export_wwmi_knowledge_output(knowledge: &WwmiKnowledgeBase, output: &Path) -> AppResult<()> {
    write_pretty_json(output, knowledge)
}

pub fn export_wwmi_anchor_report_output(report: &WwmiAnchorReport, output: &Path) -> AppResult<()> {
    write_pretty_json(output, report)
}

pub fn export_mod_dependency_profile_output(
    profile: &WwmiModDependencyProfile,
    output: &Path,
) -> AppResult<()> {
    write_pretty_json(output, profile)
}

pub fn export_mod_dependency_baseline_set_output(
    baseline_set: &WwmiModDependencyBaselineSet,
    output: &Path,
) -> AppResult<()> {
    write_pretty_json(output, baseline_set)
}

pub fn export_inference_output(report: &InferenceReport, output: &Path) -> AppResult<()> {
    write_pretty_json(output, report)
}

pub fn export_mapping_proposal_output(
    proposal: &MappingProposalOutput,
    output: &Path,
) -> AppResult<()> {
    write_pretty_json(output, proposal)
}

pub fn export_proposal_patch_draft_output(
    patch_draft: &ProposalPatchDraftOutput,
    output: &Path,
) -> AppResult<()> {
    write_pretty_json(output, patch_draft)
}

pub fn export_version_diff_report_v2(report: &VersionDiffReportV2, output: &Path) -> AppResult<()> {
    write_pretty_json(output, report)
}

pub fn export_version_continuity_output(
    artifact: &VersionContinuityArtifact,
    output: &Path,
) -> AppResult<()> {
    write_pretty_json(output, artifact)
}

pub fn export_text_output(content: &str, output: &Path) -> AppResult<()> {
    validate_artifact_output_path(output)?;
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    fs::write(output, content)?;
    Ok(())
}

fn write_pretty_json<T>(output: &Path, value: &T) -> AppResult<()>
where
    T: Serialize,
{
    validate_artifact_output_path(output)?;
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    fs::write(output, serde_json::to_string_pretty(value)?)?;
    Ok(())
}
