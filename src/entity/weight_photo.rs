use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tbl_weightPhoto")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(column_name = "serialNo")]
    pub serial_no: Option<String>,
    #[sea_orm(column_name = "plateNumber")]
    pub plate_number: Option<String>,
    #[sea_orm(column_name = "imageType")]
    pub image_type: Option<String>,
    #[sea_orm(column_name = "captureTime")]
    pub capture_time: Option<NaiveDateTime>,
    #[sea_orm(column_name = "captureImage")]
    pub capture_image: Option<Vec<u8>>,
    #[sea_orm(column_name = "clientId")]
    pub client_id: Option<String>,
    #[sea_orm(column_name = "consigneeUnit")]
    pub consignee_unit: Option<String>,
    #[sea_orm(column_name = "forwardingUnit")]
    pub forwarding_unit: Option<String>,
    #[sea_orm(column_name = "delFlag")]
    pub del_flag: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
