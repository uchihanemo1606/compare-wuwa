use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "whashreonator",
    version,
    about = "Semi-automated hash mapping pipeline for Wuthering Waves / WWMI"
)]
pub struct Cli {
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Map(MapArgs),
    MapLocal(MapLocalArgs),
    Snapshot(SnapshotArgs),
    SnapshotReport(SnapshotReportArgs),
    CompareSnapshots(CompareSnapshotsArgs),
    ExtractWwmiKnowledge(ExtractWwmiKnowledgeArgs),
    InferFixes(InferFixesArgs),
    GenerateProposals(GenerateProposalsArgs),
}

#[derive(Debug, Clone, Args)]
pub struct MapArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct MapLocalArgs {
    #[arg(long)]
    pub old_root: PathBuf,
    #[arg(long)]
    pub new_root: PathBuf,
    #[arg(long)]
    pub report_output: Option<PathBuf>,
    #[arg(long)]
    pub mapping_output: Option<PathBuf>,
    #[arg(long)]
    pub patch_draft_output: Option<PathBuf>,
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args)]
pub struct SnapshotArgs {
    #[arg(long)]
    pub source_root: PathBuf,
    #[arg(long)]
    pub version_id: String,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long, value_enum, default_value_t = SnapshotCaptureScopeArg::Full)]
    pub capture_scope: SnapshotCaptureScopeArg,
    #[arg(long)]
    pub prepared_inventory: Option<PathBuf>,
    #[arg(long)]
    pub store_in_report: bool,
    #[arg(long)]
    pub report_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SnapshotCaptureScopeArg {
    Full,
    Content,
    Character,
    Prepared,
}

#[derive(Debug, Clone, Args)]
pub struct SnapshotReportArgs {
    #[arg(long = "snapshot", required = true)]
    pub snapshots: Vec<PathBuf>,
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct CompareSnapshotsArgs {
    #[arg(long)]
    pub old_snapshot: PathBuf,
    #[arg(long)]
    pub new_snapshot: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct ExtractWwmiKnowledgeArgs {
    #[arg(long)]
    pub repo: String,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long, default_value_t = 200)]
    pub max_commits: usize,
}

#[derive(Debug, Clone, Args)]
pub struct InferFixesArgs {
    #[arg(long)]
    pub compare_report: PathBuf,
    #[arg(long)]
    pub wwmi_knowledge: PathBuf,
    #[arg(long)]
    pub continuity_artifact: Option<PathBuf>,
    #[arg(long)]
    pub report_root: Option<PathBuf>,
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct GenerateProposalsArgs {
    #[arg(long)]
    pub inference_report: PathBuf,
    #[arg(long)]
    pub mapping_output: Option<PathBuf>,
    #[arg(long)]
    pub patch_draft_output: Option<PathBuf>,
    #[arg(long)]
    pub summary_output: Option<PathBuf>,
    #[arg(long, default_value_t = 0.85)]
    pub min_confidence: f32,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command, SnapshotCaptureScopeArg};

    #[test]
    fn snapshot_command_defaults_capture_scope_to_full() {
        let cli = Cli::parse_from([
            "whashreonator",
            "snapshot",
            "--source-root",
            "D:/fake-game",
            "--version-id",
            "2.4.0",
            "--output",
            "out/snapshot.json",
        ]);

        let Command::Snapshot(args) = cli.command else {
            panic!("expected snapshot command");
        };

        assert_eq!(args.capture_scope, SnapshotCaptureScopeArg::Full);
        assert!(args.prepared_inventory.is_none());
        assert!(!args.store_in_report);
        assert!(args.report_root.is_none());
    }

    #[test]
    fn snapshot_command_parses_capture_scope_option() {
        let cli = Cli::parse_from([
            "whashreonator",
            "snapshot",
            "--source-root",
            "D:/fake-game",
            "--version-id",
            "2.4.0",
            "--output",
            "out/snapshot.json",
            "--capture-scope",
            "character",
        ]);

        let Command::Snapshot(args) = cli.command else {
            panic!("expected snapshot command");
        };

        assert_eq!(args.capture_scope, SnapshotCaptureScopeArg::Character);
    }

    #[test]
    fn snapshot_command_parses_prepared_capture_and_storage_flags() {
        let cli = Cli::parse_from([
            "whashreonator",
            "snapshot",
            "--source-root",
            "D:/prepared-game",
            "--version-id",
            "6.1.0",
            "--output",
            "out/snapshot.json",
            "--capture-scope",
            "prepared",
            "--prepared-inventory",
            "out/prepared-assets.json",
            "--store-in-report",
            "--report-root",
            "out/report",
        ]);

        let Command::Snapshot(args) = cli.command else {
            panic!("expected snapshot command");
        };

        assert_eq!(args.capture_scope, SnapshotCaptureScopeArg::Prepared);
        assert_eq!(
            args.prepared_inventory.as_deref(),
            Some(PathBuf::from("out/prepared-assets.json").as_path())
        );
        assert!(args.store_in_report);
        assert_eq!(
            args.report_root.as_deref(),
            Some(PathBuf::from("out/report").as_path())
        );
    }

    #[test]
    fn infer_fixes_command_parses_optional_continuity_inputs() {
        let cli = Cli::parse_from([
            "whashreonator",
            "infer-fixes",
            "--compare-report",
            "out/compare.json",
            "--wwmi-knowledge",
            "out/knowledge.json",
            "--continuity-artifact",
            "out/continuity.json",
            "--report-root",
            "out/report",
            "--output",
            "out/inference.json",
        ]);

        let Command::InferFixes(args) = cli.command else {
            panic!("expected infer-fixes command");
        };

        assert_eq!(
            args.continuity_artifact.as_deref(),
            Some(PathBuf::from("out/continuity.json").as_path())
        );
        assert_eq!(
            args.report_root.as_deref(),
            Some(PathBuf::from("out/report").as_path())
        );
    }
}
