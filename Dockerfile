# Multi-stage build for strait-node — the single binary that wires together
# all ingesters, the join engine, and the HTTP API.
#
# No DATABASE_URL is needed at build time: strait-store uses runtime-checked
# sqlx::query (not the query!/query_as! macros), and migrations are embedded
# via sqlx::migrate!("./migrations") at compile time from source, not fetched
# from a live DB.

FROM rust:1-bookworm AS builder
WORKDIR /app

# Cache dependency compilation separately from source changes.
COPY Cargo.toml Cargo.lock ./
COPY crates/strait-core/Cargo.toml crates/strait-core/Cargo.toml
COPY crates/strait-bitcoin/Cargo.toml crates/strait-bitcoin/Cargo.toml
COPY crates/strait-evm/Cargo.toml crates/strait-evm/Cargo.toml
COPY crates/strait-join/Cargo.toml crates/strait-join/Cargo.toml
COPY crates/strait-store/Cargo.toml crates/strait-store/Cargo.toml
COPY crates/strait-api/Cargo.toml crates/strait-api/Cargo.toml
COPY crates/strait-node/Cargo.toml crates/strait-node/Cargo.toml

COPY crates/ crates/
RUN cargo build --release -p strait-node

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/strait-node /usr/local/bin/strait-node

# Render (and most PaaS hosts) inject $PORT and expect the app to bind to it;
# strait-node reads API_PORT/API_HOST instead, so map one to the other at
# startup rather than hardcoding a port the platform doesn't control.
ENV API_HOST=0.0.0.0
CMD ["sh", "-c", "API_PORT=${PORT:-8080} exec strait-node"]
