use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub matcher: MatcherConfig,
    pub validator: ValidatorConfig,
}

impl AppConfig {
    pub fn load(path: Option<&Path>) -> AppResult<Self> {
        let config = match path {
            Some(path) => serde_json::from_str(&fs::read_to_string(path)?)?,
            None => Self::default(),
        };

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> AppResult<()> {
        let weight_sum = self.matcher.weights.total();
        if (weight_sum - 1.0).abs() > 0.001 {
            return Err(AppError::InvalidConfig(format!(
                "matcher weights must sum to 1.0, got {weight_sum:.3}"
            )));
        }

        if !(0.0..=1.0).contains(&self.validator.needs_review_threshold)
            || !(0.0..=1.0).contains(&self.validator.matched_threshold)
            || !(0.0..=1.0).contains(&self.validator.minimum_margin)
        {
            return Err(AppError::InvalidConfig(
                "validator thresholds must be between 0.0 and 1.0".to_string(),
            ));
        }

        if self.validator.matched_threshold < self.validator.needs_review_threshold {
            return Err(AppError::InvalidConfig(
                "matched_threshold must be greater than or equal to needs_review_threshold"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MatcherConfig {
    pub weights: MatcherWeights,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MatcherWeights {
    pub kind: f32,
    pub exact_name: f32,
    pub name_token_overlap: f32,
    pub path_token_overlap: f32,
    pub vertex_count: f32,
    pub index_count: f32,
    pub material_slots: f32,
    pub section_count: f32,
    pub tag_overlap: f32,
}

impl MatcherWeights {
    pub fn total(&self) -> f32 {
        self.kind
            + self.exact_name
            + self.name_token_overlap
            + self.path_token_overlap
            + self.vertex_count
            + self.index_count
            + self.material_slots
            + self.section_count
            + self.tag_overlap
    }
}

impl Default for MatcherWeights {
    fn default() -> Self {
        Self {
            kind: 0.10,
            exact_name: 0.25,
            name_token_overlap: 0.10,
            path_token_overlap: 0.15,
            vertex_count: 0.15,
            index_count: 0.10,
            material_slots: 0.05,
            section_count: 0.05,
            tag_overlap: 0.05,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidatorConfig {
    pub matched_threshold: f32,
    pub needs_review_threshold: f32,
    pub minimum_margin: f32,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            matched_threshold: 0.85,
            needs_review_threshold: 0.55,
            minimum_margin: 0.10,
        }
    }
}
