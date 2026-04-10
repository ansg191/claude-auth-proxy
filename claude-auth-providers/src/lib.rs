pub mod claude_code;

pub trait ClaudeAuthProvider {
    fn get_access_token(&self) -> impl Future<Output = Result<String, Error>>;
    fn has_credentials(&self) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("No credentials found or available")]
    NoCredentials,
    #[cfg(target_os = "macos")]
    #[error("Failed to read credentials from keychain: {0}")]
    Keychain(#[from] security_framework::base::Error),
}
