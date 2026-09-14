package main

import (
	"context"
	"database/sql"
	"testing"
	"time"
)

func TestDeleteRecordRequiresMinimumAge(t *testing.T) {
	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		t.Fatalf("open sqlite: %v", err)
	}
	defer db.Close()

	ctx := context.Background()
	if err := migrate(ctx, "sqlite", db); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	now := time.Now().UTC()
	oldTime := timeString(now.Add(-8 * 24 * time.Hour))
	recentTime := timeString(now)
	_, err = db.ExecContext(ctx, `
		INSERT INTO wds_receive_records
			(record_key, entity_type, key_type, source_time, source, source_database, source_table, ingested_at, cloud_updated_at)
		VALUES
			('weight_info:old', 'weight_info', 'serialNo', ?, 'test', 'test', 'tbl_weightInfo', ?, ?),
			('weight_info:recent', 'weight_info', 'serialNo', ?, 'test', 'test', 'tbl_weightInfo', ?, ?)
	`, oldTime, oldTime, oldTime, recentTime, recentTime, recentTime)
	if err != nil {
		t.Fatalf("insert records: %v", err)
	}

	s := &server{cfg: config{minDeleteAge: defaultMinDeleteAge}, db: db}
	deleted, err := s.deleteRecord(ctx, "record_key = ?", "weight_info:old")
	if err != nil {
		t.Fatalf("delete old record: %v", err)
	}
	if !deleted {
		t.Fatal("old record was not deleted")
	}

	deleted, err = s.deleteRecord(ctx, "record_key = ?", "weight_info:recent")
	if err != nil {
		t.Fatalf("delete recent record: %v", err)
	}
	if deleted {
		t.Fatal("recent record was deleted before the minimum age")
	}
}
