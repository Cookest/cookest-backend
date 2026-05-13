# Cookest API — Agent Instructions

You are working on the **Cookest API**, a Rust REST API built with Actix-Web 4, SeaORM, and PostgreSQL.

## Quick Reference

| Attribute | Value |
|-----------|-------|
| Language | Rust 1.78+ |
| Framework | Actix-Web 4 |
| ORM | SeaORM 1.1 |
| Database | PostgreSQL 15+ |
| Auth | Argon2id + JWT |
| AI | Ollama (llava + chat) |

## Documentation

📖 **Full documentation**: https://cookest-docs.vercel.app/docs (or run locally from `../docs/`)

Key pages:
- [Architecture Overview](../docs/content/docs/architecture/overview.mdx)
- [Repository Guide](../docs/content/docs/architecture/repositories.mdx)
- [Backend Getting Started](../docs/content/docs/backend/getting-started.mdx)
- [API Endpoints](../docs/content/docs/backend/endpoints/)
- [Database Schema](docs/database/SCHEMA.md)
- [Best Practices](../docs/content/docs/contributing/best-practices.mdx)
- [Agent Instructions](../docs/content/docs/ai/instructions.mdx)
- [Agentic Skills](../docs/content/docs/ai/skills.mdx)

## Architecture

```
src/
├── handlers/    ← Thin HTTP handlers (validate → call service → respond)
├── services/    ← All business logic (17 service files)
├── entity/      ← SeaORM entity definitions (20+ tables)
├── models/      ← DTOs (request/response types)
├── middleware/   ← JWT auth + rate limiting (governor)
├── validation/   ← Request validation rules
├── config.rs    ← Environment configuration
├── errors.rs    ← AppError definitions
├── db.rs        ← Database initialization
└── main.rs      ← App entry, route registration
```

## Key Rules

1. **Handlers are thin** — validate input, call service, return response
2. **Services own logic** — database queries, algorithms, external API calls
3. **Use `AppError`** for all errors — never `.unwrap()` or `panic!()` in handlers
4. **Pro features return 402** if user tier is insufficient
5. **Admin verification** — always check `is_admin` from DB, never trust JWT alone
6. **Refresh tokens** — stored as `SHA-256(raw_token)`, raw only in httpOnly cookie
7. **Idempotent migrations** — all use `IF NOT EXISTS`

## Commit Format

```
<type>(<scope>): <description>
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `perf`, `build`, `ci`, `chore`  
Scopes: `auth`, `recipe`, `meal-plan`, `chat`, `store`, `subscription`, `middleware`

## MCP Server

For programmatic documentation access, use the MCP server at `../docs/mcp/`.
