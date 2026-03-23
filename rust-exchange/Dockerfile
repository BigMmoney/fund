# ── Stage 1: Build ────────────────────────────────────────────
FROM rust:1.88-slim AS builder
WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/types/Cargo.toml crates/types/Cargo.toml
COPY crates/eventbus/Cargo.toml crates/eventbus/Cargo.toml
COPY crates/instruments/Cargo.toml crates/instruments/Cargo.toml
COPY crates/projections/Cargo.toml crates/projections/Cargo.toml
COPY crates/persistence/Cargo.toml crates/persistence/Cargo.toml
COPY crates/sequencer/Cargo.toml crates/sequencer/Cargo.toml
COPY crates/ledger/Cargo.toml crates/ledger/Cargo.toml
COPY crates/risk/Cargo.toml crates/risk/Cargo.toml
COPY crates/matching/Cargo.toml crates/matching/Cargo.toml
COPY crates/api/Cargo.toml crates/api/Cargo.toml

# Create stub lib.rs for each crate to cache dependency compilation
RUN for dir in types eventbus instruments projections persistence sequencer ledger risk matching; do \
      mkdir -p crates/$dir/src && echo "" > crates/$dir/src/lib.rs; \
    done && \
    mkdir -p crates/api/src && echo "fn main() {}" > crates/api/src/main.rs

RUN cargo build --release 2>/dev/null || true

# Copy real source code
COPY crates/ crates/

# Touch sources to invalidate the stubs
RUN find crates/ -name "*.rs" -exec touch {} +

RUN cargo build --release --bin api

# ── Stage 2: Runtime ──────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r exchange && useradd -r -g exchange exchange

WORKDIR /app
COPY --from=builder /build/target/release/api /app/exchange
COPY config/ /app/config/
RUN mkdir -p /app/data && chown -R exchange:exchange /app

USER exchange

ENV RUST_LOG=info
ENV API_BIND_HOST=0.0.0.0
ENV API_BIND_PORT=3030

EXPOSE 3030

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -sf http://localhost:3030/health || exit 1

ENTRYPOINT ["/app/exchange"]
