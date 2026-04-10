use tracing::error;

use crate::{ClaudeAuthProvider, Error, claude_code::credential::ClaudeCredential};

mod credential;
#[cfg(target_os = "macos")]
mod keychain;

#[derive(Debug)]
pub struct ClaudeCodeAuthProvider {
    creds: Vec<ClaudeCredential>,
    active: usize,
}

impl Default for ClaudeCodeAuthProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeAuthProvider {
    pub fn new() -> Self {
        let mut creds = Vec::new();

        #[cfg(target_os = "macos")]
        {
            let res = keychain::get_credentials();
            match res {
                Ok(c) => creds.extend(c),
                Err(e) => {
                    error!("Failed to read credentials from keychain: {}", e);
                }
            }
        }

        Self { creds, active: 0 }
    }
}

impl ClaudeAuthProvider for ClaudeCodeAuthProvider {
    async fn get_access_token(&self) -> Result<String, Error> {
        // TODO: Implement expiry checks & refreshing

        self.creds
            .get(self.active)
            .map(|cred| cred.access_token.clone())
            .ok_or(Error::NoCredentials)
    }
}
