use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::wwmi::dependency::{
    WwmiModDependencyBaselineSet, WwmiModDependencyProfile, build_mod_dependency_baseline_set,
    load_mod_dependency_baseline_set, load_mod_dependency_profile, scan_mod_dependency_profile,
};
use crate::{
    cli::{
        CompareSnapshotsArgs, ExtractWwmiKnowledgeArgs, GenerateProposalsArgs, InferFixesArgs,
        IngestFrameAnalysisArgs, MapArgs, MapLocalArgs, OrchestrateVersionPairArgs,
        QualityGateModeArg, ScanModDependenciesArgs, SnapshotArgs, SnapshotCaptureScopeArg,
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
        frame_analysis::{build_prepared_inventory, parse_frame_analysis_log},
        load_bundle_from_sources,
    },
    matcher::{HeuristicMatcher, Matcher},
    output_policy::validate_artifact_output_path,
    proposal::{ProposalArtifacts, ProposalEngine},
    report::{
        VersionContinuityIndex, VersionDiffReportBuilder, VersionDiffReportV2,
        load_version_continuity_artifact,
    },
    report_storage::{ComparedVersionPair, ReportStorage},
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
    pub stored_baseline_summary_path: Option<PathBuf>,
    pub stored_profile_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionPairRunSelectedBaselineManifest {
    pub version_id: String,
    pub path: PathBuf,
    pub artifact_kind: String,
    pub artifact_label: String,
    pub evidence_posture: String,
    pub inventory_alignment: String,
    pub selection_reason: String,
    pub low_signal_for_character_analysis: bool,
    pub readiness_reasons: Vec<String>,
    pub scope_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionPairRunReadinessSideManifest {
    pub version_id: String,
    pub baseline_evidence_posture: String,
    pub low_signal_for_character_analysis: bool,
    pub missing_or_weak_signals: Vec<String>,
    pub scope_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionPairRunReadinessManifest {
    pub compare_low_signal: bool,
    pub scope_narrowing_detected: bool,
    pub scope_induced_removals_likely: bool,
    pub reasons: Vec<String>,
    pub downstream_guardrails: Vec<String>,
    pub old_snapshot: VersionPairRunReadinessSideManifest,
    pub new_snapshot: VersionPairRunReadinessSideManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionPairRunProducedArtifactsManifest {
    pub run_directory: PathBuf,
    pub compare_report: PathBuf,
    pub inference_report: PathBuf,
    pub mapping_proposal: PathBuf,
    pub patch_draft: PathBuf,
    pub human_summary: PathBuf,
    pub manifest: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionPairRunSummaryManifest {
    pub changed_assets: usize,
    pub added_assets: usize,
    pub removed_assets: usize,
    pub candidate_mapping_changes: usize,
    pub probable_crash_causes: usize,
    pub suggested_fixes: usize,
    pub candidate_mapping_hints: usize,
    pub proposed_mappings: usize,
    pub needs_review_mappings: usize,
    pub suggested_fix_actions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionPairRunManifest {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub old_version_id: String,
    pub new_version_id: String,
    pub report_root: PathBuf,
    pub selected_old_baseline: VersionPairRunSelectedBaselineManifest,
    pub selected_new_baseline: VersionPairRunSelectedBaselineManifest,
    #[serde(default)]
    pub readiness: VersionPairRunReadinessManifest,
    #[serde(default)]
    pub quality_gate: VersionPairRunQualityGateManifest,
    pub produced_artifacts: VersionPairRunProducedArtifactsManifest,
    pub summary: VersionPairRunSummaryManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionPairRunQualityGateSignalsManifest {
    pub compare_low_signal: bool,
    pub scope_narrowing_detected: bool,
    pub scope_induced_removals_likely: bool,
    pub old_baseline_evidence_posture: String,
    pub old_baseline_inventory_alignment: String,
    pub old_baseline_low_signal_for_character_analysis: bool,
    pub new_baseline_evidence_posture: String,
    pub new_baseline_inventory_alignment: String,
    pub new_baseline_low_signal_for_character_analysis: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct VersionPairRunQualityGateManifest {
    pub mode: String,
    pub status: String,
    pub passed: bool,
    pub reasons: Vec<String>,
    pub key_signals: VersionPairRunQualityGateSignalsManifest,
}

#[derive(Debug, Clone)]
pub struct VersionPairRunResult {
    pub manifest: VersionPairRunManifest,
}

#[derive(Debug, Clone)]
struct VersionPairRunOutputPaths {
    run_directory: PathBuf,
    compare_output: PathBuf,
    inference_output: PathBuf,
    mapping_output: PathBuf,
    patch_draft_output: PathBuf,
    summary_output: PathBuf,
    manifest_output: PathBuf,
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
        validate_snapshot_storage_alignment(&snapshot, args.source_root.as_path())?;
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

pub fn run_ingest_frame_analysis_command(args: IngestFrameAnalysisArgs) -> AppResult<()> {
    validate_artifact_output_path(args.output.as_path())?;

    if !args.dump_dir.exists() {
        return Err(AppError::InvalidInput(format!(
            "frame analysis dump directory does not exist: {}",
            args.dump_dir.display()
        )));
    }
    if !args.dump_dir.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "frame analysis dump path is not a directory: {}",
            args.dump_dir.display()
        )));
    }

    let dump_dir = args.dump_dir.canonicalize()?;
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

    let inventory = build_prepared_inventory(&dump, &args.version_id);
    let json = serde_json::to_string_pretty(&inventory)?;

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, json)?;

    if args.store_snapshot {
        let snapshot = create_extractor_backed_snapshot_from_file(
            &args.version_id,
            dump_dir.as_path(),
            args.output.as_path(),
        )?;
        let storage = match args.report_root {
            Some(root) => ReportStorage::new(root),
            None => ReportStorage::default(),
        };
        let stored_path = storage.save_snapshot_for_version(&snapshot)?;
        println!("stored snapshot: {}", stored_path.display());
    }

    let ib_count = inventory
        .assets
        .iter()
        .filter(|asset| asset.asset.kind.as_deref() == Some("index_buffer"))
        .count();
    let vb_count = inventory
        .assets
        .iter()
        .filter(|asset| asset.asset.kind.as_deref() == Some("vertex_buffer"))
        .count();
    let shader_count = inventory
        .assets
        .iter()
        .filter(|asset| {
            matches!(
                asset.asset.kind.as_deref(),
                Some("vertex_shader") | Some("pixel_shader")
            )
        })
        .count();
    println!(
        "frame-analysis inventory exported: draw_calls={} ib={} vb={} shaders={} assets={}",
        dump.draw_calls.len(),
        ib_count,
        vb_count,
        shader_count,
        inventory.assets.len()
    );
    println!("wrote frame-analysis inventory: {}", args.output.display());

    Ok(())
}

fn validate_snapshot_storage_alignment(
    snapshot: &GameSnapshot,
    source_root: &Path,
) -> AppResult<()> {
    let Some(launcher) = snapshot.context.launcher.as_ref() else {
        return Ok(());
    };
    if launcher.detected_version == snapshot.version_id {
        return Ok(());
    }

    Err(AppError::InvalidInput(format!(
        "refusing to store snapshot version {} under report root because launcher detected version {} under {}; use --version-id auto or an aligned explicit version before freezing a canonical baseline",
        snapshot.version_id,
        launcher.detected_version,
        source_root.display()
    )))
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

pub fn run_orchestrate_version_pair_command(
    args: &OrchestrateVersionPairArgs,
) -> AppResult<VersionPairRunResult> {
    let storage = match args.report_root.as_ref() {
        Some(root) => ReportStorage::new(root.clone()),
        None => ReportStorage::default(),
    };
    let outputs = resolve_version_pair_run_output_paths(args)?;
    let compared = storage
        .compare_versions_with_selected_baselines(&args.old_version_id, &args.new_version_id)?;
    export_snapshot_compare_output(&compared.compare, &outputs.compare_output)?;
    info!(
        output = %outputs.compare_output.display(),
        old_version = %compared.compare.old_snapshot.version_id,
        new_version = %compared.compare.new_snapshot.version_id,
        changed = compared.compare.summary.changed_assets,
        added = compared.compare.summary.added_assets,
        removed = compared.compare.summary.removed_assets,
        "version-pair compare output exported"
    );

    let quality_gate = build_version_pair_quality_gate_manifest(&compared, args.quality_gate_mode);
    if args.quality_gate_mode == QualityGateModeArg::Enforce && !quality_gate.passed {
        let manifest = build_version_pair_run_manifest(
            &storage,
            &compared,
            None,
            None,
            &outputs,
            quality_gate,
        )?;
        export_text_output(
            &serde_json::to_string_pretty(&manifest)?,
            &outputs.manifest_output,
        )?;
        info!(
            output = %outputs.manifest_output.display(),
            old_version = %manifest.old_version_id,
            new_version = %manifest.new_version_id,
            "version-pair run manifest exported"
        );

        return Err(AppError::InvalidInput(format!(
            "quality gate blocked orchestrate-version-pair for {} -> {}; status={} manifest={}",
            manifest.old_version_id,
            manifest.new_version_id,
            manifest.quality_gate.status,
            outputs.manifest_output.display()
        )));
    }

    let inference = run_infer_fixes_command(&InferFixesArgs {
        compare_report: outputs.compare_output.clone(),
        wwmi_knowledge: args.wwmi_knowledge.clone(),
        continuity_artifact: None,
        report_root: Some(storage.reports_root()),
        mod_root: None,
        mod_dependency_profile: None,
        representative_mod_baseline_set: None,
        output: outputs.inference_output.clone(),
    })?;
    let proposals = run_generate_proposals_command(&GenerateProposalsArgs {
        inference_report: outputs.inference_output.clone(),
        mapping_output: Some(outputs.mapping_output.clone()),
        patch_draft_output: Some(outputs.patch_draft_output.clone()),
        summary_output: Some(outputs.summary_output.clone()),
        min_confidence: args.min_confidence,
    })?;
    let manifest = build_version_pair_run_manifest(
        &storage,
        &compared,
        Some(&inference),
        Some(&proposals),
        &outputs,
        quality_gate,
    )?;
    export_text_output(
        &serde_json::to_string_pretty(&manifest)?,
        &outputs.manifest_output,
    )?;
    info!(
        output = %outputs.manifest_output.display(),
        old_version = %manifest.old_version_id,
        new_version = %manifest.new_version_id,
        "version-pair run manifest exported"
    );

    Ok(VersionPairRunResult { manifest })
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
    let mut stored_baseline_summary_path = None;
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
        stored_baseline_summary_path =
            Some(storage.save_mod_dependency_baseline_summary_for_version(
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
        stored_baseline_summary_path,
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
        let baseline_set = load_mod_dependency_baseline_set(path)?;
        return validate_representative_mod_baseline_set_version(&baseline_set, compare_report)
            .map(Some);
    }

    let Some(report_root) = args.report_root.as_ref() else {
        return Ok(None);
    };

    let storage = ReportStorage::new(report_root.clone());
    if let Some(baseline_set) =
        storage.load_latest_mod_dependency_baseline_set(&compare_report.old_snapshot.version_id)?
    {
        return validate_representative_mod_baseline_set_version(&baseline_set, compare_report)
            .map(Some);
    }

    Ok(None)
}

fn validate_representative_mod_baseline_set_version(
    baseline_set: &WwmiModDependencyBaselineSet,
    compare_report: &SnapshotCompareReport,
) -> AppResult<WwmiModDependencyBaselineSet> {
    if baseline_set.version_id == compare_report.old_snapshot.version_id {
        return Ok(baseline_set.clone());
    }

    Err(AppError::InvalidInput(format!(
        "representative mod baseline set version {} does not match compare old snapshot version {}; representative risk projection requires a prepatch baseline aligned to the old snapshot",
        baseline_set.version_id, compare_report.old_snapshot.version_id
    )))
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

fn resolve_version_pair_run_output_paths(
    args: &OrchestrateVersionPairArgs,
) -> AppResult<VersionPairRunOutputPaths> {
    let run_directory = args.output_dir.clone();
    let compare_output = resolve_run_output_path(
        &run_directory,
        args.compare_output.as_deref(),
        "compare.v1.json",
    )?;
    let inference_output = resolve_run_output_path(
        &run_directory,
        args.inference_output.as_deref(),
        "inference.v1.json",
    )?;
    let mapping_output = resolve_run_output_path(
        &run_directory,
        args.mapping_output.as_deref(),
        "mapping-proposal.v1.json",
    )?;
    let patch_draft_output = resolve_run_output_path(
        &run_directory,
        args.patch_draft_output.as_deref(),
        "proposal-patch-draft.v1.json",
    )?;
    let summary_output = resolve_run_output_path(
        &run_directory,
        args.summary_output.as_deref(),
        "human-summary.md",
    )?;
    let manifest_output = resolve_run_output_path(
        &run_directory,
        args.manifest_output.as_deref(),
        "run-manifest.v1.json",
    )?;

    Ok(VersionPairRunOutputPaths {
        run_directory,
        compare_output,
        inference_output,
        mapping_output,
        patch_draft_output,
        summary_output,
        manifest_output,
    })
}

fn resolve_run_output_path(
    run_directory: &Path,
    explicit: Option<&Path>,
    default_file_name: &str,
) -> AppResult<PathBuf> {
    let path = explicit
        .map(PathBuf::from)
        .unwrap_or_else(|| run_directory.join(default_file_name));
    let parent = path.parent().ok_or_else(|| {
        AppError::InvalidInput(format!(
            "version-pair orchestration output {} must have a parent directory",
            path.display()
        ))
    })?;
    if normalize_path(parent)? != normalize_path(run_directory)? {
        return Err(AppError::InvalidInput(format!(
            "version-pair orchestration output {} must stay under run directory {}",
            path.display(),
            run_directory.display()
        )));
    }
    Ok(path)
}

fn normalize_path(path: &Path) -> AppResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(std::env::current_dir()?.join(path))
}

fn build_version_pair_run_manifest(
    storage: &ReportStorage,
    compared: &ComparedVersionPair,
    inference: Option<&InferenceReport>,
    proposals: Option<&ProposalResult>,
    outputs: &VersionPairRunOutputPaths,
    quality_gate: VersionPairRunQualityGateManifest,
) -> AppResult<VersionPairRunManifest> {
    let readiness = match (inference, proposals) {
        (Some(inference), Some(proposals)) => {
            build_version_pair_readiness_manifest(compared, inference, proposals)
        }
        _ => VersionPairRunReadinessManifest::default(),
    };
    Ok(VersionPairRunManifest {
        schema_version: "whashreonator.version-pair-run.v1".to_string(),
        generated_at_unix_ms: current_unix_ms()?,
        old_version_id: compared.old_baseline.version_id.clone(),
        new_version_id: compared.new_baseline.version_id.clone(),
        report_root: storage.reports_root(),
        selected_old_baseline: baseline_manifest_entry(&compared.old_baseline),
        selected_new_baseline: baseline_manifest_entry(&compared.new_baseline),
        readiness,
        quality_gate,
        produced_artifacts: VersionPairRunProducedArtifactsManifest {
            run_directory: outputs.run_directory.clone(),
            compare_report: outputs.compare_output.clone(),
            inference_report: outputs.inference_output.clone(),
            mapping_proposal: outputs.mapping_output.clone(),
            patch_draft: outputs.patch_draft_output.clone(),
            human_summary: outputs.summary_output.clone(),
            manifest: outputs.manifest_output.clone(),
        },
        summary: VersionPairRunSummaryManifest {
            changed_assets: compared.compare.summary.changed_assets,
            added_assets: compared.compare.summary.added_assets,
            removed_assets: compared.compare.summary.removed_assets,
            candidate_mapping_changes: compared.compare.summary.candidate_mapping_changes,
            probable_crash_causes: inference
                .map(|report| report.summary.probable_crash_causes)
                .unwrap_or(0),
            suggested_fixes: inference
                .map(|report| report.summary.suggested_fixes)
                .unwrap_or(0),
            candidate_mapping_hints: inference
                .map(|report| report.summary.candidate_mapping_hints)
                .unwrap_or(0),
            proposed_mappings: proposals
                .map(|result| result.artifacts.mapping_proposal.summary.proposed_mappings)
                .unwrap_or(0),
            needs_review_mappings: proposals
                .map(|result| {
                    result
                        .artifacts
                        .mapping_proposal
                        .summary
                        .needs_review_mappings
                })
                .unwrap_or(0),
            suggested_fix_actions: proposals
                .map(|result| {
                    result
                        .artifacts
                        .mapping_proposal
                        .summary
                        .suggested_fix_actions
                })
                .unwrap_or(0),
        },
    })
}

fn build_version_pair_quality_gate_manifest(
    compared: &ComparedVersionPair,
    mode: QualityGateModeArg,
) -> VersionPairRunQualityGateManifest {
    let old_scope = crate::snapshot::assess_snapshot_scope(&compared.old_baseline.snapshot);
    let new_scope = crate::snapshot::assess_snapshot_scope(&compared.new_baseline.snapshot);

    let key_signals = VersionPairRunQualityGateSignalsManifest {
        compare_low_signal: compared.compare.scope.low_signal_compare,
        scope_narrowing_detected: compared.compare.scope.scope_narrowing_detected,
        scope_induced_removals_likely: compared.compare.scope.scope_induced_removals_likely,
        old_baseline_evidence_posture: compared.old_baseline.evidence_posture.clone(),
        old_baseline_inventory_alignment: compared.old_baseline.inventory_alignment.clone(),
        old_baseline_low_signal_for_character_analysis: old_scope
            .is_low_signal_for_character_analysis(),
        new_baseline_evidence_posture: compared.new_baseline.evidence_posture.clone(),
        new_baseline_inventory_alignment: compared.new_baseline.inventory_alignment.clone(),
        new_baseline_low_signal_for_character_analysis: new_scope
            .is_low_signal_for_character_analysis(),
    };

    let mut reasons = Vec::new();
    if key_signals.compare_low_signal {
        reasons.push(
            "compare scope is low-signal, so downstream repair review should stay audit-first"
                .to_string(),
        );
    }
    if key_signals.old_baseline_low_signal_for_character_analysis {
        reasons.push(format!(
            "selected old baseline {} remains low-signal for character/content analysis",
            compared.old_baseline.version_id
        ));
    }
    if key_signals.new_baseline_low_signal_for_character_analysis {
        reasons.push(format!(
            "selected new baseline {} remains low-signal for character/content analysis",
            compared.new_baseline.version_id
        ));
    }
    if key_signals.scope_narrowing_detected {
        reasons.push(
            "scope narrowing was detected across the selected pair, so missing paths may reflect capture scope drift"
                .to_string(),
        );
    }
    if key_signals.scope_induced_removals_likely {
        reasons.push(
            "scope-induced removals are likely, so removal-heavy compare results should not hard-gate repair conclusions"
                .to_string(),
        );
    }
    if compared.old_baseline.inventory_alignment != "aligned" {
        reasons.push(format!(
            "selected old baseline inventory alignment is {}",
            compared.old_baseline.inventory_alignment
        ));
    }
    if compared.new_baseline.inventory_alignment != "aligned" {
        reasons.push(format!(
            "selected new baseline inventory alignment is {}",
            compared.new_baseline.inventory_alignment
        ));
    }

    let block = key_signals.compare_low_signal
        && (key_signals.scope_induced_removals_likely
            || (key_signals.old_baseline_low_signal_for_character_analysis
                && key_signals.new_baseline_low_signal_for_character_analysis));
    let warn = !block
        && (key_signals.compare_low_signal
            || key_signals.scope_narrowing_detected
            || key_signals.old_baseline_low_signal_for_character_analysis
            || key_signals.new_baseline_low_signal_for_character_analysis
            || compared.old_baseline.inventory_alignment != "aligned"
            || compared.new_baseline.inventory_alignment != "aligned");
    let status = if block {
        "block"
    } else if warn {
        "warn"
    } else {
        "pass"
    };

    VersionPairRunQualityGateManifest {
        mode: match mode {
            QualityGateModeArg::Advisory => "advisory".to_string(),
            QualityGateModeArg::Enforce => "enforce".to_string(),
        },
        status: status.to_string(),
        passed: !block,
        reasons,
        key_signals,
    }
}

fn baseline_manifest_entry(
    baseline: &crate::report_storage::SelectedSnapshotBaseline,
) -> VersionPairRunSelectedBaselineManifest {
    let scope = crate::snapshot::assess_snapshot_scope(&baseline.snapshot);
    let quality = crate::snapshot::summarize_snapshot_capture_quality(&baseline.snapshot);

    VersionPairRunSelectedBaselineManifest {
        version_id: baseline.version_id.clone(),
        path: baseline.path.clone(),
        artifact_kind: format!("{:?}", baseline.artifact_kind),
        artifact_label: baseline.artifact_label.clone(),
        evidence_posture: baseline.evidence_posture.clone(),
        inventory_alignment: baseline.inventory_alignment.clone(),
        selection_reason: baseline.selection_reason.clone(),
        low_signal_for_character_analysis: scope.is_low_signal_for_character_analysis(),
        readiness_reasons: build_snapshot_readiness_reasons(&scope, &quality),
        scope_note: scope.note,
    }
}

fn build_version_pair_readiness_manifest(
    compared: &ComparedVersionPair,
    inference: &InferenceReport,
    proposals: &ProposalResult,
) -> VersionPairRunReadinessManifest {
    let old_scope = crate::snapshot::assess_snapshot_scope(&compared.old_baseline.snapshot);
    let new_scope = crate::snapshot::assess_snapshot_scope(&compared.new_baseline.snapshot);
    let old_quality =
        crate::snapshot::summarize_snapshot_capture_quality(&compared.old_baseline.snapshot);
    let new_quality =
        crate::snapshot::summarize_snapshot_capture_quality(&compared.new_baseline.snapshot);
    let old_reasons = build_snapshot_readiness_reasons(&old_scope, &old_quality);
    let new_reasons = build_snapshot_readiness_reasons(&new_scope, &new_quality);

    let mut reasons = Vec::new();
    if compared.compare.scope.low_signal_compare {
        reasons.push(
            "compare readiness is low-signal because one or both selected baselines remain shallow, sparse, or only weakly enriched for character/content analysis"
                .to_string(),
        );
    }
    reasons.extend(old_reasons.iter().map(|reason| {
        format!(
            "old snapshot {} readiness: {reason}",
            compared.old_baseline.version_id
        )
    }));
    reasons.extend(new_reasons.iter().map(|reason| {
        format!(
            "new snapshot {} readiness: {reason}",
            compared.new_baseline.version_id
        )
    }));
    if compared.compare.scope.scope_narrowing_detected {
        reasons.push(
            "snapshot scope narrowing was detected across the selected version pair, so missing paths may reflect capture scope rather than real removal"
                .to_string(),
        );
    }
    if compared.compare.scope.scope_induced_removals_likely {
        reasons.push(
            "removed-only deltas include scope-induced risk, so removal-heavy interpretation should stay audit-first"
                .to_string(),
        );
    }
    reasons.extend(compared.compare.scope.notes.iter().cloned());

    let mut downstream_guardrails = Vec::new();
    if inference.scope.low_signal_compare {
        downstream_guardrails.push(
            "inference remains conservative under low-signal compare scope and does not treat shallow evidence as strong semantic certainty"
                .to_string(),
        );
        downstream_guardrails.push(
            "proposal generation keeps low-signal mapping candidates review-first unless strong-evidence checks still pass"
                .to_string(),
        );
    }
    if compared.compare.scope.scope_induced_removals_likely {
        downstream_guardrails.push(
            "scope-induced removals were kept out of crash-cause promotion until reviewer validation confirms true version drift"
                .to_string(),
        );
    }
    if inference
        .representative_mod_baseline_input
        .as_ref()
        .is_some_and(|baseline| !baseline.material_for_repair_review)
    {
        downstream_guardrails.push(
            "representative mod baseline support stays review-only because the sampled baseline set is limited"
                .to_string(),
        );
    }
    if inference.scope.low_signal_compare
        && proposals
            .artifacts
            .mapping_proposal
            .summary
            .proposed_mappings
            == 0
        && proposals
            .artifacts
            .mapping_proposal
            .summary
            .needs_review_mappings
            > 0
    {
        downstream_guardrails.push(
            "all current mapping outputs stayed in NeedsReview, which is expected while readiness remains low-signal"
                .to_string(),
        );
    }

    VersionPairRunReadinessManifest {
        compare_low_signal: compared.compare.scope.low_signal_compare,
        scope_narrowing_detected: compared.compare.scope.scope_narrowing_detected,
        scope_induced_removals_likely: compared.compare.scope.scope_induced_removals_likely,
        reasons,
        downstream_guardrails,
        old_snapshot: VersionPairRunReadinessSideManifest {
            version_id: compared.old_baseline.version_id.clone(),
            baseline_evidence_posture: compared.old_baseline.evidence_posture.clone(),
            low_signal_for_character_analysis: old_scope.is_low_signal_for_character_analysis(),
            missing_or_weak_signals: old_reasons,
            scope_note: old_scope.note,
        },
        new_snapshot: VersionPairRunReadinessSideManifest {
            version_id: compared.new_baseline.version_id.clone(),
            baseline_evidence_posture: compared.new_baseline.evidence_posture.clone(),
            low_signal_for_character_analysis: new_scope.is_low_signal_for_character_analysis(),
            missing_or_weak_signals: new_reasons,
            scope_note: new_scope.note,
        },
    }
}

fn build_snapshot_readiness_reasons(
    scope: &crate::snapshot::SnapshotScopeAssessment,
    quality: &crate::snapshot::SnapshotCaptureQualitySummary,
) -> Vec<String> {
    let mut reasons = quality.low_signal_reasons(scope);
    if let Some(reason) = quality.extractor_alignment_reason() {
        reasons.push(reason.to_string());
    }
    reasons
}

fn current_unix_ms() -> AppResult<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::InvalidInput(format!("system clock error: {error}")))?
        .as_millis())
}
