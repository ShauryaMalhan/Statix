-- ClickHouse merge pressure monitoring for statix.workload_metrics (Grafana / alerting).
-- Alert thresholds on active_parts: > 300 = P1 alert, > 1000 = P0 page.

-- Parts count alert (fire when > 300 active parts):
SELECT
    count() AS active_parts,
    sum(rows) AS total_rows,
    formatReadableSize(sum(bytes_on_disk)) AS disk_size
FROM system.parts
WHERE database = 'statix'
  AND table = 'workload_metrics'
  AND active = 1;

-- Merge queue depth:
SELECT
    count() AS active_merges,
    sum(num_parts) AS parts_being_merged,
    formatReadableSize(sum(total_size_bytes_compressed)) AS merge_bytes
FROM system.merges
WHERE database = 'statix'
  AND table = 'workload_metrics';
