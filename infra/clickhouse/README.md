# ClickHouse init (relocated)

The canonical SQL bootstrap is **[`deploy/clickhouse/01_init.sql`](../../deploy/clickhouse/01_init.sql)**.

`docker-compose.yml` mounts that file. The former `init.sql` in this directory was removed to avoid duplicate schemas.
