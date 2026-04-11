use std::io;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("HOME environment variable is not set")]
    NoHome,

    #[error("failed to create LaunchAgents directory: {0}")]
    CreateDir(#[source] io::Error),

    #[error("failed to write plist: {0}")]
    WritePlist(#[source] io::Error),

    #[error("failed to run launchctl: {0}")]
    Launchctl(#[source] io::Error),

    #[error("launchctl load failed: {0}")]
    LaunchctlLoad(String),

    #[error("failed to remove plist: {0}")]
    RemovePlist(#[source] io::Error),

    #[error("platform not supported: install/uninstall is only supported on macOS")]
    #[allow(dead_code)]
    UnsupportedPlatform,
}
