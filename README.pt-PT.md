# Cookest

Cookest é uma plataforma de gestão de refeições e cozinha com assistência de IA. Este repositório contém atualmente a **API backend em Rust** que suporta autenticação, receitas, gestão de inventário, planeamento de refeições e fluxos de chat com IA.

> À procura do frontend? Consulte a nota sobre o **ramo UI** em [Visão Geral do Ramo UI](#visão-geral-do-ramo-ui).

## O que a aplicação faz

O Cookest combina dados estruturados de alimentação com contexto do utilizador para apoiar decisões do dia a dia na cozinha:

- Criação de conta e autenticação segura.
- Pesquisa de receitas e consulta de detalhe da receita.
- Pesquisa de ingredientes + metadados nutricionais.
- Gestão de inventário pessoal (incluindo itens a expirar em breve).
- Preferências de perfil (agregado familiar, restrições alimentares, alergias).
- Geração de plano de refeições e lista de compras.
- Interações com receitas (avaliações, favoritos, histórico de “cozinhado”).
- Sessões de chat com IA que podem usar contexto do utilizador (inventário/preferências/histórico) para responder a perguntas de cozinha.

## Stack tecnológica

### Backend (neste ramo)

- **Linguagem/Framework:** Rust + Actix Web
- **ORM/Acesso a BD:** SeaORM
- **Base de dados:** PostgreSQL
- **Autenticação:** hash Argon2id + fluxo JWT access/refresh
- **Middleware de segurança:** rate limiting, middleware JWT, CORS, uso de cookies seguros
- **Integração IA:** endpoint local compatível com Ollama

## Superfície da API (alto nível)

O Cookest expõe endpoints estilo REST sob `/api/*`:

- `/api/auth/*` — registo/login/refresh/logout
- `/api/recipes/*` — listar receitas + obter por id/slug
- `/api/ingredients/*` — pesquisar ingredientes + detalhe de ingrediente
- `/api/inventory/*` — CRUD de inventário e itens a expirar
- `/api/me/*` — perfil, histórico, favoritos
- `/api/meal-plans/*` — gerar/plano atual/lista de compras/marcar concluído
- `/api/chat/*` — sessões e mensagens de chat com IA

Para configuração detalhada e orientação por endpoint, consulte [`docs/pt-PT/BUILD_AND_USAGE.md`](docs/pt-PT/BUILD_AND_USAGE.md).

## Visão Geral do Ramo UI

O código Flutter foi **movido para a sua própria pasta dedicada** (`../UI/`) no monorepo do projeto.

Anteriormente o frontend era mantido num `ramo ui` separado dentro deste repositório da API. Foi extraído para uma pasta independente de forma a que o frontend e o backend possam evoluir de forma autónoma, mantendo um único ponto de entrada no repositório.

Para trabalhar no frontend:

```bash
cd ../UI
flutter pub get
flutter run
```

Consulte [`../UI/README.md`](../UI/README.md) para instruções de configuração completas.

## Início rápido

### 1) Iniciar PostgreSQL

```bash
docker-compose up -d
```

### 2) Configurar ambiente

Copiar e editar:

```bash
cp .env.example .env
```

Depois confirme que os valores estão corretos para o seu Postgres e configuração JWT.

### 3) Executar a API

```bash
cargo run
```

Por omissão, faz bind em `127.0.0.1:8080`, salvo override por variáveis de ambiente.

## Índice de documentação

A documentação completa do projeto está no site Fumadocs em `../docs/`. Execute com:

```bash
cd ../docs
bun run dev
# Abra http://localhost:3000/docs
```

| Secção | Caminho |
|---|---|
| Visão geral da arquitetura | `/docs/architecture/overview` |
| Autenticação da API | `/docs/backend/authentication` |
| Endpoints da API | `/docs/backend/endpoints/recipes` |
| App móvel (Flutter) | `/docs/mobile/theme` |
| Guia do utilizador | `/docs/user-guide/overview` |

Ficheiros de referência locais nesta pasta:

- Guia de build e operação: [`docs/pt-PT/BUILD_AND_USAGE.md`](docs/pt-PT/BUILD_AND_USAGE.md)
- Schema da base de dados: [`docs/database/SCHEMA.md`](docs/database/SCHEMA.md)

## Notas do âmbito atual

- Este ramo é centrado na API.
- As migrações são aplicadas no arranque a partir de SQL em `src/main.rs`.
- Ollama é opcional; só é necessário para endpoints de chat IA.
