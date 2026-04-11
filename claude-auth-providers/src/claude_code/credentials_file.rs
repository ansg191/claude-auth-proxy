use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use crate::claude_code::credential::{ClaudeCredential, parse_credentials};

/// Environment variable that overrides the default credentials file path.
const CREDENTIALS_FILE_ENV: &str = "CLAUDE_CREDENTIALS_FILE";

/// Reads Claude Code credentials from `~/.claude/.credentials.json` (or the
/// path specified by `CLAUDE_CREDENTIALS_FILE`).
///
/// Returns an empty vec on any failure (missing file, parse error, etc.).
pub fn get_credentials() -> Vec<ClaudeCredential> {
    let Some(path) = credentials_file_path() else {
        debug!("No home directory available; skipping credentials file");
        return Vec::new();
    };

    load_from(&path)
}

/// Reads and parses credentials from the given file path. Returns an empty
/// vec on any failure (missing file, parse error, etc.).
fn load_from(path: &Path) -> Vec<ClaudeCredential> {
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!(path = %path.display(), "Credentials file not found");
            return Vec::new();
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Failed to read credentials file");
            return Vec::new();
        }
    };

    #[allow(clippy::option_if_let_else)]
    if let Some(cred) = parse_credentials(&contents) {
        debug!(path = %path.display(), "Loaded credential from file");
        vec![cred]
    } else {
        warn!(path = %path.display(), "Failed to parse credentials file");
        Vec::new()
    }
}

fn credentials_file_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(CREDENTIALS_FILE_ENV)
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }

    let mut path = dirs::home_dir()?;
    path.push(".claude");
    path.push(".credentials.json");
    Some(path)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn write_temp_file(name: &str, contents: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "claude-auth-proxy-test-{}-{}.json",
            name,
            std::process::id(),
        ));
        let mut file = std::fs::File::create(&path).expect("create temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file");
        path
    }

    #[test]
    fn parses_direct_format() {
        let path = write_temp_file(
            "direct",
            r#"{"accessToken":"at","refreshToken":"rt","expiresAt":1000}"#,
        );
        let creds = load_from(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].access_token, "at");
        assert_eq!(creds[0].refresh_token.as_deref(), Some("rt"));
        assert_eq!(creds[0].expires_at, 1000);
    }

    #[test]
    fn parses_wrapped_format() {
        let path = write_temp_file(
            "wrapped",
            r#"{"claudeAiOauth":{"accessToken":"at","refreshToken":"rt","expiresAt":1000}}"#,
        );
        let creds = load_from(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].access_token, "at");
    }

    #[test]
    fn parses_without_refresh_token() {
        let path = write_temp_file("no_refresh", r#"{"accessToken":"at","expiresAt":1000}"#);
        let creds = load_from(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].access_token, "at");
        assert!(creds[0].refresh_token.is_none());
    }

    #[test]
    fn returns_empty_for_missing_file() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "claude-auth-proxy-test-missing-{}.json",
            std::process::id(),
        ));
        // Guarantee the path does not exist (cleans up any prior run).
        let _ = std::fs::remove_file(&path);
        assert!(!path.exists());
        assert!(load_from(&path).is_empty());
    }

    #[test]
    fn returns_empty_for_malformed_json() {
        let path = write_temp_file("malformed", "not json at all");
        let creds = load_from(&path);
        let _ = std::fs::remove_file(&path);

        assert!(creds.is_empty());
    }
}
