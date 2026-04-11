pub mod claude_code;

pub trait ClaudeAuthProvider {
    fn get_access_token(&self) -> impl Future<Output = Result<String, Error>>;
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
