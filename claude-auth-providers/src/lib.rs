pub mod claude_code;
pub mod env_var;

use crate::{claude_code::ClaudeCodeAuthProvider, env_var::EnvVarAuthProvider};

pub trait ClaudeAuthProvider {
    fn get_access_token(&self) -> impl Future<Output = Result<String, Error>>;
    /// Attempt to force-refresh credentials.
    ///
    /// Implementations that override this method may bypass any expiry cache
    /// and obtain a genuinely new token, for example after an upstream 401.
    /// The default implementation simply delegates to
    /// [`get_access_token`](Self::get_access_token), so it may still return a
    /// cached or otherwise non-refreshed token.
    fn force_refresh_token(&self) -> impl Future<Output = Result<String, Error>> {
        self.get_access_token()
    }
    fn has_credentials(&self) -> bool {
        true
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("No credentials found or available")]
    NoCredentials,
    #[cfg(target_os = "macos")]
    #[error("Failed to read credentials from keychain: {0}")]
    Keychain(#[from] security_framework::base::Error),
    #[error("Failed to refresh access token: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Failed to parse response: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Failed to refresh access token: {0}")]
    Refresh(String),
    #[error("Spawn Failed: {0}")]
    Spawn(#[from] std::io::Error),
}

/// Composite auth provider that dispatches to a concrete implementation
/// chosen at runtime.
///
/// The trait's `-> impl Future` return type isn't dyn-compatible, so this
/// enum provides static dispatch across the available providers.
#[derive(Debug)]
pub enum AnyAuthProvider {
    ClaudeCode(ClaudeCodeAuthProvider),
    EnvVar(EnvVarAuthProvider),
}

impl AnyAuthProvider {
    /// Constructs an auth provider by inspecting the environment.
    ///
    /// Prefers `CLAUDE_ACCESS_TOKEN` if set (explicit user override);
    /// otherwise falls back to `ClaudeCodeAuthProvider`, which reads from
    /// the macOS Keychain and `~/.claude/.credentials.json`.
    #[must_use]
    pub fn from_env() -> Self {
        EnvVarAuthProvider::from_env().map_or_else(
            || Self::ClaudeCode(ClaudeCodeAuthProvider::new()),
            Self::EnvVar,
        )
    }
}

impl ClaudeAuthProvider for AnyAuthProvider {
    async fn get_access_token(&self) -> Result<String, Error> {
        match self {
            Self::ClaudeCode(p) => p.get_access_token().await,
            Self::EnvVar(p) => p.get_access_token().await,
        }
    }

    async fn force_refresh_token(&self) -> Result<String, Error> {
        match self {
            Self::ClaudeCode(p) => p.force_refresh_token().await,
            Self::EnvVar(p) => p.force_refresh_token().await,
        }
    }

    fn has_credentials(&self) -> bool {
        match self {
            Self::ClaudeCode(p) => p.has_credentials(),
            Self::EnvVar(p) => p.has_credentials(),
        }
    }
}
