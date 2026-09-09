// b-replicator copies records from the C receiver into a local MySQL database.
package main

import (
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
	"sort"
	"strconv"
	"strings"
	"syscall"
	"time"

	_ "github.com/go-sql-driver/mysql"
)

const (
	defaultQueryRoute   = "/weighing-data-sync/records"
	defaultCleanupRoute = "/weighing-data-sync/records"
)

type config struct {
	mysqlDSN       string
	cBaseURL       *url.URL
	queryRoute     string
	cleanupRoute   string
	queryAuth      authConfig
	cleanupAuth    authConfig
	batchSize      int
	fetchInterval  time.Duration
	deleteInterval time.Duration
	httpTimeout    time.Duration
	dbTimeout      time.Duration
	autoMigrate    bool
}
type authConfig struct{ token, secret string }
type app struct {
	cfg    config
	db     *sql.DB
	client *http.Client
}
type queryResponse struct {
	Records []cRecord `json:"records"`
}
type cRecord struct {
	ID         int64           `json:"id"`
	RecordKey  string          `json:"record_key"`
	EntityType string          `json:"entity_type"`
	KeyType    string          `json:"key_type"`
	SerialNo   string          `json:"serialNo"`
	PlateNo    string          `json:"plateNo"`
	SourceTime string          `json:"source_time"`
	Source     string          `json:"source"`
	Database   string          `json:"database"`
	Table      string          `json:"table"`
	UploadedAt string          `json:"uploaded_at"`
	IngestedAt string          `json:"ingested_at"`
	Record     json.RawMessage `json:"record"`
}
type deleteJob struct {
	ID, CRecordID int64
	RecordKey     string
	Retries       int
}

func main() {
	cfg, err := loadConfig()
	if err != nil {
		log.Fatalf("config error: %v", err)
	}
	db, err := sql.Open("mysql", cfg.mysqlDSN)
	if err != nil {
		log.Fatalf("open mysql: %v", err)
	}
	defer db.Close()
	db.SetMaxOpenConns(envInt("MYSQL_MAX_OPEN_CONNS", 10))
	db.SetMaxIdleConns(envInt("MYSQL_MAX_IDLE_CONNS", 5))
	db.SetConnMaxLifetime(time.Duration(envInt("MYSQL_CONN_MAX_LIFETIME_SECONDS", 300)) * time.Second)
	ctx, cancel := context.WithTimeout(context.Background(), cfg.dbTimeout)
	if err := db.PingContext(ctx); err != nil {
		cancel()
		log.Fatalf("ping mysql: %v", err)
	}
	if cfg.autoMigrate {
		if err := migrate(ctx, db); err != nil {
			cancel()
			log.Fatalf("migrate mysql: %v", err)
		}
	}
	cancel()
	a := &app{cfg: cfg, db: db, client: &http.Client{Timeout: cfg.httpTimeout}}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go a.fetchLoop(ctx)
	go a.deleteLoop(ctx)
	log.Printf("b replicator started: c=%s query=%s mysql connected", cfg.cBaseURL, cfg.queryRoute)
	<-ctx.Done()
	log.Printf("b replicator stopped")
}

func loadConfig() (config, error) {
	baseText := strings.TrimRight(strings.TrimSpace(os.Getenv("C_BASE_URL")), "/")
	if baseText == "" {
		return config{}, errors.New("C_BASE_URL is required")
	}
	base, err := url.Parse(baseText)
	if err != nil || base.Scheme == "" || base.Host == "" {
		return config{}, errors.New("C_BASE_URL must be an absolute HTTP URL")
	}
	cfg := config{mysqlDSN: strings.TrimSpace(os.Getenv("MYSQL_DSN")), cBaseURL: base, queryRoute: envString("C_QUERY_ROUTE", defaultQueryRoute), cleanupRoute: envString("C_CLEANUP_ROUTE", defaultCleanupRoute), queryAuth: authConfig{envString("QUERY_API_TOKEN", ""), envString("QUERY_SIGN_SECRET", "")}, cleanupAuth: authConfig{envString("CLEANUP_API_TOKEN", ""), envString("CLEANUP_SIGN_SECRET", "")}, batchSize: envInt("FETCH_BATCH_SIZE", 100), fetchInterval: envDurationSeconds("FETCH_INTERVAL_SECONDS", 5), deleteInterval: envDurationSeconds("DELETE_INTERVAL_SECONDS", 2), httpTimeout: envDurationSeconds("HTTP_TIMEOUT_SECONDS", 30), dbTimeout: envDurationSeconds("DB_TIMEOUT_SECONDS", 30), autoMigrate: envBool("AUTO_MIGRATE", true)}
	if cfg.mysqlDSN == "" {
		return cfg, errors.New("MYSQL_DSN is required")
	}
	if err := validateRoute("C_QUERY_ROUTE", cfg.queryRoute); err != nil {
		return cfg, err
	}
	if err := validateRoute("C_CLEANUP_ROUTE", cfg.cleanupRoute); err != nil {
		return cfg, err
	}
	if cfg.queryAuth.token == "" || cfg.queryAuth.secret == "" {
		return cfg, errors.New("QUERY_API_TOKEN and QUERY_SIGN_SECRET are required")
	}
	if cfg.cleanupAuth.token == "" || cfg.cleanupAuth.secret == "" {
		return cfg, errors.New("CLEANUP_API_TOKEN and CLEANUP_SIGN_SECRET are required")
	}
	if cfg.batchSize < 1 || cfg.fetchInterval <= 0 || cfg.deleteInterval <= 0 || cfg.httpTimeout <= 0 || cfg.dbTimeout <= 0 {
		return cfg, errors.New("batch size and interval/timeout settings must be positive")
	}
	return cfg, nil
}

func (a *app) fetchLoop(ctx context.Context) {
	for {
		if err := a.fetchAndStore(ctx); err != nil && !errors.Is(err, context.Canceled) {
			log.Printf("fetch/store failed: %v", err)
		}
		if !waitContext(ctx, a.cfg.fetchInterval) {
			return
		}
	}
}
func (a *app) deleteLoop(ctx context.Context) {
	for {
		if err := a.deletePending(ctx); err != nil && !errors.Is(err, context.Canceled) {
			log.Printf("delete worker failed: %v", err)
		}
		if !waitContext(ctx, a.cfg.deleteInterval) {
			return
		}
	}
}

func (a *app) fetchAndStore(ctx context.Context) error {
	endpoint := a.endpoint(a.cfg.queryRoute)
	q := endpoint.Query()
	q.Set("include_raw", "true")
	q.Set("limit", strconv.Itoa(a.cfg.batchSize))
	endpoint.RawQuery = q.Encode()
	var response queryResponse
	if err := a.doJSON(ctx, http.MethodGet, endpoint, a.cfg.queryAuth, &response); err != nil {
		return err
	}
	if len(response.Records) == 0 {
		return nil
	}
	for _, record := range response.Records {
		if record.ID <= 0 || strings.TrimSpace(record.RecordKey) == "" {
			return fmt.Errorf("C returned invalid identity id=%d record_key=%q", record.ID, record.RecordKey)
		}
		if len(record.Record) == 0 || string(record.Record) == "null" || !json.Valid(record.Record) {
			return fmt.Errorf("C record %q has no raw record; set STORE_RAW_RECORDS=true on C", record.RecordKey)
		}
		if err := a.storeRecord(ctx, record); err != nil {
			return fmt.Errorf("store C record %q: %w", record.RecordKey, err)
		}
	}
	log.Printf("stored %d record(s) and queued cleanup", len(response.Records))
	return nil
}

func (a *app) storeRecord(parent context.Context, r cRecord) error {
	ctx, cancel := context.WithTimeout(parent, a.cfg.dbTimeout)
	defer cancel()
	tx, err := a.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	entityType := r.EntityType
	if entityType == "" {
		entityType = "weight_info"
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO wds_replicated_records (record_key,c_record_id,entity_type,key_type,serial_no,plate_no,source_time,source,source_database,source_table,uploaded_at,ingested_at,raw_record) VALUES (?, ?, ?, ?, NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), ?) ON DUPLICATE KEY UPDATE c_record_id=VALUES(c_record_id),entity_type=VALUES(entity_type),key_type=VALUES(key_type),serial_no=VALUES(serial_no),plate_no=VALUES(plate_no),source_time=VALUES(source_time),source=VALUES(source),source_database=VALUES(source_database),source_table=VALUES(source_table),uploaded_at=VALUES(uploaded_at),ingested_at=VALUES(ingested_at),raw_record=VALUES(raw_record),replicated_at=UTC_TIMESTAMP(6)`, r.RecordKey, r.ID, entityType, r.KeyType, r.SerialNo, r.PlateNo, r.SourceTime, r.Source, r.Database, r.Table, r.UploadedAt, r.IngestedAt, string(r.Record))
	if err != nil {
		return err
	}
	if err := upsertBusinessRecord(ctx, tx, entityType, r); err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO wds_c_delete_queue (c_record_id,record_key,status,retry_count,next_attempt_at,last_error) VALUES (?, ?, 'pending', 0, UTC_TIMESTAMP(6), NULL) ON DUPLICATE KEY UPDATE c_record_id=VALUES(c_record_id),status='pending',retry_count=0,next_attempt_at=UTC_TIMESTAMP(6),last_error=NULL,updated_at=UTC_TIMESTAMP(6)`, r.ID, r.RecordKey)
	if err != nil {
		return err
	}
	return tx.Commit()
}

func (a *app) deletePending(ctx context.Context) error {
	jobs, err := a.loadDeleteJobs(ctx)
	if err != nil {
		return err
	}
	for _, job := range jobs {
		endpoint := a.endpoint(a.cfg.cleanupRoute + "/" + strconv.FormatInt(job.CRecordID, 10))
		if err := a.doJSON(ctx, http.MethodDelete, endpoint, a.cfg.cleanupAuth, nil); err != nil {
			if markErr := a.markDeleteFailure(ctx, job, err); markErr != nil {
				return fmt.Errorf("delete %q: %w; save retry: %v", job.RecordKey, err, markErr)
			}
			log.Printf("cleanup retry queued record_key=%s error=%v", job.RecordKey, err)
			continue
		}
		if err := a.markDeleteDone(ctx, job.ID); err != nil {
			return err
		}
		log.Printf("cleaned C record id=%d record_key=%s", job.CRecordID, job.RecordKey)
	}
	return nil
}
func (a *app) loadDeleteJobs(parent context.Context) ([]deleteJob, error) {
	ctx, cancel := context.WithTimeout(parent, a.cfg.dbTimeout)
	defer cancel()
	rows, err := a.db.QueryContext(ctx, `SELECT id,c_record_id,record_key,retry_count FROM wds_c_delete_queue WHERE status IN ('pending','failed') AND next_attempt_at <= UTC_TIMESTAMP(6) ORDER BY id LIMIT ?`, a.cfg.batchSize)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var jobs []deleteJob
	for rows.Next() {
		var job deleteJob
		if err := rows.Scan(&job.ID, &job.CRecordID, &job.RecordKey, &job.Retries); err != nil {
			return nil, err
		}
		jobs = append(jobs, job)
	}
	return jobs, rows.Err()
}
func (a *app) markDeleteDone(parent context.Context, id int64) error {
	ctx, cancel := context.WithTimeout(parent, a.cfg.dbTimeout)
	defer cancel()
	_, err := a.db.ExecContext(ctx, `UPDATE wds_c_delete_queue SET status='done',last_error=NULL,completed_at=UTC_TIMESTAMP(6),updated_at=UTC_TIMESTAMP(6) WHERE id=?`, id)
	return err
}
func (a *app) markDeleteFailure(parent context.Context, job deleteJob, cause error) error {
	ctx, cancel := context.WithTimeout(parent, a.cfg.dbTimeout)
	defer cancel()
	retries := job.Retries + 1
	delay := retryDelay(retries)
	_, err := a.db.ExecContext(ctx, `UPDATE wds_c_delete_queue SET status='failed',retry_count=?,next_attempt_at=DATE_ADD(UTC_TIMESTAMP(6), INTERVAL ? SECOND),last_error=?,updated_at=UTC_TIMESTAMP(6) WHERE id=?`, retries, int(delay.Seconds()), truncate(cause.Error(), 4000), job.ID)
	return err
}

func (a *app) endpoint(route string) *url.URL {
	u := *a.cfg.cBaseURL
	u.Path = strings.TrimRight(a.cfg.cBaseURL.Path, "/") + route
	u.RawQuery = ""
	return &u
}
func (a *app) doJSON(ctx context.Context, method string, endpoint *url.URL, auth authConfig, destination any) error {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	nonce, err := newNonce()
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, method, endpoint.String(), nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+auth.token)
	req.Header.Set("X-Timestamp", timestamp)
	req.Header.Set("X-Nonce", nonce)
	req.Header.Set("X-Signature", signRequest(auth.secret, method, endpoint.EscapedPath(), canonicalQuery(endpoint.Query()), timestamp, nonce, nil))
	resp, err := a.client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	responseBody, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("C %s %s returned %s: %s", method, endpoint.Path, resp.Status, truncate(strings.TrimSpace(string(responseBody)), 1000))
	}
	if destination != nil && len(responseBody) > 0 {
		if err := json.Unmarshal(responseBody, destination); err != nil {
			return fmt.Errorf("decode C response: %w", err)
		}
	}
	return nil
}
func signRequest(secret, method, path, query, timestamp, nonce string, body []byte) string {
	hash := sha256.Sum256(body)
	canonical := strings.Join([]string{strings.ToUpper(method), path, query, timestamp, nonce, hex.EncodeToString(hash[:])}, "\n")
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
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
func newNonce() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(b[:]), nil
}
func waitContext(ctx context.Context, d time.Duration) bool {
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-timer.C:
		return true
	}
}
func retryDelay(retries int) time.Duration {
	if retries > 8 {
		retries = 8
	}
	return time.Duration(1<<uint(retries-1)) * time.Second
}
func truncate(s string, max int) string {
	if len(s) <= max {
		return s
	}
	return s[:max]
}
func validateRoute(name, route string) error {
	if !strings.HasPrefix(route, "/") || strings.Contains(route, "?") {
		return fmt.Errorf("%s must be an absolute path without query", name)
	}
	return nil
}
func envString(key, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		return value
	}
	return fallback
}
func envInt(key string, fallback int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed < 0 {
		return fallback
	}
	return parsed
}
func envDurationSeconds(key string, fallback int) time.Duration {
	return time.Duration(envInt(key, fallback)) * time.Second
}
func envBool(key string, fallback bool) bool {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.ParseBool(value)
	if err != nil {
		return fallback
	}
	return parsed
}

type fieldSpec struct {
	JSONName string
	Column   string
}

var weightInfoFields = []fieldSpec{
	{"serialNo", "serial_no"},
	{"sysNo", "sys_no"},
	{"setId", "set_id"},
	{"cardNo", "card_no"},
	{"plateNo", "plate_no"},
	{"weightType", "weight_type"},
	{"transportUnit", "transport_unit"},
	{"forwardingUnit", "forwarding_unit"},
	{"consigneeUnit", "consignee_unit"},
	{"goodsName", "goods_name"},
	{"goodsSpec", "goods_spec"},
	{"grossWeight", "gross_weight"},
	{"tareWeight", "tare_weight"},
	{"netWeight", "net_weight"},
	{"buckleWeight", "buckle_weight"},
	{"actualWeight", "actual_weight"},
	{"weightUnit", "weight_unit"},
	{"unitPrice", "unit_price"},
	{"sumAmt", "sum_amt"},
	{"scaleNum", "scale_num"},
	{"squareNum", "square_num"},
	{"weighingFee", "weighing_fee"},
	{"grossStation", "gross_station"},
	{"tareStation", "tare_station"},
	{"grossMan", "gross_man"},
	{"tareMan", "tare_man"},
	{"grossTime", "gross_time"},
	{"tareTime", "tare_time"},
	{"firstTime", "first_time"},
	{"secondTime", "second_time"},
	{"updateTime", "update_time"},
	{"printNum", "print_num"},
	{"isCancle", "is_cancle"},
	{"isUploadLocal", "is_upload_local"},
	{"isUploadCloud", "is_upload_cloud"},
	{"strBackup1", "str_backup1"},
	{"strBackup2", "str_backup2"},
	{"strBackup3", "str_backup3"},
	{"strBackup4", "str_backup4"},
	{"strBackup5", "str_backup5"},
	{"strBackup6", "str_backup6"},
	{"strBackup7", "str_backup7"},
	{"strBackup8", "str_backup8"},
	{"strBackup9", "str_backup9"},
	{"numBackup1", "num_backup1"},
	{"numBackup2", "num_backup2"},
	{"numBackup3", "num_backup3"},
	{"numBackup4", "num_backup4"},
	{"numBackup5", "num_backup5"},
	{"numBackup6", "num_backup6"},
	{"numBackup7", "num_backup7"},
	{"numBackup8", "num_backup8"},
	{"numBackup9", "num_backup9"},
	{"timeBackup1", "time_backup1"},
	{"timeBackup2", "time_backup2"},
	{"timeBackup3", "time_backup3"},
	{"fGuid", "f_guid"},
	{"fID", "f_id"},
	{"relNo", "rel_no"},
	{"relSer", "rel_ser"},
	{"regNo", "reg_no"},
	{"dataType", "data_type"},
	{"dataLog", "data_log"},
	{"isFinish", "is_finish"},
	{"remark", "remark"},
	{"del_flag", "del_flag"},
}

var weightPhotoFields = []fieldSpec{
	{"id", "source_id"},
	{"serialNo", "serial_no"},
	{"captureImage", "capture_image_base64"},
	{"isUploadLocal", "is_upload_local"},
	{"isUploadCloud", "is_upload_cloud"},
	{"plateNumber", "plate_number"},
	{"imageType", "image_type"},
	{"delFlag", "del_flag"},
	{"clientId", "client_id"},
	{"consigneeUnit", "consignee_unit"},
	{"forwardingUnit", "forwarding_unit"},
}

func upsertBusinessRecord(ctx context.Context, tx *sql.Tx, entityType string, r cRecord) error {
	var fields []fieldSpec
	table := ""
	switch entityType {
	case "weight_photo":
		table = "wds_weight_photo_records"
		fields = weightPhotoFields
	default:
		table = "wds_weight_info_records"
		fields = weightInfoFields
	}

	values := map[string]any{}
	if err := json.Unmarshal(r.Record, &values); err != nil {
		return fmt.Errorf("decode business record: %w", err)
	}

	columns := []string{"record_key", "c_record_id"}
	placeholders := []string{"?", "?"}
	args := []any{r.RecordKey, r.ID}
	updates := []string{"c_record_id=VALUES(c_record_id)"}
	for _, field := range fields {
		columns = append(columns, field.Column)
		placeholders = append(placeholders, "?")
		args = append(args, normalizeJSONValue(values[field.JSONName]))
		updates = append(updates, field.Column+"=VALUES("+field.Column+")")
	}
	columns = append(columns, "raw_record")
	placeholders = append(placeholders, "?")
	args = append(args, string(r.Record))
	updates = append(updates, "raw_record=VALUES(raw_record)", "replicated_at=UTC_TIMESTAMP(6)")

	query := fmt.Sprintf("INSERT INTO %s (%s) VALUES (%s) ON DUPLICATE KEY UPDATE %s", table, strings.Join(columns, ","), strings.Join(placeholders, ","), strings.Join(updates, ","))
	_, err := tx.ExecContext(ctx, query, args...)
	return err
}

func normalizeJSONValue(value any) any {
	switch v := value.(type) {
	case nil:
		return nil
	case string:
		if strings.TrimSpace(v) == "" {
			return nil
		}
		return v
	case float64:
		return strconv.FormatFloat(v, 'f', -1, 64)
	case bool:
		if v {
			return 1
		}
		return 0
	default:
		return fmt.Sprint(v)
	}
}

func migrate(ctx context.Context, db *sql.DB) error {
	for _, statement := range []string{createRecordsTable, addEntityTypeToRecordsTable, createWeightInfoTable, createWeightPhotoTable, createDeleteQueueTable} {
		if _, err := db.ExecContext(ctx, statement); err != nil {
			if strings.Contains(err.Error(), "Duplicate column name") {
				continue
			}
			return err
		}
	}
	return nil
}

const createRecordsTable = `CREATE TABLE IF NOT EXISTS wds_replicated_records (id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,record_key VARCHAR(191) NOT NULL,c_record_id BIGINT NOT NULL,entity_type VARCHAR(32) NOT NULL DEFAULT 'weight_info',key_type VARCHAR(64) NOT NULL,serial_no VARCHAR(191) NULL,plate_no VARCHAR(191) NULL,source_time VARCHAR(64) NULL,source VARCHAR(191) NULL,source_database VARCHAR(191) NULL,source_table VARCHAR(191) NULL,uploaded_at VARCHAR(64) NULL,ingested_at VARCHAR(64) NULL,raw_record LONGTEXT NOT NULL,replicated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),PRIMARY KEY (id),UNIQUE KEY uk_wds_replicated_record_key (record_key),KEY idx_wds_replicated_entity_type (entity_type,replicated_at),KEY idx_wds_replicated_serial_no (serial_no),KEY idx_wds_replicated_plate_time (plate_no,source_time)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci`
const addEntityTypeToRecordsTable = `ALTER TABLE wds_replicated_records ADD COLUMN entity_type VARCHAR(32) NOT NULL DEFAULT 'weight_info' AFTER c_record_id`
const createWeightInfoTable = `CREATE TABLE IF NOT EXISTS wds_weight_info_records (
id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
record_key VARCHAR(191) NOT NULL,
c_record_id BIGINT NOT NULL,
serial_no VARCHAR(191) NULL,
sys_no VARCHAR(191) NULL,
set_id VARCHAR(64) NULL,
card_no VARCHAR(191) NULL,
plate_no VARCHAR(191) NULL,
weight_type VARCHAR(191) NULL,
transport_unit VARCHAR(255) NULL,
forwarding_unit VARCHAR(255) NULL,
consignee_unit VARCHAR(255) NULL,
goods_name VARCHAR(255) NULL,
goods_spec VARCHAR(255) NULL,
gross_weight VARCHAR(64) NULL,
tare_weight VARCHAR(64) NULL,
net_weight VARCHAR(64) NULL,
buckle_weight VARCHAR(64) NULL,
actual_weight VARCHAR(64) NULL,
weight_unit VARCHAR(64) NULL,
unit_price VARCHAR(64) NULL,
sum_amt VARCHAR(64) NULL,
scale_num VARCHAR(64) NULL,
square_num VARCHAR(64) NULL,
weighing_fee VARCHAR(64) NULL,
gross_station VARCHAR(191) NULL,
tare_station VARCHAR(191) NULL,
gross_man VARCHAR(191) NULL,
tare_man VARCHAR(191) NULL,
gross_time VARCHAR(64) NULL,
tare_time VARCHAR(64) NULL,
first_time VARCHAR(64) NULL,
second_time VARCHAR(64) NULL,
update_time VARCHAR(64) NULL,
print_num VARCHAR(64) NULL,
is_cancle VARCHAR(64) NULL,
is_upload_local VARCHAR(64) NULL,
is_upload_cloud VARCHAR(64) NULL,
str_backup1 TEXT NULL,
str_backup2 TEXT NULL,
str_backup3 TEXT NULL,
str_backup4 TEXT NULL,
str_backup5 TEXT NULL,
str_backup6 TEXT NULL,
str_backup7 TEXT NULL,
str_backup8 TEXT NULL,
str_backup9 TEXT NULL,
num_backup1 VARCHAR(64) NULL,
num_backup2 VARCHAR(64) NULL,
num_backup3 VARCHAR(64) NULL,
num_backup4 VARCHAR(64) NULL,
num_backup5 VARCHAR(64) NULL,
num_backup6 VARCHAR(64) NULL,
num_backup7 VARCHAR(64) NULL,
num_backup8 VARCHAR(64) NULL,
num_backup9 VARCHAR(64) NULL,
time_backup1 VARCHAR(64) NULL,
time_backup2 VARCHAR(64) NULL,
time_backup3 VARCHAR(64) NULL,
f_guid VARCHAR(191) NULL,
f_id VARCHAR(191) NULL,
rel_no VARCHAR(191) NULL,
rel_ser VARCHAR(64) NULL,
reg_no VARCHAR(191) NULL,
data_type VARCHAR(191) NULL,
data_log TEXT NULL,
is_finish VARCHAR(64) NULL,
remark TEXT NULL,
del_flag VARCHAR(64) NULL,
raw_record LONGTEXT NOT NULL,
replicated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
PRIMARY KEY (id),
UNIQUE KEY uk_wds_weight_info_record_key (record_key),
KEY idx_wds_weight_info_serial_no (serial_no),
KEY idx_wds_weight_info_plate_no (plate_no),
KEY idx_wds_weight_info_times (update_time,second_time,first_time)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci`
const createWeightPhotoTable = `CREATE TABLE IF NOT EXISTS wds_weight_photo_records (
id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
record_key VARCHAR(191) NOT NULL,
c_record_id BIGINT NOT NULL,
source_id VARCHAR(64) NULL,
serial_no VARCHAR(191) NULL,
capture_image_base64 LONGTEXT NULL,
is_upload_local VARCHAR(64) NULL,
is_upload_cloud VARCHAR(64) NULL,
plate_number VARCHAR(191) NULL,
image_type VARCHAR(191) NULL,
del_flag VARCHAR(64) NULL,
client_id VARCHAR(191) NULL,
consignee_unit VARCHAR(255) NULL,
forwarding_unit VARCHAR(255) NULL,
raw_record LONGTEXT NOT NULL,
replicated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
PRIMARY KEY (id),
UNIQUE KEY uk_wds_weight_photo_record_key (record_key),
KEY idx_wds_weight_photo_serial_no (serial_no),
KEY idx_wds_weight_photo_plate_type (plate_number,image_type)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci`
const createDeleteQueueTable = `CREATE TABLE IF NOT EXISTS wds_c_delete_queue (id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,c_record_id BIGINT NOT NULL,record_key VARCHAR(191) NOT NULL,status VARCHAR(16) NOT NULL DEFAULT 'pending',retry_count INT NOT NULL DEFAULT 0,next_attempt_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),last_error TEXT NULL,created_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),updated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),completed_at DATETIME(6) NULL,PRIMARY KEY (id),UNIQUE KEY uk_wds_c_delete_record_key (record_key),KEY idx_wds_c_delete_pending (status,next_attempt_at,id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci`
