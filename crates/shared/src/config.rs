use secrecy::SecretString;
use std::env;

/// Load a required environment variable
pub fn require_env(name: &'static str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name))
}

/// Load an optional environment variable with default
pub fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

/// Load a required env var as a SecretString
pub fn require_secret(name: &'static str) -> Result<SecretString, ConfigError> {
    require_env(name).map(SecretString::from)
}

/// Parse an env var as a number
pub fn env_parse<T: std::str::FromStr>(name: &'static str, default: T) -> Result<T, ConfigError>
where
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(val) => val.parse::<T>().map_err(|_| ConfigError::InvalidValue(name)),
        Err(_) => Ok(default),
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Missing(&'static str),
    InvalidValue(&'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Missing(var) => write!(f, "Missing environment variable: {}", var),
            ConfigError::InvalidValue(msg) => write!(f, "Invalid configuration: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}
