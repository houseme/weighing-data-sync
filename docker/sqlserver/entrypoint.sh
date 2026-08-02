#!/usr/bin/env bash
set -euo pipefail

SQLCMD="/opt/mssql-tools18/bin/sqlcmd"
SQLCMD_ENCRYPTION=(-No)
if [[ ! -x "${SQLCMD}" ]]; then
    SQLCMD="/opt/mssql-tools/bin/sqlcmd"
    SQLCMD_ENCRYPTION=()
fi

PASSWORD="${MSSQL_SA_PASSWORD:-${SA_PASSWORD:-}}"
if [[ -z "${PASSWORD}" ]]; then
    echo "MSSQL_SA_PASSWORD is required" >&2
    exit 1
fi

/opt/mssql/bin/sqlservr &
SQLSERVER_PID="$!"

cleanup() {
    kill "${SQLSERVER_PID}" 2>/dev/null || true
    wait "${SQLSERVER_PID}" 2>/dev/null || true
}
trap cleanup SIGINT SIGTERM

for _ in $(seq 1 90); do
    if "${SQLCMD}" "${SQLCMD_ENCRYPTION[@]}" -S localhost -U sa -P "${PASSWORD}" -Q "SELECT 1" -b >/dev/null 2>&1; then
        break
    fi
    sleep 2
done

"${SQLCMD}" "${SQLCMD_ENCRYPTION[@]}" -S localhost -U sa -P "${PASSWORD}" -i /opt/wds/e2e/init.sql -b
touch /var/opt/mssql/.wds-e2e-seeded

wait "${SQLSERVER_PID}"
