# AD Portfolio API

The Cargo package is `ad-portfolio-api`. Start with [PORTFOLIO.md](PORTFOLIO.md) for its setup, authentication, roles, blog/project APIs, editor schema, SEO contract, and tests. Cargo builds the `portfolio-api` executable and `portfolio_api` library from `src/portfolio/`. The upstream demo below is preserved as reference and is not the active application.

A minimalist, domain-driven Rust API server template using Axum and SQLx.  
Designed for clarity, scalability, and rapid development.

---

## 🔧 Features

- **Clean Architecture**: Clear separation of domain, infrastructure, and API layers
- **Modular Domains**: Self-contained features (auth, user, device, file)
- **SQLx Integration**: Compile-time-checked queries in offline mode
- **JWT Auth**: Secure authentication and authorization
- **File Uploads**: Asynchronous handling and secure asset serving
- **OpenAPI Docs**: Swagger UI powered by Utoipa
- **Observability**: OpenTelemetry tracing and metrics
- **Testing**: Unit and integration tests with `tokio::test` and `tower::ServiceExt`

---

## 📦 Project Structure

Recommended layout:

```text
├── src/
│   ├── main.rs                         # Application entry point
│   ├── app.rs                          # Router setup and middleware
│   ├── lib.rs                          # Module declarations
│   ├── common.rs
│   ├── common/                         # Shared components and utilities
│   │   ├── app_state.rs                # AppState struct for dependency injection
│   │   ├── bootstrap.rs                # Service initialization and AppState construction
│   │   ├── config.rs                   # Environment variable configuration loader
│   │   ├── dto.rs                      # Shared/global DTOs
│   │   ├── error.rs                    # AppError enum and error mappers
│   │   ├── hash_util.rs                # Hashing utilities (e.g., bcrypt)
│   │   ├── jwt.rs                      # JWT encoding, decoding, and validation
│   │   ├── multipart_helper.rs         # Multipart Helper
│   │   ├── opentelemetry.rs            # OpenTelemetry setup
│   │   └── ts_format.rs                # Custom timestamp serialization formatting

│   ├── domains.rs                      # Domain modules declarations
│   ├── domains/                        # Feature modules
│   │   ├── <feature>/                  # e.g., auth, user, device, file
│   │   │   ├── api/
│   │   │   │   ├── handlers.rs         # Route handlers
│   │   │   │   └── routes.rs           # Route definitions
│   │   │   ├── domain/                 # Domain models, traits
│   │   │   │   ├── model.rs
│   │   │   │   ├── repository.rs
│   │   │   │   └── service.rs
│   │   │   ├── dto/                    # Data Transfer Objects
│   │   │   │   └── <feature>_dto.rs
│   │   │   └── infra/                  # Infrastructure-layer implementations
│   │   │       ├── impl_repository.rs
│   │   │       └── impl_service.rs
│   │   ├── <feature>.rs                 # Module entry point

├── tests/
│   ├── asset/
│   ├── test_helpers.rs                 # Shared setup and utilities for tests
│   └── test_<feature>_routes.rs
├── .env                                # Environment variables for local development
├── .env.test                           # Environment overrides for test environment
```

When adding a new feature module, register it in:

> - `src/domains.rs`
> - `src/app.rs`
> - `src/common/app_state.rs`
> - `src/common/bootstrap.rs`

---

## 🛠 Getting Started

### Prerequisites

- Rust (latest stable)
- PostgreSQL
- Docker & Docker Compose (optional)

### Quickstart

Choose your preferred setup:

**Using Docker Compose:**

```bash
docker-compose up --build
```

To stop and clean up:

```bash
docker-compose down --rmi all
```

**Manual Setup:**

1. Create database tables and seed data:

   ```bash
   db-seed/01-tables.sql
   db-seed/02-seed.sql
   ```

2. Configure environment variables in `.env`:

   ```env
   DATABASE_URL=postgres://testuser:pass@localhost:5432/testdb
   JWT_SECRET_KEY=your_super_secret_key
   SERVICE_PORT=8080
   ```

3. Prepare SQLx offline mode with validation:

   ```bash
   cargo sqlx prepare --check
   ```

4. Run the server locally:

   ```bash
   cargo run
   ```

---

## 🚀 Usage

### Authenticate and Call Protected API

1. Login to obtain a JWT token:

   ```bash
   curl -X POST http://localhost:8080/auth/login \
     -H "Content-Type: application/json" \
     -d '{"client_id":"apitest01","client_secret":"test_password"}'
   ```

2. Use the returned `token` to access protected endpoints:

   ```bash
   curl http://localhost:8080/user -H "Authorization: Bearer $token"
   ```

### API Documentation

Open [http://localhost:8080/docs](http://localhost:8080/docs) in your browser for Swagger UI.

- Authenticate via `/auth/login` (POST) with JSON payload:

  ```json
  {
    "client_id": "apitest01",
    "client_secret": "test_password"
  }
  ```

- Copy the returned JWT token.
- Click the 🔒 Authorize button in Swagger UI and paste the token to authorize requests.

---

## 💡 Architecture

- **Domain**: Traits and models define core business logic.
- **Infra**: Concrete implementations (SQLx repositories, services)
- **API**: Axum handlers and route definitions
- **DTOs**: Typed request/response contracts
- **Bootstrap**: Wires dependencies into `AppState`

1. Create `domains/<feature>/` with `api/`, `domain/`, `infra/`, `dto/`
2. Register in `domains.rs`, `app.rs`, `common/app_state.rs`, `common/bootstrap.rs`

## 🧠 Domain-Driven Design & Architecture

### Domain Layer

- `model.rs`: holds your core structs and enums that represent entities or value objects.
- **Model Type Reference**: Conversions between Rust and PostgreSQL types.  
  [See SQLx Postgres types mapping](https://docs.rs/sqlx/latest/sqlx/postgres/types/index.html)
- `repository.rs`: declares the trait(s) that encapsulate persistence operations for the feature (e.g., `UserRepository`).
- `service.rs`: declares the trait(s) for feature service operations.

### Infra Layer

Each feature owns its own `impl_repository.rs` and `impl_service.rs`

`sqlx::query`

- Runtime-checked
- Flexibility: Handy when the SQL must be constructed dynamically—adding WHERE clauses on the fly, for instance.

`sqlx::query!`

- Compile-time-checked: The macro reads your SQL at build time (in “offline mode” if configured) and verifies it against your database schema. Mistyped column names or wrong argument types become compiler errors, not runtime surprises.
- Automatic type inference: You list your Rust values after the SQL string, and SQLx figures out how to map them to the placeholder types ($1, $2, …).
- Struct-level safety: If you use query_as!, it also confirms that the columns you select match the fields of your target struct.

### API Layer

- Route handlers accept DTOs, invoke feature logic, and return serialized responses.
- Each feature owns its own `routes.rs` and `handlers.rs`.
- Supports asynchronous multipart file uploads with validation.
- Secure file serving validates user permissions and sanitizes file paths.

### DTOs & Validation

- Request and response DTOs reside in each feature's `dto.rs`.
- Explicit mapping between DTOs and feature models.
- Uses `serde` and optionally the [validator](https://docs.rs/validator) crate for input validation.

### Automatic DTO Model Mapping

This project uses the [`simple_dto_mapper_derive::DtoFrom`](https://docs.rs/simple_dto_mapper_derive/) derive macro
to automatically generate `From` implementations for mapping between domain models and Data Transfer Objects (DTOs).
It reduces boilerplate by mapping fields with matching names and types, while allowing customization through attributes
for field renaming, type conversion, or transformation functions. This ensures consistent and maintainable
conversion logic between internal domain structures and API-facing DTOs.

### Use Case Isolation & Dependency Inversion

- Domain service traits define business contracts.
- Concrete implementations live in `impl_service.rs`, constructed via factory methods.
- `bootstrap.rs` wires services and builds `AppState` for dependency injection.

## Domain Code Autogeneration From a “CREATE TABLE” Script

- [domain_codegen](https://github.com/sukjaelee/domain_codegen) project provides a code generator specifically designed for the clean_axum_demo project. It automatically generates the feature layer structure under gen/, which you can copy and customize as needed.

---

## 🧱 Demo Database Schema

See the `db-seed/` directory for table definitions and sample data.  
The database structure is illustrated in the ERD:

![ERD](./ERD.png)

---

## 📚 API Documentation

- Swagger UI is available at `/docs` (powered by Utoipa). Open [http://localhost:8080/docs](http://localhost:8080/docs) in your browser for Swagger UI.
- DTOs and endpoints are annotated for OpenAPI specification.

---

## 📦 API Response Format

All endpoints return a consistent JSON envelope:

```json
{
  "status": 200,
  "message": "success",
  "data": { ... }
}
```

Implemented as:

- `ApiResponse<T>` – generic response wrapper
- `RestApiResponse<T>` – wrapper implementing Axum's `IntoResponse` trait

See definitions in `common/dto.rs`.

---

## 🧪 Testing

- Unit tests cover feature logic and core components.
- Integration tests exercise HTTP endpoints and database interactions.
- Use `#[tokio::test]` and `tower::ServiceExt` for HTTP simulation.
- Test assets and helpers are located in the `tests/` directory.

---

## 🚨 Error Handling

- Centralized `AppError` enum implements `IntoResponse`.
- Errors map to appropriate HTTP status codes with JSON structure, e.g.:

```json
{
  "status": 400,
  "message": "Invalid request data",
  "data": null
}
```

---

## 🧪 Environment Configuration

Configure via `.env` at the project root.  
Set database URL, JWT secret, service port, and asset settings.

Example `.env`:

```env
DATABASE_URL=postgres://testuser:pass@localhost:5432/testdb
JWT_SECRET_KEY=your_super_secret_key
SERVICE_PORT=8080
```

---

## 📡 OpenTelemetry (Tracing & Metrics)

This project supports distributed tracing, logging, and metrics via OpenTelemetry.

### Setup Jaeger Collector:

```bash
docker run --rm --name jaeger \
  -e COLLECTOR_OTLP_ENABLED=true \
  -p 16686:16686 \
  -p 4317:4317 \
  -p 4318:4318 \
  -p 5778:5778 \
  -p 9411:9411 \
  jaegertracing/jaeger:2.6.0
```

Access the Jaeger UI at [http://localhost:16686](http://localhost:16686).

### Enable OpenTelemetry Feature:

- Run with OpenTelemetry:

  ```bash
  cargo run --features opentelemetry
  ```

- Build with OpenTelemetry:

  ```bash
  cargo build --features opentelemetry
  ```

For details, see the [Jaeger Getting Started guide](https://www.jaegertracing.io/docs/2.6/getting-started/).

---

## 🤝 Contributing

Contributions are welcome! Feel free to open issues, suggest improvements, or submit pull requests with your ideas.

---

## 📄 License & Resources

- MIT License. See [LICENSE](./LICENSE) for details.

### Useful Links

- [Axum](https://docs.rs/axum)
- [SQLx](https://docs.rs/sqlx)
- [Utoipa (OpenAPI)](https://docs.rs/utoipa)
- [Tokio](https://tokio.rs/)
- [Validator (crate)](https://docs.rs/validator)
- [simple_dto_mapper_derive](https://docs.rs/simple_dto_mapper_derive/)
