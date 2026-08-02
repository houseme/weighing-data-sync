SET NOCOUNT ON;
USE [yunfu];

DECLARE @remaining_pending int = (
    SELECT COUNT(*)
    FROM dbo.tbl_weightInfo
    WHERE ISNULL(isUploadCloud, 0) = 0
      AND ISNULL(del_flag, 0) = 0
);
IF @remaining_pending <> 0
    THROW 51010, 'expected no remaining pending rows after sync', 1;

DECLARE @uploaded_e2e int = (
    SELECT COUNT(*)
    FROM dbo.tbl_weightInfo
    WHERE serialNo LIKE N'WDS-E2E-[0-9][0-9][0-9][0-9]'
      AND isUploadCloud = 1
);
IF @uploaded_e2e <> 100
    THROW 51011, 'expected all 100 E2E rows to be marked uploaded', 1;

DECLARE @seeded_total int = (
    SELECT COUNT(*)
    FROM dbo.tbl_weightInfo
    WHERE serialNo LIKE N'WDS-E2E-[0-9][0-9][0-9][0-9]'
);
IF @seeded_total <> 100
    THROW 51012, 'expected exactly 100 seeded E2E rows', 1;

SELECT 'sqlserver verification passed' AS result;
