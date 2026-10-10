-- Pending batched purges of request payload rows.
--
-- Clearing request logs no longer deletes every payload row inside one huge
-- transaction (that blew up the WAL and held the write lock for a long time)
-- and no longer runs VACUUM. The clear transaction only records, per payload
-- table, the largest rowid that existed when it committed together with the
-- clear timestamp. Rows at or below max_rowid whose created_at is not later
-- than cleared_at belong to the cleared data and are removed in small batches
-- by maintenance, which then deletes the marker.
--
-- The request_log_payload_blobs row is a garbage-collection marker instead.
-- Blobs are content addressed and shared, so they are never purged by rowid.
-- cursor_rowid tracks how far the reference-checked sweep has progressed.
CREATE TABLE IF NOT EXISTS request_log_payload_purges (
  table_name TEXT PRIMARY KEY,
  max_rowid INTEGER NOT NULL,
  cleared_at INTEGER NOT NULL DEFAULT 0,
  cursor_rowid INTEGER NOT NULL DEFAULT 0
);
