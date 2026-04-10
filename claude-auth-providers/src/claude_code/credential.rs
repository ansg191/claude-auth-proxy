use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, trace};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCredential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
}
pub fn parse_credentials(raw: &str) -> Option<ClaudeCredential> {
    let data = {
        let mut parsed = serde_json::from_str::<Value>(raw).ok()?;

        #[allow(clippy::option_if_let_else)]
        if let Some(claude_ai_oauth) = parsed.get_mut("claudeAiOauth") {
            claude_ai_oauth.take()
        } else {
            parsed
        }
    };

    let creds = serde_json::from_value::<ClaudeCredential>(data).ok()?;

    trace!(?creds, "Parsed Claude credentials");
    debug!("Credentials found");

    Some(creds)
}
