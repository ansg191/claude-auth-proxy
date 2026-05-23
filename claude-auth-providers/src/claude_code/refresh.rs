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

const EXPIRE_BUFFER: Duration = Duration::from_hours(1);
const CLI_TIMEOUT: Duration = Duration::from_mins(1);
#[cfg(not(test))]
const FILE_RELOAD_TTL: Duration = Duration::from_secs(30);
#[cfg(test)]
const FILE_RELOAD_TTL: Duration = Duration::from_millis(1);

pub async fn refresh_access_token(
    auth: &ClaudeCodeAuthProvider,
    mut creds: ClaudeCredential,
    force: bool,
) -> Result<ClaudeCredential, Error> {
    if maybe_reload_credentials(auth)
        && let Some(reloaded) = auth.get_active_credential()
    {
        creds = reloaded;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time before UNIX EPOCH")
        .as_secs();

    if !force && now + EXPIRE_BUFFER.as_secs() < creds.expires_at {
        return Ok(creds);
    }

    // First try refreshing via the API (only if we have a refresh token)
    if let Some(refresh_token) = creds.refresh_token.as_deref() {
        let result = refresh_oauth(refresh_token).await;
        if let Ok(cred) = result {
            // Replacing the old credential with the new one
            let mut creds_guard = auth.creds.write().expect("Poisoned Lock");
            *creds_guard
                .get_mut(auth.active)
                .expect("Active credential index out of bounds") = cred.clone();
            return Ok(cred);
        }
    }

    // If that fails, try refreshing via the CLI
    refresh_cli(auth).await
}

fn maybe_reload_credentials(auth: &ClaudeCodeAuthProvider) -> bool {
    let should_reload = auth
        .last_reload_at
        .lock()
        .expect("Poisoned Lock")
        .is_none_or(|last| last.elapsed() >= FILE_RELOAD_TTL);

    if should_reload {
        auth.reload();
    }

    should_reload
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
        .await
        .map_err(Error::FailedOAuthRequest)?
        .error_for_status()
        .map_err(Error::FailedOAuthRequest)?;

    let body = res.text().await.map_err(Error::FailedOAuthRequest)?;
    let response: OAuthResponse =
        serde_json::from_str(&body).map_err(Error::FailedOAuthResponse)?;

    if let Some(error) = response.error {
        error!("Failed to refresh access token: {}", error);
        return Err(Error::Refresh(error));
    }

    Ok(ClaudeCredential {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
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
        let mut child = Command::new("claude")
            .args(["-p", ".", "--model", "haiku"])
            .env("TERM", "dumb")
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(Error::ClaudeCodeSpawn)?;

        let status = if let Ok(status) = tokio::time::timeout(CLI_TIMEOUT, child.wait()).await {
            status.map_err(Error::ClaudeCodeSpawn)?
        } else {
            error!(
                attempt = i,
                timeout_secs = CLI_TIMEOUT.as_secs(),
                "CLI refresh timed out"
            );
            let _ = child.start_kill();
            let _ = child.wait().await;
            continue;
        };

        if status.success() {
            // After refreshing via CLI, we need to reload credentials from the keychain
            auth.reload();
            return auth.get_active_credential().ok_or_else(|| {
                Error::Refresh("Failed to get active credential after CLI refresh".into())
            });
        }

        info!(%status, "CLI failed to refresh access token, retrying");
    }

    Err(Error::Refresh(
        "Failed to refresh access token via CLI".into(),
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        sync::Mutex,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use crate::ClaudeAuthProvider;

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn now_epoch_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before UNIX EPOCH")
            .as_secs()
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create tokio runtime")
    }

    fn write_credentials_file(
        path: &std::path::Path,
        access: &str,
        refresh: &str,
        expires_at: u64,
    ) {
        let mut file = fs::File::create(path).expect("create credentials file");
        write!(
            file,
            r#"{{"accessToken":"{access}","refreshToken":"{refresh}","expiresAt":{expires_at}}}"#
        )
        .expect("write credentials file");
    }

    #[test]
    fn observes_external_credentials_file_update() {
        let _guard = ENV_LOCK.lock().expect("Poisoned Lock");

        let mut path = std::env::temp_dir();
        path.push(format!(
            "claude-auth-proxy-refresh-external-{}-{}.json",
            std::process::id(),
            now_epoch_secs(),
        ));

        write_credentials_file(&path, "old_access", "old_refresh", now_epoch_secs() + 7200);
        unsafe { std::env::set_var("CLAUDE_CREDENTIALS_FILE", &path) };

        let auth = ClaudeCodeAuthProvider::new();
        let runtime = runtime();
        assert_eq!(
            runtime
                .block_on(auth.get_access_token())
                .expect("first token"),
            "old_access"
        );

        write_credentials_file(&path, "new_access", "new_refresh", now_epoch_secs() + 7200);
        std::thread::sleep(FILE_RELOAD_TTL + Duration::from_millis(5));

        assert_eq!(
            runtime
                .block_on(auth.get_access_token())
                .expect("reloaded token"),
            "new_access"
        );

        unsafe { std::env::remove_var("CLAUDE_CREDENTIALS_FILE") };
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn skips_oauth_refresh_when_disk_credentials_are_fresh() {
        let _guard = ENV_LOCK.lock().expect("Poisoned Lock");

        let mut path = std::env::temp_dir();
        path.push(format!(
            "claude-auth-proxy-refresh-fresh-disk-{}-{}.json",
            std::process::id(),
            now_epoch_secs(),
        ));

        write_credentials_file(
            &path,
            "fresh_access",
            "fresh_refresh",
            now_epoch_secs() + 7200,
        );
        unsafe { std::env::set_var("CLAUDE_CREDENTIALS_FILE", &path) };

        let auth = ClaudeCodeAuthProvider::new();
        {
            let mut guard = auth.creds.write().expect("Poisoned Lock");
            guard[0] = ClaudeCredential {
                access_token: "stale_access".into(),
                refresh_token: Some("stale_refresh".into()),
                expires_at: now_epoch_secs() + 10,
                subscription_type: None,
            };
        }
        *auth.last_reload_at.lock().expect("Poisoned Lock") = None;

        let stale = auth.get_active_credential().expect("stale credential");
        let runtime = runtime();
        let refreshed = runtime
            .block_on(refresh_access_token(&auth, stale, false))
            .expect("should use fresh disk credential");

        assert_eq!(refreshed.access_token, "fresh_access");
        assert_eq!(
            auth.get_active_credential()
                .expect("active credential")
                .access_token,
            "fresh_access"
        );

        unsafe { std::env::remove_var("CLAUDE_CREDENTIALS_FILE") };
        let _ = fs::remove_file(&path);
    }
}
