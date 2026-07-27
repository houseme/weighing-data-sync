# 同步结构体字段清单

本文档记录所有已建结构体字段的同步处理范围。当前同步协议以 `serialNo` / `id` 做幂等键，批量上报到 `/weighing-data-sync/put`，主称重记录字段沿用现有接口的 `records` 数组，云端建议使用 `docs/mysql_sync_schema.sql` 中的 upsert 模板落库。

## 同步范围

| 结构体 | 来源表 | 目标 MySQL 表 | 幂等键 | 同步策略 |
| --- | --- | --- | --- | --- |
| `WeightInfoSyncRecord` | `yunfu.dbo.tbl_weightInfo` | `wds_weight_info` | `serial_no` | 全字段 upsert，成功后本地可回写 `isUploadCloud = 1` |
| `WeightPhotoSyncRecord` | `yunfu.dbo.tbl_weightPhoto` | `wds_weight_photo` | `id` | 全字段 upsert，按 `serial_no` 关联称重单 |
| `LocalWeighingRecordSyncRecord` | 本地 SQLite `weighing_records` | `wds_local_weighing_record` | `id` | 全字段 upsert，用于离线缓存镜像和补偿 |

## `WeightInfoSyncRecord` 字段

| JSON 字段 | Rust 字段 | MySQL 字段 | 类型建议 | 处理 |
| --- | --- | --- | --- | --- |
| `serialNo` | `serial_no` | `serial_no` | `varchar(64)` | 必填，主键 |
| `sysNo` | `sys_no` | `sys_no` | `varchar(64)` | 同步 |
| `setId` | `set_id` | `set_id` | `varchar(64)` | 同步 |
| `cardNo` | `card_no` | `card_no` | `varchar(64)` | 同步 |
| `plateNo` | `plate_no` | `plate_no` | `varchar(32)` | 同步，建立车牌时间索引 |
| `weightType` | `weight_type` | `weight_type` | `varchar(64)` | 同步 |
| `transportUnit` | `transport_unit` | `transport_unit` | `varchar(255)` | 同步 |
| `forwardingUnit` | `forwarding_unit` | `forwarding_unit` | `varchar(255)` | 同步 |
| `consigneeUnit` | `consignee_unit` | `consignee_unit` | `varchar(255)` | 同步 |
| `goodsName` | `goods_name` | `goods_name` | `varchar(255)` | 同步，建立物料时间索引 |
| `goodsSpec` | `goods_spec` | `goods_spec` | `varchar(255)` | 同步 |
| `grossWeight` | `gross_weight` | `gross_weight` | `decimal(18,3)` | 同步 |
| `tareWeight` | `tare_weight` | `tare_weight` | `decimal(18,3)` | 同步 |
| `netWeight` | `net_weight` | `net_weight` | `decimal(18,3)` | 同步 |
| `buckleWeight` | `buckle_weight` | `buckle_weight` | `decimal(18,3)` | 同步 |
| `actualWeight` | `actual_weight` | `actual_weight` | `decimal(18,3)` | 同步 |
| `weightUnit` | `weight_unit` | `weight_unit` | `varchar(16)` | 同步 |
| `unitPrice` | `unit_price` | `unit_price` | `decimal(18,4)` | 同步 |
| `sumAmt` | `sum_amt` | `sum_amt` | `decimal(18,2)` | 同步 |
| `scaleNum` | `scale_num` | `scale_num` | `varchar(64)` | 同步 |
| `squareNum` | `square_num` | `square_num` | `varchar(64)` | 同步 |
| `weighingFee` | `weighing_fee` | `weighing_fee` | `decimal(18,2)` | 同步 |
| `grossStation` | `gross_station` | `gross_station` | `varchar(128)` | 同步 |
| `tareStation` | `tare_station` | `tare_station` | `varchar(128)` | 同步 |
| `grossMan` | `gross_man` | `gross_man` | `varchar(64)` | 同步 |
| `tareMan` | `tare_man` | `tare_man` | `varchar(64)` | 同步 |
| `grossTime` | `gross_time` | `gross_time` | `datetime(3)` | 同步 |
| `tareTime` | `tare_time` | `tare_time` | `datetime(3)` | 同步 |
| `firstTime` | `first_time` | `first_time` | `datetime(3)` | 同步 |
| `secondTime` | `second_time` | `second_time` | `datetime(3)` | 同步 |
| `updateTime` | `update_time` | `update_time` | `datetime(3)` | 同步，参与待同步扫描索引 |
| `printNum` | `print_num` | `print_num` | `int` | 同步 |
| `isCancle` | `is_cancle` | `is_cancle` | `tinyint` | 同步，保留原始拼写 |
| `isUploadLocal` | `is_upload_local` | `is_upload_local` | `tinyint` | 同步 |
| `isUploadCloud` | `is_upload_cloud` | `is_upload_cloud` | `tinyint` | 同步，参与待同步扫描索引 |
| `strBackup1` 到 `strBackup9` | `str_backup1` 到 `str_backup9` | `str_backup1` 到 `str_backup9` | `varchar(512)` | 全部同步 |
| `numBackup1` 到 `numBackup9` | `num_backup1` 到 `num_backup9` | `num_backup1` 到 `num_backup9` | `decimal(18,4)` | 全部同步 |
| `timeBackup1` 到 `timeBackup3` | `time_backup1` 到 `time_backup3` | `time_backup1` 到 `time_backup3` | `datetime(3)` | 全部同步 |
| `fGuid` | `f_guid` | `f_guid` | `char(36)` | 同步 |
| `fID` | `f_id` | `f_id` | `bigint` | 同步 |
| `relNo` | `rel_no` | `rel_no` | `varchar(64)` | 同步 |
| `relSer` | `rel_ser` | `rel_ser` | `varchar(64)` | 同步 |
| `regNo` | `reg_no` | `reg_no` | `varchar(64)` | 同步 |
| `dataType` | `data_type` | `data_type` | `varchar(64)` | 同步 |
| `dataLog` | `data_log` | `data_log` | `text` | 同步 |
| `isFinish` | `is_finish` | `is_finish` | `tinyint` | 同步 |
| `remark` | `remark` | `remark` | `varchar(1024)` | 同步 |
| `del_flag` | `del_flag` | `del_flag` | `tinyint` | 同步，参与待同步扫描索引 |

## `WeightPhotoSyncRecord` 字段

| JSON 字段 | Rust 字段 | MySQL 字段 | 类型建议 | 处理 |
| --- | --- | --- | --- | --- |
| `id` | `id` | `id` | `bigint` | 必填，主键 |
| `serialNo` | `serial_no` | `serial_no` | `varchar(64)` | 同步，关联 `wds_weight_info.serial_no` |
| `plateNumber` | `plate_number` | `plate_number` | `varchar(32)` | 同步 |
| `imageType` | `image_type` | `image_type` | `varchar(64)` | 同步 |
| `captureTime` | `capture_time` | `capture_time` | `datetime(3)` | 同步 |
| `captureImage` | `capture_image` | `capture_image` | `longblob` | 同步，建议与主表分批写入 |
| `clientId` | `client_id` | `client_id` | `varchar(64)` | 同步 |
| `consigneeUnit` | `consignee_unit` | `consignee_unit` | `varchar(255)` | 同步 |
| `forwardingUnit` | `forwarding_unit` | `forwarding_unit` | `varchar(255)` | 同步 |
| `delFlag` | `del_flag` | `del_flag` | `tinyint` | 同步 |

## `LocalWeighingRecordSyncRecord` 字段

| JSON 字段 | Rust 字段 | MySQL 字段 | 类型建议 | 处理 |
| --- | --- | --- | --- | --- |
| `id` | `id` | `id` | `char(36)` | 必填，主键 |
| `ticket_no` | `ticket_no` | `ticket_no` | `varchar(64)` | 同步 |
| `scale_no` | `scale_no` | `scale_no` | `varchar(64)` | 同步 |
| `plate_no` | `plate_no` | `plate_no` | `varchar(32)` | 同步 |
| `weight_kg` | `weight_kg` | `weight_kg` | `decimal(18,3)` | 同步 |
| `original_unit` | `original_unit` | `original_unit` | `varchar(16)` | 同步 |
| `measured_at` | `measured_at` | `measured_at` | `datetime(3)` | 同步 |
| `status` | `status` | `status` | `enum` | 同步 |
| `retry_count` | `retry_count` | `retry_count` | `int` | 同步 |
| `last_error` | `last_error` | `last_error` | `varchar(1024)` | 同步 |
| `created_at` | `created_at` | `created_at` | `datetime(3)` | 同步 |
| `updated_at` | `updated_at` | `updated_at` | `datetime(3)` | 同步 |

## 同步处理规则

- `tbl_weightInfo` 当前代码已经按完整字段列表读取并上报 JSON；字段列表应与 `WeightInfoSyncRecord` 保持一致。
- `tbl_weightPhoto` 已有 Entity 和 MySQL 落库脚本；图片二进制体较大，建议生产同步中拆分为独立批次，避免影响主称重记录吞吐。
- 所有上报接口应支持幂等重放：同一个 `serialNo` 或 `id` 重复到达时只更新字段，不重复插入。
- 云端落库成功后返回 `accepted_serial_nos`，本地 SQL Server 同步器可据此回写 `isUploadCloud = 1`。
