use secrecy::{ExposeSecret, SecretString};
use std::env;

#[derive(Clone, Debug, PartialEq)]
pub enum FoodDataSource {
    Local,
    FatSecret,
    Hybrid,
    OpenFoodFacts,
}

#[derive(Clone)]
pub struct Config {
    pub database_url: SecretString,
    pub host: String,
    pub port: u16,
    pub cors_origin: String,
    pub fs_client_id: Option<String>,
    pub fs_client_secret: Option<SecretString>,
    pub food_data_source: FoodDataSource,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        let database_url = env::var("FOOD_DATABASE_URL")
            .or_else(|_| env::var("DATABASE_URL"))
            .map_err(|_| ConfigError::Missing("FOOD_DATABASE_URL"))?;

        let host = env::var("FOOD_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port: u16 = env::var("FOOD_PORT")
            .unwrap_or_else(|_| "8081".to_string())
            .parse()
            .map_err(|_| ConfigError::InvalidValue("FOOD_PORT must be a valid port number"))?;
        let cors_origin = env::var("FOOD_CORS_ORIGIN").unwrap_or_else(|_| "*".to_string());

        let fs_client_id = env::var("FS_CLIENT_ID").ok();
        let fs_client_secret = env::var("FS_CLIENT_SECRET").map(SecretString::from).ok();

        let food_data_source = match env::var("FOOD_DATA_SOURCE").as_deref() {
            Ok("fatsecret") => {
                if fs_client_id.is_none() {
                    return Err(ConfigError::InvalidValue(
                        "FOOD_DATA_SOURCE=fatsecret requires FS_CLIENT_ID and FS_CLIENT_SECRET",
                    ));
                }
                FoodDataSource::FatSecret
            }
            Ok("hybrid") => {
                if fs_client_id.is_none() {
                    return Err(ConfigError::InvalidValue(
                        "FOOD_DATA_SOURCE=hybrid requires FS_CLIENT_ID and FS_CLIENT_SECRET",
                    ));
                }
                FoodDataSource::Hybrid
            }
            Ok("openfoodfacts") => FoodDataSource::OpenFoodFacts,
            _ => {
                if fs_client_id.is_some() {
                    FoodDataSource::Hybrid
                } else {
                    FoodDataSource::Local
                }
            }
        };

        Ok(Self {
            database_url: SecretString::from(database_url),
            host,
            port,
            cors_origin,
            fs_client_id,
            fs_client_secret,
            food_data_source,
        })
    }

    pub fn database_url(&self) -> &str {
        self.database_url.expose_secret()
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
