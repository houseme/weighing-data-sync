package main

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"os"
	"os/signal"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	_ "modernc.org/sqlite"
)

const (
	defaultAddr         = ":18081"
	defaultRoute        = "/weighing-data-sync/put"
	defaultQueryRoute   = "/weighing-data-sync/records"
	defaultMaxBodyBytes = 16 << 20
	defaultLimit        = 100
	defaultMaxLimit     = 1000
	defaultSignSkew     = 5 * time.Minute
	insertChunkSize     = 500
)

type config struct {
	addr             string
	sqliteDSN        string
	writeAuth        authConfig
	readAuth         authConfig
	deleteAuth       authConfig
	route            string
	queryRoute       string
	maxBodyBytes     int64
	defaultLimit     int
	maxLimit         int
	signSkew         time.Duration
	autoMigrate      bool
	storeRawRecords  bool
	storeRawPayload  bool
	returnRawRecords bool
	requireAuth      bool
}

type authConfig struct {
	apiToken   string
	signSecret string
}

type server struct {
	cfg    config
	db     *sql.DB
	nonces *nonceStore
}

type uploadPayload struct {
	Source             string           `json:"source"`
	Database           string           `json:"database"`
	Table              string           `json:"table"`
	UploadedAt         any              `json:"uploaded_at"`
	Records            []map[string]any `json:"records"`
	WeightInfoRecords  []map[string]any `json:"weight_info_records"`
	WeightPhotoRecords []map[string]any `json:"weight_photo_records"`
}

type recordRow struct {
	ID             int64
	RecordKey      string
	EntityType     string
	KeyType        string
	SerialNo       sql.NullString
	PlateNo        sql.NullString
	SourceTime     sql.NullString
	Source         sql.NullString
	SourceDatabase sql.NullString
	SourceTable    sql.NullString
	UploadedAt     sql.NullString
	IngestedAt     string
	RawRecord      sql.NullString
}

type nonceStore struct {
	mu    sync.Mutex
	items map[string]time.Time
}

type entityRecordBatch struct {
	entityType  string
	sourceTable string
	records     []map[string]any
}

type entityRecordBatches []entityRecordBatch

func (b entityRecordBatches) total() int {
	total := 0
	for _, batch := range b {
		total += len(batch.records)
	}
	return total
}

func (p uploadPayload) normalizedWeightInfo() []map[string]any {
	if len(p.WeightInfoRecords) > 0 {
		return p.WeightInfoRecords
	}
	return p.Records
}

func (p uploadPayload) entityRecords() entityRecordBatches {
	return entityRecordBatches{
		{entityType: "weight_info", sourceTable: "tbl_weightInfo", records: p.normalizedWeightInfo()},
		{entityType: "weight_photo", sourceTable: "tbl_weightPhoto", records: p.WeightPhotoRecords},
	}
}

func acceptedWeightInfoSerials(payload uploadPayload, acceptedKeys []string) []string {
	keySet := map[string]struct{}{}
	for _, key := range acceptedKeys {
		keySet[key] = struct{}{}
	}
	serials := make([]string, 0)
	for _, record := range payload.normalizedWeightInfo() {
		_, rawKey := extractRecordKey("weight_info", record)
		if _, ok := keySet["weight_info:"+rawKey]; !ok {
			continue
		}
		if serial := extractString(record, "serialNo", "serial_no"); serial != "" {
			serials = append(serials, serial)
		}
	}
	return serials
}

func main() {
	cfg, err := loadConfig()
	if err != nil {
		log.Fatalf("config error: %v", err)
	}

	db, err := sql.Open("sqlite", cfg.sqliteDSN)
	if err != nil {
		log.Fatalf("open sqlite: %v", err)
	}
	defer db.Close()

	db.SetMaxOpenConns(envInt("SQLITE_MAX_OPEN_CONNS", 1))
	db.SetMaxIdleConns(envInt("SQLITE_MAX_IDLE_CONNS", 1))
	db.SetConnMaxLifetime(time.Duration(envInt("SQLITE_CONN_MAX_LIFETIME_SECONDS", 300)) * time.Second)

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	if err := db.PingContext(ctx); err != nil {
		cancel()
		log.Fatalf("ping sqlite: %v", err)
	}
	if err := configureSQLite(ctx, db); err != nil {
		cancel()
		log.Fatalf("configure sqlite: %v", err)
	}
	cancel()

	if cfg.autoMigrate {
		ctx, cancel = context.WithTimeout(context.Background(), 20*time.Second)
		if err := migrate(ctx, db); err != nil {
			cancel()
			log.Fatalf("migrate sqlite: %v", err)
		}
		cancel()
	}

	app := &server{
		cfg:    cfg,
		db:     db,
		nonces: &nonceStore{items: make(map[string]time.Time)},
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", app.health)
	mux.HandleFunc("POST "+cfg.route, app.handlePost)
	mux.HandleFunc("GET "+cfg.queryRoute, app.handleQuery)
	mux.HandleFunc("DELETE "+cfg.queryRoute, app.handleDelete)
	mux.HandleFunc("DELETE "+cfg.queryRoute+"/{id}", app.handleDeleteByID)
	mux.HandleFunc("DELETE "+cfg.queryRoute+"/by-key/{record_key}", app.handleDeleteByKey)

	httpServer := &http.Server{
		Addr:              cfg.addr,
		Handler:           requestLog(mux),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      30 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	go func() {
		log.Printf("go receiver listening on %s, post=%s, query=%s", cfg.addr, cfg.route, cfg.queryRoute)
		if err := httpServer.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Fatalf("http server: %v", err)
		}
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)
	<-stop

	ctx, cancel = context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := httpServer.Shutdown(ctx); err != nil {
		log.Printf("http shutdown error: %v", err)
	}
}

func loadConfig() (config, error) {
	sqliteDSN, err := loadSQLiteDSN()
	if err != nil {
		return config{}, err
	}
	cfg := config{
		addr:             envString("SERVER_ADDR", defaultAddr),
		sqliteDSN:        sqliteDSN,
		writeAuth:        roleAuth("INGEST", "WRITE"),
		readAuth:         roleAuth("QUERY", "READ"),
		deleteAuth:       roleAuth("CLEANUP", "DELETE"),
		route:            envString("POST_ROUTE", defaultRoute),
		queryRoute:       envString("QUERY_ROUTE", defaultQueryRoute),
		maxBodyBytes:     int64(envInt("MAX_BODY_BYTES", defaultMaxBodyBytes)),
		defaultLimit:     envInt("QUERY_DEFAULT_LIMIT", defaultLimit),
		maxLimit:         envInt("QUERY_MAX_LIMIT", defaultMaxLimit),
		signSkew:         time.Duration(envInt("SIGN_SKEW_SECONDS", int(defaultSignSkew.Seconds()))) * time.Second,
		autoMigrate:      envBool("AUTO_MIGRATE", true),
		storeRawRecords:  envBool("STORE_RAW_RECORDS", false),
		storeRawPayload:  envBool("STORE_RAW_PAYLOAD", false),
		returnRawRecords: envBool("RETURN_RAW_RECORDS", false),
		requireAuth:      envBool("REQUIRE_AUTH", true),
	}
	if cfg.defaultLimit <= 0 || cfg.maxLimit <= 0 || cfg.defaultLimit > cfg.maxLimit {
		return cfg, errors.New("invalid query limit settings")
	}
	if cfg.requireAuth {
		if err := cfg.writeAuth.validate("INGEST"); err != nil {
			return cfg, err
		}
		if err := cfg.readAuth.validate("QUERY"); err != nil {
			return cfg, err
		}
		if err := cfg.deleteAuth.validate("CLEANUP"); err != nil {
			return cfg, err
		}
	}
	return cfg, nil
}

func roleAuth(prefix string, aliases ...string) authConfig {
	tokenKeys := []string{prefix + "_API_TOKEN"}
	secretKeys := []string{prefix + "_SIGN_SECRET"}
	for _, alias := range aliases {
		tokenKeys = append(tokenKeys, alias+"_API_TOKEN")
		secretKeys = append(secretKeys, alias+"_SIGN_SECRET")
	}
	tokenKeys = append(tokenKeys, "API_TOKEN")
	secretKeys = append(secretKeys, "SIGN_SECRET")
	return authConfig{
		apiToken:   envFirst(tokenKeys...),
		signSecret: envFirst(secretKeys...),
	}
}

func (a authConfig) validate(role string) error {
	if a.apiToken == "" {
		return fmt.Errorf("%s_API_TOKEN is required when REQUIRE_AUTH=true", role)
	}
	if a.signSecret == "" {
		return fmt.Errorf("%s_SIGN_SECRET is required when REQUIRE_AUTH=true", role)
	}
	return nil
}

func loadSQLiteDSN() (string, error) {
	if dsn := strings.TrimSpace(os.Getenv("SQLITE_DSN")); dsn != "" {
		return dsn, nil
	}

	path := envString("SQLITE_PATH", "data/receiver.db")
	if path != ":memory:" && !strings.HasPrefix(path, "file:") {
		dir := filepath.Dir(path)
		if dir != "." {
			if err := os.MkdirAll(dir, 0o755); err != nil {
				return "", fmt.Errorf("create sqlite directory: %w", err)
			}
		}
	}
	return path, nil
}

func configureSQLite(ctx context.Context, db *sql.DB) error {
	for _, stmt := range []string{
		"PRAGMA foreign_keys = ON",
		"PRAGMA journal_mode = WAL",
		"PRAGMA synchronous = NORMAL",
		"PRAGMA busy_timeout = 5000",
	} {
		if _, err := db.ExecContext(ctx, stmt); err != nil {
			return err
		}
	}
	return nil
}

func (s *server) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"service": "weighing-data-go-receiver",
		"status":  "ok",
		"time":    time.Now().UTC().Format(time.RFC3339Nano),
	})
}

func (s *server) handlePost(w http.ResponseWriter, r *http.Request) {
	body, err := readBody(r, s.cfg.maxBodyBytes)
	if err != nil {
		writeError(w, http.StatusRequestEntityTooLarge, err.Error())
		return
	}
	if err := s.authorize(r, body, s.cfg.writeAuth); err != nil {
		writeError(w, http.StatusUnauthorized, err.Error())
		return
	}

	var payload uploadPayload
	dec := json.NewDecoder(bytes.NewReader(body))
	dec.UseNumber()
	if err := dec.Decode(&payload); err != nil {
		writeError(w, http.StatusBadRequest, "invalid json body")
		return
	}
	entities := payload.entityRecords()
	if entities.total() == 0 {
		writeError(w, http.StatusBadRequest, "records is missing or empty")
		return
	}

	requestID := newUUID()
	uploadedAt := parseAnyTime(payload.UploadedAt)
	ctx, cancel := context.WithTimeout(r.Context(), 30*time.Second)
	defer cancel()

	accepted, failed, err := s.insertPayload(ctx, requestID, payload, uploadedAt)
	if err != nil {
		log.Printf("insert payload failed request_id=%s error=%v", requestID, err)
		writeError(w, http.StatusInternalServerError, "sqlite write failed")
		return
	}
	acceptedSerials := acceptedWeightInfoSerials(payload, accepted)

	writeJSON(w, http.StatusOK, map[string]any{
		"request_id":           requestID,
		"accepted":             len(failed) == 0,
		"accepted_record_keys": accepted,
		"failed_record_keys":   failed,
		"accepted_serial_nos":  acceptedSerials,
		"failed_serial_nos":    []string{},
		"records_count":        entities.total(),
		"weight_info_count":    len(payload.normalizedWeightInfo()),
		"weight_photo_count":   len(payload.WeightPhotoRecords),
		"received_at":          time.Now().UTC().Format(time.RFC3339Nano),
	})
}

func (s *server) handleQuery(w http.ResponseWriter, r *http.Request) {
	if err := s.authorize(r, nil, s.cfg.readAuth); err != nil {
		writeError(w, http.StatusUnauthorized, err.Error())
		return
	}

	q := r.URL.Query()
	id, idOK := parseOptionalInt64(q.Get("id"))
	if q.Get("id") != "" && !idOK {
		writeError(w, http.StatusBadRequest, "invalid id")
		return
	}
	limit := parseBoundedInt(q.Get("limit"), s.cfg.defaultLimit, 1, s.cfg.maxLimit)
	offset := parseBoundedInt(q.Get("offset"), 0, 0, 1_000_000)
	filter := queryFilter{
		ID:         id,
		RecordKey:  firstNonEmpty(q.Get("record_key"), q.Get("recordKey"), q.Get("key")),
		EntityType: firstNonEmpty(q.Get("entity_type"), q.Get("entityType")),
		SerialNo:   firstNonEmpty(q.Get("serialNo"), q.Get("serial_no")),
		PlateNo:    firstNonEmpty(q.Get("plateNo"), q.Get("plate_no"), q.Get("plate")),
		From:       parseQueryTime(q.Get("from")),
		To:         parseQueryTime(q.Get("to")),
		Limit:      limit,
		Offset:     offset,
	}

	rows, err := s.queryRecords(r.Context(), filter)
	if err != nil {
		log.Printf("query records failed: %v", err)
		writeError(w, http.StatusInternalServerError, "sqlite query failed")
		return
	}

	out := make([]map[string]any, 0, len(rows))
	for _, row := range rows {
		item := map[string]any{
			"id":          row.ID,
			"record_key":  row.RecordKey,
			"entity_type": row.EntityType,
			"key_type":    row.KeyType,
			"ingested_at": row.IngestedAt,
		}
		putNullString(item, "serialNo", row.SerialNo)
		putNullString(item, "plateNo", row.PlateNo)
		putNullString(item, "source_time", row.SourceTime)
		putNullString(item, "source", row.Source)
		putNullString(item, "database", row.SourceDatabase)
		putNullString(item, "table", row.SourceTable)
		putNullString(item, "uploaded_at", row.UploadedAt)

		if row.RawRecord.Valid && shouldReturnRaw(r, s.cfg.returnRawRecords) {
			var raw any
			if err := json.Unmarshal([]byte(row.RawRecord.String), &raw); err == nil {
				item["record"] = raw
			} else {
				item["record"] = row.RawRecord.String
			}
		}
		out = append(out, item)
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"records": out,
		"count":   len(out),
		"limit":   limit,
		"offset":  offset,
	})
}

func (s *server) handleDelete(w http.ResponseWriter, r *http.Request) {
	if err := s.authorize(r, nil, s.cfg.deleteAuth); err != nil {
		writeError(w, http.StatusUnauthorized, err.Error())
		return
	}

	q := r.URL.Query()
	if id, ok := parseOptionalInt64(q.Get("id")); q.Get("id") != "" {
		if !ok {
			writeError(w, http.StatusBadRequest, "invalid id")
			return
		}
		s.deleteByID(w, r, id)
		return
	}
	key := firstNonEmpty(q.Get("record_key"), q.Get("recordKey"), q.Get("key"))
	if key != "" {
		s.deleteByKey(w, r, key)
		return
	}
	writeError(w, http.StatusBadRequest, "id or record_key is required")
}

func (s *server) handleDeleteByID(w http.ResponseWriter, r *http.Request) {
	if err := s.authorize(r, nil, s.cfg.deleteAuth); err != nil {
		writeError(w, http.StatusUnauthorized, err.Error())
		return
	}
	id, ok := parseOptionalInt64(r.PathValue("id"))
	if !ok || id <= 0 {
		writeError(w, http.StatusBadRequest, "invalid id")
		return
	}
	s.deleteByID(w, r, id)
}

func (s *server) handleDeleteByKey(w http.ResponseWriter, r *http.Request) {
	if err := s.authorize(r, nil, s.cfg.deleteAuth); err != nil {
		writeError(w, http.StatusUnauthorized, err.Error())
		return
	}
	key := strings.TrimSpace(r.PathValue("record_key"))
	if key == "" {
		writeError(w, http.StatusBadRequest, "record_key is required")
		return
	}
	s.deleteByKey(w, r, key)
}

func (s *server) deleteByID(w http.ResponseWriter, r *http.Request, id int64) {
	deleted, err := s.deleteRecord(r.Context(), "id = ?", id)
	if err != nil {
		log.Printf("delete record failed id=%d error=%v", id, err)
		writeError(w, http.StatusInternalServerError, "sqlite delete failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"deleted": deleted, "id": id})
}

func (s *server) deleteByKey(w http.ResponseWriter, r *http.Request, key string) {
	deleted, err := s.deleteRecord(r.Context(), "record_key = ?", key)
	if err != nil {
		log.Printf("delete record failed record_key=%s error=%v", key, err)
		writeError(w, http.StatusInternalServerError, "sqlite delete failed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"deleted": deleted, "record_key": key})
}

func (s *server) insertPayload(ctx context.Context, requestID string, payload uploadPayload, uploadedAt *time.Time) ([]string, []string, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, nil, err
	}
	defer tx.Rollback()

	source := fallback(payload.Source, "unknown")
	database := fallback(payload.Database, "unknown")
	table := fallback(payload.Table, "multiple")
	entities := payload.entityRecords()
	var rawPayload any
	if s.cfg.storeRawPayload {
		encoded, _ := json.Marshal(payload)
		rawPayload = string(encoded)
	}

	if _, err := tx.ExecContext(ctx, `
		INSERT INTO wds_receive_batches
			(request_id, source, source_database, source_table, records_count, accepted_count, failed_count, uploaded_at, raw_payload)
		VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)
		ON CONFLICT(request_id) DO UPDATE SET
			records_count = excluded.records_count,
			accepted_count = excluded.accepted_count,
			failed_count = excluded.failed_count,
			uploaded_at = excluded.uploaded_at,
			raw_payload = excluded.raw_payload
	`, requestID, source, database, table, entities.total(), entities.total(), nullableTimeString(uploadedAt), rawPayload); err != nil {
		return nil, nil, err
	}

	accepted := make([]string, 0, entities.total())
	failed := make([]string, 0)
	for _, batch := range entities {
		for start := 0; start < len(batch.records); start += insertChunkSize {
			end := start + insertChunkSize
			if end > len(batch.records) {
				end = len(batch.records)
			}
			chunkAccepted, chunkFailed, err := upsertRecords(ctx, tx, batch.entityType, batch.sourceTable, batch.records[start:end], source, database, uploadedAt, s.cfg.storeRawRecords)
			if err != nil {
				return nil, nil, err
			}
			accepted = append(accepted, chunkAccepted...)
			failed = append(failed, chunkFailed...)
		}
	}

	if _, err := tx.ExecContext(ctx, `
		UPDATE wds_receive_batches
		SET accepted_count = ?, failed_count = ?
		WHERE request_id = ?
	`, len(accepted), len(failed), requestID); err != nil {
		return nil, nil, err
	}

	if err := tx.Commit(); err != nil {
		return nil, nil, err
	}
	return accepted, failed, nil
}

func upsertRecords(ctx context.Context, tx *sql.Tx, entityType, table string, records []map[string]any, source, database string, uploadedAt *time.Time, storeRaw bool) ([]string, []string, error) {
	var (
		values   []string
		args     []any
		accepted []string
		failed   []string
	)

	for _, record := range records {
		keyType, rawKey := extractRecordKey(entityType, record)
		key := entityType + ":" + rawKey
		if rawKey == "" {
			failed = append(failed, "")
			continue
		}

		serialNo := extractString(record, "serialNo", "serial_no")
		plateNo := extractString(record, "plateNo", "plate_no", "plateNumber", "plate_number")
		sourceTime := firstTime(
			parseAnyTime(record["updateTime"]),
			parseAnyTime(record["secondTime"]),
			parseAnyTime(record["firstTime"]),
			parseAnyTime(record["tareTime"]),
			parseAnyTime(record["grossTime"]),
			parseAnyTime(record["measured_at"]),
			parseAnyTime(record["measuredAt"]),
		)
		var rawRecord any
		if storeRaw {
			encoded, err := json.Marshal(record)
			if err != nil {
				failed = append(failed, key)
				continue
			}
			rawRecord = string(encoded)
		}

		values = append(values, "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
		args = append(args,
			key,
			entityType,
			keyType,
			nullableString(serialNo),
			nullableString(plateNo),
			nullableTimeString(sourceTime),
			source,
			database,
			table,
			nullableTimeString(uploadedAt),
			rawRecord,
		)
		accepted = append(accepted, key)
	}

	if len(values) == 0 {
		return accepted, failed, nil
	}

	stmt := `
		INSERT INTO wds_receive_records
			(record_key, entity_type, key_type, serial_no, plate_no, source_time, source, source_database, source_table, uploaded_at, raw_record)
		VALUES ` + strings.Join(values, ",") + `
		ON CONFLICT(record_key) DO UPDATE SET
			entity_type = excluded.entity_type,
			key_type = excluded.key_type,
			serial_no = excluded.serial_no,
			plate_no = excluded.plate_no,
			source_time = excluded.source_time,
			source = excluded.source,
			source_database = excluded.source_database,
			source_table = excluded.source_table,
			uploaded_at = excluded.uploaded_at,
			raw_record = excluded.raw_record,
			cloud_updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
	`
	if _, err := tx.ExecContext(ctx, stmt, args...); err != nil {
		return nil, nil, err
	}
	return accepted, failed, nil
}

type queryFilter struct {
	ID         int64
	RecordKey  string
	EntityType string
	SerialNo   string
	PlateNo    string
	From       *time.Time
	To         *time.Time
	Limit      int
	Offset     int
}

func (s *server) queryRecords(ctx context.Context, filter queryFilter) ([]recordRow, error) {
	where := []string{"1 = 1"}
	args := []any{}
	if filter.ID > 0 {
		where = append(where, "id = ?")
		args = append(args, filter.ID)
	}
	if filter.RecordKey != "" {
		where = append(where, "record_key = ?")
		args = append(args, filter.RecordKey)
	}
	if filter.EntityType != "" {
		where = append(where, "entity_type = ?")
		args = append(args, filter.EntityType)
	}
	if filter.SerialNo != "" {
		where = append(where, "serial_no = ?")
		args = append(args, filter.SerialNo)
	}
	if filter.PlateNo != "" {
		where = append(where, "plate_no = ?")
		args = append(args, filter.PlateNo)
	}
	if filter.From != nil {
		where = append(where, "source_time >= ?")
		args = append(args, timeString(*filter.From))
	}
	if filter.To != nil {
		where = append(where, "source_time <= ?")
		args = append(args, timeString(*filter.To))
	}
	args = append(args, filter.Limit, filter.Offset)

	query := `
		SELECT id, record_key, entity_type, key_type, serial_no, plate_no, source_time, source,
		       source_database, source_table, uploaded_at, ingested_at, raw_record
		FROM wds_receive_records
		WHERE ` + strings.Join(where, " AND ") + `
		ORDER BY COALESCE(source_time, ingested_at) DESC, record_key DESC
		LIMIT ? OFFSET ?
	`

	sqlRows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer sqlRows.Close()

	rows := []recordRow{}
	for sqlRows.Next() {
		var row recordRow
		if err := sqlRows.Scan(
			&row.ID,
			&row.RecordKey,
			&row.EntityType,
			&row.KeyType,
			&row.SerialNo,
			&row.PlateNo,
			&row.SourceTime,
			&row.Source,
			&row.SourceDatabase,
			&row.SourceTable,
			&row.UploadedAt,
			&row.IngestedAt,
			&row.RawRecord,
		); err != nil {
			return nil, err
		}
		rows = append(rows, row)
	}
	return rows, sqlRows.Err()
}

func (s *server) deleteRecord(ctx context.Context, predicate string, arg any) (bool, error) {
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	result, err := s.db.ExecContext(ctx, "DELETE FROM wds_receive_records WHERE "+predicate, arg)
	if err != nil {
		return false, err
	}
	affected, err := result.RowsAffected()
	if err != nil {
		return false, err
	}
	return affected > 0, nil
}

func (s *server) authorize(r *http.Request, body []byte, auth authConfig) error {
	if auth.apiToken != "" {
		token, ok := extractBearer(r.Header.Get("Authorization"))
		if token == "" || token != auth.apiToken {
			return errors.New("invalid or missing bearer token")
		}
		if !ok {
			return errors.New("invalid or missing bearer token")
		}
	}
	if auth.signSecret != "" {
		return s.verifySignature(r, body, auth.signSecret)
	}
	return nil
}

func (s *server) verifySignature(r *http.Request, body []byte, secret string) error {
	timestamp := firstNonEmpty(r.Header.Get("X-Timestamp"), r.URL.Query().Get("timestamp"))
	nonce := firstNonEmpty(r.Header.Get("X-Nonce"), r.URL.Query().Get("nonce"))
	signature := firstNonEmpty(r.Header.Get("X-Signature"), r.URL.Query().Get("signature"), r.URL.Query().Get("sign"))
	if timestamp == "" || nonce == "" || signature == "" {
		return errors.New("missing signature parameters")
	}

	tsUnix, err := strconv.ParseInt(timestamp, 10, 64)
	if err != nil {
		return errors.New("invalid timestamp")
	}
	ts := time.Unix(tsUnix, 0)
	if time.Since(ts) > s.cfg.signSkew || time.Until(ts) > s.cfg.signSkew {
		return errors.New("signature timestamp expired")
	}

	expected := signRequest(secret, r.Method, r.URL.Path, canonicalQuery(r.URL.Query()), timestamp, nonce, body)
	provided, err := hex.DecodeString(strings.TrimSpace(signature))
	if err != nil {
		return errors.New("invalid signature format")
	}
	expectedBytes, _ := hex.DecodeString(expected)
	if !hmac.Equal(provided, expectedBytes) {
		return errors.New("invalid signature")
	}
	if !s.nonces.Add(nonce, ts.Add(s.cfg.signSkew)) {
		return errors.New("nonce already used")
	}
	return nil
}

func extractBearer(header string) (string, bool) {
	scheme, token, ok := strings.Cut(strings.TrimSpace(header), " ")
	if !ok || !strings.EqualFold(scheme, "Bearer") {
		return "", false
	}
	return strings.TrimSpace(token), true
}

func signRequest(secret, method, path, query, timestamp, nonce string, body []byte) string {
	bodyHash := sha256.Sum256(body)
	canonical := strings.Join([]string{
		strings.ToUpper(method),
		path,
		query,
		timestamp,
		nonce,
		hex.EncodeToString(bodyHash[:]),
	}, "\n")
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write([]byte(canonical))
	return hex.EncodeToString(mac.Sum(nil))
}

func canonicalQuery(values url.Values) string {
	clone := url.Values{}
	for key, vals := range values {
		if key == "signature" || key == "sign" {
			continue
		}
		copied := append([]string(nil), vals...)
		sort.Strings(copied)
		clone[key] = copied
	}
	return clone.Encode()
}

func (n *nonceStore) Add(nonce string, expiresAt time.Time) bool {
	n.mu.Lock()
	defer n.mu.Unlock()
	now := time.Now()
	for key, expiry := range n.items {
		if now.After(expiry) {
			delete(n.items, key)
		}
	}
	if _, ok := n.items[nonce]; ok {
		return false
	}
	n.items[nonce] = expiresAt
	return true
}

func readBody(r *http.Request, maxBytes int64) ([]byte, error) {
	defer r.Body.Close()
	body, err := io.ReadAll(io.LimitReader(r.Body, maxBytes+1))
	if err != nil {
		return nil, errors.New("failed to read body")
	}
	if int64(len(body)) > maxBytes {
		return nil, fmt.Errorf("body too large, max %d bytes", maxBytes)
	}
	return body, nil
}

func migrate(ctx context.Context, db *sql.DB) error {
	for _, stmt := range []string{createRecordsTableSQL, addRecordsEntityTypeColumnSQL, createRecordsEntityTypeIndexSQL, createRecordsPlateTimeIndexSQL, createRecordsSerialNoIndexSQL, createRecordsSourceTimeIndexSQL, createBatchesTableSQL, createBatchesSourceTimeIndexSQL} {
		if _, err := db.ExecContext(ctx, stmt); err != nil {
			if strings.Contains(err.Error(), "duplicate column name") {
				continue
			}
			return err
		}
	}
	return nil
}

const createRecordsTableSQL = `
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
)
`

const addRecordsEntityTypeColumnSQL = `
ALTER TABLE wds_receive_records
ADD COLUMN entity_type text NOT NULL DEFAULT 'weight_info'
`

const createRecordsEntityTypeIndexSQL = `
CREATE INDEX IF NOT EXISTS idx_wds_receive_entity_type
ON wds_receive_records (entity_type, ingested_at, record_key)
`

const createRecordsPlateTimeIndexSQL = `
CREATE INDEX IF NOT EXISTS idx_wds_receive_plate_time
ON wds_receive_records (plate_no, source_time, record_key)
`

const createRecordsSerialNoIndexSQL = `
CREATE INDEX IF NOT EXISTS idx_wds_receive_serial_no
ON wds_receive_records (serial_no)
`

const createRecordsSourceTimeIndexSQL = `
CREATE INDEX IF NOT EXISTS idx_wds_receive_source_time
ON wds_receive_records (source_database, source_table, source_time, record_key)
`

const createBatchesTableSQL = `
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
)
`

const createBatchesSourceTimeIndexSQL = `
CREATE INDEX IF NOT EXISTS idx_wds_receive_batch_source_time
ON wds_receive_batches (source_database, source_table, received_at)
`

func extractRecordKey(entityType string, record map[string]any) (string, string) {
	candidates := []struct {
		keyType string
		keys    []string
	}{}
	if entityType == "weight_photo" {
		candidates = []struct {
			keyType string
			keys    []string
		}{
			{"id", []string{"id"}},
			{"serialNo", []string{"serialNo", "serial_no"}},
		}
	} else {
		candidates = []struct {
			keyType string
			keys    []string
		}{
			{"serialNo", []string{"serialNo", "serial_no"}},
			{"id", []string{"id"}},
			{"ticket_no", []string{"ticket_no", "ticketNo"}},
		}
	}
	for _, candidate := range candidates {
		if value := extractString(record, candidate.keys...); value != "" {
			return candidate.keyType, value
		}
	}
	return "", ""
}

func extractString(record map[string]any, keys ...string) string {
	for _, key := range keys {
		value, ok := record[key]
		if !ok || value == nil {
			continue
		}
		switch v := value.(type) {
		case string:
			return strings.TrimSpace(v)
		case json.Number:
			return v.String()
		case float64:
			return strconv.FormatFloat(v, 'f', -1, 64)
		case bool:
			return strconv.FormatBool(v)
		}
	}
	return ""
}

func parseAnyTime(value any) *time.Time {
	if value == nil {
		return nil
	}
	text := ""
	switch v := value.(type) {
	case string:
		text = strings.TrimSpace(v)
	case json.Number:
		text = v.String()
	default:
		return nil
	}
	if text == "" {
		return nil
	}
	return parseTimeText(text)
}

func parseQueryTime(text string) *time.Time {
	text = strings.TrimSpace(text)
	if text == "" {
		return nil
	}
	return parseTimeText(text)
}

func parseTimeText(text string) *time.Time {
	layouts := []string{
		time.RFC3339Nano,
		time.RFC3339,
		"2006-01-02 15:04:05.999",
		"2006-01-02 15:04:05",
		"2006-01-02",
	}
	for _, layout := range layouts {
		if t, err := time.ParseInLocation(layout, text, time.Local); err == nil {
			return &t
		}
	}
	return nil
}

func firstTime(values ...*time.Time) *time.Time {
	for _, value := range values {
		if value != nil {
			return value
		}
	}
	return nil
}

func nullableString(value string) any {
	if value == "" {
		return nil
	}
	return value
}

func nullableTimeString(value *time.Time) any {
	if value == nil {
		return nil
	}
	return timeString(*value)
}

func timeString(value time.Time) string {
	return value.UTC().Format(time.RFC3339Nano)
}

func putNullString(m map[string]any, key string, value sql.NullString) {
	if value.Valid {
		m[key] = value.String
	}
}

func writeJSON(w http.ResponseWriter, status int, payload any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(payload); err != nil {
		log.Printf("write json failed: %v", err)
	}
}

func writeError(w http.ResponseWriter, status int, message string) {
	writeJSON(w, status, map[string]any{"error": message})
}

func requestLog(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		next.ServeHTTP(w, r)
		log.Printf("%s %s elapsed=%s", r.Method, r.URL.RequestURI(), time.Since(start))
	})
}

func newUUID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return strconv.FormatInt(time.Now().UnixNano(), 10)
	}
	b[6] = (b[6] & 0x0f) | 0x40
	b[8] = (b[8] & 0x3f) | 0x80
	return fmt.Sprintf("%08x-%04x-%04x-%04x-%012x",
		b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}

func envString(key, fallbackValue string) string {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		return value
	}
	return fallbackValue
}

func envFirst(keys ...string) string {
	for _, key := range keys {
		if value := strings.TrimSpace(os.Getenv(key)); value != "" {
			return value
		}
	}
	return ""
}

func envInt(key string, fallbackValue int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallbackValue
	}
	parsed, err := strconv.Atoi(value)
	if err != nil {
		return fallbackValue
	}
	return parsed
}

func envBool(key string, fallbackValue bool) bool {
	value := strings.ToLower(strings.TrimSpace(os.Getenv(key)))
	if value == "" {
		return fallbackValue
	}
	return value == "1" || value == "true" || value == "yes" || value == "on"
}

func parseOptionalInt64(text string) (int64, bool) {
	text = strings.TrimSpace(text)
	if text == "" {
		return 0, true
	}
	value, err := strconv.ParseInt(text, 10, 64)
	if err != nil || value <= 0 {
		return 0, false
	}
	return value, true
}

func parseBoundedInt(text string, fallbackValue, minValue, maxValue int) int {
	if text == "" {
		return fallbackValue
	}
	value, err := strconv.Atoi(text)
	if err != nil {
		return fallbackValue
	}
	if value < minValue {
		return minValue
	}
	if value > maxValue {
		return maxValue
	}
	return value
}

func shouldReturnRaw(r *http.Request, fallbackValue bool) bool {
	value := strings.ToLower(strings.TrimSpace(r.URL.Query().Get("include_raw")))
	if value == "" {
		return fallbackValue
	}
	return value == "1" || value == "true" || value == "yes" || value == "on"
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}

func fallback(value, fallbackValue string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return fallbackValue
	}
	return value
}
