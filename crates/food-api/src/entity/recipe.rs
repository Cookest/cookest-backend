//! Recipe entity - core recipe record

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "recipes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    #[sea_orm(column_type = "Text")]
    pub name: String,

    /// URL-friendly identifier e.g. "spaghetti-carbonara"
    #[sea_orm(unique, column_type = "Text")]
    pub slug: String,

    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,

    /// e.g. "Italian", "Japanese", "Portuguese"
    #[sea_orm(column_type = "Text", nullable)]
    pub cuisine: Option<String>,

    /// e.g. "breakfast", "lunch", "dinner", "snack", "dessert"
    #[sea_orm(column_type = "Text", nullable)]
    pub category: Option<String>,

    /// "easy", "medium", "hard"
    #[sea_orm(column_type = "Text", nullable)]
    pub difficulty: Option<String>,

    /// Base number of servings this recipe produces
    pub servings: i32,

    /// Preparation time in minutes
    pub prep_time_min: Option<i32>,

    /// Cooking time in minutes
    pub cook_time_min: Option<i32>,

    /// Total time in minutes (prep + cook + rest)
    pub total_time_min: Option<i32>,

    // Dietary flags — stored as booleans for fast filtering
    pub is_vegetarian: bool,
    pub is_vegan: bool,
    pub is_gluten_free: bool,
    pub is_dairy_free: bool,
    pub is_nut_free: bool,

    /// Where this recipe came from (for attribution)
    #[sea_orm(column_type = "Text", nullable)]
    pub source_url: Option<String>,

    /// Average rating 0.0-5.0 (denormalized for fast queries)
    pub average_rating: Option<Decimal>,

    /// Total number of ratings
    pub rating_count: i32,

    /// Optional contributor ID (no FK to users — food-api has no user table)
    pub author_id: Option<Uuid>,

    /// Whether this recipe is publicly visible
    pub is_public: bool,

    // --- Dataset / scraping fields ---

    /// ISO 639-1 language code of the recipe content: "en", "fr", "de", "it", "es", "pt"
    #[sea_orm(column_type = "Text")]
    pub language: String,

    /// Searchable tags array e.g. {"quick", "kid-friendly", "one-pot"}
    #[sea_orm(column_type = "Array(RcOrArc::new(ColumnType::Text))", nullable)]
    pub tags: Option<Vec<String>>,

    /// Source domain e.g. "bbcgoodfood.com"
    #[sea_orm(column_type = "Text", nullable)]
    pub source_site: Option<String>,

    /// When this recipe was scraped from the web
    pub scraped_at: Option<DateTimeWithTimeZone>,

    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::recipe_ingredient::Entity")]
    RecipeIngredients,

    #[sea_orm(has_many = "super::recipe_step::Entity")]
    RecipeSteps,

    #[sea_orm(has_many = "super::recipe_image::Entity")]
    RecipeImages,

    #[sea_orm(has_one = "super::recipe_nutrition::Entity")]
    RecipeNutrition,
}

impl Related<super::recipe_ingredient::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RecipeIngredients.def()
    }
}

impl Related<super::recipe_step::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RecipeSteps.def()
    }
}

impl Related<super::recipe_image::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RecipeImages.def()
    }
}

impl Related<super::recipe_nutrition::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RecipeNutrition.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
