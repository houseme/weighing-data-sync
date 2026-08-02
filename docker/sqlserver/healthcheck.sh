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
    exit 1
fi

"${SQLCMD}" "${SQLCMD_ENCRYPTION[@]}" -S localhost -U sa -P "${PASSWORD}" -d yunfu -h -1 -W -Q "
SET NOCOUNT ON;
IF OBJECT_ID('dbo.tbl_weightInfo', 'U') IS NULL
    THROW 51000, 'dbo.tbl_weightInfo is not initialized', 1;
DECLARE @seeded int = (
    SELECT COUNT(*)
    FROM dbo.tbl_weightInfo
);
IF @seeded <> 100
    THROW 51001, 'unexpected seeded row count', 1;
SELECT 1;
" -b >/dev/null
