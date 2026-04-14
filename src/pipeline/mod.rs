use std::path::{Path, PathBuf};

use tracing::info;

use crate::wwmi::dependency::{
    WwmiModDependencyBaselineSet, WwmiModDependencyProfile, build_mod_dependency_baseline_set,
    load_mod_dependency_baseline_set, load_mod_dependency_profile, scan_mod_dependency_profile,
};
use crate::{
    cli::{
        CompareSnapshotsArgs, ExtractWwmiKnowledgeArgs, GenerateProposalsArgs, InferFixesArgs,
        MapArgs, MapLocalArgs, ScanModDependenciesArgs, SnapshotArgs, SnapshotCaptureScopeArg,
        SnapshotReportArgs,
    },
    compare::{SnapshotCompareReport, SnapshotComparer, load_snapshot_compare_report},
    config::AppConfig,
    domain::{MatchDecision, MatchStatus, PipelineReport, PipelineSummary},
    error::{AppError, AppResult},
    export::{
        Exporter, JsonExporter, export_inference_output, export_mapping_output,
        export_mapping_proposal_output, export_mod_dependency_baseline_set_output,
        export_patch_draft_output, export_proposal_patch_draft_output,
        export_snapshot_compare_output, export_snapshot_output, export_text_output,
        export_wwmi_knowledge_output,
    },
    fingerprint::{DefaultFingerprinter, Fingerprinter},
    human_summary::HumanSummaryRenderer,
    inference::{FixInferenceEngine, InferenceReport, load_inference_report},
    ingest::{
        AssetSourceSpec, IngestSource, JsonFileIngestSource, LocalSnapshotCaptureScope,
        load_bundle_from_sources,
    },
    matcher::{HeuristicMatcher, Matcher},
    proposal::{ProposalArtifacts, ProposalEngine},
    report::{
        VersionContinuityIndex, VersionDiffReportBuilder, VersionDiffReportV2,
        load_version_continuity_artifact,
    },
    report_storage::ReportStorage,
    snapshot::{
        GameSnapshot, create_extractor_backed_snapshot_from_file,
        create_local_snapshot_with_capture_scope,
    },
    snapshot_report::{SnapshotInventoryReport, SnapshotReportRenderer, load_snapshots},
    validator::{ThresholdValidator, Validator},
    wwmi::{WwmiKnowledgeBase, WwmiKnowledgeExtractor, WwmiRepoInput, load_wwmi_knowledge},
};

pub struct MatchPipeline<I, F, M, V, E> {
    ingest: I,
    fingerprinter: F,
    matcher: M,
    validator: V,
    exporter: E,
}

#[derive(Debug, Clone)]
pub struct SourceMapOutputs {
    pub report_output: Option<PathBuf>,
    pub mapping_output: Option<PathBuf>,
    pub patch_draft_output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ProposalOutputs {
    pub mapping_output: Option<PathBuf>,
    pub patch_draft_output: Option<PathBuf>,
    pub summary_output: Option<PathBuf>,
}

impl ProposalOutputs {
    pub fn is_empty(&self) -> bool {
        self.mapping_output.is_none()
            && self.patch_draft_output.is_none()
            && self.summary_output.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct ProposalResult {
    pub artifacts: ProposalArtifacts,
    pub outputs: ProposalOutputs,
}

#[derive(Debug, Clone)]
pub struct SnapshotCommandResult {
    pub snapshot: GameSnapshot,
    pub stored_snapshot_path: Option<PathBuf>,
    pub stored_extractor_inventory_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ScanModDependenciesResult {
    pub baseline_set: WwmiModDependencyBaselineSet,
    pub stored_baseline_set_path: Option<PathBuf>,
    pub stored_profile_paths: Vec<PathBuf>,
}

impl SourceMapOutputs {
    pub fn is_empty(&self) -> bool {
        self.report_output.is_none()
            && self.mapping_output.is_none()
            && self.patch_draft_output.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct SourceMapResult {
    pub report: PipelineReport,
    pub outputs: SourceMapOutputs,
}

#[derive(Debug, Clone)]
pub enum SourceMapOutcome {
    DryRun(SourceMapResult),
    Exported(SourceMapResult),
}

impl<I, F, M, V, E> MatchPipeline<I, F, M, V, E>
where
    I: IngestSource,
    F: Fingerprinter,
    M: Matcher,
    V: Validator,
    E: Exporter,
{
    pub fn new(ingest: I, fingerprinter: F, matcher: M, validator: V, exporter: E) -> Self {
        Self {
            ingest,
            fingerprinter,
            matcher,
            validator,
            exporter,
        }
    }

    pub fn execute(&self, input: &Path, output: &Path) -> AppResult<PipelineReport> {
        info!(input = %input.display(), "loading assumed JSON bundle");
        let bundle = self.ingest.load_bundle(input)?;
        self.execute_bundle(bundle, output)
    }

    pub fn inspect_bundle(&self, bundle: crate::domain::AssetBundle) -> PipelineReport {
        let old_fingerprints = bundle
            .old_assets
            .iter()
            .map(|asset| self.fingerprinter.fingerprint(asset))
            .collect::<Vec<_>>();
        let new_fingerprints = bundle
            .new_assets
            .iter()
            .map(|asset| self.fingerprinter.fingerprint(asset))
            .collect::<Vec<_>>();

        info!(
            old_assets = old_fingerprints.len(),
            new_assets = new_fingerprints.len(),
            "fingerprints extracted"
        );

        let decisions = self
            .matcher
            .best_matches(&old_fingerprints, &new_fingerprints)
            .into_iter()
            .map(|scored_match| self.validator.validate(scored_match))
            .collect::<Vec<_>>();

        let report = PipelineReport {
            assumptions: assumptions(),
            summary: summarize(&decisions),
            decisions,
        };

        report
    }

    pub fn execute_bundle(
        &self,
        bundle: crate::domain::AssetBundle,
        output: &Path,
    ) -> AppResult<PipelineReport> {
        let report = self.inspect_bundle(bundle);
        self.exporter.export(&report, output)?;
        info!(output = %output.display(), "report exported");

        Ok(report)
    }
}

pub fn run_map_command(args: &MapArgs) -> AppResult<()> {
    let config = AppConfig::load(args.config.as_deref())?;
    run_map(args.input.as_path(), args.output.as_path(), &config)?;
    Ok(())
}

pub fn run_snapshot_command(args: &SnapshotArgs) -> AppResult<SnapshotCommandResult> {
    let snapshot = match args.capture_scope {
        SnapshotCaptureScopeArg::Full
        | SnapshotCaptureScopeArg::Content
        | SnapshotCaptureScopeArg::Character => {
            if let Some(extractor_inventory) = args.extractor_inventory.as_deref() {
                return Err(AppError::InvalidInput(format!(
                    "--extractor-inventory/--prepared-inventory {} requires --capture-scope extractor",
                    extractor_inventory.display()
                )));
            }

            create_local_snapshot_with_capture_scope(
                &args.version_id,
                args.source_root.as_path(),
                to_local_capture_scope(args.capture_scope)?,
            )?
        }
        SnapshotCaptureScopeArg::Extractor => {
            let extractor_inventory = args.extractor_inventory.as_deref().ok_or_else(|| {
                AppError::InvalidInput(
                    "snapshot --capture-scope extractor requires --extractor-inventory/--prepared-inventory"
                        .to_string(),
                )
            })?;
            create_extractor_backed_snapshot_from_file(
                &args.version_id,
                args.source_root.as_path(),
                extractor_inventory,
            )?
        }
    };
    export_snapshot_output(&snapshot, args.output.as_path())?;

    let mut stored_snapshot_path = None;
    let mut stored_extractor_inventory_path = None;
    if args.store_in_report {
        let storage = match args.report_root.as_ref() {
            Some(root) => ReportStorage::new(root.clone()),
            None => ReportStorage::default(),
        };
        stored_snapshot_path = Some(storage.save_snapshot_for_version(&snapshot)?);
        if let Some(extractor_inventory) = args.extractor_inventory.as_deref() {
            stored_extractor_inventory_path =
                Some(storage.save_extractor_inventory_input_for_version(
                    &snapshot.version_id,
                    extractor_inventory,
                )?);
        }
    }

    info!(
        output = %args.output.display(),
        version_id = %snapshot.version_id,
        asset_count = snapshot.asset_count,
        "snapshot output exported"
    );

    Ok(SnapshotCommandResult {
        snapshot,
        stored_snapshot_path,
        stored_extractor_inventory_path,
    })
}

fn to_local_capture_scope(value: SnapshotCaptureScopeArg) -> AppResult<LocalSnapshotCaptureScope> {
    match value {
        SnapshotCaptureScopeArg::Full => Ok(LocalSnapshotCaptureScope::FullInventory),
        SnapshotCaptureScopeArg::Content => Ok(LocalSnapshotCaptureScope::ContentFocused),
        SnapshotCaptureScopeArg::Character => Ok(LocalSnapshotCaptureScope::CharacterFocused),
        SnapshotCaptureScopeArg::Extractor => Err(AppError::InvalidInput(
            "extractor-backed capture does not map to local filesystem capture scope".to_string(),
        )),
    }
}

pub fn run_snapshot_report_command(
    args: &SnapshotReportArgs,
) -> AppResult<SnapshotInventoryReport> {
    let snapshots = load_snapshots(&args.snapshots)?;
    let report = SnapshotReportRenderer.render(&snapshots)?;
    export_text_output(&report.markdown, args.output.as_path())?;
    info!(
        output = %args.output.display(),
        version_count = report.version_count,
        resonator_count = report.resonator_count,
        pair_count = report.pair_count,
        "snapshot report exported"
    );

    Ok(report)
}

pub fn run_compare_snapshots_command(
    args: &CompareSnapshotsArgs,
) -> AppResult<SnapshotCompareReport> {
    let report = SnapshotComparer.compare(
        &crate::snapshot::load_snapshot(args.old_snapshot.as_path())?,
        &crate::snapshot::load_snapshot(args.new_snapshot.as_path())?,
    );
    export_snapshot_compare_output(&report, args.output.as_path())?;
    info!(
        output = %args.output.display(),
        old_version = %report.old_snapshot.version_id,
        new_version = %report.new_snapshot.version_id,
        changed = report.summary.changed_assets,
        added = report.summary.added_assets,
        removed = report.summary.removed_assets,
        "snapshot compare output exported"
    );

    Ok(report)
}

pub fn run_scan_mod_dependencies_command(
    args: &ScanModDependenciesArgs,
) -> AppResult<ScanModDependenciesResult> {
    let profiles = args
        .mod_roots
        .iter()
        .map(|path| scan_mod_dependency_profile(path.as_path()))
        .collect::<AppResult<Vec<_>>>()?;
    let baseline_set = build_mod_dependency_baseline_set(&args.version_id, profiles)?;
    export_mod_dependency_baseline_set_output(&baseline_set, args.output.as_path())?;

    let mut stored_baseline_set_path = None;
    let mut stored_profile_paths = Vec::new();
    if args.store_in_report {
        let storage = match args.report_root.as_ref() {
            Some(root) => ReportStorage::new(root.clone()),
            None => ReportStorage::default(),
        };
        stored_baseline_set_path = Some(storage.save_mod_dependency_baseline_set_for_version(
            &baseline_set.version_id,
            &baseline_set,
        )?);
        stored_profile_paths = baseline_set
            .profiles
            .iter()
            .map(|profile| {
                storage.save_mod_dependency_profile_for_version(&baseline_set.version_id, profile)
            })
            .collect::<AppResult<Vec<_>>>()?;
    }

    info!(
        output = %args.output.display(),
        version_id = %baseline_set.version_id,
        profile_count = baseline_set.profile_count,
        "mod dependency baseline set exported"
    );

    Ok(ScanModDependenciesResult {
        baseline_set,
        stored_baseline_set_path,
        stored_profile_paths,
    })
}

pub fn build_version_diff_report_v2(
    old_snapshot: &GameSnapshot,
    new_snapshot: &GameSnapshot,
    inference: Option<&InferenceReport>,
) -> VersionDiffReportV2 {
    let compare = SnapshotComparer.compare(old_snapshot, new_snapshot);
    let builder = VersionDiffReportBuilder;
    let report = builder.from_compare(old_snapshot, new_snapshot, &compare);
    match inference {
        Some(inference) => builder.enrich_with_inference(report, inference),
        None => report,
    }
}

pub fn run_extract_wwmi_knowledge_command(
    args: &ExtractWwmiKnowledgeArgs,
) -> AppResult<WwmiKnowledgeBase> {
    let extractor = WwmiKnowledgeExtractor::new_default();
    let knowledge = extractor.extract(&WwmiRepoInput::parse(&args.repo), args.max_commits)?;
    export_wwmi_knowledge_output(&knowledge, args.output.as_path())?;
    info!(
        output = %args.output.display(),
        analyzed_commits = knowledge.summary.analyzed_commits,
        fix_like_commits = knowledge.summary.fix_like_commits,
        patterns = knowledge.summary.discovered_patterns,
        "wwmi knowledge output exported"
    );

    Ok(knowledge)
}

pub fn run_infer_fixes_command(args: &InferFixesArgs) -> AppResult<InferenceReport> {
    let compare_report = load_snapshot_compare_report(args.compare_report.as_path())?;
    let knowledge = load_wwmi_knowledge(args.wwmi_knowledge.as_path())?;
    let continuity = resolve_inference_continuity(args, &compare_report)?;
    let mod_dependency_profile = resolve_inference_mod_dependency_profile(args)?;
    let representative_mod_baseline_set =
        resolve_inference_representative_mod_baseline_set(args, &compare_report)?;
    let report = FixInferenceEngine.infer_with_context(
        &compare_report,
        &knowledge,
        continuity.as_ref(),
        mod_dependency_profile.as_ref(),
        representative_mod_baseline_set.as_ref(),
    );
    export_inference_output(&report, args.output.as_path())?;
    info!(
        output = %args.output.display(),
        crash_causes = report.summary.probable_crash_causes,
        suggested_fixes = report.summary.suggested_fixes,
        mapping_hints = report.summary.candidate_mapping_hints,
        highest_confidence = report.summary.highest_confidence,
        "inference output exported"
    );

    Ok(report)
}

fn resolve_inference_continuity(
    args: &InferFixesArgs,
    compare_report: &SnapshotCompareReport,
) -> AppResult<Option<VersionContinuityIndex>> {
    if let Some(path) = args.continuity_artifact.as_deref() {
        let artifact = load_version_continuity_artifact(path)?;
        return Ok(Some(artifact.continuity));
    }

    let Some(report_root) = args.report_root.as_ref() else {
        return Ok(None);
    };

    let storage = ReportStorage::new(report_root.clone());
    storage.load_version_continuity_index_for_pair(
        &compare_report.old_snapshot.version_id,
        &compare_report.new_snapshot.version_id,
    )
}

fn resolve_inference_mod_dependency_profile(
    args: &InferFixesArgs,
) -> AppResult<Option<WwmiModDependencyProfile>> {
    if args.mod_root.is_some() && args.mod_dependency_profile.is_some() {
        return Err(AppError::InvalidInput(
            "infer-fixes accepts either --mod-root or --mod-dependency-profile, not both"
                .to_string(),
        ));
    }

    if let Some(path) = args.mod_dependency_profile.as_deref() {
        return Ok(Some(load_mod_dependency_profile(path)?));
    }

    if let Some(path) = args.mod_root.as_deref() {
        return Ok(Some(scan_mod_dependency_profile(path)?));
    }

    Ok(None)
}

fn resolve_inference_representative_mod_baseline_set(
    args: &InferFixesArgs,
    compare_report: &SnapshotCompareReport,
) -> AppResult<Option<WwmiModDependencyBaselineSet>> {
    if let Some(path) = args.representative_mod_baseline_set.as_deref() {
        return Ok(Some(load_mod_dependency_baseline_set(path)?));
    }

    let Some(report_root) = args.report_root.as_ref() else {
        return Ok(None);
    };

    let storage = ReportStorage::new(report_root.clone());
    if let Some(baseline_set) =
        storage.load_latest_mod_dependency_baseline_set(&compare_report.old_snapshot.version_id)?
    {
        return Ok(Some(baseline_set));
    }

    storage.load_latest_mod_dependency_baseline_set(&compare_report.new_snapshot.version_id)
}

pub fn run_generate_proposals_command(args: &GenerateProposalsArgs) -> AppResult<ProposalResult> {
    let outputs = ProposalOutputs {
        mapping_output: args.mapping_output.clone(),
        patch_draft_output: args.patch_draft_output.clone(),
        summary_output: args.summary_output.clone(),
    };
    if outputs.is_empty() {
        return Err(AppError::InvalidInput(
            "generate-proposals requires at least one of --mapping-output, --patch-draft-output, or --summary-output"
                .to_string(),
        ));
    }

    let inference_report = load_inference_report(args.inference_report.as_path())?;
    let artifacts = ProposalEngine.generate(&inference_report, args.min_confidence);

    if let Some(output) = outputs.mapping_output.as_deref() {
        export_mapping_proposal_output(&artifacts.mapping_proposal, output)?;
        info!(output = %output.display(), "mapping proposal exported");
    }

    if let Some(output) = outputs.patch_draft_output.as_deref() {
        export_proposal_patch_draft_output(&artifacts.patch_draft, output)?;
        info!(output = %output.display(), "proposal patch draft exported");
    }

    if let Some(output) = outputs.summary_output.as_deref() {
        let content = HumanSummaryRenderer.render(&inference_report, &artifacts);
        export_text_output(&content, output)?;
        info!(output = %output.display(), "human-readable summary exported");
    }

    info!(
        proposed_mappings = artifacts.mapping_proposal.summary.proposed_mappings,
        needs_review_mappings = artifacts.mapping_proposal.summary.needs_review_mappings,
        suggested_fix_actions = artifacts.mapping_proposal.summary.suggested_fix_actions,
        highest_confidence = artifacts.mapping_proposal.summary.highest_confidence,
        "proposal artifacts generated"
    );

    Ok(ProposalResult { artifacts, outputs })
}

pub fn run_map_local_command(args: &MapLocalArgs) -> AppResult<SourceMapOutcome> {
    let config = AppConfig::load(args.config.as_deref())?;
    let old_source = AssetSourceSpec::LocalSnapshot {
        root: args.old_root.clone(),
    };
    let new_source = AssetSourceSpec::LocalSnapshot {
        root: args.new_root.clone(),
    };
    let outputs = SourceMapOutputs {
        report_output: args.report_output.clone(),
        mapping_output: args.mapping_output.clone(),
        patch_draft_output: args.patch_draft_output.clone(),
    };
    let report = preview_map_sources(&old_source, &new_source, &config)?;

    if args.dry_run {
        return Ok(SourceMapOutcome::DryRun(SourceMapResult {
            report,
            outputs,
        }));
    }

    if outputs.is_empty() {
        return Err(AppError::InvalidInput(
            "map-local requires at least one of --report-output, --mapping-output, or --patch-draft-output unless --dry-run is set"
                .to_string(),
        ));
    }

    write_selected_outputs(&report, &outputs)?;

    Ok(SourceMapOutcome::Exported(SourceMapResult {
        report,
        outputs,
    }))
}

pub fn run_map(input: &Path, output: &Path, config: &AppConfig) -> AppResult<PipelineReport> {
    let pipeline = build_pipeline(config);
    pipeline.execute(input, output)
}

pub fn preview_map_sources(
    old_source: &AssetSourceSpec,
    new_source: &AssetSourceSpec,
    config: &AppConfig,
) -> AppResult<PipelineReport> {
    let pipeline = build_pipeline(config);
    let bundle = load_bundle_from_sources(old_source, new_source)?;
    Ok(pipeline.inspect_bundle(bundle))
}

pub fn run_map_sources(
    old_source: &AssetSourceSpec,
    new_source: &AssetSourceSpec,
    output: &Path,
    config: &AppConfig,
) -> AppResult<PipelineReport> {
    let pipeline = build_pipeline(config);
    let bundle = load_bundle_from_sources(old_source, new_source)?;
    pipeline.execute_bundle(bundle, output)
}

fn build_pipeline(
    config: &AppConfig,
) -> MatchPipeline<
    JsonFileIngestSource,
    DefaultFingerprinter,
    HeuristicMatcher,
    ThresholdValidator,
    JsonExporter,
> {
    MatchPipeline::new(
        JsonFileIngestSource,
        DefaultFingerprinter,
        HeuristicMatcher::new(config.matcher.clone()),
        ThresholdValidator::new(config.validator.clone()),
        JsonExporter,
    )
}

fn write_selected_outputs(report: &PipelineReport, outputs: &SourceMapOutputs) -> AppResult<()> {
    if let Some(output) = outputs.report_output.as_deref() {
        JsonExporter.export(report, output)?;
        info!(output = %output.display(), "report exported");
    }

    if let Some(output) = outputs.mapping_output.as_deref() {
        export_mapping_output(report, output)?;
        info!(output = %output.display(), "mapping output exported");
    }

    if let Some(output) = outputs.patch_draft_output.as_deref() {
        export_patch_draft_output(report, output)?;
        info!(output = %output.display(), "patch draft output exported");
    }

    Ok(())
}

fn assumptions() -> Vec<String> {
    vec![
        "Input currently expects a prepared JSON bundle with old_assets and new_assets."
            .to_string(),
        "Direct parsing from a local Wuthering Waves installation is not implemented in this MVP."
            .to_string(),
        "Confidence is heuristic and should be recalibrated once real WW/WWMI samples are available."
            .to_string(),
    ]
}

fn summarize(decisions: &[MatchDecision]) -> PipelineSummary {
    let matched = decisions
        .iter()
        .filter(|decision| decision.status == MatchStatus::Matched)
        .count();
    let needs_review = decisions
        .iter()
        .filter(|decision| decision.status == MatchStatus::NeedsReview)
        .count();
    let rejected = decisions
        .iter()
        .filter(|decision| decision.status == MatchStatus::Rejected)
        .count();

    PipelineSummary {
        total_old_assets: decisions.len(),
        matched,
        needs_review,
        rejected,
    }
}
