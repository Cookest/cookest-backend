//! Actix-Web route handlers; each module owns one resource group.
pub mod auth;
pub mod recipe;
pub mod ingredient;
pub mod user;
pub mod chat;
pub mod onboarding;
pub mod shopping_list;
pub mod subscription;
pub mod store;
pub mod browse;
pub mod recipe_gen;
pub mod nutrition;
pub mod taste_profile;
pub mod household;
pub mod meal_poll;
pub mod eat_out;
pub mod notification;
pub mod suggestion;

pub use auth::configure as configure_auth;
pub use recipe::configure as configure_recipes;
pub use recipe::configure_protected as configure_recipes_protected;
pub use ingredient::configure as configure_ingredients;
pub use user::configure as configure_user;
pub use chat::configure_chat;
pub use onboarding::configure_onboarding;
pub use shopping_list::configure_shopping_list;
pub use subscription::configure_subscription;
pub use subscription::configure_subscription_protected;
pub use store::configure_stores;
pub use browse::configure_browse;
pub use browse::FoodApiClient;
pub use recipe_gen::configure_recipe_gen;
pub use nutrition::configure_nutrition;
pub use taste_profile::configure_taste_profile;
pub use household::configure_households;
pub use meal_poll::{configure_polls_public, configure_polls_protected};
pub use eat_out::configure_eat_out;
pub use notification::configure as configure_notification;
pub use suggestion::configure as configure_suggestion;

pub mod import;
pub use import::configure_import_proxy;

pub mod admin;
pub use admin::{configure_admin, configure_admin_setup};

pub mod admin_ingredient;
pub use admin_ingredient::configure_admin_ingredients;
