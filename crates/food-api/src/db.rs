use sea_orm::{Database, DatabaseConnection};

pub async fn establish_connection(database_url: &str) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let db = Database::connect(database_url).await?;
    tracing::info!("Food API database connection established");
    Ok(db)
}
