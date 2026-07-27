//! 称重数据同步结构体定义草案。
//!
//! 本文件用于对齐上报协议、MySQL 落库字段和 Rust 业务模型。生产代码仍以
//! `src/entity/` 和 `src/source/sqlserver.rs` 为准；这里保存完整同步 DTO，便于
//! 云端服务、数据库脚本和接口文档共同引用。

use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeighingDataSyncPutRequest {
    pub source: String,
    pub database: String,
    pub table: String,
    pub uploaded_at: DateTime<Utc>,
    #[serde(default, alias = "weight_info_records")]
    pub records: Vec<WeightInfoSyncRecord>,
    #[serde(default)]
    pub weight_photo_records: Vec<WeightPhotoSyncRecord>,
    #[serde(default)]
    pub local_cache_records: Vec<LocalWeighingRecordSyncRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeighingDataSyncPutResponse {
    pub request_id: Uuid,
    pub accepted: bool,
    #[serde(default)]
    pub accepted_serial_nos: Vec<String>,
    #[serde(default)]
    pub failed_serial_nos: Vec<String>,
    pub records_count: usize,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightInfoSyncRecord {
    #[serde(rename = "serialNo")]
    pub serial_no: String,
    #[serde(rename = "sysNo")]
    pub sys_no: Option<String>,
    #[serde(rename = "setId")]
    pub set_id: Option<String>,
    #[serde(rename = "cardNo")]
    pub card_no: Option<String>,
    #[serde(rename = "plateNo")]
    pub plate_no: Option<String>,
    #[serde(rename = "weightType")]
    pub weight_type: Option<String>,
    #[serde(rename = "transportUnit")]
    pub transport_unit: Option<String>,
    #[serde(rename = "forwardingUnit")]
    pub forwarding_unit: Option<String>,
    #[serde(rename = "consigneeUnit")]
    pub consignee_unit: Option<String>,
    #[serde(rename = "goodsName")]
    pub goods_name: Option<String>,
    #[serde(rename = "goodsSpec")]
    pub goods_spec: Option<String>,
    #[serde(rename = "grossWeight")]
    pub gross_weight: Option<Decimal>,
    #[serde(rename = "tareWeight")]
    pub tare_weight: Option<Decimal>,
    #[serde(rename = "netWeight")]
    pub net_weight: Option<Decimal>,
    #[serde(rename = "buckleWeight")]
    pub buckle_weight: Option<Decimal>,
    #[serde(rename = "actualWeight")]
    pub actual_weight: Option<Decimal>,
    #[serde(rename = "weightUnit")]
    pub weight_unit: Option<String>,
    #[serde(rename = "unitPrice")]
    pub unit_price: Option<Decimal>,
    #[serde(rename = "sumAmt")]
    pub sum_amt: Option<Decimal>,
    #[serde(rename = "scaleNum")]
    pub scale_num: Option<String>,
    #[serde(rename = "squareNum")]
    pub square_num: Option<String>,
    #[serde(rename = "weighingFee")]
    pub weighing_fee: Option<Decimal>,
    #[serde(rename = "grossStation")]
    pub gross_station: Option<String>,
    #[serde(rename = "tareStation")]
    pub tare_station: Option<String>,
    #[serde(rename = "grossMan")]
    pub gross_man: Option<String>,
    #[serde(rename = "tareMan")]
    pub tare_man: Option<String>,
    #[serde(rename = "grossTime")]
    pub gross_time: Option<NaiveDateTime>,
    #[serde(rename = "tareTime")]
    pub tare_time: Option<NaiveDateTime>,
    #[serde(rename = "firstTime")]
    pub first_time: Option<NaiveDateTime>,
    #[serde(rename = "secondTime")]
    pub second_time: Option<NaiveDateTime>,
    #[serde(rename = "updateTime")]
    pub update_time: Option<NaiveDateTime>,
    #[serde(rename = "printNum")]
    pub print_num: Option<i32>,
    #[serde(rename = "isCancle")]
    pub is_cancle: Option<i32>,
    #[serde(rename = "isUploadLocal")]
    pub is_upload_local: Option<i32>,
    #[serde(rename = "isUploadCloud")]
    pub is_upload_cloud: Option<i32>,
    #[serde(rename = "strBackup1")]
    pub str_backup1: Option<String>,
    #[serde(rename = "strBackup2")]
    pub str_backup2: Option<String>,
    #[serde(rename = "strBackup3")]
    pub str_backup3: Option<String>,
    #[serde(rename = "strBackup4")]
    pub str_backup4: Option<String>,
    #[serde(rename = "strBackup5")]
    pub str_backup5: Option<String>,
    #[serde(rename = "strBackup6")]
    pub str_backup6: Option<String>,
    #[serde(rename = "strBackup7")]
    pub str_backup7: Option<String>,
    #[serde(rename = "strBackup8")]
    pub str_backup8: Option<String>,
    #[serde(rename = "strBackup9")]
    pub str_backup9: Option<String>,
    #[serde(rename = "numBackup1")]
    pub num_backup1: Option<Decimal>,
    #[serde(rename = "numBackup2")]
    pub num_backup2: Option<Decimal>,
    #[serde(rename = "numBackup3")]
    pub num_backup3: Option<Decimal>,
    #[serde(rename = "numBackup4")]
    pub num_backup4: Option<Decimal>,
    #[serde(rename = "numBackup5")]
    pub num_backup5: Option<Decimal>,
    #[serde(rename = "numBackup6")]
    pub num_backup6: Option<Decimal>,
    #[serde(rename = "numBackup7")]
    pub num_backup7: Option<Decimal>,
    #[serde(rename = "numBackup8")]
    pub num_backup8: Option<Decimal>,
    #[serde(rename = "numBackup9")]
    pub num_backup9: Option<Decimal>,
    #[serde(rename = "timeBackup1")]
    pub time_backup1: Option<NaiveDateTime>,
    #[serde(rename = "timeBackup2")]
    pub time_backup2: Option<NaiveDateTime>,
    #[serde(rename = "timeBackup3")]
    pub time_backup3: Option<NaiveDateTime>,
    #[serde(rename = "fGuid")]
    pub f_guid: Option<Uuid>,
    #[serde(rename = "fID")]
    pub f_id: Option<i64>,
    #[serde(rename = "relNo")]
    pub rel_no: Option<String>,
    #[serde(rename = "relSer")]
    pub rel_ser: Option<String>,
    #[serde(rename = "regNo")]
    pub reg_no: Option<String>,
    #[serde(rename = "dataType")]
    pub data_type: Option<String>,
    #[serde(rename = "dataLog")]
    pub data_log: Option<String>,
    #[serde(rename = "isFinish")]
    pub is_finish: Option<i32>,
    pub remark: Option<String>,
    #[serde(rename = "del_flag")]
    pub del_flag: Option<i32>,
}

impl WeightInfoSyncRecord {
    pub const MYSQL_COLUMNS: &[&str] = &[
        "serial_no",
        "sys_no",
        "set_id",
        "card_no",
        "plate_no",
        "weight_type",
        "transport_unit",
        "forwarding_unit",
        "consignee_unit",
        "goods_name",
        "goods_spec",
        "gross_weight",
        "tare_weight",
        "net_weight",
        "buckle_weight",
        "actual_weight",
        "weight_unit",
        "unit_price",
        "sum_amt",
        "scale_num",
        "square_num",
        "weighing_fee",
        "gross_station",
        "tare_station",
        "gross_man",
        "tare_man",
        "gross_time",
        "tare_time",
        "first_time",
        "second_time",
        "update_time",
        "print_num",
        "is_cancle",
        "is_upload_local",
        "is_upload_cloud",
        "str_backup1",
        "str_backup2",
        "str_backup3",
        "str_backup4",
        "str_backup5",
        "str_backup6",
        "str_backup7",
        "str_backup8",
        "str_backup9",
        "num_backup1",
        "num_backup2",
        "num_backup3",
        "num_backup4",
        "num_backup5",
        "num_backup6",
        "num_backup7",
        "num_backup8",
        "num_backup9",
        "time_backup1",
        "time_backup2",
        "time_backup3",
        "f_guid",
        "f_id",
        "rel_no",
        "rel_ser",
        "reg_no",
        "data_type",
        "data_log",
        "is_finish",
        "remark",
        "del_flag",
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightPhotoSyncRecord {
    pub id: i64,
    #[serde(rename = "serialNo")]
    pub serial_no: Option<String>,
    #[serde(rename = "plateNumber")]
    pub plate_number: Option<String>,
    #[serde(rename = "imageType")]
    pub image_type: Option<String>,
    #[serde(rename = "captureTime")]
    pub capture_time: Option<NaiveDateTime>,
    #[serde(rename = "captureImage")]
    pub capture_image: Option<Vec<u8>>,
    #[serde(rename = "clientId")]
    pub client_id: Option<String>,
    #[serde(rename = "consigneeUnit")]
    pub consignee_unit: Option<String>,
    #[serde(rename = "forwardingUnit")]
    pub forwarding_unit: Option<String>,
    #[serde(rename = "delFlag")]
    pub del_flag: Option<i32>,
}

impl WeightPhotoSyncRecord {
    pub const MYSQL_COLUMNS: &[&str] = &[
        "id",
        "serial_no",
        "plate_number",
        "image_type",
        "capture_time",
        "capture_image",
        "client_id",
        "consignee_unit",
        "forwarding_unit",
        "del_flag",
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalWeighingRecordSyncRecord {
    pub id: Uuid,
    pub ticket_no: String,
    pub scale_no: String,
    pub plate_no: Option<String>,
    pub weight_kg: f64,
    pub original_unit: String,
    pub measured_at: DateTime<Utc>,
    pub status: String,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
