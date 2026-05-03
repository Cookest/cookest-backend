<div align="center">

# 🍳 Cookest API

### AI-Powered Meal Planning & Kitchen Management Backend

[![Rust](https://img.shields.io/badge/Rust-1.78+-orange.svg)](https://www.rust-lang.org/)
[![Actix Web](https://img.shields.io/badge/Actix%20Web-4-blue.svg)](https://actix.rs/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-15+-blue.svg)](https://www.postgresql.org/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

*A high-performance Rust backend powering intelligent meal planning, inventory management, and AI-assisted cooking*

[Quick Start](#-quick-start) • [Documentation](https://docs.cookest.app) • [API Reference](https://docs.cookest.app/docs/backend/endpoints)

</div>

---

## 📋 Overview

Cookest is an AI-assisted meal planning and kitchen management platform. This repository contains the **Rust backend API** that powers:

- 🔐 **Authentication & User Management** - Secure JWT-based auth with Argon2id
- 🍽️ **Recipe Management** - Search, create, and personalize recipes with AI
- 🥬 **Smart Inventory** - Track ingredients with expiration alerts
- 📅 **Meal Planning** - AI-generated weekly meal plans with flex days
- 🛒 **Shopping Lists** - Auto-sync from meal plans with price optimization
- 🤖 **AI Chat** - Ollama-powered cooking assistant
- 💰 **Subscriptions** - Stripe integration with tiered features
- 🏷️ **Price Scraping** - Vision-based PDF extraction from supermarket flyers

---

## 🛠️ Tech Stack

**Core:** Rust 1.78+ • Actix-Web 4 • SeaORM 1.1 • PostgreSQL 15+

**Security:** Argon2id • JWT • Rate Limiting (governor) • Secure httpOnly cookies

**AI & Integrations:** Ollama (LLM) • llava (vision) • Stripe (payments) • pdftoppm (PDF)

---

## 🚀 Quick Start

### Prerequisites

- Rust 1.78+ (`rustup update stable`)
- PostgreSQL 15+ with `pg_trgm` and `uuid-ossp` extensions
- [Ollama](https://ollama.ai) with `llava` model (optional, for PDF price extraction)

### Installation

```bash
# Clone and navigate
git clone <repo-url>
cd api

# Configure environment
cp .env.example .env
# Edit .env with your DATABASE_URL and JWT_SECRET

# Create database
createdb cookest

# Run (migrations execute automatically)
cargo run
```

**Server starts on:** `http://0.0.0.0:8080`

---

## ⚙️ Environment Variables

Required environment variables:

| Variable | Description |
|----------|-------------|
| `DATABASE_URL` | PostgreSQL connection string |
| `JWT_SECRET` | Secret key for signing JWTs (min 32 chars) |

Optional: `OLLAMA_URL`, `STRIPE_WEBHOOK_SECRET`, `CORS_ORIGIN`, and more.

See [`.env.example`](.env.example) for all options or check the [environment documentation](https://docs.cookest.app/docs/backend/environment).

---

## 📡 API Endpoints

The API provides RESTful endpoints organized by domain:

- **🔐 Auth** - `/api/auth/*` - Registration, login, token refresh
- **👤 Profile** - `/api/me/*` - User profile, preferences, favorites
- **🍽️ Recipes** - `/api/recipes/*` - Search, create, rate recipes
- **📅 Meal Plans** - `/api/meal-plans/*` - AI-generated weekly plans
- **🥬 Inventory** - `/api/inventory/*` - Track ingredients and expiration
- **🛒 Shopping** - `/api/shopping-list/*` - Manage shopping lists
- **💳 Subscription** - `/api/subscription/*` - Stripe payment integration
- **🤖 AI Chat** - `/api/chat` - Ollama-powered cooking assistant
- **🏪 Stores** - `/api/stores/*` - Supermarket price scraping

> 📚 For complete endpoint reference with request/response schemas, see the [API documentation](https://docs.cookest.app/docs/backend/endpoints).

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Flutter Mobile App                        │
└────────────────────────┬────────────────────────────────────┘
                         │ HTTPS (JWT Bearer)
┌────────────────────────▼────────────────────────────────────┐
│                  Actix-Web API Server                        │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Middleware: JWT Auth • Rate Limit • CORS              │ │
│  └────────────────────────┬───────────────────────────────┘ │
│  ┌────────────────────────▼───────────────────────────────┐ │
│  │  Services: Auth • Recipe • MealPlan • Inventory        │ │
│  └────────────────────────┬───────────────────────────────┘ │
│  ┌────────────────────────▼───────────────────────────────┐ │
│  │            SeaORM + PostgreSQL                          │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────┬───────────────────────────────────┘
                          │ Background Workers
                          ▼
              PDF Processing • AI Chat • Notifications
```

> 📚 For detailed architecture, database schema, and design decisions, see the [full documentation](https://docs.cookest.app).

---

## 📚 Documentation

The complete documentation is available at **[docs.cookest.app](https://docs.cookest.app)**.

### Key Documentation Sections

| Section | Description |
|---------|-------------|
| **[Getting Started](https://docs.cookest.app/docs/backend/getting-started)** | Installation, setup, and first steps |
| **[Authentication](https://docs.cookest.app/docs/backend/authentication)** | JWT implementation and security |
| **[API Endpoints](https://docs.cookest.app/docs/backend/endpoints)** | Complete endpoint reference |
| **[Database Schema](https://docs.cookest.app/docs/backend/database)** | Database design and relationships |
| **[PDF Pipeline](https://docs.cookest.app/docs/backend/pdf-pipeline)** | AI-powered price extraction |
| **[Environment](https://docs.cookest.app/docs/backend/environment)** | Configuration options |

---

<div align="center">

**Built with ❤️ using Rust and Actix-Web**

[⬆ Back to Top](#-cookest-api)

</div>
