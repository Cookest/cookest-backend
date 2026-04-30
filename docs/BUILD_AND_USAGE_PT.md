# Cookest — Guia de Build e Utilização

Este documento explica como compilar, executar, configurar e usar o backend Cookest em desenvolvimento local.

## 1. Pré-requisitos

- Toolchain Rust (stable)
- Cargo
- Docker + Docker Compose
- Ferramentas cliente PostgreSQL (opcional, mas útil)
- Opcional para chat IA: runtime Ollama local

## 2. Estrutura do projeto

- `src/main.rs` — bootstrap da API, configuração de middleware e migrações no arranque.
- `src/handlers/*` — handlers HTTP por domínio.
- `src/services/*` — lógica de aplicação/negócio.
- `src/entity/*` — entidades SeaORM.
- `src/config.rs` — parsing de variáveis de ambiente e validação de configuração.
- `docker-compose.yml` — serviço PostgreSQL local.

## 3. Configuração de ambiente

1. Copie `.env.example` para `.env`.
2. Atualize os valores para o seu ambiente.

Variáveis principais usadas em runtime:

- `DATABASE_URL` — string de ligação ao PostgreSQL.
- `JWT_SECRET` — **mínimo 32 caracteres**.
- `JWT_ACCESS_EXPIRY_SECONDS` — duração do access token.
- `JWT_REFRESH_EXPIRY_SECONDS` — duração do refresh token.
- `HOST`, `PORT`, `CORS_ORIGIN` — configuração de rede.
- `OLLAMA_URL`, `OLLAMA_MODEL` — integração de chat IA.

> Nota: Se copiou `.env.example`, valide se os nomes das variáveis de expiração de token estão alinhados com o que a aplicação espera (`JWT_ACCESS_EXPIRY_SECONDS`, `JWT_REFRESH_EXPIRY_SECONDS`).

## 4. Iniciar dependências

```bash
docker-compose up -d
```

Isto inicia o PostgreSQL na porta `5432`.

## 5. Executar o backend

```bash
cargo run
```

No arranque, a API:

1. Lê variáveis de ambiente.
2. Liga ao PostgreSQL.
3. Executa SQL de migração de schema.
4. Inicia o servidor HTTP.

Por omissão, faz bind em `127.0.0.1:8080` (salvo override).

## 6. Artefactos de build

### Build de desenvolvimento

```bash
cargo build
```

### Build de produção

```bash
cargo build --release
```

## 7. Utilização da API

Os exemplos abaixo assumem base URL:

- `http://127.0.0.1:8080`

### 7.1 Fluxo de autenticação

1. Registo: `POST /api/auth/register`
2. Login: `POST /api/auth/login`
3. Usar o access token devolvido para endpoints protegidos.
4. Refresh quando necessário: `POST /api/auth/refresh`
5. Logout: `POST /api/auth/logout`

### 7.2 Grupos principais de endpoints

- Receitas:
  - `GET /api/recipes`
  - `GET /api/recipes/{id}`
  - `GET /api/recipes/slug/{slug}`
- Ingredientes:
  - `GET /api/ingredients`
  - `GET /api/ingredients/{id}`
- Inventário:
  - `GET /api/inventory`
  - `POST /api/inventory`
  - `PUT /api/inventory/{id}`
  - `DELETE /api/inventory/{id}`
  - `GET /api/inventory/expiring`
- Perfil + interações:
  - `GET /api/me`
  - `PUT /api/me`
  - `GET /api/me/history`
  - `GET /api/me/favourites`
  - `POST /api/recipes/{id}/rate`
  - `POST /api/recipes/{id}/favourite`
  - `POST /api/recipes/{id}/cook`
- Planos de refeição:
  - `POST /api/meal-plans/generate`
  - `GET /api/meal-plans/current`
  - `GET /api/meal-plans/current/shopping-list`
  - `PUT /api/meal-plans/{plan_id}/slots/{slot_id}/complete`
- Chat:
  - `POST /api/chat`
  - `GET /api/chat/sessions`
  - `GET /api/chat/sessions/{id}/messages`
  - `DELETE /api/chat/sessions/{id}`

## 8. Notas de integração com o ramo UI

A implementação UI é mantida num track de ramo UI dedicado.

Checklist recomendado de integração:

1. Apontar o ambiente UI para a base URL do backend (`HOST:PORT`).
2. Alinhar gestão de tokens com o contrato login/refresh do backend.
3. Verificar se `CORS_ORIGIN` no backend coincide com a origem de desenvolvimento da UI.
4. Validar chamadas a rotas protegidas com utilizador autenticado real.
5. Confirmar funcionalidades de chat apenas quando Ollama estiver ativo.

## 9. Resolução de problemas

- **Erros de ligação à BD:** verificar saúde do container e credenciais em `DATABASE_URL`.
- **Erros de configuração JWT:** confirmar segredo com >= 32 caracteres.
- **Problemas CORS a partir da UI:** atualizar `CORS_ORIGIN`.
- **Falhas de chat:** confirmar que `OLLAMA_URL` está acessível e que o modelo existe.

## 10. Documentação complementar

- Schema e relações da base de dados: [`database/SCHEMA.pt-PT.md`](database/SCHEMA.pt-PT.md)
- Visão geral do repositório: [`../README.pt-PT.md`](../README.pt-PT.md)
