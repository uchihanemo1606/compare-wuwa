use std::path::PathBuf;

use whashreonator::{
    compare::SnapshotComparer,
    inference::FixInferenceEngine,
    proposal::{ProposalEngine, ProposalStatus},
    wwmi::load_wwmi_knowledge,
};

#[test]
fn regression_fixture_keeps_ambiguous_remaps_in_review_but_proposes_clean_replacements() {
    let fixture_dir = fixture_dir();
    let compare_report = SnapshotComparer
        .compare_files(
            &fixture_dir.join("old_snapshot.json"),
            &fixture_dir.join("new_snapshot.json"),
        )
        .expect("compare fixture snapshots");
    let knowledge =
        load_wwmi_knowledge(&fixture_dir.join("wwmi_knowledge.json")).expect("load wwmi fixture");

    assert_eq!(compare_report.summary.changed_assets, 1);
    assert_eq!(compare_report.summary.removed_assets, 2);
    assert_eq!(compare_report.summary.added_assets, 3);
    assert_eq!(compare_report.summary.candidate_mapping_changes, 2);

    let hair_candidate = compare_report
        .candidate_mapping_changes
        .iter()
        .find(|candidate| candidate.old_asset.path == "Content/Character/Encore/Hair.mesh")
        .expect("hair mapping candidate");
    assert!(!hair_candidate.ambiguous);

    let pistol_candidate = compare_report
        .candidate_mapping_changes
        .iter()
        .find(|candidate| candidate.old_asset.path == "Content/Weapon/Pistol_Main.weapon")
        .expect("pistol mapping candidate");
    assert!(pistol_candidate.ambiguous);
    assert!(pistol_candidate.runner_up_confidence.is_some());

    let inference = FixInferenceEngine.infer(&compare_report, &knowledge);
    assert!(
        inference
            .probable_crash_causes
            .iter()
            .any(|cause| cause.code == "buffer_layout_changed")
    );
    assert!(
        inference
            .probable_crash_causes
            .iter()
            .any(|cause| cause.code == "asset_paths_or_mapping_shifted")
    );

    let hair_hint = inference
        .candidate_mapping_hints
        .iter()
        .find(|hint| hint.old_asset_path == "Content/Character/Encore/Hair.mesh")
        .expect("hair mapping hint");
    assert!(!hair_hint.ambiguous);
    assert!(hair_hint.confidence >= 0.85);

    let pistol_hint = inference
        .candidate_mapping_hints
        .iter()
        .find(|hint| hint.old_asset_path == "Content/Weapon/Pistol_Main.weapon")
        .expect("pistol mapping hint");
    assert!(pistol_hint.ambiguous);
    assert!(pistol_hint.confidence < 0.85);

    let proposals = ProposalEngine.generate(&inference, 0.85);
    let hair_proposal = proposals
        .mapping_proposal
        .mappings
        .iter()
        .find(|entry| entry.old_asset_path == "Content/Character/Encore/Hair.mesh")
        .expect("hair mapping proposal");
    assert_eq!(hair_proposal.status, ProposalStatus::Proposed);

    let pistol_proposal = proposals
        .mapping_proposal
        .mappings
        .iter()
        .find(|entry| entry.old_asset_path == "Content/Weapon/Pistol_Main.weapon")
        .expect("pistol mapping proposal");
    assert_eq!(pistol_proposal.status, ProposalStatus::NeedsReview);

    assert!(
        proposals
            .patch_draft
            .actions
            .iter()
            .any(|action| action.action == "review_fix")
    );
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("version-regression")
}
