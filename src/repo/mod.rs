use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    error::{AppError, AppResult},
    wwmi::WwmiRepoInput,
};

pub trait RepoHistorySource {
    fn load_history(
        &self,
        repo_input: &WwmiRepoInput,
        max_commits: usize,
    ) -> AppResult<RepoCommitHistory>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoCommitHistory {
    pub repo_path: PathBuf,
    pub origin_url: Option<String>,
    pub commits: Vec<RepoCommit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoCommit {
    pub hash: String,
    pub unix_time: i64,
    pub subject: String,
    pub decorations: String,
    pub commit_url: Option<String>,
    pub changed_files: Vec<String>,
    pub added_lines: Vec<String>,
    pub removed_lines: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct GitRepoHistorySource;

impl RepoHistorySource for GitRepoHistorySource {
    fn load_history(
        &self,
        repo_input: &WwmiRepoInput,
        max_commits: usize,
    ) -> AppResult<RepoCommitHistory> {
        if max_commits == 0 {
            return Err(AppError::InvalidInput(
                "max_commits must be greater than zero".to_string(),
            ));
        }

        let repo_path = materialize_repo(repo_input)?;
        let origin_url = read_origin_url(&repo_path).ok();
        let commit_rows = run_git(
            Some(&repo_path),
            &[
                "log".to_string(),
                "--date-order".to_string(),
                "--no-merges".to_string(),
                format!("-n{max_commits}"),
                "--pretty=format:%H%x1f%ct%x1f%s%x1f%D".to_string(),
            ],
        )?;

        let commits = commit_rows
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| parse_commit_line(line, origin_url.as_deref(), &repo_path))
            .collect::<AppResult<Vec<_>>>()?;

        Ok(RepoCommitHistory {
            repo_path,
            origin_url,
            commits,
        })
    }
}

fn parse_commit_line(
    line: &str,
    origin_url: Option<&str>,
    repo_path: &Path,
) -> AppResult<RepoCommit> {
    let parts = line.split('\u{1f}').collect::<Vec<_>>();
    if parts.len() < 4 {
        return Err(AppError::CommandFailed(format!(
            "unable to parse git log line: {line}"
        )));
    }

    let hash = parts[0].to_string();
    let unix_time = parts[1].parse::<i64>().map_err(|error| {
        AppError::CommandFailed(format!("unable to parse commit unix time: {error}"))
    })?;
    let subject = parts[2].to_string();
    let decorations = parts[3].to_string();
    let changed_files = run_git(
        Some(repo_path),
        &[
            "show".to_string(),
            "--name-only".to_string(),
            "--format=".to_string(),
            hash.clone(),
        ],
    )?
    .lines()
    .filter(|line| !line.trim().is_empty())
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>();
    let patch = run_git(
        Some(repo_path),
        &[
            "show".to_string(),
            "--unified=0".to_string(),
            "--format=".to_string(),
            hash.clone(),
        ],
    )?;

    Ok(RepoCommit {
        commit_url: origin_url.and_then(|url| github_commit_url(url, &hash)),
        hash,
        unix_time,
        subject,
        decorations,
        changed_files,
        added_lines: collect_patch_lines(&patch, '+'),
        removed_lines: collect_patch_lines(&patch, '-'),
    })
}

fn collect_patch_lines(patch: &str, prefix: char) -> Vec<String> {
    patch
        .lines()
        .filter_map(|line| match prefix {
            '+' if line.starts_with('+') && !line.starts_with("+++") => Some(line[1..].to_string()),
            '-' if line.starts_with('-') && !line.starts_with("---") => Some(line[1..].to_string()),
            _ => None,
        })
        .collect()
}

fn materialize_repo(repo_input: &WwmiRepoInput) -> AppResult<PathBuf> {
    match repo_input {
        WwmiRepoInput::Local(path) => {
            if !path.exists() {
                return Err(AppError::InvalidInput(format!(
                    "repo path does not exist: {}",
                    path.display()
                )));
            }
            let canonical_path = canonical_repo_path(path);
            verify_work_tree(&canonical_path)?;
            Ok(canonical_path)
        }
        WwmiRepoInput::Remote(url) => {
            let cache_dir = repo_cache_dir(url);
            if cache_dir.exists() {
                verify_work_tree(&cache_dir)?;
                run_git(
                    Some(&cache_dir),
                    &[
                        "fetch".to_string(),
                        "--tags".to_string(),
                        "--prune".to_string(),
                        "origin".to_string(),
                    ],
                )?;
            } else {
                if let Some(parent) = cache_dir.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                run_git_raw(
                    None,
                    &[
                        "clone".to_string(),
                        "--no-checkout".to_string(),
                        url.clone(),
                        cache_dir.display().to_string(),
                    ],
                )?;
            }
            Ok(cache_dir)
        }
    }
}

fn verify_work_tree(repo_path: &Path) -> AppResult<()> {
    let output = run_git(
        Some(repo_path),
        &["rev-parse".to_string(), "--is-inside-work-tree".to_string()],
    )?;
    if output.trim() == "true" {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "path is not a git repository: {}",
            repo_path.display()
        )))
    }
}

fn read_origin_url(repo_path: &Path) -> AppResult<String> {
    let output = run_git(
        Some(repo_path),
        &[
            "remote".to_string(),
            "get-url".to_string(),
            "origin".to_string(),
        ],
    )?;
    Ok(output.trim().to_string())
}

fn github_commit_url(origin_url: &str, hash: &str) -> Option<String> {
    let normalized = normalize_github_remote(origin_url)?;
    Some(format!("{normalized}/commit/{hash}"))
}

fn normalize_github_remote(origin_url: &str) -> Option<String> {
    if origin_url.starts_with("https://github.com/") {
        return Some(origin_url.trim_end_matches(".git").to_string());
    }

    let prefix = "git@github.com:";
    origin_url
        .strip_prefix(prefix)
        .map(|suffix| format!("https://github.com/{}", suffix.trim_end_matches(".git")))
}

fn repo_cache_dir(url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let hash = hasher.finish();
    std::env::temp_dir()
        .join("whashreonator")
        .join("repo-cache")
        .join(format!("{hash:016x}"))
}

fn run_git(repo_path: Option<&Path>, args: &[String]) -> AppResult<String> {
    let mut git_args = Vec::new();
    if let Some(repo_path) = repo_path {
        git_args.push("-c".to_string());
        git_args.push(git_safe_directory_arg(repo_path));
    }
    git_args.extend(args.iter().cloned());
    run_git_raw(repo_path, &git_args)
}

fn git_safe_directory_arg(repo_path: &Path) -> String {
    format!(
        "safe.directory={}",
        normalize_git_safe_directory_path(&canonical_repo_path(repo_path))
    )
}

fn canonical_repo_path(repo_path: &Path) -> PathBuf {
    repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
}

fn normalize_git_safe_directory_path(repo_path: &Path) -> String {
    let path = repo_path.display().to_string();
    path.strip_prefix(r"\\?\")
        .unwrap_or(&path)
        .replace('\\', "/")
}

fn run_git_raw(repo_path: Option<&Path>, args: &[String]) -> AppResult<String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(repo_path) = repo_path {
        command.current_dir(repo_path);
    }

    let output = command.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(AppError::CommandFailed(format!(
            "git {} failed: {}",
            args.join(" "),
            detail
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{git_safe_directory_arg, normalize_github_remote};

    #[test]
    fn normalizes_https_remote() {
        assert_eq!(
            normalize_github_remote("https://github.com/SpectrumQT/WWMI-Package.git").as_deref(),
            Some("https://github.com/SpectrumQT/WWMI-Package")
        );
    }

    #[test]
    fn normalizes_ssh_remote() {
        assert_eq!(
            normalize_github_remote("git@github.com:SpectrumQT/WWMI-Package.git").as_deref(),
            Some("https://github.com/SpectrumQT/WWMI-Package")
        );
    }

    #[test]
    fn safe_directory_arg_uses_canonical_absolute_path() {
        let temp_dir = std::env::temp_dir().join(format!(
            "whashreonator-repo-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("valid time")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let safe_arg = git_safe_directory_arg(&temp_dir);

        assert!(safe_arg.starts_with("safe.directory="));
        assert!(
            safe_arg.contains(
                &temp_dir
                    .canonicalize()
                    .expect("canonicalize temp dir")
                    .display()
                    .to_string()
                    .trim_start_matches(r"\\?\")
                    .replace('\\', "/")
            )
        );
        assert!(!safe_arg.contains(r"\\?\"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
