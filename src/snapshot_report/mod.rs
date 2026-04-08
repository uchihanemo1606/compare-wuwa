use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::{
    compare::{CandidateMappingChange, SnapshotComparer},
    error::{AppError, AppResult},
    snapshot::{GameSnapshot, SnapshotScopeAssessment, assess_snapshot_scope, load_snapshot},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInventoryReport {
    pub markdown: String,
    pub version_count: usize,
    pub resonator_count: usize,
    pub pair_count: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SnapshotReportRenderer;

impl SnapshotReportRenderer {
    pub fn render(&self, snapshots: &[GameSnapshot]) -> AppResult<SnapshotInventoryReport> {
        if snapshots.is_empty() {
            return Err(AppError::InvalidInput(
                "snapshot-report requires at least one --snapshot input".to_string(),
            ));
        }

        let resonator_counts = snapshots
            .iter()
            .map(snapshot_resonator_counts)
            .collect::<Vec<_>>();
        let scope_assessments = snapshots
            .iter()
            .map(assess_snapshot_scope)
            .collect::<Vec<_>>();
        let all_resonators = resonator_counts
            .iter()
            .flat_map(|counts| counts.keys().cloned())
            .collect::<BTreeSet<_>>();
        let markdown = render_markdown(
            snapshots,
            &resonator_counts,
            &all_resonators,
            &scope_assessments,
        );

        Ok(SnapshotInventoryReport {
            markdown,
            version_count: snapshots.len(),
            resonator_count: all_resonators.len(),
            pair_count: snapshots.len().saturating_sub(1),
        })
    }
}

pub fn load_snapshots(paths: &[PathBuf]) -> AppResult<Vec<GameSnapshot>> {
    if paths.is_empty() {
        return Err(AppError::InvalidInput(
            "snapshot-report requires at least one --snapshot input".to_string(),
        ));
    }

    paths
        .iter()
        .map(|path| load_snapshot(path.as_path()))
        .collect::<AppResult<Vec<_>>>()
}

fn render_markdown(
    snapshots: &[GameSnapshot],
    resonator_counts: &[BTreeMap<String, usize>],
    all_resonators: &BTreeSet<String>,
    scope_assessments: &[SnapshotScopeAssessment],
) -> String {
    let mut lines = Vec::new();
    lines.push("# Snapshot Report".to_string());
    lines.push(String::new());
    lines.push(format!(
        "Compared {} snapshot(s) in the provided order.",
        snapshots.len()
    ));
    lines.push(String::new());

    lines.push("## Version Summary".to_string());
    lines.push(
        "| Version | Reuse Version | Total Assets | Resonators | Character Assets | Other Assets | Source Root |"
            .to_string(),
    );
    lines.push("| --- | --- | ---: | ---: | ---: | ---: | --- |".to_string());
    for (snapshot, counts) in snapshots.iter().zip(resonator_counts.iter()) {
        let character_assets = counts.values().sum::<usize>();
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            md_cell(&snapshot.version_id),
            md_cell(
                snapshot
                    .context
                    .launcher
                    .as_ref()
                    .and_then(|launcher| launcher.reuse_version.as_deref())
                    .unwrap_or("-")
            ),
            snapshot.asset_count,
            counts.len(),
            character_assets,
            snapshot.asset_count.saturating_sub(character_assets),
            md_cell(&snapshot.source_root)
        ));
    }
    lines.push(String::new());

    lines.push("## Scope & Coverage".to_string());
    lines.push(
        "| Version | Capture Mode | Scope | Content-like Paths | Character-like Paths | Non-content Paths | Note |"
            .to_string(),
    );
    lines.push("| --- | --- | --- | ---: | ---: | ---: | --- |".to_string());
    for (snapshot, scope) in snapshots.iter().zip(scope_assessments.iter()) {
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            md_cell(&snapshot.version_id),
            md_cell(scope.capture_mode.as_deref().unwrap_or("-")),
            scope_label(scope),
            scope.coverage.content_like_path_count,
            scope.coverage.character_path_count,
            scope.coverage.non_content_path_count,
            md_cell(&scope_note(scope))
        ));
    }
    lines.push(String::new());

    lines.push("## Analysis Limitations".to_string());
    let mut low_signal_lines = Vec::new();
    for (snapshot, scope) in snapshots.iter().zip(scope_assessments.iter()) {
        if scope.is_low_signal_for_character_analysis() {
            low_signal_lines.push(format!(
                "- {}: install/package-level or low-coverage snapshot; resonator-level and mapping-level interpretation can be incomplete.",
                snapshot.version_id
            ));
        }
    }
    if low_signal_lines.is_empty() {
        lines.push(
            "Scope metadata indicates character/content coverage is present for the analyzed snapshots."
                .to_string(),
        );
    } else {
        lines.push(
            "The following snapshots are low-signal for deep character/resonator analysis:"
                .to_string(),
        );
        lines.extend(low_signal_lines);
    }
    lines.push(String::new());

    lines.push("## Resonator Matrix".to_string());
    if scope_assessments
        .iter()
        .any(SnapshotScopeAssessment::is_low_signal_for_character_analysis)
    {
        lines.push(
            "Resonator matrix is still shown, but low-signal snapshots should be treated as inventory-level hints."
                .to_string(),
        );
    }
    if all_resonators.is_empty() {
        if scope_assessments
            .iter()
            .all(SnapshotScopeAssessment::is_low_signal_for_character_analysis)
        {
            lines.push(
                "No `Content/Character/<Name>/...` assets were found, and scope metadata indicates install/package-level coverage for this snapshot set."
                    .to_string(),
            );
        } else {
            lines.push(
                "No `Content/Character/<Name>/...` assets were found in the provided snapshots."
                    .to_string(),
            );
        }
    } else {
        let mut header = vec!["Resonator".to_string()];
        header.extend(snapshots.iter().map(|snapshot| snapshot.version_id.clone()));
        lines.push(render_table_row(&header));
        lines.push(render_table_divider(header.len()));
        for resonator in all_resonators {
            let mut row = vec![resonator.clone()];
            for counts in resonator_counts {
                row.push(
                    counts
                        .get(resonator)
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                );
            }
            lines.push(render_table_row(&row));
        }
    }
    lines.push(String::new());

    lines.push("## Version-to-Version Changes".to_string());
    if snapshots.len() < 2 {
        lines.push(
            "Only one snapshot was provided, so there is no pairwise comparison yet.".to_string(),
        );
        return lines.join("\n");
    }

    for index in 1..snapshots.len() {
        let old_snapshot = &snapshots[index - 1];
        let new_snapshot = &snapshots[index];
        let old_counts = &resonator_counts[index - 1];
        let new_counts = &resonator_counts[index];
        let old_scope = &scope_assessments[index - 1];
        let new_scope = &scope_assessments[index];
        let compare_report = SnapshotComparer.compare(old_snapshot, new_snapshot);
        let added_resonators = new_counts
            .keys()
            .filter(|name| !old_counts.contains_key(*name))
            .cloned()
            .collect::<BTreeSet<_>>();
        let removed_resonators = old_counts
            .keys()
            .filter(|name| !new_counts.contains_key(*name))
            .cloned()
            .collect::<BTreeSet<_>>();
        let changed_resonators =
            changed_resonators(&compare_report, &added_resonators, &removed_resonators);

        lines.push(format!(
            "### {} -> {}",
            old_snapshot.version_id, new_snapshot.version_id
        ));
        if old_scope.is_low_signal_for_character_analysis()
            || new_scope.is_low_signal_for_character_analysis()
        {
            lines.push(
                "Scope note: this pair includes install/package-level or low-coverage snapshots, so resonator-level and candidate-remap signals are limited."
                    .to_string(),
            );
        }
        lines.push("| Metric | Value |".to_string());
        lines.push("| --- | --- |".to_string());
        lines.push(format!(
            "| Added assets | {} |",
            compare_report.summary.added_assets
        ));
        lines.push(format!(
            "| Removed assets | {} |",
            compare_report.summary.removed_assets
        ));
        lines.push(format!(
            "| Changed assets | {} |",
            compare_report.summary.changed_assets
        ));
        lines.push(format!(
            "| Candidate remaps | {} |",
            compare_report.summary.candidate_mapping_changes
        ));
        lines.push(format!(
            "| Added resonators | {} |",
            join_or_dash(added_resonators.iter().map(String::as_str))
        ));
        lines.push(format!(
            "| Removed resonators | {} |",
            join_or_dash(removed_resonators.iter().map(String::as_str))
        ));
        lines.push(format!(
            "| Changed resonators | {} |",
            join_or_dash(changed_resonators.iter().map(String::as_str))
        ));
        lines.push(String::new());

        let impacted_resonators = added_resonators
            .iter()
            .chain(removed_resonators.iter())
            .chain(changed_resonators.iter())
            .cloned()
            .collect::<BTreeSet<_>>();
        if impacted_resonators.is_empty() {
            lines.push("No resonator-level changes detected for this pair.".to_string());
        } else {
            lines.push("| Resonator | Old Count | New Count | Status |".to_string());
            lines.push("| --- | ---: | ---: | --- |".to_string());
            for resonator in impacted_resonators {
                let status = if added_resonators.contains(&resonator) {
                    "added"
                } else if removed_resonators.contains(&resonator) {
                    "removed"
                } else {
                    "changed"
                };
                lines.push(format!(
                    "| {} | {} | {} | {} |",
                    md_cell(&resonator),
                    old_counts.get(&resonator).copied().unwrap_or_default(),
                    new_counts.get(&resonator).copied().unwrap_or_default(),
                    status
                ));
            }
        }
        lines.push(String::new());

        lines.push("#### Candidate Remaps".to_string());
        if compare_report.candidate_mapping_changes.is_empty() {
            lines.push("No candidate remaps were inferred for this pair.".to_string());
        } else {
            lines.push("| Resonator | Old Asset | New Asset | Confidence | Review |".to_string());
            lines.push("| --- | --- | --- | ---: | --- |".to_string());
            for candidate in sorted_candidate_remaps(&compare_report.candidate_mapping_changes)
                .into_iter()
                .take(10)
            {
                let resonator = infer_resonator_name(&candidate.old_asset.path)
                    .or_else(|| infer_resonator_name(&candidate.new_asset.path))
                    .unwrap_or_else(|| "-".to_string());
                let review = if candidate.ambiguous {
                    "needs review"
                } else {
                    "strongest"
                };
                lines.push(format!(
                    "| {} | {} | {} | {:.3} | {} |",
                    md_cell(&resonator),
                    md_cell(&candidate.old_asset.path),
                    md_cell(&candidate.new_asset.path),
                    candidate.confidence,
                    review
                ));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

fn scope_label(scope: &SnapshotScopeAssessment) -> &'static str {
    if scope.mostly_install_or_package_level {
        "mostly install/package-level"
    } else if scope.meaningful_content_coverage && scope.meaningful_character_coverage {
        "content/character-oriented inventory"
    } else {
        "mixed or partial coverage"
    }
}

fn scope_note(scope: &SnapshotScopeAssessment) -> String {
    if let Some(note) = scope.note.as_deref() {
        return note.to_string();
    }

    if scope.observed_fallback_used {
        return "scope metadata missing; inferred from observed paths".to_string();
    }

    "-".to_string()
}

fn snapshot_resonator_counts(snapshot: &GameSnapshot) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for asset in &snapshot.assets {
        if let Some(resonator) = infer_resonator_name(&asset.path) {
            *counts.entry(resonator).or_default() += 1;
        }
    }
    counts
}

fn changed_resonators(
    compare_report: &crate::compare::SnapshotCompareReport,
    added_resonators: &BTreeSet<String>,
    removed_resonators: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    for asset in &compare_report.changed_assets {
        collect_asset_change_resonators(
            &mut changed,
            asset.old_asset.as_ref(),
            asset.new_asset.as_ref(),
        );
    }
    for asset in &compare_report.added_assets {
        collect_asset_change_resonators(
            &mut changed,
            asset.old_asset.as_ref(),
            asset.new_asset.as_ref(),
        );
    }
    for asset in &compare_report.removed_assets {
        collect_asset_change_resonators(
            &mut changed,
            asset.old_asset.as_ref(),
            asset.new_asset.as_ref(),
        );
    }
    for candidate in &compare_report.candidate_mapping_changes {
        if let Some(resonator) = infer_resonator_name(&candidate.old_asset.path) {
            changed.insert(resonator);
        }
        if let Some(resonator) = infer_resonator_name(&candidate.new_asset.path) {
            changed.insert(resonator);
        }
    }
    changed.retain(|name| !added_resonators.contains(name) && !removed_resonators.contains(name));
    changed
}

fn collect_asset_change_resonators(
    target: &mut BTreeSet<String>,
    old_asset: Option<&crate::compare::SnapshotAssetSummary>,
    new_asset: Option<&crate::compare::SnapshotAssetSummary>,
) {
    if let Some(asset) = old_asset {
        if let Some(resonator) = infer_resonator_name(&asset.path) {
            target.insert(resonator);
        }
    }
    if let Some(asset) = new_asset {
        if let Some(resonator) = infer_resonator_name(&asset.path) {
            target.insert(resonator);
        }
    }
}

fn sorted_candidate_remaps(candidates: &[CandidateMappingChange]) -> Vec<CandidateMappingChange> {
    let mut sorted = candidates.to_vec();
    sorted.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| left.old_asset.path.cmp(&right.old_asset.path))
            .then_with(|| left.new_asset.path.cmp(&right.new_asset.path))
    });
    sorted
}

fn infer_resonator_name(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let parts = normalized.split('/').collect::<Vec<_>>();
    parts
        .windows(3)
        .find(|window| {
            window[0].eq_ignore_ascii_case("content")
                && window[1].eq_ignore_ascii_case("character")
                && !window[2].is_empty()
        })
        .map(|window| window[2].to_string())
}

fn join_or_dash<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let collected = values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if collected.is_empty() {
        "-".to_string()
    } else {
        collected.join(", ")
    }
}

fn render_table_row(values: &[String]) -> String {
    format!(
        "| {} |",
        values
            .iter()
            .map(|value| md_cell(value))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

fn render_table_divider(columns: usize) -> String {
    let segments = (0..columns).map(|_| "---").collect::<Vec<_>>();
    format!("| {} |", segments.join(" | "))
}

fn md_cell(value: &str) -> String {
    value.replace('|', r"\|")
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{
        GameSnapshot, SnapshotAsset, SnapshotContext, SnapshotCoverageSignals, SnapshotFingerprint,
        SnapshotHashFields, SnapshotLauncherContext, SnapshotScopeContext,
    };

    use super::{SnapshotReportRenderer, infer_resonator_name};

    #[test]
    fn renderer_outputs_version_summary_resonator_matrix_and_pairwise_changes() {
        let old_snapshot = sample_snapshot(
            "2.4.0",
            Some("2.3.0"),
            vec![
                asset("Content/Character/Encore/Body.mesh", "encore body"),
                asset("Content/Character/Encore/Hair.mesh", "encore hair"),
                asset("Content/Weapon/Pistol_Main.weapon", "pistol main"),
            ],
        );
        let new_snapshot = sample_snapshot(
            "2.5.0",
            None,
            vec![
                asset("Content/Character/Encore/Body.mesh", "encore body"),
                asset("Content/Character/Encore/Hair_LOD0.mesh", "encore hair"),
                asset("Content/Character/Camellya/Body.mesh", "camellya body"),
                asset("Content/Weapon/Pistol_Main_A.weapon", "pistol main"),
            ],
        );

        let report = SnapshotReportRenderer
            .render(&[old_snapshot, new_snapshot])
            .expect("render report");

        assert_eq!(report.version_count, 2);
        assert_eq!(report.resonator_count, 2);
        assert_eq!(report.pair_count, 1);
        assert!(report.markdown.contains("## Version Summary"));
        assert!(report.markdown.contains("## Scope & Coverage"));
        assert!(report.markdown.contains("## Analysis Limitations"));
        assert!(report.markdown.contains("| 2.4.0 | 2.3.0 |"));
        assert!(report.markdown.contains("## Resonator Matrix"));
        assert!(report.markdown.contains("| Encore | 2 | 2 |"));
        assert!(report.markdown.contains("| Camellya | - | 1 |"));
        assert!(report.markdown.contains("### 2.4.0 -> 2.5.0"));
        assert!(report.markdown.contains("Scope note: this pair includes"));
        assert!(report.markdown.contains("| Added resonators | Camellya |"));
        assert!(report.markdown.contains("#### Candidate Remaps"));
        assert!(report.markdown.contains("Hair.mesh"));
    }

    #[test]
    fn renderer_marks_install_level_snapshots_as_low_signal() {
        let old_snapshot = sample_snapshot_with_scope(
            "3.0.0",
            None,
            vec![asset(
                "Client/Content/Paks/pakchunk0-WindowsNoEditor.pak",
                "pak",
            )],
            install_level_scope("local install-level inventory", 1, 0, 2),
        );
        let new_snapshot = sample_snapshot_with_scope(
            "3.1.0",
            None,
            vec![asset(
                "Client/Content/Paks/pakchunk1-WindowsNoEditor.pak",
                "pak",
            )],
            install_level_scope("local install-level inventory", 1, 0, 3),
        );

        let report = SnapshotReportRenderer
            .render(&[old_snapshot, new_snapshot])
            .expect("render report");

        assert!(report.markdown.contains("## Version Summary"));
        assert!(report.markdown.contains("## Analysis Limitations"));
        assert!(report.markdown.contains(
            "install/package-level or low-coverage snapshot; resonator-level and mapping-level interpretation can be incomplete."
        ));
        assert!(report.markdown.contains(
            "Resonator matrix is still shown, but low-signal snapshots should be treated as inventory-level hints."
        ));
    }

    #[test]
    fn renderer_keeps_resonator_sections_for_meaningful_scope_snapshots() {
        let old_snapshot = sample_snapshot_with_scope(
            "4.0.0",
            None,
            vec![
                asset("Content/Character/Encore/Body.mesh", "encore body"),
                asset("Content/Character/Encore/Hair.mesh", "encore hair"),
            ],
            meaningful_scope("local asset-level inventory", 12, 6, 1),
        );
        let new_snapshot = sample_snapshot_with_scope(
            "4.1.0",
            None,
            vec![
                asset("Content/Character/Encore/Body.mesh", "encore body"),
                asset("Content/Character/Camellya/Body.mesh", "camellya body"),
            ],
            meaningful_scope("local asset-level inventory", 14, 7, 1),
        );

        let report = SnapshotReportRenderer
            .render(&[old_snapshot, new_snapshot])
            .expect("render report");

        assert!(report.markdown.contains("## Resonator Matrix"));
        assert!(report.markdown.contains("| Encore |"));
        assert!(
            report
                .markdown
                .contains("Scope metadata indicates character/content coverage is present")
        );
    }

    #[test]
    fn infer_resonator_name_reads_character_path_segment() {
        assert_eq!(
            infer_resonator_name("Content/Character/Encore/Body.mesh").as_deref(),
            Some("Encore")
        );
        assert_eq!(
            infer_resonator_name("Content/Weapon/Pistol_Main.weapon"),
            None
        );
    }

    fn sample_snapshot(
        version_id: &str,
        reuse_version: Option<&str>,
        assets: Vec<SnapshotAsset>,
    ) -> GameSnapshot {
        sample_snapshot_with_scope(
            version_id,
            reuse_version,
            assets,
            SnapshotScopeContext::default(),
        )
    }

    fn sample_snapshot_with_scope(
        version_id: &str,
        reuse_version: Option<&str>,
        assets: Vec<SnapshotAsset>,
        scope: SnapshotScopeContext,
    ) -> GameSnapshot {
        GameSnapshot {
            schema_version: "whashreonator.snapshot.v1".to_string(),
            version_id: version_id.to_string(),
            created_at_unix_ms: 1,
            source_root: format!("fixtures/{version_id}"),
            asset_count: assets.len(),
            assets,
            context: SnapshotContext {
                launcher: reuse_version.map(|reuse_version| SnapshotLauncherContext {
                    source_file: "launcherDownloadConfig.json".to_string(),
                    detected_version: version_id.to_string(),
                    reuse_version: Some(reuse_version.to_string()),
                    state: Some("ready".to_string()),
                    is_pre_download: false,
                    app_id: Some("50004".to_string()),
                }),
                resource_manifest: None,
                scope,
                notes: Vec::new(),
            },
        }
    }

    fn install_level_scope(
        note: &str,
        content_like_path_count: usize,
        character_path_count: usize,
        non_content_path_count: usize,
    ) -> SnapshotScopeContext {
        SnapshotScopeContext {
            capture_mode: Some("local_filesystem_inventory".to_string()),
            mostly_install_or_package_level: Some(true),
            meaningful_content_coverage: Some(false),
            meaningful_character_coverage: Some(false),
            coverage: SnapshotCoverageSignals {
                content_like_path_count,
                character_path_count,
                non_content_path_count,
            },
            note: Some(note.to_string()),
        }
    }

    fn meaningful_scope(
        note: &str,
        content_like_path_count: usize,
        character_path_count: usize,
        non_content_path_count: usize,
    ) -> SnapshotScopeContext {
        SnapshotScopeContext {
            capture_mode: Some("local_filesystem_inventory".to_string()),
            mostly_install_or_package_level: Some(false),
            meaningful_content_coverage: Some(true),
            meaningful_character_coverage: Some(true),
            coverage: SnapshotCoverageSignals {
                content_like_path_count,
                character_path_count,
                non_content_path_count,
            },
            note: Some(note.to_string()),
        }
    }

    fn asset(path: &str, logical_name: &str) -> SnapshotAsset {
        SnapshotAsset {
            id: path.to_string(),
            path: path.to_string(),
            kind: Some(
                if path.ends_with(".weapon") {
                    "weapon"
                } else {
                    "mesh"
                }
                .to_string(),
            ),
            metadata: crate::domain::AssetMetadata {
                logical_name: Some(logical_name.to_string()),
                vertex_count: Some(100),
                index_count: Some(200),
                material_slots: Some(1),
                section_count: Some(1),
                tags: Vec::new(),
            },
            fingerprint: SnapshotFingerprint {
                normalized_kind: Some("mesh".to_string()),
                normalized_name: Some(logical_name.to_string()),
                name_tokens: logical_name
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
                path_tokens: path.split('/').map(ToOwned::to_owned).collect(),
                tags: Vec::new(),
                vertex_count: Some(100),
                index_count: Some(200),
                material_slots: Some(1),
                section_count: Some(1),
            },
            hash_fields: SnapshotHashFields::default(),
        }
    }
}
