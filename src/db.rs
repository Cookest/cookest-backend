use sea_orm::{Database, DatabaseConnection};
use crate::config::Config;

pub async fn establish_connection(config: &Config) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let db = Database::connect(config.database_url()).await?;
    
    tracing::info!("Database connection established");
    
    Ok(db)
}
