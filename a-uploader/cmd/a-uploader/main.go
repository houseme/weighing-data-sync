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

	_ "github.com/denisenkom/go-mssqldb"
)

const defaultPendingWhere = "ISNULL([isUploadCloud], 0) = 0 AND ISNULL([del_flag], 0) = 0"

type config struct {
	endpoint                                                                                               *url.URL
	token, secret, dsn, database, schema, table, primaryKey, serialColumn, pendingWhere, stateFile, source string
	batchSize                                                                                              int
	poll, timeout                                                                                          time.Duration
	runOnce                                                                                                bool
}
type uploadPayload struct {
	Source     string           `json:"source"`
	Database   string           `json:"database"`
	Table      string           `json:"table"`
	UploadedAt string           `json:"uploaded_at"`
	Records    []map[string]any `json:"records"`
}
type uploadResponse struct {
	Accepted []string `json:"accepted_serial_nos"`
	Failed   []string `json:"failed_serial_nos"`
}
type sourceRecord struct {
	pk, serial string
	data       map[string]any
}
type stateEntry struct {
	PrimaryKey string `json:"primary_key"`
	SerialNo   string `json:"serial_no,omitempty"`
	AcceptedAt string `json:"accepted_at"`
}
type stateStore struct {
	path string
	mu   sync.Mutex
	done map[string]struct{}
}
type uploader struct {
	cfg    config
	db     *sql.DB
	client *http.Client
	state  *stateStore
}

func main() {
	cfg, err := loadConfig()
	if err != nil {
		log.Fatal(err)
	}
	state, err := loadState(cfg.stateFile)
	if err != nil {
		log.Fatalf("load state: %v", err)
	}
	db, err := sql.Open("sqlserver", cfg.dsn)
	if err != nil {
		log.Fatalf("open SQL Server: %v", err)
	}
	defer db.Close()
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	ctx, cancel := context.WithTimeout(context.Background(), cfg.timeout)
	err = db.PingContext(ctx)
	cancel()
	if err != nil {
		log.Fatalf("ping SQL Server: %v", err)
	}
	u := &uploader{cfg: cfg, db: db, client: &http.Client{Timeout: cfg.timeout}, state: state}
	if cfg.runOnce {
		if err := u.syncOnce(context.Background()); err != nil {
			log.Fatal(err)
		}
		return
	}
	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)
	ticker := time.NewTicker(cfg.poll)
	defer ticker.Stop()
	for {
		if err := u.syncOnce(context.Background()); err != nil {
			log.Printf("sync failed: %v", err)
		}
		select {
		case <-stop:
			return
		case <-ticker.C:
		}
	}
}

func loadConfig() (config, error) {
	endpoint, err := url.Parse(env("C_ENDPOINT", "http://127.0.0.1:18081/weighing-data-sync/put"))
	if err != nil || endpoint.Scheme == "" || endpoint.Host == "" {
		return config{}, errors.New("C_ENDPOINT must be an absolute URL")
	}
	token, secret, database := os.Getenv("INGEST_API_TOKEN"), os.Getenv("INGEST_SIGN_SECRET"), env("SQLSERVER_DATABASE", "")
	if strings.TrimSpace(token) == "" || strings.TrimSpace(secret) == "" {
		return config{}, errors.New("INGEST_API_TOKEN and INGEST_SIGN_SECRET are required")
	}
	if database == "" {
		return config{}, errors.New("SQLSERVER_DATABASE is required")
	}
	table := env("SQLSERVER_TABLE", "tbl_weightInfo")
	cfg := config{endpoint: endpoint, token: token, secret: secret, dsn: buildDSN(), database: database, schema: env("SQLSERVER_SCHEMA", "dbo"), table: table, primaryKey: env("SQLSERVER_PRIMARY_KEY", "serialNo"), serialColumn: env("SQLSERVER_SERIAL_COLUMN", "serialNo"), pendingWhere: env("SQLSERVER_PENDING_WHERE", defaultPendingWhere), stateFile: env("STATE_FILE", "data/a-uploader-state.jsonl"), source: env("SOURCE_NAME", "sqlserver-"+database+"-"+table), batchSize: envInt("BATCH_SIZE", 100), poll: seconds("POLL_INTERVAL_SECONDS", 30), timeout: seconds("HTTP_TIMEOUT_SECONDS", 30), runOnce: envBool("RUN_ONCE", false)}
	for _, name := range []string{cfg.schema, cfg.table, cfg.primaryKey, cfg.serialColumn} {
		if !identifier(name) {
			return config{}, fmt.Errorf("invalid SQL Server identifier: %q", name)
		}
	}
	if cfg.batchSize < 1 || cfg.poll < time.Second || cfg.timeout < time.Second {
		return config{}, errors.New("batch size and timeout settings must be positive")
	}
	if strings.TrimSpace(cfg.pendingWhere) == "" || strings.Contains(cfg.pendingWhere, ";") {
		return config{}, errors.New("SQLSERVER_PENDING_WHERE must be a non-empty single SQL expression")
	}
	return cfg, nil
}

func buildDSN() string {
	if dsn := strings.TrimSpace(os.Getenv("SQLSERVER_DSN")); dsn != "" {
		return dsn
	}
	values := url.Values{"database": {env("SQLSERVER_DATABASE", "")}, "encrypt": {env("SQLSERVER_ENCRYPT", "disable")}}
	if envBool("SQLSERVER_TRUST_SERVER_CERTIFICATE", true) {
		values.Set("TrustServerCertificate", "true")
	}
	u := url.URL{
		Scheme:   "sqlserver",
		User:     url.UserPassword(env("SQLSERVER_USERNAME", ""), os.Getenv("SQLSERVER_PASSWORD")),
		Host:     env("SQLSERVER_HOST", "127.0.0.1") + ":" + strconv.Itoa(envInt("SQLSERVER_PORT", 1433)),
		RawQuery: values.Encode(),
	}
	return u.String()
}

func (u *uploader) syncOnce(parent context.Context) error {
	ctx, cancel := context.WithTimeout(parent, u.cfg.timeout)
	defer cancel()
	records, err := u.fetch(ctx)
	if err != nil {
		return err
	}
	pending := make([]sourceRecord, 0, len(records))
	for _, record := range records {
		if !u.state.contains(record.pk) {
			pending = append(pending, record)
		}
	}
	if len(pending) == 0 {
		log.Printf("no unconfirmed SQL Server records")
		return nil
	}
	bodyRecords := make([]map[string]any, 0, len(pending))
	for _, record := range pending {
		bodyRecords = append(bodyRecords, record.data)
	}
	response, err := u.post(ctx, bodyRecords)
	if err != nil {
		return err
	}
	accepted := acknowledged(pending, response)
	if len(accepted) == 0 {
		return fmt.Errorf("C accepted no records from %d submitted", len(pending))
	}
	entries := make([]stateEntry, 0, len(accepted))
	now := time.Now().UTC().Format(time.RFC3339Nano)
	for _, record := range accepted {
		entries = append(entries, stateEntry{PrimaryKey: record.pk, SerialNo: record.serial, AcceptedAt: now})
	}
	if err := u.state.append(entries); err != nil {
		return fmt.Errorf("C accepted %d records but state append failed: %w", len(entries), err)
	}
	log.Printf("submitted=%d accepted=%d failed=%d", len(pending), len(entries), len(response.Failed))
	return nil
}

func (u *uploader) fetch(ctx context.Context) ([]sourceRecord, error) {
	query := fmt.Sprintf("SELECT TOP (@p1) * FROM %s WHERE %s ORDER BY %s ASC", qualified(u.cfg.schema, u.cfg.table), u.cfg.pendingWhere, quote(u.cfg.primaryKey))
	rows, err := u.db.QueryContext(ctx, query, sql.Named("p1", u.cfg.batchSize))
	if err != nil {
		return nil, fmt.Errorf("query SQL Server: %w", err)
	}
	defer rows.Close()
	columns, err := rows.Columns()
	if err != nil {
		return nil, err
	}
	types, err := rows.ColumnTypes()
	if err != nil {
		return nil, err
	}
	result := make([]sourceRecord, 0)
	for rows.Next() {
		values, dest := make([]any, len(columns)), make([]any, len(columns))
		for i := range values {
			dest[i] = &values[i]
		}
		if err := rows.Scan(dest...); err != nil {
			return nil, err
		}
		data := make(map[string]any, len(columns))
		for i, column := range columns {
			data[column] = toJSON(values[i], types[i].DatabaseTypeName())
		}
		pk, serial := stringValue(data[u.cfg.primaryKey]), stringValue(data[u.cfg.serialColumn])
		if pk == "" || serial == "" {
			return nil, fmt.Errorf("row has an empty %s or %s", u.cfg.primaryKey, u.cfg.serialColumn)
		}
		result = append(result, sourceRecord{pk: pk, serial: serial, data: data})
	}
	return result, rows.Err()
}

func (u *uploader) post(ctx context.Context, records []map[string]any) (uploadResponse, error) {
	body, err := json.Marshal(uploadPayload{Source: u.cfg.source, Database: u.cfg.database, Table: u.cfg.table, UploadedAt: time.Now().UTC().Format(time.RFC3339Nano), Records: records})
	if err != nil {
		return uploadResponse{}, err
	}
	nonce, err := nonce()
	if err != nil {
		return uploadResponse{}, err
	}
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, u.cfg.endpoint.String(), bytes.NewReader(body))
	if err != nil {
		return uploadResponse{}, err
	}
	req.Header.Set("Authorization", "Bearer "+u.cfg.token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Timestamp", timestamp)
	req.Header.Set("X-Nonce", nonce)
	req.Header.Set("X-Signature", sign(u.cfg.secret, req.Method, req.URL.Path, canonicalQuery(req.URL.Query()), timestamp, nonce, body))
	resp, err := u.client.Do(req)
	if err != nil {
		return uploadResponse{}, fmt.Errorf("send C request: %w", err)
	}
	defer resp.Body.Close()
	responseBody, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return uploadResponse{}, err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return uploadResponse{}, fmt.Errorf("C returned %s: %s", resp.Status, strings.TrimSpace(string(responseBody)))
	}
	if resp.StatusCode == http.StatusNoContent || len(bytes.TrimSpace(responseBody)) == 0 {
		return uploadResponse{}, nil
	}
	var result uploadResponse
	if err := json.Unmarshal(responseBody, &result); err != nil {
		return uploadResponse{}, fmt.Errorf("decode C response: %w", err)
	}
	return result, nil
}

func acknowledged(records []sourceRecord, response uploadResponse) []sourceRecord {
	if len(response.Accepted) == 0 && len(response.Failed) == 0 {
		return records
	}
	set := map[string]struct{}{}
	for _, serial := range response.Accepted {
		set[serial] = struct{}{}
	}
	result := make([]sourceRecord, 0, len(set))
	for _, record := range records {
		if _, ok := set[record.serial]; ok {
			result = append(result, record)
		}
	}
	return result
}

func loadState(path string) (*stateStore, error) {
	store := &stateStore{path: path, done: map[string]struct{}{}}
	file, err := os.Open(path)
	if errors.Is(err, os.ErrNotExist) {
		return store, nil
	}
	if err != nil {
		return nil, err
	}
	defer file.Close()
	decoder := json.NewDecoder(file)
	for {
		var entry stateEntry
		err := decoder.Decode(&entry)
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("invalid state file %s: %w", path, err)
		}
		if entry.PrimaryKey == "" {
			return nil, fmt.Errorf("state file %s contains empty primary_key", path)
		}
		store.done[entry.PrimaryKey] = struct{}{}
	}
	return store, nil
}
func (s *stateStore) contains(key string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, ok := s.done[key]
	return ok
}
func (s *stateStore) append(entries []stateEntry) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	dir := filepath.Dir(s.path)
	if dir != "." {
		if err := os.MkdirAll(dir, 0o750); err != nil {
			return err
		}
	}
	file, err := os.OpenFile(s.path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	defer file.Close()
	for _, entry := range entries {
		if _, ok := s.done[entry.PrimaryKey]; ok {
			continue
		}
		encoded, err := json.Marshal(entry)
		if err != nil {
			return err
		}
		if _, err := file.Write(append(encoded, '\n')); err != nil {
			return err
		}
		s.done[entry.PrimaryKey] = struct{}{}
	}
	return file.Sync()
}

func sign(secret, method, path, query, timestamp, nonce string, body []byte) string {
	hash := sha256.Sum256(body)
	canonical := strings.Join([]string{strings.ToUpper(method), path, query, timestamp, nonce, hex.EncodeToString(hash[:])}, "\n")
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
	return hex.EncodeToString(mac.Sum(nil))
}
func canonicalQuery(values url.Values) string {
	copied := url.Values{}
	for key, values := range values {
		if key == "signature" || key == "sign" {
			continue
		}
		copied[key] = append([]string(nil), values...)
		sort.Strings(copied[key])
	}
	return copied.Encode()
}
func nonce() (string, error) {
	var value [16]byte
	if _, err := rand.Read(value[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(value[:]), nil
}
func toJSON(value any, databaseType string) any {
	switch value := value.(type) {
	case nil:
		return nil
	case time.Time:
		return value.Format("2006-01-02 15:04:05")
	case []byte:
		if strings.Contains(strings.ToUpper(databaseType), "BINARY") || strings.Contains(strings.ToUpper(databaseType), "IMAGE") {
			return fmt.Sprintf("<%d bytes>", len(value))
		}
		return string(value)
	case float32, float64:
		return stringValue(value)
	default:
		return value
	}
}
func stringValue(value any) string {
	switch value := value.(type) {
	case nil:
		return ""
	case string:
		return strings.TrimSpace(value)
	case []byte:
		return strings.TrimSpace(string(value))
	case bool:
		return strconv.FormatBool(value)
	case int, int8, int16, int32, int64, uint, uint8, uint16, uint32, uint64, float32, float64:
		return fmt.Sprint(value)
	default:
		return ""
	}
}
func qualified(schema, table string) string { return quote(schema) + "." + quote(table) }
func quote(value string) string             { return "[" + strings.ReplaceAll(value, "]", "]]") + "]" }
func identifier(value string) bool {
	return value != "" && strings.IndexFunc(value, func(r rune) bool {
		return !(r == '_' || r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9')
	}) == -1
}
func env(key, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		return value
	}
	return fallback
}
func envInt(key string, fallback int) int {
	value, err := strconv.Atoi(env(key, ""))
	if err != nil {
		return fallback
	}
	return value
}
func envBool(key string, fallback bool) bool {
	value, err := strconv.ParseBool(env(key, ""))
	if err != nil {
		return fallback
	}
	return value
}
func seconds(key string, fallback int) time.Duration {
	return time.Duration(envInt(key, fallback)) * time.Second
}
