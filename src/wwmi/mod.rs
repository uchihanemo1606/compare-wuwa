use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    repo::{GitRepoHistorySource, RepoCommit, RepoHistorySource},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WwmiRepoInput {
    Local(PathBuf),
    Remote(String),
}

impl WwmiRepoInput {
    pub fn parse(value: &str) -> Self {
        if value.starts_with("https://")
            || value.starts_with("http://")
            || value.starts_with("git@")
        {
            Self::Remote(value.to_string())
        } else {
            Self::Local(PathBuf::from(value))
        }
    }

    pub fn display_value(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Remote(url) => url.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WwmiKnowledgeBase {
    pub schema_version: String,
    pub generated_at_unix_ms: u128,
    pub repo: WwmiKnowledgeRepoInfo,
    pub summary: WwmiKnowledgeSummary,
    pub patterns: Vec<WwmiFixPattern>,
    pub keyword_stats: Vec<WwmiKeywordStat>,
    pub evidence_commits: Vec<WwmiEvidenceCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WwmiKnowledgeRepoInfo {
    pub input: String,
    pub resolved_path: String,
    pub origin_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WwmiKnowledgeSummary {
    pub analyzed_commits: usize,
    pub fix_like_commits: usize,
    pub discovered_patterns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WwmiFixPattern {
    pub kind: WwmiPatternKind,
    pub description: String,
    pub frequency: usize,
    pub average_fix_likelihood: f32,
    pub example_commits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WwmiPatternKind {
    RuntimeConfigChange,
    ShaderLogicChange,
    MappingOrHashUpdate,
    StartupTimingAdjustment,
    BufferLayoutOrCapacityFix,
    CompatibilityOrDetectionChange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WwmiKeywordStat {
    pub keyword: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WwmiEvidenceCommit {
    pub hash: String,
    pub subject: String,
    pub unix_time: i64,
    pub decorations: String,
    pub commit_url: Option<String>,
    pub fix_likelihood: f32,
    pub changed_files: Vec<String>,
    pub detected_patterns: Vec<WwmiPatternKind>,
    pub detected_keywords: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WwmiKnowledgeExtractor<H = GitRepoHistorySource> {
    history_source: H,
}

impl WwmiKnowledgeExtractor<GitRepoHistorySource> {
    pub fn new_default() -> Self {
        Self {
            history_source: GitRepoHistorySource,
        }
    }
}

impl<H> WwmiKnowledgeExtractor<H>
where
    H: RepoHistorySource,
{
    pub fn new(history_source: H) -> Self {
        Self { history_source }
    }

    pub fn extract(
        &self,
        repo_input: &WwmiRepoInput,
        max_commits: usize,
    ) -> AppResult<WwmiKnowledgeBase> {
        let history = self.history_source.load_history(repo_input, max_commits)?;
        let mut pattern_buckets = BTreeMap::<WwmiPatternKind, Vec<&AnalyzedCommit>>::new();
        let mut keyword_counts = BTreeMap::<String, usize>::new();
        let analyzed_commits = history
            .commits
            .iter()
            .map(analyze_commit)
            .collect::<Vec<_>>();

        for commit in &analyzed_commits {
            for keyword in &commit.keywords {
                *keyword_counts.entry(keyword.clone()).or_default() += 1;
            }

            if commit.fix_likelihood >= 0.40 {
                for pattern in &commit.patterns {
                    pattern_buckets
                        .entry(pattern.clone())
                        .or_default()
                        .push(commit);
                }
            }
        }

        let patterns = pattern_buckets
            .into_iter()
            .map(|(kind, commits)| WwmiFixPattern {
                description: pattern_description(&kind).to_string(),
                frequency: commits.len(),
                average_fix_likelihood: commits
                    .iter()
                    .map(|commit| commit.fix_likelihood)
                    .sum::<f32>()
                    / commits.len() as f32,
                example_commits: commits
                    .iter()
                    .take(5)
                    .map(|commit| commit.commit.hash.clone())
                    .collect(),
                kind,
            })
            .collect::<Vec<_>>();

        let keyword_stats = keyword_counts
            .into_iter()
            .map(|(keyword, count)| WwmiKeywordStat { keyword, count })
            .collect::<Vec<_>>();
        let mut keyword_stats = keyword_stats;
        keyword_stats.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.keyword.cmp(&right.keyword))
        });

        let evidence_commits = analyzed_commits
            .iter()
            .filter(|commit| commit.fix_likelihood >= 0.40)
            .map(|commit| WwmiEvidenceCommit {
                hash: commit.commit.hash.clone(),
                subject: commit.commit.subject.clone(),
                unix_time: commit.commit.unix_time,
                decorations: commit.commit.decorations.clone(),
                commit_url: commit.commit.commit_url.clone(),
                fix_likelihood: commit.fix_likelihood,
                changed_files: commit.commit.changed_files.clone(),
                detected_patterns: commit.patterns.clone(),
                detected_keywords: commit.keywords.iter().cloned().collect(),
                reasons: commit.reasons.clone(),
            })
            .collect::<Vec<_>>();

        Ok(WwmiKnowledgeBase {
            schema_version: "whashreonator.wwmi-knowledge.v1".to_string(),
            generated_at_unix_ms: current_unix_ms()?,
            repo: WwmiKnowledgeRepoInfo {
                input: repo_input.display_value(),
                resolved_path: history.repo_path.display().to_string(),
                origin_url: history.origin_url,
            },
            summary: WwmiKnowledgeSummary {
                analyzed_commits: analyzed_commits.len(),
                fix_like_commits: evidence_commits.len(),
                discovered_patterns: patterns.len(),
            },
            patterns,
            keyword_stats,
            evidence_commits,
        })
    }
}

pub fn load_wwmi_knowledge(path: &std::path::Path) -> AppResult<WwmiKnowledgeBase> {
    let knowledge: WwmiKnowledgeBase = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(knowledge)
}

#[derive(Debug, Clone)]
struct AnalyzedCommit {
    commit: RepoCommit,
    fix_likelihood: f32,
    patterns: Vec<WwmiPatternKind>,
    keywords: BTreeSet<String>,
    reasons: Vec<String>,
}

fn analyze_commit(commit: &RepoCommit) -> AnalyzedCommit {
    let subject_lower = commit.subject.to_ascii_lowercase();
    let files_lower = commit
        .changed_files
        .iter()
        .map(|path| path.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let diff_text = commit
        .added_lines
        .iter()
        .chain(commit.removed_lines.iter())
        .map(|line| line.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let mut reasons = Vec::new();
    let mut keywords = BTreeSet::new();
    let mut patterns = Vec::new();
    let mut score: f32 = 0.0;

    if contains_any(
        &subject_lower,
        &["fix", "fixed", "hotfix", "patch", "resolve", "resolved"],
    ) {
        score += 0.35;
        reasons.push("subject contains fix-oriented keywords".to_string());
        collect_keywords(&mut keywords, &subject_lower);
    }

    if contains_any(
        &format!("{subject_lower}\n{diff_text}"),
        &[
            "crash",
            "compat",
            "compatibility",
            "startup",
            "init",
            "hook",
            "delay",
            "reliable",
        ],
    ) {
        score += 0.20;
        reasons.push("subject or diff contains crash/compatibility/timing signals".to_string());
        collect_keywords(&mut keywords, &format!("{subject_lower}\n{diff_text}"));
    }

    let touches_ini = files_lower.iter().any(|path| path.ends_with(".ini"));
    if touches_ini {
        score += 0.10;
        reasons.push("commit changes ini/config files".to_string());
    }

    let touches_shader = files_lower.iter().any(|path| {
        path.ends_with(".hlsl") || path.contains("/shaders/") || path.contains("\\shaders\\")
    });
    if touches_shader {
        score += 0.10;
        reasons.push("commit changes shader logic files".to_string());
    }

    let mapping_related = contains_any(
        &format!("{subject_lower}\n{diff_text}\n{}", files_lower.join("\n")),
        &[
            "hash",
            "mapping",
            "remap",
            "resource",
            "shaderoverride",
            "vertexlimitraise",
        ],
    );
    if mapping_related {
        score += 0.15;
        reasons.push("commit contains mapping/hash/resource signals".to_string());
    }

    let buffer_related = contains_any(
        &format!("{subject_lower}\n{diff_text}"),
        &[
            "buffer", "resize", "vertex", "index", "stride", "shapekey", "dispatch", "slot", "uav",
        ],
    );
    if buffer_related {
        score += 0.10;
        reasons.push("commit contains buffer/layout/capacity signals".to_string());
    }

    let detection_related = contains_any(
        &format!("{subject_lower}\n{diff_text}"),
        &[
            "detect",
            "detection",
            "menu",
            "dressing",
            "character",
            "resolution",
            "screen",
        ],
    );
    if detection_related {
        score += 0.05;
        reasons.push("commit contains compatibility/detection signals".to_string());
    }

    if touches_ini
        && contains_any(
            &diff_text,
            &[
                "delay",
                "allow_buffer_resize",
                "d3dx.ini",
                "crash",
                "compat",
            ],
        )
    {
        patterns.push(WwmiPatternKind::RuntimeConfigChange);
    }
    if touches_shader {
        patterns.push(WwmiPatternKind::ShaderLogicChange);
    }
    if mapping_related {
        patterns.push(WwmiPatternKind::MappingOrHashUpdate);
    }
    if contains_any(
        &format!("{subject_lower}\n{diff_text}"),
        &["delay", "startup", "init", "hook"],
    ) {
        patterns.push(WwmiPatternKind::StartupTimingAdjustment);
    }
    if buffer_related {
        patterns.push(WwmiPatternKind::BufferLayoutOrCapacityFix);
    }
    if detection_related || contains_any(&subject_lower, &["compat", "support"]) {
        patterns.push(WwmiPatternKind::CompatibilityOrDetectionChange);
    }

    dedupe_patterns(&mut patterns);
    if keywords.is_empty() {
        collect_keywords(
            &mut keywords,
            &format!("{subject_lower}\n{}", files_lower.join("\n")),
        );
    }

    AnalyzedCommit {
        commit: commit.clone(),
        fix_likelihood: score.clamp(0.0, 1.0),
        patterns,
        keywords,
        reasons,
    }
}

fn dedupe_patterns(patterns: &mut Vec<WwmiPatternKind>) {
    patterns.sort();
    patterns.dedup();
}

fn pattern_description(pattern: &WwmiPatternKind) -> &'static str {
    match pattern {
        WwmiPatternKind::RuntimeConfigChange => {
            "INI or runtime configuration changes that adjust loader/importer behaviour."
        }
        WwmiPatternKind::ShaderLogicChange => {
            "Shader logic changes that modify how WWMI applies or reads modded data."
        }
        WwmiPatternKind::MappingOrHashUpdate => {
            "Hash, resource, or mapping-oriented changes likely tied to asset remapping."
        }
        WwmiPatternKind::StartupTimingAdjustment => {
            "Timing, startup, or hook-order changes used to avoid crashy initialization races."
        }
        WwmiPatternKind::BufferLayoutOrCapacityFix => {
            "Buffer/layout/capacity changes for asset formats or expanded data shapes."
        }
        WwmiPatternKind::CompatibilityOrDetectionChange => {
            "Compatibility or scene/detection changes needed after a game update."
        }
    }
}

fn contains_any(haystack: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| haystack.contains(keyword))
}

fn collect_keywords(target: &mut BTreeSet<String>, text: &str) {
    const TRACKED_KEYWORDS: &[&str] = &[
        "fix",
        "crash",
        "compatibility",
        "compat",
        "hash",
        "mapping",
        "buffer",
        "resize",
        "vertex",
        "index",
        "shader",
        "shapekey",
        "delay",
        "startup",
        "init",
        "hook",
        "resource",
        "detection",
        "screen",
    ];

    for keyword in TRACKED_KEYWORDS {
        if text.contains(keyword) {
            target.insert((*keyword).to_string());
        }
    }
}

fn current_unix_ms() -> AppResult<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::InvalidInput(format!("system clock error: {error}")))?
        .as_millis())
}

#[cfg(test)]
mod tests {
    use crate::repo::{RepoCommit, RepoCommitHistory, RepoHistorySource};

    use super::{WwmiKnowledgeExtractor, WwmiPatternKind, WwmiRepoInput};

    struct MockRepoHistorySource {
        history: RepoCommitHistory,
    }

    impl RepoHistorySource for MockRepoHistorySource {
        fn load_history(
            &self,
            _repo_input: &WwmiRepoInput,
            _max_commits: usize,
        ) -> crate::error::AppResult<RepoCommitHistory> {
            Ok(self.history.clone())
        }
    }

    #[test]
    fn extractor_discovers_fix_patterns_from_commit_history() {
        let extractor = WwmiKnowledgeExtractor::new(MockRepoHistorySource {
            history: RepoCommitHistory {
                repo_path: std::env::temp_dir(),
                origin_url: Some("https://github.com/SpectrumQT/WWMI-Package".to_string()),
                commits: vec![
                    RepoCommit {
                        hash: "abc123".to_string(),
                        unix_time: 1,
                        subject: "Fixed startup crash by increasing dll_initialization_delay"
                            .to_string(),
                        decorations: String::new(),
                        commit_url: Some(
                            "https://github.com/SpectrumQT/WWMI-Package/commit/abc123".to_string(),
                        ),
                        changed_files: vec!["WWMI/d3dx.ini".to_string()],
                        added_lines: vec!["dll_initialization_delay = 500".to_string()],
                        removed_lines: vec!["dll_initialization_delay = 50".to_string()],
                    },
                    RepoCommit {
                        hash: "def456".to_string(),
                        unix_time: 2,
                        subject: "Updated shapekey hash mapping for 2.5".to_string(),
                        decorations: String::new(),
                        commit_url: Some(
                            "https://github.com/SpectrumQT/WWMI-Package/commit/def456".to_string(),
                        ),
                        changed_files: vec![
                            "WWMI/Core/WWMI/WuWa-Model-Importer.ini".to_string(),
                            "WWMI/Core/WWMI/Shaders/ShapeKeyOverrider.hlsl".to_string(),
                        ],
                        added_lines: vec![
                            "hash = 123456".to_string(),
                            "Buffer<float4> ShapeKeyData : register(t0);".to_string(),
                        ],
                        removed_lines: vec!["hash = 654321".to_string()],
                    },
                ],
            },
        });

        let knowledge = extractor
            .extract(
                &WwmiRepoInput::Remote(
                    "https://github.com/SpectrumQT/WWMI-Package.git".to_string(),
                ),
                10,
            )
            .expect("extract knowledge");

        assert_eq!(knowledge.summary.analyzed_commits, 2);
        assert_eq!(knowledge.summary.fix_like_commits, 2);
        assert!(
            knowledge
                .patterns
                .iter()
                .any(|pattern| pattern.kind == WwmiPatternKind::StartupTimingAdjustment)
        );
        assert!(
            knowledge
                .patterns
                .iter()
                .any(|pattern| pattern.kind == WwmiPatternKind::MappingOrHashUpdate)
        );
        assert!(
            knowledge
                .keyword_stats
                .iter()
                .any(|stat| stat.keyword == "hash")
        );
    }
}
