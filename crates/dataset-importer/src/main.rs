use std::error::Error;
use std::env;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct RawRecipe {
    name: String,
    id: i64,
    minutes: i32,
    contributor_id: i64,
    submitted: String,
    tags: String,
    nutrition: String,
    n_steps: i32,
    steps: String,
    description: Option<String>,
    ingredients: String,
    n_ingredients: i32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    info!("Starting Dataset Importer...");
    // Database connection would go here
    
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        error!("Usage: {} <path_to_kaggle_recipes.csv>", args[0]);
        return Ok(());
    }
    
    let file_path = PathBuf::from(&args[1]);
    info!("Reading CSV from: {:?}", file_path);

    // Stream CSV without loading into memory
    let mut rdr = csv::Reader::from_path(file_path)?;
    let mut batch = Vec::new();
    let batch_size = 1000;
    let mut total_inserted = 0;

    for result in rdr.deserialize() {
        let record: RawRecipe = result?;
        batch.push(record);

        if batch.len() >= batch_size {
            // TODO: Map RawRecipe to SeaORM entities and insert
            // e.g. Recipe::insert_many(mapped_batch).exec(&db).await?;
            total_inserted += batch.len();
            info!("Processed {} recipes so far...", total_inserted);
            batch.clear(); // Free memory
        }
    }

    // Process remaining
    if !batch.is_empty() {
        total_inserted += batch.len();
        info!("Processed remaining {} recipes...", batch.len());
        batch.clear();
    }

    info!("Finished! Total recipes processed: {}", total_inserted);

    Ok(())
}
