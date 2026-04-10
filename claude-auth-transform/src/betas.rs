use std::{
    collections::{HashMap, HashSet},
    env,
    sync::{LazyLock, RwLock},
};

use crate::config::CONFIG;

static ENV_BETA_FLAGS: LazyLock<Option<String>> =
    LazyLock::new(|| env::var("ANTHROPIC_BETA_FLAGS").ok());

fn get_required_betas() -> Vec<&'static str> {
    if let Some(flags) = ENV_BETA_FLAGS.as_deref() {
        flags
            .split(',')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .collect()
    } else {
        Vec::from(CONFIG.base_betas)
    }
}

pub static BETA_MANAGER: LazyLock<BetaManager> = LazyLock::new(BetaManager::new);

pub struct BetaManager {
    /// Session-level cache of excluded beta flags per model
    excluded_betas: RwLock<HashMap<String, HashSet<&'static str>>>,
}

impl BetaManager {
    pub fn new() -> Self {
        Self {
            excluded_betas: RwLock::new(HashMap::new()),
        }
    }

    // TODO: add & clear excluded beta methods

    pub fn get_model_betas(&self, model: &str) -> Vec<&'static str> {
        let mut betas = get_required_betas();

        // TODO: handle 1m context beta

        // Apply per-model overrides (e.g., haiku excludes claude-code-20250219)
        if let Some(override_) = CONFIG.get_model_override(model) {
            betas.retain(|b| !override_.exclude.contains(b));
            betas.extend_from_slice(override_.add);
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
