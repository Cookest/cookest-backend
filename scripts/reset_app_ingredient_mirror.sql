-- One-time reset of the app-db ingredient mirror.
--
-- WHY: app-api's `ingredients` table used to accumulate junk free-text rows with
-- auto-serial ids. The catalog is now the food-api master ("cookest_food"), and
-- app-db keeps a SAME-ID mirror (a local row's id == the master ingredient's id),
-- populated lazily via IngredientService::ensure_local_mirror. To avoid id
-- collisions between the old serial ids and the canonical master ids, wipe the
-- mirror and everything that references it so they repopulate from the master.
--
-- RUN AGAINST THE APP DATABASE ONLY (cookest_app, default :5433). Do NOT run this
-- against the food database (cookest_food) — that one holds the master catalog.
--
--   psql "$APP_DATABASE_URL" -f scripts/reset_app_ingredient_mirror.sql
--   # e.g. APP_DATABASE_URL=postgresql://postgres:postgres@localhost:5433/cookest_app
--
-- This is a destructive DEV reset: it clears the local pantry, recipe↔ingredient
-- links, and shopping lists. The master catalog (food-api) is untouched, and the
-- app will re-mirror ingredients on demand as users reference them.

BEGIN;

-- CASCADE also truncates every table that references ingredients
-- (ingredient_nutrients, portion_sizes, recipe_ingredients, inventory_items,
--  shopping_list_items, store_promotion_ingredients, …).
TRUNCATE TABLE ingredients RESTART IDENTITY CASCADE;

COMMIT;
