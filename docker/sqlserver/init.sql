SET NOCOUNT ON;

IF DB_ID(N'yunfu') IS NULL
BEGIN
    CREATE DATABASE [yunfu];
END;
GO

USE [yunfu];
GO

IF OBJECT_ID(N'dbo.tbl_weightInfo', N'U') IS NOT NULL
BEGIN
    DROP TABLE dbo.tbl_weightInfo;
END;
GO

CREATE TABLE dbo.tbl_weightInfo (
    serialNo nvarchar(64) NOT NULL CONSTRAINT PK_tbl_weightInfo PRIMARY KEY,
    sysNo nvarchar(64) NULL,
    setId nvarchar(64) NULL,
    cardNo nvarchar(64) NULL,
    plateNo nvarchar(32) NULL,
    weightType nvarchar(64) NULL,
    transportUnit nvarchar(255) NULL,
    forwardingUnit nvarchar(255) NULL,
    consigneeUnit nvarchar(255) NULL,
    goodsName nvarchar(255) NULL,
    goodsSpec nvarchar(255) NULL,
    grossWeight decimal(18, 3) NULL,
    tareWeight decimal(18, 3) NULL,
    netWeight decimal(18, 3) NULL,
    buckleWeight decimal(18, 3) NULL,
    actualWeight decimal(18, 3) NULL,
    weightUnit nvarchar(16) NULL,
    unitPrice decimal(18, 4) NULL,
    sumAmt decimal(18, 2) NULL,
    scaleNum nvarchar(64) NULL,
    squareNum nvarchar(64) NULL,
    weighingFee decimal(18, 2) NULL,
    grossStation nvarchar(128) NULL,
    tareStation nvarchar(128) NULL,
    grossMan nvarchar(64) NULL,
    tareMan nvarchar(64) NULL,
    grossTime datetime2(0) NULL,
    tareTime datetime2(0) NULL,
    firstTime datetime2(0) NULL,
    secondTime datetime2(0) NULL,
    updateTime datetime2(0) NULL,
    printNum int NULL,
    isCancle tinyint NULL,
    isUploadLocal tinyint NULL,
    isUploadCloud tinyint NULL,
    strBackup1 nvarchar(512) NULL,
    strBackup2 nvarchar(512) NULL,
    strBackup3 nvarchar(512) NULL,
    strBackup4 nvarchar(512) NULL,
    strBackup5 nvarchar(512) NULL,
    strBackup6 nvarchar(512) NULL,
    strBackup7 nvarchar(512) NULL,
    strBackup8 nvarchar(512) NULL,
    strBackup9 nvarchar(512) NULL,
    numBackup1 decimal(18, 4) NULL,
    numBackup2 decimal(18, 4) NULL,
    numBackup3 decimal(18, 4) NULL,
    numBackup4 decimal(18, 4) NULL,
    numBackup5 decimal(18, 4) NULL,
    numBackup6 decimal(18, 4) NULL,
    numBackup7 decimal(18, 4) NULL,
    numBackup8 decimal(18, 4) NULL,
    numBackup9 decimal(18, 4) NULL,
    timeBackup1 datetime2(0) NULL,
    timeBackup2 datetime2(0) NULL,
    timeBackup3 datetime2(0) NULL,
    fGuid uniqueidentifier NULL,
    fID bigint NULL,
    relNo nvarchar(64) NULL,
    relSer nvarchar(64) NULL,
    regNo nvarchar(64) NULL,
    dataType nvarchar(64) NULL,
    dataLog nvarchar(max) NULL,
    isFinish tinyint NULL,
    remark nvarchar(1024) NULL,
    del_flag tinyint NULL
);
GO

CREATE INDEX IX_tbl_weightInfo_upload_scan
ON dbo.tbl_weightInfo (isUploadCloud, del_flag, updateTime);
GO

DECLARE @i int = 1;
DECLARE @suffix nvarchar(4);
DECLARE @gross decimal(18, 3);
DECLARE @tare decimal(18, 3);
DECLARE @net decimal(18, 3);
DECLARE @unit_price decimal(18, 4);
DECLARE @event_time datetime2(0);

WHILE @i <= 100
BEGIN
    SET @suffix = RIGHT(CONCAT(N'0000', @i), 4);
    SET @gross = CAST(10000.000 + (@i * 100.125) AS decimal(18, 3));
    SET @tare = CAST(3500.000 + (@i * 10.250) AS decimal(18, 3));
    SET @net = @gross - @tare;
    SET @unit_price = CAST(1.5000 + (@i * 0.0100) AS decimal(18, 4));
    SET @event_time = DATEADD(minute, @i, '2026-08-02T08:00:00');

    INSERT INTO dbo.tbl_weightInfo (
        serialNo, sysNo, setId, cardNo, plateNo, weightType,
        transportUnit, forwardingUnit, consigneeUnit, goodsName, goodsSpec,
        grossWeight, tareWeight, netWeight, buckleWeight, actualWeight, weightUnit,
        unitPrice, sumAmt, scaleNum, squareNum, weighingFee,
        grossStation, tareStation, grossMan, tareMan,
        grossTime, tareTime, firstTime, secondTime, updateTime,
        printNum, isCancle, isUploadLocal, isUploadCloud,
        strBackup1, strBackup2, strBackup3, strBackup4, strBackup5, strBackup6, strBackup7, strBackup8, strBackup9,
        numBackup1, numBackup2, numBackup3, numBackup4, numBackup5, numBackup6, numBackup7, numBackup8, numBackup9,
        timeBackup1, timeBackup2, timeBackup3,
        fGuid, fID, relNo, relSer, regNo, dataType, dataLog, isFinish, remark, del_flag
    )
    VALUES (
        CONCAT(N'WDS-E2E-', @suffix),
        CONCAT(N'SYS-', RIGHT(CONCAT(N'00', ((@i - 1) % 5) + 1), 2)),
        CONCAT(N'SET-', ((@i - 1) % 10) + 1),
        CONCAT(N'CARD-', @suffix),
        CONCAT(N'皖H', RIGHT(CONCAT(N'00000', @i), 5)),
        CASE WHEN @i % 3 = 0 THEN N'采购' WHEN @i % 3 = 1 THEN N'销售' ELSE N'调拨' END,
        CONCAT(N'运输单位-', RIGHT(CONCAT(N'00', ((@i - 1) % 8) + 1), 2)),
        CONCAT(N'货代公司-', RIGHT(CONCAT(N'00', ((@i - 1) % 6) + 1), 2)),
        CONCAT(N'收货单位-', RIGHT(CONCAT(N'00', ((@i - 1) % 7) + 1), 2)),
        CASE WHEN @i % 4 = 0 THEN N'玉米' WHEN @i % 4 = 1 THEN N'小麦' WHEN @i % 4 = 2 THEN N'砂石' ELSE N'钢材' END,
        CASE WHEN @i % 2 = 0 THEN N'一级' ELSE N'二级' END,
        @gross,
        @tare,
        @net,
        CAST(@i % 5 AS decimal(18, 3)),
        @net - CAST(@i % 5 AS decimal(18, 3)),
        N'kg',
        @unit_price,
        CAST(@net * @unit_price AS decimal(18, 2)),
        CONCAT(N'SCALE-', ((@i - 1) % 4) + 1),
        CONCAT(N'SQ-', RIGHT(CONCAT(N'00', ((@i - 1) % 12) + 1), 2)),
        CAST(10.00 + (@i % 9) AS decimal(18, 2)),
        CONCAT(N'毛重站-', ((@i - 1) % 4) + 1),
        CONCAT(N'皮重站-', ((@i - 1) % 4) + 1),
        CONCAT(N'gross-operator-', RIGHT(CONCAT(N'00', ((@i - 1) % 6) + 1), 2)),
        CONCAT(N'tare-operator-', RIGHT(CONCAT(N'00', ((@i - 1) % 6) + 1), 2)),
        DATEADD(minute, -10, @event_time),
        @event_time,
        DATEADD(minute, -10, @event_time),
        @event_time,
        DATEADD(second, 30, @event_time),
        @i % 3,
        0,
        0,
        0,
        CONCAT(N'备用字段1-', @suffix),
        CONCAT(N'备用字段2-', @suffix),
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        CAST(@i * 1.1000 AS decimal(18, 4)),
        CAST(@i * 2.2000 AS decimal(18, 4)),
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        DATEADD(minute, 1, @event_time),
        NULL,
        NULL,
        NEWID(),
        100000 + @i,
        CONCAT(N'REL-', @suffix),
        CONCAT(N'RELSER-', @suffix),
        CONCAT(N'REG-', @suffix),
        N'e2e',
        CONCAT(N'e2e seed row ', @i),
        1,
        CONCAT(N'第 ', @i, N' 条端到端待同步记录'),
        0
    );

    SET @i += 1;
END;
GO

SELECT COUNT(*) AS seeded_rows FROM dbo.tbl_weightInfo;
GO
