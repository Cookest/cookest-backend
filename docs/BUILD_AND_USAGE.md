# Cookest Build & Usage Guide

This document explains how to build, run, configure, and use the Cookest backend in local development.

## 1. Prerequisites

- Rust toolchain (stable)
- Cargo
- Docker + Docker Compose
- PostgreSQL client tools (optional, but useful)
- Optional for AI chat: local Ollama runtime

## 2. Project layout

- `src/main.rs` — API bootstrap, middleware wiring, and startup migrations.
- `src/handlers/*` — HTTP route handlers by domain.
- `src/services/*` — application/business logic.
- `src/entity/*` — SeaORM entities.
- `src/config.rs` — environment parsing and runtime config validation.
- `docker-compose.yml` — local PostgreSQL service.

## 3. Environment configuration

1. Copy `.env.example` into `.env`.
2. Update values based on your setup.

Key variables used at runtime:

- `DATABASE_URL` — PostgreSQL connection string.
- `JWT_SECRET` — **must be at least 32 chars**.
- `JWT_ACCESS_EXPIRY_SECONDS` — access token lifetime.
- `JWT_REFRESH_EXPIRY_SECONDS` — refresh token lifetime.
- `HOST`, `PORT`, `CORS_ORIGIN` — network config.
- `OLLAMA_URL`, `OLLAMA_MODEL` — AI chat integration.

> Note: If you copied `.env.example`, verify token expiry variable names align with runtime expectations (`JWT_ACCESS_EXPIRY_SECONDS`, `JWT_REFRESH_EXPIRY_SECONDS`).

## 4. Start dependencies

```bash
docker-compose up -d
```

This starts PostgreSQL on port `5432`.

## 5. Run the backend

```bash
cargo run
```

On startup the API:

1. Reads environment variables.
2. Connects to PostgreSQL.
3. Executes schema migration SQL.
4. Starts HTTP server.

By default it binds to `127.0.0.1:8080` (unless overridden).

## 6. Build artifacts

### Development build

```bash
cargo build
```

### Release build

```bash
cargo build --release
```

## 7. Using the API

Base URL examples below assume:

- `http://127.0.0.1:8080`

### 7.1 Authentication flow

1. Register: `POST /api/auth/register`
2. Login: `POST /api/auth/login`
3. Use returned access token for protected endpoints.
4. Refresh when needed: `POST /api/auth/refresh`
5. Logout: `POST /api/auth/logout`

### 7.2 Core endpoint groups

- Recipes:
  - `GET /api/recipes`
  - `GET /api/recipes/{id}`
  - `GET /api/recipes/slug/{slug}`
- Ingredients:
  - `GET /api/ingredients`
  - `GET /api/ingredients/{id}`
- Inventory:
  - `GET /api/inventory`
  - `POST /api/inventory`
  - `PUT /api/inventory/{id}`
  - `DELETE /api/inventory/{id}`
  - `GET /api/inventory/expiring`
- Profile + interactions:
  - `GET /api/me`
  - `PUT /api/me`
  - `GET /api/me/history`
  - `GET /api/me/favourites`
  - `POST /api/recipes/{id}/rate`
  - `POST /api/recipes/{id}/favourite`
  - `POST /api/recipes/{id}/cook`
- Meal plans:
  - `POST /api/meal-plans/generate`
  - `GET /api/meal-plans/current`
  - `GET /api/meal-plans/current/shopping-list`
  - `PUT /api/meal-plans/{plan_id}/slots/{slot_id}/complete`
- Chat:
  - `POST /api/chat`
  - `GET /api/chat/sessions`
  - `GET /api/chat/sessions/{id}/messages`
  - `DELETE /api/chat/sessions/{id}`

## 8. Flutter app integration notes

> **⚠️ UI branch deprecated.** The `ui` branch that previously existed in this repository is **no longer active**. The Flutter codebase has been extracted from that branch into the `UI/` folder at the monorepo root (`../UI/`). All future Flutter development happens there.

To connect the Flutter app (`../UI/`) to this API:

1. Point the Flutter app to the backend base URL — update `baseUrl` in `UI/lib/src/core/api/`.
2. Set `CORS_ORIGIN` in this API's `.env` to the Flutter dev origin (or leave default for emulator use).
3. Ensure the access token is stored in memory only (never in SharedPreferences) — the httpOnly refresh cookie handles persistence.
4. Handle HTTP **402** responses from Pro-gated endpoints by showing the upgrade paywall.
5. Confirm Ollama is running and accessible at `OLLAMA_URL` before testing AI chat features.

See [`../UI/README.md`](../UI/README.md) for full Flutter setup instructions.

## 9. Troubleshooting

- **DB connection errors:** verify container health and `DATABASE_URL` credentials.
- **JWT config errors:** ensure secret length >= 32 chars.
- **CORS issues from UI:** update `CORS_ORIGIN`.
- **Chat failures:** ensure `OLLAMA_URL` is reachable and model exists.

## 10. Extended docs

- Database schema and relationships: [`database/SCHEMA.md`](database/SCHEMA.md)
- Repository overview: [`../README.md`](../README.md)
