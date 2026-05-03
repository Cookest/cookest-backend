pub mod user;

// Ingredient & nutrition layer
pub mod ingredient;
pub mod ingredient_nutrient;
pub mod portion_size;

// Recipe system
pub mod recipe;
pub mod recipe_ingredient;
pub mod recipe_step;
pub mod recipe_image;
pub mod recipe_nutrition;

// User ↔ Recipe interactions
pub mod user_favorite;
pub mod recipe_rating;
pub mod cooking_history;

// Inventory
pub mod inventory_item;

// Meal planning
pub mod meal_plan;
pub mod meal_plan_slot;

// Shopping list
pub mod shopping_list_item;

// Push notifications
pub mod user_push_token;

// AI Chat
pub mod chat_session;
pub mod chat_message;

// ML Preferences
pub mod user_preference;

// Store & price system
pub mod store;
pub mod store_promotion;
pub mod store_promotion_candidate;
pub mod pdf_processing_job;

// Stripe idempotency
pub mod stripe_processed_event;
