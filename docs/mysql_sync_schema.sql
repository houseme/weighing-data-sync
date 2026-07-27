-- 称重数据同步系统 MySQL 高性能落库脚本
-- 目标版本：MySQL 8.0+
-- 设计要点：
-- 1. InnoDB + utf8mb4，兼顾中文车牌、单位、物料名称。
-- 2. 使用主键 / 唯一键 + INSERT ... ON DUPLICATE KEY UPDATE 做幂等批量同步。
-- 3. 对待同步扫描、车牌时间查询、图片关联查询建立覆盖索引。
-- 4. Decimal 字段保留精度，图片字段使用 LONGBLOB，完整结构体字段全部落库。

CREATE DATABASE IF NOT EXISTS `weighing_data_sync`
  DEFAULT CHARACTER SET utf8mb4
  DEFAULT COLLATE utf8mb4_0900_ai_ci;

USE `weighing_data_sync`;

CREATE TABLE IF NOT EXISTS `wds_weight_info` (
  `serial_no` varchar(64) NOT NULL COMMENT 'serialNo，称重流水号',
  `sys_no` varchar(64) DEFAULT NULL COMMENT 'sysNo',
  `set_id` varchar(64) DEFAULT NULL COMMENT 'setId',
  `card_no` varchar(64) DEFAULT NULL COMMENT 'cardNo',
  `plate_no` varchar(32) DEFAULT NULL COMMENT 'plateNo',
  `weight_type` varchar(64) DEFAULT NULL COMMENT 'weightType',
  `transport_unit` varchar(255) DEFAULT NULL COMMENT 'transportUnit',
  `forwarding_unit` varchar(255) DEFAULT NULL COMMENT 'forwardingUnit',
  `consignee_unit` varchar(255) DEFAULT NULL COMMENT 'consigneeUnit',
  `goods_name` varchar(255) DEFAULT NULL COMMENT 'goodsName',
  `goods_spec` varchar(255) DEFAULT NULL COMMENT 'goodsSpec',
  `gross_weight` decimal(18,3) DEFAULT NULL COMMENT 'grossWeight',
  `tare_weight` decimal(18,3) DEFAULT NULL COMMENT 'tareWeight',
  `net_weight` decimal(18,3) DEFAULT NULL COMMENT 'netWeight',
  `buckle_weight` decimal(18,3) DEFAULT NULL COMMENT 'buckleWeight',
  `actual_weight` decimal(18,3) DEFAULT NULL COMMENT 'actualWeight',
  `weight_unit` varchar(16) DEFAULT NULL COMMENT 'weightUnit',
  `unit_price` decimal(18,4) DEFAULT NULL COMMENT 'unitPrice',
  `sum_amt` decimal(18,2) DEFAULT NULL COMMENT 'sumAmt',
  `scale_num` varchar(64) DEFAULT NULL COMMENT 'scaleNum',
  `square_num` varchar(64) DEFAULT NULL COMMENT 'squareNum',
  `weighing_fee` decimal(18,2) DEFAULT NULL COMMENT 'weighingFee',
  `gross_station` varchar(128) DEFAULT NULL COMMENT 'grossStation',
  `tare_station` varchar(128) DEFAULT NULL COMMENT 'tareStation',
  `gross_man` varchar(64) DEFAULT NULL COMMENT 'grossMan',
  `tare_man` varchar(64) DEFAULT NULL COMMENT 'tareMan',
  `gross_time` datetime(3) DEFAULT NULL COMMENT 'grossTime',
  `tare_time` datetime(3) DEFAULT NULL COMMENT 'tareTime',
  `first_time` datetime(3) DEFAULT NULL COMMENT 'firstTime',
  `second_time` datetime(3) DEFAULT NULL COMMENT 'secondTime',
  `update_time` datetime(3) DEFAULT NULL COMMENT 'updateTime',
  `print_num` int DEFAULT NULL COMMENT 'printNum',
  `is_cancle` tinyint DEFAULT NULL COMMENT 'isCancle，保留原拼写',
  `is_upload_local` tinyint DEFAULT NULL COMMENT 'isUploadLocal',
  `is_upload_cloud` tinyint DEFAULT 0 COMMENT 'isUploadCloud',
  `str_backup1` varchar(512) DEFAULT NULL COMMENT 'strBackup1',
  `str_backup2` varchar(512) DEFAULT NULL COMMENT 'strBackup2',
  `str_backup3` varchar(512) DEFAULT NULL COMMENT 'strBackup3',
  `str_backup4` varchar(512) DEFAULT NULL COMMENT 'strBackup4',
  `str_backup5` varchar(512) DEFAULT NULL COMMENT 'strBackup5',
  `str_backup6` varchar(512) DEFAULT NULL COMMENT 'strBackup6',
  `str_backup7` varchar(512) DEFAULT NULL COMMENT 'strBackup7',
  `str_backup8` varchar(512) DEFAULT NULL COMMENT 'strBackup8',
  `str_backup9` varchar(512) DEFAULT NULL COMMENT 'strBackup9',
  `num_backup1` decimal(18,4) DEFAULT NULL COMMENT 'numBackup1',
  `num_backup2` decimal(18,4) DEFAULT NULL COMMENT 'numBackup2',
  `num_backup3` decimal(18,4) DEFAULT NULL COMMENT 'numBackup3',
  `num_backup4` decimal(18,4) DEFAULT NULL COMMENT 'numBackup4',
  `num_backup5` decimal(18,4) DEFAULT NULL COMMENT 'numBackup5',
  `num_backup6` decimal(18,4) DEFAULT NULL COMMENT 'numBackup6',
  `num_backup7` decimal(18,4) DEFAULT NULL COMMENT 'numBackup7',
  `num_backup8` decimal(18,4) DEFAULT NULL COMMENT 'numBackup8',
  `num_backup9` decimal(18,4) DEFAULT NULL COMMENT 'numBackup9',
  `time_backup1` datetime(3) DEFAULT NULL COMMENT 'timeBackup1',
  `time_backup2` datetime(3) DEFAULT NULL COMMENT 'timeBackup2',
  `time_backup3` datetime(3) DEFAULT NULL COMMENT 'timeBackup3',
  `f_guid` char(36) DEFAULT NULL COMMENT 'fGuid',
  `f_id` bigint DEFAULT NULL COMMENT 'fID',
  `rel_no` varchar(64) DEFAULT NULL COMMENT 'relNo',
  `rel_ser` varchar(64) DEFAULT NULL COMMENT 'relSer',
  `reg_no` varchar(64) DEFAULT NULL COMMENT 'regNo',
  `data_type` varchar(64) DEFAULT NULL COMMENT 'dataType',
  `data_log` text COMMENT 'dataLog',
  `is_finish` tinyint DEFAULT NULL COMMENT 'isFinish',
  `remark` varchar(1024) DEFAULT NULL COMMENT 'remark',
  `del_flag` tinyint DEFAULT 0 COMMENT 'del_flag',
  `source_database` varchar(64) NOT NULL DEFAULT 'yunfu' COMMENT '来源数据库',
  `source_table` varchar(64) NOT NULL DEFAULT 'tbl_weightInfo' COMMENT '来源表',
  `sync_status` enum('pending','synced','failed') NOT NULL DEFAULT 'synced' COMMENT '云端同步落库状态',
  `sync_retry_count` int NOT NULL DEFAULT 0 COMMENT '云端处理重试次数',
  `sync_last_error` varchar(1024) DEFAULT NULL COMMENT '云端处理错误',
  `ingested_at` datetime(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) COMMENT '云端接收时间',
  `cloud_updated_at` datetime(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3) COMMENT '云端更新时间',
  PRIMARY KEY (`serial_no`),
  KEY `idx_wds_weight_info_pending` (`is_upload_cloud`, `del_flag`, `update_time`, `serial_no`),
  KEY `idx_wds_weight_info_plate_time` (`plate_no`, `update_time`, `serial_no`),
  KEY `idx_wds_weight_info_goods_time` (`goods_name`, `update_time`, `serial_no`),
  KEY `idx_wds_weight_info_source_time` (`source_database`, `source_table`, `update_time`, `serial_no`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci
  COMMENT='称重信息全量同步表，对应 SQL Server tbl_weightInfo';

CREATE TABLE IF NOT EXISTS `wds_weight_photo` (
  `id` bigint NOT NULL COMMENT '图片主键',
  `serial_no` varchar(64) DEFAULT NULL COMMENT 'serialNo，关联称重流水号',
  `plate_number` varchar(32) DEFAULT NULL COMMENT 'plateNumber',
  `image_type` varchar(64) DEFAULT NULL COMMENT 'imageType',
  `capture_time` datetime(3) DEFAULT NULL COMMENT 'captureTime',
  `capture_image` longblob COMMENT 'captureImage',
  `client_id` varchar(64) DEFAULT NULL COMMENT 'clientId',
  `consignee_unit` varchar(255) DEFAULT NULL COMMENT 'consigneeUnit',
  `forwarding_unit` varchar(255) DEFAULT NULL COMMENT 'forwardingUnit',
  `del_flag` tinyint DEFAULT 0 COMMENT 'delFlag',
  `source_database` varchar(64) NOT NULL DEFAULT 'yunfu',
  `source_table` varchar(64) NOT NULL DEFAULT 'tbl_weightPhoto',
  `ingested_at` datetime(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  `cloud_updated_at` datetime(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
  PRIMARY KEY (`id`),
  KEY `idx_wds_weight_photo_serial_type` (`serial_no`, `image_type`, `capture_time`, `id`),
  KEY `idx_wds_weight_photo_plate_time` (`plate_number`, `capture_time`, `id`),
  CONSTRAINT `fk_wds_weight_photo_serial_no`
    FOREIGN KEY (`serial_no`) REFERENCES `wds_weight_info` (`serial_no`)
    ON UPDATE CASCADE ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci
  COMMENT='称重图片全量同步表，对应 SQL Server tbl_weightPhoto';

CREATE TABLE IF NOT EXISTS `wds_local_weighing_record` (
  `id` char(36) NOT NULL COMMENT '本地缓存 UUID',
  `ticket_no` varchar(64) NOT NULL,
  `scale_no` varchar(64) NOT NULL,
  `plate_no` varchar(32) DEFAULT NULL,
  `weight_kg` decimal(18,3) NOT NULL,
  `original_unit` varchar(16) NOT NULL,
  `measured_at` datetime(3) NOT NULL,
  `status` enum('pending','synced','failed') NOT NULL DEFAULT 'pending',
  `retry_count` int NOT NULL DEFAULT 0,
  `last_error` varchar(1024) DEFAULT NULL,
  `created_at` datetime(3) NOT NULL,
  `updated_at` datetime(3) NOT NULL,
  `ingested_at` datetime(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  PRIMARY KEY (`id`),
  KEY `idx_wds_local_status_time` (`status`, `measured_at`, `id`),
  KEY `idx_wds_local_plate_time` (`plate_no`, `measured_at`, `id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci
  COMMENT='本地 SQLite 缓存结构的云端镜像表';

CREATE TABLE IF NOT EXISTS `wds_sync_batch` (
  `request_id` char(36) NOT NULL,
  `source` varchar(128) NOT NULL,
  `source_database` varchar(64) NOT NULL,
  `source_table` varchar(64) NOT NULL,
  `records_count` int NOT NULL,
  `accepted_count` int NOT NULL DEFAULT 0,
  `failed_count` int NOT NULL DEFAULT 0,
  `uploaded_at` datetime(3) NOT NULL,
  `received_at` datetime(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  `raw_payload` json,
  PRIMARY KEY (`request_id`),
  KEY `idx_wds_sync_batch_source_time` (`source_database`, `source_table`, `received_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci
  COMMENT='每次 /weighing-data-sync/put 请求的批次记录';

-- 高吞吐写入建议：
-- 1. 服务端使用预编译 SQL，一次 INSERT 拼 100 到 1000 行，按 max_allowed_packet 控制批大小。
-- 2. 对大图片 capture_image 单独批量写 wds_weight_photo，避免阻塞称重主表。
-- 3. 批量写入建议包裹事务：START TRANSACTION; batch upsert; batch log; COMMIT;

-- tbl_weightInfo 全字段幂等 upsert 模板。VALUES 部分由服务端按批次展开。
INSERT INTO `wds_weight_info` (
  `serial_no`, `sys_no`, `set_id`, `card_no`, `plate_no`, `weight_type`,
  `transport_unit`, `forwarding_unit`, `consignee_unit`, `goods_name`, `goods_spec`,
  `gross_weight`, `tare_weight`, `net_weight`, `buckle_weight`, `actual_weight`,
  `weight_unit`, `unit_price`, `sum_amt`, `scale_num`, `square_num`, `weighing_fee`,
  `gross_station`, `tare_station`, `gross_man`, `tare_man`, `gross_time`, `tare_time`,
  `first_time`, `second_time`, `update_time`, `print_num`, `is_cancle`,
  `is_upload_local`, `is_upload_cloud`, `str_backup1`, `str_backup2`, `str_backup3`,
  `str_backup4`, `str_backup5`, `str_backup6`, `str_backup7`, `str_backup8`,
  `str_backup9`, `num_backup1`, `num_backup2`, `num_backup3`, `num_backup4`,
  `num_backup5`, `num_backup6`, `num_backup7`, `num_backup8`, `num_backup9`,
  `time_backup1`, `time_backup2`, `time_backup3`, `f_guid`, `f_id`, `rel_no`,
  `rel_ser`, `reg_no`, `data_type`, `data_log`, `is_finish`, `remark`, `del_flag`,
  `source_database`, `source_table`, `sync_status`, `sync_last_error`
) VALUES
  (
    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'synced', NULL
  )
ON DUPLICATE KEY UPDATE
  `sys_no` = VALUES(`sys_no`),
  `set_id` = VALUES(`set_id`),
  `card_no` = VALUES(`card_no`),
  `plate_no` = VALUES(`plate_no`),
  `weight_type` = VALUES(`weight_type`),
  `transport_unit` = VALUES(`transport_unit`),
  `forwarding_unit` = VALUES(`forwarding_unit`),
  `consignee_unit` = VALUES(`consignee_unit`),
  `goods_name` = VALUES(`goods_name`),
  `goods_spec` = VALUES(`goods_spec`),
  `gross_weight` = VALUES(`gross_weight`),
  `tare_weight` = VALUES(`tare_weight`),
  `net_weight` = VALUES(`net_weight`),
  `buckle_weight` = VALUES(`buckle_weight`),
  `actual_weight` = VALUES(`actual_weight`),
  `weight_unit` = VALUES(`weight_unit`),
  `unit_price` = VALUES(`unit_price`),
  `sum_amt` = VALUES(`sum_amt`),
  `scale_num` = VALUES(`scale_num`),
  `square_num` = VALUES(`square_num`),
  `weighing_fee` = VALUES(`weighing_fee`),
  `gross_station` = VALUES(`gross_station`),
  `tare_station` = VALUES(`tare_station`),
  `gross_man` = VALUES(`gross_man`),
  `tare_man` = VALUES(`tare_man`),
  `gross_time` = VALUES(`gross_time`),
  `tare_time` = VALUES(`tare_time`),
  `first_time` = VALUES(`first_time`),
  `second_time` = VALUES(`second_time`),
  `update_time` = VALUES(`update_time`),
  `print_num` = VALUES(`print_num`),
  `is_cancle` = VALUES(`is_cancle`),
  `is_upload_local` = VALUES(`is_upload_local`),
  `is_upload_cloud` = VALUES(`is_upload_cloud`),
  `str_backup1` = VALUES(`str_backup1`),
  `str_backup2` = VALUES(`str_backup2`),
  `str_backup3` = VALUES(`str_backup3`),
  `str_backup4` = VALUES(`str_backup4`),
  `str_backup5` = VALUES(`str_backup5`),
  `str_backup6` = VALUES(`str_backup6`),
  `str_backup7` = VALUES(`str_backup7`),
  `str_backup8` = VALUES(`str_backup8`),
  `str_backup9` = VALUES(`str_backup9`),
  `num_backup1` = VALUES(`num_backup1`),
  `num_backup2` = VALUES(`num_backup2`),
  `num_backup3` = VALUES(`num_backup3`),
  `num_backup4` = VALUES(`num_backup4`),
  `num_backup5` = VALUES(`num_backup5`),
  `num_backup6` = VALUES(`num_backup6`),
  `num_backup7` = VALUES(`num_backup7`),
  `num_backup8` = VALUES(`num_backup8`),
  `num_backup9` = VALUES(`num_backup9`),
  `time_backup1` = VALUES(`time_backup1`),
  `time_backup2` = VALUES(`time_backup2`),
  `time_backup3` = VALUES(`time_backup3`),
  `f_guid` = VALUES(`f_guid`),
  `f_id` = VALUES(`f_id`),
  `rel_no` = VALUES(`rel_no`),
  `rel_ser` = VALUES(`rel_ser`),
  `reg_no` = VALUES(`reg_no`),
  `data_type` = VALUES(`data_type`),
  `data_log` = VALUES(`data_log`),
  `is_finish` = VALUES(`is_finish`),
  `remark` = VALUES(`remark`),
  `del_flag` = VALUES(`del_flag`),
  `source_database` = VALUES(`source_database`),
  `source_table` = VALUES(`source_table`),
  `sync_status` = 'synced',
  `sync_retry_count` = 0,
  `sync_last_error` = NULL,
  `cloud_updated_at` = CURRENT_TIMESTAMP(3);

-- tbl_weightPhoto 全字段幂等 upsert 模板。
INSERT INTO `wds_weight_photo` (
  `id`, `serial_no`, `plate_number`, `image_type`, `capture_time`, `capture_image`,
  `client_id`, `consignee_unit`, `forwarding_unit`, `del_flag`, `source_database`, `source_table`
) VALUES
  (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON DUPLICATE KEY UPDATE
  `serial_no` = VALUES(`serial_no`),
  `plate_number` = VALUES(`plate_number`),
  `image_type` = VALUES(`image_type`),
  `capture_time` = VALUES(`capture_time`),
  `capture_image` = VALUES(`capture_image`),
  `client_id` = VALUES(`client_id`),
  `consignee_unit` = VALUES(`consignee_unit`),
  `forwarding_unit` = VALUES(`forwarding_unit`),
  `del_flag` = VALUES(`del_flag`),
  `source_database` = VALUES(`source_database`),
  `source_table` = VALUES(`source_table`),
  `cloud_updated_at` = CURRENT_TIMESTAMP(3);

-- 本地缓存结构全字段幂等 upsert 模板。
INSERT INTO `wds_local_weighing_record` (
  `id`, `ticket_no`, `scale_no`, `plate_no`, `weight_kg`, `original_unit`,
  `measured_at`, `status`, `retry_count`, `last_error`, `created_at`, `updated_at`
) VALUES
  (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON DUPLICATE KEY UPDATE
  `ticket_no` = VALUES(`ticket_no`),
  `scale_no` = VALUES(`scale_no`),
  `plate_no` = VALUES(`plate_no`),
  `weight_kg` = VALUES(`weight_kg`),
  `original_unit` = VALUES(`original_unit`),
  `measured_at` = VALUES(`measured_at`),
  `status` = VALUES(`status`),
  `retry_count` = VALUES(`retry_count`),
  `last_error` = VALUES(`last_error`),
  `created_at` = VALUES(`created_at`),
  `updated_at` = VALUES(`updated_at`);

-- 待同步扫描 SQL，对应本地/云端补偿任务。
SELECT `serial_no`
FROM `wds_weight_info`
WHERE `is_upload_cloud` = 0
  AND `del_flag` = 0
ORDER BY `update_time` ASC, `serial_no` ASC
LIMIT ?;

-- 批次日志写入模板。
INSERT INTO `wds_sync_batch` (
  `request_id`, `source`, `source_database`, `source_table`,
  `records_count`, `accepted_count`, `failed_count`, `uploaded_at`, `raw_payload`
) VALUES
  (?, ?, ?, ?, ?, ?, ?, ?, ?);
