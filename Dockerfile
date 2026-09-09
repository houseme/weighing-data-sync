FROM rust:1.98-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release --bin sync-daemon

FROM golang:1.26-bookworm AS go-receiver-builder

WORKDIR /app/cmd/receiver
COPY cmd/receiver/go.mod cmd/receiver/go.sum ./
RUN go mod download
COPY cmd/receiver ./
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /out/go-receiver .

FROM debian:bookworm-slim AS go-receiver

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates bash curl sqlite3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=go-receiver-builder /out/go-receiver /usr/local/bin/go-receiver
COPY docker/e2e /opt/wds/e2e

ENTRYPOINT ["go-receiver"]

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl jq sqlite3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/sync-daemon /usr/local/bin/sync-daemon
COPY config ./config
COPY docker/e2e /opt/wds/e2e

ENTRYPOINT ["sync-daemon"]
