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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SnapshotCaptureScopeArg {
    Full,
    Content,
    Character,
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
}
