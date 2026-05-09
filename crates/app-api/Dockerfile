# Multi-stage build for cookest-app-api
FROM rust:1.88-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/shared/Cargo.toml crates/shared/Cargo.toml
COPY crates/food-api/Cargo.toml crates/food-api/Cargo.toml
COPY crates/app-api/Cargo.toml crates/app-api/Cargo.toml

# Create dummy sources for dependency caching
RUN mkdir -p crates/shared/src crates/food-api/src crates/app-api/src && \
    echo "pub fn stub() {}" > crates/shared/src/lib.rs && \
    echo "fn main() {}" > crates/food-api/src/main.rs && \
    echo "fn main() {}" > crates/app-api/src/main.rs && \
    cargo build --release -p cookest-app-api 2>/dev/null || true

# Copy real sources and build
COPY crates/shared crates/shared
COPY crates/app-api crates/app-api
RUN touch crates/app-api/src/main.rs && \
    cargo build --release -p cookest-app-api

# Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/cookest-app-api /usr/local/bin/

EXPOSE 8080
CMD ["cookest-app-api"]
