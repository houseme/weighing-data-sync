FROM rust:1.98-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release --bin sync-daemon

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl jq sqlite3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/sync-daemon /usr/local/bin/sync-daemon
COPY config ./config
COPY docker/e2e /opt/wds/e2e

ENTRYPOINT ["sync-daemon"]
