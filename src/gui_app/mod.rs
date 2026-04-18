use std::path::{Path, PathBuf};

use crate::{
    compare::SnapshotComparer,
    error::AppResult,
    human_summary::HumanSummaryRenderer,
    inference::{FixInferenceEngine, InferenceReport},
    output_policy::resolve_artifact_root,
    proposal::{ProposalArtifacts, ProposalEngine},
    report::{DiffStatus, ResonatorDiffEntry, VersionDiffReportV2},
    report_storage::{ReportStorage, VersionArtifactKind},
    scan::{
        ExecuteVersionScanResult, LocalSnapshotFactory, PrepareVersionScanResult,
        PreparedVersionScan, VersionScanService,
    },
    snapshot::{GameSnapshot, assess_snapshot_scope},
    wwmi::load_wwmi_knowledge,
};

#[derive(Debug, Clone)]
pub struct ScanForm {
    pub source_root: String,
    pub version_override: String,
    pub knowledge_path: String,
}

impl Default for ScanForm {
    fn default() -> Self {
        Self {
            source_root: String::new(),
            version_override: String::new(),
            knowledge_path: resolve_artifact_root()
                .join("wwmi-knowledge.json")
                .display()
                .to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VersionRowView {
    pub version_id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct VersionDetailView {
    pub summary: String,
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReportDetailView {
    pub summary: String,
    pub table_rows: Vec<CompareTableRow>,
    pub quality_gate_text: String,
    pub inference_text: String,
    pub proposal_text: String,
    pub human_summary_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct CompareTableRow {
    pub resonator: String,
    pub item_type: String,
    pub status: String,
    pub confidence: String,
    pub path: String,
    pub asset_hash: String,
    pub shader_hash: String,
}

#[derive(Debug, Clone)]
pub enum ScanStartResult {
    Ready(PreparedVersionScan),
    VersionAlreadyExists(PreparedVersionScan),
}

#[derive(Debug, Clone)]
pub enum ScanRunResult {
    Created {
        version_id: String,
        saved_path: PathBuf,
        summary: String,
    },
    NoChangesDetected {
        version_id: String,
        saved_path: PathBuf,
        summary: String,
    },
    Overwritten {
        version_id: String,
        saved_path: PathBuf,
        summary: String,
    },
}

#[derive(Debug, Clone)]
pub struct GuiController {
    storage: ReportStorage,
}

impl Default for GuiController {
    fn default() -> Self {
        Self {
            storage: ReportStorage::default(),
        }
    }
}

impl GuiController {
    pub fn new(storage: ReportStorage) -> Self {
        Self { storage }
    }

    pub fn prepare_scan(&self, form: &ScanForm) -> AppResult<ScanStartResult> {
        let service = VersionScanService::new(self.storage.clone(), LocalSnapshotFactory);
        let source_root = Path::new(form.source_root.trim());
        let version_override = optional_text(&form.version_override);

        match service.prepare_scan(source_root, version_override.as_deref())? {
            PrepareVersionScanResult::Ready(prepared) => Ok(ScanStartResult::Ready(prepared)),
            PrepareVersionScanResult::VersionAlreadyExists(prepared) => {
                Ok(ScanStartResult::VersionAlreadyExists(prepared))
            }
        }
    }

    pub fn run_scan(
        &self,
        prepared: &PreparedVersionScan,
        force_rescan: bool,
        knowledge_path: &str,
    ) -> AppResult<ScanRunResult> {
        let service = VersionScanService::new(self.storage.clone(), LocalSnapshotFactory);
        let knowledge_path = optional_text(knowledge_path);

        match service.execute_scan(prepared, force_rescan)? {
            ExecuteVersionScanResult::Created {
                version_id,
                snapshot_path,
                snapshot,
            } => Ok(ScanRunResult::Created {
                summary: self.scan_summary_with_phase3(
                    &version_id,
                    &render_scan_summary(&snapshot),
                    knowledge_path.as_deref(),
                )?,
                saved_path: snapshot_path,
                version_id,
            }),
            ExecuteVersionScanResult::NoChangesDetected {
                version_id,
                snapshot_path,
            } => Ok(ScanRunResult::NoChangesDetected {
                summary: self.scan_summary_with_phase3(
                    &version_id,
                    &format!(
                        "Scan Version {}\nNo changes detected.\nSnapshot: {}",
                        version_id,
                        snapshot_path.display()
                    ),
                    knowledge_path.as_deref(),
                )?,
                saved_path: snapshot_path,
                version_id,
            }),
            ExecuteVersionScanResult::Overwritten {
                version_id,
                snapshot_path,
                snapshot,
            } => Ok(ScanRunResult::Overwritten {
                summary: self.scan_summary_with_phase3(
                    &version_id,
                    &render_scan_summary(&snapshot),
                    knowledge_path.as_deref(),
                )?,
                saved_path: snapshot_path,
                version_id,
            }),
        }
    }

    pub fn list_versions(&self) -> AppResult<Vec<VersionRowView>> {
        let versions = self.storage.list_versions()?;
        Ok(versions
            .into_iter()
            .map(|entry| VersionRowView {
                label: format!("wuwa_{}", entry.version_id),
                version_id: entry.version_id,
            })
            .collect())
    }

    pub fn open_version(&self, version_id: &str) -> AppResult<VersionDetailView> {
        let artifacts = self.storage.list_version_artifacts(version_id)?;
        let snapshot = self.storage.load_snapshot_by_version(version_id)?;
        let report = self.storage.load_version_report(version_id)?;
        let inference = self.storage.load_latest_inference(version_id)?;
        let mapping_proposal = self.storage.load_latest_mapping_proposal(version_id)?;
        let patch_draft = self.storage.load_latest_patch_draft(version_id)?;
        let human_summary = self.storage.load_latest_human_summary(version_id)?;

        let summary = render_version_summary(
            version_id,
            snapshot.as_ref(),
            report.as_ref(),
            inference.as_ref(),
            mapping_proposal.as_ref(),
            patch_draft.as_ref(),
            human_summary.as_deref(),
            &artifacts,
        );
        let artifacts = render_version_artifact_lines(&artifacts);

        Ok(VersionDetailView { summary, artifacts })
    }

    pub fn compare_versions(
        &self,
        old_version: &str,
        new_version: &str,
        resonator_filter: &str,
        hide_unchanged: bool,
    ) -> AppResult<ReportDetailView> {
        let report = self.storage.compare_versions(old_version, new_version)?;
        let mut view = render_detail_view(&report, resonator_filter, hide_unchanged);

        let inference = self
            .storage
            .load_latest_inference(new_version)
            .ok()
            .flatten();
        let mapping_proposal = self
            .storage
            .load_latest_mapping_proposal(new_version)
            .ok()
            .flatten();
        let patch_draft = self
            .storage
            .load_latest_patch_draft(new_version)
            .ok()
            .flatten();
        let human_summary = self
            .storage
            .load_latest_human_summary(new_version)
            .ok()
            .flatten();

        view.quality_gate_text = render_quality_gate_preview(&report);
        view.inference_text = render_inference_preview(inference.as_ref());
        view.proposal_text =
            render_proposal_preview(mapping_proposal.as_ref(), patch_draft.as_ref());
        view.human_summary_text = render_human_summary_preview(human_summary.as_deref());

        Ok(view)
    }

    pub fn reports_root_label(&self) -> String {
        self.storage.reports_root().display().to_string()
    }

    pub fn artifact_root_label(&self) -> String {
        resolve_artifact_root().display().to_string()
    }

    fn scan_summary_with_phase3(
        &self,
        scanned_version: &str,
        scan_summary: &str,
        knowledge_path: Option<&str>,
    ) -> AppResult<String> {
        let Some(knowledge_path) = knowledge_path
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(format!(
                "{scan_summary}\n\nPhase 3: skipped (knowledge path is empty)."
            ));
        };
        let knowledge_path = Path::new(knowledge_path);
        if !knowledge_path.exists() {
            return Ok(format!(
                "{scan_summary}\n\nPhase 3: skipped (knowledge file not found: {}).",
                knowledge_path.display()
            ));
        }

        let Some(baseline_version) = self.storage.select_baseline_version(scanned_version)? else {
            return Ok(format!(
                "{scan_summary}\n\nPhase 3: skipped (no baseline version available for compare)."
            ));
        };

        if baseline_version == scanned_version {
            return Ok(format!(
                "{scan_summary}\n\nPhase 3: skipped (baseline equals scanned version)."
            ));
        }

        let old_snapshot = self
            .storage
            .load_snapshot_by_version(&baseline_version)?
            .ok_or_else(|| {
                crate::error::AppError::InvalidInput(format!(
                    "baseline snapshot {} not found",
                    baseline_version
                ))
            })?;
        let new_snapshot = self
            .storage
            .load_snapshot_by_version(scanned_version)?
            .ok_or_else(|| {
                crate::error::AppError::InvalidInput(format!(
                    "scanned snapshot {} not found",
                    scanned_version
                ))
            })?;
        let compare = SnapshotComparer.compare(&old_snapshot, &new_snapshot);
        let knowledge = load_wwmi_knowledge(knowledge_path)?;
        let continuity = self
            .storage
            .load_version_continuity_index_for_pair(&baseline_version, scanned_version)?;
        let inference =
            FixInferenceEngine.infer_with_continuity(&compare, &knowledge, continuity.as_ref());
        let proposals = ProposalEngine.generate(&inference, 0.85);
        let human_summary = HumanSummaryRenderer.render(&inference, &proposals);
        let report = crate::report::VersionDiffReportBuilder.enrich_with_inference(
            crate::report::VersionDiffReportBuilder.from_compare(
                &old_snapshot,
                &new_snapshot,
                &compare,
            ),
            &inference,
        );

        self.storage.save_phase3_outputs(
            &report,
            &old_snapshot,
            &new_snapshot,
            &compare,
            &inference,
            &proposals,
            &human_summary,
        )?;

        let phase3_summary = render_phase3_generation_summary(
            &baseline_version,
            scanned_version,
            &inference,
            &proposals,
            compare.scope.low_signal_compare,
            &compare.scope.notes,
        );
        Ok(format!("{scan_summary}\n\n{phase3_summary}"))
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn optional_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn render_scan_summary(snapshot: &crate::snapshot::GameSnapshot) -> String {
    let scope = assess_snapshot_scope(snapshot);
    let mut lines = vec![
        format!("Scan Version {}", snapshot.version_id),
        format!("Assets: {}", snapshot.asset_count),
        format!("Source root: {}", snapshot.source_root),
        format!(
            "Launcher version: {}",
            snapshot
                .context
                .launcher
                .as_ref()
                .map(|launcher| launcher.detected_version.as_str())
                .unwrap_or("n/a")
        ),
        format!(
            "Snapshot scope: mode={} install_or_package_level={} meaningful_content={} meaningful_character={}",
            scope.capture_mode.as_deref().unwrap_or("unknown"),
            scope.mostly_install_or_package_level,
            scope.meaningful_content_coverage,
            scope.meaningful_character_coverage
        ),
        format!(
            "Coverage signals: content_like_paths={} character_paths={} non_content_paths={}",
            scope.coverage.content_like_path_count,
            scope.coverage.character_path_count,
            scope.coverage.non_content_path_count
        ),
    ];

    if scope.is_low_signal_for_character_analysis() {
        lines.push(
            "Scope caution: this snapshot is install/package-level or low-coverage; deep character-level interpretation may be limited."
                .to_string(),
        );
    }
    if let Some(note) = scope.note {
        lines.push(format!("Scope note: {note}"));
    }

    lines.join("\n")
}

fn render_phase3_generation_summary(
    baseline_version: &str,
    scanned_version: &str,
    inference: &InferenceReport,
    proposals: &ProposalArtifacts,
    low_signal_compare: bool,
    scope_notes: &[String],
) -> String {
    let mut lines = vec![format!(
        "Phase 3 generated using baseline {} -> {}.\nCrash causes: {} | Suggested fixes: {} | Mapping hints: {}.\nProposed mappings: {} | Needs review: {} | Patch actions: {}.",
        baseline_version,
        scanned_version,
        inference.summary.probable_crash_causes,
        inference.summary.suggested_fixes,
        inference.summary.candidate_mapping_hints,
        proposals.mapping_proposal.summary.proposed_mappings,
        proposals.mapping_proposal.summary.needs_review_mappings,
        proposals.patch_draft.actions.len()
    )];
    if low_signal_compare {
        lines.push(
            "Phase 3 caution: compare scope is install/package-level or low-coverage, so inference/proposal outputs are low-signal and should be reviewed conservatively."
                .to_string(),
        );
        for note in scope_notes.iter().take(2) {
            lines.push(format!("  scope: {note}"));
        }
    }
    let continuity_review_mappings = proposals
        .mapping_proposal
        .mappings
        .iter()
        .filter(|mapping| mapping_has_continuity_caution(mapping))
        .count();
    if !continuity_causes(inference).is_empty()
        || continuity_fix(inference).is_some()
        || continuity_review_mappings > 0
    {
        lines.push(format!(
            "Phase 3 continuity caution: broader thread history is keeping {} mapping(s) review-first.",
            continuity_review_mappings
        ));
        if let Some(cause) = continuity_causes(inference).first() {
            lines.push(format!(
                "  continuity cause [{}] confidence={:.3}",
                cause.code, cause.confidence
            ));
        }
        if let Some(fix) = continuity_fix(inference) {
            lines.push(format!(
                "  continuity fix [{}] confidence={:.3}",
                fix.code, fix.confidence
            ));
        }
    }

    lines.join("\n")
}

fn render_version_summary(
    version_id: &str,
    snapshot: Option<&GameSnapshot>,
    report: Option<&VersionDiffReportV2>,
    inference: Option<&InferenceReport>,
    mapping_proposal: Option<&crate::proposal::MappingProposalOutput>,
    patch_draft: Option<&crate::proposal::ProposalPatchDraftOutput>,
    human_summary: Option<&str>,
    artifacts: &[crate::report_storage::VersionArtifactEntry],
) -> String {
    let mut lines = vec![
        format!("Version: wuwa_{version_id}"),
        format!("Artifacts: {}", artifacts.len()),
    ];

    if let Some(snapshot) = snapshot {
        let scope = assess_snapshot_scope(snapshot);
        lines.push(format!(
            "Snapshot: assets={} source_root={}",
            snapshot.asset_count, snapshot.source_root
        ));
        lines.push(format!(
            "Snapshot scope: mode={} install_or_package_level={} meaningful_content={} meaningful_character={} content_like_paths={} character_paths={} non_content_paths={}",
            scope.capture_mode.as_deref().unwrap_or("unknown"),
            scope.mostly_install_or_package_level,
            scope.meaningful_content_coverage,
            scope.meaningful_character_coverage,
            scope.coverage.content_like_path_count,
            scope.coverage.character_path_count,
            scope.coverage.non_content_path_count
        ));
        if scope.is_low_signal_for_character_analysis() {
            lines.push(
                "Snapshot caution: install/package-level or low-coverage scope; deep character/resonator interpretation may be limited."
                    .to_string(),
            );
        }
        if let Some(note) = scope.note {
            lines.push(format!("Snapshot note: {note}"));
        }
        if let Some(launcher) = snapshot.context.launcher.as_ref() {
            lines.push(format!(
                "Launcher detected_version={} reuse_version={}",
                launcher.detected_version,
                launcher.reuse_version.as_deref().unwrap_or("-")
            ));
        }
        if let Some(manifest) = snapshot.context.resource_manifest.as_ref() {
            lines.push(format!(
                "Hash context: resources={} matched_assets={} unmatched_assets={}",
                manifest.resource_count,
                manifest.matched_assets,
                manifest.unmatched_snapshot_assets
            ));
        }
    } else {
        lines.push("Snapshot: not found".to_string());
    }

    if let Some(report) = report {
        lines.push(format!(
            "Latest report bundle: {} -> {} | changed={} added={} removed={} mapping_candidates={}",
            report.old_version.version_id,
            report.new_version.version_id,
            report.summary.changed_items,
            report.summary.added_items,
            report.summary.removed_items,
            report.summary.mapping_candidates
        ));
        if !report.scope_notes.is_empty() {
            for note in report.scope_notes.iter().take(3) {
                lines.push(format!("Report scope note: {note}"));
            }
        }
    } else {
        lines.push("Latest report bundle: not found".to_string());
    }

    if let Some(inference) = inference {
        lines.push(format!(
            "Inference: crash_causes={} fixes={} mapping_hints={} highest_confidence={:.3}",
            inference.summary.probable_crash_causes,
            inference.summary.suggested_fixes,
            inference.summary.candidate_mapping_hints,
            inference.summary.highest_confidence
        ));
        for cause in inference.probable_crash_causes.iter().take(3) {
            lines.push(format!(
                "  crash cause [{}] risk={:?} confidence={:.3} affected={}",
                cause.code,
                cause.risk,
                cause.confidence,
                cause.affected_assets.len()
            ));
        }
        for fix in inference.suggested_fixes.iter().take(3) {
            lines.push(format!(
                "  suggested fix [{}] priority={:?} confidence={:.3} actions={}",
                fix.code,
                fix.priority,
                fix.confidence,
                fix.actions.len()
            ));
        }
        let continuity_causes = continuity_causes(inference);
        if !continuity_causes.is_empty() || continuity_fix(inference).is_some() {
            lines.push(format!(
                "Continuity caution: unstable_causes={} review_fix={} ",
                continuity_causes.len(),
                yes_no(continuity_fix(inference).is_some())
            ));
            if let Some(cause) = continuity_causes.first() {
                lines.push(format!(
                    "  continuity cause [{}] risk={:?} confidence={:.3}",
                    cause.code, cause.risk, cause.confidence
                ));
            }
            if let Some(fix) = continuity_fix(inference) {
                lines.push(format!(
                    "  continuity fix [{}] priority={:?} confidence={:.3}",
                    fix.code, fix.priority, fix.confidence
                ));
            }
        }
    } else {
        lines.push("Inference: not found".to_string());
    }

    if let Some(mapping_proposal) = mapping_proposal {
        lines.push(format!(
            "Mapping proposal: proposed={} review={} total={} highest_confidence={:.3}",
            mapping_proposal.summary.proposed_mappings,
            mapping_proposal.summary.needs_review_mappings,
            mapping_proposal.summary.total_mapping_candidates,
            mapping_proposal.summary.highest_confidence
        ));
        for mapping in mapping_proposal.mappings.iter().take(5) {
            lines.push(format!(
                "  {} -> {} | status={:?} confidence={:.3}",
                mapping.old_asset_path, mapping.new_asset_path, mapping.status, mapping.confidence
            ));
        }
        let continuity_review_mappings = mapping_proposal
            .mappings
            .iter()
            .filter(|mapping| mapping_has_continuity_caution(mapping))
            .collect::<Vec<_>>();
        if !continuity_review_mappings.is_empty() {
            lines.push(format!(
                "  continuity-backed review mappings={}",
                continuity_review_mappings.len()
            ));
            for mapping in continuity_review_mappings.into_iter().take(2) {
                lines.push(format!(
                    "  continuity review {} -> {} | {}",
                    mapping.old_asset_path,
                    mapping.new_asset_path,
                    mapping_continuity_note(mapping).unwrap_or_else(|| {
                        "broader continuity history keeps this mapping review-first".to_string()
                    })
                ));
            }
        }
    } else {
        lines.push("Mapping proposal: not found".to_string());
    }

    if let Some(patch_draft) = patch_draft {
        lines.push(format!(
            "Patch draft: actions={} min_confidence={:.3}",
            patch_draft.actions.len(),
            patch_draft.min_confidence
        ));
    } else {
        lines.push("Patch draft: not found".to_string());
    }

    if let Some(summary) = human_summary {
        lines.push("Human summary preview:".to_string());
        for line in summary.lines().take(6) {
            lines.push(format!("  {line}"));
        }
    } else {
        lines.push("Human summary preview: not found".to_string());
    }

    lines.join("\n")
}

fn render_version_artifact_lines(
    artifacts: &[crate::report_storage::VersionArtifactEntry],
) -> Vec<String> {
    artifacts
        .iter()
        .map(|artifact| {
            format!(
                "{} | {}",
                artifact_kind_label(artifact.kind),
                artifact.path.display()
            )
        })
        .collect()
}

fn artifact_kind_label(kind: VersionArtifactKind) -> &'static str {
    match kind {
        VersionArtifactKind::Snapshot => "snapshot",
        VersionArtifactKind::ReportBundle => "report_bundle",
        VersionArtifactKind::ContinuityData => "continuity",
        VersionArtifactKind::InferenceData => "inference",
        VersionArtifactKind::ProposalData => "proposal",
        VersionArtifactKind::HumanSummary => "human_summary",
        VersionArtifactKind::ModDependencyBaselineSummary => "mod_dependency_baseline_summary",
        VersionArtifactKind::ExtractorInventory => "extractor_inventory",
        VersionArtifactKind::BufferData => "buffer",
        VersionArtifactKind::HashData => "hash",
        VersionArtifactKind::Auxiliary => "auxiliary",
        VersionArtifactKind::LegacySnapshot => "legacy_snapshot",
        VersionArtifactKind::LegacyReportBundle => "legacy_report_bundle",
    }
}

fn continuity_causes(inference: &InferenceReport) -> Vec<&crate::inference::ProbableCrashCause> {
    inference
        .probable_crash_causes
        .iter()
        .filter(|cause| cause.code == "continuity_thread_instability")
        .collect()
}

fn continuity_fix(inference: &InferenceReport) -> Option<&crate::inference::SuggestedFix> {
    inference
        .suggested_fixes
        .iter()
        .find(|fix| fix.code == "review_continuity_thread_history_before_repair")
}

fn mapping_has_continuity_caution(mapping: &crate::proposal::MappingProposalEntry) -> bool {
    mapping
        .continuity
        .as_ref()
        .is_some_and(|continuity| continuity.has_review_caution())
        || mapping_continuity_note(mapping).is_some()
}

fn mapping_continuity_note(mapping: &crate::proposal::MappingProposalEntry) -> Option<String> {
    mapping
        .continuity
        .as_ref()
        .and_then(structured_continuity_note)
        .or_else(|| {
            mapping
                .reasons
                .iter()
                .find(|reason| is_legacy_continuity_text(reason))
                .cloned()
        })
        .or_else(|| {
            mapping
                .evidence
                .iter()
                .find(|evidence| is_legacy_continuity_text(evidence))
                .cloned()
        })
}

fn structured_continuity_note(
    continuity: &crate::inference::InferredMappingContinuityContext,
) -> Option<String> {
    if continuity.total_layout_drift_steps >= 2 {
        Some(format!(
            "repeated layout drift across {} continuity step(s) keeps this mapping review-first",
            continuity.total_layout_drift_steps
        ))
    } else if let Some(relation) = continuity.terminal_relation.as_ref() {
        let timing = if continuity.terminal_after_current {
            "later terminal"
        } else {
            "terminal"
        };
        Some(format!(
            "{timing} state {} in {} keeps this mapping review-first",
            continuity_relation_label(relation),
            continuity.terminal_version.as_deref().unwrap_or("unknown")
        ))
    } else if continuity.review_required_history {
        Some("review-required thread history keeps this mapping review-first".to_string())
    } else {
        None
    }
}

fn is_legacy_continuity_text(value: &str) -> bool {
    value.contains("continuity")
        || value.contains("terminal state")
        || value.contains("review-required")
        || value.contains("repeated layout drift")
}

fn continuity_relation_label(relation: &crate::report::VersionContinuityRelation) -> &'static str {
    match relation {
        crate::report::VersionContinuityRelation::Persisted => "persisted",
        crate::report::VersionContinuityRelation::RenameOrRepath => "rename/repath",
        crate::report::VersionContinuityRelation::ContainerMovement => "container movement",
        crate::report::VersionContinuityRelation::LayoutDrift => "layout drift",
        crate::report::VersionContinuityRelation::Replacement => "replacement",
        crate::report::VersionContinuityRelation::Ambiguous => "ambiguous",
        crate::report::VersionContinuityRelation::InsufficientEvidence => "insufficient evidence",
        crate::report::VersionContinuityRelation::Removed => "removed",
    }
}

pub fn render_detail_view(
    report: &VersionDiffReportV2,
    resonator_filter: &str,
    hide_unchanged: bool,
) -> ReportDetailView {
    let filter = resonator_filter.to_ascii_lowercase();
    let resonators = report
        .resonators
        .iter()
        .filter(|entry| filter.is_empty() || entry.resonator.to_ascii_lowercase().contains(&filter))
        .collect::<Vec<_>>();

    let mut summary = format!(
        "Compare {} -> {}\nResonators: {}\nUnchanged: {}\nChanged: {}\nAdded: {}\nRemoved: {}\nUncertain: {}\nMapping candidates: {}",
        report.old_version.version_id,
        report.new_version.version_id,
        report.summary.resonator_count,
        report.summary.unchanged_items,
        report.summary.changed_items,
        report.summary.added_items,
        report.summary.removed_items,
        report.summary.uncertain_items,
        report.summary.mapping_candidates
    );
    if hide_unchanged && report.summary.unchanged_items > 0 {
        summary.push_str(&format!(
            "\nFilter: hiding {} unchanged item(s) — toggle 'Show unchanged' to see them.",
            report.summary.unchanged_items
        ));
    }
    if !report.scope_notes.is_empty() {
        for note in report.scope_notes.iter().take(2) {
            summary.push_str(&format!("\nScope note: {note}"));
        }
    }

    ReportDetailView {
        summary,
        table_rows: build_compare_table_rows(&resonators, hide_unchanged),
        quality_gate_text: String::new(),
        inference_text: String::new(),
        proposal_text: String::new(),
        human_summary_text: String::new(),
    }
}

fn build_compare_table_rows(
    resonators: &[&ResonatorDiffEntry],
    hide_unchanged: bool,
) -> Vec<CompareTableRow> {
    const MAX_ROWS: usize = 400;

    let mut rows = Vec::new();
    let mut truncated: usize = 0;

    for entry in resonators {
        for item in &entry.items {
            if hide_unchanged && matches!(item.status, DiffStatus::Unchanged) {
                continue;
            }
            if rows.len() >= MAX_ROWS {
                truncated += 1;
                continue;
            }

            let path = item
                .new
                .as_ref()
                .or(item.old.as_ref())
                .map(|side| side.path.clone().unwrap_or_else(|| side.label.clone()))
                .unwrap_or_else(|| "-".to_string());
            let confidence = item
                .confidence
                .map(|value| format!("{value:.3}"))
                .unwrap_or_else(|| "-".to_string());

            let asset_hash = format_hash_transition(
                item.old.as_ref().and_then(|s| s.metadata.asset_hash.as_deref()),
                item.new.as_ref().and_then(|s| s.metadata.asset_hash.as_deref()),
            );
            let shader_hash = format_hash_transition(
                item.old.as_ref().and_then(|s| s.metadata.shader_hash.as_deref()),
                item.new.as_ref().and_then(|s| s.metadata.shader_hash.as_deref()),
            );

            rows.push(CompareTableRow {
                resonator: entry.resonator.clone(),
                item_type: format!("{:?}", item.item_type),
                status: format!("{:?}", item.status),
                confidence,
                path,
                asset_hash,
                shader_hash,
            });
        }
    }

    if truncated > 0 {
        rows.push(CompareTableRow {
            resonator: "…".to_string(),
            item_type: "truncated".to_string(),
            status: format!("+{truncated}"),
            confidence: "-".to_string(),
            path: "Open the stored report bundle for the full list.".to_string(),
            asset_hash: String::new(),
            shader_hash: String::new(),
        });
    }

    rows
}

fn format_hash_transition(old_hash: Option<&str>, new_hash: Option<&str>) -> String {
    match (old_hash, new_hash) {
        (None, None) => "-".to_string(),
        (Some(hash), None) => format!("{hash} → (removed)"),
        (None, Some(hash)) => format!("(added) → {hash}"),
        (Some(old), Some(new)) if old == new => old.to_string(),
        (Some(old), Some(new)) => format!("{old} → {new}"),
    }
}

fn render_quality_gate_preview(report: &VersionDiffReportV2) -> String {
    let mut lines = vec![format!(
        "Quality / scope signals for {} -> {}",
        report.old_version.version_id, report.new_version.version_id
    )];

    let scope_notes = &report.scope_notes;
    let mut low_signal_found = false;
    for note in scope_notes {
        let lower = note.to_ascii_lowercase();
        if lower.contains("low-signal")
            || lower.contains("shallow")
            || lower.contains("low-coverage")
            || lower.contains("install/package-level")
        {
            low_signal_found = true;
            lines.push(format!("WARN: {note}"));
        }
    }
    if !low_signal_found {
        lines.push("No low-signal warnings detected from compare scope notes.".to_string());
    }

    for note in scope_notes
        .iter()
        .filter(|note| note.to_ascii_lowercase().contains("selected baseline"))
        .take(2)
    {
        lines.push(format!("Baseline: {note}"));
    }

    lines.join("\n")
}

fn render_inference_preview(inference: Option<&InferenceReport>) -> String {
    let Some(inference) = inference else {
        return "Inference preview: not stored yet for the new version. Run Scan Version first so Phase 3 artifacts are persisted.".to_string();
    };

    let mut lines = vec![format!(
        "Inference: crash_causes={} fixes={} mapping_hints={} highest_confidence={:.3}",
        inference.summary.probable_crash_causes,
        inference.summary.suggested_fixes,
        inference.summary.candidate_mapping_hints,
        inference.summary.highest_confidence
    )];
    for cause in inference.probable_crash_causes.iter().take(5) {
        lines.push(format!(
            "- cause [{}] risk={:?} confidence={:.3} affected={}",
            cause.code,
            cause.risk,
            cause.confidence,
            cause.affected_assets.len()
        ));
    }
    for fix in inference.suggested_fixes.iter().take(5) {
        lines.push(format!(
            "- fix   [{}] priority={:?} confidence={:.3} actions={}",
            fix.code,
            fix.priority,
            fix.confidence,
            fix.actions.len()
        ));
    }
    lines.join("\n")
}

fn render_proposal_preview(
    mapping_proposal: Option<&crate::proposal::MappingProposalOutput>,
    patch_draft: Option<&crate::proposal::ProposalPatchDraftOutput>,
) -> String {
    let Some(mapping_proposal) = mapping_proposal else {
        return "Mapping proposal preview: not stored yet for the new version.".to_string();
    };

    let mut lines = vec![format!(
        "Mapping proposal: proposed={} review={} total={} highest_confidence={:.3}",
        mapping_proposal.summary.proposed_mappings,
        mapping_proposal.summary.needs_review_mappings,
        mapping_proposal.summary.total_mapping_candidates,
        mapping_proposal.summary.highest_confidence
    )];
    for mapping in mapping_proposal.mappings.iter().take(5) {
        lines.push(format!(
            "- {} -> {} | status={:?} confidence={:.3}",
            mapping.old_asset_path, mapping.new_asset_path, mapping.status, mapping.confidence
        ));
    }
    if let Some(patch_draft) = patch_draft {
        lines.push(format!(
            "Patch draft: actions={} min_confidence={:.3}",
            patch_draft.actions.len(),
            patch_draft.min_confidence
        ));
    }
    lines.join("\n")
}

fn render_human_summary_preview(summary: Option<&str>) -> String {
    let Some(summary) = summary else {
        return "Human summary preview: not stored yet for the new version.".to_string();
    };
    let mut lines = Vec::new();
    for line in summary.lines().take(30) {
        lines.push(line.to_string());
    }
    if summary.lines().count() > 30 {
        lines.push("... (human summary truncated in GUI preview; open the stored .md file for the full text)".to_string());
    }
    lines.join("\n")
}


#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use crate::{
        domain::AssetMetadata,
        inference::{
            InferenceCompareInput, InferenceKnowledgeInput, InferenceReport, InferenceScopeContext,
            InferenceSummary, InferredMappingContinuityContext, InferredMappingHint,
            ProbableCrashCause, SuggestedFix,
        },
        proposal::ProposalEngine,
        report::{
            DiffStatus, ReportItemType, ReportReason, ResonatorDiffEntry, ResonatorItemDiff,
            ResonatorVersionView, TechnicalMetadata, VersionContinuityRelation,
            VersionDiffReportV2, VersionDiffSummary, VersionSide, VersionedItem,
        },
        report_storage::ReportStorage,
        snapshot::{
            GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotFingerprint, SnapshotHashFields,
        },
        wwmi::{
            WwmiEvidenceCommit, WwmiFixPattern, WwmiKeywordStat, WwmiKnowledgeBase,
            WwmiKnowledgeRepoInfo, WwmiKnowledgeSummary, WwmiPatternKind,
        },
    };

    use super::{GuiController, ScanForm, ScanRunResult, render_detail_view};

    #[test]
    fn detail_view_renders_table_rows_for_resonator_items() {
        let report = VersionDiffReportV2 {
            schema_version: "whashreonator.report.v2".to_string(),
            generated_at_unix_ms: 1,
            old_version: VersionSide {
                version_id: "2.4.0".to_string(),
                source_root: "old".to_string(),
                asset_count: 1,
            },
            new_version: VersionSide {
                version_id: "2.5.0".to_string(),
                source_root: "new".to_string(),
                asset_count: 1,
            },
            lineage: Default::default(),
            summary: VersionDiffSummary {
                resonator_count: 1,
                unchanged_items: 0,
                changed_items: 1,
                added_items: 0,
                removed_items: 0,
                uncertain_items: 0,
                mapping_candidates: 0,
            },
            resonators: vec![ResonatorDiffEntry {
                resonator: "Encore".to_string(),
                old_version: ResonatorVersionView {
                    asset_count: 1,
                    buffer_count: 1,
                    mapping_count: 0,
                },
                new_version: ResonatorVersionView {
                    asset_count: 1,
                    buffer_count: 1,
                    mapping_count: 0,
                },
                items: vec![ResonatorItemDiff {
                    item_type: ReportItemType::Asset,
                    status: DiffStatus::Changed,
                    confidence: Some(0.9),
                    compatibility: None,
                    old: Some(item("Content/Character/Encore/Body.mesh")),
                    new: Some(item("Content/Character/Encore/Body_v2.mesh")),
                    reasons: vec![ReportReason {
                        code: "vertex_count_changed".to_string(),
                        message: "vertex count changed".to_string(),
                    }],
                }],
            }],
            scope_notes: vec![
                "scope warning: compare results are based on install/package-level snapshots"
                    .to_string(),
            ],
            review: Default::default(),
        };

        let detail = render_detail_view(&report, "", false);
        assert!(detail.summary.contains("Compare 2.4.0 -> 2.5.0"));
        assert!(detail.summary.contains("Scope note: scope warning"));
        assert_eq!(detail.table_rows.len(), 1);
        let row = &detail.table_rows[0];
        assert_eq!(row.resonator, "Encore");
        assert_eq!(row.item_type, "Asset");
        assert_eq!(row.status, "Changed");
        assert_eq!(row.path, "Content/Character/Encore/Body_v2.mesh");
    }

    #[test]
    fn controller_lists_version_after_scan_flow() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::with_legacy_root(
            test_root.join("out").join("report"),
            test_root.join("out").join("reports"),
        );
        let controller = GuiController::new(storage);
        let game_root = test_root.join("game");
        seed_game_root(&game_root, "3.2.1");

        let prepare = controller
            .prepare_scan(&ScanForm {
                source_root: game_root.display().to_string(),
                version_override: String::new(),
                knowledge_path: String::new(),
            })
            .expect("prepare scan");
        let prepared = match prepare {
            super::ScanStartResult::Ready(prepared) => prepared,
            other => panic!("expected ready, got {other:?}"),
        };

        let result = controller.run_scan(&prepared, false, "").expect("run scan");
        let summary = match result {
            ScanRunResult::Created { summary, .. } => summary,
            other => panic!("expected created, got {other:?}"),
        };
        assert!(summary.contains("Snapshot scope: mode="));
        assert!(summary.contains("Coverage signals:"));

        let versions = controller.list_versions().expect("list versions");
        assert!(versions.iter().any(|version| version.version_id == "3.2.1"));

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn controller_scan_uses_full_inventory_for_version_library_flow() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::with_legacy_root(
            test_root.join("out").join("report"),
            test_root.join("out").join("reports"),
        );
        let controller = GuiController::new(storage);
        let game_root = test_root.join("game");
        seed_game_root(&game_root, "3.2.2");
        let non_content_path = game_root
            .join("Client")
            .join("Binaries")
            .join("Win64")
            .join("Game.exe");
        fs::create_dir_all(non_content_path.parent().expect("non-content parent"))
            .expect("create non-content parent");
        fs::write(&non_content_path, b"exe").expect("write non-content asset");

        let prepare = controller
            .prepare_scan(&ScanForm {
                source_root: game_root.display().to_string(),
                version_override: String::new(),
                knowledge_path: String::new(),
            })
            .expect("prepare scan");
        let prepared = match prepare {
            super::ScanStartResult::Ready(prepared) => prepared,
            other => panic!("expected ready, got {other:?}"),
        };

        let result = controller.run_scan(&prepared, false, "").expect("run scan");
        let summary = match result {
            ScanRunResult::Created { summary, .. } => summary,
            other => panic!("expected created, got {other:?}"),
        };

        assert!(summary.contains("mode=local_filesystem_inventory"));
        assert!(summary.contains("non_content_paths=2"));

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn controller_compares_two_versions_from_storage() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::with_legacy_root(
            test_root.join("out").join("report"),
            test_root.join("out").join("reports"),
        );
        storage
            .save_snapshot_for_version(&sample_snapshot("3.2.5", 1))
            .expect("save 3.2.5");
        std::thread::sleep(Duration::from_millis(2));
        storage
            .save_snapshot_for_version(&sample_snapshot("3.3.1", 2))
            .expect("save 3.3.1");
        let controller = GuiController::new(storage);

        let detail = controller
            .compare_versions("3.2.5", "3.3.1", "", false)
            .expect("compare");
        assert!(detail.summary.contains("Compare 3.2.5 -> 3.3.1"));
        assert!(detail.summary.contains("Scope note:"));

        let _ = fs::remove_dir_all(test_root);
    }

    #[test]
    fn phase3_generation_summary_surfaces_continuity_caution() {
        let inference = continuity_flagged_inference();
        let proposals = ProposalEngine.generate(&inference, 0.85);

        let summary = super::render_phase3_generation_summary(
            "8.0.0",
            "8.1.0",
            &inference,
            &proposals,
            false,
            &[],
        );

        assert!(summary.contains("Phase 3 continuity caution"));
        assert!(summary.contains("review-first"));
        assert!(summary.contains("continuity cause [continuity_thread_instability]"));
        assert!(
            summary.contains("continuity fix [review_continuity_thread_history_before_repair]")
        );
    }

    #[test]
    fn version_summary_surfaces_continuity_backed_review_behavior() {
        let inference = continuity_flagged_inference();
        let proposals = ProposalEngine.generate(&inference, 0.85);

        let summary = super::render_version_summary(
            "8.1.0",
            None,
            None,
            Some(&inference),
            Some(&proposals.mapping_proposal),
            Some(&proposals.patch_draft),
            Some("# summary"),
            &[],
        );

        assert!(summary.contains("Continuity caution: unstable_causes=1 review_fix=yes"));
        assert!(summary.contains("continuity cause [continuity_thread_instability]"));
        assert!(summary.contains("continuity-backed review mappings=1"));
        assert!(summary.contains("continuity review Content/Character/Encore/Body.mesh -> Content/Character/Encore/Body_v2.mesh"));
    }

    #[test]
    fn scan_auto_generates_phase3_outputs_and_open_version_renders_them() {
        let test_root = unique_test_dir();
        let storage = ReportStorage::with_legacy_root(
            test_root.join("out").join("report"),
            test_root.join("out").join("reports"),
        );
        let controller = GuiController::new(storage.clone());
        storage
            .save_snapshot_for_version(&sample_snapshot("3.2.1", 1))
            .expect("seed baseline snapshot");
        let game_root = test_root.join("game");
        seed_game_root(&game_root, "3.3.1");
        let knowledge_path = test_root.join("out").join("wwmi-knowledge.json");
        fs::create_dir_all(knowledge_path.parent().expect("knowledge parent"))
            .expect("create knowledge dir");
        fs::write(
            &knowledge_path,
            serde_json::to_string_pretty(&sample_knowledge()).expect("serialize knowledge"),
        )
        .expect("write knowledge");

        let prepare = controller
            .prepare_scan(&ScanForm {
                source_root: game_root.display().to_string(),
                version_override: String::new(),
                knowledge_path: knowledge_path.display().to_string(),
            })
            .expect("prepare scan");
        let prepared = match prepare {
            super::ScanStartResult::Ready(prepared) => prepared,
            other => panic!("expected ready, got {other:?}"),
        };

        let result = controller
            .run_scan(&prepared, false, &knowledge_path.display().to_string())
            .expect("run scan with phase3");
        let summary = match result {
            ScanRunResult::Created { summary, .. } => summary,
            other => panic!("expected created result, got {other:?}"),
        };
        assert!(summary.contains("Phase 3 generated using baseline 3.2.1 -> 3.3.1"));
        assert!(summary.contains("Snapshot scope: mode="));
        assert!(summary.contains("Phase 3 caution: compare scope is install/package-level"));

        let detail = controller.open_version("3.3.1").expect("open version");
        assert!(detail.summary.contains("Snapshot scope: mode="));
        assert!(detail.summary.contains("Inference: crash_causes="));
        assert!(detail.summary.contains("Mapping proposal: proposed="));
        assert!(detail.summary.contains("Human summary preview:"));
        assert!(
            detail
                .artifacts
                .iter()
                .any(|line| line.contains("inference |"))
        );
        assert!(
            detail
                .artifacts
                .iter()
                .any(|line| line.contains("proposal |"))
        );
        assert!(
            detail
                .artifacts
                .iter()
                .any(|line| line.contains("human_summary |"))
        );

        let mapping_proposal = storage
            .load_latest_mapping_proposal("3.3.1")
            .expect("load mapping proposal")
            .expect("mapping proposal exists");
        assert_eq!(
            mapping_proposal.schema_version,
            "whashreonator.mapping-proposal.v1"
        );
        let patch_draft = storage
            .load_latest_patch_draft("3.3.1")
            .expect("load patch draft")
            .expect("patch draft exists");
        assert_eq!(
            patch_draft.schema_version,
            "whashreonator.proposal-patch-draft.v1"
        );
        assert!(patch_draft.actions.len() >= mapping_proposal.mappings.len());

        let _ = fs::remove_dir_all(test_root);
    }

    fn seed_game_root(root: &Path, version: &str) {
        let asset_path = root
            .join("Content")
            .join("Character")
            .join("Encore")
            .join("Body.mesh");
        fs::create_dir_all(asset_path.parent().expect("asset parent"))
            .expect("create asset parent");
        fs::write(asset_path, b"asset").expect("write asset");
        fs::write(
            root.join("launcherDownloadConfig.json"),
            format!(
                r#"{{"version":"{version}","reUseVersion":"","state":"ready","isPreDownload":false,"appId":"50004"}}"#
            ),
        )
        .expect("write launcher config");
    }

    fn sample_snapshot(version_id: &str, vertex_count: u32) -> GameSnapshot {
        GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.to_string(),
            created_at_unix_ms: 1,
            source_root: "root".to_string(),
            asset_count: 1,
            assets: vec![SnapshotAsset {
                id: "asset-1".to_string(),
                path: "Content/Character/Encore/Body.mesh".to_string(),
                kind: Some("mesh".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some("body".to_string()),
                    vertex_count: Some(vertex_count),
                    index_count: Some(1),
                    material_slots: Some(1),
                    section_count: Some(1),
                    tags: Vec::new(),
                    ..Default::default()
                },
                fingerprint: SnapshotFingerprint {
                    normalized_kind: Some("mesh".to_string()),
                    normalized_name: Some("body".to_string()),
                    name_tokens: vec!["body".to_string()],
                    path_tokens: vec!["content".to_string()],
                    tags: Vec::new(),
                    vertex_count: Some(vertex_count),
                    index_count: Some(1),
                    material_slots: Some(1),
                    section_count: Some(1),
                    ..Default::default()
                },
                hash_fields: SnapshotHashFields::default(),
                source: crate::domain::AssetSourceContext::default(),
            }],
            context: SnapshotContext::default(),
        }
    }

    fn item(path: &str) -> VersionedItem {
        VersionedItem {
            key: path.to_string(),
            label: path.to_string(),
            path: Some(path.to_string()),
            metadata: TechnicalMetadata::default(),
        }
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();

        std::env::temp_dir().join(format!("whashreonator-gui-app-test-{nanos}"))
    }

    fn sample_knowledge() -> WwmiKnowledgeBase {
        WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: 1,
            repo: WwmiKnowledgeRepoInfo {
                input: "repo".to_string(),
                resolved_path: "repo".to_string(),
                origin_url: None,
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: 4,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            patterns: vec![
                WwmiFixPattern {
                    kind: WwmiPatternKind::MappingOrHashUpdate,
                    description: "mapping".to_string(),
                    frequency: 2,
                    average_fix_likelihood: 0.8,
                    example_commits: vec!["abc".to_string()],
                },
                WwmiFixPattern {
                    kind: WwmiPatternKind::BufferLayoutOrCapacityFix,
                    description: "buffer".to_string(),
                    frequency: 1,
                    average_fix_likelihood: 0.7,
                    example_commits: vec!["def".to_string()],
                },
            ],
            keyword_stats: vec![WwmiKeywordStat {
                keyword: "mapping".to_string(),
                count: 2,
            }],
            evidence_commits: vec![WwmiEvidenceCommit {
                hash: "abc".to_string(),
                subject: "fix mapping".to_string(),
                unix_time: 1,
                decorations: String::new(),
                commit_url: None,
                fix_likelihood: 0.8,
                changed_files: vec!["WWMI/d3dx.ini".to_string()],
                detected_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                detected_keywords: vec!["mapping".to_string()],
                reasons: vec!["subject contains fix".to_string()],
            }],
        }
    }

    fn continuity_flagged_inference() -> InferenceReport {
        InferenceReport {
            schema_version: "whashreonator.inference.v1".to_string(),
            generated_at_unix_ms: 1,
            compare_input: InferenceCompareInput {
                old_version_id: "8.0.0".to_string(),
                new_version_id: "8.1.0".to_string(),
                changed_assets: 0,
                added_assets: 1,
                removed_assets: 1,
                candidate_mapping_changes: 1,
            },
            knowledge_input: InferenceKnowledgeInput {
                repo: "repo".to_string(),
                analyzed_commits: 4,
                fix_like_commits: 2,
                discovered_patterns: 2,
            },
            mod_dependency_input: None,
            representative_mod_baseline_input: None,
            scope: InferenceScopeContext::default(),
            summary: InferenceSummary {
                probable_crash_causes: 1,
                suggested_fixes: 1,
                candidate_mapping_hints: 1,
                highest_confidence: 0.90,
            },
            probable_crash_causes: vec![ProbableCrashCause {
                code: "continuity_thread_instability".to_string(),
                summary: "broader continuity history is unstable".to_string(),
                confidence: 0.83,
                risk: crate::compare::RiskLevel::High,
                affected_assets: vec!["Content/Character/Encore/Body.mesh".to_string()],
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                reasons: vec!["continuity surfaces unstable thread history".to_string()],
                evidence: vec![
                    "continuity thread Content/Character/Encore/Body_v2.mesh spans 7.0.0 -> 8.2.0"
                        .to_string(),
                ],
            }],
            suggested_fixes: vec![SuggestedFix {
                code: "review_continuity_thread_history_before_repair".to_string(),
                summary: "review broader continuity history".to_string(),
                confidence: 0.81,
                priority: crate::compare::RiskLevel::High,
                related_patterns: vec![WwmiPatternKind::MappingOrHashUpdate],
                actions: vec!["inspect continuity milestones".to_string()],
                reasons: vec!["thread later terminates".to_string()],
                evidence: vec![
                    "continuity thread later terminates as removed in 8.2.0".to_string(),
                ],
            }],
            candidate_mapping_hints: vec![InferredMappingHint {
                old_asset_path: "Content/Character/Encore/Body.mesh".to_string(),
                new_asset_path: "Content/Character/Encore/Body_v2.mesh".to_string(),
                confidence: 0.90,
                compatibility: crate::compare::RemapCompatibility::CompatibleWithCaution,
                needs_review: true,
                ambiguous: false,
                confidence_gap: Some(0.18),
                continuity: Some(InferredMappingContinuityContext {
                    thread_id: Some("encore_body".to_string()),
                    first_seen_version: Some("7.0.0".to_string()),
                    latest_observed_version: Some("8.2.0".to_string()),
                    latest_live_version: None,
                    stable_before_current_change: false,
                    total_rename_steps: 1,
                    total_container_movement_steps: 0,
                    total_layout_drift_steps: 0,
                    review_required_history: true,
                    terminal_relation: Some(VersionContinuityRelation::Removed),
                    terminal_version: Some("8.2.0".to_string()),
                    terminal_after_current: true,
                    instability_detected: true,
                }),
                reasons: vec!["same_parent_directory: same folder".to_string()],
                evidence: vec!["compare candidate confidence 0.900".to_string()],
            }],
            surface_intersection: Default::default(),
            representative_risk_projections: Vec::new(),
        }
    }
}
