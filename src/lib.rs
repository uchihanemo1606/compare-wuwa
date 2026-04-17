pub mod cli;
pub mod compare;
pub mod config;
pub mod domain;
pub mod error;
pub mod export;
pub mod fingerprint;
pub mod gui_app;
pub mod human_summary;
pub mod inference;
pub mod ingest;
pub mod matcher;
pub mod output_policy;
pub mod pipeline;
pub mod proposal;
pub mod repo;
pub mod report;
pub mod report_storage;
pub mod scan;
pub mod snapshot;
pub mod snapshot_report;
pub mod validator;
pub mod wwmi;

use cli::{Cli, Command};
use error::AppResult;
use pipeline::{ProposalOutputs, SourceMapOutcome, SourceMapOutputs};

pub fn run(cli: Cli) -> AppResult<()> {
    match cli.command {
        Command::Map(args) => {
            pipeline::run_map_command(&args)?;
        }
        Command::Snapshot(args) => {
            let result = pipeline::run_snapshot_command(&args)?;
            println!(
                "snapshot exported: version={} assets={}",
                result.snapshot.version_id, result.snapshot.asset_count
            );
            println!("wrote snapshot: {}", args.output.display());
            if let Some(path) = result.stored_snapshot_path.as_deref() {
                println!("stored snapshot: {}", path.display());
            }
            if let Some(path) = result.stored_extractor_inventory_path.as_deref() {
                println!("stored extractor-inventory: {}", path.display());
            }
        }
        Command::SnapshotReport(args) => {
            let report = pipeline::run_snapshot_report_command(&args)?;
            println!(
                "snapshot report exported: versions={} resonators={} pairwise_compares={}",
                report.version_count, report.resonator_count, report.pair_count
            );
            println!("wrote snapshot-report: {}", args.output.display());
        }
        Command::CompareSnapshots(args) => {
            let report = pipeline::run_compare_snapshots_command(&args)?;
            println!(
                "snapshot compare exported: old={} new={} changed={} added={} removed={} mapping_candidates={}",
                report.old_snapshot.version_id,
                report.new_snapshot.version_id,
                report.summary.changed_assets,
                report.summary.added_assets,
                report.summary.removed_assets,
                report.summary.candidate_mapping_changes
            );
            println!("wrote compare-report: {}", args.output.display());
        }
        Command::OrchestrateVersionPair(args) => {
            let result = pipeline::run_orchestrate_version_pair_command(&args)?;
            println!(
                "version-pair orchestration exported: old={} new={} changed={} added={} removed={} causes={} fixes={} mapping_hints={} proposed={} needs_review={}",
                result.manifest.old_version_id,
                result.manifest.new_version_id,
                result.manifest.summary.changed_assets,
                result.manifest.summary.added_assets,
                result.manifest.summary.removed_assets,
                result.manifest.summary.probable_crash_causes,
                result.manifest.summary.suggested_fixes,
                result.manifest.summary.candidate_mapping_hints,
                result.manifest.summary.proposed_mappings,
                result.manifest.summary.needs_review_mappings,
            );
            println!(
                "wrote compare-report: {}",
                result.manifest.produced_artifacts.compare_report.display()
            );
            println!(
                "wrote inference-report: {}",
                result
                    .manifest
                    .produced_artifacts
                    .inference_report
                    .display()
            );
            println!(
                "wrote mapping-proposal: {}",
                result
                    .manifest
                    .produced_artifacts
                    .mapping_proposal
                    .display()
            );
            println!(
                "wrote proposal-patch-draft: {}",
                result.manifest.produced_artifacts.patch_draft.display()
            );
            println!(
                "wrote summary: {}",
                result.manifest.produced_artifacts.human_summary.display()
            );
            println!(
                "wrote manifest: {}",
                result.manifest.produced_artifacts.manifest.display()
            );
        }
        Command::ScanModDependencies(args) => {
            let result = pipeline::run_scan_mod_dependencies_command(&args)?;
            let surfaces = result.baseline_set.represented_surface_labels().join(", ");
            println!(
                "mod dependency baselines exported: version={} profiles={} mods={} surfaces={} strength={:?} material_for_review={}",
                result.baseline_set.version_id,
                result.baseline_set.profile_count,
                result.baseline_set.review.included_mod_count,
                if surfaces.is_empty() {
                    "none"
                } else {
                    surfaces.as_str()
                },
                result.baseline_set.review.strength,
                if result.baseline_set.review.material_for_repair_review {
                    "yes"
                } else {
                    "no"
                }
            );
            println!("wrote mod-dependency-baselines: {}", args.output.display());
            if let Some(path) = result.stored_baseline_set_path.as_deref() {
                println!("stored mod-dependency-baselines: {}", path.display());
            }
            if let Some(path) = result.stored_baseline_summary_path.as_deref() {
                println!("stored mod-dependency-baseline-summary: {}", path.display());
            }
            for path in &result.stored_profile_paths {
                println!("stored mod-dependency-profile: {}", path.display());
            }
            for note in &result.baseline_set.review.caution_notes {
                println!("mod-dependency-baseline-caution: {note}");
            }
        }
        Command::ExtractWwmiKnowledge(args) => {
            let knowledge = pipeline::run_extract_wwmi_knowledge_command(&args)?;
            println!(
                "wwmi knowledge exported: analyzed_commits={} fix_like_commits={} patterns={}",
                knowledge.summary.analyzed_commits,
                knowledge.summary.fix_like_commits,
                knowledge.summary.discovered_patterns
            );
            println!("wrote wwmi-knowledge: {}", args.output.display());
        }
        Command::InferFixes(args) => {
            let report = pipeline::run_infer_fixes_command(&args)?;
            println!(
                "inference exported: crash_causes={} suggested_fixes={} mapping_hints={} highest_confidence={:.3}",
                report.summary.probable_crash_causes,
                report.summary.suggested_fixes,
                report.summary.candidate_mapping_hints,
                report.summary.highest_confidence
            );
            println!("wrote inference-report: {}", args.output.display());
        }
        Command::GenerateProposals(args) => {
            let result = pipeline::run_generate_proposals_command(&args)?;
            println!(
                "proposal artifacts generated: proposed={} needs_review={} fix_actions={} highest_confidence={:.3}",
                result.artifacts.mapping_proposal.summary.proposed_mappings,
                result
                    .artifacts
                    .mapping_proposal
                    .summary
                    .needs_review_mappings,
                result
                    .artifacts
                    .mapping_proposal
                    .summary
                    .suggested_fix_actions,
                result.artifacts.mapping_proposal.summary.highest_confidence,
            );
            print_proposal_outputs("wrote", &result.outputs);
        }
        Command::MapLocal(args) => match pipeline::run_map_local_command(&args)? {
            SourceMapOutcome::DryRun(result) => {
                println!(
                    "dry-run summary: total={} matched={} needs_review={} rejected={}",
                    result.report.summary.total_old_assets,
                    result.report.summary.matched,
                    result.report.summary.needs_review,
                    result.report.summary.rejected
                );
                print_selected_outputs("would write", &result.outputs);
            }
            SourceMapOutcome::Exported(result) => {
                println!(
                    "report exported: total={} matched={} needs_review={} rejected={}",
                    result.report.summary.total_old_assets,
                    result.report.summary.matched,
                    result.report.summary.needs_review,
                    result.report.summary.rejected
                );
                print_selected_outputs("wrote", &result.outputs);
            }
        },
    }

    Ok(())
}

fn print_selected_outputs(prefix: &str, outputs: &SourceMapOutputs) {
    if let Some(path) = outputs.report_output.as_deref() {
        println!("{prefix} report: {}", path.display());
    }

    if let Some(path) = outputs.mapping_output.as_deref() {
        println!("{prefix} mapping: {}", path.display());
    }

    if let Some(path) = outputs.patch_draft_output.as_deref() {
        println!("{prefix} patch-draft: {}", path.display());
    }

    if outputs.is_empty() {
        println!("{prefix} outputs: none selected");
    }
}

fn print_proposal_outputs(prefix: &str, outputs: &ProposalOutputs) {
    if let Some(path) = outputs.mapping_output.as_deref() {
        println!("{prefix} mapping-proposal: {}", path.display());
    }

    if let Some(path) = outputs.patch_draft_output.as_deref() {
        println!("{prefix} proposal-patch-draft: {}", path.display());
    }

    if let Some(path) = outputs.summary_output.as_deref() {
        println!("{prefix} summary: {}", path.display());
    }

    if outputs.is_empty() {
        println!("{prefix} outputs: none selected");
    }
}
