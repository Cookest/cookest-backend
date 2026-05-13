//! Dataset Importer — ingests the MM-Food-100K.csv Kaggle dataset
//! into the Cookest food database.
//!
//! Usage:
//!   dataset-importer --csv <path> [--dry-run] [--database-url <url>]
//!
//! Env: DATABASE_URL or FOOD_DATABASE_URL

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use rust_decimal::Decimal;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, Statement,
};
use slug::slugify;
use std::collections::HashMap;
use std::str::FromStr;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "dataset-importer", about = "Import MM-Food-100K.csv into the Cookest food database")]
struct Args {
    #[arg(short, long)]
    csv: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    database_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawRecipe {
    name: String,
    id: i64,
    minutes: Option<i32>,
    #[allow(dead_code)]
    contributor_id: Option<i64>,
    #[allow(dead_code)]
    submitted: Option<String>,
    tags: Option<String>,
    nutrition: Option<String>,
    n_steps: Option<i32>,
    steps: Option<String>,
    description: Option<String>,
    ingredients: Option<String>,
    n_ingredients: Option<i32>,
}

#[derive(Debug)]
struct Recipe {
    name: String,
    slug: String,
    description: Option<String>,
    total_time_min: Option<i32>,
    difficulty: String,
    is_vegetarian: bool,
    is_vegan: bool,
    is_gluten_free: bool,
    is_dairy_free: bool,
    is_nut_free: bool,
    tags: Vec<String>,
    steps: Vec<String>,
    ingredient_names: Vec<String>,
    nutrition: Option<NutritionData>,
}

#[derive(Debug, Clone)]
struct NutritionData {
    calories: Option<Decimal>,
    protein_g: Option<Decimal>,
    carbs_g: Option<Decimal>,
    fat_g: Option<Decimal>,
    sugar_g: Option<Decimal>,
    sodium_mg: Option<Decimal>,
    saturated_fat_g: Option<Decimal>,
}

fn parse_py_list(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() || s == "[]" { return vec![]; }
    let inner = s.trim_start_matches('[').trim_end_matches(']');
    let re = Regex::new(r"'((?:[^'\\]|\\.)*)'").unwrap();
    re.captures_iter(inner)
        .map(|cap| cap[1].replace("\\'", "'").replace("\\\\", "\\").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_nutrition(s: &str) -> Option<NutritionData> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let parts: Vec<f64> = s.split(',').filter_map(|p| p.trim().parse::<f64>().ok()).collect();
    if parts.len() < 7 { return None; }
    fn d(f: f64) -> Option<Decimal> { Decimal::from_str(&format!("{:.4}", f)).ok() }
    Some(NutritionData {
        calories:        d(parts[0]),
        fat_g:           d(parts[1] * 78.0 / 100.0),
        sugar_g:         d(parts[2] * 50.0 / 100.0),
        sodium_mg:       d(parts[3] * 2300.0 / 100.0),
        protein_g:       d(parts[4] * 50.0 / 100.0),
        saturated_fat_g: d(parts[5] * 20.0 / 100.0),
        carbs_g:         d(parts[6] * 275.0 / 100.0),
    })
}

fn infer_difficulty(t: Option<i32>, steps: Option<i32>, ings: Option<i32>) -> String {
    let score = t.unwrap_or(30) / 15 + steps.unwrap_or(5) + ings.unwrap_or(5);
    if score <= 10 { "easy" } else if score <= 20 { "medium" } else { "hard" }.to_string()
}

fn detect_flags(ings: &[String]) -> (bool, bool, bool, bool, bool) {
    let all = ings.join(" ").to_lowercase();
    let has = |kw: &str| all.contains(kw);
    let not_veg = ["chicken","beef","pork","lamb","turkey","duck","fish","salmon","tuna",
                   "shrimp","prawn","lobster","anchov","gelatin","lard","bacon","ham","sausage"];
    let not_vegan_extra = ["milk","cream","butter","cheese","yogurt","egg","honey","whey","ghee"];
    let gluten = ["flour","wheat","barley","rye","bread","pasta","noodle","spaghetti","breadcrumb","soy sauce"];
    let dairy  = ["milk","cream","butter","cheese","yogurt","whey","casein","ghee","mozzarella","parmesan"];
    let nuts   = ["almond","walnut","cashew","pecan","hazelnut","pistachio","macadamia","pine nut","brazil nut"];
    let is_veg = !not_veg.iter().any(|k| has(k));
    let is_vegan = is_veg && !not_vegan_extra.iter().any(|k| has(k));
    (!not_veg.iter().any(|k| has(k)), is_vegan,
     !gluten.iter().any(|k| has(k)), !dairy.iter().any(|k| has(k)), !nuts.iter().any(|k| has(k)))
}

fn categorize_ingredient(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |kw: &str| n.contains(kw);
    if ["chicken","beef","pork","fish","shrimp","salmon","tuna","lamb","turkey","tofu","lentil","bean","chickpea"].iter().any(|k| has(k)) { "protein" }
    else if ["milk","cream","butter","cheese","yogurt","whey","ghee"].iter().any(|k| has(k)) { "dairy" }
    else if ["flour","rice","pasta","bread","wheat","oat","barley","couscous","quinoa","noodle"].iter().any(|k| has(k)) { "grain" }
    else if ["tomato","onion","garlic","pepper","carrot","broccoli","spinach","cucumber","mushroom","potato","celery"].iter().any(|k| has(k)) { "vegetable" }
    else if ["apple","banana","lemon","lime","orange","strawberry","blueberry","mango","pineapple","grape","cherry"].iter().any(|k| has(k)) { "fruit" }
    else if ["oil","lard","shortening","margarine"].iter().any(|k| has(k)) { "fat" }
    else if ["salt","pepper","cumin","paprika","turmeric","cinnamon","oregano","basil","thyme","rosemary","ginger","curry"].iter().any(|k| has(k)) { "spice" }
    else if ["sugar","honey","maple syrup","molasses"].iter().any(|k| has(k)) { "sweetener" }
    else { "other" }
}

fn normalise_row(raw: &RawRecipe) -> Option<Recipe> {
    let name = raw.name.trim().to_string();
    if name.is_empty() { return None; }
    let tags = raw.tags.as_deref().map(parse_py_list).unwrap_or_default();
    let steps = raw.steps.as_deref().map(parse_py_list).unwrap_or_default();
    let ingredient_names: Vec<String> = raw.ingredients.as_deref()
        .map(parse_py_list).unwrap_or_default()
        .into_iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
    let total_time_min = raw.minutes.filter(|&m| m > 0 && m < 10_000);
    let (is_vegetarian, is_vegan, is_gluten_free, is_dairy_free, is_nut_free) = detect_flags(&ingredient_names);
    Some(Recipe {
        slug: format!("{}-{}", slugify(&name), raw.id),
        name,
        description: raw.description.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        total_time_min,
        difficulty: infer_difficulty(total_time_min, raw.n_steps, raw.n_ingredients),
        is_vegetarian, is_vegan, is_gluten_free, is_dairy_free, is_nut_free,
        tags, steps, ingredient_names,
        nutrition: raw.nutrition.as_deref().and_then(parse_nutrition),
    })
}

async fn ensure_ingredient(db: &DatabaseConnection, name: &str, cache: &mut HashMap<String, i64>) -> Result<i64> {
    if let Some(&id) = cache.get(name) { return Ok(id); }
    let row = db.query_one(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT id FROM ingredients WHERE name = $1", [name.into()],
    )).await?;
    if let Some(row) = row {
        let id: i64 = row.try_get("", "id")?;
        cache.insert(name.to_string(), id);
        return Ok(id);
    }
    let now = Utc::now().fixed_offset();
    let row = db.query_one(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO ingredients (name, category, language, created_at) VALUES ($1, $2, 'en', $3) ON CONFLICT (name) DO UPDATE SET category = EXCLUDED.category RETURNING id",
        [name.into(), categorize_ingredient(name).into(), now.into()],
    )).await?.ok_or_else(|| anyhow!("INSERT ingredient returned nothing for {}", name))?;
    let id: i64 = row.try_get("", "id")?;
    cache.insert(name.to_string(), id);
    Ok(id)
}

async fn upsert_recipe(db: &DatabaseConnection, r: &Recipe) -> Result<i64> {
    let now = Utc::now().fixed_offset();
    let tags_str: Vec<serde_json::Value> = r.tags.iter().map(|t| serde_json::Value::String(t.clone())).collect();
    let row = db.query_one(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO recipes (name, slug, description, total_time_min, difficulty, is_vegetarian, is_vegan, is_gluten_free, is_dairy_free, is_nut_free, tags, language, source_site, servings, rating_count, is_public, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::text[], 'en', 'food-com', 2, 0, true, $12, $12)
         ON CONFLICT (slug) DO UPDATE SET updated_at = $12 RETURNING id",
        [r.name.clone().into(), r.slug.clone().into(), r.description.clone().into(), r.total_time_min.into(),
         r.difficulty.clone().into(), r.is_vegetarian.into(), r.is_vegan.into(), r.is_gluten_free.into(),
         r.is_dairy_free.into(), r.is_nut_free.into(), serde_json::Value::Array(tags_str).into(), now.into()],
    )).await?.ok_or_else(|| anyhow!("INSERT recipe returned nothing for {}", r.slug))?;
    Ok(row.try_get("", "id")?)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt().with_env_filter("info,dataset_importer=debug").init();
    dotenvy::dotenv().ok();
    let database_url = args.database_url
        .or_else(|| std::env::var("FOOD_DATABASE_URL").ok())
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .context("Set DATABASE_URL or pass --database-url")?;

    info!("Dataset Importer");
    info!("CSV: {}", args.csv);
    if args.dry_run { info!("DRY RUN — no writes"); }

    let db = sea_orm::Database::connect(&database_url).await?;
    info!("DB connected");

    let total = std::io::BufRead::lines(std::io::BufReader::new(std::fs::File::open(&args.csv)?)).count().saturating_sub(1);
    let pb = ProgressBar::new(total as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40}] {pos}/{len} ({eta}) {msg}").unwrap()
        .progress_chars("=>-"));

    let mut rdr = csv::Reader::from_path(&args.csv)?;
    let mut cache: HashMap<String, i64> = HashMap::new();
    let mut imported = 0u64; let mut skipped = 0u64; let mut errors = 0u64;

    for result in rdr.deserialize::<RawRecipe>() {
        pb.inc(1);
        let raw = match result { Ok(r) => r, Err(e) => { warn!("CSV: {}", e); errors += 1; continue; } };
        let Some(recipe) = normalise_row(&raw) else { skipped += 1; continue; };
        if args.dry_run { imported += 1; continue; }

        let recipe_id = match upsert_recipe(&db, &recipe).await {
            Ok(id) => id,
            Err(e) => { warn!("Recipe '{}': {}", recipe.name, e); errors += 1; continue; }
        };

        // Steps
        let _ = db.execute(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "DELETE FROM recipe_steps WHERE recipe_id = $1", [recipe_id.into()])).await;
        for (i, step) in recipe.steps.iter().enumerate() {
            if step.trim().is_empty() { continue; }
            let _ = db.execute(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO recipe_steps (recipe_id, step_number, instruction) VALUES ($1, $2, $3)",
                [recipe_id.into(), ((i+1) as i32).into(), step.trim().into()])).await;
        }

        // Ingredients
        let _ = db.execute(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "DELETE FROM recipe_ingredients WHERE recipe_id = $1", [recipe_id.into()])).await;
        for (ord, name) in recipe.ingredient_names.iter().enumerate() {
            if let Ok(ing_id) = ensure_ingredient(&db, name, &mut cache).await {
                let _ = db.execute(Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Postgres,
                    "INSERT INTO recipe_ingredients (recipe_id, ingredient_id, display_order) VALUES ($1,$2,$3)",
                    [recipe_id.into(), ing_id.into(), (ord as i32).into()])).await;
            }
        }

        // Nutrition
        if let Some(ref n) = recipe.nutrition {
            let _ = db.execute(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO recipe_nutrition (recipe_id, per_serving, calories, protein_g, carbs_g, fat_g, sugar_g, sodium_mg, saturated_fat_g)
                 VALUES ($1, true, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (recipe_id) DO UPDATE SET calories=EXCLUDED.calories, protein_g=EXCLUDED.protein_g, carbs_g=EXCLUDED.carbs_g, fat_g=EXCLUDED.fat_g, sugar_g=EXCLUDED.sugar_g, sodium_mg=EXCLUDED.sodium_mg, saturated_fat_g=EXCLUDED.saturated_fat_g",
                [recipe_id.into(), n.calories.into(), n.protein_g.into(), n.carbs_g.into(), n.fat_g.into(), n.sugar_g.into(), n.sodium_mg.into(), n.saturated_fat_g.into()]
            )).await;
        }

        imported += 1;
        if imported % 1000 == 0 {
            pb.set_message(format!("ok={} skip={} err={} cache={}", imported, skipped, errors, cache.len()));
        }
    }

    pb.finish_with_message("Done!");
    info!("Imported: {}  Skipped: {}  Errors: {}  Ingredients: {}", imported, skipped, errors, cache.len());
    Ok(())
}
