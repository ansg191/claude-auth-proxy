use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;
use tracing::{debug, error, info};

use crate::{
    Error,
    claude_code::{ClaudeCodeAuthProvider, credential::ClaudeCredential, credentials_file},
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
    if let Some(reloaded) = maybe_reload_credentials(auth)
        && reloaded.expires_at >= creds.expires_at
    {
        creds = reloaded;
        let mut creds_guard = auth.creds.write().expect("Poisoned Lock");
        if let Some(active) = creds_guard.get_mut(auth.active) {
            *active = creds.clone();
        }
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

fn maybe_reload_credentials(auth: &ClaudeCodeAuthProvider) -> Option<ClaudeCredential> {
    let should_reload = auth
        .last_reload_at
        .lock()
        .expect("Poisoned Lock")
        .is_none_or(|last| last.elapsed() >= FILE_RELOAD_TTL);
    if !should_reload {
        return None;
    }

    let Some(reloaded) = credentials_file::get_credentials().into_iter().next() else {
        debug!("No file-backed credential found during refresh-path reload");
        return None;
    };
    *auth.last_reload_at.lock().expect("Poisoned Lock") = Some(std::time::Instant::now());
    Some(reloaded)
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
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use crate::ClaudeAuthProvider;

    use super::*;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    const TEST_TIMING_BUFFER: Duration = Duration::from_millis(20);
    const TEST_TOKEN_EXPIRY_OFFSET: u64 = 7_200;

    fn now_epoch_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before UNIX EPOCH")
            .as_secs()
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

    #[tokio::test(flavor = "current_thread")]
    async fn observes_external_credentials_file_update() {
        let _guard = ENV_LOCK.lock().await;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "claude-auth-proxy-refresh-external-{}-{}.json",
            std::process::id(),
            now_epoch_secs(),
        ));

        write_credentials_file(
            &path,
            "old_access",
            "old_refresh",
            now_epoch_secs() + TEST_TOKEN_EXPIRY_OFFSET,
        );
        credentials_file::set_test_credentials_file_path(Some(path.clone()));

        let auth = ClaudeCodeAuthProvider::new();
        assert_eq!(
            auth.get_access_token().await.expect("first token"),
            "old_access"
        );

        write_credentials_file(
            &path,
            "new_access",
            "new_refresh",
            now_epoch_secs() + TEST_TOKEN_EXPIRY_OFFSET,
        );
        std::thread::sleep(FILE_RELOAD_TTL + TEST_TIMING_BUFFER);

        assert_eq!(
            auth.get_access_token().await.expect("reloaded token"),
            "new_access"
        );

        credentials_file::set_test_credentials_file_path(None);
        let _ = fs::remove_file(&path);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn skips_oauth_refresh_when_disk_credentials_are_fresh() {
        let _guard = ENV_LOCK.lock().await;

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
            now_epoch_secs() + TEST_TOKEN_EXPIRY_OFFSET,
        );
        credentials_file::set_test_credentials_file_path(Some(path.clone()));

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
        let refreshed = refresh_access_token(&auth, stale, false)
            .await
            .expect("should use fresh disk credential");

        assert_eq!(refreshed.access_token, "fresh_access");
        assert_eq!(
            auth.get_active_credential()
                .expect("active credential")
                .access_token,
            "fresh_access"
        );

        credentials_file::set_test_credentials_file_path(None);
        let _ = fs::remove_file(&path);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn does_not_replace_fresher_in_memory_credential_with_stale_disk_credential() {
        let _guard = ENV_LOCK.lock().await;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "claude-auth-proxy-refresh-stale-disk-{}-{}.json",
            std::process::id(),
            now_epoch_secs(),
        ));

        write_credentials_file(&path, "disk_access", "disk_refresh", now_epoch_secs() + 100);
        credentials_file::set_test_credentials_file_path(Some(path.clone()));

        let auth = ClaudeCodeAuthProvider::new();
        let fresher = ClaudeCredential {
            access_token: "memory_access".into(),
            refresh_token: Some("memory_refresh".into()),
            expires_at: now_epoch_secs() + TEST_TOKEN_EXPIRY_OFFSET,
            subscription_type: None,
        };
        {
            let mut guard = auth.creds.write().expect("Poisoned Lock");
            guard[0] = fresher.clone();
        }
        *auth.last_reload_at.lock().expect("Poisoned Lock") = None;

        let refreshed = refresh_access_token(&auth, fresher, false)
            .await
            .expect("should keep fresher in-memory credential");

        assert_eq!(refreshed.access_token, "memory_access");
        assert_eq!(
            auth.get_active_credential()
                .expect("active credential")
                .access_token,
            "memory_access"
        );

        credentials_file::set_test_credentials_file_path(None);
        let _ = fs::remove_file(&path);
    }
}
