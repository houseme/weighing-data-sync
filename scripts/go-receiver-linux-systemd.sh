#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="${SERVICE_NAME:-go-receiver}"
INSTALL_DIR="${INSTALL_DIR:-/opt/weighing/go-receiver}"
CONFIG_DIR="${CONFIG_DIR:-/etc/weighing}"
DATA_DIR="${DATA_DIR:-/var/lib/weighing/go-receiver}"
ENV_FILE="${ENV_FILE:-${CONFIG_DIR}/go-receiver.env}"
UNIT_FILE="${UNIT_FILE:-/etc/systemd/system/${SERVICE_NAME}.service}"
EXE_PATH="${EXE_PATH:-./go-receiver}"
RUN_USER="${RUN_USER:-root}"
ACTION="${1:-install}"

usage() {
    cat <<EOF
Usage: $0 <install|start|stop|restart|status|logs|uninstall>

Environment overrides:
  EXE_PATH=/path/to/go-receiver
  SERVICE_NAME=go-receiver
  INSTALL_DIR=/opt/weighing/go-receiver
  CONFIG_DIR=/etc/weighing
  DATA_DIR=/var/lib/weighing/go-receiver
  ENV_FILE=/etc/weighing/go-receiver.env
  SERVER_ADDR=:80
  SQLITE_PATH=/var/lib/weighing/go-receiver/receiver.db
  STORE_RAW_RECORDS=true
  STORE_RAW_PAYLOAD=false
  MAX_BODY_BYTES=67108864
  INGEST_API_TOKEN=...
  INGEST_SIGN_SECRET=...
  QUERY_API_TOKEN=...
  QUERY_SIGN_SECRET=...
  CLEANUP_API_TOKEN=...
  CLEANUP_SIGN_SECRET=...
  OVERWRITE_ENV=1
  REMOVE_DATA=1
  REMOVE_CONFIG=1

install preserves an existing ENV_FILE unless OVERWRITE_ENV=1 is set.
EOF
}

require_root() {
    if [[ "$(id -u)" != "0" ]]; then
        echo "please run as root" >&2
        exit 1
    fi
}

require_systemd() {
    if ! command -v systemctl >/dev/null 2>&1; then
        echo "systemctl is required" >&2
        exit 1
    fi
}

rand_hex() {
    local bytes="$1"
    if command -v openssl >/dev/null 2>&1; then
        openssl rand -hex "$bytes"
    else
        od -An -N "$bytes" -tx1 /dev/urandom | tr -d ' \n'
    fi
}

existing_env_value() {
    local key="$1"
    if [[ -f "$ENV_FILE" ]]; then
        sed -n "s/^${key}=//p" "$ENV_FILE" | tail -n 1
    fi
}

value_or_existing_or_generated() {
    local key="$1"
    local current="${!key:-}"
    if [[ -n "$current" ]]; then
        printf '%s\n' "$current"
        return
    fi

    current="$(existing_env_value "$key" || true)"
    if [[ -n "$current" ]]; then
        printf '%s\n' "$current"
        return
    fi

    rand_hex 32
}

write_env_file() {
    if [[ -f "$ENV_FILE" && "${OVERWRITE_ENV:-0}" != "1" ]]; then
        echo "kept existing env file: $ENV_FILE"
        return
    fi

    install -d -m 0755 "$CONFIG_DIR"
    install -d -m 0750 "$DATA_DIR"

    local sqlite_path="${SQLITE_PATH:-${DATA_DIR}/receiver.db}"
    local server_addr="${SERVER_ADDR:-:80}"
    local store_raw_records="${STORE_RAW_RECORDS:-true}"
    local store_raw_payload="${STORE_RAW_PAYLOAD:-false}"
    local max_body_bytes="${MAX_BODY_BYTES:-67108864}"

    local ingest_token ingest_secret query_token query_secret cleanup_token cleanup_secret
    ingest_token="$(value_or_existing_or_generated INGEST_API_TOKEN)"
    ingest_secret="$(value_or_existing_or_generated INGEST_SIGN_SECRET)"
    query_token="$(value_or_existing_or_generated QUERY_API_TOKEN)"
    query_secret="$(value_or_existing_or_generated QUERY_SIGN_SECRET)"
    cleanup_token="$(value_or_existing_or_generated CLEANUP_API_TOKEN)"
    cleanup_secret="$(value_or_existing_or_generated CLEANUP_SIGN_SECRET)"

    umask 077
    {
        printf 'SERVER_ADDR=%s\n' "$server_addr"
        printf 'SQLITE_PATH=%s\n' "$sqlite_path"
        printf 'STORE_RAW_RECORDS=%s\n' "$store_raw_records"
        printf 'STORE_RAW_PAYLOAD=%s\n' "$store_raw_payload"
        printf 'MAX_BODY_BYTES=%s\n' "$max_body_bytes"
        printf 'INGEST_API_TOKEN=%s\n' "$ingest_token"
        printf 'INGEST_SIGN_SECRET=%s\n' "$ingest_secret"
        printf 'QUERY_API_TOKEN=%s\n' "$query_token"
        printf 'QUERY_SIGN_SECRET=%s\n' "$query_secret"
        printf 'CLEANUP_API_TOKEN=%s\n' "$cleanup_token"
        printf 'CLEANUP_SIGN_SECRET=%s\n' "$cleanup_secret"
    } > "$ENV_FILE"
    chmod 0600 "$ENV_FILE"
    echo "wrote env file: $ENV_FILE"
}

write_unit_file() {
    cat > "$UNIT_FILE" <<EOF
[Unit]
Description=Weighing Data Go Receiver
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${RUN_USER}
WorkingDirectory=${INSTALL_DIR}
EnvironmentFile=${ENV_FILE}
ExecStart=${INSTALL_DIR}/go-receiver
Restart=always
RestartSec=5
ReadWritePaths=${DATA_DIR}
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
EOF
    chmod 0644 "$UNIT_FILE"
    echo "wrote systemd unit: $UNIT_FILE"
}

install_service() {
    require_root
    require_systemd

    if [[ ! -f "$EXE_PATH" ]]; then
        echo "go-receiver binary not found: $EXE_PATH" >&2
        exit 1
    fi

    install -d -m 0755 "$INSTALL_DIR"
    install -m 0755 "$EXE_PATH" "${INSTALL_DIR}/go-receiver"
    write_env_file
    write_unit_file

    systemctl daemon-reload
    systemctl enable "$SERVICE_NAME"
    systemctl restart "$SERVICE_NAME"
    systemctl --no-pager --full status "$SERVICE_NAME"
}

uninstall_service() {
    require_root
    require_systemd

    systemctl stop "$SERVICE_NAME" >/dev/null 2>&1 || true
    systemctl disable "$SERVICE_NAME" >/dev/null 2>&1 || true
    rm -f "$UNIT_FILE"
    systemctl daemon-reload

    rm -f "${INSTALL_DIR}/go-receiver"
    rmdir "$INSTALL_DIR" >/dev/null 2>&1 || true

    if [[ "${REMOVE_DATA:-0}" == "1" ]]; then
        rm -rf "$DATA_DIR"
        echo "removed data directory: $DATA_DIR"
    else
        echo "kept data directory: $DATA_DIR"
    fi

    if [[ "${REMOVE_CONFIG:-0}" == "1" ]]; then
        rm -f "$ENV_FILE"
        rmdir "$CONFIG_DIR" >/dev/null 2>&1 || true
        echo "removed env file: $ENV_FILE"
    else
        echo "kept env file: $ENV_FILE"
    fi
}

case "$ACTION" in
    install)
        install_service
        ;;
    start|stop|restart|status)
        require_systemd
        systemctl "$ACTION" "$SERVICE_NAME"
        ;;
    logs)
        require_systemd
        journalctl -u "$SERVICE_NAME" -f
        ;;
    uninstall)
        uninstall_service
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
