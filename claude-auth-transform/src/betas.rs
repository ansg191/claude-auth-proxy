use std::{
    collections::{HashMap, HashSet},
    sync::RwLock,
};

use crate::{TransformConfig, config::CONFIG};

fn get_required_betas(config: &TransformConfig) -> Vec<String> {
    config.beta_flags_override.as_ref().map_or_else(
        || CONFIG.base_betas.iter().map(|s| (*s).to_owned()).collect(),
        Clone::clone,
    )
}

#[derive(Debug)]
pub struct BetaManager {
    /// Session-level cache of excluded beta flags per model
    excluded_betas: RwLock<HashMap<String, HashSet<String>>>,
}

impl BetaManager {
    pub fn new() -> Self {
        Self {
            excluded_betas: RwLock::new(HashMap::new()),
        }
    }

    // TODO: add & clear excluded beta methods

    pub fn get_model_betas(&self, model: &str, config: &TransformConfig) -> Vec<String> {
        let mut betas = get_required_betas(config);

        // TODO: handle 1m context beta

        // Apply per-model overrides (e.g., haiku excludes claude-code-20250219)
        if let Some(override_) = CONFIG.get_model_override(model) {
            betas.retain(|b| !override_.exclude.contains(&b.as_str()));
            betas.extend(override_.add.iter().map(|s| (*s).to_owned()));
        }

        // Filter out excluded betas (from previous failed requests due to long context errors)
        if let Some(excluded_betas) = self.excluded_betas.read().unwrap().get(model) {
            betas.retain(|b| !excluded_betas.contains(b));
        }

        // betas.sort_unstable();
        betas.dedup();
        betas
    }
}
