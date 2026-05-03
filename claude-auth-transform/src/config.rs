pub struct ModelConfig<'a> {
    pub cc_version: &'a str,
    pub base_betas: &'a [&'a str],
    // pub long_context_betas: &'a [&'a str],
    pub model_overrides: &'a [ModelOverride<'a>],
}

pub struct ModelOverride<'a> {
    pub model: &'a str,
    pub exclude: &'a [&'a str],
    pub add: &'a [&'a str],
    pub disable_effort: bool,
}

impl ModelConfig<'_> {
    /// Find the override entry matching a model ID.
    /// Keys are matched via `contains` against the lowercased model ID.
    ///
    /// First-match - wins: if multiple keys match, only the first (by insertion
    /// order) is returned. List more specific keys before broader ones
    /// (e.g. "opus-4-6" before "opus") so they take priority.
    pub fn get_model_override(&self, model: &str) -> Option<&ModelOverride<'_>> {
        let lower = model.to_lowercase();
        for override_ in self.model_overrides {
            let pattern = override_.model;
            if lower.contains(pattern) {
                return Some(override_);
            }
        }
        None
    }
}

pub const CONFIG: ModelConfig<'static> = ModelConfig {
    cc_version: "2.1.112",
    base_betas: &[
        "claude-code-20250219",
        "oauth-2025-04-20",
        "interleaved-thinking-2025-05-14",
        "prompt-caching-scope-2026-01-05",
        "context-management-2025-06-27",
        "advisor-tool-2026-03-01",
    ],
    // long_context_betas: &["context-1m-2025-08-07", "interleaved-thinking-2025-05-14"],
    model_overrides: &[
        ModelOverride {
            model: "haiku",
            exclude: &["interleaved-thinking-2025-05-14"],
            add: &[],
            disable_effort: true,
        },
        ModelOverride {
            model: "4-6",
            exclude: &[],
            add: &["effort-2025-11-24"],
            disable_effort: false,
        },
        ModelOverride {
            model: "4-7",
            exclude: &[],
            add: &["effort-2025-11-24"],
            disable_effort: false,
        },
    ],
};
