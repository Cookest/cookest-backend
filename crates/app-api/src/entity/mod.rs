//! SeaORM entity modules — one per database table.
pub mod user;

// Ingredient & nutrition layer
pub mod ingredient;
pub mod ingredient_nutrient;
pub mod portion_size;

// Recipe system
pub mod recipe;
pub mod recipe_image;
pub mod recipe_ingredient;
pub mod recipe_nutrition;
pub mod recipe_step;

// User ↔ Recipe interactions
pub mod cooking_history;
pub mod recipe_rating;
pub mod user_favorite;

// Inventory
pub mod inventory_deduction;
pub mod inventory_item;

// Meal planning
pub mod meal_plan;
pub mod meal_plan_slot;

// Shopping list
pub mod shopping_list_item;

// Push notifications
pub mod user_push_token;

// AI Chat
pub mod chat_message;
pub mod chat_session;

// ML Preferences
pub mod user_preference;

// Store & price system
pub mod osm_store_poi;
pub mod pdf_processing_job;
pub mod store;
pub mod store_promotion;
pub mod store_promotion_candidate;

// Households (family groups)
pub mod household;
pub mod household_invite;
pub mod household_member;
pub mod meal_plan_suggestion;
pub mod notification;

// Meal polls (shareable voting, incl. non-app users)
pub mod meal_poll;
pub mod meal_poll_option;
pub mod meal_poll_vote;

// Nutrition knowledge base (RAG)
pub mod knowledge_chunk;

// Stripe idempotency
pub mod stripe_processed_event;
