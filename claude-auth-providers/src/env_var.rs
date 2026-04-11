use tracing::debug;

use crate::{ClaudeAuthProvider, Error};

/// Environment variable name holding a Claude access token.
const ACCESS_TOKEN_ENV: &str = "CLAUDE_ACCESS_TOKEN";

/// Auth provider that returns a static access token read from the
/// `CLAUDE_ACCESS_TOKEN` environment variable.
///
/// Intended for CI, Docker, and testing scenarios where an OAuth-based
/// provider is impractical. There is no refresh path: the token is used
/// as-is for the lifetime of the process.
#[derive(Debug)]
pub struct EnvVarAuthProvider {
    token: String,
}

impl EnvVarAuthProvider {
    /// Constructs a provider from `CLAUDE_ACCESS_TOKEN` if it is set and
    /// non-empty, otherwise returns `None`.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let token = std::env::var(ACCESS_TOKEN_ENV).ok()?;
        if token.is_empty() {
            return None;
        }
        debug!("Loaded access token from {ACCESS_TOKEN_ENV}");
        Some(Self { token })
    }
}

impl ClaudeAuthProvider for EnvVarAuthProvider {
    async fn get_access_token(&self) -> Result<String, Error> {
        Ok(self.token.clone())
    }

    fn has_credentials(&self) -> bool {
        true
    }
}
