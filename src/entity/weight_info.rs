use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tbl_weightInfo")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_name = "serialNo")]
    pub serial_no: String,
    #[sea_orm(column_name = "sysNo")]
    pub sys_no: Option<String>,
    #[sea_orm(column_name = "setId")]
    pub set_id: Option<String>,
    #[sea_orm(column_name = "cardNo")]
    pub card_no: Option<String>,
    #[sea_orm(column_name = "plateNo")]
    pub plate_no: Option<String>,
    #[sea_orm(column_name = "weightType")]
    pub weight_type: Option<String>,
    #[sea_orm(column_name = "transportUnit")]
    pub transport_unit: Option<String>,
    #[sea_orm(column_name = "forwardingUnit")]
    pub forwarding_unit: Option<String>,
    #[sea_orm(column_name = "consigneeUnit")]
    pub consignee_unit: Option<String>,
    #[sea_orm(column_name = "goodsName")]
    pub goods_name: Option<String>,
    #[sea_orm(column_name = "goodsSpec")]
    pub goods_spec: Option<String>,
    #[sea_orm(column_name = "grossWeight")]
    pub gross_weight: Option<Decimal>,
    #[sea_orm(column_name = "tareWeight")]
    pub tare_weight: Option<Decimal>,
    #[sea_orm(column_name = "netWeight")]
    pub net_weight: Option<Decimal>,
    #[sea_orm(column_name = "buckleWeight")]
    pub buckle_weight: Option<Decimal>,
    #[sea_orm(column_name = "actualWeight")]
    pub actual_weight: Option<Decimal>,
    #[sea_orm(column_name = "weightUnit")]
    pub weight_unit: Option<String>,
    #[sea_orm(column_name = "unitPrice")]
    pub unit_price: Option<Decimal>,
    #[sea_orm(column_name = "sumAmt")]
    pub sum_amt: Option<Decimal>,
    #[sea_orm(column_name = "scaleNum")]
    pub scale_num: Option<String>,
    #[sea_orm(column_name = "squareNum")]
    pub square_num: Option<String>,
    #[sea_orm(column_name = "weighingFee")]
    pub weighing_fee: Option<Decimal>,
    #[sea_orm(column_name = "grossStation")]
    pub gross_station: Option<String>,
    #[sea_orm(column_name = "tareStation")]
    pub tare_station: Option<String>,
    #[sea_orm(column_name = "grossMan")]
    pub gross_man: Option<String>,
    #[sea_orm(column_name = "tareMan")]
    pub tare_man: Option<String>,
    #[sea_orm(column_name = "grossTime")]
    pub gross_time: Option<NaiveDateTime>,
    #[sea_orm(column_name = "tareTime")]
    pub tare_time: Option<NaiveDateTime>,
    #[sea_orm(column_name = "firstTime")]
    pub first_time: Option<NaiveDateTime>,
    #[sea_orm(column_name = "secondTime")]
    pub second_time: Option<NaiveDateTime>,
    #[sea_orm(column_name = "updateTime")]
    pub update_time: Option<NaiveDateTime>,
    #[sea_orm(column_name = "printNum")]
    pub print_num: Option<i32>,
    #[sea_orm(column_name = "isCancle")]
    pub is_cancle: Option<i32>,
    #[sea_orm(column_name = "isUploadLocal")]
    pub is_upload_local: Option<i32>,
    #[sea_orm(column_name = "isUploadCloud")]
    pub is_upload_cloud: Option<i32>,
    #[sea_orm(column_name = "strBackup1")]
    pub str_backup1: Option<String>,
    #[sea_orm(column_name = "strBackup2")]
    pub str_backup2: Option<String>,
    #[sea_orm(column_name = "strBackup3")]
    pub str_backup3: Option<String>,
    #[sea_orm(column_name = "strBackup4")]
    pub str_backup4: Option<String>,
    #[sea_orm(column_name = "strBackup5")]
    pub str_backup5: Option<String>,
    #[sea_orm(column_name = "strBackup6")]
    pub str_backup6: Option<String>,
    #[sea_orm(column_name = "strBackup7")]
    pub str_backup7: Option<String>,
    #[sea_orm(column_name = "strBackup8")]
    pub str_backup8: Option<String>,
    #[sea_orm(column_name = "strBackup9")]
    pub str_backup9: Option<String>,
    #[sea_orm(column_name = "numBackup1")]
    pub num_backup1: Option<Decimal>,
    #[sea_orm(column_name = "numBackup2")]
    pub num_backup2: Option<Decimal>,
    #[sea_orm(column_name = "numBackup3")]
    pub num_backup3: Option<Decimal>,
    #[sea_orm(column_name = "numBackup4")]
    pub num_backup4: Option<Decimal>,
    #[sea_orm(column_name = "numBackup5")]
    pub num_backup5: Option<Decimal>,
    #[sea_orm(column_name = "numBackup6")]
    pub num_backup6: Option<Decimal>,
    #[sea_orm(column_name = "numBackup7")]
    pub num_backup7: Option<Decimal>,
    #[sea_orm(column_name = "numBackup8")]
    pub num_backup8: Option<Decimal>,
    #[sea_orm(column_name = "numBackup9")]
    pub num_backup9: Option<Decimal>,
    #[sea_orm(column_name = "timeBackup1")]
    pub time_backup1: Option<NaiveDateTime>,
    #[sea_orm(column_name = "timeBackup2")]
    pub time_backup2: Option<NaiveDateTime>,
    #[sea_orm(column_name = "timeBackup3")]
    pub time_backup3: Option<NaiveDateTime>,
    #[sea_orm(column_name = "fGuid")]
    pub f_guid: Option<Uuid>,
    #[sea_orm(column_name = "fID")]
    pub f_id: Option<i64>,
    #[sea_orm(column_name = "relNo")]
    pub rel_no: Option<String>,
    #[sea_orm(column_name = "relSer")]
    pub rel_ser: Option<String>,
    #[sea_orm(column_name = "regNo")]
    pub reg_no: Option<String>,
    #[sea_orm(column_name = "dataType")]
    pub data_type: Option<String>,
    #[sea_orm(column_name = "dataLog")]
    pub data_log: Option<String>,
    #[sea_orm(column_name = "isFinish")]
    pub is_finish: Option<i32>,
    pub remark: Option<String>,
    #[sea_orm(column_name = "del_flag")]
    pub del_flag: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
