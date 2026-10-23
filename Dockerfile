FROM rust:1.97-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/portfolio ./src/portfolio
COPY migrations ./migrations
COPY examples ./examples
COPY docs/openapi.json docs/swagger-ui.html ./docs/
RUN cargo build --locked --release --bin portfolio-api

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home portfolio
WORKDIR /app
COPY --from=builder /app/target/release/portfolio-api /usr/local/bin/portfolio-api
USER portfolio
ENV SERVICE_HOST=0.0.0.0
EXPOSE 8080
ENTRYPOINT ["portfolio-api"]
CMD ["serve"]
