# Portfolio backend

This repository was cloned from `https://github.com/sukjaelee/clean_axum_demo` at commit `428cec16821a2ef8334636ced57db341dd7448a4`. The original demo source, tests, seed scripts, and license are preserved. Cargo explicitly builds the new `portfolio-api` binary and `portfolio_api` library under `src/portfolio/`; the demo entry points and tests are not registered targets. Use the portfolio commands below rather than the upstream demo instructions or its environment files.

## Layout

Case studies now use the same CMS persistence, upload and publication workflow. See [case study setup, import, API and admin editing](docs/case-studies.md) for the idempotent import of the 13 existing entries.

```text
src/portfolio/
  main.rs                 serve, migrate, bootstrap-owner, prune-sessions
  app.rs                  routes, CORS, body limit, timeout, response headers
  common/                 configuration, errors, extraction, security, state, validation
  domains/auth/           authentication API, DTOs, repository, service
  domains/user/           account and role API, DTOs, repository, service
  domains/content/        shared blog/project persistence, editor rendering, SEO, API
migrations/               versioned PostgreSQL schema and permission seeds
examples/                 writable JSON mapped from the actual frontend records
docs/openapi.json         OpenAPI 3.1 contract, also served at /openapi.json
tests/portfolio_api.rs    validation, security, and PostgreSQL API tests
```

Blogs and projects use a common publication record with typed, separately validated domain fields. This keeps publication, versioning, slug history, SEO, and authorization behavior consistent without duplicating that logic.

## Database and application setup

The Ubuntu deployment runs PostgreSQL 17 directly on the operating system and only the API in Docker. Follow [Ubuntu deployment and database migration](docs/ubuntu-deployment.md) to install PostgreSQL, transfer an existing Docker database, and start the API. `docker-compose.yml` contains only the `api` service and uses Linux host networking to connect to PostgreSQL at `127.0.0.1:5432`. Both PostgreSQL and the API stay on loopback; a reverse proxy on Ubuntu serves public HTTPS traffic.

For a fresh checkout, copy `.env.portfolio.example` to `.env.portfolio`, set the database password and origins, and provision the database using the guide. Existing private environment files are not changed automatically. The previous Docker database used `127.0.0.1:55432`; change that URL only after restoring and verifying the native database. Do not remove the old database container or volume during migration.

For development with Rust installed, the API can also run directly against a provisioned PostgreSQL database:

```powershell
cd Backend
cargo run --bin portfolio-api -- migrate
```

Migration execution is explicit; starting the API does not silently change the schema. Readiness fails until the portfolio schema exists. When transferring an existing database, restore its complete dump, including `_sqlx_migrations`, before running `migrate` to apply any pending changes.

Set these variables in your shell or the private `.env.portfolio` before bootstrapping your account:

```text
BOOTSTRAP_OWNER_EMAIL=your-email@example.com
BOOTSTRAP_OWNER_NAME=Your Name
BOOTSTRAP_OWNER_PASSWORD=a-unique-password-of-at-least-12-characters
```

```powershell
cargo run --bin portfolio-api -- bootstrap-owner
cargo run --bin portfolio-api -- serve
```

Bootstrap is a local CLI operation protected against concurrent execution. It refuses to replace an existing owner. Remove bootstrap values from the environment file after creating the account. Public registration creates accounts without any roles; it never grants workspace access. There are no default admin credentials or automatic content imports. `examples/blog.json` maps the existing frontend's single article; `examples/project.json` maps its hotel-booking project. They can be submitted through the authenticated API after provisioning your owner.

`PORTFOLIO_ENV_FILE` can select a different private file. The upstream `.env` and `.env.test` are intentionally not loaded by the application. In production, inject configuration through the deployment environment instead of committing credentials.

### Explore the API

Project galleries support multi-select R2 uploads in the existing admin and an ordered, validated `fields.gallery` array in the Rust project API. See [project gallery uploads and API contract](docs/project-gallery.md).

Start the API with `cargo run --bin portfolio-api -- serve`, then open `http://localhost:8080/docs` for interactive Swagger UI. Its assets are loaded from a pinned CDN version, so the browser needs an internet connection. The raw OpenAPI 3.1 document is at `http://localhost:8080/openapi.json` and `docs/openapi.json`; it can also be imported into Postman or Insomnia. Its server URL is relative to the API host; point imported clients at `http://localhost:8080`.

Public routes and health checks work without a token. To call an admin route, first bootstrap an owner account, send `POST /api/v1/auth/login` with its email and password, and copy the returned `accessToken` into an `Authorization: Bearer <accessToken>` header. Example request bodies for content creation are in `examples/blog.json` and `examples/project.json`. No content is imported automatically, so the public lists initially return empty results.

## Authentication

| Endpoint | Behavior |
| --- | --- |
| `POST /api/v1/auth/login` | `{ "email": "...", "password": "..." }`; returns tokens and effective account permissions |
| `POST /api/v1/auth/register` | Create an account with no roles or admin permissions; returns 201 |
| `POST /api/v1/auth/forgot-password` | Generic 202 response; emails a 15-minute reset link when the account exists |
| `POST /api/v1/auth/reset-password` | Set a new password with a single-use token; revokes every existing session |
| `POST /api/v1/auth/refresh` | `{ "refreshToken": "..." }`; rotates both access and refresh tokens |
| `GET /api/v1/auth/me` | Current account, roles, and current effective permissions |
| `POST /api/v1/auth/logout` | Revokes the current session; 204 |
| `POST /api/v1/auth/change-password` | `{ "currentPassword": "...", "newPassword": "..." }`; revokes all account sessions; 204 |

Protected requests use `Authorization: Bearer <accessToken>`. Tokens are opaque 256-bit random values, not JWTs. Only SHA-256 token hashes are stored. Passwords use salted Argon2id. Login returns the same error for unknown accounts, inactive accounts, and invalid passwords, and performs a dummy password verification for unknown accounts. Expired or revoked access tokens fail authentication. Public registration creates an account without roles; an owner must assign a role before it can read drafts or use the workspace.

Password recovery requires `SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, and `SMTP_FROM` in the private backend environment. Leave `SMTP_HOST` unset to disable email recovery; the request route then returns 503 for every address. The reset token is stored only as a hash, is single use, expires after 15 minutes, and is delivered by email. Do not put real SMTP credentials in `.env.portfolio.example` or version control.

Refresh tokens expire at the original session deadline (seven days by default); rotation does not extend that deadline. Reusing an already rotated refresh token revokes that session, including its new tokens. Password changes and account deactivation revoke all account sessions. Permissions are fetched on each authenticated request, so existing access tokens do not retain removed permissions. The default access lifetime is 30 minutes. Run `prune-sessions` periodically to remove expired sessions and their replay history.

Login and refresh share a limit of 10 attempts per minute per direct peer IP per API process. No forwarded IP header is trusted automatically. A proxy deployment should enforce a shared edge rate limit and configure its trusted network explicitly; multiple API replicas do not share this in-memory limiter. Password work is bounded and runs off the async executor. Request bodies and credentials are not logged.

The API uses bearer authentication and does not set cookies. For the future admin panel, a server-side session/BFF can keep tokens in secure HttpOnly cookies; cookie-based actions must include CSRF protection. Use HTTPS for all public deployments. CORS is an explicit origin allowlist and does not enable credentialed wildcard access.

## Users, roles, and permissions

| Role | Default permissions |
| --- | --- |
| owner | All permissions; provisioned only by bootstrap |
| admin | Content reading, creation, publication, deletion, user reading, role/permission reading |
| editor | Content reading and draft writing; cannot modify live content, publish, archive, or delete |
| viewer | Read drafts and archived content only |

Permissions: `users:read`, `users:write`, `roles:read`, `roles:write`, `content:read`, `blogs:write`, `blogs:publish`, `blogs:delete`, `projects:write`, `projects:publish`, and `projects:delete`.

| Endpoint | Permission |
| --- | --- |
| `GET /api/v1/admin/users?page=1&perPage=20` | `users:read` |
| `POST /api/v1/admin/users` | `users:write`; accepts email, name, password, optional public headline; creates a viewer |
| `PUT /api/v1/admin/users/{id}` | `users:write`; name, headline, active, optional replacement password |
| `PUT /api/v1/admin/users/{id}/roles` | `roles:write`; `{ "roleIds": ["uuid"] }` |
| `GET /api/v1/admin/roles` | `roles:read` |
| `POST /api/v1/admin/roles` | `roles:write`; name, description, permission-name array |
| `PUT /api/v1/admin/roles/{id}` | `roles:write`; replace a custom role |
| `DELETE /api/v1/admin/roles/{id}` | `roles:write`; delete a custom role |
| `GET /api/v1/admin/permissions` | `roles:read` |

Built-in roles are protected from modification/deletion. The owner role cannot be assigned through HTTP, and owner accounts cannot be deactivated or changed through account administration. Owners can change their own password through the authenticated password endpoint. These rules prevent deleting or demoting the last owner. Custom roles can combine known permissions, and role/assignment updates are transactional. Public author bylines use `headline`, not the person's administrative role. Password hashes are never returned.

## Blog and project API

### AI draft assistance

`POST /api/v1/admin/ai/generate` returns a validated, unsaved field patch for a blog or project. It requires the built-in owner/admin role plus the corresponding write permission. Generation never calls content persistence. The endpoint is documented in `/openapi.json` and `/docs`, including field paths, request modes, tagged context, and response/error contracts. See `docs/ai-content.md` for architecture and setup.

Configure `AI_PROVIDER=gemini`, `AI_API_KEY`, and `AI_MODEL=gemini-3.5-flash-lite` in the backend environment, then restart the API. Optional controls are `AI_REQUEST_TIMEOUT_SECS` (default 90, maximum 120), `AI_MAX_OUTPUT_TOKENS` (default 8192, maximum 16384), `AI_MAX_CONCURRENT_REQUESTS` (default 2), and `AI_REQUESTS_PER_ADMIN_HOUR` (default 20). Limits apply per API instance. No AI database migrations are required.

The shared Gemini client uses schema-constrained JSON, a fixed HTTPS endpoint, header credentials, bounded response bytes/time, and at most one retry for connection failures or 502/503/504. Provider errors are mapped to safe API errors. Per-user and global generation guards bound costs, including overlapping requests. Rich blocks are converted into supported Tiptap nodes and validated by the normal content renderer. Asset references, canonical URLs, and years are derived only from known administrator-supplied values. Model output still requires editorial review; schema checks cannot establish the truth of every narrative claim.

Run `cargo test --lib` for mocked provider, quota, HTTP authorization, request, output, and safety tests. These tests do not contact Google or require PostgreSQL. The existing PostgreSQL CRUD tests remain separate.

Both content types have the same route structure (`<type>` is `blogs` or `projects`):

| Endpoint | Behavior |
| --- | --- |
| `GET /api/v1/<type>` | Public summaries, published and non-deleted only |
| `GET /api/v1/<type>/{slug}` | Full public content and resolved SEO; old slugs return 308 |
| `GET /api/v1/admin/<type>` | All non-deleted content, including drafts; `content:read` |
| `GET /api/v1/admin/<type>/{id}` | Full editable document; `content:read` |
| `POST /api/v1/admin/<type>` | Create; `<type>:write` |
| `PUT /api/v1/admin/<type>/{id}` | Replace editable document; `<type>:write` |
| `DELETE /api/v1/admin/<type>/{id}?version=1` | Soft delete; `<type>:delete` |

Publishing, archiving, and **any edit of currently published content** also require `<type>:publish`. A draft editor cannot bypass publication permissions by altering an already live record. Published records with future `publishedAt` are visible only to administrators until the timestamp passes, without requiring a background publication job.

Lists accept `page`, `perPage` (1–100), `category`, `tag`, `featured`, `q`, and `sort` (`newest`, `oldest`, `title`). Administration also accepts `status` (`draft`, `published`, `archived`). `q` searches title, description, and rendered body through PostgreSQL full-text search. Tags apply to blogs, featured to projects. Results use `{ data: [...], pagination: { page, perPage, total, totalPages } }`. Lists intentionally omit full bodies; use the detail route to read or edit content.

Writes contain `slug`, `title`, `description`, `status`, optional RFC3339 `publishedAt`, typed `fields`, optional `editorContent`, `seo`, and `faqs`. Omit `version` for creation; include the current integer `version` for a PUT. Successful edits increment it; a stale edit or deletion returns 409 instead of overwriting another change. PUT replaces the editable document, so preserve fields you want to keep. Unknown write properties are rejected. Author identity comes from the authenticated creator and cannot be supplied in a content request.

### Frontend field mapping

The frontend was inspected at `src/data/blog/types.ts` and `src/data/projects.ts`.

- Blog `fields`: `category` (`insights`, `growth`, `ai`, `news`), `cover`, `coverAlt`, `socialImage`, `intro`, `takeaways`, `sections`, and `tags`. Sections preserve IDs, paragraphs, points, cards, tables, and callouts.
- Project `fields`: `image`, `imageAlt`, `category`, four-digit string `year`, `technologies`, `githubUrl`, `liveUrl`, `featured`, `overview`, `problem`, `solution`, `features`, `technical`, `challenges`, and `results`.
- Public detail responses flatten those domain fields alongside `id`, `slug`, `title`, `description`, `author`, `publishedAt`, `updatedAt`, `faqs`, and the editor/SEO output. IDs are UUID strings. Display dates use `YYYY-MM-DD` for frontend compatibility; full timestamps are also supplied.
- Unavailable project links are empty strings; placeholder `#` URLs from the static frontend were removed from the example input. Image URLs accept HTTP(S) or root-relative frontend paths. `imageAlt` is added to the existing project model for accessibility.
- Editable administration responses retain the nested `fields` object, raw SEO overrides, status, version, and full RFC3339 timestamps suitable for the write schema. Public responses never expose account email, password hashes, or administrative permissions.

### Future Tiptap editor

`editorContent` is an optional Tiptap JSON document. Supported nodes are `doc`, `text`, `paragraph`, headings 2–6, bullet/ordered lists, list items, blockquotes, code blocks, hard breaks, horizontal rules, images, and tables/rows/cells/headers. Marks are bold, italic, strike, underline, code, and validated links. Other extensions are rejected until server support is added. Documents are bounded by request size, node count, and nesting depth.

The server renders the document and sanitizes the result with Ammonia; clients cannot submit arbitrary HTML or JSON-LD. Dangerous URL schemes and embedded executable nodes are rejected. If `editorContent` is absent, the existing structured blog/project fields generate the HTML. If present, it is authoritative for `renderedHtml` and full-text search; a future frontend should display that safe HTML or a matching safe Tiptap renderer rather than stale legacy section fields. Metadata and FAQs stay separate, and FAQ answers are rendered alongside their matching schema.

## SEO and frontend integration

Public details return:

- `seo`: meta title/description, canonical URL, Open Graph, Twitter, robots directives, and sensible defaults from the content.
- `structuredData`: BlogPosting or CreativeWork, BreadcrumbList, and FAQPage data generated from the stored visible content.
- `renderedHtml`: semantic, sanitized body content for server rendering; `readingMinutes` for archive cards.

SEO overrides include `metaTitle`, `metaDescription`, `canonicalUrl`, `ogTitle`, `ogDescription`, `ogImage`, `ogImageAlt`, `twitterTitle`, `twitterDescription`, `twitterImage`, `noIndex`, and `noFollow`. `FRONTEND_URL` supplies absolute frontend canonical and image URLs; the API's own URLs are not used as canonical article URLs.

`GET /sitemap.xml` contains only currently published, indexable, non-deleted content with self-canonical frontend URLs, including modification dates. Drafts, future publications, deleted records, noindex records, and records canonicalized elsewhere are omitted. `GET /robots.txt` discourages indexing backend JSON and administration paths and advertises the sitemap. API responses also carry `X-Robots-Tag: noindex, nofollow`; their purpose is to supply the frontend, not become search result pages.

The frontend workspace uses the authenticated content API and uploads images through a same-origin server endpoint backed by Cloudflare R2. Public blog routes now load this API through TanStack Query and route loaders, render the sanitized HTML on the server, map resolved SEO into route heads, and embed escaped article/breadcrumb/FAQ JSON-LD. Missing articles return 404 and previous slugs redirect to their current URL. Public project pages still use static data. Set `FRONTEND_URL` to the real public origin and add the backend sitemap URL to the frontend's robots/sitemap configuration.

## Verification

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Database API tests are ignored by default because they create temporary databases and require `CREATEDB`. Run them only against a disposable local PostgreSQL instance, never production. To use the supplied database without echoing its connection string:

```powershell
Get-Content .env.portfolio | ForEach-Object {
  if ($_ -match '^DATABASE_URL=(.+)$') { $env:DATABASE_URL = $Matches[1] }
}
cargo test --test portfolio_api -- --include-ignored
```

The suite covers publication visibility, scheduling, optimistic locking, slug redirects, sitemap exclusions, frontend-shaped data, editor safety, effective RBAC, owner protection, refresh rotation/replay, password changes, logout, rate limiting, and response headers. Each SQLx test uses its own database. The upstream demo tests remain present but are not Cargo targets.

## Deployment

Follow [Ubuntu deployment and database migration](docs/ubuntu-deployment.md) for the native PostgreSQL setup, backup/restore commands, verification, and rollback procedure. Build and start only the API:

```bash
docker compose --env-file .env.portfolio build api
docker compose --env-file .env.portfolio run --rm api migrate
docker compose --env-file .env.portfolio up -d api
```

The image runs as an unprivileged user and does not contain environment files or database backups. Runtime configuration comes from the private `.env.portfolio`. The service handles Docker's stop signal using the API's existing graceful shutdown. The database account must not be a PostgreSQL superuser. Terminate HTTPS at a reverse proxy on Ubuntu, configure shared login throttling there, and schedule `docker compose --env-file .env.portfolio run --rm api prune-sessions`. Health endpoints are `/health/live` and `/health/ready`.

Implementation references: [Axum](https://docs.rs/axum/0.8/axum/), [SQLx migrations](https://docs.rs/sqlx/0.8.6/sqlx/macro.migrate.html), [Argon2 0.5](https://docs.rs/argon2/0.5.3/argon2/), and [Ammonia](https://docs.rs/ammonia/latest/ammonia/).
