#!/usr/bin/env bash
set -euo pipefail

DB_PATH="${WDS_E2E_RECEIVER_DB:-/data/weighing_sync.db}"
EXPECTED_SOURCE="sqlserver-yunfu-tbl_weightInfo"
EXPECTED_DATABASE="yunfu"
EXPECTED_TABLE="tbl_weightInfo"

for _ in $(seq 1 60); do
    if [[ -f "${DB_PATH}" ]] \
        && sqlite3 "${DB_PATH}" "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'inbound_payloads';" | grep -q 1; then
        break
    fi
    sleep 1
done

if [[ ! -f "${DB_PATH}" ]]; then
    echo "receiver SQLite database not found: ${DB_PATH}" >&2
    exit 1
fi

batch_count="$(
    sqlite3 "${DB_PATH}" "
        SELECT COUNT(*)
        FROM inbound_payloads
        WHERE source = '${EXPECTED_SOURCE}'
          AND source_db = '${EXPECTED_DATABASE}'
          AND source_table = '${EXPECTED_TABLE}'
          AND records_count = 100;
    "
)"

if [[ "${batch_count}" != "1" ]]; then
    echo "expected exactly one persisted SQL Server batch with 100 records, got ${batch_count}" >&2
    exit 1
fi

accepted_count="$(
    sqlite3 "${DB_PATH}" "
        SELECT COUNT(DISTINCT json_extract(json_each.value, '$.serialNo'))
        FROM inbound_payloads, json_each(inbound_payloads.payload_json, '$.records')
        WHERE inbound_payloads.source = '${EXPECTED_SOURCE}'
          AND json_extract(json_each.value, '$.serialNo') GLOB 'WDS-E2E-[0-9][0-9][0-9][0-9]';
    "
)"

if [[ "${accepted_count}" != "100" ]]; then
    echo "expected all 100 pending serial numbers in receiver payload, got ${accepted_count}" >&2
    exit 1
fi

out_of_range_count="$(
    sqlite3 "${DB_PATH}" "
        SELECT COUNT(*)
        FROM inbound_payloads, json_each(inbound_payloads.payload_json, '$.records')
        WHERE json_extract(json_each.value, '$.serialNo') NOT GLOB 'WDS-E2E-[0-9][0-9][0-9][0-9]';
    "
)"

if [[ "${out_of_range_count}" != "0" ]]; then
    echo "receiver payload included records outside the seeded 100-row range" >&2
    exit 1
fi

echo "receiver persistence verification passed"
