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
        Command::ScanModDependencies(args) => {
            let result = pipeline::run_scan_mod_dependencies_command(&args)?;
            println!(
                "mod dependency baselines exported: version={} profiles={}",
                result.baseline_set.version_id, result.baseline_set.profile_count
            );
            println!("wrote mod-dependency-baselines: {}", args.output.display());
            if let Some(path) = result.stored_baseline_set_path.as_deref() {
                println!("stored mod-dependency-baselines: {}", path.display());
            }
            for path in &result.stored_profile_paths {
                println!("stored mod-dependency-profile: {}", path.display());
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
