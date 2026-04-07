use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use whashreonator::{
    cli::ExtractWwmiKnowledgeArgs,
    pipeline::run_extract_wwmi_knowledge_command,
    wwmi::{WwmiKnowledgeBase, WwmiPatternKind},
};

#[test]
fn extract_wwmi_knowledge_command_mines_fix_patterns_from_git_history() {
    let test_root = unique_test_dir();
    let repo_root = test_root.join("wwmi-repo");
    let output_path = test_root.join("out").join("wwmi-knowledge.json");

    init_git_repo(&repo_root);
    write_file(
        &repo_root,
        "WWMI/d3dx.ini",
        "[System]\ndll_initialization_delay = 50\nallow_buffer_resize = 0\n",
    );
    commit_all(&repo_root, "Initial import");

    write_file(
        &repo_root,
        "WWMI/d3dx.ini",
        "[System]\ndll_initialization_delay = 500\nallow_buffer_resize = 1\n",
    );
    commit_all(
        &repo_root,
        "Fixed startup crash by increasing dll_initialization_delay",
    );

    write_file(
        &repo_root,
        "WWMI/Core/WWMI/WuWa-Model-Importer.ini",
        "[Hashes]\nshapekey_hash = 123456\n",
    );
    write_file(
        &repo_root,
        "WWMI/Core/WWMI/Shaders/ShapeKeyOverrider.hlsl",
        "Buffer<float4> ShapeKeyData : register(t0);\n",
    );
    commit_all(&repo_root, "Updated shapekey hash mapping for 2.5");

    let knowledge = run_extract_wwmi_knowledge_command(&ExtractWwmiKnowledgeArgs {
        repo: repo_root.display().to_string(),
        output: output_path.clone(),
        max_commits: 10,
    })
    .expect("extract wwmi knowledge");

    let output = fs::read_to_string(&output_path).expect("read knowledge output");
    let parsed: WwmiKnowledgeBase = serde_json::from_str(&output).expect("parse knowledge output");

    assert_eq!(knowledge.schema_version, "whashreonator.wwmi-knowledge.v1");
    assert_eq!(parsed.summary.analyzed_commits, 3);
    assert!(parsed.summary.fix_like_commits >= 2);
    assert!(
        parsed
            .patterns
            .iter()
            .any(|pattern| pattern.kind == WwmiPatternKind::StartupTimingAdjustment)
    );
    assert!(
        parsed
            .patterns
            .iter()
            .any(|pattern| pattern.kind == WwmiPatternKind::MappingOrHashUpdate)
    );
    assert!(
        parsed
            .evidence_commits
            .iter()
            .any(|commit| commit.subject.contains("startup crash"))
    );

    let _ = fs::remove_dir_all(&test_root);
}

fn init_git_repo(repo_root: &Path) {
    fs::create_dir_all(repo_root).expect("create repo root");
    run_git(repo_root, &["init"]);
    run_git(repo_root, &["config", "user.email", "test@example.com"]);
    run_git(repo_root, &["config", "user.name", "WhashReonator Test"]);
    run_git(
        repo_root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/SpectrumQT/WWMI-Package.git",
        ],
    );
}

fn write_file(repo_root: &Path, relative_path: &str, contents: &str) {
    let full_path = repo_root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(full_path, contents).expect("write fixture file");
}

fn commit_all(repo_root: &Path, message: &str) {
    run_git(repo_root, &["add", "."]);
    run_git(repo_root, &["commit", "-m", message]);
}

fn run_git(repo_root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .expect("run git");

    if !output.status.success() {
        panic!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid time")
        .as_nanos();

    std::env::temp_dir().join(format!("whashreonator-wwmi-knowledge-test-{nanos}"))
}
