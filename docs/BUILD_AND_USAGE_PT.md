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

## 8. Notas de integração com a app Flutter

> **⚠️ Ramo UI descontinuado.** O ramo `ui` que existia anteriormente neste repositório **já não está ativo**. O código Flutter foi extraído desse ramo para a pasta `UI/` na raiz do monorepo (`../UI/`). Todo o desenvolvimento Flutter futuro acontece lá.

Para ligar a app Flutter (`../UI/`) a esta API:

1. Apontar a app Flutter para a base URL do backend — atualizar `baseUrl` em `UI/lib/src/core/api/`.
2. Definir `CORS_ORIGIN` no `.env` desta API para a origem de desenvolvimento Flutter (ou manter o valor por omissão para uso em emulador).
3. Garantir que o access token é guardado apenas em memória (nunca em SharedPreferences) — o cookie httpOnly de refresh trata da persistência.
4. Tratar respostas HTTP **402** de endpoints exclusivos Pro mostrando o paywall de upgrade.
5. Confirmar que Ollama está em execução e acessível em `OLLAMA_URL` antes de testar funcionalidades de chat IA.

Consulte [`../UI/README.md`](../UI/README.md) para instruções completas de configuração do Flutter.

## 9. Resolução de problemas

- **Erros de ligação à BD:** verificar saúde do container e credenciais em `DATABASE_URL`.
- **Erros de configuração JWT:** confirmar segredo com >= 32 caracteres.
- **Problemas CORS a partir da UI:** atualizar `CORS_ORIGIN`.
- **Falhas de chat:** confirmar que `OLLAMA_URL` está acessível e que o modelo existe.

## 10. Documentação complementar

- Schema e relações da base de dados: [`database/SCHEMA.pt-PT.md`](database/SCHEMA.pt-PT.md)
- Visão geral do repositório: [`../README.pt-PT.md`](../README.pt-PT.md)
