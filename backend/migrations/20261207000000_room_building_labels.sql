-- Recover building labels from room numbers imported before building parsing existed.
UPDATE rooms
SET building = substring(name from '^([0-9]+)\.')
WHERE (building IS NULL OR trim(building) = '')
  AND name ~ '^[0-9]+\.';
