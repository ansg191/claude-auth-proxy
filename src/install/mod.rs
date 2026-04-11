//! Service manager integration for running the proxy as a background daemon.

pub mod error;

use error::InstallError;

fn default_binary_path() -> std::path::PathBuf {
    std::env::current_exe().expect("failed to resolve current executable path")
}

/// Arguments for the `install` subcommand.
#[derive(Debug, clap::Args)]
pub struct InstallArgs {
    /// Path to the proxy binary to register with the service manager.
    ///
    /// Defaults to the currently running binary.
    #[arg(long, default_value_t = default_binary_path().display().to_string())]
    pub binary_path: String,

    /// Proxy server configuration embedded into the installed service definition.
    #[command(flatten)]
    pub config: crate::config::ServerConfig,
}

/// Install the proxy as a background daemon using the platform service manager.
///
/// # Errors
///
/// Returns an error if the service cannot be installed on the current platform.
pub fn install(args: InstallArgs) -> Result<(), InstallError> {
    platform::install(args)
}

/// Uninstall the proxy daemon registered with the platform service manager.
///
/// # Errors
///
/// Returns an error if the service cannot be uninstalled on the current platform.
pub fn uninstall() -> Result<(), InstallError> {
    platform::uninstall()
}

#[cfg(target_os = "macos")]
mod platform {
    use tracing::{info, warn};

    use super::{InstallArgs, InstallError};

    const PLIST_FILENAME: &str = "com.claude-auth-proxy.plist";
    const LOG_FILENAME: &str = "claude-auth-proxy.log";

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    fn build_plist(binary: &str, log_path: &str, config: &crate::config::ServerConfig) -> String {
        let prog_args_xml = format!(
            "\t\t<string>{}</string>\n\t\t<string>run</string>",
            xml_escape(binary)
        );

        let env_vars = [
            ("RUST_LOG", "info".to_string()),
            ("CLAUDE_PROXY_HOST", xml_escape(&config.host.to_string())),
            ("CLAUDE_PROXY_PORT", config.port.to_string()),
            (
                "PROXY_CONNECT_TIMEOUT_SECS",
                config.connect_timeout.as_secs().to_string(),
            ),
            (
                "PROXY_READ_TIMEOUT_SECS",
                config.read_timeout.as_secs().to_string(),
            ),
            ("PROXY_MAX_RETRIES", config.max_retries.to_string()),
            ("PROXY_RETRY_ON_5XX", config.retry_on_5xx.to_string()),
            ("PROXY_5XX_MAX_RETRIES", config.max_5xx_retries.to_string()),
        ];
        let env_vars_xml: String = env_vars
            .iter()
            .map(|(k, v)| format!("\t\t<key>{k}</key>\n\t\t<string>{v}</string>"))
            .collect::<Vec<_>>()
            .join("\n");

        let log_path_escaped = xml_escape(log_path);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>com.claude-auth-proxy</string>
	<key>ProgramArguments</key>
	<array>
{prog_args_xml}
	</array>
	<key>KeepAlive</key>
	<true/>
	<key>RunAtLoad</key>
	<true/>
	<key>StandardOutPath</key>
	<string>{log_path_escaped}</string>
	<key>StandardErrorPath</key>
	<string>{log_path_escaped}</string>
	<key>EnvironmentVariables</key>
	<dict>
{env_vars_xml}
	</dict>
</dict>
</plist>
"#
        )
    }

    fn launchctl_unload(plist_path: &str) {
        match std::process::Command::new("launchctl")
            .arg("unload")
            .arg(plist_path)
            .output()
        {
            Ok(out) if !out.status.success() => {
                warn!(
                    path = %plist_path,
                    stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                    "launchctl unload failed; continuing",
                );
            }
            Err(e) => {
                warn!(
                    path = %plist_path,
                    error = %e,
                    "failed to run launchctl unload; continuing",
                );
            }
            Ok(_) => {}
        }
    }

    pub fn install(args: InstallArgs) -> Result<(), InstallError> {
        let home = std::env::var("HOME").map_err(|_| InstallError::NoHome)?;

        let binary_str = args.binary_path;
        if binary_str.contains("target/debug") || binary_str.contains("target/release") {
            warn!(
                path = %binary_str,
                "binary path looks like a build artifact and may not exist after cargo clean",
            );
        }

        let launch_agents = format!("{home}/Library/LaunchAgents");
        std::fs::create_dir_all(&launch_agents).map_err(InstallError::CreateDir)?;

        let logs_dir = format!("{home}/Library/Logs");
        std::fs::create_dir_all(&logs_dir).map_err(InstallError::CreateDir)?;

        let plist_path = format!("{launch_agents}/{PLIST_FILENAME}");
        let log_path = format!("{logs_dir}/{LOG_FILENAME}");

        if std::path::Path::new(&plist_path).exists() {
            launchctl_unload(&plist_path);
        }

        let plist_xml = build_plist(&binary_str, &log_path, &args.config);
        std::fs::write(&plist_path, &plist_xml).map_err(InstallError::WritePlist)?;

        let output = std::process::Command::new("launchctl")
            .arg("load")
            .arg(&plist_path)
            .output()
            .map_err(InstallError::Launchctl)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(InstallError::LaunchctlLoad(stderr.into_owned()));
        }

        info!(plist = %plist_path, log = %log_path, "installed");
        Ok(())
    }

    pub fn uninstall() -> Result<(), InstallError> {
        let home = std::env::var("HOME").map_err(|_| InstallError::NoHome)?;

        let plist_path = format!("{home}/Library/LaunchAgents/{PLIST_FILENAME}");

        if !std::path::Path::new(&plist_path).exists() {
            info!("not installed, nothing to do");
            return Ok(());
        }

        launchctl_unload(&plist_path);
        std::fs::remove_file(&plist_path).map_err(InstallError::RemovePlist)?;

        info!(plist = %plist_path, "uninstalled");
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{InstallArgs, InstallError};

    pub fn install(_args: InstallArgs) -> Result<(), InstallError> {
        Err(InstallError::UnsupportedPlatform)
    }

    pub fn uninstall() -> Result<(), InstallError> {
        Err(InstallError::UnsupportedPlatform)
    }
}
