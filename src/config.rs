use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: IpAddr,
    pub port: u16,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let host = std::env::var("CLAUDE_PROXY_HOST").unwrap_or_else(|_| "0.0.0.0".to_owned());
        let port = std::env::var("CLAUDE_PROXY_PORT").unwrap_or_else(|_| "3000".to_owned());
        let host = host
            .parse()
            .map_err(|_| ConfigError::InvalidHost(host.clone()))?;
        let port = port
            .parse()
            .map_err(|_| ConfigError::InvalidPort(port.clone()))?;
        Ok(Self { host, port })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Invalid host: {0}")]
    InvalidHost(String),
    #[error("Invalid port: {0}")]
    InvalidPort(String),
}
