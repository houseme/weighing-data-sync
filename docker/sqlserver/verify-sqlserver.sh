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

"${SQLCMD}" "${SQLCMD_ENCRYPTION[@]}" -S sqlserver -U sa -P "${PASSWORD}" -i /opt/wds/e2e/verify-sqlserver.sql -b
