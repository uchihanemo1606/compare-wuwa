use crate::{
    config::ValidatorConfig,
    domain::{AssetRecord, AssetSummary, MatchDecision, MatchReason, MatchStatus},
    matcher::ScoredMatch,
};
use tracing::warn;

pub trait Validator {
    fn validate(&self, scored_match: ScoredMatch) -> MatchDecision;
}

#[derive(Debug, Clone)]
pub struct ThresholdValidator {
    config: ValidatorConfig,
}

impl ThresholdValidator {
    pub fn new(config: ValidatorConfig) -> Self {
        Self { config }
    }
}

impl Validator for ThresholdValidator {
    fn validate(&self, scored_match: ScoredMatch) -> MatchDecision {
        let ScoredMatch {
            old_asset,
            candidate,
            second_best_confidence,
        } = scored_match;
        let old_asset_summary = AssetSummary::from(&old_asset);

        match candidate {
            Some(candidate) => {
                let gap =
                    second_best_confidence.map(|second| (candidate.confidence - second).max(0.0));
                let mut reasons = candidate.reasons;
                let mut status = if candidate.confidence >= self.config.matched_threshold {
                    if let Some(gap) = gap {
                        if gap < self.config.minimum_margin {
                            reasons.push(MatchReason {
                                code: "low_margin".to_string(),
                                message: format!(
                                    "top candidate gap {gap:.3} is below minimum margin {:.3}",
                                    self.config.minimum_margin
                                ),
                                contribution: 0.0,
                            });
                            MatchStatus::NeedsReview
                        } else {
                            MatchStatus::Matched
                        }
                    } else {
                        MatchStatus::Matched
                    }
                } else if candidate.confidence >= self.config.needs_review_threshold {
                    MatchStatus::NeedsReview
                } else {
                    reasons.push(MatchReason {
                        code: "confidence_below_review_threshold".to_string(),
                        message: format!(
                            "confidence {:.3} is below review threshold {:.3}",
                            candidate.confidence, self.config.needs_review_threshold
                        ),
                        contribution: 0.0,
                    });
                    MatchStatus::Rejected
                };

                let layout_reasons = layout_guard_reasons(&old_asset, &candidate.new_asset);
                if !layout_reasons.is_empty() {
                    let should_downgrade = status == MatchStatus::Matched;
                    reasons.extend(layout_reasons);

                    if should_downgrade {
                        warn!(
                            old_asset = %old_asset.path,
                            new_asset = %candidate.new_asset.path,
                            "downgrading matched candidate to needs_review due to structural layout drift"
                        );
                        status = MatchStatus::NeedsReview;
                    }
                }

                MatchDecision {
                    old_asset: old_asset_summary,
                    new_asset: Some(AssetSummary::from(&candidate.new_asset)),
                    confidence: candidate.confidence,
                    status,
                    reasons,
                    top_candidate_gap: gap,
                }
            }
            None => MatchDecision {
                old_asset: old_asset_summary,
                new_asset: None,
                confidence: 0.0,
                status: MatchStatus::Rejected,
                reasons: vec![MatchReason {
                    code: "no_candidate".to_string(),
                    message: "no candidate produced any positive match signal".to_string(),
                    contribution: 0.0,
                }],
                top_candidate_gap: None,
            },
        }
    }
}

fn layout_guard_reasons(old_asset: &AssetRecord, new_asset: &AssetRecord) -> Vec<MatchReason> {
    let mut reasons = Vec::new();

    push_resize_risk(
        &mut reasons,
        "vertex_count",
        old_asset.metadata.vertex_count,
        new_asset.metadata.vertex_count,
    );
    push_resize_risk(
        &mut reasons,
        "index_count",
        old_asset.metadata.index_count,
        new_asset.metadata.index_count,
    );
    push_layout_mismatch(
        &mut reasons,
        "material_slots",
        old_asset.metadata.material_slots,
        new_asset.metadata.material_slots,
    );
    push_layout_mismatch(
        &mut reasons,
        "section_count",
        old_asset.metadata.section_count,
        new_asset.metadata.section_count,
    );

    reasons
}

fn push_resize_risk(
    reasons: &mut Vec<MatchReason>,
    field_name: &str,
    old_value: Option<u32>,
    new_value: Option<u32>,
) {
    match (old_value, new_value) {
        (Some(old_value), Some(new_value)) if old_value == new_value => {}
        (Some(old_value), Some(new_value)) if new_value > old_value => {
            reasons.push(MatchReason {
                code: format!("{field_name}_resize_risk"),
                message: format!(
                    "{field_name} increased from {old_value} to {new_value}; review required because WWMI-style runtime fixes are needed when buffers outgrow previous layout assumptions"
                ),
                contribution: 0.0,
            });
        }
        (Some(old_value), Some(new_value)) => {
            reasons.push(MatchReason {
                code: format!("{field_name}_changed"),
                message: format!(
                    "{field_name} changed from {old_value} to {new_value}; review required before proposing an automatic mapping"
                ),
                contribution: 0.0,
            });
        }
        _ => {}
    }
}

fn push_layout_mismatch(
    reasons: &mut Vec<MatchReason>,
    field_name: &str,
    old_value: Option<u32>,
    new_value: Option<u32>,
) {
    match (old_value, new_value) {
        (Some(old_value), Some(new_value)) if old_value != new_value => {
            reasons.push(MatchReason {
                code: format!("{field_name}_changed"),
                message: format!(
                    "{field_name} changed from {old_value} to {new_value}; review required before proposing an automatic mapping"
                ),
                contribution: 0.0,
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::ValidatorConfig,
        domain::{AssetMetadata, AssetRecord, MatchReason, MatchStatus},
        matcher::{CandidateMatch, ScoredMatch},
        validator::{ThresholdValidator, Validator},
    };

    fn asset(id: &str) -> AssetRecord {
        AssetRecord {
            id: id.to_string(),
            path: format!("Content/{id}.mesh"),
            kind: Some("mesh".to_string()),
            metadata: AssetMetadata::default(),
        }
    }

    #[test]
    fn downgrades_high_confidence_when_margin_is_small() {
        let validator = ThresholdValidator::new(ValidatorConfig::default());
        let decision = validator.validate(ScoredMatch {
            old_asset: asset("old"),
            candidate: Some(CandidateMatch {
                new_asset: asset("new"),
                confidence: 0.90,
                reasons: vec![MatchReason {
                    code: "name_exact".to_string(),
                    message: "exact name".to_string(),
                    contribution: 0.25,
                }],
            }),
            second_best_confidence: Some(0.86),
        });

        assert_eq!(decision.status, MatchStatus::NeedsReview);
        let gap = decision.top_candidate_gap.expect("expected candidate gap");
        assert!((gap - 0.04).abs() < 0.000_1);
    }

    #[test]
    fn downgrades_matched_candidate_when_vertex_count_grows() {
        let validator = ThresholdValidator::new(ValidatorConfig::default());
        let mut old_asset = asset("old");
        old_asset.metadata.vertex_count = Some(12000);
        old_asset.metadata.index_count = Some(18000);
        let mut new_asset = asset("new");
        new_asset.metadata.vertex_count = Some(24000);
        new_asset.metadata.index_count = Some(18000);

        let decision = validator.validate(ScoredMatch {
            old_asset,
            candidate: Some(CandidateMatch {
                new_asset,
                confidence: 0.95,
                reasons: vec![MatchReason {
                    code: "name_exact".to_string(),
                    message: "exact name".to_string(),
                    contribution: 0.25,
                }],
            }),
            second_best_confidence: None,
        });

        assert_eq!(decision.status, MatchStatus::NeedsReview);
        assert!(
            decision
                .reasons
                .iter()
                .any(|reason| reason.code == "vertex_count_resize_risk")
        );
    }

    #[test]
    fn keeps_matched_candidate_when_layout_is_stable() {
        let validator = ThresholdValidator::new(ValidatorConfig::default());
        let mut old_asset = asset("old");
        old_asset.metadata.vertex_count = Some(12000);
        old_asset.metadata.index_count = Some(18000);
        old_asset.metadata.material_slots = Some(2);
        let mut new_asset = asset("new");
        new_asset.metadata.vertex_count = Some(12000);
        new_asset.metadata.index_count = Some(18000);
        new_asset.metadata.material_slots = Some(2);

        let decision = validator.validate(ScoredMatch {
            old_asset,
            candidate: Some(CandidateMatch {
                new_asset,
                confidence: 0.95,
                reasons: vec![MatchReason {
                    code: "name_exact".to_string(),
                    message: "exact name".to_string(),
                    contribution: 0.25,
                }],
            }),
            second_best_confidence: None,
        });

        assert_eq!(decision.status, MatchStatus::Matched);
        assert!(
            !decision
                .reasons
                .iter()
                .any(|reason| reason.code.contains("resize_risk"))
        );
    }
}
