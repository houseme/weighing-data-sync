-- SQL Server init.sql -> MySQL 8.0 conversion
-- Purpose:
--   Recreate the SQL Server sample source table `yunfu.dbo.tbl_weightInfo`
--   as a MySQL table, including the same 100 deterministic seed rows.
--
-- Main type conversions:
--   nvarchar(n)      -> varchar(n) CHARACTER SET utf8mb4
--   nvarchar(max)    -> longtext
--   datetime2(0)     -> datetime(0)
--   uniqueidentifier -> char(36)
--   tinyint / int    -> tinyint / int
--   decimal(p, s)    -> decimal(p, s)
--   NEWID()          -> UUID()
--   DATEADD(...)     -> DATE_ADD(...)
--   ISNULL(a, b)     -> IFNULL(a, b)
--
-- This file keeps the source column names unchanged, because the sync daemon
-- reads SQL Server `tbl_weightInfo` as camelCase JSON fields. The cloud-side
-- snake_case schema is defined separately in `docs/mysql_sync_schema.sql`.

CREATE DATABASE IF NOT EXISTS `yunfu`
  DEFAULT CHARACTER SET utf8mb4
  DEFAULT COLLATE utf8mb4_0900_ai_ci;

USE `yunfu`;

DROP TABLE IF EXISTS `tbl_weightInfo`;

CREATE TABLE `tbl_weightInfo` (
  `serialNo` varchar(64) NOT NULL,
  `sysNo` varchar(64) DEFAULT NULL,
  `setId` varchar(64) DEFAULT NULL,
  `cardNo` varchar(64) DEFAULT NULL,
  `plateNo` varchar(32) DEFAULT NULL,
  `weightType` varchar(64) DEFAULT NULL,
  `transportUnit` varchar(255) DEFAULT NULL,
  `forwardingUnit` varchar(255) DEFAULT NULL,
  `consigneeUnit` varchar(255) DEFAULT NULL,
  `goodsName` varchar(255) DEFAULT NULL,
  `goodsSpec` varchar(255) DEFAULT NULL,
  `grossWeight` decimal(18,3) DEFAULT NULL,
  `tareWeight` decimal(18,3) DEFAULT NULL,
  `netWeight` decimal(18,3) DEFAULT NULL,
  `buckleWeight` decimal(18,3) DEFAULT NULL,
  `actualWeight` decimal(18,3) DEFAULT NULL,
  `weightUnit` varchar(16) DEFAULT NULL,
  `unitPrice` decimal(18,4) DEFAULT NULL,
  `sumAmt` decimal(18,2) DEFAULT NULL,
  `scaleNum` varchar(64) DEFAULT NULL,
  `squareNum` varchar(64) DEFAULT NULL,
  `weighingFee` decimal(18,2) DEFAULT NULL,
  `grossStation` varchar(128) DEFAULT NULL,
  `tareStation` varchar(128) DEFAULT NULL,
  `grossMan` varchar(64) DEFAULT NULL,
  `tareMan` varchar(64) DEFAULT NULL,
  `grossTime` datetime(0) DEFAULT NULL,
  `tareTime` datetime(0) DEFAULT NULL,
  `firstTime` datetime(0) DEFAULT NULL,
  `secondTime` datetime(0) DEFAULT NULL,
  `updateTime` datetime(0) DEFAULT NULL,
  `printNum` int DEFAULT NULL,
  `isCancle` tinyint DEFAULT NULL,
  `isUploadLocal` tinyint DEFAULT NULL,
  `isUploadCloud` tinyint DEFAULT NULL,
  `strBackup1` varchar(512) DEFAULT NULL,
  `strBackup2` varchar(512) DEFAULT NULL,
  `strBackup3` varchar(512) DEFAULT NULL,
  `strBackup4` varchar(512) DEFAULT NULL,
  `strBackup5` varchar(512) DEFAULT NULL,
  `strBackup6` varchar(512) DEFAULT NULL,
  `strBackup7` varchar(512) DEFAULT NULL,
  `strBackup8` varchar(512) DEFAULT NULL,
  `strBackup9` varchar(512) DEFAULT NULL,
  `numBackup1` decimal(18,4) DEFAULT NULL,
  `numBackup2` decimal(18,4) DEFAULT NULL,
  `numBackup3` decimal(18,4) DEFAULT NULL,
  `numBackup4` decimal(18,4) DEFAULT NULL,
  `numBackup5` decimal(18,4) DEFAULT NULL,
  `numBackup6` decimal(18,4) DEFAULT NULL,
  `numBackup7` decimal(18,4) DEFAULT NULL,
  `numBackup8` decimal(18,4) DEFAULT NULL,
  `numBackup9` decimal(18,4) DEFAULT NULL,
  `timeBackup1` datetime(0) DEFAULT NULL,
  `timeBackup2` datetime(0) DEFAULT NULL,
  `timeBackup3` datetime(0) DEFAULT NULL,
  `fGuid` char(36) DEFAULT NULL,
  `fID` bigint DEFAULT NULL,
  `relNo` varchar(64) DEFAULT NULL,
  `relSer` varchar(64) DEFAULT NULL,
  `regNo` varchar(64) DEFAULT NULL,
  `dataType` varchar(64) DEFAULT NULL,
  `dataLog` longtext,
  `isFinish` tinyint DEFAULT NULL,
  `remark` varchar(1024) DEFAULT NULL,
  `del_flag` tinyint DEFAULT NULL,
  PRIMARY KEY (`serialNo`),
  KEY `idx_tbl_weightInfo_upload_scan` (`isUploadCloud`, `del_flag`, `updateTime`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci
  COMMENT='MySQL equivalent of SQL Server yunfu.dbo.tbl_weightInfo';

DELIMITER $$

DROP PROCEDURE IF EXISTS `seed_tbl_weightInfo`$$

CREATE PROCEDURE `seed_tbl_weightInfo`()
BEGIN
  DECLARE i int DEFAULT 1;
  DECLARE suffix varchar(4);
  DECLARE gross decimal(18,3);
  DECLARE tare decimal(18,3);
  DECLARE net decimal(18,3);
  DECLARE unit_price decimal(18,4);
  DECLARE event_time datetime(0);

  WHILE i <= 100 DO
    SET suffix = LPAD(i, 4, '0');
    SET gross = CAST(10000.000 + (i * 100.125) AS decimal(18,3));
    SET tare = CAST(3500.000 + (i * 10.250) AS decimal(18,3));
    SET net = gross - tare;
    SET unit_price = CAST(1.5000 + (i * 0.0100) AS decimal(18,4));
    SET event_time = DATE_ADD(TIMESTAMP('2026-08-02 08:00:00'), INTERVAL i MINUTE);

    INSERT INTO `tbl_weightInfo` (
      `serialNo`, `sysNo`, `setId`, `cardNo`, `plateNo`, `weightType`,
      `transportUnit`, `forwardingUnit`, `consigneeUnit`, `goodsName`, `goodsSpec`,
      `grossWeight`, `tareWeight`, `netWeight`, `buckleWeight`, `actualWeight`,
      `weightUnit`, `unitPrice`, `sumAmt`, `scaleNum`, `squareNum`, `weighingFee`,
      `grossStation`, `tareStation`, `grossMan`, `tareMan`,
      `grossTime`, `tareTime`, `firstTime`, `secondTime`, `updateTime`,
      `printNum`, `isCancle`, `isUploadLocal`, `isUploadCloud`,
      `strBackup1`, `strBackup2`, `strBackup3`, `strBackup4`, `strBackup5`,
      `strBackup6`, `strBackup7`, `strBackup8`, `strBackup9`,
      `numBackup1`, `numBackup2`, `numBackup3`, `numBackup4`, `numBackup5`,
      `numBackup6`, `numBackup7`, `numBackup8`, `numBackup9`,
      `timeBackup1`, `timeBackup2`, `timeBackup3`,
      `fGuid`, `fID`, `relNo`, `relSer`, `regNo`, `dataType`, `dataLog`,
      `isFinish`, `remark`, `del_flag`
    )
    VALUES (
      CONCAT('WDS-E2E-', suffix),
      CONCAT('SYS-', LPAD(((i - 1) % 5) + 1, 2, '0')),
      CONCAT('SET-', ((i - 1) % 10) + 1),
      CONCAT('CARD-', suffix),
      CONCAT('皖H', LPAD(i, 5, '0')),
      CASE WHEN i % 3 = 0 THEN '采购' WHEN i % 3 = 1 THEN '销售' ELSE '调拨' END,
      CONCAT('运输单位-', LPAD(((i - 1) % 8) + 1, 2, '0')),
      CONCAT('货代公司-', LPAD(((i - 1) % 6) + 1, 2, '0')),
      CONCAT('收货单位-', LPAD(((i - 1) % 7) + 1, 2, '0')),
      CASE WHEN i % 4 = 0 THEN '玉米' WHEN i % 4 = 1 THEN '小麦' WHEN i % 4 = 2 THEN '砂石' ELSE '钢材' END,
      CASE WHEN i % 2 = 0 THEN '一级' ELSE '二级' END,
      gross,
      tare,
      net,
      CAST(i % 5 AS decimal(18,3)),
      net - CAST(i % 5 AS decimal(18,3)),
      'kg',
      unit_price,
      CAST(net * unit_price AS decimal(18,2)),
      CONCAT('SCALE-', ((i - 1) % 4) + 1),
      CONCAT('SQ-', LPAD(((i - 1) % 12) + 1, 2, '0')),
      CAST(10.00 + (i % 9) AS decimal(18,2)),
      CONCAT('毛重站-', ((i - 1) % 4) + 1),
      CONCAT('皮重站-', ((i - 1) % 4) + 1),
      CONCAT('gross-operator-', LPAD(((i - 1) % 6) + 1, 2, '0')),
      CONCAT('tare-operator-', LPAD(((i - 1) % 6) + 1, 2, '0')),
      DATE_ADD(event_time, INTERVAL -10 MINUTE),
      event_time,
      DATE_ADD(event_time, INTERVAL -10 MINUTE),
      event_time,
      DATE_ADD(event_time, INTERVAL 30 SECOND),
      i % 3,
      0,
      0,
      0,
      CONCAT('备用字段1-', suffix),
      CONCAT('备用字段2-', suffix),
      NULL,
      NULL,
      NULL,
      NULL,
      NULL,
      NULL,
      NULL,
      CAST(i * 1.1000 AS decimal(18,4)),
      CAST(i * 2.2000 AS decimal(18,4)),
      NULL,
      NULL,
      NULL,
      NULL,
      NULL,
      NULL,
      NULL,
      DATE_ADD(event_time, INTERVAL 1 MINUTE),
      NULL,
      NULL,
      UUID(),
      100000 + i,
      CONCAT('REL-', suffix),
      CONCAT('RELSER-', suffix),
      CONCAT('REG-', suffix),
      'e2e',
      CONCAT('e2e seed row ', i),
      1,
      CONCAT('第 ', i, ' 条端到端待同步记录'),
      0
    );

    SET i = i + 1;
  END WHILE;
END$$

DELIMITER ;

CALL `seed_tbl_weightInfo`();
DROP PROCEDURE `seed_tbl_weightInfo`;

SELECT COUNT(*) AS `seeded_rows`
FROM `tbl_weightInfo`;

-- MySQL equivalent of the sync daemon pending-scan query:
SELECT `serialNo`
FROM `tbl_weightInfo`
WHERE IFNULL(`isUploadCloud`, 0) = 0
  AND IFNULL(`del_flag`, 0) = 0
ORDER BY IFNULL(`updateTime`, IFNULL(`secondTime`, IFNULL(`firstTime`, `grossTime`))) ASC,
  `serialNo` ASC
LIMIT 100;

-- MySQL equivalent of the successful-upload write-back:
-- UPDATE `tbl_weightInfo`
-- SET `isUploadCloud` = 1
-- WHERE `serialNo` IN ('WDS-E2E-0001', 'WDS-E2E-0002');
