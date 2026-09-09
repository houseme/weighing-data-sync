#!/usr/bin/env bash
set -euo pipefail

DB_PATH="${WDS_E2E_RECEIVER_DB:-/data/receiver.db}"
EXPECTED_SOURCE="sqlserver-yunfu-tbl_weightInfo"
EXPECTED_DATABASE="yunfu"
EXPECTED_TABLE="tbl_weightInfo"

for _ in $(seq 1 60); do
    if [[ -f "${DB_PATH}" ]] \
        && sqlite3 "${DB_PATH}" "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'wds_receive_batches';" | grep -q 1; then
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
        FROM wds_receive_batches
        WHERE source = '${EXPECTED_SOURCE}'
          AND source_database = '${EXPECTED_DATABASE}'
          AND source_table = '${EXPECTED_TABLE}'
          AND records_count = 100
          AND accepted_count = 100
          AND failed_count = 0;
    "
)"

if [[ "${batch_count}" != "1" ]]; then
    echo "expected exactly one Go receiver batch with 100 accepted SQL Server records, got ${batch_count}" >&2
    exit 1
fi

accepted_count="$(
    sqlite3 "${DB_PATH}" "
        SELECT COUNT(DISTINCT serial_no)
        FROM wds_receive_records
        WHERE source = '${EXPECTED_SOURCE}'
          AND source_database = '${EXPECTED_DATABASE}'
          AND source_table = '${EXPECTED_TABLE}'
          AND entity_type = 'weight_info'
          AND serial_no GLOB 'WDS-E2E-[0-9][0-9][0-9][0-9]';
    "
)"

if [[ "${accepted_count}" != "100" ]]; then
    echo "expected all 100 pending serial numbers in Go receiver records, got ${accepted_count}" >&2
    exit 1
fi

out_of_range_count="$(
    sqlite3 "${DB_PATH}" "
        SELECT COUNT(*)
        FROM wds_receive_records
        WHERE entity_type = 'weight_info'
          AND serial_no NOT GLOB 'WDS-E2E-[0-9][0-9][0-9][0-9]';
    "
)"

if [[ "${out_of_range_count}" != "0" ]]; then
    echo "Go receiver records included rows outside the seeded 100-row range" >&2
    exit 1
fi

raw_record_count="$(
    sqlite3 "${DB_PATH}" "
        SELECT COUNT(*)
        FROM wds_receive_records
        WHERE entity_type = 'weight_info'
          AND raw_record IS NOT NULL
          AND json_valid(raw_record);
    "
)"

if [[ "${raw_record_count}" != "100" ]]; then
    echo "expected Go receiver to persist 100 raw records for downstream replication, got ${raw_record_count}" >&2
    exit 1
fi

echo "receiver persistence verification passed"
