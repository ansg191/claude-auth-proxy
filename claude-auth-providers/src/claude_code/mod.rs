use std::{collections::HashSet, sync::RwLock};

#[cfg(target_os = "macos")]
use tracing::error;

use crate::{ClaudeAuthProvider, Error, claude_code::credential::ClaudeCredential};

mod credential;
mod credentials_file;
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
    #[must_use]
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

        // Source 1: macOS Keychain (macOS only)
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

        // Source 2: ~/.claude/.credentials.json (cross-platform fallback)
        creds.extend(credentials_file::get_credentials());

        // Deduplicate by access_token — keychain and file may contain the
        // same credential on macOS (Claude Code mirrors to both locations).
        let mut seen = HashSet::new();
        creds.retain(|c| seen.insert(c.access_token.clone()));

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
        !self.creds.read().expect("Poisoned Lock").is_empty()
    }
}
