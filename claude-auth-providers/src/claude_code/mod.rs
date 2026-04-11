use std::sync::RwLock;

use tracing::error;

use crate::{ClaudeAuthProvider, Error, claude_code::credential::ClaudeCredential};

mod credential;
#[cfg(target_os = "macos")]
mod keychain;
mod refresh;

#[derive(Debug)]
pub struct ClaudeCodeAuthProvider {
    creds: RwLock<Vec<ClaudeCredential>>,
    active: usize,
}

impl Default for ClaudeCodeAuthProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeAuthProvider {
    pub fn new() -> Self {
        let this = Self {
            creds: RwLock::new(Vec::new()),
            active: 0,
        };
        this.reload();
        this
    }

    fn reload(&self) {
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

        *self.creds.write().expect("Poisoned Lock") = creds;
    }

    fn get_active_credential(&self) -> Option<ClaudeCredential> {
        self.creds
            .read()
            .expect("Poisoned Lock")
            .get(self.active)
            .cloned()
    }
}

impl ClaudeAuthProvider for ClaudeCodeAuthProvider {
    async fn get_access_token(&self) -> Result<String, Error> {
        let creds = self.get_active_credential().ok_or(Error::NoCredentials)?;
        let creds = refresh::refresh_access_token(self, creds).await?;
        Ok(creds.access_token)
    }

    fn has_credentials(&self) -> bool {
        !self.creds.is_empty()
    }
}
