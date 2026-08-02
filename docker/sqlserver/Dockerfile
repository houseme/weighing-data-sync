FROM mcr.microsoft.com/mssql/server:2022-latest

USER root
COPY docker/sqlserver/entrypoint.sh /opt/wds/e2e/entrypoint.sh
COPY docker/sqlserver/healthcheck.sh /opt/wds/e2e/healthcheck.sh
COPY docker/sqlserver/init.sql /opt/wds/e2e/init.sql
COPY docker/sqlserver/verify-sqlserver.sh /opt/wds/e2e/verify-sqlserver.sh
COPY docker/sqlserver/verify-sqlserver.sql /opt/wds/e2e/verify-sqlserver.sql
RUN chmod +x /opt/wds/e2e/*.sh
USER mssql

ENTRYPOINT ["/opt/wds/e2e/entrypoint.sh"]
