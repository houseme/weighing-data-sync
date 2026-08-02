#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${ROOT_DIR}/docker/docker-compose.e2e.yml"
PROJECT_NAME="${WDS_E2E_PROJECT_NAME:-weighing-data-sync-e2e}"

cleanup() {
    docker compose -p "${PROJECT_NAME}" -f "${COMPOSE_FILE}" down -v --remove-orphans >/dev/null 2>&1 || true
}

ARCH="$(uname -m)"
if [[ "${ARCH}" != "x86_64" && "${ARCH}" != "amd64" && "${WDS_E2E_ALLOW_UNSUPPORTED_SQLSERVER_EMULATION:-}" != "1" ]]; then
    cat >&2 <<'MSG'
SQL Server Linux containers are officially supported only on Intel/AMD x86-64 Linux hosts.
Set WDS_E2E_ALLOW_UNSUPPORTED_SQLSERVER_EMULATION=1 to try Docker Desktop/OrbStack emulation anyway.
MSG
    exit 2
fi

trap cleanup EXIT
cleanup
docker compose -p "${PROJECT_NAME}" -f "${COMPOSE_FILE}" up --build --abort-on-container-exit --exit-code-from e2e e2e
