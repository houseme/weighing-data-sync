CREATE TABLE IF NOT EXISTS wds_receive_records (
  id integer PRIMARY KEY AUTOINCREMENT CHECK (id >= 0),
  record_key text NOT NULL,
  entity_type text NOT NULL DEFAULT 'weight_info',
  key_type text NOT NULL,
  serial_no text,
  plate_no text,
  source_time text,
  source text NOT NULL DEFAULT 'unknown',
  source_database text NOT NULL DEFAULT 'unknown',
  source_table text NOT NULL DEFAULT 'unknown',
  uploaded_at text,
  raw_record text CHECK (raw_record IS NULL OR json_valid(raw_record)),
  ingested_at text NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  cloud_updated_at text NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (record_key)
);

CREATE INDEX IF NOT EXISTS idx_wds_receive_plate_time
ON wds_receive_records (plate_no, source_time, record_key);

CREATE INDEX IF NOT EXISTS idx_wds_receive_entity_type
ON wds_receive_records (entity_type, ingested_at, record_key);

CREATE INDEX IF NOT EXISTS idx_wds_receive_serial_no
ON wds_receive_records (serial_no);

CREATE INDEX IF NOT EXISTS idx_wds_receive_source_time
ON wds_receive_records (source_database, source_table, source_time, record_key);

CREATE TABLE IF NOT EXISTS wds_receive_batches (
  id integer PRIMARY KEY AUTOINCREMENT CHECK (id >= 0),
  request_id text NOT NULL,
  source text NOT NULL,
  source_database text NOT NULL,
  source_table text NOT NULL,
  records_count integer NOT NULL,
  accepted_count integer NOT NULL DEFAULT 0,
  failed_count integer NOT NULL DEFAULT 0,
  uploaded_at text,
  received_at text NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  raw_payload text CHECK (raw_payload IS NULL OR json_valid(raw_payload)),
  UNIQUE (request_id)
);

CREATE INDEX IF NOT EXISTS idx_wds_receive_batch_source_time
ON wds_receive_batches (source_database, source_table, received_at);
