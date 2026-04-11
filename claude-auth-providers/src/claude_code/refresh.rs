use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;
use tracing::{debug, error, info};

use crate::{
    Error,
    claude_code::{ClaudeCodeAuthProvider, credential::ClaudeCredential},
};

const OAUTH_TOKEN_URL: &str = "https://claude.ai/v1/oauth/token";
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

const EXPIRE_BUFFER: Duration = Duration::from_mins(60);

pub async fn refresh_access_token(
    auth: &ClaudeCodeAuthProvider,
    creds: ClaudeCredential,
) -> Result<ClaudeCredential, Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time before UNIX EPOCH")
        .as_secs();

    if now + EXPIRE_BUFFER.as_secs() >= creds.expires_at {
        return Ok(creds);
    }

    // First try refreshing via the API
    let result = refresh_oauth(&creds.refresh_token).await;
    if let Ok(cred) = result {
        // Replacing the old credential with the new one
        auth.creds
            .write()
            .expect("Poisoned Lock")
            .insert(auth.active, cred.clone());
        return Ok(cred);
    }

    // If that fails, try refreshing via the CLI
    refresh_cli(auth).await
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn refresh_oauth(refresh_token: &str) -> Result<ClaudeCredential, Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("claude-auth-providers/0.1.0")
        .build()
        .expect("Failed to build HTTP client");

    let body = format!(
        "grant_type=refresh_token&client_id={OAUTH_CLIENT_ID}&refresh_token={refresh_token}"
    );
    let body = urlencoding::encode(&body);

    let res = client
        .post(OAUTH_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body.into_owned())
        .send()
        .await?
        .error_for_status()?;

    let body = res.text().await?;
    let response: OAuthResponse = serde_json::from_str(&body)?;

    if let Some(error) = response.error {
        error!("Failed to refresh access token: {}", error);
        return Err(Error::Refresh(error));
    }

    Ok(ClaudeCredential {
        access_token: response.access_token,
        refresh_token: response.refresh_token.unwrap_or_default(),
        expires_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time before UNIX EPOCH")
            .as_secs()
            + response.expires_in,
        subscription_type: None,
    })
}

async fn refresh_cli(auth: &ClaudeCodeAuthProvider) -> Result<ClaudeCredential, Error> {
    const MAX_ATTEMPTS: usize = 3;

    for i in 0..MAX_ATTEMPTS {
        debug!(attempt = i, "Attempting to refresh access token using CLI");
        let status = Command::new("claude")
            .args(["-p", ".", "--model", "haiku"])
            .env("TERM", "dumb")
            .stdin(std::process::Stdio::null())
            .status()
            .await?;

        if status.success() {
            // After refreshing via CLI, we need to reload credentials from the keychain
            auth.reload();
            return Ok(auth.get_active_credential().ok_or_else(|| {
                Error::Refresh("Failed to get active credential after CLI refresh".into())
            })?);
        }

        info!(%status, "CLI failed to refresh access token, retrying");
    }

    Err(Error::Refresh(
        "Failed to refresh access token via CLI".into(),
    ))
}
