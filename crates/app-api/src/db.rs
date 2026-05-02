use crate::config::Config;
use sea_orm::DatabaseConnection;

pub async fn establish_connection(config: &Config) -> Result<DatabaseConnection, sea_orm::DbErr> {
    cookest_shared::db::establish_connection(config.database_url()).await
}
