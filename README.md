# Cookest API

Cookest is an AI-assisted meal planning and kitchen management platform. This repository contains the **Rust backend API** powering authentication, recipes, inventory tracking, meal planning, subscription management, AI chat, and supermarket price scraping.

> **⚠️ UI branch notice**
> The `ui` branch that previously existed in this repository is **no longer active**. The Flutter mobile app has been extracted from that branch and now lives in the dedicated `UI/` folder at the monorepo root (`../UI/`). All future Flutter development happens there. The `ui` branch will not receive further updates and should be considered archived. See [`../UI/README.md`](../UI/README.md) for the current Flutter codebase.

---

## Table of Contents

1. [What the app does](#what-the-app-does)
2. [Tech stack](#tech-stack)
3. [Quick start](#quick-start)
4. [Environment variables](#environment-variables)
5. [Architecture overview](#architecture-overview)
6. [Authentication](#authentication)
7. [Subscription tiers](#subscription-tiers)
8. [Endpoints reference](#endpoints-reference)
9. [Flutter integration notes](#flutter-integration-notes)
10. [PDF price scraping pipeline](#pdf-price-scraping-pipeline)

---

## What the app does

Cookest combines structured food data and user context to support everyday cooking decisions:

- Secure account creation, sign-in, and JWT-based session management.
- Recipe search, filtering, and detail retrieval; user-created recipes (Pro tier).
- Ingredient search and nutrition metadata.
- Personal inventory management (including expiring-soon alerts).
- User profile preferences (household size, dietary restrictions, allergies, health goals).
- AI-scored weekly meal plan generation with breakfast/lunch/dinner/snack slots.
- Flex/relief day system: mark any slot as effort, nutrition, mental, or social relief day.
- Persistent shopping list with optional price comparison across local supermarkets.
- Recipe interactions (ratings, favourites, cooking history with inventory deduction).
- Learned taste preferences via online gradient-descent personalisation.
- AI chat sessions using Ollama with user context (inventory/preferences/history).
- Push notification token registration for iOS, Android, and web.
- Stripe-based subscription with webhook processing.
- Admin PDF upload pipeline: weekly supermarket flyers to AI price extraction to live promotions.

---

## Tech stack

- **Language / Framework:** Rust + Actix-Web 4
- **ORM / DB access:** SeaORM 1.1
- **Database:** PostgreSQL 15+ (with `pg_trgm`, `uuid-ossp`)
- **Auth:** Argon2id password hashing + JWT (access + refresh token pair)
- **Security middleware:** rate limiting (governor), JWT auth middleware, CORS, secure httpOnly cookies
- **AI integration:** Ollama-compatible local model endpoint (llava for vision, any model for chat)
- **Payments:** Stripe (webhook HMAC-SHA256 verification + idempotent event processing)
- **PDF processing:** `pdftoppm` (poppler) + Ollama vision model

---

## Quick start

### Prerequisites

- Rust 1.78+ (`rustup update stable`)
- PostgreSQL 15+ with `pg_trgm` and `uuid-ossp` extensions
- [Ollama](https://ollama.ai) with `llava` model (for PDF price extraction, optional)
- `pdftoppm` CLI tool (`poppler-utils` on Debian/Ubuntu, `poppler` on macOS via Homebrew)

### Setup

```bash
# 1. Clone and enter the project
git clone <repo-url>
cd api

# 2. Copy environment template
cp .env.example .env
# Edit .env with your values

# 3. Create the database
createdb cookest

# 4. Run -- migrations execute automatically on startup
cargo run

# Server starts on http://0.0.0.0:8080
```

---

## Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | Yes | -- | PostgreSQL connection string (`postgres://user:pass@host/db`) |
| `JWT_SECRET` | Yes | -- | Secret key for signing JWTs (min 32 chars) |
| `HOST` | No | `0.0.0.0` | Bind address |
| `PORT` | No | `8080` | Bind port |
| `CORS_ORIGIN` | No | `http://localhost:3000` | Allowed CORS origin |
| `JWT_ACCESS_EXPIRY_SECONDS` | No | `900` | Access token TTL (default 15 min) |
| `JWT_REFRESH_EXPIRY_SECONDS` | No | `2592000` | Refresh token TTL (default 30 days) |
| `OLLAMA_URL` | No | `http://localhost:11434` | Ollama API base URL |
| `OLLAMA_MODEL` | No | `llava` | Vision model for PDF price extraction |
| `PDF_UPLOAD_DIR` | No | `/var/cookest/pdfs` | Directory for uploaded PDF files (must be writable) |
| `STRIPE_WEBHOOK_SECRET` | No | -- | Stripe webhook signing secret (`whsec_...`) |

---

## Architecture overview

```
+-----------------------------------------------------+
|                  Flutter Mobile App                  |
+----------------------+------------------------------+
                       | HTTPS (JWT Bearer)
+----------------------v------------------------------+
|                 Actix-Web API Server                 |
|  +------------+ +------------+ +------------------+ |
|  | JWT Auth   | | Rate Limit | |   CORS           | |
|  | Middleware | | (governor) | |   (configurable) | |
|  +------------+ +------------+ +------------------+ |
|  +----------------------------------------------+   |
|  |              Handlers (HTTP layer)            |   |
|  |  auth / user / recipe / meal_plan / store     |   |
|  |  shopping_list / subscription / chat          |   |
|  +------------------+---------------------------+   |
|  +------------------v---------------------------+   |
|  |             Services (business logic)         |   |
|  |  AuthService   RecipeService   MealPlanService|   |
|  |  PreferenceService   InventoryService         |   |
|  |  StoreService  PushTokenService  SubService   |   |
|  +------------------+---------------------------+   |
|  +------------------v---------------------------+   |
|  |          SeaORM + PostgreSQL                  |   |
|  +----------------------------------------------+   |
+-----------------------------------------------------+
            | tokio::spawn (async background)
+-----------v--------------------------------------------+
|  PDF Processing Worker                                 |
|  PDF -> pdftoppm -> PNG -> base64 -> Ollama llava      |
|  -> structured JSON -> promotions DB                   |
+--------------------------------------------------------+
```

### Key design decisions

- **Subscription tier in JWT**: tier is embedded in access tokens (TTL 15 min) so middleware gates features without a DB round-trip. On every token refresh, tier is re-read from DB.
- **Online preference learning**: `PreferenceService` applies incremental gradient updates (learning rate 0.01) to cuisine/ingredient/difficulty weights as users rate and cook recipes.
- **Idempotent migrations**: all `CREATE TABLE IF NOT EXISTS` and `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` safe to run on every startup.
- **SHA-256 for refresh tokens**: stored as `sha256(raw_token)` in the DB, never the raw token.
- **Admin endpoints**: always verify `is_admin=true` from the DB, never trusting the JWT alone.

---

## Authentication

All protected endpoints require:

```
Authorization: Bearer <access_token>
```

Access tokens expire in **15 minutes**. Refresh using `POST /api/auth/refresh` with the `refresh_token` httpOnly cookie (set automatically on login).

### Token Claims

```json
{
  "sub": "uuid-of-user",
  "exp": 1234567890,
  "tier": "pro",
  "is_admin": false
}
```

---

## Subscription tiers

| Feature | Free | Pro (9.99/mo) | Family (14.99/mo) |
|---|:---:|:---:|:---:|
| Inventory + basic meal plan | Yes | Yes | Yes |
| AI-scored meal plan generation | No | Yes | Yes |
| AI Chat | 10 msg/day | Unlimited | Unlimited |
| Price comparison | No | Yes | Yes |
| Create user recipes | No | Yes | Yes |
| Shopping list optimizer | No | Yes | Yes |
| Multiple household profiles | No | No | Yes |

HTTP **402** is returned when a Free user hits a Pro-gated endpoint:

```json
{ "error": "subscription_required", "feature": "user_recipes" }
```

---

## Endpoints reference

### Auth and Account

| Method | Path | Auth | Description |
|---|---|---|---|
| POST | `/api/auth/register` | -- | Register new user |
| POST | `/api/auth/login` | -- | Login, returns access token + refresh cookie |
| POST | `/api/auth/refresh` | Cookie | Refresh access token |
| POST | `/api/auth/logout` | JWT | Invalidate refresh token |
| POST | `/api/auth/onboarding` | JWT | Complete onboarding (cooking profile, household) |
| POST | `/api/me/change-password` | JWT | Change password (invalidates all sessions) |
| DELETE | `/api/me` | JWT | Delete account (requires password confirmation) |

### Profile and Preferences

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/api/me` | JWT | Full profile with subscription info |
| PUT | `/api/me` | JWT | Update profile fields |
| GET | `/api/me/preferences` | JWT | Current AI taste preference weights |
| DELETE | `/api/me/preferences` | JWT | Reset all weights to neutral |
| GET | `/api/me/favourites` | JWT | Favourite recipes |
| GET | `/api/me/history` | JWT | Cooking history |

### Push Notifications

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/api/me/push-tokens` | JWT | List registered device tokens |
| POST | `/api/me/push-tokens` | JWT | Register a device token |
| DELETE | `/api/me/push-tokens/:id` | JWT | Remove a device token |

**POST body:** `{ "token": "ExponentPushToken[xxx]", "platform": "ios" }`
Platforms: `ios` / `android` / `web`

### Inventory

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/api/inventory` | JWT | All inventory items |
| POST | `/api/inventory` | JWT | Add item |
| GET | `/api/inventory/expiring` | JWT | Items expiring within 3 days |
| PUT | `/api/inventory/:id` | JWT | Update item (quantity, expiry, storage location) |
| DELETE | `/api/inventory/:id` | JWT | Remove item |

### Recipes

| Method | Path | Auth | Tier | Description |
|---|---|---|---|---|
| GET | `/api/recipes` | -- | Free | List with filters and pagination |
| GET | `/api/recipes?match_inventory=true` | JWT | Free | Adds match_pct per recipe |
| GET | `/api/recipes/mine` | JWT | Free | User's own recipes |
| GET | `/api/recipes/slug/:slug` | -- | Free | Detail by URL slug |
| GET | `/api/recipes/:id` | -- | Free | Full detail by ID |
| POST | `/api/recipes` | JWT | Pro | Create a recipe |
| PUT | `/api/recipes/:id` | JWT | Pro | Update own recipe |
| DELETE | `/api/recipes/:id` | JWT | Free | Delete own recipe (author only) |
| POST | `/api/recipes/:id/rate` | JWT | Free | Rate 1-5 stars |
| POST | `/api/recipes/:id/favourite` | JWT | Free | Toggle favourite |
| POST | `/api/recipes/:id/cook` | JWT | Free | Mark cooked (deducts inventory by household_size) |

**GET /api/recipes query params:** `q`, `cuisine`, `category`, `difficulty`, `vegetarian`, `vegan`, `gluten_free`, `dairy_free`, `max_time`, `match_inventory`, `page`, `per_page`

### Meal Plans

| Method | Path | Auth | Description |
|---|---|---|---|
| POST | `/api/meal-plans/generate` | JWT | Generate AI meal plan (4 slots/day x 7 days) |
| GET | `/api/meal-plans/current` | JWT | Current week's plan with all slot detail |
| GET | `/api/meal-plans/current/shopping-list` | JWT | Ingredients needed minus inventory |
| GET | `/api/meal-plans` | JWT | List all past plans (paginated) |
| GET | `/api/meal-plans/:id` | JWT | Specific plan with slots |
| DELETE | `/api/meal-plans/:id` | JWT | Delete plan and all slots |
| GET | `/api/meal-plans/:id/nutrition` | JWT | Weekly macro totals vs goals |
| PUT | `/api/meal-plans/:plan_id/slots/:slot_id` | JWT | Swap recipe in a slot |
| PUT | `/api/meal-plans/:plan_id/slots/:slot_id/complete` | JWT | Mark slot completed |
| PUT | `/api/meal-plans/:plan_id/slots/:slot_id/flex` | JWT | Mark slot as flex/relief day |

**Flex types:** `effort` / `nutrition` / `mental` / `social`

**Nutrition summary response example:**

```json
{
  "week_start": "2025-01-06",
  "totals": {
    "calories": 14200,
    "protein_g": 350,
    "carbs_g": 1800,
    "fat_g": 420,
    "fiber_g": 180
  },
  "daily_average": { "calories": 2028, "protein_g": 50 },
  "goals": { "calories": 14000, "protein_g": 350 },
  "percent_of_goal": { "calories": 101, "protein_g": 100 }
}
```

### Shopping List

| Method | Path | Auth | Tier | Description |
|---|---|---|---|---|
| GET | `/api/shopping-list` | JWT | Free | Current list |
| POST | `/api/shopping-list/sync` | JWT | Free | Sync from current meal plan |
| POST | `/api/shopping-list/items` | JWT | Free | Add manual item |
| PATCH | `/api/shopping-list/items/:id/check` | JWT | Free | Toggle checked |
| DELETE | `/api/shopping-list/items/:id` | JWT | Free | Remove item |
| GET | `/api/shopping-list/prices` | JWT | Pro | Prices per item from active promotions |
| GET | `/api/shopping-list/optimize` | JWT | Pro | Cheapest single-store and cheapest split |

### Subscription

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/api/subscription` | JWT | Tier, features, valid_until |
| POST | `/api/subscription/checkout` | JWT | Stripe checkout session URL |
| POST | `/api/webhooks/stripe` | -- | Stripe webhook (HMAC-SHA256 verified) |

### Stores and Price Scraping

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/api/stores` | -- | All registered stores |
| POST | `/api/admin/stores` | JWT Admin | Create store |
| POST | `/api/admin/stores/:id/promotions/upload` | JWT Admin | Upload weekly promo PDF |
| GET | `/api/admin/stores/:id/jobs` | JWT Admin | PDF processing job status |

### AI Chat

| Method | Path | Auth | Tier | Description |
|---|---|---|---|---|
| POST | `/api/chat` | JWT | Free (10/day) | Chat with Ollama about cooking |

### Ingredients

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/api/ingredients` | -- | Search ingredients |
| GET | `/api/ingredients/:id` | -- | Ingredient detail |

---

## Flutter integration notes

### Auth flow

Keep the access token in memory only -- **never** in SharedPreferences. The httpOnly refresh cookie handles persistence securely.

```dart
// Login
final res = await http.post('/api/auth/login', body: {...});
final accessToken = res.body['access_token'];

// Refresh before expiry (access token TTL = 15 min)
final refreshRes = await http.post('/api/auth/refresh');
// New access token returned; cookie auto-renewed
```

### Subscription gating

Decode the access token locally to show/hide Pro UI without an extra API call:

```dart
final payload = JwtDecoder.decode(accessToken);
final tier = payload['tier'] as String; // "free" | "pro" | "family"
if (tier == 'free') showUpgradePrompt();
```

Always handle HTTP **402** responses by showing the upgrade paywall.

### Push token registration

```dart
// On login / app start
final token = await FirebaseMessaging.instance.getToken();
await api.post('/api/me/push-tokens', {
  'token': token,
  'platform': Platform.isIOS ? 'ios' : 'android',
});
```

### Inventory match

```
GET /api/recipes?match_inventory=true&category=dinner
Authorization: Bearer <token>
```

Response adds `match_pct` (0-100), `owned_ingredients`, `total_ingredients` per recipe. Use this for "what can I cook tonight?" screens.

### Meal plan slot fields

- `day_of_week`: 0 = Monday ... 6 = Sunday
- `meal_type`: `breakfast` / `lunch` / `dinner` / `snack`
- `is_flex`: true when marked as a relief day
- `flex_type`: `effort` / `nutrition` / `mental` / `social`

---

## PDF price scraping pipeline

1. Admin uploads a weekly promotional PDF via `POST /api/admin/stores/:id/promotions/upload`
2. A `pdf_processing_job` row is created with `status=pending`
3. Background Tokio task:
   - Converts PDF pages to PNG via `pdftoppm`
   - Encodes each PNG as base64
   - Sends to Ollama `llava` with a structured extraction prompt
   - Parses JSON response (product, brand, original/discounted price, unit, validity dates)
   - Inserts into `store_promotion_candidates` staging table
4. Admin reviews candidates and promotes to `store_promotions` (live)
5. Pro users access prices via `GET /api/shopping-list/prices`

### Requirements

```bash
# poppler for pdftoppm
sudo apt install poppler-utils   # Debian/Ubuntu
brew install poppler             # macOS

# Ollama vision model
ollama pull llava
```

---

## Documentation

The full project documentation is maintained in the Fumadocs site at `../docs/`. Run it with:

```bash
cd ../docs
bun run dev
# Open http://localhost:3000/docs
```

| Section | URL path |
|---|---|
| Architecture overview | `/docs/architecture/overview` |
| API authentication | `/docs/backend/authentication` |
| API endpoints | `/docs/backend/endpoints/recipes` |
| Mobile app (Flutter) | `/docs/mobile/theme` |
| User guide | `/docs/user-guide/overview` |

Local reference files in this folder:

- Build and operational guide: [`docs/BUILD_AND_USAGE.md`](docs/BUILD_AND_USAGE.md)
- Database schema: [`docs/database/SCHEMA.md`](docs/database/SCHEMA.md)
