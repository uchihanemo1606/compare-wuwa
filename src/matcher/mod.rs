use crate::{
    config::MatcherConfig,
    domain::{AssetRecord, MatchReason},
    fingerprint::AssetFingerprint,
};

pub trait Matcher {
    fn best_matches(
        &self,
        old_fingerprints: &[AssetFingerprint],
        new_fingerprints: &[AssetFingerprint],
    ) -> Vec<ScoredMatch>;
}

#[derive(Debug, Clone)]
pub struct HeuristicMatcher {
    config: MatcherConfig,
}

impl HeuristicMatcher {
    pub fn new(config: MatcherConfig) -> Self {
        Self { config }
    }

    fn score_pair(
        &self,
        old: &AssetFingerprint,
        new: &AssetFingerprint,
    ) -> Option<(f32, Vec<MatchReason>)> {
        let weights = &self.config.weights;
        let mut score = 0.0;
        let mut reasons = Vec::new();

        if old.normalized_kind.is_some() && old.normalized_kind == new.normalized_kind {
            score += weights.kind;
            reasons.push(reason(
                "kind_exact",
                format!(
                    "asset kind matched exactly: {}",
                    old.normalized_kind.as_deref().unwrap_or("unknown")
                ),
                weights.kind,
            ));
        }

        if old.normalized_name.is_some() && old.normalized_name == new.normalized_name {
            score += weights.exact_name;
            reasons.push(reason(
                "name_exact",
                format!(
                    "logical or derived name matched exactly: {}",
                    old.normalized_name.as_deref().unwrap_or("unknown")
                ),
                weights.exact_name,
            ));
        } else {
            let overlap = jaccard(&old.name_tokens, &new.name_tokens);
            if overlap > 0.0 {
                let contribution = overlap * weights.name_token_overlap;
                score += contribution;
                reasons.push(reason(
                    "name_token_overlap",
                    format!("name token overlap score: {overlap:.3}"),
                    contribution,
                ));
            }
        }

        let path_overlap = jaccard(&old.path_tokens, &new.path_tokens);
        if path_overlap > 0.0 {
            let contribution = path_overlap * weights.path_token_overlap;
            score += contribution;
            reasons.push(reason(
                "path_token_overlap",
                format!("path token overlap score: {path_overlap:.3}"),
                contribution,
            ));
        }

        let tag_overlap = jaccard(&old.tags, &new.tags);
        if tag_overlap > 0.0 {
            let contribution = tag_overlap * weights.tag_overlap;
            score += contribution;
            reasons.push(reason(
                "tag_overlap",
                format!("tag overlap score: {tag_overlap:.3}"),
                contribution,
            ));
        }

        add_exact_numeric_match(
            &mut score,
            &mut reasons,
            "vertex_count_exact",
            "vertex count matched exactly",
            old.vertex_count,
            new.vertex_count,
            weights.vertex_count,
        );
        add_exact_numeric_match(
            &mut score,
            &mut reasons,
            "index_count_exact",
            "index count matched exactly",
            old.index_count,
            new.index_count,
            weights.index_count,
        );
        add_exact_numeric_match(
            &mut score,
            &mut reasons,
            "material_slots_exact",
            "material slot count matched exactly",
            old.material_slots,
            new.material_slots,
            weights.material_slots,
        );
        add_exact_numeric_match(
            &mut score,
            &mut reasons,
            "section_count_exact",
            "section count matched exactly",
            old.section_count,
            new.section_count,
            weights.section_count,
        );

        if reasons.is_empty() {
            None
        } else {
            Some((score.clamp(0.0, 1.0), reasons))
        }
    }
}

impl Matcher for HeuristicMatcher {
    fn best_matches(
        &self,
        old_fingerprints: &[AssetFingerprint],
        new_fingerprints: &[AssetFingerprint],
    ) -> Vec<ScoredMatch> {
        old_fingerprints
            .iter()
            .map(|old| {
                let mut candidates = new_fingerprints
                    .iter()
                    .filter_map(|new| {
                        self.score_pair(old, new)
                            .map(|(confidence, reasons)| CandidateMatch {
                                new_asset: new.asset.clone(),
                                confidence,
                                reasons,
                            })
                    })
                    .collect::<Vec<_>>();

                candidates.sort_by(|left, right| {
                    right
                        .confidence
                        .total_cmp(&left.confidence)
                        .then_with(|| left.new_asset.path.cmp(&right.new_asset.path))
                        .then_with(|| left.new_asset.id.cmp(&right.new_asset.id))
                });

                let second_best_confidence =
                    candidates.get(1).map(|candidate| candidate.confidence);
                let candidate = candidates.into_iter().next();

                ScoredMatch {
                    old_asset: old.asset.clone(),
                    candidate,
                    second_best_confidence,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct CandidateMatch {
    pub new_asset: AssetRecord,
    pub confidence: f32,
    pub reasons: Vec<MatchReason>,
}

#[derive(Debug, Clone)]
pub struct ScoredMatch {
    pub old_asset: AssetRecord,
    pub candidate: Option<CandidateMatch>,
    pub second_best_confidence: Option<f32>,
}

fn add_exact_numeric_match(
    score: &mut f32,
    reasons: &mut Vec<MatchReason>,
    code: &str,
    message: &str,
    left: Option<u32>,
    right: Option<u32>,
    weight: f32,
) {
    if left.is_some() && left == right {
        *score += weight;
        reasons.push(reason(code, message.to_string(), weight));
    }
}

fn reason(code: &str, message: String, contribution: f32) -> MatchReason {
    MatchReason {
        code: code.to_string(),
        message,
        contribution,
    }
}

fn jaccard(
    left: &std::collections::BTreeSet<String>,
    right: &std::collections::BTreeSet<String>,
) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let intersection = left.intersection(right).count() as f32;
    let union = left.union(right).count() as f32;

    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::MatcherConfig,
        domain::{AssetMetadata, AssetRecord},
        fingerprint::{DefaultFingerprinter, Fingerprinter},
        matcher::{HeuristicMatcher, Matcher},
    };

    fn asset(
        id: &str,
        path: &str,
        logical_name: &str,
        vertex_count: u32,
        index_count: u32,
    ) -> AssetRecord {
        AssetRecord {
            id: id.to_string(),
            path: path.to_string(),
            kind: Some("mesh".to_string()),
            metadata: AssetMetadata {
                logical_name: Some(logical_name.to_string()),
                vertex_count: Some(vertex_count),
                index_count: Some(index_count),
                material_slots: Some(2),
                section_count: Some(1),
                tags: vec!["hero".to_string(), "body".to_string()],
                ..Default::default()
            },
        }
    }

    #[test]
    fn prefers_exact_name_and_numeric_match() {
        let fingerprinter = DefaultFingerprinter;
        let matcher = HeuristicMatcher::new(MatcherConfig::default());
        let old_assets = [asset(
            "old",
            "Content/Character/HeroA/Body.mesh",
            "HeroA_Body",
            12000,
            18000,
        )];
        let new_assets = [
            asset(
                "new-good",
                "Content/Character/HeroA/Body_v2.mesh",
                "HeroA_Body",
                12000,
                18000,
            ),
            asset(
                "new-weak",
                "Content/Character/HeroB/Body.mesh",
                "HeroB_Body",
                9000,
                14000,
            ),
        ];

        let old_fingerprints = old_assets
            .iter()
            .map(|asset| fingerprinter.fingerprint(asset))
            .collect::<Vec<_>>();
        let new_fingerprints = new_assets
            .iter()
            .map(|asset| fingerprinter.fingerprint(asset))
            .collect::<Vec<_>>();

        let result = matcher.best_matches(&old_fingerprints, &new_fingerprints);
        let best = result[0].candidate.as_ref().expect("expected candidate");

        assert_eq!(best.new_asset.id, "new-good");
        assert!(best.confidence > 0.80);
    }
}
